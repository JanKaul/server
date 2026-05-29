//! Interface stub for `ha_rocksdb_h__ha_rocksdb` — **the central handler hub**.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.h` (lines 147..996, 850 LoC body)
//! v4 manifest sub-unit: `ha_rocksdb_h__ha_rocksdb`
//! parent: `ha_rocksdb_h`
//!
//! ## Mapping
//! This is the central `ha_rocksdb : public handler` C++ class declaration —
//! the largest single declaration in MyRocks. It is one struct in C++ but
//! splits into ~22 method buckets in v4 (handler vtable methods + helpers,
//! see `ha_rocksdb_cc__ha_rocksdb__*` sub-units).
//!
//! In Rust we expose:
//! - **`HaSlateDb` struct** — the handler state record. All scratch buffers
//!   and per-handler state declared here.
//! - **`HandlerVtable` trait** — the public MariaDB handler vtable surface.
//!   Implemented by `HaSlateDb`; individual method impls live in the
//!   v4 sub-unit files (one per bucket).
//!
//! Per _DESIGN.md §1, mapping is straightforward:
//! - `rocksdb::Iterator*` → wraps `slatedb::DbIterator` from `Db::scan(...)`.
//! - `rocksdb::Snapshot*` → wraps `Arc<slatedb::DbSnapshot>` from `Db::snapshot()`.
//! - `rocksdb::ColumnFamilyHandle*` → `cf_id: u32` + `PrefixExtractor`.
//! - `Rdb_transaction*` → wraps `slatedb::DbTransaction` (see
//!   `ha_rocksdb_cc__Rdb_transaction.rs`).
//! - `rocksdb::Slice` for borrowed bytes → `&[u8]`; for owned → `bytes::Bytes`.
//! - `uchar*` row buffers → `&[u8]` / `&mut [u8]`.
//!
//! ## Out-of-scope methods
//! - `register_query_cache_table` (ha_rocksdb.h:930) — query cache is a
//!   MariaDB legacy feature; we keep the override returning `FALSE` (not
//!   supported), matching MyRocks.
//! - `MARIAROCKS_NOT_YET` block (ha_rocksdb.h:977-988) — Read-Free
//!   Replication RPL hooks. §1 non-goal. Not exposed.

use bytes::Bytes;
use slatedb::{DbIterator, DbSnapshot, DbTransaction, Error};
use std::sync::Arc;

use crate::ha_rocksdb_h__Rdb_table_handler::TableHandler;
use crate::ha_rocksdb_h__unique_sk_buf_info::UniqueSkBufInfo;
use crate::rdb_buff_h::StringWriter;
use crate::rdb_comparator_h::KeyDirection;
use crate::rdb_global_h::GlIndexId;

// Forward decls — the concrete types live in v4 sub-units / dependent units.
pub trait TblDefRef: Send + Sync {}
pub trait KeyDefRef: Send + Sync {}
pub trait ConverterRef: Send + Sync {}
pub trait TxnRef: Send + Sync {}

// --- POD shapes replacing C++ MariaDB types in the public interface ---
//
// We never expose `THD`, `TABLE`, `Field`, `key_range`, `Item` through the
// Rust surface. Instead, we expose narrow POD views populated by the shim
// from the C++ side before each handler entry point.

/// Read-only view of a `TABLE_SHARE` enough for codec / DDL operations.
/// Populated by the cxx shim from the MariaDB-side `TABLE*` per call.
#[derive(Debug)]
pub struct TableShareView {
    pub schema_name: String,
    pub table_name: String,
    pub primary_key_index: Option<u32>,
    /// Per-key descriptors (forward decl).
    pub keys: Vec<KeyShareView>,
}

#[derive(Debug)]
pub struct KeyShareView {
    pub name: String,
    pub key_parts: Vec<KeyPartShareView>,
    pub flags: u32,
}

#[derive(Debug)]
pub struct KeyPartShareView {
    pub field_offset: u32,
    pub field_length: u32,
    pub null_offset: Option<u32>,
}

