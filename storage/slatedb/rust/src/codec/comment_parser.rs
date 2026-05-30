//! Table-comment qualifier parser.
//!
//! Translated from `Rdb_key_def::parse_comment_for_qualifier`
//! (`rdb_datadic.cc:753..836`). MyRocks lets users carry per-table /
//! per-index knobs in the `CREATE TABLE ... COMMENT='...'` field using a
//! `;`-separated `key=value` grammar:
//!
//! ```text
//! CREATE TABLE t (...) COMMENT='ttl_duration=3600;cfname=audit_log'
//! ```
//!
//! Partition-specific overrides use a `partition_name_qualifier=value`
//! prefix and take precedence over the table-level value:
//!
//! ```text
//! COMMENT='ttl_duration=3600;p0_ttl_duration=60;p1_ttl_duration=120'
//! ```
//!
//! In our port the C++ `TABLE*`/`Rdb_tbl_def*` args are replaced by an
//! optional `partition_name` — pushing the partition-name lookup out to
//! the caller keeps the parser purely about string scanning.
//!
//! Today's known callers (deferred to later batches):
//! - `KeyDef::extract_ttl_duration` (qualifier `"ttl_duration"`).
//! - `KeyDef::extract_ttl_col` (qualifier `"ttl_col"`).
//! - CF-name lookup at index-create time (qualifier `"cfname"`).

use crate::globals::{QUALIFIER_SEP, QUALIFIER_VALUE_SEP};
use crate::utils::parse::parse_into_tokens;

/// Result of a successful qualifier match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifierMatch {
    pub value: String,
    /// `true` if the value came from a partition-specific override; `false`
    /// if it came from the plain table-level entry.
    pub per_part_match: bool,
}

/// Search `comment` for `qualifier`'s value. When `partition_name` is
/// `Some(p)`, look for a `{p}_{qualifier}=value` token first; if found,
/// it takes precedence over a plain `{qualifier}=value` entry.
///
/// Returns:
/// - `Some({value, per_part_match: true})` — a partition-specific entry
///   parsed cleanly.
/// - `Some({value, per_part_match: false})` — only the table-level entry
///   matched.
/// - `None` — no match, or a matched entry was malformed (lacked exactly
///   one `=` after the qualifier). Mirrors the C++ "empty result" for
///   malformed entries — caller cannot distinguish "missing" from "bad",
///   matching MyRocks behaviour.
///
/// **Faithful behaviour:** a malformed partition-prefixed match
/// *blocks* the fallback to the table-level entry (the C++ returns
/// `empty_result` immediately on the malformed-partition path). Tests
/// pin this.
pub fn parse_qualifier(
    comment: &str,
    qualifier: &str,
    partition_name: Option<&str>,
) -> Option<QualifierMatch> {
    if comment.is_empty() {
        return None;
    }

    let table_prefix = format!("{qualifier}{QUALIFIER_VALUE_SEP}");
    let part_prefix = partition_name.map(|name| {
        // Mirrors `gen_qualifier_for_table(qualifier, partition_name)`:
        // `{partition_name}_{qualifier}=`
        format!("{name}_{qualifier}{QUALIFIER_VALUE_SEP}")
    });

    let tokens = parse_into_tokens(comment, QUALIFIER_SEP);

    // First pass: partition-prefixed match (precedence).
    if let Some(prefix) = part_prefix.as_deref() {
        for tok in &tokens {
            if tok.starts_with(prefix) {
                return finalise_match(tok, /* per_part_match: */ true);
            }
        }
    }

    // Second pass: table-level match.
    for tok in &tokens {
        if tok.starts_with(&table_prefix) {
            return finalise_match(tok, /* per_part_match: */ false);
        }
    }

    None
}

