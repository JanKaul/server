//! C++ ↔ Rust FFI surface.
//!
//! Currently a **minimum-viable scope** — three lifecycle calls plus the
//! pre-existing `slatedb_version()`. Designed to prove the wiring works
//! (cxx bridge + tokio runtime + global engine state + dict-integrated
//! catalogue) end-to-end before we commit to the larger handler-bucket
//! surface.
//!
//! ## What's exposed today
//!
//! | C++ entry point              | Rust delegate                | Purpose |
//! |------------------------------|------------------------------|---------|
//! | `slatedb::slatedb_version()` | [`slatedb_version`]          | Library identity |
//! | `slatedb::init_in_memory(name)` | [`slatedb_init_in_memory`] | Boot runtime + open in-memory engine + empty DdlManager |
//! | `slatedb::shutdown()`        | [`slatedb_shutdown`]         | Close the engine; runtime stays installed (singleton) |
//! | `slatedb::has_table(name)`   | [`slatedb_has_table`]        | Catalogue probe — useful for smoke tests from C++ |
//!
//! ## Design notes
//!
//! ### Global state
//!
//! [`ENGINE`] is a `parking_lot::RwLock<Option<EngineState>>` — set by
//! `init_in_memory`, cleared by `shutdown`. We don't use `OnceLock`
//! because MariaDB plugin lifecycle wants reentrant init/shutdown (load
//! plugin → uninstall plugin → reload plugin within the same process).
//!
//! The runtime itself ([`crate::runtime`]) is a true singleton via
//! `OnceLock` — once installed it can't be torn down. That's fine: a
//! tokio runtime with empty workqueues is cheap to leave running.
//!
//! ### Async → sync boundary
//!
//! Every bridge function that touches the engine wraps an async block
//! in [`crate::runtime::block_on`]. This blocks the calling MariaDB
//! handler thread on the tokio runtime — exactly the pattern the
//! runtime module documents in its top doc-comment.
//!
//! ### Return codes
//!
//! `i32` status codes, defined as constants in [`status`]. `0` = OK,
//! non-zero = failure. Failure modes are coarse-grained at this stage
//! (init / double-init / io); richer error info will flow when the
//! handler surface lands and we have a stable error → HA_ERR
//! translation channel (we have [`crate::error::slatedb_error_to_ha_err`]
//! already, but it expects a live `slatedb::Error`, not an integer
//! round-trip).
//!
//! ### What's NOT exposed
//!
//! Anything involving the codec, transactions, or row-level operations.
//! Those land alongside the handler buckets. The MVS deliberately keeps
//! the cxx surface to the smallest thing that proves the lifecycle
//! works.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::engine::db::EngineDb;
use crate::engine::ddl_manager::DdlManager;
use crate::engine::txn_registry::TxnRegistry;

#[cxx::bridge(namespace = "slatedb")]
pub mod ffi {
    extern "Rust" {
        /// Library identity string. Always succeeds.
        fn slatedb_version() -> String;

        /// Boot the runtime + open a fresh in-memory engine + install
        /// an empty DdlManager. Returns 0 on success; see
        /// [`super::status`] for error codes.
        fn slatedb_init_in_memory(name: String) -> i32;

        /// Close the engine. Idempotent — returns 0 if no engine is
        /// installed. The tokio runtime stays installed (singleton).
        fn slatedb_shutdown() -> i32;

        /// `true` iff a table with `name` is in the catalogue. Returns
        /// `false` if no engine is installed (uninited / post-shutdown).
        fn slatedb_has_table(name: String) -> bool;

        /// DROP TABLE callback — counterpart of the (future) CREATE
        /// TABLE wiring. Marks each of the table's indexes as
        /// pending-drop in the system CF, deletes the DDL entry, and
        /// removes the in-memory catalogue entry. Idempotent —
        /// returns `super::status::OK` whether or not the table
        /// was present.
        ///
        /// `name` is MariaDB's on-disk path form (`./db/tbl` or
        /// `./db/tbl#P#part`) — normalized internally before lookup.
        ///
        /// Returns: OK on success (including not-present),
        /// NO_ENGINE pre-init, BAD_TABLE_PATH on malformed input,
        /// ENGINE_IO_FAILED on dict-write failure.
        fn slatedb_drop_table(name: String) -> i32;

        // ----- per-handler lifecycle -----
        //
        // Opaque `HaSlateDb` handle owned on the C++ side as
        // `unique_ptr<HaSlateDb>`. C++ calls `new_ha_slatedb()` once
        // per (THD, table) and then drives `ha_open` / `ha_close`.

        type HaSlateDb;

        /// Construct a fresh handler instance. Always succeeds.
        fn new_ha_slatedb() -> Box<HaSlateDb>;

        /// Bind the handler to the table at `name` (MariaDB on-disk
        /// path form, e.g. `./db/tbl` or `./db/tbl#P#part`).
        ///
        /// Returns 0 on success; see [`super::handler::status`] for
        /// failure codes (NO_ENGINE / BAD_TABLE_PATH /
        /// NO_SUCH_TABLE / ENGINE_IO_FAILED).
        fn ha_open(self: &mut HaSlateDb, name: String) -> i32;

        /// Release the handler's per-table state. Idempotent.
        /// Always returns 0.
        fn ha_close(self: &mut HaSlateDb) -> i32;

        /// Decide the row-lock mode + (possibly downgraded) THR_LOCK
        /// type for this statement. Returns the chosen
        /// `thr_lock_type` as `i32` (the cxx side maps back to the
        /// C++ enum). Side effect: updates `lock_rows` +
        /// `db_lock_type` on the handler.
        ///
        /// `in_lock_tables` is true when the THD is inside an
        /// explicit `LOCK TABLES`. `tablespace_op` is true for
        /// `DISCARD/IMPORT TABLESPACE`. Both come from THD reads on
        /// the cxx side (`thd_in_lock_tables` / `thd_tablespace_op`).
        ///
        /// `requested_lock_type` is the raw `enum thr_lock_type`
        /// value; unknown values collapse to `TL_IGNORE` (which is
        /// the C++'s "leave the decision alone" sentinel).
        fn ha_store_lock(
            self: &mut HaSlateDb,
            in_lock_tables: bool,
            tablespace_op: bool,
            requested_lock_type: i32,
        ) -> i32;

        /// Capability/hint toggle. `extra_op` is the raw
        /// `enum ha_extra_function` value from `include/my_base.h`;
        /// unknown values become a silent no-op. Always returns 0.
        fn ha_extra(self: &mut HaSlateDb, extra_op: i32) -> i32;

        /// Statement-boundary hook. `lock_type` is the raw
        /// `F_RDLCK=1 / F_WRLCK=2 / F_UNLCK=8` from `<sys/file.h>`.
        /// `autocommit_boundary` is true when an F_UNLCK should
        /// commit the txn (caller computed from
        /// thd->variables.option_bits & (OPTION_NOT_AUTOCOMMIT |
        /// OPTION_BEGIN) + n_mysql_tables_in_use).
        ///
        /// Returns status::OK on success; failure codes are
        /// status::NO_ENGINE / status::BAD_TABLE_PATH (used here as
        /// "unknown lock_type") / status::ENGINE_IO_FAILED.
        fn ha_external_lock(
            self: &mut HaSlateDb,
            thd_id: u64,
            lock_type: i32,
            autocommit_boundary: bool,
        ) -> i32;

        // ----- table-scan read path (rnd_init / rnd_next / rnd_end) -----
        //
        // Gated by `field_callbacks` because `ha_rnd_next` takes
        // `&TableRef` to decode the value blob into MariaDB row
        // storage. `ha_rnd_init` and `ha_rnd_end` are gated for
        // symmetry — the three live and die together.

        /// Open a full-table scan on the PK keyspace, scoped to
        /// the per-THD transaction's snapshot. Stashes the
        /// resulting iterator on this handler. `thd_id` must
        /// already have a registered txn (external_lock first).
        ///
        /// Returns `OK` on success; `NO_ENGINE` pre-init;
        /// `BAD_TABLE_PATH` if the handler isn't open or has no
        /// PK; `ENGINE_IO_FAILED` if the txn isn't registered or
        /// the underlying scan_prefix fails.
        #[cfg(feature = "field_callbacks")]
        fn ha_rnd_init(self: &mut HaSlateDb, thd_id: u64) -> i32;

        /// Advance the active scan iterator and decode the
        /// returned row into the live MariaDB row buffer
        /// (`table->record[0]`) via the Field/TABLE callbacks.
        ///
        /// Returns `OK` on a successful row decode;
        /// `END_OF_FILE` when the scan is exhausted;
        /// `ENGINE_IO_FAILED` for I/O / codec failures or for
        /// explicit-PK tables (Stage 0 limit — see method docs);
        /// `BAD_TABLE_PATH` if `rnd_init` wasn't called first.
        #[cfg(feature = "field_callbacks")]
        fn ha_rnd_next(self: &mut HaSlateDb, table: &TableRef) -> i32;

        /// Tear down the active scan. Idempotent — calling on a
        /// handler that hasn't run `rnd_init` is OK. Always
        /// returns `OK`.
        #[cfg(feature = "field_callbacks")]
        fn ha_rnd_end(self: &mut HaSlateDb) -> i32;

        // ----- handlerton txn callbacks -----
        //
        // Free functions invoked by MariaDB on transaction boundaries
        // (commit, rollback, connection close, savepoint). The C++
        // handlerton plugin registration wires these into the
        // `handlerton->{commit, rollback, ...}` slots.
        //
        // All take `thd_id` — the same opaque per-THD identifier
        // `ha_external_lock` uses. Status codes are
        // [`super::status`]: OK / NOT_SUPPORTED / ENGINE_IO_FAILED
        // / NO_ENGINE / RUNTIME_INIT_FAILED.

        /// MariaDB `commit` callback. `commit_tx=true` → full
        /// commit; `false` → statement boundary (no-op in Stage 0,
        /// no savepoint support per Q10).
        fn slatedb_handlerton_commit(thd_id: u64, commit_tx: bool) -> i32;

        /// MariaDB `start_consistent_snapshot` callback —
        /// `START TRANSACTION WITH CONSISTENT SNAPSHOT`. Pre-acquires
        /// the per-THD txn so the read view is pinned at statement
        /// start instead of first read. SlateDB's SerializableSnapshot
        /// default captures the snapshot at `begin`, so this is a
        /// thin wrapper around `get_or_create_tx`.
        fn slatedb_handlerton_start_consistent_snapshot(thd_id: u64) -> i32;

        /// MariaDB `rollback` callback. `rollback_tx=true` → full
        /// rollback; `false` → statement rollback (no-op in Stage 0).
        fn slatedb_handlerton_rollback(thd_id: u64, rollback_tx: bool) -> i32;

        /// MariaDB `close_connection` callback. Silently rolls back
        /// any in-flight txn.
        fn slatedb_handlerton_close_connection(thd_id: u64) -> i32;

        /// MariaDB `savepoint` callback. Stage 0 stub per Q10 —
        /// returns [`super::status::NOT_SUPPORTED`].
        fn slatedb_handlerton_savepoint(thd_id: u64) -> i32;

        /// MariaDB `rollback_to_savepoint` callback. Stage 0 stub.
        fn slatedb_handlerton_rollback_to_savepoint(thd_id: u64) -> i32;

        /// MariaDB `rollback_to_savepoint_can_release_mdl` query.
        /// Constant `false`.
        fn slatedb_handlerton_rollback_to_savepoint_can_release_mdl(
            thd_id: u64,
        ) -> bool;

        /// MariaDB `commit_ordered` hook. No-op.
        fn slatedb_handlerton_commit_ordered(thd_id: u64, all: bool);

        /// MariaDB `checkpoint_request` hook. No-op.
        fn slatedb_handlerton_checkpoint_request();

        // ----- Field/TABLE row-I/O entry points -----
        //
        // Gated by the `field_callbacks` Cargo feature so
        // `cargo test --lib` doesn't try to link against the
        // C++-side `slatedb_field_callbacks.h` forwarders.

        /// CREATE TABLE entry — called by the C++ shim's
        /// `ha_slatedb::create` once it has wrapped `TABLE *form`
        /// in a `TableRef`. Allocates index ids, builds skeleton
        /// `KeyDef`s for each declared key (and a synthetic hidden
        /// PK if none was declared), and writes the resulting
        /// `TblDef` to the system-CF catalogue via
        /// `DdlManager::put_and_write`. `name` is MariaDB's
        /// on-disk path form (`./db/tbl[#P#part]`); Rust normalises.
        #[cfg(feature = "field_callbacks")]
        fn slatedb_create_table(name: String, table: &TableRef) -> i32;

        /// INSERT row entry — called by `ha_slatedb::write_row`
        /// after `ha_external_lock(F_WRLCK)` has created the per-THD
        /// transaction. Builds the PK row key + value blob from
        /// `table` (live row in `record[0]`) and puts them via the
        /// per-THD transaction. `name` is the canonical
        /// `db.tbl[#P#part]` catalogue key.
        ///
        /// Stage 0: writes only the PK row, no secondary keys, no
        /// unique-check pre-read, no auto-incr field bump from the
        /// row, no TTL prefix, no debug checksum suffix.
        #[cfg(feature = "field_callbacks")]
        fn slatedb_write_row(thd_id: u64, name: String, table: &TableRef) -> i32;

        /// DELETE row entry — called by `ha_slatedb::delete_row`
        /// for explicit-PK tables. Builds the PK row key from
        /// `table` and issues `delete` on the per-THD transaction.
        ///
        /// Stage 0 limitation: explicit-PK only. Hidden-PK delete
        /// needs the captured rowid from the prior scan/read
        /// (MyRocks's `m_last_rowkey`), which isn't plumbed —
        /// returns `ENGINE_IO_FAILED` at the guard for hidden-PK
        /// tables.
        #[cfg(feature = "field_callbacks")]
        fn slatedb_delete_row(thd_id: u64, name: String, table: &TableRef) -> i32;

        /// UPDATE row entry — called by `ha_slatedb::update_row`.
        /// For explicit-PK tables with the PK column(s) unchanged
        /// this is identical to a write_row at the same PK
        /// (overwrites the value blob in place via the per-THD
        /// txn).
        ///
        /// ## Stage 0 limitations
        ///
        /// - **Explicit-PK only.** Hidden-PK update would need the
        ///   captured rowid (MyRocks's `m_last_rowkey`) to reach
        ///   the existing row's key; `slatedb_write_row` would
        ///   instead allocate a *new* rowid and silently leak the
        ///   old row. Returns `ENGINE_IO_FAILED` at the guard for
        ///   hidden-PK tables.
        /// - **PK columns must not change.** A PK-changing update
        ///   would need delete-of-old + insert-of-new, which needs
        ///   the old PK to come from `record[1]` (record-buffer
        ///   swap on the C++ side, deferred). Caller-side
        ///   contract — the Rust side can't detect a PK change
        ///   without packing both old and new PKs first, which is
        ///   the work that's deferred. SQL-layer users should
        ///   prefer `DELETE … INSERT …` for PK changes in Stage 0.
        #[cfg(feature = "field_callbacks")]
        fn slatedb_update_row(thd_id: u64, name: String, table: &TableRef) -> i32;
    }

