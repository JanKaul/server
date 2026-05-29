# INTERFACE phase — critique

**Reviewer:** independent agent, 2026-05-29
**Scope:** 132 per-unit stubs in `storage/slatedb/migration/interfaces/*.rs`
plus `_DESIGN.md` v2, read cold against the MyRocks engine in
`storage/rocksdb/` and the design's own claims about SlateDB 0.13.1.

## Headline

The design has done real work — `_DESIGN.md` v2 is honest about what
SlateDB provides natively, drops v1's over-cautious "Re-impl" verdicts
in favor of "Map (native)" where the API exists, and produces stubs that
trace cleanly back to specific C++ line numbers. The exemplars
(`atomic_stat_h.rs`, `event_listener_h.rs`, `rdb_global_h.rs`,
`rdb_comparator_h.rs`) are tight and well-justified.

**But the design is silent on the single biggest semantic shift it
proposes** — replacing MyRocks' RocksDB-`TransactionDB` pessimistic
locking with SlateDB's optimistic SSI — and the stub set has structural
defects (duplicate type declarations, async-vs-sync trait confusion,
unresolved imports) that will cascade through TRANSLATE if not fixed at
the design-freeze gate. Six findings rise to severity P1 (blocks
TRANSLATE), five to P2 (will cause rework). I would not greenlight
TRANSLATE in the current state.

---

## P1 — concurrency model shift is undocumented and probably user-visible

This is the most consequential finding. MyRocks runs on RocksDB's
`TransactionDB` with **pessimistic locking**:

- `storage/rocksdb/ha_rocksdb.cc:3395` — every `SELECT ... FOR UPDATE`
  takes a real X lock via `m_rocksdb_tx->GetForUpdate(...)`.
- `storage/rocksdb/ha_rocksdb.cc:3425-3427` — per-statement
  `lock_timeout` and `deadlock_detect` / `deadlock_detect_depth`
  configured per-THD.
- `storage/rocksdb/ha_rocksdb.cc:5647` — `TransactionDB::Open(...)`
  globally.

The design proposes replacing this with SlateDB's SSI:

- `_DESIGN.md` §5 maps `SERIALIZABLE → SerializableSnapshot` and
  treats `mark_read` as "what lets the engine detect read-write
  conflicts at commit time."
- `ha_rocksdb_cc__Rdb_transaction.rs:111-117` (`get_for_update`):
  "in `Snapshot` mode we just call `get` (no lock taken — MyRocks'
  pessimistic lock has no SlateDB analogue)."
- `rdb_mutex_wrapper_cc.rs:14-16`: "SlateDB has no equivalent
  lock-manager extension point — its transactions are SSI/SI, no
  row-level locks."

The stubs acknowledge the mismatch in scattered places, but the design
document never confronts the user-visible consequences:

1. **`SELECT ... FOR UPDATE` no longer blocks competing writers.**
   Under MyRocks, a `FOR UPDATE` lock causes other writers to wait
   (and possibly time out / deadlock-abort). Under the proposed
   SlateDB-SSI mapping, the read returns immediately and the conflict
   surfaces only at commit time on **whichever** transaction commits
   second. The first transaction to commit always wins. Applications
   that use `FOR UPDATE` to serialize work (the dominant motivation
   for using it) will see correctness regressions, not just
   performance ones.

2. **`HA_ERR_LOCK_DEADLOCK` semantics rotate.** Today this means "the
   server picked you as a deadlock victim before you finished". Under
   SSI it means "you successfully ran the whole transaction, then we
   threw it away at commit." The retry cost rotates from milliseconds
   to whatever the transaction's full work was. Long-running
   transactions with high contention will livelock or starve.

3. **`HA_ERR_LOCK_WAIT_TIMEOUT` is degraded to "I/O timeout".**
   `ha_rocksdb_cc____free__error_helpers.rs:79` maps
   `slatedb::ErrorKind::Unavailable → HA_ERR_LOCK_WAIT_TIMEOUT`.
   That's not a lock-wait timeout; it's an object-store reachability
   timeout. The `innodb_lock_wait_timeout`-analog sysvar
   (`slatedb_lock_wait_timeout` mentioned in
   `Rdb_transaction_impl.rs:67`) controls nothing on the SlateDB
   side. Users tuning this knob will be tuning something the engine
   ignores.

