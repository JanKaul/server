//! Per-index descriptor (`KeyDef`).
//!
//! Translated from `storage/rocksdb/rdb_datadic.{h,cc}` (`class Rdb_key_def`,
//! ~625 LoC of header + scattered impl). This first batch lands:
//!
//! - Layout constants, flag enums, format-version constants.
//! - The [`KeyDef`] struct itself.
//! - Inline accessors and direction/format-version checks.
//! - Range bounds (`get_infimum_key` / `get_supremum_key` /
//!   `get_first_key` / `get_last_key`) — direction-aware, byte-faithful to
//!   MyRocks.
//! - `successor` / `predecessor` (full port from `rdb_datadic.cc:1054..1087`).
//! - `covers_key` / `cmp_full_keys` / `value_matches_prefix`.
//! - Index-flag bitmap helpers (`has_index_flag` /
//!   `calculate_index_flag_offset`) including the per-bit length table.
//! - The four `gen_*_qualifier_for_partition` formatters.
//!
//! Deferred (each documented at the method body):
//! - `new`, `duplicate`, `setup` — need a real table view and the
//!   `FieldPacking::setup` dispatcher.
//! - Pack/unpack walkers (`compare_keys`, `key_length`,
//!   `get_primary_key_tuple`, `get_memcmp_sk_parts`, `read_memcmp_key_part`)
//!   — need a populated `pack_info` vector.
//! - Covering-lookup machinery (`get_lookup_bitmap`, `covers_lookup`,
//!   `can_cover_lookup`, `can_unpack`, `has_unpack_info`) — same.
//! - TTL / comment-qualifier extractors — need table-comment parsing.
//! - Checksum / unpack-header helpers — need the data-tag constants from a
//!   later batch.
//! - `write_index_flag_field`, `get_table_field_for_part_no`,
//!   `table_has_hidden_pk`, `report_checksum_mismatch`.
//!
//! Fields not yet present in this struct vs. the C++:
//! - `prefix_extractor` — vestigial. SlateDB takes one global
//!   `MyRocksPrefixExtractor` (see `codec::prefix`); per-index extractors
//!   are not a concept in our world.
//! - `setup_mutex` — guards `setup()` lazy init; lands with setup.
//! - `stats` (`IndexStats`) — lands when `properties_collector` ports.

use crate::codec::field_pack::FieldPacking;
use crate::engine::comparator::KeyDirection;
use crate::globals::GlIndexId;

// ---------- layout-size constants (rdb_datadic.h:468) ----------

pub const INDEX_NUMBER_SIZE: usize = 4;
pub const VERSION_SIZE: usize = 2;
pub const CF_NUMBER_SIZE: usize = 4;
pub const CF_FLAG_SIZE: usize = 4;
pub const PACKED_SIZE: usize = 4;

// ---------- CF bit-flags persisted in the data dictionary (rdb_datadic.h:477) ----------

pub const REVERSE_CF_FLAG: u32 = 1;
/// Deprecated; kept to ignore the bit on load.
pub const AUTO_CF_FLAG: u32 = 2;
pub const PER_PARTITION_CF_FLAG: u32 = 4;
pub const CF_FLAGS_TO_IGNORE: u32 = PER_PARTITION_CF_FLAG;

// ---------- index-flag bits stored in the on-record header (rdb_datadic.h:485) ----------

/// Bytes stored per `IndexFlag` slot in the on-record header. Indexed by
/// bit position — only `TtlFlag` (bit 0) has a payload (a u64 timestamp);
/// `MaxFlag` is a sentinel for "header ends here" and has no payload.
const INDEX_FLAG_LENGTHS: [u32; 1] = [8];

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexFlag {
    TtlFlag = 1 << 0,
    MaxFlag = 1 << 1,
}

// ---------- index types (rdb_datadic.h:549) ----------

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexType {
    Primary = 1,
    Secondary = 2,
    HiddenPrimary = 3,
}

// ---------- schema versions for each data-dict entry shape (rdb_datadic.h:515) ----------