/// Split the matched `token` on `=`, return Some on exactly-2-parts, None
/// otherwise. Mirrors `if (tokens.size() == 2) ... else return empty_result`.
fn finalise_match(token: &str, per_part_match: bool) -> Option<QualifierMatch> {
    let pieces = parse_into_tokens(token, QUALIFIER_VALUE_SEP);
    if pieces.len() == 2 {
        Some(QualifierMatch {
            value: pieces.into_iter().nth(1).unwrap_or_default(),
            per_part_match,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(s: &str, per_part: bool) -> Option<QualifierMatch> {
        Some(QualifierMatch {
            value: s.to_string(),
            per_part_match: per_part,
        })
    }

    #[test]
    fn empty_comment_yields_none() {
        assert_eq!(parse_qualifier("", "ttl_duration", None), None);
        assert_eq!(parse_qualifier("", "ttl_duration", Some("p0")), None);
    }

    #[test]
    fn missing_qualifier_yields_none() {
        assert_eq!(
            parse_qualifier("cfname=audit", "ttl_duration", None),
            None
        );
    }

    #[test]
    fn plain_table_level_match() {
        assert_eq!(
            parse_qualifier("ttl_duration=3600", "ttl_duration", None),
            val("3600", false)
        );
    }

    #[test]
    fn semicolon_separated_grammar() {
        // Surrounding entries should not interfere.
        let comment = "cfname=audit;ttl_duration=42;ttl_col=created_at";
        assert_eq!(
            parse_qualifier(comment, "ttl_duration", None),
            val("42", false)
        );
        assert_eq!(
            parse_qualifier(comment, "cfname", None),
            val("audit", false)
        );
        assert_eq!(
            parse_qualifier(comment, "ttl_col", None),
            val("created_at", false)
        );
    }

    #[test]
    fn whitespace_around_tokens_is_tolerated() {
        // parse_into_tokens trims; the parser inherits that.
        let comment = " ttl_duration=99 ; cfname=x ";
        assert_eq!(
            parse_qualifier(comment, "ttl_duration", None),
            val("99", false)
        );
    }

    #[test]
    fn malformed_table_match_yields_none() {
        // No '=' after the qualifier.
        assert_eq!(
            parse_qualifier("ttl_duration", "ttl_duration", None),
            None
        );
        // Multiple '=' signs split into 3 tokens → empty result.
        assert_eq!(
            parse_qualifier("ttl_duration=3600=extra", "ttl_duration", None),
            None
        );
    }

    #[test]
    fn partition_override_takes_precedence() {
        let comment = "ttl_duration=3600;p0_ttl_duration=60;p1_ttl_duration=120";
        assert_eq!(
            parse_qualifier(comment, "ttl_duration", Some("p0")),
            val("60", true)
        );
        assert_eq!(
            parse_qualifier(comment, "ttl_duration", Some("p1")),
            val("120", true)
        );
        // Different partition name with no override → falls back to table-level.
        assert_eq!(
            parse_qualifier(comment, "ttl_duration", Some("p9")),
            val("3600", false)
        );
    }

    #[test]
    fn partition_only_no_table_level() {
        // Only a partition-prefixed entry, no plain one.
        let comment = "p0_ttl_duration=60";
        assert_eq!(
            parse_qualifier(comment, "ttl_duration", Some("p0")),
            val("60", true)
        );
        // Different partition asked — no fallback because no table-level.
        assert_eq!(
            parse_qualifier(comment, "ttl_duration", Some("p1")),
            None
        );
    }

    #[test]
    fn malformed_partition_match_blocks_table_level_fallback() {
        // Load-bearing C++ behaviour: when the partition-prefixed token
        // matches the `{p}_{qualifier}=` prefix but the value-split is
        // malformed, the C++ returns empty immediately without trying the
        // table-level entry.
        //
        // Construction note: the partition match has to actually match the
        // `=` prefix, then fail the "exactly 2 parts" split. We use a
        // double-`=` value so the prefix match succeeds and the split
        // produces 3 parts.
        let comment = "ttl_duration=3600;p0_ttl_duration=60=extra";
        assert_eq!(
            parse_qualifier(comment, "ttl_duration", Some("p0")),
            None,
            "malformed partition match should suppress table-level fallback"
        );
        // Sanity: with no partition arg the partition-prefixed entry is
        // ignored entirely, so the table-level value comes through.
        assert_eq!(
            parse_qualifier(comment, "ttl_duration", None),
            val("3600", false)
        );
    }

    #[test]
    fn partition_prefix_must_match_the_equals_for_block_to_apply() {
        // A partition-prefixed entry WITHOUT the `=` (e.g. just
        // `"p0_ttl_duration"`) does not match the `{p}_{qualifier}=`
        // search prefix at all, so it neither matches nor blocks. The
        // parser falls through to the table-level entry normally.
        let comment = "ttl_duration=3600;p0_ttl_duration";
        assert_eq!(
            parse_qualifier(comment, "ttl_duration", Some("p0")),
            val("3600", false)
        );
    }

    #[test]
    fn no_partition_provided_ignores_partition_prefixed_entries() {
        let comment = "p0_ttl_duration=60";
        assert_eq!(
            parse_qualifier(comment, "ttl_duration", None),
            None
        );
    }

    #[test]
    fn qualifier_prefix_match_is_anchored_not_substring() {
        // `ttl_duration_extra=…` must NOT be picked up for qualifier=`ttl_duration`,
        // because the search prefix is `ttl_duration=` — that's the whole
        // point of the trailing '='.
        let comment = "ttl_duration_extra=99";
        assert_eq!(
            parse_qualifier(comment, "ttl_duration", None),
            None
        );
    }

    #[test]
    fn empty_value_round_trips_as_none() {
        // `ttl_col=` has prefix matching but splits to just ["ttl_col"]
        // (parse_into_tokens drops empties), so it's malformed → None.
        // Matches the C++ "if (tokens.size() == 2)" guard.
        assert_eq!(
            parse_qualifier("ttl_col=", "ttl_col", None),
            None
        );
    }
}
