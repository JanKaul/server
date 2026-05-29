//! Interface stub for `rdb_datadic_h__Rdb_key_field_iterator`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (lines 99..136)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_key_field_iterator`
//! Parent unit: `rdb_datadic_h`
//! Approx body LoC accounted for here: 38
//!
//! ## Mapping
//! In-scope codec types — preserved from MyRocks per _DESIGN.md §2/§3. This
//! iterator walks the parts of a memcomparable key in order, calling the
//! per-field decoder from `Rdb_convert_to_record_key_decoder` and tracking
//! covered-bitmap / hidden-PK / null-bit state across the loop. It is the
//! shared driver loop for "unpack a key into a record buffer".
//!
//! In Rust this becomes a struct holding cursors + state, plus inherent
//! methods that mirror the C++ ones. We intentionally do NOT implement
//! `std::iter::Iterator` because the C++ `next()` returns a status int and
//! mutates many out-parameters at once — wrapping it would distort the
//! contract. A future refactor MAY introduce a `Result<Item, Error>`-yielding
//! `Iterator` once we know what `Item` should hold.
//!
//! ## Out-of-scope methods
//! None — every public method is reproduced. The state fields are all
//! `pub(crate)` because the iterator is consumed only by callers in this
//! same crate (the handler-side `convert_record_from_storage_format`).

use slatedb::Error;

use crate::rdb_buff_h::StringReader;
use crate::rdb_datadic_h__Rdb_convert_to_record_key_decoder::{
    FieldPacking, FieldView, RowBytes, TableShareView,
};

/// Opaque forward to the future Rdb_key_def stub (the "meta" half).
pub struct KeyDef; // TODO(human): wire to rdb_datadic_h__Rdb_key_def stub

/// POD substitute for MySQL's `MY_BITMAP*` — the covered-columns bitmap that
/// secondary indexes carry to indicate which columns are reconstructible from
/// the index alone (covering-index fast path).
pub struct CoveredBitmap<'a> {
    /// Backing storage; bit `i` (LSB-first within each byte) indicates field
    /// `i` is covered.
    pub bits: &'a [u8],
}

/// Forward-only iterator over the key-parts of one memcomparable key.
///
/// The iterator holds cursors into both the main key reader and the
/// (optional) unpack_info reader, plus enough table/key-def state to dispatch
/// the right per-field decoder.
///
/// Original: rdb_datadic.h:99 — `class Rdb_key_field_iterator`.
pub struct KeyFieldIterator<'a, 'b> {
    pub(crate) pack_info: &'a mut [FieldPacking],
    pub(crate) iter_index: i32,
    pub(crate) iter_end: i32,
    pub(crate) table: &'a TableShareView,
    pub(crate) reader: &'a mut StringReader<'b>,
    pub(crate) unp_reader: Option<&'a mut StringReader<'b>>,
    pub(crate) curr_bitmap_pos: u32,
    pub(crate) covered_bitmap: Option<&'a CoveredBitmap<'a>>,
    pub(crate) buf: RowBytes<'a>,
    pub(crate) has_unpack_info: bool,
    pub(crate) key_def: &'a KeyDef,
    pub(crate) secondary_key: bool,
    pub(crate) hidden_pk_exists: bool,
    pub(crate) is_hidden_pk: bool,
    pub(crate) is_null: bool,
    pub(crate) field: Option<&'a mut FieldView>,
    pub(crate) offset: u32,
    pub(crate) fpi: Option<&'a mut FieldPacking>,
}

impl<'a, 'b> KeyFieldIterator<'a, 'b> {
    /// Construct.
    ///
    /// **Inputs:** every field of the iterator state. The C++ ctor takes the
    /// same set of pointers; we keep argument order to ease translation.
    /// **Errors:** none — pure assignment.
    ///
    /// Original: rdb_datadic.h:123 — ctor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        _key_def: &'a KeyDef,
        _pack_info: &'a mut [FieldPacking],
        _reader: &'a mut StringReader<'b>,
        _unp_reader: Option<&'a mut StringReader<'b>>,
        _table: &'a TableShareView,
        _has_unpack_info: bool,
        _covered_bitmap: Option<&'a CoveredBitmap<'a>>,
        _buf: RowBytes<'a>,
    ) -> Self {
        todo!("port Rdb_key_field_iterator ctor — derives iter_index/iter_end \
               from key_def->m_key_parts and hidden-PK presence")
    }

    /// Advance to the next field, decoding it into `buf`.
    ///
    /// **Output:** `HA_EXIT_SUCCESS` (0) on success, or an `HA_ERR_*` code on
    /// failure (mirrors C++ contract; the handler layer translates to
    /// `Result<(), Error>` at the boundary).
    /// **Errors:** `HA_ERR_INTERNAL_ERROR` on a truncated key.
    /// **Invariants:** advances `m_iter_index` by exactly 1 on success.
    ///
    /// Original: rdb_datadic.h:130 — `next`.
    pub fn next(&mut self) -> i32 {
        todo!("port Rdb_key_field_iterator::next from rdb_datadic.cc")
    }

    /// True if there is at least one more part left to decode.
    /// Original: rdb_datadic.h:131 — `has_next`.
    pub fn has_next(&self) -> bool {
        self.iter_index < self.iter_end
    }

    /// True if the most recently decoded part is SQL NULL.
    /// Original: rdb_datadic.h:132 — `get_is_null`.
    pub fn get_is_null(&self) -> bool { self.is_null }

    /// Borrow the field descriptor of the most recently decoded part.
    /// Returns `None` before the first `next()` call.
    /// Original: rdb_datadic.h:133 — `get_field`.
    pub fn get_field(&self) -> Option<&FieldView> {
        self.field.as_deref()
    }

    /// Index of the field within the TABLE's field list. Returns -1 before
    /// the first `next()` and for the hidden-PK part.
    /// Original: rdb_datadic.h:134 — `get_field_index`.
    pub fn get_field_index(&self) -> i32 {
        todo!("derive from self.field's m_field_index slot")
    }

    /// Pointer/offset of the destination byte slot in `buf` for the most
    /// recently decoded part.
    /// Original: rdb_datadic.h:135 — `get_dst`.
    pub fn get_dst(&self) -> usize { self.offset as usize }
}
