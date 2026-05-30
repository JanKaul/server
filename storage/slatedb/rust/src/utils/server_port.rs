//! MariaDB server-port compat: regex-list pattern matcher and key-duplicate
//! formatting.
//!
//! Translated from `storage/rocksdb/rdb_mariadb_server_port.h`. The PSI key
//! parameter on `Regex_list_handler` goes away (PSI is out of scope per
//! `_DESIGN.md §1`); the rwlock becomes `parking_lot::RwLock` and the regex
//! side uses the `regex` crate. `print_keydup_error` becomes
//! [`format_keydup_error`] which takes a pre-marshalled [`KeyDupContext`]
//! instead of `TABLE*`/`KEY*`/`THD*` (server-internal types are never
//! exposed across the cxx bridge).

use slatedb::Error;

/// Compiled-pattern list with read/write locking. Used to back sysvars such
/// as `rocksdb_skip_unique_check_tables`.
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
    /// Constructor. Default `,` matches the C++ default.
    pub fn new(delimiter: char) -> Self {
        Self {
            delimiter,
            inner: parking_lot::RwLock::new(Inner {
                patterns: Vec::new(),
                bad_pattern: String::new(),
            }),
        }
    }

    /// Replace the pattern list. Splits `patterns` on the configured
    /// delimiter, compiles each segment. On any compile failure, leaves the
    /// previous list untouched and returns `Err(Invalid)` with the offending
    /// pattern recorded in `bad_pattern()`.
    pub fn set_patterns(&self, patterns: &str) -> Result<(), Error> {
        let mut compiled = Vec::new();
        for p in patterns.split(self.delimiter) {
            match regex::Regex::new(p) {
                Ok(re) => compiled.push(re),
                Err(_) => {
                    let mut g = self.inner.write();
                    g.bad_pattern = p.to_string();
                    return Err(Error::invalid(format!("regex compile failed: {p:?}")));
                }
            }
        }
        let mut g = self.inner.write();
        g.patterns = compiled;
        g.bad_pattern.clear();
        Ok(())
    }

    /// True if any compiled pattern matches `s`.
    pub fn matches(&self, s: &str) -> bool {
        let g = self.inner.read();
        g.patterns.iter().any(|re| re.is_match(s))
    }

    pub fn bad_pattern(&self) -> String {
        self.inner.read().bad_pattern.clone()
    }
}

/// Pre-formatted context for a key-duplicate error. The cxx bridge marshals
/// the server-side `TABLE*`/`KEY*`/`THD*` into this before calling
/// [`format_keydup_error`].
#[derive(Debug, Clone)]
pub struct KeyDupContext {
    pub key_name: String,
    /// Already CSV-formatted column values.
    pub formatted_key_value: String,
    pub table_name: String,
}

/// Format a "duplicate key" message for the MariaDB diagnostics area. The
/// C++ shim hands the returned string to `my_printf_error`.
pub fn format_keydup_error(ctx: &KeyDupContext) -> Result<String, Error> {
    if ctx.key_name.is_empty() {
        return Err(Error::internal("KeyDupContext.key_name empty".into()));
    }
    Ok(format!(
        "Duplicate entry '{}' for key '{}.{}'",
        ctx.formatted_key_value, ctx.table_name, ctx.key_name
    ))
}

/// Log a warning when the most recent `set_patterns()` call rejected a
/// pattern. Identifies the offending sysvar via `name`.
pub fn warn_about_bad_patterns(handler: &RegexListHandler, name: &str) {
    let bad = handler.bad_pattern();
    if !bad.is_empty() {
        tracing::warn!(sysvar = name, pattern = %bad, "rejected bad regex pattern");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_after_set_patterns() {
        let h = RegexListHandler::new(',');
        h.set_patterns("^foo.*,bar$").expect("compile");
        assert!(h.matches("foobar"));
        assert!(h.matches("bar"));
        assert!(!h.matches("baz"));
    }

    #[test]
    fn empty_list_never_matches() {
        let h = RegexListHandler::new(',');
        assert!(!h.matches("anything"));
        assert_eq!(h.bad_pattern(), "");
    }

    #[test]
    fn bad_pattern_leaves_prior_list_intact() {
        let h = RegexListHandler::new(',');
        h.set_patterns("^good$").expect("compile");
        let err = h.set_patterns("^good$,[invalid").unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Invalid));
        // Prior list still active:
        assert!(h.matches("good"));
        assert_eq!(h.bad_pattern(), "[invalid");
    }

    #[test]
    fn successful_set_clears_bad_pattern_record() {
        let h = RegexListHandler::new(',');
        h.set_patterns("[invalid").unwrap_err();
        assert_eq!(h.bad_pattern(), "[invalid");
        h.set_patterns("^ok$").expect("compile");
        assert_eq!(h.bad_pattern(), "");
    }

    #[test]
    fn keydup_format_message() {
        let ctx = KeyDupContext {
            key_name: "PRIMARY".into(),
            formatted_key_value: "42".into(),
            table_name: "t".into(),
        };
        assert_eq!(
            format_keydup_error(&ctx).expect("format"),
            "Duplicate entry '42' for key 't.PRIMARY'"
        );
    }

    #[test]
    fn keydup_rejects_empty_key_name() {
        let ctx = KeyDupContext {
            key_name: String::new(),
            formatted_key_value: "x".into(),
            table_name: "t".into(),
        };
        assert!(format_keydup_error(&ctx).is_err());
    }
}
