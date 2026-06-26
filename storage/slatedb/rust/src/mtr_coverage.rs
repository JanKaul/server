//! Rust-level mirrors of the MTR test suite in
//! `storage/slatedb/mysql-test/slatedb/t/`.
//!
//! Every test here corresponds to one `.test` file in the MTR suite and
//! exercises the same logical scenario directly against the Rust engine
//! APIs — no MariaDB server, no C++ bridge required.  The MTR tests
//! prove SQL-level correctness through the full stack; these tests
//! prove storage-layer correctness (`EngineTxn::put` → `commit` →
//! `scan_prefix`) using the same storage modules the shim delegates to
//! at runtime.
//!
//! ## Key encoding used in this file
//!
//! `pk_key(index_num, pk_val)` encodes a 4-byte big-endian index prefix
//! followed by a 4-byte sign-flipped big-endian INT — identical to what
//! `KeyDef::pack_record` + the integer packer produce for a signed INT
//! primary key column.  The sign-flip makes byte order match numeric
//! order for both positive and negative values, so `scan_prefix` returns
//! rows sorted by PK as the MTR `--sorted_result` queries expect.

#[cfg(test)]
mod tests {
    use crate::engine::db::EngineDb;
    use crate::engine::txn_registry::TxnRegistry;
    use slatedb::KeyValue;

    // ----------------------------------------------------------------
    // Helpers
    // ----------------------------------------------------------------

    async fn fresh(name: &str) -> EngineDb {
        EngineDb::open_in_memory(name).await.expect("open_in_memory")
    }

    /// 4-byte big-endian index prefix + 4-byte memcomparable signed INT.
    fn pk_key(index_num: u32, pk_val: i32) -> Vec<u8> {
        let mut k = Vec::with_capacity(8);
        k.extend_from_slice(&index_num.to_be_bytes());
        k.extend_from_slice(&((pk_val as u32) ^ 0x8000_0000).to_be_bytes());
        k
    }

    /// Simple 4-byte LE INT value blob (one non-PK, non-nullable INT field).
    fn int_val(v: i32) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    /// Value blob for a nullable INT: 1-byte null bitmap + 4-byte LE INT.
    fn nullable_int_val(v: Option<i32>) -> Vec<u8> {
        match v {
            None => vec![0x01],
            Some(x) => {
                let mut b = vec![0x00];
                b.extend_from_slice(&x.to_le_bytes());
                b
            }
        }
    }

    /// Drain an async `DbIterator` into a `Vec<KeyValue>`.
    async fn drain(mut iter: slatedb::DbIterator) -> Vec<KeyValue> {
        let mut out = Vec::new();
        while let Some(kv) = iter.next().await.expect("iter.next") {
            out.push(kv);
        }
        out
    }

    // ----------------------------------------------------------------
    // Mirrors: 1st.test
    // CREATE TABLE / INSERT / SELECT / DROP
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn basic_dml() {
        let db = fresh("mtr_1st").await;
        let reg = TxnRegistry::new();
        const IDX: u32 = 1;

        reg.get_or_create(1, &db).await.expect("begin");
        {
            let mut txn = reg.take(1).expect("txn");
            txn.put(&pk_key(IDX, 1), &int_val(1)).expect("put row 1");
            txn.put(&pk_key(IDX, 2), &int_val(2)).expect("put row 2");
            reg.reinsert(1, txn);
        }
        reg.commit(1).await.expect("commit");

        reg.get_or_create(2, &db).await.expect("begin read");
        let txn = reg.take(2).expect("txn");
        let iter = txn
            .scan_prefix(&IDX.to_be_bytes())
            .await
            .expect("scan");
        reg.reinsert(2, txn);
        let mut rows = drain(iter).await;
        rows.sort_by(|a, b| a.key.cmp(&b.key));
        reg.rollback(2);

        assert_eq!(rows.len(), 2, "both rows visible");
        assert_eq!(rows[0].key.as_ref(), pk_key(IDX, 1).as_slice());
        assert_eq!(rows[0].value.as_ref(), int_val(1).as_slice());
        assert_eq!(rows[1].key.as_ref(), pk_key(IDX, 2).as_slice());
        assert_eq!(rows[1].value.as_ref(), int_val(2).as_slice());

        db.close().await.expect("close");
    }

