//! Interface stub for `rdb_datadic_h__Rdb_convert_to_record_key_decoder`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (lines 52..71)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_convert_to_record_key_decoder`
//! Parent unit: `rdb_datadic_h`
//! Approx body LoC accounted for here: 20
//!
//! ## Mapping
//! In-scope codec types — preserved from MyRocks per _DESIGN.md §2/§3. This
//! is a stateless static-method "namespace" that walks a packed memcomparable
//! key and writes back into a MySQL `record[]` buffer, one field at a time.
//! Because the on-disk key format is preserved bit-for-bit, the decoding
//! algorithm is identical; only the buffer/reader types change.
//!
//! All three methods are `static` in C++; we translate to free functions in
//! this module (no associated `struct` state is needed). The C++ class only
//! existed to forbid copy/move, which is moot in Rust.
//!
//! Public callers live in the `Rdb_key_field_iterator` loop body and in
//! `convert_record_from_storage_format` (handler-side). Both will switch to
//! these functions in TRANSLATE.
//!
//! ## Out-of-scope methods
//! None — every static method has a direct Rust analogue. The private
//! `decode_field` helper is also exposed here as `pub(crate)` since it's
//! the per-field worker that the public `decode` loops over.

use slatedb::Error;

use crate::rdb_buff_h::StringReader;

/// Opaque forward to the future Rdb_field_packing unit. See
/// `rdb_datadic_h__Rdb_field_packing`.
pub struct FieldPacking; // TODO(human): wire to Rdb_field_packing stub

/// POD substitute for MySQL's `TABLE*`. Holds only the per-table metadata the
/// decoder actually reads (charsets per field, null bitmap offsets, the field
/// list itself). Real definition will live in the future TABLE-shape unit.
pub struct TableShareView; // TODO(human): forward to TABLE-shape unit

/// POD substitute for MySQL's `Field*`. Carries the field's MySQL type, the
/// destination pointer/offset into `RowBytes`, and the null-byte mask.
pub struct FieldView; // TODO(human): forward to TABLE-shape unit

/// Mutable view over the `record[]` byte buffer the decoder writes into.
/// Aliased to a `&mut [u8]` in practice; declared as a wrapper so a future
/// row-builder can swap to `BytesMut` without changing the function signature.
pub type RowBytes<'a> = &'a mut [u8];

/// Decode one key-part from `reader` into `buf` at `offset`. Returns the new
/// offset after the field's bytes have been written.
///
/// **Inputs:**
/// - `buf`: row buffer to write into.
/// - `offset`: current write position; updated by callee.
/// - `fpi`: field-packing descriptor (tells us width, charset, nullability).
/// - `table`: view over the TABLE — only `charset_info` and null bitmap
///   pointers are read.
/// - `field`: descriptor of the field we are writing into.
/// - `has_unpack_info`: whether the index carries the sidechannel.
/// - `reader`: memcomparable key cursor.
/// - `unpack_reader`: sidechannel cursor (None if `!has_unpack_info`).
///
/// **Output:** `UNPACK_SUCCESS` (0) on success; `UNPACK_FAILURE` (1) on
/// truncated input. We mirror MyRocks's status code rather than `Result` so
/// the hot decode loop avoids allocation; a `Result<(), Error>` wrapper is
/// exposed at the iterator boundary.
///
/// **Errors:** `slatedb::ErrorKind::Data` (via wrapper) if the input bytes
/// don't match the declared field-pack schema.
/// **Invariants:** byte-for-byte identical to MyRocks `decode`.
///
/// Original: rdb_datadic.h:59 — `Rdb_convert_to_record_key_decoder::decode`.
pub fn decode(
    _buf: RowBytes<'_>,
    _offset: &mut u32,
    _fpi: &mut FieldPacking,
    _table: &TableShareView,
    _field: &mut FieldView,
    _has_unpack_info: bool,
    _reader: &mut StringReader,
    _unpack_reader: Option<&mut StringReader>,
) -> i32 {
    todo!("port Rdb_convert_to_record_key_decoder::decode from rdb_datadic.cc")
}

/// Skip one key-part without writing it. Used when only a subset of the
/// index parts are needed (covering-lookup fast path).
///
/// **Inputs:** `fpi` / `field` describe the part to skip; `reader` and
/// `unpack_reader` advance their cursors past it.
/// **Output:** `UNPACK_SUCCESS` (0) on success; `UNPACK_FAILURE` (1) on
/// truncated input.
/// **Errors:** `slatedb::ErrorKind::Data` (via wrapper) on truncation.
///
/// Original: rdb_datadic.h:63 — `Rdb_convert_to_record_key_decoder::skip`.
pub fn skip(
    _fpi: &FieldPacking,
    _field: &FieldView,
    _reader: &mut StringReader,
    _unpack_reader: Option<&mut StringReader>,
) -> i32 {
    todo!("port Rdb_convert_to_record_key_decoder::skip from rdb_datadic.cc")
}

/// Private-in-C++ worker used by `decode`. Exposed `pub(crate)` here so
/// `Rdb_key_field_iterator` can call it directly without re-paying the
/// public-`decode` argument-validation cost.
///
/// **Inputs:** as `decode`, plus `default_value` — a pointer to the field's
/// default value used when the memcmp encoding indicates a NULL.
/// **Output:** `UNPACK_SUCCESS` / `UNPACK_FAILURE`.
/// **Errors:** as `decode`.
///
/// Original: rdb_datadic.h:67 — `Rdb_convert_to_record_key_decoder::decode_field`.
pub(crate) fn decode_field(
    _fpi: &mut FieldPacking,
    _field: &mut FieldView,
    _reader: &mut StringReader,
    _default_value: Option<&[u8]>,
    _unpack_reader: Option<&mut StringReader>,
) -> i32 {
    todo!("port Rdb_convert_to_record_key_decoder::decode_field from rdb_datadic.cc")
}

/// Return code constants (mirroring the C++ enum in rdb_datadic.h:206).
pub const UNPACK_SUCCESS: i32 = 0;
pub const UNPACK_FAILURE: i32 = 1;
