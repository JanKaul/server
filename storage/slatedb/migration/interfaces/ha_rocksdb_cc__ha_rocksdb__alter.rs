//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__alter`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (372 LoC body, 4 methods)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__alter`
//! parent: `ha_rocksdb_cc`
//!
//! ## Mapping
//! Inplace-ALTER vtable methods. Per _DESIGN.md §1, the dropped-index sweep
//! during commit uses SlateDB's `CompactionFilter` (feature
//! `compaction_filters`); the added-index backfill streams through our
//! per-statement write aggregator into a `WriteBatch`.
//!
//! The `Rdb_inplace_alter_ctx` (see `ha_rocksdb_h__Rdb_inplace_alter_ctx.rs`)
//! is the scratch struct threaded across these four callbacks.
//!
//! ## Out-of-scope methods
//! None — all four inplace-alter callbacks are in-scope. Some columns of
//! supported alterations may degrade to copy-alter (returning
//! `HA_ALTER_INPLACE_NOT_SUPPORTED`) if SlateDB lacks a primitive — flagged
//! by `// TODO(human):` in `check_if_supported_inplace_alter` once impl
//! starts.

use slatedb::Error;

use crate::ha_rocksdb_h__Rdb_inplace_alter_ctx::RdbInplaceAlterCtx;
use crate::ha_rocksdb_h__ha_rocksdb::{HaSlateDb, TableShareView};

/// Result enum mirroring MariaDB's `enum_alter_inplace_result`. The shim
/// translates back to the C++ enum at the cxx boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlterInplaceResult {
    /// Inplace alter not supported for this case — fall back to copy alter.
    NotSupported,
    /// Inplace alter supported, no exclusive lock needed.
    NoLock,
    /// Inplace alter supported, shared lock needed.
    SharedLock,
    /// Inplace alter supported, exclusive lock needed.
    ExclusiveLock,
}

impl HaSlateDb {
    /// Decide whether the requested alter can be done inplace.
    /// Original: ha_rocksdb.h:957 — `check_if_supported_inplace_alter`.
    ///
    /// Returns: appropriate result; never errors (decision is local).
    pub fn check_if_supported_inplace_alter(
        &self,
        altered: &TableShareView,
        ha_alter_info: &AlterInfoView,
    ) -> AlterInplaceResult {
        todo!("inspect ha_alter_info, return AlterInplaceResult based on what we can do inplace")
    }

    /// Phase 1: prepare. Allocate the `Rdb_inplace_alter_ctx` (returned via
    /// `ha_alter_info.handler_ctx`) and reserve new index ids in the system CF.
    /// Original: ha_rocksdb.h:961 — `prepare_inplace_alter_table`.
    ///
    /// Returns `Ok(ctx)` on success, `Err(slatedb::Error::invalid(...))` if
    /// the requested change can't be performed (e.g., adding a unique index
    /// on a column with existing duplicates).
    pub async fn prepare_inplace_alter_table(
        &mut self,
        altered: &TableShareView,
        ha_alter_info: &AlterInfoView,
    ) -> Result<RdbInplaceAlterCtx, Error> {
        todo!("reserve index ids; populate new_tdef + added/dropped sets; build ctx")
    }

    /// Phase 2: backfill added indexes. Scans the PK index (via `DbSnapshot`)
    /// and writes each row's SK encoding into the new index. Uses
    /// `inplace_populate_sk` (see `ha_rocksdb_cc__ha_rocksdb__write_path.rs`
    /// or whichever bucket holds it).
    /// Original: ha_rocksdb.h:965 — `inplace_alter_table`.
    ///
    /// Returns `Ok(false)` on success (note: MariaDB's bool-return convention
    /// — `false` is success). `Err(slatedb::Error)` on duplicate-key
    /// violation, I/O failure, or operator-cancellation.
    pub async fn inplace_alter_table(
        &mut self,
        altered: &TableShareView,
        ha_alter_info: &AlterInfoView,
        ctx: &mut RdbInplaceAlterCtx,
    ) -> Result<bool, Error> {
        todo!("for each added index: scan PK, encode SK, batch-write via WriteBatcher")
    }

    /// Phase 3: commit (or rollback). On commit: register dropped indexes in
    /// the system-CF dropped-index registry (which `Rdb_compact_filter` reads),
    /// swap the table definition atomically, return success.
    /// On rollback: discard the backfilled SKs (single tombstone-prefix write).
    /// Original: ha_rocksdb.h:969 — `commit_inplace_alter_table`.
    pub async fn commit_inplace_alter_table(
        &mut self,
        altered: &TableShareView,
        ha_alter_info: &AlterInfoView,
        ctx: RdbInplaceAlterCtx,
        commit: bool,
    ) -> Result<bool, Error> {
        todo!("commit: register dropped-index sweep + swap tdef; rollback: tombstone added-index prefix")
    }
}

/// Read-only view of MariaDB's `Alter_inplace_info`. Populated by the shim
/// from the C++ side before each phase. Fields are the subset our handler
/// inspects.
#[derive(Debug)]
pub struct AlterInfoView {
    /// Bitmask of `Alter_inplace_info::HA_ALTER_FLAGS`. We treat as raw u64
    /// and decode via constants matching `sql/handler.h`.
    pub handler_flags: u64,
    pub added_index_count: u32,
    pub dropped_index_count: u32,
}
