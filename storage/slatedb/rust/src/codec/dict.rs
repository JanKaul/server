//! Data-dictionary substrate.
//!
//! Per `_DESIGN.md §1 + §2`: data-dictionary entries (table metadata,
//! dropped-index registry, auto-increment counters, DDL progress markers,
//! …) live in the **system area** of the keyspace, identified by the
//! reserved `cf_id = u32::MAX`. Per `_DESIGN.md §11 Q1` the per-record-type
//! key shape mirrors the user-data shape:
//!
//! ```text
//! varint(u32::MAX) || u32_be(record_type) || suffix_bytes
//! ```
//!
//! The `record_type` segment plays the role `index_id` does for user data:
//! each [`DataDictType`] variant is its own logical "index" within the
//! system area, so `scan_prefix` on `varint(u32::MAX) || u32_be(t)` returns
//! exactly the rows of type `t`.
//!
//! This module is the **substrate** — key shape + thin read/write helpers.
//! The higher-level `Rdb_dict_manager` (table metadata, autoinc, DDL
//! markers) will sit on top of these primitives once translated.
//!
//! ## Schema versions
//!
//! Each record type has its own schema version constant (see
//! `rdb_datadic.h:515..594`). Callers stamp the version into the value
//! payload; this module doesn't interpret values.

use bytes::Bytes;
use slatedb::{Db, DbIterator, Error};

use crate::codec::prefix::{build_key_prefix, varint_u32_len, INDEX_ID_LEN};
use crate::globals::SYSTEM_CF_ID;

/// Data-dictionary record-type tags. Each lives at its own
/// `(SYSTEM_CF_ID, record_type)` prefix.
///
/// Translated from `rdb_datadic.h:498`. The numeric values are on-disk
/// stable; adding a new variant means picking an unused slot. `10..=12`
/// are reserved upstream by MariaDB and intentionally skipped.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataDictType {
    DdlEntryIndexStartNumber = 1,
    IndexInfo = 2,
    CfDefinition = 3,
    BinlogInfoIndexNumber = 4,
    DdlDropIndexOngoing = 5,
    IndexStatistics = 6,
    MaxIndexId = 7,
    DdlCreateIndexOngoing = 8,
    AutoInc = 9,
    TableVersion = 20,
    EndDictIndexId = 255,
}

// --- schema-version constants (rdb_datadic.h:515) ---

pub const DDL_ENTRY_INDEX_VERSION: u16 = 1;
pub const CF_DEFINITION_VERSION: u16 = 1;
pub const BINLOG_INFO_INDEX_NUMBER_VERSION: u16 = 1;
pub const DDL_DROP_INDEX_ONGOING_VERSION: u16 = 1;
pub const MAX_INDEX_ID_VERSION: u16 = 1;
pub const DDL_CREATE_INDEX_ONGOING_VERSION: u16 = 1;
pub const AUTO_INCREMENT_VERSION: u16 = 1;

// --- key construction ---

/// Build the prefix that scopes a scan to one record type:
/// `varint(u32::MAX) || u32_be(record_type)`.
pub fn system_record_prefix(record_type: DataDictType) -> Bytes {
    build_key_prefix(SYSTEM_CF_ID, record_type as u32)
}

/// Build a full system-area key: the record-type prefix followed by
/// `suffix`. Use the empty suffix for singleton records like
/// [`DataDictType::MaxIndexId`].
pub fn system_key(record_type: DataDictType, suffix: &[u8]) -> Bytes {
    let prefix_len = varint_u32_len(SYSTEM_CF_ID) + INDEX_ID_LEN;
    let mut out = Vec::with_capacity(prefix_len + suffix.len());
    out.extend_from_slice(&system_record_prefix(record_type));
    out.extend_from_slice(suffix);
    Bytes::from(out)
}

// --- read / write helpers ---
//
// All take `&slatedb::Db` rather than `&EngineDb` so dictionary code can be
// called from inside a transaction by reaching `EngineTxn::raw().get(...)`
// when the caller wants the read or write to participate in SSI tracking.
// These free functions are the "no-txn" / autocommit flavour.

pub async fn get(
    db: &Db,
    record_type: DataDictType,
    suffix: &[u8],
) -> Result<Option<Bytes>, Error> {
    db.get(&system_key(record_type, suffix)).await
}

pub async fn put(
    db: &Db,
    record_type: DataDictType,
    suffix: &[u8],
    value: &[u8],
) -> Result<(), Error> {
    db.put(&system_key(record_type, suffix), value).await?;
    Ok(())
}

pub async fn delete(
    db: &Db,
    record_type: DataDictType,
    suffix: &[u8],
) -> Result<(), Error> {
    db.delete(&system_key(record_type, suffix)).await?;
    Ok(())
}

