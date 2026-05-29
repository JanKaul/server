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
//! relies on this: `varint(cf_id) || index_id_u32_be || memcmp_key`.
//!
//! Reverse-ordered indexes (MyRocks' `Rdb_rev_comparator`) must therefore be
//! implemented at the **encoder** layer: the memcomparable encoding for a
//! reverse-ordered column XORs all bytes with `0xff` so byte-lexicographic
//! ordering produces the desired reverse semantic ordering. This is a behavior
//! change visible only in this interface — the engine itself sees bytewise sort.
//!
//! ## Out-of-scope methods
//! - `FindShortestSeparator` / `FindShortSuccessor` — RocksDB-specific
//!   compaction-time tip-trimming. SlateDB has no analogue. Not exposed.
//! - `Name()` — comparator-identity string written into SST metadata. Not
//!   applicable to SlateDB (no per-CF comparator metadata).

use crate::error::SlateError;

/// Marker for indexes whose memcomparable encoding is bit-inverted, so that
/// byte-lexicographic order produces reverse semantic order.
///
/// Carried on a per-index basis in the data dictionary (replaces MyRocks'
/// choice of `Rdb_pk_comparator` vs `Rdb_rev_comparator` at CF registration).
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

/// Byte-comparison wrapper used in tests and in places that compare keys without
/// going through SlateDB (e.g., the in-memory write batch before flush).
///
/// Replaces both `Rdb_pk_comparator::Compare` and `Rdb_rev_comparator::Compare`
/// from rdb_comparator.h:45, 72.
pub fn compare(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    a.cmp(b)
}
