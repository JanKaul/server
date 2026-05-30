//! Engine background-task framework.
//!
//! Translated from `storage/rocksdb/rdb_threads.h`. MyRocks used a pthread
//! base class (`Rdb_thread`) with a stop-flag + condvar wakeup. The Rust
//! crate uses Tokio tasks instead, with `CancellationToken` for the stop
//! signal and `JoinSet` for shutdown bookkeeping.
//!
//! Concrete tasks (background compaction, drop-index sweep, stats refresh,
//! manual-compaction worker) implement [`EngineTask`] and are spawned at
//! plugin init via [`spawn_task`].

use slatedb::Error;
use tokio_util::sync::CancellationToken;

/// Long-running background work the engine spawns at plugin init.
///
/// Implementors take ownership of their state and consume themselves in
/// [`run`](EngineTask::run). The `CancellationToken` fires when the plugin
/// is being torn down — every loop body should check it.
#[async_trait::async_trait]
pub trait EngineTask: Send + 'static {
    /// Long-running task body. Should exit cleanly when `cancel.is_cancelled()`.
    /// Returns `Err` only for unexpected failures (logged by the framework);
    /// cancellation returns `Ok(())`.
    async fn run(self, cancel: CancellationToken) -> Result<(), Error>;

    /// Symbolic name for logs / metrics. Used by the runtime to tag spans.
    fn name(&self) -> &'static str;
}

/// Spawn an [`EngineTask`] onto the given runtime, registering it with the
/// supplied `JoinSet` so the shutdown path can await completion.
pub fn spawn_task<T: EngineTask>(
    task: T,
    runtime: &tokio::runtime::Handle,
    cancel: CancellationToken,
    join_set: &mut tokio::task::JoinSet<()>,
) {
    let name = task.name();
    join_set.spawn_on(
        async move {
            if let Err(e) = task.run(cancel).await {
                tracing::error!(task = name, error = ?e, "engine task failed");
            }
        },
        runtime,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountToCancel {
        ticks: Arc<AtomicUsize>,
        started: Arc<tokio::sync::Notify>,
    }

    #[async_trait::async_trait]
    impl EngineTask for CountToCancel {
        async fn run(self, cancel: CancellationToken) -> Result<(), Error> {
            self.started.notify_one();
            loop {
                if cancel.is_cancelled() {
                    return Ok(());
                }
                self.ticks.fetch_add(1, Ordering::Relaxed);
                tokio::task::yield_now().await;
            }
        }
        fn name(&self) -> &'static str {
            "count-to-cancel"
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancellation_stops_the_task() {
        let cancel = CancellationToken::new();
        let mut join_set = tokio::task::JoinSet::new();
        let ticks = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(tokio::sync::Notify::new());
        let task = CountToCancel {
            ticks: Arc::clone(&ticks),
            started: Arc::clone(&started),
        };
        spawn_task(task, &tokio::runtime::Handle::current(), cancel.clone(), &mut join_set);

        started.notified().await;
        // Yield once more so the task body runs at least one full iteration.
        tokio::task::yield_now().await;
        cancel.cancel();

        while join_set.join_next().await.is_some() {}
        assert!(ticks.load(Ordering::Relaxed) > 0, "task should have ticked");
    }

    struct AlwaysErr;

    #[async_trait::async_trait]
    impl EngineTask for AlwaysErr {
        async fn run(self, _cancel: CancellationToken) -> Result<(), Error> {
            Err(Error::invalid("boom".into()))
        }
        fn name(&self) -> &'static str {
            "always-err"
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn error_returning_task_is_swallowed_by_framework() {
        let cancel = CancellationToken::new();
        let mut join_set = tokio::task::JoinSet::new();
        spawn_task(AlwaysErr, &tokio::runtime::Handle::current(), cancel, &mut join_set);
        // Framework logs and discards the Err; join_next never panics.
        while let Some(res) = join_set.join_next().await {
            assert!(res.is_ok());
        }
    }
}
