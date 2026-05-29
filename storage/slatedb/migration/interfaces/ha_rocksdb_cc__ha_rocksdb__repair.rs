//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__repair`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (body LoC 519, span 7922..13878)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__repair`
//!
//! ## Mapping
//! Mixed bucket: integrity / maintenance handler entry points plus the inner
//! uniqueness-locking helpers used by `dml::write_row` / `dml::update_row`.
//! Despite the bucket name "repair" the methods split three ways:
//!
//! 1. **Uniqueness probes** (`check_and_lock_unique_pk`,
//!    `check_and_lock_sk`, `check_uniqueness_and_lock`, `check_duplicate_sk`)
//!    — read the candidate key + take a row-lock to prevent a concurrent
//!    insert from inserting a colliding row before our commit. Per _DESIGN.md
//!    §5: `txn.get(key)` issues the probe; `txn.mark_read(key)` records the
//!    read in the SSI conflict tracker. The "get_for_update" semantic of
//!    MyRocks maps to `IsolationLevel::SerializableSnapshot` + `mark_read`
//!    (write-read conflict detection at commit) — we do NOT take an explicit
//!    row lock because SlateDB doesn't expose one; we rely on SSI to retry on
//!    conflict.
//! 2. **DDL-time capability checks** (`check_keyread_allowed`,
//!    `check_if_incompatible_data`) — pure metadata inspection; no SlateDB
//!    call. The former determines whether `HA_KEYREAD_ONLY` can be advertised
//!    for an index part (Rdb_field_packing.setup probes the column).
//! 3. **Admin commands** (`check`, `optimize`, `analyze`) — `CHECK TABLE`,
//!    `OPTIMIZE TABLE`, `ANALYZE TABLE`. Per _DESIGN.md §1 these map to:
//!    - `check` → walk every SK with a snapshot iterator + cross-check the PK
//!      via `Db::get`; counts rows + verifies row checksums.
//!    - `optimize` → no direct SlateDB equivalent of
//!      `rocksdb::DB::CompactRange`. SlateDB has its own background compactor
//!      driven by the `CompactorBuilder`; user-initiated compaction is a
//!      manifest checkpoint plus a force-compact loop. Degraded but available.
//!    - `analyze` → recompute per-index cardinality via approximate-size
//!      queries on the manifest's SST set; refresh the in-memory stats cache.
//! 4. **Iterator-setup helper** (`check_bloom_and_set_bounds`) — decides
//!    whether to use SlateDB's prefix-aware bloom filter for the scan and
//!    sets the iterator upper/lower bounds. Per _DESIGN.md §0/§2 SlateDB has
//!    a global `PrefixExtractor`; this helper just decides whether the query
//!    fits the configured prefix and, if not, falls back to bounds-only.
//!
//! ## Out-of-scope methods
//! None. `optimize` is degraded (per §1 "bulk loader" / "block cache" rows
//! noting "degraded") but not non-goal; `analyze` and `check` are fully
//! supported via SlateDB primitives.

use bytes::Bytes;
use slatedb::Error;

use crate::ha_rocksdb_h__update_row_info::UpdateRowInfo;
use crate::ha_rocksdb_h__ha_rocksdb::HaSlateDb;
use crate::ha_rocksdb_cc__ha_rocksdb__lifecycle::ThdRef;

/// Mirror of MyRocks `HA_CHECK_OPT` — the bitmask passed by the SQL layer to
/// `CHECK` / `OPTIMIZE` / `ANALYZE TABLE`. Forwarded as an opaque tuple; the
/// cxx bridge interprets the bits.
#[derive(Debug, Clone, Copy, Default)]
pub struct HaCheckOpt {
    pub flags: u32,
    pub sql_flags: u32,
}

/// Iterator-bound buffer pair filled by `check_bloom_and_set_bounds`. Each
/// slice is a Bytes view into a caller-owned buffer of size `bound_len`.
#[derive(Debug, Clone, Default)]
pub struct ScanBounds {
    pub lower: Option<Bytes>,
    pub upper: Option<Bytes>,
}

