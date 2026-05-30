//! Read-only snapshot wrapper.
//!
//! Wraps `Arc<slatedb::DbSnapshot>` for read paths that want a frozen view
//! of the keyspace at a specific sequence number. Use cases:
//! - `SHOW ENGINE STATUS` aggregation that must see a consistent state
//!   even while writers are committing.
//! - Dump threads / `SHOW BINLOG EVENTS` style readers that anchor at a
//!   point in time and tolerate seeing nothing committed afterwards.
//! - Read-only handlers that don't need SSI tracking.
//!
//! Snapshots are cheap to take and cheap to clone (`Arc` internally).
//! Reads through an [`EngineSnapshot`] never see writes committed after
//! the snapshot was taken.

use bytes::Bytes;
use slatedb::{DbIterator, DbSnapshot, Error};
use std::sync::Arc;

use crate::engine::db::EngineDb;

/// Frozen read view of the engine. Cloneable.
#[derive(Clone)]
pub struct EngineSnapshot {
    inner: Arc<DbSnapshot>,
}

impl EngineSnapshot {
    fn wrap(inner: Arc<DbSnapshot>) -> Self {
        Self { inner }
    }

    /// SlateDB sequence number this snapshot reads at.
    pub fn seq(&self) -> u64 {
        self.inner.seq()
    }

    /// Borrow the underlying snapshot for SlateDB-only APIs we haven't
    /// surfaced (scan ranges, `scan_with_options`, …).
    pub fn raw(&self) -> &DbSnapshot {
        &self.inner
    }

    pub async fn get(&self, key: &[u8]) -> Result<Option<Bytes>, Error> {
        self.inner.get(key).await
    }

    pub async fn scan_prefix(&self, prefix: &[u8]) -> Result<DbIterator, Error> {
        self.inner.scan_prefix(prefix).await
    }
}

impl EngineDb {
    /// Take a snapshot at the current durable sequence number.
    pub async fn snapshot(&self) -> Result<EngineSnapshot, Error> {
        let inner = self.db().snapshot().await?;
        Ok(EngineSnapshot::wrap(inner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn snapshot_sees_pre_existing_committed_writes() {
        let engine = EngineDb::open_in_memory("snap_pre")
            .await
            .expect("open");
        let mut t = engine.begin_default().await.expect("begin");
        t.put(b"k", b"v").expect("put");
        t.commit().await.expect("commit");

        let snap = engine.snapshot().await.expect("snapshot");
        let got = snap.get(b"k").await.expect("get").expect("present");
        assert_eq!(&got[..], b"v");

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn snapshot_does_not_see_writes_committed_after_it() {
        let engine = EngineDb::open_in_memory("snap_post")
            .await
            .expect("open");

        // Seed at seq S0.
        let mut t = engine.begin_default().await.expect("begin");
        t.put(b"k", b"old").expect("put");
        t.commit().await.expect("commit");

        // Snapshot here.
        let snap = engine.snapshot().await.expect("snapshot");
        let seq_before = snap.seq();

        // New writes after the snapshot.
        let mut t = engine.begin_default().await.expect("begin");
        t.put(b"k", b"new").expect("put");
        t.put(b"k2", b"fresh").expect("put");
        t.commit().await.expect("commit");

        // Snapshot still reads the old value, doesn't see the new key.
        let got = snap.get(b"k").await.expect("get").expect("present");
        assert_eq!(&got[..], b"old");
        assert!(snap.get(b"k2").await.expect("get").is_none());
        assert_eq!(snap.seq(), seq_before);

        // A fresh snapshot sees both.
        let snap2 = engine.snapshot().await.expect("snapshot");
        assert_eq!(
            &snap2.get(b"k").await.expect("get").expect("present")[..],
            b"new"
        );
        assert_eq!(
            &snap2.get(b"k2").await.expect("get").expect("present")[..],
            b"fresh"
        );
        assert!(snap2.seq() >= snap.seq());

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn snapshot_clone_shares_view() {
        let engine = EngineDb::open_in_memory("snap_clone")
            .await
            .expect("open");
        let mut t = engine.begin_default().await.expect("begin");
        t.put(b"k", b"v").expect("put");
        t.commit().await.expect("commit");

        let s1 = engine.snapshot().await.expect("snapshot");
        let s2 = s1.clone();
        assert_eq!(s1.seq(), s2.seq());
        assert_eq!(
            s1.get(b"k").await.expect("get"),
            s2.get(b"k").await.expect("get")
        );

        engine.close().await.expect("close");
    }
}
