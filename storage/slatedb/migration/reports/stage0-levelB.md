# Stage 0 — End-of-stage Level B verification

**Status:** Passed. All three Stage 0 checkpoints are now tier-green at Level B as well.

## The decisive output

```
mariadb> INSTALL PLUGIN slatedb SONAME 'ha_slatedb.so'; SHOW ENGINES;
Engine    Support  Comment
SLATEDB   YES      SlateDB storage engine (Stage 0 skeleton — no storage yet)
...
```

Server log:
```
[Note] SLATEDB: cxx bridge live (slatedb-engine 0.1.0)
```

That log line is the full Stage 0 in one sentence: mariadbd dlopened the `.so`, read the
`maria_plugin_declarations_` table, called `slatedb_init_func`, which called into Rust via
cxx, which returned the version `String`, which the shim formatted via `sql_print_information`.
All three layers live, all in a real `mariadbd` process.

## Verification ritual (reproducible)

After the cmake configure from checkpoint 1 (`MYSQL_MAINTAINER_MODE=OFF` etc.):

```sh
cd build
make -j$(nproc) mariadbd mariadb my_print_defaults slatedb

# Init a throwaway datadir. --srcdir + --builddir together, NOT --basedir;
# --force to skip hostname resolution which fails in containerised env.
./scripts/mariadb-install-db \
  --srcdir=/home/work/workspace/github/server \
  --builddir=/home/work/workspace/github/server/build \
  --datadir=/tmp/slatedb-leveLB-datadir \
  --force

# Start mariadbd. Knobs that matter:
#   --plugin-maturity=experimental — our plugin is marked EXPERIMENTAL, server is gamma
#   --default-storage-engine=MyISAM + --innodb=OFF — we disabled InnoDB at build time
./sql/mariadbd --no-defaults \
  --datadir=/tmp/slatedb-leveLB-datadir \
  --plugin-dir=$PWD/storage/slatedb \
  --plugin-maturity=experimental \
  --socket=/tmp/slatedb-levelB.sock \
  --skip-networking --skip-grant-tables \
  --log-error=/tmp/slatedb-mariadbd.log --skip-log-bin \
  --default-storage-engine=MyISAM --default-tmp-storage-engine=MyISAM --innodb=OFF &

# Probe
./client/mariadb --no-defaults --socket=/tmp/slatedb-levelB.sock -e \
  "INSTALL PLUGIN slatedb SONAME 'ha_slatedb.so'; SHOW ENGINES;"

# Cleanup
./client/mariadb --no-defaults --socket=/tmp/slatedb-levelB.sock -e "SHUTDOWN;"
```

## What this rules out

- ✅ `dlopen` of the Rust-containing `.so` works against a real mariadbd process (not just `nm` symbol presence).
- ✅ `__rust_alloc` and libstdc++ ABI resolve cleanly when loaded next to mariadbd's own allocator.
- ✅ `maria_declare_plugin` macro produces a parsable plugin descriptor for v13.0.1.
- ✅ The shim → cxx → Rust path executes within `handlerton::init` without crashing the server.
- ✅ The handler vtable subclass satisfies whatever mariadbd checks at INSTALL PLUGIN time.

What this does **not** test (deliberately — that's later stages):
- Any actual storage. `CREATE TABLE ... ENGINE=SLATEDB` would succeed but yield a no-op table.
- Tokio runtime lifecycle. We haven't constructed one yet; checkpoint 2's bridge fn is sync.
- SlateDB-in-bridge. The unit test from checkpoint 3 exercises SlateDB inside `cargo test`,
  not inside a mariadbd process.

## Gotchas encountered (for the next time we do this)

- **`make mariadbd` only** is not enough for a usable in-tree test setup. Also need
  `make mariadb my_print_defaults` for the install/probe ritual.
- **`mariadb-install-db` argument trap.** `--basedir` and `--srcdir` are mutually exclusive;
  for in-tree builds you must use `--srcdir <SOURCE> --builddir <BUILD>`. The error message
  ("ERROR: Specify either --basedir or --srcdir, not both") is misleading — `--builddir` is
  the third option that's not in the error text.
- **Default storage engine.** We disabled InnoDB at cmake time; mariadbd refuses to start if
  the default storage engine is missing. `--default-storage-engine=MyISAM` (+ the tmp variant)
  is the cheap fix.
- **`--plugin-maturity` policy.** Server default is `--plugin-maturity=beta`; our plugin is
  `EXPERIMENTAL`. The first `INSTALL PLUGIN` failed with a confusing `errno: 1` message that
  said "Loading of experimental plugin SLATEDB is prohibited by --plugin-maturity=beta" — the
  `.so` actually loaded fine, the rejection was at the maturity policy gate *after* dlopen.
  Will need to revisit when we promote past `EXPERIMENTAL`.

## Stage 0 closeout

| Checkpoint | Level A | Level B |
|---|---|---|
| 1: empty plugin loads | ✅ | ✅ |
| 2: cxx wired end-to-end | ✅ | ✅ |
| 3: SlateDB runs (cargo test) | ✅ | N/A |

Stage 0 is **done**. Substrate is validated end-to-end: MariaDB build, plugin system, cxx
bridge, Rust toolchain, SlateDB-on-object-storage. The bottom turtle holds.

Ready for the INDEX phase (§5): submodule init, `compile_commands.json`, clang AST dump
over `storage/rocksdb/`, dependency graph, `manifest.json`.
