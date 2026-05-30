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
//! | `cf_id`        | `record_type`             | Behaviour                         |
//! |----------------|---------------------------|-----------------------------------|
//! | `u32::MAX`     | `DataDictType::AutoInc`   | MAX-merge versioned `u64`         |
//! | `u32::MAX`     | any other                 | reject                            |
//! | anything else  | —                         | reject                            |
//!
//! AutoInc MAX-merge mirrors MyRocks' `Rdb_system_merge_op`: concurrent
//! writers and crash recovery converge to the largest observed value. The
//! operand format is `u16_be(version) || u64_be(value)` — same as
//! [`crate::codec::dict::autoinc::encode_value`], so a write via
//! `Db::merge` reaches reads as a value of identical shape.
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
    /// MAX-merge on the versioned `u64` autoinc format.
    MaxAutoInc,
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
            MergeRoute::MaxAutoInc
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
            MergeRoute::MaxAutoInc => max_autoinc(existing_value, value),
            MergeRoute::Reject(msg) => Err(MergeOperatorError::Callback { message: msg }),
        }
    }
}

fn max_autoinc(
    existing: Option<Bytes>,
    operand: Bytes,
) -> Result<Bytes, MergeOperatorError> {
    let new_val = decode_autoinc(&operand)?;
    match existing {
        Some(e) => {
            let old_val = decode_autoinc(&e)?;
            Ok(if new_val > old_val { operand } else { e })
        }
        None => Ok(operand),
    }
}

/// Decode the versioned autoinc payload (`u16_be(version) || u64_be(value)`,
/// 10 bytes). Format must match
/// [`crate::codec::dict::autoinc::encode_value`].
fn decode_autoinc(bytes: &[u8]) -> Result<u64, MergeOperatorError> {
    crate::codec::dict::autoinc::decode_value(bytes).map_err(|e| MergeOperatorError::Callback {
        message: format!("merge: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::dict::{self, autoinc};
    use crate::engine::db::EngineDb;

    fn gl(cf_id: u32, index_id: u32) -> crate::globals::GlIndexId {
        crate::globals::GlIndexId { cf_id, index_id }
    }

    /// Build an autoinc key directly from a GlIndexId (mirrors what
    /// `codec::dict::autoinc::write` computes internally).
    fn autoinc_key_for(g: crate::globals::GlIndexId) -> Bytes {
        let mut suffix = [0u8; 8];
        suffix[..4].copy_from_slice(&g.cf_id.to_be_bytes());
        suffix[4..].copy_from_slice(&g.index_id.to_be_bytes());
        dict::system_key(DataDictType::AutoInc, &suffix)
    }

    /// Versioned autoinc operand (matches `dict::autoinc::encode_value`).
    fn autoinc_value(v: u64) -> Bytes {
        Bytes::copy_from_slice(&autoinc::encode_value(v))
    }

    // ----- Routing-only unit tests (no SlateDB needed) -----

    #[test]
    fn autoinc_first_operand_becomes_value() {
        let op = EngineMergeOperator;
        let got = op
            .merge(&autoinc_key_for(gl(1, 100)), None, autoinc_value(42))
            .expect("merge");
        assert_eq!(autoinc::decode_value(&got).expect("decode"), 42);
    }

    #[test]
    fn autoinc_keeps_larger_existing() {
        let op = EngineMergeOperator;
        let got = op
            .merge(
                &autoinc_key_for(gl(1, 100)),
                Some(autoinc_value(100)),
                autoinc_value(7),
            )
            .expect("merge");
        assert_eq!(autoinc::decode_value(&got).expect("decode"), 100);
    }

    #[test]
    fn autoinc_takes_larger_operand() {
        let op = EngineMergeOperator;
        let got = op
            .merge(
                &autoinc_key_for(gl(1, 100)),
                Some(autoinc_value(7)),
                autoinc_value(100),
            )
            .expect("merge");
        assert_eq!(autoinc::decode_value(&got).expect("decode"), 100);
    }

    #[test]
    fn autoinc_rejects_wrong_size_operand() {
        let op = EngineMergeOperator;
        let err = op
            .merge(
                &autoinc_key_for(gl(1, 100)),
                None,
                Bytes::from_static(b"abc"),
            )
            .unwrap_err();
        assert!(matches!(err, MergeOperatorError::Callback { .. }));
    }

    #[test]
    fn unsupported_system_record_type_is_rejected() {
        let op = EngineMergeOperator;
        let key = dict::system_key(DataDictType::TableVersion, b"t");
        let err = op
            .merge(&key, None, autoinc_value(1))
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
        let err = op.merge(&key, None, autoinc_value(1)).unwrap_err();
        let MergeOperatorError::Callback { message } = err else {
            panic!("expected Callback variant");
        };
        assert!(message.contains("user CF"), "msg was: {message}");
    }

    #[test]
    fn unparseable_key_is_rejected() {
        let op = EngineMergeOperator;
        let err = op.merge(&Bytes::new(), None, autoinc_value(1)).unwrap_err();
        assert!(matches!(err, MergeOperatorError::Callback { .. }));
    }

    // ----- End-to-end: merge operands resolve through Db::get -----

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn db_merge_then_get_returns_max() {
        let engine = EngineDb::open_in_memory("merge_e2e_max")
            .await
            .expect("open");
        let pk = gl(7, 42);

        // Three merge operands via the typed autoinc bump path.
        autoinc::bump(engine.db(), pk, 5).await.expect("bump 5");
        autoinc::bump(engine.db(), pk, 3).await.expect("bump 3");
        autoinc::bump(engine.db(), pk, 10).await.expect("bump 10");

        let got = autoinc::read(engine.db(), pk).await.expect("read");
        assert_eq!(got, Some(10));

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn db_merge_after_put_takes_max_of_both() {
        let engine = EngineDb::open_in_memory("merge_e2e_with_base")
            .await
            .expect("open");
        let pk = gl(7, 42);

        // Seed via the put-based autoinc writer.
        autoinc::write(engine.db(), pk, 50).await.expect("seed");

        // Bump with a smaller value — should be ignored.
        autoinc::bump(engine.db(), pk, 7).await.expect("bump small");
        assert_eq!(autoinc::read(engine.db(), pk).await.expect("read"), Some(50));

        // Bump with a larger value — should win.
        autoinc::bump(engine.db(), pk, 99).await.expect("bump large");
        assert_eq!(autoinc::read(engine.db(), pk).await.expect("read"), Some(99));

        engine.close().await.expect("close");
    }
}
