//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__scan`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (body LoC 305, span 8058..11200)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__scan`
//!
//! ## Mapping
//! Handler-vtable full-table-scan + rowid lookup bucket: `rnd_init`,
//! `rnd_next`, `rnd_next_with_direction`, `rnd_end`, `position`, `rnd_pos`,
//! plus the cross-cutting `position_to_correct_key` helper that maps
//! `ha_rkey_function` (KEY_EXACT, BEFORE_KEY, AFTER_KEY, PREFIX_LAST, ...) to
//! iterator-seek primitives.
//!
//! Per _DESIGN.md §1 (iterators row): all of these are `Map (native)` against
//! SlateDB's `DbReadOps::scan(range)` / `scan_with_options(range, opts)`:
//!
//! - `rnd_init(scan=true)` builds a `DbIterator` over the PK's full range
//!   (`[infimum(pk), supremum(pk)]`) using `Db::scan_prefix(pk_prefix)`. The
//!   `m_rnd_scan_is_new_snapshot` flag is set if the txn didn't yet hold one
//!   (used by `should_recreate_snapshot` retry).
//! - `rnd_next` calls `rnd_next_with_direction(buf, true)` and retries on
//!   invalidated-record signals from the SSI conflict checker.
//! - `rnd_next_with_direction` pumps the `DbIterator::next() ->
//!   Option<KeyValue>` and decodes each. If `lock_rows != None`, also issues
//!   `get_for_update` per row.
//! - `position` packs the PK from `table->record[0]` into the SQL-layer
//!   `ref` buffer (pure codec; no SlateDB call).
//! - `rnd_pos` does `get_row_by_rowid(buf, pos, len)` — point lookup via
//!   `Db::get(pk_key)` or `DbTransaction::get(pk_key)` if a txn is open.
//! - `position_to_correct_key` switches on `ha_rkey_function` and routes to
//!   `read_key_exact`/`read_before_key`/`read_after_key` (which live in the
//!   `read` sub-unit). Used by `read_range_first` and similar.
//!
//! Reverse scans use `IterationOrder::Descending` via
//! `ScanOptions::with_order` (see `rdb_comparator_h::iteration_order`).
//!
//! ## Out-of-scope methods
//! None. All seven methods translate to SlateDB scan/get primitives.

use bytes::Bytes;
use slatedb::Error;

use crate::ha_rocksdb_cc__ha_rocksdb__lifecycle::HaSlateDb;
use crate::ha_rocksdb_cc__ha_rocksdb__dml::RowBuf;

/// Forwarded to a future TABLE-shape unit. Mirrors `enum ha_rkey_function`
/// from `sql/handler.h`. Only the variants the handler acts on are listed.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaRkeyFunction {
    KeyExact,
    BeforeKey,
    AfterKey,
    KeyOrNext,
    KeyOrPrev,
    Prefix,
    PrefixLast,
    PrefixLastOrPrev,
}

/// Forwarded to a future TABLE-shape unit. Mirrors `key_part_map` — a
/// bitmap of which key parts are populated in a search tuple.
pub type KeyPartMap = u64;

/// Forwarded to a future TABLE-shape unit. A search tuple for `index_read_map`
/// / `position_to_correct_key`: the packed key bytes + which parts are set.
#[derive(Debug, Clone)]
pub struct SearchKey {
    pub key: Bytes,
    pub keypart_map: KeyPartMap,
}

/// Outcome of `position_to_correct_key`: whether subsequent iterator
/// advances should move forward (`Next`) or backward (`Prev`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IterDirection {
    Forward,
    Backward,
}

impl HaSlateDb {
    /// `int ha_rocksdb::rnd_init(bool scan)` — original C++ source line
    /// 10593.
    ///
    /// Inputs: `scan` — `true` for full-table sequential scan, `false` for
    /// `rnd_pos`-only usage (no iterator needed).
    /// Outputs: `Ok(())` on success.
    /// Errors:
    /// - `ErrorKind::Invalid` if `ha_thd().killed` is set
    ///   (mapped to `HA_ERR_QUERY_INTERRUPTED`).
    /// - `ErrorKind::Unavailable` if `Db::scan` fails to allocate an iterator
    ///   (object-store I/O for warmup).
    /// Invariants:
    /// - If `scan == true`, must build the iterator NOW (not lazily) so that
    ///   `rnd_next` can pump it without a separate setup call.
    /// - If `lock_rows == None`, also call `tx.acquire_snapshot()` so reads
    ///   are consistent across the scan.
    pub async fn rnd_init(&mut self, scan: bool) -> Result<(), Error> {
        let _ = scan;
        todo!("get_or_create_tx; setup_field_decoders; if scan: setup_iterator_for_rnd_scan(); tx.acquire_snapshot if lock_rows==None")
    }

