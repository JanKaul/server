//! Interface stub for `rdb_psi_h`.
//!
//! C++ source: `storage/rocksdb/rdb_psi.h` (58 LoC)
//!
//! ## Mapping
//! MariaDB's Performance Schema (P_S, a.k.a. PSI) instruments mutexes,
//! rwlocks, condvars, threads, and execution stages so DBAs can introspect
//! lock contention from `information_schema.events_*`. The header declares a
//! big block of `PSI_*_key` globals that get registered on plugin init.
//!
//! Per _DESIGN.md §1 ("rdb_psi/rdb_threads/rdb_mutex_wrapper — replace with
//! Rust-native primitives") + the contract note that PSI integration is
//! **OUT OF SCOPE for v1**, this header collapses to:
//!
//! - **Zero PSI keys.** None of the `PSI_mutex_key`, `PSI_cond_key`,
//!   `PSI_thread_key`, `PSI_rwlock_key`, or `PSI_stage_info` items survive.
//! - **One no-op `init_psi_keys()` function** so the call site in plugin
//!   init compiles unchanged. Future v2 work can add real PSI bindings via
//!   the cxx bridge.
//!
//! Lock contention visibility in v1 is provided via SlateDB's metrics
//! (per `rdb_perf_context_h`) and via SHOW ENGINE STATUS rather than
//! P_S. Documented as a known gap in the migration notes.
//!
//! ## Out-of-scope methods
//! - All `PSI_*_key` registrations — entire feature out of scope for v1.
//! - `stage_waiting_on_row_lock` — wait-event stage instrumentation; v2.

use slatedb::Error;

/// No-op PSI initialization. Kept so the plugin's init sequence compiles
/// unchanged. Returns `Ok(())` immediately.
///
/// Replaces C++ `init_rocksdb_psi_keys()` (rdb_psi.h:54).
///
/// Future v2 work: re-introduce PSI keys behind a `feature = "psi"` flag,
/// wire them into `parking_lot::Mutex` via a custom Raw lock that calls back
/// into MariaDB's PSI macros through the cxx bridge.
pub fn init_psi_keys() -> Result<(), Error> {
    // TODO(human): wire real PSI integration in v2 once the cxx bridge is up.
    Ok(())
}

/// Marker constant for the wait-event we'd publish if PSI were on.
/// Currently unused. Preserved as a string so any code that wanted to
/// reference the C++ `stage_waiting_on_row_lock` symbol has something to
/// resolve.
pub const STAGE_WAITING_ON_TXN_COMMIT: &str = "Waiting on transaction commit";
