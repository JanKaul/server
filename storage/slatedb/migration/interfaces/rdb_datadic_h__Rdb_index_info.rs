//! Interface stub for `rdb_datadic_h__Rdb_index_info`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (struct at line 1525)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_index_info`
//! parent: `rdb_datadic_h`
//!
//! ## Mapping
//! Per-index summary record persisted in the system CF. Holds:
//! `(cf_id, index_id, index_type, key_count, kv_version, ttl_duration)`.
//! Per _DESIGN.md §2, format preserved from MyRocks (codec compatibility).
//!
//! ## Out-of-scope methods
//! None — pure data carrier.

use crate::rdb_global_h::GlIndexId;

/// Index-summary record persisted in the system CF, one per index.
///
/// Original: rdb_datadic.h:1525 — `struct Rdb_index_info`.
#[derive(Debug, Clone)]
pub struct IndexInfo {
    pub gl_index_id: GlIndexId,
    /// `0` = primary, `1` = secondary, `2` = hidden PK (constants match MyRocks).
    pub index_type: u8,
    /// Key-value format version (for backward-compat codec).
    pub kv_version: u16,
    /// Index dictionary version.
    pub index_dict_version: u32,
    /// Number of key columns.
    pub key_count: u32,
    /// TTL duration in seconds; 0 = no TTL.
    pub ttl_duration: u64,
}

impl IndexInfo {
    /// Encode to the system-CF value bytes (preserves MyRocks format).
    pub fn serialize(&self) -> bytes::Bytes {
        todo!("preserve MyRocks Rdb_index_info encoding")
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self, slatedb::Error> {
        todo!("inverse of serialize()")
    }
}
