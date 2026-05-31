//! MariaDB `handler` analog — per-table-open runtime state.
//!
//! Translated from the `ha_rocksdb` C++ class. MariaDB creates one
//! handler instance per (THD, opened table); methods on it implement
//! the storage-engine vtable that the SQL layer calls.
//!
//! ## Scope of this commit
//!
//! Only **`HaSlateDb::open`** and **`HaSlateDb::close`** from the
//! lifecycle bucket — the smallest meaningful slice. Both are
//! catalogue-only operations: bind / release a reference to the
//! `Arc<TblDef>` looked up from the global engine's
//! [`crate::engine::ddl_manager::DdlManager`]. No transactions, no
//! key buffers, no row I/O yet.
//!
//! ## What's NOT here (deferred)
//!
//! - `init_with_fields` — needs cached_table_flags + check_keyread_allowed
//! - `store_lock` — pure MariaDB THR_LOCK wiring; needs ThdRef plumbing
//! - `external_lock` — txn lifecycle (get_or_create_tx, commit-on-unlock);
//!   needs the per-THD txn registry surface, which lands with the
//!   transaction handlerton callbacks
//! - `extra(HaExtraFunction)` — capability toggles; lands when the read
//!   path needs the `HA_EXTRA_KEYREAD` / `HA_EXTRA_FLUSH` toggles
//! - Per-handler key buffers (`m_pk_packed_tuple`, `m_sk_packed_tuple`,
//!   `m_pk_unpack_info`); lands with `write_row` / `index_read`
//! - `update_row_stats` — lands with `GlobalStats` infrastructure
//!
//! ## Cxx surface
//!
//! Exposed via `[`crate::bridge`]:
//! - `new_ha_slatedb()` returns `Box<HaSlateDb>` (opaque on the C++ side)
//! - `ha_open(handler, name) -> i32` / `ha_close(handler) -> i32`
//!   with status codes from [`crate::bridge::status`] extended in
//!   [`status`].

use std::sync::Arc;

use slatedb::Error;

use crate::codec::tbl_def::TblDef;

/// Status codes specific to handler operations. Reuses the
/// [`crate::bridge::status`] codes for engine-not-installed; adds
/// table-lookup-specific codes here.
pub mod status {
    pub use crate::bridge::status::{
        ALREADY_INITIALISED, ENGINE_IO_FAILED, OK, RUNTIME_INIT_FAILED,
    };

    /// No engine is installed — caller must call `slatedb_init_in_memory`
    /// first.
    pub const NO_ENGINE: i32 = 10;
    /// `open` couldn't normalize the input path (not `./db/tbl` shape).
    pub const BAD_TABLE_PATH: i32 = 11;
    /// The catalogue has no entry for the requested table.
    pub const NO_SUCH_TABLE: i32 = 12;
}

/// MariaDB's `enum thr_lock_type` from `include/thr_lock.h`. Numeric
/// values are stable across versions — they're part of MariaDB's
/// internal ABI.
///
/// We translate this enum so [`HaSlateDb::store_lock`] can pattern-
/// match on the values the C++ side passes us. The cxx surface
/// marshals them as `i32` and the bridge maps to/from this enum.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrLockType {
    Ignore = -1,
    Unlock = 0,
    ReadDefault = 1,
    Read = 2,
    ReadHighPriority = 3,
    ReadNoInsert = 4,
    ReadWithSharedLocks = 5,
    WriteAllowWrite = 6,
    WriteConcurrentInsert = 7,
    WriteDelayed = 8,
    WriteDefault = 9,
    WriteLowPriority = 10,
    Write = 11,
    WriteOnly = 12,
}

impl ThrLockType {
    /// Map from the raw i32 the cxx bridge passes us. Unknown values
    /// (out of range for the enum) collapse to [`ThrLockType::Ignore`]
    /// — matches the C++'s `if (lock_type != TL_IGNORE)` gate, which
    /// effectively treats anything it doesn't recognise as "leave the
    /// decision alone".
    pub fn from_i32(v: i32) -> Self {
        match v {
            -1 => Self::Ignore,
            0 => Self::Unlock,
            1 => Self::ReadDefault,
            2 => Self::Read,
            3 => Self::ReadHighPriority,
            4 => Self::ReadNoInsert,
            5 => Self::ReadWithSharedLocks,
            6 => Self::WriteAllowWrite,
            7 => Self::WriteConcurrentInsert,
            8 => Self::WriteDelayed,
            9 => Self::WriteDefault,
            10 => Self::WriteLowPriority,
            11 => Self::Write,
            12 => Self::WriteOnly,
            _ => Self::Ignore,
        }
    }
}

