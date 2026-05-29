//! Interface stub for `rdb_mutex_wrapper_h`.
//!
//! C++ source: `storage/rocksdb/rdb_mutex_wrapper.h` (143 LoC)
//!
//! ## Mapping
//! MyRocks wrapped RocksDB's pluggable mutex API to integrate with MySQL's
//! PSI instrumentation. SlateDB uses `parking_lot::Mutex` internally and
//! exposes no pluggable-mutex hook.
//!
//! Per _DESIGN.md §1, PSI integration is **out-of-scope v1**. We use
//! `parking_lot::Mutex` directly (already a transitive dep of slatedb).
//! This stub file just documents the redirection; no Rust trait needed.
//!
//! ## Out-of-scope methods
//! - All MyRocks mutex-wrapper classes — replaced by direct
//!   `parking_lot::Mutex` use in callers.

// Per the mapping above, no Rust types are exposed here. Callers that
// previously took `Rdb_mutex_wrapper` parameters now take
// `parking_lot::Mutex<T>` directly.
//
// PSI hook points are dropped per _DESIGN.md "rdb_psi: out-of-scope v1".
// If we revisit PSI integration in a future version we'd add a wrapper
// here that delegates to `parking_lot::Mutex` and emits PSI events.
