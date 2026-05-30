//! Byte-level buffer helpers.
//!
//! Translated from `storage/rocksdb/rdb_buff.h`. Network-byte-order (big
//! endian) encoders, sliding-window readers, and bit-level pack/unpack. The
//! on-disk key format is preserved bit-for-bit (`_DESIGN.md §2`), so each
//! helper is a direct port.

use slatedb::Error;

use crate::globals::GlIndexId;

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

/// `index_id` is just a big-endian `u32` — alias for clarity.
pub fn store_index(dst: &mut [u8], index_id: u32) {
    store_u32_be(dst, index_id);
}

// --- network-byte-order reads ---

pub fn read_u64_be(src: &[u8]) -> Option<u64> {
    Some(u64::from_be_bytes(src.get(..8)?.try_into().ok()?))
}
pub fn read_u32_be(src: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(src.get(..4)?.try_into().ok()?))
}
pub fn read_u16_be(src: &[u8]) -> Option<u16> {
    Some(u16::from_be_bytes(src.get(..2)?.try_into().ok()?))
}

// --- string reader (sliding window over an immutable byte slice) ---

pub struct StringReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> StringReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn read(&mut self, size: usize) -> Option<&'a [u8]> {
        if self.pos.checked_add(size)? > self.buf.len() {
            return None;
        }
        let out = &self.buf[self.pos..self.pos + size];
        self.pos += size;
        Some(out)
    }

    pub fn read_u8(&mut self) -> Option<u8> {
        self.read(1).map(|s| s[0])
    }
    pub fn read_u16_be(&mut self) -> Option<u16> {
        self.read(2).and_then(read_u16_be)
    }
    pub fn read_u32_be(&mut self) -> Option<u32> {
        self.read(4).and_then(read_u32_be)
    }
    pub fn read_u64_be(&mut self) -> Option<u64> {
        self.read(8).and_then(read_u64_be)
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
    pub fn current_pos(&self) -> usize {
        self.pos
    }
}

// --- string writer (growing byte buffer) ---

#[derive(Default)]
pub struct StringWriter {
    data: Vec<u8>,
}

impl StringWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }
    pub fn write_u8(&mut self, val: u8) {
        self.data.push(val);
    }
    pub fn write_u16_be(&mut self, val: u16) {
        append_u16_be(&mut self.data, val);
    }
    pub fn write_u32_be(&mut self, val: u32) {
        append_u32_be(&mut self.data, val);
    }
    pub fn write(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
    }

    pub fn ptr(&self) -> &[u8] {
        &self.data
    }
    pub fn ptr_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
    pub fn current_pos(&self) -> usize {
        self.data.len()
    }

    pub fn write_u8_at(&mut self, pos: usize, val: u8) {
        self.data[pos] = val;
    }

    pub fn write_u16_at(&mut self, pos: usize, val: u16) {
        store_u16_be(&mut self.data[pos..], val);
    }

    pub fn truncate(&mut self, pos: usize) {
        self.data.truncate(pos);
    }

    pub fn allocate(&mut self, len: usize, val: u8) {
        self.data.resize(self.data.len() + len, val);
    }

    pub fn into_bytes(self) -> bytes::Bytes {
        bytes::Bytes::from(self.data)
    }
}

// --- bit-level packing ---

/// Bit writer that pushes into a `StringWriter`. Each call appends to the
/// current trailing byte until full, then allocates a new zero byte.
///
/// Port of `Rdb_bit_writer` (`rdb_buff.h:417..446`). The bit-packing loop is
/// translated 1:1 — `value` is consumed MSB-first and placed in increasing
/// offsets within each byte.
pub struct BitWriter<'a> {
    writer: &'a mut StringWriter,
    offset: u8,
}

impl<'a> BitWriter<'a> {
    pub fn new(writer: &'a mut StringWriter) -> Self {
        Self { writer, offset: 0 }
    }

    pub fn write(&mut self, size: u32, value: u32) {
        debug_assert!(
            size <= 32 && (size == 32 || (value & ((1u32 << size) - 1)) == value),
            "value does not fit in size bits"
        );

        let mut size = size;
        while size > 0 {
            if self.offset == 0 {
                self.writer.write_u8(0);
            }
            let bits = size.min(8 - u32::from(self.offset));
            let mask = if bits == 32 { u32::MAX } else { (1u32 << bits) - 1 };
            let chunk = ((value >> (size - bits)) & mask) as u8;
            let pos = self.writer.current_pos() - 1;
            self.writer.data[pos] |= chunk << self.offset;
            size -= bits;
            self.offset = (self.offset + bits as u8) & 0x7;
        }
    }
}

/// Bit reader pulling from a `StringReader`. Sister of `BitWriter`.
///
/// Port of `Rdb_bit_reader` (`rdb_buff.h:447..487`).
pub struct BitReader<'a, 'b: 'a> {
    reader: &'a mut StringReader<'b>,
    offset: u8,
    cur: u8,
}

