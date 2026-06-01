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

/// Source of per-field info the value-blob encoder needs. The cxx
/// wrapper implements this against a live `TableRef`; tests
/// implement it with mock data.
///
/// Methods take `field_idx` referring to the slot in the table's
/// field declaration order — same indexing as the
/// [`ValueNullBitmapLayout::field_positions`] vec.
pub trait RowValueSource {
    /// True iff this field is a PK keypart and must NOT appear in
    /// the value blob (PK fields live in the row key and are
    /// reconstructed from there on read).
    fn is_in_pk(&self, field_idx: u32) -> bool;

    /// True iff this field currently holds SQL NULL.
    fn is_null(&self, field_idx: u32) -> bool;

    /// Number of bytes this field contributes when non-null
    /// (MariaDB `Field::pack_length()`).
    fn pack_length(&self, field_idx: u32) -> u32;

    /// Write `pack_length(field_idx)` bytes for this non-null
    /// field into the front of `dst`. Returns the actual count
    /// written — must equal `pack_length(field_idx)`; the encoder
    /// rejects a mismatch as corruption.
    fn write_field_bytes(
        &mut self,
        field_idx: u32,
        dst: &mut [u8],
    ) -> Result<usize, slatedb::Error>;
}

/// Encode a row's value blob into `dst` from the row's column
/// data. Counterpart of MyRocks' `Rdb_converter::encode_value_slice`
/// at `rdb_converter.cc:688`.
///
/// Wire format (Stage 0):
/// ```text
/// [ null bitmap: layout.bitmap_bytes bytes ]
/// [ per-non-PK-field bytes... ]
/// ```
/// `layout` must have been computed by [`compute_value_null_bitmap_layout`]
/// for this table's schema; `field_count` is the total number of
/// declared fields (matching `layout.field_positions.len()`).
///
/// Iteration is in field declaration order. For each field:
/// - If `source.is_in_pk(i)`: skipped (PK fields aren't in the
///   value blob).
/// - Else if `source.is_null(i)`: the null bit is set in the
///   bitmap; no bytes are appended.
/// - Else: `source.pack_length(i)` bytes are appended (via
///   `source.write_field_bytes`).
///
/// Returns the number of bytes appended to `dst`.
///
/// ## Stage 0 omissions (each documented at the deferral site
/// in MyRocks)
///
/// - TTL prefix bytes — written before the null bitmap when the
///   table has `ttl_duration > 0` (`rdb_converter.cc:704`).
/// - unpack_info block from the PK pack's side channel — appended
///   between the bitmap and the field bytes
///   (`rdb_converter.cc:759`).
/// - Debug checksum suffix — appended after the field bytes when
///   `store_row_debug_checksums` is on.
///
/// All three land alongside their producers (TTL plumbing,
/// pack_record unpack_info writer, checksum sysvar).
///
/// ## Errors
///
/// - `Invalid` if `source.write_field_bytes` returns a byte count
///   different from `pack_length(field_idx)` (encoder-side
///   sanity check on the source contract).
/// - Whatever `source.write_field_bytes` returns.
pub fn encode_row_value(
    layout: &ValueNullBitmapLayout,
    field_count: u32,
    source: &mut dyn RowValueSource,
    dst: &mut Vec<u8>,
) -> Result<usize, slatedb::Error> {
    let start = dst.len();
    let bitmap_start = start;

    // Reserve the leading null-bitmap region (zeroed). Later
    // `set_null_bit` writes land here as we discover NULL fields.
    dst.resize(start + layout.bitmap_bytes as usize, 0);

    for i in 0..field_count {
        if source.is_in_pk(i) {
            continue;
        }
        if source.is_null(i) {
            let bm_end = bitmap_start + layout.bitmap_bytes as usize;
            set_null_bit(&mut dst[bitmap_start..bm_end], layout, i);
            continue;
        }
        let n = source.pack_length(i) as usize;
        let write_start = dst.len();
        dst.resize(write_start + n, 0);
        let written =
            source.write_field_bytes(i, &mut dst[write_start..write_start + n])?;
        if written != n {
            return Err(slatedb::Error::invalid(format!(
                "encode_row_value: field {i} source wrote {written} bytes, \
                 expected pack_length {n}",
            )));
        }
    }

    Ok(dst.len() - start)
}

