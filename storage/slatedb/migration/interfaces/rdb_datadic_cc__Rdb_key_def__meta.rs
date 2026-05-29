//! Interface stub for `Rdb_key_def__meta`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.cc` (lines 293..1209 + 1517..1796 + 3647..3684)
//! C++ header: `storage/rocksdb/rdb_datadic.h` (Rdb_key_def meta side)
//! v4 manifest sub-unit: `Rdb_key_def__meta`
//! Parent unit: `rdb_datadic.cc`
//! Approx body LoC accounted for here: ~700 (ctor/dtor + setup + gen_/extract_ + meta queries)
//!
//! ## Mapping
//! Per _DESIGN.md §2: the `Rdb_key_def` struct itself is preserved; its
//! identity is `GL_INDEX_ID { cf_id, index_id }` (see `rdb_global_h.rs`).
//! Lifecycle methods (`setup`, `successor`, `predecessor`, `compare_keys`,
//! `key_length`) translate 1:1.
//!
//! **TTL handling is rerouted per _DESIGN.md §3.** MyRocks' `extract_ttl_*`
//! routines split into two concerns:
//! 1. **Parsing the CREATE TABLE comment** to discover that an index *has*
//!    TTL and which column carries it. This remains here (`extract_ttl_duration`,
//!    `extract_ttl_col`) and is pure string parsing.
//! 2. **Reading the TTL value from a row.** In MyRocks this read the 8-byte
//!    TTL prefix from the value blob. We now read `RowEntry.expire_ts`
//!    instead — see `extract_ttl_from_row_entry` below, which wraps the
//!    SlateDB-native field.
//!
//! `compare_keys` is retained for in-process comparisons (e.g., WriteBatch
//! pre-commit ordering check) but is NOT registered with SlateDB — the
//! engine compares bytes itself (per `rdb_comparator_h.rs`).
//!
//! ## Out-of-scope methods
//! - None as a hard non-goal; however `get_lookup_bitmap` / `covers_lookup`
//!   are PERFORMANCE-PATH optimizations that we will implement as wrappers
//!   around SlateDB's `PrefixExtractor` + `Settings::min_filter_keys`
//!   (per _DESIGN.md §0). The MyRocks bitmap concept survives.

use bytes::Bytes;
use slatedb::Error;

use crate::rdb_buff_h::StringWriter;
use crate::rdb_comparator_h::KeyDirection;
use crate::rdb_global_h::GlIndexId;

/// MyRocks index types (PRIMARY / SECONDARY / HIDDEN_PRIMARY).
/// C++: rdb_datadic.h enum `INDEX_TYPE_*`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexType {
    Primary = 1,
    Secondary = 2,
    HiddenPrimary = 3,
}

/// MyRocks per-index flags packed into a u32 bitmap.
/// C++: rdb_datadic.h enum `INDEX_FLAG`.
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum IndexFlag {
    Ttl = 1 << 0,
    Max = 1 << 1,
}

/// `Rdb_key_def` — per-index codec/meta descriptor.
///
/// In MyRocks this owns a `rocksdb::ColumnFamilyHandle*` and a sidechannel
/// per-CF comparator. In SlateDB we instead carry the `GL_INDEX_ID` and a
/// `KeyDirection`; CF and comparator concepts collapse into key-prefix +
/// byte-comparison (per _DESIGN.md §2 / `rdb_comparator_h.rs`).
///
/// C++: rdb_datadic.h class `Rdb_key_def`.
pub struct KeyDef {
    pub gl_index_id: GlIndexId,
    pub index_type: IndexType,
    pub index_dict_version: u16,
    pub kv_format_version: u16,
    pub direction: KeyDirection,
    pub is_per_partition_cf: bool,
    pub name: String,
    pub index_flags_bitmap: u32,
    pub ttl_rec_offset: u32,
    /// TTL duration in seconds. 0 = no TTL. Maps to `Ttl::ExpireAfter` at
    /// put time (per _DESIGN.md §3 / open question 3).
    pub ttl_duration: u64,
    /// Optional name of the column carrying TTL (for "TTL_COL" tables).
    pub ttl_column: String,
    pub key_parts: u32,
    pub pk_key_parts: u32,
    pub ttl_pk_key_part_offset: u32,
    pub ttl_field_index: u32,
    pub max_length: usize,
    pub stats: crate::rdb_global_h::GlobalStats, // placeholder; real Rdb_index_stats lives elsewhere
    // TODO(human): m_pack_info, m_pk_part_no allocations.
}

