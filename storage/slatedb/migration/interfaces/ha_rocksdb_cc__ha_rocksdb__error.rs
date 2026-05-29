//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__error`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (span 6940..14655, body ~109 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__error`
//!
//! ## Mapping
//! Error-translation bridge between SlateDB's `slatedb::Error` and MariaDB's
//! `HA_ERR_*` codes, plus the user-facing `SHOW ENGINE` error-message hook.
//!
//! Per _DESIGN.md §4: we do NOT invent a `SlateError` enum. The crate exposes
//! `slatedb::Error` with a 6-variant `ErrorKind`; this bucket owns the
//! translation table.
//!
//! Key collapse: MyRocks' `rdb_error_to_mysql` had a 13-arm switch over
//! `rocksdb::Status::Code`. Ours is a 6-arm match per the design doc. The
//! `IsLockLimit` / `IsDeadlock` sub-status checks in the C++ version
//! disappear because SlateDB's `ErrorKind::Transaction` already encodes the
//! conflict-detection result without needing a sub-code.
//!
//! ## Out-of-scope methods
//! None. All three methods translate cleanly.

use slatedb::Error;
use slatedb::ErrorKind;

/// Forward-declared. Owned by A2's `ha_rocksdb_h` stub.
pub struct HaSlateDb;

/// MariaDB `HA_ERR_*` codes we need. The actual numbers come from
/// `include/my_base.h`; we re-declare a minimal set here. The real engine
/// will pull these from the cxx bridge to avoid drift.
pub mod ha_err {
    pub const SUCCESS: i32 = 0;
    pub const GENERIC: i32 = 120;
    pub const LOCK_WAIT_TIMEOUT: i32 = 146;
    pub const LOCK_DEADLOCK: i32 = 149;
    pub const CRASHED: i32 = 126;
    pub const INTERNAL_ERROR: i32 = 168;

    // SlateDB-engine-specific codes start at HA_ERR_SLATEDB_FIRST
    // (see rdb_global_h::HA_ERR_SLATEDB_FIRST = 550). The full message table
    // lives in `error_messages` below.
    pub const SLATEDB_FIRST: i32 = 550;
    pub const SLATEDB_LAST: i32 = SLATEDB_FIRST + 7;
}

/// One row per HA_ERR_SLATEDB_* code. Indexed by `code - HA_ERR_SLATEDB_FIRST`.
/// Matches the C++ `rdb_error_messages` table layout so the cxx-bridge can
/// stay 1:1.
pub static ERROR_MESSAGES: &[&str] = &[
    "SlateDB: lock acquire wait timeout",
    "SlateDB: transaction conflict — concurrent write detected",
    "SlateDB: durably-stored data is corrupt or inconsistent",
    "SlateDB: object store unavailable",
    "SlateDB: invalid configuration or argument",
    "SlateDB: database has been shut down",
    "SlateDB: internal bug — please report",
    "SlateDB: too many open snapshots",
];

impl HaSlateDb {
    /// Append a human-readable error message for `error` to `out`. Returns
    /// `true` iff a message was appended (i.e., the code was recognized).
    /// Falls back to no append + `false` for codes outside the SlateDB range
    /// — the SQL layer then uses its own generic message.
    ///
    /// In MyRocks this also pulled the per-txn `m_detailed_error` for
    /// `LOCK_WAIT_TIMEOUT`/`LOCK_DEADLOCK`/`STATUS_BUSY`. We do the same
    /// via the txn-registry lookup (`crate::rdb_global_h::TrxInfo`), but
    /// only if the current thread has an active txn.
    ///
    /// Original C++: ha_rocksdb.cc:6940 — `bool ha_rocksdb::get_error_message(...)`.
    pub fn get_error_message(&self, error: i32, out: &mut String) -> bool {
        let _ = (error, out);
        todo!("if LOCK_*: append txn.m_detailed_error; else if SLATEDB range: append ERROR_MESSAGES[code-FIRST]; else false")
    }

    /// Translate a `slatedb::Error` into a MariaDB `HA_ERR_*` code, then
    /// call `my_error(ER_GET_ERRMSG, ...)` with the message text. Returns
    /// the chosen `HA_ERR_*` code.
    ///
    /// This is the single point in the engine where every `Result<_, Error>`
    /// from SlateDB gets adapted — per _DESIGN.md §4 the mapping is fixed:
    ///
    /// | `ErrorKind` | `HA_ERR_*`                  |
    /// |-------------|-----------------------------|
    /// | Transaction | `LOCK_DEADLOCK`             |
    /// | Closed(_)   | `CRASHED`                   |
    /// | Unavailable | `LOCK_WAIT_TIMEOUT`         |
    /// | Invalid     | `GENERIC` (should be unreachable in well-formed callers) |
    /// | Data        | `CRASHED`                   |
    /// | Internal    | `INTERNAL_ERROR`            |
    ///
    /// `opt_msg` is an optional extra context string concatenated into the
    /// final user-facing message (matches the C++ `opt_msg` param).
    ///
    /// Original C++: ha_rocksdb.cc:6975.
    pub fn slatedb_error_to_ha(&self, err: &Error, opt_msg: Option<&str>) -> i32 {
        let _ = opt_msg;
        let _ = err;
        // Body would be:
        //   match err.kind() {
        //     ErrorKind::Transaction => ha_err::LOCK_DEADLOCK,
        //     ErrorKind::Closed(_)   => ha_err::CRASHED,
        //     ErrorKind::Unavailable => ha_err::LOCK_WAIT_TIMEOUT,
        //     ErrorKind::Invalid     => ha_err::GENERIC,
        //     ErrorKind::Data        => ha_err::CRASHED,
        //     ErrorKind::Internal    => ha_err::INTERNAL_ERROR,
        //   }
        // — plus a my_error() callout. Left as todo!() so the cxx-bridge
        // can be wired in TRANSLATE.
        todo!("apply the 6-arm match from _DESIGN.md §4 + my_error() callout")
    }

    /// Pre-`print_error` hook. The C++ version rewrites
    /// `HA_ERR_ROCKSDB_STATUS_BUSY → HA_ERR_LOCK_DEADLOCK` because the
    /// SQL layer needs the canonical deadlock code to trigger a retry.
    /// We collapse that: with our 6-arm mapping above, `Transaction` already
    /// maps directly to `LOCK_DEADLOCK`, so this rewrite is unnecessary —
    /// the function reduces to a delegation to `handler::print_error`,
    /// which the cxx bridge owns.
    ///
    /// Original C++: ha_rocksdb.cc:14650.
    pub fn print_error(&self, error: i32, errflag: u32) {
        let _ = (error, errflag);
        todo!("call into cxx::ffi::handler_print_error(self.handler_ptr, error, errflag)")
    }
}

// Compile-time check that the kind→code mapping is exhaustive. Disabled
// (commented) because `ErrorKind` is non_exhaustive in some slatedb versions.
//
// const _: () = {
//     fn _exhaustive(k: ErrorKind) -> i32 {
//         match k {
//             ErrorKind::Transaction => 0,
//             ErrorKind::Closed(_)   => 0,
//             ErrorKind::Unavailable => 0,
//             ErrorKind::Invalid     => 0,
//             ErrorKind::Data        => 0,
//             ErrorKind::Internal    => 0,
//         }
//     }
// };
#[allow(dead_code)]
fn _kind_marker(_: ErrorKind) {}
