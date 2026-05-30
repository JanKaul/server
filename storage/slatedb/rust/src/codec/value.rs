//! Codec boundary types — Rust side of the cxx shim.
//!
//! Per `_DESIGN.md §2 + §3`, the codec preserves the MyRocks on-disk row
//! and key layouts bit-for-bit. To do that without leaking MariaDB-internal
//! C++ types (`TABLE*`, `Field*`, `KEY*`) across the cxx bridge, the shim
//! marshals the metadata the codec actually needs into the POD types
//! defined here.
//!
//! Today this module is **definitions only** — there is no packing /
//! unpacking logic. The codec encode/decode helpers (`FieldPacking`,
//! `decode_field`, `pack_record`, …) consume these types and are landed
//! in separate batches.
//!
//! ## Field set rationale
//!
//! The C++ `Field*` exposes ~40 accessor methods. We carry only what the
//! codec reads:
//! - `mysql_type` — selects the encoding rules
//! - `pack_length` — bytes the field occupies in the row buffer
//! - `output_offset` — where in the row buffer this field's bytes land
//! - `null_marker` — `(byte_offset, bit_mask)` for the row's null bitmap;
//!   `None` for NOT NULL columns
//! - `charset_id` — collation id for string types
//! - `flags` — `UNSIGNED_FLAG`, `ZEROFILL_FLAG`, `BLOB_FLAG`, etc.
//! - `length` — declared max byte length
//! - `decimals` — fractional-digit count for DECIMAL / time-with-fsp types
//!
//! Adding a field is cheap (just extend the struct + bridge marshaller),
//! removing one is expensive (touches the cxx surface), so the bar for
//! inclusion is "the codec actually reads it today."

use bytes::BytesMut;

/// Subset of MariaDB's `enum_field_types` that MyRocks actually packs.
/// Numeric values are stable so the cxx shim can pass them as `u8`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MysqlType {
    Decimal = 0,
    Tiny = 1,
    Short = 2,
    Long = 3,
    Float = 4,
    Double = 5,
    Null = 6,
    Timestamp = 7,
    LongLong = 8,
    Int24 = 9,
    Date = 10,
    Time = 11,
    DateTime = 12,
    Year = 13,
    NewDate = 14,
    Varchar = 15,
    Bit = 16,
    Timestamp2 = 17,
    DateTime2 = 18,
    Time2 = 19,
    NewDecimal = 246,
    Enum = 247,
    Set = 248,
    TinyBlob = 249,
    MediumBlob = 250,
    LongBlob = 251,
    Blob = 252,
    VarString = 253,
    String = 254,
    Geometry = 255,
}

/// `UNSIGNED_FLAG` from MariaDB's `mysql_com.h`. Marshalled in via the
/// cxx shim from `Field::flags`. Used by callers like
/// `KeyDef::extract_ttl_col` that need to validate column attributes
/// without re-touching the bridge.
pub const UNSIGNED_FLAG: u32 = 32;

/// Per-column field descriptor. POD across the cxx boundary.
///
/// All offsets are in bytes within the row buffer; `null_marker` selects
/// one bit within the row's leading null bitmap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldView {
    /// Column name as declared in the table — case-sensitive ASCII match
    /// per MariaDB conventions. Carried so callers like
    /// `KeyDef::extract_ttl_col` can resolve `ttl_col=NAME` qualifiers
    /// without an extra bridge call.
    pub name: String,
    pub mysql_type: MysqlType,
    /// Bytes this field occupies in the row buffer (the in-memory MySQL
    /// row format, not the on-disk encoding).
    pub pack_length: u32,
    /// Offset within the row buffer where this field's bytes land.
    pub output_offset: u32,
    /// `(null_byte_offset, null_bit_mask)` within the row's leading null
    /// bitmap; `None` for NOT NULL columns.
    pub null_marker: Option<(u32, u8)>,
    /// Declared maximum byte length. Drives variable-length encoding
    /// (VARCHAR, BLOB).
    pub length: u32,
    /// Collation id for string types; meaningless for numerics.
    pub charset_id: u32,
    /// `UNSIGNED_FLAG`, `ZEROFILL_FLAG`, `BLOB_FLAG`, etc. Mirrors
    /// MariaDB's `Field::flags`.
    pub flags: u32,
    /// Fractional-digit count for DECIMAL / fractional-second time types.
    pub decimals: u8,
}

impl FieldView {
    /// True iff this column declared NOT NULL.
    pub fn is_not_null(&self) -> bool {
        self.null_marker.is_none()
    }

    /// Read the null bit for this column out of a row's null bitmap.
    /// Returns `false` when the column is NOT NULL.
    pub fn is_null_in_row(&self, row: &[u8]) -> bool {
        match self.null_marker {
            Some((byte_offset, bit_mask)) => {
                row.get(byte_offset as usize)
                    .map(|b| (b & bit_mask) != 0)
                    .unwrap_or(false)
            }
            None => false,
        }
    }

