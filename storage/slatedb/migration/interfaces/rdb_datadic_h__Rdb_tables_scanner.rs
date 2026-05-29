//! Interface stub for `rdb_datadic_h__Rdb_tables_scanner`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (interface at line 1178)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_tables_scanner`
//! parent: `rdb_datadic_h`
//!
//! ## Mapping
//! Visitor interface for `Rdb_ddl_manager::scan_for_tables`. The DDL
//! manager invokes `add_table` once per cached tbl_def; the visitor
//! decides what to do (typically populate an I_S rowset).
//!
//! ## Out-of-scope methods
//! None — pure interface.

use slatedb::Error;
use std::sync::Arc;

use crate::rdb_datadic_h__Rdb_tbl_def::TblDef;

/// Visitor invoked once per cached tbl_def during enumeration. Implementors
/// typically populate an I_S rowset or perform DDL discovery.
///
/// Original: rdb_datadic.h:1178 — `interface Rdb_tables_scanner` (where
/// `interface` is a `struct` macro from `rdb_utils.h`).
pub trait TablesScanner: Send {
    /// Process one tbl_def. Return `Err` to abort the scan.
    fn add_table(&mut self, tbl_def: Arc<TblDef>) -> Result<(), Error>;
}
