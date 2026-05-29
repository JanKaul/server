//! Interface stub for `Rdb_field_packing`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.cc` (lines 3212..3528)
//! C++ header: `storage/rocksdb/rdb_datadic.h` (Rdb_field_packing declarations)
//! v4 manifest sub-unit: `Rdb_field_packing`
//! Parent unit: `rdb_datadic.cc`
//! Approx body LoC accounted for here: ~320
//!
//! ## Mapping
//! Per _DESIGN.md §2 / §3 — pure codec metadata, no SlateDB API surface.
//! `Rdb_field_packing` is a per-column descriptor populated at
//! `Rdb_key_def::setup()` time. It carries:
//!
//! - `m_max_image_len`: bytes the column takes in the packed key.
//! - `m_pack_func` / `m_unpack_func` / `m_skip_func`: function-pointer
//!   triple selected by MySQL field type + charset.
//! - `m_make_unpack_info_func`: optional sidechannel writer.
//! - `m_maybe_null`, `m_unpack_info_stores_value`, etc.: capability flags.
//!
//! In Rust we replace function pointers with enum-tag dispatch (cleaner
//! and inlinable) — see the `PackKind` enum below. The dispatch table is
//! built once at setup() and consulted on every pack/unpack call.
//!
//! ## Out-of-scope methods
//! None. `fill_hidden_pk_val` is retained verbatim — hidden PK is part of
//! _DESIGN.md §2.

use slatedb::Error;

use crate::rdb_key_def__encode::FieldPacking as EncodeStub;
use crate::rdb_key_def__decode::FieldPacking as DecodeStub;

/// Dispatch tag for pack/unpack/skip routines. Replaces C++ function-pointer
/// triples with an enum the codec switches on. Adding a new field type =
/// adding an enum variant + matching arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackKind {
    /// Fixed-width integer: int8/16/24/32/64 (signed or unsigned).
    Integer { width: u8, signed: bool },
    Float,
    Double,
    NewDate,
    BinaryString,
    Utf8String,
    /// Varchar with explicit length-prefix encoding, no space padding.
    BinaryOrUtf8Varchar,
    /// Varchar with space-padding encoding (PAD_SPACE collations).
    BinaryOrUtf8VarcharSpacePad,
    /// "Simple" collation varchar (case-insensitive, single-byte charset).
    SimpleVarcharSpacePad,
    /// "Simple" collation fixed-width string.
    Simple,
    /// Type whose memcomparable encoding loses the original bytes — both
    /// encode and decode go via the unpack_info sidechannel.
    Unknown,
    UnknownVarchar,
    /// Hidden PK (varint rowid) — special case.
    HiddenPk,
}

/// `Rdb_field_packing` — per-column codec descriptor.
pub struct FieldPacking {
    pub pack_kind: PackKind,
    /// Max bytes this column emits into the packed key.
    pub max_image_len: usize,
    /// Caller-allocated `Field*` offset in `TABLE::record[0]`. Stored as
    /// raw offset so we don't carry a MariaDB `Field*` lifetime here.
    pub field_offset: usize,
    pub null_offset: usize,
    pub null_bit: u8,
    pub field_index: u32,
    pub field_real_type: u8,
    pub field_keytype: u8,
    /// True if the column may be NULL.
    pub maybe_null: bool,
    /// True for "covered" varchar fields whose original bytes are stored in
    /// unpack_info and can satisfy index-only scans.
    pub covered: bool,
    /// True if make_unpack_info_func writes a 2-byte length, else 1-byte.
    pub unpack_info_uses_two_bytes: bool,
    /// True if the column contributes original bytes to unpack_info (i.e.
    /// is recoverable from key + unpack_info).
    pub unpack_info_stores_value: bool,
    /// Offset into unpack_info where this column's sidechannel data starts.
    pub unpack_data_offset: u32,
    /// KV format version of the parent key_def — drives feature gating.
    pub kv_format_version: u16,
}

impl FieldPacking {
    /// One-shot setup from a MariaDB `Field*`. Decides `pack_kind` based on
    /// `field->real_type()` + `field->charset()`. Returns false if the
    /// column cannot be packed (e.g. unsupported geometry type).
    ///
    /// Errors: `slatedb::Error::invalid` on unsupported schema.
    /// C++: rdb_datadic.cc:3212.
    pub fn setup(
        &mut self,
        _key_descr: &(),     // TODO(human): &KeyDef
        _field: &(),         // TODO(human): MariaDB Field*
        _keynr: u32,
        _key_part: u32,
        _key_length: u16,
    ) -> Result<bool, Error> {
        todo!("port C++ setup at rdb_datadic.cc:3212 — large switch over Field::real_type()")
    }

    /// Resolve the `Field*` for this packing back inside a TABLE.
    /// C++: rdb_datadic.cc:3511.
    pub fn get_field_in_table<'t>(&self, _table: &'t ()) -> &'t () {
        todo!("rdb_datadic.cc:3511")
    }

    /// Write the synthetic 8-byte hidden-PK value into `dst`, advancing the
    /// cursor by 8.
    /// C++: rdb_datadic.cc:3515.
    pub fn fill_hidden_pk_val(&self, _dst: &mut Vec<u8>, _hidden_pk_id: i64) {
        todo!("rdb_datadic.cc:3515")
    }

    /// True if `pack_kind` requires unpack_info to round-trip.
    pub fn uses_unpack_info(&self) -> bool {
        matches!(
            self.pack_kind,
            PackKind::Unknown
                | PackKind::UnknownVarchar
                | PackKind::SimpleVarcharSpacePad
                | PackKind::Simple
                | PackKind::BinaryOrUtf8VarcharSpacePad
        )
    }
}

/// Dispatch helper: returns the encode-side stub for this packing. The
/// codec calls this once per field per pack op.
pub fn encode_dispatch(_fpi: &FieldPacking) -> &'static EncodeStub {
    todo!("dispatch table: pack_kind -> fn pointer in Rdb_key_def__encode")
}

/// Dispatch helper: returns the decode-side stub.
pub fn decode_dispatch(_fpi: &FieldPacking) -> &'static DecodeStub {
    todo!("dispatch table: pack_kind -> fn pointer in Rdb_key_def__decode")
}