    // ----- Field/TABLE C++ callback surface -----
    //
    // Counterpart of `storage/slatedb/shim/slatedb_field_callbacks.h`.
    // Memory ownership stays on the MariaDB side throughout — Rust
    // only ever sees borrowed `&FieldRef` / `&TableRef`, never owns
    // them, never frees them. The underlying `Field*` / `TABLE*` is
    // pinned by the blocked MariaDB thread that issued the Rust call;
    // Rust callers MUST NOT retain a borrowed ref past the cxx
    // callback that returned it.
    //
    // Column bytes are exchanged by fill-buffer (Rust supplies `&mut
    // [u8]` for reads, `&[u8]` for writes) — no allocation crosses
    // the boundary.
    //
    // cxx requires a `&mut` argument to return `&mut T`, so the
    // mutating ops are exposed as TableRef-indexed free functions
    // (`table_field_set_value` / `_null` / `_notnull`) rather than
    // methods on `Pin<&mut FieldRef>` — the underlying `Field` is
    // pinned by the blocked thread regardless of how we borrow.

    #[cfg(feature = "field_callbacks")]
    unsafe extern "C++" {
        include!("slatedb_field_callbacks.h");

        /// Opaque wrapper around MariaDB's `Field*`. Borrowed only.
        type FieldRef;

        /// Opaque wrapper around MariaDB's `TABLE*`. Borrowed only.
        type TableRef;

        // ----- Field accessors (read-only, primitive return) -----

        /// True iff the Field is NULLABLE.
        fn field_real_maybe_null(f: &FieldRef) -> bool;

        /// True iff the Field currently holds SQL NULL.
        fn field_is_real_null(f: &FieldRef) -> bool;

        /// Storage-format length (on-record byte count).
        fn field_pack_length(f: &FieldRef) -> u32;

        /// Logical data length (e.g. VARCHAR runtime length).
        fn field_data_length(f: &FieldRef) -> u32;

        /// Character-unit length (charset-aware).
        fn field_char_length(f: &FieldRef) -> u32;

        /// Declared column length (`field_length` member).
        fn field_field_length(f: &FieldRef) -> u32;

        /// MYSQL_TYPE_* (enum_field_types) code from `real_type()`.
        fn field_real_type(f: &FieldRef) -> u32;

        /// HA_KEYTYPE_* (enum ha_base_keytype) code from `key_type()`.
        fn field_key_type(f: &FieldRef) -> u32;

        /// Position of this Field in its TABLE's `field[]` array.
        fn field_field_index(f: &FieldRef) -> u32;

        /// Collation id (`charset()->number`). The full
        /// `CHARSET_INFO*` is not exposed — id avoids another
        /// opaque lifetime to manage.
        fn field_charset_number(f: &FieldRef) -> u32;

        /// Bit position within the null-byte for this Field.
        fn field_null_bit(f: &FieldRef) -> u32;

        /// Null-byte offset (relative to `record[0]` start). `-1`
        /// if the Field is non-nullable.
        fn field_null_offset(f: &FieldRef) -> i32;

        /// Bitmap of "which keys does this Field participate in"
        /// (MariaDB caps at 64 keys per table).
        fn field_part_of_key(f: &FieldRef) -> u64;

        /// `Field::flags` — bitmap of column flags
        /// (`UNSIGNED_FLAG`, `BLOB_FLAG`, `ZEROFILL_FLAG`, etc.).
        /// Consumed by unpack functions that need to disambiguate
        /// signed vs unsigned (e.g. `unpack_integer`).
        fn field_flags(f: &FieldRef) -> u32;

        // ----- Read column bytes (fill-buffer) -----

        /// Encode the column's value into `dst` in memcmp (sort)
        /// form. Writes `min(max_len, dst.len())` bytes. The C++
        /// side wraps in `dbug_tmp_use_all_columns` to bypass
        /// read-set checks (mirrors MyRocks `pack_with_make_sort_key`).
        fn field_sort_string(f: &FieldRef, dst: &mut [u8], max_len: u32);

        /// Copy the raw on-record bytes into `dst`. Writes
        /// `min(field.pack_length(), dst.len())` bytes.
        fn field_ptr_bytes(f: &FieldRef, dst: &mut [u8]);

        // ----- Write column state (table-indexed) -----

        /// Copy `src` into the `i`-th field's storage. Returns
        /// the number of bytes written
        /// (`min(pack_length, src.len())`).
        fn table_field_set_value(t: &TableRef, i: u32, src: &[u8]) -> u32;

        /// Mark the `i`-th field as SQL NULL. Caller ensures
        /// nullability.
        fn table_field_set_null(t: &TableRef, i: u32);

        /// Mark the `i`-th field as NOT NULL.
        fn table_field_set_notnull(t: &TableRef, i: u32);

        // ----- TABLE access -----

        /// Borrow the Field at `field_index` from a TABLE. The
        /// C++ side returns a reference to a thread-local scratch
        /// `FieldRef` whose `.ptr` is re-pointed on each call;
        /// the single-thread-per-handler-call invariant from
        /// MariaDB makes this safe.
        fn table_field_at(t: &TableRef, field_index: u32) -> &FieldRef;

        /// Borrow the in-progress row buffer (`record[0]`).
        /// Length is `table->s->stored_rec_length`.
        fn table_record_buf(t: &TableRef) -> &[u8];

        // ----- Schema introspection (CREATE TABLE / value blob) -----

        /// Opaque wrapper around MariaDB's `KEY *` (one entry of
        /// `TABLE_SHARE::key_info`). Borrowed only.
        type KeyInfoRef;

        /// Number of declared columns (`TABLE_SHARE::fields`).
        fn table_field_count(t: &TableRef) -> u32;

        /// Number of declared keys (`TABLE_SHARE::keys`).
        fn table_key_count(t: &TableRef) -> u32;

        /// True iff the table has a user-declared PRIMARY KEY.
        fn table_has_primary_key(t: &TableRef) -> bool;

        /// Index of the PRIMARY KEY within `key_info[]`. Only
        /// meaningful when [`table_has_primary_key`] is `true`.
        fn table_primary_key_index(t: &TableRef) -> u32;

        /// Borrow the `i`-th `KEY` from a TABLE's `key_info[]`.
        /// Same thread-local-scratch pattern as `table_field_at`.
        fn table_key_at(t: &TableRef, key_index: u32) -> &KeyInfoRef;

        /// Key name (`KEY::name`). Allocates a fresh `String` —
        /// CREATE TABLE is a one-shot path where per-key
        /// allocation is acceptable.
        fn key_name(k: &KeyInfoRef) -> String;

        /// `KEY::user_defined_key_parts` — count of keyparts the
        /// user wrote in `CREATE INDEX(...)`. Excludes the
        /// extended-keys tail.
        fn key_user_defined_parts(k: &KeyInfoRef) -> u32;

        /// `KEY::ext_key_parts` — total keypart count including
        /// the extended-keys tail (PK columns implicitly appended
        /// to non-unique SKs).
        fn key_ext_parts(k: &KeyInfoRef) -> u32;

        /// Opaque wrapper around MariaDB's `KEY_PART_INFO *` (one
        /// entry of `KEY::key_part[]`). Same lifetime contract
        /// as [`KeyInfoRef`] — borrowed only.
        type KeyPartRef;

        /// Borrow the `i`-th keypart from a `KEY`'s `key_part[]`
        /// array (`i < key_ext_parts(k)`). Same thread-local
        /// scratch pattern as the rest of the schema callbacks.
        fn key_part_at(k: &KeyInfoRef, part_idx: u32) -> &KeyPartRef;

        /// `KEY_PART_INFO::fieldnr` — index of the column this
        /// keypart packs in `TABLE_SHARE::field[]`. **Returned
        /// as 0-based** even though the underlying C++ field is
        /// 1-based; the conversion happens in the shim so Rust
        /// callers don't need to remember the off-by-one.
        fn key_part_field_index(kp: &KeyPartRef) -> u32;

        /// `KEY_PART_INFO::length` — bytes of this keypart's
        /// mem-comparable image. Drives prefix-index sizing
        /// (`KEY(name(20))` → 20).
        fn key_part_length(kp: &KeyPartRef) -> u16;

        /// `TABLE_SHARE::null_bytes` — bytes of null bitmap at
        /// the start of every row buffer. Always
        /// `(nullable_field_count + 7) / 8`.
        fn table_null_bytes(t: &TableRef) -> u32;

        /// `TABLE_SHARE::reclength` — total bytes of a packed
        /// row buffer (`null_bytes + sum(pack_length)`).
        fn table_record_length(t: &TableRef) -> u32;
    }
}

