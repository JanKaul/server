//! Global merge operator.
//!
//! Per `_DESIGN.md §0 + §1`, SlateDB takes one merge operator at
//! `DbBuilder::with_merge_operator`. MyRocks attached a custom merge
//! operator only to the system CF (`Rdb_system_merge_op`); we collapse
//! that into a single [`EngineMergeOperator`] that routes by the key's
//! `cf_id` head (extracted via [`crate::codec::prefix::parse_key_prefix`])
//! and, within the system CF, by the [`DataDictType`] record-type segment.
//!
//! ## Routing table (today)
//!
//! | `cf_id`        | `record_type`             | Behaviour          |
//! |----------------|---------------------------|--------------------|
//! | `u32::MAX`     | `DataDictType::AutoInc`   | MAX-merge `u64_be` |
//! | `u32::MAX`     | any other                 | reject             |
//! | anything else  | —                         | reject             |
//!
//! AutoInc MAX-merge mirrors MyRocks' `Rdb_system_merge_op`: concurrent
//! writers and crash recovery converge to the largest observed value. The
//! operand format is the same `u64_be` that
//! [`crate::codec::dict::autoinc`] uses at rest, so a write via
//! `Db::merge` reaches reads as a value of the same shape.
//!
//! ## Future extensions
//!
//! New use cases (counter SUM, append-only list, etc.) become new arms in
//! the routing table. The "reject" arms guarantee that an accidental
//! `Db::merge` call on a key the operator doesn't know about surfaces as
//! a loud `MergeOperatorError::Callback` rather than silent data loss.

use bytes::Bytes;
use slatedb::{MergeOperator, MergeOperatorError};

use crate::codec::dict::DataDictType;
use crate::codec::prefix::parse_key_prefix;
use crate::globals::SYSTEM_CF_ID;

/// The crate's single `slatedb::MergeOperator`. Stateless; cheap to
/// `Arc::new` and hand to the builder.
pub struct EngineMergeOperator;

enum MergeRoute {
    MaxU64Be,
    Reject(String),
}

impl EngineMergeOperator {
    fn route(key: &Bytes) -> MergeRoute {
        let Some(parsed) = parse_key_prefix(key) else {
            return MergeRoute::Reject(
                "merge: key prefix unparseable (varint cf_id + u32_be index_id required)"
                    .into(),
            );
        };
        if parsed.cf_id != SYSTEM_CF_ID {
            return MergeRoute::Reject(format!(
                "merge: user CF cf_id={} has no merge operator wired",
                parsed.cf_id
            ));
        }
        if parsed.index_id == DataDictType::AutoInc as u32 {
            MergeRoute::MaxU64Be
        } else {
            MergeRoute::Reject(format!(
                "merge: system record type {} has no merge operator wired",
                parsed.index_id
            ))
        }
    }
}

impl MergeOperator for EngineMergeOperator {
    fn merge(
        &self,
        key: &Bytes,
        existing_value: Option<Bytes>,
        value: Bytes,
    ) -> Result<Bytes, MergeOperatorError> {
        match Self::route(key) {
            MergeRoute::MaxU64Be => max_u64_be(existing_value, value),
            MergeRoute::Reject(msg) => Err(MergeOperatorError::Callback { message: msg }),
        }
    }
}

fn max_u64_be(
    existing: Option<Bytes>,
    operand: Bytes,
) -> Result<Bytes, MergeOperatorError> {
    let new_val = decode_u64_be(&operand)?;
    match existing {
        Some(e) => {
            let old_val = decode_u64_be(&e)?;
            Ok(if new_val > old_val { operand } else { e })
        }
        None => Ok(operand),
    }
}

