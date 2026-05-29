//! Interface stub for `rdb_sst_info_h`.
//!
//! C++ source: `storage/rocksdb/rdb_sst_info.h` (265 LoC)
//! C++ classes: `Rdb_sst_file`, `Rdb_sst_file_ordered`, `Rdb_sst_info`,
//!              `Rdb_sst_info::Rdb_sst_commit_info`
//!
//! ## Mapping
//! MyRocks' SST bulk-load path: build sorted SST files outside the LSM, then
//! `IngestExternalFile` them. The classes here wrap RocksDB's
//! `SstFileWriter`, keep a stack to enforce key-order, and bundle a list of
//! committed files into one atomic ingest.
//!
//! Per _DESIGN.md §1 ("SST bulk loader → Map (degraded)") the SlateDB
//! analogue is **`WriteBatch` with a high `flush_interval`** — there is no
//! external-SST-then-ingest path. So this header collapses to two pieces:
//!
//! 1. **`SstInfo`** — accepts the same `put(key, val)` / `finish()` /
//!    `commit()` API but routes into a SlateDB `WriteBatch` plus a final
//!    `Db::write(batch)`. The `key/val` order constraint is dropped (SlateDB
//!    sorts internally), but we preserve `Rdb_sst_stack`-style accumulation
//!    so call sites don't have to know.
//!
//! 2. **`SstFileMetadata` / `VersionedManifest` shims** — for I_S exposure
//!    of "what's in the LSM right now". SlateDB exposes this natively via
//!    `DbMetadataOps`; we expose it through Rust types under the same names
//!    the I_S layer already uses.
//!
//! The `Rdb_sst_file_ordered` ordering buffer + the `print_client_error` path
//! and the per-file commit_mutex disappear in this model — single
//! `Db::write(batch)` is atomic.
//!
//! ## Out-of-scope methods
//! - `Rdb_sst_file::open()` / `Rdb_sst_file::commit()` — RocksDB
//!   `SstFileWriter` is RocksDB-specific. SlateDB writes go through
//!   `Db::write(batch)`.
//! - `report_error_msg(rocksdb::Status, ...)` — RocksDB error type doesn't
//!   exist in our world; we use `slatedb::Error`.
//! - `init(rocksdb::DB*)` — static init that grabs RocksDB's DB options.
//!   Not needed (SlateDB options live on our engine handle).

use bytes::Bytes;
use slatedb::Error;
use std::sync::Arc;

// --- SlateDB-side "SST" metadata, surfaced to I_S ---

/// Per-SST metadata. Maps to SlateDB `SstFileMetadata` from
/// `VersionedManifest` (see _DESIGN.md §0 "DbMetadataOps").
#[derive(Debug, Clone)]
pub struct SstFileMetadata {
    /// Path within the object store.
    pub path: String,
    pub size_bytes: u64,
    pub smallest_key: Bytes,
    pub largest_key: Bytes,
    pub num_entries: u64,
    pub num_tombstones: u64,
    pub level: u8,
}

/// Snapshot of the current manifest's SST set. Wraps slatedb's
/// `VersionedManifest`. Used by `information_schema.rocksdb_sst_props`.
#[derive(Debug, Clone, Default)]
pub struct VersionedManifestSnapshot {
    pub ssts: Vec<SstFileMetadata>,
    /// Manifest version (monotonic).
    pub version: u64,
}

impl VersionedManifestSnapshot {
    /// Pull the latest manifest from the engine handle. Errors:
    /// `Unavailable` if the object store is unreachable; `Closed` if the DB
    /// is shut down.
    pub async fn fetch(_db: &Arc<slatedb::Db>) -> Result<Self, Error> {
        todo!("call db.metadata().manifest_snapshot() and map to SstFileMetadata")
    }
}

// --- bulk-load accumulator ---

/// One unit of work returned from `SstInfo::finish` — the batch of writes to
/// be applied atomically. Replaces C++ `Rdb_sst_commit_info`. Holds a
/// SlateDB `WriteBatch`; the calling layer hands it to `Db::write(batch)`.
pub struct SstCommitInfo {
    /// CF id of the index being loaded (key-prefix component).
    pub cf_id: u32,
    /// The accumulated WriteBatch. `None` after `commit()` returns.
    pub batch: Option<slatedb::WriteBatch>,
}

