//! `slatedb::Error` ↔ MariaDB `HA_ERR_*` translation.
//!
//! Translated from `ha_rocksdb.cc:14001..14083` and the `rdb_log_status_error`
//! family in `rdb_utils.{h,cc}`. Per `_DESIGN.md §4`, the Rust crate carries
//! `slatedb::Error` natively; this module is the single boundary that
//! converts it to the integer error codes the cxx shim hands back to MariaDB.

use slatedb::Error;

/// Where the I/O failure originated. Mirrors the C++ `RDB_IO_ERROR_TYPE` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RdbIoErrorType {
    /// Failure during a user-transaction commit's WAL write. Fatal.
    TxCommit,
    /// Failure during data-dictionary WAL write. Fatal.
    DictCommit,
    /// Failure on a background-task write (compaction / drop-index). Fatal.
    BgThread,
    /// General I/O failure; logged, surfaced as `HA_ERR_GENERIC`.
    General,
}

/// String form for logging.
pub fn io_error_kind_str(t: RdbIoErrorType) -> &'static str {
    match t {
        RdbIoErrorType::TxCommit => "RDB_IO_ERROR_TX_COMMIT",
        RdbIoErrorType::DictCommit => "RDB_IO_ERROR_DICT_COMMIT",
        RdbIoErrorType::BgThread => "RDB_IO_ERROR_BG_THREAD",
        RdbIoErrorType::General => "RDB_IO_ERROR_GENERAL",
    }
}

/// `HA_ERR_*` codes used by the shim. Duplicated so the Rust crate stays
/// dependency-free of the MariaDB headers.
pub mod ha_err {
    pub const HA_ERR_GENERIC: i32 = 120;
    pub const HA_ERR_LOCK_DEADLOCK: i32 = 149;
    pub const HA_ERR_LOCK_WAIT_TIMEOUT: i32 = 146;
    pub const HA_ERR_CRASHED: i32 = 126;
    pub const HA_ERR_INTERNAL_ERROR: i32 = 168;
}

/// Translate `slatedb::Error → HA_ERR_*`. See `_DESIGN.md §4`.
pub fn slatedb_error_to_ha_err(e: &Error) -> i32 {
    use slatedb::ErrorKind::*;
    match e.kind() {
        Transaction => ha_err::HA_ERR_LOCK_DEADLOCK,
        Closed(_) => ha_err::HA_ERR_CRASHED,
        Unavailable => ha_err::HA_ERR_LOCK_WAIT_TIMEOUT,
        Invalid => ha_err::HA_ERR_GENERIC,
        Data => ha_err::HA_ERR_CRASHED,
        Internal => ha_err::HA_ERR_INTERNAL_ERROR,
        // `ErrorKind` is `#[non_exhaustive]`; future variants fall back to
        // the generic code rather than silently breaking the shim.
        _ => ha_err::HA_ERR_GENERIC,
    }
}

/// `tracing::error!` wrapper.
pub fn log_status_error(e: &Error, msg: &str) {
    tracing::error!(error = ?e, "{msg}");
}

/// Logged-and-aborting handler for fatal I/O failures, returning `Err` for the
/// `General` variant.
///
/// MyRocks' WAL-failure policy is `abort()`; we preserve that semantic by
/// panicking, which the cxx bridge surfaces as a process abort.
pub fn handle_io_error(e: Error, kind: RdbIoErrorType) -> Result<(), Error> {
    match kind {
        RdbIoErrorType::TxCommit
        | RdbIoErrorType::DictCommit
        | RdbIoErrorType::BgThread => {
            tracing::error!(
                error = ?e,
                kind = io_error_kind_str(kind),
                "aborting on WAL write error",
            );
            std::process::abort();
        }
        RdbIoErrorType::General => {
            log_status_error(&e, "SlateDB I/O failure");
            Err(e)
        }
    }
}

/// Compatibility marker filename. Read on startup to decide if we abort.
/// Distinct from MyRocks' `ROCKSDB_CORRUPTED` because both engines co-exist.
pub fn corruption_marker_file_name() -> &'static str {
    "SLATEDB_CORRUPTED"
}

/// Persist a corruption marker so the next startup refuses to load the
/// engine unless `--slatedb-allow-to-start-after-corruption=1`.
pub fn persist_corruption_marker(datadir: &std::path::Path) -> Result<(), Error> {
    let path = datadir.join(corruption_marker_file_name());
    std::fs::write(&path, b"")
        .map_err(|e| Error::data(format!("persist corruption marker: {e}")))?;
    tracing::error!(?path, "SlateDB corruption marker written");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_mapping_table_matches_design() {
        assert_eq!(
            slatedb_error_to_ha_err(&Error::invalid("nope".into())),
            ha_err::HA_ERR_GENERIC,
        );
        assert_eq!(
            slatedb_error_to_ha_err(&Error::data("bad".into())),
            ha_err::HA_ERR_CRASHED,
        );
    }

    #[test]
    fn corruption_marker_constant() {
        assert_eq!(corruption_marker_file_name(), "SLATEDB_CORRUPTED");
    }
}
