//! Interface stub for `nosql_access_h`.
//!
//! C++ source: `storage/rocksdb/nosql_access.h` (36 LoC)
//!
//! ## Mapping
//! Direct point-lookup fast path that bypassed the SQL layer in MyRocks.
//! Per _DESIGN.md §1, this is a **§1 NON-GOAL** — dropped.
//!
//! ## Out-of-scope methods
//! All — every entry point returns `HA_ERR_WRONG_COMMAND` (via
//! `slatedb::Error::invalid`).

use slatedb::Error;

/// The MariaDB-internal entry point for the NoSQL access path. We always
/// reject so MariaDB falls back through the standard handler vtable.
///
/// Original: nosql_access.h — `rocksdb_handle_single_table_select`.
pub fn handle_single_table_select() -> Result<(), Error> {
    Err(Error::invalid("non-goal: NoSQL access path".into()))
}
