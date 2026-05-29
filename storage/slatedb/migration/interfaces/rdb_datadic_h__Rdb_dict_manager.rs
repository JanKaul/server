//! Interface stub for `rdb_datadic_h__Rdb_dict_manager`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (class at line 1371)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_dict_manager`
//! parent: `rdb_datadic_h`
//!
//! ## Mapping
//! Persistent data-dictionary in the system CF. Stores: index-id → tbl/key,
//! tbl-name → tbl-id, cf-name → cf-id, dropped-index registry, auto-incr
//! per table.
//!
//! Per _DESIGN.md §1, persisted via `DbTransaction::put/get` on system-CF-
//! prefixed keys (`SYSTEM_CF_ID = u32::MAX`).
//!
//! ## Out-of-scope methods
//! None.

use slatedb::bytes::Bytes;
use slatedb::Error;
use std::sync::Arc;

use crate::rdb_datadic_h__Rdb_tbl_def::TblDef;
use crate::rdb_global_h::GlIndexId;

/// Persistent dictionary manager. Singleton; one instance per Db.
///
/// Original: rdb_datadic.h:1371 — `class Rdb_dict_manager`.
pub struct DictManager {
    /// Borrowed Db handle for system-CF reads/writes.
    db: Arc<slatedb::Db>,
}

impl DictManager {
    pub fn new(db: Arc<slatedb::Db>) -> Self {
        Self { db }
    }

    /// Persist a fresh tbl_def into the system CF.
    pub async fn put_tbl_def(&self, tbl_def: &TblDef) -> Result<(), Error> {
        todo!("encode tbl_def; write to system_cf_prefix||tbl_id key via Db::put")
    }

    /// Load a tbl_def by name.
    pub async fn get_tbl_def(&self, name: &str) -> Result<Option<TblDef>, Error> {
        todo!("Db::get(system_cf_prefix||name_to_id_key) → tbl_id → load_by_id")
    }

    /// Mark indexes as dropped; the CompactionFilter picks these up.
    /// Original: rdb_datadic.cc — `add_drop_index`.
    pub async fn add_drop_index(&self, indexes: &[GlIndexId]) -> Result<(), Error> {
        todo!("append to dropped-index registry in system CF")
    }

    /// Read the current dropped-index registry. Called by `Rdb_compact_filter`.
    pub async fn get_dropped_indexes(&self) -> Result<Vec<GlIndexId>, Error> {
        todo!("scan_prefix(system_cf||dropped_index_prefix); decode")
    }

    /// Persist a new auto-incr value (called on overflow / explicit set).
    pub async fn put_auto_incr(&self, tbl_name: &str, value: u64) -> Result<(), Error> {
        todo!()
    }

    pub async fn get_auto_incr(&self, tbl_name: &str) -> Result<Option<u64>, Error> {
        todo!()
    }
}
