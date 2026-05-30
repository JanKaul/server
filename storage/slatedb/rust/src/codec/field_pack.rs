//! Per-keypart pack/unpack vtable.
//!
//! Translated from the cluster of types around `Rdb_field_packing`
//! (`storage/rocksdb/rdb_datadic.h:912..1009`). Bundles:
//! - the four function-pointer slots (pack / make_unpack_info / unpack /
//!   skip) that the codec dispatches on,
//! - [`PackFieldContext`] (the unpack_info writer threaded across the
//!   pack/make_unpack call pair),
//! - [`CollationCodec`] (per-collation precomputed encode/decode tables),
//! - [`FieldPacking`] (the per-keypart descriptor owned by a `KeyDef`),
//! - the `UNPACK_*` return-code constants.
//!
//! **This batch is shape-only.** `FieldPacking::setup` (the big switch over
//! the MySQL field type that populates the four function-pointer slots)
//! and `get_field_in_table` (which needs a `KeyDef → keynr → table_share`
//! lookup) land when `codec::key` materialises — they need cross-module
//! context that doesn't exist yet.

use std::sync::Arc;

use crate::codec::value::FieldView;
use crate::utils::buff::{StringReader, StringWriter};

// --- return-code constants (rdb_datadic.h:206) ---

pub const UNPACK_SUCCESS: i32 = 0;
pub const UNPACK_FAILURE: i32 = 1;

// --- function-pointer aliases ---
//
// `fn`-pointer (not boxed closures) is faithful to the C++ — the dispatch
// table is static and stateless. Each slot is `Option<fn(...)>` on
// FieldPacking so "not applicable for this field type" is type-distinct
// from "applicable but errored."

/// Pack one keypart from the row buffer into the mem-comparable image.
/// Mirrors `rdb_index_field_pack_t` (`rdb_datadic.h:155`).
pub type IndexFieldPackFn = fn(
    fpi: &mut FieldPacking,
    field: &mut FieldView,
    buf: &mut [u8],
    dst: &mut Vec<u8>,
    pack_ctx: &mut PackFieldContext<'_>,
);

/// Advance a reader past one keypart without writing it (covering-lookup
/// fast path). Mirrors `rdb_index_field_skip_t` (`rdb_datadic.h:152`).
pub type IndexFieldSkipFn = fn(
    fpi: &FieldPacking,
    field: &FieldView,
    reader: &mut StringReader,
) -> i32;

/// Emit unpack_info bytes for one field. Mirrors `rdb_make_unpack_info_t`
/// (`rdb_datadic.h:145`).
pub type MakeUnpackInfoFn = fn(
    codec: &CollationCodec,
    field: &FieldView,
    pack_ctx: &mut PackFieldContext<'_>,
);

/// Decode one packed memcmp field into the record buffer. Mirrors
/// `rdb_index_field_unpack_t` (`rdb_datadic.h:148`).
pub type IndexFieldUnpackFn = fn(
    fpi: &mut FieldPacking,
    field: &mut FieldView,
    field_ptr: &mut [u8],
    reader: &mut StringReader,
    unpack_reader: Option<&mut StringReader>,
) -> i32;

// --- PackFieldContext ---

/// Threads the unpack_info writer across the pack / make_unpack call pair.
/// Stack-only; never stored long-lived.
///
/// `writer = None` ⇒ caller is not producing unpack_info for this index
/// (covering reads disabled or unsupported).
pub struct PackFieldContext<'w> {
    pub writer: Option<&'w mut StringWriter>,
}

impl<'w> PackFieldContext<'w> {
    pub fn new(writer: Option<&'w mut StringWriter>) -> Self {
        Self { writer }
    }

    pub fn has_writer(&self) -> bool {
        self.writer.is_some()
    }
}

// --- CollationCodec ---

/// Per-"simple" collation pack/unpack table. Each source byte maps to one
/// destination byte via `strnxfrm`; because that mapping is not injective,
/// decode needs `dec_idx` to recover the original byte.
///
/// The encoded bytes are preserved bit-for-bit from MyRocks (per
/// `_DESIGN.md §2`), so the tables themselves are loaded from the MariaDB
/// `CHARSET_INFO` registry at startup; we only carry the precomputed form.
pub struct CollationCodec {
    /// MariaDB charset id (`CHARSET_INFO::number`).
    pub charset_id: u32,

    /// `[VARCHAR(n), CHAR(n)]` make-unpack-info routines.
    pub make_unpack_info_func: [MakeUnpackInfoFn; 2],
    /// `[VARCHAR(n), CHAR(n)]` unpack routines.
    pub unpack_func: [IndexFieldUnpackFn; 2],

    /// `src_byte → idx` table written into the sidechannel during encode.
    pub enc_idx: [u8; 256],
    /// `src_byte → encoded-form length in bytes`.
    pub enc_size: [u8; 256],

    /// `idx → decoded-length` lookup used during decode.
    pub dec_size: [u8; 256],
    /// `dec_idx[idx][packed_byte] → original_byte`. Variable outer length
    /// because the number of disambiguating indices is collation-dependent.
    pub dec_idx: Vec<[u8; 256]>,
}

/// Mutex guarding lazy `CollationCodec` slot insertions.
/// Original: `rdb_collation_data_mutex` (`rdb_datadic.h:907`).
pub static COLLATION_DATA_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

// --- FieldPacking ---