pub const DDL_ENTRY_INDEX_VERSION: u16 = 1;
pub const CF_DEFINITION_VERSION: u16 = 1;
pub const BINLOG_INFO_INDEX_NUMBER_VERSION: u16 = 1;
pub const DDL_DROP_INDEX_ONGOING_VERSION: u16 = 1;
pub const MAX_INDEX_ID_VERSION: u16 = 1;
pub const DDL_CREATE_INDEX_ONGOING_VERSION: u16 = 1;
pub const AUTO_INCREMENT_VERSION: u16 = 1;

// ---------- index-info schema versions (rdb_datadic.h:529) ----------

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IndexInfoVersion {
    Initial = 1,
    KvFormat = 2,
    GlobalId = 3,
    VerifyKvFormat = 4,
    Ttl = 5,
    FieldFlags = 6,
}
pub const INDEX_INFO_VERSION_LATEST: IndexInfoVersion = IndexInfoVersion::FieldFlags;

// ---------- PK/SK key-value format versions (rdb_datadic.h:556) ----------

pub const PRIMARY_FORMAT_VERSION_INITIAL: u16 = 10;
pub const PRIMARY_FORMAT_VERSION_UPDATE1: u16 = 11;
pub const PRIMARY_FORMAT_VERSION_UPDATE2: u16 = 12;
pub const PRIMARY_FORMAT_VERSION_TTL: u16 = 13;
pub const PRIMARY_FORMAT_VERSION_LATEST: u16 = PRIMARY_FORMAT_VERSION_TTL;

pub const SECONDARY_FORMAT_VERSION_INITIAL: u16 = 10;
pub const SECONDARY_FORMAT_VERSION_UPDATE1: u16 = 11;
pub const SECONDARY_FORMAT_VERSION_UPDATE2: u16 = 12;
pub const SECONDARY_FORMAT_VERSION_TTL: u16 = 13;
pub const SECONDARY_FORMAT_VERSION_LATEST: u16 = SECONDARY_FORMAT_VERSION_TTL;
pub const SECONDARY_FORMAT_VERSION_UPDATE3: u16 = 65535;

// ---------- main struct ----------

/// Per-index descriptor. One instance per logical index, shared between
/// every handler that opens the table. Long-lived; refcounted via `Arc` at
/// the `Rdb_tbl_def` level (when that lands).
///
/// Original: `rdb_datadic.h:251` — `class Rdb_key_def`.
//
// The PK-tracking and pack_info fields are populated by `setup()` (deferred)
// and consumed by the pack/unpack walkers (deferred). Marked dead_code-allowed
// so the struct shape doesn't churn when those land.
#[allow(dead_code)]
pub struct KeyDef {
    // ----- public C++ fields (rdb_datadic.h:798..833) -----
    pub index_dict_version: u16,
    pub index_type: IndexType,
    pub kv_format_version: u16,
    /// True if the column family stores data in reverse byte order. At
    /// encode time the codec XORs the mem-comparable bytes with 0xff so
    /// byte-lex order produces the desired reverse semantic order.
    pub is_reverse_cf: bool,
    pub is_per_partition_cf: bool,
    pub name: String,
    pub index_flags_bitmap: u32,
    pub total_index_flags_length: u32,
    pub ttl_rec_offset: u32,
    pub ttl_duration: u64,
    pub ttl_column: String,

    // ----- private-in-C++ fields (rdb_datadic.h:781..874) -----
    pub(crate) index_number: u32,
    pub(crate) index_number_storage_form: [u8; INDEX_NUMBER_SIZE],
    /// CF identity. Replaces `rocksdb::ColumnFamilyHandle*` per
    /// `_DESIGN.md §2`.
    pub(crate) cf_id: u32,
    pub(crate) pk_key_parts: u32,
    /// `pk_part_no[X] = Some(Y)` ⇒ keypart #X is part #Y of the PK;
    /// `None` means not in PK. Length is `key_parts`.
    pub(crate) pk_part_no: Vec<Option<u32>>,
    pub(crate) pack_info: Vec<FieldPacking>,
    pub(crate) keyno: u32,
    pub(crate) key_parts: u32,
    pub(crate) ttl_pk_key_part_offset: u32,
    pub(crate) ttl_field_index: u32,
    pub(crate) maxlength: u32,
}

