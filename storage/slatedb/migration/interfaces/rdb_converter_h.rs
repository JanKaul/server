//! Interface stub for `rdb_converter_h`.
//!
//! C++ source: `storage/rocksdb/rdb_converter.h` (247 LoC)
//! C++ classes: `Rdb_converter`,
//!              `Rdb_convert_to_record_value_decoder`,
//!              `Rdb_value_field_iterator<>`,
//!              `READ_FIELD`
//!
//! ## Mapping
//! Per _DESIGN.md §3 (value encoding): "MyRocks TLV row format preserved per
//! column" — this converter is the encode/decode boundary between SlateDB
//! `Bytes` values and per-column MariaDB `Field` values.
//!
//! Per the contract we cannot expose `TABLE*` / `Field*` / `THD*` to Rust.
//! So the Rust-side interface deals only in:
//!
//! - `TableDef` — our Rust handle for the table's column/encoding schema
//!   (lives in `rdb_datadic` translation, opaque here).
//! - `Bytes` — the SlateDB-side key/value blob.
//! - `RowBuffer` — a writable byte slice with per-column offsets that the
//!   cxx bridge maps into a `TABLE::record[0]` on the C++ side. Lives in our
//!   crate's `codec/value.rs` module.
//!
//! Native SlateDB TTL (`expire_ts`) replaces the MyRocks "TTL prefix bytes"
//! per §3: `encode_value()` does NOT embed the TTL into the value blob; the
//! TTL is returned alongside as `Option<PutOptions>` for the caller to set
//! on the SlateDB write.
//!
//! ## Out-of-scope methods
//! - Anything taking `TABLE*`/`Field*` directly — replaced with
//!   `TableDef` and `RowBuffer` opaque types in the Rust API.
//! - `Rdb_value_field_iterator<>` template specialization — collapsed into a
//!   single concrete iterator struct (no virtual-call avoidance needed in
//!   Rust; the compiler monomorphizes generics already).

use bytes::Bytes;
use slatedb::Error;

/// Opaque handle to the parsed table schema. Lives in `rdb_datadic`'s
/// translation. Carries per-column encoder/decoder metadata.
pub struct TableDef;

/// Writable row buffer with per-column offsets. The cxx bridge marshals this
/// into the server's `TABLE::record[0]` on the C++ side, but the Rust side
/// works with raw bytes + offsets only — no server types crossing the line.
pub struct RowBuffer {
    pub data: Vec<u8>,
    /// Byte offset of each column within `data`. Length = number of columns.
    pub field_offsets: Vec<usize>,
    /// One bit per column; 1 = SQL NULL.
    pub null_bitmap: Vec<u8>,
}

/// Describes how to decode one field from the value slice. Matches the
/// C++ `READ_FIELD` (rdb_converter.h:39) — `m_field_enc` index into the
/// `TableDef`'s field-encoder array, `m_decode`/`m_skip` flags.
#[derive(Debug, Clone, Copy)]
pub struct ReadField {
    pub field_index: u32,
    pub decode: bool,
    pub skip_bytes: i32,
}

/// MyRocks TLV row encoder/decoder. Replaces C++ `Rdb_converter`.
///
/// Construction is bridge-side: the cxx layer hands us the `TableDef`
/// pointer it built from the server's `TABLE*`.
pub struct Converter {
    table_def: std::sync::Arc<TableDef>,
    decoders: Vec<ReadField>,
    verify_row_debug_checksums: bool,
    /// True if any field in the PK requires unpack_info (sub-set of columns
    /// where the encoded-in-key bytes are lossy and need a side channel).
    maybe_unpack_info: bool,
    /// True if the current query needs the PK columns decoded (e.g., we're
    /// scanning a secondary index).
    key_requested: bool,
    /// Number of rows for which a row-debug checksum has been validated.
    row_checksums_checked: u64,
    /// Bytes used by the SQL NULL bitmap in the on-disk record.
    null_bytes_length_in_record: usize,
}

