//! Interface stub for `rdb_threads_h`.
//!
//! C++ source: `storage/rocksdb/rdb_threads.h` (195 LoC)
//!
//! ## Mapping
//! MyRocks' background-thread base class (`Rdb_thread`). Used by
//! `Rdb_background_thread`, `Rdb_drop_index_thread`,
//! `Rdb_manual_compaction_thread`, etc.
//!
//! Per _DESIGN.md, replaced by **Tokio tasks** on the engine runtime.
//! Cancellation via `tokio_util::sync::CancellationToken`. PSI hooks
//! dropped (out-of-scope v1).
//!
//! ## Out-of-scope methods
//! None — the underlying primitive shifts from pthread to Tokio task,
//! but every public method has a clean equivalent.

use slatedb::Error;

/// Base trait for engine background tasks. Implementors provide `run()`;
/// the framework spawns them on the engine runtime and tracks them in a
/// `JoinSet` so shutdown can await completion.
///
/// Original: rdb_threads.h — `class Rdb_thread`.
#[async_trait::async_trait]
pub trait EngineTask: Send + 'static {
    /// Long-running task body. Should exit cleanly when `cancel.is_cancelled()`.
    /// Returns `Err` only for unexpected failures (logged); cancellation
    /// returns `Ok(())`.
    async fn run(self, cancel: tokio_util::sync::CancellationToken) -> Result<(), Error>;

    /// Symbolic name for logs / metrics. Used by the runtime to tag spans.
    fn name(&self) -> &'static str;
}

/// Spawn an `EngineTask` onto the engine's runtime, registering it with
/// the supplied `JoinSet` so the shutdown path can join. Mirrors
/// `Rdb_thread::start`.
pub fn spawn_task<T: EngineTask>(
    task: T,
    runtime: &tokio::runtime::Handle,
    cancel: tokio_util::sync::CancellationToken,
    join_set: &mut tokio::task::JoinSet<()>,
) {
    let name = task.name();
    let cancel_clone = cancel.clone();
    join_set.spawn_on(
        async move {
            if let Err(e) = task.run(cancel_clone).await {
                tracing::error!("engine task {} failed: {:?}", name, e);
            }
        },
        runtime,
    );
}