/// MyRocks' internal row-lock mode parsed out of `store_lock` and
/// used by the rest of the handler. Mirrors C++ `enum {
/// RDB_LOCK_NONE, RDB_LOCK_READ, RDB_LOCK_WRITE }`. Per
/// `_DESIGN.md §5` this drives the SI vs SSI isolation choice and
/// whether scans tag the txn via `txn.mark_read(...)`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowLockMode {
    #[default]
    None = 0,
    Read = 1,
    Write = 2,
}

/// Subset of MariaDB's `enum ha_extra_function` from
/// `include/my_base.h:133`. Only the variants [`HaSlateDb::extra`]
/// acts on are listed here — the full enum has 50+ entries, most of
/// which collapse to a silent no-op (matching the C++'s
/// `default: break;` at `ha_rocksdb.cc:11968`).
///
/// Numeric values match MariaDB's `enum ha_extra_function` exactly
/// (part of the internal handler ABI). The bridge marshals the C++
/// value as `i32` and we map via [`HaExtraFunction::from_i32`].
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaExtraFunction {
    /// `HA_EXTRA_KEYREAD = 7` — next read only needs the keypart
    /// bytes, no row data. Drives the covered-read fast path.
    Keyread = 7,
    /// `HA_EXTRA_NO_KEYREAD = 8` — caller is done with the keyread
    /// fast path; reset.
    NoKeyread = 8,
    /// `HA_EXTRA_FLUSH = 22` — invalidate any per-handler cached row
    /// buffer. Does NOT trigger a SlateDB-side flush.
    Flush = 22,
    /// `HA_EXTRA_NO_IGNORE_DUP_KEY = 26` — end of the `INSERT … ON
    /// DUPLICATE KEY UPDATE` / `REPLACE` window.
    NoIgnoreDupKey = 26,
    /// `HA_EXTRA_INSERT_WITH_UPDATE = 41` — start of `INSERT … ON
    /// DUPLICATE KEY UPDATE`. Tells the handler to cache row lookups
    /// for the upcoming update.
    InsertWithUpdate = 41,
    /// Anything else (e.g. `HA_EXTRA_PREPARE_FOR_RENAME`,
    /// `HA_EXTRA_CACHE`, ...) — silent no-op in our handler. The
    /// `-1` sentinel doesn't collide with any real `ha_extra_function`
    /// value (the real enum starts at 0).
    Other = -1,
}

impl HaExtraFunction {
    /// Map from the raw C++ `enum ha_extra_function` `i32` to a Rust
    /// variant. Unknown values (which is most of them) collapse to
    /// [`HaExtraFunction::Other`], matching the C++ `default: break;`
    /// behaviour.
    pub fn from_i32(v: i32) -> Self {
        match v {
            7 => Self::Keyread,
            8 => Self::NoKeyread,
            22 => Self::Flush,
            26 => Self::NoIgnoreDupKey,
            41 => Self::InsertWithUpdate,
            _ => Self::Other,
        }
    }
}

/// Subset of MariaDB `THD` state that [`HaSlateDb::store_lock`] needs.
/// Filled by the cxx side from a live `THD*` before calling in.
///
/// Currently carries only the two booleans that the simple decision
/// path consults. The `lock_scanned_rows` upgrade branch in the C++
/// (`ha_rocksdb.cc:11299..11323`) needs additional THD reads
/// (`thd_sql_command`, `thd_tx_isolation`, `thd_test_options`, sysvar
/// `lock_scanned_rows`); that path is deferred until we plumb those
/// sysvars across the cxx boundary.
#[derive(Debug, Clone, Copy, Default)]
pub struct StoreLockThd {
    /// True when MariaDB is inside an explicit `LOCK TABLES`.
    pub in_lock_tables: bool,
    /// True for tablespace DDL (DISCARD/IMPORT TABLESPACE).
    pub tablespace_op: bool,
}

/// Per-table-open handler state.
///
/// The C++ `ha_rocksdb` class accretes ~50 fields over its lifetime
/// (key buffers, transaction handle, lock mode, key-read mode, dup-PK
/// flag, row checksum buffer, etc.). We add fields only as the
/// corresponding handler-bucket methods land — keeps the type honest.
///
/// **Today** the only field is `tbl_def`: an `Option<Arc<TblDef>>` that
/// `open` binds and `close` clears.
pub struct HaSlateDb {
    /// `Some` once `open` succeeds, `None` after `close` (or before
    /// the first `open`). The Arc clones cheaply from
    /// [`crate::engine::ddl_manager::DdlManager`]'s in-memory
    /// catalogue.
    tbl_def: Option<Arc<TblDef>>,