pub use crate::handler::HaSlateDb;

/// `cxx::bridge` constructor — produces a `Box<HaSlateDb>` which cxx
/// translates to `unique_ptr<HaSlateDb>` on the C++ side.
fn new_ha_slatedb() -> Box<HaSlateDb> {
    Box::new(HaSlateDb::new())
}

// Method bodies for the `self: &mut HaSlateDb` cxx-bridge entries.
// They live here (next to the bridge declaration) rather than in
// `handler.rs` so the cxx surface is localised to this module —
// `handler.rs` exposes a Rust-native API; the bridge module owns the
// status-code translation and the method shape cxx wants.
impl HaSlateDb {
    /// Cxx wrapper — delegates to the Rust-native [`HaSlateDb::open`]
    /// and collapses the error to a stable i32 via
    /// [`crate::handler::open_result_to_status`].
    fn ha_open(&mut self, name: String) -> i32 {
        crate::handler::open_result_to_status(self.open(&name))
    }

    /// Cxx wrapper — [`HaSlateDb::close`] is infallible today, so this
    /// always returns OK; preserved as a fallible signature so future
    /// closes that flush per-handler state can surface errors.
    fn ha_close(&mut self) -> i32 {
        crate::handler::open_result_to_status(self.close())
    }

    /// Cxx wrapper — marshals the flat C++ inputs into the typed
    /// `StoreLockThd` + `ThrLockType` and returns the chosen lock
    /// type as `i32`. The handler-side method is infallible so no
    /// status-code mapping is needed.
    fn ha_store_lock(
        &mut self,
        in_lock_tables: bool,
        tablespace_op: bool,
        requested_lock_type: i32,
    ) -> i32 {
        let thd = crate::handler::StoreLockThd {
            in_lock_tables,
            tablespace_op,
        };
        let chosen = self.store_lock(
            thd,
            crate::handler::ThrLockType::from_i32(requested_lock_type),
        );
        chosen as i32
    }

    /// Cxx wrapper — translates the raw `enum ha_extra_function` `i32`
    /// to the Rust enum and delegates to [`HaSlateDb::extra`]. Always
    /// returns OK; `extra` is infallible.
    fn ha_extra(&mut self, extra_op: i32) -> i32 {
        crate::handler::open_result_to_status(
            self.extra(crate::handler::HaExtraFunction::from_i32(extra_op)),
        )
    }

    /// Cxx wrapper — drives [`HaSlateDb::external_lock`] under the
    /// global tokio runtime via `runtime::block_on`. Unknown
    /// `lock_type` ints collapse to `status::BAD_TABLE_PATH`
    /// (re-purposed here as "bad input"); a missing engine returns
    /// `status::NO_ENGINE`; SlateDB I/O failure (incl. SSI conflict)
    /// returns `status::ENGINE_IO_FAILED`.
    fn ha_external_lock(
        &mut self,
        thd_id: u64,
        lock_type: i32,
        autocommit_boundary: bool,
    ) -> i32 {
        let typed = match crate::handler::ExternalLockType::from_i32(lock_type) {
            Some(t) => t,
            None => return crate::handler::status::BAD_TABLE_PATH,
        };
        let runtime = match crate::runtime::get() {
            Some(rt) => rt,
            None => return status::RUNTIME_INIT_FAILED,
        };
        let result = runtime.block_on(self.external_lock(thd_id, typed, autocommit_boundary));
        crate::handler::open_result_to_status(result)
    }

    /// Cxx wrapper — opens the PK-prefix scan on the per-THD txn
    /// via the global [`TxnRegistry`] and stashes the iterator on
    /// this handler. Delegates to the Rust-native
    /// [`HaSlateDb::rnd_init`].
    #[cfg(feature = "field_callbacks")]
    fn ha_rnd_init(&mut self, thd_id: u64) -> i32 {
        let Some(registry) = current_txn_registry() else {
            return crate::handler::status::NO_ENGINE;
        };
        let runtime = match crate::runtime::get() {
            Some(rt) => rt,
            None => return status::RUNTIME_INIT_FAILED,
        };
        let result = runtime.block_on(self.rnd_init(thd_id, &registry));
        crate::handler::open_result_to_status(result)
    }

    /// Cxx wrapper — advances the scan iterator and decodes the
    /// row into `table`'s live row buffer (`record[0]`).
    ///
    /// Two-phase decode:
    /// 1. **PK columns** (explicit-PK only): unpack from
    ///    `kv.key` via [`unpack_record_via_table`] — the PK
    ///    keypart bytes get unpacked into MariaDB's PK Field
    ///    storage. Hidden-PK tables skip this step (the rowid
    ///    isn't a real column).
    /// 2. **Non-PK columns**: decode the value blob via
    ///    [`TableRefRowValueSink`] +
    ///    [`crate::codec::row_value::decode_row_value`].
    ///
    /// The two writes target disjoint fields (the sink's
    /// `is_in_pk` mask skips PK keyparts), so order between
    /// the two phases is irrelevant.
    ///
    /// Returns: `OK` on a successful row decode; `END_OF_FILE`
    /// when the scan is exhausted; `ENGINE_IO_FAILED` on I/O or
    /// codec failure (including a keypart whose
    /// `FieldPacking::unpack_func` slot is unwired — e.g.
    /// VARCHAR / non-binary-collation CHAR);
    /// `BAD_TABLE_PATH` if the handler isn't open or rnd_init
    /// wasn't called.
    #[cfg(feature = "field_callbacks")]
    fn ha_rnd_next(&mut self, table: &ffi::TableRef) -> i32 {
        use crate::codec::key::IndexType;
        use crate::codec::row_value::{
            compute_value_null_bitmap_layout, decode_row_value,
        };

        let tdef = match self.tbl_def() {
            Some(t) => t.clone(),
            None => return crate::handler::status::BAD_TABLE_PATH,
        };
        let Some(pk_kd) = find_pk_keydef(&tdef) else {
            return status::ENGINE_IO_FAILED;
        };

        let runtime = match crate::runtime::get() {
            Some(rt) => rt,
            None => return status::RUNTIME_INIT_FAILED,
        };
        let kv = match runtime.block_on(self.rnd_next()) {
            Ok(Some(kv)) => kv,
            Ok(None) => return status::END_OF_FILE,
            Err(_) => return status::ENGINE_IO_FAILED,
        };

        // ----- PK columns (explicit-PK only) -----
        //
        // Hidden-PK tables have no real PK column to reconstruct;
        // the rowid stays in the key and isn't surfaced to the
        // SQL layer.
        if pk_kd.index_type == IndexType::Primary
            && unpack_record_via_table(&pk_kd, table, false, &kv.key).is_err()
        {
            return status::ENGINE_IO_FAILED;
        }

        // ----- Non-PK columns: value-blob decode -----
        let field_count = ffi::table_field_count(table);
        let pk_field_mask =
            TableRefRowValueSource::pk_field_mask_for(&pk_kd, field_count);
        let layout = {
            let mask = &pk_field_mask;
            compute_value_null_bitmap_layout(
                field_count,
                |i| ffi::field_real_maybe_null(ffi::table_field_at(table, i)),
                |i| mask.get(i as usize).copied().unwrap_or(false),
            )
        };
        let mut sink = TableRefRowValueSink::new(table, pk_field_mask);

        match decode_row_value(&layout, field_count, &mut sink, &kv.value) {
            Ok(_) => status::OK,
            Err(_) => status::ENGINE_IO_FAILED,
        }
    }

    /// Cxx wrapper — drops the active scan iterator. Delegates
    /// to [`HaSlateDb::rnd_end`], which is infallible.
    #[cfg(feature = "field_callbacks")]
    fn ha_rnd_end(&mut self) -> i32 {
        crate::handler::open_result_to_status(self.rnd_end())
    }
}

// ---------------------------------------------------------------------------
// Status codes
// ---------------------------------------------------------------------------

/// FFI return codes. Stable across the cxx boundary.
pub mod status {
    /// Operation succeeded.
    pub const OK: i32 = 0;
    /// Runtime init failed (tokio couldn't construct the io pool).
    pub const RUNTIME_INIT_FAILED: i32 = 1;
    /// `init_in_memory` called while an engine is already installed —
    /// caller must `shutdown` first.
    pub const ALREADY_INITIALISED: i32 = 2;
    /// SlateDB returned an error during the async open / close
    /// sequence. Coarse-grained at this stage; richer info will
    /// surface through the handler-bucket error channel.
    pub const ENGINE_IO_FAILED: i32 = 3;
    /// Caller asked for a feature the SlateDB engine doesn't yet
    /// support (Stage 0 savepoint stubs etc.). The C++ side maps
    /// this to `HA_ERR_WRONG_COMMAND`.
    pub const NOT_SUPPORTED: i32 = 4;
    /// `rnd_next` reached end-of-scan. The C++ side maps this to
    /// `HA_ERR_END_OF_FILE` — MariaDB's signal that the iterator
    /// is exhausted (not actually an error; the SQL layer treats
    /// it as "no more rows").
    pub const END_OF_FILE: i32 = 5;
}

