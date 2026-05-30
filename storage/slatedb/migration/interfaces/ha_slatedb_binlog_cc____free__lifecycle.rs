//! Interface stub for `ha_slatedb_binlog_cc____free__lifecycle` (NEW unit).
//!
//! C++ source: `sql/handler.h` slots `binlog_init` (line 1604) and
//! `set_binlog_max_size` (line 1607). Coordinator call sites: server
//! startup in `sql/mysqld.cc:5763..5793` for `binlog_init`; sysvar
//! update path for `set_binlog_max_size`.
//!
//! v4 manifest sub-unit: NEW — added 2026-05-29 for the SlateDB-as-binlog
//! interface (`_DESIGN.md §14`).
//!
//! ## Mapping
//!
//! Per `_DESIGN.md §14`:
//! - `binlog_init` runs once at server startup. Our job:
//!   1. Open (or join) the shared SlateDB `Db` handle that the data
//!      engine also uses.
//!   2. Read `binlog_meta:rotation` to find the active file_no.
//!   3. Range-scan `binlog:*` looking for `ChunkType::XaPrepare` /
//!      `XaComplete` records (user-XA recovery — Stage 2; in Stage 1
//!      the scan is a no-op).
//!   4. Populate `recover_xid_hash` with one `BinlogXidInfo` per
//!      pending XID (Stage 2 only).
//! - `set_binlog_max_size` updates an `AtomicU64` cell read by the
//!   rotation policy in `admin::binlog_flush`.
//!
//! ## Out-of-scope methods
//! None — the two slots are fully scoped here.

use crate::ha_slatedb_binlog_h__types::XidRecoveryHash;
use slatedb::Error;

/// Coordinator entry point: server startup recovery.
///
/// Original: `sql/handler.h:1604` —
/// `bool (*binlog_init)(size_t binlog_size, const char *directory,
///                      HASH *recover_xid_hash);`
///
/// Called once during `TC_LOG_BINLOG::open()` (`sql/log.cc:12996`).
/// `binlog_size` is the configured `max_binlog_size`; `directory` is
/// the legacy `--log-bin-index` path (we use it as a hint for the
/// SlateDB object-store path if no other binding is set);
/// `recover_xid_hash` is a server-owned `HASH` keyed by XID bytes —
/// we insert one `BinlogXidInfo` per pending XID.
///
/// # Errors
/// `Error::data` if the binlog scan finds a malformed chunk;
/// `Error::unavailable` if the underlying object store is unreachable.
pub async fn binlog_init(
    _binlog_size: usize,
    _directory: &str,
    _recover_xid_hash: &mut XidRecoveryHash,
) -> Result<(), Error> {
    todo!(
        "1. Db::builder(directory, ...).build() OR join shared handle from data engine.\n\
         2. Read binlog_meta:rotation to discover active file_no.\n\
         3. (Stage 2) range-scan binlog: for XA chunks, populate recover_xid_hash.\n\
         4. Stage 1: no XA recovery; recover_xid_hash stays empty."
    )
}

/// Coordinator entry point: dynamic sysvar update.
///
/// Original: `sql/handler.h:1607` —
/// `void (*set_binlog_max_size)(size_t binlog_size);`
///
/// Updates the rotation threshold. In-flight writes are unaffected;
/// the new size takes effect on the next `binlog_flush` boundary check.
pub fn set_binlog_max_size(_binlog_size: usize) {
    todo!("BINLOG_MAX_SIZE.store(binlog_size, Ordering::Relaxed)")
}
