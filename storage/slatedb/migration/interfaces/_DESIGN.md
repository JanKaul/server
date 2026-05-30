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
**Not supported in Stage 0** (Q10 ruling 2026-05-29). SlateDB has no native
savepoint API and no write-batch truncate primitive. The three savepoint
handlerton hooks (`savepoint_set`, `savepoint_rollback`, `savepoint_release`)
all return `HA_ERR_WRONG_COMMAND`. MTR tests that use savepoints are excluded
from the Stage 0 suite via the same skip mechanism as other §1 non-goals.
Re-evaluated post Stage 1.

**Conflict-detection hooks** (`mark_read`/`unmark_write`): for SQL DML, we
call `txn.mark_read(...)` on every key returned by a non-`SELECT FOR UPDATE`
read in `SerializableSnapshot` mode. This is what lets the engine detect
read-write conflicts at commit time.

## 5.2 Concurrency model migration (pessimistic → optimistic)

**Ruling 2026-05-29: option (A) approved.** Embrace SSI; document and
deprecate the lock-related sysvars. The §5.2 action items list (below) is
now active work for TRANSLATE.

**This is the single largest behavioural shift in the engine and warrants
its own section.** Added in response to the interface-phase critique that
caught the gap in §5 above.

### The shift

MyRocks runs on RocksDB's `TransactionDB` with **pessimistic row locking**:
`SELECT ... FOR UPDATE` acquires a real X lock; competing writers block
(or deadlock-abort early); `innodb_lock_wait_timeout` is a meaningful knob.
See `storage/rocksdb/ha_rocksdb.cc:3395` (`GetForUpdate`), `:5647`
(`TransactionDB::Open`), `:3425-3427` (per-THD `lock_timeout` /
`deadlock_detect`).

SlateDB has **optimistic SSI** only (`IsolationLevel::Snapshot` or
`SerializableSnapshot`, per `transaction_manager.rs:16`). There is no
lock-manager extension point. `mark_read`/`unmark_write` track the
read/write sets used at commit-time conflict detection — they do not
block.

### What changes for users

| Surface | MyRocks today | SlateDB engine |
|---|---|---|
| `SELECT ... FOR UPDATE` | Acquires X lock; blocks competing writers | Returns immediately; conflict surfaces at commit on **whichever** txn commits second |
| `HA_ERR_LOCK_DEADLOCK` | "Server picked you as victim before you finished work" | "You ran the whole txn, then we threw it away at commit" — retry cost rotates from ms to "whatever the txn's work was" |
| `innodb_lock_wait_timeout` analog (`slatedb_lock_wait_timeout`) | Tunes lock-wait behaviour | **No-op** (no lock-wait path exists) |
| `deadlock_detect` / `deadlock_detect_depth` | Toggle the deadlock-detection graph traversal | **No-ops** |
| `information_schema.rocksdb_locks` | Held row locks per txn | Buffered writes per txn — schema preserved, semantics shifted (see `rdb_i_s_cc__lock_info.rs` for the explicit note) |
| `information_schema.rocksdb_deadlock` | History of deadlock cycles | History of commit-time SSI conflict victims (`path.len() == 1` typical) |
| Long-running contended writes | First writer to ask wins (early-victim semantics) | Last writer to commit may livelock under contention; needs application-level backoff |

### Why this matters NOW

Applications use `FOR UPDATE` in two distinct patterns:

1. **"Cheap-read, expensive-update" with serialization intent** — read
   a row, compute new value, write back, expecting that competing
   readers wait. Under SSI this gives correctness regressions: two
   competing increments can both commit (whichever races first wins),
   and the loser silently retries — but only if the application is
   coded for retry. Most aren't.
2. **"Pessimistic acquire" for queue dispatch** — `SELECT ... FOR
   UPDATE SKIP LOCKED` patterns to fan out work across consumers.
   Without locks, two consumers grab the same row and both succeed at
   commit. Application-level idempotency or SKIP-LOCKED-equivalent
   semantics aren't free.

Both patterns are common in MyRocks workloads. Migration without
disclosure leads to silent data corruption (pattern 1) or duplicate
work (pattern 2).

### Three options

