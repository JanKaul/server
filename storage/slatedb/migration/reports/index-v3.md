# INDEX phase — v3 manifest (AST-split megafiles)

**Status:** v3 split complete. 4 of 5 needs_split units broken into sub-units via
libclang. `rdb_i_s_cc` deferred to v4. Awaiting human review before INTERFACE.

## What changed from v2

`index_v3.py` libclang-parses each needs_split parent, walks the AST, and emits
sub-units by `(class, optional functional-group)`. For `ha_rocksdb`'s 155-method
handler class specifically, methods are grouped by name-prefix into 11 functional
buckets (lifecycle / DDL / DML / scan / index / info / alter / txn / repair /
convert / locks).

`rdb_i_s_cc` is intentionally not split here — its structure is per-information_
schema-table, driven by macros, not per-class. A v4 with custom per-table extraction
would be needed (deferred until INTERFACE shows whether it's actually required).

## Headline

- **56 sub-units** across **4 parents** (`ha_rocksdb_h`, `ha_rocksdb_cc`,
  `rdb_datadic_h`, `rdb_datadic_cc`). Plus the 5th parent (`rdb_i_s_cc`)
  remaining as one unit pending v4.
- Combined with the 45 v2 units (5 of which became parents), the manifest now
  effectively has **96 translation-targetable units**.
- **3 sub-units still over 2× target** (1000 LoC):
  - `ha_rocksdb_cc____free_functions` — 3,153 LoC across 199 free functions
    (sysvars, helper utilities). Could be further split by section.
  - `ha_rocksdb_cc__ha_rocksdb__other` — 2,422 LoC across 77 methods my naming
    prefixes didn't match. Refine the `HA_ROCKSDB_GROUPS` table to absorb these.
  - `rdb_datadic_cc__Rdb_key_def` — 2,015 LoC across 59 methods. The key
    definition codec; would split further by codec direction (encode/decode).

These 3 are the v4 backlog. Everything else is ≤1000 LoC.

## ha_rocksdb_cc breakdown (the headline file)

```
body_loc  methods  sub-unit
3153      199      ha_rocksdb_cc____free_functions          [needs v4 split]
2422      77       ha_rocksdb_cc__ha_rocksdb__other         [needs v4 split]
 582      48       ha_rocksdb_cc__Rdb_transaction
 561       9       ha_rocksdb_cc__ha_rocksdb__ddl
 533      13       ha_rocksdb_cc__ha_rocksdb__index
 519      10       ha_rocksdb_cc__ha_rocksdb__repair
 433       6       ha_rocksdb_cc__ha_rocksdb__lifecycle
 372       4       ha_rocksdb_cc__ha_rocksdb__alter
 347       6       ha_rocksdb_cc__ha_rocksdb__info
 307      29       ha_rocksdb_cc__Rdb_transaction_impl
 305       7       ha_rocksdb_cc__ha_rocksdb__scan
 149       9       ha_rocksdb_cc__Rdb_snapshot_status
 145      25       ha_rocksdb_cc__Rdb_writebatch_impl
 127       4       ha_rocksdb_cc__ha_rocksdb__dml
 ...      ...      ...  (10 more, all ≤126 LoC)
```

The doc said "expect ~20-30 units from `ha_rocksdb.cc` alone." We got 23 (counting
parent). Right ballpark.

## rdb_datadic_cc breakdown

```
body_loc  methods  sub-unit
2015      59       rdb_datadic_cc__Rdb_key_def              [needs v4 split]
 603      33       rdb_datadic_cc__Rdb_dict_manager
 551      19       rdb_datadic_cc__Rdb_ddl_manager
 311       3       rdb_datadic_cc__Rdb_field_packing
 236      11       rdb_datadic_cc____free_functions
 174       6       rdb_datadic_cc__Rdb_binlog_manager
 ...      ...      ...
```

Clean per-class split. `Rdb_key_def` is the codec core (encode/decode for primary
and secondary keys); v4 would split by codec direction.

## What v3 buys for INTERFACE

INTERFACE phase can now propose Rust interfaces at the sub-unit level for the four
exploded parents. Concretely, for `ha_rocksdb_cc`:

- One Rust trait covering the handler vtable, with method groups roughly matching
  the functional sub-units (so the INTERFACE proposal can be organized as: "DDL
  methods → Rust trait `Ddl`, DML methods → trait `Dml`, ...").
- Separate Rust types for each helper class (`Rdb_transaction`, `Rdb_writebatch_impl`,
  `Rdb_snapshot_status`, etc.) — these aren't part of the handler vtable but provide
  the engine's transaction substrate.

For `rdb_datadic_cc`:

- The key-def codec gets its own Rust trait/struct ecosystem (still needs further
  v4 split since `Rdb_key_def` alone is 2k LoC).
- Dict/DDL managers each become coherent Rust modules.

## Indexer correctness notes

- `body_loc` = sum of method body LoC for the sub-unit (not the file-span). This
  is the actually useful number — span was misleading when methods of one class
  were scattered across thousands of lines.
- Path normalization: libclang reports header paths with `./` components from
  `#include "./foo.h"`. Both sides of the filter compare with `Path.resolve()` to
  handle this.
- Header AST extraction: headers aren't standalone TUs; we parse the paired `.cc`
  as the TU and filter AST nodes by the header's resolved path.

## Recommended next step

INTERFACE phase (§6). Three sub-units are still oversized but tractable: the
INTERFACE proposal for each can use sub-grouping comments or be done in 2-3 passes.
Defer v4 (per-table for rdb_i_s_cc, codec-direction split for Rdb_key_def, name-prefix
refinement for ha_rocksdb__other) until INTERFACE actually struggles with the current
granularity.

## Artifacts

- `storage/slatedb/migration/index_v3.py` — AST splitter (~200 lines, libclang-based)
- `storage/slatedb/migration/manifest.json` — now includes `sub_units` section (56 entries)
- `storage/slatedb/migration/reports/index-v[1,2,3].md` — phase reports

To re-run after MyRocks file changes:
```sh
cd build && cmake . -DCMAKE_EXPORT_COMPILE_COMMANDS=ON -DPLUGIN_ROCKSDB=DYNAMIC
cd .. && python3 storage/slatedb/migration/index.py     # base manifest
python3 storage/slatedb/migration/index_v3.py           # AST split (~3 min)
```
