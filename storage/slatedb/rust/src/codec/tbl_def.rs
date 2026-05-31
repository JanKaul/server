//! Per-table descriptor (`Rdb_tbl_def`).
//!
//! Translated from `storage/rocksdb/rdb_datadic.{h,cc}` (class
//! `Rdb_tbl_def`, ~75 LoC header + ~165 LoC of methods).
//!
//! One [`TblDef`] per opened table. Owns the per-table identity
//! (db / table / partition name parts), the [`KeyDef`] list, and the
//! mutable per-table atomics (hidden-PK counter, auto-increment,
//! read-free-replication flag, in-memory `update_time`). Long-lived;
//! shared between handlers via `Arc<TblDef>` (cf. C++ which uses raw
//! pointers and explicit lifecycle management in the DDL manager).
//!
//! ## What's translated
//!
//! - Construction from a normalised `db.tbl[#P#part]` name, via
//!   [`crate::utils::names::split_normalized_tablename`].
//! - System-table classification (`mysql` / `performance_schema` /
//!   `information_schema`), matching the C++ `system_dbs[]` allow-list.
//! - Name accessors (`full_tablename`, `base_dbname`, `base_tablename`,
//!   `base_partition`).
//! - Atomic state: `hidden_pk_val` (i64, signed to match C++
//!   `longlong`), `auto_incr_val` (u64), `is_read_free_rpl_table`
//!   (bool), `update_time` (i64 epoch seconds).
//! - `get_autoincr_gl_index_id` — the panic-on-missing-PK lookup the
//!   C++ does at `rdb_datadic.cc:3722`.
//! - `check_and_set_read_free_rpl_table` — wired to the
//!   MARIAROCKS_NOT_YET stub (always sets `false`, matching the C++
//!   `#if 0` branch).
//!
//! ## What's deferred
//!
//! - **`get_create_time`** — reads the table's `.frm` ctime. We don't
//!   have `.frm` files in SlateDB; this stub returns
//!   [`CREATE_TIME_NULL`] (matching the C++ "no data available" branch
//!   when `my_stat` fails). The lazy-load-from-filesystem behaviour
//!   isn't applicable to an object-store-backed deployment.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use slatedb::{Db, Error, WriteBatch};

use crate::codec::dict::{
    cf_flags, ddl_entry_index_start_number, index_info, system_key, DataDictType,
};
use crate::codec::key::{
    IndexInfoVersion, IndexType, KeyDef, CF_FLAGS_TO_IGNORE, PER_PARTITION_CF_FLAG,
    REVERSE_CF_FLAG,
};
use crate::globals::GlIndexId;
use crate::utils::names::split_normalized_tablename;

/// "No data available" sentinel for `create_time` — matches the C++
/// branch at `rdb_datadic.cc:3636` that runs when `my_stat` fails. SQL
/// surfaces this as NULL.
pub const CREATE_TIME_NULL: i64 = 0;

/// MariaDB databases that the per-table classifier treats as "system":
/// dropping / renaming columns in these is gated by extra checks
/// elsewhere in the engine. Matches the C++ allow-list at
/// `rdb_datadic.cc:3687`.
const SYSTEM_DBS: &[&str] = &["mysql", "performance_schema", "information_schema"];

pub struct TblDef {
    // ----- identity (immutable after construction) -----
    dbname_tablename: String,
    dbname: String,
    tablename: String,
    partition: Option<String>,
    is_mysql_system_table: bool,

    // ----- index list (set at construction / schema change) -----
    /// `Arc` per index because the same KeyDef instance is shared
    /// across handlers and across the ddl-manager's index-id map.
    /// `Vec` (not RwLock) because the C++ replaces the whole array
    /// wholesale on schema change; in Rust we'd build a fresh TblDef
    /// and swap its outer `Arc<TblDef>` in the ddl manager.
    key_descrs: Vec<Arc<KeyDef>>,

