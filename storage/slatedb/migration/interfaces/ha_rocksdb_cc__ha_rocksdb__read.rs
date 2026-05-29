//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__read`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (span 7955..9108, body ~422 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__read`
//!
//! ## Mapping
//! Point-lookup + range-positioning primitives shared by the index and scan
//! buckets. Maps to **`DbSnapshot::get` / `DbSnapshot::scan` plus key-codec
//! decoding** (_DESIGN.md §1 row "Snapshots" + "Iterators").
//!
//! Key collapses vs MyRocks:
//! - `rocksdb_smart_seek` (which inverts dir based on `Rdb_rev_comparator`)
//!   becomes a single `iter.seek(key)` because we encode reverse-indexes at
//!   write time (see `rdb_comparator_h::apply_direction`) — there's no
//!   per-CF reverse-comparator in SlateDB.
//! - `should_hide_ttl_rec` is collapsed into `Ok(false)` (SlateDB's iterator
//!   already filters expired entries — see the `ttl` bucket sibling stub).
//! - `get_for_update` maps to `DbTransaction::get` after `mark_read` and
//!   relies on `IsolationLevel::SerializableSnapshot` for conflict detection
//!   (_DESIGN.md §5 — `mark_read` is what makes SSI work).
//!
//! ## Out-of-scope methods
//! None — all 11 read methods are in scope. Read-Free Replication is gated
//! upstream of this bucket (in the `write_path` bucket via `use_read_free_rpl`
//! returning false).

use slatedb::Error;
use bytes::Bytes;

use crate::rdb_comparator_h::KeyDirection;

/// Forward-declared. Owned by A2's `ha_rocksdb_h` stub.
use crate::ha_rocksdb_h__ha_rocksdb::HaSlateDb;

/// Marker for the SQL-layer "find-flag" enum. We translate from MariaDB's
/// `enum ha_rkey_function` at the cxx bridge; the Rust core sees this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindFlag {
    KeyExact,
    BeforeKey,
    AfterKey,
    KeyOrNext,
    KeyOrPrev,
    Prefix,
    PrefixLast,
    PrefixLastOrPrev,
}

/// Outcome of an internal seek: did the cursor land on a row, or did we
/// hit end-of-index / no-match? Maps to MyRocks' HA_EXIT_SUCCESS / HA_ERR_KEY_NOT_FOUND.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekOutcome {
    Found,
    NotFound,
    Interrupted,
}

impl HaSlateDb {
    /// Seek the engine's scan iterator to the first key whose prefix equals
    /// `key_slice`. Returns `Found` if a matching row exists, `NotFound`
    /// otherwise.
    ///
    /// `_ttl_filter_ts` parameter is preserved in the signature so call sites
    /// match MyRocks 1:1, but it's **ignored** in our impl — SlateDB already
    /// filters expired rows via `RowEntry.expire_ts` (per _DESIGN.md §1 row
    /// "Compaction filters (TTL)").
    ///
    /// Original C++: ha_rocksdb.cc:7955.
    pub fn read_key_exact(
        &mut self,
        direction: KeyDirection,
        key_slice: &Bytes,
        _ttl_filter_ts: i64,
    ) -> Result<SeekOutcome, Error> {
        let _ = (direction, key_slice);
        todo!("m_scan_it.seek(key_slice); loop while valid && prefix_matches; return Found/NotFound; honor thd.killed via check_interrupted()")
    }

    /// Seek to the greatest key strictly less than `key_slice`. Used for
    /// HA_READ_BEFORE_KEY. With reverse-encoded indexes the caller already
    /// XORed the bytes; we use forward semantics here.
    ///
    /// Original C++: ha_rocksdb.cc:7992.
    pub fn read_before_key(
        &mut self,
        direction: KeyDirection,
        full_key_match: bool,
        key_slice: &Bytes,
        _ttl_filter_ts: i64,
    ) -> Result<SeekOutcome, Error> {
        let _ = (direction, full_key_match, key_slice);
        todo!("seek(key) then step prev; loop until !at-exact-match || !ttl-hidden (ttl is no-op here)")
    }

    /// Seek to the smallest key strictly greater than `key_slice` (or
    /// greater-or-equal for HA_READ_KEY_OR_NEXT — caller passes the right flag).
    /// Original C++: ha_rocksdb.cc:8028.
    pub fn read_after_key(
        &mut self,
        direction: KeyDirection,
        key_slice: &Bytes,
        _ttl_filter_ts: i64,
    ) -> Result<SeekOutcome, Error> {
        let _ = (direction, key_slice);
        todo!("iter.seek(key); native ScanOptions handles direction; no manual TTL skip")
    }

    /// Decode the row currently pointed-to by `m_scan_it` and write the
    /// unpacked record into the handler's row buffer. Used when the caller
    /// already positioned the iterator on a PK row.
    ///
    /// Three paths inside:
    ///   1. `m_lock_rows != NONE`: re-fetch via `get_for_update` (SSI lock acquisition).
    ///   2. `m_lock_rows == NONE`: unpack the iterator's value in place.
    ///
    /// Output: the handler's TableHandle owns the destination row buffer;
    /// we do NOT expose it here (per _DESIGN.md, no `TABLE*` in the Rust API).
    ///
    /// Original C++: ha_rocksdb.cc:8179.
    pub fn read_row_from_primary_key(&mut self) -> Result<(), Error> {
        todo!("if lock_rows != NONE -> get_row_by_rowid; else convert_record_from_storage_format")
    }

