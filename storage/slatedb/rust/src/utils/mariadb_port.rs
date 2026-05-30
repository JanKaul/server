//! MariaDB compat-shim leftovers.
//!
//! Translated from `storage/rocksdb/rdb_mariadb_port.h`. The original is
//! almost entirely C-preprocessor and disappears at compile time; what
//! survives is the IO-perf counter struct, a small split helper, and the
//! last-resort abort routine.
//!
//! `mysql_bin_log_commit_pos` lives on the C++ side and is reached via the
//! cxx bridge; it is intentionally not declared here.

use crate::utils::atomic_stat::AtomicStatU64;

/// Atomic per-thread-pool IO perf counters. Relaxed-consistency.
pub struct IoPerfAtomic {
    pub bytes: AtomicStatU64,
    pub requests: AtomicStatU64,
    /// Time spent inside the actual read/write syscall, in nanoseconds.
    pub svc_time: AtomicStatU64,
    pub svc_time_max: AtomicStatU64,
    /// Time spent enqueued before the request was serviced.
    pub wait_time: AtomicStatU64,
    pub wait_time_max: AtomicStatU64,
    /// Count of requests that exceeded the "slow IO" threshold sysvar.
    pub slow_ios: AtomicStatU64,
}

impl IoPerfAtomic {
    pub const fn new() -> Self {
        Self {
            bytes: AtomicStatU64::new(),
            requests: AtomicStatU64::new(),
            svc_time: AtomicStatU64::new(),
            svc_time_max: AtomicStatU64::new(),
            wait_time: AtomicStatU64::new(),
            wait_time_max: AtomicStatU64::new(),
            slow_ios: AtomicStatU64::new(),
        }
    }
}

impl Default for IoPerfAtomic {
    fn default() -> Self {
        Self::new()
    }
}

/// Split `input` on every `delimiter`, preserving empty segments to match the
/// C++ `split_into_vector` semantics.
pub fn split_into_vector(input: &str, delimiter: char) -> Vec<&str> {
    input.split(delimiter).collect()
}

/// Last-resort exit: log + backtrace + `abort`. Used when invariants are
/// violated and we'd rather crash than continue with corrupt state.
pub fn abort_with_stack_traces(msg: &str) -> ! {
    let backtrace = std::backtrace::Backtrace::force_capture();
    tracing::error!(?backtrace, "SlateDB abort: {msg}");
    std::process::abort()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_preserves_empty_segments() {
        assert_eq!(split_into_vector("a,,b,", ','), vec!["a", "", "b", ""]);
    }

    #[test]
    fn io_perf_atomic_starts_zero() {
        let p = IoPerfAtomic::new();
        assert_eq!(p.bytes.load(), 0);
        assert_eq!(p.slow_ios.load(), 0);
    }
}
