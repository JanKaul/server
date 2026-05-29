//! Interface stub for `rdb_datadic_h__Rdb_field_encoder`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (lines 1018..1052)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_field_encoder`
//! Parent unit: `rdb_datadic_h`
//! Approx body LoC accounted for here: 35
//!
//! ## Mapping
//! In-scope codec types — preserved from MyRocks per _DESIGN.md §2/§3.
//! `Rdb_field_encoder` is a smaller / value-side companion to
//! `Rdb_field_packing`: it describes how a field is stored in the
//! ROW VALUE blob (not in the index key). The TLV row format defined in
//! _DESIGN.md §3 is preserved bit-for-bit; the encoder only needs to know
//! which storage tier a field is in (`STORE_NONE` / `STORE_SOME` /
//! `STORE_ALL`), where the null byte for it lives, and how many bytes it
//! occupies in the on-record image.
//!
//! No constructors, no methods other than two tiny accessors — pure data.
//!
//! ## Out-of-scope methods
//! None.

/// Storage-tier classification for a single column in the row-value blob.
/// Used to decide whether the column's bytes need to be written at all (vs.
/// reconstructible from the mem-comparable PK image plus optional unpack_info).
///
/// Original: rdb_datadic.h:1030 — `enum STORAGE_TYPE`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StorageType {
    /// Column is fully reconstructible from the mem-comparable image alone.
    #[default]
    StoreNone = 0,
    /// Column needs mem-comparable image + unpack_info to reconstruct.
    StoreSome = 1,
    /// Column must be stored verbatim in the row-value blob.
    StoreAll = 2,
}

/// MySQL field-type tag. Mirrors `my_core::enum_field_types`. We keep the
/// numeric value identical for direct comparison with values read out of
/// the data dictionary.
///
/// NOT a full enum — we only need to identify VARCHAR / BLOB / fixed-width
/// for the `uses_variable_len_encoding` check; everything else flows through
/// as opaque. A full enum will live in the mariadb-port unit.
pub type FieldTypeTag = u8;

/// `MYSQL_TYPE_BLOB` from mariadb's field.h. Carried here as a constant so
/// our `uses_variable_len_encoding` matches the C++ source.
pub const MYSQL_TYPE_BLOB: FieldTypeTag = 252;
/// `MYSQL_TYPE_VARCHAR` from mariadb's field.h.
pub const MYSQL_TYPE_VARCHAR: FieldTypeTag = 15;

/// Per-column value-side descriptor. One instance per non-PK column, owned
/// by the enclosing `Rdb_tbl_def` (will live in its `field_enc` array).
///
/// Original: rdb_datadic.h:1018 — `class Rdb_field_encoder`.
#[derive(Default)]
pub struct FieldEncoder {
    pub storage_type: StorageType,

    /// Byte offset within the MySQL `record[]` buffer where this field's
    /// null-byte lives. Same units MyRocks uses.
    pub null_offset: u32,
    /// Position of this field within the TABLE's field list. Used to look
    /// up the `FieldView` at encode/decode time.
    pub field_index: u16,

    /// Bitmask within `record[null_offset]` selecting this field's null bit.
    /// `0` means the field is NOT NULL.
    pub null_mask: u8,

    pub field_type: FieldTypeTag,
    /// On-record byte-length of the packed image (0 for variable-length fields).
    pub pack_length_in_rec: u32,
}

impl FieldEncoder {
    /// True iff this field is nullable.
    /// Original: rdb_datadic.h:1046 — `maybe_null`.
    pub fn maybe_null(&self) -> bool { self.null_mask != 0 }

    /// True iff this field uses variable-length encoding in the row-value
    /// blob (currently BLOB and VARCHAR).
    /// Original: rdb_datadic.h:1048 — `uses_variable_len_encoding`.
    pub fn uses_variable_len_encoding(&self) -> bool {
        self.field_type == MYSQL_TYPE_BLOB || self.field_type == MYSQL_TYPE_VARCHAR
    }
}
