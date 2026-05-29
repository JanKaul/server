//! Interface stub for `nosql_access_cc`.
//!
//! C++ source: `storage/rocksdb/nosql_access.cc` (53 LoC, mostly stub in tree)
//! C++ free fn: `myrocks::rocksdb_handle_single_table_select`
//!
//! ## Mapping
//! Per _DESIGN.md §1 ("NoSQL access path" row — Non-goal): MyRocks shipped a
//! bypass that performed point lookups directly against RocksDB without going
//! through the SQL optimizer. We do not port this. All entry points return
//! `slatedb::Error::invalid("non-goal: NoSQL access path")` which the shim
//! layer translates to `HA_ERR_WRONG_COMMAND` (see _DESIGN.md §4).
//!
//! Even in upstream MyRocks the MariaDB port of this file is a no-op stub
//! (the `.cc` body is literally `return false;`), so dropping the feature
//! changes nothing observable for MariaDB users.
//!
//! ## Out-of-scope methods (returning non-goal Error)
//! - `rocksdb_handle_single_table_select` — bypass entrypoint.

use slatedb::Error;

/// Attempt to handle a `SELECT * FROM t WHERE pk = ?` via direct point-lookup.
///
/// Inputs (kept as opaque slot placeholders — never expose `THD` / `st_select_lex`
/// at the Rust API surface per the task contract):
/// - `_thd_slot`: opaque session handle.
/// - `_select_slot`: opaque SELECT_LEX handle.
///
/// Output: `Ok(false)` would mean "not handled, fall through to normal path";
/// `Ok(true)` would mean "handled, result already sent to client". We return
/// `Err` to signal the feature is not supported and the SQL layer should
/// produce `ER_NOT_SUPPORTED_YET`.
///
/// Errors: always `slatedb::ErrorKind::Invalid` with the non-goal marker.
///
/// Original: nosql_access.cc:48 — `rocksdb_handle_single_table_select`.
pub fn handle_single_table_select(_thd_slot: usize, _select_slot: usize) -> Result<bool, Error> {
    Err(Error::invalid("non-goal: NoSQL access path".into()))
}
