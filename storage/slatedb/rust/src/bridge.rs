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
}

// ---------------------------------------------------------------------------
// Bridge bodies
// ---------------------------------------------------------------------------

fn slatedb_version() -> String {
    format!("slatedb-engine {}", env!("CARGO_PKG_VERSION"))
}

fn slatedb_init_in_memory(name: String) -> i32 {
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
        Ok(EngineState { db, ddl })
    });

    match opened {
        Ok(state) => {
            *ENGINE.write() = Some(state);
            status::OK
        }
        Err(_) => status::ENGINE_IO_FAILED,
    }
}

fn slatedb_shutdown() -> i32 {
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

fn slatedb_has_table(name: String) -> bool {
    let guard = ENGINE.read();
    match guard.as_ref() {
        Some(state) => state.ddl.find(&name).is_some(),
        None => false,
    }
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
        ];
        let mut set: std::collections::HashSet<i32> = std::collections::HashSet::new();
        for c in codes {
            assert!(set.insert(c), "duplicate status code {c}");
        }
    }
}
