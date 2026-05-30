//! Index ordering direction and the `IterationOrder` mapping.
//!
//! Translated from `storage/rocksdb/rdb_comparator.h`. SlateDB sorts bytes
//! lexicographically across the board, so the MyRocks per-CF comparator
//! collapses to two mechanisms (per `_DESIGN.md §2`):
//!
//! - **Encode-time** for indexes declared reverse at CREATE TABLE: the codec
//!   XORs each byte of the memcomparable encoding with `0xff` so byte
//!   ordering produces reverse semantic order. This preserves SlateDB's
//!   "stored-bytes already in scan order" invariant.
//! - **Scan-time** for ad-hoc `ORDER BY ... DESC`:
//!   `slatedb::IterationOrder::Descending`.

/// Marker for indexes whose memcomparable encoding is bit-inverted, so byte
/// order produces reverse semantic order. Carried per-index in the data
/// dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum KeyDirection {
    #[default]
    Forward,
    Reverse,
}

/// Encode a memcomparable key with the given direction. `Forward` returns the
/// input unchanged; `Reverse` XORs each byte with `0xff` — a length-preserving
/// involution that flips bytewise ordering.
pub fn apply_direction(direction: KeyDirection, key: &mut [u8]) {
    if let KeyDirection::Reverse = direction {
        for b in key.iter_mut() {
            *b ^= 0xff;
        }
    }
}

/// Decode the original bytes from a direction-applied key. XOR is its own
/// inverse; alias for clarity at call sites.
pub fn unapply_direction(direction: KeyDirection, key: &mut [u8]) {
    apply_direction(direction, key);
}

/// Byte comparison wrapper. Used in tests and in in-memory `WriteBatch`
/// ordering checks; SlateDB does its own comparison on the storage side.
pub fn compare(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    a.cmp(b)
}

/// Map `(KeyDirection, sql_descending)` → physical `IterationOrder` to pass
/// to `Db::scan_with_options`.
///
/// | KeyDirection | SQL scan dir | IterationOrder |
/// |--------------|--------------|----------------|
/// | Forward      | ASC          | Ascending      |
/// | Forward      | DESC         | Descending     |
/// | Reverse      | ASC          | Descending     |
/// | Reverse      | DESC         | Ascending      |
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_leaves_bytes_alone() {
        let mut key = [0x12u8, 0x34, 0x56];
        apply_direction(KeyDirection::Forward, &mut key);
        assert_eq!(key, [0x12, 0x34, 0x56]);
    }

    #[test]
    fn reverse_xor_is_an_involution() {
        let original = [0x00u8, 0x12, 0x34, 0xab, 0xff];
        let mut key = original;
        apply_direction(KeyDirection::Reverse, &mut key);
        assert_eq!(key, [0xff, 0xed, 0xcb, 0x54, 0x00]);
        unapply_direction(KeyDirection::Reverse, &mut key);
        assert_eq!(key, original);
    }

    #[test]
    fn reverse_flips_byte_order_relation() {
        let mut a = [0x10u8, 0x20];
        let mut b = [0x10u8, 0x30];
        assert!(compare(&a, &b).is_lt());
        apply_direction(KeyDirection::Reverse, &mut a);
        apply_direction(KeyDirection::Reverse, &mut b);
        assert!(compare(&a, &b).is_gt());
    }

    #[test]
    fn iteration_order_truth_table() {
        use slatedb::IterationOrder;
        assert!(matches!(
            iteration_order(KeyDirection::Forward, false),
            IterationOrder::Ascending
        ));
        assert!(matches!(
            iteration_order(KeyDirection::Forward, true),
            IterationOrder::Descending
        ));
        assert!(matches!(
            iteration_order(KeyDirection::Reverse, false),
            IterationOrder::Descending
        ));
        assert!(matches!(
            iteration_order(KeyDirection::Reverse, true),
            IterationOrder::Ascending
        ));
    }
}
