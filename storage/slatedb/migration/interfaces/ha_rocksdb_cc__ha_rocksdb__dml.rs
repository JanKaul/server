//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__dml`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (body LoC 127, span 211..11267)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__dml`
//!
//! ## Mapping
//! Handler-vtable row-mutation bucket: `write_row`, `delete_row`, `update_row`,
//! plus the `update_row_stats` accounting helper. Each translates to
//! `put`/`delete` calls on a `slatedb::DbTransaction` (see _DESIGN.md §5),
//! followed by per-secondary-index writes through the same txn:
//!
//! - `write_row` (INSERT) → for the PK: `txn.put(key, value)`; for each SK:
//!   `txn.put(sk_key, sk_value)`. Unique-check is `txn.mark_read(sk_key)`
//!   before the put.
//! - `delete_row` (DELETE) → `txn.delete(pk_key)`; for each SK:
//!   `txn.delete(sk_key)`.
//! - `update_row` (UPDATE) → delegates to `update_write_row` (in the
//!   `write_path` sub-unit) which performs read-modify-write per index.
//!
//! Per _DESIGN.md §1 (DML row): all of these are `Map (native)` — DbTransaction
//! `put`/`delete` are synchronous and buffered into the txn's write batch; the
//! actual SlateDB I/O happens at `commit().await` time (driven by the
//! `external_lock` unlock branch — see `lifecycle.rs`).
//!
//! `update_row_stats` is pure in-memory counter bookkeeping — declared (and
//! todo-bodied) in `lifecycle.rs` as `pub(crate)`; this file re-exports the
//! contract via a comment to keep the bucket complete.
//!
//! ## Out-of-scope methods
//! None — none of these methods hit a §1 non-goal. (RFR — Read-Free
//! Replication — would intersect `update_row`/`delete_row` if enabled; it's
//! disabled at table_flags time per _DESIGN.md §1, so DML never hits the
//! `use_read_free_rpl()` branches.)

use bytes::Bytes;
use slatedb::Error;

use crate::ha_rocksdb_h__ha_rocksdb::HaSlateDb;
use crate::rdb_global_h::OperationType;

/// Forwarded to a future TABLE-shape unit. A MariaDB-format row buffer
/// (`table->record[0]` style). Carries `field_count` POD field values; the
/// codec packs/unpacks against it.
#[derive(Debug, Clone)]
pub struct RowBuf {
    pub bytes: Bytes,
}

impl HaSlateDb {
    /// `int ha_rocksdb::write_row(const uchar *buf)` — original C++ source
    /// line 9606.
    ///
    /// Inputs: `buf` — the new row in `table->record[0]` format.
    /// Outputs: `Ok(())` on a successful buffered write.
    /// Errors:
    /// - `ErrorKind::Transaction` if a unique-check mark_read collides with
    ///   another in-flight txn at SSI level.
    /// - `ErrorKind::Invalid` if the codec rejects the row (encoding mismatch,
    ///   `auto_increment` exhaustion).
    /// - `ErrorKind::Unavailable` if the txn's bounded I/O channel is full.
    /// Invariants on entry:
    /// - `self.lock_rows == RowLockMode::Write` (asserted upstream).
    /// - `buf == table->record[0]` (asserted upstream).
    /// On success increments `OperationType::Inserted`.
    ///
    /// Maps to: (1) `update_auto_increment` if `next_number_field` set;
    /// (2) `update_write_row(None, buf, skip_unique_check())`; (3)
    /// `update_row_stats(Inserted)`.
    pub async fn write_row(&mut self, buf: &RowBuf) -> Result<(), Error> {
        let _ = buf;
        todo!("if next_number_field: update_auto_increment; update_write_row(None, buf, skip_unique_check).await; update_row_stats(Inserted)")
    }

    /// `int ha_rocksdb::delete_row(const uchar *buf)` — original C++ source
    /// line 10866.
    ///
    /// Inputs: `buf` — the row being deleted (used to derive every SK key).
    /// Outputs: `Ok(())` on a buffered delete.
    /// Errors:
    /// - `ErrorKind::Transaction` on a PK conflict if `can_use_single_delete`
    ///   takes the `single_delete` path (treated as conflict-able).
    /// - `ErrorKind::Invalid` if `read_hidden_pk_id_from_rowkey` fails for a
    ///   table with hidden PK + secondary indexes.
    /// - `ErrorKind::Unavailable` on bounded-channel saturation.
    /// Invariants:
    /// - `self.last_rowkey` must be the PK of this row (set by the scan that
    ///   located it, or computed by `set_last_rowkey` for RBR).
    /// - Per-SK deletes use `delete` (NOT `single_delete`) because secondary
    ///   indexes can have multiple writers per key.
    /// On success increments `OperationType::Deleted`.
    pub async fn delete_row(&mut self, buf: &RowBuf) -> Result<(), Error> {
        let _ = buf;
        todo!("set_last_rowkey(buf); for PK: delete_or_singledelete; for each SK: txn.delete(sk_key); update_row_stats(Deleted)")
    }

    /// `int ha_rocksdb::update_row(const uchar *old_data, const uchar *new_data)`
    /// — original C++ source line 11243.
    ///
    /// Inputs: `old_data` (record as it was just read), `new_data` (the new
    /// record in `table->record[0]`).
    /// Outputs: `Ok(())` on a buffered RMW.
    /// Errors: same set as `write_row` + `delete_row`. If PK changed, both
    /// the delete and insert paths must succeed atomically (within one txn);
    /// commit failure surfaces at `external_lock(F_UNLCK)` time.
    /// Invariants on entry:
    /// - `self.lock_rows == RowLockMode::Write`.
    /// - `new_data == table->record[0]`.
    /// - `old_data` was set by a prior `rnd_pos`/`index_read_*` on this
    ///   handler instance.
    /// On success increments `OperationType::Updated`.
    ///
    /// Per _DESIGN.md §5: a PK-change update writes a tombstone for the old
    /// PK key and a put for the new — both buffered into the same
    /// `DbTransaction`. The codec decides whether each SK row need a
    /// delete+put (when the SK changed) or can skip (when it didn't).
    pub async fn update_row(
        &mut self,
        old_data: &RowBuf,
        new_data: &RowBuf,
    ) -> Result<(), Error> {
        let _ = (old_data, new_data);
        todo!("update_write_row(Some(old), new, skip_unique_check).await; update_row_stats(Updated)")
    }

    /// `void ha_rocksdb::update_row_stats(const operation_type &type)` —
    /// original C++ source line 211. Pure in-memory counter bump; per the v4
    /// manifest this lives in the `dml` bucket but the canonical declaration
    /// is in `lifecycle.rs` as `pub(crate) fn update_row_stats`. Re-stated
    /// here as documentation so the bucket's method list is complete.
    ///
    /// Inputs: `ty` — the operation that just succeeded.
    /// Outputs: `()` (no error path).
    /// Errors: none — the underlying `AtomicStatU64` is lock-free.
    /// Invariants: must be called exactly once per successful DML row.
    ///
    /// **Implemented in:** `ha_rocksdb_cc__ha_rocksdb__lifecycle::HaSlateDb::update_row_stats`.
    pub fn dml_update_row_stats(&self, ty: OperationType) {
        self.update_row_stats(ty)
    }
}