/// Prefix-scan all rows of the given record type. Iteration order is
/// byte-lexicographic on the suffix.
pub async fn scan(
    db: &Db,
    record_type: DataDictType,
) -> Result<DbIterator, Error> {
    db.scan_prefix(&system_record_prefix(record_type)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::prefix::parse_key_prefix;
    use crate::engine::db::EngineDb;

    #[test]
    fn system_record_prefix_uses_max_cf_and_record_type_as_index() {
        // u32::MAX needs the full 5-byte varint.
        let prefix = system_record_prefix(DataDictType::IndexInfo);
        let parsed = parse_key_prefix(&prefix).expect("parse");
        assert_eq!(parsed.cf_id, SYSTEM_CF_ID);
        assert_eq!(parsed.index_id, DataDictType::IndexInfo as u32);
        assert_eq!(parsed.prefix_len, prefix.len());
    }

    #[test]
    fn system_key_is_record_prefix_then_suffix() {
        let prefix = system_record_prefix(DataDictType::AutoInc);
        let key = system_key(DataDictType::AutoInc, b"table_name");
        assert!(key.starts_with(&prefix));
        assert_eq!(&key[prefix.len()..], b"table_name");
    }

    #[test]
    fn distinct_record_types_have_disjoint_prefixes() {
        // Two types must produce prefixes that aren't substrings of each
        // other — otherwise scans would leak between record types.
        let a = system_record_prefix(DataDictType::IndexInfo);
        let b = system_record_prefix(DataDictType::AutoInc);
        assert_ne!(a, b);
        assert!(!a.starts_with(&b[..]));
        assert!(!b.starts_with(&a[..]));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn put_get_round_trip() {
        let engine = EngineDb::open_in_memory("dict_put_get").await.expect("open");

        put(engine.db(), DataDictType::AutoInc, b"users", b"\x00\x00\x00\x00\x00\x00\x00\x2a")
            .await
            .expect("put");
        let got = get(engine.db(), DataDictType::AutoInc, b"users")
            .await
            .expect("get")
            .expect("present");
        assert_eq!(&got[..], b"\x00\x00\x00\x00\x00\x00\x00\x2a");

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn delete_removes_existing_key() {
        let engine = EngineDb::open_in_memory("dict_delete").await.expect("open");

        put(engine.db(), DataDictType::MaxIndexId, b"", b"\x00\x00\x00\x07")
            .await
            .expect("put");
        delete(engine.db(), DataDictType::MaxIndexId, b"")
            .await
            .expect("delete");
        let got = get(engine.db(), DataDictType::MaxIndexId, b"")
            .await
            .expect("get");
        assert!(got.is_none());

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn scan_returns_only_chosen_record_type() {
        let engine = EngineDb::open_in_memory("dict_scan_iso").await.expect("open");

        // Mix two record types under the same suffix.
        put(engine.db(), DataDictType::AutoInc, b"t1", b"1")
            .await
            .expect("put autoinc t1");
        put(engine.db(), DataDictType::AutoInc, b"t2", b"2")
            .await
            .expect("put autoinc t2");
        put(engine.db(), DataDictType::TableVersion, b"t1", b"v")
            .await
            .expect("put tableversion t1");

        let mut it = scan(engine.db(), DataDictType::AutoInc)
            .await
            .expect("scan");
        let mut seen: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        while let Some(kv) = it.next().await.expect("iter") {
            seen.push((kv.key.to_vec(), kv.value.to_vec()));
        }
        assert_eq!(seen.len(), 2, "should not see the TableVersion row");
        // The two AutoInc rows come back in byte order:
        let prefix = system_record_prefix(DataDictType::AutoInc);
        assert!(seen[0].0.starts_with(&prefix));
        assert!(seen[1].0.starts_with(&prefix));
        let s0 = &seen[0].0[prefix.len()..];
        let s1 = &seen[1].0[prefix.len()..];
        assert_eq!(s0, b"t1");
        assert_eq!(s1, b"t2");

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn scan_yields_suffixes_in_lexicographic_order() {
        let engine = EngineDb::open_in_memory("dict_scan_order").await.expect("open");
        for s in [b"c" as &[u8], b"a", b"b"] {
            put(engine.db(), DataDictType::IndexStatistics, s, b"")
                .await
                .expect("put");
        }
        let mut it = scan(engine.db(), DataDictType::IndexStatistics)
            .await
            .expect("scan");
        let prefix = system_record_prefix(DataDictType::IndexStatistics);
        let mut order = Vec::new();
        while let Some(kv) = it.next().await.expect("iter") {
            order.push(kv.key[prefix.len()..].to_vec());
        }
        assert_eq!(order, vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
        engine.close().await.expect("close");
    }
}
