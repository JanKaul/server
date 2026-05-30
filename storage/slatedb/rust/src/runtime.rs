//! Tokio runtime singleton and queue-depth gate.
//!
//! Per `_DESIGN.md §7`:
//! - **One** `tokio::runtime::Runtime` per handlerton lifetime, held in a
//!   `OnceLock`.
//! - cxx bridge functions are synchronous from C++ and internally call
//!   [`EngineRuntime::block_on`] from the *handler* thread (never from inside
//!   a Tokio worker — re-entrant deadlock).
//! - `slatedb_io_threads` sysvar sizes the worker pool (default
//!   `num_cpus / 2`, minimum 1).
//! - `slatedb_io_queue_depth` sysvar sizes a `Semaphore` that gates hot-path
//!   admissions; full queue surfaces as `slatedb::ErrorKind::Unavailable`,
//!   mapped to `HA_ERR_LOCK_WAIT_TIMEOUT` by `error::slatedb_error_to_ha_err`.
//!
//! The testable type is [`EngineRuntime`], constructed directly. The
//! [`init`] / [`get`] / [`block_on`] free functions are the singleton
//! bindings used at plugin lifetime.

use slatedb::Error;
use std::future::Future;
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::runtime::Runtime;
use tokio::sync::Semaphore;

/// Engine-wide runtime + admission gate.
pub struct EngineRuntime {
    runtime: Runtime,
    gate: Arc<Semaphore>,
    /// Cached for diagnostics / SHOW STATUS.
    io_threads: usize,
    queue_depth: usize,
}

impl EngineRuntime {
    /// Build an `EngineRuntime` with `io_threads` worker threads and a
    /// `queue_depth`-sized admission Semaphore.
    ///
    /// `io_threads` is clamped to `>= 1`; `queue_depth` is clamped to
    /// `>= 1` so the gate is always operable.
    pub fn new(io_threads: usize, queue_depth: usize) -> Result<Self, Error> {
        let workers = io_threads.max(1);
        let depth = queue_depth.max(1);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .enable_all()
            .thread_name("slatedb-engine")
            .build()
            .map_err(|e| Error::internal(format!("tokio runtime build: {e}")))?;
        Ok(Self {
            runtime,
            gate: Arc::new(Semaphore::new(depth)),
            io_threads: workers,
            queue_depth: depth,
        })
    }

    /// Default sizing: `num_cpus / 2` worker threads (min 1), queue depth 256.
    pub fn with_defaults() -> Result<Self, Error> {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        Self::new(cpus / 2, 256)
    }

    pub fn io_threads(&self) -> usize {
        self.io_threads
    }
    pub fn queue_depth(&self) -> usize {
        self.queue_depth
    }
    pub fn handle(&self) -> tokio::runtime::Handle {
        self.runtime.handle().clone()
    }

    /// Run `future` to completion on the engine runtime, blocking the
    /// calling thread. Caller must not already be inside a Tokio task —
    /// see the module docs.
    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }

    /// Try to take one admission permit. Returns `Err(Unavailable)` if the
    /// gate is full. The returned permit must be held for the duration of
    /// the gated work; dropping it returns the slot to the pool.
    pub fn try_acquire_permit(&self) -> Result<tokio::sync::OwnedSemaphorePermit, Error> {
        match Arc::clone(&self.gate).try_acquire_owned() {
            Ok(permit) => Ok(permit),
            Err(_) => Err(Error::unavailable(
                "slatedb engine queue depth reached — retry later".into(),
            )),
        }
    }

    pub fn available_permits(&self) -> usize {
        self.gate.available_permits()
    }
}

// --- process-global singleton bindings ---

static ENGINE_RUNTIME: OnceLock<EngineRuntime> = OnceLock::new();

/// Install the global engine runtime. Returns `Err(Invalid)` if called more
/// than once.
pub fn init(io_threads: usize, queue_depth: usize) -> Result<(), Error> {
    let rt = EngineRuntime::new(io_threads, queue_depth)?;
    ENGINE_RUNTIME
        .set(rt)
        .map_err(|_| Error::invalid("slatedb runtime already initialised".into()))
}

/// Read the global engine runtime if installed.
pub fn get() -> Option<&'static EngineRuntime> {
    ENGINE_RUNTIME.get()
}

/// Convenience: `get().block_on(future)`. Panics with a clear message when
/// the runtime hasn't been installed yet, since calling the bridge before
/// `init()` is a programmer bug, not a runtime condition.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let rt = ENGINE_RUNTIME
        .get()
        .unwrap_or_else(|| panic!("slatedb runtime not initialised — call runtime::init() first"));
    rt.block_on(future)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_on_runs_a_future() {
        let rt = EngineRuntime::new(2, 4).expect("build");
        let value = rt.block_on(async { 17 + 25 });
        assert_eq!(value, 42);
    }

    #[test]
    fn permit_gate_returns_unavailable_when_full() {
        let rt = EngineRuntime::new(1, 2).expect("build");
        let _p1 = rt.try_acquire_permit().expect("permit 1");
        let _p2 = rt.try_acquire_permit().expect("permit 2");
        let err = rt.try_acquire_permit().unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Unavailable));
    }

    #[test]
    fn dropping_a_permit_returns_a_slot() {
        let rt = EngineRuntime::new(1, 1).expect("build");
        {
            let _p = rt.try_acquire_permit().expect("permit");
            assert!(rt.try_acquire_permit().is_err());
        }
        // _p has dropped; the slot is back.
        assert!(rt.try_acquire_permit().is_ok());
    }

    #[test]
    fn worker_threads_and_queue_depth_are_clamped_to_one() {
        let rt = EngineRuntime::new(0, 0).expect("build");
        assert_eq!(rt.io_threads(), 1);
        assert_eq!(rt.queue_depth(), 1);
    }

    #[test]
    fn with_defaults_yields_a_usable_runtime() {
        let rt = EngineRuntime::with_defaults().expect("defaults");
        assert!(rt.io_threads() >= 1);
        assert!(rt.queue_depth() >= 1);
        // Smoke: future actually runs.
        rt.block_on(async {});
    }

    // The OnceLock-based init/get/block_on free functions are exercised by
    // their callers (the cxx bridge); we deliberately don't install the
    // global singleton from unit tests because OnceLock state would carry
    // across tests sharing the same process.
}
