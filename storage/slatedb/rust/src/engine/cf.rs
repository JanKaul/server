//! CF (column family) → key-prefix mapper.
//!
//! Per `_DESIGN.md §1` SlateDB has exactly one physical CF. Our MyRocks-side
//! notion of a CF is the `cf_id` byte prefix prepended to every key.
//!
//! This module lands the **in-memory subset** of the manager:
//! - [`CfHandle`] — `(cf_id, direction)` replacement for the C++
//!   `ColumnFamilyHandle*`.
//! - [`CfManager`] — `name ↔ id` maps and `next_cf_id` allocator.
//! - [`CfManager::parse_direction`] — the legacy MyRocks `rev:` /
//!   `$per_index_cf$rev` name prefix detector.
//! - In-memory `get_*` lookups.
//!
//! Persistence (`load_from_system_cf`, `get_or_create_cf` persistence,
//! `drop_cf` tombstone) is deferred until the system-CF read/write helper
//! lands — those routines need to scan the `SYSTEM_CF_ID` prefix and write
//! `cf_name → cf_id` rows back through a `WriteBatch`. Until that batch
//! ships, the in-memory bits stand alone for unit testing the manager's
//! bookkeeping logic.

use parking_lot::RwLock;
use slatedb::Error;
use std::collections::HashMap;
use std::sync::Arc;

use crate::engine::comparator::KeyDirection;
use crate::globals::{DEFAULT_CF_NAME, DEFAULT_SYSTEM_CF_NAME, SYSTEM_CF_ID};

/// Minimal CF handle. Replaces RocksDB's `ColumnFamilyHandle*` — there is
/// no underlying SlateDB CF object, just the numeric id plus a direction
/// flag carried for parity with the legacy `rev:`-named CFs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CfHandle {
    pub id: u32,
    pub direction: KeyDirection,
}

/// `name ↔ id` mapper. Persisted in the system CF prefix; this module
/// holds the in-memory cache.
pub struct CfManager {
    #[allow(dead_code)] // wired up once the persistence helper lands
    db: Arc<slatedb::Db>,
    inner: RwLock<Inner>,
}

struct Inner {
    name_to_handle: HashMap<String, CfHandle>,
    id_to_name: HashMap<u32, String>,
    /// Monotonic. Restored from the highest persisted id at startup; until
    /// the persistence helper lands, starts at 1 (0 reserved for default;
    /// `u32::MAX` reserved for the system CF).
    next_cf_id: u32,
}

impl CfManager {
    pub fn new(db: Arc<slatedb::Db>) -> Self {
        let mut name_to_handle = HashMap::new();
        let mut id_to_name = HashMap::new();
        // The default CF is implicit — pre-seed so callers can look it up.
        let default = CfHandle {
            id: 0,
            direction: KeyDirection::Forward,
        };
        name_to_handle.insert(DEFAULT_CF_NAME.to_string(), default);
        id_to_name.insert(0, DEFAULT_CF_NAME.to_string());
        Self {
            db,
            inner: RwLock::new(Inner {
                name_to_handle,
                id_to_name,
                next_cf_id: 1,
            }),
        }
    }

    /// Detect the legacy `cf_$reverse:…` / `rev:…` name prefixes used by
    /// MyRocks to signal a reverse-comparator CF.
    pub fn parse_direction(name: &str) -> KeyDirection {
        if name.starts_with("rev:") || name.starts_with("$per_index_cf$rev") {
            KeyDirection::Reverse
        } else {
            KeyDirection::Forward
        }
    }

    /// Read-only lookup by name.
    pub fn get_cf_by_name(&self, cf_name: &str) -> Option<CfHandle> {
        self.inner.read().name_to_handle.get(cf_name).copied()
    }

    /// Read-only lookup by id.
    pub fn get_cf_by_id(&self, id: u32) -> Option<CfHandle> {
        let g = self.inner.read();
        let name = g.id_to_name.get(&id)?;
        g.name_to_handle.get(name).copied()
    }

    pub fn get_cf_names(&self) -> Vec<String> {
        self.inner.read().name_to_handle.keys().cloned().collect()
    }

    pub fn get_all_cf(&self) -> Vec<CfHandle> {
        self.inner
            .read()
            .name_to_handle
            .values()
            .copied()
            .collect()
    }

    pub fn next_cf_id(&self) -> u32 {
        self.inner.read().next_cf_id
    }

    // ----- in-memory installer used by tests and (later) the persistence
    // load path. Kept crate-public so the system-CF loader can repopulate
    // the map from disk without going through `get_or_create_cf`.

    // Allowed dead_code: the system-CF loader will call this once landed;
    // until then only the tests exercise it.
    #[allow(dead_code)]
    pub(crate) fn install(&self, name: &str, handle: CfHandle) -> Result<(), Error> {
        if name == DEFAULT_SYSTEM_CF_NAME || handle.id == SYSTEM_CF_ID {
            return Err(Error::invalid("system CF name/id is reserved".into()));
        }
        let mut g = self.inner.write();
        if g.name_to_handle.contains_key(name) {
            return Err(Error::invalid(format!(
                "cf '{name}' already installed"
            )));
        }
        if g.id_to_name.contains_key(&handle.id) {
            return Err(Error::invalid(format!(
                "cf id {} already installed",
                handle.id
            )));
        }
        g.name_to_handle.insert(name.to_string(), handle);
        g.id_to_name.insert(handle.id, name.to_string());
        if handle.id >= g.next_cf_id {
            g.next_cf_id = handle.id.saturating_add(1);
        }
        Ok(())
    }

