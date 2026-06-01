//! C++ ↔ Rust FFI surface.
//!
//! Currently a **minimum-viable scope** — three lifecycle calls plus the
//! pre-existing `slatedb_version()`. Designed to prove the wiring works
//! (cxx bridge + tokio runtime + global engine state + dict-integrated
//! catalogue) end-to-end before we commit to the larger handler-bucket
//! surface.
//!
//! ## What's exposed today
//!
//! | C++ entry point              | Rust delegate                | Purpose |
//! |------------------------------|------------------------------|---------|
//! | `slatedb::slatedb_version()` | [`slatedb_version`]          | Library identity |
//! | `slatedb::init_in_memory(name)` | [`slatedb_init_in_memory`] | Boot runtime + open in-memory engine + empty DdlManager |
//! | `slatedb::shutdown()`        | [`slatedb_shutdown`]         | Close the engine; runtime stays installed (singleton) |
//! | `slatedb::has_table(name)`   | [`slatedb_has_table`]        | Catalogue probe — useful for smoke tests from C++ |
//!
//! ## Design notes
//!
//! ### Global state
//!
//! [`ENGINE`] is a `parking_lot::RwLock<Option<EngineState>>` — set by
//! `init_in_memory`, cleared by `shutdown`. We don't use `OnceLock`
//! because MariaDB plugin lifecycle wants reentrant init/shutdown (load
//! plugin → uninstall plugin → reload plugin within the same process).
//!
//! The runtime itself ([`crate::runtime`]) is a true singleton via
//! `OnceLock` — once installed it can't be torn down. That's fine: a
//! tokio runtime with empty workqueues is cheap to leave running.
//!
//! ### Async → sync boundary
//!
//! Every bridge function that touches the engine wraps an async block
//! in [`crate::runtime::block_on`]. This blocks the calling MariaDB
//! handler thread on the tokio runtime — exactly the pattern the
//! runtime module documents in its top doc-comment.
//!
//! ### Return codes
//!
//! `i32` status codes, defined as constants in [`status`]. `0` = OK,
//! non-zero = failure. Failure modes are coarse-grained at this stage
//! (init / double-init / io); richer error info will flow when the
//! handler surface lands and we have a stable error → HA_ERR
//! translation channel (we have [`crate::error::slatedb_error_to_ha_err`]
//! already, but it expects a live `slatedb::Error`, not an integer
//! round-trip).
//!
//! ### What's NOT exposed
//!
//! Anything involving the codec, transactions, or row-level operations.
//! Those land alongside the handler buckets. The MVS deliberately keeps
//! the cxx surface to the smallest thing that proves the lifecycle
//! works.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::engine::db::EngineDb;
use crate::engine::ddl_manager::DdlManager;
use crate::engine::txn_registry::TxnRegistry;

