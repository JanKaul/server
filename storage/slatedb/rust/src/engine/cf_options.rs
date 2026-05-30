//! Per-CF options parsing.
//!
//! Translated from `storage/rocksdb/rdb_cf_options.{h,cc}`. Per
//! `_DESIGN.md §1`, SlateDB has one global block cache / filter policy /
//! compression codec; per-CF tuning knobs from MyRocks are accepted by the
//! parser for backwards compatibility but recorded as `silently_ignored` and
//! discarded. Only `comparator` (forward/reverse) and `ttl_duration` are
//! actually consumed.
//!
//! The override-map parser preserves the MyRocks grammar:
//! `cf_name1={key=val;…};cf_name2={key=val;…}` with brace-balanced inner
//! sections so `{…{nested}…}` values pass through.

use slatedb::Error;
use std::collections::HashMap;

use crate::engine::comparator::KeyDirection;

/// One CF's configured options post-parse, in the subset SlateDB respects.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CfOptionsSnapshot {
    pub direction: KeyDirection,
    /// TTL in seconds, if set via `ttl_duration=N`.
    pub ttl_seconds: Option<u64>,
    /// Per-CF option keys accepted but discarded.
    pub silently_ignored: Vec<String>,
}

/// Container for `default_cf_options` (applies to CFs not in the map) and
/// the parsed `override_cf_options` map.
pub struct CfOptions {
    name_map: HashMap<String, String>,
    default_config: String,
    default_snapshot: CfOptionsSnapshot,
}

impl CfOptions {
    pub fn new() -> Self {
        Self {
            name_map: HashMap::new(),
            default_config: String::new(),
            default_snapshot: CfOptionsSnapshot::default(),
        }
    }

    /// Parse both sysvar values at startup.
    pub fn init(
        &mut self,
        default_cf_options: &str,
        override_cf_options: &str,
    ) -> Result<(), Error> {
        self.set_default(default_cf_options)?;
        self.set_override(override_cf_options)?;
        Ok(())
    }

    /// Parsed options for the named CF; falls back to the default snapshot.
    pub fn get(&self, cf_name: &str) -> CfOptionsSnapshot {
        if let Some(raw) = self.name_map.get(cf_name) {
            Self::parse_cf_options(raw).unwrap_or_default()
        } else {
            self.default_snapshot.clone()
        }
    }

    /// Live-update one CF's options (`SET GLOBAL rocksdb_update_cf_options=...`).
    pub fn update(&mut self, cf_name: &str, cf_options: &str) -> Result<(), Error> {
        let _ = Self::parse_cf_options(cf_options)?;
        self.name_map
            .insert(cf_name.to_string(), cf_options.to_string());
        Ok(())
    }

    fn set_default(&mut self, s: &str) -> Result<(), Error> {
        self.default_snapshot = Self::parse_cf_options(s)?;
        self.default_config = s.to_string();
        Ok(())
    }

    fn set_override(&mut self, s: &str) -> Result<(), Error> {
        self.name_map = Self::parse_cf_options_map(s)?;
        Ok(())
    }

    /// Parse a single CF-options body (`key1=val1;key2=val2;…`).
    pub fn parse_cf_options(s: &str) -> Result<CfOptionsSnapshot, Error> {
        let mut snap = CfOptionsSnapshot::default();
        for tok in s.split(';').map(str::trim).filter(|t| !t.is_empty()) {
            let Some((k, v)) = tok.split_once('=') else {
                return Err(Error::invalid(format!(
                    "cf_options token missing '=': {tok:?}"
                )));
            };
            match k.trim() {
                "comparator" => {
                    snap.direction = if v.trim().eq_ignore_ascii_case("reverse") {
                        KeyDirection::Reverse
                    } else {
                        KeyDirection::Forward
                    };
                }
                "ttl_duration" => {
                    snap.ttl_seconds = Some(v.trim().parse().map_err(|_| {
                        Error::invalid(format!("ttl_duration not u64: {v:?}"))
                    })?);
                }
                other => snap.silently_ignored.push(other.to_string()),
            }
        }
        Ok(snap)
    }

    /// Parse the override sysvar grammar:
    /// `cf_name1={key=val;…};cf_name2={key=val;…}`. Brace-balanced inner
    /// sections so nested `{` `}` pass through.
    ///
    /// Port of the C++ brace walker (`rdb_cf_options.cc:206..296`).
    pub fn parse_cf_options_map(s: &str) -> Result<HashMap<String, String>, Error> {
        let bytes = s.as_bytes();
        let mut pos = 0;
        let mut out: HashMap<String, String> = HashMap::new();

        while pos < bytes.len() {
            skip_ascii_ws(bytes, &mut pos);
            if pos >= bytes.len() {
                break;
            }
            let cf = scan_cf_name(bytes, &mut pos)?;
            if pos >= bytes.len() || bytes[pos] != b'=' {
                return Err(Error::invalid(format!(
                    "cf_options override: '=' expected after cf name {cf:?}"
                )));
            }
            pos += 1; // skip '='
            skip_ascii_ws(bytes, &mut pos);
            let opts = scan_braced(bytes, &mut pos)?;
            if out.contains_key(&cf) {
                return Err(Error::invalid(format!(
                    "cf_options override: duplicate entry for {cf:?}"
                )));
            }
            out.insert(cf, opts);
            skip_ascii_ws(bytes, &mut pos);
            if pos < bytes.len() && bytes[pos] == b';' {
                pos += 1;
            }
        }
        Ok(out)
    }