    // ----- mutable atomic state -----
    /// Next hidden-PK value to issue. `i64` to match C++ `longlong`
    /// (sign matters for the wraparound check elsewhere in MyRocks).
    hidden_pk_val: AtomicI64,
    /// Next auto-increment value. Bumped via fetch_max so concurrent
    /// writers and crash recovery converge on the highest observed.
    auto_incr_val: AtomicU64,
    /// True iff this table participates in read-free replication.
    /// Wired to `false` today (`MARIAROCKS_NOT_YET`).
    is_read_free_rpl_table: AtomicBool,
    /// Most recent row-mutation time, epoch seconds. In-memory only —
    /// not persisted; cleared on restart. Matches C++
    /// `std::atomic<time_t> m_update_time`.
    update_time: AtomicI64,
}

impl TblDef {
    /// Construct an empty TblDef from a `db.tbl[#P#part]` name. The
    /// `key_descrs` list starts empty and must be populated via
    /// [`TblDef::with_keys`] before the descriptor is published to
    /// handlers.
    ///
    /// Errors: `Invalid` if `full_name` doesn't contain a `.`
    /// separator.
    pub fn new(full_name: &str) -> Result<Self, Error> {
        let split = split_normalized_tablename(full_name)?;
        let is_system = SYSTEM_DBS.iter().any(|sys| *sys == split.db);
        Ok(Self {
            dbname_tablename: full_name.to_owned(),
            dbname: split.db,
            tablename: split.table,
            partition: split.partition,
            is_mysql_system_table: is_system,
            key_descrs: Vec::new(),
            hidden_pk_val: AtomicI64::new(0),
            auto_incr_val: AtomicU64::new(0),
            is_read_free_rpl_table: AtomicBool::new(false),
            update_time: AtomicI64::new(0),
        })
    }

    /// Builder helper — set the key descriptor list. Typical usage is
    /// `TblDef::new("db.tbl")?.with_keys(vec![pk_arc, sk_arc])`.
    pub fn with_keys(mut self, keys: Vec<Arc<KeyDef>>) -> Self {
        self.key_descrs = keys;
        self
    }

    // ----- name accessors -----

    pub fn full_tablename(&self) -> &str {
        &self.dbname_tablename
    }
    pub fn base_dbname(&self) -> &str {
        &self.dbname
    }
    pub fn base_tablename(&self) -> &str {
        &self.tablename
    }
    pub fn base_partition(&self) -> Option<&str> {
        self.partition.as_deref()
    }
    pub fn is_mysql_system_table(&self) -> bool {
        self.is_mysql_system_table
    }

    // ----- index-list accessors -----

    pub fn key_count(&self) -> usize {
        self.key_descrs.len()
    }
    pub fn key_descrs(&self) -> &[Arc<KeyDef>] {
        &self.key_descrs
    }
    pub fn key(&self, idx: usize) -> Option<&Arc<KeyDef>> {
        self.key_descrs.get(idx)
    }

    /// Locate the index that owns the auto-increment counter — the
    /// table's primary or hidden-primary key.
    ///
    /// Panics if the table has neither (the C++ aborts with the same
    /// rationale at `rdb_datadic.cc:3732`: "Every table must have a
    /// primary key, even if it's hidden"). Returning an `Option` here
    /// would force every call site into an awkward `expect()` that
    /// hides the same invariant; panic surfaces the bug at its
    /// origin.
    pub fn get_autoincr_gl_index_id(&self) -> GlIndexId {
        for kd in &self.key_descrs {
            if matches!(
                kd.index_type,
                IndexType::Primary | IndexType::HiddenPrimary
            ) {
                return kd.get_gl_index_id();
            }
        }
        panic!(
            "TblDef::get_autoincr_gl_index_id: table {:?} has no primary key",
            self.dbname_tablename,
        );
    }

    // ----- atomic counter accessors -----

