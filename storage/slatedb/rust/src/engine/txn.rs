//! Transaction wrapper.
//!
//! Per `_DESIGN.md §5 + §11 Q9`: the data engine commits to SlateDB's
//! `SerializableSnapshot` isolation as the default — full SSI with both
//! write-write and read-write conflict detection at commit time. Callers
//! that explicitly opt down to plain snapshot isolation use
//! [`EngineDb::begin_snapshot`].
//!
//! Per `_DESIGN.md §11 Q10`: MyRocks-style savepoints are **not** implemented
//! in Stage 0. The three savepoint methods return `Err(Invalid)` so the
//! cxx bridge surfaces `HA_ERR_GENERIC` to the SQL layer. Latent
//! savepoint-aware code in the handler buckets is documented at the
//! relevant interface stubs.

use bytes::Bytes;
use slatedb::{DbIterator, DbTransaction, Error, IsolationLevel};

use crate::engine::db::EngineDb;

/// Newtype over `slatedb::DbTransaction`. Adds the savepoint stubs and the
/// SSI-default begin path. Read/write/commit/rollback are simple
/// pass-throughs.
pub struct EngineTxn {
    inner: DbTransaction,
}

impl EngineTxn {
    fn wrap(inner: DbTransaction) -> Self {
        Self { inner }
    }

    /// Borrow the underlying transaction. Useful when callers want to use
    /// SlateDB-only APIs (scans, mark_read, merge) that we haven't
    /// surfaced yet.
    pub fn raw(&self) -> &DbTransaction {
        &self.inner
    }

    pub fn raw_mut(&mut self) -> &mut DbTransaction {
        &mut self.inner
    }

    pub async fn get(&self, key: &[u8]) -> Result<Option<Bytes>, Error> {
        self.inner.get(key).await
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), Error> {
        self.inner.put(key, value)
    }

    pub fn delete(&mut self, key: &[u8]) -> Result<(), Error> {
        self.inner.delete(key)
    }

    /// Open a prefix-bounded iterator scoped to this transaction's
    /// snapshot. The returned `DbIterator` reads the keyspace as
    /// of the txn's begin sequence — so two `scan_prefix` calls on
    /// the same txn see the same data, and a `scan_prefix` here
    /// is SSI-consistent with this txn's `get` / `put` /
    /// `delete` ops at commit time.
    ///
    /// Counterpart of [`crate::engine::db::EngineDb::scan_prefix`]
    /// (live-engine view, no txn snapshot). The read path
    /// (`rnd_init` / `index_read`) should prefer this one when an
    /// active transaction exists.
    ///
    /// `prefix` is typically the index prefix produced by
    /// [`crate::codec::key::KeyDef::get_infimum_key`]
    /// (`varint(cf_id) || u32_be(index_number)`).
    pub async fn scan_prefix(&self, prefix: &[u8]) -> Result<DbIterator, Error> {
        self.inner.scan_prefix(prefix).await
    }

    /// Commit. Returns the SSI conflict as `slatedb::Error` with
    /// `ErrorKind::Transaction` (mapped to `HA_ERR_LOCK_DEADLOCK` by
    /// `error::slatedb_error_to_ha_err`).
    pub async fn commit(self) -> Result<(), Error> {
        self.inner.commit().await.map(|_handle| ())
    }

    /// Roll back. Consumes the txn; no return value (always succeeds
    /// per SlateDB contract — the txn just drops its buffered writes).
    pub fn rollback(self) {
        self.inner.rollback();
    }

    pub fn seqnum(&self) -> u64 {
        self.inner.seqnum()
    }

    // --- Savepoint stubs (Q10) ---

    /// Stage 0 stub. Returns `Err(Invalid)` so the bridge surfaces
    /// `HA_ERR_GENERIC`. Will route to a real savepoint stack once
    /// implemented.
    pub fn savepoint(&self, _name: &str) -> Result<(), Error> {
        Err(Error::invalid(
            "savepoint not supported in Stage 0 (Q10)".into(),
        ))
    }

