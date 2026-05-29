# INTERFACE phase — cross-cutting design decisions

**Status:** proposed. **Frozen if approved.** All per-unit interface stubs in
`migration/interfaces/<unit>.rs` conform to these decisions; changing any of
them after approval re-opens the TRANSLATE loop for affected units. Per §6 of
the doc, this is what the human reviews FIRST. Per-unit stubs are downstream.

## 1. SlateDB ↔ RocksDB feature map

The single most consequential design surface. Each row is a decision the
TRANSLATE loop must NOT relitigate.

| RocksDB feature | MyRocks usage | SlateDB strategy | Verdict |
|---|---|---|---|
| Column families | Per-index CF for hot/cold separation, custom compactors | **Key-prefix scheme** (varint `cf_id`). One SlateDB instance; no physical separation. | Map |
| Merge operators | `UpdateCounter` for `INSERT ... ON DUPLICATE KEY UPDATE` counter columns | **Rust-side RMW** in the write-batcher: GET → mutate → PUT. SlateDB has no merge. | Re-impl |
| Compaction filters | TTL row expiration, dropped-secondary-index sweep | **Background sweep task** (Tokio task on a periodic timer scanning by TTL prefix). NOT inline with compaction. | Re-impl |
| Read-Free Replication | Replication slave applies write-ops directly without read | **NOT SUPPORTED.** §1 non-goal. Returns `HA_ERR_WRONG_COMMAND`. | Non-goal |
| SST bulk loader | `LOAD DATA INFILE` fast path via direct SST write | **Buffered batch ingestion** through the write-batcher with elevated queue depth. No direct file write. | Re-impl |
| Snapshots | `db->GetSnapshot()` for repeatable-read txns | **SlateDB read-view at sequence number** (`db.read_view(seq)`). | Map |
| Iterators | `db->NewIterator(cf)` with prefix/upper/lower bounds | **SlateDB `scan(range)`** returning `impl Stream<Item = (Bytes, Bytes)>`. | Map |
| Two-phase commit | `Prepare(xid) → Commit(xid)` for XA | **SlateDB durable WAL fsync at prepare**, commit is metadata flip. | Map |
| Block cache | Shared `LRUCache`; per-CF block size tuning | **SlateDB built-in block cache** (single global). Per-CF tuning not exposed. | Map (degraded) |
| Bloom filters | Per-CF, configurable | **SlateDB built-in**, not per-CF tunable. | Map (degraded) |
| Compression | Per-CF (snappy/zstd/lz4) | **SlateDB built-in** (zstd by default). Per-CF override not supported. | Map (degraded) |
| Encryption-at-rest | RocksDB encrypted_env | **NOT SUPPORTED.** §1 non-goal. Defer to object-store-layer encryption (S3 SSE). | Non-goal |
| `myrocks_hotbackup` | Snapshot-based physical backup | **NOT SUPPORTED.** §1 non-goal. Backup is an object-store concern. | Non-goal |
| NoSQL access path | `nosql_access.cc` direct point-lookup bypass | **NOT SUPPORTED.** §1 non-goal. Stub returns `HA_ERR_WRONG_COMMAND`. | Non-goal |
| `information_schema` tables | 13 introspection tables | **Re-implemented** against SlateDB internals where the concept maps; others return empty rowset with a note. | Re-impl / Empty |
| `rdb_perf_context` | RocksDB perf counters | **SlateDB metrics** (Prometheus-exposable counters). Different shape; same SHOW STATUS surface. | Re-impl |

## 2. Key encoding (preserved from MyRocks)

Memcomparable encoding is preserved bit-for-bit. SlateDB sorts bytewise, so
the encoding correctness rules are identical to RocksDB's. The key format is:

```
key = cf_prefix || index_id || memcmp_key_bytes
cf_prefix = varint(cf_id)         // 1-5 bytes
index_id = u32 big-endian         // 4 bytes
memcmp_key_bytes = per-column memcomparable encoding (see Rdb_key_def)
```

Hidden PK (auto-generated rowid for tables without an explicit PK):

```
hidden_pk = varint(rowid)         // monotonic per-table
```

Why preserved: secondary-index key ordering must match value semantics for
range queries to return correct results. The MyRocks codec is correct and
battle-tested; reimplementing it would be both labor and a correctness risk.

## 3. Value encoding (preserved from MyRocks)

The TLV row format from `rdb_datadic.cc` is preserved. Each row value is:

```
row_value = checksum_byte || field_count_varint || (field_id_varint || field_value)*
```

Field values themselves use MyRocks' "storage format" (variable-length
encoding distinct from memcomparable, since values don't need to sort). This
is also preserved.

## 4. Error model

