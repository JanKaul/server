//! Row-value codec — encodes a row's non-PK column data into the
//! SlateDB value blob that pairs with the PK row-key.
//!
//! Counterpart of MyRocks' `Rdb_converter::encode_value_slice`
//! (`rdb_converter.cc:688`). Stage 0 lands the **layout primitives**
//! used by the encoder (null bitmap), not the encoder itself —
//! per-field byte writing needs the cxx Field surface (`field_ptr_bytes`)
//! and PK exclusion needs the wired primary-key-keypart→field-index
//! mapping that `KeyDef::setup` populates.
//!
//! ## Wire format (target — for Stage 0 and beyond)
//!
//! ```text
//! [ null bitmap bytes ]  (ceil(nullable-non-PK-count / 8) bytes;
//!                         each bit = 1 iff the corresponding
//!                         nullable non-PK field is SQL NULL)
//! [ unpack_info? ]       (variable, from PK pack's side channel;
//!                         present only if PK produced one)
//! [ field bytes... ]     (raw on-record bytes per non-PK field, in
//!                         declaration order; for NULL fields nothing
//!                         is appended — readers consult the bitmap)
//! [ debug checksum? ]    (1 byte if storage-record checksums enabled)
//! ```
//!
//! This module currently handles the **null bitmap** portion — the
//! layout computation (which fields go where) and the bit-set /
//! bit-test helpers. The per-field byte writer and the overall
//! encoder are follow-up slices.

/// Position of one nullable-non-PK field within the value-blob's
/// leading null bitmap. The bitmap is bit-addressed in MariaDB's
/// usual layout: bit `n` of byte `b` corresponds to the
/// `(b * 8 + n)`-th nullable non-PK field in declaration order.
///
/// `bit_mask` is one of `0x01, 0x02, 0x04, ..., 0x80`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NullBitPosition {
    pub byte_offset: u32,
    pub bit_mask: u8,
}

/// Computed layout of a table's value-blob null bitmap. The
/// `field_positions` vec has one slot per declared field
/// (length = `field_count` from the input); a `Some(NullBitPosition)`
/// means "this field participates in the bitmap"; `None` means it
/// doesn't (either non-nullable, or excluded because it's a PK
/// column — PK fields live in the row key, not the value).
#[derive(Debug, Clone)]
pub struct ValueNullBitmapLayout {
    /// Total bytes the bitmap consumes at the head of the value
    /// blob. Zero when no field qualifies.
    pub bitmap_bytes: u32,
    /// Per-field assignment. `field_positions.len() == field_count`.
    pub field_positions: Vec<Option<NullBitPosition>>,
}

/// Compute the value-blob null bitmap layout for a table.
///
/// Walks fields in declaration order. Each field that is BOTH
/// nullable AND not part of the primary key gets the next available
/// bit position. Other fields get `None`.
///
/// Total bitmap byte count is the ceiling of (number of
/// participating fields) / 8. Returns 0 bytes when the table has
/// no nullable non-PK columns.
///
/// Counterpart of MyRocks' `m_null_bytes_length_in_record`
/// computation in `Rdb_converter::setup_field_decoders`.
pub fn compute_value_null_bitmap_layout(
    field_count: u32,
    is_nullable: impl Fn(u32) -> bool,
    is_in_pk: impl Fn(u32) -> bool,
) -> ValueNullBitmapLayout {
    let mut field_positions: Vec<Option<NullBitPosition>> =
        Vec::with_capacity(field_count as usize);
    let mut next_bit: u32 = 0;
    for i in 0..field_count {
        if is_nullable(i) && !is_in_pk(i) {
            field_positions.push(Some(NullBitPosition {
                byte_offset: next_bit / 8,
                bit_mask: 1u8 << (next_bit % 8),
            }));
            next_bit += 1;
        } else {
            field_positions.push(None);
        }
    }
    ValueNullBitmapLayout {
        bitmap_bytes: next_bit.div_ceil(8),
        field_positions,
    }
}

/// Set the null bit for `field_idx` in `bitmap`. No-op if the field
/// has no bitmap position (non-nullable or PK column).
///
/// Caller is responsible for `bitmap.len() >= layout.bitmap_bytes`.
pub fn set_null_bit(
    bitmap: &mut [u8],
    layout: &ValueNullBitmapLayout,
    field_idx: u32,
) {
    if let Some(Some(pos)) = layout.field_positions.get(field_idx as usize) {
        bitmap[pos.byte_offset as usize] |= pos.bit_mask;
    }
}

