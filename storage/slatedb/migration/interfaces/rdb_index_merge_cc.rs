//! Interface stub for `rdb_index_merge_cc`.
//!
//! C++ source: `storage/rocksdb/rdb_index_merge.cc` (630 LoC)
//! C++ class:  `Rdb_index_merge` (impl)
//!
//! ## Mapping
//! External merge sort used during inplace CREATE INDEX. Per _DESIGN.md §1
//! ("SST bulk loader" row — Map degraded): we don't have a direct
//! SST-writer API to stream pre-sorted data into SlateDB. We use the same
//! external-merge-sort + write-batch path:
//!
//! 1. Sort chunks fit in `merge_buf_size` go directly into a `WriteBatch`.
//! 2. Larger sorts spill to disk (same algorithm as MyRocks — n-way merge
//!    of sorted run files in a tmpfile) and stream the merged output into
//!    `WriteBatch` flushes.
//!
//! The disk file format is preserved bit-for-bit so MyRocks tooling and
//! tests apply unchanged. The CF handle parameter becomes a `CfDescriptor`
//! (see `rdb_cf_manager_cc::CfDescriptor`) used only to drive the
//! comparator on the merge-heap (which is `Bytes::cmp` — SlateDB sorts
//! bytewise, see _DESIGN.md §2).
//!
//! ## Out-of-scope methods
//! - `merge_record_compare(Comparator*)` — we use plain bytewise `cmp`.
//! - All `rocksdb::Slice` parameters — replaced by `&[u8]` or `Bytes`.
//! - `File` (MariaDB filehandle) — replaced by `std::fs::File` / tokio
//!   async file under TRANSLATE.

use crate::rdb_cf_manager_cc::CfDescriptor;
use slatedb::bytes::Bytes;
use slatedb::Error;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// On-disk merge file constants. Names match C++ `RDB_MERGE_*` for
/// cross-reference. Per the doc, the file format is preserved bit-for-bit.
pub const MERGE_CHUNK_LEN: usize = 8;
pub const MERGE_REC_DELIMITER: usize = 8;
pub const MERGE_KEY_DELIMITER: usize = 8;
pub const MERGE_VAL_DELIMITER: usize = 8;

/// Sort-buffer accounting struct. Tracks one unsorted-or-sorted in-memory
/// block. Disk-side spilling reuses the same layout.
///
/// Original: rdb_index_merge.h — `struct merge_buf_info`.
pub struct MergeBufInfo {
    pub block: Vec<u8>,
    pub total_size: u64,
    pub curr_offset: u64,
    pub disk_start_offset: u64,
    pub disk_curr_offset: u64,
}

/// One open run on disk, plus its current chunk buffer in memory.
///
/// Original: rdb_index_merge.h — `struct merge_heap_entry`.
pub struct MergeHeapEntry {
    pub chunk_info: MergeBufInfo,
    pub key: Bytes,
    pub val: Bytes,
}

/// External merge sort context. Owned by the bulk-load handler for the
/// duration of one CREATE INDEX statement.
///
/// Original: rdb_index_merge.cc:31 — `Rdb_index_merge::Rdb_index_merge`.
pub struct IndexMerge {
    pub tmpfile_path: Option<PathBuf>,
    pub merge_buf_size: u64,
    pub merge_combine_read_size: u64,
    pub merge_tmp_file_removal_delay: u64,
    pub cf: CfDescriptor,
    // Internal state — populated during merge run.
    pub offset_tree: BTreeSet<Bytes>,
    pub num_sort_buffers: u32,
}

impl IndexMerge {
    /// Construct with sysvar-derived sizing.
    /// Original: rdb_index_merge.cc:31 — ctor.
    pub fn new(
        tmpfile_path: Option<PathBuf>,
        merge_buf_size: u64,
        merge_combine_read_size: u64,
        merge_tmp_file_removal_delay: u64,
        cf: CfDescriptor,
    ) -> Self {
        Self {
            tmpfile_path,
            merge_buf_size,
            merge_combine_read_size,
            merge_tmp_file_removal_delay,
            cf,
            offset_tree: BTreeSet::new(),
            num_sort_buffers: 0,
        }
    }

    /// Allocate buffers, create the spill tmpfile.
    ///
    /// Inputs: none.
    /// Output: `Ok(())` on success.
    /// Errors:
    /// - `Unavailable` if tmpfile creation fails (disk full / permissions).
    /// - `Invalid` if `merge_buf_size == 0`.
    ///
    /// Original: rdb_index_merge.cc:76 — `init`.
    pub fn init(&mut self) -> Result<(), Error> {
        todo!("mysql_tmpfile in self.tmpfile_path; allocate rec_buf_unsorted + output_buf")
    }

    /// Add one (key, value) pair. If the unsorted buffer would overflow,
    /// sorts it and spills to disk first.
    ///
    /// Inputs: `key`, `val` — both encoded by the codec already.
    /// Output: `Ok(())`.
    /// Errors:
    /// - `Invalid` if key+value exceeds `merge_buf_size` (caller bug —
    ///   buffer too small for a single record).
    /// - `Data` if a duplicate-unique-key is detected.
    /// - `Unavailable` on tmpfile I/O failure.
    ///
    /// Original: rdb_index_merge.cc:139 — `add`.
    pub fn add(&mut self, key: &[u8], val: &[u8]) -> Result<(), Error> {
        let _ = (key, val);
        todo!("offset_tree.insert(key); if total >= buf_size, merge_buf_write")
    }

    /// Sort current buffer and spill to disk.
    /// Original: rdb_index_merge.cc:196 — `merge_buf_write`.
    pub fn merge_buf_write(&mut self) -> Result<(), Error> {
        todo!("write chunk header, iterate offset_tree in order, write to tmpfile, sync")
    }

    /// Prepare the n-way merge heap by reading the first record of each run.
    /// Called by `next()` on first invocation.
    /// Original: rdb_index_merge.cc:265 — `merge_heap_prepare`.
    pub fn merge_heap_prepare(&mut self) -> Result<(), Error> {
        todo!("for each sort buffer, mmap or pread chunk, push initial entry on min-heap")
    }

    /// Pop the next sorted (key, value) from the merged stream.
    ///
    /// Inputs: none.
    /// Output:
    /// - `Some((key, value))` for each merged record, in ascending key order.
    /// - `None` when the stream is exhausted.
    ///
    /// Errors: I/O failure during heap refill, `Unavailable`.
    ///
    /// Original: rdb_index_merge.cc:325 — `next`.
    pub fn next(&mut self) -> Result<Option<(Bytes, Bytes)>, Error> {
        todo!("heap.pop, refill from that run, return top key/value")
    }
}

impl Drop for IndexMerge {
    fn drop(&mut self) {
        // Replaces the C++ destructor's `my_close` + optional trim-stall
        // mitigation. The tokio runtime owns the tmpfile cleanup once we
        // open via `tokio::fs::File`; nothing to do here in the stub.
        // TODO(human): if `merge_tmp_file_removal_delay > 0`, do the
        // staggered-truncate dance from rdb_index_merge.cc:44.
    }
}
