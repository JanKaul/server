//! Dropped-index compaction filter.
//!
//! Per `_DESIGN.md §1`: when an index is DROPped, MyRocks doesn't
//! immediately walk the keyspace deleting every matching row. Instead it
//! adds the `gl_index_id` to a *dropped-index registry*
//! ([`crate::codec::dict::dropped_indexes`]) and relies on the compaction
//! filter to sweep the leftover rows the next time their SSTs are
//! compacted.
//!
//! Translated from `rdb_compact_filter.h`. The C++ implements
//! `rocksdb::CompactionFilter` and `rocksdb::CompactionFilterFactory`;
//! SlateDB's equivalents are [`slatedb::CompactionFilter`] and
//! [`slatedb::CompactionFilterSupplier`], so we implement those directly.
//!
//! ## Per-job snapshot semantic
//!
//! [`EngineCompactFilterSupplier::create_compaction_filter`] loads the
//! dropped-index set once at job-start and hands it to the filter
//! instance for the duration of that compaction job. A DROP INDEX that
//! happens mid-compaction won't be swept until the next job — matches
//! the C++ behaviour and avoids racing the supplier against the
//! registry.
//!
//! ## TTL handling
//!
//! SlateDB has native `expire_ts` support; TTL filtering is automatic and
//! not the compaction filter's concern (`_DESIGN.md §1`). The filter
//! only handles dropped-index sweep.
//!
//! ## "Keep on parse failure" policy
//!
//! Keys that don't decode under [`crate::codec::prefix::parse_key_prefix`]
//! (no varint cf_id + u32_be index_id head) are kept, not dropped. Our
//! codec writes only well-formed keys, so an unparseable key is either
//! a future format we don't know about or a bug — neither is a reason
//! to silently delete user data. The C++ implicitly relies on the same
//! invariant; we make the policy explicit.

use slatedb::{
    CompactionFilter, CompactionFilterDecision, CompactionFilterError,
    CompactionFilterSupplier, CompactionJobContext, Db, RowEntry,
};
use std::collections::HashSet;
use std::sync::{Arc, OnceLock, Weak};

use crate::codec::dict;
use crate::codec::prefix::parse_key_prefix;
use crate::globals::{GlIndexId, SYSTEM_CF_ID};

/// Per-job filter instance. Holds the dropped-index snapshot frozen at
/// job-start so the decision is deterministic across the whole job.
pub struct EngineCompactFilter {
    /// Snapshot of dropped indexes at job-start. Frozen for the job.
    dropped_indexes: HashSet<GlIndexId>,
}

impl EngineCompactFilter {
    /// Construct from an explicit set. Used by the supplier and by
    /// tests that don't want to spin up the dict.
    pub fn new(dropped_indexes: HashSet<GlIndexId>) -> Self {
        Self { dropped_indexes }
    }

    /// Inspect one key, return the decision.
    fn decide(&self, key: &[u8]) -> CompactionFilterDecision {
        let Some(parsed) = parse_key_prefix(key) else {
            // Unparseable → keep (see module doc).
            return CompactionFilterDecision::Keep;
        };
        // System-CF keys are never dropped by this filter — dropped-index
        // bookkeeping itself lives there and dropping it would cause us
        // to lose track of in-flight drops.
        if parsed.cf_id == SYSTEM_CF_ID {
            return CompactionFilterDecision::Keep;
        }
        let gl = GlIndexId {
            cf_id: parsed.cf_id,
            index_id: parsed.index_id,
        };
        if self.dropped_indexes.contains(&gl) {
            CompactionFilterDecision::Drop
        } else {
            CompactionFilterDecision::Keep
        }
    }
}

#[async_trait::async_trait]
impl CompactionFilter for EngineCompactFilter {
    async fn filter(
        &mut self,
        entry: &RowEntry,
    ) -> Result<CompactionFilterDecision, CompactionFilterError> {
        Ok(self.decide(&entry.key))
    }

    async fn on_compaction_end(&mut self) -> Result<(), CompactionFilterError> {
        // No-op today; a future commit could emit a "swept N keys for
        // indexes [...]" metric here.
        Ok(())
    }
}

