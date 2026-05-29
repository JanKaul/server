//! Interface stub for `Rdb_key_def__decode`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.cc` (lines 1624..2467 + 2530..3005)
//! C++ header: `storage/rocksdb/rdb_datadic.h` (Rdb_key_def decode side)
//! v4 manifest sub-unit: `Rdb_key_def__decode`
//! Parent unit: `rdb_datadic.cc`
//! Approx body LoC accounted for here: ~1300 (unpack_*, skip_*, calc_unpack_*, make_unpack_*)
//!
//! ## Mapping
//! Per _DESIGN.md §2: **memcomparable key decoding is preserved bit-for-bit**
//! from MyRocks. Each `unpack_*` routine is the inverse of the corresponding
//! `pack_*` in `Rdb_key_def__encode`. Each `skip_*` routine fast-forwards a
//! `StringReader` past one packed field without materializing it.
//!
//! `calc_unpack_*` routines compute the on-disk image length of a packed
//! field given its first byte (the length-prefix flag for variable-format
//! varchar). They are pure functions over bytes and translate trivially.
//!
//! `make_unpack_*` routines write the sidechannel "unpack_info" bytes for
//! field types whose memcmp encoding loses original-byte information
//! (e.g. simple-collation case-insensitive varchars). They are paired with
//! the encode side but conceptually belong here because they exist to
//! enable decode.
//!
//! Per _DESIGN.md §3: TTL prefix bytes are NOT consumed at decode time —
//! SlateDB returns `expire_ts` separately on the `KeyValue` / `RowEntry`.
//! Use `Rdb_key_def__meta::extract_ttl_from_row_entry` instead.
//!
//! ## Out-of-scope methods
//! None — every unpack/skip/make_unpack routine has a direct Rust analogue.
//! Methods that route to per-type dispatch tables are listed with the
//! generic dispatch name; the concrete table is populated at setup() time
//! (in `Rdb_key_def__meta`).

use slatedb::Error;

use crate::rdb_buff_h::{StringReader, StringWriter, BitReader};

pub struct FieldPacking; // see Rdb_field_packing stub
pub struct KeyDef;        // see Rdb_key_def__meta stub
pub struct CollationCodec;

/// Unpack a record from packed key + unpack_info into a MySQL `record` buffer.
///
/// Inputs:
/// - `key`: packed memcomparable key bytes (cf_id/index_id prefix already
///   stripped by the caller).
/// - `unpack_info`: sidechannel bytes; may be empty.
/// - `out_record`: caller-allocated `Vec<u8>` sized to the table's record
///   width.
///
/// Output: `Ok(())` on success.
/// Errors: `slatedb::Error::data(...)` for corrupt input; matches MyRocks'
/// `HA_ERR_ROCKSDB_CORRUPT_DATA`.
///
/// C++: rdb_datadic.cc:1624.
pub fn unpack_record(
    _kd: &KeyDef,
    _key: &[u8],
    _unpack_info: &[u8],
    _out_record: &mut [u8],
) -> Result<(), Error> {
    todo!("port C++ unpack_record at rdb_datadic.cc:1624")
}

/// Per-field unpack: integer. Reads `fpi.m_max_image_len` bytes from
/// `reader`, decodes via the sign-flip XOR convention used by MyRocks
/// memcomparable integers.
///
/// C++: rdb_datadic.cc:1898.
pub fn unpack_integer(
    _fpi: &FieldPacking,
    _out: &mut [u8],
    _reader: &mut StringReader,
    _unp_reader: Option<&mut StringReader>,
) -> Result<(), Error> {
    todo!("port C++ unpack_integer at rdb_datadic.cc:1898")
}

/// Per-field unpack: floating-point base routine.
/// C++: rdb_datadic.cc:1968.
pub fn unpack_floating_point(
    _fpi: &FieldPacking,
    _out: &mut [u8],
    _reader: &mut StringReader,
    _unp_reader: Option<&mut StringReader>,
    _is_double: bool,
) -> Result<(), Error> {
    todo!("port C++ unpack_floating_point at rdb_datadic.cc:1968")
}

pub fn unpack_double(
    _fpi: &FieldPacking, _out: &mut [u8],
    _reader: &mut StringReader, _unp_reader: Option<&mut StringReader>,
) -> Result<(), Error> { todo!("rdb_datadic.cc:2030") }

pub fn unpack_float(
    _fpi: &FieldPacking, _out: &mut [u8],
    _reader: &mut StringReader, _unp_reader: Option<&mut StringReader>,
) -> Result<(), Error> { todo!("rdb_datadic.cc:2055") }

pub fn unpack_newdate(
    _fpi: &FieldPacking, _out: &mut [u8],
    _reader: &mut StringReader, _unp_reader: Option<&mut StringReader>,
) -> Result<(), Error> { todo!("rdb_datadic.cc:2072") }

pub fn unpack_binary_str(
    _fpi: &FieldPacking, _out: &mut [u8],
    _reader: &mut StringReader, _unp_reader: Option<&mut StringReader>,
) -> Result<(), Error> { todo!("rdb_datadic.cc:2096") }

