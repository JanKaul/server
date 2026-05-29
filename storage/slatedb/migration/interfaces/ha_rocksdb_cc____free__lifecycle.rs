//! Interface stub for `ha_rocksdb_cc____free__lifecycle`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (sub-unit span 5139..6329)
//! v4 manifest sub-unit: `ha_rocksdb_cc____free__lifecycle`
//!
//! ## Mapping
//! **THIS IS WHERE THE SlateDB `Db` IS CONSTRUCTED.**
//!
//! `rocksdb_init_func` is the MariaDB handlerton init callback. It:
//!   1. validates the configured corruption marker
//!   2. parses sysvars (rocksdb_datadir, block cache size, compression, etc.)
//!   3. **builds the SlateDB `Db`** via `Db::builder(path, object_store)`
//!      with `.with_settings(...)`, `.with_merge_operator(...)` (the
//!      MyRocks counter merge, per _DESIGN.md §1), `.with_compaction_filter_supplier(...)`
//!      (the dropped-index sweep, per §1), `.with_block_cache(...)`,
//!      `.with_compactor_builder(...)`, and `.build()`
//!   4. spawns the Tokio runtime (per §7, one runtime per handlerton)
//!   5. spawns the background tasks (`StatsRefreshTask`, drop-index,
//!      manual-compaction — per §1 row "RocksDB event listener" and
//!      "Background threads", these are Tokio tasks coordinating WITH
//!      SlateDB's native compactor, not replacing it)
//!   6. loads the data dictionary from the SYSTEM CF prefix
//!
//! `rocksdb_done_func` is the symmetric tear-down: cancels Tokio tasks,
//! awaits in-flight commits, calls `Db::close()`, drops the runtime.
//!
//! `rocksdb_create_handler` is the per-table-instance handler factory; the
//! returned object is the cxx `ha_slatedb` C++ shim that holds an
//! `Arc<engine::Engine>`.
//!
//! ## Out-of-scope methods
//! - `check_rocksdb_options_compatibility` — checks compatibility of saved
//!   RocksDB OPTIONS file with the in-use options. Per _DESIGN.md §1 row
//!   "Column families → key-prefix" we have no OPTIONS file; this method
//!   logs a one-time deprecation warning and returns `Ok(())`.

use slatedb::Error;

/// Opaque handlerton pointer (`my_core::handlerton*`). The cxx bridge
/// translates from/to this.
pub struct OpaqueHandlerton(pub *mut ());

unsafe impl Send for OpaqueHandlerton {}
unsafe impl Sync for OpaqueHandlerton {}

/// Opaque handler instance pointer (`handler*`). Returned by
/// `create_handler` — wraps the cxx `ha_slatedb` shim.
pub struct OpaqueHandler(pub *mut ());

unsafe impl Send for OpaqueHandler {}
unsafe impl Sync for OpaqueHandler {}

/// Opaque optimizer-cost descriptor (`OPTIMIZER_COSTS*`). The cxx bridge
/// populates the fields directly; this is a marker type only.
pub struct OpaqueOptimizerCosts(pub *mut ());

// -----------------------------------------------------------------------
// Pre-init compatibility checks (no-ops in SlateDB, kept for surface parity)
// -----------------------------------------------------------------------

/// Verify saved RocksDB OPTIONS file matches the live options. Per design,
/// SlateDB has no OPTIONS-file concept; we always succeed.
///
/// Original: ha_rocksdb.cc:5139 — `check_rocksdb_options_compatibility`.
pub fn check_rocksdb_options_compatibility() -> Result<(), Error> {
    // SEE OUT-OF-SCOPE NOTE
    Ok(())
}

/// Version check at startup. Reads the persisted engine-version marker
/// from the system CF and refuses to start if it's newer than ours
/// (forward-incompatible) or not in our supported range.
///
/// Returns `slatedb::ErrorKind::Invalid` on version mismatch — fatal.
/// Returns `slatedb::ErrorKind::Unavailable` if the marker can't be read.
///
/// Original: ha_rocksdb.cc:5194 — `rocksdb_check_version`.
pub fn check_version(hton: &OpaqueHandlerton) -> Result<(), Error> {
    let _ = hton;
    todo!("read SYSTEM_CF_ID:engine_version; compare to PLUGIN_VERSION; Err(Invalid) on mismatch")
}

