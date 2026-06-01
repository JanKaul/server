//! Cxx bridge — Field/TABLE callback surface (`extern "C++"`).
//!
//! Counterpart of `storage/slatedb/shim/slatedb_field_callbacks.h`.
//! Declares the C++ functions the Rust codec will call to access
//! MariaDB `Field*` / `TABLE*` state.
//!
//! ## Lifetime contract
//!
//! Memory ownership stays on the MariaDB side throughout. Rust
//! never sees a `Box<FieldRef>`, never owns a `UniquePtr<FieldRef>`,
//! never frees a `Field` / `TABLE`. The bridge marshals borrowed
//! references (`&FieldRef`, `Pin<&mut FieldRef>`) — the underlying
//! `Field*` / `TABLE*` is pinned by the blocked MariaDB thread
//! that issued the Rust call.
//!
//! The Rust caller must NOT retain a `&FieldRef` past the cxx
//! callback that returned it.
//!
//! ## Buffer semantics
//!
//! Column data is exchanged by fill-buffer — no allocation crosses
//! the boundary:
//! - Read direction: Rust supplies `&mut [u8]`, C++ writes into it.
//! - Write direction: Rust supplies `&[u8]`, C++ copies into Field.
//!
//! ## Build gating
//!
//! This module is gated by the `field_callbacks` Cargo feature.
//! Default `cargo test --lib` doesn't enable it (no C++ build
//! context = no link target for the `extern "C++"` symbols).
//! CMake's `corrosion_import_crate(... FEATURES field_callbacks)`
//! turns it on for the plugin build, at which point the cxx-
//! generated bridge .cc `#include`s `slatedb_field_callbacks.h`
//! which inline-forwards to MariaDB's Field/TABLE methods.
//!
//! ## Status
//!
//! **Slice 1**: bridge surface (Rust `extern "C++"` + C++ shim).
//! **Slice 2**: first Rust caller — [`pack_with_sort_string`].
//! Wraps [`ffi::field_sort_string`] with size validation and a
//! `Result` return. This is the Rust counterpart of MyRocks'
//! `Rdb_key_def::pack_with_make_sort_key` (`rdb_datadic.cc:1489`)
//! — the universal "encode a field's value in memcmp form via
//! `field->sort_string`" pack routine.
//!
//! The function is not yet installed in [`FieldPacking::pack_func`].
//! That slot's current signature (`fn(&mut FieldPacking, &mut
//! FieldView, ...)`) was modelled before the cxx bridge existed
//! and uses a Rust-side metadata POD rather than a live
//! `&FieldRef`. Redesigning the slot's signature to accept
//! `&FieldRef` (and threading that through every call site) is a
//! separate slice that lands alongside the rest of the codec
//! pack pipeline (`KeyDef::pack_record`).

