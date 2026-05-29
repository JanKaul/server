//! Interface stub for `rdb_datadic_h__Rdb_seq_generator`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (class at line 1158)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_seq_generator`
//! parent: `rdb_datadic_h`
//!
//! ## Mapping
//! Monotonic id allocator. Used for assigning cf_id, index_id, table_id
//! when creating new entries. Backed by a system-CF counter that's
//! incremented atomically and persisted on overflow.
//!
//! Per _DESIGN.md §1, system metadata writes go through the system CF.
//! The generator caches a small batch of ids in-memory and reserves a new
//! batch when exhausted (reduces per-create roundtrips).
//!
//! ## Out-of-scope methods
//! None.

use slatedb::Error;
use std::sync::Arc;

/// Reserve-and-allocate id generator. One per id-kind (cf, index, table).
///
/// Original: rdb_datadic.h:1158 — `class Rdb_seq_generator`.
pub struct SeqGenerator {
    /// Next id to hand out (in-memory).
    next: std::sync::atomic::AtomicU64,
    /// Persisted upper-bound for the current reservation (inclusive).
    reserved_until: std::sync::atomic::AtomicU64,
    /// System-CF key prefix where the persisted counter lives.
    counter_key: Vec<u8>,
    /// Batch size to reserve at a time (e.g., 1024).
    batch_size: u64,
    db: Arc<slatedb::Db>,
}

impl SeqGenerator {
    pub fn new(db: Arc<slatedb::Db>, counter_key: Vec<u8>, batch_size: u64) -> Self {
        Self {
            next: std::sync::atomic::AtomicU64::new(0),
            reserved_until: std::sync::atomic::AtomicU64::new(0),
            counter_key,
            batch_size,
            db,
        }
    }

    /// Get the next id. Fast path: atomic increment of `next` if below
    /// `reserved_until`. Slow path: persist a new reservation, retry.
    pub async fn next_id(&self) -> Result<u64, Error> {
        todo!("CAS-based fast path; on overflow, lock + extend reservation via Db::put")
    }

    /// Initialize from the persisted counter at startup.
    pub async fn init(&self) -> Result<(), Error> {
        todo!("Db::get(counter_key); seed both next and reserved_until")
    }
}