/// Read the null bit for `field_idx` from `bitmap`. Returns `false`
/// if the field has no bitmap position (treats non-nullable as
/// not-null; treats out-of-range as not-null).
pub fn is_null_bit_set(
    bitmap: &[u8],
    layout: &ValueNullBitmapLayout,
    field_idx: u32,
) -> bool {
    let Some(Some(pos)) = layout.field_positions.get(field_idx as usize) else {
        return false;
    };
    let Some(byte) = bitmap.get(pos.byte_offset as usize) else {
        return false;
    };
    (byte & pos.bit_mask) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_no_fields_gives_zero_bytes() {
        let layout = compute_value_null_bitmap_layout(0, |_| true, |_| false);
        assert_eq!(layout.bitmap_bytes, 0);
        assert_eq!(layout.field_positions.len(), 0);
    }

    #[test]
    fn layout_only_non_nullable_fields_gives_zero_bytes() {
        let layout = compute_value_null_bitmap_layout(5, |_| false, |_| false);
        assert_eq!(layout.bitmap_bytes, 0);
        assert_eq!(layout.field_positions.len(), 5);
        assert!(layout.field_positions.iter().all(|p| p.is_none()));
    }

    #[test]
    fn layout_one_nullable_field_gets_bit_zero() {
        let layout = compute_value_null_bitmap_layout(1, |_| true, |_| false);
        assert_eq!(layout.bitmap_bytes, 1);
        assert_eq!(
            layout.field_positions[0],
            Some(NullBitPosition { byte_offset: 0, bit_mask: 0x01 }),
        );
    }

    #[test]
    fn layout_eight_nullable_fields_fit_in_one_byte() {
        let layout = compute_value_null_bitmap_layout(8, |_| true, |_| false);
        assert_eq!(layout.bitmap_bytes, 1);
        for (i, p) in layout.field_positions.iter().enumerate() {
            let want = NullBitPosition {
                byte_offset: 0,
                bit_mask: 1u8 << i,
            };
            assert_eq!(*p, Some(want), "field {i}");
        }
    }

    #[test]
    fn layout_nine_nullable_fields_use_two_bytes() {
        let layout = compute_value_null_bitmap_layout(9, |_| true, |_| false);
        assert_eq!(layout.bitmap_bytes, 2);
        assert_eq!(
            layout.field_positions[8],
            Some(NullBitPosition { byte_offset: 1, bit_mask: 0x01 }),
        );
    }

    #[test]
    fn layout_excludes_pk_columns_from_bitmap() {
        // 4 fields, all nullable. Fields 0 and 2 are PK columns
        // (excluded from value blob). Bitmap covers only fields 1
        // and 3.
        let pk_set = [true, false, true, false];
        let layout = compute_value_null_bitmap_layout(
            4,
            |_| true,
            |i| pk_set[i as usize],
        );
        assert_eq!(layout.bitmap_bytes, 1);
        assert_eq!(layout.field_positions[0], None); // PK excluded
        assert_eq!(
            layout.field_positions[1],
            Some(NullBitPosition { byte_offset: 0, bit_mask: 0x01 }),
        );
        assert_eq!(layout.field_positions[2], None); // PK excluded
        assert_eq!(
            layout.field_positions[3],
            Some(NullBitPosition { byte_offset: 0, bit_mask: 0x02 }),
        );
    }

    #[test]
    fn layout_mixed_nullable_and_pk_assigns_only_qualifying_fields() {
        // 10 fields. Nullable iff i is even. PK iff i in {0, 2}.
        // Qualifying (nullable && !PK): i in {4, 6, 8} → 3 bits, 1 byte.
        let layout = compute_value_null_bitmap_layout(
            10,
            |i| i % 2 == 0,
            |i| i == 0 || i == 2,
        );
        assert_eq!(layout.bitmap_bytes, 1);
        let assigned: Vec<u32> = layout
            .field_positions
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.map(|_| i as u32))
            .collect();
        assert_eq!(assigned, vec![4, 6, 8]);
        // Bits assigned sequentially: 4 → bit0, 6 → bit1, 8 → bit2.
        assert_eq!(layout.field_positions[4].unwrap().bit_mask, 0x01);
        assert_eq!(layout.field_positions[6].unwrap().bit_mask, 0x02);
        assert_eq!(layout.field_positions[8].unwrap().bit_mask, 0x04);
    }

    #[test]
    fn set_and_test_null_bit_round_trip() {
        let layout = compute_value_null_bitmap_layout(3, |_| true, |_| false);
        let mut bitmap = vec![0u8; layout.bitmap_bytes as usize];

        // Initially nothing is null.
        for i in 0..3 {
            assert!(!is_null_bit_set(&bitmap, &layout, i));
        }

        // Set field 1.
        set_null_bit(&mut bitmap, &layout, 1);
        assert!(!is_null_bit_set(&bitmap, &layout, 0));
        assert!(is_null_bit_set(&bitmap, &layout, 1));
        assert!(!is_null_bit_set(&bitmap, &layout, 2));

        // Setting again is idempotent (OR with same mask).
        set_null_bit(&mut bitmap, &layout, 1);
        assert!(is_null_bit_set(&bitmap, &layout, 1));
    }

    #[test]
    fn set_null_bit_on_non_bitmap_field_is_noop() {
        // Field 0 is non-nullable; field 1 is nullable.
        let nullable = [false, true];
        let layout = compute_value_null_bitmap_layout(2, |i| nullable[i as usize], |_| false);
        let mut bitmap = vec![0u8; layout.bitmap_bytes as usize];

        // Setting the non-bitmap field doesn't touch any bytes.
        set_null_bit(&mut bitmap, &layout, 0);
        assert_eq!(bitmap, vec![0u8]);

        // Setting out-of-range is also a no-op.
        set_null_bit(&mut bitmap, &layout, 99);
        assert_eq!(bitmap, vec![0u8]);
    }

    #[test]
    fn is_null_bit_set_returns_false_for_out_of_range_field() {
        let layout = compute_value_null_bitmap_layout(2, |_| true, |_| false);
        let bitmap = vec![0xFFu8; layout.bitmap_bytes as usize];
        // Out-of-range field idx: treats as not-null.
        assert!(!is_null_bit_set(&bitmap, &layout, 99));
    }

    #[test]
    fn is_null_bit_set_returns_false_for_short_bitmap() {
        let layout = compute_value_null_bitmap_layout(8, |_| true, |_| false);
        // bitmap is 0 bytes (caller bug); reads return false rather
        // than panicking.
        let bitmap: Vec<u8> = vec![];
        for i in 0..8 {
            assert!(!is_null_bit_set(&bitmap, &layout, i));
        }
    }
}