#[cfg(feature = "field_callbacks")]
#[cxx::bridge(namespace = "slatedb")]
pub mod ffi {
    unsafe extern "C++" {
        include!("slatedb_field_callbacks.h");

        /// Opaque wrapper around MariaDB's `Field*`. Lifetime is
        /// managed by MariaDB — Rust only ever sees `&FieldRef`
        /// (or `Pin<&mut FieldRef>`), never by value.
        type FieldRef;

        /// Opaque wrapper around MariaDB's `TABLE*`. Same
        /// non-ownership contract as [`FieldRef`].
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
        /// opaque lifetime to manage. A future `charset_by_id`
        /// callback resolves the id when collation-aware unpack
        /// lands.
        fn field_charset_number(f: &FieldRef) -> u32;

        /// Bit position within the null-byte for this Field.
        fn field_null_bit(f: &FieldRef) -> u32;

        /// Null-byte offset (relative to `record[0]` start).
        /// Returns `-1` if the Field is non-nullable.
        fn field_null_offset(f: &FieldRef) -> i32;

        /// Bitmap of "which keys does this Field participate in"
        /// (MariaDB caps at 64 keys per table).
        fn field_part_of_key(f: &FieldRef) -> u64;

        // ----- Read column bytes (fill-buffer) -----

        /// Encode the column's value into `dst` in memcmp (sort)
        /// form. Writes `min(max_len, dst.len())` bytes. The C++
        /// side wraps the call in `dbug_tmp_use_all_columns` to
        /// bypass read-set checks (matches the MyRocks
        /// `pack_with_make_sort_key` pattern).
        fn field_sort_string(f: &FieldRef, dst: &mut [u8], max_len: u32);

        /// Copy the raw on-record bytes into `dst`. Writes
        /// `min(field.pack_length(), dst.len())` bytes.
        fn field_ptr_bytes(f: &FieldRef, dst: &mut [u8]);

        // ----- Write column state (table-indexed) -----
        //
        // cxx requires a `&mut` argument to return `&mut T`, so
        // we expose the mutating ops as TableRef-indexed free
        // functions rather than methods on `Pin<&mut FieldRef>`.
        // The lifetime gymnastics for the latter add no real
        // safety — the underlying `Field` (owned by MariaDB) is
        // pinned by the blocked thread regardless of how we
        // borrow into it.

        /// Copy `src` into the `i`-th field's storage. Returns
        /// the number of bytes written
        /// (`min(pack_length, src.len())`). The decoder always
        /// passes exactly `pack_length(i)` bytes so the full
        /// source is copied.
        fn table_field_set_value(t: &TableRef, i: u32, src: &[u8]) -> u32;

        /// Mark the `i`-th field as SQL NULL. Caller must ensure
        /// the field is NULLABLE.
        fn table_field_set_null(t: &TableRef, i: u32);

        /// Mark the `i`-th field as NOT NULL.
        fn table_field_set_notnull(t: &TableRef, i: u32);

        // ----- TABLE access -----

        /// Borrow the Field at `field_index` from a TABLE. The
        /// returned reference shares the TABLE's lifetime (bounded
        /// by the blocked MariaDB thread); see module-doc for the
        /// retention rule.
        ///
        /// The C++ side returns a reference to a thread-local
        /// scratch `FieldRef` whose `.ptr` is re-pointed on each
        /// call. Single-thread-per-handler-call invariant from
        /// MariaDB makes this safe.
        fn table_field_at(t: &TableRef, field_index: u32) -> &FieldRef;

        /// Borrow the in-progress row buffer (`record[0]`).
        /// Length is `table->s->stored_rec_length`.
        fn table_record_buf(t: &TableRef) -> &[u8];

        // ----- Schema introspection (CREATE TABLE / value blob) -----

        /// Opaque wrapper around MariaDB's `KEY *` (one entry of
        /// `TABLE_SHARE::key_info`). Same lifetime contract as
        /// [`FieldRef`] / [`TableRef`].
        type KeyInfoRef;

        /// Number of declared columns (`TABLE_SHARE::fields`).
        /// The value-blob encoder uses this to bound its
        /// field-iteration loop.
        fn table_field_count(t: &TableRef) -> u32;

        /// Number of declared keys (`TABLE_SHARE::keys`).
        fn table_key_count(t: &TableRef) -> u32;

        /// True iff the table has a user-declared PRIMARY KEY.
        /// `false` means the SQL layer didn't supply one and the
        /// engine should synthesise a hidden PK at CREATE time.
        fn table_has_primary_key(t: &TableRef) -> bool;

        /// Index of the PRIMARY KEY within `key_info[]`. Only
        /// meaningful when [`table_has_primary_key`] is `true`
        /// (callers must guard with that first).
        fn table_primary_key_index(t: &TableRef) -> u32;

        /// Borrow the `i`-th `KEY` from a TABLE's `key_info[]`.
        /// Same thread-local-scratch pattern as
        /// [`table_field_at`] — must not be retained past the
        /// callback boundary.
        fn table_key_at(t: &TableRef, key_index: u32) -> &KeyInfoRef;

        /// Key name (`KEY::name`). Allocates a fresh `String` —
        /// CREATE TABLE is a one-shot path where per-key
        /// allocation is acceptable.
        fn key_name(k: &KeyInfoRef) -> String;
    }