/// Per-engine supplier. Holds the `Db` handle as a `Weak<Db>` to avoid
/// the Arc cycle that would otherwise form: `Arc<Db> → Compactor →
/// Arc<dyn CompactionFilterSupplier> → Arc<Db>`.
///
/// ## Lifecycle
///
/// The supplier is built *before* the `Db` exists (because the
/// `CompactorBuilder` consumes it at `DbBuilder` time). Construction
/// goes through [`new_deferred`](Self::new_deferred); the `Db` is
/// wired in afterwards via [`install_db`](Self::install_db).
///
/// If `create_compaction_filter` fires before `install_db` (a wiring
/// bug) or after the `Db` has been dropped, we return
/// `CompactionFilterError::CreationError` with a clear message — the
/// compaction job aborts rather than running with an empty drop set
/// (which would silently leak dropped-index data).
pub struct EngineCompactFilterSupplier {
    db: OnceLock<Weak<Db>>,
}

impl EngineCompactFilterSupplier {
    /// Build the supplier without a `Db` reference yet. Call
    /// [`install_db`](Self::install_db) once the `Db` is open.
    pub fn new_deferred() -> Arc<Self> {
        Arc::new(Self {
            db: OnceLock::new(),
        })
    }

    /// Wire the freshly-opened `Db` into the supplier. Subsequent
    /// `create_compaction_filter` calls will read the dropped-index
    /// registry through this handle.
    ///
    /// Stores a `Weak<Db>` to break the cycle. Calling twice is a
    /// programmer bug and is silently ignored (the first wins) —
    /// `OnceLock::set` returns `Err`, which we drop.
    pub fn install_db(&self, db: &Arc<Db>) {
        let _ = self.db.set(Arc::downgrade(db));
    }
}

