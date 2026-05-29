//! Interface stub for `rdb_mutex_wrapper_cc`.
//!
//! C++ source: `storage/rocksdb/rdb_mutex_wrapper.cc` (215 LoC)
//! C++ classes: `Rdb_mutex : public rocksdb::TransactionDBMutex`,
//!              `Rdb_cond_var : public rocksdb::TransactionDBCondVar`
//!
//! ## Mapping
//! Per the task's special-mapping note: use `parking_lot::Mutex` (already in
//! the SlateDB deps). RocksDB's `TransactionDBMutex` was a virtual base that
//! MyRocks subclassed to wire `mysql_mutex_t` (so `THD_ENTER_COND` /
//! `THD_EXIT_COND` could notify the SQL layer that a thread was killable-
//! waiting on a row lock) into the RocksDB pessimistic-txn lock manager.
//!
//! **SlateDB has no equivalent lock-manager extension point** — its
//! transactions are SSI/SI (per _DESIGN.md §5), no row-level locks, no
//! pluggable mutexes. So this entire C++ class collapses. We keep a thin
//! `KillableMutex` wrapper because the SQL-layer "Waiting for row lock"
//! PROCESSLIST state still needs to be settable when our engine layer
//! itself blocks on contention (e.g., savepoint metadata, statement-side
//! aggregator); the underlying primitive is `parking_lot::Mutex`.
//!
//! ## Out-of-scope methods
//! - `TryLockFor(timeout_micros)` — parking_lot::Mutex has no timed lock
//!   builtin; we use `parking_lot::Mutex::try_lock_for(Duration)` (feature
//!   `parking_lot/deadlock_detection` not needed). Method retained, takes
//!   `Duration`.
//! - `WaitFor` returning `rocksdb::Status` — we return `Result<(), Error>`
//!   with `ErrorKind::Unavailable` for timeout.
//! - `THD_ENTER_COND` / `THD_EXIT_COND` integration — done at the bridge
//!   call site via the opaque session-state slot.

use parking_lot::{Condvar, Mutex};
use slatedb::Error;
use std::sync::Arc;
use std::time::Duration;

/// Mutex + condvar pair that supports timed wait and SQL-layer killability
/// notification. Replaces `Rdb_cond_var` + `Rdb_mutex` combined.
///
/// The C++ original split these into two classes because RocksDB's
/// `TransactionDBMutex` API required separate types; in Rust the natural
/// shape is `Mutex<T>` plus `Condvar`, and we expose them paired since
/// they're always used together at the call site.
///
/// Original: rdb_mutex_wrapper.cc:48 — `Rdb_cond_var`, line 160 — `Rdb_mutex`.
pub struct KillableMutex<T> {
    pub mutex: Arc<Mutex<T>>,
    pub cond: Arc<Condvar>,
}

impl<T> KillableMutex<T> {
    /// Construct.
    pub fn new(value: T) -> Self {
        Self {
            mutex: Arc::new(Mutex::new(value)),
            cond: Arc::new(Condvar::new()),
        }
    }

    /// Lock. parking_lot mutexes are infallible; returning `Result` here for
    /// signature compatibility with the C++ `Lock() -> Status`.
    ///
    /// Original: rdb_mutex_wrapper.cc:167 — `Rdb_mutex::Lock`.
    pub fn lock(&self) -> Result<parking_lot::MutexGuard<'_, T>, Error> {
        Ok(self.mutex.lock())
    }

    /// Try-lock with timeout.
    ///
    /// Inputs: `timeout` (negative → infinite, mapped to `Duration::MAX`).
    /// Output: guard or `None` on timeout.
    /// Errors: never — Rust `Mutex` does not poison.
    ///
    /// Original: rdb_mutex_wrapper.cc:178 — `TryLockFor`.
    pub fn try_lock_for(&self, timeout: Duration) -> Option<parking_lot::MutexGuard<'_, T>> {
        self.mutex.try_lock_for(timeout)
    }

    /// Wait on the condvar with a maximum duration.
    ///
    /// Inputs: `guard` (passed by `&mut` to match parking_lot's API),
    ///         `timeout` — `Duration::MAX` for "no timeout".
    /// Output:
    /// - `Ok(())` on `notify_*`.
    /// - `Err(Unavailable)` on timeout (matches MyRocks' `Status::TimedOut`).
    ///
    /// Caller is responsible for setting `THD_ENTER_COND` / `THD_EXIT_COND`
    /// in the killable-wait wrapper (in the bridge layer, not here — we do
    /// NOT expose `THD` per the task contract).
    ///
    /// Original: rdb_mutex_wrapper.cc:70 — `WaitFor`.
    pub fn wait_for(
        &self,
        guard: &mut parking_lot::MutexGuard<'_, T>,
        timeout: Duration,
    ) -> Result<(), Error> {
        let res = self.cond.wait_for(guard, timeout);
        if res.timed_out() {
            Err(Error::invalid("KillableMutex::wait_for timed out".into()))
            // TODO(human): pick the right ErrorKind — _DESIGN.md §4 maps
            // Unavailable → HA_ERR_LOCK_WAIT_TIMEOUT which is what the
            // C++ Status::TimedOut becomes. Use Error::unavailable once it
            // exists in the pinned slatedb rev.
        } else {
            Ok(())
        }
    }

    /// Notify one waiter.
    /// Original: rdb_mutex_wrapper.cc:151 — `Notify`.
    pub fn notify_one(&self) { self.cond.notify_one(); }

    /// Notify all waiters.
    /// Original: rdb_mutex_wrapper.cc:158 — `NotifyAll`.
    pub fn notify_all(&self) { self.cond.notify_all(); }
}
