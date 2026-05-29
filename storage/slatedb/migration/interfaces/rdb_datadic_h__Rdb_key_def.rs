//! Interface stub for `rdb_datadic_h__Rdb_key_def`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (lines 251..875)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_key_def`
//! Parent unit: `rdb_datadic_h`
//! Approx body LoC accounted for here: 625
//!
//! ## Mapping
//! In-scope codec types — preserved from MyRocks per _DESIGN.md §2/§3.
//! `Rdb_key_def` is the central per-index descriptor: it owns the index id,
//! the prefix bytes used to scope SlateDB scans to this index, the array of
//! `Rdb_field_packing` per key-part, the reverse-CF flag, TTL metadata, and
//! all the format-version constants.
//!
//! **Companion stubs already exist** and host the bulk of the encode/decode
//! routines (~2400 LoC of C++):
//! - `Rdb_key_def__encode` — `pack_record`, `pack_field`, `pack_index_tuple`,
//!   `pack_hidden_pk`, the static `pack_with_*` helpers, `write_index_flag_field`.
//! - `Rdb_key_def__decode` — `unpack_record`, every `unpack_*` static helper,
//!   every `skip_*` static helper, every `make_unpack_*` static helper, the
//!   `calc_unpack_*` length helpers, `read_memcmp_key_part`.
//!
//! **This stub holds the META / STATE half** the header file declares:
//! - constructor / destructor / member state (`m_index_number`, `m_cf_handle`,
//!   `m_is_reverse_cf`, `m_ttl_*`, `m_pack_info`, …).
//! - inline accessors (`get_index_number`, `get_keyno`, `get_name`, `get_cf`,
//!   `get_key_parts`, `get_ttl_field_index`, `get_extractor`, `max_storage_fmt_length`,
//!   `get_gl_index_id`, `has_ttl`).
//! - inline infimum/supremum/first/last key helpers (covered_key range
//!   computation; these stay byte-identical to MyRocks).
//! - the `setup()` driver that initializes the `Rdb_field_packing` array.
//! - the format-version / index-type / data-dict-type enums (translated to
//!   `#[repr]` enums and `pub const`s).
//! - misc utility methods that are neither encode nor decode (`successor`,
//!   `predecessor`, `cmp_full_keys`, `covers_key`, `covers_lookup`,
//!   `get_primary_key_tuple`, `get_memcmp_sk_parts`, `compare_keys`,
//!   `key_length`, `get_lookup_bitmap`, `value_matches_prefix`,
//!   `unpack_info_has_checksum`, `index_format_min_check`,
//!   `report_checksum_mismatch`, `table_has_hidden_pk`, `extract_ttl_duration`,
//!   `extract_ttl_col`, `has_index_flag`, `calculate_index_flag_offset`,
//!   `gen_*` qualifier helpers, `parse_comment_for_qualifier`,
//!   `get_unpack_header_size`, `get_table_field_for_part_no`, `can_unpack`,
//!   `has_unpack_info`, `use_covered_bitmap_format`, `can_cover_lookup`,
//!   `use_legacy_varbinary_format`, `is_unpack_data_tag`).
//!
//! **Per _DESIGN.md §2:** the `m_cf_handle` (`rocksdb::ColumnFamilyHandle*`)
//! field maps to a `cf_id: u32` here — SlateDB has no per-CF handle; we
//! identify the logical CF by id and prepend `varint(cf_id) || u32_be(index_id)`
//! to every encoded key. The `get_extractor()` accessor returns our
//! `PrefixExtractor` impl instead of a `rocksdb::SliceTransform`.
//!
//! **Per _DESIGN.md §3:** TTL is set via `PutOptions::ttl` on writes; the
//! `m_ttl_duration` / `m_ttl_column` / `m_ttl_field_index` metadata is still
//! needed to compute the expiry timestamp at write time (we derive
//! `Ttl::ExpireAt` from `now + m_ttl_duration` or from the TTL column value).
//! No TTL bytes are written into the row blob.
//!
//! ## Out-of-scope methods
//! - `report_checksum_mismatch` is in scope but degrades to a `log::error!`
//!   in Rust (no MyRocks `print_keydup_error`-style ERROR_LOG plumbing yet).
//! - `compare_keys` is exposed but normally unused — SlateDB does the sort
//!   itself per _DESIGN.md §1 ("Column families" row). Kept for tests.
//! - `Rdb_index_stats` is forwarded to the `properties_collector_cc` unit
//!   (it's a value-collector struct, not codec-side metadata).

