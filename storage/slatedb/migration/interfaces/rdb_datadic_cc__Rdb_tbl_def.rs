//! Interface stub for `Rdb_tbl_def`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.cc` (lines 3530..3735)
//! C++ header: `storage/rocksdb/rdb_datadic.h` (Rdb_tbl_def declarations)
//! v4 manifest sub-unit: `Rdb_tbl_def`
//! Parent unit: `rdb_datadic.cc`
//! Approx body LoC accounted for here: ~200
//!
//! ## Mapping
//! Per _DESIGN.md §1: per-table metadata, persisted in the SYSTEM region via
//! `DictManager`. The `Rdb_tbl_def` struct holds:
//! - `m_dbname`, `m_tablename`, `m_partition` parsed from the
//!   `dbname.tablename` MySQL-internal name.
//! - `m_key_descr_arr[m_key_count]` — owned `Arc<KeyDef>` per index.
//! - `m_is_mysql_system_table` flag (used to suppress `Rdb_event_listener`
//!   stats updates per _DESIGN.md §1).
//! - `m_is_read_free_rpl_table` — **always false** per _DESIGN.md §1
//!   (read-free replication is a non-goal).
//! - `m_create_time` lazily read from the .frm file.
//!
//! `put_dict()` writes the DDL entry: `dbname.tablename → version + [(cf_id,
//! index_nr)*]`. With our key-prefix scheme `cf_id` is still allocated
//! (per `Rdb_seq_generator`) but the SlateDB instance is single — `cf_id`
//! becomes a key-prefix segment, not a separate `ColumnFamily`.
//!
//! ## Out-of-scope methods
//! - `check_and_set_read_free_rpl_table()` — always sets `false`; the
//!   feature is unsupported per §1.

use slatedb::Error;
use std::sync::Arc;

use crate::Rdb_dict_manager::{DictManager, WriteBatch};
use crate::rdb_global_h::GlIndexId;

pub struct KeyDef; // see Rdb_key_def__meta.rs

/// `Rdb_tbl_def` — owned per-table metadata.
pub struct TblDef {
    pub dbname_tablename: String,
    pub dbname: String,
    pub tablename: String,
    pub partition: String,
    pub key_descr_arr: Vec<Arc<KeyDef>>,
    pub is_mysql_system_table: bool,
    /// Always `false` — see _DESIGN.md §1 read-free-replication non-goal.
    pub is_read_free_rpl_table: bool,
    /// Lazy: 0 sentinel for "unknown", filled on first call to `create_time()`.
    pub create_time: std::sync::atomic::AtomicI64,
    /// True if this table has a hidden PK column added by MyRocks. Set by
    /// `Rdb_key_def::table_has_hidden_pk` during `Rdb_key_def::setup`.
    pub has_hidden_pk: bool,
}

impl TblDef {
    /// Drop-time cleanup. Removes each owned key-def from the ddl manager's
    /// `key_defs` cache. In Rust this is `Drop::drop` rather than an
    /// explicit destructor.
    /// C++: rdb_datadic.cc:3530.
    pub fn release(&mut self) {
        todo!("port C++ ~Rdb_tbl_def at rdb_datadic.cc:3530")
    }

    /// Persist the DDL entry into the system region.
    ///
    /// Writes:
    /// ```text
    /// key   = system_prefix || DDL_ENTRY_INDEX_VERSION_KEY || dbname.tablename
    /// value = u16 version || (u32 cf_id || u32 index_nr)*key_count
    /// ```
    ///
    /// Side-effect: ensures `cf_flags` rows exist for each referenced cf_id;
    /// fails if a cf_id is reused with incompatible flags.
    ///
    /// Errors: `slatedb::Error::invalid` for cf-flag conflict; `Unavailable`
    /// on I/O.
    /// C++: rdb_datadic.cc:3558.
    pub fn put_dict(
        &self,
        _dict: &DictManager,
        _batch: &mut WriteBatch,
        _key: &[u8],
    ) -> Result<(), Error> {
        todo!("port C++ put_dict at rdb_datadic.cc:3558")
    }

    /// Return the file creation time (lazily stat-ed from the .frm).
    /// Returns `0` if the .frm is gone or unreadable.
    /// C++: rdb_datadic.cc:3622.
    pub fn create_time_or_zero(&self) -> i64 {
        todo!("port C++ get_create_time at rdb_datadic.cc:3622")
    }

    /// Set `is_mysql_system_table` based on the dbname.
    /// C++: rdb_datadic.cc:3686.
    pub fn check_if_is_mysql_system_table(&mut self) {
        let system_dbs = ["mysql", "performance_schema", "information_schema"];
        self.is_mysql_system_table = system_dbs.iter().any(|d| *d == self.dbname);
    }

    /// Always sets `is_read_free_rpl_table = false`. The feature is a
    /// non-goal per _DESIGN.md §1.
    /// C++: rdb_datadic.cc:3702.
    pub fn check_and_set_read_free_rpl_table(&mut self) {
        self.is_read_free_rpl_table = false;
    }

    /// Parse `name` into (dbname, tablename, partition) using MariaDB's
    /// normalized name format.
    /// Errors: `slatedb::Error::invalid` if name is malformed.
    /// C++: rdb_datadic.cc:3711.
    pub fn set_name(&mut self, _name: &str) -> Result<(), Error> {
        todo!("port C++ set_name at rdb_datadic.cc:3711; call rdb_split_normalized_tablename")
    }

    /// Return the GL_INDEX_ID of the PK / hidden-PK index — used for
    /// auto-increment lookups.
    /// C++: rdb_datadic.cc:3722.
    pub fn get_autoincr_gl_index_id(&self) -> Option<GlIndexId> {
        todo!("port C++ get_autoincr_gl_index_id at rdb_datadic.cc:3722")
    }

    pub fn base_dbname(&self) -> &str { &self.dbname }
    pub fn base_tablename(&self) -> &str { &self.tablename }
    pub fn base_partition(&self) -> &str { &self.partition }
}
