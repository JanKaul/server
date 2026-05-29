//! Interface stub for `rdb_compact_filter_h`.
//!
//! C++ source: `storage/rocksdb/rdb_compact_filter.h` (216 LoC)
//!
//! ## Mapping
//! MyRocks' compaction filter implementations:
//! - `Rdb_compact_filter`: dropped-secondary-index sweep
//! - `Rdb_compact_filter_factory`: per-CF factory hook
//!
//! Per _DESIGN.md §1, these map to **SlateDB's `CompactionFilter` trait**
//! (feature `compaction_filters`). Since SlateDB has native `expire_ts`,
//! TTL filtering is automatic — our filter only handles the dropped-index
//! sweep case (return `Drop` for keys whose `gl_index_id` is in the
//! dropped-index registry).
//!
//! ## Out-of-scope methods
//! None — the dropped-index sweep is in-scope; TTL is delegated to SlateDB.

use slatedb::Error;
use slatedb::bytes::Bytes;

// SlateDB's CompactionFilter trait is feature-gated; we re-export the
// types we use to keep this stub compile-checkable even when the feature
// is off. Implementation lives in `rdb_compact_filter_cc.rs` (which is
// only built when feature `compaction_filters` is enabled).

/// Decision returned by our filter for each entry visited during compaction.
/// Mirrors `slatedb::CompactionFilterDecision`.
#[derive(Debug, Clone, PartialEq)]
pub enum CompactionDecision {
    Keep,
    /// Drop the entry entirely. Used for dropped-index sweep.
    Drop,
}

/// Per-job filter instance. Created by `RdbCompactFilterSupplier`.
///
/// Original: rdb_compact_filter.h — `class Rdb_compact_filter`.
pub struct RdbCompactFilter {
    /// Snapshot of dropped-index ids at job-start time. Frozen for the
    /// duration of this compaction job (in line with SlateDB's
    /// `CompactionFilterSupplier` contract).
    pub dropped_indexes: std::collections::HashSet<crate::rdb_global_h::GlIndexId>,
}

impl RdbCompactFilter {
    /// Decide what to do with this entry. Reads the gl_index_id from the
    /// key prefix; returns `Drop` if it's in `dropped_indexes`.
    pub fn filter(&mut self, key: &[u8]) -> Result<CompactionDecision, Error> {
        todo!("parse gl_index_id from key prefix; check membership in dropped_indexes")
    }
}

/// Supplier (factory). Registered once at engine init.
/// Original: `class Rdb_compact_filter_factory`.
pub struct RdbCompactFilterSupplier;

impl RdbCompactFilterSupplier {
    /// Build a fresh filter instance for a compaction job. Reads the
    /// current dropped-index registry from `DictManager` at this point
    /// and freezes it onto the filter.
    pub async fn create_compaction_filter(&self) -> Result<RdbCompactFilter, Error> {
        todo!("read DictManager.get_dropped_indexes(); construct RdbCompactFilter")
    }
}
