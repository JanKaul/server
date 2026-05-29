//! Interface stub for `rdb_cf_manager_h`.
//!
//! C++ source: `storage/rocksdb/rdb_cf_manager.h` (108 LoC)
//! C++ class: `Rdb_cf_manager`
//!
//! ## Mapping
//! In MyRocks the CF manager owns the `ColumnFamilyHandle*` for every CF,
//! creates new CFs on demand from `Db::CreateColumnFamily`, and tracks them
//! by both name and numeric id.
//!
//! Per _DESIGN.md §1 ("Column families → Key-prefix scheme") **SlateDB has
//! exactly one CF** — our notion of a CF is a `varint(cf_id)` byte prefix
//! that's prepended to every key (per §2). So this class collapses to:
//!
//! - A bijective `cf_name ↔ cf_id` map persisted in the system CF prefix
//!   (`SYSTEM_CF_ID = u32::MAX`, see `rdb_global_h`).
//! - A monotonic id allocator (`next_cf_id`) for CREATE TABLE.
//! - No real "drop" — `drop_cf` becomes a soft-tombstone in the dictionary
//!   that the compaction filter (`rdb_compact_filter_h`) sweeps later.
//!
//! `get_or_create_cf` does NOT touch SlateDB — there's nothing to create.
//! It only allocates a `cf_id` and writes the name→id mapping to the system
//! prefix.
//!
//! ## Out-of-scope methods
//! - `init(handles)` (with a vector of `ColumnFamilyHandle*`): there are no
//!   handles in our world. Replaced by `load_from_system_cf(...)` which
//!   re-reads persisted name→id mappings.
//! - `is_cf_name_reverse(name)`: the `cf_$reverse:...` name convention is
//!   preserved as a sysvar-driven parse step but maps to `KeyDirection`
//!   on the index def, not to a per-CF comparator. We expose it as a
//!   helper.

use slatedb::Error;
use std::sync::Arc;

/// Minimal "CF handle" in our world: just the numeric id, plus a flag for
/// reverse-ordered comparator semantics (which now lives on the index, not
/// the CF — kept here for parity). Replaces RocksDB's
/// `ColumnFamilyHandle*` everywhere it appeared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CfHandle {
    pub id: u32,
    pub direction: crate::rdb_comparator_h::KeyDirection,
}

/// Maps `cf_name ↔ cf_id`. Persisted in the system CF prefix. Replaces
/// `Rdb_cf_manager`.
pub struct CfManager {
    /// Underlying SlateDB handle — read so that we can fetch the persisted
    /// name→id rows from the system CF prefix at startup.
    db: Arc<slatedb::Db>,
    cf_options: crate::rdb_cf_options_h::CfOptions,
    inner: parking_lot::RwLock<Inner>,
}

struct Inner {
    name_to_id: std::collections::HashMap<String, CfHandle>,
    id_to_name: std::collections::HashMap<u32, String>,
    /// Monotonically increasing; next CREATE TABLE-allocated CF gets this id.
    /// Restored from the highest persisted id at startup.
    next_cf_id: u32,
}

impl CfManager {
    pub fn new(db: Arc<slatedb::Db>, cf_options: crate::rdb_cf_options_h::CfOptions) -> Self {
        Self {
            db,
            cf_options,
            inner: parking_lot::RwLock::new(Inner {
                name_to_id: std::collections::HashMap::new(),
                id_to_name: std::collections::HashMap::new(),
                next_cf_id: 1, // 0 reserved for default; u32::MAX reserved for system.
            }),
        }
    }

    /// Detects the legacy `cf_$reverse:...` and `rev:...` name prefixes used
    /// by MyRocks to signal a reverse-comparator CF. C++
    /// `is_cf_name_reverse`. Returns `Reverse` if so, `Forward` otherwise.
    pub fn parse_direction(name: &str) -> crate::rdb_comparator_h::KeyDirection {
        if name.starts_with("rev:") || name.starts_with("$per_index_cf$rev") {
            crate::rdb_comparator_h::KeyDirection::Reverse
        } else {
            crate::rdb_comparator_h::KeyDirection::Forward
        }
    }

    /// Re-populate the `name ↔ id` maps from the system CF prefix at startup.
    /// Replaces `Rdb_cf_manager::init` (which took `ColumnFamilyHandle*`).
    pub async fn load_from_system_cf(&self) -> Result<(), Error> {
        let _ = &self.db;
        todo!("scan `SYSTEM_CF_ID || 'cf:'` prefix, populate inner; restore next_cf_id from max")
    }

    /// Look up by name, or allocate a fresh `cf_id` and persist the mapping.
    /// Called by CREATE TABLE. `cf_name=None` means the default CF
    /// (`DEFAULT_CF_NAME`, id 0).
    ///
    /// Errors: `Unavailable` on object-store I/O, `Invalid` on name conflict
    /// with the reserved system CF.
    pub async fn get_or_create_cf(
        &self,
        cf_name: Option<&str>,
    ) -> Result<CfHandle, Error> {
        let name = cf_name.unwrap_or(crate::rdb_global_h::DEFAULT_CF_NAME);
        if name == crate::rdb_global_h::DEFAULT_SYSTEM_CF_NAME {
            return Err(Error::invalid("system CF name is reserved".into()));
        }
        // Fast path: existing.
        if let Some(h) = self.inner.read().name_to_id.get(name).copied() {
            return Ok(h);
        }
        // Slow path: allocate id, persist mapping, install in map.
        let _ = &self.cf_options;
        todo!("allocate next_cf_id, persist cf_name→cf_id row in system CF, insert into both maps")
    }

    /// Read-only lookup by name. C++ `get_cf(name, lock_held_by_caller)`.
    pub fn get_cf_by_name(&self, cf_name: &str) -> Option<CfHandle> {
        self.inner.read().name_to_id.get(cf_name).copied()
    }

    /// Read-only lookup by id. C++ `get_cf(uint32_t id)`.
    pub fn get_cf_by_id(&self, id: u32) -> Option<CfHandle> {
        // Walk: id→name to confirm exists, then direction comes from the
        // persisted snapshot — we always know `direction` at insert time, so
        // a single id→name + back-resolve is fine.
        let g = self.inner.read();
        let name = g.id_to_name.get(&id)?;
        g.name_to_id.get(name).copied()
    }

    pub fn get_cf_names(&self) -> Vec<String> {
        self.inner.read().name_to_id.keys().cloned().collect()
    }

    pub fn get_all_cf(&self) -> Vec<CfHandle> {
        self.inner.read().name_to_id.values().copied().collect()
    }

    /// Soft-drop a CF. Writes a tombstone in the system CF; the actual key
    /// sweep happens later via the compaction filter (`rdb_compact_filter_h`).
    ///
    /// Errors: `Invalid` if the CF doesn't exist; `Unavailable` on I/O.
    pub async fn drop_cf(&self, cf_name: &str) -> Result<(), Error> {
        let _ = cf_name;
        todo!("look up id, write drop-tombstone in system CF, optionally remove from maps")
    }

    /// Re-export of the options getter — the bridge layer calls this directly.
    pub fn get_cf_options(&self, cf_name: &str) -> crate::rdb_cf_options_h::CfOptionsSnapshot {
        self.cf_options.get(cf_name)
    }

    pub fn update_options(
        &mut self,
        cf_name: &str,
        new_options: &str,
    ) -> Result<(), Error> {
        self.cf_options.update(cf_name, new_options)
    }
}