impl SstCommitInfo {
    pub fn has_work(&self) -> bool { self.batch.is_some() }

    /// Mark the commit as done — drops the batch so reset() doesn't try to
    /// roll back. C++ `Rdb_sst_commit_info::commit()`.
    pub fn commit(&mut self) { self.batch = None; }

    /// Roll back unfinalized state. Idempotent.
    pub fn reset(&mut self) { self.batch = None; }

    pub fn get_cf_id(&self) -> u32 { self.cf_id }
}

impl Drop for SstCommitInfo {
    fn drop(&mut self) { self.reset(); }
}

/// SlateDB-flavored replacement for `Rdb_sst_info`. Accepts a stream of
/// `(key, value)` writes from the bulk loader and accumulates them into a
/// `WriteBatch` for atomic ingest.
///
/// Key ordering: SlateDB sorts internally, so we do NOT need the
/// `Rdb_sst_file_ordered` re-sort logic — accepted in any order.
pub struct SstInfo {
    cf_id: u32,
    table_name: String,
    index_name: String,
    /// Total bytes pending in `batch`. When this exceeds `max_size`,
    /// `put()` triggers a flush via the engine handle. Bounded so we don't
    /// blow RAM on huge `LOAD DATA INFILE`.
    curr_size: u64,
    max_size: u64,
    /// Pending writes. Single in-flight batch (MyRocks supported multiple
    /// committed files — irrelevant in our model).
    batch: slatedb::WriteBatch,
    /// First non-success status observed during background work, if any.
    background_error: std::sync::atomic::AtomicI32,
    done: bool,
    tracing: bool,
}

impl SstInfo {
    pub fn new(
        cf_id: u32,
        table_name: String,
        index_name: String,
        max_size: u64,
        tracing: bool,
    ) -> Self {
        Self {
            cf_id,
            table_name,
            index_name,
            curr_size: 0,
            max_size,
            batch: slatedb::WriteBatch::new(),
            background_error: std::sync::atomic::AtomicI32::new(0),
            done: false,
            tracing,
        }
    }

    /// Add a row to the bulk-load batch.
    ///
    /// Errors: `Internal` if `finish()` already ran; `Unavailable` if an
    /// auto-flush failed.
    pub fn put(&mut self, key: Bytes, value: Bytes) -> Result<(), Error> {
        if self.done {
            return Err(Error::internal("SstInfo::put after finish()".into()));
        }
        self.curr_size = self.curr_size.saturating_add((key.len() + value.len()) as u64);
        // `WriteBatch::put` is fallible only on internal capacity overflow.
        todo!("self.batch.put(key, value)?; if curr_size > max_size, hand off + reset")
    }

    /// Finalize: hand the accumulated batch to the caller for an atomic
    /// `Db::write(batch)`. C++ `Rdb_sst_info::finish`.
    pub fn finish(
        &mut self,
        commit_info: &mut SstCommitInfo,
        _print_client_error: bool,
    ) -> Result<(), Error> {
        if self.done {
            return Err(Error::internal("SstInfo::finish double-call".into()));
        }
        self.done = true;
        // Move batch out via swap with a fresh empty one.
        let batch = std::mem::replace(&mut self.batch, slatedb::WriteBatch::new());
        commit_info.cf_id = self.cf_id;
        commit_info.batch = Some(batch);
        Ok(())
    }

    pub fn is_done(&self) -> bool { self.done }

    pub fn have_background_error(&self) -> bool {
        self.background_error.load(std::sync::atomic::Ordering::Relaxed) != 0
    }

    /// Atomically swap the stored background-error code to 0, returning the
    /// old value. C++ `get_and_reset_background_error`.
    pub fn get_and_reset_background_error(&self) -> i32 {
        self.background_error.swap(0, std::sync::atomic::Ordering::AcqRel)
    }

    /// Latch a non-success code if none was set yet. Idempotent for >1 calls.
    pub fn set_background_error(&self, code: i32) {
        let _ = self.background_error.compare_exchange(
            0, code,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    pub fn cf_id(&self) -> u32 { self.cf_id }
    pub fn table_name(&self) -> &str { &self.table_name }
    pub fn index_name(&self) -> &str { &self.index_name }
    pub fn tracing(&self) -> bool { self.tracing }
}
