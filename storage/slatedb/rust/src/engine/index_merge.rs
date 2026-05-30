//! External merge-sort for inplace `CREATE INDEX` (`ALTER TABLE … ADD INDEX`).
//!
//! Translated from `storage/rocksdb/rdb_index_merge.{h,cc}` (227 + 630 LoC).
//!
//! ## Why we keep this algorithm
//!
//! Per `_DESIGN.md §1`, SlateDB has no equivalent of RocksDB's
//! `SstFileWriter`-streaming-ingest API. Inplace index build still needs to
//! deliver the SK rows in sorted order, so the C++ external-merge-sort path
//! ports over as-is — sort in memory until the buffer fills, spill chunks
//! to disk, then n-way merge back out. The output stream is fed into a
//! SlateDB `WriteBatch` by the caller (handler-bucket level).
//!
//! ## On-disk format (matches C++ byte-for-byte on the same target)
//!
//! Each chunk occupies exactly `merge_buf_size` bytes on disk (chunk N
//! starts at `N * merge_buf_size`; trailing bytes within a slot are unused
//! padding). A chunk is:
//!
//! ```text
//!   chunk_total_size  : u64 (native-endian)   — total bytes of chunk header + records
//!   record* {
//!     key_len         : u64 (native-endian)
//!     key             : [u8; key_len]
//!     val_len         : u64 (native-endian)
//!     val             : [u8; val_len]
//!   }
//! ```
//!
//! Native-endian u64 matches the C++ `memcpy(dst, &n, sizeof(n))`. On the
//! LE targets MyRocks supports (x86_64, aarch64), this is little-endian.
//!
//! ## Sort order
//!
//! Bytewise lexicographic (`std::cmp::Ord` on `&[u8]`). SlateDB stores
//! bytewise per `_DESIGN.md §2`, so the merged output is in the order
//! SlateDB will store it. Reverse-CF semantics are handled elsewhere (by
//! flipping key bounds in [`crate::codec::key`], not here).
//!
//! ## Sizing constraints
//!
//! `merge_buf_size` must hold at least one full record (8-byte chunk
//! header + 8 + key + 8 + val); enforced in `add`. During merge,
//! `merge_combine_read_size / num_sort_buffers` (the per-chunk read
//! window) must also hold the largest record — otherwise the merge phase
//! returns `Unavailable("merge window too small for record")`. MyRocks'
//! defaults are several MB so this is not a runtime concern, but the
//! check exists rather than infinite-looping on a malformed input.
//!
//! ## Differences from the C++
//!
//! - In-memory unsorted records live in a `BTreeMap<Bytes, Bytes>` instead
//!   of a `std::set<merge_record>` keyed by offsets into a single packed
//!   buffer. The pre-spill threshold uses the same accounting formula as
//!   the C++ so the spill cadence matches.
//! - `merge_tmp_file_removal_delay_ms` is accepted (parameter compat) but
//!   not implemented — the flash-trim-stall mitigation was specific to
//!   bare-metal SSD MyRocks deployments and doesn't apply to S3-backed
//!   SlateDB.
//! - The tmpfile is removed on `Drop` rather than at open time. The
//!   POSIX-anonymous-unlink trick was traded for cross-platform behaviour
//!   and easier debugging.

use bytes::Bytes;
use slatedb::Error;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Length of the per-chunk size header. Matches C++ `RDB_MERGE_CHUNK_LEN`.
pub const MERGE_CHUNK_LEN: u64 = 8;
/// Length of the per-record key/val length prefix. Matches C++
/// `RDB_MERGE_REC_DELIMITER` / `_KEY_DELIMITER` / `_VAL_DELIMITER` (all
/// `sizeof(size_t)` in the C++).
pub const MERGE_REC_DELIMITER: u64 = 8;