    /// `int ha_rocksdb::rnd_next(uchar *buf)` — original C++ source line
    /// 10627. Forward-direction next-row wrapper with SSI-conflict retry.
    ///
    /// Inputs: `buf` — destination row buffer in `table->record[0]` shape.
    /// Outputs: `Ok(())` on a successful decode of one row into `buf`.
    /// Errors:
    /// - `ErrorKind::Transaction` if a `get_for_update` finds the row was
    ///   deleted since the snapshot was taken (mapped to
    ///   `HA_ERR_KEY_NOT_FOUND`).
    /// - `ErrorKind::Invalid` (`HA_ERR_QUERY_INTERRUPTED`) if killed.
    /// - `ErrorKind::Internal` if the codec returns a length-mismatch.
    /// End-of-scan: returns `Ok(())` with an in-band sentinel? No — to mirror
    /// the C++ contract that uses `HA_ERR_END_OF_FILE`, we surface EOF as
    /// `ErrorKind::Invalid` with the well-known message `"end-of-file"` that
    /// the error mapper recognizes. (Alternatively: change return type to
    /// `Result<Option<()>, Error>`; see TODO below.)
    ///
    // TODO(human): _DESIGN.md doesn't pin the EOF-signaling shape. Options:
    // (a) `Result<Option<()>, Error>` with `Ok(None)` for EOF — cleanest in
    //     Rust but every caller (`rnd_pos`, `index_read_map`, etc.) needs to
    //     re-thread it.
    // (b) reserve a `slatedb::Error::invalid("end-of-file")` sentinel —
    //     wire-compatible with the existing `HA_ERR_END_OF_FILE` translation.
    // Lean (b) for stub uniformity; flag for the design reviewer.
    pub async fn rnd_next(&mut self, buf: &mut RowBuf) -> Result<(), Error> {
        let _ = buf;
        todo!("loop: rnd_next_with_direction(buf, true); on should_recreate_snapshot: release+rebuild iterator; map HA_ERR_KEY_NOT_FOUND -> HA_ERR_END_OF_FILE")
    }

    /// `int ha_rocksdb::rnd_next_with_direction(uchar *buf, bool move_forward)`
    /// — original C++ source line 10653.
    ///
    /// Inputs: `buf`, `move_forward`.
    /// Outputs: `Ok(())` on a row decoded into `buf`.
    /// Errors: see `rnd_next` (this is the inner loop).
    /// Invariants:
    /// - `self.scan_it` is valid on entry (otherwise returns the EOF sentinel
    ///   immediately).
    /// - When `m_skip_scan_it_next_call == true`, the iterator's current
    ///   position is consumed without advancing; flag is cleared.
    /// - When `lock_rows != None`, each row goes through `get_for_update`
    ///   (which may upgrade to a row lock); the value returned by
    ///   `get_for_update` overrides the iterator's value (it can be fresher
    ///   if `lock_rows != None` and no snapshot was held).
    /// - TTL-expired rows are skipped (`should_hide_ttl_rec`) — replaced in
    ///   our impl by SlateDB's native `expire_ts` filter (no per-row check
    ///   needed; iterator already drops them).
    pub async fn rnd_next_with_direction(
        &mut self,
        buf: &mut RowBuf,
        move_forward: bool,
    ) -> Result<(), Error> {
        let _ = (buf, move_forward);
        todo!("pump DbIterator::next/seek_prev; check pk_descr.covers_key; if lock_rows!=None: get_for_update; decode into buf")
    }

    /// `int ha_rocksdb::rnd_end()` — original C++ source line 10767.
    ///
    /// Inputs: none. Outputs: `Ok(())` always.
    /// Errors: none — releasing the iterator is infallible.
    /// Invariants: idempotent. After this call `self.scan_it == None` and
    /// any held snapshot reference is released (the snapshot itself outlives
    /// this call if the txn still holds it).
    pub fn rnd_end(&mut self) -> Result<(), Error> {
        todo!("release_scan_iterator()")
    }

