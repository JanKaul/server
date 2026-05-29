//! Interface stub for `Rdb_binlog_manager`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.cc` (lines 4520..4735)
//! C++ header: `storage/rocksdb/rdb_datadic.h` (Rdb_binlog_manager declarations)
//! v4 manifest sub-unit: `Rdb_binlog_manager`
//! Parent unit: `rdb_datadic.cc`
//! Approx body LoC accounted for here: ~215
//!
//! ## Mapping
//! Per _DESIGN.md §1 (system metadata via DictManager). The binlog manager
//! persists three bits of replication state into the SYSTEM region:
//!
//! - **Master position** (`binlog_name`, `binlog_pos`, `binlog_gtid`) —
//!   written from every committed transaction so the server can recover
//!   `SHOW MASTER STATUS` after a crash. Maps to one fixed system key.
//! - **Slave GTID state** — `mysql.gtid_slave_pos`-equivalent rows. One
//!   system key per slave-server-uuid.
//!
//! All ops go through `DictManager::put_key` / `get_value` on a dedicated
//! `BINLOG_INFO_INDEX` key-type prefix.
//!
//! ## Out-of-scope methods
//! None as a hard non-goal, but **2PC integration** (per _DESIGN.md §1
//! "Two-phase commit" → Re-impl thin) means `update()` is called from
//! the XA-prepare path, not the autocommit path. The thin 2PC layer that
//! drives this is in `engine/txn.rs` (TRANSLATE phase).

use bytes::Bytes;
use slatedb::Error;
use std::sync::Arc;

use crate::Rdb_dict_manager::{DictManager, WriteBatch};

/// `Rdb_binlog_manager` — owns the replication-position system keys.
pub struct BinlogManager {
    pub dict: Arc<DictManager>,
    /// Pre-computed system key for the master-status entry. Same key for
    /// the lifetime of the server; written-over on every commit.
    pub key_master_status: Bytes,
}

impl BinlogManager {
    /// Bootstrap: builds `key_master_status` from the BINLOG_INFO_INDEX
    /// key-type. Does not read any value.
    /// C++: rdb_datadic.cc:4520.
    pub fn init(_dict: Arc<DictManager>) -> Result<Self, Error> {
        todo!("port C++ init at rdb_datadic.cc:4520")
    }

    pub fn cleanup(&self) { /* no-op — same as C++ rdb_datadic.cc:4530 */ }

    /// Stage an update to the master-status entry into `batch`. The actual
    /// fsync happens at `batch.commit()` (per _DESIGN.md §0 `flush_with_options(Wal)`
    /// when `WriteOptions::await_durable = true`).
    ///
    /// Inputs:
    /// - `binlog_name`: e.g. "mysql-bin.000123"
    /// - `binlog_pos`: byte offset within the file
    /// - `binlog_gtid`: optional GTID string
    ///
    /// C++: rdb_datadic.cc:4542.
    pub fn update(
        &self,
        _batch: &mut WriteBatch,
        _binlog_name: &str,
        _binlog_pos: u64,
        _binlog_gtid: Option<&str>,
    ) {
        todo!("port C++ update at rdb_datadic.cc:4542")
    }

    /// Read the master-status entry. Returns `Ok(None)` if never written.
    /// C++: rdb_datadic.cc:4590.
    pub async fn read(&self) -> Result<Option<MasterStatus>, Error> {
        todo!("port C++ read at rdb_datadic.cc:4590")
    }

    /// Decode the on-disk master-status TLV. Pure parsing — extracted as a
    /// helper because the encoded format is shared with `read()`.
    /// C++: rdb_datadic.cc:4615.
    pub fn unpack_value(_value: &[u8]) -> Result<MasterStatus, Error> {
        todo!("port C++ unpack_value at rdb_datadic.cc:4615")
    }

    /// Update the per-slave GTID rows. Called from `Rotate_log_event` and
    /// `Gtid_log_event` apply paths. Each entry is one system key keyed by
    /// `(SLAVE_GTID_INFO_INDEX, server_id, source_uuid)`.
    /// C++: rdb_datadic.cc:4688.
    pub fn update_slave_gtid_info(
        &self,
        _batch: &mut WriteBatch,
        _id: u32,
        _db: &str,
        _gtid: &str,
    ) {
        todo!("port C++ update_slave_gtid_info at rdb_datadic.cc:4688")
    }
}

/// Decoded master-status row.
#[derive(Debug, Clone, Default)]
pub struct MasterStatus {
    pub binlog_name: String,
    pub binlog_pos: u64,
    pub binlog_gtid: Option<String>,
}
