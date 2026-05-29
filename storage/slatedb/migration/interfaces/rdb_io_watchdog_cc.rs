//! Interface stub for `rdb_io_watchdog_cc`.
//!
//! C++ source: `storage/rocksdb/rdb_io_watchdog.cc` (241 LoC)
//! C++ class:  `Rdb_io_watchdog` (impl)
//!
//! ## Mapping
//! Per _DESIGN.md §1 (we already mapped event listeners; the same logic
//! applies here): the MyRocks watchdog polled write latency to a known
//! directory and aborted the process if a `write()` syscall blocked longer
//! than `m_write_timeout` seconds. With SlateDB the equivalent signal is
//! exposed as a metric on the object store I/O subsystem; we subscribe to
//! that metric via `slatedb_common::metrics` and trigger our shutdown path
//! on threshold breach instead of polling.
//!
//! Since SlateDB's I/O path goes through the `object_store` crate
//! (not a filesystem `write()`), the original "abort if pwrite blocks
//! >N seconds" semantics translate to "shut down cleanly if
//! `object_store_put_latency_p99 > N seconds for K consecutive samples"`.
//! We refuse to `abort()` from Rust because SlateDB's panic path handles
//! durability correctly; instead we issue `Db::close` and surface the event
//! through the engine status.
//!
//! ## Out-of-scope methods
//! - `expire_io_callback`, `io_check_callback`, `check_write_access` —
//!   syscall-driven; replaced by metric subscription.
//! - `stop_timers` — no POSIX timers; we use `tokio::time::interval`.
//! - The `_wrapper` static functions — POSIX `sigevent` thunks; gone.

use slatedb::Error;
use std::time::Duration;

/// Configuration for the watchdog. Mirrors the sysvar surface so SHOW
/// VARIABLES looks identical to MyRocks operators.
#[derive(Debug, Clone)]
pub struct IoWatchdogConfig {
    /// Seconds; 0 disables.
    pub write_timeout: u32,
    /// How often to sample the metric.
    pub sample_interval: Duration,
    /// How many consecutive bad samples before we shut down.
    pub consecutive_breaches_required: u32,
}

/// Background task that watches SlateDB I/O latency metrics.
///
/// Lifecycle: spawned in `plugin::init` if `write_timeout > 0`; cancelled
/// via `CancellationToken` in `plugin::done`.
pub struct IoWatchdog {
    pub config: IoWatchdogConfig,
    pub consecutive_breaches: u32,
}

impl IoWatchdog {
    /// Construct.
    /// Original: rdb_io_watchdog.cc — constructor (in the .h, but
    /// referenced from .cc).
    pub fn new(config: IoWatchdogConfig) -> Self {
        Self { config, consecutive_breaches: 0 }
    }

    /// Update the watchdog timeout in response to a sysvar change.
    /// Setting `write_timeout = 0` disables; non-zero re-arms.
    ///
    /// Inputs: `write_timeout` in seconds.
    /// Output: `Ok(())`.
    /// Errors: `Invalid` if `write_timeout > 24 * 3600` (sanity).
    ///
    /// Original: rdb_io_watchdog.cc:155 — `reset_timeout`.
    pub async fn reset_timeout(&mut self, write_timeout: u32) -> Result<(), Error> {
        if write_timeout > 24 * 3600 {
            return Err(Error::invalid(format!(
                "watchdog timeout {} > 86400s",
                write_timeout
            )));
        }
        self.config.write_timeout = write_timeout;
        self.consecutive_breaches = 0;
        Ok(())
    }

    /// One sample tick. Reads the current `object_store_put_latency_p99`
    /// from `slatedb_common::metrics`, compares to the threshold, increments
    /// or resets the breach counter, and triggers shutdown on saturation.
    ///
    /// Inputs: none (reads from the metric registry).
    /// Output: `true` if a shutdown was triggered on this sample.
    ///
    /// Errors:
    /// - `Unavailable` if the metric registry handle is gone.
    /// - `Internal` on shutdown-init failure (caller should log and continue).
    ///
    /// Original: rdb_io_watchdog.cc:52 — `io_check_callback`.
    pub async fn tick(&mut self) -> Result<bool, Error> {
        todo!("read metric; if > threshold, increment breaches; if >= required, db.close() and return true")
    }

    /// Run-forever loop. Sleeps `config.sample_interval`, calls `tick`,
    /// exits on cancel.
    pub async fn run(mut self, cancel: tokio_util::sync::CancellationToken) {
        let _ = (&mut self, &cancel);
        todo!(
            "loop { select! { _ = sleep(self.config.sample_interval) => self.tick().await?, \
             _ = cancel.cancelled() => break } }"
        )
    }
}