/// Per-keypart descriptor. One instance per (index, key-part) pair, owned
/// by the enclosing `KeyDef`. The codec dispatches on
/// `pack_func`/`unpack_func`/`skip_func` to pick the encoding routine.
///
/// Original: `rdb_datadic.h:912` — `class Rdb_field_packing`.
#[derive(Default)]
pub struct FieldPacking {
    /// Length of the mem-comparable image of the field, in bytes.
    pub max_image_len: i32,
    /// Length of the unpack-info image for this field, in bytes.
    pub unpack_data_len: i32,
    /// Offset within the per-row unpack_info blob where this field's bytes
    /// begin (set by `KeyDef::setup` when that lands).
    pub unpack_data_offset: i32,

    /// True iff the field has a stored NULL-byte.
    pub maybe_null: bool,

    /// VARCHAR-only charset id (`None` for non-VARCHAR).
    pub varchar_charset: Option<u32>,
    /// True iff the field uses the pre-`PRIMARY_FORMAT_VERSION_UPDATE2`
    /// binary variable-length encoding (the old multiple-of-8 quirk).
    pub use_legacy_varbinary_format: bool,

    /// VARCHAR + space-pad encoding: bytes per segment.
    pub segment_size: u32,

    /// True ⇒ unpack_info uses 2 bytes for the trimmed-spaces count;
    /// false ⇒ 1 byte.
    pub unpack_info_uses_two_bytes: bool,

    /// True ⇒ index-only read is always possible for this field. False ⇒
    /// depends on per-record content.
    pub covered: bool,

    /// Lazily-initialised space-padding transform bytes (the charset's
    /// mem-cmp image of one space character). `None` until first observed.
    pub space_xfrm: Option<&'static Vec<u8>>,
    pub space_xfrm_len: usize,
    pub space_mb_len: usize,

    /// Per-charset codec table (lives in the future `CollationDataTable`).
    /// `None` for non-simple-collation fields.
    pub charset_codec: Option<Arc<CollationCodec>>,

    /// True iff the encoded image is followed by a non-empty unpack_info
    /// block (depends on the field's pack routine).
    pub unpack_info_stores_value: bool,

    /// Pack / make-unpack / unpack / skip routine slots. `None` means the
    /// dispatch is not applicable for this field's type — e.g. fixed-width
    /// integers don't produce unpack_info, so `make_unpack_info_func` is
    /// `None`.
    pub pack_func: Option<IndexFieldPackFn>,
    pub make_unpack_info_func: Option<MakeUnpackInfoFn>,
    pub unpack_func: Option<IndexFieldUnpackFn>,
    pub skip_func: Option<IndexFieldSkipFn>,

    /// Index number this field belongs to (for extended-keys disambiguation).
    pub(crate) keynr: u32,
    /// Position of this field within the key (0-based).
    pub(crate) key_part: u32,
}

impl FieldPacking {
    /// True iff this field's encoding emits any unpack_info bytes.
    pub fn uses_unpack_info(&self) -> bool {
        self.make_unpack_info_func.is_some()
    }

    /// Write the hidden-PK value (big-endian `u64`) at
    /// `dst[*dst_offset..*dst_offset+8]` and advance the offset by 8.
    ///
    /// Panics if `dst` is too short (caller is responsible for sizing).
    pub fn fill_hidden_pk_val(
        &self,
        dst: &mut [u8],
        dst_offset: &mut usize,
        hidden_pk_id: i64,
    ) {
        let id_bytes = (hidden_pk_id as u64).to_be_bytes();
        dst[*dst_offset..*dst_offset + 8].copy_from_slice(&id_bytes);
        *dst_offset += 8;
    }

    pub fn keynr(&self) -> u32 {
        self.keynr
    }
    pub fn key_part(&self) -> u32 {
        self.key_part
    }
}

/// Mutex protecting [`FieldPacking::space_xfrm`] lazy initialisation.
/// Original: `rdb_mem_cmp_space_mutex` (`rdb_datadic.h:908`).
pub static MEM_CMP_SPACE_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Convert MyRocks's `(true, false)` UNPACK return code to a Rust `Result`
/// at the iterator boundary.
pub fn unpack_status_to_result(code: i32) -> Result<(), slatedb::Error> {
    match code {
        UNPACK_SUCCESS => Ok(()),
        UNPACK_FAILURE => Err(slatedb::Error::data(
            "memcomparable unpack failed".into(),
        )),
        _ => Err(slatedb::Error::internal(format!(
            "unexpected unpack code {code}"
        ))),
    }
}

// ===========================================================================
// Pack/unpack/skip free functions
// ===========================================================================
//
// Translated from `rdb_datadic.cc:1768..2108` (and the helpers around line
// 2453). These are the concrete encode/decode routines that
// [`FieldPacking::setup`] wires into the function-pointer slots based on
// the column's MySQL type and collation.
//
// They take `&mut FieldPacking` / `&mut FieldView` to match the C++
// `Rdb_field_packing*` / `Field*` shape even when no mutation happens;
// keeps the function-pointer typedef stable across all variants.

/// Variable-length-string segment size. Each segment is `RDB_ESCAPE_LENGTH`
/// bytes: `RDB_ESCAPE_LENGTH - 1` data bytes + one trailing flag byte that
/// tells the decoder how many of those data bytes are real and whether
/// more segments follow.
///
/// Translated from `rdb_datadic.cc:1781` — `#define RDB_ESCAPE_LENGTH 9`.
pub const RDB_ESCAPE_LENGTH: usize = 9;