    pub fn hidden_pk_val(&self) -> i64 {
        self.hidden_pk_val.load(Ordering::Relaxed)
    }
    pub fn store_hidden_pk_val(&self, v: i64) {
        self.hidden_pk_val.store(v, Ordering::Relaxed);
    }
    /// Atomically take the larger of the current value and `v`,
    /// returning the previous value (matches MyRocks' max-merge
    /// semantic for hidden-PK).
    pub fn fetch_max_hidden_pk_val(&self, v: i64) -> i64 {
        self.hidden_pk_val.fetch_max(v, Ordering::Relaxed)
    }

    /// Atomically add `delta` to the hidden-PK counter and return
    /// the *previous* value (i.e. the rowid to use for the new row).
    /// Matches C++ `m_tbl_def->m_hidden_pk_val++` at
    /// `ha_rocksdb.cc:6272` (post-increment returns the old value).
    pub fn fetch_add_hidden_pk_val(&self, delta: i64) -> i64 {
        self.hidden_pk_val.fetch_add(delta, Ordering::Relaxed)
    }

    pub fn auto_incr_val(&self) -> u64 {
        self.auto_incr_val.load(Ordering::Relaxed)
    }
    pub fn store_auto_incr_val(&self, v: u64) {
        self.auto_incr_val.store(v, Ordering::Relaxed);
    }
    pub fn fetch_max_auto_incr_val(&self, v: u64) -> u64 {
        self.auto_incr_val.fetch_max(v, Ordering::Relaxed)
    }
    /// Borrow the raw atomic — needed by `HaSlateDb::get_auto_increment`'s
    /// CAS loop, which can't be expressed through the high-level
    /// store/fetch_max helpers because the value it stores depends on
    /// the value it loaded (capped at `max_val`, replication sequence
    /// math, etc.).
    pub fn auto_incr_atomic(&self) -> &AtomicU64 {
        &self.auto_incr_val
    }

    pub fn is_read_free_rpl_table(&self) -> bool {
        self.is_read_free_rpl_table.load(Ordering::Relaxed)
    }

    /// Today this is a no-op that pins the flag to `false` — read-free
    /// replication isn't supported. Mirrors the C++ `#if 0`
    /// MARIAROCKS_NOT_YET branch at `rdb_datadic.cc:3702`.
    pub fn check_and_set_read_free_rpl_table(&self) {
        self.is_read_free_rpl_table.store(false, Ordering::Relaxed);
    }

    pub fn update_time(&self) -> i64 {
        self.update_time.load(Ordering::Relaxed)
    }
    pub fn store_update_time(&self, v: i64) {
        self.update_time.store(v, Ordering::Relaxed);
    }

    /// Stubbed: returns [`CREATE_TIME_NULL`]. The C++ reads ctime from
    /// the table's `.frm` file; SlateDB stores no `.frm`, so the
    /// information isn't available here. Surfaces in SQL as NULL —
    /// same as the C++ "stat failed" branch.
    pub fn get_create_time(&self) -> i64 {
        CREATE_TIME_NULL
    }

