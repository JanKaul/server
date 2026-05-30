//! SlateDB `Db` wrapper.
//!
//! Per `_DESIGN.md §8` the engine wraps `slatedb::Db` with our
//! [`crate::codec::prefix::MyRocksPrefixExtractor`] wired into the
//! `DbBuilder::with_segment_extractor` slot so per-CF / per-index bloom
//! filtering works.
//!
//! Construction is async (SlateDB's builder reads the manifest before
//! `build()` returns); call sites bridge through
//! [`crate::runtime::EngineRuntime::block_on`].

use object_store::ObjectStore;
use slatedb::{Db, DbBuilder};
use slatedb::Error;
use std::sync::Arc;

use crate::codec::prefix::MyRocksPrefixExtractor;

/// Wrapper around an open `slatedb::Db` instance. Holds the segment
/// extractor as a separate `Arc` so other engine code (e.g. the bloom
/// filter / per-index scan paths) can reach the same extractor instance.
#[derive(Clone)]
pub struct EngineDb {
    db: Arc<Db>,
    extractor: Arc<MyRocksPrefixExtractor>,
}

impl EngineDb {
    /// Open the database at `path` against `object_store`. The
    /// `MyRocksPrefixExtractor` is wired in automatically so all SST builds
    /// hash the `varint(cf_id) || u32_be(index_id)` prefix into the bloom
    /// filter.
    pub async fn open(
        path: impl Into<object_store::path::Path>,
        object_store: Arc<dyn ObjectStore>,
    ) -> Result<Self, Error> {
        let extractor = Arc::new(MyRocksPrefixExtractor);
        let db = DbBuilder::new(path, object_store)
            .with_segment_extractor(Arc::clone(&extractor) as Arc<dyn slatedb::PrefixExtractor>)
            .build()
            .await?;
        Ok(Self {
            db: Arc::new(db),
            extractor,
        })
    }

    /// Convenience for tests: open against an in-memory object store rooted
    /// at `name`. Each call creates a fresh store, so tests don't share
    /// state.
    pub async fn open_in_memory(name: &str) -> Result<Self, Error> {
        let object_store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());
        Self::open(object_store::path::Path::from(name), object_store).await
    }

    /// Underlying `slatedb::Db` handle for direct use by hot paths.
    pub fn db(&self) -> &Arc<Db> {
        &self.db
    }

    /// The same extractor instance that was passed to `DbBuilder`. Other
    /// engine code that needs to compute prefixes (scan-range trimming, key
    /// inspection in tests) should use this one to stay consistent.
    pub fn extractor(&self) -> &Arc<MyRocksPrefixExtractor> {
        &self.extractor
    }

    /// Close the database cleanly. Idempotent at the SlateDB level.
    pub async fn close(&self) -> Result<(), Error> {
        self.db.close().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn open_close_round_trip() {
        let engine = EngineDb::open_in_memory("test_open_close")
            .await
            .expect("open");
        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn put_then_get_round_trip_with_prefix_extractor() {
        let engine = EngineDb::open_in_memory("test_put_get")
            .await
            .expect("open");

        // Build a key with our standard prefix so the segment extractor
        // sees a parseable head.
        let mut key = Vec::from(&crate::codec::prefix::build_key_prefix(7, 42)[..]);
        key.extend_from_slice(b"row1");

        engine
            .db()
            .put(&key, b"hello")
            .await
            .expect("put");
        let got = engine
            .db()
            .get(&key)
            .await
            .expect("get")
            .expect("present");
        assert_eq!(got, Bytes::from_static(b"hello"));

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn extractor_is_shared_with_builder() {
        let engine = EngineDb::open_in_memory("test_extractor_share")
            .await
            .expect("open");
        // The Arc we hold is the same one the builder consumed (refcount > 1).
        assert!(Arc::strong_count(engine.extractor()) >= 2);
        engine.close().await.expect("close");
    }
}