    // ----------------------------------------------------------------
    // Mirrors: insert.test
    // INSERT variants, multi-row, INSERT..SELECT
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn insert_multi_row() {
        let db = fresh("mtr_insert").await;
        let reg = TxnRegistry::new();
        const IDX: u32 = 10;

        reg.get_or_create(1, &db).await.expect("begin");
        {
            let mut txn = reg.take(1).expect("txn");
            for pk in [100i32, 1, 2, 3, 4, 5] {
                txn.put(&pk_key(IDX, pk), &int_val(pk)).expect("put");
            }
            reg.reinsert(1, txn);
        }
        reg.commit(1).await.expect("commit");

        reg.get_or_create(2, &db).await.expect("begin read");
        let txn = reg.take(2).expect("txn");
        let iter = txn.scan_prefix(&IDX.to_be_bytes()).await.expect("scan");
        reg.reinsert(2, txn);
        let rows = drain(iter).await;
        reg.rollback(2);

        assert_eq!(rows.len(), 6);
        // Byte-sorted order equals numeric order (sign-flip encoding).
        assert_eq!(rows[0].key.as_ref(), pk_key(IDX, 1).as_slice());
        assert_eq!(rows[1].key.as_ref(), pk_key(IDX, 2).as_slice());
        assert_eq!(rows[5].key.as_ref(), pk_key(IDX, 100).as_slice());

        db.close().await.expect("close");
    }

    // ----------------------------------------------------------------
    // Mirrors: select.test
    // Basic SELECT, ORDER BY PK (scan_prefix returns in key order)
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn select_order_by_pk() {
        let db = fresh("mtr_select").await;
        let reg = TxnRegistry::new();
        const IDX: u32 = 20;

        // Insert in non-ascending order.
        reg.get_or_create(1, &db).await.expect("begin");
        {
            let mut txn = reg.take(1).expect("txn");
            txn.put(&pk_key(IDX, 200), &int_val(200)).expect("put");
            txn.put(&pk_key(IDX, 1), &int_val(1)).expect("put");
            txn.put(&pk_key(IDX, 100), &int_val(100)).expect("put");
            reg.reinsert(1, txn);
        }
        reg.commit(1).await.expect("commit");

        reg.get_or_create(2, &db).await.expect("begin read");
        let txn = reg.take(2).expect("txn");
        let iter = txn.scan_prefix(&IDX.to_be_bytes()).await.expect("scan");
        reg.reinsert(2, txn);
        let rows = drain(iter).await;
        reg.rollback(2);

        assert_eq!(rows.len(), 3);
        // scan_prefix must return keys in byte-sorted (= PK numeric) order.
        assert!(rows[0].key < rows[1].key && rows[1].key < rows[2].key, "ascending");
        assert_eq!(rows[0].key.as_ref(), pk_key(IDX, 1).as_slice());
        assert_eq!(rows[1].key.as_ref(), pk_key(IDX, 100).as_slice());
        assert_eq!(rows[2].key.as_ref(), pk_key(IDX, 200).as_slice());

        db.close().await.expect("close");
    }

    // ----------------------------------------------------------------
    // Mirrors: transaction.test — committed insert is visible
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn transaction_commit() {
        let db = fresh("mtr_txn_commit").await;
        let reg = TxnRegistry::new();
        const IDX: u32 = 30;

        reg.get_or_create(1, &db).await.expect("begin");
        {
            let mut txn = reg.take(1).expect("txn");
            txn.put(&pk_key(IDX, 1), &int_val(10)).expect("put");
            reg.reinsert(1, txn);
        }
        reg.commit(1).await.expect("commit");

        reg.get_or_create(2, &db).await.expect("begin");
        let txn = reg.take(2).expect("txn");
        let iter = txn.scan_prefix(&IDX.to_be_bytes()).await.expect("scan");
        reg.reinsert(2, txn);
        let rows = drain(iter).await;
        reg.rollback(2);

        assert_eq!(rows.len(), 1, "committed row is visible");

        db.close().await.expect("close");
    }

