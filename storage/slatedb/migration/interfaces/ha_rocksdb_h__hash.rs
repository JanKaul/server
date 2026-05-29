//! Interface stub for `ha_rocksdb_h__hash`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.h` (lines 130..137, 8 LoC body)
//! v4 manifest sub-unit: `ha_rocksdb_h__hash`
//! parent: `ha_rocksdb_h`
//!
//! ## Mapping
//! C++ `std::hash<GL_INDEX_ID>` specialization so `GL_INDEX_ID` can be a
//! `std::unordered_set` / `std::unordered_map` key. In Rust this is just a
//! `Hash` derive on `GlIndexId` (already present in `rdb_global_h.rs:148`).
//! This stub is a no-op marker confirming the design decision.
//!
//! ## Out-of-scope methods
//! None — no runtime contract beyond the `Hash` impl.

// `crate::rdb_global_h::GlIndexId` derives `Hash` directly:
//
//   #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
//   pub struct GlIndexId { pub cf_id: u32, pub index_id: u32 }
//
// The MyRocks C++ specialization packs `(cf_id, index_id)` into a u64 and
// then hashes that u64. The default Rust derive hashes the two fields
// sequentially, which produces a different bit pattern but the same
// equivalence class. Both are correct; we use the default because the C++
// pack-into-u64 was a micro-optimization for `std::hash`'s u64 fast path,
// which doesn't apply to Rust's `Hasher` API (it does its own finalization).
//
// If a hot-path benchmark later shows hashing dominates, we can switch to a
// manual `impl Hash` that packs (cf_id << 32 | index_id) before hashing.
