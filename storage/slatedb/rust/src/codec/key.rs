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

/// Resolved TTL column descriptor returned by
/// [`KeyDef::extract_ttl_col`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtlColumn {
    pub column_name: String,
    pub field_index: u32,
}

/// Result of [`KeyDef::read_memcmp_key_part`]. Distinguishes "field was
/// stored NULL" from "I/O / format error" — both have different
/// semantics at the caller (NULL is data; Error is a decode bug).
///
/// Replaces the C++ tri-valued `int` return: `0` → `Ok`, `-1` → `Null`,
/// `1` → `Error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadKeyPart {
    Ok,
    Null,
    Error,
}

/// Result of [`KeyDef::compare_keys`]. The C++ overloads `column_index`
/// with the "all equal" signal by returning `key_parts`; the enum types
/// that out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareResult {
    /// Every keypart compared equal.
    Equal,
    /// First (0-based) keypart where the keys differ.
    DifferAt(u32),
}

// ---------- layout-size constants (rdb_datadic.h:468) ----------

pub const INDEX_NUMBER_SIZE: usize = 4;
pub const VERSION_SIZE: usize = 2;
pub const CF_NUMBER_SIZE: usize = 4;
pub const CF_FLAG_SIZE: usize = 4;
pub const PACKED_SIZE: usize = 4;

// ---------- unpack-info / checksum format (rdb_datadic.h:162..194) ----------

/// Two CRC32 checksums (key + value) live at the tail of the unpack_info
/// blob when the index uses the checksum-trailer format.
pub const RDB_CHECKSUM_SIZE: usize = 4;
/// Wire size of the trailer: `tag(1) || crc32_key(4) || crc32_value(4)`.
pub const RDB_CHECKSUM_CHUNK_SIZE: usize = 2 * RDB_CHECKSUM_SIZE + 1;
pub const RDB_CHECKSUM_DATA_TAG: u8 = 0x01;

/// `tag(1) || u16_be(total_skip_length_incl_header)`.
pub const RDB_UNPACK_DATA_TAG: u8 = 0x02;
const RDB_UNPACK_DATA_LEN_SIZE: usize = 2;
pub const RDB_UNPACK_HEADER_SIZE: usize = 1 + RDB_UNPACK_DATA_LEN_SIZE;

/// `tag(1) || u16_be(total_skip_length_incl_header) || u16_be(covered_bitmap)`.
pub const RDB_UNPACK_COVERED_DATA_TAG: u8 = 0x03;
const RDB_UNPACK_COVERED_DATA_LEN_SIZE: usize = 2;
const RDB_COVERED_BITMAP_SIZE: usize = 2;
pub const RDB_UNPACK_COVERED_HEADER_SIZE: usize =
    1 + RDB_UNPACK_COVERED_DATA_LEN_SIZE + RDB_COVERED_BITMAP_SIZE;

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