    /// Stage 0 stub. See `savepoint`.
    pub fn rollback_to_savepoint(&self, _name: &str) -> Result<(), Error> {
        Err(Error::invalid(
            "rollback_to_savepoint not supported in Stage 0 (Q10)".into(),
        ))
    }

    /// Stage 0 stub. See `savepoint`. MyRocks ignored release; we keep that
    /// behaviour by returning `Ok(())` rather than erroring — a release on
    /// a non-existent savepoint is a no-op even in real implementations.
    pub fn release_savepoint(&self, _name: &str) -> Result<(), Error> {
        Ok(())
    }
}

impl EngineDb {
    /// Begin a transaction at `SerializableSnapshot` — the engine's default
    /// per `_DESIGN.md §11 Q9`.
    pub async fn begin_default(&self) -> Result<EngineTxn, Error> {
        self.db()
            .begin(IsolationLevel::SerializableSnapshot)
            .await
            .map(EngineTxn::wrap)
    }

    /// Begin a transaction at plain `Snapshot` (write-write conflicts only).
    /// Use only for paths that explicitly opt down — most callers want
    /// [`begin_default`].
    pub async fn begin_snapshot(&self) -> Result<EngineTxn, Error> {
        self.db()
            .begin(IsolationLevel::Snapshot)
            .await
            .map(EngineTxn::wrap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn put_commit_then_read_in_new_txn() {
        let engine = EngineDb::open_in_memory("txn_put_commit").await.expect("open");

        let mut t1 = engine.begin_default().await.expect("begin");
        t1.put(b"k1", b"v1").expect("put");
        t1.commit().await.expect("commit");

        let t2 = engine.begin_default().await.expect("begin");
        let v = t2.get(b"k1").await.expect("get").expect("present");
        assert_eq!(&v[..], b"v1");
        t2.rollback();

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ssi_detects_write_write_conflict_on_second_commit() {
        let engine = EngineDb::open_in_memory("txn_ssi_ww").await.expect("open");

        let mut a = engine.begin_default().await.expect("begin a");
        let mut b = engine.begin_default().await.expect("begin b");
        a.put(b"shared", b"from_a").expect("a put");
        b.put(b"shared", b"from_b").expect("b put");

        a.commit().await.expect("a commits");
        let err = b.commit().await.unwrap_err();
        assert!(
            matches!(err.kind(), slatedb::ErrorKind::Transaction),
            "expected Transaction conflict, got {err:?}"
        );

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rollback_discards_buffered_writes() {
        let engine = EngineDb::open_in_memory("txn_rollback").await.expect("open");

        let mut t = engine.begin_default().await.expect("begin");
        t.put(b"k", b"v").expect("put");
        t.rollback();

        let t2 = engine.begin_default().await.expect("begin");
        assert!(t2.get(b"k").await.expect("get").is_none());
        t2.rollback();

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn savepoint_returns_q10_stub_error() {
        let engine = EngineDb::open_in_memory("txn_savepoint").await.expect("open");
        let t = engine.begin_default().await.expect("begin");

        let err = t.savepoint("sp1").unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Invalid));
        let err = t.rollback_to_savepoint("sp1").unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Invalid));
        // Release is a no-op (matches MyRocks semantics).
        t.release_savepoint("sp1").expect("release is no-op");

        t.rollback();
        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn txn_scan_prefix_sees_committed_data_under_prefix() {
        let engine = EngineDb::open_in_memory("txn_scan_basic")
            .await
            .expect("open");

        let prefix = crate::codec::prefix::build_key_prefix(7, 100);

        // Seed three rows under the prefix.
        let mut seed = engine.begin_default().await.expect("begin seed");
        for suffix in [&[0u8][..], &[1u8][..], &[2u8][..]] {
            let mut k = prefix.to_vec();
            k.extend_from_slice(suffix);
            seed.put(&k, &[42u8]).expect("put");
        }
        seed.commit().await.expect("commit seed");

        // Open a reader txn and scan.
        let reader = engine.begin_default().await.expect("begin reader");
        let mut it = reader.scan_prefix(&prefix).await.expect("scan");
        let mut count = 0;
        while let Some(_kv) = it.next().await.expect("next") {
            count += 1;
        }
        assert_eq!(count, 3);

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn txn_scan_prefix_does_not_see_writes_committed_after_begin() {
        // SSI snapshot semantics: a scan via a txn sees data as of
        // its begin sequence, NOT writes committed after.
        let engine = EngineDb::open_in_memory("txn_scan_snapshot")
            .await
            .expect("open");

        let prefix = crate::codec::prefix::build_key_prefix(7, 100);

        // Seed one row.
        let mut seed = engine.begin_default().await.expect("seed");
        {
            let mut k = prefix.to_vec();
            k.push(0x01);
            seed.put(&k, &[42u8]).expect("put");
        }
        seed.commit().await.expect("commit seed");

        // Reader begins (snapshot pinned here).
        let reader = engine.begin_default().await.expect("reader");

        // Writer commits ANOTHER row under the same prefix AFTER the
        // reader's snapshot.
        let mut writer = engine.begin_default().await.expect("writer");
        {
            let mut k = prefix.to_vec();
            k.push(0x02);
            writer.put(&k, &[43u8]).expect("put");
        }
        writer.commit().await.expect("commit writer");

        // Reader still sees only the seed row.
        let mut it = reader.scan_prefix(&prefix).await.expect("scan");
        let mut count = 0;
        while let Some(_kv) = it.next().await.expect("next") {
            count += 1;
        }
        assert_eq!(count, 1, "txn scan must not see post-begin writes");

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn txn_scan_prefix_excludes_other_index_prefixes() {
        // Sibling indexes' rows don't bleed into a scan of THIS
        // index's prefix.
        let engine = EngineDb::open_in_memory("txn_scan_isolation")
            .await
            .expect("open");

        let want_prefix = crate::codec::prefix::build_key_prefix(7, 100);
        let other_prefix = crate::codec::prefix::build_key_prefix(7, 200);

        let mut seed = engine.begin_default().await.expect("begin");
        for (pfx, suffixes) in
            [(&want_prefix[..], &[&[0u8][..]][..]), (&other_prefix[..], &[&[0u8][..], &[1u8][..]][..])]
        {
            for s in suffixes {
                let mut k = pfx.to_vec();
                k.extend_from_slice(s);
                seed.put(&k, &[]).expect("put");
            }
        }
        seed.commit().await.expect("commit");

        let reader = engine.begin_default().await.expect("reader");
        let mut it = reader.scan_prefix(&want_prefix).await.expect("scan");
        let mut count = 0;
        while let Some(_kv) = it.next().await.expect("next") {
            count += 1;
        }
        // Only the single row under want_prefix.
        assert_eq!(count, 1);

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn snapshot_isolation_does_not_detect_rw_conflict() {
        // Plain Snapshot mode allows a read-then-write pattern that SSI
        // would flag. Verify the opt-down path is wired through.
        let engine = EngineDb::open_in_memory("txn_snapshot").await.expect("open");

        // Seed.
        let mut s = engine.begin_default().await.expect("begin seed");
        s.put(b"seed", b"v").expect("put seed");
        s.commit().await.expect("commit seed");

        // Reader and writer overlap. Under Snapshot only W-W matters.
        let r = engine.begin_snapshot().await.expect("begin reader");
        let mut w = engine.begin_snapshot().await.expect("begin writer");
        let _ = r.get(b"seed").await.expect("get");
        w.put(b"other_key", b"new").expect("put");
        w.commit().await.expect("writer commits");
        // Reader writes a different key — no W-W conflict.
        let mut r = r;
        r.put(b"another", b"v").expect("reader writes a fresh key");
        r.commit().await.expect("reader commits");

        engine.close().await.expect("close");
    }
}
