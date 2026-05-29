//! Interface stub for `rdb_cf_manager_cc`.
//!
//! C++ source: `storage/rocksdb/rdb_cf_manager.cc` (269 LoC)
//! C++ class:  `Rdb_cf_manager` (impl)
//!
//! ## Mapping
//! Per _DESIGN.md §1 ("Column families" row — Map, native support via key
//! prefixes): MyRocks used one RocksDB CF per index group; we use one SlateDB
//! instance with a `varint(cf_id) || u32_be(index_id) || ...` key-prefix
//! scheme. The CF manager therefore becomes a pure in-memory bookkeeping
//! struct that maps `cf_name <-> cf_id`. There are no real RocksDB CF handles
//! to create or drop; "creating a CF" allocates a new `cf_id` and writes a
//! dictionary entry, "dropping a CF" enqueues a `CompactionFilter` sweep
//! (see `rdb_compact_filter_cc.rs`).
//!
//! Per-CF tuning options are silently ignored (per the special-mapping note
//! in the task contract): SlateDB has global `Settings` only. Our `get_cf`
//! returns the same metadata for every CF; the codec uses it for prefix
//! extraction, not for opening separate column families.
//!
//! ## Out-of-scope methods
//! - `get_or_create_cf` returning a `rocksdb::ColumnFamilyHandle*` — replaced
//!   by returning a `CfDescriptor` (id + name + direction). No native handle.
//! - `DropColumnFamily` on RocksDB — replaced by marking the CF as
//!   drop-pending in the dict and letting the compaction filter sweep it.
//! - Per-CF `ColumnFamilyOptions` — silently ignored. The factory still
//!   accepts them so old CREATE-TABLE comments don't error, but only
//!   `cf_name` is honored.

use crate::rdb_comparator_h::KeyDirection;
use crate::rdb_global_h::{DEFAULT_CF_NAME, DEFAULT_SYSTEM_CF_NAME, PER_INDEX_CF_NAME, SYSTEM_CF_ID};
use parking_lot::Mutex;
use slatedb::Error;
use std::collections::HashMap;

/// Minimal CF metadata. Replaces `rocksdb::ColumnFamilyHandle*` everywhere it
/// appeared as a return type. No live RocksDB resource is implied.
///
/// Original: this didn't exist in MyRocks — it's the residual data we still
/// need after collapsing CFs into key prefixes.
#[derive(Debug, Clone)]
pub struct CfDescriptor {
    pub id: u32,
    pub name: String,
    /// `Reverse` only if the user opted in via the `rev:` name prefix at
    /// CREATE TABLE time. Drives the codec's XOR direction (see
    /// `rdb_comparator_h::KeyDirection`).
    pub direction: KeyDirection,
}

/// In-memory CF registry. Mutex protects both maps; per _DESIGN.md special
/// mapping for `rdb_mutex_wrapper_cc` we use `parking_lot::Mutex`.
///
/// Original: rdb_cf_manager.cc — `Rdb_cf_manager`.
pub struct CfManager {
    inner: Mutex<CfManagerInner>,
}

struct CfManagerInner {
    by_name: HashMap<String, CfDescriptor>,
    by_id: HashMap<u32, CfDescriptor>,
    next_id: u32,
}

impl CfManager {
    /// Bootstrap with the default + system CFs. Replaces
    /// `Rdb_cf_manager::init(cf_options, handles)`.
    /// Original: rdb_cf_manager.cc:44 — `init`.
    pub fn new() -> Self {
        // TODO(human): pick the initial `next_id` from the dict on warm restart
        // — for cold init, 0 = default, u32::MAX = system, others assigned.
        let mut by_name = HashMap::new();
        let mut by_id = HashMap::new();
        let default_cf = CfDescriptor {
            id: 0,
            name: DEFAULT_CF_NAME.to_string(),
            direction: KeyDirection::Forward,
        };
        let system_cf = CfDescriptor {
            id: SYSTEM_CF_ID,
            name: DEFAULT_SYSTEM_CF_NAME.to_string(),
            direction: KeyDirection::Forward,
        };
        by_name.insert(default_cf.name.clone(), default_cf.clone());
        by_id.insert(default_cf.id, default_cf);
        by_name.insert(system_cf.name.clone(), system_cf.clone());
        by_id.insert(system_cf.id, system_cf);
        Self {
            inner: Mutex::new(CfManagerInner { by_name, by_id, next_id: 1 }),
        }
    }

