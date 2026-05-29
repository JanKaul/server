//! Interface stub for `ha_rocksdb_h__key_def_cf_info`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.h` (lines 668..672, 5 LoC body)
//! v4 manifest sub-unit: `ha_rocksdb_h__key_def_cf_info`
//! parent: `ha_rocksdb_h`
//!
//! ## Mapping
//! Per-key CF metadata bundle used during table CREATE / inplace ALTER to
//! pass CF-handle + reverse-flag + per-partition-flag to `create_key_def`.
//!
//! Per _DESIGN.md §1 (CFs → key-prefix scheme, native `PrefixExtractor`):
//! `cf_handle` is replaced by `cf_id: u32`. `is_reverse_cf` carries through
//! as `KeyDirection::Reverse` (see `rdb_comparator_h.rs`). `is_per_partition_cf`
//! is a layout hint preserved for the codec.
//!
//! ## Out-of-scope methods
//! None — pure data carrier.

use crate::rdb_comparator_h::KeyDirection;

/// Per-index CF metadata for `create_key_def`. Constructed during table DDL
/// by walking the partition options / comment qualifiers (see
/// `rdb_datadic_h__Rdb_tbl_def.rs`).
///
/// Original: ha_rocksdb.h:668 — `struct key_def_cf_info`.
#[derive(Debug, Clone, Copy)]
pub struct KeyDefCfInfo {
    /// CF id (replaces MyRocks' `rocksdb::ColumnFamilyHandle*`).
    pub cf_id: u32,
    /// Forward vs reverse key ordering for this index.
    pub direction: KeyDirection,
    /// True when this CF was created via per-partition `cfname` qualifier
    /// (affects how partition tables map to CFs — purely informational here).
    pub is_per_partition_cf: bool,
}
