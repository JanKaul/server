//! Interface stub for `rdb_utils_h`.
//!
//! C++ source: `storage/rocksdb/rdb_utils.h` (335 LoC)
//!
//! ## Mapping
//! Mostly free functions: string-parsing helpers, a couple of MyRocks-specific
//! constants, and the `Ensure_cleanup` RAII guard. The macros at the top of
//! the C++ header (`HA_EXIT_SUCCESS`, `DBUG_ENTER_FUNC`, `SHIP_ASSERT`, etc.)
//! are pure preprocessor and don't cross into Rust.
//!
//! - **Parsers** (`rdb_skip_spaces`, `rdb_parse_id`, `rdb_check_next_token`,
//!   `rdb_skip_id`, `rdb_find_in_string`, `parse_into_tokens`,
//!   `rdb_compare_strings_ic`): preserved as Rust fns. The C++ side uses
//!   `CHARSET_INFO*` for case-insensitive comparisons; the Rust side uses
//!   `eq_ignore_ascii_case` — adequate because the call sites only pass
//!   ASCII sysvar names.
//!
//! - **`Ensure_cleanup`**: replaced by an idiomatic Rust `Drop` impl —
//!   constructor takes a `FnOnce()` closure, `skip()` consumes it.
//!
//! - **`rdb_hexdump`**: small fmt helper; preserved as a Rust fn returning
//!   `String`.
//!
//! - **`rdb_log_status_error`**, **`rdb_check_rocksdb_corruption`**,
//!   **`rdb_persist_corruption_marker`**, **`rdb_database_exists`**,
//!   **`get_rocksdb_supported_compression_types`**, **`purge_all_jemalloc_arenas`**:
//!   these depend on RocksDB types or on the global server filesystem. We
//!   re-target them at SlateDB: corruption-marker writes a file via the
//!   configured `ObjectStore`; supported compression returns the
//!   `slatedb::CompressionCodec` feature flags enabled in our build.
//!
//! ## Out-of-scope methods
//! - `rdb_check_mutex_call_result` — wraps `mysql_mutex_lock` error checking.
//!   Rust uses `parking_lot::Mutex` whose `lock()` never fails; replaced by
//!   the lock itself.
//! - `purge_all_jemalloc_arenas` — Rust manages its own allocator; we don't
//!   expose jemalloc-specific knobs. Returns `Ok(())` as a no-op.

use slatedb::Error;
use std::sync::Arc;

// --- naming / numeric constants (rdb_utils.h:165 + others) ---

pub const RDB_MAX_HEXDUMP_LEN: usize = 1000;
pub const HA_EXIT_SUCCESS: i32 = 0;
pub const HA_EXIT_FAILURE: i32 = 1;

// --- string parsers ---

/// Skip leading whitespace, return the trimmed `&str`. C++
/// `rdb_skip_spaces(cs, str)`. The charset argument is dropped because all
/// call sites pass `system_charset_info` and the tokens are ASCII.
pub fn skip_spaces(s: &str) -> &str {
    s.trim_start()
}

/// Case-insensitive ASCII string compare. C++ `rdb_compare_strings_ic`.
pub fn compare_strings_ic(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Find `pattern` (literal substring) in `str`; return the byte offset just
/// past the match if found, else `None`. The third C++ out-param `succeeded`
/// is collapsed into the `Option` return.
/// C++ `rdb_find_in_string`.
pub fn find_in_string(s: &str, pattern: &str) -> Option<usize> {
    s.find(pattern).map(|i| i + pattern.len())
}

/// If `s[pos..]` (after skipping spaces) starts with `pattern`, advance past
/// it and return the new offset; else return `None`. C++
/// `rdb_check_next_token`.
pub fn check_next_token<'a>(s: &'a str, pattern: &str) -> Option<&'a str> {
    let s = skip_spaces(s);
    s.strip_prefix(pattern)
}

/// Parse a SQL identifier from the front of `s`. Returns `(remainder, id)`
/// if a valid identifier was at the start, else `None`. C++ `rdb_parse_id`.
///
/// Accepts both unquoted (`[A-Za-z_][A-Za-z0-9_]*`) and backquoted forms.
pub fn parse_id(s: &str) -> Option<(&str, String)> {
    todo!("port rdb_parse_id from rdb_utils.cc — handle both backquoted and plain idents")
}

