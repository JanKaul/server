//! Interface stub for `rdb_buff_h`.
//!
//! C++ source: `storage/rocksdb/rdb_buff.h` (549 LoC)
//!
//! ## Mapping
//! Pure byte-level buffer helpers — network-byte-order (big-endian) encoders,
//! a string reader/writer, and a bit reader/writer. All in-scope and translates
//! directly to safe Rust. Encoders preserve bit-for-bit format because the
//! on-disk key format (per _DESIGN.md §2) is preserved from MyRocks.
//!
//! ## Out-of-scope
//! None — pure data manipulation, fully in scope.

use crate::error::SlateError;

// --- network-byte-order primitives ---
//
// MyRocks calls these `netstr` (writes to a `String*`) and `netbuf` (writes to
// a `uchar*`). In Rust we collapse both onto `Vec<u8>` (owned writer) and
// `&mut [u8]` (in-place writer); the original split was a C++ artifact.

/// Append a big-endian `u64` to a `Vec<u8>` writer.
/// Original: rdb_buff.h:61 — `rdb_netstr_append_uint64`.
pub fn append_u64_be(out: &mut Vec<u8>, val: u64) {
    out.extend_from_slice(&val.to_be_bytes());
}
pub fn append_u32_be(out: &mut Vec<u8>, val: u32) {
    out.extend_from_slice(&val.to_be_bytes());
}
pub fn append_u16_be(out: &mut Vec<u8>, val: u16) {
    out.extend_from_slice(&val.to_be_bytes());
}

/// In-place big-endian store into a fixed-size buffer slice. Panics if
/// `dst.len() < 8` — caller's responsibility (matches C++ which assumes
/// the caller pre-sized the buffer).
/// Original: rdb_buff.h:95 — `rdb_netbuf_store_uint64`.
pub fn store_u64_be(dst: &mut [u8], val: u64) {
    dst[..8].copy_from_slice(&val.to_be_bytes());
}
pub fn store_u32_be(dst: &mut [u8], val: u32) {
    dst[..4].copy_from_slice(&val.to_be_bytes());
}
pub fn store_u16_be(dst: &mut [u8], val: u16) {
    dst[..2].copy_from_slice(&val.to_be_bytes());
}
pub fn store_byte(dst: &mut [u8], val: u8) {
    dst[0] = val;
}

/// Index id is just a big-endian u32 — alias for clarity.
/// Original: rdb_buff.h:128 — `rdb_netbuf_store_index`.
pub fn store_index(dst: &mut [u8], index_id: u32) {
    store_u32_be(dst, index_id);
}

// --- network-byte-order reads ---

pub fn read_u64_be(src: &[u8]) -> u64 {
    u64::from_be_bytes(src[..8].try_into().expect("buffer too small"))
}
pub fn read_u32_be(src: &[u8]) -> u32 {
    u32::from_be_bytes(src[..4].try_into().expect("buffer too small"))
}
pub fn read_u16_be(src: &[u8]) -> u16 {
    u16::from_be_bytes(src[..2].try_into().expect("buffer too small"))
}

// --- string reader (sliding window over an immutable byte slice) ---

/// Reads sequential bytes from a slice, advancing an internal cursor.
/// Per-read length checks return an error rather than panic — matches the
/// MyRocks pattern of returning `nullptr` on under-read.
///
/// Original: rdb_buff.h:240 — `class Rdb_string_reader`.
pub struct StringReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> StringReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Read `size` bytes, advancing the cursor. Returns `None` if under-read
    /// (matches C++ `read()` returning `nullptr`).
    pub fn read(&mut self, size: usize) -> Option<&'a [u8]> {
        if self.pos + size > self.buf.len() {
            return None;
        }
        let out = &self.buf[self.pos..self.pos + size];
        self.pos += size;
        Some(out)
    }

    pub fn read_u8(&mut self) -> Option<u8> { self.read(1).map(|s| s[0]) }
    pub fn read_u16_be(&mut self) -> Option<u16> { self.read(2).map(read_u16_be) }
    pub fn read_u32_be(&mut self) -> Option<u32> { self.read(4).map(read_u32_be) }
    pub fn read_u64_be(&mut self) -> Option<u64> { self.read(8).map(read_u64_be) }

    pub fn remaining(&self) -> usize { self.buf.len() - self.pos }
    pub fn current_pos(&self) -> usize { self.pos }
}

// --- string writer (growing byte buffer) ---

/// Append-only byte buffer with random-access patch helpers (for back-fill
/// of length prefixes etc.).
///
/// Original: rdb_buff.h:349 — `class Rdb_string_writer`.
pub struct StringWriter {
    data: Vec<u8>,
}

impl StringWriter {
    pub fn new() -> Self { Self { data: Vec::new() } }

    pub fn clear(&mut self) { self.data.clear(); }
    pub fn write_u8(&mut self, val: u8) { self.data.push(val); }
    pub fn write_u16_be(&mut self, val: u16) { append_u16_be(&mut self.data, val); }
    pub fn write_u32_be(&mut self, val: u32) { append_u32_be(&mut self.data, val); }
    pub fn write(&mut self, bytes: &[u8]) { self.data.extend_from_slice(bytes); }

