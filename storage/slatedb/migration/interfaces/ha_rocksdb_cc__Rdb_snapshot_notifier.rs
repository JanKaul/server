//! Interface stub for `ha_rocksdb_cc__Rdb_snapshot_notifier`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 2260..2275 + 3721..3726, ~21 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__Rdb_snapshot_notifier`
//!
//! ## Mapping
//! In MyRocks, `Rdb_snapshot_notifier : public rocksdb::TransactionNotifier`
//! is the callback object passed to `rocksdb::Transaction::SetSnapshotOnNextOperation`.
//! RocksDB invokes `SnapshotCreated(snapshot)` async-ly when the txn lazily
//! takes its first snapshot; the notifier forwards it back to the owning
//! `Rdb_transaction` to populate `m_read_opts.snapshot`.
//!
//! Per _DESIGN.md §0 (verified SlateDB API): SlateDB's `DbTransaction` does
//! **not** have a "set snapshot on next operation" mechanism. The delayed
//! snapshot pattern from MyRocks is modelled in our Rust port directly inside
//! `RdbTransactionImpl::snapshot: SnapshotState::{None, Pending, Held(Arc<DbSnapshot>)}`
//! (see `ha_rocksdb_cc__Rdb_transaction_impl.rs`). When `acquire_snapshot(true)`
//! is called or the first read happens, we call `Db::snapshot()` synchronously
//! and store the `Arc<DbSnapshot>` — no callback needed.
//!
//! Therefore this whole class **collapses to zero** in the Rust port. We keep
//! the unit ID for traceability (every C++ class gets a stub) but the file
//! exists only to document the elimination.
//!
//! See also `event_listener_h.rs` for the analogous collapse of MyRocks'
//! callback-based `Rdb_event_listener` into a SlateDB watch-channel pattern.
//!
//! ## Out-of-scope methods
//! - `SnapshotCreated(const rocksdb::Snapshot*)` — no async callback exists
//!   in SlateDB; replaced by inline acquisition in `RdbTransactionImpl::acquire_snapshot`.
//! - `detach()` — pointer-safety hook for the C++ `shared_ptr<Rdb_snapshot_notifier>`
//!   outliving its owning txn. Not needed: Rust ownership rules make the
//!   `Arc<DbSnapshot>` self-contained.

// Nothing is exported from this module. The file is intentionally near-empty
// — its presence in the interface tree is the artifact, per the contract that
// every C++ class in the manifest produces a Rust stub.