/// Sink for the value-blob decoder. Cxx-wrapper counterpart
/// implements the trait by calling `field_set_null` /
/// `field_set_value` on a live `Pin<&mut FieldRef>`. Tests use a
/// mock.
///
/// Methods take `field_idx` referring to the slot in the table's
/// field declaration order — same indexing as
/// [`RowValueSource`].
pub trait RowValueSink {
    /// True iff this field is a PK keypart and therefore lives in
    /// the row key, not the value blob. The decoder skips PK
    /// fields entirely — the caller reconstructs them from the
    /// row key separately.
    fn is_in_pk(&self, field_idx: u32) -> bool;

    /// Number of bytes the field consumes from the value blob
    /// when non-null. Must match the encoder's
    /// `RowValueSource::pack_length` for this field.
    fn pack_length(&self, field_idx: u32) -> u32;

    /// Mark this field as SQL NULL. Called when the corresponding
    /// null bit is set in the bitmap; no bytes are consumed.
    fn set_null(&mut self, field_idx: u32);

    /// Copy `src` (exactly `pack_length(field_idx)` bytes) into
    /// the field's storage. Called for non-null non-PK fields.
    fn set_field_bytes(
        &mut self,
        field_idx: u32,
        src: &[u8],
    ) -> Result<(), slatedb::Error>;
}

