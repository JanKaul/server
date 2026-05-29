//! Interface stub for `rdb_datadic_h__Rdb_field_packing`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (lines 912..1009)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_field_packing`
//! Parent unit: `rdb_datadic_h`
//! Approx body LoC accounted for here: 98
//!
//! ## Mapping
//! In-scope codec types — preserved from MyRocks per _DESIGN.md §2/§3.
//! `Rdb_field_packing` is the per-column descriptor that the codec dispatches
//! on. It bundles the width, charset, nullability, and the three function
//! pointers (`m_pack_func`, `m_make_unpack_info_func`, `m_unpack_func`) used
//! by `Rdb_key_field_iterator` / `Rdb_key_def`.
//!
//! Translation is straightforward: each `m_*` member becomes a `pub` field of
//! a Rust struct, function pointers become `Option<fn>` aliases (None == not
//! applicable for this field type). The C++ `setup()` populates these slots
//! based on the field's MySQL type; the Rust port keeps that responsibility
//! here (the type-dispatch table itself lives in the codec module).
//!
//! `rdb_mem_cmp_space_mutex` (declared in `Rdb_collation_codec`'s neighborhood
//! at rdb_datadic.h:908) actually protects the lazy `space_xfrm`
//! initialization that happens inside this struct's setup — so we host it
//! here.
//!
//! ## Out-of-scope methods
//! None — every public member and method has a direct Rust analogue.

use slatedb::Error;

use crate::rdb_datadic_h__Rdb_collation_codec::{
    CollationCodec, IndexFieldUnpackFn, MakeUnpackInfoFn,
};
use crate::rdb_datadic_h__Rdb_convert_to_record_key_decoder::{FieldView, TableShareView};
use crate::rdb_datadic_h__Rdb_pack_field_context::PackFieldContext;
use crate::rdb_buff_h::StringReader;

/// Forward to the future Rdb_key_def stub.
pub struct KeyDef; // TODO(human): wire to rdb_datadic_h__Rdb_key_def stub

/// Function pointer aliases — see Rdb_collation_codec for the unpack/make
/// variants. Pack and skip variants live here because no other unit needs them.

/// Mirrors C++ `rdb_index_field_pack_t` from rdb_datadic.h:155.
pub type IndexFieldPackFn = fn(
    fpi: &mut FieldPacking,
    field: &mut FieldView,
    buf: &mut [u8],
    dst: &mut Vec<u8>,
    pack_ctx: &mut PackFieldContext<'_>,
);

/// Mirrors C++ `rdb_index_field_skip_t` from rdb_datadic.h:152.
pub type IndexFieldSkipFn = fn(
    fpi: &FieldPacking,
    field: &FieldView,
    reader: &mut StringReader,
) -> i32;

/// Per-column descriptor — the codec dispatches on `pack_func` / `unpack_func`
/// / `skip_func` to pick the encoding routine for this field. One instance
/// per (index, key-part) pair, owned by the enclosing `Rdb_key_def`.
///
/// Original: rdb_datadic.h:912 — `class Rdb_field_packing`.
#[derive(Default)]
pub struct FieldPacking {
    /// Length of the mem-comparable image of the field, in bytes.
    pub max_image_len: i32,
    /// Length of the unpack-info image for this field, in bytes.
    pub unpack_data_len: i32,
    /// Offset within the per-row unpack_info blob where this field's bytes
    /// begin (set by `Rdb_key_def::setup`).
    pub unpack_data_offset: i32,

    /// True iff the field has a stored NULL-byte (i.e. is nullable).
    pub maybe_null: bool,

    /// VARCHAR-only: the charset id of the column (None for non-VARCHAR).
    /// Carried as `u32` for the same reason as `CollationCodec::charset_id`.
    pub varchar_charset: Option<u32>,
    /// True iff the field uses the pre-PRIMARY_FORMAT_VERSION_UPDATE2 binary
    /// variable-length encoding (the old multiple-of-8 quirk).
    pub use_legacy_varbinary_format: bool,

    /// VARCHAR + space-pad encoding: bytes per segment.
    pub segment_size: u32,

    /// True iff `unpack_info` uses 2 bytes to encode the trimmed-spaces count;
    /// false means 1 byte.
    pub unpack_info_uses_two_bytes: bool,

    /// True iff an index-only read is always possible for this field. False
    /// means it depends on the per-record content.
    pub covered: bool,

    /// Lazily-initialized space-padding transform bytes (charset's mem-cmp
    /// image of one space character). Computed once per charset under
    /// `MEM_CMP_SPACE_MUTEX`. `None` until first observed.
    pub space_xfrm: Option<&'static Vec<u8>>,
    pub space_xfrm_len: usize,
    pub space_mb_len: usize,

