# INDEX phase — v4 manifest (finer-grain AST split + I_S per-table)

**Status:** v4 split complete. All three v3-backlog sub-units split, plus the
deferred `rdb_i_s_cc` extracted per-information_schema-table. **Every sub-unit
is now ≤1000 LoC.** Awaiting human review before INTERFACE.

## What changed from v3

| Item | v3 | v4 |
|---|---|---|
| Oversized sub-units (>1000 LoC) | 3 | 0 |
| Sub-units total | 56 | 92 |
| Translation-targetable units | 96 | 132 |
| Split parents | 4 | 5 |
| `__other` catchall | 1 (2422 LoC, 77 methods) | gone — absorbed |
| `__free_functions` catchall | 1 (3153 LoC, 199 fns) | gone — split into 11 |
| Deferred backlog | 4 items | 0 |

## Four refinements

### 1. HA_ROCKSDB_GROUPS expanded (11 new buckets)

The v3 `ha_rocksdb__other` (77 methods, 2422 LoC) is replaced by 11 functional
buckets that fully absorb its contents:

```
LoC   methods  bucket
390     13     auto_incr     (auto-increment + hidden PK maintenance)
422     11     read          (read_key_*, read_row_*, prepare_*_scan, get_for_update)
121      4     iter_setup    (RocksDB iterator construction + teardown)
 90      3     ttl           (TTL expiration filters)
109      3     error         (RocksDB→MySQL error mapping)
125      7     metadata      (table/key/CF name accessors)
351     10     write_path    (update_write_*, delete_or_singledelete, locks)
 94      4     bulk_load_helpers
154      4     buffer        (key buffer alloc/free, last rowkey)
124      4     key_compare   (compare_keys, compare_key_parts, full-key checks)
442     14     table_mgmt    (update_create_info, get_range, stats, idx_cond_push, ...)
```

Total: **77 methods absorbed**, no `__other` group emitted.

### 2. ha_rocksdb.cc free functions classified (11 groups)

The v3 `____free_functions` (199 functions, 3153 LoC) split into:

```
LoC   methods  group
351     79     show_callbacks   (rocksdb_show_* + myrocks stat callbacks)
542     23     sysvar_set       (rocksdb_set_*, rocksdb_validate_*, mysql_value_to_bool)
455     17     txn_handlers     (rocksdb_commit/rollback/prepare/savepoint/...)
773      6     lifecycle        (rocksdb_init_func dominates here — it's just big)
287     19     cf_ops           (compact/flush/drop_index/create_checkpoint/...)
 74     12     dbug_helpers     (dbug_* and rdb_dbug_*)
215     12     error_helpers    (print_*, format_string, rdb_xid_*, ...)
 32      3     status_helpers   (show_status, open_table_names, perf_counters)
104      5     name_helpers     (rdb_normalize_tablename / dir / collation)
 42      5     table_version    (lookup_key, save/get/delete table_version)
278     18     accessors        (rdb_get_*, rdb_is_*, misc small helpers)
```

Total: **199 functions classified**, zero in `__misc`.

### 3. Rdb_key_def split by codec direction

The v3 `Rdb_key_def` (59 methods, 2015 LoC) split into:

```
LoC   methods  sub-unit
450    10     Rdb_key_def__encode   (pack_*, write_index_flag_field)
889    30     Rdb_key_def__decode   (unpack_*, make_unpack_*, skip_*, calc_unpack_*, ...)
676    19     Rdb_key_def__meta     (setup, gen_*, successor/predecessor, compare, ...)
```

Decode is the largest (889 LoC) but acceptable. If INTERFACE struggles with it,
v5 could further split decode by field-type family (int / float / str / varchar).

### 4. rdb_i_s.cc per-table extraction (13 tables)

```
LoC   decls  table
284     5     cfoptions
170     5     sst_props
165     5     index_file_map
163     6     global_info       (+ rdb_global_info_fill_row helper)
131     5     trx_info
119     5     deadlock_info
108     5     perf_context
105     5     cfstats
101     5     dbstats
 96     4     compact_stats
 88     5     lock_info
 87     5     ddl
 75     5     perf_context_global
 19     3     __shared          (rdb_i_s_info, rdb_i_s_deinit, rdb_filename_without_path)
```

