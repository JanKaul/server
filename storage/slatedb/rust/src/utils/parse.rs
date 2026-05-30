//! String parsers, hexdump, and the `EnsureCleanup` RAII guard.
//!
//! Translated from `storage/rocksdb/rdb_utils.{h,cc}`. Object-store
//! corruption-marker variants and `database_exists` are deferred until the
//! engine-init module lands (they need an `ObjectStore` handle / data-dir
//! sysvar that don't exist yet).

// --- naming / numeric constants ---

pub const RDB_MAX_HEXDUMP_LEN: usize = 1000;
pub const HA_EXIT_SUCCESS: i32 = 0;
pub const HA_EXIT_FAILURE: i32 = 1;

// --- string parsers ---

pub fn skip_spaces(s: &str) -> &str {
    s.trim_start()
}

pub fn compare_strings_ic(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Find `pattern` (literal substring) in `s`; return the byte offset just
/// past the match if found, else `None`.
pub fn find_in_string(s: &str, pattern: &str) -> Option<usize> {
    s.find(pattern).map(|i| i + pattern.len())
}

/// If `s` (after skipping leading spaces) starts with `pattern`, return the
/// tail past it. Else `None`.
pub fn check_next_token<'a>(s: &'a str, pattern: &str) -> Option<&'a str> {
    skip_spaces(s).strip_prefix(pattern)
}

/// Parse a SQL identifier from the front of `s`. Returns `(remainder, id)` if
/// a non-empty identifier was found, else `None` (empty input or only
/// whitespace).
///
/// Accepts both unquoted (terminated by whitespace / `(`, `)`, `.`, `,`) and
/// backtick / double-quote quoted forms. Doubled quotes inside a quoted ident
/// collapse to a single quote in the output.
///
/// Port of `rdb_parse_id` (`rdb_utils.cc:145`).
pub fn parse_id(s: &str) -> Option<(&str, String)> {
    let s = skip_spaces(s);
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let (quote, mut cursor) = match bytes[0] {
        b'`' => (Some(b'`'), 1),
        b'"' => (Some(b'"'), 1),
        _ => (None, 0),
    };

    let start = cursor;
    let mut id = String::new();
    let mut closed = quote.is_none();

    while cursor < bytes.len() {
        let c = bytes[cursor];
        match quote {
            Some(q) if c == q => {
                cursor += 1;
                if cursor < bytes.len() && bytes[cursor] == q {
                    // Escaped quote — emit one quote, keep walking.
                    id.push(q as char);
                    cursor += 1;
                } else {
                    closed = true;
                    break;
                }
            }
            None if c == b' '
                || c == b'\t'
                || c == b'\n'
                || c == b'\r'
                || c == b'('
                || c == b')'
                || c == b'.'
                || c == b',' =>
            {
                break;
            }
            _ => {
                id.push(c as char);
                cursor += 1;
            }
        }
    }

    if quote.is_some() && !closed {
        // Unterminated quoted ident — mirror the C++ behaviour which returns
        // the empty tail and leaves the partial id intact.
        return Some((&s[bytes.len()..], id));
    }
    if id.is_empty() && start == cursor {
        return None;
    }
    Some((&s[cursor..], id))
}

/// Skip a SQL identifier (parse + discard the returned id).
pub fn skip_id(s: &str) -> Option<&str> {
    parse_id(s).map(|(rest, _)| rest)
}