/// Borrowed view of `MariaDB::key_range`. Used for index scans.
#[derive(Debug)]
pub struct KeyRangeRef<'a> {
    pub key: &'a [u8],
    pub length: u32,
    pub flag: KeyRangeFlag,
}

#[derive(Debug, Clone, Copy)]
pub enum KeyRangeFlag {
    /// `HA_READ_KEY_EXACT` / `HA_READ_KEY_OR_NEXT` etc. Encoded as MariaDB
    /// `ha_rkey_function` raw value to preserve semantics across the bridge.
    Raw(i32),
}

/// Row buffer alias — bytes in MariaDB record-layout format.
pub type RowBytes<'a> = &'a [u8];
pub type RowBytesMut<'a> = &'a mut [u8];

// --- The handler struct ---

/// Per-open-handler state. One instance per `handler::open()` call (i.e. per
/// connection × table open). Most fields are scratch buffers reused across
/// method calls within the handler's lifetime.
///
/// **All methods on this struct are declared in the v4 sub-unit files**
/// (`ha_rocksdb_cc__ha_rocksdb__*.rs`); this struct is the shared state
/// they manipulate.
///
/// Original: ha_rocksdb.h:147 — `class ha_rocksdb : public my_core::handler`.
pub struct HaSlateDb {
    // --- per-table shared state (Arc into the open-tables map) ---
    /// Reference to the table handler entry in the global open-tables map.
    /// Original: ha_rocksdb.h:150 — `Rdb_table_handler *m_table_handler`.
    pub table_handler: Arc<TableHandler>,

    /// Table definition (column / index metadata).
    /// Original: ha_rocksdb.h:170 — `Rdb_tbl_def *m_tbl_def`.
    pub tbl_def: Arc<dyn TblDefRef>,

    /// Primary key codec.
    /// Original: ha_rocksdb.h:173 — `std::shared_ptr<Rdb_key_def> m_pk_descr`.
    pub pk_descr: Arc<dyn KeyDefRef>,

    /// All index codecs (PK + secondary).
    /// Original: ha_rocksdb.h:176 — `std::shared_ptr<Rdb_key_def> *m_key_descr_arr`.
    pub key_descrs: Vec<Arc<dyn KeyDefRef>>,

    /// Cached: column count in PK. Original: ha_rocksdb.h:184.
    pub pk_key_parts: u32,
    /// Cached: whether PK columns can be decoded from the index.
    /// Original: ha_rocksdb.h:189.
    pub pk_can_be_decoded: bool,

    /// MariaDB↔SlateDB row format converter.
    /// Original: ha_rocksdb.h:240 — `std::shared_ptr<Rdb_converter> m_converter`.
    pub converter: Arc<dyn ConverterRef>,

    // --- scan state ---
    /// Current SlateDB iterator (if a scan is in flight).
    /// Replaces: `rocksdb::Iterator *m_scan_it` (ha_rocksdb.h:153).
    pub scan_it: Option<DbIterator>,

    /// Snapshot the current iterator was created against.
    /// Replaces: `const rocksdb::Snapshot *m_scan_it_snapshot` (ha_rocksdb.h:162).
    pub scan_it_snapshot: Option<Arc<DbSnapshot>>,

    /// Lower bound bytes for the current iterator (memcomparable key form).
    /// Original: ha_rocksdb.h:165 — `uchar *m_scan_it_lower_bound`.
    pub scan_it_lower_bound: Bytes,
    /// Upper bound bytes for the current iterator.
    pub scan_it_upper_bound: Bytes,

    /// Whether `m_scan_it` was created with bloom-filter skipping.
    pub scan_it_skips_bloom: bool,

    // --- scratch buffers ---
    /// PK in KeyTupleFormat (MariaDB-wire key format).
    /// Original: ha_rocksdb.h:191.
    pub pk_tuple: Vec<u8>,
    /// PK in StorageFormat (memcomparable). Original: ha_rocksdb.h:192.
    pub pk_packed_tuple: Vec<u8>,