#[cxx::bridge(namespace = "slatedb")]
mod ffi {
    extern "Rust" {
        /// Library identity string. Always succeeds.
        fn slatedb_version() -> String;

        /// Boot the runtime + open a fresh in-memory engine + install
        /// an empty DdlManager. Returns 0 on success; see
        /// [`super::status`] for error codes.
        fn slatedb_init_in_memory(name: String) -> i32;

        /// Close the engine. Idempotent — returns 0 if no engine is
        /// installed. The tokio runtime stays installed (singleton).
        fn slatedb_shutdown() -> i32;

        /// `true` iff a table with `name` is in the catalogue. Returns
        /// `false` if no engine is installed (uninited / post-shutdown).
        fn slatedb_has_table(name: String) -> bool;

        // ----- per-handler lifecycle -----
        //
        // Opaque `HaSlateDb` handle owned on the C++ side as
        // `unique_ptr<HaSlateDb>`. C++ calls `new_ha_slatedb()` once
        // per (THD, table) and then drives `ha_open` / `ha_close`.

        type HaSlateDb;

        /// Construct a fresh handler instance. Always succeeds.
        fn new_ha_slatedb() -> Box<HaSlateDb>;

        /// Bind the handler to the table at `name` (MariaDB on-disk
        /// path form, e.g. `./db/tbl` or `./db/tbl#P#part`).
        ///
        /// Returns 0 on success; see [`super::handler::status`] for
        /// failure codes (NO_ENGINE / BAD_TABLE_PATH /
        /// NO_SUCH_TABLE / ENGINE_IO_FAILED).
        fn ha_open(self: &mut HaSlateDb, name: String) -> i32;

        /// Release the handler's per-table state. Idempotent.
        /// Always returns 0.
        fn ha_close(self: &mut HaSlateDb) -> i32;

        /// Decide the row-lock mode + (possibly downgraded) THR_LOCK
        /// type for this statement. Returns the chosen
        /// `thr_lock_type` as `i32` (the cxx side maps back to the
        /// C++ enum). Side effect: updates `lock_rows` +
        /// `db_lock_type` on the handler.
        ///
        /// `in_lock_tables` is true when the THD is inside an
        /// explicit `LOCK TABLES`. `tablespace_op` is true for
        /// `DISCARD/IMPORT TABLESPACE`. Both come from THD reads on
        /// the cxx side (`thd_in_lock_tables` / `thd_tablespace_op`).
        ///
        /// `requested_lock_type` is the raw `enum thr_lock_type`
        /// value; unknown values collapse to `TL_IGNORE` (which is
        /// the C++'s "leave the decision alone" sentinel).
        fn ha_store_lock(
            self: &mut HaSlateDb,
            in_lock_tables: bool,
            tablespace_op: bool,
            requested_lock_type: i32,
        ) -> i32;

        /// Capability/hint toggle. `extra_op` is the raw
        /// `enum ha_extra_function` value from `include/my_base.h`;
        /// unknown values become a silent no-op. Always returns 0.
        fn ha_extra(self: &mut HaSlateDb, extra_op: i32) -> i32;

        /// Statement-boundary hook. `lock_type` is the raw
        /// `F_RDLCK=1 / F_WRLCK=2 / F_UNLCK=8` from `<sys/file.h>`.
        /// `autocommit_boundary` is true when an F_UNLCK should
        /// commit the txn (caller computed from
        /// thd->variables.option_bits & (OPTION_NOT_AUTOCOMMIT |
        /// OPTION_BEGIN) + n_mysql_tables_in_use).
        ///
        /// Returns status::OK on success; failure codes are
        /// status::NO_ENGINE / status::BAD_TABLE_PATH (used here as
        /// "unknown lock_type") / status::ENGINE_IO_FAILED.
        fn ha_external_lock(
            self: &mut HaSlateDb,
            thd_id: u64,
            lock_type: i32,
            autocommit_boundary: bool,
        ) -> i32;

        // ----- handlerton txn callbacks -----
        //
        // Free functions invoked by MariaDB on transaction boundaries
        // (commit, rollback, connection close, savepoint). The C++
        // handlerton plugin registration wires these into the
        // `handlerton->{commit, rollback, ...}` slots.
        //
        // All take `thd_id` — the same opaque per-THD identifier
        // `ha_external_lock` uses. Status codes are
        // [`super::status`]: OK / NOT_SUPPORTED / ENGINE_IO_FAILED
        // / NO_ENGINE / RUNTIME_INIT_FAILED.

        /// MariaDB `commit` callback. `commit_tx=true` → full
        /// commit; `false` → statement boundary (no-op in Stage 0,
        /// no savepoint support per Q10).
        fn slatedb_handlerton_commit(thd_id: u64, commit_tx: bool) -> i32;

        /// MariaDB `start_consistent_snapshot` callback —
        /// `START TRANSACTION WITH CONSISTENT SNAPSHOT`. Pre-acquires
        /// the per-THD txn so the read view is pinned at statement
        /// start instead of first read. SlateDB's SerializableSnapshot
        /// default captures the snapshot at `begin`, so this is a
        /// thin wrapper around `get_or_create_tx`.
        fn slatedb_handlerton_start_consistent_snapshot(thd_id: u64) -> i32;

        /// MariaDB `rollback` callback. `rollback_tx=true` → full
        /// rollback; `false` → statement rollback (no-op in Stage 0).
        fn slatedb_handlerton_rollback(thd_id: u64, rollback_tx: bool) -> i32;

        /// MariaDB `close_connection` callback. Silently rolls back
        /// any in-flight txn.
        fn slatedb_handlerton_close_connection(thd_id: u64) -> i32;

        /// MariaDB `savepoint` callback. Stage 0 stub per Q10 —
        /// returns [`super::status::NOT_SUPPORTED`].
        fn slatedb_handlerton_savepoint(thd_id: u64) -> i32;

        /// MariaDB `rollback_to_savepoint` callback. Stage 0 stub.
        fn slatedb_handlerton_rollback_to_savepoint(thd_id: u64) -> i32;

        /// MariaDB `rollback_to_savepoint_can_release_mdl` query.
        /// Constant `false`.
        fn slatedb_handlerton_rollback_to_savepoint_can_release_mdl(
            thd_id: u64,
        ) -> bool;

        /// MariaDB `commit_ordered` hook. No-op.
        fn slatedb_handlerton_commit_ordered(thd_id: u64, all: bool);

        /// MariaDB `checkpoint_request` hook. No-op.
        fn slatedb_handlerton_checkpoint_request();
    }
}