// ---------------------------------------------------------------------------
// Global engine state
// ---------------------------------------------------------------------------

/// Per-process engine state. `None` before init or after shutdown.
///
/// `parking_lot::RwLock` (not `std::sync::RwLock`) for the const-fn
/// constructor — `parking_lot::RwLock::new` is `const` so this works
/// at static-initialiser time without a `OnceLock` indirection.
static ENGINE: RwLock<Option<EngineState>> = RwLock::new(None);

/// What the bridge owns once `init_in_memory` succeeds. Kept private —
/// the cxx surface only deals in flat i32/bool/String.
struct EngineState {
    db: Arc<EngineDb>,
    ddl: Arc<DdlManager>,
    /// Per-THD transaction registry. Empty at init; populated by
    /// `external_lock` (and friends) once that lands.
    txn_registry: Arc<TxnRegistry>,
}

// ---------------------------------------------------------------------------
// Bridge bodies
// ---------------------------------------------------------------------------

pub(crate) fn slatedb_version() -> String {
    format!("slatedb-engine {}", env!("CARGO_PKG_VERSION"))
}

pub(crate) fn slatedb_init_in_memory(name: String) -> i32 {
    // 1. Install the runtime if it isn't already. "Already installed"
    //    is fine — runtime is singleton-by-design.
    if crate::runtime::get().is_none() {
        match crate::runtime::init(4, 64) {
            Ok(()) => {}
            Err(_) => {
                // The only way init can fail today is if some other
                // caller raced us between get() and init(). Re-check;
                // if still None, we have a real failure.
                if crate::runtime::get().is_none() {
                    return status::RUNTIME_INIT_FAILED;
                }
            }
        }
    }

    // 2. Reject re-init while an engine is installed — caller is
    //    expected to shutdown first. Matches MariaDB's expectation
    //    that plugin init runs against a clean slate.
    if ENGINE.read().is_some() {
        return status::ALREADY_INITIALISED;
    }

    // 3. Open the engine + empty catalogue. `block_on` is required
    //    because SlateDB's open is async and the bridge entry is sync.
    let runtime = match crate::runtime::get() {
        Some(rt) => rt,
        None => return status::RUNTIME_INIT_FAILED,
    };
    let opened: Result<EngineState, slatedb::Error> = runtime.block_on(async {
        let db = Arc::new(EngineDb::open_in_memory(&name).await?);
        let ddl = Arc::new(DdlManager::new());
        // init() is cheap on a fresh in-memory engine — it scans the
        // (empty) dict and seeds the sequence past EndDictIndexId.
        ddl.init(db.db()).await?;
        let txn_registry = Arc::new(TxnRegistry::new());
        Ok(EngineState {
            db,
            ddl,
            txn_registry,
        })
    });

    match opened {
        Ok(state) => {
            *ENGINE.write() = Some(state);
            status::OK
        }
        Err(_) => status::ENGINE_IO_FAILED,
    }
}

pub(crate) fn slatedb_shutdown() -> i32 {
    // Drop the catalogue + take the EngineDb out under the lock.
    let db_to_close = {
        let mut guard = ENGINE.write();
        guard.take().map(|state| state.db)
    };
    let Some(db) = db_to_close else {
        // Nothing to do — caller already shut down or never inited.
        return status::OK;
    };
    let Some(runtime) = crate::runtime::get() else {
        // Runtime is gone — shouldn't happen since we never tear it
        // down, but be safe: the close call would panic without it.
        return status::ENGINE_IO_FAILED;
    };
    match runtime.block_on(db.close()) {
        Ok(()) => status::OK,
        Err(_) => status::ENGINE_IO_FAILED,
    }
}

pub(crate) fn slatedb_has_table(name: String) -> bool {
    let guard = ENGINE.read();
    match guard.as_ref() {
        Some(state) => state.ddl.find(&name).is_some(),
        None => false,
    }
}

pub(crate) fn slatedb_drop_table(name: String) -> i32 {
    let Some(state) = ({
        let guard = ENGINE.read();
        guard.as_ref().map(|s| (s.ddl.clone(), s.db.clone()))
    }) else {
        return crate::handler::status::NO_ENGINE;
    };
    let (ddl, engine) = state;
    let normalized = match crate::utils::names::normalize_tablename(&name) {
        Ok(n) => n,
        Err(_) => return crate::handler::status::BAD_TABLE_PATH,
    };
    let Some(runtime) = crate::runtime::get() else {
        return status::RUNTIME_INIT_FAILED;
    };
    match runtime.block_on(ddl.drop_table_with_dict(engine.db(), &normalized)) {
        Ok(_) => status::OK,
        Err(_) => status::ENGINE_IO_FAILED,
    }
}

// ---------------------------------------------------------------------------
// Internal accessors (not exposed via cxx)
// ---------------------------------------------------------------------------

/// Hand the current `DdlManager` to internal Rust callers. Returns
/// `None` if no engine is installed. Each caller gets an `Arc` clone
/// so the read-lock guard doesn't outlive the call.
pub(crate) fn current_ddl() -> Option<Arc<DdlManager>> {
    let guard = ENGINE.read();
    guard.as_ref().map(|state| state.ddl.clone())
}

/// Hand the current `EngineDb` to internal Rust callers. Same shape as
/// [`current_ddl`]. Currently only used by handler test fixtures; will
/// be needed by handler buckets that issue direct dict reads.
#[allow(dead_code)]
pub(crate) fn current_engine() -> Option<Arc<EngineDb>> {
    let guard = ENGINE.read();
    guard.as_ref().map(|state| state.db.clone())
}

/// Hand the per-process [`TxnRegistry`] to internal Rust callers.
/// Consumed by `HaSlateDb::external_lock` and friends once that
/// lands.
#[allow(dead_code)]
pub(crate) fn current_txn_registry() -> Option<Arc<TxnRegistry>> {
    let guard = ENGINE.read();
    guard.as_ref().map(|state| state.txn_registry.clone())
}

// ---------------------------------------------------------------------------
// Handlerton txn callback wrappers
// ---------------------------------------------------------------------------
//
// Free functions matching the cxx bridge declarations. Each resolves
// the global [`TxnRegistry`] and delegates to
// [`crate::engine::handlerton`].

fn handlerton_result_to_status(r: Result<(), slatedb::Error>) -> i32 {
    match r {
        Ok(()) => status::OK,
        Err(e) => match e.kind() {
            slatedb::ErrorKind::Invalid => status::NOT_SUPPORTED,
            _ => status::ENGINE_IO_FAILED,
        },
    }
}

pub(crate) fn slatedb_handlerton_commit(thd_id: u64, commit_tx: bool) -> i32 {
    let Some(registry) = current_txn_registry() else {
        return crate::handler::status::NO_ENGINE;
    };
    let Some(runtime) = crate::runtime::get() else {
        return status::RUNTIME_INIT_FAILED;
    };
    let r = runtime.block_on(crate::engine::handlerton::commit(
        &registry, thd_id, commit_tx,
    ));
    handlerton_result_to_status(r)
}

pub(crate) fn slatedb_handlerton_start_consistent_snapshot(thd_id: u64) -> i32 {
    let Some(registry) = current_txn_registry() else {
        return crate::handler::status::NO_ENGINE;
    };
    let Some(db) = current_engine() else {
        return crate::handler::status::NO_ENGINE;
    };
    let Some(runtime) = crate::runtime::get() else {
        return status::RUNTIME_INIT_FAILED;
    };
    let r = runtime.block_on(
        crate::engine::handlerton::start_tx_and_assign_read_view(
            &registry, thd_id, &db,
        ),
    );
    handlerton_result_to_status(r)
}

pub(crate) fn slatedb_handlerton_rollback(thd_id: u64, rollback_tx: bool) -> i32 {
    let Some(registry) = current_txn_registry() else {
        return crate::handler::status::NO_ENGINE;
    };
    handlerton_result_to_status(crate::engine::handlerton::rollback(
        &registry,
        thd_id,
        rollback_tx,
    ))
}

pub(crate) fn slatedb_handlerton_close_connection(thd_id: u64) -> i32 {
    let Some(registry) = current_txn_registry() else {
        return crate::handler::status::NO_ENGINE;
    };
    handlerton_result_to_status(crate::engine::handlerton::close_connection(
        &registry, thd_id,
    ))
}

pub(crate) fn slatedb_handlerton_savepoint(thd_id: u64) -> i32 {
    handlerton_result_to_status(crate::engine::handlerton::savepoint(thd_id))
}

pub(crate) fn slatedb_handlerton_rollback_to_savepoint(thd_id: u64) -> i32 {
    handlerton_result_to_status(crate::engine::handlerton::rollback_to_savepoint(
        thd_id,
    ))
}

pub(crate) fn slatedb_handlerton_rollback_to_savepoint_can_release_mdl(
    thd_id: u64,
) -> bool {
    crate::engine::handlerton::rollback_to_savepoint_can_release_mdl(thd_id)
}

pub(crate) fn slatedb_handlerton_commit_ordered(thd_id: u64, all: bool) {
    crate::engine::handlerton::commit_ordered(thd_id, all);
}

pub(crate) fn slatedb_handlerton_checkpoint_request() {
    crate::engine::handlerton::checkpoint_request();
}

// ---------------------------------------------------------------------------
// Field/TABLE row-I/O entry-point bodies + codec wrappers
// ---------------------------------------------------------------------------
//
// Cxx-side declarations live in the `mod ffi` block above (gated by
// `field_callbacks`). Bodies and Rust-only helpers follow here, all
// likewise gated.

/// Pack one keypart's value into `dst` in memcmp (sort) form via
/// the C++ `field->sort_string` callback. Counterpart of MyRocks'
/// `Rdb_key_def::pack_with_make_sort_key` at `rdb_datadic.cc:1489`
/// — the universal pack routine for fixed-width key parts (every
/// integer, date, float family in MyRocks installs this as the
/// `pack_func` slot).
///
/// Writes exactly `fpi.max_image_len` bytes into the front of
/// `dst` and returns that byte count so the caller can advance
/// its write cursor.
///
/// ## Errors
///
/// - `Invalid` if `fpi.max_image_len < 0` (corruption — the
///   metadata wasn't initialised by [`FieldPacking::setup`])
/// - `Invalid` if `dst.len() < max_image_len` (the caller didn't
///   reserve enough output space)
///
/// ## Lifetime
///
/// `field` is borrowed from MariaDB and valid only for the
/// duration of this call. See the cxx surface doc for the
/// retention rule.
#[cfg(feature = "field_callbacks")]
pub fn pack_with_sort_string(
    fpi: &crate::codec::field_pack::FieldPacking,
    field: &ffi::FieldRef,
    dst: &mut [u8],
) -> Result<usize, slatedb::Error> {
    if fpi.max_image_len < 0 {
        return Err(slatedb::Error::invalid(format!(
            "pack_with_sort_string: invalid max_image_len {} \
             (FieldPacking::setup must have run first)",
            fpi.max_image_len,
        )));
    }
    let max_len = fpi.max_image_len as usize;
    if dst.len() < max_len {
        return Err(slatedb::Error::invalid(format!(
            "pack_with_sort_string: dst too short — have {} bytes, \
             need {} (max_image_len)",
            dst.len(),
            max_len,
        )));
    }
    ffi::field_sort_string(field, &mut dst[..max_len], max_len as u32);
    Ok(max_len)
}