impl Converter {
    /// C++ `Rdb_converter::Rdb_converter(THD*, Rdb_tbl_def*, TABLE*)`.
    /// We accept only the bridge-mediated `TableDef` here.
    pub fn new(table_def: std::sync::Arc<TableDef>) -> Self {
        Self {
            table_def,
            decoders: Vec::new(),
            verify_row_debug_checksums: false,
            maybe_unpack_info: false,
            key_requested: false,
            row_checksums_checked: 0,
            null_bytes_length_in_record: 0,
        }
    }

    /// Build the `decoders` array from the bitmap of fields the query
    /// actually needs. C++ `setup_field_decoders(field_map, decode_all)`.
    ///
    /// `requested_fields` is a bitset; the Rust side gets it as a slice so we
    /// don't pull in MyBitmap. `decode_all=true` overrides the bitset.
    pub fn setup_field_decoders(
        &mut self,
        requested_fields: &[u8],
        decode_all_fields: bool,
    ) {
        let _ = (requested_fields, decode_all_fields);
        todo!("walk TableDef's field-encoder array, push ReadField for each match")
    }

    /// Decode one `(key, value)` row into `dst`.
    ///
    /// `key_def_index` selects which index's encoding rules to apply (PK vs
    /// secondary; affects which columns come from `key` vs `value`).
    ///
    /// Errors:
    /// - `Data` — corrupted row (bad checksum, length under-read, unknown
    ///   tag).
    /// - `Internal` — the requested fields bitmap doesn't match the schema.
    pub fn decode(
        &mut self,
        key_def_index: u32,
        dst: &mut RowBuffer,
        key: &Bytes,
        value: &Bytes,
    ) -> Result<(), Error> {
        let _ = (key_def_index, dst, key, value);
        todo!("decode_value_header + walk decoders + Rdb_convert_to_record_value_decoder per field")
    }

    /// Encode the per-column values from `src` into a value slice for
    /// SlateDB. Returns the encoded blob plus optional TTL (passed to
    /// `slatedb::PutOptions.ttl`).
    ///
    /// `pk_packed` is the already-encoded PK bytes (needed for the
    /// PK→secondary copy of unpack_info).
    ///
    /// Errors:
    /// - `Invalid` — schema mismatch in `src`.
    /// - `Data` — column value out of range for its declared type.
    pub fn encode_value(
        &mut self,
        pk_def_index: u32,
        pk_packed: &Bytes,
        pk_unpack_info: Option<&Bytes>,
        src: &RowBuffer,
        is_update_row: bool,
        store_row_debug_checksums: bool,
    ) -> Result<EncodedValue, Error> {
        let _ = (pk_def_index, pk_packed, pk_unpack_info, src, is_update_row, store_row_debug_checksums);
        todo!("checksum_byte + field_count_varint + (field_id_varint || field_value)*")
    }

    pub fn row_checksums_checked(&self) -> u64 { self.row_checksums_checked }
    pub fn verify_row_debug_checksums(&self) -> bool { self.verify_row_debug_checksums }
    pub fn set_verify_row_debug_checksums(&mut self, v: bool) {
        self.verify_row_debug_checksums = v;
    }
    pub fn null_bytes_length_in_record(&self) -> usize { self.null_bytes_length_in_record }
    pub fn maybe_unpack_info(&self) -> bool { self.maybe_unpack_info }
    pub fn set_key_requested(&mut self, v: bool) { self.key_requested = v; }
    pub fn decoders(&self) -> &[ReadField] { &self.decoders }
    pub fn table_def(&self) -> &TableDef { &self.table_def }
}

/// Output of `Converter::encode_value`. Per _DESIGN.md §3, TTL is carried
/// out-of-band — we set it via `slatedb::PutOptions.ttl`, not by embedding
/// bytes in `value`.
pub struct EncodedValue {
    pub value: Bytes,
    /// Set when the index has `ttl_duration > 0` and the row had a TTL
    /// timestamp column. `None` → SlateDB write uses `Ttl::Default`.
    pub ttl_seconds: Option<u64>,
}