fn decode_u64_be(bytes: &[u8]) -> Result<u64, MergeOperatorError> {
    if bytes.len() != 8 {
        return Err(MergeOperatorError::Callback {
            message: format!("merge: expected 8 bytes for u64_be operand, got {}", bytes.len()),
        });
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(bytes);
    Ok(u64::from_be_bytes(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::dict::{self, autoinc};
    use crate::engine::db::EngineDb;

    fn autoinc_key(table: &str) -> Bytes {
        dict::system_key(DataDictType::AutoInc, table.as_bytes())
    }

    fn u64_be(v: u64) -> Bytes {
        Bytes::copy_from_slice(&v.to_be_bytes())
    }

    // ----- Routing-only unit tests (no SlateDB needed) -----

    #[test]
    fn autoinc_first_operand_becomes_value() {
        let op = EngineMergeOperator;
        let got = op
            .merge(&autoinc_key("t"), None, u64_be(42))
            .expect("merge");
        assert_eq!(&got[..], &42u64.to_be_bytes());
    }

    #[test]
    fn autoinc_keeps_larger_existing() {
        let op = EngineMergeOperator;
        let got = op
            .merge(&autoinc_key("t"), Some(u64_be(100)), u64_be(7))
            .expect("merge");
        assert_eq!(&got[..], &100u64.to_be_bytes());
    }

    #[test]
    fn autoinc_takes_larger_operand() {
        let op = EngineMergeOperator;
        let got = op
            .merge(&autoinc_key("t"), Some(u64_be(7)), u64_be(100))
            .expect("merge");
        assert_eq!(&got[..], &100u64.to_be_bytes());
    }

    #[test]
    fn autoinc_rejects_wrong_size_operand() {
        let op = EngineMergeOperator;
        let err = op
            .merge(&autoinc_key("t"), None, Bytes::from_static(b"abc"))
            .unwrap_err();
        assert!(matches!(err, MergeOperatorError::Callback { .. }));
    }

    #[test]
    fn unsupported_system_record_type_is_rejected() {
        let op = EngineMergeOperator;
        let key = dict::system_key(DataDictType::TableVersion, b"t");
        let err = op
            .merge(&key, None, u64_be(1))
            .unwrap_err();
        let MergeOperatorError::Callback { message } = err else {
            panic!("expected Callback variant");
        };
        assert!(message.contains("system record type"), "msg was: {message}");
    }

    #[test]
    fn user_cf_is_rejected() {
        let op = EngineMergeOperator;
        let key = crate::codec::prefix::build_key_prefix(0, 1); // user CF
        let err = op
            .merge(&key, None, u64_be(1))
            .unwrap_err();
        let MergeOperatorError::Callback { message } = err else {
            panic!("expected Callback variant");
        };
        assert!(message.contains("user CF"), "msg was: {message}");
    }

    #[test]
    fn unparseable_key_is_rejected() {
        let op = EngineMergeOperator;
        // Empty key — varint decode fails.
        let err = op
            .merge(&Bytes::new(), None, u64_be(1))
            .unwrap_err();
        assert!(matches!(err, MergeOperatorError::Callback { .. }));
    }

    // ----- End-to-end: merge operands resolve through Db::get -----

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn db_merge_then_get_returns_max() {
        let engine = EngineDb::open_in_memory("merge_e2e_max")
            .await
            .expect("open");
        let key = autoinc_key("users");

        // Three merge operands: 5, 3, 10 → MAX-merge → 10.
        engine.db().merge(&key, &5u64.to_be_bytes()).await.expect("merge 5");
        engine.db().merge(&key, &3u64.to_be_bytes()).await.expect("merge 3");
        engine.db().merge(&key, &10u64.to_be_bytes()).await.expect("merge 10");

        // Routing via the dict::autoinc reader (which decodes u64_be) — proves the
        // resolved value has the right shape.
        let got = autoinc::read(engine.db(), "users").await.expect("read");
        assert_eq!(got, Some(10));

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn db_merge_after_put_takes_max_of_both() {
        let engine = EngineDb::open_in_memory("merge_e2e_with_base")
            .await
            .expect("open");

        // Seed via the regular put-based autoinc writer.
        autoinc::write(engine.db(), "users", 50)
            .await
            .expect("seed");

        // Merge a smaller value — should be ignored.
        engine
            .db()
            .merge(&autoinc_key("users"), &7u64.to_be_bytes())
            .await
            .expect("merge");
        assert_eq!(
            autoinc::read(engine.db(), "users").await.expect("read"),
            Some(50)
        );

        // Merge a larger value — should win.
        engine
            .db()
            .merge(&autoinc_key("users"), &99u64.to_be_bytes())
            .await
            .expect("merge");
        assert_eq!(
            autoinc::read(engine.db(), "users").await.expect("read"),
            Some(99)
        );

        engine.close().await.expect("close");
    }
}