/// Decode a value blob written by [`encode_row_value`] back into
/// the row's columns via `sink`. Counterpart of MyRocks'
/// `Rdb_converter::decode` at `rdb_converter.cc` (the
/// per-row decoder loop).
///
/// Wire format (Stage 0 — same as the encoder):
/// ```text
/// [ null bitmap: layout.bitmap_bytes bytes ]
/// [ per-non-PK-field bytes... ]
/// ```
///
/// For each field in declaration order:
/// - If `sink.is_in_pk(i)`: skipped (PK fields are decoded from
///   the row key by the caller).
/// - Else if the null bit is set in the bitmap header: invokes
///   `sink.set_null(i)`; no bytes consumed.
/// - Else: consumes `sink.pack_length(i)` bytes from the input
///   and invokes `sink.set_field_bytes(i, slice)`.
///
/// Returns the number of bytes consumed from `src`. A well-formed
/// value blob is consumed exactly; trailing bytes (if any) are
/// ignored at this level (the caller can detect them by comparing
/// against `src.len()`).
///
/// ## Errors
///
/// - `Data` if `src` is shorter than the null bitmap or runs out
///   mid-field (truncated value blob — corruption or schema drift).
/// - Whatever `sink.set_field_bytes` returns.
///
/// ## Stage 0 omissions
///
/// Symmetric with the encoder — no TTL prefix, no unpack_info
/// block, no debug checksum suffix. Decode of any of those would
/// need to slot in BEFORE the null bitmap (TTL), BETWEEN it and
/// the field bytes (unpack_info), or AFTER (checksum).
pub fn decode_row_value(
    layout: &ValueNullBitmapLayout,
    field_count: u32,
    sink: &mut dyn RowValueSink,
    src: &[u8],
) -> Result<usize, slatedb::Error> {
    let bitmap_bytes = layout.bitmap_bytes as usize;
    if src.len() < bitmap_bytes {
        return Err(slatedb::Error::data(format!(
            "decode_row_value: value blob too short for null bitmap — \
             have {} bytes, need {}",
            src.len(),
            bitmap_bytes,
        )));
    }
    let bitmap = &src[..bitmap_bytes];
    let mut cursor = bitmap_bytes;

    for i in 0..field_count {
        if sink.is_in_pk(i) {
            continue;
        }
        if is_null_bit_set(bitmap, layout, i) {
            sink.set_null(i);
            continue;
        }
        let n = sink.pack_length(i) as usize;
        if cursor.saturating_add(n) > src.len() {
            return Err(slatedb::Error::data(format!(
                "decode_row_value: value blob truncated at field {i} — \
                 have {} bytes remaining, need {n}",
                src.len() - cursor,
            )));
        }
        sink.set_field_bytes(i, &src[cursor..cursor + n])?;
        cursor += n;
    }

    Ok(cursor)
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

    // ----- encode_row_value -----

    /// Test fixture: pre-canned per-field state, mock pack_length,
    /// pre-canned field bytes. Implements `RowValueSource`.
    struct MockSource {
        is_in_pk: Vec<bool>,
        is_null: Vec<bool>,
        pack_length: Vec<u32>,
        // For each field, the bytes the source writes when asked.
        // Length must match pack_length[i].
        bytes: Vec<Vec<u8>>,
        // Counts of write_field_bytes calls per field index.
        calls: Vec<usize>,
        // Optional error to return on a specific field's write.
        error_on_field: Option<u32>,
        // Optional "wrong byte count" mode for the wrong-length test.
        short_write_on_field: Option<u32>,
    }

    impl RowValueSource for MockSource {
        fn is_in_pk(&self, i: u32) -> bool {
            self.is_in_pk[i as usize]
        }
        fn is_null(&self, i: u32) -> bool {
            self.is_null[i as usize]
        }
        fn pack_length(&self, i: u32) -> u32 {
            self.pack_length[i as usize]
        }
        fn write_field_bytes(
            &mut self,
            i: u32,
            dst: &mut [u8],
        ) -> Result<usize, slatedb::Error> {
            self.calls[i as usize] += 1;
            if Some(i) == self.error_on_field {
                return Err(slatedb::Error::invalid(
                    "MockSource: synthetic field write error".into(),
                ));
            }
            let src = &self.bytes[i as usize];
            if Some(i) == self.short_write_on_field {
                // Write one fewer byte than pack_length expects.
                let n = src.len().saturating_sub(1);
                dst[..n].copy_from_slice(&src[..n]);
                return Ok(n);
            }
            dst[..src.len()].copy_from_slice(src);
            Ok(src.len())
        }
    }

    fn mock(n: usize) -> MockSource {
        MockSource {
            is_in_pk: vec![false; n],
            is_null: vec![false; n],
            pack_length: vec![1; n],
            bytes: (0..n).map(|i| vec![i as u8]).collect(),
            calls: vec![0; n],
            error_on_field: None,
            short_write_on_field: None,
        }
    }

    #[test]
    fn encode_row_value_emits_only_bitmap_when_no_non_pk_fields() {
        let layout =
            compute_value_null_bitmap_layout(0, |_| false, |_| false);
        let mut src = mock(0);
        let mut dst = Vec::new();
        let n = encode_row_value(&layout, 0, &mut src, &mut dst).expect("enc");
        assert_eq!(n, 0);
        assert_eq!(dst, vec![] as Vec<u8>);
    }

    #[test]
    fn encode_row_value_single_non_nullable_field_appends_only_bytes() {
        // 1 field, non-nullable, not-in-PK, pack_length=2 bytes.
        let layout =
            compute_value_null_bitmap_layout(1, |_| false, |_| false);
        let mut src = mock(1);
        src.pack_length = vec![2];
        src.bytes = vec![vec![0xAA, 0xBB]];

        let mut dst = Vec::new();
        let n = encode_row_value(&layout, 1, &mut src, &mut dst).expect("enc");
        // No bitmap bytes (no nullable field), 2 field bytes.
        assert_eq!(n, 2);
        assert_eq!(dst, vec![0xAA, 0xBB]);
        assert_eq!(src.calls, vec![1]);
    }

    #[test]
    fn encode_row_value_nullable_non_null_field_emits_bitmap_plus_bytes() {
        // 1 field, nullable, not-in-PK, pack_length=2, currently not-null.
        let layout =
            compute_value_null_bitmap_layout(1, |_| true, |_| false);
        let mut src = mock(1);
        src.pack_length = vec![2];
        src.bytes = vec![vec![0xCC, 0xDD]];

        let mut dst = Vec::new();
        let n = encode_row_value(&layout, 1, &mut src, &mut dst).expect("enc");
        // 1 bitmap byte (zeroed — field is not-null), 2 field bytes.
        assert_eq!(n, 3);
        assert_eq!(dst, vec![0x00, 0xCC, 0xDD]);
    }

    #[test]
    fn encode_row_value_nullable_null_field_sets_bit_appends_nothing() {
        // 1 field, nullable, currently NULL.
        let layout =
            compute_value_null_bitmap_layout(1, |_| true, |_| false);
        let mut src = mock(1);
        src.is_null = vec![true];
        src.pack_length = vec![4];
        src.bytes = vec![vec![0xDE, 0xAD, 0xBE, 0xEF]];

        let mut dst = Vec::new();
        let n = encode_row_value(&layout, 1, &mut src, &mut dst).expect("enc");
        // 1 bitmap byte with bit 0 set, no field bytes.
        assert_eq!(n, 1);
        assert_eq!(dst, vec![0x01]);
        // write_field_bytes NOT called for NULL fields.
        assert_eq!(src.calls, vec![0]);
    }

    #[test]
    fn encode_row_value_skips_pk_fields() {
        // 3 fields: 0=PK (i64-ish, 8 bytes), 1=value (4 bytes,
        // nullable, not-null), 2=PK (4 bytes). Only field 1
        // should land in the value blob.
        let layout = compute_value_null_bitmap_layout(
            3,
            |i| i == 1, // only field 1 is nullable
            |i| i == 0 || i == 2,
        );
        let mut src = mock(3);
        src.is_in_pk = vec![true, false, true];
        src.pack_length = vec![8, 4, 4];
        src.bytes = vec![
            vec![0; 8],
            vec![0x11, 0x22, 0x33, 0x44],
            vec![0; 4],
        ];

        let mut dst = Vec::new();
        let n = encode_row_value(&layout, 3, &mut src, &mut dst).expect("enc");
        // 1 bitmap byte (field 1 is nullable, not-null → bit 0 clear),
        // 4 bytes for field 1, nothing for PK fields.
        assert_eq!(n, 5);
        assert_eq!(dst, vec![0x00, 0x11, 0x22, 0x33, 0x44]);
        // write_field_bytes called only for field 1.
        assert_eq!(src.calls, vec![0, 1, 0]);
    }

    #[test]
    fn encode_row_value_multiple_fields_emits_in_declaration_order() {
        // 3 non-PK fields, mix of nullable and not. Field 1 is
        // NULL, others not-null.
        let layout = compute_value_null_bitmap_layout(
            3,
            |i| i == 1 || i == 2, // fields 1, 2 nullable
            |_| false,
        );
        let mut src = mock(3);
        src.pack_length = vec![1, 2, 3];
        src.bytes = vec![vec![0xAA], vec![0xBB, 0xCC], vec![0xDD, 0xEE, 0xFF]];
        src.is_null = vec![false, true, false];

        let mut dst = Vec::new();
        let n = encode_row_value(&layout, 3, &mut src, &mut dst).expect("enc");
        // 1 bitmap byte (field 1 NULL → bit 0; field 2 not-null →
        // bit 1 clear). Bitmap = 0b00000001 = 0x01.
        // Field 0 bytes (1), field 1 bytes (skipped — NULL),
        // field 2 bytes (3). Total = 1 + 1 + 3 = 5.
        assert_eq!(n, 5);
        assert_eq!(dst, vec![0x01, 0xAA, 0xDD, 0xEE, 0xFF]);
        assert_eq!(src.calls, vec![1, 0, 1]);
    }

    #[test]
    fn encode_row_value_appends_to_existing_dst_content() {
        // Encoder writes after whatever's already in `dst`.
        let layout =
            compute_value_null_bitmap_layout(1, |_| false, |_| false);
        let mut src = mock(1);
        src.pack_length = vec![1];
        src.bytes = vec![vec![0xFE]];

        let mut dst: Vec<u8> = vec![0xCA, 0xFE];
        let n = encode_row_value(&layout, 1, &mut src, &mut dst).expect("enc");
        assert_eq!(n, 1);
        assert_eq!(dst, vec![0xCA, 0xFE, 0xFE]);
    }

    #[test]
    fn encode_row_value_propagates_source_error() {
        let layout =
            compute_value_null_bitmap_layout(2, |_| false, |_| false);
        let mut src = mock(2);
        src.pack_length = vec![1, 1];
        src.error_on_field = Some(1);

        let mut dst = Vec::new();
        let err = match encode_row_value(&layout, 2, &mut src, &mut dst) {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert!(err.to_string().contains("synthetic field write error"));
    }

    #[test]
    fn encode_row_value_rejects_short_field_write() {
        // Source claims pack_length=4 but only writes 3 bytes —
        // encoder catches the mismatch.
        let layout =
            compute_value_null_bitmap_layout(1, |_| false, |_| false);
        let mut src = mock(1);
        src.pack_length = vec![4];
        src.bytes = vec![vec![1, 2, 3, 4]];
        src.short_write_on_field = Some(0);

        let mut dst = Vec::new();
        let err = match encode_row_value(&layout, 1, &mut src, &mut dst) {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(err.to_string().contains("expected pack_length"));
    }

    // ----- decode_row_value -----

    /// Recording sink for the decoder. Captures set_null /
    /// set_field_bytes calls so tests can assert what happened.
    struct MockSink {
        is_in_pk: Vec<bool>,
        pack_length: Vec<u32>,
        // For each field, the last set_field_bytes call's input
        // (or None if not called / set_null instead).
        last_set_bytes: Vec<Option<Vec<u8>>>,
        // For each field, whether set_null was called.
        set_null_called: Vec<bool>,
        // Optional error to return on a specific field's
        // set_field_bytes.
        error_on_field: Option<u32>,
    }

    impl RowValueSink for MockSink {
        fn is_in_pk(&self, i: u32) -> bool {
            self.is_in_pk[i as usize]
        }
        fn pack_length(&self, i: u32) -> u32 {
            self.pack_length[i as usize]
        }
        fn set_null(&mut self, i: u32) {
            self.set_null_called[i as usize] = true;
        }
        fn set_field_bytes(
            &mut self,
            i: u32,
            src: &[u8],
        ) -> Result<(), slatedb::Error> {
            if Some(i) == self.error_on_field {
                return Err(slatedb::Error::invalid(
                    "MockSink: synthetic field set error".into(),
                ));
            }
            self.last_set_bytes[i as usize] = Some(src.to_vec());
            Ok(())
        }
    }

    fn mock_sink(n: usize) -> MockSink {
        MockSink {
            is_in_pk: vec![false; n],
            pack_length: vec![1; n],
            last_set_bytes: vec![None; n],
            set_null_called: vec![false; n],
            error_on_field: None,
        }
    }

    #[test]
    fn decode_row_value_empty_blob_no_fields() {
        let layout = compute_value_null_bitmap_layout(0, |_| false, |_| false);
        let mut sink = mock_sink(0);
        let n = decode_row_value(&layout, 0, &mut sink, &[]).expect("dec");
        assert_eq!(n, 0);
    }

    #[test]
    fn decode_row_value_single_non_nullable_field() {
        let layout = compute_value_null_bitmap_layout(1, |_| false, |_| false);
        let mut sink = mock_sink(1);
        sink.pack_length = vec![2];

        // No bitmap (no nullable fields), 2 bytes of field data.
        let blob = [0xAA, 0xBB];
        let n = decode_row_value(&layout, 1, &mut sink, &blob).expect("dec");
        assert_eq!(n, 2);
        assert_eq!(sink.last_set_bytes[0], Some(vec![0xAA, 0xBB]));
        assert!(!sink.set_null_called[0]);
    }

    #[test]
    fn decode_row_value_nullable_null_field_calls_set_null_consumes_no_bytes() {
        let layout = compute_value_null_bitmap_layout(1, |_| true, |_| false);
        let mut sink = mock_sink(1);
        sink.pack_length = vec![4];

        // Bitmap byte with bit 0 set — field 0 is NULL.
        let blob = [0x01];
        let n = decode_row_value(&layout, 1, &mut sink, &blob).expect("dec");
        assert_eq!(n, 1);
        assert!(sink.set_null_called[0]);
        assert!(sink.last_set_bytes[0].is_none());
    }

    #[test]
    fn decode_row_value_nullable_non_null_field_reads_bytes_after_bitmap() {
        let layout = compute_value_null_bitmap_layout(1, |_| true, |_| false);
        let mut sink = mock_sink(1);
        sink.pack_length = vec![3];

        // Bitmap byte zero (field not-null), then 3 field bytes.
        let blob = [0x00, 0xDE, 0xAD, 0xBE];
        let n = decode_row_value(&layout, 1, &mut sink, &blob).expect("dec");
        assert_eq!(n, 4);
        assert!(!sink.set_null_called[0]);
        assert_eq!(sink.last_set_bytes[0], Some(vec![0xDE, 0xAD, 0xBE]));
    }

    #[test]
    fn decode_row_value_skips_pk_fields_entirely() {
        // 3 fields: PK at 0 and 2; field 1 nullable, present.
        let layout = compute_value_null_bitmap_layout(
            3,
            |i| i == 1,
            |i| i == 0 || i == 2,
        );
        let mut sink = mock_sink(3);
        sink.is_in_pk = vec![true, false, true];
        sink.pack_length = vec![8, 4, 4];

        // Bitmap: field 1 not-null → 0x00. Field 1 bytes follow.
        let blob = [0x00, 0x11, 0x22, 0x33, 0x44];
        let n = decode_row_value(&layout, 3, &mut sink, &blob).expect("dec");
        assert_eq!(n, 5);
        // Only field 1 should have been touched.
        assert_eq!(sink.last_set_bytes[0], None);
        assert_eq!(sink.last_set_bytes[1], Some(vec![0x11, 0x22, 0x33, 0x44]));
        assert_eq!(sink.last_set_bytes[2], None);
    }

    #[test]
    fn decode_row_value_multiple_fields_consumed_in_order() {
        let layout = compute_value_null_bitmap_layout(
            3,
            |i| i == 1 || i == 2,
            |_| false,
        );
        let mut sink = mock_sink(3);
        sink.pack_length = vec![1, 2, 3];

        // Bitmap: field 1 NULL, field 2 not-null.
        // Bit 0 → field 1 NULL → bit 0 set.
        // Bit 1 → field 2 NULL → bit 1 clear.
        // Bitmap = 0b00000001 = 0x01.
        // Field 0 bytes (1), field 1 skipped (NULL), field 2 bytes (3).
        let blob = [0x01, 0xAA, 0xDD, 0xEE, 0xFF];
        let n = decode_row_value(&layout, 3, &mut sink, &blob).expect("dec");
        assert_eq!(n, 5);
        assert_eq!(sink.last_set_bytes[0], Some(vec![0xAA]));
        assert!(sink.set_null_called[1]);
        assert_eq!(sink.last_set_bytes[2], Some(vec![0xDD, 0xEE, 0xFF]));
    }

    #[test]
    fn decode_row_value_rejects_truncated_bitmap() {
        let layout = compute_value_null_bitmap_layout(8, |_| true, |_| false);
        // bitmap needs 1 byte; provide 0.
        let mut sink = mock_sink(8);
        let blob: Vec<u8> = vec![];
        let err = match decode_row_value(&layout, 8, &mut sink, &blob) {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert_eq!(err.kind(), slatedb::ErrorKind::Data);
        assert!(err.to_string().contains("null bitmap"));
    }

    #[test]
    fn decode_row_value_rejects_truncated_field_bytes() {
        let layout = compute_value_null_bitmap_layout(1, |_| false, |_| false);
        let mut sink = mock_sink(1);
        sink.pack_length = vec![4];

        // Only 2 bytes of field data, needs 4.
        let blob = [0xAA, 0xBB];
        let err = match decode_row_value(&layout, 1, &mut sink, &blob) {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert_eq!(err.kind(), slatedb::ErrorKind::Data);
        assert!(err.to_string().contains("truncated"));
    }

    #[test]
    fn decode_row_value_propagates_sink_error() {
        let layout = compute_value_null_bitmap_layout(2, |_| false, |_| false);
        let mut sink = mock_sink(2);
        sink.pack_length = vec![1, 1];
        sink.error_on_field = Some(1);

        let blob = [0xAA, 0xBB];
        let err = match decode_row_value(&layout, 2, &mut sink, &blob) {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert!(err.to_string().contains("synthetic field set error"));
    }

    // ----- encode/decode round-trip -----

    #[test]
    fn encode_decode_round_trips_for_a_realistic_table() {
        // 4 fields: 0 = PK (non-null, 8 bytes), 1 = nullable str
        // (currently null, 16 bytes when present), 2 = non-nullable
        // int (4 bytes), 3 = nullable int (currently present, 4 bytes).
        let is_nullable_vec = [false, true, false, true];
        let is_in_pk_vec = [true, false, false, false];
        let layout = compute_value_null_bitmap_layout(
            4,
            |i| is_nullable_vec[i as usize],
            |i| is_in_pk_vec[i as usize],
        );

        // Encode.
        let mut enc_src = mock(4);
        enc_src.is_in_pk = is_in_pk_vec.to_vec();
        enc_src.is_null = vec![false, true, false, false];
        enc_src.pack_length = vec![8, 16, 4, 4];
        enc_src.bytes = vec![
            vec![0; 8],                   // PK — not written
            vec![0; 16],                  // NULL — not written
            vec![0xCA, 0xFE, 0xBA, 0xBE], // int = 0xCAFEBABE
            vec![0x12, 0x34, 0x56, 0x78], // nullable int present
        ];
        let mut blob: Vec<u8> = Vec::new();
        encode_row_value(&layout, 4, &mut enc_src, &mut blob).expect("enc");

        // Decode into a fresh sink and assert the symmetry.
        let mut dec_sink = mock_sink(4);
        dec_sink.is_in_pk = is_in_pk_vec.to_vec();
        dec_sink.pack_length = enc_src.pack_length.clone();
        let n =
            decode_row_value(&layout, 4, &mut dec_sink, &blob).expect("dec");
        assert_eq!(n, blob.len(), "decoder consumes the whole blob");

        // PK field: never touched.
        assert!(!dec_sink.set_null_called[0]);
        assert_eq!(dec_sink.last_set_bytes[0], None);
        // Field 1 (nullable, was NULL).
        assert!(dec_sink.set_null_called[1]);
        assert_eq!(dec_sink.last_set_bytes[1], None);
        // Field 2 (non-nullable, was present).
        assert!(!dec_sink.set_null_called[2]);
        assert_eq!(
            dec_sink.last_set_bytes[2],
            Some(vec![0xCA, 0xFE, 0xBA, 0xBE]),
        );
        // Field 3 (nullable, present).
        assert!(!dec_sink.set_null_called[3]);
        assert_eq!(
            dec_sink.last_set_bytes[3],
            Some(vec![0x12, 0x34, 0x56, 0x78]),
        );
    }
}
