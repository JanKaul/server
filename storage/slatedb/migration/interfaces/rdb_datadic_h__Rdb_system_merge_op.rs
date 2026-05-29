//! Interface stub for `rdb_datadic_h__Rdb_system_merge_op`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (class at line 1546)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_system_merge_op`
//! parent: `rdb_datadic_h`
//!
//! ## Mapping
//! Merge operator for system-CF entries (counters, version stamps). MyRocks
//! used `rocksdb::AssociativeMergeOperator` registered on the system CF.
//!
//! Per _DESIGN.md §1, **SlateDB has native `MergeOperator`** — we register
//! this directly via `DbBuilder::with_merge_operator(...)`. The operator
//! is key-aware (per `KeyPrefixMergeOperator` example in
//! merge_operator.rs:857), routing by the system-CF sub-prefix to different
//! merge semantics (counter sum, version max, etc.).
//!
//! ## Out-of-scope methods
//! None.

use slatedb::bytes::Bytes;
use slatedb::{MergeOperator, MergeOperatorError};

/// System-CF merge operator. Registered once at engine init.
///
/// Original: rdb_datadic.h:1546 — `class Rdb_system_merge_op : public rocksdb::AssociativeMergeOperator`.
pub struct SystemMergeOp;

impl MergeOperator for SystemMergeOp {
    /// Merge an existing value with a new operand. Routes by key prefix:
    /// - `system_cf||"counter:*"`: sum (le-u64).
    /// - `system_cf||"version:*"`: max (le-u64).
    /// - `system_cf||"set:*"`: byte-wise union of length-prefixed members.
    /// - other: concat (debug fallback).
    fn merge(
        &self,
        key: &Bytes,
        existing_value: Option<Bytes>,
        operand: Bytes,
    ) -> Result<Bytes, MergeOperatorError> {
        todo!("route by key sub-prefix; implement per-merge-type combine")
    }

    // Default `merge_batch` impl is fine (calls `merge` pairwise). Override
    // later if a hot bucket needs O(N) instead of O(N^2).
}