    extern "Rust" {
        /// CREATE TABLE entry point — called by the C++ shim's
        /// `ha_slatedb::create` once it has wrapped `TABLE *form`
        /// in a `TableRef`. Allocates index ids, builds skeleton
        /// `KeyDef`s for each declared key (and a synthetic hidden
        /// PK if none was declared), and writes the resulting
        /// `TblDef` to the system-CF catalogue via
        /// `DdlManager::put_and_write`.
        ///
        /// `name` is MariaDB's on-disk path form
        /// (`./db/tbl[#P#part]`); the Rust side normalises it.
        ///
        /// Per-keypart `FieldPacking::setup` is deferred — the
        /// keys land as skeletons that the future codec pack
        /// pipeline fleshes out. CREATE TABLE only needs the
        /// index identity (cf_id + index_id + name + type) for
        /// catalogue lookup to work.
        fn slatedb_create_table(name: String, table: &TableRef) -> i32;

        /// INSERT row entry — called by `ha_slatedb::write_row`
        /// after `ha_external_lock(F_WRLCK)` has created the per-THD
        /// transaction. Builds the PK row key + value blob from
        /// `table` (live row in `record[0]`) and puts them via the
        /// per-THD transaction.
        ///
        /// `name` is the already-canonical `db.tbl[#P#part]`
        /// catalogue key — the C++ shim builds it from
        /// `TABLE_SHARE::db` + `TABLE_SHARE::table_name` (path
        /// normalisation isn't needed because we don't get a path
        /// from MariaDB at write_row time).
        /// `thd_id` is `thd_get_thread_id(thd)`. The thd must
        /// already have a registered txn — `external_lock(F_WRLCK)`
        /// is the responsibility of the SQL layer to call first.
        ///
        /// Stage 0: writes only the PK row, no secondary keys, no
        /// unique-check pre-read, no auto-incr field bump from the
        /// row, no TTL prefix, no debug checksum suffix.
        fn slatedb_write_row(thd_id: u64, name: String, table: &TableRef) -> i32;

        /// DELETE row entry — called by `ha_slatedb::delete_row`
        /// for explicit-PK tables. Builds the PK row key from
        /// `table` (live row in `record[0]`) and issues a
        /// `delete` on the per-THD transaction.
        ///
        /// `name` is the already-canonical `db.tbl[#P#part]`
        /// catalogue key. `thd_id` must already have a registered
        /// txn (`external_lock(F_WRLCK)` runs first).
        ///
        /// Stage 0 limitation: **explicit-PK tables only**.
        /// Hidden-PK delete needs the captured rowid from the
        /// prior scan/read (MyRocks's `m_last_rowkey`), which
        /// isn't plumbed yet. Calling this against a hidden-PK
        /// table returns `ENGINE_IO_FAILED`. No SK deletes, no
        /// pre-read for foreign-key checks, no read-free-rpl
        /// optimisation — Stage 0 ports the minimum that makes
        /// `DELETE FROM t WHERE pk = ?` correct.
        fn slatedb_delete_row(thd_id: u64, name: String, table: &TableRef) -> i32;
    }
}

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
/// duration of this call. See the module doc for the retention
/// rule.
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
/// This is the missing link between the pure-Rust pack_record
/// orchestrator and the C++ Field methods — once the C++ shim's
/// `ha_slatedb::write_row` builds a `TableRef` and calls this,
/// PK and SK row-key construction works end-to-end.
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
        // Per-keypart dispatch. Stage 0 has only the sort_string
        // variant; once more pack helpers exist (VARCHAR length-
        // prefix, BLOB prefix, etc.), this branches on
        // `fpi.max_image_len`/charset metadata.
        let field = ffi::table_field_at(table, fpi.field_index());
        pack_with_sort_string(fpi, field, kp_dst)
    };
    key_def.pack_record(&mut packer, hidden_pk_id, dst)
}