    // ----------------------------------------------------------------
    // Mirrors: transaction.test — rolled-back insert is not visible
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn transaction_rollback() {
        let db = fresh("mtr_txn_rollback").await;
        let reg = TxnRegistry::new();
        const IDX: u32 = 31;

        reg.get_or_create(1, &db).await.expect("begin");
        {
            let mut txn = reg.take(1).expect("txn");
            txn.put(&pk_key(IDX, 1), &int_val(10)).expect("put");
            reg.reinsert(1, txn);
        }
        reg.rollback(1);

        reg.get_or_create(2, &db).await.expect("begin");
        let txn = reg.take(2).expect("txn");
        let iter = txn.scan_prefix(&IDX.to_be_bytes()).await.expect("scan");
        reg.reinsert(2, txn);
        let rows = drain(iter).await;
        reg.rollback(2);

        assert!(rows.is_empty(), "rolled-back put must not be visible");

        db.close().await.expect("close");
    }

    // ----------------------------------------------------------------
    // Mirrors: type_int.test
    // Memcomparable INT encoding — negative < zero < positive in byte order
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn type_int_ordering() {
        let db = fresh("mtr_type_int").await;
        let reg = TxnRegistry::new();
        const IDX: u32 = 40;

        let ascending = [i32::MIN, -1, 0, 1, i32::MAX];

        // Insert in scrambled order.
        reg.get_or_create(1, &db).await.expect("begin");
        {
            let mut txn = reg.take(1).expect("txn");
            for &v in &[0i32, i32::MAX, i32::MIN, 1, -1] {
                txn.put(&pk_key(IDX, v), &int_val(v)).expect("put");
            }
            reg.reinsert(1, txn);
        }
        reg.commit(1).await.expect("commit");

        reg.get_or_create(2, &db).await.expect("begin");
        let txn = reg.take(2).expect("txn");
        let iter = txn.scan_prefix(&IDX.to_be_bytes()).await.expect("scan");
        reg.reinsert(2, txn);
        let rows = drain(iter).await;
        reg.rollback(2);

        assert_eq!(rows.len(), ascending.len());
        for (i, &expected_pk) in ascending.iter().enumerate() {
            assert_eq!(
                rows[i].key.as_ref(),
                pk_key(IDX, expected_pk).as_slice(),
                "row {i}: expected PK {expected_pk}"
            );
        }

        db.close().await.expect("close");
    }