    /// Per-statement row-lock decision, set by [`Self::store_lock`].
    /// Drives the SI vs SSI isolation choice elsewhere in the handler
    /// (per `_DESIGN.md §5`). Defaults to `None` between statements.
    lock_rows: RowLockMode,

    /// Current MariaDB-level lock type for this handler instance
    /// (mirrors the C++ `m_db_lock.type`). Tracked so `store_lock`'s
    /// THR_LOCK downgrade only triggers on the `Unlock → request`
    /// transition — matches the C++ `m_db_lock.type == TL_UNLOCK`
    /// gate at `ha_rocksdb.cc:11327`.
    db_lock_type: ThrLockType,

    /// Set by `HA_EXTRA_KEYREAD`, cleared by `HA_EXTRA_NO_KEYREAD`.
    /// When true, the next read should only fetch keypart bytes and
    /// skip the row payload. Drives the covered-read fast path.
    keyread_only: bool,

    /// Set by `HA_EXTRA_INSERT_WITH_UPDATE`, cleared by
    /// `HA_EXTRA_NO_IGNORE_DUP_KEY`. When true, the handler caches
    /// row lookups during `INSERT … ON DUPLICATE KEY UPDATE` to
    /// avoid the second read on the update step.
    insert_with_update: bool,

    /// Cached pull-bytes from the most recent point lookup. Cleared
    /// by `HA_EXTRA_FLUSH` because if the table has BLOB columns the
    /// caller may have started reading them out of this buffer and
    /// MariaDB needs to invalidate that handle. C++ counterpart is
    /// `m_retrieved_record` at `ha_rocksdb.cc:11954`.
    retrieved_record: bytes::BytesMut,
}

impl Default for HaSlateDb {
    fn default() -> Self {
        Self::new()
    }
}

impl HaSlateDb {
    /// Construct a fresh handler. No I/O. The C++ constructor
    /// initialises ~30 default-valued fields; we'll add them as the
    /// corresponding methods land.
    pub fn new() -> Self {
        Self {
            tbl_def: None,
            lock_rows: RowLockMode::None,
            db_lock_type: ThrLockType::Unlock,
            keyread_only: false,
            insert_with_update: false,
            retrieved_record: bytes::BytesMut::new(),
        }
    }

    /// Bind the handler to the table at `name`. The C++ signature is
    /// `int open(const char *name, int mode, uint test_if_locked)`; we
    /// drop `mode` and `test_if_locked` because no current code path
    /// inspects them.
    ///
    /// `name` is the MariaDB on-disk path form (`./db/tbl` or
    /// `./db/tbl#P#part`). We normalise via
    /// [`crate::utils::names::normalize_tablename`] to the
    /// `db.tbl[#P#part]` catalogue key.
    ///
    /// Errors:
    /// - `Invalid` if `name` isn't `./db/tbl[#P#part]` shape (matches
    ///   MyRocks' `HA_ERR_ROCKSDB_INVALID_TABLE` at the same site).
    /// - `Data` if the catalogue has no entry for the normalised name
    ///   (corruption: caller has a `.frm` we don't know about). Matches
    ///   the C++ `HA_ERR_INTERNAL_ERROR` at `ha_rocksdb.cc:6711` plus
    ///   error log.
    /// - `Invalid` if no engine is installed (`slatedb_init_in_memory`
    ///   wasn't called yet) — a programmer / lifecycle bug rather than
    ///   a runtime condition.
    ///
    /// Translated from `ha_rocksdb::open` (`ha_rocksdb.cc:6711`). The
    /// big-bear C++ version also allocates key buffers, computes
    /// `m_pk_descr` / `m_sk_descr` pointers, sets up `m_pk_packed_tuple`
    /// scratch, binds the per-table `table_handler` (refcounted I/O
    /// stats), and pre-fetches the auto-increment value. All deferred.
    pub fn open(&mut self, name: &str) -> Result<(), Error> {
        let normalized = crate::utils::names::normalize_tablename(name)?;
        let ddl = crate::bridge::current_ddl().ok_or_else(|| {
            Error::invalid(
                "HaSlateDb::open: no engine installed — call slatedb_init_* first".into(),
            )
        })?;
        let tbl_def = ddl.find(&normalized).ok_or_else(|| {
            Error::data(format!(
                "HaSlateDb::open: no catalogue entry for table {normalized:?}"
            ))
        })?;
        self.tbl_def = Some(tbl_def);
        Ok(())
    }

