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
}
