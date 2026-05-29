//! Interface stub for `Rdb_convert_to_record_key_decoder`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.cc` (lines 78..197)
//! C++ header: `storage/rocksdb/rdb_datadic.h` (Rdb_convert_to_record_key_decoder)
//! v4 manifest sub-unit: `Rdb_convert_to_record_key_decoder`
//! Parent unit: `rdb_datadic.cc`
//! Approx body LoC accounted for here: ~120
//!
//! ## Mapping
//! Per _DESIGN.md §2 (key encoding preserved). This is a small adapter that
//! sits between `Rdb_key_field_iterator::next()` and the per-type unpack
//! routines in `Rdb_key_def__decode`. It handles:
//!
//! - The NULL-prefix byte for nullable columns.
//! - Routing to `fpi.unpack_func` or `fpi.skip_func` based on whether the
//!   caller wants the column materialized or merely advanced past.
//! - The MariaDB `Field::move_field` shenanigans for writing into a
//!   non-record-zero buffer.
//!
//! In Rust we replace `Field::move_field` with a direct offset write: the
//! caller passes `buf: &mut [u8]` and the offset within `buf` to write to.
//! No allocation, no MariaDB-internal pointer juggling.
//!
//! ## Out-of-scope methods
//! None. All 3 static methods translate directly.

use slatedb::Error;

use crate::rdb_buff_h::StringReader;
use crate::Rdb_field_packing::FieldPacking;

pub struct ConvertToRecordKeyDecoder;

impl ConvertToRecordKeyDecoder {
    /// Decode one field from the key reader into the MySQL record buffer.
    ///
    /// Inputs:
    /// - `fpi`: per-column descriptor (carries unpack-func, nullability).
    /// - `out_field`: destination slice within the record buffer (
    ///   `&mut record[field_offset..field_offset+field_len]`).
    /// - `default_value`: bytes to copy in if the value turns out NULL.
    /// - `reader`: positioned at the start of this field's bytes in the key.
    /// - `unp_reader`: optional unpack-info reader; may be `None` if the
    ///   index has no unpack_info.
    ///
    /// Returns: `Ok(())` on success.
    /// Errors: `slatedb::Error::data(...)` for corrupt input — maps to
    /// MyRocks `HA_ERR_ROCKSDB_CORRUPT_DATA`. EOF in the reader is also
    /// reported as corrupt-data.
    ///
    /// C++: rdb_datadic.cc:78.
    pub fn decode_field(
        _fpi: &FieldPacking,
        _out_field: &mut [u8],
        _set_null: &mut bool,
        _default_value: &[u8],
        _reader: &mut StringReader,
        _unp_reader: Option<&mut StringReader>,
    ) -> Result<(), Error> {
        todo!("port C++ decode_field at rdb_datadic.cc:78")
    }

    /// Decode one field into `buf` at `*offset`, advancing the offset
    /// pointer to the end of the written bytes.
    ///
    /// Translated contract: takes `&mut [u8]` buf + `&mut usize` offset.
    /// Internally dispatches to `decode_field`.
    ///
    /// C++: rdb_datadic.cc:118.
    pub fn decode(
        _buf: &mut [u8],
        _offset: &mut usize,
        _fpi: &FieldPacking,
        _field_index: u32,
        _has_unpack_info: bool,
        _reader: &mut StringReader,
        _unp_reader: Option<&mut StringReader>,
    ) -> Result<(), Error> {
        todo!("port C++ decode at rdb_datadic.cc:118")
    }

    /// Skip past one field's bytes in the reader (no materialization).
    /// Used when the SQL layer doesn't need this column for the current
    /// query (covering-index, partial-row decode).
    ///
    /// C++: rdb_datadic.cc:163.
    pub fn skip(
        _fpi: &FieldPacking,
        _reader: &mut StringReader,
        _unp_reader: Option<&mut StringReader>,
    ) -> Result<(), Error> {
        todo!("port C++ skip at rdb_datadic.cc:163")
    }
}