/// Cxx wrapper around [`crate::codec::key::KeyDef::pack_record`].
/// Supplies a packer closure that, for each keypart, fetches a
/// live `&FieldRef` from `table` via `table_field_at(field_index)`
/// and routes through [`pack_with_sort_string`].
///
/// Today only the `pack_with_sort_string` family is implemented
/// (the universal fixed-width pack — integers, dates, floats,
/// NEWDECIMAL when its image fits in `max_image_len`). Other
/// per-type pack helpers (VARCHAR / BLOB / collation-aware
/// strings) land in follow-up slices and would extend this
/// dispatcher.
///
/// `hidden_pk_id` is plumbed straight through to the
/// orchestrator: `None` for explicit PK or SK on a table with a
/// declared PRIMARY KEY; `Some(rowid)` for SK on a hidden-PK
/// table — the rowid lands at the SK's tail keypart.
#[cfg(feature = "field_callbacks")]
pub fn pack_record_via_table(
    key_def: &crate::codec::key::KeyDef,
    table: &ffi::TableRef,
    hidden_pk_id: Option<i64>,
    dst: &mut [u8],
) -> Result<usize, slatedb::Error> {
    let mut packer = |_kp_idx: usize,
                       fpi: &crate::codec::field_pack::FieldPacking,
                       kp_dst: &mut [u8]|
     -> Result<usize, slatedb::Error> {
        let field = ffi::table_field_at(table, fpi.field_index());
        pack_with_sort_string(fpi, field, kp_dst)
    };
    key_def.pack_record(&mut packer, hidden_pk_id, dst)
}

/// Cxx wrapper around [`crate::codec::key::KeyDef::unpack_record`].
/// Supplies an unpacker closure that, for each keypart, dispatches
/// to [`crate::codec::field_pack::FieldPacking::unpack_func`],
/// writes the unpacked bytes into a scratch buffer, and copies them
/// into the field's storage via
/// [`ffi::table_field_set_value`] / [`ffi::table_field_set_null`]
/// (depending on the per-keypart null marker).
///
/// Symmetric counterpart of [`pack_record_via_table`]: the same
/// scratch-per-keypart shape, but writing into the `TableRef`
/// instead of reading from it.
///
/// `hidden_pk_id_present` is `true` when scanning an SK on a
/// hidden-PK table — the orchestrator skips the trailing 8 rowid
/// bytes (they aren't a real column). For PK scans (the only
/// `ha_rnd_next` caller today) the value is `false`.
///
/// ## Stage 0 limitation
///
/// Keypart unpack only succeeds for fields whose
/// `FieldPacking::unpack_func` slot is populated by
/// `FieldPacking::setup` — i.e. fixed-width integer / float /
/// date / decimal / time-with-fsp / year / NewDate / CHAR(n)
/// with binary collation. VARCHAR + non-binary-collation CHAR
/// columns leave `unpack_func == None`; for those, this wrapper
/// returns `Invalid` rather than silently leaving the field
/// uninitialised. `ha_rnd_next` propagates that to
/// `ENGINE_IO_FAILED`.
#[cfg(feature = "field_callbacks")]
pub fn unpack_record_via_table(
    key_def: &crate::codec::key::KeyDef,
    table: &ffi::TableRef,
    hidden_pk_id_present: bool,
    src: &[u8],
) -> Result<(), slatedb::Error> {
    let mut unpacker =
        |_kp_idx: usize,
         fpi: &crate::codec::field_pack::FieldPacking,
         is_null: bool,
         reader: &mut crate::utils::buff::StringReader|
         -> Result<(), slatedb::Error> {
            let field_idx = fpi.field_index();
            if is_null {
                ffi::table_field_set_null(table, field_idx);
                return Ok(());
            }
            let unpack_fn = fpi.unpack_func.ok_or_else(|| {
                slatedb::Error::invalid(format!(
                    "unpack_record_via_table: no unpack_func for keypart \
                     mapped to field {field_idx} (Stage 0: only fixed-width \
                     integer/float/date/decimal columns are supported)",
                ))
            })?;

            // Allocate a scratch buffer of pack_length bytes — the
            // unpack function writes the field's in-record image
            // here, then we hand it to MariaDB via
            // table_field_set_value.
            let pack_len = ffi::field_pack_length(ffi::table_field_at(
                table, field_idx,
            )) as usize;
            let mut scratch = vec![0u8; pack_len];

            // unpack_func takes &mut FieldPacking and &mut FieldView
            // even though none of the wired routines mutate them.
            // Build local mutable copies so the orchestrator can
            // keep &self / & FieldPacking.
            let mut fpi_local = fpi.clone();
            let mut field_local = crate::codec::value::FieldView {
                name: String::new(),
                mysql_type: crate::codec::value::MysqlType::Null,
                pack_length: pack_len as u32,
                output_offset: 0,
                null_marker: None,
                length: 0,
                charset_id: 0,
                // Crucial — unpack_integer reads UNSIGNED_FLAG from here.
                flags: ffi::field_flags(ffi::table_field_at(table, field_idx)),
                decimals: 0,
            };

            let code = unpack_fn(
                &mut fpi_local,
                &mut field_local,
                &mut scratch,
                reader,
                None,
            );
            crate::codec::field_pack::unpack_status_to_result(code)?;

            // Clear NULL first (the field may have been marked NULL
            // by a previous row's decode) and copy bytes in.
            ffi::table_field_set_notnull(table, field_idx);
            let _ = ffi::table_field_set_value(table, field_idx, &scratch);
            Ok(())
        };
    key_def.unpack_record(&mut unpacker, hidden_pk_id_present, src)
}

/// `RowValueSource` implementation backed by a live `TableRef`.
/// The cxx-side counterpart of
/// [`crate::codec::row_value::RowValueSource`] — supplies the
/// per-field info that
/// [`crate::codec::row_value::encode_row_value`] consumes when
/// the encoder runs against an actual MariaDB row.
///
/// `is_in_pk` is precomputed at construction from the PK
/// `KeyDef`'s `pack_info[].field_index` values. `is_null` /
/// `pack_length` / `write_field_bytes` all dispatch through the
/// cxx callbacks.
#[cfg(feature = "field_callbacks")]
pub struct TableRefRowValueSource<'a> {
    table: &'a ffi::TableRef,
    /// Per-field mask: `pk_field_mask[i] = true` iff field `i` is
    /// a PK keypart and must be skipped in the value blob.
    pk_field_mask: Vec<bool>,
}

#[cfg(feature = "field_callbacks")]
impl<'a> TableRefRowValueSource<'a> {
    /// Build a source from a TableRef + a precomputed PK
    /// exclusion mask. The mask is a `Vec<bool>` of length
    /// `field_count` where `mask[i] = true` iff field `i` is a
    /// PK keypart (and thus skipped in the value blob).
    ///
    /// Build the mask via [`Self::pk_field_mask_for`].
    pub fn new(table: &'a ffi::TableRef, pk_field_mask: Vec<bool>) -> Self {
        Self {
            table,
            pk_field_mask,
        }
    }

    /// Build the per-field PK exclusion mask. `mask[i] = true`
    /// iff field `i` is a keypart of `pk_def`.
    pub fn pk_field_mask_for(
        pk_def: &crate::codec::key::KeyDef,
        field_count: u32,
    ) -> Vec<bool> {
        let mut mask = vec![false; field_count as usize];
        for fpi in &pk_def.pack_info {
            let idx = fpi.field_index() as usize;
            if idx < mask.len() {
                mask[idx] = true;
            }
        }
        mask
    }
}

#[cfg(feature = "field_callbacks")]
impl<'a> crate::codec::row_value::RowValueSource for TableRefRowValueSource<'a> {
    fn is_in_pk(&self, i: u32) -> bool {
        self.pk_field_mask
            .get(i as usize)
            .copied()
            .unwrap_or(false)
    }

    fn is_null(&self, i: u32) -> bool {
        let field = ffi::table_field_at(self.table, i);
        ffi::field_is_real_null(field)
    }

    fn pack_length(&self, i: u32) -> u32 {
        let field = ffi::table_field_at(self.table, i);
        ffi::field_pack_length(field)
    }

    fn write_field_bytes(
        &mut self,
        i: u32,
        dst: &mut [u8],
    ) -> Result<usize, slatedb::Error> {
        let field = ffi::table_field_at(self.table, i);
        ffi::field_ptr_bytes(field, dst);
        Ok(ffi::field_pack_length(field) as usize)
    }
}

/// `RowValueSink` implementation backed by a live `TableRef`.
/// Symmetric counterpart of [`TableRefRowValueSource`] — the
/// read-path decoder writes column bytes back into MariaDB
/// `Field`s through this sink.
#[cfg(feature = "field_callbacks")]
pub struct TableRefRowValueSink<'a> {
    table: &'a ffi::TableRef,
    pk_field_mask: Vec<bool>,
}

#[cfg(feature = "field_callbacks")]
impl<'a> TableRefRowValueSink<'a> {
    /// Build a sink from a TableRef + a precomputed PK exclusion
    /// mask (see [`TableRefRowValueSource::pk_field_mask_for`] —
    /// the helper is shared).
    pub fn new(table: &'a ffi::TableRef, pk_field_mask: Vec<bool>) -> Self {
        Self {
            table,
            pk_field_mask,
        }
    }
}

#[cfg(feature = "field_callbacks")]
impl<'a> crate::codec::row_value::RowValueSink for TableRefRowValueSink<'a> {
    fn is_in_pk(&self, i: u32) -> bool {
        self.pk_field_mask
            .get(i as usize)
            .copied()
            .unwrap_or(false)
    }

    fn pack_length(&self, i: u32) -> u32 {
        let field = ffi::table_field_at(self.table, i);
        ffi::field_pack_length(field)
    }

    fn set_null(&mut self, i: u32) {
        ffi::table_field_set_null(self.table, i);
    }

    fn set_field_bytes(
        &mut self,
        i: u32,
        src: &[u8],
    ) -> Result<(), slatedb::Error> {
        // Mark NOT NULL first — MariaDB's null flag lives in a
        // separate bit from `field->ptr`'s contents.
        ffi::table_field_set_notnull(self.table, i);
        let _n = ffi::table_field_set_value(self.table, i, src);
        Ok(())
    }
}