    /// Release the handler's per-table state. Idempotent — calling on
    /// an already-closed handler is OK (matches `ha_rocksdb::close`
    /// at `ha_rocksdb.cc:6875`, which is also free of failure paths).
    ///
    /// Does NOT touch the global engine state. Returns the
    /// previously-bound `TblDef` `Arc` count to zero only when no other
    /// references remain.
    pub fn close(&mut self) -> Result<(), Error> {
        self.tbl_def = None;
        Ok(())
    }

    /// Whether `open` has been called and `close` hasn't yet. Useful
    /// for tests and for the cxx bridge to gate ops that require an
    /// open handler.
    pub fn is_open(&self) -> bool {
        self.tbl_def.is_some()
    }

    /// Snapshot the bound `TblDef`. Returns `None` if the handler
    /// isn't currently open.
    pub fn tbl_def(&self) -> Option<&Arc<TblDef>> {
        self.tbl_def.as_ref()
    }

    /// Decide our internal row-lock mode + the MariaDB THR_LOCK type
    /// to install on the handler. Pure decision function — no SlateDB
    /// I/O. Translated from `ha_rocksdb::store_lock` at
    /// `ha_rocksdb.cc:11283`.
    ///
    /// Two decisions, mirroring the C++ structure:
    ///
    /// 1. **`lock_rows`** (`m_lock_rows` in C++): the internal
    ///    READ/WRITE/NONE mode that drives later txn-tagging
    ///    decisions.
    /// 2. **THR_LOCK downgrade**: possibly weakens the lock_type the
    ///    SQL layer should install — `TL_WRITE_CONCURRENT_INSERT..=TL_WRITE`
    ///    collapses to `TL_WRITE_ALLOW_WRITE` outside `LOCK TABLES`
    ///    so concurrent writers aren't blocked; `TL_READ_NO_INSERT`
    ///    becomes `TL_READ` outside `LOCK TABLES` to allow inserts
    ///    into the read-locked table in INSERT-SELECT patterns.
    ///
    /// Returns the (possibly downgraded) `ThrLockType` for the SQL
    /// layer to install. The C++ also writes the lock into a
    /// `THR_LOCK_DATA**` cursor — that's a cxx-bridge-side concern.
    ///
    /// ## What's deferred
    ///
    /// The `lock_scanned_rows` upgrade path (`ha_rocksdb.cc:11299..11323`)
    /// — when the THD's `lock_scanned_rows` sysvar is on AND the
    /// isolation level is `>= REPEATABLE_READ` (or `SERIALIZABLE`),
    /// MyRocks upgrades a NONE-mode read to a READ-mode read so
    /// scanned rows hold their lock past the scan. We don't model
    /// sysvars or `thd_tx_isolation` across cxx yet; deferred until
    /// the sysvar plumbing lands.
    pub fn store_lock(&mut self, thd: StoreLockThd, requested: ThrLockType) -> ThrLockType {
        // ----- 1. row-lock-mode decision -----
        if (requested as i32) >= (ThrLockType::WriteAllowWrite as i32) {
            self.lock_rows = RowLockMode::Write;
        } else if requested == ThrLockType::ReadWithSharedLocks {
            self.lock_rows = RowLockMode::Read;
        } else if requested != ThrLockType::Ignore {
            self.lock_rows = RowLockMode::None;
            // lock_scanned_rows + tx_isolation upgrade is deferred —
            // see method docs.
        }

        // ----- 2. THR_LOCK downgrade -----
        //
        // The C++ guards this with `m_db_lock.type == TL_UNLOCK` so a
        // re-entrant store_lock on an already-locked handler doesn't
        // downgrade the active lock. Match by checking `db_lock_type`.
        if requested != ThrLockType::Ignore && self.db_lock_type == ThrLockType::Unlock {
            let mut chosen = requested;

            // Concurrent-writes downgrade: TL_WRITE_CONCURRENT_INSERT..=TL_WRITE
            // → TL_WRITE_ALLOW_WRITE when outside LOCK TABLES + not a
            // tablespace op.
            if (chosen as i32) >= (ThrLockType::WriteConcurrentInsert as i32)
                && (chosen as i32) <= (ThrLockType::Write as i32)
                && !thd.in_lock_tables
                && !thd.tablespace_op
            {
                chosen = ThrLockType::WriteAllowWrite;
            }

            // INSERT…SELECT pattern: TL_READ_NO_INSERT → TL_READ
            // outside LOCK TABLES so the source table doesn't block
            // inserts into itself.
            if chosen == ThrLockType::ReadNoInsert && !thd.in_lock_tables {
                chosen = ThrLockType::Read;
            }

            self.db_lock_type = chosen;
            return chosen;
        }

        requested
    }