4. **The `deadlock_detect` / `deadlock_detect_depth` THDVARs become
   no-ops.** `rdb_i_s_cc__deadlock_info.rs:19,26` admits this, but
   the I_S table still exists with the same columns; users will see
   empty results and not know whether that means "no deadlocks" or
   "no detection."

5. **`information_schema.rocksdb_locks` is a fiction.**
   `rdb_i_s_cc__lock_info.rs:10-21` reports "the write set of each
   active transaction as `MODE = 'X'` rows". But these aren't held
   locks — they're buffered writes that no other transaction can see
   yet. The semantic is fundamentally different from what the column
   name promises. Operators using this table to debug contention will
   be misled.

The design needs an explicit §5.2 "Concurrency model migration" that
either (a) defers MyRocks parity to a future phase and documents the
gap loudly, (b) layers a pessimistic-lock manager on top of SlateDB
SSI in the engine, or (c) deprecates the lock-related sysvars and
table layouts. None of those choices is in v2.

---

## P1 — `HaSlateDb` declared 11 times with inconsistent shapes

`grep "pub struct HaSlateDb"` across `interfaces/*.rs` returns 11
files:

- `ha_rocksdb_h__ha_rocksdb.rs:113` — canonical 30+ field record.
- `ha_rocksdb_cc__ha_rocksdb__lifecycle.rs:101` — `{ _private: () }`.
- `ha_rocksdb_cc__ha_rocksdb__ttl.rs:40` — unit struct.
- `ha_rocksdb_cc__ha_rocksdb__auto_incr.rs:31` — unit struct.
- `ha_rocksdb_cc__ha_rocksdb__buffer.rs:36` — unit struct.
- `ha_rocksdb_cc__ha_rocksdb__iter_setup.rs:37` — unit struct.
- `ha_rocksdb_cc__ha_rocksdb__bulk_load_helpers.rs:35` — unit struct.
- `ha_rocksdb_cc__ha_rocksdb__metadata.rs:29` — unit struct.
- `ha_rocksdb_cc__ha_rocksdb__error.rs:27` — unit struct.
- `ha_rocksdb_cc__ha_rocksdb__read.rs:33` — unit struct.
- `ha_rocksdb_cc__ha_rocksdb__write_path.rs:41` — unit struct.

The hub stub (line 109-110) explicitly says "All methods on this struct
are declared in the v4 sub-unit files; this struct is the shared state
they manipulate." The intent is one struct + ten `impl` blocks. What
shipped is eleven incompatible types, each with `impl HaSlateDb { ... }`
attaching methods to a different definition. At module-tree assembly
time you get either ten duplicate-definition errors, or — worse — every
sub-unit's `self.lock_rows` / `self.scan_it` / `self.retrieved_record`
field access fails because the placeholder is a unit struct.

**Suggested fix:** delete the ten placeholders, replace with `use
crate::ha_rocksdb_h__ha_rocksdb::HaSlateDb;`. Pattern already used
correctly in `ha_rocksdb_cc__ha_rocksdb__txn.rs` (which is not in the
grep results).

---

## P1 — `UpdateRowInfo` defined three times with divergent fields

- `ha_rocksdb_h__update_row_info.rs:29` — `UpdateRowInfo<'a>` with
  borrowed `tx`, `new_data`, `old_data`, `new_pk_unpack_info`, etc.
  Holds a borrow of `crate::ha_rocksdb_cc__Rdb_transaction::Txn` — a
  type that **does not exist** in `Rdb_transaction.rs` (which defines
  the trait `RdbTransaction`, not `Txn`).