/// Find the primary-key `KeyDef` in a `TblDef`. Returns the
/// `KeyDef` whose `index_type` is `Primary` or `HiddenPrimary`.
#[cfg(feature = "field_callbacks")]
fn find_pk_keydef(
    tdef: &crate::codec::tbl_def::TblDef,
) -> Option<std::sync::Arc<crate::codec::key::KeyDef>> {
    use crate::codec::key::IndexType;
    tdef.key_descrs()
        .iter()
        .find(|kd| {
            matches!(kd.index_type, IndexType::Primary | IndexType::HiddenPrimary)
        })
        .cloned()
}

/// Cxx `extern "Rust"` entry — called by the C++ shim's
/// `ha_slatedb::write_row`. Builds the PK row key + value blob
/// from the live MariaDB row (accessed via `table`) and issues a
/// `put` on the per-THD transaction.
///
/// ## Flow
///
/// 1. Resolve `TblDef` by name (DdlManager lookup). Fail with
///    `NO_SUCH_TABLE` if not in the catalogue.
/// 2. Pick the PK `KeyDef`.
/// 3. Build PK row key:
///    - Hidden-PK: allocate a fresh rowid via
///      `TblDef::fetch_add_hidden_pk_val(1)`, encode
///      `u32_be(index_number) || u64_be(rowid)`.
///    - Explicit-PK: call [`pack_record_via_table`] to walk the
///      keyparts and emit memcmp bytes via `field_sort_string`.
/// 4. Build value blob: precompute the null-bitmap layout via
///    [`crate::codec::row_value::compute_value_null_bitmap_layout`],
///    construct a [`TableRefRowValueSource`], call
///    [`crate::codec::row_value::encode_row_value`].
/// 5. Look up the per-THD txn in [`crate::engine::txn_registry::TxnRegistry`],
///    call `put(pk_key, value_blob)`.
///
/// Stage 0 limitations: no SK writes, no unique-check pre-read,
/// no auto-incr field bump from the row value, no TTL prefix,
/// no debug checksum.
#[cfg(feature = "field_callbacks")]
fn slatedb_write_row(thd_id: u64, name: String, table: &ffi::TableRef) -> i32 {
    use crate::codec::key::{IndexType, INDEX_NUMBER_SIZE};
    use crate::codec::row_value::{
        compute_value_null_bitmap_layout, encode_row_value,
    };

    let Some(ddl) = current_ddl() else {
        return crate::handler::status::NO_ENGINE;
    };
    let Some(engine) = current_engine() else {
        return crate::handler::status::NO_ENGINE;
    };
    let Some(registry) = current_txn_registry() else {
        return crate::handler::status::NO_ENGINE;
    };
    let Some(tdef) = ddl.find(&name) else {
        return crate::handler::status::NO_SUCH_TABLE;
    };
    let Some(pk_kd) = find_pk_keydef(&tdef) else {
        // A table with no PK keydef at all is a corruption — every
        // table either has an explicit PK or a synthetic hidden PK.
        return status::ENGINE_IO_FAILED;
    };

    // ----- PK row key -----
    let is_hidden_pk = pk_kd.index_type == IndexType::HiddenPrimary;
    let mut pk_buf: Vec<u8> = Vec::new();

    if is_hidden_pk {
        let rowid = tdef.fetch_add_hidden_pk_val(1);
        pk_buf.resize(INDEX_NUMBER_SIZE + crate::globals::SIZEOF_HIDDEN_PK_COLUMN, 0);
        let mut written = 0usize;
        pk_kd.get_infimum_key(&mut pk_buf, &mut written);
        match pk_kd.build_hidden_pk_id_buf(rowid, &mut pk_buf[written..]) {
            Ok(_) => {}
            Err(_) => return status::ENGINE_IO_FAILED,
        }
    } else {
        pk_buf.resize(pk_kd.max_storage_fmt_length() as usize, 0);
        let written = match pack_record_via_table(&pk_kd, table, None, &mut pk_buf) {
            Ok(n) => n,
            Err(_) => return status::ENGINE_IO_FAILED,
        };
        pk_buf.truncate(written);
    }

    // ----- value blob -----
    let field_count = ffi::table_field_count(table);
    let pk_field_mask =
        TableRefRowValueSource::pk_field_mask_for(&pk_kd, field_count);
    let layout = {
        let mask = &pk_field_mask;
        compute_value_null_bitmap_layout(
            field_count,
            |i| ffi::field_real_maybe_null(ffi::table_field_at(table, i)),
            |i| mask.get(i as usize).copied().unwrap_or(false),
        )
    };
    let mut source = TableRefRowValueSource::new(table, pk_field_mask);

    let mut value_buf: Vec<u8> = Vec::new();
    if encode_row_value(&layout, field_count, &mut source, &mut value_buf).is_err()
    {
        return status::ENGINE_IO_FAILED;
    }

    // ----- txn put -----
    let Some(mut txn) = registry.take(thd_id) else {
        return crate::handler::status::NO_ENGINE;
    };
    let put_result = txn.put(&pk_buf, &value_buf);
    registry.reinsert(thd_id, txn);
    // engine borrow is unused — we go through the registry's txn.
    // Keep the lookup so this fails fast when init/shutdown is mid-flight.
    let _ = engine;

    match put_result {
        Ok(_) => status::OK,
        Err(_) => status::ENGINE_IO_FAILED,
    }
}

/// Cxx `extern "Rust"` entry — called by the C++ shim's
/// `ha_slatedb::delete_row`. Symmetric with
/// [`slatedb_write_row`] but writes a delete instead of a put,
/// and only handles explicit-PK tables.
///
/// Stage 0 limitation: hidden-PK delete returns
/// `ENGINE_IO_FAILED` (needs `m_last_rowkey` captured from prior
/// scan/read, not plumbed yet).
#[cfg(feature = "field_callbacks")]
fn slatedb_delete_row(thd_id: u64, name: String, table: &ffi::TableRef) -> i32 {
    use crate::codec::key::IndexType;

    let Some(ddl) = current_ddl() else {
        return crate::handler::status::NO_ENGINE;
    };
    let Some(registry) = current_txn_registry() else {
        return crate::handler::status::NO_ENGINE;
    };
    let Some(tdef) = ddl.find(&name) else {
        return crate::handler::status::NO_SUCH_TABLE;
    };
    let Some(pk_kd) = find_pk_keydef(&tdef) else {
        return status::ENGINE_IO_FAILED;
    };

    // Hidden-PK delete needs m_last_rowkey — deferred.
    if pk_kd.index_type == IndexType::HiddenPrimary {
        return status::ENGINE_IO_FAILED;
    }

    // ----- PK row key -----
    let mut pk_buf: Vec<u8> = vec![0u8; pk_kd.max_storage_fmt_length() as usize];
    let written = match pack_record_via_table(&pk_kd, table, None, &mut pk_buf) {
        Ok(n) => n,
        Err(_) => return status::ENGINE_IO_FAILED,
    };
    pk_buf.truncate(written);

    // ----- txn delete -----
    let Some(mut txn) = registry.take(thd_id) else {
        return crate::handler::status::NO_ENGINE;
    };
    let delete_result = txn.delete(&pk_buf);
    registry.reinsert(thd_id, txn);

    match delete_result {
        Ok(_) => status::OK,
        Err(_) => status::ENGINE_IO_FAILED,
    }
}

/// Cxx `extern "Rust"` entry — called by the C++ shim's
/// `ha_slatedb::update_row`. For explicit-PK tables with the PK
/// columns unchanged, an update is just a write at the same PK —
/// SlateDB's last-write-wins semantics overwrite the value blob
/// in place under the per-THD txn.
///
/// Implementation is intentionally a thin guard wrapping
/// [`slatedb_write_row`]: we look up the PK keydef, refuse
/// hidden-PK tables (would silently leak the old row to a fresh
/// rowid), and delegate.
///
/// ## Stage 0 caller-side contract
///
/// The Rust side does not detect whether the PK columns
/// actually changed — that would require packing both the new
/// and old PKs and comparing them, and the "old PK" path needs
/// `record[1]` access on the C++ side (deferred). A
/// PK-changing UPDATE would leave an orphaned row at the old
/// PK; SQL-layer users should prefer `DELETE … INSERT …` for
/// those cases in Stage 0.
#[cfg(feature = "field_callbacks")]
fn slatedb_update_row(thd_id: u64, name: String, table: &ffi::TableRef) -> i32 {
    use crate::codec::key::IndexType;

    let Some(ddl) = current_ddl() else {
        return crate::handler::status::NO_ENGINE;
    };
    let Some(tdef) = ddl.find(&name) else {
        return crate::handler::status::NO_SUCH_TABLE;
    };
    let Some(pk_kd) = find_pk_keydef(&tdef) else {
        return status::ENGINE_IO_FAILED;
    };

    // Hidden-PK update would call slatedb_write_row, which
    // allocates a fresh rowid and inserts at THAT key — leaving
    // the original row orphaned. Refuse rather than silently
    // corrupt.
    if pk_kd.index_type == IndexType::HiddenPrimary {
        return status::ENGINE_IO_FAILED;
    }

    // Explicit-PK + PK-unchanged: write_row overwrites the same
    // key (PK is derived from row data, identical → same key).
    slatedb_write_row(thd_id, name, table)
}

/// Build a [`crate::codec::tbl_def::TblDef`] from primitive
/// schema inputs the C++ shim extracted from `TABLE *form` at
/// CREATE TABLE time.
///
/// Each `key_names[i]` becomes a skeleton `KeyDef` in slot `i`.
/// The key at `primary_key_index` (if `Some`) gets
/// `IndexType::Primary`; all others get `IndexType::Secondary`.
/// If no primary was declared, a synthetic `IndexType::HiddenPrimary`
/// is appended.
///
/// `index_ids` must have exactly one id per `key_names` entry plus
/// one extra when a hidden PK is synthesised. All keys land on
/// `default_cf_id` — Stage 0 has no per-key CF routing
/// (MyRocks' `COMMENT='cf=...'` syntax is deferred).
///
/// This helper is the testable core of [`slatedb_create_table`] —
/// pure Rust, no cxx dependency, so it compiles and tests both
/// with and without the `field_callbacks` feature.
#[cfg_attr(not(feature = "field_callbacks"), allow(dead_code))]
pub(crate) fn build_tbl_def_from_schema(
    full_name: &str,
    key_names: &[String],
    primary_key_index: Option<usize>,
    index_ids: &[u32],
    default_cf_id: u32,
) -> Result<crate::codec::tbl_def::TblDef, slatedb::Error> {
    use crate::codec::key::{
        IndexType, KeyDef, INDEX_INFO_VERSION_LATEST,
        PRIMARY_FORMAT_VERSION_LATEST, SECONDARY_FORMAT_VERSION_LATEST,
    };
    use crate::codec::tbl_def::TblDef;
    use std::sync::Arc;

    if let Some(pk_idx) = primary_key_index {
        if pk_idx >= key_names.len() {
            return Err(slatedb::Error::invalid(format!(
                "build_tbl_def_from_schema: primary_key_index {} \
                 out of range for {} keys",
                pk_idx,
                key_names.len(),
            )));
        }
    }

    let needs_hidden_pk = primary_key_index.is_none();
    let expected_id_count = key_names.len() + usize::from(needs_hidden_pk);
    if index_ids.len() != expected_id_count {
        return Err(slatedb::Error::invalid(format!(
            "build_tbl_def_from_schema: expected {} index_ids \
             (one per key{}), got {}",
            expected_id_count,
            if needs_hidden_pk { " + 1 for hidden PK" } else { "" },
            index_ids.len(),
        )));
    }

    let mut keys: Vec<Arc<KeyDef>> = Vec::with_capacity(expected_id_count);
    for (i, name) in key_names.iter().enumerate() {
        let is_pk = primary_key_index == Some(i);
        let (index_type, format_version) = if is_pk {
            (IndexType::Primary, PRIMARY_FORMAT_VERSION_LATEST)
        } else {
            (IndexType::Secondary, SECONDARY_FORMAT_VERSION_LATEST)
        };
        keys.push(Arc::new(KeyDef::new_skeleton(
            index_ids[i],
            default_cf_id,
            i as u32,
            INDEX_INFO_VERSION_LATEST as u16,
            index_type,
            format_version,
            false,
            name.clone(),
        )));
    }

    if needs_hidden_pk {
        let hidden_id = index_ids[key_names.len()];
        keys.push(Arc::new(KeyDef::new_skeleton(
            hidden_id,
            default_cf_id,
            key_names.len() as u32,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::HiddenPrimary,
            PRIMARY_FORMAT_VERSION_LATEST,
            false,
            "HIDDEN_PK_NAME",
        )));
    }

    Ok(TblDef::new(full_name)?.with_keys(keys))
}

