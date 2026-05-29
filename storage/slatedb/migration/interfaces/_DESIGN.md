# INTERFACE phase — cross-cutting design decisions

**Status:** v2 — rewritten 2026-05-29 after auditing the actual SlateDB source
at `../slatedb/` (workspace version 0.13.0, git HEAD `87c997c`). v1 made many
assumptions that turned out to be wrong: SlateDB has native merge operators,
compaction filters (feature-gated), transactions with isolation levels,
snapshots, write batches, prefix extractors, TTL, and status subscriptions.
**Most of the v1 "Re-impl" / "Non-goal" verdicts collapsed into "Map".**

This file is reviewed FIRST. Every per-unit stub conforms to it. Per §6 of
the doc, after approval the design is frozen for the TRANSLATE loop.

## 0. Verified SlateDB API surface (the facts this design is built on)

All from `../slatedb/slatedb/src/`:

- **`Db` builder** (`db.rs`, `db/builder.rs`):
  `Db::builder(path: Into<Path>, object_store: Arc<dyn ObjectStore>) -> DbBuilder`
  with `.with_settings(Settings)`, `.with_merge_operator(Arc<dyn MergeOperator>)`,
  `.with_compaction_filter_supplier(Arc<dyn CompactionFilterSupplier>)` (feature `compaction_filters`),
  `.with_block_cache(Arc<dyn DbCache>)`, `.with_compactor_builder(CompactorBuilder)`,
  `.build()`.
- **Operation traits** (`ops.rs`): `DbReadOps`, `DbWriteOps`, `DbTransactionOps`,
  `DbMetadataOps`, `DbCacheManagerOps`. These are the read/write/txn/meta API
  contracts implemented by `Db`/`DbReader`/`DbSnapshot`/`DbTransaction`.
- **Read API** (`DbReadOps`): `get`, `get_with_options(ReadOptions)`, `get_key_value`
  (returns `KeyValue { key, value, seq, create_ts, expire_ts }`), `scan(range)`,
  `scan_with_options(range, ScanOptions)`, `scan_prefix(prefix)`.
- **Write API** (`DbWriteOps`): `put`/`delete`/`merge` (each async, returns
  `WriteHandle`), `write(WriteBatch)`, `flush`/`flush_with_options(FlushOptions)`
  (FlushType: `Wal` or `MemTable`), `begin(IsolationLevel) -> DbTransaction`.
- **Txn API** (`DbTransactionOps`): `put`/`delete`/`merge` (sync, buffered),
  `mark_read(keys)` for SSI conflict detection, `unmark_write(keys)` to
  exclude keys from conflict checks, `seqnum()`, `id() -> Uuid`,
  `commit() -> Option<WriteHandle>`, `rollback()`.
- **Isolation levels** (`transaction_manager.rs:16`): `Snapshot` (default —
  write-write conflicts only) or `SerializableSnapshot` (SSI — write-write
  AND read-write conflicts, plus phantom-read detection via `read_ranges`).
- **Snapshots** (`db_snapshot.rs`): `Db::snapshot() -> Arc<DbSnapshot>` —
  read-only handle frozen at a sequence number. Implements `DbReadOps`.
- **Iterators** (`db_iter.rs`): `DbIterator::next() -> Option<KeyValue>`,
  `next_entry() -> Option<RowEntry>` (raw with tombstones), `seek(key)`.
  `IterationOrder::{Ascending, Descending}` is supported via
  `ScanOptions::with_order` — **reverse scans are native.**
- **Write batch** (`batch.rs`): `WriteBatch` — atomic batch of put/delete/merge.
  `Db::write(batch)` commits it.
- **Merge operator** (`merge_operator.rs:68`): `trait MergeOperator { fn merge(...); fn merge_batch(...); }`.
  Receives `key: &Bytes, existing_value: Option<Bytes>, operand: Bytes`.
  **Key-aware merge routing is supported** (e.g., one operator can sum for
  `sum:*` keys and concat for `list:*` keys — see the `KeyPrefixMergeOperator`
  test in merge_operator.rs:857).
