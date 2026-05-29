//! Interface stub for `Rdb_key_field_iterator`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.cc` (lines 199..288)
//! C++ header: `storage/rocksdb/rdb_datadic.h` (Rdb_key_field_iterator)
//! v4 manifest sub-unit: `Rdb_key_field_iterator`
//! Parent unit: `rdb_datadic.cc`
//! Approx body LoC accounted for here: ~90
//!
//! ## Mapping
//! Per _DESIGN.md §2: walks each field-part of a packed key in order,
//! using `Rdb_convert_to_record_key_decoder::decode/skip` for the per-field
//! work. Handles two MyRocks-specific quirks:
//!
//! 1. **Hidden PK at the end of secondary keys** — for SK that don't have
//!    an explicit PK column suffix (MyRocks "extended keys"), the last
//!    part is the auto-generated hidden PK. Skip-only (no field to write
//!    into).
//! 2. **Covered-column bitmap** — for varchar fields, the iterator consults
//!    a `MY_BITMAP` to decide whether the value can be unpacked from
//!    unpack_info (covered) or must be left NULL in the partial record.
//!
//! In Rust we replace `MY_BITMAP*` with `&[u64]` (or a thin
//! `bitvec::BitSlice` adapter). The iterator is a `&mut`-borrowing struct
//! with explicit `next()` returning `Result<Option<FieldStep>, Error>` —
//! NOT the `Iterator` trait, because the per-call output is not a
//! self-owned `Item`.
//!
//! ## Out-of-scope methods
//! None. All accessors translate.

use slatedb::Error;

use crate::rdb_buff_h::StringReader;
use crate::Rdb_field_packing::FieldPacking;
use crate::Rdb_key_def__meta::{IndexType, KeyDef};

/// Per-iteration step output. `field_index` is `None` for the hidden-PK
/// skip step (no MariaDB Field exists for it).
pub struct FieldStep {
    pub field_index: Option<u32>,
    pub is_null: bool,
    pub offset: usize,
}

/// `Rdb_key_field_iterator` — drives field-by-field key decode.
pub struct KeyFieldIterator<'a> {
    pub key_def: &'a KeyDef,
    pub pack_info: &'a [FieldPacking],
    pub reader: &'a mut StringReader<'a>,
    pub unp_reader: Option<&'a mut StringReader<'a>>,
    pub buf: &'a mut [u8],
    pub iter_index: usize,
    pub iter_end: usize,
    pub has_unpack_info: bool,
    pub covered_bitmap: Option<&'a [u64]>,
    pub curr_bitmap_pos: u32,
    pub offset: usize,
    pub secondary_key: bool,
    pub hidden_pk_exists: bool,
    pub is_hidden_pk: bool,
    /// Cached for `get_field`/`get_field_index` between calls.
    pub current_field_index: Option<u32>,
    pub current_is_null: bool,
}

impl<'a> KeyFieldIterator<'a> {
    /// Construct. Derives `secondary_key`/`hidden_pk_exists`/`is_hidden_pk`
    /// from `key_def.index_type` and the table schema (passed via
    /// pre-computed `hidden_pk_exists` because we don't carry a TABLE
    /// pointer in Rust).
    ///
    /// C++: rdb_datadic.cc:199.
    pub fn new(
        key_def: &'a KeyDef,
        pack_info: &'a [FieldPacking],
        reader: &'a mut StringReader<'a>,
        unp_reader: Option<&'a mut StringReader<'a>>,
        buf: &'a mut [u8],
        has_unpack_info: bool,
        covered_bitmap: Option<&'a [u64]>,
        hidden_pk_exists: bool,
    ) -> Self {
        let _ = (key_def, pack_info, reader, unp_reader, buf, has_unpack_info,
                 covered_bitmap, hidden_pk_exists);
        todo!("port C++ ctor at rdb_datadic.cc:199 — initialize from key_def.key_parts")
    }

    /// Returns true if there are more fields to advance past.
    /// C++: rdb_datadic.cc:235.
    pub fn has_next(&self) -> bool {
        self.iter_index < self.iter_end
    }

    /// Advance one field. Each call either:
    /// - Materializes the field into `self.buf[self.offset..]` (returning
    ///   a `FieldStep` with `field_index = Some(...)`), then returns.
    /// - Skips an uncovered field and continues the loop.
    /// - Hits the hidden-PK suffix and returns with `field_index = None`.
    ///
    /// Errors: `slatedb::Error::data` for corrupt key bytes.
    /// C++: rdb_datadic.cc:240.
    pub fn next(&mut self) -> Result<Option<FieldStep>, Error> {
        let _ = IndexType::HiddenPrimary;
        todo!("port C++ next() at rdb_datadic.cc:240")
    }

    pub fn get_field_index(&self) -> Option<u32> { self.current_field_index }
    pub fn get_is_null(&self) -> bool { self.current_is_null }
    pub fn get_offset(&self) -> usize { self.offset }
}
