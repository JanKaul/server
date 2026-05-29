//! Interface stub for `rdb_datadic_h__Rdb_tbl_def`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (class at line 1079)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_tbl_def`
//! parent: `rdb_datadic_h`
//!
//! ## Mapping
//! Per-table metadata: name, set of `Rdb_key_def` keys, hidden-PK indicator,
//! auto-incr generator. Persisted in our system CF (`SYSTEM_CF_ID`, see
//! `rdb_global_h.rs`). Per _DESIGN.md §2/§3, codec types preserved from MyRocks.
//!
//! ## Out-of-scope methods
//! None — pure metadata.

use bytes::Bytes;
use slatedb::Error;
use std::sync::Arc;

use crate::ha_rocksdb_h__ha_rocksdb::KeyDefRef;
use crate::rdb_global_h::GlIndexId;

/// Per-table metadata record. One per CREATE TABLE; persisted in
/// the system CF, cached by `Rdb_ddl_manager`.
///
/// Original: rdb_datadic.h:1079 — `class Rdb_tbl_def`.
pub struct TblDef {
    /// Normalized DB-qualified name ("schema.table").
    pub name: String,
    /// All keys (PK at index 0 if present, else hidden PK).
    pub keys: Vec<Arc<dyn KeyDefRef>>,
    /// True if the PK was synthesized (HIDDEN_PK_ID column).
    pub has_hidden_pk: bool,
    /// Current auto-increment value (monotonic across restarts).
    pub auto_incr_val: u64,
    /// Per-table version generation (bumped on each DDL).
    pub version: u64,
}

impl TblDef {
    /// Encode this tbl_def to bytes for system-CF persistence. Format
    /// preserved from MyRocks (see `rdb_datadic_cc__Rdb_tbl_def.rs` for
    /// the implementation).
    pub fn serialize(&self) -> Bytes {
        todo!("MyRocks tbl_def serialization (preserves format)")
    }

    /// Decode from system-CF bytes.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, Error> {
        todo!("inverse of serialize()")
    }
}
