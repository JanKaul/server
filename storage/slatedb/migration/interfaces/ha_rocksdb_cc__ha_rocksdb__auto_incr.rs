//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__auto_incr`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (span 6099..12418, body ~390 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__auto_incr`
//!
//! ## Mapping
//! Auto-increment and hidden-PK rowid management for `ha_rocksdb`. In MyRocks
//! these methods talk to `Rdb_dict_manager` (for the persisted auto-incr value)
//! and to `Rdb_transaction::set_auto_incr` (for the per-txn reservation).
//!
//! In our SlateDB world:
//! - The persisted auto-incr value lives in the **system CF**
//!   (`SYSTEM_CF_ID = u32::MAX` per `rdb_global_h.rs`). The dict-manager
//!   stub will offer `get_auto_incr_val` / `set_auto_incr_val` keyed by
//!   `GlIndexId` and we layer on top of those.
//! - Hidden-PK rowids are likewise persisted in the system CF, but a single
//!   in-memory `AtomicU64` per `HaSlateDb` instance is the hot path
//!   (`update_hidden_pk_val` is `fetch_add(1)`).
//! - `load_auto_incr_value_from_index` is the upgrade-time fallback: scan the
//!   PK index in **descending order** (`IterationOrder::Descending`) and read
//!   the first key. _DESIGN.md §1 row "Iterators".
//!
//! ## Out-of-scope methods
//! None — auto-incr is a fully-supported feature in our target.

use slatedb::Error;

use crate::rdb_global_h::GlIndexId;

use crate::ha_rocksdb_h__ha_rocksdb::HaSlateDb;
use crate::ha_rocksdb_h__update_row_info::UpdateRowInfo;

impl HaSlateDb {
    /// Load the persisted auto-increment value for this handler's table into
    /// the in-memory `m_tbl_def->m_auto_incr_val`. Falls back to a
    /// descending PK scan (`load_auto_incr_value_from_index`) if the
    /// dict-manager has no entry — this is the upgrade path from a server
    /// that did not persist the value, or the brand-new-table path.
    ///
    /// Errors: only the inner scan can fail; dict-manager misses are treated
    /// as "no entry" (Ok with auto_incr=0 → falls through to scan).
    ///
    /// Original C++: ha_rocksdb.cc:6099 — `void ha_rocksdb::load_auto_incr_value()`.
    pub async fn load_auto_incr_value(&mut self) -> Result<(), Error> {
        todo!("dict_manager.get_auto_incr_val; if empty -> load_auto_incr_value_from_index; update_auto_incr_val")
    }

    /// Last-resort: open a snapshot-pinned descending iterator on the
    /// auto-incr PK index, read the first key, decode the int column, and
    /// return `last_val + 1` (capped at `max_val` for the column type).
    ///
    /// In MyRocks this calls `index_last`; for us, `Db::scan_with_options
    /// (range, ScanOptions::new().with_order(IterationOrder::Descending))`
    /// then `iter.next().map(|kv| kv.key)`.
    ///
    /// Returns 0 if the table is empty.
    ///
    /// Original C++: ha_rocksdb.cc:6132.
    pub async fn load_auto_incr_value_from_index(&mut self) -> Result<u64, Error> {
        todo!("snapshot scan desc on PK index; decode rightmost int-col value")
    }

    /// CAS-bump the in-memory `m_tbl_def->m_auto_incr_val` to `val` if `val`
    /// is greater. No-op on a smaller `val`. Pure memory operation; no I/O.
    ///
    /// Original C++: ha_rocksdb.cc:6191.
    pub fn update_auto_incr_val(&mut self, val: u64) {
        todo!("AtomicU64 compare_exchange_weak loop matching the C++ semantics")
    }

    /// Read the auto-incr column from the in-progress row (`new_data` buffer
    /// owned by the handler) and CAS-bump the in-memory counter to
    /// `field_value + 1`. Also calls `txn.set_auto_incr(gl_index_id, new_val)`
    /// so the per-txn write-batch will persist it on commit.
    ///
    /// The actual column extraction happens via the codec (not exposed here —
    /// the `Field` pointer is hidden per _DESIGN.md constraints; the handler
    /// resolves it internally from its `TableHandle` member).
    ///
    /// Original C++: ha_rocksdb.cc:6201.
    pub fn update_auto_incr_val_from_field(&mut self) -> Result<(), Error> {
        todo!("decode auto-incr column from current row, CAS-bump, txn.set_auto_incr")
    }

    /// Load the persisted hidden-PK high-water-mark into
    /// `m_tbl_def->m_hidden_pk_val`. Same shape as `load_auto_incr_value` but
    /// targets the hidden-PK column (8-byte big-endian, see
    /// `SIZEOF_HIDDEN_PK_COLUMN = 8` in `rdb_global_h`).
    ///
    /// Falls back to a descending PK scan and decodes the rowid from the key
    /// via `read_hidden_pk_id_from_rowkey`.
    ///
    /// Returns HA_EXIT_SUCCESS on success; mapped to `Ok(())` here.
    ///
    /// Original C++: ha_rocksdb.cc:6227.
    pub async fn load_hidden_pk_value(&mut self) -> Result<(), Error> {
        todo!("scan PK index desc; decode hidden_pk_id; CAS-bump m_hidden_pk_val")
    }