    /// Borrowed pointer to the per-charset codec table (lives in
    /// `CollationDataTable`). `None` for non-simple-collation fields.
    pub charset_codec: Option<std::sync::Arc<CollationCodec>>,

    /// True iff the encoded image is followed by a non-empty unpack_info
    /// block (depends on the field's pack routine).
    pub unpack_info_stores_value: bool,

    /// Pack / make-unpack / unpack / skip routine slots. `None` means the
    /// dispatch is not applicable — e.g. fixed-width integers do not produce
    /// unpack_info, so `make_unpack_info_func` is `None`.
    pub pack_func: Option<IndexFieldPackFn>,
    pub make_unpack_info_func: Option<MakeUnpackInfoFn>,
    pub unpack_func: Option<IndexFieldUnpackFn>,
    pub skip_func: Option<IndexFieldSkipFn>,

    // ---- private in C++; kept pub(crate) here for codec-internal access ----
    /// Index number this field belongs to (for "extended-keys" disambiguation).
    pub(crate) keynr: u32,
    /// Position of this field within the key (0-based).
    pub(crate) key_part: u32,
}

impl FieldPacking {
    /// True iff this field's encoding emits any unpack_info bytes.
    /// Original: rdb_datadic.h:956 — `uses_unpack_info`.
    pub fn uses_unpack_info(&self) -> bool {
        self.make_unpack_info_func.is_some()
    }

    /// Populate every dispatch slot and width based on the field's MySQL type.
    ///
    /// **Inputs:**
    /// - `key_descr`: enclosing key definition (used for format-version checks).
    /// - `field`: the MySQL field we're describing.
    /// - `keynr`: index number within the table.
    /// - `key_part`: 0-based part within the index.
    /// - `key_length`: declared key-prefix length (for partial-column indexes).
    ///
    /// **Output:** `true` if a supported encoding was selected, `false` if
    /// the column type is unsupported (the index will be rejected).
    /// **Errors:** none in the `bool` return; callers convert to
    /// `slatedb::Error::invalid` when the index is being created.
    /// **Invariants:** after a `true` return all four `*_func` slots are set
    /// consistently (e.g. if `pack_func` produces unpack_info then both
    /// `make_unpack_info_func` and `unpack_func` must read it).
    ///
    /// Original: rdb_datadic.h:1004 — `setup`.
    pub fn setup(
        &mut self,
        _key_descr: &KeyDef,
        _field: &FieldView,
        _keynr: u32,
        _key_part: u32,
        _key_length: u16,
    ) -> bool {
        todo!("port Rdb_field_packing::setup — big switch over field->real_type()")
    }

    /// Look up the MySQL `Field*` corresponding to this descriptor in the
    /// given TABLE. Returns `None` if `m_field_index` is out of range
    /// (shouldn't happen with a well-formed `Rdb_tbl_def`).
    ///
    /// Original: rdb_datadic.h:1007 — `get_field_in_table`.
    pub fn get_field_in_table<'t>(&self, _tbl: &'t TableShareView) -> Option<&'t FieldView> {
        todo!("look up by self.keynr/self.key_part in the table share")
    }

    /// Write the hidden-PK value (big-endian u64) into `dst` and advance
    /// `dst_offset` by `SIZEOF_HIDDEN_PK_COLUMN` (8).
    ///
    /// Original: rdb_datadic.h:1008 — `fill_hidden_pk_val`.
    pub fn fill_hidden_pk_val(&self, dst: &mut [u8], dst_offset: &mut usize, hidden_pk_id: i64) {
        let id_bytes = (hidden_pk_id as u64).to_be_bytes();
        dst[*dst_offset..*dst_offset + 8].copy_from_slice(&id_bytes);
        *dst_offset += 8;
    }
}

/// Mutex protecting `Rdb_field_packing::space_xfrm` lazy initialization.
/// Declared as a global in C++ at rdb_datadic.h:908 (`rdb_mem_cmp_space_mutex`).
pub static MEM_CMP_SPACE_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Helper: convert MyRocks's `(true, false)` UNPACK return code to a
/// `Result<(), Error>` at the iterator boundary.
pub fn unpack_status_to_result(code: i32) -> Result<(), Error> {
    use crate::rdb_datadic_h__Rdb_convert_to_record_key_decoder::{UNPACK_FAILURE, UNPACK_SUCCESS};
    match code {
        UNPACK_SUCCESS => Ok(()),
        UNPACK_FAILURE => Err(Error::data("memcomparable unpack failed".into())),
        _ => Err(Error::internal(format!("unexpected unpack code {code}"))),
    }
}