pub use crate::handler::HaSlateDb;

/// `cxx::bridge` constructor — produces a `Box<HaSlateDb>` which cxx
/// translates to `unique_ptr<HaSlateDb>` on the C++ side.
fn new_ha_slatedb() -> Box<HaSlateDb> {
    Box::new(HaSlateDb::new())
}

// Method bodies for the `self: &mut HaSlateDb` cxx-bridge entries.
// They live here (next to the bridge declaration) rather than in
// `handler.rs` so the cxx surface is localised to this module —
// `handler.rs` exposes a Rust-native API; the bridge module owns the
// status-code translation and the method shape cxx wants.
impl HaSlateDb {
    /// Cxx wrapper — delegates to the Rust-native [`HaSlateDb::open`]
    /// and collapses the error to a stable i32 via
    /// [`crate::handler::open_result_to_status`].
    fn ha_open(&mut self, name: String) -> i32 {
        crate::handler::open_result_to_status(self.open(&name))
    }

    /// Cxx wrapper — [`HaSlateDb::close`] is infallible today, so this
    /// always returns OK; preserved as a fallible signature so future
    /// closes that flush per-handler state can surface errors.
    fn ha_close(&mut self) -> i32 {
        crate::handler::open_result_to_status(self.close())
    }

    /// Cxx wrapper — marshals the flat C++ inputs into the typed
    /// `StoreLockThd` + `ThrLockType` and returns the chosen lock
    /// type as `i32`. The handler-side method is infallible so no
    /// status-code mapping is needed.
    fn ha_store_lock(
        &mut self,
        in_lock_tables: bool,
        tablespace_op: bool,
        requested_lock_type: i32,
    ) -> i32 {
        let thd = crate::handler::StoreLockThd {
            in_lock_tables,
            tablespace_op,
        };
        let chosen = self.store_lock(
            thd,
            crate::handler::ThrLockType::from_i32(requested_lock_type),
        );
        chosen as i32
    }

    /// Cxx wrapper — translates the raw `enum ha_extra_function` `i32`
    /// to the Rust enum and delegates to [`HaSlateDb::extra`]. Always
    /// returns OK; `extra` is infallible.
    fn ha_extra(&mut self, extra_op: i32) -> i32 {
        crate::handler::open_result_to_status(
            self.extra(crate::handler::HaExtraFunction::from_i32(extra_op)),
        )
    }