impl KeyDef {
    /// Construct a fresh key def. Replaces the C++ ctor at rdb_datadic.cc:293.
    pub fn new(
        _gl_index_id: GlIndexId,
        _index_type: IndexType,
        _kv_format_version: u16,
        _direction: KeyDirection,
        _name: String,
    ) -> Self {
        todo!("port C++ ctor at rdb_datadic.cc:293")
    }

    /// One-shot lazy setup. Computes `key_parts`, allocates per-field
    /// packing descriptors, derives `max_length`. Idempotent — guarded by an
    /// internal mutex in MyRocks; in Rust we use `OnceCell` or `RwLock`.
    ///
    /// Errors: `slatedb::Error::invalid` on schema validation failure
    /// (matches MyRocks `HA_EXIT_FAILURE` path).
    ///
    /// C++: rdb_datadic.cc:388.
    pub fn setup(
        &mut self,
        _table: &(),       // TODO(human): bridge to MariaDB TABLE
        _tbl_def: &(),     // TODO(human): bridge to Rdb_tbl_def
    ) -> Result<(), Error> {
        todo!("port C++ setup at rdb_datadic.cc:388")
    }

    /// Memcomparable "next-key" — append `0x00` and clamp at max length. Used
    /// to convert a key prefix to a strict upper-bound for `scan(range)`.
    ///
    /// C++: rdb_datadic.cc:1054.
    pub fn successor(_packed: &mut Vec<u8>) {
        todo!("port C++ successor at rdb_datadic.cc:1054")
    }

    /// Memcomparable "prev-key". Decrement the trailing byte with borrow.
    ///
    /// C++: rdb_datadic.cc:1073.
    pub fn predecessor(_packed: &mut Vec<u8>) {
        todo!("port C++ predecessor at rdb_datadic.cc:1073")
    }

    /// Byte-comparison of two packed keys. Used only for in-process checks
    /// (WriteBatch pre-commit). SlateDB does its own comparison at runtime.
    ///
    /// C++: rdb_datadic.cc:1517.
    pub fn compare_keys(_a: &[u8], _b: &[u8]) -> std::cmp::Ordering {
        todo!("port C++ compare_keys at rdb_datadic.cc:1517 — wraps memcmp")
    }

    /// Max packed length of a row's key for this index.
    /// C++: rdb_datadic.cc:1591.
    pub fn key_length(&self, _table: &()) -> usize {
        todo!("port C++ key_length at rdb_datadic.cc:1591")
    }

    /// Static: parse the table comment to discover the TTL duration. Pure
    /// string parsing — translates verbatim.
    ///
    /// C++: rdb_datadic.cc:607.
    pub fn extract_ttl_duration(
        _table_comment: &str,
        _partition: Option<&str>,
    ) -> Result<u64, Error> {
        todo!("port C++ extract_ttl_duration at rdb_datadic.cc:607")
    }

    /// Static: parse the table comment to discover the TTL column name.
    /// C++: rdb_datadic.cc:648.
    pub fn extract_ttl_col(
        _table_comment: &str,
        _partition: Option<&str>,
        _skip_checks: bool,
    ) -> Result<(String, u32), Error> {
        todo!("port C++ extract_ttl_col at rdb_datadic.cc:648 — returns (column_name, field_index)")
    }

    /// **Replaces MyRocks "read TTL prefix from value bytes" path.**
    /// Per _DESIGN.md §3, TTL lives in `RowEntry.expire_ts` natively.
    /// This wrapper translates the SlateDB-native field to the seconds-
    /// since-epoch convention the upper layer expects.
    pub fn extract_ttl_from_row_entry(
        expire_ts: Option<i64>,
    ) -> Option<u64> {
        expire_ts.map(|ts| ts.max(0) as u64)
    }

    /// Static: synthesize a CREATE TABLE qualifier string.
    /// C++: rdb_datadic.cc:700.
    pub fn gen_qualifier_for_table(_qualifier: &str, _partition: Option<&str>) -> String {
        todo!("port C++ gen_qualifier_for_table at rdb_datadic.cc:700")
    }

