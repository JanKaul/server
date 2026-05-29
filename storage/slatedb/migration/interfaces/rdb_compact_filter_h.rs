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
//! We implement `slatedb::CompactionFilter` directly rather than inventing
//! a parallel type — that way the supplier returns a `Box<dyn slatedb::
//! CompactionFilter>` that `DbBuilder::with_compaction_filter_supplier(...)`
//! consumes verbatim. The `Modify(ValueDeletable)` decision variant is
//! preserved for future use (legacy-format conversion, value rewrites).
//!
//! ## Out-of-scope methods
//! None — the dropped-index sweep is in-scope; TTL is delegated to SlateDB.

#![cfg(feature = "compaction_filters")]

use slatedb::{
    CompactionFilter, CompactionFilterDecision, CompactionFilterError,
    CompactionFilterSupplier, CompactionJobContext, RowEntry,
};
use std::collections::HashSet;
use std::sync::Arc;

use crate::rdb_global_h::GlIndexId;

// Forward decl — real type lives in `rdb_datadic_h__Rdb_dict_manager.rs`.
pub trait DictManagerRef: Send + Sync {}

/// Per-job compaction filter for our engine. Created by
/// [`RdbCompactFilterSupplier::create_compaction_filter`] at the start
/// of each compaction job; freezes the dropped-index set at job start.
///
/// Original: rdb_compact_filter.h — `class Rdb_compact_filter`.
pub struct RdbCompactFilter {
    /// Snapshot of dropped-index ids at job-start time. Frozen for the
    /// duration of this compaction job (in line with SlateDB's
    /// `CompactionFilterSupplier` contract).
    dropped_indexes: HashSet<GlIndexId>,
}

#[async_trait::async_trait]
impl CompactionFilter for RdbCompactFilter {
    /// Inspect one entry. Returns:
    /// - `Drop` if the entry's key prefix decodes to a `gl_index_id` in
    ///   `dropped_indexes` (the index was DROPped; sweep the leftover rows).
    /// - `Keep` otherwise. We do NOT use `Modify` today — value rewrites are
    ///   reserved for a future legacy-format-conversion filter.
    ///
    /// Per the SlateDB doc on `Drop` for `Merge` entries: prefer `Drop` over
    /// `Modify(Tombstone)` for merge operands — relevant if `MyRocks::merge`
    /// is ever used (currently only `Rdb_system_merge_op` uses Merge, and
    /// dropped indexes can't contain system-CF entries by construction).
    async fn filter(
        &mut self,
        entry: &RowEntry,
    ) -> Result<CompactionFilterDecision, CompactionFilterError> {
        let _ = entry; // Use entry.key — read gl_index_id from the key prefix.
        todo!("parse gl_index_id from entry.key prefix; if in self.dropped_indexes return Drop else Keep")
    }

    /// Called after all entries are processed (only on successful compactions).
    /// Used to log how many entries we dropped per job.
    async fn on_compaction_end(&mut self) -> Result<(), CompactionFilterError> {
        todo!("emit metric: dropped_count_by_index")
    }
}

/// Factory; registered once at engine init via
/// `DbBuilder::with_compaction_filter_supplier(Arc::new(RdbCompactFilterSupplier::new(...)))`.
///
/// Original: rdb_compact_filter.h — `class Rdb_compact_filter_factory`.
pub struct RdbCompactFilterSupplier {
    /// Borrowed dict manager for reading the dropped-index registry at
    /// job-start time.
    dict: Arc<dyn DictManagerRef>,
}

impl RdbCompactFilterSupplier {
    pub fn new(dict: Arc<dyn DictManagerRef>) -> Self {
        Self { dict }
    }
}

#[async_trait::async_trait]
impl CompactionFilterSupplier for RdbCompactFilterSupplier {
    /// Build a fresh filter instance for a compaction job. The dropped-index
    /// set is frozen at this point — if a DROP INDEX happens mid-job, its
    /// indexes won't be swept until the next job.
    ///
    /// `_context.retention_min_seq` could be checked to avoid dropping
    /// entries that active snapshots still see; we don't do that today
    /// because the dropped-index sweep is itself snapshot-isolated (the
    /// DROP INDEX commit predates any reader's snapshot of the dropped
    /// index id).
    async fn create_compaction_filter(
        &self,
        _context: &CompactionJobContext,
    ) -> Result<Box<dyn CompactionFilter>, CompactionFilterError> {
        let _dict = self.dict.clone();
        todo!("dict.get_dropped_indexes().await; box up RdbCompactFilter { dropped_indexes }")
    }
}