    // ----- persistence-bound routines (system-CF helper not yet landed) -----

    /// Re-populate the maps from the system CF prefix at startup.
    pub async fn load_from_system_cf(&self) -> Result<(), Error> {
        // TRANSLATE-DEFER: needs the system-CF read helper. Will scan
        // `varint(SYSTEM_CF_ID) || u32_be(CF_DEFINITION) || …` rows and
        // call `install()` for each surviving CF.
        Err(Error::internal(
            "CfManager::load_from_system_cf: system-CF helper not landed".into(),
        ))
    }

    /// Look up by name, or allocate a fresh `cf_id`. Persists the mapping
    /// in the system CF.
    pub async fn get_or_create_cf(
        &self,
        cf_name: Option<&str>,
    ) -> Result<CfHandle, Error> {
        let name = cf_name.unwrap_or(DEFAULT_CF_NAME);
        if name == DEFAULT_SYSTEM_CF_NAME {
            return Err(Error::invalid("system CF name is reserved".into()));
        }
        if let Some(h) = self.get_cf_by_name(name) {
            return Ok(h);
        }
        // TRANSLATE-DEFER: needs the system-CF write helper. Body will:
        //   1. allocate next_cf_id under the inner write lock,
        //   2. write `cf_name → handle` to the system CF prefix,
        //   3. call self.install(name, handle).
        Err(Error::internal(
            "CfManager::get_or_create_cf: system-CF helper not landed".into(),
        ))
    }

    /// Soft-drop a CF (writes a tombstone in the system CF).
    pub async fn drop_cf(&self, _cf_name: &str) -> Result<(), Error> {
        // TRANSLATE-DEFER: needs the system-CF write helper.
        Err(Error::internal(
            "CfManager::drop_cf: system-CF helper not landed".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::db::EngineDb;

    async fn fresh_manager(name: &str) -> (EngineDb, CfManager) {
        let engine = EngineDb::open_in_memory(name).await.expect("open");
        let mgr = CfManager::new(Arc::clone(engine.db()));
        (engine, mgr)
    }

    #[test]
    fn parse_direction_recognises_legacy_prefixes() {
        assert_eq!(CfManager::parse_direction("rev:foo"), KeyDirection::Reverse);
        assert_eq!(
            CfManager::parse_direction("$per_index_cf$rev_bar"),
            KeyDirection::Reverse
        );
        assert_eq!(CfManager::parse_direction("default"), KeyDirection::Forward);
        assert_eq!(CfManager::parse_direction(""), KeyDirection::Forward);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn default_cf_is_pre_seeded() {
        let (engine, mgr) = fresh_manager("cf_default").await;
        let h = mgr.get_cf_by_name(DEFAULT_CF_NAME).expect("seeded");
        assert_eq!(h.id, 0);
        assert_eq!(h.direction, KeyDirection::Forward);
        assert_eq!(mgr.get_cf_by_id(0), Some(h));
        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn install_records_both_directions_and_bumps_next_id() {
        let (engine, mgr) = fresh_manager("cf_install").await;
        mgr.install(
            "user_cf",
            CfHandle {
                id: 5,
                direction: KeyDirection::Reverse,
            },
        )
        .expect("install");
        assert_eq!(mgr.get_cf_by_name("user_cf").map(|h| h.id), Some(5));
        assert_eq!(
            mgr.get_cf_by_id(5).map(|h| h.direction),
            Some(KeyDirection::Reverse)
        );
        assert_eq!(mgr.next_cf_id(), 6);
        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn install_rejects_duplicates_and_system_ids() {
        let (engine, mgr) = fresh_manager("cf_dup").await;
        mgr.install(
            "first",
            CfHandle {
                id: 1,
                direction: KeyDirection::Forward,
            },
        )
        .expect("install");
        // Duplicate name:
        assert!(mgr
            .install(
                "first",
                CfHandle {
                    id: 99,
                    direction: KeyDirection::Forward,
                },
            )
            .is_err());
        // Duplicate id:
        assert!(mgr
            .install(
                "second",
                CfHandle {
                    id: 1,
                    direction: KeyDirection::Forward,
                },
            )
            .is_err());
        // System id reserved:
        assert!(mgr
            .install(
                "third",
                CfHandle {
                    id: SYSTEM_CF_ID,
                    direction: KeyDirection::Forward,
                },
            )
            .is_err());
        // System name reserved:
        assert!(mgr
            .install(
                DEFAULT_SYSTEM_CF_NAME,
                CfHandle {
                    id: 7,
                    direction: KeyDirection::Forward,
                },
            )
            .is_err());
        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deferred_persistence_methods_surface_a_clear_error() {
        let (engine, mgr) = fresh_manager("cf_deferred").await;
        assert!(mgr.load_from_system_cf().await.is_err());
        assert!(mgr.get_or_create_cf(Some("brand_new")).await.is_err());
        assert!(mgr.drop_cf("brand_new").await.is_err());
        // Pre-existing default CF still resolves without touching persistence:
        assert!(mgr.get_or_create_cf(Some(DEFAULT_CF_NAME)).await.is_ok());
        engine.close().await.expect("close");
    }
}