use slatedb::Error;

use crate::rdb_buff_h::{StringReader, StringWriter};
use crate::rdb_comparator_h::KeyDirection;
use crate::rdb_datadic_h__Rdb_convert_to_record_key_decoder::{FieldView, TableShareView};
use crate::rdb_datadic_h__Rdb_field_packing::FieldPacking;
use crate::rdb_global_h::GlIndexId;

// ---------- enum constants from rdb_datadic.h:468..594 ----------

/// Layout-related size constants. Mirrors C++ anonymous enum at rdb_datadic.h:468.
pub const INDEX_NUMBER_SIZE: usize = 4;
pub const VERSION_SIZE: usize = 2;
pub const CF_NUMBER_SIZE: usize = 4;
pub const CF_FLAG_SIZE: usize = 4;
pub const PACKED_SIZE: usize = 4;

/// CF bit-flags persisted in the data dictionary (rdb_datadic.h:477).
pub const REVERSE_CF_FLAG: u32 = 1;
/// Deprecated; kept to ignore the bit on load.
pub const AUTO_CF_FLAG: u32 = 2;
pub const PER_PARTITION_CF_FLAG: u32 = 4;
pub const CF_FLAGS_TO_IGNORE: u32 = PER_PARTITION_CF_FLAG;

/// Index-flag bits stored in the on-record header (rdb_datadic.h:485).
/// Currently only TTL_FLAG is used; MAX_FLAG marks "actual record starts here".
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexFlag {
    TtlFlag = 1 << 0,
    MaxFlag = 1 << 1,
}

/// Data-dictionary record-type tags (rdb_datadic.h:498). One per kind of
/// metadata entry that lives in the system-CF prefix.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataDictType {
    DdlEntryIndexStartNumber = 1,
    IndexInfo = 2,
    CfDefinition = 3,
    BinlogInfoIndexNumber = 4,
    DdlDropIndexOngoing = 5,
    IndexStatistics = 6,
    MaxIndexId = 7,
    DdlCreateIndexOngoing = 8,
    AutoInc = 9,
    // 10..12 are reserved upstream (MariaDB).
    TableVersion = 20,
    EndDictIndexId = 255,
}

/// Schema versions for each data-dict entry shape (rdb_datadic.h:515).
pub const DDL_ENTRY_INDEX_VERSION: u16 = 1;
pub const CF_DEFINITION_VERSION: u16 = 1;
pub const BINLOG_INFO_INDEX_NUMBER_VERSION: u16 = 1;
pub const DDL_DROP_INDEX_ONGOING_VERSION: u16 = 1;
pub const MAX_INDEX_ID_VERSION: u16 = 1;
pub const DDL_CREATE_INDEX_ONGOING_VERSION: u16 = 1;
pub const AUTO_INCREMENT_VERSION: u16 = 1;

/// Index-info schema versions (rdb_datadic.h:529).
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

/// MyRocks index-type tag (rdb_datadic.h:549).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexType {
    Primary = 1,
    Secondary = 2,
    HiddenPrimary = 3,
}

/// PK key/value format versions (rdb_datadic.h:556).
pub const PRIMARY_FORMAT_VERSION_INITIAL: u16 = 10;
pub const PRIMARY_FORMAT_VERSION_UPDATE1: u16 = 11;
pub const PRIMARY_FORMAT_VERSION_UPDATE2: u16 = 12;
pub const PRIMARY_FORMAT_VERSION_TTL: u16 = 13;
pub const PRIMARY_FORMAT_VERSION_LATEST: u16 = PRIMARY_FORMAT_VERSION_TTL;

/// SK key/value format versions (rdb_datadic.h:576).
pub const SECONDARY_FORMAT_VERSION_INITIAL: u16 = 10;
pub const SECONDARY_FORMAT_VERSION_UPDATE1: u16 = 11;
pub const SECONDARY_FORMAT_VERSION_UPDATE2: u16 = 12;
pub const SECONDARY_FORMAT_VERSION_TTL: u16 = 13;
pub const SECONDARY_FORMAT_VERSION_LATEST: u16 = SECONDARY_FORMAT_VERSION_TTL;
pub const SECONDARY_FORMAT_VERSION_UPDATE3: u16 = 65535;