/// Populate the optimizer-cost table (read / write / index-scan microcosts).
/// In MyRocks these are mostly hard-coded; we mirror those numbers and
/// adjust write-cost upward because SlateDB has higher per-put overhead
/// than RocksDB's LSM (object-store round-trip vs local disk).
///
/// Original: ha_rocksdb.cc:5213 — `rocksdb_update_optimizer_costs`.
pub fn update_optimizer_costs(costs: &mut OpaqueOptimizerCosts) {
    let _ = costs;
    todo!("fill costs.{disk_read_cost, key_lookup_cost, ...} with SlateDB-tuned values")
}

// -----------------------------------------------------------------------
// Handlerton init / done — THE ENGINE LIFECYCLE
// -----------------------------------------------------------------------

/// Build the SlateDB `Db`, initialize the Tokio runtime, spawn maintenance
/// tasks, load the data dictionary, and wire all handlerton callbacks.
///
/// Failure of any step returns `Err`, and the caller (`ha_rocksdb` plugin
/// shim) must surface it to MariaDB so the plugin load fails cleanly.
///
/// Error mapping (per _DESIGN.md §4):
///   - `slatedb::ErrorKind::Invalid` → bad config (block cache too small,
///     unknown compression codec, etc.) — surfaced as plugin-init failure
///   - `slatedb::ErrorKind::Unavailable` → object store unreachable
///   - `slatedb::ErrorKind::Data` → corruption marker present and
///     `rocksdb_allow_to_start_after_corruption=OFF` — fatal, mariadbd exits
///
/// Original: ha_rocksdb.cc:5228 — `rocksdb_init_func`.
pub fn init_func(hton: &OpaqueHandlerton) -> Result<(), Error> {
    let _ = hton;
    todo!(
        "build object_store from sysvars; \
         build Settings from sysvars; \
         spawn Tokio Runtime (per §7); \
         Db::builder(path, object_store) \
             .with_settings(settings) \
             .with_merge_operator(Arc::new(MyRocksMergeOp)) \
             .with_compaction_filter_supplier(Arc::new(DroppedIndexFilterSupplier)) \
             .with_block_cache(block_cache) \
             .with_compactor_builder(CompactorBuilder::default()) \
             .build() \
             .await?; \
         spawn StatsRefreshTask, drop_index_thread, manual_compaction_thread; \
         load DDL manager from SYSTEM CF; \
         wire handlerton callbacks (close_connection, prepare, commit, ...)"
    )
}

/// Cancel all spawned tasks, drain in-flight commits, close the Db, and
/// shut down the Tokio runtime. Symmetric to `init_func`.
///
/// Idempotent — safe to call twice (subsequent calls return Ok).
///
/// Original: ha_rocksdb.cc:5804 — `rocksdb_done_func`.
pub fn done_func(hton: &OpaqueHandlerton) -> Result<(), Error> {
    let _ = hton;
    todo!(
        "cancel_token.cancel(); \
         await all maintenance tasks; \
         db.close().await?; \
         drop(runtime); \
         clear OnceLock<Engine>"
    )
}

// -----------------------------------------------------------------------
// Per-table handler factory
// -----------------------------------------------------------------------

/// Returns a new `ha_slatedb` handler instance for `table_name`. Called
/// by MariaDB once per opened table.
///
/// The returned `OpaqueHandler` is a heap-allocated cxx shim holding an
/// `Arc<engine::Engine>` plus a `Rdb_table_handler` from
/// `Rdb_open_tables_map::get_table_handler`.
///
/// Original: ha_rocksdb.cc:222 / 6325 — `rocksdb_create_handler`.
pub fn create_handler(hton: &OpaqueHandlerton, table_name: &str) -> Result<OpaqueHandler, Error> {
    let _ = (hton, table_name);
    todo!("Rdb_open_tables_map::get_table_handler(table_name); allocate ha_slatedb shim; return")
}
