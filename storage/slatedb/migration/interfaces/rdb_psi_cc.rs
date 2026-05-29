//! Interface stub for `rdb_psi_cc`.
//!
//! C++ source: `storage/rocksdb/rdb_psi.cc` (112 LoC)
//!
//! ## Mapping
//! Per the task contract: PSI (Performance Schema) integration is **OUT OF
//! SCOPE for v1**. The C++ file registers per-stage / per-thread /
//! per-mutex / per-condvar / per-rwlock instrument keys with MariaDB's
//! `mysql_*_register` API so the `performance_schema` tables show MyRocks
//! activity. Without it, P_S simply doesn't see our threads — same as
//! MariaDB engines built without `HAVE_PSI_INTERFACE`.
//!
//! We keep the symbol names as `pub static` zero-valued keys so existing
//! call sites compile without conditional compilation. All `init_*` /
//! `register_*` functions are stubbed no-ops returning `Ok(())`.
//!
//! When we re-enable P_S in a later phase the implementation lives behind a
//! `#[cfg(feature = "psi")]` flag and wires through `cxx-bridge` to the
//! MariaDB-side `PSI_server` calls.
//!
//! ## Out-of-scope methods (no-op stubs)
//! - `init_rocksdb_psi_keys` — register everything.
//! - All `key_*` / `*_psi_*_key` statics — kept as `AtomicU32(0)` so address
//!   arithmetic still works at the cxx-bridge edge.

use slatedb::Error;
use std::sync::atomic::AtomicU32;

/// PSI mutex key — opaque integer identifying one mutex class to P_S.
/// Always 0 in v1 (unregistered).
pub static RDB_PSI_OPEN_TBLS_MUTEX_KEY: AtomicU32 = AtomicU32::new(0);
pub static RDB_SIGNAL_BG_PSI_MUTEX_KEY: AtomicU32 = AtomicU32::new(0);
pub static RDB_SIGNAL_DROP_IDX_PSI_MUTEX_KEY: AtomicU32 = AtomicU32::new(0);
pub static RDB_SIGNAL_MC_PSI_MUTEX_KEY: AtomicU32 = AtomicU32::new(0);
pub static RDB_COLLATION_DATA_MUTEX_KEY: AtomicU32 = AtomicU32::new(0);
pub static RDB_MEM_CMP_SPACE_MUTEX_KEY: AtomicU32 = AtomicU32::new(0);
pub static KEY_MUTEX_TX_LIST: AtomicU32 = AtomicU32::new(0);
pub static RDB_SYSVARS_PSI_MUTEX_KEY: AtomicU32 = AtomicU32::new(0);
pub static RDB_CFM_MUTEX_KEY: AtomicU32 = AtomicU32::new(0);
pub static RDB_SST_COMMIT_KEY: AtomicU32 = AtomicU32::new(0);
pub static RDB_BLOCK_CACHE_RESIZE_MUTEX_KEY: AtomicU32 = AtomicU32::new(0);

/// PSI rwlock keys.
pub static KEY_RWLOCK_COLLATION_EXCEPTION_LIST: AtomicU32 = AtomicU32::new(0);
pub static KEY_RWLOCK_READ_FREE_RPL_TABLES: AtomicU32 = AtomicU32::new(0);
pub static KEY_RWLOCK_SKIP_UNIQUE_CHECK_TABLES: AtomicU32 = AtomicU32::new(0);

/// PSI condvar keys.
pub static RDB_SIGNAL_BG_PSI_COND_KEY: AtomicU32 = AtomicU32::new(0);
pub static RDB_SIGNAL_DROP_IDX_PSI_COND_KEY: AtomicU32 = AtomicU32::new(0);
pub static RDB_SIGNAL_MC_PSI_COND_KEY: AtomicU32 = AtomicU32::new(0);

/// PSI thread keys.
pub static RDB_BACKGROUND_PSI_THREAD_KEY: AtomicU32 = AtomicU32::new(0);
pub static RDB_DROP_IDX_PSI_THREAD_KEY: AtomicU32 = AtomicU32::new(0);
pub static RDB_MC_PSI_THREAD_KEY: AtomicU32 = AtomicU32::new(0);

/// PSI stage key — singleton, name "Waiting for row lock".
/// Original: rdb_psi.cc:27 — `stage_waiting_on_row_lock`.
pub static STAGE_WAITING_ON_ROW_LOCK: AtomicU32 = AtomicU32::new(0);

/// Register all PSI instruments. v1: no-op.
///
/// When PSI lands in a later phase this will call the cxx-bridge thunks
/// that wrap `mysql_mutex_register`, `mysql_thread_register`,
/// `mysql_stage_register`. For now it returns `Ok(())` so the handlerton
/// init sequence is unchanged.
///
/// Errors: never (out-of-scope stub).
///
/// Original: rdb_psi.cc:86 — `init_rocksdb_psi_keys`.
pub fn init_rocksdb_psi_keys() -> Result<(), Error> {
    // OUT OF SCOPE v1: no PSI integration.
    Ok(())
}