/// Errors that callers want to distinguish from generic I/O failure.
///
/// `IndexMerge::add` returns this rather than `slatedb::Error` so that the
/// ALTER-TABLE handler can map duplicate keys to `ER_DUP_ENTRY` (matching
/// MyRocks' behaviour in `rdb_index_merge.cc:184`) without parsing
/// `Error::invalid` message strings.
#[derive(Debug)]
pub enum IndexMergeError {
    /// Duplicate key seen during a unique-index build. Maps to
    /// `ER_DUP_ENTRY` at the handler boundary.
    DuplicateKey,
    /// A single (key, val) record is larger than `merge_buf_size`. Caller
    /// configuration bug — bump `slatedb_merge_buf_size`.
    BufferTooSmall,
    /// Underlying I/O / state failure.
    Io(Error),
}

impl IndexMergeError {
    /// Lossy collapse to `slatedb::Error` for callers that don't care to
    /// distinguish dup-key (e.g. non-unique index builds).
    pub fn into_slatedb(self) -> Error {
        match self {
            IndexMergeError::DuplicateKey => {
                Error::invalid("duplicate key in index merge".into())
            }
            IndexMergeError::BufferTooSmall => Error::invalid(
                "sort buffer size is too small to process merge".into(),
            ),
            IndexMergeError::Io(e) => e,
        }
    }
}

impl From<Error> for IndexMergeError {
    fn from(e: Error) -> Self {
        IndexMergeError::Io(e)
    }
}

/// External merge-sort accumulator + drainer. Replaces C++ `Rdb_index_merge`.
///
/// Lifecycle:
/// 1. `IndexMerge::new(...)` then [`IndexMerge::init`].
/// 2. [`IndexMerge::add`] per input record (in any order).
/// 3. [`IndexMerge::next`] until it returns `Ok(None)`; records come out
///    in ascending bytewise key order.
///
/// The tmpfile is removed by `Drop`.
pub struct IndexMerge {
    tmpfile_dir: PathBuf,
    merge_buf_size: u64,
    merge_combine_read_size: u64,
    #[allow(dead_code)] // accepted for parameter compat; see module docs
    merge_tmp_file_removal_delay_ms: u64,
    cf_id: u32,

    /// Tmpfile, opened by `init()`. `None` before init or after a fatal
    /// error that closed it.
    file: Option<File>,
    /// Path retained so `Drop` can `remove_file` it.
    tmpfile_path: Option<PathBuf>,
    /// Number of sorted chunks already spilled to disk.
    num_sort_buffers: u64,

    /// Sorted in-memory records waiting to either be spilled (when the
    /// projected serialized size hits `merge_buf_size`) or drained
    /// directly from `next()` if no spill ever happens.
    offset_tree: BTreeMap<Bytes, Bytes>,
    /// Sum of `8 + key.len + 8 + val.len` over `offset_tree`. The C++
    /// `m_rec_buf_unsorted->m_curr_offset` equivalent for the spill
    /// threshold.
    unsorted_size: u64,

    /// Min-heap (by `current_key`) over the chunks during merge phase.
    /// Populated lazily on the first `next()` after at least one spill.
    merge_heap: BinaryHeap<HeapEntry>,
}