    /// Cxx wrapper — drives [`HaSlateDb::external_lock`] under the
    /// global tokio runtime via `runtime::block_on`. Unknown
    /// `lock_type` ints collapse to `status::BAD_TABLE_PATH`
    /// (re-purposed here as "bad input"); a missing engine returns
    /// `status::NO_ENGINE`; SlateDB I/O failure (incl. SSI conflict)
    /// returns `status::ENGINE_IO_FAILED`.
    fn ha_external_lock(
        &mut self,
        thd_id: u64,
        lock_type: i32,
        autocommit_boundary: bool,
    ) -> i32 {
        let typed = match crate::handler::ExternalLockType::from_i32(lock_type) {
            Some(t) => t,
            None => return crate::handler::status::BAD_TABLE_PATH,
        };
        let runtime = match crate::runtime::get() {
            Some(rt) => rt,
            None => return status::RUNTIME_INIT_FAILED,
        };
        let result = runtime.block_on(self.external_lock(thd_id, typed, autocommit_boundary));
        crate::handler::open_result_to_status(result)
    }
}

// ---------------------------------------------------------------------------
// Status codes
// ---------------------------------------------------------------------------

/// FFI return codes. Stable across the cxx boundary.
pub mod status {
    /// Operation succeeded.
    pub const OK: i32 = 0;
    /// Runtime init failed (tokio couldn't construct the io pool).
    pub const RUNTIME_INIT_FAILED: i32 = 1;
    /// `init_in_memory` called while an engine is already installed —
    /// caller must `shutdown` first.
    pub const ALREADY_INITIALISED: i32 = 2;
    /// SlateDB returned an error during the async open / close
    /// sequence. Coarse-grained at this stage; richer info will
    /// surface through the handler-bucket error channel.
    pub const ENGINE_IO_FAILED: i32 = 3;
    /// Caller asked for a feature the SlateDB engine doesn't yet
    /// support (Stage 0 savepoint stubs etc.). The C++ side maps
    /// this to `HA_ERR_WRONG_COMMAND`.
    pub const NOT_SUPPORTED: i32 = 4;
}

// ---------------------------------------------------------------------------
// Global engine state
// ---------------------------------------------------------------------------

/// Per-process engine state. `None` before init or after shutdown.
///
/// `parking_lot::RwLock` (not `std::sync::RwLock`) for the const-fn
/// constructor — `parking_lot::RwLock::new` is `const` so this works
/// at static-initialiser time without a `OnceLock` indirection.
static ENGINE: RwLock<Option<EngineState>> = RwLock::new(None);

/// What the bridge owns once `init_in_memory` succeeds. Kept private —
/// the cxx surface only deals in flat i32/bool/String.
struct EngineState {
    db: Arc<EngineDb>,
    ddl: Arc<DdlManager>,
    /// Per-THD transaction registry. Empty at init; populated by
    /// `external_lock` (and friends) once that lands.
    txn_registry: Arc<TxnRegistry>,
}

// ---------------------------------------------------------------------------
// Bridge bodies
// ---------------------------------------------------------------------------

pub(crate) fn slatedb_version() -> String {
    format!("slatedb-engine {}", env!("CARGO_PKG_VERSION"))
}

