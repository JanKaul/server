//! Interface stub for `rdb_sst_info_cc`.
//!
//! C++ source: `storage/rocksdb/rdb_sst_info.cc` (562 LoC)
//!
//! ## Mapping
//! Implementation of `Rdb_sst_info` (declared in `rdb_sst_info_h.rs`). In
//! MyRocks this builds SSTs directly via the RocksDB bulk-load API. Per
//! _DESIGN.md §1 ("SST bulk loader → Map degraded"), we route through a
//! high-volume `WriteBatch` via `Db::write` instead of writing SST files
//! directly. The performance is degraded but the semantics survive.
//!
//! Most public methods on `Rdb_sst_info` (declared in the header) become
//! `WriteBatch` accumulators here.
//!
//! ## Out-of-scope methods
//! None — bulk-load behaves as a batched write path. Direct SST file
//! emission is dropped (per _DESIGN.md §1).

use slatedb::bytes::Bytes;
use slatedb::Error;
use std::sync::Arc;

/// Per-table SST builder, replaced by a `WriteBatch` aggregator.
///
/// Original: rdb_sst_info.cc — `class Rdb_sst_info` (impl side).
pub struct SstInfoImpl {
    db: Arc<slatedb::Db>,
    batch: slatedb::WriteBatch,
    /// Threshold to auto-flush mid-build (bytes accumulated in batch).
    flush_threshold_bytes: usize,
    accumulated_bytes: usize,
}

impl SstInfoImpl {
    pub fn new(db: Arc<slatedb::Db>, flush_threshold_bytes: usize) -> Self {
        todo!("SlateDB WriteBatch ctor")
    }

    /// Append one (key, value) to the building "SST". Periodically flushes
    /// the underlying batch.
    pub async fn put(&mut self, key: Bytes, value: Bytes) -> Result<(), Error> {
        todo!("batch.put(key, value); accumulated += k.len + v.len; if over threshold: Db::write(batch); reset")
    }

    /// Commit any remaining batched entries.
    pub async fn finish(self) -> Result<(), Error> {
        todo!("Db::write(self.batch) if non-empty")
    }
}