// Index-statistics value-format versions (properties_collector.h:49).
// `Initial` lacks the four entry-type counters; `EntryTypes` (latest) adds
// deletes/single-deletes/merges/others. Writers stamp `EntryTypes`; readers
// accept both.
pub const INDEX_STATS_VERSION_INITIAL: u16 = 1;
pub const INDEX_STATS_VERSION_ENTRY_TYPES: u16 = 2;

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
        c == RDB_UNPACK_DATA_TAG || c == RDB_UNPACK_COVERED_DATA_TAG
    }

    /// Header byte length for an unpack-info blob keyed by its leading tag.
    /// `Some(3)` for `RDB_UNPACK_DATA_TAG`, `Some(5)` for
    /// `RDB_UNPACK_COVERED_DATA_TAG`, `None` for anything else (callers
    /// must have established `is_unpack_data_tag` first — `None` then
    /// signals corruption).
    pub fn get_unpack_header_size(tag: u8) -> Option<usize> {
        match tag {
            RDB_UNPACK_DATA_TAG => Some(RDB_UNPACK_HEADER_SIZE),
            RDB_UNPACK_COVERED_DATA_TAG => Some(RDB_UNPACK_COVERED_HEADER_SIZE),
            _ => None,
        }
    }

    /// True iff the unpack_info blob carries a CRC32 checksum trailer.
    ///
    /// Algorithm (rdb_datadic.cc:1032..1049):
    /// 1. Empty → false.
    /// 2. If the leading byte is an unpack-data tag AND the buffer is
    ///    long enough to contain that tag's header, read the
    ///    u16-big-endian total-skip-length at `[1..3]` and skip those
    ///    bytes. (The C++ `SHIP_ASSERT`s that `len >= skip_len`; we
    ///    conservatively return `false` on malformed lengths rather
    ///    than panic, since this function is queried at read time on
    ///    bytes from the store.)
    /// 3. Remaining must be exactly `RDB_CHECKSUM_CHUNK_SIZE` (9) bytes
    ///    leading with `RDB_CHECKSUM_DATA_TAG (0x01)`.
    pub fn unpack_info_has_checksum(unpack_info: &[u8]) -> bool {
        let mut remaining: &[u8] = unpack_info;
        if remaining.is_empty() {
            return false;
        }
        if Self::is_unpack_data_tag(remaining[0]) {
            if let Some(hdr_size) = Self::get_unpack_header_size(remaining[0]) {
                if remaining.len() >= hdr_size {
                    let skip_len =
                        u16::from_be_bytes([remaining[1], remaining[2]]) as usize;
                    if remaining.len() < skip_len {
                        return false;
                    }
                    remaining = &remaining[skip_len..];
                }
            }
        }
        remaining.len() == RDB_CHECKSUM_CHUNK_SIZE
            && remaining[0] == RDB_CHECKSUM_DATA_TAG
    }

    /// True iff `table` has no explicit primary key (uses a hidden rowid).
    /// In MyRocks: `table->s->primary_key == MAX_KEY`. In our world:
    /// the `TableShareView.hidden_pk_field` slot is populated for the
    /// hidden rowid column.
    pub fn table_has_hidden_pk(table: &crate::codec::value::TableShareView) -> bool {
        table.hidden_pk_field.is_some()
    }

    /// True iff key-part `kp` can be unpacked from its mem-comparable
    /// image. Delegates to `pack_info[kp].unpack_func.is_some()`; when
    /// `false`, the field type doesn't round-trip without help from the
    /// row-side payload (e.g. some lossy collations) and index-only reads
    /// for queries that touch this part have to fall back to a PK lookup.
    pub fn can_unpack(&self, kp: u32) -> bool {
        debug_assert!(
            (kp as usize) < self.pack_info.len(),
            "can_unpack: kp {} >= pack_info.len {}",
            kp,
            self.pack_info.len()
        );
        self.pack_info[kp as usize].unpack_func.is_some()
    }

    /// True iff every keypart can be unpacked from its mem-comparable
    /// image — index-only reads always work for queries that touch this
    /// index. Vacuously true for a key with no parts.
    pub fn can_cover_lookup(&self) -> bool {
        self.pack_info.iter().all(|fp| fp.unpack_func.is_some())
    }

    /// True iff the query's `lookup_bitmap` is a subset of this row's
    /// covered-column bitmap — i.e. the index alone has every column
    /// the query touches, no PK lookup needed.
    ///
    /// Port of `rdb_datadic.cc:1173..1200`. Inspects `unpack_info`'s
    /// covered-bitmap header (which only exists when this index uses
    /// the covered-bitmap secondary format — see
    /// `use_covered_bitmap_format`).
    ///
    /// Layout of the covered header (`_DESIGN.md §0`):
    /// ```text
    /// RDB_UNPACK_COVERED_DATA_TAG (1)
    /// || u16_be(skip_length)        (2)
    /// || u16_be(covered_bitmap)     (2)  ← what we read here
    /// ```
    ///
    /// Returns `false` if:
    /// - this index isn't covered-bitmap format
    /// - `unpack_info` doesn't start with `RDB_UNPACK_COVERED_DATA_TAG`
    /// - `unpack_info` is shorter than the header
    ///
    /// MyRocks limits the bitmap to `MAX_REF_PARTS` (16) columns; we
    /// take `lookup_bitmap: u16` to match.
    pub fn covers_lookup(&self, unpack_info: &[u8], lookup_bitmap: u16) -> bool {
        if !self.use_covered_bitmap_format() {
            return false;
        }
        if unpack_info.first() != Some(&RDB_UNPACK_COVERED_DATA_TAG) {
            return false;
        }
        if unpack_info.len() < RDB_UNPACK_COVERED_HEADER_SIZE {
            return false;
        }
        // tag(1) + skip_length(2) = 3 bytes precede the covered bitmap.
        let covered = u16::from_be_bytes([unpack_info[3], unpack_info[4]]);
        // subset(a, b) ≡ (a & !b) == 0
        (lookup_bitmap & !covered) == 0
    }

    /// Compare two packed keys part-by-part without unpacking; return
    /// the first index where they differ (or `Equal` if all parts match).
    ///
    /// Port of `rdb_datadic.cc:1517..1581`. Returns `Err` on read
    /// truncation or skip_func error.
    ///
    /// **Caveat from the C++:** compare_keys passes `nullptr` for the
    /// skip_func's field arg, which works only for fixed-length skip
    /// functions. Variable-length skip routines (`skip_variable_length`,
    /// `skip_variable_space_pad`) need the field. We surface that by
    /// taking `fields: &[Option<&FieldView>]` and pass the slot through
    /// — `None` triggers the hidden-PK 8-byte raw skip (same as
    /// `read_memcmp_key_part`). Callers using indexes with variable-
    /// length columns should pass `Some(...)` for those parts.
    ///
    /// Null-byte handling matches the C++: when either marker is
    /// missing or non-{0,1} the result is `Err`; when markers differ
    /// the result is `DifferAt(i)`; when both are 0 (NULL) the part is
    /// considered equal and the walk continues.
    pub fn compare_keys(
        &self,
        key1: &[u8],
        key2: &[u8],
        fields: &[Option<&crate::codec::value::FieldView>],
    ) -> Result<CompareResult, slatedb::Error> {
        use crate::utils::buff::StringReader;
        debug_assert_eq!(
            fields.len(),
            self.key_parts as usize,
            "compare_keys: fields slice length must match self.key_parts"
        );
        let err = || slatedb::Error::data("compare_keys: truncated key".into());

        let mut r1 = StringReader::new(key1);
        let mut r2 = StringReader::new(key2);

        r1.read(INDEX_NUMBER_SIZE).ok_or_else(err)?;
        r2.read(INDEX_NUMBER_SIZE).ok_or_else(err)?;

        for i in 0..self.key_parts as usize {
            let fpi = &self.pack_info[i];

            // Null-byte handling for maybe_null parts.
            if fpi.maybe_null {
                let n1 = r1.read(1).ok_or_else(err)?[0];
                let n2 = r2.read(1).ok_or_else(err)?[0];
                if n1 != n2 {
                    return Ok(CompareResult::DifferAt(i as u32));
                }
                if n1 == 0 {
                    // both NULL: equal at this part, advance.
                    continue;
                }
                if n1 != 1 {
                    return Err(slatedb::Error::data(format!(
                        "compare_keys: invalid null marker 0x{:02x} at part {i}",
                        n1
                    )));
                }
            }

            let before1 = r1.current_pos();
            let before2 = r2.current_pos();

            match fields[i] {
                None => {
                    r1.read(crate::globals::SIZEOF_HIDDEN_PK_COLUMN)
                        .ok_or_else(err)?;
                    r2.read(crate::globals::SIZEOF_HIDDEN_PK_COLUMN)
                        .ok_or_else(err)?;
                }
                Some(f) => {
                    let skip = fpi.skip_func.ok_or_else(|| {
                        slatedb::Error::data(format!(
                            "compare_keys: missing skip_func at part {i}"
                        ))
                    })?;
                    if skip(fpi, f, &mut r1) != 0 {
                        return Err(err());
                    }
                    if skip(fpi, f, &mut r2) != 0 {
                        return Err(err());
                    }
                }
            }

            let size1 = r1.current_pos() - before1;
            let size2 = r2.current_pos() - before2;
            if size1 != size2 {
                return Ok(CompareResult::DifferAt(i as u32));
            }
            if key1[before1..before1 + size1] != key2[before2..before2 + size2] {
                return Ok(CompareResult::DifferAt(i as u32));
            }
        }

        Ok(CompareResult::Equal)
    }

    /// Extract a mem-comparable Primary Key tuple from a row of this
    /// **secondary** index. MyRocks uses "extended keys" — PK columns
    /// are appended to every SK entry so an SK→PK lookup doesn't need
    /// to re-encode the PK columns from scratch.
    ///
    /// Port of `rdb_datadic.cc:899..948`. The algorithm:
    /// 1. Write `pk_descr.index_number` BE into the first 4 bytes of
    ///    `pk_buffer`.
    /// 2. Walk every keypart of `self` (the SK) via
    ///    [`Self::read_memcmp_key_part`], capturing
    ///    `(start_offset, end_offset)` for parts that are PK columns
    ///    (per `self.pk_part_no[i] == Some(j)`).
    /// 3. Concatenate the captured byte slices into `pk_buffer` *in
    ///    PK-keypart order* — i.e. the slice captured at SK position `i`
    ///    where `pk_part_no[i] == Some(j)` goes to PK position `j`,
    ///    regardless of `i`.
    /// 4. Return total bytes written (`INDEX_NUMBER_SIZE + sum(end-start)`).
    ///
    /// Returns `None` on read truncation or a `read_memcmp_key_part`
    /// error. Caller is responsible for sizing `pk_buffer`.
    ///
    /// `fields` is one entry per **SK** keypart (length must equal
    /// `self.key_parts`); pass `None` for the hidden-PK part (won't
    /// appear in an SK, but the parameter shape stays uniform with
    /// the other walkers).
    pub fn get_primary_key_tuple(
        &self,
        pk_descr: &KeyDef,
        key: &[u8],
        pk_buffer: &mut [u8],
        fields: &[Option<&crate::codec::value::FieldView>],
    ) -> Option<usize> {
        debug_assert_eq!(
            self.index_type,
            IndexType::Secondary,
            "get_primary_key_tuple is only valid on a secondary index"
        );
        debug_assert_eq!(
            fields.len(),
            self.key_parts as usize,
            "fields slice length must match self.key_parts"
        );
        debug_assert!(self.pk_key_parts > 0, "no PK columns to extract");
        debug_assert_eq!(
            self.pk_part_no.len(),
            self.key_parts as usize,
            "pk_part_no length must match key_parts"
        );

        let pk_parts = self.pk_key_parts as usize;
        let mut starts: Vec<usize> = vec![0; pk_parts];
        let mut ends: Vec<usize> = vec![0; pk_parts];

        let mut reader = crate::utils::buff::StringReader::new(key);
        reader.read(INDEX_NUMBER_SIZE)?;

        for i in 0..self.key_parts as usize {
            let pk_idx = self.pk_part_no[i];
            if let Some(j) = pk_idx {
                starts[j as usize] = reader.current_pos();
            }
            match self.read_memcmp_key_part(&mut reader, i as u32, fields[i]) {
                ReadKeyPart::Error => return None,
                ReadKeyPart::Ok | ReadKeyPart::Null => {}
            }
            if let Some(j) = pk_idx {
                ends[j as usize] = reader.current_pos();
            }
        }

        // Write the PK index_number prefix.
        pk_buffer[..INDEX_NUMBER_SIZE]
            .copy_from_slice(&pk_descr.index_number_storage_form);
        let mut size = INDEX_NUMBER_SIZE;

        // Concatenate captured PK part bytes in PK-keypart order.
        for j in 0..pk_parts {
            let part_size = ends[j] - starts[j];
            pk_buffer[size..size + part_size]
                .copy_from_slice(&key[starts[j]..ends[j]]);
            size += part_size;
        }

        Some(size)
    }

    /// Extract the mem-comparable Secondary Key form **without** the
    /// extended PK tail. Used to feed an SK row through equality /
    /// range comparisons that should ignore the trailing PK columns.
    ///
    /// Port of `rdb_datadic.cc:959..988`. Output includes the leading
    /// `INDEX_NUMBER_SIZE` bytes (it's a *full* SK key, just without
    /// the extended PK suffix).
    ///
    /// Walks the first `user_defined_key_parts` entries of pack_info
    /// via [`Self::read_memcmp_key_part`]. The C++ reads
    /// `user_defined_key_parts` from `TABLE::key_info[m_keyno]`; we
    /// take it as a parameter because that field is populated by
    /// `KeyDef::setup` (deferred) and exposing the dependency at the
    /// call site keeps this method honest.
    ///
    /// Returns `Some((sk_memcmp_len, n_null_fields))` on success:
    /// - `sk_memcmp_len` — total bytes copied into `sk_buffer`
    ///   (index prefix + each consumed part).
    /// - `n_null_fields` — count of `read_memcmp_key_part` results
    ///   that came back as `ReadKeyPart::Null` (i.e. the field was
    ///   stored NULL).
    /// Returns `None` on read truncation or a per-part `Error`.
    pub fn get_memcmp_sk_parts(
        &self,
        key: &[u8],
        user_defined_key_parts: u32,
        sk_buffer: &mut [u8],
        fields: &[Option<&crate::codec::value::FieldView>],
    ) -> Option<(usize, u32)> {
        debug_assert!(
            user_defined_key_parts <= self.key_parts,
            "user_defined_key_parts {} > self.key_parts {}",
            user_defined_key_parts,
            self.key_parts
        );
        debug_assert!(
            fields.len() >= user_defined_key_parts as usize,
            "fields slice ({}) shorter than user_defined_key_parts ({})",
            fields.len(),
            user_defined_key_parts
        );

        let mut reader = crate::utils::buff::StringReader::new(key);
        let start = reader.current_pos();
        reader.read(INDEX_NUMBER_SIZE)?;

        let mut n_null_fields: u32 = 0;
        for i in 0..user_defined_key_parts as usize {
            match self.read_memcmp_key_part(&mut reader, i as u32, fields[i]) {
                ReadKeyPart::Error => return None,
                ReadKeyPart::Null => n_null_fields += 1,
                ReadKeyPart::Ok => {}
            }
        }

        let sk_memcmp_len = reader.current_pos() - start;
        sk_buffer[..sk_memcmp_len].copy_from_slice(&key[start..start + sk_memcmp_len]);
        Some((sk_memcmp_len, n_null_fields))
    }

    /// Total byte length of a packed key under this descriptor — the
    /// `INDEX_NUMBER_SIZE` prefix plus each keypart's mem-comparable
    /// bytes. Returns `None` on truncation or skip_func failure.
    ///
    /// Port of `rdb_datadic.cc:1591`. The MyRocks caller is `rnd_pos`
    /// (`ha_rocksdb.cc:11184`), which only ever calls this on the
    /// **primary key** descriptor — that's why this function does NOT
    /// handle the nullable-part null byte. PKs are NOT NULL by SQL
    /// convention, so the omission is sound for the only real call
    /// site. Don't call this for secondary keys with nullable parts.
    ///
    /// `fields` is one entry per keypart (length must equal
    /// `self.key_parts`). Pass `None` for the hidden-PK part — the
    /// 8 raw bytes are consumed directly without going through
    /// `skip_func`. Pass `Some(...)` for every other part.
    pub fn key_length(
        &self,
        key: &[u8],
        fields: &[Option<&crate::codec::value::FieldView>],
    ) -> Option<usize> {
        debug_assert_eq!(
            fields.len(),
            self.key_parts as usize,
            "key_length: fields slice length must match self.key_parts"
        );
        let mut reader = crate::utils::buff::StringReader::new(key);
        reader.read(INDEX_NUMBER_SIZE)?;
        for (i, field) in fields.iter().enumerate() {
            let fpi = &self.pack_info[i];
            match field {
                None => {
                    reader.read(crate::globals::SIZEOF_HIDDEN_PK_COLUMN)?;
                }
                Some(f) => {
                    let skip_func = fpi.skip_func?;
                    if skip_func(fpi, f, &mut reader) != 0 {
                        return None;
                    }
                }
            }
        }
        Some(key.len() - reader.remaining())
    }

    /// Advance `reader` past key-part `part_num`'s mem-comparable bytes
    /// without writing them anywhere. Used by upper-bound computations
    /// and by SK→PK lookups that only need to know "how long is this
    /// keypart" to find the PK tail.
    ///
    /// Port of `rdb_datadic.cc:843`. The C++ took a `TABLE*` and called
    /// `fpi->get_field_in_table(...)` internally; our port pushes the
    /// lookup out to the caller via `field: Option<&FieldView>`. Pass
    /// `None` for the hidden-PK part (the last part of a table with no
    /// explicit PK); pass `Some(...)` for every other part.
    ///
    /// Hidden-PK behaviour: 8 raw bytes (`SIZEOF_HIDDEN_PK_COLUMN`) are
    /// consumed directly without going through `skip_func` — the C++
    /// skip routine for the hidden PK ignores its field arg anyway, so
    /// the special-case is just inlining that fact.
    pub fn read_memcmp_key_part(
        &self,
        reader: &mut crate::utils::buff::StringReader,
        part_num: u32,
        field: Option<&crate::codec::value::FieldView>,
    ) -> ReadKeyPart {
        let pn = part_num as usize;
        debug_assert!(
            pn < self.pack_info.len(),
            "read_memcmp_key_part: part_num {} >= pack_info.len {}",
            part_num,
            self.pack_info.len()
        );
        let fpi = &self.pack_info[pn];

        // Null-byte prefix on nullable parts: 0x00 = NULL, 0x01 = value,
        // anything else (or under-read) is a format error.
        if fpi.maybe_null {
            let Some(slice) = reader.read(1) else {
                return ReadKeyPart::Error;
            };
            match slice[0] {
                0 => return ReadKeyPart::Null,
                1 => {} // value follows
                _ => return ReadKeyPart::Error,
            }
        }

        // Hidden-PK part: skip 8 raw bytes; the dispatched skip_func
        // would do the same and ignore its field arg.
        if field.is_none() {
            return match reader.read(crate::globals::SIZEOF_HIDDEN_PK_COLUMN) {
                Some(_) => ReadKeyPart::Ok,
                None => ReadKeyPart::Error,
            };
        }

        // Non-null, non-hidden: dispatch via the skip slot.
        let Some(skip_func) = fpi.skip_func else {
            // Missing skip_func is a setup-time bug. Treat as Error so
            // the caller can decline to decode this row.
            return ReadKeyPart::Error;
        };
        let code = skip_func(fpi, field.expect("checked Some above"), reader);
        if code == 0 {
            ReadKeyPart::Ok
        } else {
            ReadKeyPart::Error
        }
    }

    /// True iff key-part `kp` needs unpack_info sidechannel bytes to
    /// decode. Delegates to `pack_info[kp].uses_unpack_info()`.
    pub fn has_unpack_info(&self, kp: u32) -> bool {
        debug_assert!(
            (kp as usize) < self.pack_info.len(),
            "has_unpack_info: kp {} >= pack_info.len {}",
            kp,
            self.pack_info.len()
        );
        self.pack_info[kp as usize].uses_unpack_info()
    }

    /// Write `flag`'s payload bytes at the position
    /// [`calculate_index_flag_offset`] computes within an already-allocated
    /// portion of `buf`. Companion to that lookup.
    ///
    /// Port of `rdb_datadic.cc:3677`. Contract:
    /// - Caller has pre-allocated at least `offset + len` bytes in `buf`
    ///   (typically via `buf.allocate(self.total_index_flags_length, 0)`
    ///   at header-construction time).
    /// - `val.len() >= len` for the flag's payload length.
    ///
    /// Both contracts are pinned with `debug_assert!`. Violations are
    /// codec bugs at construction time, not runtime conditions, so
    /// matching the C++ `DBUG_ASSERT` semantic is appropriate.
    ///
    /// Passing [`IndexFlag::MaxFlag`] is a no-op (`len` is 0 — `MaxFlag` is
    /// a header sentinel with no payload).
    pub fn write_index_flag_field(
        &self,
        buf: &mut crate::utils::buff::StringWriter,
        val: &[u8],
        flag: IndexFlag,
    ) {
        let mut len: u32 = 0;
        let offset =
            Self::calculate_index_flag_offset(self.index_flags_bitmap, flag, Some(&mut len))
                as usize;
        let len = len as usize;
        if len == 0 {
            return; // MaxFlag / unknown bit beyond the length table
        }
        debug_assert!(
            offset + len <= buf.current_pos(),
            "write_index_flag_field: buf not pre-allocated (need {}+{}, got {})",
            offset,
            len,
            buf.current_pos()
        );
        debug_assert!(
            val.len() >= len,
            "write_index_flag_field: val too short ({} < {})",
            val.len(),
            len
        );
        buf.ptr_mut()[offset..offset + len].copy_from_slice(&val[..len]);
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

    // ----- diagnostic logging -----

    /// Emit a structured tracing event for a checksum mismatch. Port of
    /// `rdb_datadic.cc:1731` — same fields, formatted for structured
    /// logging instead of the C++ `sql_print_error` text. Drops the
    /// follow-on `my_error(ER_INTERNAL_ERROR, ...)` call; that one needs
    /// a cxx callback into the server-side diagnostics area and lands
    /// alongside the bridge.
    pub fn report_checksum_mismatch(&self, is_key: bool, data: &[u8]) {
        let kind = if is_key { "key" } else { "value" };
        let hex = crate::utils::parse::hexdump(
            data,
            crate::utils::parse::RDB_MAX_HEXDUMP_LEN,
        );
        tracing::error!(
            index_number = format!("0x{:x}", self.index_number),
            index_name = %self.name,
            bytes = data.len(),
            hex = %hex,
            "checksum mismatch in {kind} of key-value pair"
        );
    }

    // ----- TTL extractors (rdb_datadic.cc:607..) -----

    /// Read the `ttl_duration=N` qualifier from a table comment and
    /// return it in seconds. A partition-specific override
    /// (`{p}_ttl_duration=N`) takes precedence when `partition_name` is
    /// supplied — see `codec::comment_parser` for the precedence rules.
    ///
    /// Returns:
    /// - `Ok(None)` — no `ttl_duration` configured, or the entry was
    ///   malformed (matches the C++ "empty result" semantic).
    /// - `Ok(Some(n))` — TTL duration in seconds, `n > 0`.
    /// - `Err(Invalid)` — value was present but failed to parse as `u64`,
    ///   or parsed to `0`. The C++ rejects both via
    ///   `ER_RDB_TTL_DURATION_FORMAT`; we preserve that.
    ///
    /// **Deviation:** the C++ uses `strtoull` with base `0`, so it
    /// accepts `0x`-prefixed hex and `0`-prefixed octal. We accept
    /// decimal only — users always write decimal seconds and supporting
    /// the other bases would be a footgun. If a `0x...` value ever
    /// surfaces in a real migration it'll fail loudly here.
    /// Read the `ttl_col=NAME` qualifier from a table comment and resolve
    /// it against `table_share`. Returns:
    /// - `Ok(None)` — no `ttl_col` qualifier (and no validation pressure).
    /// - `Ok(Some({column_name, field_index}))` — a column matched name
    ///   AND (when `skip_checks=false`) type/null requirements.
    /// - `Err(Invalid)` — `ttl_col` was set but no column matched all the
    ///   validation requirements. Mirrors the C++
    ///   `ER_RDB_TTL_COL_FORMAT` failure.
    ///
    /// Validation when `skip_checks=false`:
    /// - `field.name == ttl_col_str` (case-sensitive ASCII match)
    /// - `field.mysql_type == MysqlType::LongLong`
    /// - `field.flags & UNSIGNED_FLAG != 0` (column declared UNSIGNED)
    /// - `field.is_not_null()` (column declared NOT NULL)
    ///
    /// When `skip_checks=true` only the name match runs. The C++ uses
    /// this from inside `setup()` when the validation has already been
    /// performed at index-create time and a redundant check would
    /// double-emit errors.
    pub fn extract_ttl_col(
        comment: &str,
        partition_name: Option<&str>,
        table_share: &crate::codec::value::TableShareView,
        skip_checks: bool,
    ) -> Result<Option<TtlColumn>, slatedb::Error> {
        let Some(m) = crate::codec::comment_parser::parse_qualifier(
            comment,
            crate::globals::TTL_COL_QUALIFIER,
            partition_name,
        ) else {
            return Ok(None);
        };

        if skip_checks {
            // Take the first name match without validation.
            for (i, field) in table_share.fields.iter().enumerate() {
                if field.name == m.value {
                    return Ok(Some(TtlColumn {
                        column_name: m.value,
                        field_index: i as u32,
                    }));
                }
            }
            return Ok(None);
        }

        for (i, field) in table_share.fields.iter().enumerate() {
            if field.name == m.value
                && field.mysql_type == crate::codec::value::MysqlType::LongLong
                && (field.flags & crate::codec::value::UNSIGNED_FLAG) != 0
                && field.is_not_null()
            {
                return Ok(Some(TtlColumn {
                    column_name: m.value,
                    field_index: i as u32,
                }));
            }
        }

        Err(slatedb::Error::invalid(format!(
            "ttl_col {:?}: must be NOT NULL BIGINT UNSIGNED column of the table",
            m.value
        )))
    }

    pub fn extract_ttl_duration(
        comment: &str,
        partition_name: Option<&str>,
    ) -> Result<Option<u64>, slatedb::Error> {
        let m = match crate::codec::comment_parser::parse_qualifier(
            comment,
            crate::globals::TTL_DURATION_QUALIFIER,
            partition_name,
        ) {
            Some(m) => m,
            None => return Ok(None),
        };
        let value = m.value.parse::<u64>().map_err(|_| {
            slatedb::Error::invalid(format!(
                "ttl_duration: expected unsigned integer, got {:?}",
                m.value
            ))
        })?;
        if value == 0 {
            return Err(slatedb::Error::invalid(format!(
                "ttl_duration must be > 0 (got {:?})",
                m.value
            )));
        }
        Ok(Some(value))
    }

    /// Populate `pack_info`, `pk_part_no`, `key_parts`, and `maxlength`
    /// for this KeyDef. Translated from `Rdb_key_def::setup`
    /// (`rdb_datadic.cc:388`).
    ///
    /// `tbl` provides the table-share view (field list + per-index
    /// schemas), and `tbl_def` provides the parent table descriptor
    /// (lets us walk the PK's key parts when extending an SK).
    ///
    /// ## Idempotency
    ///
    /// Re-entering `setup` on an already-set-up KeyDef is a no-op
    /// (early-returns `Ok(())` if `maxlength != 0`). The C++ does the
    /// same via a `maxlength != 0` short-circuit at line 401 — it's a
    /// "first thread wins" guard against the concurrent-callers
    /// scenario. We rely on `&mut self` for exclusion, but mirror the
    /// short-circuit so callers can re-validate cheaply.
    ///
    /// ## DESC indexes are rejected
    ///
    /// Matches the C++'s `ER_ILLEGAL_HA_CREATE_OPTION` (`rdb_datadic.cc:488`).
    /// We surface as `Error::invalid` so the handler can map back to
    /// the right user-facing code.
    ///
    /// ## What's not done here (vs C++)
    ///
    /// - **TTL keypart-offset lookup**
    ///   (`m_ttl_pk_key_part_offset = dst_i` when a part's field name
    ///   matches `m_ttl_column`). The C++ also calls `extract_ttl_col`
    ///   here to populate `m_ttl_column` / `m_ttl_field_index` first;
    ///   both depend on `TableShareView` carrying the table comment,
    ///   which we don't surface yet. The field defaults of `0` / empty
    ///   are harmless until TTL row encoding is wired (a separate
    ///   bear).
    /// - **`prefix_extractor` caching** (`m_prefix_extractor = opt.prefix_extractor`).
    ///   We use [`crate::codec::prefix::MyRocksPrefixExtractor`] at
    ///   bloom-filter setup time directly from `engine::db`; no
    ///   per-KeyDef cache needed.
    pub fn setup(
        &mut self,
        tbl: &crate::codec::value::TableShareView,
        tbl_def: &crate::codec::tbl_def::TblDef,
    ) -> Result<(), slatedb::Error> {
        use crate::codec::field_pack::FieldPacking;
        use crate::codec::value::IndexKeyPartView;

        if self.maxlength != 0 {
            return Ok(());
        }

        let is_hidden_pk = self.index_type == IndexType::HiddenPrimary;
        let hidden_pk_exists = Self::table_has_hidden_pk(tbl);
        let secondary_key = self.index_type == IndexType::Secondary;

        // ----- locate the SQL-layer index schema for self -----
        //
        // The hidden-PK has no entry in `tbl.indexes` (matches the C++:
        // hidden-PK isn't in MariaDB's KEY[] either). Other indexes are
        // looked up by `self.keyno`.
        let key_info: Option<&crate::codec::value::IndexSchemaView> = if is_hidden_pk {
            None
        } else {
            let idx = tbl.indexes.get(self.keyno as usize).ok_or_else(|| {
                slatedb::Error::invalid(format!(
                    "KeyDef::setup: keyno {} out of range (table has {} indexes)",
                    self.keyno,
                    tbl.indexes.len(),
                ))
            })?;
            Some(idx)
        };

        // ----- determine PK key-part count + locate PK schema -----
        let (pk_key_parts, pk_info): (u32, Option<&crate::codec::value::IndexSchemaView>) =
            if secondary_key {
                if hidden_pk_exists {
                    (1, None)
                } else {
                    let pk_idx = tbl.primary_key_index.ok_or_else(|| {
                        slatedb::Error::invalid(
                            "KeyDef::setup: SK requires either hidden_pk or primary_key_index".into(),
                        )
                    })?;
                    let pk = tbl.indexes.get(pk_idx as usize).ok_or_else(|| {
                        slatedb::Error::invalid(format!(
                            "KeyDef::setup: primary_key_index {pk_idx} out of range"
                        ))
                    })?;
                    (pk.ext_key_parts, Some(pk))
                }
            } else {
                (0, None)
            };

        // ----- compute the total key_parts the SK will hold -----
        let total_key_parts: u32 = if is_hidden_pk {
            1
        } else {
            let user_parts = key_info
                .expect("non-hidden-PK has a key_info")
                .ext_key_parts;
            if secondary_key {
                user_parts + pk_key_parts
            } else {
                user_parts
            }
        };

        // Allocate the per-part state.
        let mut pack_info: Vec<FieldPacking> =
            (0..total_key_parts).map(|_| FieldPacking::default()).collect();
        let mut pk_part_no: Vec<Option<u32>> = if secondary_key {
            vec![None; total_key_parts as usize]
        } else {
            Vec::new()
        };

        // ----- key-encoding walk -----
        let mut max_len: u32 = INDEX_NUMBER_SIZE as u32;
        let unpack_len: u32 = 0; // TODO: m_unpack_data_offset accumulator (deferred with unpack-info writers).
        // C++ tracks `max_part_len` (max max_image_len across parts)
        // but never reads it back — dropped here as dead code.
        let mut dst_i: u32 = 0;

        self.pk_key_parts = pk_key_parts;

        if is_hidden_pk {
            // Synthetic single-keypart for the hidden rowid. The
            // C++ passes `field == nullptr` and `key_part_length == 0`;
            // FieldPacking::setup handles the None case by defaulting
            // to the hidden-PK width.
            pack_info[0].setup(Some(self), None, self.keyno, 0, 0);
            pack_info[0].unpack_data_offset = unpack_len as i32;
            max_len = max_len.saturating_add(pack_info[0].max_image_len as u32);
            dst_i = 1;
        } else {
            // The user-declared parts (and extended-keys tail produced
            // by the SQL layer). For SKs we then loop again over the
            // PK's parts that aren't already covered.
            //
            // Iteration counters track three independent things — keep
            // them distinct to match the C++ shape:
            //   - `completed`: outer loop counter (C++ `src_i`). Bounds
            //     the loop to `total_key_parts` regardless of how many
            //     dedup-skips happen.
            //   - `cur_pos`: index into the currently-pointed-at
            //     array (`current_parts`). Reset to 0 when we transition
            //     into the PK-extension tail.
            //   - `dst_i`: write cursor into `pack_info` / `pk_part_no`.
            //     Does NOT advance on a dedup-skip — the final
            //     `key_parts` count is `dst_i`, smaller than
            //     `total_key_parts` when dedup happened.
            let user_view = key_info.expect("non-hidden-PK has key_info");

            let mut keyno_to_set = self.keyno;
            let mut keypart_to_set: u32 = 0;
            let mut current_parts: &[IndexKeyPartView] = &user_view.key_parts;
            let mut simulating_extkey = false;
            let mut cur_pos: u32 = 0;
            let mut completed: u32 = 0;

            while completed < total_key_parts {
                // Hidden-PK extension: synthetic 1-part tail
                // (`key_part = nullptr` branch in the C++).
                if simulating_extkey && hidden_pk_exists {
                    pack_info[dst_i as usize].setup(
                        Some(self),
                        None,
                        keyno_to_set,
                        0,
                        0,
                    );
                    pack_info[dst_i as usize].unpack_data_offset = unpack_len as i32;
                    pk_part_no[dst_i as usize] = Some(0);
                    max_len = max_len
                        .saturating_add(pack_info[dst_i as usize].max_image_len as u32);
                    dst_i += 1;
                    // Hidden-PK extension adds exactly one synthetic
                    // part; the rest of total_key_parts (if any) is
                    // accounted for by the truncate at the end.
                    break;
                }

                let kp = current_parts.get(cur_pos as usize).ok_or_else(|| {
                    slatedb::Error::invalid(format!(
                        "KeyDef::setup: ran off the end of key parts at cur_pos={cur_pos} \
                         (current_parts.len={}, completed={completed}, total={total_key_parts})",
                        current_parts.len()
                    ))
                })?;

                let field = tbl.fields.get(kp.field_idx as usize).ok_or_else(|| {
                    slatedb::Error::invalid(format!(
                        "KeyDef::setup: keypart field_idx {} out of range \
                         (table has {} fields)",
                        kp.field_idx,
                        tbl.fields.len(),
                    ))
                })?;

                // Extkey-dedup: a PK column that's already in the SK's
                // declared parts (same field_index AND same length) is
                // skipped — matches rdb_datadic.cc:494.
                if simulating_extkey && !hidden_pk_exists {
                    let already_in_sk = user_view.key_parts[..user_view.ext_key_parts as usize]
                        .iter()
                        .any(|q| {
                            q.field_idx == kp.field_idx
                                && q.key_part_length == kp.key_part_length
                        });
                    if already_in_sk {
                        cur_pos += 1;
                        completed += 1;
                        // dst_i does NOT advance; this PK part is dropped.
                        continue;
                    }
                }

                // NULL-byte accounting (C++ rdb_datadic.cc:513).
                if !field.is_not_null() {
                    max_len = max_len.saturating_add(1);
                }

                pack_info[dst_i as usize].setup(
                    Some(self),
                    Some(field),
                    keyno_to_set,
                    keypart_to_set,
                    kp.key_part_length,
                );
                pack_info[dst_i as usize].unpack_data_offset = unpack_len as i32;

                // Populate pk_part_no for SKs.
                if let Some(pk) = pk_info {
                    pk_part_no[dst_i as usize] = pk
                        .key_parts
                        .iter()
                        .take(pk_key_parts as usize)
                        .position(|q| q.field_idx == kp.field_idx)
                        .map(|p| p as u32);
                }

                max_len = max_len
                    .saturating_add(pack_info[dst_i as usize].max_image_len as u32);

                // TODO: TTL keypart-offset capture (m_ttl_pk_key_part_offset)
                // — deferred with TTL comment plumbing.

                cur_pos += 1;
                keypart_to_set = keypart_to_set.wrapping_add(1);

                // SK-extension transition: when we've consumed the
                // user-defined+extended part of the SK, switch over to
                // the PK's parts for the remaining tail. The C++
                // checks `src_i+1 == key_info->ext_key_parts` at the
                // end of the iteration body; our `completed+1` is the
                // same value.
                if secondary_key
                    && completed + 1 == user_view.ext_key_parts
                    && !simulating_extkey
                {
                    simulating_extkey = true;
                    if hidden_pk_exists {
                        // Synthetic 1-part tail handled at the top of
                        // the next iteration.
                        keyno_to_set = (tbl_def.key_count() as u32).saturating_sub(1);
                        current_parts = &[];
                        cur_pos = 0;
                        keypart_to_set = 0;
                    } else {
                        keyno_to_set = tbl
                            .primary_key_index
                            .expect("primary_key_index present when !hidden_pk_exists");
                        current_parts = &pk_info
                            .expect("pk_info present when !hidden_pk_exists")
                            .key_parts;
                        cur_pos = 0;
                        keypart_to_set = u32::MAX; // matches C++ `(uint)-1` so the next wrapping_add yields 0.
                    }
                }

                dst_i += 1;
                completed += 1;
            }
        }

        // Trim pack_info / pk_part_no to the actually-filled length
        // (extkey-dedup may have left tail slots untouched). The C++
        // does `m_key_parts = dst_i;` at line 580.
        pack_info.truncate(dst_i as usize);
        pk_part_no.truncate(dst_i as usize);

        self.pack_info = pack_info;
        self.pk_part_no = pk_part_no;
        self.key_parts = dst_i;
        self.maxlength = max_len;
        Ok(())
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
    fn unpack_header_sizes_match_constants() {
        assert_eq!(
            KeyDef::get_unpack_header_size(RDB_UNPACK_DATA_TAG),
            Some(3)
        );
        assert_eq!(
            KeyDef::get_unpack_header_size(RDB_UNPACK_COVERED_DATA_TAG),
            Some(5)
        );
        assert!(KeyDef::get_unpack_header_size(RDB_CHECKSUM_DATA_TAG).is_none());
        assert!(KeyDef::get_unpack_header_size(0xff).is_none());
    }

    #[test]
    fn unpack_info_has_checksum_empty_is_false() {
        assert!(!KeyDef::unpack_info_has_checksum(&[]));
    }

    #[test]
    fn unpack_info_has_checksum_pure_checksum_chunk_is_true() {
        // Just the 9-byte checksum chunk, no preceding unpack-data header.
        let mut chunk = [0u8; RDB_CHECKSUM_CHUNK_SIZE];
        chunk[0] = RDB_CHECKSUM_DATA_TAG;
        assert!(KeyDef::unpack_info_has_checksum(&chunk));
    }

    #[test]
    fn unpack_info_has_checksum_after_unpack_data_header_is_true() {
        // Format: unpack tag (1) + u16_be skip_len (2) + payload, then
        // checksum-chunk (9). Total skip_len = header(3) + payload(2) = 5.
        let mut buf = vec![0u8; 5 + RDB_CHECKSUM_CHUNK_SIZE];
        buf[0] = RDB_UNPACK_DATA_TAG;
        buf[1..3].copy_from_slice(&5u16.to_be_bytes()); // total skip incl. header
        // payload bytes 3..5 are anything; defaults zero.
        buf[5] = RDB_CHECKSUM_DATA_TAG;
        // rest of checksum chunk is zero bytes; doesn't matter for the predicate.
        assert!(KeyDef::unpack_info_has_checksum(&buf));
    }

    #[test]
    fn unpack_info_has_checksum_no_trailer_is_false() {
        // Unpack-data header + payload but no checksum chunk.
        let mut buf = vec![0u8; 8];
        buf[0] = RDB_UNPACK_DATA_TAG;
        buf[1..3].copy_from_slice(&8u16.to_be_bytes()); // skip entire buffer
        assert!(!KeyDef::unpack_info_has_checksum(&buf));
    }

    #[test]
    fn unpack_info_has_checksum_malformed_skip_len_is_false() {
        // skip_len says "skip past end of buffer" — corruption; conservative
        // answer is no-checksum.
        let mut buf = vec![0u8; 4];
        buf[0] = RDB_UNPACK_DATA_TAG;
        buf[1..3].copy_from_slice(&99u16.to_be_bytes());
        assert!(!KeyDef::unpack_info_has_checksum(&buf));
    }

    #[test]
    fn unpack_info_has_checksum_wrong_leading_byte_after_skip_is_false() {
        // Looks like a checksum chunk in length but the tag byte is wrong.
        let mut chunk = [0u8; RDB_CHECKSUM_CHUNK_SIZE];
        chunk[0] = 0xfe;
        assert!(!KeyDef::unpack_info_has_checksum(&chunk));
    }

    // ----- can_unpack / has_unpack_info (FieldPacking delegation) -----

    fn dummy_unpack(
        _fpi: &mut crate::codec::field_pack::FieldPacking,
        _field: &mut crate::codec::value::FieldView,
        _field_ptr: &mut [u8],
        _reader: &mut crate::utils::buff::StringReader,
        _unpack_reader: Option<&mut crate::utils::buff::StringReader>,
    ) -> i32 {
        0
    }

    fn dummy_make_unpack(
        _codec: &crate::codec::field_pack::CollationCodec,
        _field: &crate::codec::value::FieldView,
        _ctx: &mut crate::codec::field_pack::PackFieldContext<'_>,
    ) {
    }

    fn kd_with_pack_info(parts: Vec<crate::codec::field_pack::FieldPacking>) -> KeyDef {
        let mut kd = forward_pk(1);
        kd.key_parts = parts.len() as u32;
        kd.pack_info = parts;
        kd
    }

    #[test]
    fn can_unpack_reports_per_keypart_dispatch_slot() {
        let mut with_unpack = crate::codec::field_pack::FieldPacking::default();
        with_unpack.unpack_func = Some(dummy_unpack);
        let without = crate::codec::field_pack::FieldPacking::default();

        let kd = kd_with_pack_info(vec![with_unpack, without]);
        assert!(kd.can_unpack(0));
        assert!(!kd.can_unpack(1));
    }

    #[test]
    fn has_unpack_info_reports_per_keypart_make_slot() {
        let mut with_info = crate::codec::field_pack::FieldPacking::default();
        with_info.make_unpack_info_func = Some(dummy_make_unpack);
        let without = crate::codec::field_pack::FieldPacking::default();

        let kd = kd_with_pack_info(vec![with_info, without]);
        assert!(kd.has_unpack_info(0));
        assert!(!kd.has_unpack_info(1));
    }

    // ----- covers_lookup -----

    fn sk_with_covered_format() -> KeyDef {
        // Secondary index at UPDATE3 format version → covered-bitmap eligible.
        let mut kd = reverse_sk(7);
        kd.kv_format_version = SECONDARY_FORMAT_VERSION_UPDATE3;
        kd
    }

    fn covered_unpack_info(covered_bitmap: u16) -> Vec<u8> {
        let mut buf = vec![0u8; RDB_UNPACK_COVERED_HEADER_SIZE];
        buf[0] = RDB_UNPACK_COVERED_DATA_TAG;
        // Skip-length field (bytes 1..3) doesn't affect covers_lookup.
        buf[1..3].copy_from_slice(&(RDB_UNPACK_COVERED_HEADER_SIZE as u16).to_be_bytes());
        // Covered bitmap (bytes 3..5).
        buf[3..5].copy_from_slice(&covered_bitmap.to_be_bytes());
        buf
    }

    #[test]
    fn covers_lookup_requires_covered_bitmap_format() {
        // PK is never covered-bitmap eligible.
        let pk = forward_pk(1);
        let header = covered_unpack_info(0xffff);
        assert!(!pk.covers_lookup(&header, 0x0001));
    }

    #[test]
    fn covers_lookup_wrong_tag_is_false() {
        let sk = sk_with_covered_format();
        let mut header = covered_unpack_info(0xffff);
        header[0] = RDB_UNPACK_DATA_TAG; // 0x02 — not covered tag
        assert!(!sk.covers_lookup(&header, 0x0001));
    }

    #[test]
    fn covers_lookup_short_header_is_false() {
        let sk = sk_with_covered_format();
        let short = vec![RDB_UNPACK_COVERED_DATA_TAG, 0x00]; // < 5 bytes
        assert!(!sk.covers_lookup(&short, 0x0001));
    }

    #[test]
    fn covers_lookup_subset_succeeds() {
        let sk = sk_with_covered_format();
        // Index covers columns 0..4; query touches 0 and 2.
        let header = covered_unpack_info(0b0000_1111);
        assert!(sk.covers_lookup(&header, 0b0000_0101));
    }

    #[test]
    fn covers_lookup_extra_bit_in_lookup_fails() {
        let sk = sk_with_covered_format();
        // Index covers columns 0..4; query touches column 5 → not covered.
        let header = covered_unpack_info(0b0000_1111);
        assert!(!sk.covers_lookup(&header, 0b0010_0000));
    }

    #[test]
    fn covers_lookup_empty_query_is_vacuously_covered() {
        let sk = sk_with_covered_format();
        // covered=0, lookup=0 — every bit in lookup (none) is in covered.
        let header = covered_unpack_info(0);
        assert!(sk.covers_lookup(&header, 0));
    }

    #[test]
    fn covers_lookup_full_query_against_empty_covered_fails() {
        let sk = sk_with_covered_format();
        let header = covered_unpack_info(0);
        assert!(!sk.covers_lookup(&header, 0x0001));
    }

    // ----- compare_keys -----

    fn kd_two_part_fixed() -> KeyDef {
        let mut kd = forward_pk(99);
        kd.key_parts = 2;
        kd.pack_info = (0..2)
            .map(|_| {
                let mut fp = crate::codec::field_pack::FieldPacking::default();
                fp.skip_func = Some(dummy_skip_consume_4);
                fp
            })
            .collect();
        kd
    }

    #[test]
    fn compare_keys_identical_keys_are_equal() {
        let kd = kd_two_part_fixed();
        let key = [0, 0, 0, 99, 1, 2, 3, 4, 5, 6, 7, 8];
        let f = dummy_field();
        assert_eq!(
            kd.compare_keys(&key, &key, &[Some(&f), Some(&f)])
                .expect("ok"),
            CompareResult::Equal
        );
    }

    #[test]
    fn compare_keys_first_diff_is_in_part_0() {
        let kd = kd_two_part_fixed();
        let a = [0, 0, 0, 99, 1, 2, 3, 4, 5, 6, 7, 8];
        let b = [0, 0, 0, 99, 9, 2, 3, 4, 5, 6, 7, 8];
        let f = dummy_field();
        assert_eq!(
            kd.compare_keys(&a, &b, &[Some(&f), Some(&f)]).expect("ok"),
            CompareResult::DifferAt(0)
        );
    }

    #[test]
    fn compare_keys_first_diff_is_in_part_1() {
        let kd = kd_two_part_fixed();
        let a = [0, 0, 0, 99, 1, 2, 3, 4, 5, 6, 7, 8];
        let b = [0, 0, 0, 99, 1, 2, 3, 4, 5, 6, 7, 9];
        let f = dummy_field();
        assert_eq!(
            kd.compare_keys(&a, &b, &[Some(&f), Some(&f)]).expect("ok"),
            CompareResult::DifferAt(1)
        );
    }

    #[test]
    fn compare_keys_truncated_index_prefix_is_error() {
        let kd = kd_two_part_fixed();
        let key = [0u8, 0]; // < 4 bytes
        let f = dummy_field();
        assert!(kd
            .compare_keys(&key, &key, &[Some(&f), Some(&f)])
            .is_err());
    }

    #[test]
    fn compare_keys_handles_both_null_parts_as_equal() {
        let mut kd = kd_two_part_fixed();
        kd.pack_info[0].maybe_null = true;
        kd.pack_info[1].maybe_null = true;

        // Layout: idx(4) + null(0) + null(0) — both parts NULL on both sides.
        let key = [0u8, 0, 0, 99, 0, 0];
        let f = dummy_field();
        assert_eq!(
            kd.compare_keys(&key, &key, &[Some(&f), Some(&f)])
                .expect("ok"),
            CompareResult::Equal
        );
    }

    #[test]
    fn compare_keys_differing_null_markers_yield_differ_at() {
        let mut kd = kd_two_part_fixed();
        kd.pack_info[0].maybe_null = true;
        kd.pack_info[1].maybe_null = true;

        // Both have a leading-byte-0-or-1 then optionally 4 value bytes.
        let a = [0u8, 0, 0, 99, /* part 0 NULL */ 0, /* part 1 value */ 1, 1, 2, 3, 4];
        let b = [0u8, 0, 0, 99, /* part 0 value */ 1, 1, 2, 3, 4, /* part 1 NULL */ 0];
        let f = dummy_field();
        // Part 0's null markers differ → DifferAt(0).
        assert_eq!(
            kd.compare_keys(&a, &b, &[Some(&f), Some(&f)]).expect("ok"),
            CompareResult::DifferAt(0)
        );
    }

    #[test]
    fn compare_keys_invalid_null_marker_is_error() {
        let mut kd = kd_two_part_fixed();
        kd.pack_info[0].maybe_null = true;
        kd.pack_info[1].maybe_null = true;

        // Both sides have 0xff as the null marker — invalid (not 0 or 1).
        let key = [0u8, 0, 0, 99, 0xff];
        let f = dummy_field();
        assert!(kd
            .compare_keys(&key, &key, &[Some(&f), Some(&f)])
            .is_err());
    }

    // ----- get_memcmp_sk_parts -----

    #[test]
    fn get_memcmp_sk_parts_strips_extended_pk_tail() {
        // SK has 3 keyparts: 2 user-defined + 1 extended PK column.
        let sk = sk_with_pk_extension(99, vec![None, None, Some(0)], 1);

        // Input: SK_idx (4) + 2 SK cols (4 each) + 1 extended PK col (4) = 16.
        let mut key = [0u8; 16];
        key[..4].copy_from_slice(&99u32.to_be_bytes());
        key[4..8].copy_from_slice(b"sk_0");
        key[8..12].copy_from_slice(b"sk_1");
        key[12..16].copy_from_slice(b"PKta"); // extended PK — must NOT appear in output

        let mut buf = [0u8; 32];
        let f = dummy_field();
        let fields = [Some(&f), Some(&f), Some(&f)];

        let (len, nulls) = sk
            .get_memcmp_sk_parts(&key, 2, &mut buf, &fields)
            .expect("ok");

        // Output = SK_idx (4) + 2 SK cols (8) = 12 bytes; PK tail dropped.
        assert_eq!(len, 12);
        assert_eq!(nulls, 0);
        assert_eq!(&buf[..4], &99u32.to_be_bytes());
        assert_eq!(&buf[4..8], b"sk_0");
        assert_eq!(&buf[8..12], b"sk_1");
        // Trailing bytes in buf were not touched beyond `len`.
        assert_eq!(&buf[12..16], &[0; 4]);
    }

    #[test]
    fn get_memcmp_sk_parts_counts_null_fields() {
        // 2 user-defined parts, both nullable; first stored as NULL,
        // second as a value.
        let sk = {
            let mut k = sk_with_pk_extension(99, vec![None, None], 0);
            k.pack_info[0].maybe_null = true;
            k.pack_info[1].maybe_null = true;
            k
        };

        // Layout: SK_idx (4) + null marker (1, = 0) + null marker (1, = 1) + value (4).
        let mut key = vec![0u8; 4 + 1 + 1 + 4];
        key[..4].copy_from_slice(&99u32.to_be_bytes());
        key[4] = 0; // NULL
        key[5] = 1; // value follows
        key[6..10].copy_from_slice(b"v_v_");

        let mut buf = [0u8; 32];
        let f = dummy_field();
        let fields = [Some(&f), Some(&f)];

        let (len, nulls) = sk
            .get_memcmp_sk_parts(&key, 2, &mut buf, &fields)
            .expect("ok");
        assert_eq!(len, 10);
        assert_eq!(nulls, 1);
    }

    #[test]
    fn get_memcmp_sk_parts_returns_none_on_truncated_input() {
        let sk = sk_with_pk_extension(99, vec![None, None, Some(0)], 1);
        let key = [0u8, 0, 0, 99, 1]; // index + only 1 byte of the first part
        let mut buf = [0u8; 32];
        let f = dummy_field();
        assert_eq!(
            sk.get_memcmp_sk_parts(&key, 2, &mut buf, &[Some(&f), Some(&f), Some(&f)]),
            None
        );
    }

    // ----- get_primary_key_tuple -----

    fn sk_with_pk_extension(
        sk_index_num: u32,
        pk_part_no: Vec<Option<u32>>,
        pk_key_parts: u32,
    ) -> KeyDef {
        let mut sk = KeyDef::new_skeleton(
            sk_index_num,
            7,
            1,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Secondary,
            SECONDARY_FORMAT_VERSION_LATEST,
            false,
            "sk",
        );
        sk.key_parts = pk_part_no.len() as u32;
        sk.pk_part_no = pk_part_no;
        sk.pk_key_parts = pk_key_parts;
        // Every part uses the 4-byte skip helper.
        sk.pack_info = (0..sk.key_parts)
            .map(|_| {
                let mut fp = crate::codec::field_pack::FieldPacking::default();
                fp.skip_func = Some(dummy_skip_consume_4);
                fp
            })
            .collect();
        sk
    }

    #[test]
    fn get_primary_key_tuple_single_pk_column_at_end_of_sk() {
        let sk = sk_with_pk_extension(99, vec![None, Some(0)], 1);
        let pk = forward_pk(42);

        // Key layout: SK_idx (4) + SK col (4) + PK col (4) = 12 bytes.
        let mut key = [0u8; 12];
        key[..4].copy_from_slice(&99u32.to_be_bytes());
        key[4..8].copy_from_slice(b"SKxx");
        key[8..12].copy_from_slice(b"PKaa");

        let mut buf = [0u8; 32];
        let f = dummy_field();
        let fields = [Some(&f), Some(&f)];
        let n = sk
            .get_primary_key_tuple(&pk, &key, &mut buf, &fields)
            .expect("ok");

        // Expected: PK_idx (4) + PK col (4) = 8 bytes.
        assert_eq!(n, 8);
        assert_eq!(&buf[..4], &42u32.to_be_bytes());
        assert_eq!(&buf[4..8], b"PKaa");
    }

    #[test]
    fn get_primary_key_tuple_reorders_pk_columns_by_pk_keypart_index() {
        // Two-column PK extended into the SK at positions 1 and 2 in
        // REVERSE PK order: pk_part_no[1] = Some(1), pk_part_no[2] = Some(0).
        // Expected output places SK-position-2's bytes (PK kp 0) BEFORE
        // SK-position-1's bytes (PK kp 1).
        let sk = sk_with_pk_extension(
            99,
            vec![None, Some(1), Some(0), None],
            2,
        );
        let pk = forward_pk(42);

        // Key layout: SK_idx (4) + 4 cols * 4 bytes = 20 bytes.
        let mut key = [0u8; 20];
        key[..4].copy_from_slice(&99u32.to_be_bytes());
        key[4..8].copy_from_slice(b"sk_0");
        key[8..12].copy_from_slice(b"PKb1"); // pk_part_no[1] → PK keypart 1
        key[12..16].copy_from_slice(b"PKa0"); // pk_part_no[2] → PK keypart 0
        key[16..20].copy_from_slice(b"sk_3");

        let mut buf = [0u8; 32];
        let f = dummy_field();
        let fields = [Some(&f), Some(&f), Some(&f), Some(&f)];
        let n = sk
            .get_primary_key_tuple(&pk, &key, &mut buf, &fields)
            .expect("ok");

        // PK_idx (4) + PK kp 0 (4) + PK kp 1 (4) = 12 bytes.
        assert_eq!(n, 12);
        assert_eq!(&buf[..4], &42u32.to_be_bytes());
        // PK keypart 0 came from SK-position 2.
        assert_eq!(&buf[4..8], b"PKa0");
        // PK keypart 1 came from SK-position 1.
        assert_eq!(&buf[8..12], b"PKb1");
    }

    #[test]
    fn get_primary_key_tuple_returns_none_on_truncated_index_prefix() {
        let sk = sk_with_pk_extension(99, vec![Some(0)], 1);
        let pk = forward_pk(42);
        let key = [0u8, 0]; // < 4 bytes
        let mut buf = [0u8; 32];
        let f = dummy_field();
        assert_eq!(
            sk.get_primary_key_tuple(&pk, &key, &mut buf, &[Some(&f)]),
            None
        );
    }

    #[test]
    fn get_primary_key_tuple_returns_none_when_part_under_reads() {
        let sk = sk_with_pk_extension(99, vec![None, Some(0)], 1);
        let pk = forward_pk(42);
        // SK_idx (4) + SK col (4) + only 2 bytes of PK col.
        let key = [0u8, 0, 0, 99, 1, 2, 3, 4, 5, 6];
        let mut buf = [0u8; 32];
        let f = dummy_field();
        assert_eq!(
            sk.get_primary_key_tuple(&pk, &key, &mut buf, &[Some(&f), Some(&f)]),
            None
        );
    }

    // ----- key_length -----

    #[test]
    fn key_length_two_part_pk_consumes_index_prefix_plus_skip_funcs() {
        let mut fp0 = crate::codec::field_pack::FieldPacking::default();
        fp0.skip_func = Some(dummy_skip_consume_4);
        let mut fp1 = crate::codec::field_pack::FieldPacking::default();
        fp1.skip_func = Some(dummy_skip_consume_4);
        let kd = kd_with_pack_info(vec![fp0, fp1]);

        // 4-byte index_number + 4 bytes part 0 + 4 bytes part 1 = 12.
        let key = [0, 0, 0, 42, 1, 2, 3, 4, 5, 6, 7, 8];
        let f = dummy_field();
        let fields = [Some(&f), Some(&f)];
        assert_eq!(kd.key_length(&key, &fields), Some(12));
    }

    #[test]
    fn key_length_returns_none_when_index_prefix_truncated() {
        let kd = kd_with_pack_info(vec![{
            let mut f = crate::codec::field_pack::FieldPacking::default();
            f.skip_func = Some(dummy_skip_consume_4);
            f
        }]);
        let key = [0u8, 0]; // < 4 bytes
        let f = dummy_field();
        assert_eq!(kd.key_length(&key, &[Some(&f)]), None);
    }

    #[test]
    fn key_length_returns_none_when_a_part_under_reads() {
        let kd = kd_with_pack_info(vec![{
            let mut f = crate::codec::field_pack::FieldPacking::default();
            f.skip_func = Some(dummy_skip_consume_4);
            f
        }]);
        // 4-byte index_number + only 2 of the expected 4 part bytes.
        let key = [0u8, 0, 0, 42, 1, 2];
        let f = dummy_field();
        assert_eq!(kd.key_length(&key, &[Some(&f)]), None);
    }

    #[test]
    fn key_length_handles_hidden_pk_part_as_eight_raw_bytes() {
        let kd = kd_with_pack_info(vec![
            crate::codec::field_pack::FieldPacking::default(), // hidden PK
        ]);
        // 4-byte index_number + 8 bytes hidden PK = 12.
        let key = [0u8; 12];
        assert_eq!(kd.key_length(&key, &[None]), Some(12));
    }

    #[test]
    fn key_length_returns_none_when_skip_func_missing() {
        let kd = kd_with_pack_info(vec![
            // No skip_func assigned — setup-time bug surfaces as None.
            crate::codec::field_pack::FieldPacking::default(),
        ]);
        let key = [0u8; 12];
        let f = dummy_field();
        assert_eq!(kd.key_length(&key, &[Some(&f)]), None);
    }

    // ----- read_memcmp_key_part -----

    fn dummy_skip_consume_4(
        _fpi: &crate::codec::field_pack::FieldPacking,
        _field: &crate::codec::value::FieldView,
        reader: &mut crate::utils::buff::StringReader,
    ) -> i32 {
        if reader.read(4).is_some() {
            0
        } else {
            1
        }
    }

    fn dummy_field() -> crate::codec::value::FieldView {
        crate::codec::value::FieldView {
            name: "x".into(),
            mysql_type: crate::codec::value::MysqlType::Long,
            pack_length: 4,
            output_offset: 0,
            null_marker: None,
            length: 4,
            charset_id: 63,
            flags: 0,
            decimals: 0,
        }
    }

    #[test]
    fn read_memcmp_key_part_non_null_dispatches_skip_func() {
        let mut fp = crate::codec::field_pack::FieldPacking::default();
        fp.skip_func = Some(dummy_skip_consume_4);
        let kd = kd_with_pack_info(vec![fp]);

        let bytes = [0u8, 0, 0, 7, 99, 99];
        let mut reader = crate::utils::buff::StringReader::new(&bytes);
        let f = dummy_field();

        assert_eq!(
            kd.read_memcmp_key_part(&mut reader, 0, Some(&f)),
            ReadKeyPart::Ok
        );
        assert_eq!(reader.current_pos(), 4);
    }

    #[test]
    fn read_memcmp_key_part_nullable_zero_byte_is_null() {
        let mut fp = crate::codec::field_pack::FieldPacking::default();
        fp.maybe_null = true;
        fp.skip_func = Some(dummy_skip_consume_4);
        let kd = kd_with_pack_info(vec![fp]);

        let bytes = [0u8, 1, 2, 3, 4]; // leading 0 → NULL; rest untouched
        let mut reader = crate::utils::buff::StringReader::new(&bytes);
        let f = dummy_field();

        assert_eq!(
            kd.read_memcmp_key_part(&mut reader, 0, Some(&f)),
            ReadKeyPart::Null
        );
        // Only the null byte was consumed; skip_func was not called.
        assert_eq!(reader.current_pos(), 1);
    }

    #[test]
    fn read_memcmp_key_part_nullable_one_byte_then_value() {
        let mut fp = crate::codec::field_pack::FieldPacking::default();
        fp.maybe_null = true;
        fp.skip_func = Some(dummy_skip_consume_4);
        let kd = kd_with_pack_info(vec![fp]);

        let bytes = [1u8, 0xaa, 0xbb, 0xcc, 0xdd, 0xee];
        let mut reader = crate::utils::buff::StringReader::new(&bytes);
        let f = dummy_field();

        assert_eq!(
            kd.read_memcmp_key_part(&mut reader, 0, Some(&f)),
            ReadKeyPart::Ok
        );
        // Null byte + 4 value bytes = 5 consumed.
        assert_eq!(reader.current_pos(), 5);
    }

    #[test]
    fn read_memcmp_key_part_nullable_invalid_marker_is_error() {
        let mut fp = crate::codec::field_pack::FieldPacking::default();
        fp.maybe_null = true;
        fp.skip_func = Some(dummy_skip_consume_4);
        let kd = kd_with_pack_info(vec![fp]);

        let bytes = [0xffu8, 0, 0, 0, 0];
        let mut reader = crate::utils::buff::StringReader::new(&bytes);
        let f = dummy_field();
        assert_eq!(
            kd.read_memcmp_key_part(&mut reader, 0, Some(&f)),
            ReadKeyPart::Error
        );
    }

    #[test]
    fn read_memcmp_key_part_hidden_pk_skips_eight_bytes() {
        let fp = crate::codec::field_pack::FieldPacking::default();
        let kd = kd_with_pack_info(vec![fp]);

        let bytes = [0u8; 12];
        let mut reader = crate::utils::buff::StringReader::new(&bytes);
        // field = None signals "hidden PK part".
        assert_eq!(
            kd.read_memcmp_key_part(&mut reader, 0, None),
            ReadKeyPart::Ok
        );
        assert_eq!(reader.current_pos(), 8);
    }

    #[test]
    fn read_memcmp_key_part_hidden_pk_truncated_is_error() {
        let fp = crate::codec::field_pack::FieldPacking::default();
        let kd = kd_with_pack_info(vec![fp]);

        let bytes = [0u8; 5]; // < 8
        let mut reader = crate::utils::buff::StringReader::new(&bytes);
        assert_eq!(
            kd.read_memcmp_key_part(&mut reader, 0, None),
            ReadKeyPart::Error
        );
    }

    #[test]
    fn read_memcmp_key_part_missing_skip_func_is_error() {
        // pack_info[0] has no skip_func set — setup-time bug; surface as Error.
        let fp = crate::codec::field_pack::FieldPacking::default();
        let kd = kd_with_pack_info(vec![fp]);
        let bytes = [0u8; 4];
        let mut reader = crate::utils::buff::StringReader::new(&bytes);
        let f = dummy_field();
        assert_eq!(
            kd.read_memcmp_key_part(&mut reader, 0, Some(&f)),
            ReadKeyPart::Error
        );
    }

    #[test]
    fn can_cover_lookup_requires_every_keypart_unpackable() {
        let mut with_u = crate::codec::field_pack::FieldPacking::default();
        with_u.unpack_func = Some(dummy_unpack);
        let without = crate::codec::field_pack::FieldPacking::default();

        let all_unpackable = kd_with_pack_info(vec![
            { let mut f = crate::codec::field_pack::FieldPacking::default(); f.unpack_func = Some(dummy_unpack); f },
            { let mut f = crate::codec::field_pack::FieldPacking::default(); f.unpack_func = Some(dummy_unpack); f },
        ]);
        assert!(all_unpackable.can_cover_lookup());

        let one_missing = kd_with_pack_info(vec![with_u, without]);
        assert!(!one_missing.can_cover_lookup());
    }

    #[test]
    fn can_cover_lookup_is_vacuously_true_for_empty_pack_info() {
        let kd = forward_pk(1); // pack_info defaults to empty Vec
        assert!(kd.can_cover_lookup());
    }

    #[test]
    fn can_unpack_and_has_unpack_info_are_independent() {
        // A field could produce unpack_info but be unable to fully
        // unpack from the memcmp image (and vice versa) — the two slots
        // are intentionally separate.
        let mut both = crate::codec::field_pack::FieldPacking::default();
        both.unpack_func = Some(dummy_unpack);
        both.make_unpack_info_func = Some(dummy_make_unpack);

        let mut only_unpack = crate::codec::field_pack::FieldPacking::default();
        only_unpack.unpack_func = Some(dummy_unpack);

        let mut only_make = crate::codec::field_pack::FieldPacking::default();
        only_make.make_unpack_info_func = Some(dummy_make_unpack);

        let neither = crate::codec::field_pack::FieldPacking::default();

        let kd = kd_with_pack_info(vec![both, only_unpack, only_make, neither]);
        assert_eq!(
            (kd.can_unpack(0), kd.has_unpack_info(0)),
            (true, true)
        );
        assert_eq!(
            (kd.can_unpack(1), kd.has_unpack_info(1)),
            (true, false)
        );
        assert_eq!(
            (kd.can_unpack(2), kd.has_unpack_info(2)),
            (false, true)
        );
        assert_eq!(
            (kd.can_unpack(3), kd.has_unpack_info(3)),
            (false, false)
        );
    }

    // ----- write_index_flag_field -----

    fn key_def_with_ttl_flag() -> KeyDef {
        let mut kd = forward_pk(1);
        kd.index_flags_bitmap = IndexFlag::TtlFlag as u32;
        kd.total_index_flags_length = 8;
        kd
    }

    #[test]
    fn write_index_flag_field_writes_ttl_payload_at_zero_offset() {
        use crate::utils::buff::StringWriter;
        let kd = key_def_with_ttl_flag();
        let mut buf = StringWriter::new();
        buf.allocate(kd.total_index_flags_length as usize, 0);

        let payload = 0x0102_0304_0506_0708u64.to_be_bytes();
        kd.write_index_flag_field(&mut buf, &payload, IndexFlag::TtlFlag);

        assert_eq!(&buf.ptr()[..8], &payload);
    }

    #[test]
    fn write_index_flag_field_overwrites_only_the_named_region() {
        use crate::utils::buff::StringWriter;
        let kd = key_def_with_ttl_flag();
        let mut buf = StringWriter::new();
        // Pre-fill with a sentinel; padding bytes after the flag region
        // should remain untouched.
        buf.allocate(16, 0xff);

        let payload = [0xaa; 8];
        kd.write_index_flag_field(&mut buf, &payload, IndexFlag::TtlFlag);

        assert_eq!(&buf.ptr()[..8], &payload);
        assert_eq!(&buf.ptr()[8..], &[0xff; 8], "trailing bytes untouched");
    }

    #[test]
    fn write_index_flag_field_max_flag_is_noop() {
        use crate::utils::buff::StringWriter;
        let kd = key_def_with_ttl_flag();
        let mut buf = StringWriter::new();
        buf.allocate(8, 0xee);
        // MaxFlag has no payload (len=0) — must not panic on the
        // pre-allocation check and must not modify any bytes.
        kd.write_index_flag_field(&mut buf, &[], IndexFlag::MaxFlag);
        assert_eq!(buf.ptr(), &[0xee; 8]);
    }

    #[test]
    fn table_has_hidden_pk_reads_the_field_slot() {
        use crate::codec::value::TableShareView;
        let with_hidden = TableShareView {
            fields: Vec::new(),
            null_bytes: 0,
            row_length: 0,
            hidden_pk_field: Some(0),
            indexes: Vec::new(),
            primary_key_index: None,
        };
        let without = TableShareView {
            fields: Vec::new(),
            null_bytes: 0,
            row_length: 0,
            hidden_pk_field: None,
            indexes: Vec::new(),
            primary_key_index: None,
        };
        assert!(KeyDef::table_has_hidden_pk(&with_hidden));
        assert!(!KeyDef::table_has_hidden_pk(&without));
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

    // ----- extract_ttl_duration -----

    #[test]
    fn ttl_duration_missing_is_ok_none() {
        assert_eq!(KeyDef::extract_ttl_duration("", None).expect("ok"), None);
        assert_eq!(
            KeyDef::extract_ttl_duration("cfname=audit", None).expect("ok"),
            None
        );
    }

    #[test]
    fn ttl_duration_plain_parses_to_seconds() {
        assert_eq!(
            KeyDef::extract_ttl_duration("ttl_duration=3600", None).expect("ok"),
            Some(3600)
        );
    }

    #[test]
    fn ttl_duration_partition_override_wins() {
        let comment = "ttl_duration=3600;p0_ttl_duration=60";
        assert_eq!(
            KeyDef::extract_ttl_duration(comment, Some("p0")).expect("ok"),
            Some(60)
        );
        assert_eq!(
            KeyDef::extract_ttl_duration(comment, Some("p9")).expect("ok"),
            Some(3600),
            "no partition override → fall back to table-level"
        );
    }

    #[test]
    fn ttl_duration_malformed_value_is_invalid() {
        let err = KeyDef::extract_ttl_duration("ttl_duration=abc", None).unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Invalid));

        // u64 overflow.
        let err = KeyDef::extract_ttl_duration(
            "ttl_duration=99999999999999999999",
            None,
        )
        .unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Invalid));
    }

    #[test]
    fn ttl_duration_zero_is_rejected() {
        // Matches the C++ which treats strtoull-returns-0 as an error,
        // intentionally conflating "0 literal" with "parse failure".
        let err = KeyDef::extract_ttl_duration("ttl_duration=0", None).unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Invalid));
    }

    #[test]
    fn ttl_duration_malformed_qualifier_is_ok_none() {
        // The comment_parser returns None for "ttl_duration=" (no value).
        // We surface that as Ok(None), not Err — matches C++ empty-result.
        assert_eq!(
            KeyDef::extract_ttl_duration("ttl_duration=", None).expect("ok"),
            None
        );
    }

    // ----- extract_ttl_col -----

    fn build_table_share(fields: Vec<crate::codec::value::FieldView>) -> crate::codec::value::TableShareView {
        crate::codec::value::TableShareView {
            null_bytes: 0,
            row_length: fields.iter().map(|f| f.pack_length).sum(),
            hidden_pk_field: None,
            fields,
            indexes: Vec::new(),
            primary_key_index: None,
        }
    }

    fn ttl_col_field(name: &str) -> crate::codec::value::FieldView {
        crate::codec::value::FieldView {
            name: name.into(),
            mysql_type: crate::codec::value::MysqlType::LongLong,
            pack_length: 8,
            output_offset: 0,
            null_marker: None, // NOT NULL
            length: 8,
            charset_id: 63,
            flags: crate::codec::value::UNSIGNED_FLAG,
            decimals: 0,
        }
    }

    #[test]
    fn ttl_col_missing_qualifier_is_ok_none() {
        let ts = build_table_share(vec![ttl_col_field("created_at")]);
        assert_eq!(
            KeyDef::extract_ttl_col("", None, &ts, false).expect("ok"),
            None
        );
        assert_eq!(
            KeyDef::extract_ttl_col("cfname=audit", None, &ts, false).expect("ok"),
            None
        );
    }

    #[test]
    fn ttl_col_validated_match_returns_column() {
        let ts = build_table_share(vec![ttl_col_field("created_at")]);
        let got = KeyDef::extract_ttl_col("ttl_col=created_at", None, &ts, false)
            .expect("ok")
            .expect("present");
        assert_eq!(got.column_name, "created_at");
        assert_eq!(got.field_index, 0);
    }

    #[test]
    fn ttl_col_missing_column_in_table_is_error() {
        let ts = build_table_share(vec![ttl_col_field("created_at")]);
        let err = KeyDef::extract_ttl_col("ttl_col=does_not_exist", None, &ts, false)
            .unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Invalid));
    }

    #[test]
    fn ttl_col_wrong_type_is_error() {
        let mut f = ttl_col_field("created_at");
        f.mysql_type = crate::codec::value::MysqlType::Long; // INT, not BIGINT
        let ts = build_table_share(vec![f]);
        assert!(matches!(
            KeyDef::extract_ttl_col("ttl_col=created_at", None, &ts, false)
                .unwrap_err()
                .kind(),
            slatedb::ErrorKind::Invalid
        ));
    }

    #[test]
    fn ttl_col_signed_is_error() {
        let mut f = ttl_col_field("created_at");
        f.flags = 0; // strips UNSIGNED_FLAG
        let ts = build_table_share(vec![f]);
        assert!(matches!(
            KeyDef::extract_ttl_col("ttl_col=created_at", None, &ts, false)
                .unwrap_err()
                .kind(),
            slatedb::ErrorKind::Invalid
        ));
    }

    #[test]
    fn ttl_col_nullable_is_error() {
        let mut f = ttl_col_field("created_at");
        f.null_marker = Some((0, 1));
        let ts = build_table_share(vec![f]);
        assert!(matches!(
            KeyDef::extract_ttl_col("ttl_col=created_at", None, &ts, false)
                .unwrap_err()
                .kind(),
            slatedb::ErrorKind::Invalid
        ));
    }

    #[test]
    fn ttl_col_skip_checks_bypasses_type_and_null_validation() {
        let mut wrong_type = ttl_col_field("created_at");
        wrong_type.mysql_type = crate::codec::value::MysqlType::Long;
        wrong_type.null_marker = Some((0, 1));
        wrong_type.flags = 0;
        let ts = build_table_share(vec![wrong_type]);

        // skip_checks=true: name match alone succeeds.
        let got = KeyDef::extract_ttl_col("ttl_col=created_at", None, &ts, true)
            .expect("ok")
            .expect("present");
        assert_eq!(got.column_name, "created_at");
        assert_eq!(got.field_index, 0);

        // skip_checks=true with name miss: Ok(None), no error.
        assert_eq!(
            KeyDef::extract_ttl_col("ttl_col=other", None, &ts, true).expect("ok"),
            None
        );
    }

    #[test]
    fn ttl_col_resolves_index_within_multi_field_table() {
        let ts = build_table_share(vec![
            ttl_col_field("a"),
            ttl_col_field("b"),
            ttl_col_field("c"),
        ]);
        let got = KeyDef::extract_ttl_col("ttl_col=c", None, &ts, false)
            .expect("ok")
            .expect("present");
        assert_eq!(got.field_index, 2);
    }

    // ----- report_checksum_mismatch (smoke tests — logging is a side effect) -----

    #[test]
    fn report_checksum_mismatch_handles_both_kinds_without_panicking() {
        let kd = forward_pk(0xabcd_1234);
        kd.report_checksum_mismatch(true, b"some_packed_key_bytes");
        kd.report_checksum_mismatch(false, b"some_unpack_info_bytes");
    }

    #[test]
    fn report_checksum_mismatch_caps_hexdump_for_oversize_payload() {
        let kd = forward_pk(1);
        // Larger than RDB_MAX_HEXDUMP_LEN — hexdump should truncate, not
        // explode memory.
        let big = vec![0xabu8; crate::utils::parse::RDB_MAX_HEXDUMP_LEN * 2];
        kd.report_checksum_mismatch(true, &big);
    }

    #[test]
    fn report_checksum_mismatch_handles_empty_payload() {
        let kd = forward_pk(1);
        kd.report_checksum_mismatch(false, b"");
    }

    #[test]
    fn ttl_col_partition_override_wins() {
        let ts = build_table_share(vec![
            ttl_col_field("table_level"),
            ttl_col_field("p0_level"),
        ]);
        let comment = "ttl_col=table_level;p0_ttl_col=p0_level";
        let got = KeyDef::extract_ttl_col(comment, Some("p0"), &ts, false)
            .expect("ok")
            .expect("present");
        assert_eq!(got.column_name, "p0_level");
        assert_eq!(got.field_index, 1);
    }

    // ===== KeyDef::setup =====
    //
    // These tests build a small TableShareView + TblDef and run setup()
    // against KeyDef skeletons. The point is to verify the
    // walk-and-dispatch logic (pack_info slot count, pk_part_no mapping,
    // SK-extension, hidden-PK handling) — not to exercise the encode
    // path itself.

    use crate::codec::tbl_def::TblDef;
    use crate::codec::value::{
        FieldView, IndexKeyPartView, IndexSchemaView, MysqlType, TableShareView,
    };
    use std::sync::Arc;

    fn long_field(name: &str) -> FieldView {
        FieldView {
            name: name.into(),
            mysql_type: MysqlType::Long,
            pack_length: 4,
            output_offset: 0,
            null_marker: None,
            length: 4,
            charset_id: 63,
            flags: 0,
            decimals: 0,
        }
    }

    fn nullable_long_field(name: &str) -> FieldView {
        let mut f = long_field(name);
        f.null_marker = Some((0, 1));
        f
    }

    fn kp(field_idx: u32) -> IndexKeyPartView {
        IndexKeyPartView {
            field_idx,
            key_part_length: 0,
        }
    }

    fn idx(parts: Vec<IndexKeyPartView>) -> IndexSchemaView {
        IndexSchemaView {
            user_defined_key_parts: parts.len() as u32,
            ext_key_parts: parts.len() as u32,
            key_parts: parts,
        }
    }

    fn idx_extended(user_parts: usize, parts: Vec<IndexKeyPartView>) -> IndexSchemaView {
        IndexSchemaView {
            user_defined_key_parts: user_parts as u32,
            ext_key_parts: parts.len() as u32,
            key_parts: parts,
        }
    }

    fn pk_skel(index_number: u32, keyno: u32) -> KeyDef {
        KeyDef::new_skeleton(
            index_number,
            7,
            keyno,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Primary,
            PRIMARY_FORMAT_VERSION_LATEST,
            false,
            "pk",
        )
    }

    fn sk_skel(index_number: u32, keyno: u32) -> KeyDef {
        KeyDef::new_skeleton(
            index_number,
            7,
            keyno,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Secondary,
            SECONDARY_FORMAT_VERSION_LATEST,
            false,
            "sk",
        )
    }

    fn hidden_pk_skel(index_number: u32, keyno: u32) -> KeyDef {
        KeyDef::new_skeleton(
            index_number,
            7,
            keyno,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::HiddenPrimary,
            PRIMARY_FORMAT_VERSION_LATEST,
            false,
            "HIDDEN_PK",
        )
    }

    fn tbl_def(keys: Vec<Arc<KeyDef>>) -> TblDef {
        TblDef::new("db.t").unwrap().with_keys(keys)
    }

    #[test]
    fn setup_single_part_pk_on_int_column() {
        let table = TableShareView {
            fields: vec![long_field("id"), long_field("v")],
            null_bytes: 0,
            row_length: 8,
            hidden_pk_field: None,
            indexes: vec![idx(vec![kp(0)])],
            primary_key_index: Some(0),
        };
        let pk_arc = Arc::new(pk_skel(100, 0));
        let tdef = tbl_def(vec![pk_arc]);

        let mut pk = pk_skel(100, 0);
        pk.setup(&table, &tdef).expect("setup");

        assert_eq!(pk.key_parts, 1);
        assert_eq!(pk.pack_info.len(), 1);
        assert!(pk.pack_info[0].pack_func.is_none()); // pack deferred (cxx)
        assert!(pk.pack_info[0].unpack_func.is_some());
        assert_eq!(pk.pk_part_no.len(), 0); // PKs don't populate pk_part_no
        // maxlength = INDEX_NUMBER_SIZE (4) + int max_image_len (4) = 8.
        assert_eq!(pk.maxlength, 8);
    }

    #[test]
    fn setup_multi_part_pk_accumulates_maxlength() {
        // Two-part PK on (LONG, LONG). Each part max_image_len=4.
        let table = TableShareView {
            fields: vec![long_field("a"), long_field("b"), long_field("v")],
            null_bytes: 0,
            row_length: 12,
            hidden_pk_field: None,
            indexes: vec![idx(vec![kp(0), kp(1)])],
            primary_key_index: Some(0),
        };
        let tdef = tbl_def(vec![Arc::new(pk_skel(100, 0))]);

        let mut pk = pk_skel(100, 0);
        pk.setup(&table, &tdef).expect("setup");

        assert_eq!(pk.key_parts, 2);
        // INDEX_NUMBER_SIZE (4) + 4 + 4 = 12.
        assert_eq!(pk.maxlength, 12);
    }

    #[test]
    fn setup_nullable_column_adds_one_byte_to_maxlength() {
        let table = TableShareView {
            fields: vec![nullable_long_field("a")],
            null_bytes: 1,
            row_length: 5,
            hidden_pk_field: None,
            indexes: vec![idx(vec![kp(0)])],
            primary_key_index: Some(0),
        };
        let tdef = tbl_def(vec![Arc::new(pk_skel(100, 0))]);

        let mut pk = pk_skel(100, 0);
        pk.setup(&table, &tdef).expect("setup");

        // 4 (INDEX_NUMBER) + 1 (NULL byte) + 4 (LONG) = 9.
        assert_eq!(pk.maxlength, 9);
    }

    #[test]
    fn setup_sk_appends_pk_columns_as_extension() {
        // Table: (id LONG, name LONG, email LONG)
        // PK on id, SK on name (with id appended via extkey).
        let table = TableShareView {
            fields: vec![long_field("id"), long_field("name"), long_field("email")],
            null_bytes: 0,
            row_length: 12,
            hidden_pk_field: None,
            indexes: vec![
                idx(vec![kp(0)]),                                  // PK = (id)
                idx_extended(1, vec![kp(1), kp(0)]),               // SK = (name) extended by (id)
            ],
            primary_key_index: Some(0),
        };
        let tdef = tbl_def(vec![Arc::new(pk_skel(100, 0)), Arc::new(sk_skel(101, 1))]);

        let mut sk = sk_skel(101, 1);
        sk.setup(&table, &tdef).expect("setup");

        assert_eq!(sk.key_parts, 2, "SK has its own col + the PK col");
        assert_eq!(sk.pk_key_parts, 1);
        // pk_part_no: SK col 'name' is not in PK ⇒ None; SK col 'id'
        // (the extended part) matches PK col #0 ⇒ Some(0).
        assert_eq!(sk.pk_part_no, vec![None, Some(0)]);
    }

    #[test]
    fn setup_sk_dedup_drops_pk_column_that_was_already_in_sk() {
        // Table: (id LONG, name LONG)
        // PK on id, SK on (id, name) — id is already part of the SK so
        // the extkey-extension tail must NOT re-append it.
        //
        // We simulate this by giving the SK an ext_key_parts that says
        // "the SQL layer extended me by appending id (which I already
        // have)". After dedup, the SK should still have just 2 parts.
        let table = TableShareView {
            fields: vec![long_field("id"), long_field("name")],
            null_bytes: 0,
            row_length: 8,
            hidden_pk_field: None,
            indexes: vec![
                idx(vec![kp(0)]),                                  // PK = (id)
                // SK declared = (id, name); extended would attempt to
                // append (id) again. Without ext_key_parts coverage of
                // the dedup case we have to manually construct what the
                // SQL layer would have produced: user_defined=2,
                // ext=2 (already includes the PK col).
                idx(vec![kp(0), kp(1)]),
            ],
            primary_key_index: Some(0),
        };
        let tdef = tbl_def(vec![Arc::new(pk_skel(100, 0)), Arc::new(sk_skel(101, 1))]);

        let mut sk = sk_skel(101, 1);
        sk.setup(&table, &tdef).expect("setup");

        // Total parts: 2 user-declared + pk_key_parts=1 attempted
        // append; one is deduped ⇒ 2 final parts.
        assert_eq!(sk.key_parts, 2);
        // pk_part_no[0]: SK col 'id' matches PK col #0; SK col 'name'
        // doesn't match PK.
        assert_eq!(sk.pk_part_no, vec![Some(0), None]);
    }

    #[test]
    fn setup_hidden_pk_index_makes_one_synthetic_keypart() {
        let table = TableShareView {
            fields: vec![long_field("v")],
            null_bytes: 0,
            row_length: 4,
            hidden_pk_field: Some(0),
            // Hidden PK has no entry in tbl.indexes (matches the C++).
            indexes: vec![],
            primary_key_index: None,
        };
        let tdef = tbl_def(vec![Arc::new(hidden_pk_skel(200, 0))]);

        let mut hpk = hidden_pk_skel(200, 0);
        hpk.setup(&table, &tdef).expect("setup");

        assert_eq!(hpk.key_parts, 1);
        assert_eq!(hpk.pack_info.len(), 1);
        // Hidden-PK column is the 8-byte synthetic rowid.
        assert_eq!(hpk.pack_info[0].max_image_len, 8);
        // INDEX_NUMBER_SIZE (4) + 8 = 12.
        assert_eq!(hpk.maxlength, 12);
    }

    #[test]
    fn setup_sk_with_hidden_pk_extension_appends_synthetic_rowid() {
        // Table with NO declared PK ⇒ hidden_pk_field is set. The SK
        // gets the hidden-PK rowid appended as its single extension
        // part.
        let table = TableShareView {
            fields: vec![long_field("v")],
            null_bytes: 0,
            row_length: 4,
            hidden_pk_field: Some(0),
            indexes: vec![
                // SK on v. The hidden PK is *appended* by setup's
                // simulating_extkey path, so we set ext_key_parts to
                // just the user-declared count.
                idx(vec![kp(0)]),
            ],
            primary_key_index: None,
        };
        // tbl_def includes both the SK and the hidden PK at the end
        // (matches the C++: hidden PK is the last entry in m_key_descr_arr).
        let tdef = tbl_def(vec![
            Arc::new(sk_skel(101, 0)),
            Arc::new(hidden_pk_skel(200, 1)),
        ]);

        let mut sk = sk_skel(101, 0);
        sk.setup(&table, &tdef).expect("setup");

        // 1 user-declared part + 1 synthetic hidden-PK rowid.
        assert_eq!(sk.key_parts, 2);
        assert_eq!(sk.pk_key_parts, 1);
        // Synthetic tail's pk_part_no is Some(0) per C++ semantics.
        assert_eq!(sk.pk_part_no, vec![None, Some(0)]);
        // Last part should be the 8-byte synthetic rowid.
        assert_eq!(sk.pack_info[1].max_image_len, 8);
    }

    #[test]
    fn setup_is_idempotent_via_maxlength_shortcircuit() {
        let table = TableShareView {
            fields: vec![long_field("id")],
            null_bytes: 0,
            row_length: 4,
            hidden_pk_field: None,
            indexes: vec![idx(vec![kp(0)])],
            primary_key_index: Some(0),
        };
        let tdef = tbl_def(vec![Arc::new(pk_skel(100, 0))]);

        let mut pk = pk_skel(100, 0);
        pk.setup(&table, &tdef).expect("first setup");
        let first_maxlength = pk.maxlength;
        let first_key_parts = pk.key_parts;

        // Mutate post-setup, then re-call: short-circuit must leave the
        // post-setup state alone.
        pk.setup(&table, &tdef).expect("second setup (no-op)");
        assert_eq!(pk.maxlength, first_maxlength);
        assert_eq!(pk.key_parts, first_key_parts);
    }

    #[test]
    fn setup_rejects_out_of_range_keyno() {
        let table = TableShareView {
            fields: vec![long_field("id")],
            null_bytes: 0,
            row_length: 4,
            hidden_pk_field: None,
            indexes: vec![idx(vec![kp(0)])],
            primary_key_index: Some(0),
        };
        let tdef = tbl_def(vec![Arc::new(pk_skel(100, 0))]);

        // keyno=99 points past the only index.
        let mut bad = pk_skel(100, 99);
        let err = bad.setup(&table, &tdef).unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
    }

    #[test]
    fn setup_rejects_out_of_range_field_idx() {
        let table = TableShareView {
            fields: vec![long_field("id")],
            null_bytes: 0,
            row_length: 4,
            hidden_pk_field: None,
            // keypart references field 42 which doesn't exist.
            indexes: vec![idx(vec![kp(42)])],
            primary_key_index: Some(0),
        };
        let tdef = tbl_def(vec![Arc::new(pk_skel(100, 0))]);

        let mut pk = pk_skel(100, 0);
        let err = pk.setup(&table, &tdef).unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
    }
}
