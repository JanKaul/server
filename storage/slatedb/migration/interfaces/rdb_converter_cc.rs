//! Interface stub for `rdb_converter_cc`.
//!
//! C++ source: `storage/rocksdb/rdb_converter.cc` (~840 LoC)
//! C++ classes: `Rdb_converter`, `Rdb_convert_to_record_value_decoder`,
//!              `Rdb_value_field_iterator<value_field_decoder>` (template).
//!
//! ## Mapping
//! Per _DESIGN.md §3 (value encoding preserved): row TLV format is unchanged,
//! so the encode/decode logic ports directly. The only behavioural shift is
//! TTL: MyRocks reserved 8 bytes at the head of the value for an embedded
//! timestamp; we use SlateDB's native `expire_ts` instead (per
//! `ScanOptions::ttl_clock_override` + `PutOptions::ttl`). The encoder no
//! longer writes the prefix; the decoder no longer skips it.
//!
//! Per the task contract we do NOT expose `THD` / `TABLE` / `Field` types.
//! Public API takes already-marshalled column data via the `RowColumns`
//! intermediate (a column-index → bytes map) and emits the encoded `Bytes`.
//! Inverse for decode. Translation between MariaDB's `Field*` and
//! `RowColumns` is the cxx-bridge layer's job (see `bridge.rs` in TRANSLATE).
//!
//! ## Out-of-scope methods
//! - `dbug_modify_key_varchar8` — debug-only helper; ported only if
//!   MTR tests need it.
//! - `setup_field_decoders(MY_BITMAP*, bool)` taking a MariaDB bitmap —
//!   replaced by `Vec<usize>` (column indexes the caller wants decoded).
//! - All `Field*` / `TABLE*` parameters — translated at the bridge boundary.

use crate::rdb_buff_h::{StringReader, StringWriter};
use slatedb::bytes::Bytes;
use slatedb::Error;
use std::collections::BTreeMap;

/// MariaDB column-type enum (subset). The bridge converts MariaDB's
/// `enum_field_types` to this and back. We keep the discriminants matching
/// MariaDB's for direct cast where the bridge wants to.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    LongLong = 8,
    VarChar = 15,
    Blob = 252,
    Long = 3,
    String = 254,
    // ... bridge-side translation table fills the rest as needed.
    Other = 255,
}

/// One column's value, marshalled out of the MariaDB `Field*` by the bridge.
/// `None` means SQL NULL.
#[derive(Debug, Clone)]
pub struct ColumnValue {
    pub field_index: u16,
    pub ty: ColumnType,
    pub value: Option<Bytes>,
    pub pack_length_in_rec: u16,
    pub maybe_null: bool,
}

/// Whole-row marshalled form. Column-index keyed for `O(log n)` lookup during
/// per-column processing.
pub type RowColumns = BTreeMap<u16, ColumnValue>;

/// One field encoder slot — derived from the `Rdb_field_encoder` struct in
/// the C++ tree. Stored per-column, computed once at table-open time.
#[derive(Debug, Clone, Copy)]
pub struct FieldEncoder {
    pub field_index: u16,
    pub ty: ColumnType,
    pub pack_length_in_rec: u16,
    pub maybe_null: bool,
    pub null_offset: u16,
    pub null_mask: u8,
    pub storage_type: StorageType,
}

/// Where the column's value is stored. PK columns that are bit-for-bit
/// reconstructible from the key are `StoreNone`; columns whose value carries
/// unpack-info side data are `StoreSome`; everything else is `StoreAll`.
///
/// Original: rdb_converter.h — `enum storage_type_enum`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageType {
    StoreAll,
    StoreSome,
    StoreNone,
}

/// One decoder-vector slot: which field, whether to actually decode, and how
/// many opaque bytes to skip before reaching it.
///
/// Original: rdb_converter.h — `struct ReadFieldData`.
#[derive(Debug, Clone, Copy)]
pub struct DecoderSlot {
    pub encoder_index: u16,
    pub decode: bool,
    pub skip: u32,
}

/// Per-table value-format converter. Owned by the engine handler and reused
/// across statements for the same table. **Not thread-safe** (matches C++).
///
/// Original: rdb_converter.cc:329 — `Rdb_converter::Rdb_converter`.
pub struct Converter {
    /// Encoder per column, indexed by `field_index`.
    pub encoders: Vec<FieldEncoder>,
    /// Filtered decoder list (subset of columns the caller wants).
    pub decoders: Vec<DecoderSlot>,
    /// Number of bytes the value spends on the leading NULL bitmap.
    pub null_bytes_length: u16,
    /// True if any PK column has unpack-info side data.
    pub maybe_unpack_info: bool,
    /// True if the row checksum tag is expected at the value tail.
    pub verify_row_debug_checksums: bool,
}