impl HaSlateDb {
    /// `bool ha_rocksdb::check_keyread_allowed(uint inx, uint part, bool
    /// all_parts) const` — original C++ source line 7922.
    ///
    /// Inputs: `inx` = index position; `part` = key-part index within `inx`;
    /// `all_parts` = if true, require every part `<= part` to be index-only
    /// decodable.
    /// Outputs: `true` if `HA_KEYREAD_ONLY` is safe for this part.
    /// Errors: none — pure column-type inspection via `Rdb_field_packing`.
    /// Invariants: side-effect — when `inx == primary_key`, `all_parts` is
    /// set, and `part+1 == user_defined_key_parts`, caches `m_pk_can_be_decoded`.
    ///
    /// No SlateDB call.
    pub fn check_keyread_allowed(&mut self, inx: u32, part: u32, all_parts: bool) -> bool {
        let _ = (inx, part, all_parts);
        todo!("Rdb_field_packing.setup on the part's Field; if all_parts iterate predecessors; cache m_pk_can_be_decoded on the PK final-part call")
    }

    /// `int ha_rocksdb::check(THD*, HA_CHECK_OPT*)` — original C++ source
    /// line 8778. SQL `CHECK TABLE`.
    ///
    /// Inputs: `thd` (for sysvar reads), `opt` (CHECK flags).
    /// Outputs: `Ok(())` if no corruption found.
    /// Errors:
    /// - `ErrorKind::Data` on a row-checksum mismatch, missing PK row for an
    ///   existing SK entry, or SK row count mismatch with PK count.
    /// - `ErrorKind::Unavailable` on object-store I/O failure during the scan.
    /// Invariants: temporarily enables row-checksum verification via
    /// `m_converter.set_verify_row_debug_checksums(true)` and restores on exit.
    ///
    /// SlateDB: snapshot-scan every SK index in turn (`Db::scan` against the
    /// SK's prefix), and for each SK row issue `Db::get` on the implied PK
    /// key. Counts are compared at the end. Per _DESIGN.md §0 (snapshot row).
    pub async fn check(&mut self, thd: ThdRef, opt: HaCheckOpt) -> Result<(), Error> {
        let _ = (thd, opt);
        todo!("snapshot.scan(sk_prefix) for each SK; for each (sk_key, pk_ref): snapshot.get(pk_key) + verify checksum; compare counts")
    }

    /// `int ha_rocksdb::check_and_lock_unique_pk(uint key_id, const
    /// update_row_info&, bool *found)` — original C++ source line 9720.
    ///
    /// Inputs: `key_id` = PK position; `row_info` = packed PK + old/new
    /// payload; output slot `found`.
    /// Outputs: `Ok(found)` where `found=true` if a colliding PK exists in
    /// the current snapshot (and hence the caller should fail the insert with
    /// `HA_ERR_FOUND_DUPP_KEY`).
    /// Errors:
    /// - `ErrorKind::Transaction` if the read conflicts with an in-flight
    ///   write at SSI level.
    /// - `ErrorKind::Unavailable` on I/O failure.
    /// Invariants: must be called inside the active txn. Issues
    /// `txn.get(pk_key)` + `txn.mark_read([pk_key])` so SSI commit-time
    /// validation sees the read.
    ///
    /// MyRocks calls `get_for_update`; we map that to SSI conflict detection
    /// per _DESIGN.md §5 because SlateDB has no explicit row-lock primitive.
    pub async fn check_and_lock_unique_pk(
        &mut self,
        key_id: u32,
        row_info: &UpdateRowInfo,
    ) -> Result<bool, Error> {
        let _ = (key_id, row_info);
        todo!("txn.get(row_info.new_pk_slice).await; if Some -> Ok(true); else txn.mark_read([new_pk_slice]); Ok(false)")
    }

    /// `int ha_rocksdb::check_and_lock_sk(uint key_id, const update_row_info&,
    /// bool *found)` — original C++ source line 9805.
    ///
    /// Inputs: `key_id` = SK position; `row_info`.
    /// Outputs: `Ok(found)` where `found=true` if a colliding SK row exists.
    /// Errors: same as `check_and_lock_unique_pk`.
    /// Invariants:
    /// - Fast-path: if `key_info.flags & HA_NOSAME == 0` (no uniqueness
    ///   requirement), return `Ok(false)` immediately — no probe.
    /// - Fast-path: if updating and `!m_update_scope.is_set(key_id)` (no SK
    ///   columns changed), return `Ok(false)` immediately.
    /// - On match, the caller surfaces `HA_ERR_FOUND_DUPP_KEY` to the SQL layer.
    pub async fn check_and_lock_sk(
        &mut self,
        key_id: u32,
        row_info: &UpdateRowInfo,
    ) -> Result<bool, Error> {
        let _ = (key_id, row_info);
        todo!("if !unique or !update_scope.is_set: Ok(false); else pack SK key; txn.get(sk_key); txn.mark_read([sk_key]); compare PK tail")
    }