/// `RowValueSource` implementation backed by a live `TableRef`.
///
/// The cxx-side counterpart of [`crate::codec::row_value::RowValueSource`]
/// — supplies the per-field info that
/// [`crate::codec::row_value::encode_row_value`] consumes when
/// the encoder runs against an actual MariaDB row.
///
/// `is_in_pk` is precomputed at construction from the PK `KeyDef`'s
/// `pack_info[].field_index` values (populated by `KeyDef::setup`).
/// `is_null` / `pack_length` / `write_field_bytes` all dispatch
/// through the cxx callbacks (`field_is_real_null`,
/// `field_pack_length`, `field_ptr_bytes`).
///
/// Lifetime contract: the `&TableRef` is borrowed for the source's
/// lifetime — same retention rule as everywhere else in this
/// module (must not outlive the cxx callback that handed it back
/// to Rust).
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
    /// Splitting this two-step lets the caller reuse the same
    /// mask for the null-bitmap layout computation (which also
    /// needs `is_in_pk`) without borrowing the source.
    pub fn new(table: &'a ffi::TableRef, pk_field_mask: Vec<bool>) -> Self {
        Self {
            table,
            pk_field_mask,
        }
    }

    /// Build the per-field PK exclusion mask. `mask[i] = true`
    /// iff field `i` (by its `TABLE_SHARE::field[]` index) is a
    /// keypart of `pk_def`.
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
        // `field_ptr_bytes` writes `min(pack_length, dst.len())`
        // bytes. The encoder pre-sizes `dst` to exactly
        // `pack_length(i)` (which is the same value the cxx side
        // returns), so `min(pl, pl) = pl` — no short write.
        ffi::field_ptr_bytes(field, dst);
        Ok(ffi::field_pack_length(field) as usize)
    }
}

/// `RowValueSink` implementation backed by a live `TableRef`.
/// Symmetric counterpart of [`TableRefRowValueSource`] — the
/// read-path decoder writes column bytes back into MariaDB
/// `Field`s through this sink.
///
/// `is_in_pk` is precomputed at construction (same shape as
/// `TableRefRowValueSource`). `set_null` and `set_field_bytes`
/// dispatch through `field_set_null` and `field_set_value` on a
/// `Pin<&mut FieldRef>` obtained from `table_field_at_mut`.
///
/// Lifetime contract: `&TableRef` borrowed for the sink's
/// lifetime — same no-retain rule as the rest of bridge_field.
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
        // separate bit from `field->ptr`'s contents. The decoder
        // only calls set_field_bytes for non-null fields, so we
        // always clear the null bit before writing.
        ffi::table_field_set_notnull(self.table, i);
        let _n = ffi::table_field_set_value(self.table, i, src);
        // `table_field_set_value` writes
        // `min(pack_length, src.len())` bytes — the decoder
        // passes exactly `pack_length(i)`, so the full source is
        // copied. No short-write check (the sink trait doesn't
        // enforce one).
        Ok(())
    }
}