    /// Decode the row at the current SK position. If the index covers the
    /// query columns (`kd.can_cover_lookup()`), unpacks directly from the
    /// SK value bytes (the "covered-secondary-key" fast path). Otherwise,
    /// extracts the PK from the SK key tail and calls `get_row_by_rowid`.
    ///
    /// `move_forward` is the SQL-level scan direction; if the index is
    /// reverse-encoded, the caller should `move_forward = !move_forward`
    /// before reaching us (matches MyRocks' inversion at the call site).
    ///
    /// Original C++: ha_rocksdb.cc:8200.
    pub fn read_row_from_secondary_key(
        &mut self,
        sk_position: u32,
        move_forward: bool,
    ) -> Result<(), Error> {
        let _ = (sk_position, move_forward);
        todo!("decide covered-lookup vs full-row fetch; on covered, unpack record; else get_row_by_rowid")
    }

    /// Combined SK-seek + row-fetch. Used by `index_read_map_impl` and by
    /// ICP loops. Returns the row in the handler's row buffer.
    ///
    /// Original C++: ha_rocksdb.cc:8314.
    pub fn secondary_index_read(&mut self, keyno: u32) -> Result<SeekOutcome, Error> {
        let _ = keyno;
        todo!("check iter valid + covers_key; covered vs get_row_by_rowid; update_row_stats(ROWS_READ)")
    }

    /// Per-statement hook called by the optimizer before a unique-key
    /// HA_READ_KEY_EXACT lookup. We use it to install `range_key_part` and
    /// to clear end-range — same as MyRocks. Pure metadata stash, no I/O.
    ///
    /// Original C++: ha_rocksdb.cc:8432.
    pub fn prepare_index_scan(&mut self) -> Result<(), Error> {
        todo!("range_key_part = key_info[active_index].key_part; set_end_range(None)")
    }

    /// Per-statement hook called before a range scan. Sets `range_key_part`
    /// and stashes the end-range (used by `compare_key` during scan).
    /// Inputs (start_key, end_key) are passed as already-packed `Bytes`
    /// — the cxx bridge does the `key_range` translation. Per _DESIGN.md we
    /// do NOT expose `key_range` here.
    ///
    /// Original C++: ha_rocksdb.cc:8440.
    pub fn prepare_range_scan(
        &mut self,
        start_key: Option<&Bytes>,
        end_key: Option<&Bytes>,
    ) -> Result<(), Error> {
        let _ = (start_key, end_key);
        todo!("install range_key_part + stash end_key")
    }

    /// Inner loop for ICP (Index Condition Pushdown) — keep stepping
    /// the iterator until either the pushed-down predicate matches, or
    /// we exit the active index/prefix.
    ///
    /// Returns:
    ///   - `Ok(Found)`: row matches the ICP condition and is now decoded
    ///     into the handler buffer.
    ///   - `Ok(NotFound)`: walked out of range / index.
    ///   - `Err(...)`: I/O error or interrupted.
    ///
    /// The `rocksdb_skip_expired_records` call from MyRocks collapses to a
    /// no-op here (TTL filtering is native — see the `ttl` bucket).
    ///
    /// Original C++: ha_rocksdb.cc:8702.
    pub fn find_icp_matching_index_rec(
        &mut self,
        move_forward: bool,
    ) -> Result<SeekOutcome, Error> {
        let _ = move_forward;
        todo!("loop: check_interrupted; covers_key; unpack; handler_index_cond_check; step; return per ICP verdict")
    }

    /// Lock-and-fetch a row by `key`. Maps to:
    ///   - `IsolationLevel::Snapshot`: `txn.get(key)` (no lock acquired)
    ///   - `IsolationLevel::SerializableSnapshot`: `txn.mark_read(&[key]); txn.get(key)`
    ///     (records the read for SSI conflict detection at commit).
    ///
    /// In MyRocks this calls `RocksDB::GetForUpdate` with optional `do_validate`.
    /// Our `do_validate` analogue is the isolation level itself: SSI does it,
    /// SI doesn't.
    ///
    /// Errors:
    ///   - `slatedb::ErrorKind::Transaction` if SSI detects a concurrent write
    ///     to a read-marked key (mapped → `HA_ERR_LOCK_DEADLOCK` by the shim).
    ///
    /// Original C++: ha_rocksdb.cc:8978.
    pub async fn get_for_update(&mut self, key: &Bytes) -> Result<Option<Bytes>, Error> {
        let _ = key;
        todo!("if SSI -> txn.mark_read(&[key.clone()]); txn.get(key)")
    }

    /// Point-lookup of a full row by its packed PK bytes (the "rowid").
    /// Honors `skip_lookup` (blind-delete optimization — returns Ok without
    /// touching SlateDB) and `skip_ttl_check` (caller already filtered, or
    /// we trust SlateDB's native expire-ts handling — usually true).
    ///
    /// Lock semantics chosen by `m_lock_rows`:
    ///   - NONE       → `DbSnapshot::get`
    ///   - READ/WRITE → `get_for_update` (see above)
    ///
    /// Errors map per _DESIGN.md §4.
    ///
    /// Original C++: ha_rocksdb.cc:9016.
    pub async fn get_row_by_rowid(
        &mut self,
        rowid: &Bytes,
        skip_lookup: bool,
        skip_ttl_check: bool,
    ) -> Result<bool, Error> {
        let _ = (rowid, skip_lookup, skip_ttl_check);
        todo!("skip_lookup early-return; pick get vs get_for_update by lock_rows; decode value; set m_last_rowkey")
    }
}
