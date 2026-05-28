# Stage 0 — Checkpoint 2: cxx wired end-to-end

**Status:** Level A passed. Level B (runtime `INSTALL PLUGIN`) still deferred per the same
version-mismatch reason as checkpoint 1, but is now load-bearing — we'll batch-verify
both checkpoints at the end of Stage 0 once SlateDB is also in the picture.

## What landed

- `storage/slatedb/rust/Cargo.toml` — package `slatedb-engine`, `crate-type = ["staticlib"]`, dep `cxx = "1.0"`.
- `storage/slatedb/rust/rust-toolchain.toml` — `channel = "stable"`. Will tighten when SlateDB lands in checkpoint 3 (its MSRV becomes the floor per §4).
- `storage/slatedb/rust/src/lib.rs` — sets `#![forbid(unsafe_op_in_unsafe_fn)]` and the §0.4 clippy denies; re-exports `bridge`.
- `storage/slatedb/rust/src/bridge.rs` — `#[cxx::bridge(namespace = "slatedb")]` exposing one Rust→C++ function `slatedb_version() -> String`. Implementation returns `format!("slatedb-engine {}", env!("CARGO_PKG_VERSION"))`.
- `storage/slatedb/CMakeLists.txt` — `find_package(Corrosion REQUIRED)`, `corrosion_import_crate`, `corrosion_add_cxxbridge`, `MYSQL_ADD_PLUGIN ... LINK_LIBRARIES slatedb_bridge`.
- `storage/slatedb/shim/ha_slatedb.cc` — `#include "slatedb_bridge/bridge.h"`, `slatedb_init_func` calls `slatedb::slatedb_version()` and logs via `sql_print_information`.

## Gotchas discovered

- **Corrosion normalizes hyphens.** A package named `slatedb-engine` is exposed to CMake as target `slatedb_engine`. `corrosion_add_cxxbridge(... CRATE <name> ...)` must use the underscore form. The hyphen form gives `CMake Error: get_target_property() called with non-existent target`. This is `string(REPLACE "\-" "_" ...)` at CorrosionGenerator.cmake:142 and is intentional — see corrosion-rs issue #501.
- **cxxbridge-cmd auto-install.** No system package on Arch; corrosion `cargo install`s it under `build/corrosion/cxxbridge_v1.0.194/`. First build takes ~30 s extra; subsequent builds reuse.
- **Header include path.** With `corrosion_add_cxxbridge(slatedb_bridge ...)`, the generated header is at `${binary_dir}/corrosion_generated/cxxbridge/slatedb_bridge/include/slatedb_bridge/bridge.h`. C++ includes it as `"slatedb_bridge/bridge.h"`. The include dir is on the target as PUBLIC, so `MYSQL_ADD_PLUGIN ... LINK_LIBRARIES slatedb_bridge` propagates it automatically.

## Build configuration

Same cmake invocation as checkpoint 1 — no new flags. `MYSQL_MAINTAINER_MODE=OFF` still
required (server-core gcc-16 issue, unchanged).

## Verification (Level A)

- Build: `[100%] Built target slatedb` after `[100%] Linking CXX shared module ha_slatedb.so`.
- Artifact: `build/storage/slatedb/ha_slatedb.so` (~10 MB, debug, up from 2.2 MB — cxx runtime and Rust staticlib pulled in).
- Symbols (`nm`):
  - `_ZN7slatedb15slatedb_versionEv` — C++ caller-side `slatedb::slatedb_version()`.
  - `slatedb$cxxbridge1$194$slatedb_version` — cxx extern "C" wrapper.
  - `_ZN14slatedb_engine6bridge15slatedb_version17h...E` — Rust implementation.
- All three layers of the bridge present in one .so. ABI version `1.0.194` is baked into the symbol name; if the C++ side and Rust side ever drift on cxx version, the link will fail loudly.

## Level B (still deferred)

Same blocker as checkpoint 1: source tree 13.0.1 gamma vs Arch package 12.2.2 means we
need a from-source mariadbd to runtime-verify. Decision: batch Level B verification at the
end of Stage 0 (after checkpoint 3 also lands) — one mariadbd build, one `INSTALL PLUGIN`
run, covers all three checkpoints' runtime gates at once.

## Next

Checkpoint 3 — SlateDB runs in a Rust unit test. Plan:

1. Add `slatedb = "<pinned-version>"` to `rust/Cargo.toml`. Pick the dep source (crates.io vs git rev) — open question for the human.
2. Add a `tests/` module or `#[cfg(test)]` block that constructs a SlateDB instance against a local filesystem object-store backend, does one `put`/`get` roundtrip, and asserts.
3. `cargo test -p slatedb-engine` must pass without network.
4. No MariaDB integration yet — that's Stage 1.

Open questions for checkpoint 3:
- **SlateDB version source.** crates.io (if published) or git rev (pinned)? Doc §4 says "pin a specific git rev or crate version".
- **Object-store backend for tests.** Local fs adapter (cheapest), Minio (more realistic), or in-memory? Doc §11.Stage 0 preconditions left this open.