/// Variable-length-space-padded flag values (`rdb_datadic.cc:1846`).
pub const VARCHAR_CMP_LESS_THAN_SPACES: u8 = 1;
pub const VARCHAR_CMP_EQUAL_TO_SPACES: u8 = 2;
pub const VARCHAR_CMP_GREATER_THAN_SPACES: u8 = 3;

// ----- skip functions -----

/// Skip a fixed-width keypart. Just advance `reader` by `max_image_len`
/// bytes. Used for integers, dates, BINARY(n), and CHAR(n) with any
/// collation (since CHAR is space-padded to its full length at the SQL
/// layer).
///
/// Translated from `Rdb_key_def::skip_max_length` (`rdb_datadic.cc:1768`).
pub fn skip_max_length(
    fpi: &FieldPacking,
    _field: &FieldView,
    reader: &mut StringReader,
) -> i32 {
    if reader.read(fpi.max_image_len as usize).is_some() {
        UNPACK_SUCCESS
    } else {
        UNPACK_FAILURE
    }
}

/// Skip a variable-length keypart packed by `pack_with_varchar_encoding`.
/// Reads `RDB_ESCAPE_LENGTH`-byte segments, peeks the trailing flag byte
/// to count payload bytes, and stops when the flag marks the final
/// segment. `field.length` caps the cumulative payload (the max VARCHAR
/// content size in bytes) so a malformed stream can't run away.
///
/// Translated from `Rdb_key_def::skip_variable_length` (`rdb_datadic.cc:1798`).
pub fn skip_variable_length(
    fpi: &FieldPacking,
    field: &FieldView,
    reader: &mut StringReader,
) -> i32 {
    // `field.length` is the declared max content length (matches MariaDB
    // `Field_varstring::pack_length() - length_bytes`). Use `usize::MAX`
    // when no field is in scope (matches C++'s `dst_len = UINT_MAX`
    // branch when `field == nullptr`); we don't model `field = None`
    // here so length is always declared.
    let mut dst_len = field.length as usize;
    let use_legacy = fpi.use_legacy_varbinary_format;

    loop {
        let chunk = match reader.read(RDB_ESCAPE_LENGTH) {
            Some(c) => c,
            None => return UNPACK_FAILURE,
        };
        let flag = chunk[RDB_ESCAPE_LENGTH - 1];
        let (used_bytes, finished) = if use_legacy {
            calc_unpack_legacy_variable_format(flag)
        } else {
            calc_unpack_variable_format(flag)
        };

        let used = match used_bytes {
            Some(n) => n as usize,
            None => return UNPACK_FAILURE,
        };
        if dst_len < used {
            return UNPACK_FAILURE;
        }
        if finished {
            return UNPACK_SUCCESS;
        }
        dst_len -= used;
    }
}

/// Skip a variable-length-space-padded keypart packed by
/// `pack_with_varchar_space_pad`. Reads `segment_size`-byte chunks; the
/// trailing byte of each chunk is a `VARCHAR_CMP_*` flag.
/// `EQUAL_TO_SPACES` ends the field; `LESS_THAN` / `GREATER_THAN` means
/// another chunk follows; anything else is corruption.
///
/// Translated from `Rdb_key_def::skip_variable_space_pad` (`rdb_datadic.cc:1854`).
pub fn skip_variable_space_pad(
    fpi: &FieldPacking,
    field: &FieldView,
    reader: &mut StringReader,
) -> i32 {
    let segment_size = fpi.segment_size as usize;
    if segment_size == 0 {
        return UNPACK_FAILURE;
    }
    let mut dst_len = field.length as usize;
    let data_per_segment = segment_size - 1;

    loop {
        let chunk = match reader.read(segment_size) {
            Some(c) => c,
            None => return UNPACK_FAILURE,
        };
        let flag = chunk[segment_size - 1];
        match flag {
            VARCHAR_CMP_EQUAL_TO_SPACES => return UNPACK_SUCCESS,
            VARCHAR_CMP_LESS_THAN_SPACES | VARCHAR_CMP_GREATER_THAN_SPACES => {
                if data_per_segment > dst_len {
                    return UNPACK_FAILURE;
                }
                dst_len -= data_per_segment;
            }
            _ => return UNPACK_FAILURE,
        }
    }
}

// ----- variable-format flag decoders -----

/// New format (`PRIMARY_FORMAT_VERSION_UPDATE2` and later) flag byte:
/// values `1..=RDB_ESCAPE_LENGTH-1` end the field and indicate that many
/// of the chunk's payload bytes are real; `RDB_ESCAPE_LENGTH` means full
/// payload and more chunks follow; anything else is corruption.
/// Returns `(Some(used), finished)` on success, `(None, _)` on
/// corruption.
///
/// Translated from `Rdb_key_def::calc_unpack_variable_format`
/// (`rdb_datadic.cc:2469`).
pub fn calc_unpack_variable_format(flag: u8) -> (Option<u32>, bool) {
    if flag as usize > RDB_ESCAPE_LENGTH {
        return (None, false);
    }
    if (flag as usize) < RDB_ESCAPE_LENGTH {
        return (Some(flag as u32), true);
    }
    (Some((RDB_ESCAPE_LENGTH - 1) as u32), false)
}