/// Skip a SQL identifier (parse + discard the returned id).
pub fn skip_id(s: &str) -> Option<&str> {
    parse_id(s).map(|(rest, _)| rest)
}

/// Split `s` on `delim`, trimming whitespace from each token. Empty tokens
/// are dropped. C++ `parse_into_tokens`.
pub fn parse_into_tokens(s: &str, delim: char) -> Vec<String> {
    s.split(delim)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

// --- hexdump ---

/// Hex-dump `data`, capped at `maxsize` (0 = unlimited; otherwise capped to
/// `min(data.len(), maxsize)`). C++ `rdb_hexdump`.
pub fn hexdump(data: &[u8], maxsize: usize) -> String {
    let n = if maxsize == 0 { data.len() } else { data.len().min(maxsize) };
    let mut out = String::with_capacity(n * 2);
    for b in &data[..n] {
        out.push_str(&format!("{b:02x}"));
    }
    if n < data.len() {
        out.push_str("...");
    }
    out
}

// --- RAII cleanup guard (replaces C++ `Ensure_cleanup`) ---

/// RAII guard that runs `cleanup` on drop unless `skip()` was called. Direct
/// translation of C++ `Ensure_cleanup` (rdb_utils.h:317) using Rust's native
/// Drop trait — strictly safer than the C++ original because the closure
/// can't accidentally outlive captured references.
pub struct EnsureCleanup<F: FnOnce()> {
    cleanup: Option<F>,
}

impl<F: FnOnce()> EnsureCleanup<F> {
    pub fn new(cleanup: F) -> Self {
        Self { cleanup: Some(cleanup) }
    }

    /// Disarm the cleanup. Call this when the protected operation succeeded
    /// and resources should NOT be released.
    pub fn skip(mut self) {
        self.cleanup.take();
    }
}

impl<F: FnOnce()> Drop for EnsureCleanup<F> {
    fn drop(&mut self) {
        if let Some(f) = self.cleanup.take() {
            f();
        }
    }
}

// --- SlateDB-flavored utilities (replace RocksDB-flavored ones) ---

/// Names of compression codecs compiled into our build. Drives the
/// `rocksdb_supported_compression_types` SHOW STATUS variable.
/// Replaces C++ `get_rocksdb_supported_compression_types()`.
pub fn get_supported_compression_types() -> Vec<&'static str> {
    // Feature flags follow _DESIGN.md §11 open-question 7 ("zstd as default").
    let mut v = Vec::new();
    #[cfg(feature = "snappy")] v.push("snappy");
    #[cfg(feature = "zlib")]   v.push("zlib");
    #[cfg(feature = "lz4")]    v.push("lz4");
    #[cfg(feature = "zstd")]   v.push("zstd");
    v
}

/// Persist a "this database is corrupt" marker so a restart can detect it.
/// In SlateDB world the marker is an object in the configured object store
/// at a well-known key under the system CF prefix.
///
/// Errors propagate `slatedb::Error::Unavailable` on object-store I/O failure.
pub async fn persist_corruption_marker(
    _object_store: Arc<dyn slatedb::object_store::ObjectStore>,
) -> Result<(), Error> {
    todo!("write {{slatedb_root}}/__corruption_marker__ via the ObjectStore")
}

/// Probe for the corruption marker at startup.
pub async fn check_corruption_marker(
    _object_store: Arc<dyn slatedb::object_store::ObjectStore>,
) -> Result<bool, Error> {
    todo!("HEAD {{slatedb_root}}/__corruption_marker__")
}

/// True if the named database directory exists. With SlateDB the
/// per-database namespace is purely a server-side concept (databases map
/// to filename prefixes), so the check is on the server's data dir.
pub fn database_exists(db_name: &str) -> bool {
    // TODO(human): we need the server's data-dir path from a sysvar.
    let _ = db_name;
    todo!("check `{{datadir}}/{{db_name}}` is a directory")
}

/// No-op in Rust: we don't use jemalloc directly. Returns success.
/// Replaces `purge_all_jemalloc_arenas()`.
pub fn purge_allocator_arenas() -> Result<(), Error> { Ok(()) }