impl IndexMerge {
    /// Construct. `tmpfile_dir` is honoured for spill files (mirrors the
    /// C++ `--tmpdir` parameter).
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
            file: None,
            tmpfile_path: None,
            num_sort_buffers: 0,
            offset_tree: BTreeMap::new(),
            unsorted_size: 0,
            merge_heap: BinaryHeap::new(),
        }
    }

    /// Open the tmpfile. Allocations for the in-memory buffer are
    /// implicit (`BTreeMap` grows as records arrive — the spill threshold
    /// caps total growth at `merge_buf_size`).
    ///
    /// C++ counterpart: `Rdb_index_merge::init()` /
    /// `Rdb_index_merge::merge_file_create()`.
    pub fn init(&mut self) -> Result<(), Error> {
        if self.merge_buf_size <= MERGE_CHUNK_LEN + 2 * MERGE_REC_DELIMITER {
            return Err(Error::invalid(
                "merge_buf_size must exceed chunk + record headers".into(),
            ));
        }
        let (file, path) = open_tmpfile(&self.tmpfile_dir)?;
        self.file = Some(file);
        self.tmpfile_path = Some(path);
        self.num_sort_buffers = 0;
        Ok(())
    }

    /// Add a (key, value) pair. Returns `DuplicateKey` if `key` is already
    /// in the current in-memory chunk (matches `std::set::emplace`
    /// rejection in the C++).
    ///
    /// Note: this only detects dups *within a single in-memory chunk*. The
    /// C++ has the same limitation: once a chunk is spilled, a future dup
    /// is invisible to `add()`. The caller checks for global uniqueness
    /// by scanning the merged output.
    pub fn add(&mut self, key: &[u8], val: &[u8]) -> Result<(), IndexMergeError> {
        debug_assert!(
            self.merge_heap.is_empty(),
            "add() called after merge phase started",
        );

        // Projected disk footprint if we accept this record into the
        // current chunk. Matches the C++ formula in
        // `rdb_index_merge.cc:147`.
        let projected = MERGE_CHUNK_LEN
            + self.unsorted_size
            + 2 * MERGE_REC_DELIMITER
            + key.len() as u64
            + val.len() as u64;

        if projected >= self.merge_buf_size {
            if self.offset_tree.is_empty() {
                // The new record by itself doesn't fit — buffer too small.
                return Err(IndexMergeError::BufferTooSmall);
            }
            self.merge_buf_write()?;
        }

        use std::collections::btree_map::Entry;
        match self.offset_tree.entry(Bytes::copy_from_slice(key)) {
            Entry::Occupied(_) => Err(IndexMergeError::DuplicateKey),
            Entry::Vacant(slot) => {
                let added = 2 * MERGE_REC_DELIMITER + key.len() as u64 + val.len() as u64;
                self.unsorted_size += added;
                slot.insert(Bytes::copy_from_slice(val));
                Ok(())
            }
        }
    }

    /// Pop the next record in sorted order. Returns `Ok(None)` once the
    /// stream is exhausted.
    ///
    /// C++ counterpart: `Rdb_index_merge::next()`. The name matches the
    /// C++ for grep-ability; `Iterator` doesn't fit because we return
    /// `Result<Option<…>>` rather than `Option<Result<…>>`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<(Bytes, Bytes)>, Error> {
        // In-memory-only fast path: nothing was ever spilled, drain the
        // BTreeMap directly. Matches C++ `num_sort_buffers == 0` branch.
        if self.num_sort_buffers == 0 {
            return Ok(self.offset_tree.pop_first().map(|(k, v)| {
                let consumed =
                    2 * MERGE_REC_DELIMITER + k.len() as u64 + v.len() as u64;
                self.unsorted_size = self.unsorted_size.saturating_sub(consumed);
                (k, v)
            }));
        }

        // First call after at least one spill: prime the heap and return
        // the smallest element *without* popping (so the next call sees
        // the same top and knows to advance from it).
        if self.merge_heap.is_empty() {
            self.merge_heap_prepare()?;
            return Ok(self
                .merge_heap
                .peek()
                .map(|e| (e.current_key.clone(), e.current_val.clone())));
        }

        self.heap_pop_and_get_next()
    }

    /// Spill the current in-memory chunk to disk in sorted order.
    ///
    /// C++ counterpart: `Rdb_index_merge::merge_buf_write()`.
    fn merge_buf_write(&mut self) -> Result<(), Error> {
        debug_assert!(!self.offset_tree.is_empty());
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| Error::invalid("merge file not opened".into()))?;

        let chunk_payload_size = MERGE_CHUNK_LEN + self.unsorted_size;
        debug_assert!(chunk_payload_size <= self.merge_buf_size);

        // Build the chunk in a buffer sized to the on-disk slot so we
        // write `merge_buf_size` bytes per chunk (zero-padded tail). Each
        // chunk occupies a whole slot at `i * merge_buf_size` — matches
        // the C++ disk layout and keeps `merge_heap_prepare` arithmetic
        // simple.
        let mut buf = vec![0u8; self.merge_buf_size as usize];
        let mut off = 0usize;

        buf[off..off + 8].copy_from_slice(&chunk_payload_size.to_ne_bytes());
        off += 8;

        for (k, v) in &self.offset_tree {
            let klen = k.len() as u64;
            buf[off..off + 8].copy_from_slice(&klen.to_ne_bytes());
            off += 8;
            buf[off..off + k.len()].copy_from_slice(k);
            off += k.len();
            let vlen = v.len() as u64;
            buf[off..off + 8].copy_from_slice(&vlen.to_ne_bytes());
            off += 8;
            buf[off..off + v.len()].copy_from_slice(v);
            off += v.len();
        }
        debug_assert_eq!(off as u64, chunk_payload_size);

        let disk_offset = self.num_sort_buffers * self.merge_buf_size;
        file.seek(SeekFrom::Start(disk_offset))
            .map_err(|e| Error::unavailable(format!("seek merge file: {e}")))?;
        file.write_all(&buf)
            .map_err(|e| Error::unavailable(format!("write merge chunk: {e}")))?;
        file.sync_data()
            .map_err(|e| Error::unavailable(format!("fsync merge file: {e}")))?;

        self.num_sort_buffers += 1;
        self.offset_tree.clear();
        self.unsorted_size = 0;
        Ok(())
    }

    /// Prime the n-way merge heap by opening a read window over each
    /// chunk on disk and pulling the first record from each.
    ///
    /// C++ counterpart: `Rdb_index_merge::merge_heap_prepare()`.
    fn merge_heap_prepare(&mut self) -> Result<(), Error> {
        debug_assert!(self.merge_heap.is_empty());

        // Pending in-memory records become the last chunk.
        if !self.offset_tree.is_empty() {
            self.merge_buf_write()?;
        }
        debug_assert!(self.num_sort_buffers > 0);

        // Per-chunk read window size. Same formula as the C++.
        let mut window = self.merge_combine_read_size / self.num_sort_buffers;
        if window >= self.merge_buf_size {
            window = self.merge_buf_size;
        }
        if window < MERGE_CHUNK_LEN {
            return Err(Error::invalid(
                "merge window smaller than chunk header".into(),
            ));
        }

        let file = self
            .file
            .as_mut()
            .ok_or_else(|| Error::invalid("merge file not opened".into()))?;

        for i in 0..self.num_sort_buffers {
            let disk_start = i * self.merge_buf_size;
            let mut chunk_buf = vec![0u8; window as usize];

            file.seek(SeekFrom::Start(disk_start))
                .map_err(|e| Error::unavailable(format!("seek merge chunk: {e}")))?;
            let read_n = read_filled(file, &mut chunk_buf)?;
            if read_n < MERGE_CHUNK_LEN as usize {
                return Err(Error::unavailable(
                    "short read on merge chunk header".into(),
                ));
            }

            let mut hdr = [0u8; 8];
            hdr.copy_from_slice(&chunk_buf[..8]);
            let total_size = u64::from_ne_bytes(hdr);

            // Empty chunk (ALTER on empty table). The C++ `break`s here;
            // we `continue` instead — there's no semantic difference
            // since an empty chunk only happens at the tail of `add` /
            // `merge_heap_prepare` flow when offset_tree was empty at
            // flush time, but `continue` is safer if the assumption ever
            // breaks.
            if total_size == MERGE_CHUNK_LEN {
                continue;
            }

            let mut entry = HeapEntry {
                chunk_buf,
                chunk_valid: read_n,
                curr_offset_in_buf: MERGE_CHUNK_LEN as usize,
                disk_curr: disk_start,
                disk_start,
                total_size,
                current_key: Bytes::new(),
                current_val: Bytes::new(),
            };

            let (k, v) = pull_record(&mut entry, file)?.ok_or_else(|| {
                Error::unavailable("chunk header says non-empty but no record".into())
            })?;
            entry.current_key = k;
            entry.current_val = v;
            self.merge_heap.push(entry);
        }

        Ok(())
    }

    /// Pop the heap top, advance its chunk by one record, push it back if
    /// not exhausted, and return the new top.
    ///
    /// C++ counterpart: `Rdb_index_merge::merge_heap_pop_and_get_next()`.
    fn heap_pop_and_get_next(&mut self) -> Result<Option<(Bytes, Bytes)>, Error> {
        let mut entry = self
            .merge_heap
            .pop()
            .ok_or_else(|| Error::invalid("heap unexpectedly empty".into()))?;

        let file = self
            .file
            .as_mut()
            .ok_or_else(|| Error::invalid("merge file not opened".into()))?;

        if let Some((k, v)) = pull_record(&mut entry, file)? {
            entry.current_key = k;
            entry.current_val = v;
            self.merge_heap.push(entry);
        }
        // else: chunk exhausted — don't push it back.

        Ok(self
            .merge_heap
            .peek()
            .map(|e| (e.current_key.clone(), e.current_val.clone())))
    }

    /// The CF id this builder feeds — opaque label for the caller's
    /// downstream `WriteBatch`. SlateDB itself has no CFs today (see
    /// `[[no-cargo-cult-abstractions]]`), but the field is preserved so
    /// the call-site shape doesn't need rewriting if CFs land later.
    pub fn cf_id(&self) -> u32 {
        self.cf_id
    }

    /// Number of chunks already spilled. Exposed for tests.
    #[cfg(test)]
    pub(crate) fn num_sort_buffers(&self) -> u64 {
        self.num_sort_buffers
    }
}

