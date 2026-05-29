//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__index`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (body LoC 533, span 8134..10826)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__index`
//!
//! ## Mapping
//! Handler-vtable index-scan bucket. MariaDB's index-driven row access shape
//! (`index_init` → `index_read_map`/`index_first`/`index_last` →
//! `index_next`/`index_prev` → `index_end`) maps to SlateDB's
//! `DbReadOps::scan_prefix(index_prefix)` plus `IterationOrder`:
//!
//! - `index_init(idx, sorted)` → store `active_index`, get-or-create a
//!   per-statement `DbTransaction`, optionally acquire a snapshot.
//! - `index_read_map(buf, key, keypart_map, find_flag)` → delegates to
//!   `index_read_map_impl`. The optimized "PK + KEY_EXACT + full key" case
//!   is a single `DbTransaction::get(pk_key).await`. The general case opens
//!   a `DbIterator` over `scan_prefix(index_prefix)` and seeks via
//!   `position_to_correct_key`.
//! - `read_range_first(start, end, eq_range, sorted)` → wraps
//!   `index_read_map_impl` with the SQL-layer `end_range` honored.
//! - `index_next` / `index_prev` → `index_next_with_direction(buf,
//!   !is_reverse_cf)` — the reverse-CF flag (`KeyDirection::Reverse` in
//!   `rdb_comparator_h.rs`) flips the physical direction so semantic order
//!   matches user expectations.
//! - `index_first` / `index_last` → delegate to `index_first_intern` /
//!   `index_last_intern` (here as private methods); these seek to
//!   `kd.get_first_key()` / `get_last_key()` and pump one row.
//! - `index_end` → release iterator + bitmaps; clear `active_index`.
//! - `calc_eq_cond_len` → codec helper for the prefix-bloom-filter decision
//!   (used by `check_bloom_and_set_bounds` in the `repair` bucket).
//!
//! Per _DESIGN.md §1 (iterators row): all native. Per §2: the key shape
//! `varint(cf_id) || u32_be(index_id) || memcmp_key` lets us use
//! `Db::scan_prefix(index_prefix)` directly — the `PrefixExtractor` ensures
//! SST-level bloom filters reject non-matching SSTs.
//!
//! ## Out-of-scope methods
//! None. All thirteen are scan/get operations supported natively by SlateDB.

use bytes::Bytes;
use slatedb::Error;

use crate::ha_rocksdb_cc__ha_rocksdb__dml::RowBuf;
use crate::ha_rocksdb_cc__ha_rocksdb__lifecycle::HaSlateDb;
use crate::ha_rocksdb_cc__ha_rocksdb__scan::{HaRkeyFunction, SearchKey};

/// Forwarded to a future TABLE-shape unit. Mirrors `key_range` from
/// `sql/handler.h`: `key` bytes + `keypart_map` + `flag`.
#[derive(Debug, Clone)]
pub struct KeyRange {
    pub key: Bytes,
    pub keypart_map: u64,
    pub flag: HaRkeyFunction,
}

impl HaSlateDb {
    /// `int ha_rocksdb::index_init(uint idx, bool sorted)` — original C++
    /// source line 10780.
    ///
    /// Inputs: `idx` (key index in `table_share->key_info[]`),
    /// `sorted` (caller wants sorted order; we always provide it).
    /// Outputs: `Ok(())` on success.
    /// Errors:
    /// - `ErrorKind::Invalid` if `ha_thd().killed`.
    /// - `ErrorKind::Unavailable` if the txn-registry channel is full.
    /// Invariants: sets `active_index = idx`. Acquires snapshot eagerly only
    /// if `lock_rows == None` (otherwise `get_for_update` will pin one).
    pub async fn index_init(&mut self, idx: u32, sorted: bool) -> Result<(), Error> {
        let _ = (idx, sorted);
        todo!("get_or_create_tx; setup_field_decoders; get_lookup_bitmap if !keyread_only; tx.acquire_snapshot(lock_rows==None); active_index=idx")
    }

    /// `int ha_rocksdb::index_end()` — original C++ source line 10814.
    ///
    /// Inputs: none. Outputs: `Ok(())` always.
    /// Errors: none.
    /// Invariants: idempotent. Releases the iterator + ICP lookup bitmap.
    /// `active_index` set to a sentinel (`u32::MAX`, mirroring `MAX_KEY`).
    pub fn index_end(&mut self) -> Result<(), Error> {
        todo!("release_scan_iterator(); free lookup_bitmap; active_index = MAX_KEY")
    }

    /// `int ha_rocksdb::index_read_map(uchar *buf, const uchar *key,
    /// key_part_map keypart_map, enum ha_rkey_function find_flag)` —
    /// original C++ source line 8464.
    ///
    /// Trivial pass-through to `index_read_map_impl(buf, key, keypart_map,
    /// find_flag, None)`. Kept separate to match the SQL-layer vtable shape.
    pub async fn index_read_map(
        &mut self,
        buf: &mut RowBuf,
        key: &SearchKey,
        find_flag: HaRkeyFunction,
    ) -> Result<(), Error> {
        self.index_read_map_impl(buf, key, find_flag, None).await
    }