Per-table classifier matches `rdb_i_s_<table>_*` (preferred) and `rdb_<table>_*`
(catches helpers like `rdb_global_info_fill_row`). Three namespaces use legacy
names (`RDB_LOCKS_FIELD` → `lock_info`, `RDB_TRX_FIELD` → `trx_info`,
`RDB_DEADLOCK_FIELD` → `deadlock_info`) and are mapped via explicit alias table.

## Final size distribution

```
LoC bucket   sub-units
<100         40
100-300      27
300-500      13
500-800      10
800-1000     2     (ha_rocksdb_h__ha_rocksdb 850, Rdb_key_def__decode 889)
>1000        0
```

The two 800-1000 sub-units are inherent: `ha_rocksdb` is one giant C++ class
declaration (a header sub-unit, not splittable without restructuring the
class), and `Rdb_key_def__decode` is the codec read-path which is naturally
larger than the write-path. Neither blocks INTERFACE.

## Largest non-parent v2 units (untouched by v3/v4)

These were already under 1000 LoC in v2 and don't need splitting:

```
LoC   unit
1071  ha_rocksdb_h           (parent — split in v4)
1639  rdb_datadic_h          (parent — split in v4)
1975  rdb_i_s_cc             (parent — split in v4)
5439  rdb_datadic_cc         (parent — split in v4)
14791 ha_rocksdb_cc          (parent — split in v4)
 549  rdb_buff_h             (clean leaf header)
 396  rdb_global_h           (clean leaf header)
```

## What v4 buys for INTERFACE

The full 132-unit set is now uniformly translation-targetable:

- **No outsized "design later" units.** Every sub-unit fits a single Rust
  trait or module with a coherent purpose.
- **`ha_rocksdb` handler split into 22 method buckets** (lifecycle/DDL/DML/scan/
  index/info/alter/txn/repair/convert/locks/auto_incr/read/iter_setup/ttl/error/
  metadata/write_path/bulk_load_helpers/buffer/key_compare/table_mgmt) — INTERFACE
  can propose one Rust trait per bucket and verify each independently.
- **Codec direction is a Rust-natural split.** `Rdb_key_def__encode` becomes a
  `KeyEncoder` trait; `__decode` becomes a `KeyDecoder` trait; `__meta` is the
  shared metadata side (CF name parsing, TTL extraction, key-comparison).
- **I_S tables are independent.** Each is a separate `InformationSchemaTable`
  Rust impl that fills a row vector — they share only the deinit hook.

## Indexer correctness notes

- Same `body_loc` (sum of body extents, not span) and path normalization as v3.
- Free function classification uses prefix vs exact-name patterns
  (patterns ending in `_` are prefixes); first-match-wins.
- I_S table classification uses two prefix forms (`rdb_i_s_<table>_` preferred,
  `rdb_<table>_` fallback) plus a hand-maintained namespace alias for the three
  legacy-named namespaces.

## v4 backlog

None. The doc's INDEX phase target (every translation unit ≤2× LoC budget)
is met. If INTERFACE later finds a sub-unit too coarse, a v5 with surgical
splits (e.g., decode-by-fieldtype) can be added then.

## Artifacts

- `storage/slatedb/migration/index_v4.py` — splitter (~380 lines, libclang)
- `storage/slatedb/migration/manifest.json` — 45 v2 units + 92 v4 sub-units
- `storage/slatedb/migration/reports/index-v[1,2,3,4].md` — phase reports

To re-run after MyRocks file changes:
```sh
cd build && cmake . -DCMAKE_EXPORT_COMPILE_COMMANDS=ON -DPLUGIN_ROCKSDB=DYNAMIC
cd .. && python3 storage/slatedb/migration/index.py     # v2 base manifest
python3 storage/slatedb/migration/index_v4.py           # v4 split (supersedes v3)
```

## Recommended next step

**INTERFACE phase (§6).** Propose safe-Rust interface stubs across all 132 units
in topological order: leaf headers first (~12 units), then `ha_rocksdb_h__ha_rocksdb`
(the hub) with its 22 handler-method-bucket traits, then dependent headers
(~14 units), then impl sub-units. Per §6 of the doc, this is one batch for
human review before TRANSLATE.
