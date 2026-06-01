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

        // ----- Write column state (mutating) -----

        /// Copy `src` into the Field's storage. Returns the
        /// number of bytes written. Caller is responsible for
        /// validating `src.len()` against `pack_length()`.
        fn field_set_value(f: Pin<&mut FieldRef>, src: &[u8]) -> u32;

        /// Mark this Field as SQL NULL. Caller must ensure the
        /// Field is NULLABLE.
        fn field_set_null(f: Pin<&mut FieldRef>);

        /// Mark this Field as NOT NULL.
        fn field_set_notnull(f: Pin<&mut FieldRef>);

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

        // ----- Schema introspection (CREATE TABLE) -----

        /// Opaque wrapper around MariaDB's `KEY *` (one entry of
        /// `TABLE_SHARE::key_info`). Same lifetime contract as
        /// [`FieldRef`] / [`TableRef`].
        type KeyInfoRef;

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
    /// Build a source from a TableRef + the table's PK key def
    /// + the total field count. Precomputes the PK exclusion mask
    /// so `is_in_pk(i)` is O(1) per lookup.
    ///
    /// `field_count` is typically `ffi::table_field_count(table)`
    /// (when that callback lands) — taken as a parameter for now
    /// because the schema-introspection surface didn't expose it
    /// yet and tighter coupling here doesn't add value.
    pub fn new(
        table: &'a ffi::TableRef,
        pk_def: &crate::codec::key::KeyDef,
        field_count: u32,
    ) -> Self {
        let mut pk_field_mask = vec![false; field_count as usize];
        for fpi in &pk_def.pack_info {
            let idx = fpi.field_index() as usize;
            if idx < pk_field_mask.len() {
                pk_field_mask[idx] = true;
            }
        }
        Self {
            table,
            pk_field_mask,
        }
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
