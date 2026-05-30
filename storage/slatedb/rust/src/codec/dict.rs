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

/// Append a merge operand at a system-area key. Routed through the global
/// `EngineMergeOperator` at read/compaction time — see
/// `engine::merge` for the routing table that maps `record_type` to its
/// merge semantic (today: only `DataDictType::AutoInc` is wired, as
/// MAX-merge on versioned `u64` values).
pub async fn merge(
    db: &Db,
    record_type: DataDictType,
    suffix: &[u8],
    operand: &[u8],
) -> Result<(), Error> {
    db.merge(&system_key(record_type, suffix), operand).await?;
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

/// Per-index auto-increment counter.
///
/// One row per index at `DataDictType::AutoInc`. Suffix is the
/// `GlIndexId` encoded as `u32_be(cf_id) || u32_be(index_id)` (8 bytes).
/// Value is `u16_be(version) || u64_be(value)` (10 bytes), where
/// `version` is [`crate::codec::key::AUTO_INCREMENT_VERSION`] (= 1 today).
///
/// Two write modes match the MyRocks contract
/// (`Rdb_dict_manager::put_auto_incr_val`):
/// - [`write`] — Put-mode (overwrite). Used to bootstrap a fresh value
///   or roll back to a known state.
/// - [`bump`] — Merge-mode (MAX). Routed through the engine's
///   [`crate::engine::merge::EngineMergeOperator`] so concurrent writers
///   and crash recovery converge to the largest observed value.
///   The merge operator decodes the versioned format and compares the
///   u64 portion.
pub mod autoinc {
    use super::{delete, encode_gl_index_suffix, get, merge, put, DataDictType};
    use crate::codec::key::AUTO_INCREMENT_VERSION;
    use crate::globals::GlIndexId;
    use slatedb::{Db, Error};

    const VERSION_BYTES: usize = 2;
    const VALUE_BYTES: usize = 8;
    /// On-disk encoded length: `u16_be(version) || u64_be(value)`.
    pub const ENCODED_LEN: usize = VERSION_BYTES + VALUE_BYTES;

    /// Encode `value` in the on-disk format. Public so the engine's
    /// merge operator can validate/produce the same bytes without
    /// going through this module's async API.
    pub fn encode_value(value: u64) -> [u8; ENCODED_LEN] {
        let mut out = [0u8; ENCODED_LEN];
        out[..VERSION_BYTES].copy_from_slice(&AUTO_INCREMENT_VERSION.to_be_bytes());
        out[VERSION_BYTES..].copy_from_slice(&value.to_be_bytes());
        out
    }

    /// Decode the versioned value, validating that the on-disk version
    /// is recognised (`<= AUTO_INCREMENT_VERSION`). Mirrors
    /// `rdb_datadic.cc:5410` which silently treats future versions as
    /// "value not present" — we surface that as `Err(Data)` so a
    /// downgrade-without-migration is loud.
    pub fn decode_value(bytes: &[u8]) -> Result<u64, Error> {
        if bytes.len() != ENCODED_LEN {
            return Err(Error::data(format!(
                "autoinc value: expected {ENCODED_LEN} bytes, got {}",
                bytes.len()
            )));
        }
        let version = u16::from_be_bytes([bytes[0], bytes[1]]);
        if version > AUTO_INCREMENT_VERSION {
            return Err(Error::data(format!(
                "autoinc value: unsupported version {version} (latest is {AUTO_INCREMENT_VERSION})"
            )));
        }
        let mut buf = [0u8; VALUE_BYTES];
        buf.copy_from_slice(&bytes[VERSION_BYTES..]);
        Ok(u64::from_be_bytes(buf))
    }

    pub async fn read(db: &Db, gl: GlIndexId) -> Result<Option<u64>, Error> {
        match get(db, DataDictType::AutoInc, &encode_gl_index_suffix(gl)).await? {
            Some(bytes) => Ok(Some(decode_value(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Put-mode write — overwrites whatever's stored.
    pub async fn write(db: &Db, gl: GlIndexId, value: u64) -> Result<(), Error> {
        put(
            db,
            DataDictType::AutoInc,
            &encode_gl_index_suffix(gl),
            &encode_value(value),
        )
        .await
    }

    /// Merge-mode write — appends a MAX-merge operand. The current
    /// stored value rises to `max(current, value)` after the operator
    /// resolves it on read/compaction.
    pub async fn bump(db: &Db, gl: GlIndexId, value: u64) -> Result<(), Error> {
        merge(
            db,
            DataDictType::AutoInc,
            &encode_gl_index_suffix(gl),
            &encode_value(value),
        )
        .await
    }

    pub async fn remove(db: &Db, gl: GlIndexId) -> Result<(), Error> {
        delete(db, DataDictType::AutoInc, &encode_gl_index_suffix(gl)).await
    }
}

// ----- shared codecs for GlIndexId-keyed registries -----

const GL_INDEX_SUFFIX_LEN: usize = 8;

fn encode_gl_index_suffix(gl: crate::globals::GlIndexId) -> [u8; GL_INDEX_SUFFIX_LEN] {
    let mut out = [0u8; GL_INDEX_SUFFIX_LEN];
    out[..4].copy_from_slice(&gl.cf_id.to_be_bytes());
    out[4..].copy_from_slice(&gl.index_id.to_be_bytes());
    out
}

fn decode_gl_index_suffix(bytes: &[u8]) -> Result<crate::globals::GlIndexId, Error> {
    if bytes.len() != GL_INDEX_SUFFIX_LEN {
        return Err(Error::data(format!(
            "gl-index dict suffix: expected {GL_INDEX_SUFFIX_LEN} bytes, got {}",
            bytes.len()
        )));
    }
    let mut cf = [0u8; 4];
    let mut ix = [0u8; 4];
    cf.copy_from_slice(&bytes[..4]);
    ix.copy_from_slice(&bytes[4..]);
    Ok(crate::globals::GlIndexId {
        cf_id: u32::from_be_bytes(cf),
        index_id: u32::from_be_bytes(ix),
    })
}

/// Generic GlIndexId marker-set walker. Used by [`dropped_indexes`] and
/// [`ddl_create_index_ongoing`] which share the same shape: one marker
/// row per (cf_id, index_id) at a chosen `DataDictType`.
async fn list_gl_index_marker_set(
    db: &Db,
    record_type: DataDictType,
) -> Result<Vec<crate::globals::GlIndexId>, Error> {
    let prefix = system_record_prefix(record_type);
    let mut it = scan(db, record_type).await?;
    let mut out = Vec::new();
    while let Some(kv) = it.next().await? {
        out.push(decode_gl_index_suffix(&kv.key[prefix.len()..])?);
    }
    Ok(out)
}

/// Dropped-index registry.
///
/// One marker row per pending-drop index at
/// `DataDictType::DdlDropIndexOngoing`. Suffix encodes the [`GlIndexId`]
/// as `u32_be(cf_id) || u32_be(index_id)` (8 bytes). Value is
/// `u16_be(DDL_DROP_INDEX_ONGOING_VERSION)` (2 bytes) per MyRocks
/// (`rdb_datadic.cc:5095..5106`) — written for forward-compat even though
/// the C++ "doesn't check version right now since currently we always
/// store only version=1." Our read path validates the stamp loudly so a
/// format change can't silently corrupt the registry.
///
/// The compaction filter walks this registry to decide which
/// `(cf_id, index_id)` prefixes to sweep.
pub mod dropped_indexes {
    use super::{
        delete, encode_gl_index_suffix, list_gl_index_marker_set, put, DataDictType,
    };
    use crate::codec::key::DDL_DROP_INDEX_ONGOING_VERSION;
    use crate::globals::GlIndexId;
    use slatedb::{Db, Error};

    fn encoded_version() -> [u8; 2] {
        DDL_DROP_INDEX_ONGOING_VERSION.to_be_bytes()
    }

    /// Add a single index to the dropped-index registry. Idempotent —
    /// adding an already-marked index re-writes the same version stamp.
    pub async fn add(db: &Db, gl: GlIndexId) -> Result<(), Error> {
        put(
            db,
            DataDictType::DdlDropIndexOngoing,
            &encode_gl_index_suffix(gl),
            &encoded_version(),
        )
        .await
    }

    /// Remove a single index. Idempotent — removing a missing index is a
    /// SlateDB-level tombstone write that's a no-op on the visible state.
    pub async fn remove(db: &Db, gl: GlIndexId) -> Result<(), Error> {
        delete(
            db,
            DataDictType::DdlDropIndexOngoing,
            &encode_gl_index_suffix(gl),
        )
        .await
    }

    /// Snapshot the dropped-index registry. Returns rows in
    /// byte-lexicographic order of `(cf_id, index_id)`.
    pub async fn list(db: &Db) -> Result<Vec<GlIndexId>, Error> {
        list_gl_index_marker_set(db, DataDictType::DdlDropIndexOngoing).await
    }
}

/// In-progress index-create registry.
///
/// Symmetric counterpart to [`dropped_indexes`]: one marker row per index
/// whose CREATE INDEX is mid-flight, at `DataDictType::DdlCreateIndexOngoing`.
/// Same suffix encoding (8-byte `u32_be(cf_id)||u32_be(index_id)`); value
/// is `u16_be(DDL_CREATE_INDEX_ONGOING_VERSION)` per MyRocks. Crash
/// recovery walks this set to decide whether to roll the in-progress
/// creation forward (commit) or back (drop the partial keyspace).
pub mod ddl_create_index_ongoing {
    use super::{
        delete, encode_gl_index_suffix, list_gl_index_marker_set, put, DataDictType,
    };
    use crate::codec::key::DDL_CREATE_INDEX_ONGOING_VERSION;
    use crate::globals::GlIndexId;
    use slatedb::{Db, Error};

    fn encoded_version() -> [u8; 2] {
        DDL_CREATE_INDEX_ONGOING_VERSION.to_be_bytes()
    }

    pub async fn add(db: &Db, gl: GlIndexId) -> Result<(), Error> {
        put(
            db,
            DataDictType::DdlCreateIndexOngoing,
            &encode_gl_index_suffix(gl),
            &encoded_version(),
        )
        .await
    }

    pub async fn remove(db: &Db, gl: GlIndexId) -> Result<(), Error> {
        delete(
            db,
            DataDictType::DdlCreateIndexOngoing,
            &encode_gl_index_suffix(gl),
        )
        .await
    }

    pub async fn list(db: &Db) -> Result<Vec<GlIndexId>, Error> {
        list_gl_index_marker_set(db, DataDictType::DdlCreateIndexOngoing).await
    }
}

/// Monotonic index-id allocator anchor.
///
/// Singleton row at `DataDictType::MaxIndexId` (empty suffix). Value is
/// `u16_be(MAX_INDEX_ID_VERSION) || u32_be(value)` (6 bytes) per MyRocks
/// (`rdb_datadic.cc:5334..5339`). Decoder validates the version stamp;
/// a future version surfaces as `ErrorKind::Data`.
///
/// The allocator pattern (read current, write current+1) is owner-side;
/// this substrate just exposes get/put on the singleton.
pub mod max_index_id {
    use super::{delete, get, put, DataDictType};
    use crate::codec::key::MAX_INDEX_ID_VERSION;
    use slatedb::{Db, Error};

    const VERSION_BYTES: usize = 2;
    const VALUE_BYTES: usize = 4;
    pub const ENCODED_LEN: usize = VERSION_BYTES + VALUE_BYTES;

    pub fn encode_value(value: u32) -> [u8; ENCODED_LEN] {
        let mut out = [0u8; ENCODED_LEN];
        out[..VERSION_BYTES].copy_from_slice(&MAX_INDEX_ID_VERSION.to_be_bytes());
        out[VERSION_BYTES..].copy_from_slice(&value.to_be_bytes());
        out
    }

    pub fn decode_value(bytes: &[u8]) -> Result<u32, Error> {
        if bytes.len() != ENCODED_LEN {
            return Err(Error::data(format!(
                "max_index_id value: expected {ENCODED_LEN} bytes, got {}",
                bytes.len()
            )));
        }
        let version = u16::from_be_bytes([bytes[0], bytes[1]]);
        if version > MAX_INDEX_ID_VERSION {
            return Err(Error::data(format!(
                "max_index_id: unsupported version {version} (latest is {MAX_INDEX_ID_VERSION})"
            )));
        }
        let mut buf = [0u8; VALUE_BYTES];
        buf.copy_from_slice(&bytes[VERSION_BYTES..]);
        Ok(u32::from_be_bytes(buf))
    }

    pub async fn read(db: &Db) -> Result<Option<u32>, Error> {
        match get(db, DataDictType::MaxIndexId, &[]).await? {
            Some(bytes) => Ok(Some(decode_value(&bytes)?)),
            None => Ok(None),
        }
    }

    pub async fn write(db: &Db, value: u32) -> Result<(), Error> {
        put(db, DataDictType::MaxIndexId, &[], &encode_value(value)).await
    }

    pub async fn remove(db: &Db) -> Result<(), Error> {
        delete(db, DataDictType::MaxIndexId, &[]).await
    }
}

/// Per-index metadata.
///
/// One row per index at `DataDictType::IndexInfo`. Suffix encodes the
/// `GlIndexId` as `u32_be(cf_id) || u32_be(index_id)` (8 bytes). Value
/// is a **schema-stamped record** — the encoder emits the latest format
/// (`IndexInfoVersion::FieldFlags`, v6) and the decoder handles every
/// version still in use in the wild.
///
/// Latest value format (`rdb_datadic.cc:4905`):
/// ```text
/// u16_be(version=6) || u8(index_type) || u16_be(kv_format_version)
///   || u32_be(index_flags) || u64_be(ttl_duration)
/// ```
/// Total: 17 bytes.
///
/// Legacy formats accepted on read:
/// - **Ttl (v5)**: drops `index_flags`; the decoder synthesises
///   `IndexFlag::TtlFlag` when `kv_format_version == PRIMARY_FORMAT_VERSION_TTL`
///   and `ttl_duration > 0` (faithful to the C++).
/// - **VerifyKvFormat (v4)** / **GlobalId (v3)**: drops both
///   `index_flags` and `ttl_duration`.
/// - **Initial (v1)** / **KvFormat (v2)**: rejected — the C++ also
///   treats these as "too old to decode."
///
/// Decoded `kv_format_version` is validated against
/// `PRIMARY_FORMAT_VERSION_LATEST` / `SECONDARY_FORMAT_VERSION_LATEST`
/// based on `index_type`; future-version values surface as
/// `ErrorKind::Data`.
pub mod index_info {
    use super::{delete, encode_gl_index_suffix, get, put, DataDictType};
    use crate::codec::key::{
        IndexFlag, IndexInfoVersion, IndexType, PRIMARY_FORMAT_VERSION_LATEST,
        PRIMARY_FORMAT_VERSION_TTL, SECONDARY_FORMAT_VERSION_LATEST,
    };
    use crate::globals::GlIndexId;
    use slatedb::{Db, Error};

    /// Decoded per-index metadata row.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct IndexInfo {
        pub index_dict_version: IndexInfoVersion,
        pub index_type: IndexType,
        pub kv_format_version: u16,
        pub index_flags: u32,
        pub ttl_duration: u64,
    }

    const LEN_FIELD_FLAGS: usize = 2 + 1 + 2 + 4 + 8; // 17
    const LEN_TTL: usize = 2 + 1 + 2 + 8; // 13
    const LEN_VERIFY_OR_GLOBAL_ID: usize = 2 + 1 + 2; // 5

    fn decode_index_type(b: u8) -> Result<IndexType, Error> {
        match b {
            1 => Ok(IndexType::Primary),
            2 => Ok(IndexType::Secondary),
            3 => Ok(IndexType::HiddenPrimary),
            other => Err(Error::data(format!(
                "index_info: unknown index_type byte 0x{other:02x}"
            ))),
        }
    }

    fn decode_index_info_version(v: u16) -> Result<IndexInfoVersion, Error> {
        match v {
            1 => Ok(IndexInfoVersion::Initial),
            2 => Ok(IndexInfoVersion::KvFormat),
            3 => Ok(IndexInfoVersion::GlobalId),
            4 => Ok(IndexInfoVersion::VerifyKvFormat),
            5 => Ok(IndexInfoVersion::Ttl),
            6 => Ok(IndexInfoVersion::FieldFlags),
            other => Err(Error::data(format!(
                "index_info: unknown version {other}"
            ))),
        }
    }

    fn validate_kv_format_version(
        index_type: IndexType,
        kv_format_version: u16,
    ) -> Result<(), Error> {
        let max = match index_type {
            IndexType::Primary | IndexType::HiddenPrimary => PRIMARY_FORMAT_VERSION_LATEST,
            IndexType::Secondary => SECONDARY_FORMAT_VERSION_LATEST,
        };
        if kv_format_version > max {
            return Err(Error::data(format!(
                "index_info: kv_format_version {kv_format_version} exceeds max {max} for {index_type:?}"
            )));
        }
        Ok(())
    }

    /// Encode `info` in the latest format (`FieldFlags`).
    pub fn encode_value(info: &IndexInfo) -> Vec<u8> {
        let mut out = Vec::with_capacity(LEN_FIELD_FLAGS);
        out.extend_from_slice(&(IndexInfoVersion::FieldFlags as u16).to_be_bytes());
        out.push(info.index_type as u8);
        out.extend_from_slice(&info.kv_format_version.to_be_bytes());
        out.extend_from_slice(&info.index_flags.to_be_bytes());
        out.extend_from_slice(&info.ttl_duration.to_be_bytes());
        out
    }

    /// Decode any forward-compatible format. Returns `Err(Data)` on
    /// unknown version, length mismatch, bad index_type, or
    /// future-version `kv_format_version`.
    pub fn decode_value(bytes: &[u8]) -> Result<IndexInfo, Error> {
        if bytes.len() < 2 {
            return Err(Error::data(
                "index_info: value truncated before version field".into(),
            ));
        }
        let version_u16 = u16::from_be_bytes([bytes[0], bytes[1]]);
        let version = decode_index_info_version(version_u16)?;

        let info = match version {
            IndexInfoVersion::FieldFlags => {
                if bytes.len() != LEN_FIELD_FLAGS {
                    return Err(Error::data(format!(
                        "index_info FieldFlags: expected {LEN_FIELD_FLAGS} bytes, got {}",
                        bytes.len()
                    )));
                }
                IndexInfo {
                    index_dict_version: version,
                    index_type: decode_index_type(bytes[2])?,
                    kv_format_version: u16::from_be_bytes([bytes[3], bytes[4]]),
                    index_flags: u32::from_be_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]),
                    ttl_duration: u64::from_be_bytes([
                        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                        bytes[15], bytes[16],
                    ]),
                }
            }
            IndexInfoVersion::Ttl => {
                if bytes.len() != LEN_TTL {
                    return Err(Error::data(format!(
                        "index_info Ttl: expected {LEN_TTL} bytes, got {}",
                        bytes.len()
                    )));
                }
                let index_type = decode_index_type(bytes[2])?;
                let kv_format_version = u16::from_be_bytes([bytes[3], bytes[4]]);
                let ttl_duration = u64::from_be_bytes([
                    bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11],
                    bytes[12],
                ]);
                // C++ synthesises TtlFlag for PK with TTL ≥ format version.
                let index_flags =
                    if kv_format_version == PRIMARY_FORMAT_VERSION_TTL && ttl_duration > 0 {
                        IndexFlag::TtlFlag as u32
                    } else {
                        0
                    };
                IndexInfo {
                    index_dict_version: version,
                    index_type,
                    kv_format_version,
                    index_flags,
                    ttl_duration,
                }
            }
            IndexInfoVersion::VerifyKvFormat | IndexInfoVersion::GlobalId => {
                if bytes.len() != LEN_VERIFY_OR_GLOBAL_ID {
                    return Err(Error::data(format!(
                        "index_info {version:?}: expected {LEN_VERIFY_OR_GLOBAL_ID} bytes, got {}",
                        bytes.len()
                    )));
                }
                IndexInfo {
                    index_dict_version: version,
                    index_type: decode_index_type(bytes[2])?,
                    kv_format_version: u16::from_be_bytes([bytes[3], bytes[4]]),
                    index_flags: 0,
                    ttl_duration: 0,
                }
            }
            IndexInfoVersion::Initial | IndexInfoVersion::KvFormat => {
                return Err(Error::data(format!(
                    "index_info: version {version:?} too old to decode"
                )));
            }
        };

        validate_kv_format_version(info.index_type, info.kv_format_version)?;
        Ok(info)
    }

    pub async fn read(db: &Db, gl: GlIndexId) -> Result<Option<IndexInfo>, Error> {
        match get(db, DataDictType::IndexInfo, &encode_gl_index_suffix(gl)).await? {
            Some(bytes) => Ok(Some(decode_value(&bytes)?)),
            None => Ok(None),
        }
    }

    pub async fn write(db: &Db, gl: GlIndexId, info: &IndexInfo) -> Result<(), Error> {
        put(
            db,
            DataDictType::IndexInfo,
            &encode_gl_index_suffix(gl),
            &encode_value(info),
        )
        .await
    }

    pub async fn remove(db: &Db, gl: GlIndexId) -> Result<(), Error> {
        delete(db, DataDictType::IndexInfo, &encode_gl_index_suffix(gl)).await
    }
}

/// Binlog position singleton.
///
/// One row at `DataDictType::BinlogInfoIndexNumber` (empty suffix) whose
/// opaque value is the serialised `(file_name, pos, gtid)` triple. The
/// binlog manager (when translated) defines the serialisation; this
/// substrate just reads / writes raw bytes.
pub mod binlog_info {
    use super::{delete, get, put, DataDictType};
    use bytes::Bytes;
    use slatedb::{Db, Error};

    pub async fn read(db: &Db) -> Result<Option<Bytes>, Error> {
        get(db, DataDictType::BinlogInfoIndexNumber, &[]).await
    }

    pub async fn write(db: &Db, value: &[u8]) -> Result<(), Error> {
        put(db, DataDictType::BinlogInfoIndexNumber, &[], value).await
    }

    pub async fn clear(db: &Db) -> Result<(), Error> {
        delete(db, DataDictType::BinlogInfoIndexNumber, &[]).await
    }
}

/// Table-version stamp.
///
/// One row per table at `DataDictType::TableVersion`. Suffix is the
/// table's logical path (e.g. `"./db_name/table_name"`); value is a
/// u64 big-endian schema version. The C++ wraps the path in a
/// `"MariaDB:table-version:"` literal — we drop that since we don't
/// preserve MyRocks on-disk format compat (different engine, fresh DB).
pub mod table_version {
    use super::{delete, get, put, DataDictType};
    use slatedb::{Db, Error};

    fn encode(value: u64) -> [u8; 8] {
        value.to_be_bytes()
    }

    fn decode(bytes: &[u8]) -> Result<u64, Error> {
        if bytes.len() != 8 {
            return Err(Error::data(format!(
                "table_version value: expected 8 bytes, got {}",
                bytes.len()
            )));
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(bytes);
        Ok(u64::from_be_bytes(buf))
    }

    pub async fn read(db: &Db, path: &str) -> Result<Option<u64>, Error> {
        match get(db, DataDictType::TableVersion, path.as_bytes()).await? {
            Some(bytes) => Ok(Some(decode(&bytes)?)),
            None => Ok(None),
        }
    }

    pub async fn write(db: &Db, path: &str, value: u64) -> Result<(), Error> {
        put(db, DataDictType::TableVersion, path.as_bytes(), &encode(value)).await
    }

    pub async fn remove(db: &Db, path: &str) -> Result<(), Error> {
        delete(db, DataDictType::TableVersion, path.as_bytes()).await
    }
}

/// DDL-entry index-start-number record.
///
/// One row per table at `DataDictType::DdlEntryIndexStartNumber`. Suffix
/// is the normalised table name (e.g. `"dbname.tablename"`); value is
/// raw bytes whose encoding is owned by the higher-level DDL manager
/// when that lands.
///
/// MyRocks' on-disk format for the value is
/// `u16_be(version) || (u32_be(cf_id) || u32_be(index_id))*N` — the
/// list of indexes owned by the table. The DDL manager will provide a
/// typed encode/decode pair on top of this substrate; today's callers
/// can shape their own.
pub mod ddl_entry_index_start_number {
    use super::{delete, get, put, DataDictType};
    use bytes::Bytes;
    use slatedb::{Db, Error};

    pub async fn read(db: &Db, table_name: &str) -> Result<Option<Bytes>, Error> {
        get(
            db,
            DataDictType::DdlEntryIndexStartNumber,
            table_name.as_bytes(),
        )
        .await
    }

    pub async fn write(db: &Db, table_name: &str, value: &[u8]) -> Result<(), Error> {
        put(
            db,
            DataDictType::DdlEntryIndexStartNumber,
            table_name.as_bytes(),
            value,
        )
        .await
    }

    pub async fn remove(db: &Db, table_name: &str) -> Result<(), Error> {
        delete(
            db,
            DataDictType::DdlEntryIndexStartNumber,
            table_name.as_bytes(),
        )
        .await
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

    fn gl(cf_id: u32, index_id: u32) -> crate::globals::GlIndexId {
        crate::globals::GlIndexId { cf_id, index_id }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn autoinc_put_round_trip_per_index() {
        let engine = EngineDb::open_in_memory("autoinc_rt").await.expect("open");
        let users_pk = gl(1, 100);

        assert_eq!(
            autoinc::read(engine.db(), users_pk).await.expect("read miss"),
            None
        );

        autoinc::write(engine.db(), users_pk, 42).await.expect("write");
        assert_eq!(
            autoinc::read(engine.db(), users_pk).await.expect("read"),
            Some(42)
        );

        // Overwrite with a higher value via Put.
        autoinc::write(engine.db(), users_pk, 100_000)
            .await
            .expect("write");
        assert_eq!(
            autoinc::read(engine.db(), users_pk).await.expect("read"),
            Some(100_000)
        );

        autoinc::remove(engine.db(), users_pk).await.expect("remove");
        assert_eq!(
            autoinc::read(engine.db(), users_pk).await.expect("read"),
            None
        );

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn autoinc_bump_takes_max_via_merge_operator() {
        let engine = EngineDb::open_in_memory("autoinc_bump")
            .await
            .expect("open");
        let pk = gl(2, 200);

        // Three bumps: 5, 3, 10 → MAX-merge → 10.
        autoinc::bump(engine.db(), pk, 5).await.expect("bump 5");
        autoinc::bump(engine.db(), pk, 3).await.expect("bump 3");
        autoinc::bump(engine.db(), pk, 10).await.expect("bump 10");
        assert_eq!(
            autoinc::read(engine.db(), pk).await.expect("read"),
            Some(10)
        );

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn autoinc_distinct_indexes_have_independent_counters() {
        let engine = EngineDb::open_in_memory("autoinc_per_index")
            .await
            .expect("open");
        let a = gl(1, 100);
        let b = gl(1, 101);

        autoinc::write(engine.db(), a, 42).await.expect("write a");
        autoinc::write(engine.db(), b, 99).await.expect("write b");
        assert_eq!(autoinc::read(engine.db(), a).await.expect("read"), Some(42));
        assert_eq!(autoinc::read(engine.db(), b).await.expect("read"), Some(99));

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn autoinc_corrupt_value_surfaces_data_error() {
        let engine = EngineDb::open_in_memory("autoinc_corrupt")
            .await
            .expect("open");
        let pk = gl(3, 300);

        // Write a 3-byte payload through the raw substrate, bypassing autoinc.
        // Suffix must match what autoinc::read computes from GlIndexId.
        let suffix = encode_gl_index_suffix(pk);
        put(engine.db(), DataDictType::AutoInc, &suffix, b"abc")
            .await
            .expect("put");

        let err = autoinc::read(engine.db(), pk).await.unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Data));

        engine.close().await.expect("close");
    }

    #[test]
    fn autoinc_encode_decode_round_trip() {
        for v in [0u64, 1, 42, 0xdead_beef, u64::MAX] {
            let bytes = autoinc::encode_value(v);
            assert_eq!(bytes.len(), autoinc::ENCODED_LEN);
            // Version is the leading u16 BE.
            assert_eq!(
                u16::from_be_bytes([bytes[0], bytes[1]]),
                crate::codec::key::AUTO_INCREMENT_VERSION
            );
            assert_eq!(autoinc::decode_value(&bytes).expect("decode"), v);
        }
    }

    #[test]
    fn autoinc_decode_rejects_future_version() {
        let mut bytes = [0u8; autoinc::ENCODED_LEN];
        bytes[0..2].copy_from_slice(&(crate::codec::key::AUTO_INCREMENT_VERSION + 1).to_be_bytes());
        let err = autoinc::decode_value(&bytes).unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Data));
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

    // ----- ddl_create_index_ongoing -----

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn create_index_ongoing_does_not_collide_with_drop() {
        let engine = EngineDb::open_in_memory("create_ongoing_iso")
            .await
            .expect("open");

        let gl = crate::globals::GlIndexId {
            cf_id: 1,
            index_id: 42,
        };

        // Add to BOTH registries with the same gl_index_id — they live at
        // different DataDictType prefixes and must not bleed across.
        dropped_indexes::add(engine.db(), gl).await.expect("drop add");
        ddl_create_index_ongoing::add(engine.db(), gl)
            .await
            .expect("create add");

        assert_eq!(
            dropped_indexes::list(engine.db()).await.expect("drop list"),
            vec![gl]
        );
        assert_eq!(
            ddl_create_index_ongoing::list(engine.db())
                .await
                .expect("create list"),
            vec![gl]
        );

        // Removing one leaves the other intact.
        ddl_create_index_ongoing::remove(engine.db(), gl)
            .await
            .expect("create remove");
        assert!(ddl_create_index_ongoing::list(engine.db())
            .await
            .expect("create list 2")
            .is_empty());
        assert_eq!(
            dropped_indexes::list(engine.db()).await.expect("drop list 2"),
            vec![gl]
        );

        engine.close().await.expect("close");
    }

    // ----- max_index_id -----

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn max_index_id_round_trip() {
        let engine = EngineDb::open_in_memory("max_idx_rt")
            .await
            .expect("open");

        assert_eq!(max_index_id::read(engine.db()).await.expect("read"), None);
        max_index_id::write(engine.db(), 1)
            .await
            .expect("write 1");
        assert_eq!(
            max_index_id::read(engine.db()).await.expect("read"),
            Some(1)
        );
        max_index_id::write(engine.db(), 12345)
            .await
            .expect("write bump");
        assert_eq!(
            max_index_id::read(engine.db()).await.expect("read"),
            Some(12345)
        );
        max_index_id::remove(engine.db())
            .await
            .expect("remove");
        assert_eq!(max_index_id::read(engine.db()).await.expect("read"), None);

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn max_index_id_corrupt_value_is_data_error() {
        let engine = EngineDb::open_in_memory("max_idx_corrupt")
            .await
            .expect("open");
        // Bypass the typed writer with a 3-byte payload (wrong length).
        put(engine.db(), DataDictType::MaxIndexId, &[], b"abc")
            .await
            .expect("put");
        let err = max_index_id::read(engine.db()).await.unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Data));
        engine.close().await.expect("close");
    }

    #[test]
    fn max_index_id_encode_format_is_version_then_u32_be() {
        let bytes = max_index_id::encode_value(0x1234_5678);
        assert_eq!(bytes.len(), max_index_id::ENCODED_LEN);
        assert_eq!(
            u16::from_be_bytes([bytes[0], bytes[1]]),
            crate::codec::key::MAX_INDEX_ID_VERSION
        );
        assert_eq!(&bytes[2..], &0x1234_5678u32.to_be_bytes());
        assert_eq!(max_index_id::decode_value(&bytes).expect("decode"), 0x1234_5678);
    }

    #[test]
    fn max_index_id_decode_rejects_future_version() {
        let mut bytes = [0u8; max_index_id::ENCODED_LEN];
        bytes[..2]
            .copy_from_slice(&(crate::codec::key::MAX_INDEX_ID_VERSION + 1).to_be_bytes());
        assert!(matches!(
            max_index_id::decode_value(&bytes).unwrap_err().kind(),
            slatedb::ErrorKind::Data
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropped_indexes_value_carries_version_stamp() {
        let engine = EngineDb::open_in_memory("dropped_idx_value_fmt")
            .await
            .expect("open");
        let gl = crate::globals::GlIndexId { cf_id: 1, index_id: 1 };
        dropped_indexes::add(engine.db(), gl).await.expect("add");

        // Inspect the stored value directly via the substrate.
        let suffix = encode_gl_index_suffix(gl);
        let raw = get(engine.db(), DataDictType::DdlDropIndexOngoing, &suffix)
            .await
            .expect("get")
            .expect("present");
        assert_eq!(raw.len(), 2);
        assert_eq!(
            u16::from_be_bytes([raw[0], raw[1]]),
            crate::codec::key::DDL_DROP_INDEX_ONGOING_VERSION
        );

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ddl_create_index_ongoing_value_carries_version_stamp() {
        let engine = EngineDb::open_in_memory("create_ongoing_value_fmt")
            .await
            .expect("open");
        let gl = crate::globals::GlIndexId { cf_id: 1, index_id: 1 };
        ddl_create_index_ongoing::add(engine.db(), gl)
            .await
            .expect("add");

        let suffix = encode_gl_index_suffix(gl);
        let raw = get(engine.db(), DataDictType::DdlCreateIndexOngoing, &suffix)
            .await
            .expect("get")
            .expect("present");
        assert_eq!(raw.len(), 2);
        assert_eq!(
            u16::from_be_bytes([raw[0], raw[1]]),
            crate::codec::key::DDL_CREATE_INDEX_ONGOING_VERSION
        );

        engine.close().await.expect("close");
    }

    // ----- binlog_info -----

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn binlog_info_round_trip_and_clear() {
        let engine = EngineDb::open_in_memory("binlog_info_rt")
            .await
            .expect("open");

        assert!(binlog_info::read(engine.db())
            .await
            .expect("read")
            .is_none());

        binlog_info::write(engine.db(), b"mariadb-bin.000001\x00\x00\x00pos:42")
            .await
            .expect("write");
        let got = binlog_info::read(engine.db())
            .await
            .expect("read")
            .expect("present");
        assert_eq!(&got[..], b"mariadb-bin.000001\x00\x00\x00pos:42");

        // Overwrite with a different payload.
        binlog_info::write(engine.db(), b"new-bin-position")
            .await
            .expect("rewrite");
        let got = binlog_info::read(engine.db())
            .await
            .expect("read")
            .expect("present");
        assert_eq!(&got[..], b"new-bin-position");

        binlog_info::clear(engine.db()).await.expect("clear");
        assert!(binlog_info::read(engine.db())
            .await
            .expect("read")
            .is_none());

        engine.close().await.expect("close");
    }

    // ----- table_version -----

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn table_version_round_trip_per_path() {
        let engine = EngineDb::open_in_memory("tv_rt")
            .await
            .expect("open");

        assert_eq!(
            table_version::read(engine.db(), "./db1/t").await.expect("read"),
            None
        );

        table_version::write(engine.db(), "./db1/t", 7)
            .await
            .expect("write");
        assert_eq!(
            table_version::read(engine.db(), "./db1/t").await.expect("read"),
            Some(7)
        );

        // Different path is its own slot.
        table_version::write(engine.db(), "./db2/t", 99)
            .await
            .expect("write 2");
        assert_eq!(
            table_version::read(engine.db(), "./db1/t").await.expect("read"),
            Some(7)
        );
        assert_eq!(
            table_version::read(engine.db(), "./db2/t").await.expect("read"),
            Some(99)
        );

        // Overwrite.
        table_version::write(engine.db(), "./db1/t", 42)
            .await
            .expect("overwrite");
        assert_eq!(
            table_version::read(engine.db(), "./db1/t").await.expect("read"),
            Some(42)
        );

        // Remove one, leave the other.
        table_version::remove(engine.db(), "./db1/t")
            .await
            .expect("remove");
        assert_eq!(
            table_version::read(engine.db(), "./db1/t").await.expect("read"),
            None
        );
        assert_eq!(
            table_version::read(engine.db(), "./db2/t").await.expect("read"),
            Some(99)
        );

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn table_version_corrupt_value_is_data_error() {
        let engine = EngineDb::open_in_memory("tv_corrupt")
            .await
            .expect("open");
        put(
            engine.db(),
            DataDictType::TableVersion,
            b"./db1/t",
            b"abc", // wrong size
        )
        .await
        .expect("put");
        let err = table_version::read(engine.db(), "./db1/t")
            .await
            .unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Data));
        engine.close().await.expect("close");
    }

    // ----- ddl_entry_index_start_number -----

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ddl_entry_index_start_number_raw_round_trip() {
        let engine = EngineDb::open_in_memory("ddl_entry_rt")
            .await
            .expect("open");

        assert!(ddl_entry_index_start_number::read(engine.db(), "db.t")
            .await
            .expect("read")
            .is_none());

        // Substrate is opaque-bytes; tests use a sentinel payload that
        // resembles the MyRocks shape (version u16_be + two pairs).
        let payload = b"\x00\x01\x00\x00\x00\x01\x00\x00\x00\x0a\x00\x00\x00\x01\x00\x00\x00\x0b";
        ddl_entry_index_start_number::write(engine.db(), "db.t", payload)
            .await
            .expect("write");
        let got = ddl_entry_index_start_number::read(engine.db(), "db.t")
            .await
            .expect("read")
            .expect("present");
        assert_eq!(&got[..], payload);

        ddl_entry_index_start_number::remove(engine.db(), "db.t")
            .await
            .expect("remove");
        assert!(ddl_entry_index_start_number::read(engine.db(), "db.t")
            .await
            .expect("read")
            .is_none());

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ddl_entry_keyed_per_table() {
        let engine = EngineDb::open_in_memory("ddl_entry_per_table")
            .await
            .expect("open");

        ddl_entry_index_start_number::write(engine.db(), "db.t1", b"one")
            .await
            .expect("write t1");
        ddl_entry_index_start_number::write(engine.db(), "db.t2", b"two")
            .await
            .expect("write t2");

        assert_eq!(
            ddl_entry_index_start_number::read(engine.db(), "db.t1")
                .await
                .expect("read t1")
                .expect("present")
                .as_ref(),
            b"one"
        );
        assert_eq!(
            ddl_entry_index_start_number::read(engine.db(), "db.t2")
                .await
                .expect("read t2")
                .expect("present")
                .as_ref(),
            b"two"
        );

        engine.close().await.expect("close");
    }

    // ----- index_info -----

    fn sample_info(
        ttl: u64,
        flags: u32,
        index_type: crate::codec::key::IndexType,
    ) -> index_info::IndexInfo {
        index_info::IndexInfo {
            index_dict_version: crate::codec::key::IndexInfoVersion::FieldFlags,
            index_type,
            kv_format_version: match index_type {
                crate::codec::key::IndexType::Secondary => {
                    crate::codec::key::SECONDARY_FORMAT_VERSION_LATEST
                }
                _ => crate::codec::key::PRIMARY_FORMAT_VERSION_LATEST,
            },
            index_flags: flags,
            ttl_duration: ttl,
        }
    }

    #[test]
    fn index_info_encode_then_decode_latest_format_round_trips() {
        let info = sample_info(3600, 1, crate::codec::key::IndexType::Primary);
        let bytes = index_info::encode_value(&info);
        assert_eq!(bytes.len(), 17, "FieldFlags encoded length");
        let back = index_info::decode_value(&bytes).expect("decode");
        assert_eq!(back, info);
    }

    #[test]
    fn index_info_decode_legacy_ttl_synthesises_ttl_flag_for_pk() {
        // Hand-build a legacy Ttl (v5) value: u16 ver | u8 type | u16 kv | u64 ttl.
        let mut bytes = Vec::with_capacity(13);
        bytes.extend_from_slice(
            &(crate::codec::key::IndexInfoVersion::Ttl as u16).to_be_bytes(),
        );
        bytes.push(crate::codec::key::IndexType::Primary as u8);
        bytes.extend_from_slice(
            &crate::codec::key::PRIMARY_FORMAT_VERSION_TTL.to_be_bytes(),
        );
        bytes.extend_from_slice(&60u64.to_be_bytes());

        let info = index_info::decode_value(&bytes).expect("decode");
        assert_eq!(info.index_dict_version, crate::codec::key::IndexInfoVersion::Ttl);
        assert_eq!(info.ttl_duration, 60);
        assert_eq!(
            info.index_flags,
            crate::codec::key::IndexFlag::TtlFlag as u32,
            "TtlFlag synthesised when PK + TTL format + ttl>0"
        );
    }

    #[test]
    fn index_info_decode_legacy_ttl_no_flag_when_ttl_zero() {
        let mut bytes = Vec::with_capacity(13);
        bytes.extend_from_slice(
            &(crate::codec::key::IndexInfoVersion::Ttl as u16).to_be_bytes(),
        );
        bytes.push(crate::codec::key::IndexType::Primary as u8);
        bytes.extend_from_slice(
            &crate::codec::key::PRIMARY_FORMAT_VERSION_TTL.to_be_bytes(),
        );
        bytes.extend_from_slice(&0u64.to_be_bytes()); // ttl=0 → no synthesis

        let info = index_info::decode_value(&bytes).expect("decode");
        assert_eq!(info.index_flags, 0);
    }

    #[test]
    fn index_info_decode_global_id_format_yields_zero_flags_and_ttl() {
        let mut bytes = Vec::with_capacity(5);
        bytes.extend_from_slice(
            &(crate::codec::key::IndexInfoVersion::GlobalId as u16).to_be_bytes(),
        );
        bytes.push(crate::codec::key::IndexType::Secondary as u8);
        bytes.extend_from_slice(
            &crate::codec::key::SECONDARY_FORMAT_VERSION_INITIAL.to_be_bytes(),
        );

        let info = index_info::decode_value(&bytes).expect("decode");
        assert_eq!(info.index_dict_version, crate::codec::key::IndexInfoVersion::GlobalId);
        assert_eq!(info.index_flags, 0);
        assert_eq!(info.ttl_duration, 0);
    }

    #[test]
    fn index_info_decode_rejects_unknown_version() {
        // Version 99 — out of range.
        let bytes = [0, 99, 0, 0, 0];
        let err = index_info::decode_value(&bytes).unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Data));
    }

    #[test]
    fn index_info_decode_rejects_initial_or_kvformat_versions() {
        for v in [1u16, 2] {
            let bytes = v.to_be_bytes().to_vec();
            let err = index_info::decode_value(&bytes).unwrap_err();
            assert!(matches!(err.kind(), slatedb::ErrorKind::Data));
        }
    }

    #[test]
    fn index_info_decode_rejects_size_mismatch() {
        // FieldFlags version but wrong byte count.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(
            &(crate::codec::key::IndexInfoVersion::FieldFlags as u16).to_be_bytes(),
        );
        // … and that's it. Far short of 17 bytes.
        let err = index_info::decode_value(&bytes).unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Data));
    }

    #[test]
    fn index_info_decode_rejects_bad_index_type_byte() {
        // Latest format with a bogus type byte (0xff).
        let mut bytes = Vec::with_capacity(17);
        bytes.extend_from_slice(
            &(crate::codec::key::IndexInfoVersion::FieldFlags as u16).to_be_bytes(),
        );
        bytes.push(0xff); // invalid index_type
        bytes.extend_from_slice(&[0; 14]);
        let err = index_info::decode_value(&bytes).unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Data));
    }

    #[test]
    fn index_info_decode_rejects_future_kv_format_version() {
        // Latest format, valid type, but kv_format_version above the
        // PRIMARY max → Data error.
        let mut bytes = Vec::with_capacity(17);
        bytes.extend_from_slice(
            &(crate::codec::key::IndexInfoVersion::FieldFlags as u16).to_be_bytes(),
        );
        bytes.push(crate::codec::key::IndexType::Primary as u8);
        bytes.extend_from_slice(
            &(crate::codec::key::PRIMARY_FORMAT_VERSION_LATEST + 1).to_be_bytes(),
        );
        bytes.extend_from_slice(&[0; 12]);
        let err = index_info::decode_value(&bytes).unwrap_err();
        assert!(matches!(err.kind(), slatedb::ErrorKind::Data));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn index_info_round_trip_via_engine() {
        let engine = EngineDb::open_in_memory("idxinfo_rt")
            .await
            .expect("open");
        let gl = crate::globals::GlIndexId {
            cf_id: 3,
            index_id: 42,
        };
        assert!(index_info::read(engine.db(), gl).await.expect("read").is_none());

        let info = sample_info(7200, 1, crate::codec::key::IndexType::Secondary);
        index_info::write(engine.db(), gl, &info)
            .await
            .expect("write");
        let back = index_info::read(engine.db(), gl)
            .await
            .expect("read")
            .expect("present");
        assert_eq!(back, info);

        index_info::remove(engine.db(), gl).await.expect("remove");
        assert!(index_info::read(engine.db(), gl).await.expect("read").is_none());

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn index_info_distinct_gl_index_ids_are_isolated() {
        let engine = EngineDb::open_in_memory("idxinfo_isolated")
            .await
            .expect("open");
        let a = crate::globals::GlIndexId { cf_id: 1, index_id: 10 };
        let b = crate::globals::GlIndexId { cf_id: 1, index_id: 11 };
        let c = crate::globals::GlIndexId { cf_id: 2, index_id: 10 };

        index_info::write(
            engine.db(),
            a,
            &sample_info(60, 1, crate::codec::key::IndexType::Primary),
        )
        .await
        .expect("write a");
        index_info::write(
            engine.db(),
            b,
            &sample_info(120, 0, crate::codec::key::IndexType::Secondary),
        )
        .await
        .expect("write b");

        let a_read = index_info::read(engine.db(), a).await.expect("read a").expect("present");
        let b_read = index_info::read(engine.db(), b).await.expect("read b").expect("present");
        assert_eq!(a_read.ttl_duration, 60);
        assert_eq!(b_read.ttl_duration, 120);
        assert!(index_info::read(engine.db(), c).await.expect("read c").is_none());

        engine.close().await.expect("close");
    }
}
