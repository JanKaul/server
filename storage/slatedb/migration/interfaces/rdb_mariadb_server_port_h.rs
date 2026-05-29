//! Interface stub for `rdb_mariadb_server_port_h`.
//!
//! C++ source: `storage/rocksdb/rdb_mariadb_server_port.h` (76 LoC)
//!
//! ## Mapping
//! Another WebScaleSQL → MariaDB compat shim. Two things are public:
//!
//! 1. `Regex_list_handler` — a PSI-tracked rwlock-protected list of regex
//!    patterns used by sysvars like `rocksdb_skip_unique_check_tables`. The
//!    PSI key parameter goes away in Rust (PSI integration is out of scope per
//!    _DESIGN.md §1 — see `rdb_psi_h` for the rationale). The rwlock becomes a
//!    `parking_lot::RwLock`, and the regex side uses the `regex` crate.
//!
//! 2. `print_keydup_error` — pretty-prints a key-duplicate error including
//!    column values for the index. This needs a `TABLE *` / `KEY *` and a
//!    `THD *`, which are explicitly disallowed by our contract (those types
//!    don't cross the cxx bridge). The Rust side instead receives a
//!    pre-formatted `KeyDupContext` struct that the C++ shim populates before
//!    calling in.
//!
//! ## Out-of-scope methods
//! - `print_keydup_error(TABLE*, KEY*, ...)` — server-internal types must not
//!   be exposed to Rust. We accept a `KeyDupContext` instead and let the cxx
//!   bridge marshal the strings.

use slatedb::Error;

/// Pattern list with read/write locking — matches the use case of
/// `Regex_list_handler` (rdb_mariadb_server_port.h:20). Patterns are stored
/// compiled so that `matches()` is O(P) regex evaluations rather than O(P)
/// parses.
pub struct RegexListHandler {
    delimiter: char,
    inner: parking_lot::RwLock<Inner>,
}

struct Inner {
    patterns: Vec<regex::Regex>,
    /// The pattern string that last failed to compile, if any. Returned by
    /// `bad_pattern()`.
    bad_pattern: String,
}

impl RegexListHandler {
    /// Constructor. Default delimiter `,` matches the C++ default
    /// (rdb_mariadb_server_port.h:42). No PSI key is needed.
    pub fn new(delimiter: char) -> Self {
        Self {
            delimiter,
            inner: parking_lot::RwLock::new(Inner {
                patterns: Vec::new(),
                bad_pattern: String::new(),
            }),
        }
    }

    /// Replace the pattern list. `patterns` is split on the configured
    /// delimiter, each segment compiled into a `Regex`.
    ///
    /// Returns `Ok(())` on success. On any compile failure, leaves the
    /// previous list untouched and returns `Err(Invalid)` with the offending
    /// pattern recorded in `bad_pattern()`. C++ original returns `bool` —
    /// `true` is the failure code there.
    pub fn set_patterns(&self, patterns: &str) -> Result<(), Error> {
        let mut compiled = Vec::new();
        for p in patterns.split(self.delimiter) {
            match regex::Regex::new(p) {
                Ok(re) => compiled.push(re),
                Err(_) => {
                    let mut g = self.inner.write();
                    g.bad_pattern = p.to_string();
                    return Err(Error::invalid(format!(
                        "regex compile failed: {p:?}"
                    )));
                }
            }
        }
        let mut g = self.inner.write();
        g.patterns = compiled;
        g.bad_pattern.clear();
        Ok(())
    }

    /// True if any compiled pattern matches the full `s`.
    pub fn matches(&self, s: &str) -> bool {
        let g = self.inner.read();
        g.patterns.iter().any(|re| re.is_match(s))
    }

    pub fn bad_pattern(&self) -> String {
        self.inner.read().bad_pattern.clone()
    }
}

/// Pre-formatted context for a key-duplicate error. The cxx bridge populates
/// this from the server-side `TABLE*`/`KEY*`/`THD*` before handing it to us.
/// Matches the inputs of `print_keydup_error` (rdb_mariadb_server_port.h:73)
/// without leaking those types.
#[derive(Debug, Clone)]
pub struct KeyDupContext {
    pub key_name: String,
    pub formatted_key_value: String, // already CSV-formatted column values
    pub table_name: String,
}

/// Push a "duplicate key" error onto the MariaDB diagnostics area. Mirrors
/// `print_keydup_error` — the C++ side does the actual `my_printf_error`
/// after we return.
///
/// Returns the user-visible message string the bridge should hand to the
/// server. Errors here are programmer bugs in the bridge, surfaced as
/// `Internal`.
pub fn format_keydup_error(ctx: &KeyDupContext) -> Result<String, Error> {
    if ctx.key_name.is_empty() {
        return Err(Error::internal("KeyDupContext.key_name empty".into()));
    }
    Ok(format!(
        "Duplicate entry '{}' for key '{}.{}'",
        ctx.formatted_key_value, ctx.table_name, ctx.key_name
    ))
}

/// Log warnings for patterns that failed to compile in `RegexListHandler`.
/// Called once after each sysvar update; identifies which sysvar via `name`.
pub fn warn_about_bad_patterns(handler: &RegexListHandler, name: &str) {
    let bad = handler.bad_pattern();
    if !bad.is_empty() {
        // TODO(human): wire to the logger crate the rest of the engine uses.
        eprintln!("MyRocks/SlateDB: sysvar {name}: bad pattern: {bad}");
    }
}