    /// `int ha_rocksdb::check_uniqueness_and_lock(const update_row_info&,
    /// bool pk_changed)` — original C++ source line 9945.
    ///
    /// Inputs: `row_info` (covers old + new), `pk_changed` (whether the PK
    /// columns differ between old and new). For pure INSERT, `old_pk` is
    /// empty and `pk_changed` is true.
    /// Outputs: `Ok(())` if no collision on any index.
    /// Errors:
    /// - `ErrorKind::Invalid` mapped to `HA_ERR_FOUND_DUPP_KEY` at the bridge
    ///   when a duplicate is found.
    /// - Union of `check_and_lock_unique_pk` + `check_and_lock_sk` errors.
    ///
    /// Iterates every index on the table; per index dispatches to the
    /// per-key-type helper above.
    pub async fn check_uniqueness_and_lock(
        &mut self,
        row_info: &UpdateRowInfo,
        pk_changed: bool,
    ) -> Result<(), Error> {
        let _ = (row_info, pk_changed);
        todo!("for each key_id in m_tbl_def.m_key_count: dispatch to pk vs sk variant; if found -> map to ErrorKind::Invalid(\"dup_key\")")
    }

    /// `int ha_rocksdb::check_duplicate_sk(const TABLE*, const Rdb_key_def&,
    /// const Slice* key, struct unique_sk_buf_info*)` — original C++ source
    /// line 9996.
    ///
    /// Inputs: `kd_index` = SK position; `key` = candidate packed SK bytes;
    /// `state` = caller-provided rolling buffer pair for change-detection.
    /// Outputs: `Ok(())` on no duplicate detected; `Err(Invalid(...))` mapped
    /// to `HA_ERR_FOUND_DUPP_KEY` on duplicate.
    /// Errors:
    /// - `ErrorKind::Invalid` carrying "duplicate-secondary-key" on match.
    /// Invariants: used during BULK INSERT / inplace ADD INDEX to detect dups
    /// across the sorted stream. Pure in-memory key comparison; no SlateDB
    /// call (the SK rows being checked have NOT been written yet).
    pub fn check_duplicate_sk(
        &self,
        kd_index: u32,
        key: &Bytes,
        state: &mut UniqueSkBufInfo,
    ) -> Result<(), Error> {
        let _ = (kd_index, key, state);
        todo!("kd.get_memcmp_sk_parts(key, &mut sk_buf); compare to state.last_sk_memcmp; if equal -> Err(Invalid(\"dup_sk\"))")
    }

    /// `bool ha_rocksdb::check_if_incompatible_data(HA_CREATE_INFO*, uint
    /// table_changes)` — original C++ source line 11925.
    ///
    /// Inputs: `info` (proposed CREATE INFO), `table_changes` (MariaDB bitmask).
    /// Outputs: always `Ok(true)` — i.e. data IS compatible (the upstream C++
    /// returns `COMPATIBLE_DATA_NO` which is the boolean `true` constant).
    /// Errors: none.
    /// Invariants: `true` blocks online ALTER from using the "no-copy" path;
    /// our implementation matches upstream's blanket conservative answer.
    ///
    /// No SlateDB call. Pure SQL-layer compat check.
    pub fn check_if_incompatible_data(&self, info: &HaCreateInfo, table_changes: u32) -> bool {
        let _ = (info, table_changes);
        true
    }

    /// `int ha_rocksdb::optimize(THD*, HA_CHECK_OPT*)` — original C++ source
    /// line 12108. SQL `OPTIMIZE TABLE`.
    ///
    /// Inputs: `thd`, `opt`.
    /// Outputs: `Ok(())` on success.
    /// Errors:
    /// - `ErrorKind::Unavailable` on compaction I/O failure.
    ///
    /// Per _DESIGN.md §1 (bulk loader / block cache rows: degraded):
    /// SlateDB has no `CompactRange(cf, start, limit)`. We approximate by
    /// triggering a `Db::flush_with_options(FlushType::MemTable)` on the CF
    /// prefix range plus signalling the configured `Compactor` to run with
    /// `bottommost_level_compaction = kForce` semantics. If no Compactor is
    /// attached (settings-disabled), succeed without doing anything.
    ///
    /// Loops over every `key_id` and compacts that index's prefix range.
    pub async fn optimize(&self, thd: ThdRef, opt: HaCheckOpt) -> Result<(), Error> {
        let _ = (thd, opt);
        todo!("for each key_id: get_range(key_id, buf); db.flush_with_options(FlushType::MemTable).await; signal compactor")
    }

