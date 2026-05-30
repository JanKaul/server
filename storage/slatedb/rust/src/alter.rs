//! Inplace `ALTER TABLE` state carriers.
//!
//! Translated from `Rdb_inplace_alter_ctx` (`ha_rocksdb.h:1002..1058`).
//!
//! MariaDB inplace ALTER runs through four handler callbacks:
//! `check_if_supported_inplace_alter`, `prepare_inplace_alter_table`,
//! `inplace_alter_table`, and `commit_inplace_alter_table`. State that
//! needs to flow between those phases lives in this struct: the new
//! table definition, the snapshots of the pre/post-alter key lists, the
//! set of indexes being added (for backfill) or dropped (for the
//! compaction-filter sweep registry), and the running max auto-increment
//! observed at prepare-time.
//!
//! ## Why we can build it now
//!
//! `Rdb_inplace_alter_ctx` was previously blocked on `TblDef` being a
//! real Rust type — see [[no-cargo-cult-abstractions]] in the project
//! memory for why we wouldn't land an `Arc<dyn TblDefRef>` empty-trait
//! placeholder. With [`crate::codec::tbl_def::TblDef`] landed, every
//! field has a real concrete type and the struct is honest.
//!
//! ## Shape differences from the C++
//!
//! - `m_added_indexes` is `Vec<Arc<KeyDef>>` instead of
//!   `unordered_set<shared_ptr<KeyDef>>`. The C++ set is iterated
//!   only — no `count()` calls — so set semantics are unused; `Vec`
//!   matches usage. Length asserts in the C++ (`added_indexes.size()
//!   == n_added_keys`) are preserved naturally because we drop the
//!   redundant counter fields.
//! - `m_old_n_keys` / `m_new_n_keys` / `m_n_added_keys` /
//!   `m_n_dropped_keys` are dropped — `Vec::len()` and `HashSet::len()`
//!   are O(1) and equally cheap.
//! - The struct is `pub` field-level access for now since every
//!   consumer is in the as-yet-untranslated handler-bucket cluster
//!   (`ha_rocksdb_cc__ha_rocksdb__alter.rs`); methods will be added
//!   alongside those when they land.

use std::collections::HashSet;
use std::sync::Arc;

use crate::codec::key::KeyDef;
use crate::codec::tbl_def::TblDef;
use crate::globals::GlIndexId;

/// State threaded across the four inplace-ALTER callbacks. Lifetime is
/// bounded by a single ALTER TABLE statement; constructed in
/// `prepare_inplace_alter_table`, dropped in
/// `commit_inplace_alter_table` (success path) or
/// `rollback_inplace_alter_table`.
///
/// Original: `ha_rocksdb.h:1002` — `struct Rdb_inplace_alter_ctx`.
pub struct RdbInplaceAlterCtx {
    /// The post-ALTER table definition. Becomes the handler's
    /// `m_tbl_def` on commit.
    pub new_tdef: Arc<TblDef>,

    /// Pre-ALTER key list, snapshotted at `prepare` time. Used by
    /// `commit_inplace_alter_table` to identify which old indexes
    /// became "dropped" and need their data swept by the compaction
    /// filter.
    pub old_key_descrs: Vec<Arc<KeyDef>>,

    /// Post-ALTER key list, assembled during `prepare`. Spelled out as
    /// well as referenced via `new_tdef.key_descrs()` — the two stay in
    /// sync per the C++ contract (`new_tdef->m_key_descr_arr` is
    /// initialised from the same buffer in
    /// `prepare_inplace_alter_table`, `ha_rocksdb.cc:12571`).
    pub new_key_descrs: Vec<Arc<KeyDef>>,

    /// Indexes added by this ALTER. Backfilled by
    /// `inplace_populate_sk` during the `inplace_alter_table` phase.
    /// Each entry's `Arc<KeyDef>` is the same handle that appears in
    /// `new_key_descrs` (and `new_tdef.key_descrs`).
    pub added_indexes: Vec<Arc<KeyDef>>,

    /// Indexes dropped by this ALTER. Written to the dict's
    /// dropped-index registry at `commit_inplace_alter_table` time so
    /// the [`crate::engine::compact_filter`] sweep picks them up.
    pub dropped_index_ids: HashSet<GlIndexId>,

    /// Largest auto-increment value observed at the start of the alter.
    /// Used to seed the post-alter counter so we never re-issue an
    /// existing value. C++ field `m_max_auto_incr`
    /// (`ha_rocksdb.h:1031`); set from `load_auto_incr_value_from_index`
    /// when the alter changes a CREATE option (`ha_rocksdb.cc:12655`).
    pub max_auto_incr: u64,
}

