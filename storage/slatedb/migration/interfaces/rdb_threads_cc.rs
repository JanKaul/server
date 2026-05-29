//! Interface stub for `rdb_threads_cc`.
//!
//! C++ source: `storage/rocksdb/rdb_threads.cc` (79 LoC)
//!
//! ## Mapping
//! Implementation of `Rdb_thread` (declared in `rdb_threads_h.rs`). Per
//! _DESIGN.md, replaced by Tokio tasks; this impl side reduces to a
//! thin shim around `tokio::spawn_on`.
//!
//! ## Out-of-scope methods
//! None.

// All public functionality is provided by `rdb_threads_h::spawn_task`.
// The impl-side of this unit is intentionally empty in the Rust port —
// the C++ rdb_threads.cc body managed pthread state which Tokio handles
// for us. No additional code needed here beyond what's in the header.
