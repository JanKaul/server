//! Interface stub for `rdb_i_s_cc__shared`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 48..79, 1759..1766)
//! v4 manifest sub-unit: `rdb_i_s_cc__shared`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 row "`information_schema` tables (13)" — Re-implemented.
//!
//! This module factors out items shared across all 13 `rdb_i_s_*` sub-units:
//! - The `Show::*` type aliases used by every `fields_info[]` array (`Varchar`,
//!   `SLong`, `SLonglong`, `Double`, etc.).
//! - The `ROCKSDB_FIELD_INFO` / `ROCKSDB_FIELD_INFO_END` macros expressed as
//!   typed Rust helpers.
//! - The shared `rdb_i_s_deinit` callback (returns 1; the C++ comment defers
//!   actual cleanup to `rocksdb_done_func`).
//! - The shared `st_mysql_information_schema rdb_i_s_info` descriptor used
//!   as the `info` pointer of every `st_maria_plugin`.
//! - The `slatedb::Error → MariaDB int` adapter that I_S `fill_table`s use
//!   when surfacing errors to the server, plus the common `Row` / `FieldValue`
//!   buffer that every per-table `fill_table` returns to its sync wrapper.
//!
//! ## Out-of-scope methods
//! None — this is shared scaffolding; if a piece isn't usable yet it gets a
//! `// TODO(human): ...` not a non-goal stub.
//!
//! Original LoC range covered (incl. tail of file):
//!   48..79 (namespace open + `Show::*` aliases + ROCKSDB_FIELD_INFO macros)
//!   1759..1766 (`rdb_i_s_deinit` + `rdb_i_s_info`)
//!   1971..1975 (namespace close + maria_declare_plugin list epilogue)

use slatedb::Error;

// --- column-type aliases (mirror Show::* in the C++ file) ---
//
// The C++ side uses `Show::Column`, `Show::Varchar`, etc. to build the
// `ST_FIELD_INFO[]` static arrays. In Rust we model each column as a typed
// `Column` value; per-table modules build `[Column; N]` arrays at construction
// time. The exact bridge to MariaDB `ST_FIELD_INFO` is done in the cxx layer
// (`rust/src/bridge.rs`); these types are the Rust-side surface.

/// Nullability flag matching MariaDB's `NOT_NULL` / `NULLABLE`.
/// Original: rdb_i_s.cc:55 — `ROCKSDB_FIELD_INFO(...)` flag arg.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nullable {
    NotNull,
    Nullable,
}

/// One I_S column descriptor. Mirrors `Show::Column(name, type, nullable)`.
/// Original: rdb_i_s.cc:55 — `ROCKSDB_FIELD_INFO` macro.
#[derive(Debug, Clone)]
pub struct Column {
    pub name: &'static str,
    pub ty: ColumnType,
    pub nullable: Nullable,
}

/// Column type variants matching the `Show::*` helpers used in the C++ file
/// (`Varchar(n)`, `SLong`, `SLonglong`, `ULonglong`, `Double(precision)`,
/// `SShort(width)`, `STiny`).
/// Original: rdb_i_s.cc:69..78.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Varchar(u32),
    STiny,
    SShort(u32),
    SLong,
    SLonglong,
    ULonglong,
    Double(u32),
}

// --- shared row buffer ---

/// One row of any I_S table — heterogeneous values matching the table's
/// `fields_info()` layout. Each per-table `fill_table` returns `Vec<Row>`;
/// the sync bridge copies values into the MariaDB `TABLE->field[]` slots.
#[derive(Debug, Clone)]
pub struct Row {
    pub values: Vec<FieldValue>,
}

/// A single column value. `Null` is only legal for columns whose `fields_info()`
/// declared `Nullable::Nullable` — the bridge `debug_assert!`s this.
#[derive(Debug, Clone)]
pub enum FieldValue {
    Null,
    Str(String),
    I32(i32),
    I64(i64),
    U64(u64),
    F64(f64),
}

// --- plugin descriptor shared across all 13 I_S tables ---

/// Mirrors `st_mysql_information_schema rdb_i_s_info`.
/// Original: rdb_i_s.cc:1765 — `MYSQL_INFORMATION_SCHEMA_INTERFACE_VERSION`.
pub const I_S_INTERFACE_VERSION: i32 = 0x0301; // matches the MariaDB constant; bridge verifies

/// Shared `deinit` callback. The C++ version returns 1 (intentional — the
/// comment defers real cleanup to `rocksdb_done_func()` in `ha_rocksdb.cc`).
/// We preserve that semantic.
/// Original: rdb_i_s.cc:1759 — `rdb_i_s_deinit`.
pub fn deinit() -> i32 {
    1
}

// --- error adapter ---

/// Translate a `slatedb::Error` raised inside a `fill_table` body into the
/// MariaDB `int` return code expected by the schema-table machinery. Uses the
/// same mapping documented in _DESIGN.md §4 for handler entry points.
///
/// Returns 0 on success (caller passes `Ok(())`), or a positive HA_ERR_* code.
/// Original: rdb_i_s.cc:152, 245, etc. — pattern of `DBUG_RETURN(ret)` after
/// either `0` or a propagated error code.
pub fn fill_result_to_ha_err(res: Result<(), Error>) -> i32 {
    match res {
        Ok(()) => 0,
        Err(_) => {
            // TODO(human): wire to the shared `slatedb_error_to_ha_err` adapter
            // once `crate::error` lands. For now propagate a generic failure.
            todo!("call crate::error::slatedb_error_to_ha_err once available")
        }
    }
}

// --- shared bridge marker ---

/// Marker trait implemented by every per-table I_S module. The plugin
/// registration in `rust/src/plugin/` collects all implementors via inventory
/// so `maria_declare_plugin(rocksdb_se)` can list them.
///
/// Each impl exposes:
/// - the plugin name string (`"ROCKSDB_CFSTATS"`, etc.)
/// - the `fields_info()` accessor returning `&[Column]`
/// - the async `fill_table` entry point returning `Result<Vec<Row>, Error>`
/// - the `init` callback that the I_S engine invokes to wire `fields_info` +
///   `fill_table` into the `ST_SCHEMA_TABLE`.
pub trait IsTable: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn fields_info(&self) -> &'static [Column];
    fn init(&self, plugin: *mut std::ffi::c_void) -> Result<(), Error>;
}
