//! Interface stub for `properties_collector_h`.
//!
//! C++ source: `storage/rocksdb/properties_collector.h` (215 LoC)
//! C++ classes: `Rdb_compact_params`, `Rdb_index_stats`, `Rdb_tbl_card_coll`,
//!              `Rdb_tbl_prop_coll`, `Rdb_tbl_prop_coll_factory`
//!
//! ## Mapping
//! In MyRocks this is a `rocksdb::TablePropertiesCollector` that runs during
//! SST builds: it observes each `(key, value, EntryType)` as the SST is
//! flushed/compacted, accumulates per-index cardinality + entry-type counts,
//! and stamps the results into SST user properties. The DDL manager later
//! aggregates these per-SST properties into the index-stats cache.
//!
//! SlateDB does **not** expose per-SST-build hooks. Per _DESIGN.md §1 +
//! `event_listener_h` exemplar, the replacement is the
//! **`DbMetadataOps::subscribe()`** + **`VersionedManifest`** pull model:
//!
//! - Background `StatsRefreshTask` (see `event_listener_h.rs`) wakes on every
//!   manifest change.
//! - It scans `VersionedManifestSnapshot.ssts` (see `rdb_sst_info_h.rs`) and
//!   re-derives per-index stats by sampling a few keys from each SST via a
//!   `DbReader` opened at that manifest's seq.
//! - `Rdb_tbl_card_coll`'s sampling logic is preserved here as a pure-Rust
//!   `IndexCardCollector` — it processes a key stream and emits an
//!   `IndexStats` snapshot. The driver changes (push → pull) but the math
//!   doesn't.
//!
//! `Rdb_compact_params` (deletes-in-window heuristic for triggering manual
//! compaction) becomes a Rust struct stored alongside SlateDB's
//! `CompactorOptions`. SlateDB's compactor selects targets by its own
//! heuristics, so this becomes advisory metadata exposed via I_S only.
//!
//! ## Out-of-scope methods
//! - `AddUserKey`, `Finish`, `GetReadableProperties`, `NeedCompact` —
//!   `TablePropertiesCollector` virtuals. No SlateDB equivalent; replaced by
//!   the manifest-scan flow.
//! - `Rdb_tbl_prop_coll_factory::CreateTablePropertiesCollector` — factory
//!   plumbing for the above. Removed.
//! - `read_stats_from_tbl_props(table_props)` — RocksDB SST property struct;
//!   replaced by `IndexCardCollector::finish` on a sampled key stream.

use bytes::Bytes;
use slatedb::Error;
use std::sync::Arc;

/// Compaction-trigger heuristic: if more than `deletes` tombstones are seen
/// within any sliding window of `window` rows in a file of size `file_size`,
/// the index is flagged as needing compaction (a hint to the I_S layer).
/// Replaces C++ `Rdb_compact_params`.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompactParams {
    pub deletes: u64,
    pub window: u64,
    pub file_size: u64,
}

/// Per-index aggregated statistics. Persisted under the system CF prefix and
/// surfaced via I_S. Replaces C++ `Rdb_index_stats`.
///
/// The `m_distinct_keys_per_prefix` vector mirrors MyRocks' n-way cardinality
/// estimation (one entry per key-part prefix length).
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub gl_index_id: crate::rdb_global_h::GlIndexId,
    pub data_size: i64,
    pub rows: i64,
    pub actual_disk_size: i64,
    pub entry_deletes: i64,
    pub entry_single_deletes: i64,
    pub entry_merges: i64,
    pub entry_others: i64,
    pub distinct_keys_per_prefix: Vec<i64>,
    /// Index name — not persisted, populated by I_S join with the DDL manager.
    pub name: String,
}

impl IndexStats {
    /// On-disk version constants — preserved bit-for-bit since persisted
    /// stats from a pre-migration MyRocks install must remain readable.
    pub const VERSION_INITIAL: u32 = 1;
    pub const VERSION_ENTRY_TYPES: u32 = 2;

    pub fn new(gl_index_id: crate::rdb_global_h::GlIndexId) -> Self {
        Self { gl_index_id, ..Default::default() }
    }