pub(crate) fn slatedb_init_in_memory(name: String) -> i32 {
    // 1. Install the runtime if it isn't already. "Already installed"
    //    is fine — runtime is singleton-by-design.
    if crate::runtime::get().is_none() {
        match crate::runtime::init(4, 64) {
            Ok(()) => {}
            Err(_) => {
                // The only way init can fail today is if some other
                // caller raced us between get() and init(). Re-check;
                // if still None, we have a real failure.
                if crate::runtime::get().is_none() {
                    return status::RUNTIME_INIT_FAILED;
                }
            }
        }
    }

    // 2. Reject re-init while an engine is installed — caller is
    //    expected to shutdown first. Matches MariaDB's expectation
    //    that plugin init runs against a clean slate.
    if ENGINE.read().is_some() {
        return status::ALREADY_INITIALISED;
    }

    // 3. Open the engine + empty catalogue. `block_on` is required
    //    because SlateDB's open is async and the bridge entry is sync.
    let runtime = match crate::runtime::get() {
        Some(rt) => rt,
        None => return status::RUNTIME_INIT_FAILED,
    };
    let opened: Result<EngineState, slatedb::Error> = runtime.block_on(async {
        let db = Arc::new(EngineDb::open_in_memory(&name).await?);
        let ddl = Arc::new(DdlManager::new());
        // init() is cheap on a fresh in-memory engine — it scans the
        // (empty) dict and seeds the sequence past EndDictIndexId.
        ddl.init(db.db()).await?;
        let txn_registry = Arc::new(TxnRegistry::new());
        Ok(EngineState {
            db,
            ddl,
            txn_registry,
        })
    });

    match opened {
        Ok(state) => {
            *ENGINE.write() = Some(state);
            status::OK
        }
        Err(_) => status::ENGINE_IO_FAILED,
    }
}

pub(crate) fn slatedb_shutdown() -> i32 {
    // Drop the catalogue + take the EngineDb out under the lock.
    let db_to_close = {
        let mut guard = ENGINE.write();
        guard.take().map(|state| state.db)
    };
    let Some(db) = db_to_close else {
        // Nothing to do — caller already shut down or never inited.
        return status::OK;
    };
    let Some(runtime) = crate::runtime::get() else {
        // Runtime is gone — shouldn't happen since we never tear it
        // down, but be safe: the close call would panic without it.
        return status::ENGINE_IO_FAILED;
    };
    match runtime.block_on(db.close()) {
        Ok(()) => status::OK,
        Err(_) => status::ENGINE_IO_FAILED,
    }
}

