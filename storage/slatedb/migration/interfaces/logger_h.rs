//! Interface stub for `logger_h`.
//!
//! C++ source: `storage/rocksdb/logger.h` (85 LoC)
//! C++ class: `Rdb_logger : public rocksdb::Logger`
//!
//! ## Mapping
//! In MyRocks this is a `rocksdb::Logger` subclass that routes RocksDB's
//! internal log lines into MariaDB's error log (`error_log_print`). The
//! filtering is two-stage: RocksDB filters on its own `InfoLogLevel`, then
//! `Rdb_logger` re-filters before forwarding to MySQL's `loglevel` enum.
//!
//! SlateDB uses the **`tracing`** crate for structured logging — there is no
//! `slatedb::Logger` trait to subclass. Per _DESIGN.md §8 we wire SlateDB's
//! tracing output into the MariaDB error log via a `tracing_subscriber::Layer`
//! installed at plugin init.
//!
//! What remains real:
//!
//! - **`RdbLogger` struct** — installs a `tracing::Subscriber` layer that
//!   writes formatted records into the MariaDB error log via a cxx-bridged
//!   `error_log_print` shim.
//! - **Level mapping** — `tracing::Level → MariaDB loglevel`. Preserved
//!   verbatim from the C++ if-ladder.
//! - **`set_min_level`** — corresponds to the `rocksdb_log_level` sysvar
//!   (kept for parity, applied to the tracing subscriber's filter).
//!
//! ## Out-of-scope methods
//! - `Logv(InfoLogLevel, fmt, va_list)` / `Logv(fmt, va_list)` — varargs
//!   formatting from C; replaced by `tracing::event!`. The cxx bridge calls
//!   `error_log_print` directly for legacy callers.
//! - `SetRocksDBLogger(shared_ptr<rocksdb::Logger>)` — chained loggers in
//!   RocksDB. SlateDB tracing layers are composed differently; not needed.

use slatedb::Error;

/// Severity levels matching MariaDB's `loglevel` enum, exposed to Rust callers
/// so we don't need to leak the C++ type. Mirrors C++ `Rdb_logger` mapping
/// (logger.h:40-50).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MariaLogLevel {
    Error = 0,
    Warning = 1,
    Information = 2,
}

impl MariaLogLevel {
    /// Project a `tracing::Level` onto MariaDB's three-bucket scheme.
    /// Matches the C++ if-ladder: ERROR_LEVEL = TRACE+, WARN = WARN+, else INFO.
    pub fn from_tracing(level: tracing::Level) -> Self {
        match level {
            tracing::Level::ERROR => Self::Error,
            tracing::Level::WARN => Self::Warning,
            tracing::Level::INFO | tracing::Level::DEBUG | tracing::Level::TRACE => {
                Self::Information
            }
        }
    }
}

/// Plugin-lifetime logger. Owns a guard for the installed `tracing` layer so
/// dropping it un-installs cleanly at `plugin::done`.
///
/// Replaces C++ `Rdb_logger`. The fields are intentionally private since the
/// public surface is just `init`/`shutdown`/`set_min_level`.
pub struct RdbLogger {
    min_level: MariaLogLevel,
    // TODO(human): pick a concrete subscriber guard type
    // (`tracing::subscriber::DefaultGuard` for thread-local, or a
    //  `WorkerGuard` if we use `tracing_appender`).
    _guard: Option<()>,
}

impl RdbLogger {
    /// Install the layer. The cxx bridge supplies a callback that writes a
    /// fully-formatted line into MariaDB's `error_log_print`.
    ///
    /// `min_level` corresponds to the `rocksdb_log_level` sysvar.
    pub fn install(min_level: MariaLogLevel) -> Result<Self, Error> {
        let _ = min_level;
        todo!("build a tracing_subscriber::fmt::Layer with a custom MakeWriter that forwards to error_log_print")
    }

    /// Live-update the filter level when the sysvar changes.
    pub fn set_min_level(&mut self, level: MariaLogLevel) {
        self.min_level = level;
        // TODO(human): reload the EnvFilter via a `Handle<EnvFilter, _>::reload(...)`.
    }

    pub fn min_level(&self) -> MariaLogLevel { self.min_level }
}

impl Drop for RdbLogger {
    fn drop(&mut self) {
        // The subscriber guard, if held, removes the layer on drop — no
        // explicit shutdown needed.
    }
}

/// Convenience: log a one-shot message at the given level. Used by the cxx
/// bridge for callers that aren't already wired through `tracing`. Replaces
/// the `Logv(fmt, va_list)` entry points of C++ `Rdb_logger`.
pub fn log(level: MariaLogLevel, msg: &str) {
    match level {
        MariaLogLevel::Error => tracing::error!("{msg}"),
        MariaLogLevel::Warning => tracing::warn!("{msg}"),
        MariaLogLevel::Information => tracing::info!("{msg}"),
    }
}
