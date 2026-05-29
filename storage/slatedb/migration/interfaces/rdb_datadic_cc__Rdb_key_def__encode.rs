//! Interface stub for `Rdb_key_def__encode`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.cc` (lines 1001..2467)
//! C++ header: `storage/rocksdb/rdb_datadic.h` (Rdb_key_def encode side)
//! v4 manifest sub-unit: `Rdb_key_def__encode`
//! Parent unit: `rdb_datadic.cc`
//! Approx body LoC accounted for here: ~1100 (pack_* family + helpers)
//!
//! ## Mapping
//! Per _DESIGN.md §2: **memcomparable key encoding is preserved bit-for-bit**
//! from MyRocks. SlateDB sorts bytewise and the MyRocks encoding correctness
//! rules are identical to RocksDB's. Therefore every `pack_*` routine
//! translates 1:1 to a Rust fn that emits bytes into a `Vec<u8>` /
//! `BytesMut`; the produced byte sequence is identical to MyRocks for the
//! same logical row.
//!
//! Reverse-ordered indexes use `KeyDirection::Reverse` from
//! `rdb_comparator_h::apply_direction` after packing — XOR with `0xff` over
//! the memcomparable bytes (see `rdb_comparator_h.rs`). This preserves the
//! "stored bytes already in scan order" invariant.
//!
//! Per _DESIGN.md §3: TTL is **no longer written as prefix bytes in values**.
//! For TTL-bearing rows the codec only computes the `expire_ts` to pass to
//! `PutOptions::ttl`; the value blob has no TTL prefix.
//!
//! ## Out-of-scope methods
//! None — every pack routine maps to a Rust fn. The constructor / destructor
//! (`Rdb_key_def::Rdb_key_def`, `~Rdb_key_def`) live in
//! `Rdb_key_def__meta` because they are state-management, not codec.
//! `pack_field`/`pack_record` retain the same callee-allocates-buffer
//! contract; the caller passes a `&mut Vec<u8>` that the routine appends to.

use bytes::BytesMut;
use slatedb::Error;

use crate::rdb_buff_h::{StringWriter, BitWriter};

/// Opaque field descriptor — translated from `Rdb_field_packing`. The codec
/// dispatches on the descriptor's `pack_func` slot to choose the encoding
/// routine. Real definition lives in `Rdb_field_packing` stub.
pub struct FieldPacking; // TODO(human): wire to Rdb_field_packing stub
pub struct KeyDef;        // TODO(human): wire to Rdb_key_def__meta stub
pub struct CollationCodec; // TODO(human): wire to collation table

/// Pack a single field's memcomparable image. Big switch over the field's
/// MySQL type — integer/float/newdate/varchar/binary string/etc.
///
/// **Inputs:**
/// - `field_value`: raw bytes from the MySQL `Field::ptr` buffer.
/// - `pack_info`: descriptor specifying width, charset, nullability.
/// - `out`: destination buffer; the routine appends.
/// - `unpack_info`: optional sidechannel for storing data the encoding loses.
///
/// **Output:** number of bytes appended to `out`.
/// **Errors:** none expected on validated input; on programmer bug returns
/// `slatedb::Error::invalid`.
/// **Invariants:** byte-for-byte identical to MyRocks `pack_field`.
///
/// C++: rdb_datadic.cc:1210.
pub fn pack_field(
    _kd: &KeyDef,
    _field_value: &[u8],
    _pack_info: &FieldPacking,
    _out: &mut Vec<u8>,
    _unpack_info: Option<&mut StringWriter>,
) -> Result<usize, Error> {
    todo!("port C++ pack_field switch at rdb_datadic.cc:1210")
}

/// Pack the full record into the index-tuple form.
/// `pack_buffer` is a scratch area used by per-field packers; in Rust we
/// take it as `&mut Vec<u8>` and let it grow.
///
/// C++: rdb_datadic.cc:1272.
pub fn pack_record(
    _kd: &KeyDef,
    _record: &[u8],
    _pack_buffer: &mut Vec<u8>,
    _out_key: &mut Vec<u8>,
    _out_unpack_info: Option<&mut StringWriter>,
    _ttl_value: Option<u64>,
) -> Result<usize, Error> {
    todo!("port C++ pack_record at rdb_datadic.cc:1272; do NOT prepend TTL prefix bytes (per _DESIGN.md §3)")
}

