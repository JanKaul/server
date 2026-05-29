//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__metadata`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (span 6364..11725, body ~125 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__metadata`
//!
//! ## Mapping
//! Pure-Rust metadata accessors that read out of the cached `Rdb_tbl_def`
//! / `TableShare`. Almost no SlateDB interaction — these methods reach into
//! handler state that A2 owns (the `HaSlateDb` struct).
//!
//! The one exception is `generate_cf_name`, which parses the index comment
//! per the qualifier conventions defined in `rdb_global_h.rs`
//! (`CF_NAME_QUALIFIER`, `QUALIFIER_VALUE_SEP`, etc.). The result feeds
//! `engine::cf` which maps the CF name to a `cf_id` for our
//! key-prefix scheme (_DESIGN.md §2).
//!
//! `contains_foreign_key` is a flat `false` (matches MyRocks — neither
//! engine supports FK).
//!
//! `get_table_if_exists` reaches into the DDL manager (A1 owns it). We just
//! re-export the shape here for the cxx bridge.
//!
//! ## Out-of-scope methods
//! None — these are pure metadata helpers, all in scope.

use slatedb::Error;

/// Forward-declared. Owned by A2's `ha_rocksdb_h` stub.
pub struct HaSlateDb;

/// Mirror of `Rdb_tbl_def` — A1 owns the canonical type in
/// `codec::ddl`. Referenced here by reference only.
pub struct TblDef;

impl HaSlateDb {
    /// Return the table's basename (`base_tablename()` in MyRocks), borrowed
    /// from the `TblDef`. No I/O.
    ///
    /// Original C++: ha_rocksdb.cc:6364.
    pub fn get_table_basename(&self) -> &str {
        todo!("self.tbl_def.base_tablename() — borrow from cached TblDef")
    }

    /// MyRocks (and MariaDB) do not support foreign keys for RocksDB tables.
    /// Always returns `false`. Preserved as a method so the cxx bridge has
    /// a stable target; no `THD` plumbing needed.
    ///
    /// Original C++: ha_rocksdb.cc:7611.
    pub fn contains_foreign_key(&self) -> bool {
        false
    }

    /// Return the user-visible name of index `index`. For the hidden PK this
    /// returns the `HIDDEN_PK_NAME` constant from `rdb_global_h.rs`.
    /// Otherwise borrows from the cached `KeyInfo`.
    ///
    /// Inputs:
    ///   - `index`: 0..m_key_count
    ///
    /// Output: borrowed `&str` lifetime-tied to the handler's `TblDef`.
    /// Returns an empty string for invalid `index` (the caller is supposed
    /// to bounds-check; matches MyRocks debug-assert behavior in release).
    ///
    /// Original C++: ha_rocksdb.cc:9527.
    pub fn get_key_name(&self, index: u32) -> &str {
        let _ = index;
        todo!("if is_hidden_pk(index) { HIDDEN_PK_NAME } else { table.key_info[index].name }")
    }

    /// Return the comment string declared on index `index` (the place where
    /// MyRocks' "$per_index_cf" / "cfname=foo" qualifiers live). Empty for
    /// the hidden PK.
    ///
    /// Original C++: ha_rocksdb.cc:9540.
    pub fn get_key_comment(&self, index: u32) -> Option<&str> {
        let _ = index;
        todo!("None for hidden_pk; else Some(table.key_info[index].comment)")
    }

    /// Parse the index comment for a `cfname=...` qualifier. Returns the
    /// derived CF name plus a `per_part_match_found` flag indicating whether
    /// the qualifier was specifically scoped to this partition (for
    /// partitioned tables).
    ///
    /// Implementation: borrows the index comment via `get_key_comment`,
    /// hands it to `codec::key::parse_comment_for_qualifier` (A1 owns
    /// the parser). On no-comment / no-qualifier, returns
    /// `(DEFAULT_CF_NAME.to_string(), false)`.
    ///
    /// Original C++: ha_rocksdb.cc:9552.
    pub fn generate_cf_name(&self, index: u32) -> (String, bool) {
        let _ = index;
        todo!("get_key_comment(index) -> parse_comment_for_qualifier(CF_NAME_QUALIFIER) -> (cf_name, per_part_match_found)")
    }

    /// Return the table's CREATE-TABLE comment. Pure cache lookup against
    /// `TableShare.comment.str`. Used by `info_schema::tables` and by the
    /// CF-qualifier resolver (a CREATE TABLE comment can declare a default
    /// `cfname=` to apply to all indexes).
    ///
    /// Original C++: ha_rocksdb.cc:9592.
    pub fn get_table_comment(&self) -> &str {
        todo!("self.table_share.comment.str")
    }

    /// Look up an existing `TblDef` by table-path (`"./dbname/tablename"`)
    /// in the DDL manager. Returns `None` if not registered.
    ///
    /// In MyRocks the input goes through `rdb_normalize_tablename` first;
    /// we keep that normalization as a TODO for A1's codec::ddl module
    /// since it's a string-massaging concern, not a storage one.
    ///
    /// Original C++: ha_rocksdb.cc:11716.
    pub fn get_table_if_exists(&self, tablename: &str) -> Option<&TblDef> {
        let _ = tablename;
        todo!("normalize + ddl_manager.find(normalized) — see crate::codec::ddl::DdlManager")
    }
}
