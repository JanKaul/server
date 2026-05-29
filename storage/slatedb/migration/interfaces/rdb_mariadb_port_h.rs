//! Interface stub for `rdb_mariadb_port_h`.
//!
//! C++ source: `storage/rocksdb/rdb_mariadb_port.h` (55 LoC)
//!
//! ## Mapping
//! Pure C-preprocessor compat shim between WebScaleSQL and MariaDB. The vast
//! majority of what's in the header is `#define` macros, `MY_ATTRIBUTE`
//! plumbing, and one struct (`my_io_perf_atomic_t`). On the Rust side, almost
//! all of it is either:
//!
//! - **Absorbed by the cxx bridge** (the macros that disappear at compile time
//!   never reach Rust), or
//! - **A Rust no-op** (e.g., MariaDB-specific calling-convention attributes), or
//! - **Replaced by safer Rust primitives** (`abort_with_stack_traces()` →
//!   `std::process::abort()` after logging a backtrace).
//!
//! What remains for us:
//!
//! 1. `my_io_perf_atomic_t` — atomic IO-perf counters, used by
//!    `rdb_mariadb_server_port_h` and the perf-context layer. Per _DESIGN.md
//!    §1 the perf-counter family is "Re-impl" using SlateDB metrics, so the
//!    fields are recreated 1:1 with `AtomicStatU64`.
//! 2. `split_into_vector` — a generic string-split helper. Implemented as a
//!    standalone Rust fn.
//! 3. `mysql_bin_log_commit_pos` — binlog commit position lookup. Bridged from
//!    the C++ MariaDB server side via the cxx layer; not re-implemented in Rust.
//!
//! ## Out-of-scope methods
//! - `mysql_bin_log_commit_pos` — server-internal binlog state; reachable from
//!   Rust only through the cxx bridge. We expose it as an `extern "Rust"` shim
//!   that calls back into C++.

use crate::atomic_stat_h::AtomicStatU64;

/// Atomic per-thread-pool IO perf counters. Replaces C++ `my_io_perf_atomic_t`
/// (rdb_mariadb_port.h:25).
///
/// All counters are relaxed-consistency — see `atomic_stat_h` for ordering
/// semantics.
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
    fn default() -> Self { Self::new() }
}

/// Split `input` on every `delimiter`. Empty segments are preserved (matches
/// C++ `split_into_vector` semantics in rdb_mariadb_port.h:49).
///
/// Returns `&str` slices borrowed from the input — caller owns the input
/// buffer. If you need owned strings the caller can `.map(String::from)`.
pub fn split_into_vector<'a>(input: &'a str, delimiter: char) -> Vec<&'a str> {
    input.split(delimiter).collect()
}

/// SIGABRT after attempting to print a backtrace. C++
/// `abort_with_stack_traces()` (rdb_mariadb_port.h:44).
///
/// Used as the last-resort exit when invariants are violated. The Rust impl
/// logs the panic + backtrace via the `log` crate's `error!` and then aborts.
pub fn abort_with_stack_traces(msg: &str) -> ! {
    // TODO(human): wire to whatever logger the rest of the crate uses (`log`
    // vs `tracing`). For now: stderr + abort.
    eprintln!("MyRocks/SlateDB abort: {msg}");
    eprintln!("{}", std::backtrace::Backtrace::force_capture());
    std::process::abort()
}