    /// `int ha_rocksdb::analyze(THD*, HA_CHECK_OPT*)` — original C++ source
    /// line 12263. SQL `ANALYZE TABLE`.
    ///
    /// Inputs: `thd`, `opt`.
    /// Outputs: `Ok(())` on success.
    /// Errors:
    /// - `ErrorKind::Unavailable` on object-store I/O failure during the
    ///   stats refresh.
    ///
    /// Recomputes per-index cardinality from SlateDB's
    /// `DbMetadataOps::subscribe()` snapshot (most-recent manifest) and the
    /// approximate-size API per CF prefix range. Then calls
    /// `info(HA_STATUS_CONST | HA_STATUS_VARIABLE)` to push the recomputed
    /// stats into the SQL-layer `handler::stats` block (so `EXPLAIN` plans
    /// see them immediately, not after the next OPEN).
    pub async fn analyze(&mut self, thd: ThdRef, opt: HaCheckOpt) -> Result<(), Error> {
        let _ = (thd, opt);
        todo!("calculate_stats_for_table().await; self.info(HA_STATUS_CONST | HA_STATUS_VARIABLE).await")
    }

    /// `bool ha_rocksdb::check_bloom_and_set_bounds(THD*, const Rdb_key_def&,
    /// const Slice& eq_cond, bool use_all_keys, size_t bound_len, uchar*
    /// lower, uchar* upper, Slice* lower_slice, Slice* upper_slice)` —
    /// original C++ source line 13867.
    ///
    /// Inputs: `kd_index`, `eq_cond` = the equality-prefix slice, `use_all_keys`,
    /// `bound_len` = caller-allocated buffer size.
    /// Outputs: `Ok((can_use_bloom, bounds))` — if `can_use_bloom == true`,
    /// the bounds are left empty (SlateDB will use prefix-aware bloom to skip
    /// SSTs); if `false`, `bounds.lower` / `bounds.upper` are populated and
    /// the caller passes them via `ScanOptions::with_lower_bound` /
    /// `with_upper_bound`.
    /// Errors: none.
    /// Invariants: pure decision + buffer packing; no SlateDB call.
    ///
    /// Decision rule (matches upstream): if the `PrefixExtractor`-extracted
    /// prefix of `eq_cond` equals the per-CF configured prefix length, bloom
    /// is usable. Otherwise fall through to bounds.
    pub fn check_bloom_and_set_bounds(
        &self,
        thd: ThdRef,
        kd_index: u32,
        eq_cond: &Bytes,
        use_all_keys: bool,
        bound_len: usize,
    ) -> (bool, ScanBounds) {
        let _ = (thd, kd_index, eq_cond, use_all_keys, bound_len);
        todo!("can_use_bloom_filter(thd, kd, eq_cond, use_all_keys); if false: setup_iterator_bounds(kd, eq_cond, bound_len)")
    }
}

/// Mirror of `struct unique_sk_buf_info` (declared in `ha_rocksdb.h`).
/// Provided by A2 — referenced here for signature completeness only.
#[derive(Debug, Default)]
pub struct UniqueSkBufInfo {
    /// Ping-pong buffer for `kd.get_memcmp_sk_parts` output (avoids realloc).
    pub buf_a: bytes::BytesMut,
    pub buf_b: bytes::BytesMut,
    pub use_a: bool,
    pub last_sk_memcmp: bytes::Bytes,
}

/// Mirror of MyRocks `HA_CREATE_INFO` (re-declared minimally per _DESIGN.md
/// rules — no exposing `HA_CREATE_INFO` C++ type). Canonical decl lives in
/// A2's handler-hub stub.
#[derive(Debug, Default)]
pub struct HaCreateInfo {
    pub auto_increment_value: u64,
    pub used_fields: u32,
}
