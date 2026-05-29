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
//! We use `bytes::Bytes` / `BytesMut` from SlateDB's re-exported `bytes` crate
//! (`slatedb::bytes`) for zero-copy buffer handling where appropriate.
//!
//! ## Out-of-scope
//! None — pure data manipulation, fully in scope.

use slatedb::Error;

// --- network-byte-order primitives ---

pub fn append_u64_be(out: &mut Vec<u8>, val: u64) {
    out.extend_from_slice(&val.to_be_bytes());
}
pub fn append_u32_be(out: &mut Vec<u8>, val: u32) {
    out.extend_from_slice(&val.to_be_bytes());
}
pub fn append_u16_be(out: &mut Vec<u8>, val: u16) {
    out.extend_from_slice(&val.to_be_bytes());
}

/// In-place big-endian store. Caller is responsible for sizing the slice
/// (matches C++ which assumes the caller pre-sized).
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

/// Original: rdb_buff.h:240 — `class Rdb_string_reader`.
pub struct StringReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> StringReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

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

    pub fn write_u8_at(&mut self, pos: usize, val: u8) { self.data[pos] = val; }

    pub fn write_u16_at(&mut self, pos: usize, val: u16) {
        store_u16_be(&mut self.data[pos..], val);
    }

    pub fn truncate(&mut self, pos: usize) { self.data.truncate(pos); }

    pub fn allocate(&mut self, len: usize, val: u8) {
        self.data.resize(self.data.len() + len, val);
    }

    /// Convert to a `bytes::Bytes` (zero-copy via `Vec → Bytes`).
    /// Used when handing the buffer to SlateDB which takes `Bytes` for keys/values.
    pub fn into_bytes(self) -> slatedb::bytes::Bytes {
        slatedb::bytes::Bytes::from(self.data)
    }
}

impl Default for StringWriter {
    fn default() -> Self { Self::new() }
}

// --- bit-level packing ---

/// Original: rdb_buff.h:417 — `class Rdb_bit_writer`.
pub struct BitWriter<'a> {
    writer: &'a mut StringWriter,
    offset: u8,
}

impl<'a> BitWriter<'a> {
    pub fn new(writer: &'a mut StringWriter) -> Self {
        Self { writer, offset: 0 }
    }
    pub fn write(&mut self, size: u32, value: u32) {
        todo!("port C++ bit-packing loop from rdb_buff.h:428")
    }
}

/// Original: rdb_buff.h:447 — `class Rdb_bit_reader`.
pub struct BitReader<'a, 'b: 'a> {
    reader: &'a mut StringReader<'b>,
    offset: u8,
    cur: Option<u8>,
}

impl<'a, 'b> BitReader<'a, 'b> {
    pub fn new(reader: &'a mut StringReader<'b>) -> Self {
        Self { reader, offset: 0, cur: None }
    }
    pub fn read(&mut self, size: u32) -> Option<u32> {
        todo!("port C++ bit-unpacking loop from rdb_buff.h:463")
    }
}

// --- fixed-capacity stack buffer writer ---

/// Original: rdb_buff.h:486 — `template <size_t buf_length> class Rdb_buf_writer`.
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

    /// Copy the written prefix into a `Bytes` (cheap — small fixed buffers).
    pub fn to_bytes(&self) -> slatedb::bytes::Bytes {
        slatedb::bytes::Bytes::copy_from_slice(self.data())
    }
}

impl<const N: usize> Default for BufWriter<N> {
    fn default() -> Self { Self::new() }
}

/// Helper: read a `(cf_id, index_id)` pair from a netbuf, advancing the cursor.
/// Original: rdb_buff.h:225 — `rdb_netbuf_read_gl_index`.
pub fn read_gl_index_id(reader: &mut StringReader) -> Result<crate::rdb_global_h::GlIndexId, Error> {
    let cf_id = reader.read_u32_be().ok_or_else(|| Error::data("gl_index: cf_id under-read".into()))?;
    let index_id = reader.read_u32_be().ok_or_else(|| Error::data("gl_index: index_id under-read".into()))?;
    Ok(crate::rdb_global_h::GlIndexId { cf_id, index_id })
}
