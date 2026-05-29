//! Interface stub for `rdb_mariadb_server_port_cc`.
//!
//! C++ source: `storage/rocksdb/rdb_mariadb_server_port.cc` (123 LoC)
//!
//! ## Mapping
//! Thin MariaDB-vs-MySQL compatibility shim. Two distinct pieces of
//! functionality:
//!
//! - **`Regex_list_handler`** — wraps `std::regex` with an `mysql_rwlock_t`
//!   so a sysvar holding "exception patterns" (`rocksdb_read_free_rpl_tables`
//!   etc.) can be hot-swapped. Maps onto a `parking_lot::RwLock<regex::Regex>`
//!   in Rust.
//! - **`split_into_vector`** — comma-split string utility; maps to
//!   `str::split(delim).filter(non_empty).collect::<Vec<_>>()`.
//! - **`warn_about_bad_patterns`** — logs invalid regex; gone (we use
//!   the engine `log` crate's `warn!` macro at the caller).
//!
//! The original file exists only because MariaDB has slightly different
//! `mysql_*` macros than MySQL; in Rust the divergence vanishes entirely.
//!
//! ## Out-of-scope methods
//! - `mysql_rwlock_*` API surface — replaced by `parking_lot::RwLock`.

use parking_lot::RwLock;
use regex::Regex;
use slatedb::Error;

/// Hot-swappable regex list. Replaces `Regex_list_handler`.
///
/// The C++ version stored multiple patterns joined by `|` and matched with
/// `std::regex_match` (anchored match). We do the same. Reads
/// (`matches`) take the read lock; writes (`set_patterns`) take the write
/// lock and recompile.
///
/// Original: rdb_mariadb_server_port.cc:37 — `Regex_list_handler`.
pub struct RegexListHandler {
    /// Logical delimiter the user supplies (e.g., `,` or `;`). The C++
    /// version takes this as a constructor arg.
    pub delimiter: char,
    /// Current compiled pattern, `None` if all patterns rejected as bad.
    pattern: RwLock<Option<Regex>>,
    /// Last bad pattern string (for diagnostics). Empty if last set
    /// succeeded.
    bad_pattern_str: RwLock<String>,
}

impl RegexListHandler {
    /// Construct with the given delimiter. Initial pattern is empty
    /// (matches nothing).
    /// Original: rdb_mariadb_server_port.h — ctor.
    pub fn new(delimiter: char) -> Self {
        Self {
            delimiter,
            pattern: RwLock::new(None),
            bad_pattern_str: RwLock::new(String::new()),
        }
    }

    /// Replace the pattern set.
    ///
    /// Inputs: `pattern_str` — delimiter-separated regexes.
    ///
    /// Output: `true` if all patterns compiled cleanly. On `false`, the
    /// previous pattern remains in place and `bad_pattern()` returns the
    /// offending string.
    ///
    /// Errors: never returns `Err` — bad input is reported via the bool +
    /// `bad_pattern()` getter (preserves C++ contract).
    ///
    /// Original: rdb_mariadb_server_port.cc:37 — `set_patterns`.
    pub fn set_patterns(&self, pattern_str: &str) -> bool {
        let normalized: String = pattern_str
            .chars()
            .map(|c| if c == self.delimiter { '|' } else { c })
            .collect();
        match Regex::new(&normalized) {
            Ok(re) => {
                *self.pattern.write() = Some(re);
                self.bad_pattern_str.write().clear();
                true
            }
            Err(_) => {
                *self.bad_pattern_str.write() = pattern_str.to_string();
                false
            }
        }
    }

    /// Return the last rejected pattern string, if any.
    /// Original: rdb_mariadb_server_port.h — `bad_pattern()`.
    pub fn bad_pattern(&self) -> String {
        self.bad_pattern_str.read().clone()
    }

    /// Anchored match — equivalent to `std::regex_match` (not `regex_search`).
    /// Returns `false` if no pattern was ever set.
    ///
    /// Original: rdb_mariadb_server_port.cc:79 — `matches`.
    pub fn matches(&self, s: &str) -> bool {
        match &*self.pattern.read() {
            Some(re) => {
                // anchor: regex crate's `is_match` is unanchored; emulate
                // std::regex_match by requiring the whole string to match.
                if let Some(m) = re.find(s) {
                    m.start() == 0 && m.end() == s.len()
                } else {
                    false
                }
            }
            None => false,
        }
    }
}

/// Split a string by delimiter, skipping empty segments (matches C++
/// `split_into_vector` semantics: two consecutive delimiters yield one
/// gap, not an empty string).
///
/// Inputs: `input`, `delimiter`.
/// Output: vec of owned non-empty substrings.
///
/// Original: rdb_mariadb_server_port.cc:97 — `split_into_vector`.
pub fn split_into_vector(input: &str, delimiter: char) -> Vec<String> {
    input
        .split(delimiter)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Compatibility no-op — the C++ helper printed `sql_print_warning(...)`. In
/// Rust we expect callers to use `log::warn!` directly, but we provide this
/// shim so blind-translated call sites compile.
///
/// Original: rdb_mariadb_server_port.cc:21 — `warn_about_bad_patterns`.
pub fn warn_about_bad_patterns(handler: &RegexListHandler, name: &str) -> Result<(), Error> {
    log::warn!(
        "Invalid pattern in {}: {}",
        name,
        handler.bad_pattern()
    );
    Ok(())
}
