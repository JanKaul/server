# INTERFACE phase — checkpoint for cross-cutting design review

**Status:** in progress. **`_DESIGN.md` v2 written after auditing the actual
SlateDB source** (workspace 0.13.0 at `../slatedb/`, git HEAD `87c997c`); 5
exemplar leaf-header stubs refreshed to match. Awaiting human approval of
v2 before generating the remaining 127 stubs.

## What changed between v1 (initial) and v2 (verified)

The v1 _DESIGN.md was built from general knowledge of SlateDB. **Most of its
"Re-impl" / "Non-goal" verdicts were wrong** because SlateDB actually has the
feature natively. v2 was rewritten against the published API.

### v1 → v2 corrections

| v1 claim | v2 reality | Source |
|---|---|---|
| "SlateDB has no merge operator; we'd implement RMW Rust-side." | `MergeOperator` trait with key-aware routing built-in. | `merge_operator.rs:68` |
| "No compaction filter; background sweep task." | `CompactionFilter` trait with `Keep`/`Drop`/`Modify`. | `compaction_filter.rs` (feature `compaction_filters`) |
| "No snapshot API; we'd use `read_view(seq)`." | `Db::snapshot() -> Arc<DbSnapshot>` natively. | `db_snapshot.rs` |
| "Write batching is a §9 first-class unit we design from scratch." | `WriteBatch` natively, committed via `Db::write(batch)`. Our role narrowed to per-statement aggregator. | `batch.rs:52` |
| "Iterators only support forward scans." | `IterationOrder::{Ascending, Descending}` via `ScanOptions::with_order`. | `iter.rs:7`, `db_iter.rs` |
| "No native TTL; we'd encode TTL prefix in values." | `PutOptions.ttl: Ttl::{Default, NoExpiry, ExpireAfter(s), ExpireAt(ts)}`. `RowEntry.expire_ts` is a first-class field. | `config.rs:553`, `types.rs:18` |
| "No transactions exposed; we'd implement OCC." | `DbTransaction` + `IsolationLevel::{Snapshot, SerializableSnapshot}` (SI and SSI). | `transaction_manager.rs:16`, `ops.rs:386` |
| "No prefix extractor; we'd write our own SST-level filter." | `PrefixExtractor` trait + `FilterPolicy::with_prefix_extractor` natively. | `filter_policy.rs:220`, `prefix_extractor.rs` |
| "Per-flush callback like RocksDB EventListener." | `DbMetadataOps::subscribe()` returns a `watch::Receiver<DbStatus>` — pull/await model. | `ops.rs:512` |
| "Custom `SlateError` enum mapping to HA_ERR_*." | Use `slatedb::Error` + `ErrorKind` (only 6 variants — `Transaction`, `Closed`, `Unavailable`, `Invalid`, `Data`, `Internal`). | `error.rs:391` |
| "Block cache is a single global LRU we don't tune." | Pluggable via `DbBuilder::with_block_cache(Arc<dyn DbCache>)` — foyer or moka. | `db_cache/mod.rs:420` |
| "Object store hard-coded to S3." | `Arc<dyn ObjectStore>` parameterizes the builder. Features: `aws` (default), `azure`, `gcp`, `opendal`; plus `InMemory` and `LocalFileSystem` from upstream `object_store` crate. | `db.rs:707`, Cargo.toml features |
| "Read/write durability fixed." | `ReadOptions::durability_filter: DurabilityLevel::{Memory, Remote}`, `ReadOptions::dirty: bool`, `WriteOptions::await_durable: bool`, `FlushOptions::flush_type: FlushType::{Wal, MemTable}`. | `config.rs:257, 420, 448` |

### Open questions count: 12 → 8

Several v1 questions were "do we re-implement X?" — moot now that SlateDB
provides X. The remaining 8 are integration choices (which features to
enable, version pinning, statement-boundary commit policy).

### File template changes

- `use slatedb::Error` everywhere instead of `use crate::error::SlateError`.
- `bytes::Bytes` (re-exported as `slatedb::bytes`) is the keys/values type.
- Async fns are fine — they're internal; bridge surface stays sync.
- Doc-comment "Error conditions" lines name `slatedb::ErrorKind` variants.

## What's in `interfaces/` now

- `_DESIGN.md` (v2, 13 sections, 8 open questions) — **the thing to review first**
- `atomic_stat_h.rs` — unchanged from v1 (no SlateDB dependencies)
- `event_listener_h.rs` — now uses `DbStatus` subscription pattern (more SlateDB-native than the v1 polling task)
- `rdb_global_h.rs` — now uses `slatedb::Error`; SYSTEM_CF_ID reserved; references `PrefixExtractor` for the prefix scheme
- `rdb_buff_h.rs` — now uses `slatedb::Error`; adds `into_bytes()` helper for SlateDB API integration
- `rdb_comparator_h.rs` — now distinguishes scan-time (`IterationOrder::Descending`) from encode-time (`KeyDirection::Reverse`); adds `iteration_order(direction, sql_descending)` helper that composes both

## Open questions in v2 (8, down from 12)

1. **CF-id → key-prefix layout** — `varint(cf_id) || u32_be(index_id) || ...` matches MyRocks and feeds cleanly into SlateDB's `PrefixExtractor`. Confirm.
2. **Enable `compaction_filters` feature** (for the dropped-index sweep). Confirm.
3. **TTL precision** — SlateDB seconds (`ExpireAfter(u64)`) matches MyRocks. Confirm.
4. **Statement-boundary commit** — always use `Db::begin → put → commit` even for autocommit one-ops? Lean yes for uniformity.
5. **Block cache backend** — `foyer` (default) vs `moka`. Lean foyer.
6. **Object store backends** — `aws` for prod, plus `InMemory` + `LocalFileSystem` (no feature) for tests. Confirm we limit prod to `aws` initially.
7. **Compression codec** — `zstd` default. Confirm.
8. **2PC strategy** — XA prepare → `flush_with_options(FlushType::Wal)` + system-CF marker; commit → marker flip. Confirm layered approach (SlateDB has no native XA).

## SlateDB version pinning (new)

The plugin's Cargo.toml currently pins `slatedb = "=0.13.1"` (crates.io). The
local checkout is `0.13.0` HEAD. Three options:

- **A**: Pin `=0.13.0` with path dep on `../slatedb/slatedb` during dev.
- **B**: Stay on crates.io `=0.13.1`.
- **C**: Pin a specific git rev with `features = ["aws", "compaction_filters", "zstd"]`.

Lean **C** for reproducibility. The doc's §0.3 already says "SlateDB pinned
to a specific git rev".

## Next move

1. Approve `_DESIGN.md` v2 (after addressing the 8 open questions) → I generate the remaining 127 stubs in one push.
2. Revise `_DESIGN.md` → I update exemplars + design, check back.
3. Spot-check an exemplar to validate the template style before approving the design.