impl KeyDef {
    /// Skeleton constructor for tests and the data-dictionary loader that
    /// only knows the index identity. Real `KeyDef::new` (with full
    /// `FieldPacking::setup` dispatch from a table view) lands with
    /// `codec::key::setup`.
    pub fn new_skeleton(
        index_number: u32,
        cf_id: u32,
        keyno: u32,
        index_dict_version: u16,
        index_type: IndexType,
        kv_format_version: u16,
        is_reverse_cf: bool,
        name: impl Into<String>,
    ) -> Self {
        let index_number_storage_form = index_number.to_be_bytes();
        Self {
            index_dict_version,
            index_type,
            kv_format_version,
            is_reverse_cf,
            is_per_partition_cf: false,
            name: name.into(),
            index_flags_bitmap: 0,
            total_index_flags_length: 0,
            ttl_rec_offset: 0,
            ttl_duration: 0,
            ttl_column: String::new(),
            index_number,
            index_number_storage_form,
            cf_id,
            pk_key_parts: 0,
            pk_part_no: Vec::new(),
            pack_info: Vec::new(),
            keyno,
            key_parts: 0,
            ttl_pk_key_part_offset: 0,
            ttl_field_index: 0,
            maxlength: 0,
        }
    }

    // ----- inline accessors (rdb_datadic.h:409..) -----

    pub fn get_keyno(&self) -> u32 {
        self.keyno
    }
    pub fn get_index_number(&self) -> u32 {
        self.index_number
    }
    pub fn get_gl_index_id(&self) -> GlIndexId {
        GlIndexId {
            cf_id: self.cf_id,
            index_id: self.index_number,
        }
    }
    pub fn max_storage_fmt_length(&self) -> u32 {
        self.maxlength
    }
    pub fn get_key_parts(&self) -> u32 {
        self.key_parts
    }
    pub fn get_ttl_field_index(&self) -> u32 {
        self.ttl_field_index
    }
    pub fn get_name(&self) -> &str {
        &self.name
    }
    /// CF identity. Replaces the C++ `get_cf()` — SlateDB has no
    /// `ColumnFamilyHandle*`, callers want the id to build `key_prefix`.
    pub fn cf_id(&self) -> u32 {
        self.cf_id
    }
    pub fn has_ttl(&self) -> bool {
        self.ttl_duration > 0
    }
    pub fn use_covered_bitmap_format(&self) -> bool {
        self.index_type == IndexType::Secondary
            && self.kv_format_version >= SECONDARY_FORMAT_VERSION_UPDATE3
    }
    pub fn use_legacy_varbinary_format(&self) -> bool {
        !self.index_format_min_check(
            PRIMARY_FORMAT_VERSION_UPDATE2,
            SECONDARY_FORMAT_VERSION_UPDATE2,
        )
    }

    /// `KeyDirection` derived from `is_reverse_cf` — useful at codec call
    /// sites that take a `KeyDirection`.
    pub fn direction(&self) -> KeyDirection {
        if self.is_reverse_cf {
            KeyDirection::Reverse
        } else {
            KeyDirection::Forward
        }
    }

    // ----- infimum / supremum / first / last (rdb_datadic.h:287..335) -----

    /// Write `index_number` big-endian into the leading 4 bytes of `key`
    /// and set `*size = 4`.
    pub fn get_infimum_key(&self, key: &mut [u8], size: &mut usize) {
        key[..INDEX_NUMBER_SIZE].copy_from_slice(&self.index_number_storage_form);
        *size = INDEX_NUMBER_SIZE;
    }

    /// Write `index_number + 1` big-endian into the leading 4 bytes of
    /// `key` and set `*size = 4`. Used as the exclusive upper bound for
    /// range scans of this index.
    pub fn get_supremum_key(&self, key: &mut [u8], size: &mut usize) {
        key[..INDEX_NUMBER_SIZE]
            .copy_from_slice(&self.index_number.wrapping_add(1).to_be_bytes());
        *size = INDEX_NUMBER_SIZE;
    }