```rust
pub enum SlateError {
    /// Key not found. Maps to HA_ERR_KEY_NOT_FOUND / HA_ERR_END_OF_FILE.
    NotFound,
    /// Unique constraint violation. Maps to HA_ERR_FOUND_DUPP_KEY.
    KeyExists,
    /// Lock/conflict timeout. Maps to HA_ERR_LOCK_WAIT_TIMEOUT.
    LockTimeout,
    /// OCC conflict on commit. Maps to HA_ERR_LOCK_DEADLOCK.
    Conflict,
    /// I/O error (object store unreachable, etc.). Maps to HA_ERR_GENERIC.
    Io(String),
    /// Storage-level corruption. Maps to HA_ERR_CRASHED.
    Corruption(String),
    /// §1 non-goal hit (e.g., merge operator, bulk loader). Maps to HA_ERR_WRONG_COMMAND.
    NotSupported(&'static str),
    /// Internal invariant violation; should be unreachable in tested code paths.
    /// Maps to HA_ERR_INTERNAL_ERROR. Logged at error severity.
    Internal(String),
}
```

The shim translates `SlateError → HA_ERR_*` at every handler entry point. Rust
code returns `Result<T, SlateError>` uniformly. `unwrap()`/`expect()` are
forbidden in non-test code per the crate's `clippy::unwrap_used` lint.

## 5. Transaction model

```rust
pub struct TxnContext {
    pub txn_id: u64,          // SlateDB transaction handle
    pub isolation: Isolation, // ReadCommitted | RepeatableRead | Serializable
    pub read_view_seq: u64,   // SlateDB sequence number for snapshot reads
    pub savepoints: Vec<SavepointSnapshot>,
}
```

`TxnContext` lives in `THD`-scoped state on the C++ side (an opaque `Box<Txn>`
handle). Bridge functions take `&mut Txn` for mutating ops, `&Txn` for reads.

Savepoints are a Rust-side stack of `(write_batch_index, seq)` snapshots —
SlateDB has no native savepoint; rollback-to-savepoint discards write-batch
entries above the index.

## 6. Write-batching layer (§9 first-class unit)

**Per-connection** `WriteBatcher` aggregates writes within a transaction:

```rust
pub struct WriteBatcher {
    pending: Vec<WriteOp>,      // staged writes for this txn
    flush_threshold: usize,     // bytes; sysvar slatedb_batch_flush_bytes
    flush_interval: Duration,   // sysvar slatedb_batch_flush_interval_ms
}

pub enum WriteOp {
    Put(KeyBytes, ValueBytes),
    Delete(KeyBytes),
    Merge { key: KeyBytes, mutator: Box<dyn FnOnce(Option<&[u8]>) -> ValueBytes + Send> },
}
```

Why a first-class unit: per §9, p99 PUT latency on object storage is 50-100ms;
naive per-row writes do not meet OLTP SLAs. The batcher amortizes PUT cost
across all writes in a statement (or transaction, when isolation permits).
This is a deliberate design unit, NOT to be invented bottom-up by individual
TRANSLATE iterations.

The DML path goes: `write_row()` → `batcher.put(...)` → returns immediately.
`commit_txn()` → `batcher.flush()` → SlateDB `write_batch()` → fsync → return.

## 7. Async runtime

Per §3.2 of the doc:

- **One** `tokio::runtime::Runtime` per handlerton lifetime.
- Bridge functions are synchronous from C++'s view.
- Internally: submit work via bounded `mpsc::Sender` (sized by sysvar
  `slatedb_io_queue_depth`, default 4096); block on `oneshot::Receiver` for
  the result.
- On `mpsc` full → return `SlateError::LockTimeout` (maps to
  `HA_ERR_LOCK_WAIT_TIMEOUT`). Never block the handler thread indefinitely.
- **NEVER** `block_on` from inside a Tokio worker — re-entrant deadlock.

## 8. Module layout for `rust/src/`

The interface stubs in `migration/interfaces/<unit>.rs` map to crate modules
roughly as follows. This is the eventual TRANSLATE target structure; INTERFACE
stubs are standalone files first, then merged into the module tree per unit.