    /// `int ha_rocksdb::index_read_map_impl(uchar *buf, const uchar *key,
    /// key_part_map keypart_map, enum ha_rkey_function find_flag,
    /// const key_range *end_key)` — original C++ source line 8489.
    ///
    /// Inputs: `buf`, `key`, `find_flag`, optional `end_key` (range upper
    /// bound for prefix-bloom-filter sizing).
    /// Outputs: `Ok(())` if a matching row was decoded into `buf`.
    /// Errors:
    /// - `ErrorKind::Invalid` (HA_ERR_KEY_NOT_FOUND mapped via the EOF
    ///   sentinel — see scan.rs TODO) when no row matches.
    /// - `ErrorKind::Invalid` (HA_ERR_QUERY_INTERRUPTED) on `thd->killed`.
    /// - `ErrorKind::Transaction` on SSI conflict at `get_for_update`.
    /// Invariants:
    /// - Fast path: `active_index == PK && find_flag == KeyExact &&
    ///   full_key_match` ⇒ single `txn.get(pk_full_tuple).await`. No
    ///   iterator.
    /// - General path: opens a `DbIterator` via `scan_prefix(index_prefix)`
    ///   with bloom-filter pre-check (`check_bloom_and_set_bounds`); seeks
    ///   via `position_to_correct_key`.
    pub async fn index_read_map_impl(
        &mut self,
        buf: &mut RowBuf,
        key: &SearchKey,
        find_flag: HaRkeyFunction,
        end_key: Option<&KeyRange>,
    ) -> Result<(), Error> {
        let _ = (buf, key, find_flag, end_key);
        todo!("fast PK path: txn.get(pk_full_key).await; else: setup_scan_iterator + position_to_correct_key + decode")
    }

    /// `int ha_rocksdb::read_range_first(const key_range *start_key,
    /// const key_range *end_key, bool eq_range, bool sorted)` — original C++
    /// source line 8385.
    ///
    /// Inputs: `start_key` (optional), `end_key` (optional), `eq_range`,
    /// `sorted`. Outputs: `Ok(())` on first row decoded into the handler's
    /// row buffer.
    /// Errors: same set as `index_read_map_impl` + EOF if the row is past
    /// `end_range`.
    /// Invariants: `set_end_range(end_key)` first; on `start_key == None`
    /// delegate to `index_first`. After fetching, if `compare_key(end_range)
    /// > 0`, call `unlock_row()` (release the just-acquired row lock if it
    /// fell outside the range) and return EOF.
    pub async fn read_range_first(
        &mut self,
        start_key: Option<&KeyRange>,
        end_key: Option<&KeyRange>,
        eq_range: bool,
        sorted: bool,
    ) -> Result<(), Error> {
        let _ = (start_key, end_key, eq_range, sorted);
        todo!("set_end_range(end_key); if !start_key: index_first; else: index_read_map_impl; if past end_range: unlock_row + EOF")
    }

    /// `int ha_rocksdb::index_next(uchar *buf)` — original C++ source line
    /// 9115.
    ///
    /// Forward semantic next: physical direction depends on whether the
    /// active index is reverse-encoded (`KeyDirection::Reverse`). Wrapper
    /// around `index_next_with_direction(buf, moves_forward)`.
    pub async fn index_next(&mut self, buf: &mut RowBuf) -> Result<(), Error> {
        let _ = buf;
        todo!("moves_forward = !is_reverse_cf(active_index); index_next_with_direction(buf, moves_forward).await; map KEY_NOT_FOUND -> EOF")
    }

    /// `int ha_rocksdb::index_prev(uchar *buf)` — original C++ source line
    /// 9134. Mirror of `index_next`.
    pub async fn index_prev(&mut self, buf: &mut RowBuf) -> Result<(), Error> {
        let _ = buf;
        todo!("moves_forward = is_reverse_cf(active_index); index_next_with_direction(buf, moves_forward).await; map KEY_NOT_FOUND -> EOF")
    }

    /// `int ha_rocksdb::index_next_with_direction(uchar *buf, bool move_forward)`
    /// — original C++ source line 9148.
    ///
    /// Inputs: `buf`, `move_forward`.
    /// Outputs: `Ok(())` on a row.
    /// Errors: see `rnd_next_with_direction` (for PK active_index) or the
    /// SK-iterator-path errors (TTL, ICP misses).
    /// Invariants:
    /// - When `active_index == PK`, delegates to `rnd_next_with_direction`.
    /// - When active_index is a secondary index, pumps `m_scan_it` and runs
    ///   `find_icp_matching_index_rec` + `secondary_index_read` per row.
    /// - `rocksdb_skip_expired_records` is replaced by SlateDB's native
    ///   `expire_ts` filtering — the iterator never surfaces expired rows.
    pub async fn index_next_with_direction(
        &mut self,
        buf: &mut RowBuf,
        move_forward: bool,
    ) -> Result<(), Error> {
        let _ = (buf, move_forward);
        todo!("if active_index==PK: rnd_next_with_direction; else: pump m_scan_it + find_icp_matching_index_rec + secondary_index_read")
    }