- **Compaction filter** (`compaction_filter.rs`, feature `compaction_filters`):
  `trait CompactionFilter` with `async fn filter(&mut self, entry: &RowEntry)
  -> CompactionFilterDecision::{Keep, Drop, Modify(ValueDeletable)}`. Plus a
  `CompactionFilterSupplier` factory that's called per compaction job.
- **TTL** (`config.rs:553`): native — `PutOptions::ttl: Ttl` with variants
  `Default | NoExpiry | ExpireAfter(u64 seconds) | ExpireAt(i64 unix_ts)`.
  Each `KeyValue` / `RowEntry` carries `expire_ts: Option<i64>`. **No external
  TTL filter needed** — SlateDB expires entries internally.
- **Bloom filters** (`config.rs:645`): `Settings::min_filter_keys` controls
  the per-SST threshold. `FilterPolicy` (`filter_policy.rs`) is pluggable —
  you can supply a custom filter policy with prefix-aware behavior via
  `PrefixExtractor`.
- **Compression** (`config.rs:1032`): `CompressionCodec` enum (snappy / zlib
  / lz4 / zstd, each feature-gated). Configured in `Settings`.
- **Block cache**: pluggable via `DbBuilder::with_block_cache(Arc<dyn DbCache>)`.
  Backends: `foyer` (default) or `moka` (optional feature).
- **Object store** (`Cargo.toml` features): `aws` (default), `azure`, `gcp`,
  `opendal`. Plus `object_store::memory::InMemory` and
  `object_store::local::LocalFileSystem` from the upstream crate — both
  usable for tests/dev without a real cloud backend.
- **DB status subscription** (`DbMetadataOps::subscribe`): returns a
  `tokio::sync::watch::Receiver<DbStatus>` that tracks `durable_seq` and the
  current manifest snapshot. **Replaces the MyRocks
  `Rdb_event_listener::OnFlushCompleted` / `OnCompactionCompleted` pattern**.
- **Read durability filter** (`config.rs:257`): `DurabilityLevel::{Memory, Remote}`.
  `Remote` reads only data durably in object storage (skips memtable); `Memory`
  is the default. `ReadOptions::dirty: bool` enables read-uncommitted.
- **Write durability** (`config.rs:451`): `WriteOptions::await_durable: bool`
  (default `true`). When `false`, `put` returns as soon as the WAL has the
  entry queued — no fsync wait.
- **Flush control** (`config.rs:418`): explicit `FlushType::Wal` or `MemTable`
  flush via `flush_with_options(FlushOptions)`.
- **Checkpoints** (`db.rs`): `Db::create_checkpoint(opts) -> CheckpointCreateResult`.
  Used for clone/backup and for crash-safe Stage 1 testing.
- **Error model** (`error.rs`): public `slatedb::Error` with `ErrorKind` enum
  — only **6 categories**:
  - `Transaction` — txn conflict; must retry/drop.
  - `Closed(CloseReason)` — DB shut down (Clean / Fenced / Panic).
  - `Unavailable` — I/O / object-store error; retry/drop.
  - `Invalid` — bad config/argument; user must fix.
  - `Data` — persisted corruption / inconsistent state.
  - `Internal` — slatedb bug; users should report.
  Internal `SlateDBError` (in `error.rs:18`) is the rich variant set; the
  public `Error` collapses it.
- **Key/value types** (`types.rs`): `KeyValue { key: Bytes, value: Bytes, seq: u64,
  create_ts: i64, expire_ts: Option<i64> }`. `RowEntry` is the storage-side
  variant whose `value: ValueDeletable::{Value(Bytes) | Merge(Bytes) | Tombstone}`.

## 1. SlateDB ↔ RocksDB feature map (corrected)

