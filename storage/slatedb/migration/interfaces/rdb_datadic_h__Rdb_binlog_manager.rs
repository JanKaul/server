//! Interface stub for `rdb_datadic_h__Rdb_binlog_manager`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (class at line 1285)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_binlog_manager`
//! parent: `rdb_datadic_h`
//!
//! ## Mapping
//! Persists binlog position (file + offset + GTID) in the system CF on
//! each commit. MyRocks uses this for crash recovery to know which binlog
//! events have been durably applied.
//!
//! Per _DESIGN.md §1 (WAL: native), the binlog position is co-committed
//! with the user's write batch in a single `Db::write(batch)` — SlateDB's
//! atomic batch guarantees the position is durable iff the data is.
//!
//! ## Out-of-scope methods
//! None.

use bytes::Bytes;
use slatedb::Error;
use std::sync::Arc;

/// Binlog position record persisted on commit.
#[derive(Debug, Clone, Default)]
pub struct BinlogPosition {
    pub file_name: String,
    pub offset: u64,
    pub gtid: Option<String>,
}

/// Singleton; lives alongside `DictManager` and `DdlManager` in the engine.
///
/// Original: rdb_datadic.h:1285 — `class Rdb_binlog_manager`.
pub struct BinlogManager {
    db: Arc<slatedb::Db>,
}

impl BinlogManager {
    pub fn new(db: Arc<slatedb::Db>) -> Self {
        Self { db }
    }

    /// Persist a binlog position. Typically called within the user txn's
    /// `WriteBatch` so it commits atomically with the data.
    pub fn update_position(
        &self,
        batch: &mut slatedb::WriteBatch,
        pos: &BinlogPosition,
    ) -> Result<(), Error> {
        todo!("encode pos; batch.put(system_cf||binlog_pos_key, encoded)")
    }

    /// Read the last persisted position (called at startup for recovery).
    pub async fn read_position(&self) -> Result<Option<BinlogPosition>, Error> {
        todo!("Db::get(system_cf||binlog_pos_key); decode if present")
    }
}
