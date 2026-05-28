# INDEX phase — v2 manifest

**Status:** v2 manifest produced. The .h/.cc split eliminated all cycles. Awaiting
human review before proceeding to INTERFACE.

## What changed from v1

v1 paired each unit's `.h` + `.cc` into one node. That made `rdb_utils.cc → ha_rocksdb.h`-
style impl-side back-edges into cycles, collapsing 20 of 27 units into one giant SCC.

v2 makes each file its own unit (`<stem>_h` or `<stem>_cc`). Implementation units only
exist at the top of the DAG — nothing #includes a `.cc`, so impl units have no incoming
dependencies. The graph becomes strictly acyclic.

## Headline numbers

- **45 units** (27 headers + 18 implementations) in **45 singleton clusters**.
- **0 multi-unit clusters.** The DAG is fully acyclic.
- **5 units flagged `needs_split` (>2× 500-LoC target)** for v3 AST-driven method-cluster split:
  - `ha_rocksdb_h` (1,071 LoC, 62 classes, touches handler vtable)
  - `ha_rocksdb_cc` (14,791 LoC, touches handler vtable)
  - `rdb_datadic_h` (1,639 LoC, 17 classes)
  - `rdb_datadic_cc` (5,439 LoC)
  - `rdb_i_s_cc` (1,975 LoC, 14 classes)

These 5 are 73% of the codebase by LoC. The remaining 40 units average ~230 LoC.

## Topological order (highlights)

```
c00  atomic_stat_h              94    header       (leaf)
c01  event_listener_h           49    header       (leaf)
c02  ut0counter_h               203   header       (leaf)
c03  rdb_global_h               396   header
c04  rdb_mariadb_port_h         55    header
c05  rdb_utils_h                335   header
c06  rdb_buff_h                 549   header
...
c13  ha_rocksdb_h               1071  header  VT  *SPLIT*
c14  properties_collector_h     215   header
c15  rdb_datadic_h              1639  header      *SPLIT*
c16  event_listener_cc          96    impl
...
c27  ha_rocksdb_cc              14791 impl    VT  *SPLIT*
c28  nosql_access_cc            53    impl
...
c44  rdb_utils_cc               369   impl
```

The shape is the right one:

1. **Leaf headers (c00-c12)** — `atomic_stat_h`, `ut0counter_h`, `rdb_global_h`,
   `rdb_utils_h`, `rdb_buff_h`, etc. No in-scope dependencies. Ideal starting point
   for INTERFACE: design these stubs without committing to anything else first.
2. **Hub header c13 = `ha_rocksdb_h`** — defines the central types everyone depends on
   (handler subclass, key shared structs). The largest header (1,071 LoC) and the most
   consequential INTERFACE design decision. Flagged for v3 split.
3. **Headers built on the hub (c14-c26)** — `rdb_datadic_h`, `rdb_cf_options_h`,
   `rdb_cf_manager_h`, etc. INTERFACE proposals here depend on `ha_rocksdb_h` being
   designed first.
4. **Implementations (c16, c27-c44)** — start ordering after their dependent headers.
   `ha_rocksdb_cc` at c27 is the megafile; `rdb_datadic_cc` at c34; `rdb_i_s_cc` at c35.

## Implications for INTERFACE phase (§6)

INTERFACE proposes safe-Rust stubs for every unit in one batch (per §6). With v2's
clean DAG, "in one batch" can be done **header-first, in topological order**:

1. **Pass A — leaf headers** (c00 - c12, ~14 units). Cheap, mostly leaf utilities.
   Most don't touch RocksDB API; clean Rust translations.
2. **Pass B — the hub** (c13 `ha_rocksdb_h`). Single most important INTERFACE decision.
   Designs the Rust handler trait, transaction context POD (§3 of doc), error mapping.
   Needs v3 AST split first or careful manual partitioning — proposing one rust trait
   for a 62-class C++ header isn't useful guidance.
3. **Pass C — dependent headers** (c14 - c26). Proposed against the approved Pass B.
4. **Pass D — implementations** (c27 - c44). Stubs returning `HA_ERR_NOT_IMPLEMENTED`
   or §1 non-goal stubs. The big ones (`ha_rocksdb_cc`, `rdb_datadic_cc`, `rdb_i_s_cc`)
   need v3 split before they can be meaningfully INTERFACE'd.

## What v3 (AST split) buys

The 5 split-flagged units don't reduce to one Rust trait — they're collections of
loosely-related concerns:

- `ha_rocksdb_cc` has 62 classes, but they cluster into groups: handler-vtable methods,
  internal helper classes, lambdas, free functions. AST analysis would identify these
  groups (call-graph SCCs within the file) and emit them as sub-units.
- `rdb_datadic_cc` similarly: codec primitives, record encoding, index encoding, SST file
  layout — each is its own coherent translation unit.

Without v3, the INTERFACE proposal for these 5 units is forced to be coarse-grained:
one trait per file. That's likely to bake in wrong abstractions (per §6: "this is where
wrong abstractions get caught cheaply").

## Recommended next move (human decides)

Two options:

1. **Run v3 now (AST-split the 5 flagged units).** Several hours of agent work; produces
   ~70-100 truly atomic units. INTERFACE then has a tractable per-class surface. Recommend
   this if you want the doc's intended granularity.
2. **Proceed to INTERFACE on v2 with coarse stubs for the 5 megafiles.** Cheaper now;
   reserves the right to refine. Risk: the coarse stubs may need substantial revision
   once finer detail emerges.

## Artifacts

- `storage/slatedb/migration/index.py` — same tool, updated to per-file units
- `storage/slatedb/migration/manifest.json` — 45 units, 45 acyclic clusters
- `storage/slatedb/migration/reports/index-v1.md` — v1 report (kept for diff)
- `storage/slatedb/migration/reports/index-v2.md` — this file