/// Find the primary-key `KeyDef` in a `TblDef`. Returns the
/// last key slot (which is the hidden PK on hidden-PK tables, or
/// the only Primary on explicit-PK tables — search by index_type).
///
/// Used by [`slatedb_write_row`] / future read-path entries that
/// need the PK for row-key encoding / value-blob PK exclusion.
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
/// `name` is MariaDB's on-disk path form (`./db/tbl[#P#part]`);
/// `thd_id` is `thd_get_thread_id(thd)`. The thd's txn must
/// already exist — `ha_slatedb::external_lock(F_WRLCK)` creates
/// it before MariaDB issues the write.
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
/// Stage 0 limitations: no SK writes (only PK row), no
/// unique-check pre-read, no auto-incr field bump from the row
/// value, no TTL prefix, no debug checksum.
///
/// Returns: `OK` on success; `NO_ENGINE` pre-init; `NO_SUCH_TABLE`
/// if the catalogue lookup fails; `ENGINE_IO_FAILED` on any
/// codec or txn error.
#[cfg(feature = "field_callbacks")]
fn slatedb_write_row(thd_id: u64, name: String, table: &ffi::TableRef) -> i32 {
    use crate::bridge::status;
    use crate::codec::key::{IndexType, INDEX_NUMBER_SIZE};
    use crate::codec::row_value::{
        compute_value_null_bitmap_layout, encode_row_value,
    };
    use crate::handler::status as handler_status;

    // ----- engine + catalogue lookup -----
    let Some(ddl) = crate::bridge::current_ddl() else {
        return handler_status::NO_ENGINE;
    };
    let Some(engine) = crate::bridge::current_engine() else {
        return handler_status::NO_ENGINE;
    };
    let Some(registry) = crate::bridge::current_txn_registry() else {
        return handler_status::NO_ENGINE;
    };
    let Some(tdef) = ddl.find(&name) else {
        return handler_status::NO_SUCH_TABLE;
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
        // Pre-allocate enough room for the largest possible PK
        // encoding (max_storage_fmt_length / `maxlength`). The
        // orchestrator returns the actual bytes written.
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
        // Shared borrow of pk_field_mask for the layout closure.
        // Ends at the closing brace; pk_field_mask is then moved
        // into the source below.
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
        // No txn for this THD — external_lock must run first.
        return handler_status::NO_ENGINE;
    };
    let put_result = txn.put(&pk_buf, &value_buf);
    registry.reinsert(thd_id, txn);
    // engine borrow is unused — we go through the registry's txn,
    // not the raw db handle. Keep the lookup so this fails fast
    // when init/shutdown is mid-flight.
    let _ = engine;

    match put_result {
        Ok(_) => status::OK,
        Err(_) => status::ENGINE_IO_FAILED,
    }
}

/// Cxx `extern "Rust"` entry — called by the C++ shim's
/// `ha_slatedb::delete_row`. Builds the PK row key from the live
/// MariaDB row (via `pack_record_via_table`) and issues a
/// `delete` on the per-THD transaction.
///
/// `name` is the canonical `db.tbl[#P#part]` catalogue key;
/// `thd_id` must already have a registered txn.
///
/// ## Flow
///
/// 1. Resolve `TblDef` by name. Fail with `NO_SUCH_TABLE` if not
///    present.
/// 2. Pick the PK `KeyDef`. **Explicit PK required in Stage 0** —
///    hidden-PK delete needs the captured rowid from the prior
///    scan (MyRocks's `m_last_rowkey`), which isn't plumbed.
///    Returns `ENGINE_IO_FAILED` for hidden-PK tables.
/// 3. Pack the PK from `record[0]` via [`pack_record_via_table`].
/// 4. Take the per-THD txn, call `delete(pk_key)`, put it back.
///
/// Symmetric with [`slatedb_write_row`] — same lookup chain, same
/// take/reinsert dance against [`crate::engine::txn_registry::TxnRegistry`].
/// No value-blob handling (delete only needs the key).
///
/// Returns: `OK` on success; `NO_ENGINE` pre-init; `NO_SUCH_TABLE`
/// if catalogue lookup fails; `ENGINE_IO_FAILED` for hidden-PK
/// tables (not yet supported) or any codec/txn error.
#[cfg(feature = "field_callbacks")]
fn slatedb_delete_row(thd_id: u64, name: String, table: &ffi::TableRef) -> i32 {
    use crate::bridge::status;
    use crate::codec::key::IndexType;
    use crate::handler::status as handler_status;

    // ----- engine + catalogue lookup -----
    let Some(ddl) = crate::bridge::current_ddl() else {
        return handler_status::NO_ENGINE;
    };
    let Some(registry) = crate::bridge::current_txn_registry() else {
        return handler_status::NO_ENGINE;
    };
    let Some(tdef) = ddl.find(&name) else {
        return handler_status::NO_SUCH_TABLE;
    };
    let Some(pk_kd) = find_pk_keydef(&tdef) else {
        return status::ENGINE_IO_FAILED;
    };

    // Hidden-PK delete needs the captured rowid from m_last_rowkey
    // — deferred per the function-level docs.
    if pk_kd.index_type == IndexType::HiddenPrimary {
        return status::ENGINE_IO_FAILED;
    }

    // ----- PK row key -----
    let mut pk_buf: Vec<u8> =
        vec![0u8; pk_kd.max_storage_fmt_length() as usize];
    let written = match pack_record_via_table(&pk_kd, table, None, &mut pk_buf) {
        Ok(n) => n,
        Err(_) => return status::ENGINE_IO_FAILED,
    };
    pk_buf.truncate(written);

    // ----- txn delete -----
    let Some(mut txn) = registry.take(thd_id) else {
        return handler_status::NO_ENGINE;
    };
    let delete_result = txn.delete(&pk_buf);
    registry.reinsert(thd_id, txn);

    match delete_result {
        Ok(_) => status::OK,
        Err(_) => status::ENGINE_IO_FAILED,
    }
}