#### (A) Embrace SSI; document and deprecate

- `slatedb_lock_wait_timeout` and `slatedb_deadlock_detect*` become
  read-only sysvars (visible but ignored), with `SHOW WARNINGS`
  emitting an informational note on session start.
- `FOR UPDATE` doc-explicitly maps to "mark in read-set; conflict at
  commit". Documented as a non-trivial semantic difference.
- `information_schema.rocksdb_locks` renamed (or kept with a clearly
  rewritten column doc string) — see option below for whether to
  rename or preserve.
- Retry guidance documented for `HA_ERR_LOCK_DEADLOCK` (now actually
  meaning "SSI conflict at commit").

Cost: documentation work + small sysvar deprecation. Application
authors carry the burden.

#### (B) Layer engine-side pessimistic locks on top of SSI

- Add a Rust-side lock manager (`Mutex<HashMap<KeyBytes, LockHolder>>`)
  consulted on every `FOR UPDATE` and every write. Honours
  `lock_wait_timeout` and per-THD `deadlock_detect`.
- On conflict: block, time out, or deadlock-abort early as MyRocks does.
- SSI conflict detection remains as a second safety net.

Cost: significant engineering — a real lock manager is ~1000+ LoC of
careful code with its own correctness story. Reintroduces the
complexity SlateDB deliberately omits. Memory overhead per active key.
Maintenance burden.

#### (C) Hybrid: session-selectable

- New session var `slatedb_concurrency_mode = optimistic | pessimistic`
  (default: optimistic).
- Pessimistic mode opts into option (B)'s machinery for the duration
  of the session.
- Default workloads pay no overhead; legacy workloads can opt back in.

Cost: implements both (A) and (B), plus the mode-selection plumbing.
The "best of both" framing but actually "all the costs of both".

### Recommendation

**Option (A).** *Ruled 2026-05-29.* Aligned with §1 of `SlateDB_storage_engine.md`:

> This migration is **not** a drop-in MyRocks replacement; it is a new
> engine that happens to share MyRocks' SQL semantics where feasible.
> [...] Behavioural parity with MyRocks on the MTR subset that does
> **not depend on RocksDB internals** (column families, merge operators,
> TTL compaction filters, ...).

The concurrency model **is** a RocksDB internal that users built on top
of. Per §1, we don't promise parity here. Adding pessimistic locks (B)
re-introduces machinery SlateDB deliberately avoids, and at scale
likely performs worse than SSI on the object-store substrate (lock
table contention plus the writes still cost the same).

### Action items (option (A) approved — active TRANSLATE work)

1. Add a `## 5.2 Concurrency` section to user-facing engine docs
   covering the table above.
2. Deprecate (but keep accepting) `slatedb_lock_wait_timeout` and
   `slatedb_deadlock_detect*`. Emit a one-time `SHOW WARNINGS`
   informational note per session if any of these are non-default.
3. `rdb_i_s_cc__lock_info.rs` already documents the semantic shift —
   verify the per-row note reaches the column documentation (DOCSTRING
   parameter on the `Column` declaration).
4. `rdb_i_s_cc__deadlock_info.rs` rename `MODE` semantics in doc
   comment.
5. `rdb_psi_h.rs` rename `STAGE_WAITING_ON_ROW_LOCK` to
   `STAGE_WAITING_ON_TXN_COMMIT` — **done in this patch round**.
6. The 5 cited sites in `ha_rocksdb_cc____free__error_helpers.rs`
   (mapping `ErrorKind::Transaction` → `HA_ERR_LOCK_DEADLOCK`) get a
   doc-comment note flagging the rotated retry semantics.
7. `FOR UPDATE` handler path: `txn.mark_read([key])` only; no lock
   acquisition call. The `get_for_update` method in the
   `RdbTransaction` trait collapses into `get + mark_read`.
8. Savepoint stack: **N/A.** Per Q10 ruling (also 2026-05-29) savepoints
   are stubbed as `HA_ERR_WRONG_COMMAND` for Stage 0.

### Status

**Open Question 9 resolved 2026-05-29 → option (A).** TRANSLATE units
that touch `FOR UPDATE`, lock I_S, deadlock I_S, and savepoints are now
unblocked on this axis. (Savepoints additionally unblocked by Q10 →
stub as `HA_ERR_WRONG_COMMAND`.)

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

