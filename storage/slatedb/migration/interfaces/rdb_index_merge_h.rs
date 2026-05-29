//! Interface stub for `rdb_index_merge_h`.
//!
//! C++ source: `storage/rocksdb/rdb_index_merge.h` (227 LoC)
//! C++ class: `Rdb_index_merge`
//!
//! ## Mapping
//! External merge-sort used during in-place index creation (`ALTER TABLE …
//! ADD INDEX`). Rows are emitted in arbitrary order during the index build,
//! buffered in memory, spilled to chunks on disk when the in-memory buffer
//! is full, then k-way merged back into sorted order for ingestion.
//!
//! Preserved as a Rust implementation per _DESIGN.md §1 (not mentioned by
//! name there, but it's a pre-ingest sorter independent of the storage
//! engine — same algorithm regardless of RocksDB vs SlateDB).
//!
//! Rust implementation notes:
//! - Buffer chunks: `Vec<(Bytes, Bytes)>` per chunk, spilled to a temp file
//!   via `std::io::BufWriter<std::fs::File>`. Format: length-prefixed
//!   key/value pairs, one record per write — same wire layout as MyRocks's
//!   `RDB_MERGE_REC_DELIMITER`.
//! - In-memory sort: `BinaryHeap` for the k-way merge phase, `slice::sort`
//!   for in-chunk sort. Both are stable and bytewise (no per-CF comparator
//!   needed — see `rdb_comparator_h`).
//! - The `rocksdb::ColumnFamilyHandle*` parameter from the C++ ctor maps to
//!   a `cf_id: u32` (the index's CF in our key-prefix scheme).
//!
//! ## Out-of-scope methods
//! None — this is an algorithm, fully in scope.

use bytes::Bytes;
use slatedb::Error;
use std::path::PathBuf;

/// One row in the merge buffer.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MergeRecord {
    pub key: Bytes,
    pub value: Bytes,
}

/// External merge-sort accumulator + iterator. Replaces C++ `Rdb_index_merge`.
///
/// Lifecycle:
/// 1. `new()` then `init()`.
/// 2. Call `add(key, val)` for each row produced by the index scan.
/// 3. `finish()` flushes the final in-memory chunk to disk and primes the
///    k-way merge heap.
/// 4. `next()` returns rows in sorted order until exhausted.
pub struct IndexMerge {
    /// Where on-disk merge chunks live. We honour MariaDB's `--tmpdir`.
    pub tmpfile_dir: PathBuf,
    /// In-memory sort buffer size, in bytes (sysvar
    /// `slatedb_merge_buf_size`).
    pub merge_buf_size: u64,
    /// Per-chunk re-read buffer size during merge.
    pub merge_combine_read_size: u64,
    /// Seconds to delay temp-file removal (debugging aid;
    /// `slatedb_merge_tmp_file_removal_delay_ms`).
    pub merge_tmp_file_removal_delay_ms: u64,
    /// CF id this builder belongs to (key-prefix part — _DESIGN.md §2).
    pub cf_id: u32,
}

impl IndexMerge {
    pub fn new(
        tmpfile_dir: PathBuf,
        merge_buf_size: u64,
        merge_combine_read_size: u64,
        merge_tmp_file_removal_delay_ms: u64,
        cf_id: u32,
    ) -> Self {
        Self {
            tmpfile_dir,
            merge_buf_size,
            merge_combine_read_size,
            merge_tmp_file_removal_delay_ms,
            cf_id,
        }
    }

    /// Allocate the in-memory buffer, open the temp file. C++
    /// `Rdb_index_merge::init()`.
    ///
    /// Errors: `slatedb::Error::Unavailable` on FS errors (out of disk,
    /// permission denied on `tmpfile_dir`, …).
    pub fn init(&mut self) -> Result<(), Error> {
        todo!("allocate Vec<MergeRecord> of capacity merge_buf_size, mkstemp() in tmpfile_dir")
    }

    /// Add a row. May trigger a spill to disk when the in-memory buffer fills.
    ///
    /// Errors: `Unavailable` on spill failure.
    pub fn add(&mut self, key: Bytes, value: Bytes) -> Result<(), Error> {
        let _ = (key, value);
        todo!("push to in-memory buffer; spill via merge_buf_write() when full")
    }

    /// Spill the in-memory buffer to disk as one sorted chunk. C++
    /// `merge_buf_write()`. Internal — callable by `add()` and `finish()`.
    pub fn merge_buf_write(&mut self) -> Result<(), Error> {
        todo!("sort in-memory buffer, write length-prefixed records to temp file")
    }

    /// No more rows. Prime the k-way merge heap. C++ `merge_heap_prepare()`
    /// invoked from `finish()` semantics.
    pub fn finish(&mut self) -> Result<(), Error> {
        todo!("flush last chunk; build BinaryHeap<MergeHeapEntry> across chunks")
    }

    /// Pop the next record from the merge heap.
    ///
    /// Returns `Ok(None)` once exhausted. Errors: `Unavailable` on chunk read
    /// failure.
    pub fn next(&mut self) -> Result<Option<MergeRecord>, Error> {
        todo!("BinaryHeap::pop, refill from the popped entry's chunk")
    }

    /// Discard all state — used when ALTER TABLE rolls back. C++
    /// `merge_reset()`.
    pub fn reset(&mut self) {
        todo!("clear in-memory buffer, close + unlink temp file (respecting removal_delay)")
    }

    pub fn get_cf_id(&self) -> u32 { self.cf_id }
}

impl Drop for IndexMerge {
    fn drop(&mut self) {
        // Best-effort temp file cleanup; ignore I/O errors at drop.
        self.reset();
    }
}
