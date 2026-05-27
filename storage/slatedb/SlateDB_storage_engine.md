# MyRocks → Rust-on-SlateDB Migration — Claude Code Operating Guide

> **Read this file fully before doing anything.** This is the operating contract for an
> agentic, dependency-ordered, test-gated translation of the MariaDB MyRocks storage engine
> (`storage/rocksdb/`, C++ on RocksDB) into a Rust storage engine on top of **SlateDB**,
> exposed to the MariaDB server through a **cxx** FFI boundary.
>
> The goal is **not** a one-shot transpile. It is a strangler-fig migration where the C++
> MariaDB server stays intact and a Rust engine grows underneath it, validated against the
> existing engine at every step.

---

## 0. Ground rules (do not violate)

1. **Never touch the C++ MariaDB server core.** You only add a new pluggable engine. The
   only C++ you write is the thin cxx bridge shim. If a task seems to require editing
   `sql/`, stop and ask the human.
2. **Interface-first.** You propose a safe-Rust interface for every unit and get it approved
   *before* translating any implementation (§6). Do not implement against an unapproved
   interface. (Single-shot translation without an interface caps at ~15-19% success;
   interface-first + repair reaches ~32-48%. Respect this.)
3. **Every change is tier-gated.** No unit is "done" until it clears the gate tier
   appropriate to its level (§8): leaf units have a leaf gate; engine-integration units add
   the differential bridge harness; release stages add MTR and sysbench. Do not invoke a
   higher tier than the unit's level requires, and do not skip the tier that does apply.
