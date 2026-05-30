//! Relaxed-consistency stat counters.
//!
//! Translated from `storage/rocksdb/atomic_stat.h`. The C++ original is a
//! single class template parameterised on numeric type; in practice MyRocks
//! only instantiates it with `ulonglong` and (rarely) signed integers, so we
//! provide two concrete types instead of a generic.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

/// Relaxed-consistency `u64` counter. Reads may lag concurrent writes; no
/// operation establishes happens-before. Used for monitoring, not for
/// correctness-critical state.
#[derive(Debug, Default)]
pub struct AtomicStatU64(AtomicU64);

impl AtomicStatU64 {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Reset to zero with seq-cst — only this op needs strict order per the
    /// original docstring.
    pub fn clear(&self) {
        self.0.store(0, Ordering::SeqCst);
    }

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

    /// Single-attempt CAS to set to `new_val` if larger. May silently fail.
    pub fn set_max_maybe(&self, new_val: u64) {
        let cur = self.0.load(Ordering::Relaxed);
        if new_val > cur {
            let _ = self.0.compare_exchange_weak(
                cur,
                new_val,
                Ordering::Relaxed,
                Ordering::Relaxed,
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

/// Signed-i64 variant. Used for `io_perf` counters that occasionally subtract.
#[derive(Debug, Default)]
pub struct AtomicStatI64(AtomicI64);

impl AtomicStatI64 {
    pub const fn new() -> Self {
        Self(AtomicI64::new(0))
    }
    pub fn clear(&self) {
        self.0.store(0, Ordering::SeqCst);
    }
    pub fn load(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }
    pub fn inc_by(&self, n: i64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }
    pub fn dec_by(&self, n: i64) {
        self.0.fetch_sub(n, Ordering::Relaxed);
    }
    pub fn inc(&self) {
        self.inc_by(1);
    }
    pub fn dec(&self) {
        self.dec_by(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inc_and_load_round_trip() {
        let c = AtomicStatU64::new();
        c.inc();
        c.inc_by(5);
        assert_eq!(c.load(), 6);
        c.dec();
        assert_eq!(c.load(), 5);
        c.clear();
        assert_eq!(c.load(), 0);
    }

    #[test]
    fn set_max_only_increases() {
        let c = AtomicStatU64::new();
        c.inc_by(10);
        c.set_max_maybe(5);
        assert_eq!(c.load(), 10);
        c.set_max_maybe(42);
        assert_eq!(c.load(), 42);
    }
}