    /// SK scratch buffer for the current operation.
    /// Original: ha_rocksdb.h:199.
    pub sk_packed_tuple: Vec<u8>,
    /// SK scratch buffer for UPDATE's old-row encoding.
    pub sk_packed_tuple_old: Vec<u8>,
    /// SK scratch buffer for range-scan end key.
    pub end_key_packed_tuple: Vec<u8>,

    /// Unpack-info writer for the current SK (per-row scratch).
    /// Original: ha_rocksdb.h:207.
    pub sk_tails: StringWriter,
    /// Unpack-info writer for the current PK.
    pub pk_unpack_info: StringWriter,
    /// Old SK tails for UPDATE.
    pub sk_tails_old: StringWriter,

    /// Prefix bytes from `index_read_map(... HA_READ_KEY_EXACT)` saved here
    /// for use by `index_next` / `index_prev`.
    /// Original: ha_rocksdb.h:214.
    pub sk_match_prefix: Bytes,
    pub sk_match_prefix_buf: Vec<u8>,

    /// Duplicate-check ping-pong buffer for inplace unique-SK creation.
    /// Original: ha_rocksdb.h:225-226 — `m_dup_sk_packed_tuple{,_old}`.
    /// (Container is the UniqueSkBufInfo, see ha_rocksdb_h__unique_sk_buf_info.rs.)
    pub unique_sk_buf: UniqueSkBufInfo,

    /// VARCHAR encoder scratch (passed to `pack_record` / `pack_index_tuple`).
    /// Original: ha_rocksdb.h:232.
    pub pack_buffer: Vec<u8>,

    /// Row scratch buffer big enough for any record.
    /// Original: ha_rocksdb.h:237.
    pub record_buffer: Vec<u8>,

    /// TTL timestamp bytes pointer (used during UPDATE to detect TTL change).
    /// Per _DESIGN.md §3, TTL is now in `RowEntry.expire_ts`, not in value
    /// bytes — this field becomes `Option<i64>` of the original expire_ts.
    pub ttl_bytes: Option<i64>,
    /// True when the TTL value changed during this UPDATE — forces SK
    /// re-encoding even when the SK columns are unchanged.
    pub ttl_changed_in_update: bool,

    /// Most-recently-retrieved value (bytes::Bytes for zero-copy borrow from
    /// `DbIterator::next()`). Holds onto the underlying SST block until next read.
    /// Original: ha_rocksdb.h:900 — `m_retrieved_record`.
    pub retrieved_record: Bytes,

    // --- replication-state flags (m_in_rpl_*, m_force_skip_unique_check) ---
    /// True when inside a replication DELETE-ROWS event.
    /// Original: ha_rocksdb.h:992.
    pub in_rpl_delete_rows: bool,
    /// True when inside a replication UPDATE-ROWS event.
    pub in_rpl_update_rows: bool,
    /// User-forced skip of unique check (sysvar / hint).
    pub force_skip_unique_check: bool,
}

// --- handler vtable trait ---
//
// The full vtable surface (~150 methods) is split across v4 sub-units. We
// declare ONE trait here as the umbrella; each sub-unit file adds `impl`
// blocks for its bucket's methods. This keeps the per-bucket signatures
// reviewable in isolation while presenting one logical handler interface
// to the cxx bridge.
//
// Method signatures matching MariaDB's `handler` vtable but with C++ types
// replaced by Rust POD shapes. Bodies are `todo!()` in the v4 sub-unit
// files; this trait is just the contract.