/// Split `s` on `delim`, trimming whitespace from each token. Empty tokens
/// are dropped.
pub fn parse_into_tokens(s: &str, delim: char) -> Vec<String> {
    s.split(delim)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

// --- hexdump ---

/// Hex-dump `data`, capped at `maxsize` (0 = unlimited; otherwise capped to
/// `min(data.len(), maxsize)`). Appends `...` when truncated.
pub fn hexdump(data: &[u8], maxsize: usize) -> String {
    let n = if maxsize == 0 {
        data.len()
    } else {
        data.len().min(maxsize)
    };
    let mut out = String::with_capacity(n * 2);
    for b in &data[..n] {
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{b:02x}"));
    }
    if n < data.len() {
        out.push_str("...");
    }
    out
}

// --- RAII cleanup guard (replaces C++ `Ensure_cleanup`) ---

/// RAII guard that runs `cleanup` on drop unless `skip()` was called.
pub struct EnsureCleanup<F: FnOnce()> {
    cleanup: Option<F>,
}

impl<F: FnOnce()> EnsureCleanup<F> {
    pub fn new(cleanup: F) -> Self {
        Self {
            cleanup: Some(cleanup),
        }
    }

    /// Disarm the cleanup. Call after the protected operation succeeded.
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

// --- compression-codec advertisement ---

/// Names of compression codecs available in this build. Drives the
/// `rocksdb_supported_compression_types` `SHOW STATUS` variable. Per
/// `_DESIGN.md §11 Q7`, zstd is the default and currently only choice.
pub fn get_supported_compression_types() -> Vec<&'static str> {
    vec!["zstd"]
}

/// Rust manages its own allocator; no-op replacement for
/// `purge_all_jemalloc_arenas()`.
pub fn purge_allocator_arenas() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_spaces_strips_leading() {
        assert_eq!(skip_spaces("   abc"), "abc");
        assert_eq!(skip_spaces("abc"), "abc");
        assert_eq!(skip_spaces(""), "");
    }

    #[test]
    fn case_insensitive_compare() {
        assert!(compare_strings_ic("ABC", "abc"));
        assert!(!compare_strings_ic("abc", "abcd"));
    }

    #[test]
    fn parse_id_unquoted() {
        let (rest, id) = parse_id("col1, col2").expect("parse");
        assert_eq!(id, "col1");
        assert_eq!(rest, ", col2");
    }

    #[test]
    fn parse_id_backquoted_simple() {
        let (rest, id) = parse_id("`my id`(x)").expect("parse");
        assert_eq!(id, "my id");
        assert_eq!(rest, "(x)");
    }

    #[test]
    fn parse_id_doublequote_with_escaped_quote() {
        let (rest, id) = parse_id("\"he said \"\"hi\"\" here\".rest").expect("parse");
        assert_eq!(id, "he said \"hi\" here");
        assert_eq!(rest, ".rest");
    }

    #[test]
    fn parse_id_empty_input_is_none() {
        assert!(parse_id("   ").is_none());
        assert!(parse_id("").is_none());
    }

    #[test]
    fn skip_id_returns_tail() {
        assert_eq!(skip_id("foo bar"), Some(" bar"));
    }

    #[test]
    fn find_in_string_returns_past_match() {
        assert_eq!(find_in_string("hello world", "lo "), Some(6));
        assert_eq!(find_in_string("hello world", "xyz"), None);
    }

    #[test]
    fn check_next_token_consumes_match() {
        assert_eq!(check_next_token("  CREATE TABLE", "CREATE"), Some(" TABLE"));
        assert!(check_next_token("DROP TABLE", "CREATE").is_none());
    }

    #[test]
    fn parse_into_tokens_drops_empties() {
        assert_eq!(
            parse_into_tokens("a, ,b,c,", ','),
            vec!["a", "b", "c"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn hexdump_truncates_with_ellipsis() {
        let data = [0xab_u8, 0xcd, 0xef, 0x01, 0x02];
        assert_eq!(hexdump(&data, 3), "abcdef...");
        assert_eq!(hexdump(&data, 0), "abcdef0102");
    }

    #[test]
    fn ensure_cleanup_runs_on_drop() {
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            let f = std::sync::Arc::clone(&flag);
            let _guard = EnsureCleanup::new(move || {
                f.store(true, std::sync::atomic::Ordering::SeqCst);
            });
        }
        assert!(flag.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn ensure_cleanup_skip_disarms() {
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            let f = std::sync::Arc::clone(&flag);
            let guard = EnsureCleanup::new(move || {
                f.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            guard.skip();
        }
        assert!(!flag.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn supported_compression_lists_zstd() {
        assert!(get_supported_compression_types().contains(&"zstd"));
    }
}