/// Build a [`crate::codec::value::FieldView`] for field `i` from
/// the cxx Field/TABLE callbacks. The view carries enough metadata
/// for [`crate::codec::field_pack::FieldPacking::setup`] to wire
/// dispatch slots — `name`, `output_offset`, and `decimals` are
/// left at defaults because setup doesn't consult them for any of
/// the currently-wired types (integer / float / date / decimal /
/// binary string).
#[cfg(feature = "field_callbacks")]
fn build_field_view_from_cxx(
    table: &ffi::TableRef,
    i: u32,
) -> crate::codec::value::FieldView {
    use crate::codec::value::{FieldView, MysqlType};

    let f = ffi::table_field_at(table, i);
    let null_marker: Option<(u32, u8)> = if ffi::field_real_maybe_null(f) {
        let off = ffi::field_null_offset(f);
        let bit = ffi::field_null_bit(f) as u8;
        // off == -1 only when not nullable — guarded above.
        Some((off as u32, bit))
    } else {
        None
    };
    FieldView {
        // Name/output_offset/decimals defaulted — see fn doc.
        name: String::new(),
        mysql_type: MysqlType::from_u32(ffi::field_real_type(f))
            .unwrap_or(MysqlType::Null),
        pack_length: ffi::field_pack_length(f),
        output_offset: 0,
        null_marker,
        length: ffi::field_field_length(f),
        charset_id: ffi::field_charset_number(f),
        flags: ffi::field_flags(f),
        decimals: 0,
    }
}

/// Build a [`crate::codec::value::TableShareView`] for `table` by
/// walking each field and each KEY's keyparts via the cxx
/// callbacks.
///
/// `synth_hidden_pk` says whether the engine will append a
/// synthetic hidden PK (true when MariaDB didn't supply a
/// `PRIMARY KEY`). The hidden-PK signal in TableShareView is just
/// `hidden_pk_field.is_some()` — the actual index value is a
/// sentinel (`field_count`, one past the real columns) because
/// MariaDB has no real `Field` for the hidden rowid.
#[cfg(feature = "field_callbacks")]
fn build_table_share_view_from_cxx(
    table: &ffi::TableRef,
    synth_hidden_pk: bool,
) -> crate::codec::value::TableShareView {
    use crate::codec::value::{IndexKeyPartView, IndexSchemaView, TableShareView};

    let field_count = ffi::table_field_count(table);
    let fields = (0..field_count)
        .map(|i| build_field_view_from_cxx(table, i))
        .collect::<Vec<_>>();

    let key_count = ffi::table_key_count(table);
    let mut indexes: Vec<IndexSchemaView> = Vec::with_capacity(key_count as usize);
    for ki in 0..key_count {
        let key_ref = ffi::table_key_at(table, ki);
        let user_parts = ffi::key_user_defined_parts(key_ref);
        let ext_parts = ffi::key_ext_parts(key_ref);
        let key_parts: Vec<IndexKeyPartView> = (0..ext_parts)
            .map(|pi| {
                let kp = ffi::key_part_at(key_ref, pi);
                IndexKeyPartView {
                    field_idx: ffi::key_part_field_index(kp),
                    key_part_length: ffi::key_part_length(kp),
                }
            })
            .collect();
        indexes.push(IndexSchemaView {
            user_defined_key_parts: user_parts,
            ext_key_parts: ext_parts,
            key_parts,
        });
    }

    let primary_key_index: Option<u32> = if ffi::table_has_primary_key(table) {
        Some(ffi::table_primary_key_index(table))
    } else {
        None
    };
    let hidden_pk_field: Option<u32> = if synth_hidden_pk {
        Some(field_count) // sentinel — KeyDef::setup only checks is_some()
    } else {
        None
    };

    TableShareView {
        fields,
        null_bytes: ffi::table_null_bytes(table),
        row_length: ffi::table_record_length(table),
        hidden_pk_field,
        indexes,
        primary_key_index,
    }
}

/// Cxx `extern "Rust"` body for CREATE TABLE — see the
/// declaration in [`ffi`] for the wire contract. The C++ shim's
/// `ha_slatedb::create` wraps `TABLE *form` in a
/// [`ffi::TableRef`] and calls this.
///
/// Two-phase build:
/// 1. **Skeleton keys** — allocate KeyDefs via the existing
///    [`build_tbl_def_from_schema`] (Vec<Arc<KeyDef>>, every key
///    with `maxlength == 0`).
/// 2. **Setup pass** — build a [`TableShareView`] from cxx
///    callbacks and call [`crate::codec::key::KeyDef::setup`] on
///    each KeyDef via `Arc::get_mut` (refcount is 1 immediately
///    after step 1, so `get_mut` always succeeds). Populates
///    `pack_info` with dispatch slots
///    (`pack_func`/`unpack_func`/`skip_func`/`max_image_len`)
///    that the read path needs for explicit-PK row reconstruction.
/// 3. **Persist** — Arc-wrap the populated TblDef and write it
///    to the catalogue via `DdlManager::put_and_write`.
///
/// On setup failure (corrupt schema, unrecognised column type,
/// etc.) returns `ENGINE_IO_FAILED`.
#[cfg(feature = "field_callbacks")]
fn slatedb_create_table(name: String, table: &ffi::TableRef) -> i32 {
    use std::sync::Arc;

    let Some(ddl) = current_ddl() else {
        return crate::handler::status::NO_ENGINE;
    };
    let Some(engine) = current_engine() else {
        return crate::handler::status::NO_ENGINE;
    };
    let normalized = match crate::utils::names::normalize_tablename(&name) {
        Ok(n) => n,
        Err(_) => return crate::handler::status::BAD_TABLE_PATH,
    };

    let key_count = ffi::table_key_count(table);
    let has_pk = ffi::table_has_primary_key(table);
    let pk_index = if has_pk {
        Some(ffi::table_primary_key_index(table) as usize)
    } else {
        None
    };

    let mut key_names: Vec<String> = Vec::with_capacity(key_count as usize);
    for i in 0..key_count {
        let key_ref = ffi::table_key_at(table, i);
        key_names.push(ffi::key_name(key_ref));
    }

    // Allocate one id per declared key + one more for the
    // synthetic hidden PK (when needed). Sequential SeqGenerator
    // allocations.
    let needed_ids = key_count as usize + usize::from(!has_pk);
    let mut index_ids: Vec<u32> = Vec::with_capacity(needed_ids);
    for _ in 0..needed_ids {
        index_ids.push(ddl.get_and_update_next_number());
    }

    let mut tdef = match build_tbl_def_from_schema(
        &normalized,
        &key_names,
        pk_index,
        &index_ids,
        // Stage 0: everyone lives in cf_id=1. Per-key CF routing
        // (MyRocks' COMMENT='cf=...' parser) is deferred.
        1,
    ) {
        Ok(t) => t,
        Err(_) => return status::ENGINE_IO_FAILED,
    };

    // ----- setup pass: populate FieldPacking on each KeyDef -----
    let tbl_view = build_table_share_view_from_cxx(table, !has_pk);
    let total_keys = tdef.key_count() as u32;
    if !run_keydef_setup(&mut tdef, &tbl_view, total_keys) {
        return status::ENGINE_IO_FAILED;
    }

    let Some(runtime) = crate::runtime::get() else {
        return status::RUNTIME_INIT_FAILED;
    };
    match runtime.block_on(ddl.put_and_write(Arc::new(tdef), engine.db())) {
        Ok(_) => status::OK,
        Err(_) => status::ENGINE_IO_FAILED,
    }
}