| RocksDB feature | MyRocks usage | SlateDB strategy | Verdict |
|---|---|---|---|
| Column families | Per-index CF for separation + per-CF compactors | **Key-prefix scheme** with native `PrefixExtractor` for SST-filter optimization. One SlateDB instance; CF id encoded as key prefix. | Map (native support) |
| Merge operators | `UpdateCounter` etc. | **`MergeOperator` trait, registered once at `Db::builder.with_merge_operator(...)`.** Key-aware routing (sum/max/concat by prefix) supported. | Map (native) |
| Compaction filters (TTL) | Drop expired rows during compaction | **`CompactionFilter` trait (feature `compaction_filters`)** — `filter()` returns `Keep`/`Drop`/`Modify`. Plus SlateDB's native `expire_ts` on entries handles TTL without a filter. | Map (native) |
| Compaction filters (drop-secondary-index) | Sweep an index marked for deletion | **Same `CompactionFilter` mechanism** — filter returns `Drop` for entries matching the dropped index prefix. | Map (native) |
| Read-Free Replication | Apply write-ops on slave without read | **NOT SUPPORTED.** §1 non-goal. Returns `HA_ERR_WRONG_COMMAND`. | Non-goal |
| SST bulk loader | `LOAD DATA INFILE` writes SSTs directly | **`WriteBatch` with high `flush_interval`** — buffer all rows then one `Db::write(batch)`. Not as fast as direct SST write but adequate for our workload. | Map (degraded) |
| Snapshots | `db->GetSnapshot()` for repeatable-read | **`Db::snapshot() -> Arc<DbSnapshot>`** — frozen-seq read view. | Map (native) |
| Iterators | `NewIterator(cf)` with bounds | **`Db::scan(range)` / `scan_prefix(prefix)`** returning `DbIterator`. Reverse via `IterationOrder::Descending`. | Map (native) |
| Two-phase commit | `Prepare(xid) → Commit(xid)` | **No native XA in SlateDB.** We layer 2PC in our engine: prepare flushes WAL (via `flush_with_options(FlushType::Wal)`); commit writes a metadata marker. | Re-impl (thin) |
| Block cache | Shared LRUCache, per-CF tuning | **`DbBuilder::with_block_cache(Arc<dyn DbCache>)`** — foyer (default) or moka. Global, not per-CF. | Map (degraded) |
| Bloom filters | Per-CF, configurable | **`Settings::min_filter_keys` + `FilterPolicy` plus `PrefixExtractor`** — single global policy, but prefix-aware. | Map (degraded) |
| Compression | Per-CF (snappy/zstd/lz4) | **`Settings.compression_codec: CompressionCodec`** — single global codec. | Map (degraded) |
| Encryption-at-rest | RocksDB encrypted_env | **Object-store-level encryption** (S3 SSE-KMS) configured at the `object_store` layer, not in SlateDB. Inherits whatever the object store backend supports. | Map (degraded) |
| `myrocks_hotbackup` | Physical backup via SST snapshots | **`Db::create_checkpoint`** produces a manifest checkpoint usable as a backup root. Plus object-store-level snapshot tools (S3 bucket replication). | Map (different shape) |
| NoSQL access path | `nosql_access.cc` direct point-lookup bypass | **NOT SUPPORTED.** §1 non-goal. Stub returns `HA_ERR_WRONG_COMMAND`. | Non-goal |
| `information_schema` tables (13) | MyRocks introspection | **Re-implemented** against SlateDB internals where the concept maps (e.g., `compact_stats`, `sst_props`, `lock_info`, `trx_info`); others return empty rowset with a note. SlateDB's `DbStatus` + `VersionedManifest` provide most data. | Re-impl |
| `rdb_perf_context` | RocksDB perf counters | **SlateDB metrics** (Prometheus-exposable via `slatedb_common::metrics`). Different shape; same SHOW STATUS surface. | Re-impl |
| RocksDB event listener (`Rdb_event_listener`) | Updates index stats on flush/compaction | **`DbMetadataOps::subscribe()` returns a `watch::Receiver<DbStatus>`** — replaces the callback pattern with a pull/poll/await model that's more Rust-idiomatic. | Map (different shape) |
| WAL | Per-write fsync | **SlateDB WAL** with configurable `flush_interval` and `WriteOptions::await_durable`. Can be disabled entirely with feature `wal_disable`. | Map (native) |

## 2. Key encoding (preserved from MyRocks, leverages SlateDB PrefixExtractor)

Memcomparable encoding preserved bit-for-bit. SlateDB sorts bytewise; the MyRocks
encoding correctness rules are identical to RocksDB's.

```
key = cf_prefix || index_id || memcmp_key_bytes
cf_prefix = varint(cf_id)         // 1-5 bytes
index_id = u32 big-endian         // 4 bytes
memcmp_key_bytes = per-column memcomparable encoding (see Rdb_key_def)
```