    /// First key for "begin iterating from start of index". For
    /// reverse-CF indexes iteration starts at the physical supremum.
    /// Returns the count of leading bytes usable for bloom-filter prefix
    /// lookup (i.e. bytes unchanged from `index_number`).
    pub fn get_first_key(&self, key: &mut [u8], size: &mut usize) -> i32 {
        if self.is_reverse_cf {
            self.get_supremum_key(key, size);
            let unmodified = self.index_number_storage_form;
            (0..INDEX_NUMBER_SIZE)
                .take_while(|&i| key[i] == unmodified[i])
                .count() as i32
        } else {
            self.get_infimum_key(key, size);
            INDEX_NUMBER_SIZE as i32
        }
    }

    /// Last-key counterpart; symmetric with `get_first_key`.
    pub fn get_last_key(&self, key: &mut [u8], size: &mut usize) -> i32 {
        if self.is_reverse_cf {
            self.get_infimum_key(key, size);
            INDEX_NUMBER_SIZE as i32
        } else {
            self.get_supremum_key(key, size);
            let unmodified = self.index_number_storage_form;
            (0..INDEX_NUMBER_SIZE)
                .take_while(|&i| key[i] == unmodified[i])
                .count() as i32
        }
    }

    // ----- successor / predecessor (rdb_datadic.cc:1054..1087) -----

    /// Mutate `packed_tuple[..len]` into its strict successor (or, if all
    /// bytes after the first are 0xff, set them to 0 — caller must check
    /// the returned count and the leading byte to confirm a true successor
    /// was produced). Returns the count of bytes changed.
    ///
    /// The first byte is **never** modified — matches the C++ `p > packed_tuple`
    /// loop guard. Used to convert a lookup key into an exclusive upper
    /// bound for SlateDB's `scan(range)`.
    pub fn successor(packed_tuple: &mut [u8], len: usize) -> i32 {
        let mut changed = 0i32;
        if len == 0 {
            return 0;
        }
        let mut p = len - 1;
        while p > 0 {
            changed += 1;
            if packed_tuple[p] != 0xff {
                packed_tuple[p] = packed_tuple[p].wrapping_add(1);
                return changed;
            }
            packed_tuple[p] = 0;
            p -= 1;
        }
        changed
    }

    /// Mirror image of `successor`. Mutates into the strict predecessor (or
    /// 0xff-fills if all bytes after the first are 0). First byte never
    /// modified. Returns count of bytes changed.
    pub fn predecessor(packed_tuple: &mut [u8], len: usize) -> i32 {
        let mut changed = 0i32;
        if len == 0 {
            return 0;
        }
        let mut p = len - 1;
        while p > 0 {
            changed += 1;
            if packed_tuple[p] != 0x00 {
                packed_tuple[p] = packed_tuple[p].wrapping_sub(1);
                return changed;
            }
            packed_tuple[p] = 0xff;
            p -= 1;
        }
        changed
    }

    // ----- comparison helpers -----

    /// Memcmp-compare two keys treating "X is a prefix of Y" as equal.
    pub fn cmp_full_keys(&self, a: &[u8], b: &[u8]) -> std::cmp::Ordering {
        debug_assert!(self.covers_key(a));
        let n = a.len().min(b.len());
        a[..n].cmp(&b[..n])
    }

    /// True iff `slice` begins with this index's 4-byte index_number.
    pub fn covers_key(&self, slice: &[u8]) -> bool {
        slice.len() >= INDEX_NUMBER_SIZE
            && slice[..INDEX_NUMBER_SIZE] == self.index_number_storage_form
    }

    /// True iff `value` is from this index AND has `prefix` as a prefix.
    pub fn value_matches_prefix(&self, value: &[u8], prefix: &[u8]) -> bool {
        self.covers_key(value)
            && self.cmp_full_keys(value, prefix) == std::cmp::Ordering::Equal
    }

    // ----- index-flag bitmap helpers -----