Down from 12 to 8 in v2 because SlateDB removed several decisions;
expanded to 10 after the interface-phase critique surfaced
concurrency-model and 2PC-protocol gaps.

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
8. **2PC implementation strategy.** **RESOLVED 2026-05-29 → (8a)
   serialize-and-replay with swappable marker encoding.**

   SlateDB's `flush_with_options(Wal)` does NOT make a `DbTransaction`'s
   buffered writes durable — they live in-memory until commit.
   - **(8a) Serialize-and-replay** *(chosen)*: `prepare` serializes the
     txn's buffered writes into a marker key (going through the
     underlying `Db` and thus through the WAL with
     `await_durable=true`), then rolls back the in-memory txn. `commit`
     reads the marker, replays into a fresh txn, commits, deletes the
     marker. Recovery scans markers on startup and awaits the binlog
     coordinator's verdict per prepared xid.
   - **(8b) Commit-and-undo** *(rejected)*: `prepare` commits under a
     "prepared" flag; `rollback` writes tombstones; `commit` flips the
     flag. Wrong recovery semantic (prepared writes are visible to
     other readers between `prepare` and `commit`).

   **Swappable marker encoding requirement.** The TRANSLATE
   implementation MUST factor the marker into two pieces:

   ```
   fn marker_key(xid) -> Bytes        // strategy-dependent (see below)
   fn serialize_pending_ops(txn) -> Bytes  // strategy-independent
   ```

   Two strategies, selected at runtime by the binlog co-location flag:
   - **Local mode** (Stage 0/1; binlog is NOT SlateDB):
     `marker_key(xid) = b"xa_prepare:" || xid_bytes`.
     `prepare()` writes `db.put(marker_key(xid), serialize_pending_ops(txn))`
     with `await_durable=true`.
   - **Combined mode** (Stage 3+; SlateDB-as-binlog co-located):
     `marker_key` becomes a lookup into the binlog's `XA_PREPARE` chunk
     position (`binlog_meta:xa:<xid> → (file_no, offset)` — see §14.7).
     `prepare()` writes NOTHING of its own — the binlog's
     `binlog_write_xa_prepare` chunk already contains the serialized
     ops, written via the same SlateDB WAL fsync.

   Cost in Stage 0/1: ~10 lines of indirection for the helper. Payoff
   in Stage 3+: one fsync instead of two for XA prepare in co-located
   deployments — the same convergence the InnoDB-as-binlog feature
   demonstrates for InnoDB.

   Sub-rulings for (8a):
   - **Marker size cap**: split markers >4 MB across multiple keys
     (`xa_prepare:<xid>:<chunk_no>` + header at chunk 0 with total count).
     Matches SlateDB block-size sweetspot; avoids per-key memory pressure.
   - **Recovery idempotency**: replay is allowed to be re-executed —
     callers writing `merge` operands or counter ops through XA accept
     "may be applied more than once" (matches MyRocks' MERGE-under-crash
     behaviour). No per-marker progress tracking needed.
   - **Durability ordering**: the marker write completes with
     `await_durable=true` BEFORE `prepare()` returns to MariaDB.
     Otherwise the binlog could record "prepared" for a txn that hasn't
     reached object storage.
9. **Concurrency model migration (post-critique).** See §5.2.
   **RESOLVED 2026-05-29 → option (A).** Embrace SSI; deprecate the
   lock sysvars; document the semantic shift in user docs and in the
   affected I_S tables.
10. **Savepoint support in Stage 0.** SlateDB has no native savepoint
    API and `DbTransactionOps` exposes no write-batch truncate primitive.
    **RESOLVED 2026-05-29 → stub as `HA_ERR_WRONG_COMMAND`.** The three
    handlerton hooks (`savepoint_set`, `savepoint_rollback`,
    `savepoint_release`) all return `HA_ERR_WRONG_COMMAND` for Stage 0;
    MTR tests using savepoints are excluded from the Stage 0 suite.
    Re-evaluate after Stage 1: if savepoints are needed for parity, pick
    (A) replay-on-rollback (always-on op-log) or (B) abandon-and-restart
    (op-log only when a savepoint is set).

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