impl Drop for IndexMerge {
    fn drop(&mut self) {
        // Close fd before unlinking — Windows requires this.
        self.file.take();
        if let Some(path) = self.tmpfile_path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

// ---------------------------------------------------------------------------
// HeapEntry — one chunk's read cursor in the merge phase.
// ---------------------------------------------------------------------------

struct HeapEntry {
    /// In-memory read window. Refilled from disk when exhausted.
    chunk_buf: Vec<u8>,
    /// Number of valid bytes currently in `chunk_buf` (may be < capacity
    /// at end-of-chunk).
    chunk_valid: usize,
    /// Next-record offset within `chunk_buf`.
    curr_offset_in_buf: usize,
    /// Absolute disk offset of byte 0 of the current `chunk_buf`.
    disk_curr: u64,
    /// Absolute disk offset where the chunk starts.
    disk_start: u64,
    /// True on-disk chunk size, decoded from the header on first load.
    total_size: u64,
    /// Currently-pointed-at record (used for heap ordering + caller return).
    current_key: Bytes,
    current_val: Bytes,
}

impl HeapEntry {
    fn bytes_consumed(&self) -> u64 {
        self.curr_offset_in_buf as u64 + (self.disk_curr - self.disk_start)
    }

    fn is_chunk_finished(&self) -> bool {
        self.bytes_consumed() == self.total_size
    }

    fn has_room(&self, n: usize) -> bool {
        self.curr_offset_in_buf + n <= self.chunk_valid
    }

    /// Try to read a `(key, val)` pair from the current in-memory window.
    /// Returns `None` if there isn't enough room — caller refills and
    /// retries. On `None`, the offset is restored.
    fn try_read_record(&mut self) -> Option<(Bytes, Bytes)> {
        let saved = self.curr_offset_in_buf;
        let Some(klen) = self.read_u64() else {
            self.curr_offset_in_buf = saved;
            return None;
        };
        let Some(key) = self.read_slice(klen as usize) else {
            self.curr_offset_in_buf = saved;
            return None;
        };
        let Some(vlen) = self.read_u64() else {
            self.curr_offset_in_buf = saved;
            return None;
        };
        let Some(val) = self.read_slice(vlen as usize) else {
            self.curr_offset_in_buf = saved;
            return None;
        };
        Some((key, val))
    }

    fn read_u64(&mut self) -> Option<u64> {
        if !self.has_room(8) {
            return None;
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(
            &self.chunk_buf[self.curr_offset_in_buf..self.curr_offset_in_buf + 8],
        );
        self.curr_offset_in_buf += 8;
        Some(u64::from_ne_bytes(bytes))
    }

    fn read_slice(&mut self, len: usize) -> Option<Bytes> {
        if !self.has_room(len) {
            return None;
        }
        let s = Bytes::copy_from_slice(
            &self.chunk_buf[self.curr_offset_in_buf..self.curr_offset_in_buf + len],
        );
        self.curr_offset_in_buf += len;
        Some(s)
    }

    /// Slide the window forward by the amount consumed and re-fill from
    /// disk. C++ counterpart: `merge_buf_info::read_next_chunk_from_disk`.
    fn refill_from_disk(&mut self, file: &mut File) -> Result<(), Error> {
        self.disk_curr += self.curr_offset_in_buf as u64;
        file.seek(SeekFrom::Start(self.disk_curr))
            .map_err(|e| Error::unavailable(format!("seek merge chunk refill: {e}")))?;
        let n = read_filled(file, &mut self.chunk_buf)?;
        self.chunk_valid = n;
        self.curr_offset_in_buf = 0;
        Ok(())
    }
}

// Min-heap on `current_key`: smaller key has higher pop priority. We
// reverse the comparison because `BinaryHeap` is max-heap by default.
impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other.current_key.cmp(&self.current_key)
    }
}
impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Eq for HeapEntry {}
impl PartialEq for HeapEntry {
    // Consistency with Ord — equal-keyed entries are "equal" for heap
    // purposes (cross-chunk tie ordering is undefined, same as the C++).
    fn eq(&self, other: &Self) -> bool {
        self.current_key == other.current_key
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read a record from `entry`, refilling the in-memory window from disk
/// if it's exhausted. Returns `Ok(None)` if the chunk is fully drained.
fn pull_record(
    entry: &mut HeapEntry,
    file: &mut File,
) -> Result<Option<(Bytes, Bytes)>, Error> {
    if entry.is_chunk_finished() {
        return Ok(None);
    }
    if let Some(rec) = entry.try_read_record() {
        return Ok(Some(rec));
    }
    entry.refill_from_disk(file)?;
    if entry.is_chunk_finished() {
        return Ok(None);
    }
    entry
        .try_read_record()
        .ok_or_else(|| Error::unavailable("merge window too small for record".into()))
        .map(Some)
}

/// Read up to `buf.len()` bytes from `file` starting at the current
/// position. Returns the number of bytes actually read — like
/// `Read::read` but tolerant of short reads by looping until either the
/// buffer is full or EOF.
fn read_filled(file: &mut File, buf: &mut [u8]) -> Result<usize, Error> {
    let mut filled = 0usize;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(Error::unavailable(format!("read merge file: {e}")))
            }
        }
    }
    Ok(filled)
}