impl<'a, 'b> BitReader<'a, 'b> {
    pub fn new(reader: &'a mut StringReader<'b>) -> Self {
        Self {
            reader,
            offset: 0,
            cur: 0,
        }
    }

    /// Returns `None` if the underlying reader runs out of bytes mid-read.
    pub fn read(&mut self, size: u32) -> Option<u32> {
        debug_assert!(size <= 32);
        let mut size = size;
        let mut ret: u32 = 0;
        while size > 0 {
            if self.offset == 0 {
                self.cur = self.reader.read_u8()?;
            }
            let bits = size.min(8 - u32::from(self.offset));
            let mask = if bits == 32 { u32::MAX } else { (1u32 << bits) - 1 };
            ret <<= bits;
            ret |= (u32::from(self.cur) >> self.offset) & mask;
            size -= bits;
            self.offset = (self.offset + bits as u8) & 0x7;
        }
        Some(ret)
    }
}

// --- fixed-capacity stack buffer writer ---

pub struct BufWriter<const N: usize> {
    buf: [u8; N],
    pos: usize,
}

impl<const N: usize> BufWriter<N> {
    pub fn new() -> Self {
        Self {
            buf: [0; N],
            pos: 0,
        }
    }

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
    pub fn write_index(&mut self, index_id: u32) {
        self.write_u32_be(index_id);
    }

    pub fn reset(&mut self) {
        self.pos = 0;
    }
    pub fn data(&self) -> &[u8] {
        &self.buf[..self.pos]
    }
    pub fn capacity(&self) -> usize {
        N
    }
    pub fn size(&self) -> usize {
        self.pos
    }

    pub fn to_bytes(&self) -> bytes::Bytes {
        bytes::Bytes::copy_from_slice(self.data())
    }
}

impl<const N: usize> Default for BufWriter<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Read a `(cf_id, index_id)` pair from a netbuf, advancing the cursor.
pub fn read_gl_index_id(reader: &mut StringReader) -> Result<GlIndexId, Error> {
    let cf_id = reader
        .read_u32_be()
        .ok_or_else(|| Error::data("gl_index: cf_id under-read".into()))?;
    let index_id = reader
        .read_u32_be()
        .ok_or_else(|| Error::data("gl_index: index_id under-read".into()))?;
    Ok(GlIndexId { cf_id, index_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn be_round_trip_u64_u32_u16() {
        let mut out = Vec::new();
        append_u64_be(&mut out, 0x0123_4567_89ab_cdef);
        append_u32_be(&mut out, 0xdead_beef);
        append_u16_be(&mut out, 0xc0de);

        let mut r = StringReader::new(&out);
        assert_eq!(r.read_u64_be(), Some(0x0123_4567_89ab_cdef));
        assert_eq!(r.read_u32_be(), Some(0xdead_beef));
        assert_eq!(r.read_u16_be(), Some(0xc0de));
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn reader_underflow_returns_none() {
        let mut r = StringReader::new(&[0u8; 3]);
        assert_eq!(r.read(4), None);
        assert_eq!(r.current_pos(), 0);
    }

    #[test]
    fn bit_writer_then_reader_round_trip() {
        let mut w = StringWriter::new();
        {
            let mut bw = BitWriter::new(&mut w);
            bw.write(3, 0b101);
            bw.write(5, 0b1_1010);
            bw.write(12, 0xabc);
            bw.write(8, 0xff);
        }
        let bytes = w.into_bytes();
        let mut r = StringReader::new(&bytes);
        let mut br = BitReader::new(&mut r);
        assert_eq!(br.read(3), Some(0b101));
        assert_eq!(br.read(5), Some(0b1_1010));
        assert_eq!(br.read(12), Some(0xabc));
        assert_eq!(br.read(8), Some(0xff));
    }

    #[test]
    fn buf_writer_packs_then_returns_bytes() {
        let mut bw: BufWriter<16> = BufWriter::new();
        bw.write_u32_be(0x0102_0304);
        bw.write_u16_be(0x1122);
        bw.write(b"abcd");
        assert_eq!(bw.size(), 10);
        assert_eq!(
            bw.data(),
            &[0x01, 0x02, 0x03, 0x04, 0x11, 0x22, b'a', b'b', b'c', b'd']
        );
    }

    #[test]
    fn read_gl_index_id_round_trip() {
        let mut w = StringWriter::new();
        w.write_u32_be(7);
        w.write_u32_be(42);
        let bytes = w.into_bytes();
        let mut r = StringReader::new(&bytes);
        let gl = read_gl_index_id(&mut r).expect("must parse");
        assert_eq!(gl.cf_id, 7);
        assert_eq!(gl.index_id, 42);
    }
}