    /// `int ha_rocksdb::index_first(uchar *buf)` — original C++ source line
    /// 9193.
    ///
    /// Inputs: `buf`. Outputs: `Ok(())` on the first row of the active
    /// index. EOF if the index is empty.
    /// Errors: see `index_next_with_direction`.
    /// Invariants: chooses `index_first_intern` vs `index_last_intern` based
    /// on `is_reverse_cf(active_index)` — reverse-encoded indexes start
    /// from the "physical last".
    pub async fn index_first(&mut self, buf: &mut RowBuf) -> Result<(), Error> {
        let _ = buf;
        todo!("m_sk_match_prefix=None; if is_reverse_cf: index_last_intern; else: index_first_intern; map KEY_NOT_FOUND -> EOF")
    }

    /// `int ha_rocksdb::index_last(uchar *buf)` — original C++ source line
    /// 9210. Mirror of `index_first`.
    pub async fn index_last(&mut self, buf: &mut RowBuf) -> Result<(), Error> {
        let _ = buf;
        todo!("m_sk_match_prefix=None; if is_reverse_cf: index_first_intern; else: index_last_intern; map KEY_NOT_FOUND -> EOF")
    }

    /// `int ha_rocksdb::index_first_intern(uchar *buf)` (private) — original
    /// C++ source line 9253.
    ///
    /// Inputs: `buf`. Outputs: `Ok(())` on the lowest-key row of the active
    /// index.
    /// Errors: `Transaction` (on SSI retry); EOF if empty.
    /// Invariants: builds `index_key = kd.get_first_key()`; opens iterator
    /// via `scan_prefix(...)` (forward); calls `Seek(index_key)` then
    /// `index_next_with_direction(buf, true)`. SSI-conflict retry loop
    /// mirrors `should_recreate_snapshot`.
    pub async fn index_first_intern(&mut self, buf: &mut RowBuf) -> Result<(), Error> {
        let _ = buf;
        todo!("get_first_key; setup_scan_iterator; m_scan_it.Seek(index_key); skip_next_call=true; index_next_with_direction(buf, true).await with retry")
    }

    /// `int ha_rocksdb::index_last_intern(uchar *buf)` (private) — original
    /// C++ source line 9345. Mirror of `index_first_intern` using
    /// `kd.get_last_key()` + `SeekForPrev`. For PK, falls through to
    /// `rnd_next_with_direction(buf, false)` for proper row decoding.
    pub async fn index_last_intern(&mut self, buf: &mut RowBuf) -> Result<(), Error> {
        let _ = buf;
        todo!("get_last_key; setup_scan_iterator; m_scan_it.SeekForPrev; if PK: rnd_next_with_direction(buf, false); else: find_icp + secondary_index_read; SSI retry")
    }

    /// `int ha_rocksdb::calc_eq_cond_len(const Rdb_key_def&,
    /// enum ha_rkey_function, const rocksdb::Slice&, int, const key_range*,
    /// uint*)` — original C++ source line 8134.
    ///
    /// Inputs: `kd_index`, `find_flag`, `slice` (packed start key),
    /// `bytes_changed_by_succ` (delta from `kd.successor(...)`), `end_key`
    /// (optional), `end_key_packed_size` (out param folded into the return
    /// tuple).
    /// Outputs: `(eq_cond_len: usize, end_key_packed_size: u32)` — the
    /// effective equal-condition length passed to the prefix-bloom check, and
    /// the packed length of the end-key tuple.
    /// Errors: none — purely arithmetic over the codec lengths.
    /// Invariants:
    /// - `KeyExact` ⇒ returns `slice.len()`.
    /// - `PrefixLast` ⇒ returns `slice.len() - bytes_changed_by_succ`.
    /// - With `end_key` ⇒ returns the longest common prefix between start
    ///   and end packed forms.
    /// Used solely by the bloom-filter-eligibility decision.
    pub fn calc_eq_cond_len(
        &self,
        kd_index: u32,
        find_flag: HaRkeyFunction,
        slice: &Bytes,
        bytes_changed_by_succ: i32,
        end_key: Option<&KeyRange>,
    ) -> Result<(usize, u32), Error> {
        let _ = (kd_index, find_flag, slice, bytes_changed_by_succ, end_key);
        todo!("match find_flag and end_key presence to compute eq_cond_len + end_key_packed_size")
    }
}