/// Create a uniquely-named tmpfile in `dir`. Honours the caller's tmpdir
/// (mirrors C++ `mysql_tmpfile_path` semantics, which we don't have in
/// scope yet — see `_DESIGN.md §10`).
fn open_tmpfile(dir: &Path) -> Result<(File, PathBuf), Error> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id();
    let name = format!("slatedb-merge-{pid}-{nanos}-{seq}.tmp");
    let path = dir.join(name);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| {
            Error::unavailable(format!("create merge tmpfile {:?}: {e}", path))
        })?;
    Ok((file, path))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> PathBuf {
        std::env::temp_dir()
    }

    fn make(buf_size: u64, combine_read: u64) -> IndexMerge {
        let mut m = IndexMerge::new(tmpdir(), buf_size, combine_read, 0, 7);
        m.init().expect("init");
        m
    }

    fn drain(m: &mut IndexMerge) -> Vec<(Bytes, Bytes)> {
        let mut out = Vec::new();
        while let Some(rec) = m.next().expect("next") {
            out.push(rec);
        }
        out
    }

    #[test]
    fn in_memory_only_path_returns_sorted() {
        let mut m = make(64 * 1024, 64 * 1024);
        // Insert out of order.
        m.add(b"banana", b"yellow").unwrap();
        m.add(b"apple", b"red").unwrap();
        m.add(b"cherry", b"dark-red").unwrap();
        assert_eq!(m.num_sort_buffers(), 0);

        let out = drain(&mut m);
        let keys: Vec<&[u8]> = out.iter().map(|(k, _)| k.as_ref()).collect();
        assert_eq!(keys, vec![b"apple".as_ref(), b"banana", b"cherry"]);
        assert_eq!(out[0].1.as_ref(), b"red");
    }

    #[test]
    fn duplicate_in_chunk_is_rejected() {
        let mut m = make(64 * 1024, 64 * 1024);
        m.add(b"k", b"v1").unwrap();
        let err = m.add(b"k", b"v2").unwrap_err();
        assert!(matches!(err, IndexMergeError::DuplicateKey));
    }

    #[test]
    fn single_record_too_big_for_buffer() {
        // Buffer header (8) + record headers (16) + 100-byte key = 124 bytes.
        // Buffer < that → BufferTooSmall.
        let mut m = make(64, 64);
        let big_key = vec![b'k'; 100];
        let err = m.add(&big_key, b"v").unwrap_err();
        assert!(matches!(err, IndexMergeError::BufferTooSmall));
    }

    #[test]
    fn spill_then_merge_yields_sorted_stream() {
        // Tiny buffer so multiple spills happen — 256 bytes fits ~8
        // records of (8-byte key, 8-byte val) before the chunk header +
        // record headers push it over.
        let buf = 256u64;
        let mut m = make(buf, buf * 8);

        let n_records: u32 = 50;
        // Shuffle order so the in-memory pre-sort + cross-chunk merge
        // both contribute.
        let perm: Vec<u32> =
            (0..n_records).map(|i| (i * 37 + 11) % n_records).collect();
        for i in &perm {
            let key = format!("key{:05}", i);
            let val = format!("val{:05}", i);
            m.add(key.as_bytes(), val.as_bytes())
                .unwrap_or_else(|e| panic!("add {i}: {e:?}"));
        }
        assert!(
            m.num_sort_buffers() >= 1,
            "expected at least one spill",
        );

        let out = drain(&mut m);
        assert_eq!(out.len(), n_records as usize);
        // Output must be sorted bytewise.
        for w in out.windows(2) {
            assert!(w[0].0 <= w[1].0, "out-of-order: {:?} > {:?}", w[0].0, w[1].0);
        }
        // And every input key must appear exactly once.
        let got_keys: std::collections::HashSet<Bytes> =
            out.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(got_keys.len(), n_records as usize);
        for i in 0..n_records {
            let key = format!("key{:05}", i);
            assert!(got_keys.contains(&Bytes::copy_from_slice(key.as_bytes())));
        }
    }

    #[test]
    fn refill_path_exercised_with_narrow_combine_window() {
        // Multiple spilled chunks AND a per-chunk read window narrower
        // than one chunk forces `refill_from_disk` to be exercised.
        //
        // Record size: 8 (klen) + 7 (key) + 8 (vlen) + 15 (val) = 38 bytes.
        // With ~16 chunks (200 records / ~13-per-chunk), combine_read=2048
        // gives window = 128 — wider than one record (so we make progress)
        // but narrower than a full chunk (so refills happen).
        let buf = 512u64;
        let combine = 2048u64;
        let mut m = make(buf, combine);

        for i in 0u32..200 {
            let k = format!("k{:06}", i);
            let v = format!("v{:06}-payload", i);
            m.add(k.as_bytes(), v.as_bytes()).unwrap();
        }
        assert!(m.num_sort_buffers() >= 2);

        let out = drain(&mut m);
        assert_eq!(out.len(), 200);
        for w in out.windows(2) {
            assert!(w[0].0 < w[1].0);
        }
    }

    #[test]
    fn empty_merge_returns_none() {
        let mut m = make(64 * 1024, 64 * 1024);
        assert!(m.next().unwrap().is_none());
    }

    #[test]
    fn cf_id_preserved() {
        let m = IndexMerge::new(tmpdir(), 4096, 4096, 0, 42);
        assert_eq!(m.cf_id(), 42);
    }

    #[test]
    fn drop_removes_tmpfile() {
        let path: PathBuf;
        {
            let mut m = make(4096, 4096);
            path = m.tmpfile_path.clone().expect("tmpfile created");
            assert!(path.exists(), "tmpfile should exist while m is alive");
            m.add(b"k", b"v").unwrap();
            drop(m);
        }
        assert!(!path.exists(), "tmpfile should be removed on Drop");
    }
}