    /// Snapshot of [`Self::lock_rows`] — useful for tests and for the
    /// soon-to-land DML/read path that consumes it.
    pub fn lock_rows(&self) -> RowLockMode {
        self.lock_rows
    }

    /// Snapshot of the installed THR_LOCK type. Test/diagnostic accessor.
    pub fn db_lock_type(&self) -> ThrLockType {
        self.db_lock_type
    }

    /// Handler capability/hint toggle. Translated from
    /// `ha_rocksdb::extra` at `ha_rocksdb.cc:11939`.
    ///
    /// Five variants do real work; everything else falls through to
    /// a silent no-op (the C++ `default: break;`).
    ///
    /// Always returns `Ok(())` — toggles are infallible; the C++
    /// signature is `int` only because every MariaDB handler method
    /// returns `int`.
    pub fn extra(&mut self, op: HaExtraFunction) -> Result<(), Error> {
        match op {
            HaExtraFunction::Keyread => {
                self.keyread_only = true;
            }
            HaExtraFunction::NoKeyread => {
                self.keyread_only = false;
            }
            HaExtraFunction::Flush => {
                // If the table has BLOB columns they're part of the
                // retrieved-record buffer; flushing it invalidates
                // the BLOB handles the caller may still hold (C++
                // m_retrieved_record.Reset()).
                self.retrieved_record.clear();
            }
            HaExtraFunction::InsertWithUpdate => {
                // C++ gates this on the `rocksdb_enable_insert_with_update_caching`
                // sysvar (defaults true). We don't have sysvars across
                // cxx yet — hardcode the on-by-default behaviour;
                // sysvar override lands when sysvar plumbing does.
                self.insert_with_update = true;
            }
            HaExtraFunction::NoIgnoreDupKey => {
                self.insert_with_update = false;
            }
            HaExtraFunction::Other => {}
        }
        Ok(())
    }

    /// Snapshot of the keyread-only flag. Test/diagnostic accessor.
    pub fn keyread_only(&self) -> bool {
        self.keyread_only
    }

    /// Snapshot of the insert-with-update flag. Test/diagnostic accessor.
    pub fn insert_with_update(&self) -> bool {
        self.insert_with_update
    }

    /// Length of the retrieved-record buffer. Test/diagnostic accessor.
    pub fn retrieved_record_len(&self) -> usize {
        self.retrieved_record.len()
    }

    /// Push test bytes into the retrieved-record buffer. Only used
    /// by tests to verify `HA_EXTRA_FLUSH` clears it. Will be
    /// replaced by a real per-handler row-fetch path when DML/read
    /// lands.
    #[cfg(test)]
    pub(crate) fn set_retrieved_record_for_test(&mut self, bytes: &[u8]) {
        self.retrieved_record.clear();
        self.retrieved_record.extend_from_slice(bytes);
    }
}