4. **No `unwrap()` / `expect()` / `panic!` on any request path.** CI denies
   `clippy::unwrap_used` and `clippy::expect_used` for crates in `rust/src/engine/` and
   `rust/src/codec/`. Test code and `build.rs` are exempt. (A single unhandled `.unwrap()`
   took Cloudflare's global proxy offline in Nov 2025. We do not repeat that.)
5. **Two passes per unit.** Pass 1: correct-but-unidiomatic (preserve C++ semantics,
   `unsafe` allowed). Pass 2: idiomatic safe Rust (remove raw pointers, use iterators/RAII),
   re-run the full gate. Never skip pass 2, never merge before pass 2 passes — *except* per
   §7.5 when pass 2 demonstrably regresses correctness.
6. **Cheap model first, escalate on failure.** Attempt each unit with the cheap model. After
   N failed repair iterations (see §0.7), escalate to the reasoning model. Log every
   escalation in the unit's gate report.
7. **Bounded repair loops.** Max 6 compile/test repair iterations per unit. On exhaustion,
   write a structured failure report to `migration/review-queue/` and move on (§10). Do not
   loop forever.
8. **Commit per unit, never batch.** One translation unit = one commit = one PR-sized diff.
   Commit messages follow `xlate(<module>): <unit> — pass{1,2}, gate=<status>`.

---

## 1. Positioning and non-goals (read before any design choice)

SlateDB is an LSM keyed on **object storage**. MyRocks is an OLTP engine tuned for local
NVMe. These are not interchangeable substrates. This migration is **not** a drop-in MyRocks
replacement; it is a new engine that happens to share MyRocks' SQL semantics where feasible.
Frame every design decision against that.

**In scope for `ha_slatedb`:**
- A pluggable MariaDB storage engine usable for workloads where object-storage latency is
  acceptable: warm-tier OLTP, append-heavy logging, analytical staging, dev/test/CI.
- Full ACID single-node semantics within SlateDB's transactional model.
- Behavioural parity with MyRocks on **the MTR subset that does not depend on RocksDB
  internals** (column families, merge operators, TTL compaction filters, Read-Free Replication,
  SST bulk loader). The dependent subset is explicitly out of scope below.

**Out of scope (non-goals):**
- **Performance parity with MyRocks on local NVMe.** Sysbench thresholds (§8.C) are set
  against a *batched* baseline; raw RocksDB will win every write benchmark and that is fine.
- **RocksDB feature parity.** Column families, merge operators, compaction filters
  (TTL/secondary-index), Read-Free Replication, bulk loader — see §9. Each is a deliberate
  redesign or a "not supported, return `HA_ERR_WRONG_COMMAND`."
- **MyRocks status variables, NoSQL access path (`nosql_access.cc`), `myrocks_hotbackup`,
  encryption-at-rest, native compression.** These are deferred or dropped; the human sets
  per-feature policy in the INTERFACE phase.
- **`ha_rocksdb` removal.** Never. MyRocks-on-RocksDB stays the default until the human
  declares parity *for their workload*.

**Stop-and-escalate threshold:** if SlateDB p99 write latency on the target object store
exceeds ~200ms under sustained load, **stop the project and escalate**. The substrate may be
wrong for the workload, and no amount of translation fixes that. This is not a §9 footnote
— it is a hard gate before Stage 2 begins.

---

## 2. Repository layout assumptions

The MariaDB repo is local. Confirm these paths exist before starting; if they differ, update
this section and tell the human.

```
<repo>/
├── sql/handler.h              # handler / handlerton C++ API (READ-ONLY for us)
├── sql/handler.cc
├── storage/rocksdb/           # MyRocks source — our translation SOURCE of truth
│   ├── ha_rocksdb.cc / .h     # the handler subclass (14,791 + 1,071 LoC)
│   ├── rdb_*.{cc,h}           # MyRocks internals (codecs, buffers, cf mgmt, txn, ...)
│   ├── rocksdb/               # git submodule — initialize before INDEX phase
│   └── mysql-test/            # 492 MTR .test/.result pairs — our REGRESSION SPEC
└── ...
```

**Submodule precondition:** `storage/rocksdb/rocksdb/` is a git submodule and may not be
checked out in a fresh tree. Before §5 (INDEX), run `git submodule update --init --recursive
storage/rocksdb/rocksdb` or the AST dump will fail on missing headers.

Our new work lives in a sibling directory under `storage/`:

```
<repo>/storage/slatedb/        # the new engine (this directory)
├── SlateDB_storage_engine.md  # this file
├── shim/                      # C++ side of the cxx bridge
│   ├── ha_slatedb.cc          # handlerton + handler vtable, forwards into Rust via cxx
│   └── ha_slatedb.h
├── rust/                      # the Rust crate (workspace root)
│   ├── Cargo.toml             # MSRV pinned (§4); SlateDB pinned to a specific git rev
│   ├── build.rs               # cxx-build: compiles the bridge
│   └── src/
│       ├── lib.rs             # crate root; re-exports the bridge
│       ├── bridge.rs          # the cxx bridge definition
│       ├── engine/            # SlateDB-backed engine, Tokio runtime, worker pool
│       ├── codec/             # key/value encoding (translated from rdb_datadic etc.)
│       └── ...                # one module per MyRocks cluster
├── CMakeLists.txt             # wires the plugin into MariaDB's build via MYSQL_ADD_PLUGIN
└── migration/
    ├── manifest.json          # dependency-ordered unit list (generated, §5)
    ├── interfaces/            # Claude-proposed safe-Rust interface stubs, human-approved (§6)
    ├── review-queue/          # failed units needing human attention (§10)
    └── reports/               # per-unit gate reports
```

Everything under `migration/` is generated or proposed by you; the human approves
`interfaces/` once per batch and curates `review-queue/` as needed.

---

## 3. The cxx boundary (this project uses cxx, not bindgen/autocxx/Crubit)

We use **cxx** because we control both sides of the boundary and want compile-time-checked,
typed signatures rather than raw FFI. The MariaDB `handler` is a C++ virtual class, so the
shape is:

- **C++ shim owns the vtable.** `ha_slatedb` (C++) subclasses `handler` and implements every
  virtual method. Each method body is a *thin forwarder* that calls into Rust through the
  cxx bridge. The shim does no logic — its only responsibilities are (a) translating MariaDB
  types (`uchar*` row buffers, `key_range`, error codes) into the POD shapes the bridge
  accepts, and (b) reading/writing `THD`-scoped state.
- **Rust owns the logic.** The cxx bridge exposes Rust functions/opaque types that the shim
  calls. SlateDB, the async runtime, codecs, and transaction state all live in Rust.

### cxx bridge conventions

- The bridge module is `rust/src/bridge.rs`, `#[cxx::bridge(namespace = "slatedb")]`.
- **Rust→C++ exposure:** opaque Rust types (`type Engine;`, `type Txn;`, `type RowCursor;`)
  plus free functions returning `Box<T>` for Rust-owned handles. Use cxx's `UniquePtr<T>`
  *only* when wrapping a C++-owned object handed back to Rust — `Box<T>` is for the common
  Rust-owns-it case. Mixing these is a frequent footgun (see §3.1).
- **C++→Rust exposure:** only what we genuinely need from the server (e.g., a read-only
  view of `TABLE_SHARE` columns and key parts). Wrap these in small POD structs declared in
  the bridge — do **not** try to expose `Field`/`Item`/`THD` directly through cxx (that is
  autocxx/Crubit territory and out of scope here). If a unit needs deep server internals,
  stop and ask the human to widen the bridge deliberately.
- **`THD` lifecycle.** The shim, not the bridge, reads/writes `THD`. The bridge exposes a
  small `TxnContext` POD (transaction id, isolation level, savepoint stack snapshot) that
  the shim populates from `THD` at each handler entry point and writes back at exit.
  Transaction begin/commit/rollback callbacks (`hton->commit`, `hton->rollback`) live in the
  shim and translate to bridge calls like `bridge::txn_commit(engine, txn_id)`.
- **Errors cross as `Result<T, E>`** where `E` maps to a stable error enum; the shim
  converts to MariaDB `HA_ERR_*` codes. cxx auto-wraps each Rust function in a
  panic-to-exception catch, so a Rust panic does not unwind through MariaDB's C++ stack —
  but combined with §0.4 we want zero panics on request paths regardless. Do **not** rely on
  `panic = "abort"` profile settings; that's a final-binary policy owned by `mariadbd`,
  which we do not control.
- **Strings/bytes:** use cxx's `&[u8]` / `Vec<u8>` / `&CxxString` / `String` support. Keys
  and values are bytes; avoid UTF-8 assumptions. MariaDB row buffers (`uchar*` + length)
  cross as `&[u8]` (read) or `&mut [u8]` (write into the server-provided buffer).

### 3.1 cxx footguns to avoid (encode these as review checks)

- Do not return a C++ `std::unique_ptr` *into* Rust expecting raw-pointer ABI — use cxx's
  `UniquePtr<T>` type explicitly. Conversely, do not return a Rust `Box<T>` as
  `UniquePtr<T>` to C++; cxx's `Box` codegen is the right tool for Rust-owned handles.
- Shared mutable state across the boundary must be behind a Rust-side `Mutex`/channel,
  never a raw `*mut` shared with C++.
- The async SlateDB runtime must **not** be entered from arbitrary handler threads ad hoc
  (see §9). The bridge functions are synchronous from C++'s view; they internally hand work
  to the runtime and block on a bounded channel.

### 3.2 Tokio runtime lifecycle

- **One multi-thread runtime per `handlerton` lifetime.** Constructed in `hton->init` via
  `OnceLock<Runtime>`; dropped in `hton->destroy`.
- Worker thread count comes from a sysvar (`slatedb_io_threads`, default = `num_cpus / 2`).
- Bridge functions submit work via a bounded `mpsc` channel sized from another sysvar
  (`slatedb_io_queue_depth`, default = 4096). On queue full the bridge returns
  `HA_ERR_LOCK_WAIT_TIMEOUT` — never blocks the handler thread indefinitely.
- **Never** `block_on` from within a Tokio worker (re-entrant deadlock). The shim's
  blocking wait happens on the *handler* thread, which is outside the runtime.

---

## 4. Build integration

MariaDB's build is CMake. The plugin needs to compile Rust → staticlib → link into a
loadable `.so`. Specifics:

- **Wire-up:** `storage/slatedb/CMakeLists.txt` uses `MYSQL_ADD_PLUGIN(slatedb ${SOURCES}
  STORAGE_ENGINE MODULE_ONLY COMPONENT Storage)` — same pattern as `storage/example/`.
- **Rust ↔ CMake bridge:** use **corrosion-rs** (`find_package(Corrosion REQUIRED)` then
  `corrosion_import_crate(MANIFEST_PATH rust/Cargo.toml CRATE_TYPES staticlib)`). It handles
  dependency tracking, target detection, and feature flags correctly; the alternatives
  (`add_custom_command` shelling out to `cargo build`, cmake-rs) miss incremental rebuilds.
- **cxx:** the Rust crate's `build.rs` calls `cxx_build::bridge("src/bridge.rs")` and adds
  `shim/ha_slatedb.cc` as a source. The generated header lands in `OUT_DIR/cxxbridge/` and
  the shim `#include`s it.
- **Rust MSRV:** pin in `rust-toolchain.toml`. Default to the MSRV of the SlateDB crate plus
  two stable releases; bump deliberately on a SlateDB version bump.
- **SlateDB dependency:** pin a specific git rev (or crate version) in `Cargo.toml`. Do not
  track a moving branch. Version bumps are a deliberate human-approved unit, not a passive
  upgrade.
- **Feature flags:**
  - `default = []`
  - `ffi-equivalence` — enables the differential bridge harness (§8.B). Build-only flag,
    never on in production.
  - `kani` — enables `#[kani::proof]` annotations for §8.A bounded model checks.
- **Skip on unsupported targets:** mirror `storage/rocksdb/CMakeLists.txt`'s `RETURN()`
  pattern for 32-bit / big-endian / unsupported toolchains.

---

## 5. INDEX phase — build the dependency graph (one-shot, do this first)

**Goal:** produce `migration/manifest.json`: a topologically sorted list of translation
units, leaves (no dependencies) first.

Steps:
1. Ensure `storage/rocksdb/rocksdb/` submodule is initialized (§2 precondition).
2. Generate `compile_commands.json` via CMake `-DCMAKE_EXPORT_COMPILE_COMMANDS=ON` if
   absent, then run clang AST dump over `storage/rocksdb/*.{cc,h}` using those compile
   flags.
3. Build a call graph + struct/class dependency DAG.
4. **Cluster into units, target ≤500 LoC.** This is a target, not a hard limit. For the
   three megafiles (`ha_rocksdb.cc` at 14,791 LoC; `rdb_datadic.cc` at 5,439 LoC; `rdb_i_s.cc`
   at 1,975 LoC) split along *method-cluster* boundaries identified by the AST: per-method
   groups that share private state. Expect ~20-30 units from `ha_rocksdb.cc` alone. Each
   sub-unit names its parent file and its method-cluster id in `manifest.json`.
5. Topologically sort. Tag each unit with: source files, LoC, dependencies (by unit id),
   whether it touches RocksDB API directly (these need SlateDB re-mapping, not 1:1
   translation), and whether it touches the handler vtable (these are shim-forwarded).
6. Emit `manifest.json`. **Stop and have the human review the ordering** before translating.

> Units that call the RocksDB API directly do **not** get a 1:1 translation. They get
> *re-implemented* against SlateDB's narrower async API. Flag these clearly — they are
> design work, not translation, and need human design sign-off in the interface phase (§6).

---

## 6. INTERFACE phase — propose all safe-Rust stubs in one batch (human approves)

For every unit in the manifest, **you propose** a safe-Rust interface from the C++ source:
trait/function signatures with `todo!()` bodies and doc comments stating the contract
(inputs, outputs, error conditions, invariants). Write them to
`migration/interfaces/<unit>.rs`. This is where wrong abstractions get caught cheaply,
before any implementation work.

Do this as **one batch pass over the whole manifest**, not per-unit drip. Produce all the
interface stubs, then stop and present them together for a single human review/approval. Do
not implement against an unapproved interface; once approved, the interfaces are frozen
contracts for the TRANSLATE loop.

For RocksDB-facing units, the interface must explicitly state the SlateDB mapping decision
(e.g., "RocksDB column family → SlateDB key prefix" or "RocksDB merge operator → Rust-side
read-modify-write because SlateDB merge semantics differ"). Group these design decisions at
the top of the batch so the human can review the consequential ones first.

For features explicitly out of scope (§1), the interface is a stub returning the appropriate
`HA_ERR_*` code with a doc comment naming the §1 non-goal it traces back to — not a
`todo!()`. This keeps "we deliberately don't do this" visible in code, not just in this doc.

---

## 7. TRANSLATE loop — per unit, in manifest order

For each unit `U` (dependencies already translated and gate-passing):

1. **Slice context.** Assemble: `U`'s C++ source + the *approved interface stubs* of all of
   `U`'s callees + relevant struct/enum declarations + alias/lifetime notes from the index.
   Do **not** dump the whole file or whole repo into context. Per-unit slices outperform.
2. **Pass 1 (correct, unidiomatic).** Cheap model. Produce Rust that compiles against the
   approved interface and preserves C++ semantics. `unsafe` is allowed in pass 1.
3. **Repair loop.** `cargo check` → feed `rustc` + `clippy` errors back, ≤6 iterations.
   Escalate to the reasoning model on persistent failure. On exhaustion → review queue
   (§10).
4. **Gate (pass 1).** Run the tier appropriate to `U`'s level per §8:
   - Leaf units: tier A only.
   - Engine-integration units (those that touch the handler vtable or transaction state):
     tier A + tier B.
   - Stage transitions: tier C, run by the human as part of the stage gate (§11), not per
     unit.
5. **Pass 2 (idiomatic).** Refactor: raw pointers → `Box`/`Rc`/`Arc`/`&mut`; manual loops →
   iterators; manual resource cleanup → RAII/`Drop`; remove `unsafe` wherever the borrow
   checker now permits. **Re-run the same tier.** Idiomatic code that fails the gate is
   reverted to pass-1 code and queued for human review — never merge a regression for the
   sake of idiom. The pass-1 code stays in tree; the review-queue entry records why pass 2
   was reverted.
6. **Commit.** `xlate(<module>): <unit> — pass2, gate=green` (or `pass1-only` for §7.5
   reverts). Write the gate report to `migration/reports/<unit>.md`.

---

## 8. Verification harness (tiered)

The MTR suite under `storage/rocksdb/mysql-test/` is the **regression spec for Stage 2+**.
Treat 100% pass on covered tests as a hard prerequisite at Stage 2, not earlier. Gating
applies at three tiers; each unit runs the tier appropriate to its level:

### Tier A — leaf unit gate (every translation unit)

- **Unit tests** per Rust module (codecs round-trip, comparators order correctly, etc.).
- **Property-based tests** (`proptest` + `bolero`) for any unit with non-trivial input
  domain: range scans return sorted output, PK uniqueness, transaction isolation, encoders
  round-trip across the byte domain.
- **Bounded model checking** (`kani`) for any hand-written `unsafe` encode/decode code.

Tier A is fast (seconds) and must pass cleanly before pass 2 or commit.

### Tier B — engine-integration gate (handler-vtable + transaction units)

- **Differential bridge harness** (`cargo test --features ffi-equivalence`): an input
  generator (AFL++/libFuzzer style) emits CREATE/INSERT/UPDATE/DELETE/SELECT/transaction
  sequences. The harness runs each sequence through *both* `ha_slatedb` and `ha_rocksdb`
  via an in-process embedded server fixture and diffs (a) row sets, (b) error codes, (c)
  sort order of scans, (d) commit/rollback outcomes. Any divergence fails the unit.
- Tier B is minutes-scale and only runs for units that integrate at the handler boundary.
  Leaf units (codecs, comparators, formatters) do not run Tier B.

The differential harness itself is a Stage-1 deliverable (§11); pre-Stage-1, Tier B units
gate on Tier A plus a focused integration test the unit's author writes.

### Tier C — release stage gate (Stage 2 and 3, human-run)

- **MTR regression**: run the module's `.test` files against the hybrid build (C++ server +
  Rust `ha_slatedb`), compare to `.result`. Track per-feature MTR coverage in
  `migration/reports/mtr-coverage.md`.
- **Performance CI**: sysbench `point_select`, `oltp_read_write`, bulk insert. Alert on >5%
  regression vs. an **agreed batched-baseline** for `ha_slatedb` — not vs. `ha_rocksdb` raw
  numbers, which SlateDB cannot match on local NVMe (§1). The human sets the threshold per
  workload.
- **Jepsen-style crash/partition testing** against the target object store (Stage 3 only).

Tier C is hours-scale and runs in CI on stage transitions; do not run per-unit.

---

## 9. SlateDB-specific design constraints (read before any engine work)

- **Async vs sync mismatch.** SlateDB's API is async (`put`/`get`/scan return `Future`s);
  the MariaDB handler API is synchronous, thread-per-connection. **Front SlateDB with the
  Tokio runtime described in §3.2.** Bridge functions are synchronous from C++; internally
  they submit work to the bounded channel and block on the result. **Never** call
  `block_on` directly on an arbitrary handler thread in a way that could re-enter the
  runtime — that deadlocks.
- **Latency.** Object-storage PUT latency (~50-100ms typical, p99 can spike to seconds)
  means naive per-row writes will not meet OLTP SLAs. An aggressive **write-batching /
  commit-pipelining** layer is mandatory for the DML path. Design this explicitly in the
  INTERFACE phase as a first-class unit; do not let the per-unit translator invent it
  bottom-up.
- **No 1:1 RocksDB feature map.** Column families, merge operators, compaction filters
  (TTL/secondary-index), Read-Free Replication, and the bulk loader have **no direct
  SlateDB equivalent**. Each needs either a deliberate Rust re-implementation with human
  design sign-off (flagged in §5/§6) or an explicit §1 non-goal stub (§6).
- **The §1 escalation threshold (p99 write > ~200ms) applies here.** If §8.C performance
  testing shows we cross it under realistic load, stop Stage 2 and re-scope.

---

## 10. Failure handling

When a unit exhausts its repair budget or fails the gate after pass 2:

1. Do **not** keep retrying or hack the test to pass.
2. Write `migration/review-queue/<unit>.md` containing: the C++ source, the approved
   interface, the best Rust attempt, the exact failing output
   (`rustc`/`clippy`/test/fuzz diff), and a one-paragraph hypothesis of the root cause.
3. Mark the unit blocked in `manifest.json`. Skip dependent units (they're blocked
   transitively).
4. Continue with other ready units. Surface the review queue to the human at the end of
   each run.

---

## 11. Definition of done (per stage)

- **Stage 1 — skeleton:** `ha_slatedb` plugin loads; `CREATE TABLE t(a INT) ENGINE=SLATEDB;
  INSERT; SELECT;` works through the cxx bridge against SlateDB; **the §8.B differential
  bridge harness builds and runs end-to-end on a smoke-sized fuzz corpus**. The harness
  itself is the long-pole deliverable here; without it Stage 2 has no gate.
- **Stage 2 — translation:** all in-scope manifest units gate-green at their tier; full
  MTR coverage for translated modules passes Tier C; sysbench within agreed
  batched-baseline thresholds.
- **Stage 3 — hardening:** Jepsen-style crash/partition testing against the object store
  passes; opt-in behind a build flag and `--default-storage-engine` switch; MyRocks-on-
  RocksDB remains the production default until the human declares parity *for their
  workload* (§1).

### Realistic time framing

After deduplicating the megafiles in §5, expect ~70-100 translation units. Even at
optimistic Claude throughput (one unit through both passes + gate in ~30 minutes of agent
time, including repair loops), Stage 2 is hundreds of agent-hours plus design work for the
RocksDB-divergent units plus MTR debugging. Calendar-wise, **plan in quarters, not weeks**.
If the human expects a faster delivery, the scope (§1 non-goals) needs to be cut, not the
gate (§0.3).

`ha_rocksdb` is **never** removed by you. Decommissioning is a human decision made after a
long clean shadow period.

---

## 12. First actions for this session

1. Confirm the repo paths in §2; report any mismatch.
2. Ensure `storage/rocksdb/rocksdb/` submodule is initialized; ensure
   `compile_commands.json` exists (generate via CMake if needed).
3. Run the INDEX phase (§5) → `migration/manifest.json`.
4. Run the INTERFACE phase (§6): propose all safe-Rust interface stubs in one batch.
5. Stand up the **Stage-1 skeleton** (§11): a minimal `ha_slatedb` plugin that loads, with
   the cxx bridge (§3) wired through `build.rs`/CMake via corrosion-rs (§4), backed by
   SlateDB, such that `CREATE TABLE t(a INT) ENGINE=SLATEDB; INSERT; SELECT;` works
   end-to-end **and** the §8.B differential bridge harness builds and runs on a smoke
   corpus. Figure out the scaffold structure yourself within the conventions in §3 and §4.
6. **Stop and present** the manifest ordering, the batch of proposed interfaces, and the
   working Stage-1 skeleton for human review. Do not begin the TRANSLATE loop (§7) until
   interfaces are approved.