    /// Persist this descriptor to the data dictionary. Validates
    /// per-CF flag agreement, registers any new CFs, writes the
    /// per-index `IndexInfo` row for each KeyDef, then writes the
    /// typed DDL-entry row. All writes go into a single SlateDB
    /// `WriteBatch` so commit is atomic.
    ///
    /// Translated from `Rdb_tbl_def::put_dict` (`rdb_datadic.cc:3558`).
    /// The C++ takes a `rocksdb::WriteBatch *` and a pre-built key
    /// `Slice`; we build the key from `self.full_tablename()` and own
    /// the batch internally — cleaner Rust API. Caller atomicity
    /// guarantees that previously came from being passed into a
    /// larger batch are not provided here; if a caller needs that,
    /// add a `put_dict_into(batch: &mut WriteBatch)` variant later.
    ///
    /// ## Errors
    ///
    /// - `Invalid` if any KeyDef references a `cf_id` whose existing
    ///   `cf_flags` row disagrees with what `self` would write (after
    ///   masking out [`CF_FLAGS_TO_IGNORE`]). Matches the C++
    ///   `ER_CF_DIFFERENT` (`rdb_datadic.cc:3592`).
    /// - `Unavailable` on object-store failure (propagated from
    ///   SlateDB).
    pub async fn put_dict(&self, db: &Db) -> Result<(), Error> {
        // ----- pre-validate cf_flags for every cf_id this table uses -----
        //
        // Read-only pass. For CFs that don't yet exist in the dict, we
        // collect the (cf_id, flags) pair to enqueue into the batch
        // below. For CFs that DO exist, we just validate matching
        // flags (after masking the per-partition bit).
        let mut to_install: Vec<(u32, u32)> = Vec::new();
        let mut seen_cfs: HashSet<u32> = HashSet::new();

        for kd in &self.key_descrs {
            if !seen_cfs.insert(kd.cf_id) {
                continue;
            }
            let candidate = kd_to_cf_flags(kd);
            match cf_flags::read(db, kd.cf_id).await? {
                Some(existing) => {
                    let masked_existing = existing & !CF_FLAGS_TO_IGNORE;
                    let masked_candidate = candidate & !CF_FLAGS_TO_IGNORE;
                    if masked_existing != masked_candidate {
                        return Err(Error::invalid(format!(
                            "cf_flags mismatch: cf_id={} existing={:#06x} \
                             candidate={:#06x} (after masking ignored bits)",
                            kd.cf_id, masked_existing, masked_candidate,
                        )));
                    }
                }
                None => {
                    to_install.push((kd.cf_id, candidate));
                }
            }
        }

        // ----- build the WriteBatch -----
        let mut batch = WriteBatch::new();

        for (cf_id, flags) in to_install {
            batch.put(
                system_key(DataDictType::CfDefinition, &cf_id.to_be_bytes()),
                cf_flags::encode_value(flags),
            );
        }

        for kd in &self.key_descrs {
            let info = index_info::IndexInfo {
                index_dict_version: IndexInfoVersion::FieldFlags,
                index_type: kd.index_type,
                kv_format_version: kd.kv_format_version,
                index_flags: kd.index_flags_bitmap,
                ttl_duration: kd.ttl_duration,
            };
            let suffix = {
                let gl = kd.get_gl_index_id();
                let mut s = [0u8; 8];
                s[..4].copy_from_slice(&gl.cf_id.to_be_bytes());
                s[4..].copy_from_slice(&gl.index_id.to_be_bytes());
                s
            };
            batch.put(
                system_key(DataDictType::IndexInfo, &suffix),
                index_info::encode_value(&info),
            );
        }

        let ids: Vec<GlIndexId> = self
            .key_descrs
            .iter()
            .map(|kd| kd.get_gl_index_id())
            .collect();
        batch.put(
            ddl_entry_index_start_number::full_key(&self.dbname_tablename),
            ddl_entry_index_start_number::encode_value(&ids),
        );

        // ----- atomic commit -----
        db.write(batch).await?;
        Ok(())
    }
}