    /// Set the null bit for this column in a row's null bitmap. No-op for
    /// NOT NULL columns. Caller is responsible for sizing the buffer.
    pub fn set_null_in_row(&self, row: &mut [u8], is_null: bool) {
        if let Some((byte_offset, bit_mask)) = self.null_marker {
            let byte = &mut row[byte_offset as usize];
            if is_null {
                *byte |= bit_mask;
            } else {
                *byte &= !bit_mask;
            }
        }
    }
}

/// Per-table layout descriptor. POD across the cxx boundary.
///
/// Holds the field list and the row-layout constants the codec needs.
/// Keys are described by separate per-index types (KeyDef → FieldPacking)
/// landed in later batches.
#[derive(Debug, Clone)]
pub struct TableShareView {
    pub fields: Vec<FieldView>,
    /// Bytes of null bitmap at the start of every row buffer. Always
    /// `(nullable_field_count + 7) / 8`.
    pub null_bytes: u32,
    /// Total bytes of a packed row (`null_bytes + sum(pack_length)`).
    pub row_length: u32,
    /// Index into `fields` of the column whose `output_offset` is the
    /// hidden-PK rowid, or `None` if the table declares an explicit PK.
    pub hidden_pk_field: Option<u32>,
}

impl TableShareView {
    /// Allocate a fresh, zero-filled row buffer sized for this table.
    pub fn new_row_buffer(&self) -> BytesMut {
        BytesMut::zeroed(self.row_length as usize)
    }

    /// Number of NULLable columns. Equal to the `Some` count of every
    /// field's `null_marker`.
    pub fn nullable_field_count(&self) -> u32 {
        self.fields.iter().filter(|f| !f.is_not_null()).count() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nullable_int(output_offset: u32, null_byte: u32, null_bit: u8) -> FieldView {
        FieldView {
            name: "i".into(),
            mysql_type: MysqlType::Long,
            pack_length: 4,
            output_offset,
            null_marker: Some((null_byte, null_bit)),
            length: 4,
            charset_id: 63, // binary
            flags: 0,
            decimals: 0,
        }
    }

    fn not_null_int(output_offset: u32) -> FieldView {
        FieldView {
            name: "i".into(),
            mysql_type: MysqlType::Long,
            pack_length: 4,
            output_offset,
            null_marker: None,
            length: 4,
            charset_id: 63,
            flags: 0,
            decimals: 0,
        }
    }

    #[test]
    fn mysql_type_repr_is_stable() {
        // These values cross the cxx boundary; changing them is a wire
        // break. Pin a few load-bearing ones.
        assert_eq!(MysqlType::Tiny as u8, 1);
        assert_eq!(MysqlType::LongLong as u8, 8);
        assert_eq!(MysqlType::NewDecimal as u8, 246);
        assert_eq!(MysqlType::Blob as u8, 252);
        assert_eq!(MysqlType::Geometry as u8, 255);
    }

    #[test]
    fn is_null_in_row_reads_the_named_bit() {
        let f = nullable_int(1, 0, 0b0000_0100);
        let mut row = vec![0u8; 5];
        assert!(!f.is_null_in_row(&row));
        row[0] = 0b0000_0100;
        assert!(f.is_null_in_row(&row));
        // Adjacent bits don't accidentally match.
        row[0] = 0b1111_1011;
        assert!(!f.is_null_in_row(&row));
    }

    #[test]
    fn is_null_in_row_for_not_null_is_always_false() {
        let f = not_null_int(0);
        let row = vec![0xffu8; 4];
        assert!(!f.is_null_in_row(&row));
    }

    #[test]
    fn set_null_in_row_flips_only_the_named_bit() {
        let f = nullable_int(1, 0, 0b0001_0000);
        let mut row = vec![0b1010_1010, 0, 0, 0, 0];
        f.set_null_in_row(&mut row, true);
        assert_eq!(row[0], 0b1011_1010);
        f.set_null_in_row(&mut row, false);
        assert_eq!(row[0], 0b1010_1010);
    }

    #[test]
    fn set_null_in_row_for_not_null_is_a_noop() {
        let f = not_null_int(0);
        let mut row = vec![0u8; 4];
        f.set_null_in_row(&mut row, true);
        assert_eq!(row, vec![0u8; 4]);
    }

    #[test]
    fn nullable_field_count_matches_marker_count() {
        let t = TableShareView {
            fields: vec![
                not_null_int(1),
                nullable_int(5, 0, 0b0000_0001),
                nullable_int(9, 0, 0b0000_0010),
                not_null_int(13),
            ],
            null_bytes: 1,
            row_length: 17,
            hidden_pk_field: None,
        };
        assert_eq!(t.nullable_field_count(), 2);
    }

    #[test]
    fn new_row_buffer_is_zero_filled_and_sized() {
        let t = TableShareView {
            fields: vec![not_null_int(0)],
            null_bytes: 0,
            row_length: 4,
            hidden_pk_field: None,
        };
        let buf = t.new_row_buffer();
        assert_eq!(buf.len(), 4);
        assert!(buf.iter().all(|&b| b == 0));
    }
}