    pub fn ptr(&self) -> &[u8] { &self.data }
    pub fn ptr_mut(&mut self) -> &mut [u8] { &mut self.data }
    pub fn current_pos(&self) -> usize { self.data.len() }

    /// Patch a previously-written `u8` at `pos`. Caller's responsibility to ensure
    /// `pos < current_pos`.
    pub fn write_u8_at(&mut self, pos: usize, val: u8) { self.data[pos] = val; }

    pub fn write_u16_at(&mut self, pos: usize, val: u16) {
        store_u16_be(&mut self.data[pos..], val);
    }

    pub fn truncate(&mut self, pos: usize) { self.data.truncate(pos); }

    /// Reserve `len` bytes, zero-initialized (or `val`-initialized).
    /// Original: rdb_buff.h:399 — `allocate()`.
    pub fn allocate(&mut self, len: usize, val: u8) {
        self.data.resize(self.data.len() + len, val);
    }
}

impl Default for StringWriter {
    fn default() -> Self { Self::new() }
}

// --- bit-level packing ---

/// Writes variable-width bit fields into a `StringWriter`. Assumes no
/// concurrent byte-level writes happen on the underlying writer.
///
/// Original: rdb_buff.h:417 — `class Rdb_bit_writer`.
pub struct BitWriter<'a> {
    writer: &'a mut StringWriter,
    offset: u8, // 0..=7, bit position within the last byte
}

impl<'a> BitWriter<'a> {
    pub fn new(writer: &'a mut StringWriter) -> Self {
        Self { writer, offset: 0 }
    }

    /// Write `value` as `size` bits. Bits are packed LSB-first within each
    /// byte. `size <= 32`. `value` must fit in `size` bits (high bits truncated).
    pub fn write(&mut self, size: u32, value: u32) {
        todo!("port C++ bit-packing loop from rdb_buff.h:428")
    }
}

/// Reads variable-width bit fields from a `StringReader`. Owns a transient
/// result location; sequential reads overwrite it.
///
/// Original: rdb_buff.h:447 — `class Rdb_bit_reader`.
pub struct BitReader<'a, 'b: 'a> {
    reader: &'a mut StringReader<'b>,
    offset: u8,
    cur: Option<u8>, // last byte fetched from reader
}

impl<'a, 'b> BitReader<'a, 'b> {
    pub fn new(reader: &'a mut StringReader<'b>) -> Self {
        Self { reader, offset: 0, cur: None }
    }

    /// Read the next `size` bits. Returns `None` on under-read of the
    /// underlying string reader.
    pub fn read(&mut self, size: u32) -> Option<u32> {
        todo!("port C++ bit-unpacking loop from rdb_buff.h:463")
    }
}

// --- fixed-capacity stack buffer writer ---

/// Compile-time-sized stack buffer that we write into without heap allocation.
/// Used for short on-stack key/value scratch space in hot paths (e.g., a 16-byte
/// hidden-PK encoding buffer).
///
/// Original: rdb_buff.h:486 — `template <size_t buf_length> class Rdb_buf_writer`.
///
/// Rust translation: a struct generic over const `N` exposing the same write_*
/// surface plus `as_slice()` for the bytes-so-far.
pub struct BufWriter<const N: usize> {
    buf: [u8; N],
    pos: usize,
}

impl<const N: usize> BufWriter<N> {
    pub fn new() -> Self { Self { buf: [0; N], pos: 0 } }

    pub fn write_u8(&mut self, val: u8) {
        self.buf[self.pos] = val;
        self.pos += 1;
    }
    pub fn write_u16_be(&mut self, val: u16) {
        store_u16_be(&mut self.buf[self.pos..], val);
        self.pos += 2;
    }
    pub fn write_u32_be(&mut self, val: u32) {
        store_u32_be(&mut self.buf[self.pos..], val);
        self.pos += 4;
    }
    pub fn write_u64_be(&mut self, val: u64) {
        store_u64_be(&mut self.buf[self.pos..], val);
        self.pos += 8;
    }
    pub fn write(&mut self, bytes: &[u8]) {
        self.buf[self.pos..self.pos + bytes.len()].copy_from_slice(bytes);
        self.pos += bytes.len();
    }
    pub fn write_index(&mut self, index_id: u32) { self.write_u32_be(index_id); }

    pub fn reset(&mut self) { self.pos = 0; }
    pub fn data(&self) -> &[u8] { &self.buf[..self.pos] }
    pub fn capacity(&self) -> usize { N }
    pub fn size(&self) -> usize { self.pos }
}

impl<const N: usize> Default for BufWriter<N> {
    fn default() -> Self { Self::new() }
}

/// Helper: read a `(cf_id, index_id)` pair from a netbuf, advancing the cursor.
/// Original: rdb_buff.h:225 — `rdb_netbuf_read_gl_index`.
pub fn read_gl_index_id(reader: &mut StringReader) -> Result<crate::rdb_global_h::GlIndexId, SlateError> {
    let cf_id = reader.read_u32_be().ok_or(SlateError::Corruption("gl_index: cf_id under-read".into()))?;
    let index_id = reader.read_u32_be().ok_or(SlateError::Corruption("gl_index: index_id under-read".into()))?;
    Ok(crate::rdb_global_h::GlIndexId { cf_id, index_id })
}