    /// True iff `index_flags` bitmap contains `flag`.
    pub fn has_index_flag(index_flags: u32, flag: IndexFlag) -> bool {
        (index_flags & (flag as u32)) != 0
    }

    /// Byte offset within the on-record header where `flag`'s data lives,
    /// computed by summing the lengths of all flag bits set below `flag`.
    /// If `field_length` is supplied, it's filled with the byte length of
    /// `flag`'s payload (0 for `MaxFlag` — it's a header sentinel with no
    /// payload).
    pub fn calculate_index_flag_offset(
        index_flags: u32,
        flag: IndexFlag,
        mut field_length: Option<&mut u32>,
    ) -> u32 {
        let target = flag as u32;
        let mut offset: u32 = 0;
        for bit in 0..32u32 {
            let mask = 1u32 << bit;
            let len_for_bit = INDEX_FLAG_LENGTHS
                .get(bit as usize)
                .copied()
                .unwrap_or(0);
            if target & mask != 0 {
                if let Some(out) = field_length.as_mut() {
                    **out = len_for_bit;
                }
                return offset;
            }
            if index_flags & mask != 0 {
                offset += len_for_bit;
            }
        }
        offset
    }

    // ----- misc -----

    /// True iff this index's PK/SK format version meets the given minimum.
    pub fn index_format_min_check(&self, pk_min: u16, sk_min: u16) -> bool {
        match self.index_type {
            IndexType::Primary | IndexType::HiddenPrimary => self.kv_format_version >= pk_min,
            IndexType::Secondary => self.kv_format_version >= sk_min,
        }
    }

    /// `RDB_UNPACK_DATA_TAG (0x02)` or `RDB_UNPACK_COVERED_DATA_TAG (0x03)`.
    pub fn is_unpack_data_tag(c: u8) -> bool {
        c == 0x02 || c == 0x03
    }

    // ----- qualifier formatters (table-comment helpers) -----