/// Build a [`crate::codec::tbl_def::TblDef`] from primitive
/// schema inputs the C++ shim extracted from `TABLE *form` at
/// CREATE TABLE time.
///
/// Each `key_names[i]` becomes a skeleton `KeyDef` in slot `i`.
/// The key at `primary_key_index` (if `Some`) gets
/// `IndexType::Primary`; all others (including when
/// `primary_key_index` is `None`) get `IndexType::Secondary`.
/// If no primary was declared, a synthetic `IndexType::HiddenPrimary`
/// is appended at the end.
///
/// `index_ids` must have exactly one id per `key_names` entry plus
/// one extra when a hidden PK is synthesised (so total length is
/// `key_names.len()` or `key_names.len() + 1`). Caller is
/// responsible for allocating these via
/// [`crate::engine::ddl_manager::DdlManager::get_and_update_next_number`].
///
/// All keys land on `default_cf_id` — Stage 0 has no per-key CF
/// routing (MyRocks' `COMMENT='cf=...'` syntax is deferred).
///
/// Returns `Err(Invalid)` if `index_ids.len()` doesn't match what
/// `key_names.len()` + `primary_key_index` would require, or if
/// `full_name` is not a valid `db.tbl[#P#part]` path.
///
/// This helper is the testable core of
/// [`slatedb_create_table`] — pure Rust, no cxx dependency, so
/// it compiles and tests both with and without the
/// `field_callbacks` feature.
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

/// Cxx `extern "Rust"` body — see the declaration in [`ffi`] for
/// the wire contract. The C++ shim's `ha_slatedb::create` wraps
/// `TABLE *form` in a [`ffi::TableRef`] and calls this.
#[cfg(feature = "field_callbacks")]
fn slatedb_create_table(name: String, table: &ffi::TableRef) -> i32 {
    use crate::bridge::status;
    use crate::handler::status as handler_status;
    use std::sync::Arc;

    let Some(ddl) = crate::bridge::current_ddl() else {
        return handler_status::NO_ENGINE;
    };
    let Some(engine) = crate::bridge::current_engine() else {
        return handler_status::NO_ENGINE;
    };
    let normalized = match crate::utils::names::normalize_tablename(&name) {
        Ok(n) => n,
        Err(_) => return handler_status::BAD_TABLE_PATH,
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
    // synthetic hidden PK (when needed). These are sequential
    // SeqGenerator allocations — concurrent CREATE TABLEs get
    // distinct ranges because get_and_update_next_number is a
    // single atomic increment.
    let needed_ids = key_count as usize + usize::from(!has_pk);
    let mut index_ids: Vec<u32> = Vec::with_capacity(needed_ids);
    for _ in 0..needed_ids {
        index_ids.push(ddl.get_and_update_next_number());
    }

    let tdef = match build_tbl_def_from_schema(
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

    let Some(runtime) = crate::runtime::get() else {
        return status::RUNTIME_INIT_FAILED;
    };
    match runtime.block_on(ddl.put_and_write(Arc::new(tdef), engine.db())) {
        Ok(_) => status::OK,
        Err(_) => status::ENGINE_IO_FAILED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