    /// `void ha_rocksdb::position(const uchar *record)` — original C++ source
    /// line 11135.
    ///
    /// Inputs: `record` — the row to extract a PK from.
    /// Outputs: a fully-packed `Bytes` of `ref_length` bytes (the
    /// SQL-layer-visible row identifier). Zero-padded if the packed PK is
    /// shorter than `ref_length`.
    /// Errors: none — purely codec. If `has_hidden_pk()` and
    /// `read_hidden_pk_id_from_rowkey` would fail, that's a `debug_assert!`
    /// per upstream `DBUG_ASSERT(false)` — we panic in debug, return zeros in
    /// release (matches the upstream behavior bit-for-bit).
    /// Invariants: no SlateDB call. Pure `Rdb_key_def::pack_record`.
    pub fn position(&mut self, record: &RowBuf) -> Result<Bytes, Error> {
        let _ = record;
        todo!("pack_record(record, hidden_pk_id) into a Bytes of ref_length; zero-pad")
    }

    /// `int ha_rocksdb::rnd_pos(uchar *buf, uchar *pos)` — original C++
    /// source line 11175.
    ///
    /// Inputs: `buf` (destination), `pos` (the `Bytes` PK packed by
    /// `position`).
    /// Outputs: `Ok(())` if the row was fetched and decoded into `buf`.
    /// Errors:
    /// - `ErrorKind::Data` (mapped to `HA_ERR_ROCKSDB_CORRUPT_DATA`) if the
    ///   key-length probe returns `usize::MAX` (sentinel for "can't parse").
    /// - `ErrorKind::Invalid` (HA_ERR_KEY_NOT_FOUND) if the row is absent.
    /// - `ErrorKind::Unavailable` on object-store I/O failure.
    /// Invariants: this is a point lookup. Maps to:
    ///   `DbTransaction::get(pk_key).await` (or `Db::get` if no active txn).
    /// Bumps `OperationType::Read` on success.
    pub async fn rnd_pos(&mut self, buf: &mut RowBuf, pos: &Bytes) -> Result<(), Error> {
        let _ = (buf, pos);
        todo!("key_length probe; get_row_by_rowid(buf, pos, len).await; update_row_stats(Read)")
    }

    /// `int ha_rocksdb::position_to_correct_key(...)` — original C++ source
    /// line 8058. Routes an `ha_rkey_function` to the right read_*_key
    /// helper and sets `move_forward` for subsequent iterator advances.
    ///
    /// Inputs: `kd` (the active index's `RdbKeyDef` — forwarded by ref-id),
    /// `find_flag`, `full_key_match` (true if the search tuple uses every
    /// key part), `key` (search tuple bytes), `keypart_map`, `key_slice`
    /// (packed key bytes), `ttl_filter_ts` (epoch seconds; rows expiring
    /// before this are skipped).
    /// Outputs: `(IterDirection, ())` — direction the caller should pump.
    /// Errors:
    /// - `ErrorKind::Invalid` for unsupported `ha_rkey_function` variants
    ///   (`HA_READ_KEY_OR_PREV`, `HA_READ_PREFIX` in upstream).
    /// - `ErrorKind::Internal` if `read_*_key` returns an unrecognized error.
    /// Invariants: this is purely an iterator-positioning routine; it does
    /// not decode rows. After return, `self.scan_it` is at the row the
    /// caller should consider "current" (or returns
    /// `HA_ERR_KEY_NOT_FOUND` as `ErrorKind::Invalid` if no such row).
    pub async fn position_to_correct_key(
        &mut self,
        kd_index: u32,
        find_flag: HaRkeyFunction,
        full_key_match: bool,
        key: &SearchKey,
        key_slice: &Bytes,
        ttl_filter_ts: i64,
    ) -> Result<IterDirection, Error> {
        let _ = (kd_index, find_flag, full_key_match, key, key_slice, ttl_filter_ts);
        todo!("match find_flag: KeyExact -> read_key_exact; BeforeKey -> read_before_key + dir=Backward; AfterKey/KeyOrNext -> read_after_key; PrefixLast(_OrPrev) -> read_before_key + suffix check")
    }
}