pub fn unpack_utf8_str(
    _fpi: &FieldPacking, _out: &mut [u8],
    _reader: &mut StringReader, _unp_reader: Option<&mut StringReader>,
) -> Result<(), Error> { todo!("rdb_datadic.cc:2116") }

/// Unpack varchar — binary or utf8 collation, no padding.
/// C++: rdb_datadic.cc:2530.
pub fn unpack_binary_or_utf8_varchar(
    _fpi: &FieldPacking, _out: &mut [u8],
    _reader: &mut StringReader, _unp_reader: Option<&mut StringReader>,
) -> Result<(), Error> { todo!("rdb_datadic.cc:2530") }

/// Unpack varchar — binary or utf8 collation, space-padded.
/// C++: rdb_datadic.cc:2605.
pub fn unpack_binary_or_utf8_varchar_space_pad(
    _fpi: &FieldPacking, _out: &mut [u8],
    _reader: &mut StringReader, _unp_reader: Option<&mut StringReader>,
) -> Result<(), Error> { todo!("rdb_datadic.cc:2605") }

/// Unpack: simple-collation varchar with space-pad.
/// C++: rdb_datadic.cc:2874.
pub fn unpack_simple_varchar_space_pad(
    _fpi: &FieldPacking, _out: &mut [u8],
    _reader: &mut StringReader, _unp_reader: Option<&mut StringReader>,
) -> Result<(), Error> { todo!("rdb_datadic.cc:2874") }

/// Unpack: simple-collation fixed-width string.
/// C++: rdb_datadic.cc:2989.
pub fn unpack_simple(
    _fpi: &FieldPacking, _out: &mut [u8],
    _reader: &mut StringReader, _unp_reader: Option<&mut StringReader>,
) -> Result<(), Error> { todo!("rdb_datadic.cc:2989") }

/// Unpack: type whose original bytes are entirely in unpack_info.
/// C++: rdb_datadic.cc:2737.
pub fn unpack_unknown(
    _fpi: &FieldPacking, _out: &mut [u8],
    _reader: &mut StringReader, _unp_reader: Option<&mut StringReader>,
) -> Result<(), Error> { todo!("rdb_datadic.cc:2737") }

/// Unpack: varchar whose original bytes are entirely in unpack_info.
/// C++: rdb_datadic.cc:2784.
pub fn unpack_unknown_varchar(
    _fpi: &FieldPacking, _out: &mut [u8],
    _reader: &mut StringReader, _unp_reader: Option<&mut StringReader>,
) -> Result<(), Error> { todo!("rdb_datadic.cc:2784") }

// --- skip_* family (fast-forward past one packed field) ---

pub fn skip_max_length(
    _fpi: &FieldPacking, _reader: &mut StringReader,
) -> Result<(), Error> { todo!("rdb_datadic.cc:1768") }

pub fn skip_variable_length(
    _fpi: &FieldPacking, _reader: &mut StringReader,
) -> Result<(), Error> { todo!("rdb_datadic.cc:1798") }

pub fn skip_variable_space_pad(
    _fpi: &FieldPacking, _reader: &mut StringReader,
) -> Result<(), Error> { todo!("rdb_datadic.cc:1854") }

// --- calc_unpack_* family (pure header-byte arithmetic) ---

pub fn calc_unpack_legacy_variable_format(flag: u8) -> (u32, bool) {
    let _ = flag;
    todo!("rdb_datadic.cc:2453 — returns (length, done)")
}

pub fn calc_unpack_variable_format(flag: u8) -> (u32, bool) {
    let _ = flag;
    todo!("rdb_datadic.cc:2469 — returns (length, done)")
}

// --- make_unpack_* family (sidechannel writers) ---

pub fn make_unpack_unknown(
    _fpi: &FieldPacking, _field_value: &[u8], _writer: &mut StringWriter,
) { todo!("rdb_datadic.cc:2712") }

pub fn dummy_make_unpack_info(
    _fpi: &FieldPacking, _field_value: &[u8], _writer: &mut StringWriter,
) { todo!("rdb_datadic.cc:2726") }

pub fn make_unpack_unknown_varchar(
    _fpi: &FieldPacking, _field_value: &[u8], _writer: &mut StringWriter,
) { todo!("rdb_datadic.cc:2761") }

pub fn make_unpack_simple_varchar(
    _codec: &CollationCodec, _field_value: &[u8], _writer: &mut StringWriter,
) { todo!("rdb_datadic.cc:2852") }

pub fn make_unpack_simple(
    _codec: &CollationCodec, _field_value: &[u8], _writer: &mut StringWriter,
) { todo!("rdb_datadic.cc:2977") }

/// Read the unpack-simple sidechannel back into original-byte form.
/// C++ free fn: rdb_datadic.cc:2824.
pub fn read_unpack_simple(
    _reader: &mut BitReader,
    _codec: &CollationCodec,
    _out: &mut [u8],
) -> Result<u32, Error> {
    todo!("rdb_datadic.cc:2824 — returns unpacked byte count")
}
