//! Interface stub for `ha_rocksdb_cc__Rdb_open_tables_map`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 337..361 + 5981..6048 + 6307..6322, ~120 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__Rdb_open_tables_map`
//!
//! ## Mapping
//! Engine-side `name → Rdb_table_handler` cache used to share per-table
//! mutexes, perf counters, and thr_lock across all `ha_rocksdb` instances
//! pointing at the same table. **No SlateDB API involvement** — this is
//! purely a process-local registry. Direct translation to
//! `Mutex<HashMap<String, Arc<RdbTableHandler>>>`.
//!
//! Ref-counting on the C++ side via `m_ref_count`; in Rust this is the
//! `Arc` strong count — we drop the entry from the map when the last `Arc`
//! is released. We model that via a `Weak`-valued map plus an explicit
//! `release_table_handler` so the C++-style lifecycle is preserved.
//!
//! Per _DESIGN.md §1 (no row touches this — purely internal bookkeeping).
//!
//! ## Out-of-scope methods
//! None — fully in scope.

use slatedb::Error;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::ha_rocksdb_cc__Rdb_transaction::RdbTableHandler;

/// `name → handler` registry. Insertion is lazy (on first `get_table_handler`).
pub struct RdbOpenTablesMap {
    inner: Mutex<HashMap<String, Arc<RdbTableHandler>>>,
}

impl RdbOpenTablesMap {
    /// Construct empty. Original: implicit C++ default-ctor; `init` is the
    /// PSI mutex registration which is unnecessary in Rust.
    pub fn new() -> Self {
        Self { inner: Mutex::new(HashMap::new()) }
    }

    /// Lookup-or-insert. Returns an `Arc` so each ha_rocksdb instance gets a
    /// strong ref; releasing is implicit via `Arc::drop`. The optional manual
    /// `release_table_handler` path is preserved for symmetry with C++.
    ///
    /// `table_name` is the MyRocks "dbname.tablename" form (already normalized
    /// by `rdb_normalize_tablename`).
    ///
    /// Errors: `Unavailable` if the registry mutex is poisoned (should be
    /// unreachable in non-panicking code).
    ///
    /// Original: ha_rocksdb.cc:5981 — `get_table_handler`.
    pub fn get_table_handler(&self, table_name: &str) -> Result<Arc<RdbTableHandler>, Error> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| Error::invalid("Rdb_open_tables_map mutex poisoned".into()))?;
        if let Some(existing) = guard.get(table_name) {
            return Ok(Arc::clone(existing));
        }
        let _ = guard;
        todo!("allocate a fresh RdbTableHandler, insert with table_name.to_owned(), return Arc::clone")
    }

    /// Explicit release. In Rust this is normally implicit (drop the Arc),
    /// but we keep the entrypoint because the C++ engine calls it in tight
    /// loops where we may want to also evict the map entry deterministically.
    ///
    /// Original: ha_rocksdb.cc:6307 — `release_table_handler`.
    pub fn release_table_handler(&self, handler: &Arc<RdbTableHandler>) -> Result<(), Error> {
        let _ = handler;
        todo!("if Arc::strong_count(handler) == 2 (us + caller) -> remove from map")
    }

    /// Snapshot of all currently-open table names. Used by the stats refresher
    /// and by I_S population.
    ///
    /// Original: ha_rocksdb.cc:6034 — `get_table_names`.
    pub fn table_names(&self) -> Result<Vec<String>, Error> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| Error::invalid("Rdb_open_tables_map mutex poisoned".into()))?;
        Ok(guard.keys().cloned().collect())
    }

    /// Number of currently-tracked open tables.
    pub fn count(&self) -> usize {
        self.inner.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Clear all entries; called from `plugin::done`.
    /// Original: ha_rocksdb.cc:351 — `free`.
    pub fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() { g.clear(); }
    }
}

impl Default for RdbOpenTablesMap {
    fn default() -> Self { Self::new() }
}
