//! Interface stub for `ha_rocksdb_proto_h`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb_proto.h` (103 LoC)
//!
//! ## Mapping
//! This is a header of free-function prototypes — accessor singletons and
//! error-handling helpers used across the rest of MyRocks. There are no
//! classes; the contents are translated as Rust accessor fns over the
//! plugin's `OnceLock<EngineState>` (see _DESIGN.md §8 module layout, the
//! `engine` and `plugin` modules).
//!
//! Per _DESIGN.md §1 the underlying types map as follows:
//! - `rocksdb::TransactionDB*` → `Arc<slatedb::Db>` (transactions via
//!   `Db::begin(IsolationLevel)`).
//! - `Rdb_cf_manager&` → our CF-id → key-prefix mapper (cf_manager_h).
//! - `rocksdb::BlockBasedTableOptions&` → `slatedb::Settings` (single
//!   global, not per-CF, per _DESIGN.md §1 "Block cache → Map (degraded)").
//! - `Rdb_dict_manager*` / `Rdb_ddl_manager*` / `Rdb_binlog_manager*` →
//!   wrappers around system-CF reads/writes; see the existing
//!   `Rdb_dict_manager.rs` / `Rdb_ddl_manager.rs` exemplars.
//!
//! ## Out-of-scope methods
//! - `rdb_handle_io_error(rocksdb::Status, ...)` — the RocksDB `Status` type
//!   doesn't exist for us. Replaced by the `slatedb::Error → HA_ERR_*`
//!   mapping in _DESIGN.md §4, which lives in `rust/src/error.rs`.
//! - `rdb_get_rocksdb_db()` — there is no `rocksdb::TransactionDB`. We expose
//!   the SlateDB handle instead via `engine_db()`.

use slatedb::Error;
use std::sync::Arc;

/// Categories of I/O errors. Preserved 1:1 from C++ `RDB_IO_ERROR_TYPE`
/// (ha_rocksdb_proto.h:31). Used by `handle_io_error` to pick the right
/// error-log message and abort-or-continue policy.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoErrorType {
    TxCommit = 0,
    DictCommit = 1,
    BgThread = 2,
    General = 3,
}

/// Human-readable label for an `IoErrorType`. Replaces C++
/// `get_rdb_io_error_string`.
pub fn io_error_string(t: IoErrorType) -> &'static str {
    match t {
        IoErrorType::TxCommit => "transaction commit",
        IoErrorType::DictCommit => "dictionary commit",
        IoErrorType::BgThread => "background thread",
        IoErrorType::General => "general",
    }
}

/// Log + (for some categories) abort on a SlateDB-side I/O error. Replaces
/// C++ `rdb_handle_io_error(rocksdb::Status, RDB_IO_ERROR_TYPE)`.
///
/// Policy (matches MyRocks):
/// - `TxCommit`: log and return — let the caller propagate.
/// - `DictCommit`, `BgThread`: log and abort the process (data dictionary
///   inconsistency / unrecoverable background work failure).
/// - `General`: log and return.
pub fn handle_io_error(_err: &Error, _kind: IoErrorType) {
    todo!("log err.to_string() via the engine logger; if kind in (DictCommit|BgThread), abort")
}

/// Normalize a fully-qualified table name (`db.table` or `db.table#part`).
/// Replaces C++ `rdb_normalize_tablename`.
///
/// Returns the canonical form (e.g., trim whitespace, lowercase per
/// `lower_case_table_names`).
pub fn normalize_tablename(_tablename: &str) -> Result<String, Error> {
    todo!("trim, validate `db.table[#part]` form, lowercase per lower_case_table_names sysvar")
}

/// Inverse of `normalize_tablename`. Splits `db.table#partition` into the
/// three pieces; `partition` is `None` when no `#` suffix is present.
/// Replaces C++ `rdb_split_normalized_tablename`.
pub fn split_normalized_tablename(
    _fullname: &str,
) -> Result<(String, String, Option<String>), Error> {
    todo!("split on '.' then on '#'")
}

/// All currently-open table names. Used by `SHOW ENGINE ROCKSDB STATUS`.
pub fn open_table_names() -> Vec<String> {
    todo!("snapshot the open-tables registry maintained by handler::open()/close()")
}

/// Update global op-type counters. The `operation_type` arg is the C++
/// `enum operation_type`; in Rust we use `rdb_global_h::OperationType`.
/// Replaces C++ `rdb_update_global_stats`.
pub fn update_global_stats(
    _op: crate::rdb_global_h::OperationType,
    _count: u32,
    _is_system_table: bool,
) {
    todo!("bump the matching sharded counter in the engine's GlobalStats struct")
}

/// Schedule a save of in-memory stats to the system CF on the next
/// background-thread tick. C++ `rdb_queue_save_stats_request`.
pub fn queue_save_stats_request() {
    todo!("signal the Rdb_background_thread analogue via its mpsc channel")
}

/// True if TTL is enabled engine-wide (sysvar `rocksdb_enable_ttl`).
pub fn is_ttl_enabled() -> bool { todo!("read sysvar atomic") }

/// True if TTL filtering applies to reads (sysvar
/// `rocksdb_enable_ttl_read_filtering`). With SlateDB's native TTL
/// (_DESIGN.md §0) this is always-on; the sysvar is preserved for parity but
/// ignored.
pub fn is_ttl_read_filtering_enabled() -> bool { true }

/// Read the engine's SlateDB handle. Replaces C++ `rdb_get_rocksdb_db()` which
/// returned `rocksdb::TransactionDB*`.
///
/// Errors: `Closed` if the engine has not been initialized or is shutting down.
pub fn engine_db() -> Result<Arc<slatedb::Db>, Error> {
    todo!("plugin::ENGINE.get().map(|e| e.db.clone()).ok_or(Error::closed(Clean))")
}

/// Read per-table perf counters. The `Rdb_perf_counters` type lives in
/// `rdb_perf_context_h`. Replaces C++ `rdb_get_table_perf_counters`.
pub fn table_perf_counters(
    _tablename: &str,
) -> Result<crate::rdb_perf_context_h::PerfCounters, Error> {
    todo!("walk the per-table perf-counter map keyed by normalized tablename")
}

/// Global aggregate of perf counters.
pub fn global_perf_counters() -> crate::rdb_perf_context_h::PerfCounters {
    todo!("snapshot engine.atomic_perf_counters into a PerfCounters")
}