**SlateDB integration:** We register a `PrefixExtractor` that extracts
`varint(cf_id) || u32_be(index_id)` (a CF+index pair). This enables SST-level
bloom filters to be prefix-aware — point lookups within an index pre-filter
candidate SSTs efficiently.

Hidden PK (auto-generated rowid):
```
hidden_pk = varint(rowid)         // monotonic per-table
```

Reverse-ordered indexes (MyRocks `Rdb_rev_comparator`) work via two mechanisms:
- **Scan-time**: use `ScanOptions::with_order(IterationOrder::Descending)` for ad-hoc
  reverse scans. No encoding change needed; SlateDB handles it.
- **Index-side**: for indexes declared reverse-ordered at CREATE TABLE time, the
  codec XORs the memcomparable bytes with `0xff` so byte-lexicographic order
  produces the desired reverse semantic order. This is what MyRocks does and
  preserves the "stored-bytes already in scan order" invariant.

## 3. Value encoding (preserved from MyRocks)

MyRocks TLV row format preserved per column:

```
row_value = checksum_byte || field_count_varint || (field_id_varint || field_value)*
```

**Native SlateDB `expire_ts` replaces the MyRocks "TTL prefix bytes"** —
instead of embedding the TTL timestamp in the value, we set `PutOptions.ttl`
on the write. SlateDB stores it in `RowEntry.expire_ts` natively, and entries
past their expiry are filtered by SlateDB's iterators. **This removes the
need for the MyRocks `compaction_filter` TTL sweep entirely.**

## 4. Error model (uses `slatedb::Error` natively)

We do NOT invent a `SlateError` enum. The crate exposes `slatedb::Error` with
a 6-variant `ErrorKind`. Our shim translates `slatedb::Error → HA_ERR_*` at
each handler entry point:

```rust
fn slatedb_error_to_ha_err(e: &slatedb::Error) -> i32 {
    use slatedb::ErrorKind::*;
    match e.kind() {
        Transaction => HA_ERR_LOCK_DEADLOCK,        // retry-able conflict
        Closed(_)   => HA_ERR_CRASHED,              // DB shut down
        Unavailable => HA_ERR_LOCK_WAIT_TIMEOUT,    // I/O / object store
        Invalid     => HA_ERR_GENERIC,              // bad arg from us — should be unreachable
        Data        => HA_ERR_CRASHED,              // persisted corruption
        Internal    => HA_ERR_INTERNAL_ERROR,
    }
}
```

`Result<T, slatedb::Error>` is the unified return type across our Rust code.
`unwrap()`/`expect()` are forbidden in non-test code (existing crate lint).

For our engine-specific errors (codec mismatch, hidden-PK row-id exhaustion,
etc.) we layer a `MyRocksError` enum that converts INTO `slatedb::Error` via
`Error::invalid(msg)` or `Error::data(msg)`.

## 5. Transaction model (uses `DbTransaction` + `IsolationLevel`)

```rust
// per-statement handler entry
let txn = engine.db().begin(IsolationLevel::Snapshot).await?;
txn.put(key, value)?;        // buffered to write batch
// ...
let commit_handle = txn.commit().await?;  // Some(WriteHandle) or None if empty
```

**Isolation level mapping:**

| MariaDB SQL level | SlateDB level | Notes |
|---|---|---|
| `READ UNCOMMITTED` | `Snapshot` + `ReadOptions::dirty=true` | Dirty reads via the read-uncommitted flag |
| `READ COMMITTED` | `Snapshot` | SI without phantom-read protection |
| `REPEATABLE READ` | `Snapshot` | SI's per-txn snapshot already gives RR |
| `SERIALIZABLE` | `SerializableSnapshot` | SSI with read-set tracking + phantom-read detection |

**Savepoints** (MariaDB `SAVEPOINT name` / `ROLLBACK TO SAVEPOINT name`):
SlateDB has no native savepoint API. We layer them in our engine as a Rust-side
stack of `(write_batch_position, mark_read_set_snapshot)` checkpoints. Rollback
to savepoint discards write-batch entries above the position and shrinks the
read-set back to the snapshot.

