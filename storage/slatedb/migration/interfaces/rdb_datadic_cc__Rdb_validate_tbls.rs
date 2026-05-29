//! Interface stub for `Rdb_validate_tbls`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.cc` (lines 3782..3942)
//! C++ header: `storage/rocksdb/rdb_datadic.h` (Rdb_validate_tbls declarations)
//! v4 manifest sub-unit: `Rdb_validate_tbls`
//! Parent unit: `rdb_datadic.cc`
//! Approx body LoC accounted for here: ~160
//!
//! ## Mapping
//! Per _DESIGN.md §1: pure filesystem-scanning helper, no SlateDB API
//! surface. Walks the `mysql_data_home/<db>/*.frm` tree and cross-checks
//! against the in-memory `DdlManager` cache:
//!
//! - For every `.frm` belonging to engine "ROCKSDB"/"partition", expects a
//!   matching `Rdb_tbl_def` in the cache. Logs a warning if missing.
//! - Used once at server startup by `Rdb_ddl_manager::init` to detect
//!   "FRM exists but no DDL entry" inconsistencies after a crash.
//!
//! Filesystem ops use `std::fs::read_dir` (sync) rather than `tokio::fs`
//! because this runs once at startup outside the Tokio runtime.
//!
//! ## Out-of-scope methods
//! None. The MyRocks dependency on `dd_frm_type()` for engine detection is
//! a MariaDB SQL-layer call; we keep the same interface (parameter is a
//! callback supplied by the SQL layer at TRANSLATE time).

use slatedb::Error;
use std::collections::{HashMap, BTreeSet};

/// A single (tablename, is_partition) tuple within a database directory.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TblInfo {
    pub tablename: String,
    pub is_partition: bool,
}

/// `Rdb_validate_tbls` — startup .frm cross-check.
pub struct ValidateTbls {
    /// Expected-tables map keyed by dbname; values are the per-db set of
    /// `(tablename, is_partition)` tuples that the DDL cache claims exist.
    pub list: HashMap<String, BTreeSet<TblInfo>>,
}

impl ValidateTbls {
    pub fn new() -> Self { Self { list: HashMap::new() } }

    /// Insert one table-def into the expected-tables map. Temp tables
    /// (whose name begins with `#sql-`) are silently filtered.
    ///
    /// C++: rdb_datadic.cc:3782.
    pub fn add_table(
        &mut self,
        _tdef: &crate::Rdb_tbl_def::TblDef,
    ) -> Result<(), Error> {
        todo!("port C++ add_table at rdb_datadic.cc:3782")
    }

    /// Cross-check ONE `.frm` file against the expected-tables map. If the
    /// .frm engine string is "ROCKSDB", the entry must be present in `list`;
    /// otherwise a warning is logged and `*has_errors` set to true.
    ///
    /// `engine_resolver`: callback that opens `<fullpath>/<tablename>.frm`
    /// and returns its engine type string (the MariaDB `dd_frm_type` call).
    ///
    /// C++: rdb_datadic.cc:3799.
    pub fn check_frm_file(
        &mut self,
        _fullpath: &str,
        _dbname: &str,
        _tablename: &str,
        _engine_resolver: &dyn Fn(&str) -> Option<String>,
        _has_errors: &mut bool,
    ) -> Result<bool, Error> {
        todo!("port C++ check_frm_file at rdb_datadic.cc:3799")
    }

    /// Scan one db-subdirectory for `*.frm` files and dispatch each to
    /// `check_frm_file`. Removes empty per-db sets after the scan completes.
    ///
    /// C++: rdb_datadic.cc:3859.
    pub fn scan_for_frms(
        &mut self,
        _datadir: &str,
        _dbname: &str,
        _engine_resolver: &dyn Fn(&str) -> Option<String>,
        _has_errors: &mut bool,
    ) -> Result<bool, Error> {
        todo!("port C++ scan_for_frms at rdb_datadic.cc:3859")
    }

    /// Entry-point: walk `datadir`, for each db-subdirectory call
    /// `scan_for_frms`. After this completes, `self.list` contains only
    /// entries the DDL cache claims exist but no .frm was found for —
    /// i.e. "DDL entry exists but FRM is missing" inconsistencies.
    ///
    /// C++: rdb_datadic.cc:3907.
    pub fn compare_to_actual_tables(
        &mut self,
        _datadir: &str,
        _engine_resolver: &dyn Fn(&str) -> Option<String>,
        _has_errors: &mut bool,
    ) -> Result<bool, Error> {
        todo!("port C++ compare_to_actual_tables at rdb_datadic.cc:3907")
    }
}

impl Default for ValidateTbls {
    fn default() -> Self { Self::new() }
}