    /// True if the CF name starts with the legacy `rev:` prefix used to
    /// declare a reverse-ordered CF.
    /// Original: rdb_cf_manager.cc:38 — `is_cf_name_reverse`.
    pub fn is_cf_name_reverse(name: &str) -> bool {
        name.starts_with("rev:")
    }

    /// Find by name, creating a new descriptor if missing. The
    /// `PER_INDEX_CF_NAME` legacy marker triggers `ErrorKind::Invalid` to
    /// preserve MyRocks' `ER_PER_INDEX_CF_DEPRECATED` behavior.
    ///
    /// Inputs:
    /// - `cf_name`: user CF name or empty string (treated as default).
    ///
    /// Output: cloned `CfDescriptor` for use by the codec.
    ///
    /// Errors: `Invalid` for `PER_INDEX_CF_NAME`. Never returns
    /// `Unavailable` — this is all in-memory.
    ///
    /// Original: rdb_cf_manager.cc:77 — `get_or_create_cf`.
    pub fn get_or_create_cf(&self, cf_name: &str) -> Result<CfDescriptor, Error> {
        if cf_name == PER_INDEX_CF_NAME {
            return Err(Error::invalid(format!(
                "per-index CFs deprecated; cf '{}' not allowed",
                cf_name
            )));
        }
        let _ = cf_name;
        todo!("lookup-or-insert; assign next_id; direction from is_cf_name_reverse")
    }

    /// Lookup by name; never creates. Returns `None` for unknown CF.
    /// Original: rdb_cf_manager.cc:133 — `get_cf(name)`.
    pub fn get_cf_by_name(&self, cf_name: &str) -> Option<CfDescriptor> {
        let key = if cf_name.is_empty() { DEFAULT_CF_NAME } else { cf_name };
        self.inner.lock().by_name.get(key).cloned()
    }

    /// Lookup by id.
    /// Original: rdb_cf_manager.cc:157 — `get_cf(id)`.
    pub fn get_cf_by_id(&self, id: u32) -> Option<CfDescriptor> {
        self.inner.lock().by_id.get(&id).cloned()
    }

    /// All registered CF names. Order undefined.
    /// Original: rdb_cf_manager.cc:168 — `get_cf_names`.
    pub fn get_cf_names(&self) -> Vec<String> {
        self.inner.lock().by_name.keys().cloned().collect()
    }

    /// All registered CFs, cloned.
    /// Original: rdb_cf_manager.cc:180 — `get_all_cf`.
    pub fn get_all_cf(&self) -> Vec<CfDescriptor> {
        self.inner.lock().by_id.values().cloned().collect()
    }

    /// Drop a CF. In SlateDB this:
    /// 1. Refuses if `cf_name == DEFAULT_SYSTEM_CF_NAME`.
    /// 2. Confirms no live index points at this CF (caller's responsibility
    ///    to pre-scan the DDL manager).
    /// 3. Marks the CF id as drop-pending in the dict — the next compaction
    ///    will see this and the compaction filter will `Drop` matching
    ///    entries (see `rdb_compact_filter_cc.rs`).
    ///
    /// Inputs: `cf_name`.
    /// Output: `Ok(())` if accepted; CF is removed from the in-memory maps.
    /// Errors: `Invalid` for the system CF or unknown CF.
    ///
    /// Original: rdb_cf_manager.cc:218 — `drop_cf`.
    pub async fn drop_cf(&self, cf_name: &str) -> Result<(), Error> {
        if cf_name == DEFAULT_SYSTEM_CF_NAME {
            return Err(Error::invalid("cannot drop system CF".into()));
        }
        let _ = cf_name;
        todo!("scan ddl_manager for live index in this CF; if free, mark drop-pending in dict and erase from maps")
    }

    /// Tear down. Drops all in-memory state. Equivalent to `cleanup()` in C++
    /// (which deleted the `rocksdb::ColumnFamilyHandle*`s — we have none).
    /// Original: rdb_cf_manager.cc:62 — `cleanup`.
    pub fn cleanup(&self) {
        let mut g = self.inner.lock();
        g.by_name.clear();
        g.by_id.clear();
    }
}

impl Default for CfManager {
    fn default() -> Self { Self::new() }
}