impl RdbInplaceAlterCtx {
    /// Builder matching the C++ 9-arg constructor
    /// (`ha_rocksdb.h:1033`), with the redundant length parameters
    /// dropped (see module docs).
    pub fn new(
        new_tdef: Arc<TblDef>,
        old_key_descrs: Vec<Arc<KeyDef>>,
        new_key_descrs: Vec<Arc<KeyDef>>,
        added_indexes: Vec<Arc<KeyDef>>,
        dropped_index_ids: HashSet<GlIndexId>,
        max_auto_incr: u64,
    ) -> Self {
        Self {
            new_tdef,
            old_key_descrs,
            new_key_descrs,
            added_indexes,
            dropped_index_ids,
            max_auto_incr,
        }
    }

    /// Number of indexes the alter is adding. C++ `m_n_added_keys`.
    pub fn n_added_keys(&self) -> usize {
        self.added_indexes.len()
    }

    /// Number of indexes the alter is dropping. C++ `m_n_dropped_keys`.
    pub fn n_dropped_keys(&self) -> usize {
        self.dropped_index_ids.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::key::{
        IndexType, INDEX_INFO_VERSION_LATEST, PRIMARY_FORMAT_VERSION_LATEST,
        SECONDARY_FORMAT_VERSION_LATEST,
    };

    fn pk(index_number: u32, cf_id: u32) -> Arc<KeyDef> {
        Arc::new(KeyDef::new_skeleton(
            index_number,
            cf_id,
            0,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Primary,
            PRIMARY_FORMAT_VERSION_LATEST,
            false,
            "pk",
        ))
    }

    fn sk(index_number: u32, cf_id: u32, name: &str) -> Arc<KeyDef> {
        Arc::new(KeyDef::new_skeleton(
            index_number,
            cf_id,
            1,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Secondary,
            SECONDARY_FORMAT_VERSION_LATEST,
            false,
            name,
        ))
    }

    fn tbl(keys: Vec<Arc<KeyDef>>) -> Arc<TblDef> {
        Arc::new(TblDef::new("db.t").unwrap().with_keys(keys))
    }

    #[test]
    fn ctx_round_trip_carries_all_fields() {
        let old_pk = pk(10, 1);
        let old_sk = sk(11, 1, "old_sk");
        let new_pk = pk(10, 1);
        let new_sk = sk(12, 1, "new_sk");
        let added = vec![new_sk.clone()];
        let mut dropped = HashSet::new();
        dropped.insert(GlIndexId {
            cf_id: 1,
            index_id: 11,
        });

        let new_tdef = tbl(vec![new_pk.clone(), new_sk.clone()]);
        let ctx = RdbInplaceAlterCtx::new(
            new_tdef.clone(),
            vec![old_pk.clone(), old_sk.clone()],
            vec![new_pk.clone(), new_sk.clone()],
            added.clone(),
            dropped.clone(),
            42,
        );

        assert_eq!(ctx.new_tdef.key_count(), 2);
        assert_eq!(ctx.old_key_descrs.len(), 2);
        assert_eq!(ctx.new_key_descrs.len(), 2);
        assert_eq!(ctx.added_indexes.len(), 1);
        assert_eq!(
            ctx.added_indexes[0].get_index_number(),
            new_sk.get_index_number(),
        );
        assert!(ctx.dropped_index_ids.contains(&GlIndexId {
            cf_id: 1,
            index_id: 11,
        }));
        assert_eq!(ctx.max_auto_incr, 42);
    }

    #[test]
    fn n_added_keys_and_n_dropped_keys_match_collection_len() {
        let ctx = RdbInplaceAlterCtx::new(
            tbl(vec![pk(1, 0)]),
            vec![pk(1, 0)],
            vec![pk(1, 0)],
            vec![sk(2, 0, "a"), sk(3, 0, "b"), sk(4, 0, "c")],
            HashSet::from([
                GlIndexId {
                    cf_id: 0,
                    index_id: 5,
                },
                GlIndexId {
                    cf_id: 0,
                    index_id: 6,
                },
            ]),
            0,
        );
        assert_eq!(ctx.n_added_keys(), 3);
        assert_eq!(ctx.n_dropped_keys(), 2);
    }

    #[test]
    fn new_tdef_arc_is_shareable() {
        let table = tbl(vec![pk(10, 1)]);
        let ctx = RdbInplaceAlterCtx::new(
            table.clone(),
            vec![],
            vec![pk(10, 1)],
            vec![],
            HashSet::new(),
            0,
        );
        // Outside reference + inside reference both held; this just
        // confirms the Arc story doesn't accidentally clone the TblDef.
        assert!(Arc::ptr_eq(&ctx.new_tdef, &table));
    }

    #[test]
    fn ctx_is_send() {
        // Inplace-alter callbacks may move the ctx between threads via
        // the handler's `handler_ctx` slot.
        fn assert_send<T: Send>() {}
        assert_send::<RdbInplaceAlterCtx>();
    }
}