```
rust/src/
├── lib.rs                  # crate root, re-exports bridge
├── bridge.rs               # the cxx bridge ONLY — narrow surface
├── error.rs                # SlateError enum + HA_ERR_* mapping
├── runtime.rs              # Tokio runtime singleton + bounded channel plumbing
├── codec/                  # rdb_datadic translation
│   ├── mod.rs
│   ├── key.rs              # Rdb_key_def encode/decode/meta
│   ├── value.rs            # field packing / unpacking
│   ├── dict.rs             # Rdb_dict_manager
│   └── ddl.rs              # Rdb_ddl_manager, Rdb_tbl_def
├── engine/                 # the SlateDB engine wrapper + batcher
│   ├── mod.rs
│   ├── batcher.rs          # WriteBatcher (§6 of this doc)
│   ├── txn.rs              # Txn + TxnContext + Savepoint
│   ├── snapshot.rs         # read-view management
│   ├── cf.rs               # CF-id → key-prefix mapping
│   └── ttl.rs              # TTL sweep task
├── handler/                # ha_rocksdb method buckets, one module per v4 bucket
│   ├── mod.rs              # handler vtable trait
│   ├── lifecycle.rs        # open/close/external_lock/...
│   ├── ddl.rs              # create/delete_table/rename/...
│   ├── dml.rs              # write_row/update_row/delete_row/...
│   ├── scan.rs             # rnd_init/rnd_next/rnd_pos/...
│   ├── index.rs            # index_read/index_next/...
│   ├── info.rs             # info/records_in_range/table_flags/...
│   ├── alter.rs            # inplace alter
│   ├── txn.rs              # start_stmt/end_stmt/savepoint_*
│   ├── auto_incr.rs        # auto increment + hidden PK
│   ├── read.rs             # read_key_*/read_row_*/get_row_by_rowid
│   ├── iter_setup.rs       # SlateDB iterator construction
│   ├── ttl.rs              # TTL row filtering
│   ├── metadata.rs         # name/comment accessors
│   ├── write_path.rs       # update_write_*/delete_or_singledelete
│   ├── buffer.rs           # key buffer alloc
│   ├── key_compare.rs      # compare_keys/...
│   ├── table_mgmt.rs       # update_stats/truncate/...
│   ├── bulk_load.rs        # bulk_load_key/finalize_bulk_load
│   ├── repair.rs           # check/repair/analyze
│   ├── convert.rs          # convert_record_*
│   └── locks.rs            # check_*_allowed/build_decoder_*
├── sysvar/                 # rocksdb_show_* + rocksdb_set_* free fns
│   ├── mod.rs
│   ├── show.rs             # show_callbacks
│   └── set.rs              # sysvar_set + validators
├── plugin/                 # handlerton lifecycle + handler factory
│   ├── mod.rs
│   ├── init.rs             # rocksdb_init_func / done_func
│   ├── txn_handlers.rs     # commit/rollback/savepoint/...
│   └── cf_ops.rs           # admin (compact/flush/checkpoint)
├── utils/                  # leaf headers (atomic_stat, ut0counter, etc.)
│   └── ...
├── i_s/                    # information_schema tables (13)
│   └── ...                 # one module per table
└── misc/                   # unsplit small impls (mutex_wrapper, threads, etc.)
    └── ...
```

## 9. Stub file conventions

Each `migration/interfaces/<unit>.rs` file follows this template:

```rust
//! Interface stub for `<unit_id>`.
//!
//! C++ source: `storage/rocksdb/<file>.{cc,h}` (lines <start>..<end>)
//! v4 manifest sub-unit: `<sub-unit-id>` (if applicable)
//!
//! ## Mapping
//! - <one-line summary of what this unit does>
//! - <SlateDB strategy: which §1 of _DESIGN.md decision applies>
//!
//! ## Out-of-scope methods (returning HA_ERR_WRONG_COMMAND)
//! - <method name> — <which §1 non-goal>

use crate::error::SlateError;

// --- types ---
pub struct ExampleType {
    // doc: invariant on each field
}

// --- trait or free fns ---
pub trait ExampleTrait {
    /// Doc comment describing:
    /// - Inputs (with units/ranges/validity)
    /// - Outputs
    /// - Error conditions
    /// - Invariants preserved
    /// - Original C++ source line
    fn method(&self, arg: ArgType) -> Result<RetType, SlateError>;
}

// Implementations are bodied with `todo!("<short hint>")` or with explicit
// `Err(SlateError::NotSupported("<§1 non-goal>"))` for deliberately-unsupported features.
```

## 10. Per-unit dependency convention

A unit's interface file imports types from its dependencies, e.g.:

```rust
// in rdb_datadic_cc__Rdb_dict_manager.rs
use super::rdb_datadic_h__Rdb_dict_manager::DictManagerConfig;
use super::ha_rocksdb_h__ha_rocksdb::TableShareView;
```

These imports are aspirational at INTERFACE phase — the actual module wiring
happens during TRANSLATE. The imports document the dependency intent so the
human can verify the topology matches the manifest.

## 11. What this batch is NOT

- **Not implementations.** Every method body is `todo!()` or `Err(NotSupported(...))`.
  No real logic. No SlateDB calls. No real types beyond what the interface needs.
- **Not the cxx bridge.** The bridge in `rust/src/bridge.rs` is narrow and
  hand-written, NOT generated from these interfaces.
- **Not the final module structure.** Module-tree wiring happens in TRANSLATE.
- **Not MTR test design.** That's §8 / Stage 1.

## 12. Open questions (for human review)

These are decisions where I lean a particular way but want explicit human sign-off:

1. **CF-id → key-prefix scheme.** Currently I propose `varint(cf_id) || index_id_u32 || ...`.
   MyRocks uses a similar layout. Confirm we don't want a different prefix scheme.
2. **Compaction filter replacement.** Background sweep task on a timer is one approach;
   a per-write epoch check is another (cheaper per-op but tail-latency sensitive).
3. **Block-cache / bloom-filter / compression per-CF tuning** is dropped. Confirm.
4. **TTL precision.** MyRocks uses second-precision unix timestamps. Same?
5. **Write batcher policy.** When to auto-flush mid-statement vs only at commit?
   I lean: flush at statement boundary (for single-row autocommit) AND on commit.
6. **NoSQL access path.** Dropped per §1. Confirm — the file is `nosql_access.cc`,
   235 LoC, and dropping it means a small set of MariaDB-specific fast-path tests
   will fail with `HA_ERR_WRONG_COMMAND`. Acceptable?