/// Run [`crate::codec::key::KeyDef::setup`] on every KeyDef in
/// `tdef` via `Arc::get_mut`. Returns `false` on any setup error
/// or if a KeyDef's Arc has more than one strong reference (which
/// shouldn't happen right after `build_tbl_def_from_schema` —
/// every Arc has refcount 1).
///
/// Factored out so the cfg-gated `slatedb_create_table` body
/// stays focused on the cxx/marshalling concerns.
#[cfg(feature = "field_callbacks")]
fn run_keydef_setup(
    tdef: &mut crate::codec::tbl_def::TblDef,
    tbl_view: &crate::codec::value::TableShareView,
    key_count: u32,
) -> bool {
    // Walk the KeyDef Arcs and call setup. Need to grab mutable
    // access via Arc::get_mut, which requires refcount == 1.
    // We use take/replace of the inner Vec rather than exposing a
    // mutable accessor on TblDef.
    let mut keys = tdef.take_keys();
    for kd_arc in keys.iter_mut() {
        let Some(kd_mut) = std::sync::Arc::get_mut(kd_arc) else {
            return false;
        };
        if kd_mut.setup(tbl_view, key_count).is_err() {
            return false;
        }
    }
    tdef.put_keys(keys);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- Why these tests serialise on a Mutex -----
    //
    // The bridge holds process-global state (ENGINE + runtime
    // singleton). Tests in the same crate run on parallel threads by
    // default; if two `init_in_memory`-using tests ran concurrently
    // they'd race the ENGINE slot. We serialise them on a Mutex.
    //
    // Once the cxx surface grows to handler-level operations, the
    // C++ side will own this synchronisation (MariaDB itself
    // serialises plugin install/uninit). For the MVS lifecycle tests
    // we DIY.
    use parking_lot::Mutex;
    static SERIALISE: Mutex<()> = Mutex::new(());

    #[test]
    fn version_returns_crate_version_string() {
        let v = slatedb_version();
        assert!(v.starts_with("slatedb-engine "));
        assert!(v.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn init_open_query_shutdown_cycle() {
        let _g = SERIALISE.lock();
        // Ensure clean slate (a prior test may have left state).
        let _ = slatedb_shutdown();

        assert_eq!(
            slatedb_init_in_memory("bridge_cycle".into()),
            status::OK,
            "init",
        );

        // Catalogue is empty on a fresh in-memory engine.
        assert!(!slatedb_has_table("appdb.users".into()));

        assert_eq!(slatedb_shutdown(), status::OK, "shutdown");

        // After shutdown the engine is gone — has_table reads false.
        assert!(!slatedb_has_table("appdb.users".into()));
    }

    #[test]
    fn double_init_returns_already_initialised() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();

        assert_eq!(slatedb_init_in_memory("dbl_init_1".into()), status::OK);
        assert_eq!(
            slatedb_init_in_memory("dbl_init_2".into()),
            status::ALREADY_INITIALISED,
        );

        // Clean up so subsequent tests start fresh.
        assert_eq!(slatedb_shutdown(), status::OK);
    }

    #[test]
    fn shutdown_is_idempotent_when_uninitialised() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        // Second shutdown on already-empty state still returns OK.
        assert_eq!(slatedb_shutdown(), status::OK);
    }

    #[test]
    fn has_table_returns_false_before_init() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert!(!slatedb_has_table("anything".into()));
    }

    #[test]
    fn drop_table_removes_from_catalogue() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(slatedb_init_in_memory("drop_bridge".into()), status::OK);

        // Install a fixture table via the internal accessor (skipping
        // the cxx create-table surface, which isn't wired yet).
        let ddl = current_ddl().expect("ddl");
        let engine = current_engine().expect("engine");
        crate::runtime::block_on(async {
            use crate::codec::key::{
                IndexType, KeyDef, INDEX_INFO_VERSION_LATEST,
                PRIMARY_FORMAT_VERSION_LATEST,
            };
            let mut kd = KeyDef::new_skeleton(
                100,
                1,
                0,
                INDEX_INFO_VERSION_LATEST as u16,
                IndexType::Primary,
                PRIMARY_FORMAT_VERSION_LATEST,
                false,
                "pk",
            );
            kd.maxlength = 12;
            let tdef = std::sync::Arc::new(
                crate::codec::tbl_def::TblDef::new("appdb.drop_me")
                    .unwrap()
                    .with_keys(vec![std::sync::Arc::new(kd)]),
            );
            ddl.put_and_write(tdef, engine.db())
                .await
                .expect("put_and_write");
        });
        assert!(slatedb_has_table("appdb.drop_me".into()));

        // C++ shim hands us MariaDB's on-disk form `./db/tbl`.
        assert_eq!(
            slatedb_drop_table("./appdb/drop_me".into()),
            status::OK,
        );
        assert!(!slatedb_has_table("appdb.drop_me".into()));

        assert_eq!(slatedb_shutdown(), status::OK);
    }

    #[test]
    fn drop_table_missing_returns_ok() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(slatedb_init_in_memory("drop_missing".into()), status::OK);

        // DROP TABLE IF EXISTS on a never-created table.
        assert_eq!(
            slatedb_drop_table("./appdb/nowhere".into()),
            status::OK,
        );

        assert_eq!(slatedb_shutdown(), status::OK);
    }

    #[test]
    fn drop_table_without_engine_returns_no_engine() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(
            slatedb_drop_table("./db/t".into()),
            crate::handler::status::NO_ENGINE,
        );
    }

    #[test]
    fn drop_table_with_malformed_name_returns_bad_path() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(slatedb_init_in_memory("drop_bad_name".into()), status::OK);

        // Not `./db/tbl` shape → normalize_tablename errors.
        assert_eq!(
            slatedb_drop_table("not_a_path".into()),
            crate::handler::status::BAD_TABLE_PATH,
        );

        assert_eq!(slatedb_shutdown(), status::OK);
    }

    #[test]
    fn status_codes_are_distinct() {
        // Light sanity — these are stable wire constants; a copy-paste
        // typo collapsing two of them would silently break callers.
        let codes = [
            status::OK,
            status::RUNTIME_INIT_FAILED,
            status::ALREADY_INITIALISED,
            status::ENGINE_IO_FAILED,
            status::NOT_SUPPORTED,
        ];
        let mut set: std::collections::HashSet<i32> = std::collections::HashSet::new();
        for c in codes {
            assert!(set.insert(c), "duplicate status code {c}");
        }
    }

    // ----- handlerton txn callbacks -----

    #[test]
    fn handlerton_commit_without_engine_returns_no_engine() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(
            slatedb_handlerton_commit(1, true),
            crate::handler::status::NO_ENGINE,
        );
    }

    #[test]
    fn handlerton_commit_full_drains_registered_txn() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(
            slatedb_init_in_memory("handlerton_commit_bridge".into()),
            status::OK,
        );
        // Register a txn under thd_id=7 via the registry (skipping
        // the cxx external_lock path — we already test that
        // elsewhere).
        let reg = current_txn_registry().expect("registry");
        let db = current_engine().expect("engine");
        crate::runtime::block_on(async {
            reg.get_or_create(7, &db).await.expect("create");
        });
        assert!(reg.has(7));

        assert_eq!(slatedb_handlerton_commit(7, true), status::OK);
        assert!(!reg.has(7));

        assert_eq!(slatedb_shutdown(), status::OK);
    }

    #[test]
    fn handlerton_start_consistent_snapshot_creates_txn() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(
            slatedb_init_in_memory("handlerton_start_snapshot_bridge".into()),
            status::OK,
        );

        let reg = current_txn_registry().expect("registry");
        assert!(!reg.has(50));

        assert_eq!(
            slatedb_handlerton_start_consistent_snapshot(50),
            status::OK,
        );
        assert!(reg.has(50));

        assert_eq!(slatedb_shutdown(), status::OK);
    }

    #[test]
    fn handlerton_start_consistent_snapshot_without_engine_returns_no_engine() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(
            slatedb_handlerton_start_consistent_snapshot(1),
            crate::handler::status::NO_ENGINE,
        );
    }

    #[test]
    fn handlerton_savepoint_returns_not_supported() {
        // No engine state needed — savepoint is unconditionally
        // Stage-0-stubbed.
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(slatedb_handlerton_savepoint(1), status::NOT_SUPPORTED);
        assert_eq!(
            slatedb_handlerton_rollback_to_savepoint(1),
            status::NOT_SUPPORTED,
        );
    }

    #[test]
    fn handlerton_rollback_to_savepoint_can_release_mdl_is_false() {
        assert!(!slatedb_handlerton_rollback_to_savepoint_can_release_mdl(0));
    }

    #[test]
    fn handlerton_noop_hooks_do_not_panic() {
        slatedb_handlerton_commit_ordered(0, true);
        slatedb_handlerton_checkpoint_request();
    }

    // ----- build_tbl_def_from_schema (pure-Rust testable core of slatedb_create_table) -----

    use crate::codec::key::IndexType;

    #[test]
    fn build_tbl_def_with_user_pk_only() {
        let t = build_tbl_def_from_schema(
            "db.t",
            &["PRIMARY".to_string()],
            Some(0),
            &[100],
            1,
        )
        .expect("build");
        assert_eq!(t.key_count(), 1);
        let k0 = t.key(0).unwrap();
        assert_eq!(k0.index_type, IndexType::Primary);
        assert_eq!(k0.get_index_number(), 100);
        assert_eq!(k0.cf_id(), 1);
        assert_eq!(k0.get_name(), "PRIMARY");
    }

    #[test]
    fn build_tbl_def_appends_hidden_pk_when_no_user_pk() {
        let t = build_tbl_def_from_schema("db.t", &[], None, &[200], 1)
            .expect("build");
        assert_eq!(t.key_count(), 1);
        let hpk = t.key(0).unwrap();
        assert_eq!(hpk.index_type, IndexType::HiddenPrimary);
        assert_eq!(hpk.get_index_number(), 200);
        assert_eq!(hpk.get_name(), "HIDDEN_PK_NAME");
    }

    #[test]
    fn build_tbl_def_with_pk_and_secondary() {
        let t = build_tbl_def_from_schema(
            "db.t",
            &["PRIMARY".to_string(), "by_email".to_string()],
            Some(0),
            &[100, 101],
            1,
        )
        .expect("build");
        assert_eq!(t.key_count(), 2);
        assert_eq!(t.key(0).unwrap().index_type, IndexType::Primary);
        assert_eq!(t.key(1).unwrap().index_type, IndexType::Secondary);
        assert_eq!(t.key(1).unwrap().get_name(), "by_email");
    }

    #[test]
    fn build_tbl_def_pk_at_nonzero_slot() {
        // SQL layer may put the PRIMARY at any slot — verify the
        // index_type assignment follows primary_key_index, not slot 0.
        let t = build_tbl_def_from_schema(
            "db.t",
            &["by_email".to_string(), "PRIMARY".to_string()],
            Some(1),
            &[100, 101],
            1,
        )
        .expect("build");
        assert_eq!(t.key(0).unwrap().index_type, IndexType::Secondary);
        assert_eq!(t.key(1).unwrap().index_type, IndexType::Primary);
    }

    #[test]
    fn build_tbl_def_hidden_pk_with_secondaries() {
        // Table with secondary keys but no user-declared PRIMARY —
        // hidden PK lands at the end.
        let t = build_tbl_def_from_schema(
            "db.t",
            &["by_email".to_string(), "by_name".to_string()],
            None,
            &[100, 101, 102],
            1,
        )
        .expect("build");
        assert_eq!(t.key_count(), 3);
        assert_eq!(t.key(0).unwrap().index_type, IndexType::Secondary);
        assert_eq!(t.key(1).unwrap().index_type, IndexType::Secondary);
        let hpk = t.key(2).unwrap();
        assert_eq!(hpk.index_type, IndexType::HiddenPrimary);
        assert_eq!(hpk.get_index_number(), 102);
    }

    #[test]
    fn build_tbl_def_rejects_wrong_index_id_count() {
        // One key, no PK → needs 1 + 1 (hidden) = 2 index_ids, got 1.
        let err = match build_tbl_def_from_schema(
            "db.t",
            &["by_email".to_string()],
            None,
            &[100],
            1,
        ) {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(err.to_string().contains("index_ids"));
    }

    #[test]
    fn build_tbl_def_rejects_pk_index_out_of_range() {
        let err = match build_tbl_def_from_schema(
            "db.t",
            &["PRIMARY".to_string()],
            Some(5),
            &[100],
            1,
        ) {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(err.to_string().contains("primary_key_index"));
    }

    #[test]
    fn build_tbl_def_rejects_malformed_name() {
        let err = match build_tbl_def_from_schema(
            "no_dot_at_all",
            &[],
            None,
            &[100],
            1,
        ) {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
    }
}
