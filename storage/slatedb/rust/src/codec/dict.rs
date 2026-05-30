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

// ---------------------------------------------------------------------------
// Substrate consumers (small managers).
// ---------------------------------------------------------------------------

/// Per-table auto-increment counter.
///
/// One row per table at `DataDictType::AutoInc`, suffix = table name bytes,
/// value = `u64` big-endian. Schema version is
/// [`AUTO_INCREMENT_VERSION`] (callers can ignore it today — there's only
/// one version).
pub mod autoinc {
    use super::{delete, get, put, DataDictType};
    use slatedb::{Db, Error};

    fn encode_value(value: u64) -> [u8; 8] {
        value.to_be_bytes()
    }

    fn decode_value(bytes: &[u8]) -> Result<u64, Error> {
        if bytes.len() != 8 {
            return Err(Error::data(format!(
                "autoinc value: expected 8 bytes, got {}",
                bytes.len()
            )));
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(bytes);
        Ok(u64::from_be_bytes(buf))
    }

    pub async fn read(db: &Db, table_name: &str) -> Result<Option<u64>, Error> {
        match get(db, DataDictType::AutoInc, table_name.as_bytes()).await? {
            Some(bytes) => Ok(Some(decode_value(&bytes)?)),
            None => Ok(None),
        }
    }

    pub async fn write(db: &Db, table_name: &str, value: u64) -> Result<(), Error> {
        put(
            db,
            DataDictType::AutoInc,
            table_name.as_bytes(),
            &encode_value(value),
        )
        .await
    }

    pub async fn remove(db: &Db, table_name: &str) -> Result<(), Error> {
        delete(db, DataDictType::AutoInc, table_name.as_bytes()).await
    }
}

/// Dropped-index registry.
///
/// One marker row per pending-drop index at
/// `DataDictType::DdlDropIndexOngoing`. Suffix encodes the [`GlIndexId`]
/// as `u32_be(cf_id) || u32_be(index_id)` (8 bytes). Value is empty —
/// presence is the marker. The compaction filter walks this registry to
/// decide which `(cf_id, index_id)` prefixes to sweep.
pub mod dropped_indexes {
    use super::{delete, put, scan, system_record_prefix, DataDictType};
    use crate::globals::GlIndexId;
    use slatedb::{Db, Error};

    const SUFFIX_LEN: usize = 8;

    fn encode_suffix(gl: GlIndexId) -> [u8; SUFFIX_LEN] {
        let mut out = [0u8; SUFFIX_LEN];
        out[..4].copy_from_slice(&gl.cf_id.to_be_bytes());
        out[4..].copy_from_slice(&gl.index_id.to_be_bytes());
        out
    }

    fn decode_suffix(bytes: &[u8]) -> Result<GlIndexId, Error> {
        if bytes.len() != SUFFIX_LEN {
            return Err(Error::data(format!(
                "dropped-index suffix: expected {SUFFIX_LEN} bytes, got {}",
                bytes.len()
            )));
        }
        let mut cf = [0u8; 4];
        let mut ix = [0u8; 4];
        cf.copy_from_slice(&bytes[..4]);
        ix.copy_from_slice(&bytes[4..]);
        Ok(GlIndexId {
            cf_id: u32::from_be_bytes(cf),
            index_id: u32::from_be_bytes(ix),
        })
    }

    /// Add a single index to the dropped-index registry. Idempotent — adding
    /// an already-marked index is a no-op write of the same empty value.
    pub async fn add(db: &Db, gl: GlIndexId) -> Result<(), Error> {
        put(db, DataDictType::DdlDropIndexOngoing, &encode_suffix(gl), &[]).await
    }

    /// Remove a single index. Idempotent — removing a missing index is a
    /// SlateDB-level tombstone write that's a no-op on the visible state.
    pub async fn remove(db: &Db, gl: GlIndexId) -> Result<(), Error> {
        delete(db, DataDictType::DdlDropIndexOngoing, &encode_suffix(gl)).await
    }

    /// Snapshot the dropped-index registry. Returns rows in
    /// byte-lexicographic order of `(cf_id, index_id)`.
    pub async fn list(db: &Db) -> Result<Vec<GlIndexId>, Error> {
        let prefix = system_record_prefix(DataDictType::DdlDropIndexOngoing);
        let mut it = scan(db, DataDictType::DdlDropIndexOngoing).await?;
        let mut out = Vec::new();
        while let Some(kv) = it.next().await? {
            let suffix = &kv.key[prefix.len()..];
            out.push(decode_suffix(suffix)?);
        }
        Ok(out)
    }
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

    // ----- autoinc -----

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn autoinc_round_trip() {
        let engine = EngineDb::open_in_memory("autoinc_rt").await.expect("open");

        assert_eq!(
            autoinc::read(engine.db(), "users").await.expect("read miss"),
            None
        );

        autoinc::write(engine.db(), "users", 42)
            .await
            .expect("write");
        assert_eq!(
            autoinc::read(engine.db(), "users").await.expect("read"),
            Some(42)
        );

        // Overwrite with a higher value.
        autoinc::write(engine.db(), "users", 100_000)
            .await
            .expect("write");
        assert_eq!(
            autoinc::read(engine.db(), "users").await.expect("read"),
            Some(100_000)
        );

        autoinc::remove(engine.db(), "users")
            .await
            .expect("remove");
        assert_eq!(
            autoinc::read(engine.db(), "users").await.expect("read"),
            None
        );

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn autoinc_corrupt_value_surfaces_data_error() {
        let engine = EngineDb::open_in_memory("autoinc_corrupt")
            .await
            .expect("open");

        // Write a 3-byte payload through the raw substrate, bypassing autoinc.
        put(engine.db(), DataDictType::AutoInc, b"weird", b"abc")
            .await
            .expect("put");

        let err = autoinc::read(engine.db(), "weird").await.unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Data));

        engine.close().await.expect("close");
    }

    // ----- dropped_indexes -----

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropped_indexes_empty_then_add_then_remove() {
        let engine = EngineDb::open_in_memory("dropped_idx_basic")
            .await
            .expect("open");

        assert!(dropped_indexes::list(engine.db())
            .await
            .expect("list")
            .is_empty());

        let a = crate::globals::GlIndexId {
            cf_id: 1,
            index_id: 100,
        };
        let b = crate::globals::GlIndexId {
            cf_id: 1,
            index_id: 101,
        };
        let c = crate::globals::GlIndexId {
            cf_id: 2,
            index_id: 1,
        };

        // Insert out of byte order; expect list to come back sorted.
        dropped_indexes::add(engine.db(), c).await.expect("add c");
        dropped_indexes::add(engine.db(), a).await.expect("add a");
        dropped_indexes::add(engine.db(), b).await.expect("add b");

        let listed = dropped_indexes::list(engine.db()).await.expect("list");
        assert_eq!(listed, vec![a, b, c]);

        dropped_indexes::remove(engine.db(), b)
            .await
            .expect("remove b");
        let listed = dropped_indexes::list(engine.db()).await.expect("list");
        assert_eq!(listed, vec![a, c]);

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropped_indexes_add_is_idempotent() {
        let engine = EngineDb::open_in_memory("dropped_idx_idem")
            .await
            .expect("open");

        let gl = crate::globals::GlIndexId {
            cf_id: 7,
            index_id: 7,
        };
        dropped_indexes::add(engine.db(), gl).await.expect("add 1");
        dropped_indexes::add(engine.db(), gl).await.expect("add 2");

        let listed = dropped_indexes::list(engine.db()).await.expect("list");
        assert_eq!(listed, vec![gl]);

        engine.close().await.expect("close");
    }
}