/// Pack the hidden PK rowid into the key. Hidden PK is `varint(rowid)`
/// per _DESIGN.md §2.
///
/// C++: rdb_datadic.cc:1468.
pub fn pack_hidden_pk(
    _hidden_pk_id: i64,
    _out_key: &mut Vec<u8>,
) -> Result<usize, Error> {
    todo!("emit varint(hidden_pk_id) per _DESIGN.md §2")
}

/// Pack the index tuple — the prefix `varint(cf_id) || u32_be(index_id)`
/// then memcmp key bytes via `pack_record`.
///
/// C++: rdb_datadic.cc:1001.
pub fn pack_index_tuple(
    _kd: &KeyDef,
    _record: &[u8],
    _key_part_map: u32,
    _out_key: &mut Vec<u8>,
) -> Result<usize, Error> {
    todo!("port C++ pack_index_tuple at rdb_datadic.cc:1001")
}

/// Pack using a precomputed `make_sort_key` image (charset transformations
/// already applied by the SQL layer).
///
/// C++: rdb_datadic.cc:1489.
pub fn pack_with_make_sort_key(
    _fpi: &FieldPacking,
    _src: &[u8],
    _out: &mut Vec<u8>,
    _unpack_info: Option<&mut StringWriter>,
) {
    todo!("port C++ pack_with_make_sort_key at rdb_datadic.cc:1489")
}

/// Pack a varchar using the *legacy* variable-length encoding (escape-byte
/// based). Used when KV format version < new-format threshold.
///
/// C++: rdb_datadic.cc:2162.
pub fn pack_legacy_variable_format(
    _src: &[u8],
    _src_len: usize,
    _out: &mut Vec<u8>,
) {
    todo!("port C++ pack_legacy_variable_format at rdb_datadic.cc:2162")
}

/// Pack a varchar using the new variable-length encoding (length-prefix
/// based).
///
/// C++: rdb_datadic.cc:2214.
pub fn pack_variable_format(
    _src: &[u8],
    _src_len: usize,
    _out: &mut Vec<u8>,
) {
    todo!("port C++ pack_variable_format at rdb_datadic.cc:2214")
}

/// Pack a varchar with the chosen charset/collation. Routes to legacy or
/// new format internally based on `fpi.m_kv_format_version`.
///
/// C++: rdb_datadic.cc:2254.
pub fn pack_with_varchar_encoding(
    _fpi: &FieldPacking,
    _src: &[u8],
    _out: &mut Vec<u8>,
    _unpack_info: Option<&mut StringWriter>,
) {
    todo!("port C++ pack_with_varchar_encoding at rdb_datadic.cc:2254")
}

/// Pack a varchar with space-padding (used by `VARCHAR_PAD_SPACE`
/// collations). Trailing-spaces are encoded with VARCHAR_CMP_* markers.
///
/// C++: rdb_datadic.cc:2365.
pub fn pack_with_varchar_space_pad(
    _fpi: &FieldPacking,
    _src: &[u8],
    _out: &mut Vec<u8>,
    _unpack_info: Option<&mut StringWriter>,
) {
    todo!("port C++ pack_with_varchar_space_pad at rdb_datadic.cc:2365")
}

/// Sidechannel writer for the "simple" collation case — uses the
/// `Rdb_bit_writer` to pack the original-byte indices into unpack info.
///
/// C++ free fn `rdb_write_unpack_simple` at rdb_datadic.cc:2815.
pub fn write_unpack_simple(
    _writer: &mut BitWriter,
    _codec: &CollationCodec,
    _src: &[u8],
) {
    todo!("port C++ rdb_write_unpack_simple at rdb_datadic.cc:2815")
}

/// Convenience wrapper: produce a freestanding `bytes::Bytes` for the
/// packed key. The codec internals work with `Vec<u8>` for cheap appends;
/// this conversion is zero-copy via `Bytes::from(Vec<u8>)`.
pub fn finalize_key(buf: Vec<u8>) -> bytes::Bytes {
    bytes::Bytes::from(buf)
}

/// Convenience wrapper for the value side.
pub fn finalize_value(buf: BytesMut) -> bytes::Bytes {
    buf.freeze()
}
