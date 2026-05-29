//! Interface stub for `ha_rocksdb_h__update_row_info`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.h` (lines 674..687, 14 LoC body)
//! v4 manifest sub-unit: `ha_rocksdb_h__update_row_info`
//! parent: `ha_rocksdb_h`
//!
//! ## Mapping
//! Scratch struct passed across the `update_write_*` family of handler
//! methods. Bundles the transaction handle, old/new row buffers, PK slices,
//! and the unpack-info writer for the new PK.
//!
//! Per _DESIGN.md §5, the C++ `Rdb_transaction*` is replaced by a borrow of
//! the engine's `Txn` wrapper around `slatedb::DbTransaction`. `rocksdb::Slice`
//! becomes `&[u8]` for borrowed and `Bytes` for owned views.
//!
//! ## Out-of-scope methods
//! None — pure data carrier.

use slatedb::bytes::Bytes;

use crate::rdb_buff_h::StringWriter;

/// Scratch state threaded through the UPDATE pipeline. Constructed in
/// `update_row` (handler vtable, see `ha_rocksdb_cc__ha_rocksdb__dml.rs`)
/// and consumed by the `update_write_pk` / `update_write_sk` /
/// `update_write_indexes` helpers (see `ha_rocksdb_cc__ha_rocksdb__write_path.rs`).
///
/// Original: ha_rocksdb.h:674 — `struct update_row_info`.
pub struct UpdateRowInfo<'a> {
    /// Borrowed engine txn (a wrapper around `slatedb::DbTransaction`).
    /// Lifetime tied to the statement; never null while UPDATE is in flight.
    pub tx: &'a crate::ha_rocksdb_cc__Rdb_transaction::Txn,

    /// New row in MySQL-wire format. Borrowed from the MariaDB caller's
    /// `uchar*` row buffer for the duration of the UPDATE.
    pub new_data: &'a [u8],

    /// Old row in MySQL-wire format. Same lifetime as `new_data`.
    pub old_data: &'a [u8],

    /// New PK in StorageFormat (memcomparable). Borrowed from the per-handler
    /// scratch buffer (`m_pk_packed_tuple`).
    pub new_pk_slice: &'a [u8],

    /// Old PK in StorageFormat — for detecting PK changes (which require
    /// a delete+insert sequence rather than an in-place update).
    pub old_pk_slice: &'a [u8],

    /// Old PK's stored value (the row's previous value bytes). Needed to
    /// detect TTL changes / SK consistency.
    pub old_pk_rec: Bytes,

    /// Mutable unpack-info writer for the new PK value. The caller pre-allocates
    /// this on the stack/handler scratch and passes it down; helpers append
    /// to it as they encode.
    pub new_pk_unpack_info: &'a mut StringWriter,

    /// Hidden-PK rowid for tables without an explicit PK; 0 otherwise.
    /// Originals stored this as longlong; we use i64.
    pub hidden_pk_id: i64,

    /// When true, the caller has determined that no unique-check pre-read is
    /// needed (e.g., bulk-load mode). Saves a roundtrip on the hot path.
    pub skip_unique_check: bool,
}
