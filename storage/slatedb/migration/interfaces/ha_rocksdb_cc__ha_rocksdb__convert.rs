//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__convert`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (body LoC 16, span 6515..6555)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__convert`
//!
//! ## Mapping
//! Two overloaded `convert_record_from_storage_format` methods on
//! `ha_rocksdb`. Both delegate to `m_converter->decode(...)` — that is, they
//! unpack the on-disk value (the MyRocks TLV row, see _DESIGN.md §3) plus the
//! memcomparable key (see _DESIGN.md §2) into the SQL-layer `table->record[0]`
//! buffer.
//!
//! Neither method calls into SlateDB. They are pure codec calls. Per
//! _DESIGN.md §3 the value encoding is preserved bit-for-bit from MyRocks;
//! TTL is the only delta and it's pulled from `KeyValue::expire_ts` *before*
//! these methods see the row (by the iterator wrapper in the `scan` / `index`
//! buckets), so the TLV row body is the same shape.
//!
//! The two C++ overloads differ only in whether the value-slice is taken from
//! `self.m_retrieved_record` (the engine-internal cache from the last point
//! lookup) or passed explicitly by the caller. We model that with a single
//! Rust fn that takes `Option<&Bytes>` for the value: `None` means "use the
//! cached `self.retrieved_record`".
//!
//! ## Out-of-scope methods
//! None. Pure codec dispatch; no §1 non-goal touched.

use bytes::Bytes;
use slatedb::Error;

use crate::ha_rocksdb_h__ha_rocksdb::HaSlateDb;

/// A MariaDB-format row buffer (`table->record[0]` style). Same POD type used
/// by the `dml` bucket. Re-declared here for compile isolation; the canonical
/// definition will live in the handler-hub stub written by A2.
#[derive(Debug, Clone)]
pub struct RowBuf {
    pub bytes: bytes::BytesMut,
}

impl HaSlateDb {
    /// Unpack a key+value pair from MyRocks storage format into a SQL-layer
    /// row buffer.
    ///
    /// Inputs:
    /// - `key`: the memcomparable key bytes (per _DESIGN.md §2). Used only to
    ///   verify the row checksum embedded in the value; the row decode itself
    ///   needs no key bytes for non-covering indexes.
    /// - `value`: `Some(v)` — explicit value slice (matches the 3-arg C++
    ///   overload at ha_rocksdb.cc:6551). `None` — use the cached
    ///   `self.retrieved_record` slice from the last point lookup (matches the
    ///   2-arg overload at ha_rocksdb.cc:6515).
    /// - `out`: the destination row buffer (caller-allocated, big enough for
    ///   `table->s->reclength`).
    ///
    /// Outputs: `Ok(())` on a successful decode. `out.bytes` may keep pointers
    /// into `value` for blob columns (see the C++ comment at line 6536).
    ///
    /// Errors:
    /// - `ErrorKind::Data` — value checksum mismatch, truncated row, unknown
    ///   field-id, or any other corruption surfaced by `Rdb_converter::decode`.
    ///   Maps to upstream `HA_ERR_ROCKSDB_CORRUPT_DATA`.
    /// - `ErrorKind::Invalid` — `value=None` was passed but
    ///   `self.retrieved_record` is empty (caller protocol violation).
    ///
    /// Invariants:
    /// - Pure CPU work — no I/O, no SlateDB calls, no async.
    /// - The blob-aliasing rule (output may borrow from input) means the
    ///   caller MUST keep the `value` slice alive until `out` is consumed.
    ///
    /// Original C++:
    /// - ha_rocksdb.cc:6515 — 2-arg overload (`key`, `buf`); calls 3-arg form
    ///   with `&self.m_retrieved_record` as the value.
    /// - ha_rocksdb.cc:6551 — 3-arg overload (`key`, `value`, `buf`); just
    ///   forwards to `self.m_converter.decode(...)`.
    pub fn convert_record_from_storage_format(
        &self,
        key: &Bytes,
        value: Option<&Bytes>,
        out: &mut RowBuf,
    ) -> Result<(), Error> {
        let _ = (key, value, out);
        todo!("dispatch: value.unwrap_or(&self.retrieved_record) into m_converter.decode(m_pk_descr, out, key, value_slice)")
    }
}
