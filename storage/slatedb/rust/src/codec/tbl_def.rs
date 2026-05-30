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
//! - **`put_dict`** — writes the per-table DDL entry plus per-index
//!   `Rdb_index_info` rows to the dict via the dict manager. Needs
//!   `cf_flags` get/set on the dict (not yet translated) and a
//!   WriteBatch surface — wait on the dict_manager landing.
//! - **`get_create_time`** — reads the table's `.frm` ctime. We don't
//!   have `.frm` files in SlateDB; this stub returns
//!   [`CREATE_TIME_NULL`] (matching the C++ "no data available" branch
//!   when `my_stat` fails). The lazy-load-from-filesystem behaviour
//!   isn't applicable to an object-store-backed deployment.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use slatedb::Error;

use crate::codec::key::{IndexType, KeyDef};
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

    pub fn auto_incr_val(&self) -> u64 {
        self.auto_incr_val.load(Ordering::Relaxed)
    }
    pub fn store_auto_incr_val(&self, v: u64) {
        self.auto_incr_val.store(v, Ordering::Relaxed);
    }
    pub fn fetch_max_auto_incr_val(&self, v: u64) -> u64 {
        self.auto_incr_val.fetch_max(v, Ordering::Relaxed)
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
}
