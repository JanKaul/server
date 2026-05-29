//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__key_compare`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (124 LoC body, 4 methods:
//! `compare_keys`, `compare_key_parts`, `get_old_key_positions`, `is_using_full_key`)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__key_compare`
//! parent: `ha_rocksdb_cc`
//!
//! ## Mapping
//! Pure in-memory key comparisons (no SlateDB calls). These run on the
//! handler thread and operate on already-decoded `KeyShareView` data
//! (see `ha_rocksdb_h__ha_rocksdb.rs`). Per _DESIGN.md §2, memcomparable
//! encoding is preserved bit-for-bit from MyRocks — comparison semantics
//! transfer directly.
//!
//! ## Out-of-scope methods
//! None — these are utilities, all in-scope.

use slatedb::Error;

use crate::ha_rocksdb_h__ha_rocksdb::{HaSlateDb, KeyShareView, TableShareView};

impl HaSlateDb {
    /// Compare two key definitions for equality. Used during inplace ALTER
    /// to detect "the same index under a different name".
    /// Original: ha_rocksdb.cc — `compare_keys`.
    ///
    /// Returns `false` if keys differ in field set / length / direction;
    /// `true` if semantically identical.
    pub fn compare_keys(
        &self,
        old_key: &KeyShareView,
        new_key: &KeyShareView,
    ) -> bool {
        todo!("compare key_parts: field offsets, lengths, null_offsets, flags, direction")
    }

    /// Compare specific key-part sets across two keys. Used to detect
    /// "prefix overlap" in inplace ALTER.
    /// Original: ha_rocksdb.cc — `compare_key_parts`.
    ///
    /// Returns the number of leading key-parts that match between `old_key`
    /// and `new_key`; 0 if no overlap.
    pub fn compare_key_parts(
        &self,
        old_key: &KeyShareView,
        new_key: &KeyShareView,
    ) -> u32 {
        todo!("walk key_parts in lockstep; count matching prefix")
    }

    /// Build a `name → position` map for the old table's keys. Used to map
    /// surviving indexes to their new positions when ALTER reorders them.
    /// Original: ha_rocksdb.h:734 — `get_old_key_positions`.
    pub fn get_old_key_positions(
        &self,
        new_table: &TableShareView,
        old_table: &TableShareView,
    ) -> std::collections::HashMap<String, u32> {
        todo!("map each old key's name to its position in the new TABLE")
    }

    /// True when the lookup tuple covers ALL key-parts of `actual_key_parts`
    /// (i.e., `keypart_map` has bits set for parts 0..actual_key_parts).
    /// Used by `index_read_map_impl` to decide whether a point-lookup
    /// optimization applies vs needing to fall through to a range scan.
    /// Original: ha_rocksdb.h:616 — `is_using_full_key`.
    pub fn is_using_full_key(
        &self,
        keypart_map: u64,
        actual_key_parts: u32,
    ) -> bool {
        // The C++ pattern: ((keypart_map + 1) & keypart_map) == 0 means
        // keypart_map is all 1s in its low bits; combined with the count
        // check for "covers exactly actual_key_parts".
        let expected = (1u64 << actual_key_parts) - 1;
        keypart_map == expected
    }
}