    // ----------------------------------------------------------------
    // Mirrors: col_opt_null.test
    // NULL vs non-NULL encoding round-trips through put/scan
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn col_opt_null() {
        let db = fresh("mtr_null").await;
        let reg = TxnRegistry::new();
        const IDX: u32 = 50;

        reg.get_or_create(1, &db).await.expect("begin");
        {
            let mut txn = reg.take(1).expect("txn");
            txn.put(&pk_key(IDX, 1), &nullable_int_val(None)).expect("put NULL");
            txn.put(&pk_key(IDX, 2), &nullable_int_val(Some(42))).expect("put non-NULL");
            txn.put(&pk_key(IDX, 3), &nullable_int_val(Some(0))).expect("put zero");
            reg.reinsert(1, txn);
        }
        reg.commit(1).await.expect("commit");

        reg.get_or_create(2, &db).await.expect("begin");
        let txn = reg.take(2).expect("txn");
        let iter = txn.scan_prefix(&IDX.to_be_bytes()).await.expect("scan");
        reg.reinsert(2, txn);
        let rows = drain(iter).await;
        reg.rollback(2);

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].value.as_ref(), nullable_int_val(None).as_slice(), "NULL");
        assert_eq!(rows[1].value.as_ref(), nullable_int_val(Some(42)).as_slice(), "42");
        assert_eq!(rows[2].value.as_ref(), nullable_int_val(Some(0)).as_slice(), "0");

        db.close().await.expect("close");
    }

    // ----------------------------------------------------------------
    // Mirrors: col_opt_not_null.test
    // NOT NULL fields: value bytes have no null-bitmap overhead
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn col_opt_not_null() {
        let db = fresh("mtr_not_null").await;
        let reg = TxnRegistry::new();
        const IDX: u32 = 51;

        reg.get_or_create(1, &db).await.expect("begin");
        {
            let mut txn = reg.take(1).expect("txn");
            txn.put(&pk_key(IDX, 1), &int_val(42)).expect("put");
            txn.put(&pk_key(IDX, 2), &int_val(-1)).expect("put");
            reg.reinsert(1, txn);
        }
        reg.commit(1).await.expect("commit");

        reg.get_or_create(2, &db).await.expect("begin");
        let txn = reg.take(2).expect("txn");
        let iter = txn.scan_prefix(&IDX.to_be_bytes()).await.expect("scan");
        reg.reinsert(2, txn);
        let rows = drain(iter).await;
        reg.rollback(2);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].value.as_ref(), int_val(42).as_slice());
        assert_eq!(rows[1].value.as_ref(), int_val(-1).as_slice());
        // NOT NULL: no extra null-bitmap byte.
        assert_eq!(rows[0].value.len(), 4);

        db.close().await.expect("close");
    }

    // ----------------------------------------------------------------
    // Mirrors: update.test
    // Overwrite same key → only the latest value is visible
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn update_explicit_pk() {
        let db = fresh("mtr_update").await;
        let reg = TxnRegistry::new();
        const IDX: u32 = 60;

        // Initial insert.
        reg.get_or_create(1, &db).await.expect("begin");
        {
            let mut txn = reg.take(1).expect("txn");
            txn.put(&pk_key(IDX, 1), &int_val(10)).expect("insert");
            reg.reinsert(1, txn);
        }
        reg.commit(1).await.expect("commit");

        // UPDATE: overwrite same key.
        reg.get_or_create(2, &db).await.expect("begin");
        {
            let mut txn = reg.take(2).expect("txn");
            txn.put(&pk_key(IDX, 1), &int_val(99)).expect("overwrite");
            reg.reinsert(2, txn);
        }
        reg.commit(2).await.expect("commit");

        // SELECT: should see the updated value.
        reg.get_or_create(3, &db).await.expect("begin");
        let txn = reg.take(3).expect("txn");
        let val = txn.get(&pk_key(IDX, 1)).await.expect("get").expect("exists");
        reg.reinsert(3, txn);
        reg.rollback(3);
        assert_eq!(val.as_ref(), int_val(99).as_slice());

        // Rolled-back update must not be visible.
        reg.get_or_create(4, &db).await.expect("begin");
        {
            let mut txn = reg.take(4).expect("txn");
            txn.put(&pk_key(IDX, 1), &int_val(0)).expect("overwrite (rollback)");
            reg.reinsert(4, txn);
        }
        reg.rollback(4);

        reg.get_or_create(5, &db).await.expect("begin");
        let txn = reg.take(5).expect("txn");
        let val = txn.get(&pk_key(IDX, 1)).await.expect("get").expect("exists");
        reg.reinsert(5, txn);
        reg.rollback(5);
        assert_eq!(val.as_ref(), int_val(99).as_slice(), "rollback did not change value");

        db.close().await.expect("close");
    }

    // ----------------------------------------------------------------
    // Mirrors: delete.test
    // DELETE by PK — key no longer appears in scan
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn delete_explicit_pk() {
        let db = fresh("mtr_delete").await;
        let reg = TxnRegistry::new();
        const IDX: u32 = 70;

        reg.get_or_create(1, &db).await.expect("begin");
        {
            let mut txn = reg.take(1).expect("txn");
            txn.put(&pk_key(IDX, 1), &int_val(10)).expect("put");
            txn.put(&pk_key(IDX, 2), &int_val(20)).expect("put");
            txn.put(&pk_key(IDX, 3), &int_val(30)).expect("put");
            reg.reinsert(1, txn);
        }
        reg.commit(1).await.expect("commit");

        // DELETE WHERE pk = 1.
        reg.get_or_create(2, &db).await.expect("begin");
        {
            let mut txn = reg.take(2).expect("txn");
            txn.delete(&pk_key(IDX, 1)).expect("delete");
            reg.reinsert(2, txn);
        }
        reg.commit(2).await.expect("commit");

        reg.get_or_create(3, &db).await.expect("begin");
        let txn = reg.take(3).expect("txn");
        let iter = txn.scan_prefix(&IDX.to_be_bytes()).await.expect("scan");
        reg.reinsert(3, txn);
        let rows = drain(iter).await;
        reg.rollback(3);
        assert_eq!(rows.len(), 2, "one row deleted");

        // Rolled-back delete must not be visible.
        reg.get_or_create(4, &db).await.expect("begin");
        {
            let mut txn = reg.take(4).expect("txn");
            txn.delete(&pk_key(IDX, 2)).expect("delete (will rollback)");
            reg.reinsert(4, txn);
        }
        reg.rollback(4);

        reg.get_or_create(5, &db).await.expect("begin");
        let txn = reg.take(5).expect("txn");
        let iter = txn.scan_prefix(&IDX.to_be_bytes()).await.expect("scan");
        reg.reinsert(5, txn);
        let rows = drain(iter).await;
        reg.rollback(5);
        assert_eq!(rows.len(), 2, "rolled-back delete must not take effect");

        db.close().await.expect("close");
    }

    // ----------------------------------------------------------------
    // Mirrors: primary_key.test
    // PK uniqueness — overwrite is the correct outcome (no dups)
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn primary_key_uniqueness() {
        let db = fresh("mtr_pk_unique").await;
        let reg = TxnRegistry::new();
        const IDX: u32 = 80;

        reg.get_or_create(1, &db).await.expect("begin");
        {
            let mut txn = reg.take(1).expect("txn");
            txn.put(&pk_key(IDX, 1), &int_val(111)).expect("put first");
            txn.put(&pk_key(IDX, 1), &int_val(222)).expect("put duplicate key");
            reg.reinsert(1, txn);
        }
        reg.commit(1).await.expect("commit");

        reg.get_or_create(2, &db).await.expect("begin");
        let txn = reg.take(2).expect("txn");
        let iter = txn.scan_prefix(&IDX.to_be_bytes()).await.expect("scan");
        reg.reinsert(2, txn);
        let rows = drain(iter).await;
        reg.rollback(2);

        assert_eq!(rows.len(), 1, "no duplicate; last write wins");
        assert_eq!(rows[0].value.as_ref(), int_val(222).as_slice());

        db.close().await.expect("close");
    }

    // ----------------------------------------------------------------
    // Mirrors: truncate.test
    // TRUNCATE assigns a new index_number — old prefix unreachable via new scan
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn truncate() {
        let db = fresh("mtr_truncate").await;
        let reg = TxnRegistry::new();
        const IDX_OLD: u32 = 90;
        const IDX_NEW: u32 = 91;

        // Pre-TRUNCATE data.
        reg.get_or_create(1, &db).await.expect("begin");
        {
            let mut txn = reg.take(1).expect("txn");
            txn.put(&pk_key(IDX_OLD, 1), &int_val(10)).expect("put");
            txn.put(&pk_key(IDX_OLD, 2), &int_val(20)).expect("put");
            reg.reinsert(1, txn);
        }
        reg.commit(1).await.expect("commit");

        // Post-TRUNCATE: new index_number, new data.
        reg.get_or_create(2, &db).await.expect("begin");
        {
            let mut txn = reg.take(2).expect("txn");
            txn.put(&pk_key(IDX_NEW, 1), &int_val(100)).expect("put");
            txn.put(&pk_key(IDX_NEW, 2), &int_val(200)).expect("put");
            reg.reinsert(2, txn);
        }
        reg.commit(2).await.expect("commit");

        reg.get_or_create(3, &db).await.expect("begin");
        let txn = reg.take(3).expect("txn");
        let old_iter = txn.scan_prefix(&IDX_OLD.to_be_bytes()).await.expect("scan old");
        let new_iter = txn.scan_prefix(&IDX_NEW.to_be_bytes()).await.expect("scan new");
        reg.reinsert(3, txn);
        let old_rows = drain(old_iter).await;
        let new_rows = drain(new_iter).await;
        reg.rollback(3);

        // Old data still in storage (GC hasn't run); new data present.
        assert_eq!(old_rows.len(), 2, "old index data present (not gc'd)");
        assert_eq!(new_rows.len(), 2, "new index has post-truncate data");
        // New scan does NOT see old rows (different prefix).
        assert_eq!(new_rows[0].key.as_ref(), pk_key(IDX_NEW, 1).as_slice());

        db.close().await.expect("close");
    }

    // ----------------------------------------------------------------
    // Mirrors: locking_issues_case1.test
    // SSI isolation — uncommitted write in T1 is not visible to T2
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn locking_visibility() {
        let db = fresh("mtr_locking").await;
        let reg = TxnRegistry::new();
        const IDX: u32 = 100;

        // T1 starts, writes, but does NOT commit yet.
        reg.get_or_create(1, &db).await.expect("begin T1");
        {
            let mut txn = reg.take(1).expect("txn T1");
            txn.put(&pk_key(IDX, 1), &int_val(42)).expect("put T1");
            reg.reinsert(1, txn);
        }

        // T2 starts and scans — must not see T1's uncommitted write.
        reg.get_or_create(2, &db).await.expect("begin T2");
        let txn2 = reg.take(2).expect("txn T2");
        let iter = txn2.scan_prefix(&IDX.to_be_bytes()).await.expect("scan T2");
        reg.reinsert(2, txn2);
        let before = drain(iter).await;
        reg.rollback(2);

        assert!(before.is_empty(), "T2 must not see T1's uncommitted write");

        // Commit T1.
        reg.commit(1).await.expect("commit T1");

        // T3 starts after T1 commits — must see the row.
        reg.get_or_create(3, &db).await.expect("begin T3");
        let txn3 = reg.take(3).expect("txn T3");
        let iter = txn3.scan_prefix(&IDX.to_be_bytes()).await.expect("scan T3");
        reg.reinsert(3, txn3);
        let after = drain(iter).await;
        reg.rollback(3);

        assert_eq!(after.len(), 1, "T3 sees T1's committed write");

        db.close().await.expect("close");
    }

    // ----------------------------------------------------------------
    // Mirrors: autoincrement.test
    // AUTO_INCREMENT counter produces monotonically increasing IDs
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn autoincrement() {
        let db = fresh("mtr_autoinc").await;
        let reg = TxnRegistry::new();
        const IDX: u32 = 110;

        let counter = std::sync::atomic::AtomicU64::new(1);

        reg.get_or_create(1, &db).await.expect("begin");
        {
            let mut txn = reg.take(1).expect("txn");
            for _ in 0..3 {
                let id = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // Encode ID as big-endian u64 key (hidden-PK format).
                let mut key = IDX.to_be_bytes().to_vec();
                key.extend_from_slice(&id.to_be_bytes());
                txn.put(&key, &int_val(id as i32)).expect("put");
            }
            reg.reinsert(1, txn);
        }
        reg.commit(1).await.expect("commit");

        reg.get_or_create(2, &db).await.expect("begin");
        let txn = reg.take(2).expect("txn");
        let iter = txn.scan_prefix(&IDX.to_be_bytes()).await.expect("scan");
        reg.reinsert(2, txn);
        let rows = drain(iter).await;
        reg.rollback(2);

        assert_eq!(rows.len(), 3);
        assert!(rows[0].key < rows[1].key && rows[1].key < rows[2].key, "IDs monotonic");

        db.close().await.expect("close");
    }
}