// ---------- forwards ----------

/// Forward to properties_collector unit's per-index stats type.
pub struct IndexStats; // forwarded — defined in properties_collector_cc.rs

/// Forward to the comparator unit's prefix-extractor handle.
pub struct PrefixExtractor; // TODO(human): wire to codec::prefix module

/// Forward — see `Rdb_tbl_def` stub.
pub struct TblDef; // see rdb_datadic_h__Rdb_tbl_def

// ---------- the main struct ----------

/// Per-index descriptor. One instance per logical index, shared between every
/// `handler` (`TABLE*`) that opens the table. Long-lived; reference-counted via
/// `Arc` at the `Rdb_tbl_def` level.
///
/// The encode/decode hot paths are in the companion `Rdb_key_def__encode` /
/// `Rdb_key_def__decode` units; this struct's methods focus on state /
/// metadata access and key-range geometry.
///
/// Original: rdb_datadic.h:251 — `class Rdb_key_def`.
pub struct KeyDef {
    // ---- public C++ fields (rdb_datadic.h:798..833) ----
    pub index_dict_version: u16,
    pub index_type: IndexType,
    pub kv_format_version: u16,
    /// True if the column family stores data in reverse byte order. See
    /// `KeyDirection::Reverse` (_DESIGN.md §2) — at encode time the
    /// memcomparable bytes are XOR'd with `0xff` so byte-lexicographic order
    /// produces the desired reverse semantic order.
    pub is_reverse_cf: bool,
    pub is_per_partition_cf: bool,
    pub name: String,
    pub stats: IndexStats,
    pub index_flags_bitmap: u32,
    pub total_index_flags_length: u32,
    pub ttl_rec_offset: u32,
    pub ttl_duration: u64,
    pub ttl_column: String,

    // ---- private C++ fields (rdb_datadic.h:781..874) ----
    pub(crate) index_number: u32,
    pub(crate) index_number_storage_form: [u8; INDEX_NUMBER_SIZE],
    /// CF identity. Replaces `rocksdb::ColumnFamilyHandle*` per _DESIGN.md §2.
    pub(crate) cf_id: u32,
    pub(crate) pk_key_parts: u32,
    /// `pk_part_no[X] = Y` ⇒ keypart #X is part #Y of the PK; `None` means
    /// not in PK. Length is `key_parts`.
    pub(crate) pk_part_no: Vec<Option<u32>>,
    pub(crate) pack_info: Vec<FieldPacking>,
    pub(crate) keyno: u32,
    pub(crate) key_parts: u32,
    pub(crate) ttl_pk_key_part_offset: u32,
    pub(crate) ttl_field_index: u32,
    pub(crate) prefix_extractor: Option<std::sync::Arc<PrefixExtractor>>,
    pub(crate) maxlength: u32,
    pub(crate) setup_mutex: std::sync::Mutex<()>,
}

