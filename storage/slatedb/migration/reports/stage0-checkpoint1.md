# Stage 0 — Checkpoint 1: Empty plugin builds

**Status:** Level A passed. Level B (runtime `INSTALL PLUGIN` verification) deferred.

## What landed

- `storage/slatedb/CMakeLists.txt` — `MYSQL_ADD_PLUGIN(slatedb shim/ha_slatedb.cc STORAGE_ENGINE MODULE_ONLY COMPONENT Storage)`.
- `storage/slatedb/shim/ha_slatedb.{h,cc}` — minimal handlerton + handler subclass copied/trimmed from `storage/example/`. All required vtable methods stubbed. No share, no options, no sysvars.

## Build configuration

```
cmake .. \
  -DCMAKE_BUILD_TYPE=Debug \
  -DPLUGIN_SLATEDB=DYNAMIC \
  -DPLUGIN_ROCKSDB=NO -DPLUGIN_TOKUDB=NO -DPLUGIN_MROONGA=NO \
  -DPLUGIN_SPIDER=NO -DPLUGIN_CONNECT=NO -DPLUGIN_OQGRAPH=NO \
  -DPLUGIN_COLUMNSTORE=NO -DPLUGIN_SPHINX=NO -DPLUGIN_PERFSCHEMA=NO \
  -DWITH_WSREP=OFF -DWITH_JEMALLOC=NO -DWITH_UNIT_TESTS=OFF \
  -DMYSQL_MAINTAINER_MODE=OFF
make -j$(nproc) slatedb
```

`MYSQL_MAINTAINER_MODE=OFF` is required on this toolchain (gcc 16.1.1) — MariaDB 13.0.1's `include/mysql/service_encryption.h:124` uses an idiom that gcc 16 flags under `-Wparentheses`, which `-Werror` (default in maintainer mode) promotes to an error. This is a server-core issue unrelated to the plugin; per §0.1 we don't patch MariaDB, we disable the flag at the build level.

## Verification (Level A)

- Build: `[100%] Built target slatedb`
- Artifact: `build/storage/slatedb/ha_slatedb.so` (~2.2 MB, debug)
- Symbols: `_maria_plugin_declarations_`, `_maria_plugin_interface_version_` exported; all `ha_slatedb::*` vtable methods present in `nm -D`.

## Level B (deferred)

`INSTALL PLUGIN slatedb SONAME 'ha_slatedb.so'; SHOW ENGINES;` not yet run. Reasoning:

- The source tree is MariaDB 13.0.1 gamma; the Arch system package is 12.2.2 — incompatible major versions, so the system `mariadbd` cannot load our plugin.
- Building `mariadbd` from source is 30–90 min and the runtime verification of a no-op skeleton is low-information.
- Re-tighten at checkpoint 2: once the cxx bridge wires Rust into the handlerton init, a runtime load test gains real signal (it would catch Rust↔C++ symbol/ABI mismatches that compile-time can't).

## Next

Checkpoint 2 — cxx wired end-to-end. Plan:

1. `rust/Cargo.toml` with `cxx` dep, pinned `rust-toolchain.toml`.
2. `rust/src/bridge.rs` with one `#[cxx::bridge]` exposing `slatedb_version() -> String`.
3. `rust/build.rs` calling `cxx_build::bridge`.
4. `storage/slatedb/CMakeLists.txt` extended to import the Rust crate as a staticlib via corrosion-rs and link it into the slatedb plugin .so.
5. Shim's `slatedb_init_func` calls the bridge and `sql_print_information`s the version.

Open question for checkpoint 2: is corrosion-rs available on Arch (`pacman -Q corrosion`), or do we vendor it as a git subtree under `cmake/`?
