//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__iter_setup`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (span 10459..10586, body ~121 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__iter_setup`
//!
//! ## Mapping
//! Iterator construction + teardown for the `m_scan_it` slot on `ha_rocksdb`.
//! Maps directly to **`Db::scan_with_options(range, ScanOptions::new()
//! .with_order(IterationOrder::{Asc,Desc}))`** — _DESIGN.md §1 row "Iterators".
//!
//! Two collapses worth noting:
//!
//! 1. **`setup_iterator_bounds`** computes a `(lower, upper)` byte pair and
//!    swaps them if the CF is reverse. In our world the swap goes away —
//!    reverse-encoded indexes have already had their bytes XORed at write
//!    time, so byte-lex order *is* semantic order. We still compute a range,
//!    but it's always `[lower, upper)` with no swap.
//!
//! 2. **`m_scan_it_snapshot`** (the per-iterator snapshot held during
//!    `commit_in_the_middle` bulk loads) maps to `Db::snapshot() ->
//!    Arc<DbSnapshot>` and the iterator is created from that snapshot. No
//!    `rdb->ReleaseSnapshot` call needed — drop is `Arc` decref.
//!
//! Bloom-filter selection ("skip_bloom"): SlateDB's `PrefixExtractor` does
//! this implicitly per _DESIGN.md §2. We expose the on/off bit but the actual
//! filter consultation happens inside SlateDB.
//!
//! ## Out-of-scope methods
//! None — all 4 iter_setup methods are in scope.

use slatedb::Error;
use bytes::Bytes;

use crate::rdb_comparator_h::KeyDirection;

/// Forward-declared. Owned by A2's `ha_rocksdb_h` stub.
use crate::ha_rocksdb_h__ha_rocksdb::HaSlateDb;

/// A computed range for `Db::scan_with_options`. Both bounds are inclusive at
/// the lower end and exclusive at the upper end (matching SlateDB's
/// `std::ops::Range<Bytes>` convention).
#[derive(Debug, Clone)]
pub struct ScanBounds {
    pub lower: Bytes,
    pub upper: Bytes,
}

impl HaSlateDb {
    /// Compute the `(lower, upper)` byte range for a scan over `kd` whose
    /// equality-condition prefix is `eq_cond`. `bound_len` is the number of
    /// prefix bytes that should remain fixed across the range.
    ///
    /// Special case (matches C++): if `eq_cond.len() <= INDEX_NUMBER_SIZE`,
    /// use the index's infimum/supremum keys (full-index scan).
    ///
    /// Per the note in _DESIGN.md §2 + the rdb_comparator_h stub: we do NOT
    /// swap the bounds for reverse-CF — encoding is symmetric, so byte order
    /// already reflects scan order. The `direction` argument is therefore a
    /// no-op for bound computation; it's kept in the signature only so the
    /// call site mirrors MyRocks (and so we can re-introduce the swap if a
    /// future SlateDB version changes the encoding contract).
    ///
    /// Original C++: ha_rocksdb.cc:10459.
    pub fn setup_iterator_bounds(
        &self,
        eq_cond: &Bytes,
        bound_len: usize,
        direction: KeyDirection,
    ) -> ScanBounds {
        let _ = (eq_cond, bound_len, direction);
        todo!("compute infimum/supremum from kd if eq_cond small; else successor/predecessor of eq_cond[..bound_len]")
    }

    /// Allocate (or re-use) the scan iterator `m_scan_it` for this handler.
    /// `slice` is the equality-condition prefix; `eq_cond_len` how much of
    /// it is fixed; `use_all_keys` flags the all-keys-equal optimization.
    ///
    /// Steps:
    ///   1. Call `check_bloom_and_set_bounds` (from the `table_mgmt` bucket)
    ///      to decide whether to enable the prefix bloom on this iterator.
    ///   2. If existing `m_scan_it` was created with a different bloom flag,
    ///      drop it first via `release_scan_iterator`.
    ///   3. If `commit_in_the_middle()` (bulk-load mode): take a
    ///      `Db::snapshot()` and build the iterator from it. Otherwise: ask
    ///      the per-txn iterator factory (`tx.iterator(...)`).
    ///   4. Store the iterator handle in `m_scan_it`.
    ///
    /// No Result — failures from SlateDB are deferred until first `next()`.
    ///
    /// Original C++: ha_rocksdb.cc:10493.
    pub async fn setup_scan_iterator(
        &mut self,
        slice: &Bytes,
        use_all_keys: bool,
        eq_cond_len: u32,
        direction: KeyDirection,
    ) -> Result<(), Error> {
        let _ = (slice, use_all_keys, eq_cond_len, direction);
        todo!("compute bounds; pick snapshot vs txn-iter; build scan_with_options(range, order)")
    }

    /// Drop the scan iterator and its pinned snapshot (if any). Idempotent
    /// — safe to call when `m_scan_it` is already None. No SlateDB API
    /// call needed for snapshot release; `Arc::drop` handles it.
    ///
    /// Original C++: ha_rocksdb.cc:10565.
    pub fn release_scan_iterator(&mut self) {
        todo!("self.m_scan_it = None; self.m_scan_it_snapshot = None")
    }

    /// Iterator setup for table-rnd-scan (the `SELECT *` no-index path).
    /// Seeds the iterator at the PK index's infimum and arms the
    /// "skip first next() call" flag, so the very first `rnd_next` will
    /// return the seek-position row rather than its successor.
    ///
    /// Internally calls `setup_scan_iterator` with `use_all_keys=false`
    /// and the PK's `get_first_key` as the seed.
    ///
    /// Original C++: ha_rocksdb.cc:10575.
    pub async fn setup_iterator_for_rnd_scan(&mut self) -> Result<(), Error> {
        todo!("kd.get_first_key into m_pk_packed_tuple; setup_scan_iterator(..); iter.seek; m_skip_scan_it_next_call = true")
    }
}