/// Wrap a `Result<(), Error>` from a handler method as an `i32`
/// status code for the cxx boundary. `Ok` → `status::OK`; the
/// `Error` kind drives the failure code:
/// - `Invalid` → [`status::BAD_TABLE_PATH`] (or `NO_ENGINE` /
///   `ENGINE_IO_FAILED` for non-path invalids; we collapse to
///   `BAD_TABLE_PATH` for the MVS).
/// - `Data` → [`status::NO_SUCH_TABLE`]
/// - others → [`status::ENGINE_IO_FAILED`]
///
/// Coarse-grained at this stage; same trade-off as the bridge's
/// `init_in_memory` return codes — richer info will surface via the
/// HA_ERR translation channel once we have one.
pub(crate) fn open_result_to_status(r: Result<(), Error>) -> i32 {
    match r {
        Ok(()) => status::OK,
        Err(e) => match e.kind() {
            slatedb::ErrorKind::Invalid => {
                // Differentiate "no engine" vs "bad path" by message
                // text — fragile, but acceptable until we plumb a
                // richer error variant. The two come from different
                // call sites in this module so the substring is
                // discriminating.
                if e.to_string().contains("no engine installed") {
                    status::NO_ENGINE
                } else {
                    status::BAD_TABLE_PATH
                }
            }
            slatedb::ErrorKind::Data => status::NO_SUCH_TABLE,
            _ => status::ENGINE_IO_FAILED,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::codec::key::{
        IndexType, KeyDef, INDEX_INFO_VERSION_LATEST, PRIMARY_FORMAT_VERSION_LATEST,
    };
    use crate::engine::ddl_manager::DdlManager;
    use parking_lot::Mutex;
    use std::sync::Arc;

    /// Bridge tests serialise on this for the same reasons as
    /// `bridge::tests::SERIALISE` — process-global state.
    static SERIALISE: Mutex<()> = Mutex::new(());

    fn pk(index_number: u32, cf_id: u32) -> Arc<KeyDef> {
        let mut kd = KeyDef::new_skeleton(
            index_number,
            cf_id,
            0,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Primary,
            PRIMARY_FORMAT_VERSION_LATEST,
            false,
            "pk",
        );
        kd.maxlength = 12;
        Arc::new(kd)
    }

    /// Install an engine and put a fixture table in its catalogue.
    /// Tests that need a bound handler use this.
    ///
    /// Synchronous wrapper because the bridge installs its own tokio
    /// runtime; nesting a `#[tokio::test]` runtime inside would
    /// panic with "cannot start a runtime from within a runtime".
    fn install_engine_with_fixture(name_in_dot: &str) {
        // Clean slate.
        let _ = crate::bridge::slatedb_shutdown();
        assert_eq!(
            crate::bridge::slatedb_init_in_memory(format!("handler_test_{name_in_dot}")),
            status::OK,
            "init",
        );
        // Install via the live engine reference, blocking on the
        // bridge's runtime for the async put_and_write call.
        let db = crate::bridge::current_engine().expect("engine just installed");
        let ddl = crate::bridge::current_ddl().expect("ddl just installed");
        let tdef =
            Arc::new(TblDef::new(name_in_dot).unwrap().with_keys(vec![pk(100, 1)]));
        crate::runtime::block_on(async move {
            ddl.put_and_write(tdef, db.db()).await.expect("put_and_write");
        });
    }

    #[test]
    fn new_handler_is_not_open() {
        let h = HaSlateDb::new();
        assert!(!h.is_open());
        assert!(h.tbl_def().is_none());
    }

    #[test]
    fn open_rejects_malformed_path() {
        let mut h = HaSlateDb::new();
        let err = h.open("not_a_path").unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(!h.is_open());
    }

    #[test]
    fn open_then_close_round_trip() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.t1");

        let mut h = HaSlateDb::new();
        h.open("./appdb/t1").expect("open");
        assert!(h.is_open());
        assert_eq!(
            h.tbl_def().unwrap().full_tablename(),
            "appdb.t1",
            "tbl_def is bound correctly",
        );

        h.close().expect("close");
        assert!(!h.is_open());
        // Second close is OK.
        h.close().expect("close again");

        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn open_for_unknown_table_returns_data_error() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.exists");

        let mut h = HaSlateDb::new();
        let err = h.open("./appdb/does_not_exist").unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Data);
        assert!(!h.is_open());

        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn open_without_engine_returns_invalid() {
        let _g = SERIALISE.lock();
        let _ = crate::bridge::slatedb_shutdown();

        let mut h = HaSlateDb::new();
        let err = h.open("./appdb/anything").unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(err.to_string().contains("no engine installed"));
    }

    // ----- status code mapping -----

    #[test]
    fn open_result_to_status_maps_each_error_kind() {
        assert_eq!(open_result_to_status(Ok(())), status::OK);

        // Bad path (Invalid without "no engine" marker).
        let bad = HaSlateDb::new().open("not_a_path").unwrap_err();
        assert_eq!(open_result_to_status(Err(bad)), status::BAD_TABLE_PATH);

        // No engine (Invalid with "no engine" marker).
        let mut h = HaSlateDb::new();
        let no_eng = {
            let _g = SERIALISE.lock();
            let _ = crate::bridge::slatedb_shutdown();
            h.open("./appdb/x").unwrap_err()
        };
        assert_eq!(open_result_to_status(Err(no_eng)), status::NO_ENGINE);

        // Data error (no such table) — we synthesise one without
        // standing up a fixture engine.
        let data = slatedb::Error::data("no such table".into());
        assert_eq!(open_result_to_status(Err(data)), status::NO_SUCH_TABLE);

        // Other (Unavailable / Internal / etc.) → ENGINE_IO_FAILED.
        let unavail = slatedb::Error::unavailable("transient".into());
        assert_eq!(open_result_to_status(Err(unavail)), status::ENGINE_IO_FAILED);
    }

    #[test]
    fn ddl_manager_is_unused_import_silence() {
        // Tiny sanity: DdlManager is reachable from this module (we
        // use it transitively via bridge::current_ddl). This is here
        // so the import isn't flagged as unused when the doc-test for
        // DdlManager isn't compiled.
        let _ = DdlManager::new();
    }

    // ----- store_lock -----

    fn thd_default() -> StoreLockThd {
        StoreLockThd::default()
    }

    #[test]
    fn store_lock_write_set_lock_rows_write() {
        let mut h = HaSlateDb::new();
        for write_kind in [
            ThrLockType::WriteAllowWrite,
            ThrLockType::WriteConcurrentInsert,
            ThrLockType::WriteDefault,
            ThrLockType::WriteLowPriority,
            ThrLockType::Write,
            ThrLockType::WriteOnly,
        ] {
            let mut h2 = HaSlateDb::new();
            h2.store_lock(thd_default(), write_kind);
            assert_eq!(h2.lock_rows(), RowLockMode::Write, "{write_kind:?}");
        }
        // Sanity: parameterised over a clean handler each iteration; h
        // here is just to demonstrate the API also accepts a long-lived
        // handler.
        h.store_lock(thd_default(), ThrLockType::WriteDefault);
        assert_eq!(h.lock_rows(), RowLockMode::Write);
    }

    #[test]
    fn store_lock_read_with_shared_locks_sets_lock_rows_read() {
        let mut h = HaSlateDb::new();
        h.store_lock(thd_default(), ThrLockType::ReadWithSharedLocks);
        assert_eq!(h.lock_rows(), RowLockMode::Read);
    }

    #[test]
    fn store_lock_plain_read_sets_lock_rows_none() {
        let mut h = HaSlateDb::new();
        h.store_lock(thd_default(), ThrLockType::Read);
        assert_eq!(h.lock_rows(), RowLockMode::None);
    }

    #[test]
    fn store_lock_ignore_does_not_change_lock_rows() {
        let mut h = HaSlateDb::new();
        // Seed lock_rows = Write via an earlier call.
        h.store_lock(thd_default(), ThrLockType::Write);
        assert_eq!(h.lock_rows(), RowLockMode::Write);

        // TL_IGNORE must NOT clear it.
        h.store_lock(thd_default(), ThrLockType::Ignore);
        assert_eq!(h.lock_rows(), RowLockMode::Write);
    }

    #[test]
    fn store_lock_downgrades_write_range_outside_lock_tables() {
        for input in [
            ThrLockType::WriteConcurrentInsert,
            ThrLockType::WriteDelayed,
            ThrLockType::WriteDefault,
            ThrLockType::WriteLowPriority,
            ThrLockType::Write,
        ] {
            let mut h = HaSlateDb::new();
            let out = h.store_lock(thd_default(), input);
            assert_eq!(
                out,
                ThrLockType::WriteAllowWrite,
                "downgrade for {input:?}",
            );
            assert_eq!(h.db_lock_type(), ThrLockType::WriteAllowWrite);
        }
    }

    #[test]
    fn store_lock_does_not_downgrade_inside_lock_tables() {
        let thd = StoreLockThd {
            in_lock_tables: true,
            tablespace_op: false,
        };
        let mut h = HaSlateDb::new();
        let out = h.store_lock(thd, ThrLockType::WriteDefault);
        assert_eq!(out, ThrLockType::WriteDefault, "inside LOCK TABLES, no downgrade");
    }

    #[test]
    fn store_lock_does_not_downgrade_for_tablespace_op() {
        let thd = StoreLockThd {
            in_lock_tables: false,
            tablespace_op: true,
        };
        let mut h = HaSlateDb::new();
        let out = h.store_lock(thd, ThrLockType::WriteDefault);
        assert_eq!(out, ThrLockType::WriteDefault, "tablespace op blocks downgrade");
    }

    #[test]
    fn store_lock_downgrades_read_no_insert_outside_lock_tables() {
        let mut h = HaSlateDb::new();
        let out = h.store_lock(thd_default(), ThrLockType::ReadNoInsert);
        assert_eq!(out, ThrLockType::Read);
        // The lock_rows decision for plain reads is still None.
        assert_eq!(h.lock_rows(), RowLockMode::None);
    }

    #[test]
    fn store_lock_does_not_redowngrade_when_lock_already_installed() {
        // After the first store_lock installs a lock, a second call
        // must NOT re-downgrade it — matches the C++ `m_db_lock.type
        // == TL_UNLOCK` gate. Re-entrant call returns the input lock
        // unchanged.
        let mut h = HaSlateDb::new();
        let first = h.store_lock(thd_default(), ThrLockType::Write);
        assert_eq!(first, ThrLockType::WriteAllowWrite);

        // Now the handler holds WriteAllowWrite. A second call should
        // see db_lock_type != Unlock and return its input verbatim.
        let second = h.store_lock(thd_default(), ThrLockType::ReadNoInsert);
        assert_eq!(
            second,
            ThrLockType::ReadNoInsert,
            "no downgrade once a lock is installed",
        );
    }

    #[test]
    fn store_lock_returns_input_for_lock_types_outside_downgrade_set() {
        let mut h = HaSlateDb::new();
        let out = h.store_lock(thd_default(), ThrLockType::ReadHighPriority);
        // Not in the WriteConcurrentInsert..=Write range and not
        // ReadNoInsert ⇒ returned unchanged.
        assert_eq!(out, ThrLockType::ReadHighPriority);
    }

    #[test]
    fn thr_lock_type_from_i32_round_trips_known_values() {
        for v in [-1i32, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12] {
            assert_eq!(ThrLockType::from_i32(v) as i32, v);
        }
        // Unknown -> Ignore.
        assert_eq!(ThrLockType::from_i32(99), ThrLockType::Ignore);
        assert_eq!(ThrLockType::from_i32(-2), ThrLockType::Ignore);
    }

    // ----- extra -----

    #[test]
    fn extra_keyread_toggles_flag() {
        let mut h = HaSlateDb::new();
        assert!(!h.keyread_only());
        h.extra(HaExtraFunction::Keyread).unwrap();
        assert!(h.keyread_only());
        h.extra(HaExtraFunction::NoKeyread).unwrap();
        assert!(!h.keyread_only());
    }

    #[test]
    fn extra_flush_clears_retrieved_record() {
        let mut h = HaSlateDb::new();
        h.set_retrieved_record_for_test(b"row-bytes");
        assert_eq!(h.retrieved_record_len(), 9);
        h.extra(HaExtraFunction::Flush).unwrap();
        assert_eq!(h.retrieved_record_len(), 0);
    }

    #[test]
    fn extra_insert_with_update_pairs() {
        let mut h = HaSlateDb::new();
        assert!(!h.insert_with_update());
        h.extra(HaExtraFunction::InsertWithUpdate).unwrap();
        assert!(h.insert_with_update());
        h.extra(HaExtraFunction::NoIgnoreDupKey).unwrap();
        assert!(!h.insert_with_update());
    }

    #[test]
    fn extra_other_is_a_silent_noop() {
        let mut h = HaSlateDb::new();
        // Seed every flag in the "on" state.
        h.extra(HaExtraFunction::Keyread).unwrap();
        h.extra(HaExtraFunction::InsertWithUpdate).unwrap();
        h.set_retrieved_record_for_test(b"data");

        // Other must not touch any of them.
        h.extra(HaExtraFunction::Other).unwrap();
        assert!(h.keyread_only(), "keyread unchanged");
        assert!(h.insert_with_update(), "insert_with_update unchanged");
        assert_eq!(h.retrieved_record_len(), 4, "retrieved_record unchanged");
    }

    #[test]
    fn extra_flush_does_not_touch_other_flags() {
        let mut h = HaSlateDb::new();
        h.extra(HaExtraFunction::Keyread).unwrap();
        h.extra(HaExtraFunction::InsertWithUpdate).unwrap();
        h.set_retrieved_record_for_test(b"data");

        h.extra(HaExtraFunction::Flush).unwrap();
        assert_eq!(h.retrieved_record_len(), 0, "flush cleared the buffer");
        assert!(h.keyread_only(), "flush did not touch keyread");
        assert!(h.insert_with_update(), "flush did not touch insert_with_update");
    }

    #[test]
    fn ha_extra_function_from_i32_matches_mariadb_values() {
        // Pinned values from include/my_base.h:133.
        assert_eq!(HaExtraFunction::from_i32(7), HaExtraFunction::Keyread);
        assert_eq!(HaExtraFunction::from_i32(8), HaExtraFunction::NoKeyread);
        assert_eq!(HaExtraFunction::from_i32(22), HaExtraFunction::Flush);
        assert_eq!(
            HaExtraFunction::from_i32(26),
            HaExtraFunction::NoIgnoreDupKey,
        );
        assert_eq!(
            HaExtraFunction::from_i32(41),
            HaExtraFunction::InsertWithUpdate,
        );
        // Anything else → Other.
        for v in [0, 1, 2, 3, 4, 5, 6, 9, 10, 21, 23, 24, 25, 27, 40, 42, 99, -5] {
            assert_eq!(
                HaExtraFunction::from_i32(v),
                HaExtraFunction::Other,
                "{v} should be Other",
            );
        }
    }
}
