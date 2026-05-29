//! Interface stub for `atomic_stat_h`.
//!
//! C++ source: `storage/rocksdb/atomic_stat.h` (94 LoC, generic class template)
//!
//! ## Mapping
//! Thin wrapper around a single atomic primitive used for relaxed-consistency
//! stat counters (the original docstring explicitly permits inconsistent reads).
//! In Rust this is just `std::sync::atomic::AtomicU64` (or `AtomicI64`, etc.)
//! with `Relaxed` ordering. No abstraction over numeric type — concrete
//! per-type structs are simpler than a generic and match the only call sites
//! we see in MyRocks (always `ulonglong`).
//!
//! ## Out-of-scope methods
//! None — this is a primitive utility, fully in scope.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

/// Relaxed-consistency `u64` counter for stats. Reads may miss recent writes;
/// no operation establishes happens-before. Used for monitoring, not for
/// correctness-critical state.
///
/// Original: `atomic_stat<ulonglong>` (atomic_stat.h:36)
pub struct AtomicStatU64(AtomicU64);

impl AtomicStatU64 {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Reset to zero with seq-cst ordering — only this op needs strict order
    /// per the original docstring (atomic_stat.h:42).
    pub fn clear(&self) {
        self.0.store(0, Ordering::SeqCst);
    }

    /// Approximate read. May lag concurrent writes — caller accepts that.
    pub fn load(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    pub fn inc_by(&self, n: u64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }

    pub fn dec_by(&self, n: u64) {
        self.0.fetch_sub(n, Ordering::Relaxed);
    }

    pub fn inc(&self) {
        self.inc_by(1);
    }

    pub fn dec(&self) {
        self.dec_by(1);
    }

    /// Single-attempt CAS to set to `new_val` if larger. May silently fail
    /// (atomic_stat.h:72 — "It can fail for any reason, and we only try it once").
    pub fn set_max_maybe(&self, new_val: u64) {
        let cur = self.0.load(Ordering::Relaxed);
        if new_val > cur {
            let _ = self.0.compare_exchange_weak(
                cur, new_val, Ordering::Relaxed, Ordering::Relaxed,
            );
        }
    }

    /// Single-attempt CAS to set to `new_val`. May silently fail.
    pub fn set_maybe(&self, new_val: u64) {
        let cur = self.0.load(Ordering::Relaxed);
        let _ = self
            .0
            .compare_exchange_weak(cur, new_val, Ordering::Relaxed, Ordering::Relaxed);
    }
}

impl Default for AtomicStatU64 {
    fn default() -> Self {
        Self::new()
    }
}

/// Signed-i64 variant. MyRocks uses unsigned everywhere atomic_stat appears,
/// but we expose the signed form for completeness since `io_perf` counters
/// occasionally subtract.
pub struct AtomicStatI64(AtomicI64);

impl AtomicStatI64 {
    pub const fn new() -> Self {
        Self(AtomicI64::new(0))
    }
    pub fn clear(&self) { self.0.store(0, Ordering::SeqCst); }
    pub fn load(&self) -> i64 { self.0.load(Ordering::Relaxed) }
    pub fn inc_by(&self, n: i64) { self.0.fetch_add(n, Ordering::Relaxed); }
    pub fn dec_by(&self, n: i64) { self.0.fetch_sub(n, Ordering::Relaxed); }
    pub fn inc(&self) { self.inc_by(1); }
    pub fn dec(&self) { self.dec_by(1); }
}

impl Default for AtomicStatI64 {
    fn default() -> Self {
        Self::new()
    }
}