pub trait HandlerVtable {
    // Methods are declared per-bucket in the v4 sub-unit files:
    //   ha_rocksdb_cc__ha_rocksdb__lifecycle.rs   — open / close / external_lock / store_lock / reset / extra / init
    //   ha_rocksdb_cc__ha_rocksdb__ddl.rs         — create / delete_table / rename_table / truncate_table
    //   ha_rocksdb_cc__ha_rocksdb__dml.rs         — write_row / update_row / delete_row / delete_all_rows / bulk_insert
    //   ha_rocksdb_cc__ha_rocksdb__scan.rs        — rnd_init / rnd_next / rnd_pos / rnd_end / position
    //   ha_rocksdb_cc__ha_rocksdb__index.rs       — index_init / index_read / index_next / index_prev / index_first / index_last
    //   ha_rocksdb_cc__ha_rocksdb__info.rs        — info / records_in_range / scan_time / table_flags / index_flags
    //   ha_rocksdb_cc__ha_rocksdb__alter.rs       — check_if_supported_inplace / prepare / inplace / commit_inplace
    //   ha_rocksdb_cc__ha_rocksdb__txn.rs         — start_stmt / end_stmt / savepoint_*
    //   ha_rocksdb_cc__ha_rocksdb__repair.rs      — check / repair / analyze / optimize
    //   ha_rocksdb_cc__ha_rocksdb__convert.rs     — convert_record_to_storage_format / convert_record_from_storage_format
    //   ha_rocksdb_cc__ha_rocksdb__locks.rs       — check_keyread_allowed / build_decoder_*
    //   ha_rocksdb_cc__ha_rocksdb__auto_incr.rs   — auto-increment + hidden-PK
    //   ha_rocksdb_cc__ha_rocksdb__read.rs        — read_key_* / read_row_from_*
    //   ha_rocksdb_cc__ha_rocksdb__iter_setup.rs  — setup_iterator_*
    //   ha_rocksdb_cc__ha_rocksdb__ttl.rs         — should_hide_ttl_rec / skip_expired
    //   ha_rocksdb_cc__ha_rocksdb__error.rs       — get_error_message / rdb_error_to_mysql / print_error
    //   ha_rocksdb_cc__ha_rocksdb__metadata.rs    — name / comment / CF accessors
    //   ha_rocksdb_cc__ha_rocksdb__write_path.rs  — update_write_pk / sk / row
    //   ha_rocksdb_cc__ha_rocksdb__bulk_load_helpers.rs
    //   ha_rocksdb_cc__ha_rocksdb__buffer.rs      — alloc_key_buffers / free_key_buffers / set_last_rowkey
    //   ha_rocksdb_cc__ha_rocksdb__key_compare.rs — compare_keys / compare_key_parts
    //   ha_rocksdb_cc__ha_rocksdb__table_mgmt.rs  — truncate / get_range / idx_cond_push / update_stats
    //
    // See those files for the per-method signatures + doc comments.
}

impl HaSlateDb {
    /// Factory: construct from a freshly-opened TABLE plus its CF/index
    /// metadata. Allocates all scratch buffers up front (sized from
    /// `tbl_def`'s max-key-length). Mirrors `ha_rocksdb::open` (ha_rocksdb.h:636).
    pub async fn open(
        table_handler: Arc<TableHandler>,
        tbl_def: Arc<dyn TblDefRef>,
        pk_descr: Arc<dyn KeyDefRef>,
        key_descrs: Vec<Arc<dyn KeyDefRef>>,
        converter: Arc<dyn ConverterRef>,
    ) -> Result<Self, Error> {
        todo!("alloc all scratch buffers; populate cached fields")
    }

    /// Releases the handler. Drops `scan_it`, releases the TableHandler
    /// reference (decrementing its ref-count), and frees scratch buffers.
    /// Mirrors `ha_rocksdb::close` (ha_rocksdb.h:638).
    pub async fn close(self) -> Result<(), Error> {
        todo!("close iterator; release table_handler")
    }

    /// Borrow the current snapshot for read-side ops. Returns `None` if no
    /// txn / scan is in flight (caller should call `Db::snapshot()` themselves).
    pub fn current_snapshot(&self) -> Option<&Arc<DbSnapshot>> {
        self.scan_it_snapshot.as_ref()
    }

    /// Borrow the current scan iterator. Returns `None` between `rnd_end`
    /// (or `index_end`) and the next `rnd_init` / `index_init`.
    pub fn current_iter(&self) -> Option<&DbIterator> {
        self.scan_it.as_ref()
    }
}

impl HandlerVtable for HaSlateDb {
    // All methods provided by v4 sub-unit files.
}