/// Legacy format (pre-`PRIMARY_FORMAT_VERSION_UPDATE2`) flag byte: pad
/// count is `255 - flag`; payload bytes = `RDB_ESCAPE_LENGTH-1 - pad`.
/// Finished if fewer than full payload bytes were used.
///
/// Translated from `Rdb_key_def::calc_unpack_legacy_variable_format`
/// (`rdb_datadic.cc:2453`).
pub fn calc_unpack_legacy_variable_format(flag: u8) -> (Option<u32>, bool) {
    let pad = 255_u32.saturating_sub(flag as u32);
    let max_payload = (RDB_ESCAPE_LENGTH - 1) as u32;
    if pad > max_payload {
        return (None, false);
    }
    let used_bytes = max_payload - pad;
    (Some(used_bytes), used_bytes < max_payload)
}

// ----- unpack functions -----

/// Unpack a fixed-width integer (TINY / SHORT / INT24 / LONG / LONGLONG).
/// The encoded image is the host integer in big-endian with the sign bit
/// flipped (for signed types). We byte-reverse back to little-endian (our
/// only target) and undo the sign flip.
///
/// Translated from `Rdb_key_def::unpack_integer` (`rdb_datadic.cc:1898`).
/// The MSAN-guarded big-endian branch is dropped: MariaDB on big-endian
/// is not a supported target for SlateDB today.
pub fn unpack_integer(
    fpi: &mut FieldPacking,
    field: &mut FieldView,
    field_ptr: &mut [u8],
    reader: &mut StringReader,
    _unpack_reader: Option<&mut StringReader>,
) -> i32 {
    let length = fpi.max_image_len as usize;
    let from = match reader.read(length) {
        Some(s) => s,
        None => return UNPACK_FAILURE,
    };
    if field_ptr.len() < length {
        return UNPACK_FAILURE;
    }

    // Little-endian host: the MSB of the host integer is at index
    // `length - 1`. The memcmp image stores MSB at index 0 with the
    // sign bit flipped for signed types.
    let sign_byte = from[0];
    let signed =
        (field.flags & crate::codec::value::UNSIGNED_FLAG) == 0;
    field_ptr[length - 1] = if signed { sign_byte ^ 0x80 } else { sign_byte };
    for (i, j) in (0..length - 1).zip((1..length).rev()) {
        field_ptr[i] = from[j];
    }
    UNPACK_SUCCESS
}

/// Unpack a `double`. Reverses `change_double_for_sort`
/// (`sql/filesort.cc`). Assumes IEEE 754 and that NaN / ±Inf were never
/// persisted (the C++ assumes the same).
///
/// Translated from `Rdb_key_def::unpack_double` (`rdb_datadic.cc:2030`).
pub fn unpack_double(
    _fpi: &mut FieldPacking,
    _field: &mut FieldView,
    field_ptr: &mut [u8],
    reader: &mut StringReader,
    _unpack_reader: Option<&mut StringReader>,
) -> i32 {
    const ZERO_PATTERN: &[u8] = &[128, 0, 0, 0, 0, 0, 0, 0];
    const ZERO_VAL: &[u8] = &[0u8; 8]; // IEEE 754: +0.0 is all-zero bits.
    // f64::MANTISSA_DIGITS = 53 ⇒ exponent bits = 64 - 53 = 11.
    unpack_floating_point(field_ptr, reader, 8, 11, ZERO_PATTERN, ZERO_VAL)
}

/// Unpack a `float`. Same approach as [`unpack_double`].
///
/// Translated from `Rdb_key_def::unpack_float` (`rdb_datadic.cc:2055`).
pub fn unpack_float(
    _fpi: &mut FieldPacking,
    _field: &mut FieldView,
    field_ptr: &mut [u8],
    reader: &mut StringReader,
    _unpack_reader: Option<&mut StringReader>,
) -> i32 {
    const ZERO_PATTERN: &[u8] = &[128, 0, 0, 0];
    const ZERO_VAL: &[u8] = &[0u8; 4];
    // f32::MANTISSA_DIGITS = 24 ⇒ exponent bits = 32 - 24 = 8.
    unpack_floating_point(field_ptr, reader, 4, 8, ZERO_PATTERN, ZERO_VAL)
}

/// Reverse of `change_double_for_sort` (size-generic IEEE float unpack).
/// Reads `size` bytes from `reader`, decodes them into `dst[..size]` in
/// host byte order. Called by [`unpack_double`] and [`unpack_float`].
fn unpack_floating_point(
    dst: &mut [u8],
    reader: &mut StringReader,
    size: usize,
    exp_digit: u32,
    zero_pattern: &[u8],
    zero_val: &[u8],
) -> i32 {
    let from = match reader.read(size) {
        Some(s) => s,
        None => return UNPACK_FAILURE,
    };
    if dst.len() < size {
        return UNPACK_FAILURE;
    }
    if from == zero_pattern {
        dst[..size].copy_from_slice(zero_val);
        return UNPACK_SUCCESS;
    }

    // Build the unswapped image into `tmp`, then byte-reverse into `dst`.
    let mut tmp = [0u8; 8]; // big enough for both f32 (4) and f64 (8)
    tmp[..size].copy_from_slice(from);
    if tmp[0] & 0x80 != 0 {
        // Original value was positive: clear the high bit and subtract
        // from the 2-byte exponent prefix.
        let mut exp_part = ((tmp[0] as u16) << 8) | tmp[1] as u16;
        exp_part &= 0x7FFF;
        exp_part = exp_part.wrapping_sub(1u16 << (16 - 1 - exp_digit));
        tmp[0] = (exp_part >> 8) as u8;
        tmp[1] = exp_part as u8;
    } else {
        // Original value was negative: every byte was complemented.
        for b in &mut tmp[..size] {
            *b ^= 0xFF;
        }
    }

    // Little-endian host: reverse `tmp[..size]` into `dst[..size]`.
    for i in 0..size {
        dst[i] = tmp[size - 1 - i];
    }
    UNPACK_SUCCESS
}