**Conflict-detection hooks** (`mark_read`/`unmark_write`): for SQL DML, we
call `txn.mark_read(...)` on every key returned by a non-`SELECT FOR UPDATE`
read in `SerializableSnapshot` mode. This is what lets the engine detect
read-write conflicts at commit time.

## 6. Write-batching layer (per §9 of doc, simpler than v1)

SlateDB has `WriteBatch` natively. Our role is narrower than v1 envisioned:

- **Per-statement aggregator** — buffer the DML statement's individual writes
  into one `WriteBatch`, then `txn.commit()` (which writes the batch atomically).
- **Per-transaction aggregator** — when isolation > READ_COMMITTED, buffer
  across statements into a single `WriteBatch`; flush on `txn.commit()`.

Auto-flush is unnecessary because SlateDB already has `flush_interval` and
`await_durable` controls — we expose those as sysvars but the engine itself
just uses `txn.put` and lets SlateDB handle batching internally for the WAL.

**Open question 4:** at the statement boundary (autocommit), do we use
`Db::put` directly (no explicit batch) or always go through `Db::begin → put → commit`?
Lean: always-txn for uniformity, since `commit` is the only point that
returns conflict errors and a single-op txn has minimal overhead.

## 7. Async runtime

Per §3.2 of the doc:

- **One** `tokio::runtime::Runtime` per handlerton lifetime (`OnceLock<Runtime>`).
- Bridge functions are synchronous from C++; internally they
  `runtime.block_on(future)` on the handler thread.
- `slatedb_io_threads` sysvar sizes the runtime worker pool (default `num_cpus / 2`).
- `slatedb_io_queue_depth` sizes an `mpsc::Sender` for queue-depth limiting
  on the hot path; on full queue → return `slatedb::ErrorKind::Unavailable`
  (mapped to `HA_ERR_LOCK_WAIT_TIMEOUT`).
- **Never** `block_on` from inside a Tokio worker — re-entrant deadlock.
  The shim's `block_on` happens on the *handler* thread (outside the runtime).

`async fn` in our interface stubs is implementation detail of the Rust side.
The cxx bridge surface is all synchronous.

## 8. Module layout for `rust/src/`

```
rust/src/
├── lib.rs                  # crate root, re-exports bridge
├── bridge.rs               # the cxx bridge ONLY — narrow surface
├── error.rs                # slatedb::Error → HA_ERR_* mapping; MyRocksError enum
├── runtime.rs              # Tokio runtime singleton + bounded channel plumbing
├── codec/                  # rdb_datadic translation
│   ├── mod.rs
│   ├── key.rs              # Rdb_key_def encode/decode/meta (uses bytes::Bytes)
│   ├── value.rs            # field packing / unpacking
│   ├── dict.rs             # Rdb_dict_manager — backed by SlateDB SYSTEM CF prefix
│   ├── ddl.rs              # Rdb_ddl_manager, Rdb_tbl_def
│   └── prefix.rs           # PrefixExtractor implementation for CF+index lookups
├── engine/
│   ├── mod.rs
│   ├── db.rs               # SlateDB Db wrapper + builder
│   ├── txn.rs              # DbTransaction wrapper + Savepoint stack
│   ├── snapshot.rs         # DbSnapshot wrapper
│   ├── merge.rs            # MyRocks-style merge operator (counter / list)
│   ├── filter.rs           # CompactionFilter for dropped-index sweep
│   ├── stats_task.rs       # subscribes to DbStatus, updates index stats cache
│   └── cf.rs               # CF-id → key-prefix mapping
├── handler/                # ha_rocksdb method buckets (one module per v4 bucket)
│   └── ...                 # as in v1 _DESIGN.md
├── sysvar/                 # rocksdb_show_* + rocksdb_set_* free fns
├── plugin/                 # handlerton lifecycle + handler factory
├── utils/                  # leaf headers
├── i_s/                    # 13 information_schema tables, one module each
└── misc/
```

## 9. Stub file conventions

