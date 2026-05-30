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

    #[test]
    fn table_has_hidden_pk_reads_the_field_slot() {
        use crate::codec::value::TableShareView;
        let with_hidden = TableShareView {
            fields: Vec::new(),
            null_bytes: 0,
            row_length: 0,
            hidden_pk_field: Some(0),
        };
        let without = TableShareView {
            fields: Vec::new(),
            null_bytes: 0,
            row_length: 0,
            hidden_pk_field: None,
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
}
