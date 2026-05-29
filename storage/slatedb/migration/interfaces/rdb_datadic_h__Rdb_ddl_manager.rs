//! Interface stub for `rdb_datadic_h__Rdb_ddl_manager`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (class at line 1191)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_ddl_manager`
//! parent: `rdb_datadic_h`
//!
//! ## Mapping
//! In-memory cache of `Rdb_tbl_def` keyed by table name; layer above
//! `Rdb_dict_manager`. Provides table-creation, lookup, rename, drop.
//!
//! Per _DESIGN.md §1, this owns the table-name ↔ tbl_id mapping and the
//! cache of decoded TblDefs. Cache invalidation hooked into
//! `DbMetadataOps::subscribe()` so peers see DDL changes within the
//! manifest poll interval.
//!
//! ## Out-of-scope methods
//! None.

use slatedb::Error;
use std::sync::Arc;

use crate::rdb_datadic_h__Rdb_dict_manager::DictManager;
use crate::rdb_datadic_h__Rdb_tbl_def::TblDef;

/// DDL manager singleton. Held in the engine's `OnceLock` after init.
///
/// Original: rdb_datadic.h:1191 — `class Rdb_ddl_manager`.
pub struct DdlManager {
    dict: Arc<DictManager>,
    // m_ddl_hash equivalent — concurrent map from name → Arc<TblDef>
    // (use dashmap::DashMap or parking_lot::RwLock<HashMap> in impl)
}

impl DdlManager {
    pub fn new(dict: Arc<DictManager>) -> Self {
        Self { dict }
    }

    /// Look up a tbl_def by name. Returns the cached entry if present;
    /// else loads from `DictManager`.
    pub async fn find(&self, name: &str) -> Result<Option<Arc<TblDef>>, Error> {
        todo!("cache lookup; fall through to dict.get_tbl_def()")
    }

    /// CREATE TABLE: persist + add to cache.
    pub async fn put(&self, tbl_def: Arc<TblDef>) -> Result<(), Error> {
        todo!("dict.put_tbl_def + cache insert")
    }

    /// RENAME TABLE: atomic name swap in cache + dict.
    pub async fn rename(&self, old_name: &str, new_name: &str) -> Result<(), Error> {
        todo!()
    }

    /// DROP TABLE: remove from cache; register dropped indexes; return tbl_def
    /// so caller can complete cleanup.
    pub async fn drop(&self, name: &str) -> Result<Arc<TblDef>, Error> {
        todo!("remove from cache; dict.add_drop_index(tbl_def.indexes); return tbl_def")
    }

    /// Walk all currently-cached tbl_defs (for I_S enumeration / startup).
    /// `scanner.add_table` is called for each; abort on `Err`.
    pub async fn scan_for_tables(
        &self,
        scanner: &mut dyn crate::rdb_datadic_h__Rdb_tables_scanner::TablesScanner,
    ) -> Result<(), Error> {
        todo!("iterate cache, call scanner.add_table for each")
    }
}