```rust
//! Interface stub for `<unit_id>`.
//!
//! C++ source: `storage/rocksdb/<file>.{cc,h}` (lines <start>..<end>)
//! v4 manifest sub-unit: `<sub-unit-id>` (if applicable)
//!
//! ## Mapping
//! - <one-line summary of what this unit does>
//! - <SlateDB strategy: which decision in _DESIGN.md §1 applies>
//!
//! ## Out-of-scope methods (returning HA_ERR_WRONG_COMMAND)
//! - <method name> — <which §1 non-goal>

use slatedb::Error;
// optionally: use bytes::Bytes; use slatedb::{Db, DbTransaction, ...};

pub trait ExampleTrait {
    /// Doc comment describing:
    /// - Inputs (with units/ranges/validity)
    /// - Outputs
    /// - Error conditions (which slatedb::ErrorKind on failure)
    /// - Invariants preserved
    /// - Original C++ source line
    async fn method(&self, arg: ArgType) -> Result<RetType, Error>;
}

// Implementations are bodied with `todo!("<short hint>")` or with
// `Err(slatedb::Error::invalid("non-goal: <feature>".into()))` for
// deliberately-unsupported features.
```

## 10. What this batch is NOT

- **Not implementations.** Method bodies are `todo!()` or explicit non-goal returns.
- **Not the cxx bridge.** The bridge in `rust/src/bridge.rs` stays narrow and hand-written.
- **Not the final module wiring.** Module-tree assembly happens in TRANSLATE.
- **Not MTR test design.** That's §8 / Stage 1.

## 11. Open questions (reviewer should rule on)

Down from 12 to 8 because SlateDB removed several decisions:

1. **CF-id → key-prefix layout.** `varint(cf_id) || u32_be(index_id) || ...`
   matches MyRocks and feeds cleanly into `PrefixExtractor`. Confirm.
2. **CompactionFilter for dropped-index sweep.** Use SlateDB's
   `CompactionFilter` (feature `compaction_filters`) — adds the feature flag
   to Cargo.toml. Confirm we enable the feature.
3. **TTL precision.** SlateDB's `Ttl::ExpireAfter(u64)` is seconds; `ExpireAt(i64)`
   is unix epoch. MyRocks uses seconds. Match.
4. **Single-op txn vs direct `Db::put` at statement boundary** (see §6).
   Lean: always-txn for uniformity. Confirm.
5. **Block cache backend.** `foyer` (default in slatedb) vs `moka` (optional
   feature). Lean: foyer. Confirm.
6. **Object store backend(s) to enable.** Slatedb features: `aws` (default),
   `azure`, `gcp`, `opendal`. For Stage 0 testing we use `InMemory` (no
   feature) and `LocalFileSystem` (no feature). For prod we enable `aws`.
   Confirm we limit to `aws` initially.
7. **Compression codec.** `Settings.compression_codec` default. Lean: `zstd`
   (feature `zstd`). Confirm.
8. **2PC implementation strategy.** XA prepare → SlateDB `flush_with_options(Wal)`
   plus a metadata marker in our system CF. Commit → marker flip. Confirm
   this layered approach (no native SlateDB XA primitive exists).

## 12. SlateDB version pinning

The plugin's `Cargo.toml` currently pins `slatedb = "=0.13.1"`. The workspace
in `../slatedb/` is at `0.13.0` (HEAD `87c997c`). Three options:

- **A:** Pin to `0.13.0` (matches user's local checkout) and depend on the
  *path* `../slatedb/slatedb` during development, swap to crates.io for prod.
- **B:** Pin to crates.io `=0.13.1` (the published version). Have to read
  0.13.1's actual code if it differs.
- **C:** Pin to a specific git rev of `slatedb/slatedb` for reproducibility,
  with feature flags spelled out.

Lean **C** — pinned git rev, with `features = ["aws", "compaction_filters", "zstd"]`.
This is documented in the doc's §0.3 as "SlateDB pinned to a specific git rev".

## 13. Stage 0 substrate-test implications

§9 of the doc requires verifying p99 PUT latency before INDEX. With SlateDB's
native `flush_interval` knob (default in `Settings`), the test should:

- Use `LocalFileSystem` object store (no minio dependency) for the Stage 0
  Level A latency check. Or `InMemory` for "this is a substrate" smoke test.
- For prod-like Level C: minio is still the right comparator since
  object-store-tier latency is what matters at the §1 escalation threshold.