    /// Encode a `Vec<IndexStats>` into the system-CF blob format. Same wire
    /// layout as MyRocks `Rdb_index_stats::materialize` so existing data
    /// dictionaries round-trip.
    pub fn materialize(_stats: &[IndexStats]) -> Bytes {
        todo!("port the MyRocks marshalling: u32 version || count || per-entry tlv")
    }

    /// Inverse of `materialize`. Errors: `Data` on corrupt input.
    pub fn unmaterialize(_blob: &[u8]) -> Result<Vec<IndexStats>, Error> {
        todo!("port unmaterialize; surface format-version mismatch as Error::data")
    }

    /// Fold `s` into `self`. `increment` toggles add vs subtract; used during
    /// the post-flush refresh when stats for individual SSTs are gained/lost.
    /// Original: `Rdb_index_stats::merge` (properties_collector.h:74).
    pub fn merge(&mut self, _s: &IndexStats, _increment: bool, _estimated_data_len: i64) {
        todo!("element-wise add/sub of every counter + distinct_keys_per_prefix slot")
    }
}

/// Online cardinality sampler. Replaces C++ `Rdb_tbl_card_coll` — the
/// sampling math is preserved; the driver (was: RocksDB SST builder, now:
/// background manifest scanner) is what differs.
pub struct IndexCardCollector {
    /// Percentage of keys to sample, 1-100. Sysvar parity with MyRocks
    /// `rocksdb_table_stats_sampling_pct` (rdb_global_h::DEFAULT_TBL_STATS_SAMPLE_PCT).
    pub sampling_pct: u8,
    /// PRNG state for deterministic-per-task sampling.
    seed: u32,
    /// Buffered last key — used to detect distinct-key transitions per prefix.
    last_key: Vec<u8>,
}

impl IndexCardCollector {
    pub fn new(sampling_pct: u8) -> Self {
        Self { sampling_pct, seed: 0, last_key: Vec::new() }
    }

    /// Process one key from the sampled stream. Updates `stats` in place.
    /// Original: `Rdb_tbl_card_coll::ProcessKey`.
    ///
    /// `keydef_prefix_lens` replaces the C++ `Rdb_key_def*` argument: only the
    /// per-keypart byte-length array is needed for cardinality math, and that
    /// keeps the type out of this header (per the "no `KEY_PART`" contract).
    pub fn process_key(
        &mut self,
        _key: &[u8],
        _keydef_prefix_lens: &[usize],
        _stats: &mut IndexStats,
    ) {
        todo!("compare key against last_key per prefix-length, bump distinct_keys_per_prefix")
    }

    /// Reset between indexes — distinct-key history must not carry over.
    pub fn reset(&mut self) {
        self.last_key.clear();
    }

    /// Post-process the raw counts to compensate for sampling. Cardinality is
    /// approximate; estimates may exceed the row count (caller caps it).
    /// Original: `Rdb_tbl_card_coll::AdjustStats`.
    pub fn adjust_stats(&self, _stats: &mut IndexStats) {
        todo!("multiply distinct_keys_per_prefix by 100/sampling_pct")
    }

    fn _should_collect(&self) -> bool {
        self.sampling_pct >= 100
    }
}

/// Background task that scans the current `VersionedManifestSnapshot` and
/// refreshes per-index stats by sampling. Driven by `StatsRefreshTask`
/// (event_listener_h). Replaces C++ `Rdb_tbl_prop_coll` + `_factory`.
pub struct ManifestStatsScanner {
    pub db: Arc<slatedb::Db>,
    pub sampling_pct: u8,
    pub params: CompactParams,
}

impl ManifestStatsScanner {
    pub fn new(db: Arc<slatedb::Db>, sampling_pct: u8, params: CompactParams) -> Self {
        Self { db, sampling_pct, params }
    }

    /// Scan a single manifest snapshot, produce a fresh stats vector. Errors:
    /// `Unavailable` on object-store read; `Closed` if the DB shuts down.
    pub async fn scan(
        &self,
        _snapshot: &crate::rdb_sst_info_h::VersionedManifestSnapshot,
    ) -> Result<Vec<IndexStats>, Error> {
        todo!("per SST: open a sampled reader, run IndexCardCollector, fold into per-(cf_id, index_id)")
    }
}