## 14. Binlog-engine role (SlateDB-as-binlog)

**Status:** Interface designed 2026-05-29. **Stage 2+ feature**, not part
of the Stage 0 data-engine cut. The interface stubs land now so the API
shape is committed and the v4 manifest reflects the future role.

### 14.1 Motivation

MariaDB 12.3+ introduced the `binlog_engine_hton` role
(`sql/handler.h:1603..1737`) which lets a storage engine host the
binlog. InnoDB ships an implementation under
`storage/innobase/handler/innodb_binlog.cc`; the public claim (article
"MariaDB Innovation: InnoDB-Based Binary Log") is that co-locating the
binlog with a transactional engine **halves the number of fsyncs**
because the binlog write rides on the same WAL fsync as the engine's
own commit. Empirical: 24,475 → 77,232 TPS at safe production settings.

SlateDB is structurally well-suited to this: a binlog is exactly a
log-structured append-only KV stream, which is SlateDB's core
competency. By implementing `binlog_engine_hton` we get the same
single-fsync win for SlateDB-data + SlateDB-binlog deployments.

### 14.2 Storage model — locked decisions

| Decision | Choice | Rationale |
|---|---|---|
| Storage location | Shared SlateDB `Db` instance with the data engine, distinct `binlog:` key prefix | Maximises the single-fsync co-located-binlog win |
| OOB shape | Chunked keys with sequential offsets (`binlog:<file_no>:<offset>`) | KV range scan in key order replaces InnoDB's Zeckendorf tree forest — no log-N seek needed on a KV substrate |
| File model | Virtual files = key-range partitions; `file_no` is a rotation counter | Rotation = bump counter; purge = `delete_range` on the old prefix |
| Interface scope | All 19 slots stubbed; user-XA paths Stage 2-stubbed | Internal 2PC works in Stage 1; user-XA paths return `HA_ERR_WRONG_COMMAND` until Stage 2 |
| GTID state | Inline `ChunkType::GtidState` chunks (InnoDB-style) | Preserves on-wire compatibility; one less keyspace to maintain |
| OOB cleanup | SlateDB native TTL on pre-commit chunks; commit-time WriteBatch re-puts without TTL | No GC pass; orphaned chunks from crashes self-expire |
| Durability | One `WriteBatch` per commit with `await_durable=true`, fsync at the tail of `binlog_group_commit_ordered` only | Matches the article's halve-the-fsyncs result |

### 14.3 Key encoding

```
key   = b"binlog:" || file_no_be8 || b":" || offset_be8        // 23 bytes fixed
value = chunk_header (3 bytes) || payload
         chunk_header[0] = chunk_type | CONT_flag | LAST_flag
         chunk_header[1..3] = u16 LE payload_len
```

`chunk_type` mirrors InnoDB's `fsp_binlog_chunk_types`
(`storage/innobase/include/fsp_binlog.h:66..86`): `Commit`, `GtidState`,
`OobData`, `Dummy`, `XaPrepare`, `XaComplete`, `Filler`. Preserved
bit-for-bit so a future tool could parse either engine's binlog.

Metadata lives in a separate prefix:
```
binlog_meta:rotation              → current active file_no
binlog_meta:file_index:<file_no>  → BinlogFileEntry-style metadata per active file
binlog_meta:xa:<xid>              → Stage 2 only: user-XA prepare markers
```

### 14.4 Module layout (10 interface stubs)

All under `storage/slatedb/migration/interfaces/`:

```
ha_slatedb_binlog_h__types.rs                    shared types (ChunkType, BinlogKey,
                                                  EngineDataPtr, BinlogEventGroupInfo,
                                                  BinlogXidInfo, BinlogPurgeInfo, ...)
ha_slatedb_binlog_h__reader.rs                   BinlogReader trait (the one vtable)

ha_slatedb_binlog_cc____free__lifecycle.rs       binlog_init, set_binlog_max_size
ha_slatedb_binlog_cc____free__write_direct.rs    binlog_write_direct[_ordered]
ha_slatedb_binlog_cc____free__group_commit.rs    binlog_group_commit_ordered  ← single fsync site
ha_slatedb_binlog_cc____free__oob_path.rs        oob_data[_ordered], savepoint_rollback,
                                                  oob_reset, oob_free
ha_slatedb_binlog_cc____free__xa_path.rs         xa_prepare[_ordered], xa_rollback[_ordered],
                                                  unlog  (all Stage 2 stubs)
ha_slatedb_binlog_cc____free__admin.rs           status, get_filename, get_binlog_file_list,
                                                  flush, get_init_state, reset, purge
ha_slatedb_binlog_cc____free__get_reader.rs      get_binlog_reader factory
ha_slatedb_binlog_cc__SlateDbBinlogReader.rs     concrete reader (impl BinlogReader)
```

Free functions throughout — the handlerton is literally a struct of
function pointers, so a trait would add a layer with no implementations
to vary. The only genuine vtable is `handler_binlog_reader` (the server
creates reader objects polymorphically per dump thread); we mirror it as
the `BinlogReader` trait.

### 14.5 Coordinator interaction summary (from `sql/log.cc` analysis)

**Normal commit path** (one fsync end-to-end when co-located with data):
1. `binlog_write_direct_ordered` under `LOCK_commit_ordered` — assign
   `(file_no, offset)`, stage chunks in a thread-local buffer.
2. `binlog_write_direct` no lock — write the staged chunks to SlateDB
   with `await_durable=false`.
3. Data engine's `commit_ordered` runs (also under `LOCK_commit_ordered`,
   also `await_durable=false`).
4. `binlog_group_commit_ordered` (tail entry only, no lock) — **one
   `flush_with_options(Wal)` with `await_durable=true`** — this is the
   single fsync covering the entire commit group, both binlog and data.
5. `binlog_unlog` — Stage 1 no-op (no XID marker needed for
   internal-2PC; see §14.6).

**OOB spill path** (large transactions):
- Each `binlog_oob_data*` call writes chunks under `binlog:<file>:<offset>`
  with `PutOptions::ttl = ExpireAfter(BINLOG_OOB_TTL_SECS)`.
- On commit, the commit-time WriteBatch re-puts the chunks without TTL
  (rationale: commit makes them permanent; pre-commit was conditional).
- On rollback, `binlog_savepoint_rollback` or `binlog_oob_reset` issues
  `Db::delete` on the staged keys; orphans from a crash expire naturally.

**Reader path**:
- `get_binlog_reader(wait_durable)` allocates one `SlateDbBinlogReader`.
- `wait_durable=true` (dump thread, crash-safe replication): reader
  refuses to emit data past `db.status().last_durable_seq`. Backed by a
  `tokio::sync::watch::Receiver<DbStatus>` subscription.
- `wait_durable=false` (`SHOW BINLOG EVENTS`): reader sees latest writeable seq.

### 14.6 User-XA — Stage 2 stub

The five user-XA slots (`binlog_write_xa_prepare[_ordered]`,
`binlog_xa_rollback[_ordered]`, `binlog_unlog`) are all returning
`Err(Error::invalid("non-goal: user-XA binlog (Stage 2)"))`. The
internal-2PC path (binlog ↔ data engine for normal commits) **does
not** go through these slots — it uses `binlog_write_direct*` only —
so stubbing user-XA does not break the headline use case.

`binlog_unlog` is a sync no-op in Stage 1 (it's the only slot called
for both internal-2PC and user-XA). Internal-2PC needs no marker
cleanup because we don't persist XID-keyed markers; the data engine's
own recovery covers the participant side.

Stage 2 user-XA design will need:
- Pre-commit serialization of the txn under `binlog_meta:xa:<xid>`.
- Recovery scan in `binlog_init` to populate `recover_xid_hash` with
  `BinlogXidInfo` per pending XID (incl. `engine_count` from the
  prepare record).
- A delete-on-commit for the marker via `binlog_unlog`.

### 14.7 XA-prepare convergence with data-engine Q8 (Stage 3+)

**Cross-reference to §11 Q8** (data-engine 2PC strategy, ruled →
(8a) serialize-and-replay with **swappable marker encoding**).