- `ha_rocksdb_cc__ha_rocksdb__write_path.rs:46` — owned variant with
  `hidden_pk_id`, `new_pk_buf`, `old_pk_buf`, `new_row_buf`,
  `old_row_buf: Option<Bytes>`, `skip_unique_check`, `gl_index_id`.
  No `tx`, no `unpack_info`.
- `ha_rocksdb_cc__ha_rocksdb__auto_incr.rs:191` — minimal variant
  with `hidden_pk_id`, `new_pk_buf`, `old_pk_buf` plus a `TODO(human)`
  noting it's a placeholder that needs A2 to land canonical fields.

These three pretend to be the same type and reference each other across
stub files, but the field sets don't overlap. Whichever copy
TRANSLATE picks first wins; the others become subtle compile errors at
each call site.

The MyRocks original (`storage/rocksdb/ha_rocksdb.h:674-687`) is a
single 14-LoC struct. Producing three incompatible Rust translations
is a regression in coherence, not a translation.

**Suggested fix:** single canonical definition in
`ha_rocksdb_h__update_row_info.rs`, drop the others, fix the dangling
`Txn` reference.

---

## P1 — `bytes::Bytes` import path is inconsistent and at least one path is wrong

Two import patterns coexist:

- `use slatedb::bytes::Bytes;` — used by `rdb_buff_h.rs:129`,
  `ha_rocksdb_h__ha_rocksdb.rs:36`, `ha_rocksdb_cc____free__accessors.rs:23`,
  `ha_rocksdb_h__update_row_info.rs:19`, ~10 others.
- `use bytes::Bytes;` — used by `ha_rocksdb_cc__Rdb_transaction.rs:26`,
  `ha_rocksdb_cc__ha_rocksdb__dml.rs:35`, ~25 others.

