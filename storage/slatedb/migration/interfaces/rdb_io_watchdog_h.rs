//! Interface stub for `rdb_io_watchdog_h`.
//!
//! C++ source: `storage/rocksdb/rdb_io_watchdog.h` (119 LoC)
//! C++ class: `Rdb_io_watchdog`
//!
//! ## Mapping
//! MyRocks runs a periodic 4 KiB dummy-file write across each data directory;
//! if a write takes longer than the configured `write_timeout`, the watchdog
//! aborts the server. The rationale was that RocksDB on local disk could
//! silently hang on a stuck I/O.
//!
//! In SlateDB the durable substrate is an **object store** (S3, GCS, etc.)
//! and `slatedb::DbBuilder` plus the `object_store` crate already enforce
//! per-request timeouts and retries at the network layer. Per _DESIGN.md §1
//! (perf-counter family "Re-impl" + WAL durability bullet) the equivalent
//! signal lives in:
//!
//! - **`DbMetadataOps::subscribe()`** — `DbStatus` exposes `durable_seq`. If
//!   the value stops advancing while writes are pending, the object store has
//!   stalled. This is the SlateDB-native version of "did writes complete?".
//! - **Slatedb metrics** (`slatedb_common::metrics`) — `object_store_put_*`
//!   counters and latencies. We subscribe these in the engine's metrics task.
//!
//! So `Rdb_io_watchdog` becomes a thin Rust task that:
//!
//! 1. Subscribes to `DbStatus`.
//! 2. If `durable_seq` doesn't advance within `write_timeout_ms`, logs and
//!    optionally aborts (gated by a sysvar — abort behavior preserved only
//!    for parity, default OFF).
//!
//! We do **not** write dummy files — the object-store API already exercises
//! the I/O path on every WAL flush.
//!
//! ## Out-of-scope methods
//! - `check_write_access(dir)` — there's no local "data directory" with the
//!   SlateDB object-store model. Removed.
//! - `reset_timeout()` — the watchdog reads the sysvar live each tick, so the
//!   explicit reset API is replaced by atomic-sysvar pickup.

use slatedb::Error;
use std::time::Duration;

/// Background task that watches for stalled durability progress. Replaces
/// `Rdb_io_watchdog` for the SlateDB world.
///
/// Lifecycle: spawned in `plugin::init`, cancelled via `CancellationToken`
/// in `plugin::done`. Identical pattern to `event_listener_h::StatsRefreshTask`.
pub struct IoWatchdog {
    /// `Db::subscribe()` receiver. We watch `durable_seq` here.
    pub status_rx: tokio::sync::watch::Receiver<slatedb::DbStatus>,
    /// Timeout — if `durable_seq` doesn't advance for this long while writes
    /// are pending, take action.
    pub write_timeout: Duration,
    /// If true, `std::process::abort()` on timeout. Default false — we just
    /// log. Parity sysvar for MyRocks' `rocksdb_io_write_timeout_abort`.
    pub abort_on_timeout: bool,
}

impl IoWatchdog {
    pub fn new(
        status_rx: tokio::sync::watch::Receiver<slatedb::DbStatus>,
        write_timeout: Duration,
        abort_on_timeout: bool,
    ) -> Self {
        Self { status_rx, write_timeout, abort_on_timeout }
    }

    /// One tick of the watchdog loop. Records the current `durable_seq`,
    /// then waits up to `write_timeout` for a change. Returns:
    /// - `Ok(true)`  — durable_seq advanced (healthy).
    /// - `Ok(false)` — no change within the timeout (stalled).
    /// - `Err(_)`    — DB closed; task should exit.
    pub async fn tick(&mut self) -> Result<bool, Error> {
        todo!(concat!(
            "let baseline = self.status_rx.borrow_and_update().durable_seq; ",
            "tokio::time::timeout(self.write_timeout, self.status_rx.changed()).await; ",
            "compare new durable_seq vs baseline"
        ))
    }

    /// Run-forever loop. On stall, logs a `WARNING` row; if `abort_on_timeout`
    /// is set, aborts the process via `rdb_mariadb_port_h::abort_with_stack_traces`.
    pub async fn run(mut self, cancel: tokio_util::sync::CancellationToken) {
        let _ = &mut self;
        let _ = cancel;
        todo!("loop: select! { tick = self.tick() => act, _ = cancel.cancelled() => break }")
    }
}