Q8's marker payload and §14's `XA_PREPARE` chunk payload are the same
thing: "serialized buffered writes for a pending XID, durable in
SlateDB's WAL." Today they live in different keyspaces because the
data engine and binlog engine are designed to be independently
deployable:

| Stage | Data engine prepare-marker | Binlog XA_PREPARE chunk |
|---|---|---|
| 0 / 1 (SlateDB-as-data only) | `xa_prepare:<xid>` in data keyspace | n/a (no SlateDB binlog) |
| 2 (SlateDB-as-data + SlateDB-as-binlog) | `xa_prepare:<xid>` in data keyspace | `binlog:<file>:<off>` with `ChunkType::XaPrepare` — **duplicate content** |
| 3+ (combined-mode optimization) | reads from binlog's XA_PREPARE chunk via `binlog_meta:xa:<xid> → (file_no, offset)` lookup | unchanged (single source of truth) |

The Q8 ruling mandates that the marker encoding be **swappable** —
the helper `marker_key(xid)` is the only thing that changes between
local mode (Stage 0/1/2) and combined mode (Stage 3+). The
`serialize_pending_ops(txn)` payload helper is mode-independent.

**Why Stage 2 stays in "duplicate content" mode rather than jumping
straight to combined-mode:** the Stage 2 binlog itself is the
first time `binlog_write_xa_prepare` is unstubbed (§14.6). Until
that path has run in production, the data engine cannot safely
rely on it for recovery. Stage 3+ is the cutover point once Stage
2 is proven.

**Why this design choice matters for engine perf:** in Stage 3+
combined mode, an XA `prepare()` does one SlateDB WAL fsync
covering BOTH the binlog's XA_PREPARE chunk AND the data engine's
prepare marker (because they're the same write). Stage 0/1/2 do
two fsyncs. This is the engine-side analog of the InnoDB-binlog
"halve the fsyncs" result — applied to the XA prepare ceremony,
not the normal commit path (which already benefits at Stage 2 via
`binlog_group_commit_ordered`).

**No INTERFACE changes needed.** Q8's `prepare()` body is
`todo!()` and §14's `binlog_write_xa_prepare` is also `todo!()` /
Stage 2 stub. The convergence happens inside those bodies at
TRANSLATE; the public API is unaffected. This subsection exists
to ensure the TRANSLATE author of either side knows the other side
exists and knows that `marker_key` indirection is load-bearing
for the eventual Stage 3+ optimization.

### 14.8 Open questions

1. **Concrete `BINLOG_OOB_TTL_SECS` default.** Tradeoff: too short and a
   slow-running large txn loses its OOB chunks; too long and a crash
   leaves stale data on object storage. Lean **1 hour** (matches typical
   `wsrep_max_ws_size` upper bounds for replication).
2. **`partial_chunk` re-entrance.** `SlateDbBinlogReader::read_binlog_data`
   stashes partial chunks for the next call. Is this thread-safe across
   the cxx boundary? The C++ caller is a single dump thread per reader,
   so yes — but the contract should be documented at the cxx shim level.
3. **Reader durable-seq subscription cost.** Each `SlateDbBinlogReader`
   subscribes to `DbMetadataOps::subscribe()` (when `wait_durable`). At
   N concurrent slaves we have N subscriptions. Slatedb watch channels
   are cheap; confirm at N=100+ slaves.
4. **Engine name for `binlog_storage_engine` sysvar.** Match the data
   engine plugin name? Or a separate `slatedb_binlog` plugin name? Lean
   "same plugin, both roles" — the handlerton resolution
   (`mysqld.cc:5667..5710`) wants ONE name resolving to ONE handlerton.
   If we want separability we need to register a second plugin that
   shares the same Db handle — more wiring.
5. **InnoDB-binlog co-existence.** If a deployment runs both InnoDB and
   SlateDB, only one can be `binlog_storage_engine`. SlateDB-as-binlog
   does NOT preclude InnoDB-as-data (or vice-versa); the binlog engine
   choice is independent. Document this in user docs.
6. **`set_binlog_max_size` in-flight semantics.** The C++ contract is
   under-documented for what happens to a partially-written file when
   the size shrinks. Lean: take effect at next rotation only.