SlateDB does not publicly re-export the `bytes` crate at the path
`slatedb::bytes` in normal usage (it depends on `bytes::Bytes`
internally but doesn't typically re-export the module). `use bytes::`
is the standard pattern, but `storage/slatedb/rust/Cargo.toml:11-15`
lists only `cxx`, `slatedb`, `object_store`, `tokio` — **no `bytes`
crate dependency**. Both forms will likely fail.

**Suggested fix:** pick `use bytes::Bytes;`, add `bytes = "1"` to
Cargo.toml, mass-rewrite the ~10 `slatedb::bytes::` sites. (Verify the
exact `bytes` version SlateDB depends on to avoid a Cargo dep-tree
duplicate of `Bytes`.)

---

## P1 — `RdbTransaction` trait methods are declared sync but bodies `await`

`ha_rocksdb_cc__Rdb_transaction.rs:64-221` declares the trait. Selected
methods that need to be async but aren't:

- line 109 — `fn get(&self, cf_id: u32, key: &[u8]) -> Result<Option<Bytes>, Error>;`
- line 117 — `fn get_for_update(&mut self, ...) -> Result<Option<Bytes>, Error>;`
- line 172 — `fn commit(&mut self) -> Result<bool, Error>;`
- line 189 — `fn prepare(&mut self, xid_name: &[u8]) -> Result<(), Error>;`

The corresponding `_impl` file bodies these with `todo!` hints that
unambiguously require `.await`:

- `Rdb_transaction_impl.rs:172` — `todo!("...self.inner.as_ref().unwrap().get(prefixed_key).await")`
- `:223` — `todo!("self.inner = Some(engine.db().begin(self.isolation).await); ...")`
- `:242` — `todo!("...self.inner.take().unwrap().commit().await; ...")`

`_DESIGN.md` §0.30-31 itself confirms SlateDB's `Db::begin` and
`DbTransaction::commit` return futures, and the design's §7 has the
caveat "async fn in our interface stubs is implementation detail of
the Rust side" — but that's a generic statement, not a fix. You cannot
`.await` inside a non-async fn. TRANSLATE will need to rewrite every
one of these signatures before any body can compile.

Related: the trait bound is `pub trait RdbTransaction: Send` (line 64),
but the registry at line 225 holds `Mutex<Vec<Weak<dyn RdbTransaction>>>`
shared across threads — bound should be `Send + Sync`.

**Suggested fix:** add `async fn` to the four methods (or split the
trait into sync/async halves; either is fine if it's deliberate). Fix
the trait bound. Verify the `.await` hints match the eventual
signatures.

---

## P1 — 2PC story is too thin to survive crash recovery

`_DESIGN.md` §1 row "Two-phase commit" + open question 8:

> XA prepare → SlateDB `flush_with_options(Wal)` plus a metadata
> marker. Commit → marker flip.

`Rdb_transaction_impl.rs:266-275` (`prepare`):

> 1. merge_auto_incr_map into the txn
> 2. write a prepare marker `xa_prepare:<xid>` into the system CF
> 3. flush_with_options(FlushType::Wal)  -- durability barrier
> We do NOT call DbTransaction::commit() yet; commit happens at the
> second phase via the rocksdb_commit_by_xid handlerton callback.

This won't work as described. SlateDB's `DbTransaction` buffers writes
in-memory; calling `flush_with_options(Wal)` on the `Db` does not
flush a `DbTransaction`'s buffered batch (those writes aren't in the
WAL until commit). So step 3 doesn't make a `prepared` transaction
durable. Two ways out exist:

- **(a) Serialize-and-replay.** `prepare` serializes the
  `DbTransaction`'s buffered writes into a marker key, writes that
  marker via the underlying `Db` (which goes through the WAL),
  rolls back the in-memory txn. `commit` reads the marker and replays
  into a fresh txn. **No second-phase conflict can occur** (the
  contents are already chosen), so the replay is committable.
- **(b) Commit-and-undo.** `prepare` commits the txn under a "prepared"
  flag; `rollback` undoes by writing tombstones; `commit` flips the
  flag. Has the wrong recovery semantic — a crash between prepare and
  commit leaves the writes visible to readers.

Option (a) is what the marker-write language gestures at, but the
design doesn't explain:

- Marker durability ordering vs. WAL flush (do you `await_durable=true`
  on the marker, or rely on the WAL flush call?)
- Marker size limit (a 100 MB INSERT-batch's serialized writebatch is
  a 100 MB single key in the system CF — does SlateDB tolerate that?)
- Recovery replay path (on startup, scan `xa_prepare:*` prefix,
  deserialize each marker, await the binlog coordinator's
  commit/rollback decision)
- Replay idempotency (if recovery crashes mid-replay)

MariaDB's binlog group commit pipeline assumes XA prepare is durable
before the binlog write returns. A wrong design here is hard to
retrofit because the binlog ordering must be preserved across the
crash boundary.

**Suggested fix:** turn open question 8 into a concrete spec covering
the four points above. Acknowledge marker size limits explicitly and
either constrain transaction size in XA mode or batch the marker
across multiple system-CF keys.

---

## P1 — `CompactionFilter` stub uses an invented type instead of SlateDB's

`rdb_compact_filter_h.rs:28-51` declares its own enum:

```rust
pub enum CompactionDecision { Keep, Drop }
fn filter(&mut self, key: &[u8]) -> Result<CompactionDecision, Error>;
```

`_DESIGN.md` §0 line 54 says the real SlateDB trait is:

```rust
async fn filter(&mut self, entry: &RowEntry) -> CompactionFilterDecision::{Keep, Drop, Modify(ValueDeletable)}
```

The stub:

- drops `Modify` (needed for value rewrites — e.g. stripping legacy
  TTL prefix bytes from upgraded rows);
- takes `&[u8]` instead of `&RowEntry` (no access to value);
- doesn't `impl slatedb::CompactionFilter for ...` — so even a correct
  body isn't registrable via
  `DbBuilder::with_compaction_filter_supplier(...)`.

This is the same pattern that `merge_operator` got right (the
`KeyPrefixMergeOperator` example in the design references the actual
SlateDB trait directly). The compaction filter unit was missed.

**Suggested fix:** rewrite to implement `slatedb::CompactionFilter`
directly. The drop-secondary-index sweep needs only `entry.key`, but
future filters (legacy-format conversion) need `entry.value`.

---

## P2 — Savepoint emulation relies on a SlateDB API that may not exist

`_DESIGN.md` §5 says SlateDB has no native savepoint API and the engine
will layer them as "a Rust-side stack of `(write_batch_position,
mark_read_set_snapshot)` checkpoints." `Rdb_transaction_impl.rs:209`:

> `todo!("pop top marker; truncate the txn's buffered writes back to writes_at_set")`

This assumes `DbTransaction` exposes a way to truncate its internal
write buffer to a given index. The design's §0.30-35 documents the
public `DbTransactionOps` API (`put`, `delete`, `merge`, `mark_read`,
`unmark_write`, `commit`, `rollback`) but **does not list any
truncate-to-position method**. If SlateDB doesn't expose this, the
fallback is to rebuild the transaction from scratch by replaying every
write below the savepoint — possible but O(N) and not free.

MyRocks gets this for free via RocksDB's `WriteBatch::SetSavePoint()`
/ `RollbackToSavePoint()` (`storage/rocksdb/ha_rocksdb.cc:3459-3462`).
The design needs to either confirm SlateDB has the equivalent or
document the replay-on-rollback semantics.

---

## P2 — `Rdb_writebatch_impl::GetFromBatchAndDB` is out-of-scope; read-your-writes lost in replication

`ha_rocksdb_cc__Rdb_writebatch_impl.rs:24-29` notes this is a "semantic
change" but understates the impact. The replication thread in MyRocks
uses `WriteBatchWithIndex::GetFromBatchAndDB` so that a row updated
earlier in the same `WriteBatch` is visible to a later read in the same
batch. Common patterns this enables:

- INSERT + UPDATE on the same row in one replication group
- Self-joins on a row being modified

The stub claims "Replication is single-threaded so the only way this
matters is if the SAME statement reads-after-writes a key, which isn't
a normal pattern." This is wrong — multi-statement transactions
replicated as one event group routinely contain read-after-write
within the group. Slave replication will silently corrupt for any
workload that hits this pattern.

**Suggested fix:** either layer a read-from-batch path in
`RdbWritebatchImpl` (Rust-side `HashMap<Bytes, Bytes>` overlay on
reads), or document this as a known correctness regression and require
all replication go through `RdbTransactionImpl` instead.

---

## P2 — `single_delete → delete` collapse degrades performance more than acknowledged

`ha_rocksdb_cc__Rdb_transaction_impl.rs:165-169` collapses MyRocks
`SingleDelete` to plain delete, citing "SlateDB has no SingleDelete
optimization." The design's §1 row "DML" treats this as a non-issue.

The performance impact under SlateDB's object-store-backed LSM is
likely *worse* than under RocksDB's local-disk LSM:

- RocksDB tombstones cost a few KB per key and live until the next
  compaction picks the SST (seconds to minutes).
- SlateDB tombstones live in S3-resident SSTs; compaction is slower
  (object-store I/O) and tombstones survive longer.
- Secondary-index sweeps (where MyRocks aggressively uses
  SingleDelete) will leave 10-100x more tombstones in the read path
  before compaction catches up.

This won't be visible at small benchmarks but will degrade large-scale
secondary-index UPDATE workloads. The design should at least flag this
in §1 as "Map (performance-degraded under high-churn SI workloads)"
rather than the current implicit "Map".

---

## P2 — Block cache is global; per-CF tuning is lost without acknowledgement

`_DESIGN.md` §1 "Block cache" row says "Global, not per-CF" and tags
it "Map (degraded)" but doesn't analyze the impact. MyRocks workloads
that share an instance across hot and cold tables use per-CF cache
configuration to isolate the cold-table scans from evicting the
hot-table working set. With a single global cache, a one-time analytic
scan over a cold table can blow away the OLTP hot path's cache.

This is a deployment-shape decision (the workaround is "run multiple
SlateDB instances per server") that should be in the design, not
discovered at Stage 2 benchmarking. Same applies to per-CF compression
and per-CF bloom-filter configuration: each gets a one-line "degraded"
verdict without documenting what use cases are lost.

---

## P2 — Three forward-decl placeholder structs in `Rdb_transaction.rs` collide with real types

`ha_rocksdb_cc__Rdb_transaction.rs:32-42`:

```rust
pub struct RdbIoPerf;
pub struct RdbTblDef;
pub struct RdbKeyDef;
pub struct RdbTableHandler;
```

Each is a concrete empty struct in the module namespace, not a `use`
statement. Other stubs import these names:

- `ha_rocksdb_cc__Rdb_writebatch_impl.rs:34` — `use crate::ha_rocksdb_cc__Rdb_transaction::{RdbTblDef, ...};`
- `ha_rocksdb_cc__Rdb_transaction_impl.rs:32` — pulls four of them.

The "real" definitions live elsewhere:

- `rdb_datadic_h__Rdb_tbl_def.rs` defines `TblDef`
- `ha_rocksdb_h__Rdb_table_handler.rs:35` defines `TableHandler`
- `rdb_datadic_h__Rdb_key_def.rs` defines the key-def type
- `rdb_perf_context_h.rs` defines `IoPerf`

Naming mismatches aside, the placeholders are *type definitions* —
not aliases — so call sites silently pick the empty struct over the
real type when the import resolves them first. Method calls like
`tbl.full_name` or `tbl.gl_index_id` fail; the empty struct has no
fields.

**Suggested fix:** delete lines 32-42, replace with aliases:
`use crate::rdb_datadic_h__Rdb_tbl_def::TblDef as RdbTblDef;` etc.,
or rename uniformly so the stubs share canonical names.

---

## P3 — minor inconsistencies and polish

- **Corruption marker name conflict.**
  `ha_rocksdb_cc____free__error_helpers.rs:121` says "we keep that
  exact name `ROCKSDB_CORRUPTED` for operator muscle memory", but
  lines 124 and 128 hardcode `SLATEDB_CORRUPTED`. Pick one.
- **`Cargo.toml` is missing crates the stubs assume.** `bytes`,
  `parking_lot`, `once_cell` (or pivot to `std::sync::LazyLock`),
  `tokio-util`, `tracing`, possibly `async_trait`. Pre-decide vs.
  std-lib alternatives.
- **Design pins SlateDB 0.13.0; `Cargo.toml` is `=0.13.1`.** Almost
  certainly the same source, but every §0 API claim needs a
  re-validation tag against 0.13.1 if that's what TRANSLATE will
  build against.
- **`unwrap()`/`expect()` in exemplars violate the §0.4 ground rule.**
  `rdb_buff_h.rs:55,58,61` (hot path), `ut0counter_h.rs:60`,
  `ha_rocksdb_cc____free__name_helpers.rs:115`,
  `ha_rocksdb_cc____free__status_helpers.rs:62`. The exemplars set the
  norm; CI lint will fire.
- **Bridge surface is empty.**
  `storage/slatedb/rust/src/bridge.rs` declares only
  `slatedb_version()`. The design's §10 says "Not the cxx bridge" but
  the cxx bridge is where the ~150 `async fn` handler vtable methods
  cross to synchronous C++ — and the design doesn't sketch the
  serialization strategy for, say, returning a row from
  `rnd_next(uchar *buf)` that the bridge needs to translate from
  `Result<RowBuf, Error>` into `(int rc, uchar* buf_filled)`. Worth a
  §6.1 sketch before TRANSLATE so 150 sites don't each invent their
  own convention.
- **`WriteBatch` import path inconsistency.**
  `rdb_sst_info_h.rs:88` writes `slatedb::batch::WriteBatch` while
  `Rdb_writebatch_impl.rs:41` writes `slatedb::WriteBatch`. The
  visible re-export at `slatedb::WriteBatch` is the public API per
  `_DESIGN.md` §0.45; the `slatedb::batch::WriteBatch` form is the
  module path. Pick one to match `_DESIGN.md`.
- **PROCESSLIST "Waiting on row lock" PSI stage stays even though
  there are no row locks.** `rdb_psi_h.rs:48`. Either rename to
  "Waiting on transaction commit" (which is what it now means under
  SSI) or drop it. Operator confusion otherwise.

---

## What's solid

1. **The exemplars are good Rust.** `atomic_stat_h.rs`,
   `event_listener_h.rs`, `rdb_global_h.rs`, `rdb_comparator_h.rs`
   are tight, well-traced to C++ line numbers, and make defensible
   API choices.

2. **The §1 feature map is honest after v2.** Verifying SlateDB
   before designing was the right move; "Map (native)" verdicts are
   credible where claimed. The `event_listener` pivot from
   callback-model to `watch::Receiver` is exemplary.

3. **§1 non-goals are flagged consistently.** Every non-goal stub
   returns the same pattern (`Err(Error::invalid("non-goal: ..."))`),
   the comment cites the §1 row, and the bridge expectations match.

4. **The cf-id → key-prefix scheme (§2) is internally consistent.**
   `varint(cf_id) || u32_be(index_id) || memcmp_key` matches the
   `PrefixExtractor` design, and `cf_id = u32::MAX` for system avoids
   collision. `Rdb_cf_manager_h.rs` correctly collapses the
   create-CF path to a pure name→id allocation.

5. **The 13 I_S sub-units share a clean factoring.** `Column`,
   `ColumnType`, `Nullable`, `Row` pulled from `rdb_i_s_cc__shared`;
   each stub provides `fields_info()`, `fill_table()`, `init()`.
   Get `shared` right and 13 fall into place.

6. **Error model commitment to `slatedb::Error` directly.** §4 pays
   off — no `SlateError` enum invented, the 6-variant `ErrorKind →
   HA_ERR_*` table is complete, and stubs respect it.

---

## Recommended next moves

**Patch in place** (no design re-spin):

- Strip 10 placeholder `HaSlateDb` declarations; `use` the canonical.
- Collapse 3 `UpdateRowInfo` definitions; fix the dangling `Txn`
  reference.
- Pick `bytes::Bytes` path, add Cargo dep, mass-rewrite imports.
- Delete the 4 placeholder structs in `Rdb_transaction.rs`; alias
  real types.
- Replace `expect()`/`unwrap()` with `Result`/`Option` in `rdb_buff_h`
  and friends.
- Reconcile corruption-marker filename, `WriteBatch` import path,
  PSI stage name.

**Need a human design ruling before re-issuing affected stubs:**

- Concurrency model. Either (a) deprecate the lock-sysvars and
  document the SSI semantics in user docs, or (b) layer a
  pessimistic-lock manager. This is the headline gap.
- Async-vs-sync split for `RdbTransaction` and `RdbWritebatchImpl`.
  Decide whether the trait methods are `async fn`, return
  `Pin<Box<dyn Future>>`, or split. Affects 6+ stub files.
- 2PC ordering — turn open question 8 into a concrete spec covering
  marker size, durability ordering, recovery replay, and
  idempotency.
- `CompactionFilter` shape — verify SlateDB's actual trait
  signature, rewrite `rdb_compact_filter_h.rs` against it.
- Savepoint truncation — confirm SlateDB exposes the API the design
  assumes, or document the replay-on-rollback cost.
- Autocommit single-row policy — `Db::write(WriteBatch)` (one
  round-trip) vs. `begin → put → commit` (two). §6's "lean: always
  txn" conflicts with §9's latency budget.

**Then re-present for a second design-freeze pass.** The patch work is
bounded; the human-ruling work is the design's actual job. Going into
TRANSLATE without these decisions means each handler-vtable bucket
re-invents the same answers, badly, 22 times.

The substantive design (SlateDB API mapping, codec strategy, non-goal
triage, I_S factoring) is good and worth saving. The mechanical defects
are typical of a parallel-subagent batch under rate-limit pressure
(per the batch-complete report). The concurrency-model omission is not
— it's a design gap. Fix that one first.
