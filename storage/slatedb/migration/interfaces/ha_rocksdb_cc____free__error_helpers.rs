//! Interface stub for `ha_rocksdb_cc____free__error_helpers`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 14001..14083, ~83 LoC)
//!             + `rdb_utils.{h,cc}` (`rdb_log_status_error`).
//! v4 manifest sub-unit: `ha_rocksdb_cc____free__error_helpers`
//!
//! ## Mapping
//! Per _DESIGN.md §4 (error model): we use `slatedb::Error` natively across
//! the Rust crate. These helpers are the entry/exit-side translation layer:
//!
//!   - `slatedb_error_to_ha_err(&Error) -> i32` — collapse the 6-variant
//!     `ErrorKind` to the equivalent `HA_ERR_*` integer (mirrors the C++
//!     `ha_rocksdb::rdb_error_to_mysql`).
//!   - `handle_io_error(Error, RdbIoErrorType) -> !` or `Result<(), Error>` —
//!     decide whether to log+abort (MyRocks does `abort()` on WAL/I-O write
//!     failures) or return the error to the caller for normal handling.
//!   - `log_status_error(&Error, msg)` — structured `tracing::error!`.
//!
//! The original `RDB_IO_ERROR_TYPE` enum (4 categories) maps 1:1 to a Rust enum.
//!
//! Per _DESIGN.md §4: the unified return type is `Result<T, slatedb::Error>`;
//! `unwrap()` is forbidden in non-test code.
//!
//! ## Out-of-scope methods
//! - `abort()` on WAL write failure — we preserve the C++ semantic (abort)
//!   for `IoErrorTxCommit`/`IoErrorDictCommit`/`IoErrorBgThread`. This is
//!   a deliberate panic-on-WAL-failure policy; per _DESIGN.md §4 we lean
//!   on `Error::data` to signal Mass-corruption, after which the caller is
//!   expected to refuse new writes (engine becomes read-only or stops).

use slatedb::Error;

/// Where the I/O failure originated. Mirrors the C++ `RDB_IO_ERROR_TYPE` enum.
/// Original: declared in `rdb_utils.h`; the C++ enum has 4 values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RdbIoErrorType {
    /// `RDB_IO_ERROR_TX_COMMIT` — failure during a user-transaction commit's
    /// WAL write. Fatal (we abort/panic).
    TxCommit,
    /// `RDB_IO_ERROR_DICT_COMMIT` — failure during data-dictionary WAL write.
    /// Fatal.
    DictCommit,
    /// `RDB_IO_ERROR_BG_THREAD` — failure on a background-task write
    /// (compaction / drop-index / manual-compaction). Fatal.
    BgThread,
    /// `RDB_IO_ERROR_GENERAL` — general I/O failure; abort but log richer
    /// context (read path, etc.).
    General,
}

/// String form for logging.
/// Original: ha_rocksdb.cc:14001 — `get_rdb_io_error_string`.
pub fn io_error_kind_str(t: RdbIoErrorType) -> &'static str {
    match t {
        RdbIoErrorType::TxCommit   => "RDB_IO_ERROR_TX_COMMIT",
        RdbIoErrorType::DictCommit => "RDB_IO_ERROR_DICT_COMMIT",
        RdbIoErrorType::BgThread   => "RDB_IO_ERROR_BG_THREAD",
        RdbIoErrorType::General    => "RDB_IO_ERROR_GENERAL",
    }
}

/// HA_ERR_* codes used by the shim (these mirror MariaDB headers; we
/// duplicate them here so the Rust crate is dependency-free of MariaDB).
pub mod ha_err {
    pub const HA_ERR_GENERIC: i32 = 120;
    pub const HA_ERR_LOCK_DEADLOCK: i32 = 149;
    pub const HA_ERR_LOCK_WAIT_TIMEOUT: i32 = 146;
    pub const HA_ERR_CRASHED: i32 = 126;
    pub const HA_ERR_INTERNAL_ERROR: i32 = 168;
}

/// Translate `slatedb::Error → HA_ERR_*`. Exactly the table in _DESIGN.md §4.
/// This is the single entry point used by every cxx-bridge wrapper.
pub fn slatedb_error_to_ha_err(e: &Error) -> i32 {
    use slatedb::ErrorKind::*;
    match e.kind() {
        Transaction => ha_err::HA_ERR_LOCK_DEADLOCK,
        Closed(_)   => ha_err::HA_ERR_CRASHED,
        Unavailable => ha_err::HA_ERR_LOCK_WAIT_TIMEOUT,
        Invalid     => ha_err::HA_ERR_GENERIC,
        Data        => ha_err::HA_ERR_CRASHED,
        Internal    => ha_err::HA_ERR_INTERNAL_ERROR,
    }
}

/// Structured log + (for `TxCommit`/`DictCommit`/`BgThread`) abort. The
/// `general` variant logs and returns `Err(e)` so the caller can map to
/// `HA_ERR_GENERIC` via `slatedb_error_to_ha_err`.
///
/// Original: ha_rocksdb.cc:14027 — `rdb_handle_io_error`.
///
/// # Panics
/// Panics (= `abort` on the C++ side) on the three fatal categories. This is
/// MyRocks' explicit "fail loud" policy on write-path I/O; SlateDB has the
/// same guarantee (corruption → `Error::data`, and we treat that as fatal
/// in `TxCommit` context).
pub fn handle_io_error(e: Error, kind: RdbIoErrorType) -> Result<(), Error> {
    match kind {
        RdbIoErrorType::TxCommit
        | RdbIoErrorType::DictCommit
        | RdbIoErrorType::BgThread => {
            todo!("tracing::error!(?e, kind = ?io_error_kind_str(kind), 'aborting on WAL write error'); std::process::abort()")
        }
        RdbIoErrorType::General => {
            log_status_error(&e, "Failed to read/write in SlateDB");
            Err(e)
        }
    }
}

/// `tracing::error!` wrapper with `msg` prefix. Mirrors `rdb_log_status_error`.
///
/// Original: rdb_utils.h:258 / rdb_utils.cc:283 — `rdb_log_status_error`.
pub fn log_status_error(_e: &Error, _msg: &str) {
    todo!("tracing::error!(?_e, _msg, 'SlateDB error')")
}

/// Persist a corruption marker so the next startup refuses to load the
/// engine unless `--slatedb-allow-to-start-after-corruption=1`. MyRocks
/// writes a sentinel file `./ROCKSDB_CORRUPTED`. For the SlateDB engine
/// we use `./SLATEDB_CORRUPTED` to distinguish the two markers (the
/// engines co-exist in tree per §1 "ha_rocksdb removal: never").
/// Original: rdb_utils — `rdb_persist_corruption_marker`.
pub fn persist_corruption_marker(_datadir: &std::path::Path) -> Result<(), Error> {
    todo!("std::fs::write(datadir.join('SLATEDB_CORRUPTED'), b'') and tracing::error!")
}

/// Compatibility marker filename. Read on startup to decide if we abort.
pub fn corruption_marker_file_name() -> &'static str { "SLATEDB_CORRUPTED" }