impl Converter {
    /// Build the encoder table at table-open time. Called once per
    /// `handler::open()`. Replaces `setup_field_encoders`.
    ///
    /// Inputs:
    /// - `columns`: ordered column metadata for the table (the bridge fills
    ///   this from `TABLE::field`).
    /// - `pk_columns`: indexes (into `columns`) of the columns that form
    ///   the primary key.
    /// - `pk_can_unpack`, `pk_has_unpack_info`: bit vectors aligned with
    ///   `pk_columns`, describing which PK parts are reconstructible.
    ///
    /// Output: configured `Converter`.
    ///
    /// Errors: `Invalid` on internally inconsistent input (e.g. `pk_columns`
    /// referencing an out-of-range field).
    ///
    /// Original: rdb_converter.cc:434 — `setup_field_encoders`.
    pub fn new(
        columns: &[ColumnMeta],
        pk_columns: &[u16],
        pk_can_unpack: &[bool],
        pk_has_unpack_info: &[bool],
    ) -> Result<Self, Error> {
        let _ = (columns, pk_columns, pk_can_unpack, pk_has_unpack_info);
        todo!("build encoder array, compute null bitmap layout (1 bit per maybe_null col)")
    }

    /// Build the decoder vector for one statement. Replaces
    /// `setup_field_decoders` taking a `MY_BITMAP*`.
    ///
    /// Inputs:
    /// - `wanted_columns`: column indexes the optimizer asked to decode.
    /// - `decode_all_fields`: true if write lock (need every column).
    ///
    /// Original: rdb_converter.cc:385 — `setup_field_decoders`.
    pub fn setup_decoders(&mut self, wanted_columns: &[u16], decode_all_fields: bool) {
        let _ = (wanted_columns, decode_all_fields);
        todo!("walk encoders, push DecoderSlot for each wanted col, accumulate skip_size for the rest")
    }

    /// Encode one row from `RowColumns` into the SlateDB value blob.
    ///
    /// Inputs:
    /// - `row`: column values (from the bridge).
    /// - `pk_packed`: already-encoded PK key bytes (caller computed via
    ///   `rdb_key_def`). Used for checksum input only.
    /// - `pk_unpack_info`: unpack-info bytes if `maybe_unpack_info`.
    /// - `is_update`, `store_checksums`: behavior flags.
    ///
    /// Output: encoded value as `Bytes` ready for `Db::put`.
    ///
    /// Errors: `Invalid` if a column referenced by an encoder is missing
    /// from `row` (bridge bug).
    ///
    /// Original: rdb_converter.cc:688 — `encode_value_slice`.
    pub fn encode_value(
        &mut self,
        row: &RowColumns,
        pk_packed: &Bytes,
        pk_unpack_info: Option<&StringWriter>,
        is_update: bool,
        store_checksums: bool,
    ) -> Result<Bytes, Error> {
        let _ = (row, pk_packed, pk_unpack_info, is_update, store_checksums);
        todo!("write null bitmap, optional unpack header, per-column data; append crc32 tail if store_checksums")
    }

    /// Decode one value blob back into per-column bytes. The caller (bridge)
    /// converts each `Bytes` into a `Field::store(...)` call.
    ///
    /// Inputs:
    /// - `key`: SlateDB key (needed for PK-unpack and for checksum verify).
    /// - `value`: SlateDB value blob.
    ///
    /// Output: `RowColumns` populated for columns in `self.decoders`.
    ///
    /// Errors:
    /// - `Data` on a corrupt value (under-read, bad checksum tag).
    /// - `Invalid` if `self.decoders` is empty (caller forgot to call
    ///   `setup_decoders`).
    ///
    /// Original: rdb_converter.cc:584 — `convert_record_from_storage_format`.
    pub fn decode_value(&mut self, key: &Bytes, value: &Bytes) -> Result<RowColumns, Error> {
        let _ = (key, value);
        todo!("StringReader over value; consume null bitmap; for each DecoderSlot, skip then read len-prefixed field")
    }

    /// Decode just the value header (NULL bitmap + optional unpack slice).
    /// Used by callers that only need to know whether the row matches a
    /// predicate before paying for full decode.
    ///
    /// Original: rdb_converter.cc:537 — `decode_value_header`.
    pub fn decode_value_header<'a>(
        &mut self,
        reader: &mut StringReader<'a>,
    ) -> Result<Option<&'a [u8]>, Error> {
        let _ = reader;
        todo!("read null bytes; if maybe_unpack_info, read tag + len, return the unpack slice")
    }
}

/// Minimal column descriptor passed to `Converter::new`. The bridge fills
/// this from `TABLE::field[i]` so the converter never touches MariaDB types.
#[derive(Debug, Clone, Copy)]
pub struct ColumnMeta {
    pub field_index: u16,
    pub ty: ColumnType,
    pub pack_length_in_rec: u16,
    pub maybe_null: bool,
}
