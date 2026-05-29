//! Interface stub for `ha_rocksdb_h__Rdb_inplace_alter_ctx`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.h` (lines 1002..1058, 57 LoC body)
//! v4 manifest sub-unit: `ha_rocksdb_h__Rdb_inplace_alter_ctx`
//! parent: `ha_rocksdb_h`
//!
//! ## Mapping
//! Inplace-ALTER carries this struct between the four alter callbacks
//! (`check_if_supported`, `prepare`, `inplace`, `commit`). It records the
//! old/new key definitions, the added/dropped index sets, and the running
//! max auto-increment value at the start of the alter.
//!
//! Per _DESIGN.md §1, dropped indexes are swept by SlateDB's
//! `CompactionFilter` (feature `compaction_filters`) — the dropped-index id
//! set in this context is what `Rdb_compact_filter` reads to decide which
//! keys to `Drop` during the next compaction.
//!
//! ## Out-of-scope methods
//! None — pure data carrier (no methods beyond constructor / destructor).

use std::collections::HashSet;
use std::sync::Arc;

use crate::rdb_global_h::GlIndexId;

// Forward decls — these live in separate units of rdb_datadic_h.
// Cited here so the dependency intent is visible; concrete types come from
// `rdb_datadic_h__Rdb_tbl_def.rs` and `rdb_datadic_h__Rdb_key_def.rs`.
pub trait TblDefRef: Send + Sync {}
pub trait KeyDefRef: Send + Sync {}

/// Inplace-alter scratch context. Created in `prepare_inplace_alter_table`
/// (`ha_rocksdb_cc__ha_rocksdb__alter.rs`), threaded through
/// `inplace_alter_table` and `commit_inplace_alter_table`, dropped at the
/// end of the alter sequence.
///
/// Original: ha_rocksdb.h:1002 — `struct Rdb_inplace_alter_ctx`.
pub struct RdbInplaceAlterCtx {
    /// New post-alter table definition.
    /// Original: ha_rocksdb.h:1004 — `Rdb_tbl_def *const m_new_tdef`.
    pub new_tdef: Arc<dyn TblDefRef>,

    /// Snapshot of the pre-alter key definitions (frozen at prepare time).
    /// Original: ha_rocksdb.h:1007.
    pub old_key_descrs: Vec<Arc<dyn KeyDefRef>>,

    /// New key definitions assembled during the alter. Populated by
    /// `prepare_inplace_alter_table`; consumed by `commit_inplace_alter_table`.
    /// Original: ha_rocksdb.h:1010.
    pub new_key_descrs: Vec<Arc<dyn KeyDefRef>>,

    /// Indexes added by this alter. Their keys must be backfilled during
    /// `inplace_alter_table` via `inplace_populate_sk`.
    /// Original: ha_rocksdb.h:1019.
    pub added_indexes: HashSet<u64>, // hash of Arc<KeyDef> by ptr id; concrete shape TODO

    /// Indexes dropped by this alter. Their CompactionFilter sweep is queued
    /// at `commit_inplace_alter_table` time. These ids appear in the system
    /// CF's dropped-index registry until compaction has rolled past them.
    /// Original: ha_rocksdb.h:1022.
    pub dropped_index_ids: HashSet<GlIndexId>,

    /// Number of keys to add (cached length of `added_indexes`).
    pub n_added_keys: u32,

    /// Number of keys to drop (cached length of `dropped_index_ids`).
    pub n_dropped_keys: u32,

    /// Max auto-increment value observed at alter start; used to seed the
    /// post-alter auto-incr counter so we never re-issue an existing value.
    /// Original: ha_rocksdb.h:1031.
    pub max_auto_incr: u64,
}

impl RdbInplaceAlterCtx {
    /// Construct from prepare-phase artifacts. All-fields ctor matching the
    /// C++ 9-arg constructor (ha_rocksdb.h:1033).
    pub fn new(
        new_tdef: Arc<dyn TblDefRef>,
        old_key_descrs: Vec<Arc<dyn KeyDefRef>>,
        new_key_descrs: Vec<Arc<dyn KeyDefRef>>,
        added_indexes: HashSet<u64>,
        dropped_index_ids: HashSet<GlIndexId>,
        n_added_keys: u32,
        n_dropped_keys: u32,
        max_auto_incr: u64,
    ) -> Self {
        Self {
            new_tdef,
            old_key_descrs,
            new_key_descrs,
            added_indexes,
            dropped_index_ids,
            n_added_keys,
            n_dropped_keys,
            max_auto_incr,
        }
    }
}