#[async_trait::async_trait]
impl CompactionFilterSupplier for EngineCompactFilterSupplier {
    async fn create_compaction_filter(
        &self,
        _context: &CompactionJobContext,
    ) -> Result<Box<dyn CompactionFilter>, CompactionFilterError> {
        let weak = self.db.get().ok_or_else(|| {
            CompactionFilterError::CreationError(Box::new(std::io::Error::other(
                "compact_filter: supplier db not installed (call install_db before open)",
            )))
        })?;
        let db = weak.upgrade().ok_or_else(|| {
            CompactionFilterError::CreationError(Box::new(std::io::Error::other(
                "compact_filter: db has been dropped",
            )))
        })?;
        let listed = dict::dropped_indexes::list(&db).await.map_err(|e| {
            CompactionFilterError::CreationError(Box::new(std::io::Error::other(format!(
                "compact_filter: load dropped_indexes: {e}"
            ))))
        })?;
        let set: HashSet<GlIndexId> = listed.into_iter().collect();
        Ok(Box::new(EngineCompactFilter::new(set)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::prefix::build_key_prefix;
    use crate::engine::db::EngineDb;
    use bytes::Bytes;
    use slatedb::ValueDeletable;

    fn row(key: Bytes) -> RowEntry {
        RowEntry {
            key,
            value: ValueDeletable::Value(Bytes::from_static(b"v")),
            seq: 1,
            create_ts: None,
            expire_ts: None,
        }
    }

    fn user_key(cf_id: u32, index_id: u32, suffix: &[u8]) -> Bytes {
        let mut bytes = Vec::from(&build_key_prefix(cf_id, index_id)[..]);
        bytes.extend_from_slice(suffix);
        Bytes::from(bytes)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn empty_dropped_set_keeps_every_key() {
        let f = EngineCompactFilter::new(HashSet::new());
        let mut f = f;
        let entry = row(user_key(1, 100, b"row"));
        assert!(matches!(
            f.filter(&entry).await.expect("ok"),
            CompactionFilterDecision::Keep
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropped_index_match_drops_the_row() {
        let dropped = HashSet::from([GlIndexId { cf_id: 1, index_id: 100 }]);
        let mut f = EngineCompactFilter::new(dropped);
        let entry = row(user_key(1, 100, b"row"));
        assert!(matches!(
            f.filter(&entry).await.expect("ok"),
            CompactionFilterDecision::Drop
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn non_matching_index_is_kept() {
        let dropped = HashSet::from([GlIndexId { cf_id: 1, index_id: 100 }]);
        let mut f = EngineCompactFilter::new(dropped);

        // Different index_id.
        let other_idx = row(user_key(1, 101, b"row"));
        assert!(matches!(
            f.filter(&other_idx).await.expect("ok"),
            CompactionFilterDecision::Keep
        ));
        // Different cf_id.
        let other_cf = row(user_key(2, 100, b"row"));
        assert!(matches!(
            f.filter(&other_cf).await.expect("ok"),
            CompactionFilterDecision::Keep
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn system_cf_keys_are_never_dropped() {
        // Even if (somehow) we'd added a system-CF entry to the dropped
        // set, the filter refuses to drop it — system-CF state is what
        // tracks the drop set itself.
        let sys = GlIndexId {
            cf_id: SYSTEM_CF_ID,
            index_id: 7,
        };
        let dropped = HashSet::from([sys]);
        let mut f = EngineCompactFilter::new(dropped);
        let entry = row(user_key(SYSTEM_CF_ID, 7, b"x"));
        assert!(matches!(
            f.filter(&entry).await.expect("ok"),
            CompactionFilterDecision::Keep
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unparseable_key_is_kept() {
        let mut f = EngineCompactFilter::new(HashSet::new());
        // Empty key — varint decode fails.
        assert!(matches!(
            f.filter(&row(Bytes::new())).await.expect("ok"),
            CompactionFilterDecision::Keep
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn supplier_freezes_snapshot_against_live_registry() {
        let engine = EngineDb::open_in_memory("cf_supplier_freeze")
            .await
            .expect("open");
        let supplier = EngineCompactFilterSupplier::new_deferred();
        supplier.install_db(engine.db());

        // Start with an empty registry → supplier produces a filter that drops nothing.
        let ctx = CompactionJobContext {
            destination: 0,
            is_dest_last_run: false,
            compaction_clock_tick: 0,
            retention_min_seq: None,
        };
        let mut filter = supplier.create_compaction_filter(&ctx).await.expect("ok");
        let row1 = row(user_key(1, 100, b"row"));
        assert!(matches!(
            filter.filter(&row1).await.expect("ok"),
            CompactionFilterDecision::Keep
        ));

        // After taking the snapshot, add an index to the registry —
        // the previously-built filter shouldn't see it.
        dict::dropped_indexes::add(
            engine.db(),
            GlIndexId { cf_id: 1, index_id: 100 },
        )
        .await
        .expect("add");
        // Old filter must not see the post-snapshot drop.
        assert!(matches!(
            filter.filter(&row1).await.expect("ok"),
            CompactionFilterDecision::Keep
        ));

        // But a fresh filter built now sees the drop.
        let mut filter2 = supplier.create_compaction_filter(&ctx).await.expect("ok");
        assert!(matches!(
            filter2.filter(&row1).await.expect("ok"),
            CompactionFilterDecision::Drop
        ));

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn supplier_without_install_db_errors_clearly() {
        let supplier = EngineCompactFilterSupplier::new_deferred();
        let ctx = CompactionJobContext {
            destination: 0,
            is_dest_last_run: false,
            compaction_clock_tick: 0,
            retention_min_seq: None,
        };
        let result = supplier.create_compaction_filter(&ctx).await;
        let err = result.err().expect("expected error, got Ok");
        let msg = err.to_string();
        assert!(
            msg.contains("supplier db not installed"),
            "expected 'not installed' message; got: {msg}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn supplier_after_db_drop_errors_clearly() {
        let supplier = EngineCompactFilterSupplier::new_deferred();
        {
            let engine = EngineDb::open_in_memory("cf_supplier_drop")
                .await
                .expect("open");
            supplier.install_db(engine.db());
            engine.close().await.expect("close");
            // engine drops here; the Weak<Db> should fail to upgrade.
        }
        let ctx = CompactionJobContext {
            destination: 0,
            is_dest_last_run: false,
            compaction_clock_tick: 0,
            retention_min_seq: None,
        };
        let result = supplier.create_compaction_filter(&ctx).await;
        let err = result.err().expect("expected error, got Ok");
        let msg = err.to_string();
        assert!(
            msg.contains("db has been dropped"),
            "expected 'dropped' message; got: {msg}"
        );
    }
}
