# Stage 0 — Checkpoint 3: SlateDB runs in our toolchain

**Status:** Passed.

## What landed

- `Cargo.toml`: `slatedb = "=0.13.1"` (default features: aws, foyer), `object_store = "0.12"` with `aws` feature, `tokio` with `rt-multi-thread` + `macros` + `sync`.
- Dev-deps: `testcontainers = "0.27"`, `testcontainers-modules = "0.15"` (minio feature), `aws-sdk-s3` and `aws-config` (both `default-features = false`, `rt-tokio` + `rustls`) — used only to create the bucket since `object_store` doesn't expose bucket-creation.
- `rust/tests/slatedb_smoke.rs` — single `#[tokio::test]` that spawns Minio in podman, creates `slatedb-test` bucket, opens SlateDB against `s3://slatedb-test/checkpoint3/`, does `put(b"hello", b"world")` → `get(b"hello")` → `close()`. Asserts the value round-trips.

## Verification

```
$ cargo test --test slatedb_smoke -- --nocapture
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.24s
     Running tests/slatedb_smoke.rs
running 1 test
test put_get_roundtrip_against_minio ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 20.29s
```

20 s total: ~15 s container startup (image pull cached on first run; ~100 MB if cold), ~5 s SlateDB open + put + get + close.

## Choices that may want revisiting

- **`aws-sdk-s3` as a dev-dep just for bucket creation.** Pulls a sizable transitive tree but it's well-tested and dev-only. Alternative: roll a 30-line SigV4 PUT via `reqwest`/`rusty-s3`. Worth considering if dev-dep weight ever hurts CI build times.
- **Default Minio credentials hardcoded** (`minioadmin`/`minioadmin`). Fine for a unit test; do not copy this pattern into integration tests that touch shared infra.
- **`force_path_style(true)` on the s3 client** — needed for Minio's default URL layout. The matching `with_allow_http(true)` on `AmazonS3Builder` is also required. Both will need to flip when we eventually point at real S3.

## Stage 0 status

| Checkpoint | Level A (compile) | Level B (runtime) |
|---|---|---|
| 1: empty plugin loads | ✅ | deferred |
| 2: cxx wired end-to-end | ✅ | deferred |
| 3: SlateDB runs | ✅ (cargo test) | N/A (no MariaDB integration yet) |

## Next: end-of-Stage-0 Level B verification

Two things now warrant a real `INSTALL PLUGIN slatedb` run against a from-source `mariadbd`:

1. Checkpoint 2 added a Rust staticlib + cxx runtime into the plugin .so. A symbol-resolution surprise at `dlopen` time (e.g., `__rust_alloc` collisions, libstdc++ ABI drift) won't show up at compile-time but will at load time.
2. The plugin is no longer a 2 MB stub; it's a 10 MB linker product. The `INSTALL PLUGIN` failure mode here is more interesting than for a stub.

Recommend: build `mariadbd` from this 13.0.1 tree (30-90 min one-shot), initialize a throwaway datadir, run `INSTALL PLUGIN slatedb SONAME 'ha_slatedb.so'; SHOW ENGINES;`. If it lists SLATEDB, Stage 0 is fully gate-green and we can move to INDEX/INTERFACE.

Open question for the human: do the Level B build now (before INDEX), or in parallel with the INDEX phase (§5) which doesn't depend on a built `mariadbd`?
