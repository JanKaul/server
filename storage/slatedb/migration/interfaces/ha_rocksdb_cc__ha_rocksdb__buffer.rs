//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__buffer`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (span 6557..9659, body ~154 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__buffer`
//!
//! ## Mapping
//! Per-handler scratch-buffer lifecycle. In MyRocks this is a long list of
//! `my_malloc` / `my_free` pairs (`m_pk_tuple`, `m_pk_packed_tuple`,
//! `m_sk_packed_tuple`, `m_sk_match_prefix_buf`, `m_sk_packed_tuple_old`,
//! `m_end_key_packed_tuple`, `m_pack_buffer`, `m_record_buffer`,
//! `m_scan_it_lower_bound`, `m_scan_it_upper_bound`, plus alter-time
//! `m_dup_sk_packed_tuple` / `m_dup_sk_packed_tuple_old`) keyed to
//! `max_packed_sk_len` computed from the per-`Rdb_key_def` `max_storage_fmt_length`.
//!
//! In Rust we replace the manual allocator dance with **owned `Vec<u8>`
//! buffers held in the `HaSlateDb` struct** (allocated lazily in
//! `alloc_key_buffers`, dropped automatically on `HaSlateDb` drop or by
//! `free_key_buffers` for explicit reset). There is no SlateDB interaction at
//! all — these are pure in-process buffers used by the codec.
//!
//! The MyRocks "skip_unique_check_tables" comma-separated whitelist parser is
//! also part of this bucket; it parses the sysvar into an in-memory set and
//! stashes it on the handlerton. Pure string work, no SlateDB.
//!
//! `set_last_rowkey` is the "remember the row I just touched" helper used by
//! the read/write paths to anchor subsequent SK deletes during DML. Stores
//! the packed PK into `m_last_rowkey: Bytes`. Pure memory.
//!
//! ## Out-of-scope methods
//! None. All four are pure in-process buffer / state management.

use bytes::Bytes;
use slatedb::Error;

/// Forward-declared. Owned by A2's `ha_rocksdb_h` stub.
use crate::ha_rocksdb_h__ha_rocksdb::HaSlateDb;

/// Forward-decl mirror of the per-table key-def array. The real type lives
/// in `codec::ddl`; we reference it through a handle.
pub struct TblDefRef;

impl HaSlateDb {
    /// Allocate the per-handler packed-key + record scratch buffers, sized
    /// against `max_packed_sk_len` (the max over all `Rdb_key_def` in the
    /// table). `alloc_alter_buffers` adds the two extra `m_dup_sk_*` buffers
    /// used during INPLACE ADD UNIQUE INDEX checks.
    ///
    /// Inputs: a `TblDefRef` providing access to the index descriptors so we
    /// can compute the sizing. We do NOT expose `TABLE*` here per _DESIGN.md.
    ///
    /// Output: on success, the handler's `m_pk_tuple`, `m_pk_packed_tuple`,
    /// `m_sk_packed_tuple`, `m_sk_match_prefix_buf`, `m_sk_packed_tuple_old`,
    /// `m_end_key_packed_tuple`, `m_pack_buffer`, `m_record_buffer`,
    /// `m_scan_it_lower_bound`, `m_scan_it_upper_bound` are all populated.
    ///
    /// Errors: returns `Error::invalid("buffer alloc failed".into())` if any
    /// allocation fails. This matches the C++ `HA_ERR_OUT_OF_MEM` (which is
    /// in turn translated to the same kind by `error::slatedb_error_to_ha`).
    ///
    /// Original C++: ha_rocksdb.cc:6557.
    pub fn alloc_key_buffers(
        &mut self,
        tbl_def: &TblDefRef,
        alloc_alter_buffers: bool,
    ) -> Result<(), Error> {
        let _ = (tbl_def, alloc_alter_buffers);
        todo!("compute max_packed_sk_len; allocate Vec<u8> buffers; install on self.*; clear on partial failure")
    }

    /// Free / clear all per-handler packed-key + record scratch buffers.
    /// Idempotent — safe to call repeatedly or on a never-allocated handler.
    /// In Rust this is just setting each `Vec<u8>` to `Vec::new()` (or
    /// `None` for optional ones); the actual `free()` happens via Drop.
    ///
    /// Original C++: ha_rocksdb.cc:6649.
    pub fn free_key_buffers(&mut self) {
        todo!("self.m_pk_tuple = Vec::new(); ... reset every owned scratch buffer")
    }

    /// Parse the comma-separated `slatedb_skip_unique_check_tables` sysvar
    /// value into the per-handler in-memory whitelist. Used by
    /// `skip_unique_check()` in the `write_path` bucket.
    ///
    /// Inputs: the raw sysvar string (the cxx bridge hands us a `&str`).
    /// Output: populates `self.m_skip_unique_check_tables: HashSet<String>`.
    /// Cannot fail — empty strings yield empty sets.
    ///
    /// Note: in MyRocks this was a handlerton-global, not per-handler.
    /// We preserve the per-handler accessor for symmetry but the underlying
    /// set lives on the engine singleton.
    ///
    /// Original C++: ha_rocksdb.cc:6689.
    pub fn set_skip_unique_check_tables(&mut self, whitelist: &str) {
        let _ = whitelist;
        todo!("split on ','; trim; insert into self.m_skip_unique_check_tables")
    }

    /// Remember the most-recently-read row's packed PK bytes in
    /// `m_last_rowkey`. This is the anchor `delete_row`, `update_row`, and
    /// `unlock_row` use to address the row on subsequent operations.
    ///
    /// Inputs: `old_data` is the MariaDB row buffer the SQL layer hands us
    /// **only on the read-free-replication path** (which we do NOT support —
    /// see _DESIGN.md §1). In normal flow the rowkey is set inline by the
    /// read methods (in the `read` bucket), and this function becomes a
    /// no-op. We keep it for source-shape symmetry; the body is empty.
    ///
    /// Original C++: ha_rocksdb.cc:9650 — most of the body is guarded by
    /// `#ifdef MARIAROCKS_NOT_YET` and unreachable in mainline MariaDB.
    pub fn set_last_rowkey(&mut self, old_data: Option<&Bytes>) {
        // Read-free replication is a non-goal (_DESIGN.md §1); the C++
        // `use_read_free_rpl()` predicate is always false in MariaDB MyRocks
        // and would be false for us too.  This call therefore reduces to a
        // no-op; documented here so the cxx bridge has a stable target.
        let _ = old_data;
    }
}
