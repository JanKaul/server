//! Interface stub for `rdb_comparator_h`.
//!
//! C++ source: `storage/rocksdb/rdb_comparator.h` (85 LoC)
//! C++ classes: `Rdb_pk_comparator`, `Rdb_rev_comparator`
//!
//! ## Mapping
//! In MyRocks, these are `rocksdb::Comparator` implementations registered
//! per-CF. RocksDB needs them because each SST has CF-specific ordering.
//!
//! **In SlateDB this concept does not exist.** SlateDB sorts byte-lexicographically
//! everywhere — there's no per-CF comparator. Our key-prefix scheme (_DESIGN.md §2)
//! relies on this: `varint(cf_id) || u32_be(index_id) || memcmp_key`.
//!
//! For reverse-ordered scans we have **two complementary mechanisms**:
//!
//! 1. **Scan-time** (ad-hoc reverse): `ScanOptions::with_order(IterationOrder::Descending)`
//!    — SlateDB supports descending iteration natively (`config.rs` + `db_iter.rs`).
//!    Used for ORDER BY ... DESC queries against any index.
//!
//! 2. **Encode-time** (index declared reverse at CREATE TABLE): the codec XORs
//!    each byte of the memcomparable encoding with `0xff` so byte-lexicographic
//!    order produces the desired reverse semantic order. This preserves the
//!    "stored-bytes already in scan order" invariant that lets us use forward
//!    scans + native bloom filters for these indexes.
//!
//! Mode 2 matches MyRocks' `Rdb_rev_comparator` semantics. Mode 1 is a SlateDB
//! capability MyRocks didn't have direct access to.
//!
//! ## Out-of-scope methods
//! - `FindShortestSeparator` / `FindShortSuccessor` — RocksDB-specific
//!   compaction-time tip-trimming. SlateDB has no analogue. Not exposed.
//! - `Name()` — comparator-identity string written into SST metadata. Not
//!   applicable to SlateDB (no per-CF comparator metadata).

use slatedb::Error;

/// Marker for indexes whose memcomparable encoding is bit-inverted, so that
/// byte-lexicographic order produces reverse semantic order.
///
/// Carried on a per-index basis in the data dictionary (replaces MyRocks'
/// choice of `Rdb_pk_comparator` vs `Rdb_rev_comparator` at CF registration).
///
/// At runtime, an index-declared `Reverse` plus a `SELECT ... ORDER BY DESC`
/// can stack: a `Reverse`-encoded index scanned with
/// `IterationOrder::Descending` produces forward-semantic results — useful
/// for hash-partitioned scans where direction depends on the partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyDirection {
    #[default]
    Forward,
    Reverse,
}

/// Encode a memcomparable key with the given direction. `forward` returns the
/// input unchanged; `reverse` XORs each byte with `0xff` (a length-preserving
/// involution that flips bytewise ordering).
///
/// Used by the codec (rdb_datadic) at index-encode time, NOT at runtime
/// comparison time — SlateDB does byte comparison itself.
pub fn apply_direction(direction: KeyDirection, key: &mut [u8]) {
    match direction {
        KeyDirection::Forward => (),
        KeyDirection::Reverse => {
            for b in key.iter_mut() { *b ^= 0xff; }
        }
    }
}

/// Decode the original bytes from a direction-applied key. Same operation as
/// encode (XOR is self-inverse), exposed under a different name for clarity
/// at the codec call site.
pub fn unapply_direction(direction: KeyDirection, key: &mut [u8]) {
    apply_direction(direction, key);
}

/// Byte-comparison wrapper used in tests and in places that compare keys
/// without going through SlateDB (e.g., the in-memory `WriteBatch` order check
/// before commit). For SlateDB-side comparison, the engine does this itself.
pub fn compare(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    a.cmp(b)
}

/// Map a `KeyDirection` + caller-requested SQL scan direction to the
/// `IterationOrder` we pass to `Db::scan_with_options`.
///
/// Truth table:
/// | KeyDirection | SQL scan dir | IterationOrder      |
/// |--------------|--------------|---------------------|
/// | Forward      | ASC          | Ascending           |
/// | Forward      | DESC         | Descending          |
/// | Reverse      | ASC          | Descending          |
/// | Reverse      | DESC         | Ascending           |
pub fn iteration_order(
    direction: KeyDirection,
    sql_descending: bool,
) -> slatedb::IterationOrder {
    let physical_descending = match direction {
        KeyDirection::Forward => sql_descending,
        KeyDirection::Reverse => !sql_descending,
    };
    if physical_descending {
        slatedb::IterationOrder::Descending
    } else {
        slatedb::IterationOrder::Ascending
    }
}