/// Derive the persisted `cf_flags` value from a `KeyDef`. Matches the
/// C++ computation at `rdb_datadic.cc:3569..3572`.
fn kd_to_cf_flags(kd: &KeyDef) -> u32 {
    (if kd.is_reverse_cf { REVERSE_CF_FLAG } else { 0 })
        | (if kd.is_per_partition_cf {
            PER_PARTITION_CF_FLAG
        } else {
            0
        })
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

    fn hidden_pk(index_number: u32, cf_id: u32) -> Arc<KeyDef> {
        Arc::new(KeyDef::new_skeleton(
            index_number,
            cf_id,
            0,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::HiddenPrimary,
            PRIMARY_FORMAT_VERSION_LATEST,
            false,
            "HIDDEN_PK_NAME",
        ))
    }

    fn sk(index_number: u32, cf_id: u32) -> Arc<KeyDef> {
        Arc::new(KeyDef::new_skeleton(
            index_number,
            cf_id,
            1,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Secondary,
            SECONDARY_FORMAT_VERSION_LATEST,
            false,
            "sk",
        ))
    }

    #[test]
    fn new_splits_dbname_tablename_partition() {
        let t = TblDef::new("appdb.users").expect("simple");
        assert_eq!(t.full_tablename(), "appdb.users");
        assert_eq!(t.base_dbname(), "appdb");
        assert_eq!(t.base_tablename(), "users");
        assert_eq!(t.base_partition(), None);
        assert!(!t.is_mysql_system_table());
        assert_eq!(t.key_count(), 0);
    }

    #[test]
    fn new_extracts_partition_suffix() {
        let t = TblDef::new("appdb.events#P#p2024").expect("partitioned");
        assert_eq!(t.base_dbname(), "appdb");
        assert_eq!(t.base_tablename(), "events");
        assert_eq!(t.base_partition(), Some("p2024"));
    }

    #[test]
    fn new_classifies_system_databases() {
        for sys in ["mysql", "performance_schema", "information_schema"] {
            let t =
                TblDef::new(&format!("{sys}.user")).expect("system");
            assert!(
                t.is_mysql_system_table(),
                "{sys} must be classified as system",
            );
        }
        // Names containing "mysql" as a prefix should NOT match.
        let t = TblDef::new("mysqlx.foo").expect("non-system");
        assert!(!t.is_mysql_system_table());
    }

    #[test]
    fn new_rejects_malformed_name() {
        let err = match TblDef::new("no_dot_at_all") {
            Err(e) => e,
            Ok(_) => panic!("expected error from malformed name"),
        };
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
    }

    #[test]
    fn with_keys_populates_descriptor_list() {
        let t = TblDef::new("db.t")
            .unwrap()
            .with_keys(vec![pk(10, 1), sk(11, 1)]);
        assert_eq!(t.key_count(), 2);
        assert_eq!(t.key(0).unwrap().get_index_number(), 10);
        assert_eq!(t.key(1).unwrap().get_index_number(), 11);
        assert!(t.key(2).is_none());
    }

    #[test]
    fn get_autoincr_gl_index_id_returns_primary() {
        let t = TblDef::new("db.t")
            .unwrap()
            .with_keys(vec![pk(100, 7), sk(101, 7)]);
        assert_eq!(
            t.get_autoincr_gl_index_id(),
            GlIndexId {
                cf_id: 7,
                index_id: 100,
            },
        );
    }

    #[test]
    fn get_autoincr_gl_index_id_accepts_hidden_pk() {
        let t = TblDef::new("db.t")
            .unwrap()
            .with_keys(vec![sk(11, 0), hidden_pk(99, 0)]);
        assert_eq!(
            t.get_autoincr_gl_index_id(),
            GlIndexId {
                cf_id: 0,
                index_id: 99,
            },
        );
    }

    #[test]
    #[should_panic(expected = "no primary key")]
    fn get_autoincr_gl_index_id_panics_without_pk() {
        let t = TblDef::new("db.t").unwrap().with_keys(vec![sk(1, 0)]);
        let _ = t.get_autoincr_gl_index_id();
    }

    #[test]
    fn atomic_counters_load_store_and_fetch_max() {
        let t = TblDef::new("db.t").unwrap();
        assert_eq!(t.hidden_pk_val(), 0);
        t.store_hidden_pk_val(50);
        assert_eq!(t.hidden_pk_val(), 50);
        // fetch_max preserves the larger value.
        let prev = t.fetch_max_hidden_pk_val(30);
        assert_eq!(prev, 50);
        assert_eq!(t.hidden_pk_val(), 50);
        // and grows on a larger candidate.
        t.fetch_max_hidden_pk_val(75);
        assert_eq!(t.hidden_pk_val(), 75);

        assert_eq!(t.auto_incr_val(), 0);
        t.store_auto_incr_val(100);
        assert_eq!(t.auto_incr_val(), 100);
        t.fetch_max_auto_incr_val(200);
        assert_eq!(t.auto_incr_val(), 200);
    }

    #[test]
    fn check_and_set_read_free_rpl_table_pins_false() {
        let t = TblDef::new("db.t").unwrap();
        // Pre-set true to make sure the call clears it.
        t.is_read_free_rpl_table.store(true, Ordering::Relaxed);
        t.check_and_set_read_free_rpl_table();
        assert!(!t.is_read_free_rpl_table());
    }

    #[test]
    fn get_create_time_is_null_stub() {
        let t = TblDef::new("db.t").unwrap();
        assert_eq!(t.get_create_time(), CREATE_TIME_NULL);
    }

    #[test]
    fn tbl_def_is_send_and_sync() {
        // Compile-time assertion: callers depend on Arc<TblDef> being
        // shareable across handler threads.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TblDef>();
        assert_send_sync::<Arc<TblDef>>();
    }

    // ----- put_dict integration tests -----

    use crate::codec::dict::{cf_flags, ddl_entry_index_start_number, index_info};
    use crate::codec::key::REVERSE_CF_FLAG;
    use crate::engine::db::EngineDb;

    fn make_kd(
        index_number: u32,
        cf_id: u32,
        ix_type: IndexType,
        is_reverse_cf: bool,
    ) -> Arc<KeyDef> {
        let mut kd = KeyDef::new_skeleton(
            index_number,
            cf_id,
            0,
            INDEX_INFO_VERSION_LATEST as u16,
            ix_type,
            PRIMARY_FORMAT_VERSION_LATEST,
            is_reverse_cf,
            "k",
        );
        kd.maxlength = 12;
        kd.kv_format_version = PRIMARY_FORMAT_VERSION_LATEST;
        Arc::new(kd)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn put_dict_writes_all_three_record_types() {
        let engine = EngineDb::open_in_memory("put_dict_ok").await.expect("open");
        let tdef = TblDef::new("db.users")
            .unwrap()
            .with_keys(vec![
                make_kd(100, 1, IndexType::Primary, false),
                make_kd(101, 1, IndexType::Secondary, false),
            ]);
        tdef.put_dict(engine.db()).await.expect("put_dict");

        // cf_flags row exists for cf_id=1.
        let flags = cf_flags::read(engine.db(), 1)
            .await
            .expect("read flags")
            .expect("present");
        assert_eq!(flags, 0, "no reverse-cf, no per-partition");

        // index_info rows exist for both keys.
        let pk_info = index_info::read(
            engine.db(),
            GlIndexId { cf_id: 1, index_id: 100 },
        )
        .await
        .expect("read pk info")
        .expect("present");
        assert_eq!(pk_info.index_type, IndexType::Primary);

        let sk_info = index_info::read(
            engine.db(),
            GlIndexId { cf_id: 1, index_id: 101 },
        )
        .await
        .expect("read sk info")
        .expect("present");
        assert_eq!(sk_info.index_type, IndexType::Secondary);

        // typed DDL-entry row decodes back to the same index list.
        let ddl_entry = ddl_entry_index_start_number::read_typed(engine.db(), "db.users")
            .await
            .expect("read ddl entry")
            .expect("present");
        assert_eq!(
            ddl_entry,
            vec![
                GlIndexId { cf_id: 1, index_id: 100 },
                GlIndexId { cf_id: 1, index_id: 101 },
            ],
        );

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn put_dict_reverse_cf_flag_is_persisted() {
        let engine = EngineDb::open_in_memory("put_dict_rev").await.expect("open");
        let tdef = TblDef::new("db.t")
            .unwrap()
            .with_keys(vec![make_kd(200, 5, IndexType::Primary, true)]);
        tdef.put_dict(engine.db()).await.expect("put_dict");
        let flags = cf_flags::read(engine.db(), 5)
            .await
            .expect("ok")
            .expect("present");
        assert_eq!(flags, REVERSE_CF_FLAG);
        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn put_dict_rejects_cf_flags_mismatch() {
        // First table on cf=7 establishes flags=0 (no reverse). A
        // second table on cf=7 that claims REVERSE_CF_FLAG must be
        // rejected before any of its rows are written.
        let engine = EngineDb::open_in_memory("put_dict_mismatch")
            .await
            .expect("open");

        let t1 = TblDef::new("db.t1")
            .unwrap()
            .with_keys(vec![make_kd(100, 7, IndexType::Primary, false)]);
        t1.put_dict(engine.db()).await.expect("t1 put_dict");

        let t2 = TblDef::new("db.t2")
            .unwrap()
            .with_keys(vec![make_kd(200, 7, IndexType::Primary, true)]);
        let err = t2.put_dict(engine.db()).await.unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(err.to_string().contains("cf_flags mismatch"));

        // Verify t2's rows were NOT written.
        assert!(index_info::read(
            engine.db(),
            GlIndexId { cf_id: 7, index_id: 200 },
        )
        .await
        .expect("read")
        .is_none());
        assert!(ddl_entry_index_start_number::read_typed(engine.db(), "db.t2")
            .await
            .expect("read")
            .is_none());

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn put_dict_ignores_per_partition_bit_in_validation() {
        // CF_FLAGS_TO_IGNORE = PER_PARTITION_CF_FLAG; a partitioned
        // table joining a non-partitioned CF (or vice versa) must NOT
        // be rejected — matches rdb_datadic.cc:3588..3589.
        let engine = EngineDb::open_in_memory("put_dict_partition_ok")
            .await
            .expect("open");

        // First write installs cf_flags=0.
        let t1 = TblDef::new("db.plain")
            .unwrap()
            .with_keys(vec![make_kd(100, 3, IndexType::Primary, false)]);
        t1.put_dict(engine.db()).await.expect("t1 put_dict");

        // Second table is per-partition (sets PER_PARTITION_CF_FLAG).
        // After masking it should match cf_flags=0 and succeed.
        let mut partitioned_kd = KeyDef::new_skeleton(
            200,
            3,
            0,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Primary,
            PRIMARY_FORMAT_VERSION_LATEST,
            false,
            "k",
        );
        partitioned_kd.is_per_partition_cf = true;
        partitioned_kd.maxlength = 12;
        let t2 = TblDef::new("db.partitioned")
            .unwrap()
            .with_keys(vec![Arc::new(partitioned_kd)]);
        t2.put_dict(engine.db())
            .await
            .expect("partitioned table should NOT trigger cf_flags mismatch");

        engine.close().await.expect("close");
    }

    #[test]
    fn ddl_entry_typed_encode_decode_round_trip() {
        let ids = vec![
            GlIndexId { cf_id: 1, index_id: 100 },
            GlIndexId { cf_id: 2, index_id: 7 },
            GlIndexId { cf_id: u32::MAX, index_id: u32::MAX },
        ];
        let bytes = ddl_entry_index_start_number::encode_value(&ids);
        // 2 (version) + 3 * 8 (pairs).
        assert_eq!(bytes.len(), 2 + 3 * 8);
        let decoded = ddl_entry_index_start_number::decode_value(&bytes).expect("decode");
        assert_eq!(decoded, ids);
    }

    #[test]
    fn ddl_entry_typed_empty_index_list_is_valid() {
        let bytes = ddl_entry_index_start_number::encode_value(&[]);
        assert_eq!(bytes.len(), 2);
        let decoded = ddl_entry_index_start_number::decode_value(&bytes).expect("decode");
        assert!(decoded.is_empty());
    }

    #[test]
    fn ddl_entry_typed_rejects_unsupported_version() {
        let mut bytes = ddl_entry_index_start_number::encode_value(&[]);
        bytes[0..2].copy_from_slice(&99u16.to_be_bytes());
        let err = ddl_entry_index_start_number::decode_value(&bytes).unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Data);
    }

    #[test]
    fn ddl_entry_typed_rejects_misaligned_trailer() {
        // Version (2) + 7 trailing bytes ⇒ not a whole pair.
        let bytes = b"\x00\x01\x00\x00\x00\x01\x00\x00\x00";
        let err = ddl_entry_index_start_number::decode_value(bytes).unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Data);
    }
}