    /// `fetch_add(1)` on the in-memory hidden-PK counter. Returns the **old**
    /// value (the one to use for the new row's rowid). Caller is responsible
    /// for the matching `txn.put` into the system-CF auto-incr entry on
    /// commit (handled by `update_auto_incr_val_from_field`'s sibling write).
    ///
    /// Original C++: ha_rocksdb.cc:6270 — returns `longlong`; we return `i64`.
    pub fn update_hidden_pk_val(&mut self) -> i64 {
        todo!("AtomicI64::fetch_add(1, Ordering::Relaxed) on m_hidden_pk_val")
    }

    /// Decode the 8-byte hidden-PK from the engine's `m_last_rowkey` (set
    /// during the most-recent read). The rowkey layout is
    /// `index_id (4 BE bytes) || hidden_pk (8 BE bytes)`.
    ///
    /// Errors with `Error::data("hidden_pk: short rowkey".into())` if the
    /// rowkey is truncated (corresponds to `HA_ERR_ROCKSDB_CORRUPT_DATA`).
    ///
    /// Original C++: ha_rocksdb.cc:6277.
    pub fn read_hidden_pk_id_from_rowkey(&self) -> Result<i64, Error> {
        todo!("StringReader over m_last_rowkey; skip INDEX_NUMBER_SIZE bytes; read u64_be")
    }

    /// True iff this table's PK is a synthetic hidden PK (i.e. the user did
    /// not declare a PRIMARY KEY). Pure metadata check against the cached
    /// `TableShare`. Original C++: ha_rocksdb.cc:9487.
    pub fn has_hidden_pk(&self) -> bool {
        todo!("delegate to Rdb_key_def::table_has_hidden_pk equivalent in codec::key")
    }

    /// True iff the given key-position `index` is the hidden-PK position.
    /// Hidden PK is always the last entry in `m_tbl_def->m_key_descr_arr`.
    /// Original C++: ha_rocksdb.cc:9495.
    pub fn is_hidden_pk(&self, index: u32) -> bool {
        todo!("primary_key == MAX_INDEXES && index == m_key_count - 1")
    }

    /// Return the index-position of the primary key (the hidden-PK slot if
    /// the table has no explicit PK, otherwise the declared PK index).
    /// Original C++: ha_rocksdb.cc:9504.
    pub fn pk_index(&self) -> u32 {
        todo!("if has_hidden_pk { m_key_count - 1 } else { table.primary_key }")
    }

    /// True iff the given index-position is the table's primary key
    /// (declared or hidden). Original C++: ha_rocksdb.cc:9513.
    pub fn is_pk(&self, index: u32) -> bool {
        todo!("index == primary_key || self.is_hidden_pk(index)")
    }

    /// Build the new-row PK slice and stash it in the caller-provided
    /// `UpdateRowInfo`. For hidden-PK tables: allocates a new rowid via
    /// `update_hidden_pk_val()`. For explicit-PK tables: packs the PK
    /// columns of the in-progress new-row buffer.
    ///
    /// `UpdateRowInfo` is the Rust mirror of the C++ `struct update_row_info`
    /// declared in `ha_rocksdb.h` (provided by A2's stub). We do NOT expose
    /// `THD`, `TABLE`, or `Field` here per _DESIGN.md.
    ///
    /// Original C++: ha_rocksdb.cc:9670.
    pub fn get_pk_for_update(&mut self, row_info: &mut UpdateRowInfo) -> Result<(), Error> {
        todo!("hidden_pk path: write big-endian rowid into row_info.new_pk_buf; explicit-pk path: kd.pack_record")
    }

    /// MySQL's bulk auto-incr reservation hook. For our purposes (and
    /// matching MyRocks) we always reserve exactly one value at a time and
    /// report `nb_reserved_values = 1`. The atomic CAS loop matches the C++
    /// implementation precisely (ha_rocksdb.cc:12282-12420).
    ///
    /// Inputs:
    ///   - `off`: the SQL `AUTO_INCREMENT_OFFSET` for the session
    ///   - `inc`: the SQL `AUTO_INCREMENT_INCREMENT` for the session
    ///   - `nb_desired_values`: hint (we ignore it, matching MyRocks)
    ///
    /// Output: `(first_value, nb_reserved_values)`.
    ///
    /// Errors: none expected (pure CAS arithmetic). The CAS targets the
    /// in-memory counter; persisting happens at commit via the per-txn write.
    pub fn get_auto_increment(
        &mut self,
        off: u64,
        inc: u64,
        nb_desired_values: u64,
    ) -> Result<(u64, u64), Error> {
        let _ = nb_desired_values;
        let _ = (off, inc);
        todo!("port ha_rocksdb.cc:12282 CAS loop; returns (first_value, 1)")
    }
}

// `UpdateRowInfo` is canonically defined in `ha_rocksdb_h__update_row_info.rs`.
// Imported at the top of this file.