    pub fn gen_cf_name_qualifier_for_partition(s: &str) -> String {
        format!("{}_{}", crate::globals::CF_NAME_QUALIFIER, s)
    }
    pub fn gen_ttl_duration_qualifier_for_partition(s: &str) -> String {
        format!("{}_{}", crate::globals::TTL_DURATION_QUALIFIER, s)
    }
    pub fn gen_ttl_col_qualifier_for_partition(s: &str) -> String {
        format!("{}_{}", crate::globals::TTL_COL_QUALIFIER, s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forward_pk(index_number: u32) -> KeyDef {
        KeyDef::new_skeleton(
            index_number,
            7,
            0,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Primary,
            PRIMARY_FORMAT_VERSION_LATEST,
            false,
            "fwd_pk",
        )
    }

    fn reverse_sk(index_number: u32) -> KeyDef {
        KeyDef::new_skeleton(
            index_number,
            7,
            1,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Secondary,
            SECONDARY_FORMAT_VERSION_LATEST,
            true,
            "rev_sk",
        )
    }

    #[test]
    fn skeleton_round_trips_identity_fields() {
        let kd = forward_pk(42);
        assert_eq!(kd.get_index_number(), 42);
        assert_eq!(kd.get_keyno(), 0);
        assert_eq!(kd.cf_id(), 7);
        assert_eq!(kd.get_name(), "fwd_pk");
        assert_eq!(kd.index_type, IndexType::Primary);
        assert_eq!(
            kd.get_gl_index_id(),
            GlIndexId {
                cf_id: 7,
                index_id: 42,
            }
        );
        assert_eq!(kd.direction(), KeyDirection::Forward);
        assert_eq!(reverse_sk(99).direction(), KeyDirection::Reverse);
    }

    #[test]
    fn infimum_and_supremum_are_index_number_be_and_plus_one() {
        let kd = forward_pk(0x0102_0304);
        let mut buf = [0u8; 8];
        let mut n = 0;
        kd.get_infimum_key(&mut buf, &mut n);
        assert_eq!(n, 4);
        assert_eq!(&buf[..4], &[0x01, 0x02, 0x03, 0x04]);

        kd.get_supremum_key(&mut buf, &mut n);
        assert_eq!(n, 4);
        assert_eq!(&buf[..4], &[0x01, 0x02, 0x03, 0x05]);
    }

    #[test]
    fn first_and_last_swap_for_reverse_cf() {
        let f = forward_pk(10);
        let r = reverse_sk(10);
        let mut buf = [0u8; 8];
        let mut n = 0;

        f.get_first_key(&mut buf, &mut n);
        assert_eq!(&buf[..4], &[0, 0, 0, 10]);
        f.get_last_key(&mut buf, &mut n);
        assert_eq!(&buf[..4], &[0, 0, 0, 11]);

        r.get_first_key(&mut buf, &mut n);
        assert_eq!(&buf[..4], &[0, 0, 0, 11]);
        r.get_last_key(&mut buf, &mut n);
        assert_eq!(&buf[..4], &[0, 0, 0, 10]);
    }

    #[test]
    fn covers_key_matches_index_number_prefix() {
        let kd = forward_pk(42);
        assert!(kd.covers_key(&[0, 0, 0, 42, b'x', b'y']));
        assert!(kd.covers_key(&[0, 0, 0, 42]));
        assert!(!kd.covers_key(&[0, 0, 0, 41, b'x']));
        assert!(!kd.covers_key(&[0, 0, 0]));
    }

    #[test]
    fn value_matches_prefix_requires_both_index_and_prefix() {
        let kd = forward_pk(42);
        assert!(kd.value_matches_prefix(&[0, 0, 0, 42, b'h', b'i'], &[0, 0, 0, 42, b'h']));
        // Different index.
        assert!(!kd.value_matches_prefix(&[0, 0, 0, 43, b'h'], &[0, 0, 0, 42, b'h']));
        // Right index, wrong content.
        assert!(!kd.value_matches_prefix(&[0, 0, 0, 42, b'x'], &[0, 0, 0, 42, b'h']));
    }

    #[test]
    fn successor_bumps_last_non_ff_byte() {
        let mut buf = [0u8, 0u8, 0u8, 7u8];
        assert_eq!(KeyDef::successor(&mut buf, 4), 1);
        assert_eq!(buf, [0, 0, 0, 8]);
    }

    #[test]
    fn successor_carries_through_trailing_ff_bytes() {
        let mut buf = [0u8, 0u8, 7u8, 0xff, 0xff];
        // Carry past two 0xff bytes, bump the third-from-end.
        assert_eq!(KeyDef::successor(&mut buf, 5), 3);
        assert_eq!(buf, [0, 0, 8, 0, 0]);
    }

    #[test]
    fn successor_never_modifies_first_byte_and_zeros_the_tail() {
        // Tail entirely 0xff after the leading byte → tail zeros out and
        // first byte stays. Caller must detect this is NOT a real successor.
        let mut buf = [0x05u8, 0xff, 0xff, 0xff];
        let changed = KeyDef::successor(&mut buf, 4);
        assert_eq!(changed, 3);
        assert_eq!(buf, [0x05, 0, 0, 0]);
    }

    #[test]
    fn successor_on_len_zero_is_a_noop() {
        let mut buf: [u8; 0] = [];
        assert_eq!(KeyDef::successor(&mut buf, 0), 0);
    }

    #[test]
    fn predecessor_decrements_last_non_zero_byte() {
        let mut buf = [0u8, 0u8, 0u8, 7u8];
        assert_eq!(KeyDef::predecessor(&mut buf, 4), 1);
        assert_eq!(buf, [0, 0, 0, 6]);
    }

    #[test]
    fn predecessor_borrows_through_trailing_zero_bytes() {
        let mut buf = [0u8, 7u8, 0u8, 0u8];
        assert_eq!(KeyDef::predecessor(&mut buf, 4), 3);
        assert_eq!(buf, [0, 6, 0xff, 0xff]);
    }

    #[test]
    fn has_index_flag_bit_math() {
        assert!(KeyDef::has_index_flag(0b01, IndexFlag::TtlFlag));
        assert!(!KeyDef::has_index_flag(0b10, IndexFlag::TtlFlag));
        assert!(KeyDef::has_index_flag(0b10, IndexFlag::MaxFlag));
        assert!(KeyDef::has_index_flag(0b11, IndexFlag::TtlFlag));
        assert!(KeyDef::has_index_flag(0b11, IndexFlag::MaxFlag));
    }

    #[test]
    fn calculate_index_flag_offset_ttl_alone() {
        // Only TtlFlag set; offset for TtlFlag itself is 0, payload length is 8.
        let mut len: u32 = 0;
        let off = KeyDef::calculate_index_flag_offset(
            IndexFlag::TtlFlag as u32,
            IndexFlag::TtlFlag,
            Some(&mut len),
        );
        assert_eq!(off, 0);
        assert_eq!(len, 8);
    }

    #[test]
    fn calculate_index_flag_offset_max_flag_yields_header_end() {
        // TtlFlag set, asking MaxFlag's offset → past the TTL payload.
        let mut len: u32 = 99;
        let off = KeyDef::calculate_index_flag_offset(
            IndexFlag::TtlFlag as u32,
            IndexFlag::MaxFlag,
            Some(&mut len),
        );
        assert_eq!(off, 8);
        // MaxFlag is a sentinel — no payload, so length comes back as 0.
        assert_eq!(len, 0);
    }

    #[test]
    fn index_format_min_check_dispatches_by_index_type() {
        let pk = forward_pk(1);
        assert!(pk.index_format_min_check(PRIMARY_FORMAT_VERSION_INITIAL, 0));
        assert!(!pk.index_format_min_check(PRIMARY_FORMAT_VERSION_LATEST + 1, 0));

        let sk = reverse_sk(1);
        assert!(sk.index_format_min_check(0, SECONDARY_FORMAT_VERSION_INITIAL));
        assert!(!sk.index_format_min_check(0, SECONDARY_FORMAT_VERSION_LATEST + 1));
    }

    #[test]
    fn use_legacy_varbinary_format_matches_pre_update2() {
        let mut pk = forward_pk(1);
        pk.kv_format_version = PRIMARY_FORMAT_VERSION_INITIAL;
        assert!(pk.use_legacy_varbinary_format());

        pk.kv_format_version = PRIMARY_FORMAT_VERSION_UPDATE2;
        assert!(!pk.use_legacy_varbinary_format());
    }

    #[test]
    fn use_covered_bitmap_format_requires_sk_and_update3() {
        let mut sk = reverse_sk(1);
        sk.kv_format_version = SECONDARY_FORMAT_VERSION_UPDATE3;
        assert!(sk.use_covered_bitmap_format());
        sk.kv_format_version = SECONDARY_FORMAT_VERSION_LATEST;
        assert!(!sk.use_covered_bitmap_format());

        let mut pk = forward_pk(1);
        pk.kv_format_version = SECONDARY_FORMAT_VERSION_UPDATE3;
        assert!(!pk.use_covered_bitmap_format(), "PK never uses covered-bitmap fmt");
    }

    #[test]
    fn is_unpack_data_tag_recognises_both_tags() {
        assert!(KeyDef::is_unpack_data_tag(0x02));
        assert!(KeyDef::is_unpack_data_tag(0x03));
        assert!(!KeyDef::is_unpack_data_tag(0x01));
        assert!(!KeyDef::is_unpack_data_tag(0xff));
    }

    #[test]
    fn qualifier_formatters_combine_prefix_and_partition_name() {
        assert_eq!(
            KeyDef::gen_cf_name_qualifier_for_partition("p0"),
            format!("{}_p0", crate::globals::CF_NAME_QUALIFIER)
        );
        assert_eq!(
            KeyDef::gen_ttl_duration_qualifier_for_partition("p0"),
            format!("{}_p0", crate::globals::TTL_DURATION_QUALIFIER)
        );
        assert_eq!(
            KeyDef::gen_ttl_col_qualifier_for_partition("p0"),
            format!("{}_p0", crate::globals::TTL_COL_QUALIFIER)
        );
    }
}
