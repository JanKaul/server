//! Performance Schema (P_S / PSI) integration — v1 no-op.
//!
//! Translated from `storage/rocksdb/rdb_psi.h`. Per `_DESIGN.md §1`, PSI
//! integration is out of scope for v1: every `PSI_*_key` registration is
//! dropped, and the engine relies on SlateDB metrics + `SHOW ENGINE STATUS`
//! instead. The single surviving entry point is a no-op so the plugin's init
//! sequence compiles unchanged.

/// No-op PSI initialization. Future v2 work would re-introduce PSI keys
/// behind a `feature = "psi"` flag wired to MariaDB's PSI macros via the cxx
/// bridge.
pub fn init_psi_keys() {}

/// Marker constant for the wait-event we'd publish if PSI were on. Preserved
/// as a string so any reference to the C++ `stage_waiting_on_row_lock` symbol
/// has a target.
pub const STAGE_WAITING_ON_TXN_COMMIT: &str = "Waiting on transaction commit";