    pub fn gen_cf_name_qualifier_for_partition(prefix: &str) -> String {
        let _ = prefix; todo!("rdb_datadic.cc:729")
    }
    pub fn gen_ttl_duration_qualifier_for_partition(prefix: &str) -> String {
        let _ = prefix; todo!("rdb_datadic.cc:737")
    }
    pub fn gen_ttl_col_qualifier_for_partition(prefix: &str) -> String {
        let _ = prefix; todo!("rdb_datadic.cc:745")
    }
    pub fn parse_comment_for_qualifier(
        _comment: &str, _partition: Option<&str>, _qualifier: &str,
    ) -> (String, bool) {
        todo!("rdb_datadic.cc:753 — returns (value, per_part_match_found)")
    }

    /// Look up the PK tuple from a secondary-key tuple, by extracting the
    /// PK columns from the SK suffix (extended-keys feature).
    /// C++: rdb_datadic.cc:899.
    pub fn get_primary_key_tuple(
        &self, _table: &(), _pk_descr: &KeyDef, _sk_key: &[u8], _out_pk: &mut Vec<u8>,
    ) -> Result<usize, Error> { todo!("rdb_datadic.cc:899") }

    /// Decompose a packed SK key into its memcmp parts.
    /// C++: rdb_datadic.cc:959.
    pub fn get_memcmp_sk_parts(
        &self, _table: &(), _key: &[u8], _out_parts: &mut [&mut [u8]],
    ) -> Result<usize, Error> { todo!("rdb_datadic.cc:959") }

    /// Read a single memcmp key part from a slice, advancing the reader.
    /// Returns -1 for NULL, 0 on success, error on corrupt input.
    /// C++: rdb_datadic.cc:843.
    pub fn read_memcmp_key_part(
        &self, _table: &(), _reader: &mut crate::rdb_buff_h::StringReader, _part_num: u32,
    ) -> Result<i32, Error> { todo!("rdb_datadic.cc:843") }

    /// Lookup-bitmap bookkeeping for covered-index queries.
    /// Maps to SlateDB `PrefixExtractor` (per _DESIGN.md §2) at scan time.
    /// C++: rdb_datadic.cc:1107.
    pub fn get_lookup_bitmap(&self, _table: &(), _out_map: &mut Vec<u8>) {
        todo!("rdb_datadic.cc:1107")
    }

    /// Whether this index can satisfy a covered lookup for `unpack_info`.
    /// C++: rdb_datadic.cc:1173.
    pub fn covers_lookup(&self, _unpack_info: &[u8], _lookup_bitmap: &[u8]) -> bool {
        todo!("rdb_datadic.cc:1173")
    }

    pub fn can_cover_lookup(&self) -> bool { todo!("rdb_datadic.cc:1203") }

    pub fn unpack_info_has_checksum(_unpack_info: &[u8]) -> bool {
        todo!("rdb_datadic.cc:1032")
    }

    pub fn get_unpack_header_size(_tag: u8) -> usize {
        todo!("rdb_datadic.cc:1096")
    }

    pub fn table_has_hidden_pk(_table: &()) -> bool {
        todo!("rdb_datadic.cc:1727")
    }

    pub fn report_checksum_mismatch(&self, _is_key: bool, _data: &[u8], _expected: &[u8]) {
        todo!("rdb_datadic.cc:1731")
    }

    pub fn index_format_min_check(&self, _pk_min: i32, _sk_min: i32) -> bool {
        todo!("rdb_datadic.cc:1746")
    }

    // --- index-flags bookkeeping (static helpers) ---

    pub fn has_index_flag(index_flags: u32, flag: IndexFlag) -> bool {
        (flag as u32) & index_flags != 0
    }

    /// Returns (offset, length) of `flag` within the flags region of a value.
    /// C++: rdb_datadic.cc:3651.
    pub fn calculate_index_flag_offset(_index_flags: u32, _flag: IndexFlag) -> (u32, u32) {
        todo!("rdb_datadic.cc:3651")
    }

    /// C++: rdb_datadic.cc:3677.
    pub fn write_index_flag_field(&self, _buf: &mut StringWriter, _val: &[u8], _flag: IndexFlag) {
        todo!("rdb_datadic.cc:3677")
    }

    /// Convenience: convert the canonical key-prefix bytes for this index.
    /// Format per _DESIGN.md §2: `varint(cf_id) || u32_be(index_id)`.
    pub fn prefix_bytes(&self) -> Bytes {
        // TODO(human): emit varint(cf_id) followed by u32_be(index_id)
        todo!("emit prefix per _DESIGN.md §2")
    }
}