    pub fn default_snapshot(&self) -> &CfOptionsSnapshot {
        &self.default_snapshot
    }
}

impl Default for CfOptions {
    fn default() -> Self {
        Self::new()
    }
}

// --- brace-balanced parser helpers ---

fn skip_ascii_ws(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
}

/// Read characters up to (but not including) `=`. Trims trailing spaces.
/// Spaces inside the name are preserved (matches the C++ behaviour).
fn scan_cf_name(bytes: &[u8], pos: &mut usize) -> Result<String, Error> {
    let beg = *pos;
    let mut end = beg; // inclusive upper bound when end > beg
    let mut saw_any_nonspace = false;
    while *pos < bytes.len() && bytes[*pos] != b'=' {
        if bytes[*pos] != b' ' {
            end = *pos + 1;
            saw_any_nonspace = true;
        }
        *pos += 1;
    }
    if !saw_any_nonspace {
        return Err(Error::invalid(
            "cf_options override: empty cf name".into(),
        ));
    }
    std::str::from_utf8(&bytes[beg..end])
        .map(|s| s.trim().to_owned())
        .map_err(|_| Error::invalid("cf_options override: non-utf8 cf name".into()))
}

/// Expects `bytes[*pos]` to be `{`. Walks until the matching `}` accounting
/// for nested `{` `}`. Returns the inner string (exclusive of the outer
/// braces), advances `*pos` past the closing `}`.
fn scan_braced(bytes: &[u8], pos: &mut usize) -> Result<String, Error> {
    if *pos >= bytes.len() || bytes[*pos] != b'{' {
        return Err(Error::invalid(
            "cf_options override: '{' expected".into(),
        ));
    }
    *pos += 1; // skip opening '{'
    let beg = *pos;
    let mut depth: usize = 1;
    while *pos < bytes.len() {
        match bytes[*pos] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let inner = std::str::from_utf8(&bytes[beg..*pos])
                        .map_err(|_| {
                            Error::invalid(
                                "cf_options override: non-utf8 in options body".into(),
                            )
                        })?
                        .to_owned();
                    *pos += 1; // past closing '}'
                    return Ok(inner);
                }
            }
            _ => {}
        }
        *pos += 1;
    }
    Err(Error::invalid(
        "cf_options override: unmatched '{' — '}' expected".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_cf_body_parses() {
        let snap = CfOptions::parse_cf_options("comparator=reverse; ttl_duration=3600").unwrap();
        assert_eq!(snap.direction, KeyDirection::Reverse);
        assert_eq!(snap.ttl_seconds, Some(3600));
        assert!(snap.silently_ignored.is_empty());
    }

    #[test]
    fn unknown_keys_recorded_as_silently_ignored() {
        let snap =
            CfOptions::parse_cf_options("write_buffer_size=4096; comparator=forward").unwrap();
        assert_eq!(snap.direction, KeyDirection::Forward);
        assert_eq!(snap.silently_ignored, vec!["write_buffer_size"]);
    }

    #[test]
    fn token_missing_equals_is_invalid() {
        assert!(CfOptions::parse_cf_options("comparator").is_err());
    }

    #[test]
    fn ttl_duration_must_be_u64() {
        assert!(CfOptions::parse_cf_options("ttl_duration=notanumber").is_err());
    }

    #[test]
    fn override_map_simple_two_cfs() {
        let map = CfOptions::parse_cf_options_map(
            "cf1={comparator=reverse};cf2={ttl_duration=10}",
        )
        .unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("cf1").map(String::as_str), Some("comparator=reverse"));
        assert_eq!(map.get("cf2").map(String::as_str), Some("ttl_duration=10"));
    }

    #[test]
    fn override_map_handles_nested_braces() {
        // Inner braces should pass through verbatim.
        let map = CfOptions::parse_cf_options_map(
            "cf_a={opts={inner=value}; trailing=yes}",
        )
        .unwrap();
        assert_eq!(
            map.get("cf_a").map(String::as_str),
            Some("opts={inner=value}; trailing=yes")
        );
    }

    #[test]
    fn override_map_with_whitespace() {
        let map =
            CfOptions::parse_cf_options_map("  cf1 = { comparator = forward } ; cf2={}  ")
                .unwrap();
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("cf1"));
        assert!(map.contains_key("cf2"));
    }

    #[test]
    fn override_map_rejects_duplicate_cf() {
        assert!(
            CfOptions::parse_cf_options_map("a={x=1};a={y=2}")
                .is_err()
        );
    }

    #[test]
    fn override_map_rejects_unmatched_brace() {
        assert!(CfOptions::parse_cf_options_map("cf={oops").is_err());
    }

    #[test]
    fn override_map_rejects_missing_equals() {
        assert!(CfOptions::parse_cf_options_map("cf{x=1}").is_err());
    }

    #[test]
    fn get_falls_back_to_default() {
        let mut opts = CfOptions::new();
        opts.init("comparator=reverse", "explicit={ttl_duration=99}")
            .unwrap();
        // Named CF picks up its override:
        assert_eq!(opts.get("explicit").ttl_seconds, Some(99));
        // Unknown CF gets the default:
        assert_eq!(opts.get("unknown").direction, KeyDirection::Reverse);
    }

    #[test]
    fn update_validates_body_before_storing() {
        let mut opts = CfOptions::new();
        assert!(opts.update("cf", "ttl_duration=bad").is_err());
        assert!(opts.update("cf", "ttl_duration=42").is_ok());
        assert_eq!(opts.get("cf").ttl_seconds, Some(42));
    }
}