impl KeyDef {
    /// Construct. Long parameter list mirrors the C++ ctor at rdb_datadic.h:459;
    /// the cleanup helpers (cf_handle → cf_id, optional stats) are folded in.
    ///
    /// **Errors:** none — pure assignment. `setup()` does the real work later.
    /// Original: rdb_datadic.h:459 — ctor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        _indexnr_arg: u32,
        _keyno_arg: u32,
        _cf_id_arg: u32,
        _index_dict_version_arg: u16,
        _index_type_arg: IndexType,
        _kv_format_version_arg: u16,
        _is_reverse_cf_arg: bool,
        _is_per_partition_cf: bool,
        _name: &str,
        _stats: IndexStats,
        _index_flags: u32,
        _ttl_rec_offset: u32,
        _ttl_duration: u64,
    ) -> Self {
        todo!("port Rdb_key_def ctor — store index_number_storage_form via store_index")
    }

    /// Copy-construct. Required by C++ semantics; in Rust this would be
    /// `Clone`. Kept as an explicit method so the inevitable refcount/setup
    /// duplication is intentional.
    /// Original: rdb_datadic.h:458.
    pub fn duplicate(&self) -> Self {
        todo!("port Rdb_key_def copy ctor")
    }

    // ---- inline accessors (all trivial) ----

    /// Original: rdb_datadic.h:409.
    pub fn get_keyno(&self) -> u32 { self.keyno }
    /// Original: rdb_datadic.h:411.
    pub fn get_index_number(&self) -> u32 { self.index_number }
    /// Original: rdb_datadic.h:413.
    pub fn get_gl_index_id(&self) -> GlIndexId {
        GlIndexId { cf_id: self.cf_id, index_id: self.index_number }
    }
    /// Original: rdb_datadic.h:431.
    pub fn max_storage_fmt_length(&self) -> u32 { self.maxlength }
    /// Original: rdb_datadic.h:433.
    pub fn get_key_parts(&self) -> u32 { self.key_parts }
    /// Original: rdb_datadic.h:435.
    pub fn get_ttl_field_index(&self) -> u32 { self.ttl_field_index }
    /// Original: rdb_datadic.h:449.
    pub fn get_name(&self) -> &str { &self.name }
    /// Original: rdb_datadic.h:451.
    pub fn get_extractor(&self) -> Option<&std::sync::Arc<PrefixExtractor>> {
        self.prefix_extractor.as_ref()
    }
    /// CF identity. Replaces `get_cf` (rdb_datadic.h:629) — no `ColumnFamilyHandle*`
    /// in SlateDB; callers want the id to build their `key_prefix`.
    pub fn cf_id(&self) -> u32 { self.cf_id }
    /// Original: rdb_datadic.h:605.
    pub fn has_ttl(&self) -> bool { self.ttl_duration > 0 }
    /// Original: rdb_datadic.h:390. True for SK with the new covering format.
    pub fn use_covered_bitmap_format(&self) -> bool {
        self.index_type == IndexType::Secondary
            && self.kv_format_version >= SECONDARY_FORMAT_VERSION_UPDATE3
    }
    /// True iff the index's stored bytes use the older binary varlen format.
    /// Original: rdb_datadic.h:760.
    pub fn use_legacy_varbinary_format(&self) -> bool {
        !self.index_format_min_check(PRIMARY_FORMAT_VERSION_UPDATE2,
                                     SECONDARY_FORMAT_VERSION_UPDATE2)
    }

    /// Derive the index's `KeyDirection` for the codec layer.
    pub fn direction(&self) -> KeyDirection {
        if self.is_reverse_cf { KeyDirection::Reverse } else { KeyDirection::Forward }
    }

    // ---- infimum / supremum (range bounds for SlateDB scans) ----

    /// Write the index's infimum (`index_number` big-endian, padded to 4 bytes).
    /// Original: rdb_datadic.h:287.
    pub fn get_infimum_key(&self, key: &mut [u8], size: &mut usize) {
        key[..INDEX_NUMBER_SIZE].copy_from_slice(&self.index_number.to_be_bytes());
        *size = INDEX_NUMBER_SIZE;
    }

    /// Write the index's supremum (`index_number + 1` big-endian).
    /// Original: rdb_datadic.h:293.
    pub fn get_supremum_key(&self, key: &mut [u8], size: &mut usize) {
        key[..INDEX_NUMBER_SIZE].copy_from_slice(&(self.index_number + 1).to_be_bytes());
        *size = INDEX_NUMBER_SIZE;
    }

    /// First key for "begin iterating from start of index". Differs from
    /// infimum for reverse-CF indexes (where iteration starts at supremum
    /// physically). Returns the count of bytes usable for bloom-filter prefix
    /// lookup.
    /// Original: rdb_datadic.h:306.
    pub fn get_first_key(&self, key: &mut [u8], size: &mut usize) -> i32 {
        if self.is_reverse_cf {
            self.get_supremum_key(key, size);
            // count of matching leading bytes vs. unmodified index number
            let mut unmodified = [0u8; INDEX_NUMBER_SIZE];
            unmodified.copy_from_slice(&self.index_number.to_be_bytes());
            (0..INDEX_NUMBER_SIZE).take_while(|&i| key[i] == unmodified[i]).count() as i32
        } else {
            self.get_infimum_key(key, size);
            INDEX_NUMBER_SIZE as i32
        }
    }

    /// Last-key counterpart; symmetric with `get_first_key`.
    /// Original: rdb_datadic.h:335.
    pub fn get_last_key(&self, key: &mut [u8], size: &mut usize) -> i32 {
        if self.is_reverse_cf {
            self.get_infimum_key(key, size);
            INDEX_NUMBER_SIZE as i32
        } else {
            self.get_supremum_key(key, size);
            let mut unmodified = [0u8; INDEX_NUMBER_SIZE];
            unmodified.copy_from_slice(&self.index_number.to_be_bytes());
            (0..INDEX_NUMBER_SIZE).take_while(|&i| key[i] == unmodified[i]).count() as i32
        }
    }

    /// Make a key strictly greater than `packed_tuple`. Used to convert a
    /// lookup key into an exclusive upper bound for SlateDB's `scan(range)`.
    /// Returns `HA_EXIT_FAILURE` if all bytes are 0xff (no successor exists).
    /// Original: rdb_datadic.h:357 — `static successor`.
    pub fn successor(packed_tuple: &mut [u8], len: usize) -> i32 {
        let _ = (packed_tuple, len);
        todo!("port Rdb_key_def::successor — increment last non-0xff byte")
    }

    /// Make a key strictly less than `packed_tuple`. Mirror image of `successor`.
    /// Original: rdb_datadic.h:360 — `static predecessor`.
    pub fn predecessor(packed_tuple: &mut [u8], len: usize) -> i32 {
        let _ = (packed_tuple, len);
        todo!("port Rdb_key_def::predecessor — decrement last non-0x00 byte")
    }

    /// Memcmp-compare two keys, treating "X is prefix of Y" as equal.
    /// Original: rdb_datadic.h:368.
    pub fn cmp_full_keys(&self, a: &[u8], b: &[u8]) -> std::cmp::Ordering {
        debug_assert!(self.covers_key(a));
        a[..a.len().min(b.len())].cmp(&b[..a.len().min(b.len())])
    }

    /// True iff `slice` begins with this index's 4-byte index_number.
    /// Original: rdb_datadic.h:375.
    pub fn covers_key(&self, slice: &[u8]) -> bool {
        slice.len() >= INDEX_NUMBER_SIZE
            && slice[..INDEX_NUMBER_SIZE] == self.index_number_storage_form
    }

    /// True iff `value` is from this index AND has `prefix` as a prefix.
    /// Original: rdb_datadic.h:404.
    pub fn value_matches_prefix(&self, value: &[u8], prefix: &[u8]) -> bool {
        self.covers_key(value)
            && self.cmp_full_keys(value, prefix) == std::cmp::Ordering::Equal
    }

    // ---- bitmap helpers for covering-lookup ----

    /// Populate `map` with the bitmap of columns this index can serve from
    /// the mem-comparable image alone (covering-lookup eligibility).
    /// Original: rdb_datadic.h:385.
    pub fn get_lookup_bitmap(&self, _table: &TableShareView, _map: &mut Vec<u8>) {
        todo!("port get_lookup_bitmap from rdb_datadic.cc")
    }

    /// True iff this index's `unpack_info` carries enough data to satisfy
    /// every column in `map`.
    /// Original: rdb_datadic.h:387.
    pub fn covers_lookup(&self, _unpack_info: &[u8], _map: &[u8]) -> bool {
        todo!("port covers_lookup — checks the covered-bitmap header")
    }

    /// True iff every key part can be unpacked (≡ index-only reads always work).
    /// Original: rdb_datadic.h:396.
    pub fn can_cover_lookup(&self) -> bool {
        todo!("scan pack_info: every entry has unpack_func set")
    }

    /// True iff `kp` can be unpacked.
    /// Original: rdb_datadic.h:1060.
    pub fn can_unpack(&self, kp: u32) -> bool {
        debug_assert!(kp < self.key_parts);
        self.pack_info[kp as usize].unpack_func.is_some()
    }

    /// True iff `kp` needs unpack_info bytes to decode.
    /// Original: rdb_datadic.h:1065.
    pub fn has_unpack_info(&self, kp: u32) -> bool {
        debug_assert!(kp < self.key_parts);
        self.pack_info[kp as usize].uses_unpack_info()
    }

    /// Look up the table-side `FieldView` for keypart #`part_no`.
    /// Original: rdb_datadic.h:1054.
    pub fn get_table_field_for_part_no<'t>(
        &self,
        _table: &'t TableShareView,
        _part_no: u32,
    ) -> Option<&'t FieldView> {
        todo!("delegate to FieldPacking::get_field_in_table")
    }

    // ---- setup ----

    /// Initialize the per-keypart `FieldPacking` array from `table` / `tbl_def`.
    /// Idempotent; guarded by `setup_mutex`. Returns the number of key parts
    /// configured.
    /// **Errors:** none in the `u32` return; an unsupported column type causes
    /// the index to be rejected by `Rdb_tbl_def::open` (one level up).
    /// Original: rdb_datadic.h:596.
    pub fn setup(&mut self, _table: &TableShareView, _tbl_def: &TblDef) -> u32 {
        todo!("port Rdb_key_def::setup from rdb_datadic.cc")
    }

    // ---- TTL plumbing ----

    /// Parse the table's CREATE TABLE comment for the `ttl_duration` qualifier.
    /// Sets `*ttl_duration` on success.
    /// **Errors:** returns nonzero `HA_ERR_*` on malformed qualifier.
    /// Original: rdb_datadic.h:598.
    pub fn extract_ttl_duration(
        _table_arg: &TableShareView,
        _tbl_def_arg: &TblDef,
        _ttl_duration: &mut u64,
    ) -> u32 {
        todo!("parse ttl_duration qualifier from table comment")
    }

    /// Parse the table's CREATE TABLE comment for the `ttl_col` qualifier.
    /// On success fills `ttl_column` (name) and `ttl_field_index` (offset in
    /// the field list).
    /// Original: rdb_datadic.h:601.
    pub fn extract_ttl_col(
        _table_arg: &TableShareView,
        _tbl_def_arg: &TblDef,
        _ttl_column: &mut String,
        _ttl_field_index: &mut u32,
        _skip_checks: bool,
    ) -> u32 {
        todo!("parse ttl_col qualifier from table comment")
    }

    // ---- index flag bit ops ----

    /// True iff `index_flags` bitmap contains `flag`.
    /// Original: rdb_datadic.h:607.
    pub fn has_index_flag(index_flags: u32, flag: IndexFlag) -> bool {
        (index_flags & (flag as u32)) != 0
    }

    /// Compute the byte offset within the on-record header where `flag`'s
    /// data lives. Optionally returns the field's length.
    /// Original: rdb_datadic.h:608.
    pub fn calculate_index_flag_offset(
        _index_flags: u32,
        _flag: IndexFlag,
        _field_length: Option<&mut u32>,
    ) -> u32 {
        todo!("walk the index_flags bitmap counting bits below `flag`")
    }

    /// Write `val` into the index-flag region of `buf` at the offset computed
    /// by `calculate_index_flag_offset`.
    /// Original: rdb_datadic.h:611.
    pub fn write_index_flag_field(&self, _buf: &mut StringWriter, _val: &[u8], _flag: IndexFlag) {
        todo!("port write_index_flag_field — appears in encode side")
    }

    // ---- comment qualifier construction ----

    /// Build "qualifier_partition" string with the `=` separator scheme.
    /// Original: rdb_datadic.h:615.
    pub fn gen_qualifier_for_table(_qualifier: &str, _partition_name: &str) -> String {
        todo!("port gen_qualifier_for_table — string concatenation only")
    }

    /// Original: rdb_datadic.h:617.
    pub fn gen_cf_name_qualifier_for_partition(s: &str) -> String {
        format!("{}_{}", crate::rdb_global_h::CF_NAME_QUALIFIER, s)
    }
    /// Original: rdb_datadic.h:619.
    pub fn gen_ttl_duration_qualifier_for_partition(s: &str) -> String {
        format!("{}_{}", crate::rdb_global_h::TTL_DURATION_QUALIFIER, s)
    }
    /// Original: rdb_datadic.h:621.
    pub fn gen_ttl_col_qualifier_for_partition(s: &str) -> String {
        format!("{}_{}", crate::rdb_global_h::TTL_COL_QUALIFIER, s)
    }

    /// Parse the table comment for a named qualifier, optionally partition-matched.
    /// Returns the qualifier's value or empty if not present.
    /// Original: rdb_datadic.h:624.
    pub fn parse_comment_for_qualifier(
        _comment: &str,
        _table_arg: &TableShareView,
        _tbl_def_arg: &TblDef,
        _per_part_match_found: &mut bool,
        _qualifier: &str,
    ) -> String {
        todo!("port parse_comment_for_qualifier from rdb_datadic.cc")
    }

    // ---- compare/scan helpers ----

    /// Compare two keys lexicographically, returning the column index where
    /// they first differ. Only used by tests / the in-memory write-batch
    /// pre-commit order check (per _DESIGN.md §1 SlateDB does its own sort).
    /// Original: rdb_datadic.h:281.
    pub fn compare_keys(&self, _key1: &[u8], _key2: &[u8], _column_index: &mut usize) -> i32 {
        todo!("port compare_keys — walks key parts using FieldPacking")
    }

    /// Total length of a packed key (the C++ overload was `key_length(table, slice)`).
    /// Used to validate decoded keys.
    /// Original: rdb_datadic.h:284.
    pub fn key_length(&self, _table: &TableShareView, _key: &[u8]) -> usize {
        todo!("walk pack_info summing max_image_len + null bytes")
    }

    /// Walk PK reader to recover the PK tuple from a secondary-key entry.
    /// Only valid for secondary keys. Returns the PK tuple length in bytes.
    /// Original: rdb_datadic.h:422.
    pub fn get_primary_key_tuple(
        &self,
        _tbl: &TableShareView,
        _pk_descr: &KeyDef,
        _key: &[u8],
        _pk_buffer: &mut [u8],
    ) -> u32 {
        todo!("port get_primary_key_tuple from rdb_datadic.cc")
    }

    /// Extract the mem-comparable SK parts (all but the trailing PK parts).
    /// Returns the length in bytes; sets `n_null_fields` to the count of
    /// nullable key parts that were NULL.
    /// Original: rdb_datadic.h:427.
    pub fn get_memcmp_sk_parts(
        &self,
        _table: &TableShareView,
        _key: &[u8],
        _sk_buffer: &mut [u8],
        _n_null_fields: &mut u32,
    ) -> u32 {
        todo!("port get_memcmp_sk_parts from rdb_datadic.cc")
    }

    /// Read the bytes of keypart #`part_num` from `reader` into nothing
    /// (just advances the cursor; used by upper-bound calculations).
    /// Original: rdb_datadic.h:418.
    pub fn read_memcmp_key_part(
        &self,
        _table_arg: &TableShareView,
        _reader: &mut StringReader,
        _part_num: u32,
    ) -> i32 {
        todo!("delegate to FieldPacking::skip_func")
    }

    // ---- misc ----

    /// True iff the unpack_info blob carries a checksum trailer.
    /// Original: rdb_datadic.h:280.
    pub fn unpack_info_has_checksum(_unpack_info: &[u8]) -> bool {
        todo!("scan unpack_info for the RDB_CHECKSUM_DATA_TAG byte")
    }

    /// True iff this index's PK/SK format version meets the given minimum.
    /// Original: rdb_datadic.h:644.
    pub fn index_format_min_check(&self, pk_min: u16, sk_min: u16) -> bool {
        match self.index_type {
            IndexType::Primary | IndexType::HiddenPrimary => self.kv_format_version >= pk_min,
            IndexType::Secondary => self.kv_format_version >= sk_min,
        }
    }

    /// Original: rdb_datadic.h:765.
    pub fn is_unpack_data_tag(c: u8) -> bool {
        // RDB_UNPACK_DATA_TAG (0x02) / RDB_UNPACK_COVERED_DATA_TAG (0x03)
        c == 0x02 || c == 0x03
    }

    /// Return the on-disk header length for the given unpack_info tag.
    /// Original: rdb_datadic.h:455.
    pub fn get_unpack_header_size(_tag: u8) -> usize {
        todo!("3 for plain UNPACK, 5 for COVERED UNPACK — see rdb_datadic.h:182/192")
    }

    /// True iff `table` has a hidden (auto-generated rowid) PK.
    /// Original: rdb_datadic.h:637.
    pub fn table_has_hidden_pk(_table: &TableShareView) -> bool {
        todo!("table->s->primary_key == MAX_KEY")
    }

    /// Log a checksum mismatch. Per _DESIGN.md notes above, this degrades to
    /// `log::error!` until the full ERROR_LOG plumbing exists.
    /// Original: rdb_datadic.h:639.
    pub fn report_checksum_mismatch(&self, _is_key: bool, _data: &[u8]) {
        todo!("log via tracing::error! with index name + hex dump")
    }
}
