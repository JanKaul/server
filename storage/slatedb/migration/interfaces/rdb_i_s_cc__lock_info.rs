//! Interface stub for `rdb_i_s_cc__lock_info`.
//!
//! C++ source: `storage/rocksdb/rdb_i_s.cc` (lines 1439..1519, plug at 1928..1942)
//! Body LoC: ~81
//! v4 manifest sub-unit: `rdb_i_s_cc__lock_info`
//! Parent: `rdb_i_s_cc`
//!
//! ## Mapping
//! Per _DESIGN.md §1 — **Re-implemented**.
//!
//! MyRocks calls `TransactionDB::GetLockStatusData()` which returns a
//! `unordered_multimap<cf_id, KeyLockInfo{key, ids, exclusive}>`. SlateDB has
//! no row-locking layer (it uses optimistic SSI via `mark_read`/conflict
//! detection at commit), so there are no "held locks" in the RocksDB sense.
//!
//! We re-implement against our **per-Txn registry** (see `_DESIGN.md` §1 row
//! "lock_info / trx_info / deadlock_info"): for each active `DbTransaction`
//! we wrap, we track the read/write key sets. The `lock_info` table reports
//! the *write set* of each active transaction as `MODE = "X"` rows. We don't
//! report a separate `MODE = "S"` row set — SlateDB's SSI uses range tracking,
//! not point-shared locks. The `cf_id` is decoded from each key's prefix.
//!
//! For tables explicitly opened `LOCK IN SHARE MODE` / `FOR UPDATE` we
//! additionally record an entry in our registry annotated as shared/exclusive,
//! preserving the column semantics for users who rely on this I_S table.
//!
//! ## Out-of-scope methods
//! None — schema preserved, semantics adapted.

use crate::rdb_i_s_cc__shared::{Column, ColumnType, Nullable, Row};
use slatedb::Error;

/// Column layout: (COLUMN_FAMILY_ID, TRANSACTION_ID, KEY, MODE).
/// Original: rdb_i_s.cc:1446 — `rdb_i_s_lock_info_fields_info[]`.
pub fn fields_info() -> &'static [Column] {
    static FIELDS: once_cell::sync::Lazy<Vec<Column>> = once_cell::sync::Lazy::new(|| {
        vec![
            Column { name: "COLUMN_FAMILY_ID", ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "TRANSACTION_ID",   ty: ColumnType::SLong,        nullable: Nullable::NotNull },
            Column { name: "KEY",              ty: ColumnType::Varchar(513), nullable: Nullable::NotNull },
            Column { name: "MODE",             ty: ColumnType::Varchar(32),  nullable: Nullable::NotNull },
        ]
    });
    &FIELDS
}

/// One held-lock entry as recorded by our per-Txn registry.
#[derive(Debug, Clone)]
pub struct HeldLock {
    pub cf_id: u32,
    /// We use the low 32 bits of the `DbTransaction.id()` UUID for the
    /// `TRANSACTION_ID` column (SLong-typed) — the full UUID is in `trx_info`.
    pub trx_id_short: u32,
    /// Key in hex form (mirrors C++ `rdb_hexdump`).
    pub key_hex: String,
    pub exclusive: bool,
}

pub struct FillCtx<'a> {
    pub held_locks: &'a [HeldLock],
}

/// Build the rowset for `information_schema.ROCKSDB_LOCKS`.
///
/// Original C++ source: rdb_i_s.cc:1454 — `rdb_i_s_lock_info_fill_table`.
pub async fn fill_table(_ctx: FillCtx<'_>) -> Result<Vec<Row>, Error> {
    // TODO(human): confirm the per-Txn registry's `HeldLock` shape after the
    // engine wrapper lands in `crate::engine::txn`. The `cf_id` decode helper
    // lives in `crate::codec::cf::cf_from_key_prefix`.
    todo!("map each HeldLock → 4-column Row, MODE = X|S")
}

pub const PLUGIN_NAME: &str = "ROCKSDB_LOCKS";

/// Original C++ source: rdb_i_s.cc:1506 — `rdb_i_s_lock_info_init`.
pub fn init(_plugin: *mut std::ffi::c_void) -> Result<(), Error> {
    todo!("wire fields_info + sync wrapper into the ST_SCHEMA_TABLE")
}