pub(crate) fn slatedb_has_table(name: String) -> bool {
    let guard = ENGINE.read();
    match guard.as_ref() {
        Some(state) => state.ddl.find(&name).is_some(),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Internal accessors (not exposed via cxx)
// ---------------------------------------------------------------------------

/// Hand the current `DdlManager` to internal Rust callers. Returns
/// `None` if no engine is installed. Each caller gets an `Arc` clone
/// so the read-lock guard doesn't outlive the call.
pub(crate) fn current_ddl() -> Option<Arc<DdlManager>> {
    let guard = ENGINE.read();
    guard.as_ref().map(|state| state.ddl.clone())
}

/// Hand the current `EngineDb` to internal Rust callers. Same shape as
/// [`current_ddl`]. Currently only used by handler test fixtures; will
/// be needed by handler buckets that issue direct dict reads.
#[allow(dead_code)]
pub(crate) fn current_engine() -> Option<Arc<EngineDb>> {
    let guard = ENGINE.read();
    guard.as_ref().map(|state| state.db.clone())
}

/// Hand the per-process [`TxnRegistry`] to internal Rust callers.
/// Consumed by `HaSlateDb::external_lock` and friends once that
/// lands.
#[allow(dead_code)]
pub(crate) fn current_txn_registry() -> Option<Arc<TxnRegistry>> {
    let guard = ENGINE.read();
    guard.as_ref().map(|state| state.txn_registry.clone())
}

// ---------------------------------------------------------------------------
// Handlerton txn callback wrappers
// ---------------------------------------------------------------------------
//
// Free functions matching the cxx bridge declarations. Each resolves
// the global [`TxnRegistry`] and delegates to
// [`crate::engine::handlerton`].

fn handlerton_result_to_status(r: Result<(), slatedb::Error>) -> i32 {
    match r {
        Ok(()) => status::OK,
        Err(e) => match e.kind() {
            slatedb::ErrorKind::Invalid => status::NOT_SUPPORTED,
            _ => status::ENGINE_IO_FAILED,
        },
    }
}

pub(crate) fn slatedb_handlerton_commit(thd_id: u64, commit_tx: bool) -> i32 {
    let Some(registry) = current_txn_registry() else {
        return crate::handler::status::NO_ENGINE;
    };
    let Some(runtime) = crate::runtime::get() else {
        return status::RUNTIME_INIT_FAILED;
    };
    let r = runtime.block_on(crate::engine::handlerton::commit(
        &registry, thd_id, commit_tx,
    ));
    handlerton_result_to_status(r)
}

pub(crate) fn slatedb_handlerton_start_consistent_snapshot(thd_id: u64) -> i32 {
    let Some(registry) = current_txn_registry() else {
        return crate::handler::status::NO_ENGINE;
    };
    let Some(db) = current_engine() else {
        return crate::handler::status::NO_ENGINE;
    };
    let Some(runtime) = crate::runtime::get() else {
        return status::RUNTIME_INIT_FAILED;
    };
    let r = runtime.block_on(
        crate::engine::handlerton::start_tx_and_assign_read_view(
            &registry, thd_id, &db,
        ),
    );
    handlerton_result_to_status(r)
}

pub(crate) fn slatedb_handlerton_rollback(thd_id: u64, rollback_tx: bool) -> i32 {
    let Some(registry) = current_txn_registry() else {
        return crate::handler::status::NO_ENGINE;
    };
    handlerton_result_to_status(crate::engine::handlerton::rollback(
        &registry,
        thd_id,
        rollback_tx,
    ))
}

pub(crate) fn slatedb_handlerton_close_connection(thd_id: u64) -> i32 {
    let Some(registry) = current_txn_registry() else {
        return crate::handler::status::NO_ENGINE;
    };
    handlerton_result_to_status(crate::engine::handlerton::close_connection(
        &registry, thd_id,
    ))
}

pub(crate) fn slatedb_handlerton_savepoint(thd_id: u64) -> i32 {
    handlerton_result_to_status(crate::engine::handlerton::savepoint(thd_id))
}

pub(crate) fn slatedb_handlerton_rollback_to_savepoint(thd_id: u64) -> i32 {
    handlerton_result_to_status(crate::engine::handlerton::rollback_to_savepoint(
        thd_id,
    ))
}

pub(crate) fn slatedb_handlerton_rollback_to_savepoint_can_release_mdl(
    thd_id: u64,
) -> bool {
    crate::engine::handlerton::rollback_to_savepoint_can_release_mdl(thd_id)
}

pub(crate) fn slatedb_handlerton_commit_ordered(thd_id: u64, all: bool) {
    crate::engine::handlerton::commit_ordered(thd_id, all);
}

pub(crate) fn slatedb_handlerton_checkpoint_request() {
    crate::engine::handlerton::checkpoint_request();
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- Why these tests serialise on a Mutex -----
    //
    // The bridge holds process-global state (ENGINE + runtime
    // singleton). Tests in the same crate run on parallel threads by
    // default; if two `init_in_memory`-using tests ran concurrently
    // they'd race the ENGINE slot. We serialise them on a Mutex.
    //
    // Once the cxx surface grows to handler-level operations, the
    // C++ side will own this synchronisation (MariaDB itself
    // serialises plugin install/uninit). For the MVS lifecycle tests
    // we DIY.
    use parking_lot::Mutex;
    static SERIALISE: Mutex<()> = Mutex::new(());

    #[test]
    fn version_returns_crate_version_string() {
        let v = slatedb_version();
        assert!(v.starts_with("slatedb-engine "));
        assert!(v.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn init_open_query_shutdown_cycle() {
        let _g = SERIALISE.lock();
        // Ensure clean slate (a prior test may have left state).
        let _ = slatedb_shutdown();

        assert_eq!(
            slatedb_init_in_memory("bridge_cycle".into()),
            status::OK,
            "init",
        );

        // Catalogue is empty on a fresh in-memory engine.
        assert!(!slatedb_has_table("appdb.users".into()));

        assert_eq!(slatedb_shutdown(), status::OK, "shutdown");

        // After shutdown the engine is gone — has_table reads false.
        assert!(!slatedb_has_table("appdb.users".into()));
    }

    #[test]
    fn double_init_returns_already_initialised() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();

        assert_eq!(slatedb_init_in_memory("dbl_init_1".into()), status::OK);
        assert_eq!(
            slatedb_init_in_memory("dbl_init_2".into()),
            status::ALREADY_INITIALISED,
        );

        // Clean up so subsequent tests start fresh.
        assert_eq!(slatedb_shutdown(), status::OK);
    }

    #[test]
    fn shutdown_is_idempotent_when_uninitialised() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        // Second shutdown on already-empty state still returns OK.
        assert_eq!(slatedb_shutdown(), status::OK);
    }

    #[test]
    fn has_table_returns_false_before_init() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert!(!slatedb_has_table("anything".into()));
    }

    #[test]
    fn status_codes_are_distinct() {
        // Light sanity — these are stable wire constants; a copy-paste
        // typo collapsing two of them would silently break callers.
        let codes = [
            status::OK,
            status::RUNTIME_INIT_FAILED,
            status::ALREADY_INITIALISED,
            status::ENGINE_IO_FAILED,
            status::NOT_SUPPORTED,
        ];
        let mut set: std::collections::HashSet<i32> = std::collections::HashSet::new();
        for c in codes {
            assert!(set.insert(c), "duplicate status code {c}");
        }
    }

    // ----- handlerton txn callbacks -----

    #[test]
    fn handlerton_commit_without_engine_returns_no_engine() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(
            slatedb_handlerton_commit(1, true),
            crate::handler::status::NO_ENGINE,
        );
    }

    #[test]
    fn handlerton_commit_full_drains_registered_txn() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(
            slatedb_init_in_memory("handlerton_commit_bridge".into()),
            status::OK,
        );
        // Register a txn under thd_id=7 via the registry (skipping
        // the cxx external_lock path — we already test that
        // elsewhere).
        let reg = current_txn_registry().expect("registry");
        let db = current_engine().expect("engine");
        crate::runtime::block_on(async {
            reg.get_or_create(7, &db).await.expect("create");
        });
        assert!(reg.has(7));

        assert_eq!(slatedb_handlerton_commit(7, true), status::OK);
        assert!(!reg.has(7));

        assert_eq!(slatedb_shutdown(), status::OK);
    }

    #[test]
    fn handlerton_start_consistent_snapshot_creates_txn() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(
            slatedb_init_in_memory("handlerton_start_snapshot_bridge".into()),
            status::OK,
        );

        let reg = current_txn_registry().expect("registry");
        assert!(!reg.has(50));

        assert_eq!(
            slatedb_handlerton_start_consistent_snapshot(50),
            status::OK,
        );
        assert!(reg.has(50));

        assert_eq!(slatedb_shutdown(), status::OK);
    }

    #[test]
    fn handlerton_start_consistent_snapshot_without_engine_returns_no_engine() {
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(
            slatedb_handlerton_start_consistent_snapshot(1),
            crate::handler::status::NO_ENGINE,
        );
    }

    #[test]
    fn handlerton_savepoint_returns_not_supported() {
        // No engine state needed — savepoint is unconditionally
        // Stage-0-stubbed.
        let _g = SERIALISE.lock();
        let _ = slatedb_shutdown();
        assert_eq!(slatedb_handlerton_savepoint(1), status::NOT_SUPPORTED);
        assert_eq!(
            slatedb_handlerton_rollback_to_savepoint(1),
            status::NOT_SUPPORTED,
        );
    }

    #[test]
    fn handlerton_rollback_to_savepoint_can_release_mdl_is_false() {
        assert!(!slatedb_handlerton_rollback_to_savepoint_can_release_mdl(0));
    }

    #[test]
    fn handlerton_noop_hooks_do_not_panic() {
        slatedb_handlerton_commit_ordered(0, true);
        slatedb_handlerton_checkpoint_request();
    }
}