/// Unpack a `NEWDATE` (3-byte packed date). The encoded form swaps the
/// byte order for memcmp friendliness; decoding reverses the bytes.
///
/// Translated from `Rdb_key_def::unpack_newdate` (`rdb_datadic.cc:2072`).
pub fn unpack_newdate(
    fpi: &mut FieldPacking,
    _field: &mut FieldView,
    field_ptr: &mut [u8],
    reader: &mut StringReader,
    _unpack_reader: Option<&mut StringReader>,
) -> i32 {
    debug_assert_eq!(fpi.max_image_len, 3, "NEWDATE max_image_len must be 3");
    let from = match reader.read(3) {
        Some(s) => s,
        None => return UNPACK_FAILURE,
    };
    if field_ptr.len() < 3 {
        return UNPACK_FAILURE;
    }
    field_ptr[0] = from[2];
    field_ptr[1] = from[1];
    field_ptr[2] = from[0];
    UNPACK_SUCCESS
}

/// Unpack a fixed-width binary string by copying it over. Used for
/// `BINARY(n)` and `CHAR(n)` under `_bin` collations where the
/// mem-comparable form is the string itself.
///
/// Translated from `Rdb_key_def::unpack_binary_str` (`rdb_datadic.cc:2096`).
pub fn unpack_binary_str(
    fpi: &mut FieldPacking,
    _field: &mut FieldView,
    field_ptr: &mut [u8],
    reader: &mut StringReader,
    _unpack_reader: Option<&mut StringReader>,
) -> i32 {
    let length = fpi.max_image_len as usize;
    let from = match reader.read(length) {
        Some(s) => s,
        None => return UNPACK_FAILURE,
    };
    if field_ptr.len() < length {
        return UNPACK_FAILURE;
    }
    field_ptr[..length].copy_from_slice(from);
    UNPACK_SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_field_packing_has_no_dispatch_slots() {
        let fp = FieldPacking::default();
        assert!(!fp.uses_unpack_info());
        assert!(fp.pack_func.is_none());
        assert!(fp.unpack_func.is_none());
        assert!(fp.skip_func.is_none());
        assert!(fp.make_unpack_info_func.is_none());
        assert_eq!(fp.max_image_len, 0);
        assert!(!fp.maybe_null);
    }

    #[test]
    fn uses_unpack_info_is_driven_by_make_unpack_info_slot() {
        fn dummy_make_unpack(_c: &CollationCodec, _f: &FieldView, _ctx: &mut PackFieldContext<'_>) {
        }
        let mut fp = FieldPacking::default();
        assert!(!fp.uses_unpack_info());
        fp.make_unpack_info_func = Some(dummy_make_unpack);
        assert!(fp.uses_unpack_info());
    }

    #[test]
    fn fill_hidden_pk_val_writes_be_u64_and_advances_offset() {
        let fp = FieldPacking::default();
        let mut buf = vec![0u8; 12];
        let mut off = 2;
        fp.fill_hidden_pk_val(&mut buf, &mut off, 0x0102_0304_0506_0708);
        assert_eq!(off, 10);
        assert_eq!(
            &buf[2..10],
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
        );
        // Surrounding bytes untouched.
        assert_eq!(buf[0], 0);
        assert_eq!(buf[1], 0);
        assert_eq!(buf[10], 0);
        assert_eq!(buf[11], 0);
    }

    #[test]
    fn fill_hidden_pk_val_round_trips_negative_id_via_unsigned_cast() {
        // The C++ stores the bit pattern as u64. -1 → 0xFFFF_FFFF_FFFF_FFFF.
        let fp = FieldPacking::default();
        let mut buf = vec![0u8; 8];
        let mut off = 0;
        fp.fill_hidden_pk_val(&mut buf, &mut off, -1);
        assert_eq!(&buf[..], &[0xff; 8]);
    }

    #[test]
    fn pack_field_context_tracks_writer_presence() {
        let mut w = StringWriter::new();
        let ctx_some = PackFieldContext::new(Some(&mut w));
        assert!(ctx_some.has_writer());
        drop(ctx_some);

        let ctx_none = PackFieldContext::new(None);
        assert!(!ctx_none.has_writer());
    }

    #[test]
    fn unpack_status_maps_to_result() {
        assert!(unpack_status_to_result(UNPACK_SUCCESS).is_ok());
        let e = unpack_status_to_result(UNPACK_FAILURE).unwrap_err();
        assert!(matches!(e.kind(), slatedb::ErrorKind::Data));
        let e = unpack_status_to_result(42).unwrap_err();
        assert!(matches!(e.kind(), slatedb::ErrorKind::Internal));
    }

    // ----- skip + variable-format helpers -----

    use crate::codec::value::MysqlType;

    fn varchar_field(len: u32) -> FieldView {
        FieldView {
            name: "v".into(),
            mysql_type: MysqlType::Varchar,
            pack_length: len + 1,
            output_offset: 0,
            null_marker: None,
            length: len,
            charset_id: 63,
            flags: 0,
            decimals: 0,
        }
    }

    fn int_field() -> FieldView {
        FieldView {
            name: "i".into(),
            mysql_type: MysqlType::Long,
            pack_length: 4,
            output_offset: 0,
            null_marker: None,
            length: 4,
            charset_id: 63,
            flags: 0,
            decimals: 0,
        }
    }

    #[test]
    fn skip_max_length_advances_exactly_max_image_len() {
        let mut fpi = FieldPacking::default();
        fpi.max_image_len = 4;
        let field = int_field();
        let buf = [0u8; 10];
        let mut r = StringReader::new(&buf);
        assert_eq!(skip_max_length(&fpi, &field, &mut r), UNPACK_SUCCESS);
        assert_eq!(r.current_pos(), 4);
    }

    #[test]
    fn skip_max_length_short_read_fails() {
        let mut fpi = FieldPacking::default();
        fpi.max_image_len = 8;
        let field = int_field();
        let buf = [0u8; 3];
        let mut r = StringReader::new(&buf);
        assert_eq!(skip_max_length(&fpi, &field, &mut r), UNPACK_FAILURE);
    }

    #[test]
    fn calc_unpack_variable_format_table() {
        // Terminal segments (1..=8 used bytes, finished=true).
        for used in 1..=(RDB_ESCAPE_LENGTH - 1) as u32 {
            let (n, done) = calc_unpack_variable_format(used as u8);
            assert_eq!(n, Some(used));
            assert!(done, "flag={used} must mark finished");
        }
        // Full-payload-with-more-to-come (flag == RDB_ESCAPE_LENGTH).
        let (n, done) = calc_unpack_variable_format(RDB_ESCAPE_LENGTH as u8);
        assert_eq!(n, Some((RDB_ESCAPE_LENGTH - 1) as u32));
        assert!(!done);
        // Invalid flag > RDB_ESCAPE_LENGTH.
        let (n, _) = calc_unpack_variable_format(RDB_ESCAPE_LENGTH as u8 + 1);
        assert!(n.is_none());
        // Flag = 0 — current C++ logic returns Some(0), finished=true
        // (you've written zero payload bytes, this segment ends the field).
        let (n, done) = calc_unpack_variable_format(0);
        assert_eq!(n, Some(0));
        assert!(done);
    }

    #[test]
    fn calc_unpack_legacy_variable_format_table() {
        // pad = 0 (flag = 255) → full payload, more to come.
        let (n, done) = calc_unpack_legacy_variable_format(255);
        assert_eq!(n, Some((RDB_ESCAPE_LENGTH - 1) as u32));
        assert!(!done);
        // pad = 1 (flag = 254) → 7 payload bytes, finished.
        let (n, done) = calc_unpack_legacy_variable_format(254);
        assert_eq!(n, Some((RDB_ESCAPE_LENGTH - 2) as u32));
        assert!(done);
        // pad = RDB_ESCAPE_LENGTH-1 = 8 (flag = 247) → 0 bytes, finished.
        let (n, done) = calc_unpack_legacy_variable_format(247);
        assert_eq!(n, Some(0));
        assert!(done);
        // pad > max payload (flag < 247) → corruption.
        let (n, _) = calc_unpack_legacy_variable_format(246);
        assert!(n.is_none());
    }

    #[test]
    fn skip_variable_length_consumes_one_terminal_segment() {
        let fpi = FieldPacking::default();
        let field = varchar_field(100);
        // One segment with flag=3 ⇒ 3 payload bytes, finished.
        let mut buf = vec![0u8; RDB_ESCAPE_LENGTH];
        buf[RDB_ESCAPE_LENGTH - 1] = 3;
        let mut r = StringReader::new(&buf);
        assert_eq!(skip_variable_length(&fpi, &field, &mut r), UNPACK_SUCCESS);
        assert_eq!(r.current_pos(), RDB_ESCAPE_LENGTH);
    }

    #[test]
    fn skip_variable_length_consumes_multi_segment() {
        let fpi = FieldPacking::default();
        let field = varchar_field(100);
        // Two segments: first full-payload+more (flag=RDB_ESCAPE_LENGTH=9),
        // second terminal (flag=2).
        let mut buf = vec![0u8; RDB_ESCAPE_LENGTH * 2];
        buf[RDB_ESCAPE_LENGTH - 1] = RDB_ESCAPE_LENGTH as u8;
        buf[RDB_ESCAPE_LENGTH * 2 - 1] = 2;
        let mut r = StringReader::new(&buf);
        assert_eq!(skip_variable_length(&fpi, &field, &mut r), UNPACK_SUCCESS);
        assert_eq!(r.current_pos(), RDB_ESCAPE_LENGTH * 2);
    }

    #[test]
    fn skip_variable_length_caps_at_field_length() {
        // field.length = 5 but two full-payload segments (8+more) would be
        // 16 bytes — that exceeds 5 and must be rejected.
        let fpi = FieldPacking::default();
        let field = varchar_field(5);
        let mut buf = vec![0u8; RDB_ESCAPE_LENGTH * 2];
        buf[RDB_ESCAPE_LENGTH - 1] = RDB_ESCAPE_LENGTH as u8;
        buf[RDB_ESCAPE_LENGTH * 2 - 1] = 2;
        let mut r = StringReader::new(&buf);
        assert_eq!(skip_variable_length(&fpi, &field, &mut r), UNPACK_FAILURE);
    }

    #[test]
    fn skip_variable_length_rejects_bad_flag() {
        let fpi = FieldPacking::default();
        let field = varchar_field(100);
        let mut buf = vec![0u8; RDB_ESCAPE_LENGTH];
        buf[RDB_ESCAPE_LENGTH - 1] = (RDB_ESCAPE_LENGTH + 1) as u8; // bad
        let mut r = StringReader::new(&buf);
        assert_eq!(skip_variable_length(&fpi, &field, &mut r), UNPACK_FAILURE);
    }

    #[test]
    fn skip_variable_space_pad_consumes_one_terminal_segment() {
        let mut fpi = FieldPacking::default();
        fpi.segment_size = 9;
        let field = varchar_field(100);
        let mut buf = vec![0u8; 9];
        buf[8] = VARCHAR_CMP_EQUAL_TO_SPACES;
        let mut r = StringReader::new(&buf);
        assert_eq!(skip_variable_space_pad(&fpi, &field, &mut r), UNPACK_SUCCESS);
        assert_eq!(r.current_pos(), 9);
    }

    #[test]
    fn skip_variable_space_pad_consumes_multi_segment() {
        let mut fpi = FieldPacking::default();
        fpi.segment_size = 9;
        let field = varchar_field(100);
        let mut buf = vec![0u8; 18];
        buf[8] = VARCHAR_CMP_LESS_THAN_SPACES;
        buf[17] = VARCHAR_CMP_EQUAL_TO_SPACES;
        let mut r = StringReader::new(&buf);
        assert_eq!(skip_variable_space_pad(&fpi, &field, &mut r), UNPACK_SUCCESS);
        assert_eq!(r.current_pos(), 18);
    }

    #[test]
    fn skip_variable_space_pad_rejects_unknown_flag() {
        let mut fpi = FieldPacking::default();
        fpi.segment_size = 9;
        let field = varchar_field(100);
        let mut buf = vec![0u8; 9];
        buf[8] = 42; // not a VARCHAR_CMP_* value
        let mut r = StringReader::new(&buf);
        assert_eq!(skip_variable_space_pad(&fpi, &field, &mut r), UNPACK_FAILURE);
    }

    // ----- unpack functions -----

    use crate::codec::value::UNSIGNED_FLAG;

    fn signed_int_field(pack_len: u32) -> FieldView {
        FieldView {
            name: "i".into(),
            mysql_type: MysqlType::Long,
            pack_length: pack_len,
            output_offset: 0,
            null_marker: None,
            length: pack_len,
            charset_id: 63,
            flags: 0,
            decimals: 0,
        }
    }

    fn unsigned_int_field(pack_len: u32) -> FieldView {
        let mut f = signed_int_field(pack_len);
        f.flags = UNSIGNED_FLAG;
        f
    }

    #[test]
    fn unpack_integer_signed_positive_round_trip() {
        // 32-bit signed +1 in LE memory: [1, 0, 0, 0]. Memcmp image is
        // BE with sign bit flipped: [0x80, 0, 0, 1].
        let mut fpi = FieldPacking::default();
        fpi.max_image_len = 4;
        let mut field = signed_int_field(4);
        let image: [u8; 4] = [0x80, 0x00, 0x00, 0x01];
        let mut dst = [0u8; 4];
        let mut r = StringReader::new(&image);
        assert_eq!(
            unpack_integer(&mut fpi, &mut field, &mut dst, &mut r, None),
            UNPACK_SUCCESS
        );
        assert_eq!(dst, [0x01, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn unpack_integer_signed_negative_round_trip() {
        // 32-bit signed -1 in LE memory: [0xFF; 4]. Memcmp image:
        // [0xFF^0x80, 0xFF, 0xFF, 0xFF] = [0x7F, 0xFF, 0xFF, 0xFF].
        let mut fpi = FieldPacking::default();
        fpi.max_image_len = 4;
        let mut field = signed_int_field(4);
        let image: [u8; 4] = [0x7F, 0xFF, 0xFF, 0xFF];
        let mut dst = [0u8; 4];
        let mut r = StringReader::new(&image);
        assert_eq!(
            unpack_integer(&mut fpi, &mut field, &mut dst, &mut r, None),
            UNPACK_SUCCESS
        );
        assert_eq!(dst, [0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn unpack_integer_unsigned_preserves_sign_byte() {
        // unsigned 1 LE: [1, 0, 0, 0]. Memcmp image (no sign flip): [0, 0, 0, 1].
        let mut fpi = FieldPacking::default();
        fpi.max_image_len = 4;
        let mut field = unsigned_int_field(4);
        let image: [u8; 4] = [0x00, 0x00, 0x00, 0x01];
        let mut dst = [0u8; 4];
        let mut r = StringReader::new(&image);
        assert_eq!(
            unpack_integer(&mut fpi, &mut field, &mut dst, &mut r, None),
            UNPACK_SUCCESS
        );
        assert_eq!(dst, [0x01, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn unpack_integer_64bit_round_trip() {
        // i64 = 0x0102030405060708 LE-bytes: [08, 07, 06, 05, 04, 03, 02, 01].
        // Memcmp image (BE + flip): [0x81, 02, 03, 04, 05, 06, 07, 08].
        let mut fpi = FieldPacking::default();
        fpi.max_image_len = 8;
        let mut field = signed_int_field(8);
        let image: [u8; 8] = [0x81, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let mut dst = [0u8; 8];
        let mut r = StringReader::new(&image);
        assert_eq!(
            unpack_integer(&mut fpi, &mut field, &mut dst, &mut r, None),
            UNPACK_SUCCESS
        );
        assert_eq!(dst, [0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]);
    }

    #[test]
    fn unpack_integer_short_read_fails() {
        let mut fpi = FieldPacking::default();
        fpi.max_image_len = 4;
        let mut field = signed_int_field(4);
        let image: [u8; 2] = [0xFF, 0xFF];
        let mut dst = [0u8; 4];
        let mut r = StringReader::new(&image);
        assert_eq!(
            unpack_integer(&mut fpi, &mut field, &mut dst, &mut r, None),
            UNPACK_FAILURE
        );
    }

    #[test]
    fn unpack_double_zero_pattern() {
        let mut fpi = FieldPacking::default();
        let mut field = signed_int_field(8);
        let image: [u8; 8] = [128, 0, 0, 0, 0, 0, 0, 0];
        let mut dst = [0xFFu8; 8];
        let mut r = StringReader::new(&image);
        assert_eq!(
            unpack_double(&mut fpi, &mut field, &mut dst, &mut r, None),
            UNPACK_SUCCESS
        );
        assert_eq!(dst, [0u8; 8]);
        // And the bytes interpret as 0.0.
        assert_eq!(f64::from_le_bytes(dst), 0.0);
    }

    #[test]
    fn unpack_double_positive_one() {
        // 1.0 IEEE 754 double bytes (BE): [0x3F, 0xF0, 0, 0, 0, 0, 0, 0].
        // change_double_for_sort positive path:
        //   exp_part = 0x3FF0 + 0x10 = 0x4000 (add 1<<(16-1-11)=0x10).
        //   set high bit ⇒ 0xC000.
        // Encoded image: [0xC0, 0x00, 0, 0, 0, 0, 0, 0].
        let mut fpi = FieldPacking::default();
        let mut field = signed_int_field(8);
        let image: [u8; 8] = [0xC0, 0x00, 0, 0, 0, 0, 0, 0];
        let mut dst = [0u8; 8];
        let mut r = StringReader::new(&image);
        assert_eq!(
            unpack_double(&mut fpi, &mut field, &mut dst, &mut r, None),
            UNPACK_SUCCESS
        );
        assert_eq!(f64::from_le_bytes(dst), 1.0);
    }

    #[test]
    fn unpack_double_negative_one() {
        // -1.0 IEEE bytes (BE): [0xBF, 0xF0, 0, 0, 0, 0, 0, 0].
        // change_double_for_sort negative path: XOR every byte with 0xFF
        // ⇒ [0x40, 0x0F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF].
        let mut fpi = FieldPacking::default();
        let mut field = signed_int_field(8);
        let image: [u8; 8] = [0x40, 0x0F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let mut dst = [0u8; 8];
        let mut r = StringReader::new(&image);
        assert_eq!(
            unpack_double(&mut fpi, &mut field, &mut dst, &mut r, None),
            UNPACK_SUCCESS
        );
        assert_eq!(f64::from_le_bytes(dst), -1.0);
    }

    #[test]
    fn unpack_float_zero_and_round_trip() {
        let mut fpi = FieldPacking::default();
        let mut field = signed_int_field(4);
        // 0.0
        let mut dst = [0xFFu8; 4];
        let mut r = StringReader::new(&[128, 0, 0, 0]);
        assert_eq!(
            unpack_float(&mut fpi, &mut field, &mut dst, &mut r, None),
            UNPACK_SUCCESS
        );
        assert_eq!(f32::from_le_bytes(dst), 0.0);

        // 1.0 IEEE float bytes (BE): [0x3F, 0x80, 0, 0].
        // positive: exp_part = 0x3F80 + 0x80 (=1<<7) = 0x4000, set high
        // ⇒ 0xC000. Encoded: [0xC0, 0, 0, 0].
        let mut dst2 = [0u8; 4];
        let mut r2 = StringReader::new(&[0xC0, 0, 0, 0]);
        assert_eq!(
            unpack_float(&mut fpi, &mut field, &mut dst2, &mut r2, None),
            UNPACK_SUCCESS
        );
        assert_eq!(f32::from_le_bytes(dst2), 1.0);
    }

    #[test]
    fn unpack_newdate_reverses_three_bytes() {
        let mut fpi = FieldPacking::default();
        fpi.max_image_len = 3;
        let mut field = signed_int_field(3);
        let image: [u8; 3] = [0xAA, 0xBB, 0xCC];
        let mut dst = [0u8; 3];
        let mut r = StringReader::new(&image);
        assert_eq!(
            unpack_newdate(&mut fpi, &mut field, &mut dst, &mut r, None),
            UNPACK_SUCCESS
        );
        assert_eq!(dst, [0xCC, 0xBB, 0xAA]);
    }

    #[test]
    fn unpack_binary_str_memcpy() {
        let mut fpi = FieldPacking::default();
        fpi.max_image_len = 5;
        let mut field = signed_int_field(5);
        let image = b"hello";
        let mut dst = [0u8; 5];
        let mut r = StringReader::new(image);
        assert_eq!(
            unpack_binary_str(&mut fpi, &mut field, &mut dst, &mut r, None),
            UNPACK_SUCCESS
        );
        assert_eq!(&dst, image);
    }
}
