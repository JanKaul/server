//! Interface stub for `ha_rocksdb_h__unique_sk_buf_info`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.h` (lines 693..713, 21 LoC body)
//! v4 manifest sub-unit: `ha_rocksdb_h__unique_sk_buf_info`
//! parent: `ha_rocksdb_h`
//!
//! ## Mapping
//! Two-buffer ping-pong scratch used during inplace unique-index creation
//! to retain the prior memcmp-form SK while building the next one for
//! duplicate-detection comparison.
//!
//! In SlateDB terms (per _DESIGN.md §1, "SST bulk loader → Map degraded"),
//! inplace SK creation uses our `WriteBatcher` to stream batched writes; the
//! ping-pong buffer compares adjacent SK encodings for duplicate detection
//! before the batch is committed.
//!
//! ## Out-of-scope methods
//! None — pure data carrier with one method.

use slatedb::bytes::Bytes;

/// Ping-pong buffer for inplace unique-index dup detection. Each iteration
/// of the inplace-populate-sk loop calls `swap_and_get_sk_buf()` to switch
/// between the two storage buffers, then encodes the next SK into the
/// returned mutable buffer. The previously-encoded SK remains intact in the
/// other buffer for comparison (`sk_memcmp_key_old`).
///
/// Original: ha_rocksdb.h:693 — `struct unique_sk_buf_info`.
pub struct UniqueSkBufInfo {
    /// Toggle: false → use buf A, true → use buf B.
    /// Original: ha_rocksdb.h:694 — `bool sk_buf_switch`.
    sk_buf_switch: bool,

    /// View of the most-recently-encoded SK (memcmp form). Updated after each
    /// `swap_and_get_sk_buf()` call.
    pub sk_memcmp_key: Bytes,
    /// View of the previously-encoded SK — compared against `sk_memcmp_key`
    /// to detect duplicates.
    pub sk_memcmp_key_old: Bytes,

    /// Backing buffer A.
    dup_sk_buf: Vec<u8>,
    /// Backing buffer B.
    dup_sk_buf_old: Vec<u8>,
}

impl UniqueSkBufInfo {
    pub fn new() -> Self {
        Self {
            sk_buf_switch: false,
            sk_memcmp_key: Bytes::new(),
            sk_memcmp_key_old: Bytes::new(),
            dup_sk_buf: Vec::new(),
            dup_sk_buf_old: Vec::new(),
        }
    }

    /// Toggle and return a mutable reference to the buffer the encoder should
    /// write the next SK into. The OTHER buffer retains the prior SK.
    ///
    /// Original: ha_rocksdb.h:709 — `inline uchar *swap_and_get_sk_buf()`.
    pub fn swap_and_get_sk_buf(&mut self) -> &mut Vec<u8> {
        self.sk_buf_switch = !self.sk_buf_switch;
        if self.sk_buf_switch { &mut self.dup_sk_buf } else { &mut self.dup_sk_buf_old }
    }
}

impl Default for UniqueSkBufInfo {
    fn default() -> Self {
        Self::new()
    }
}
