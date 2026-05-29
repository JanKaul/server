//! Interface stub for `ha_rocksdb_cc____free__table_version`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 14658..14725, ~68 LoC).
//! v4 manifest sub-unit: `ha_rocksdb_cc____free__table_version`
//!
//! ## Mapping
//! MyRocks stores a per-table version number in the data dictionary; MariaDB
//! reads it via `ha_rocksdb::table_version()` to detect schema changes and
//! invalidate cached query plans.
//!
//! Key format (preserved from MyRocks):
//! ```text
//! u32_be(TABLE_VERSION_INDEX_ID) || "MariaDB:table-version:" || path
//! ```
//! Value: big-endian `u64`.
//!
//! Per _DESIGN.md §2 (key encoding preserved) and §1 row "data dictionary"
//! (re-impl, stored under the `__system__` CF prefix `cf_id = u32::MAX`):
//! these calls go through `crate::engine::db::EngineSlateDb` to put/get/del
//! the version row from the system CF.
//!
//! All three operations are part of an in-progress `WriteBatch` on the
//! C++ side (so they appear atomically with the schema change). The Rust
//! side accepts a `&mut slatedb::WriteBatch` for the `save`/`delete` ops.
//!
//! ## Out-of-scope methods
//! - None — fully in scope.

use slatedb::Error;

use crate::rdb_global_h::SYSTEM_CF_ID;

/// `Rdb_key_def::TABLE_VERSION` constant. The MyRocks dict reserves a small
/// set of "magic index_ids" for metadata; this is one of them.
/// TODO(human): cross-check the actual number from `rdb_datadic.h` —
/// in the C++ it's `Rdb_key_def::TABLE_VERSION` (an enum value).
pub const TABLE_VERSION_INDEX_ID: u32 = /* TODO */ 0xFFFF_FFFB;

/// Build the lookup key for a given `path` ("./db/tbl"). Format must match
/// the C++ bit-for-bit so we can read MyRocks-written values during
/// migration.
/// Original: ha_rocksdb.cc:14658 — `make_table_version_lookup_key`.
pub fn make_lookup_key(path: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 22 + path.len());
    out.extend_from_slice(&TABLE_VERSION_INDEX_ID.to_be_bytes());
    out.extend_from_slice(b"MariaDB:table-version:");
    out.extend_from_slice(path.as_bytes());
    out
}

/// Queue a put of `(version)` to the system CF inside the given batch.
/// Caller commits the batch as part of the surrounding DDL operation.
///
/// Per _DESIGN.md §2, the system CF prefix is `varint(u32::MAX)`; the rest
/// of the key is the bytes returned by `make_lookup_key`.
///
/// Original: ha_rocksdb.cc:14677 — `save_table_version`.
pub fn save_table_version(
    _batch: &mut slatedb::WriteBatch,
    path: &str,
    version: u64,
) {
    let lookup_key = make_lookup_key(path);
    let _system_prefixed = prepend_system_cf_prefix(&lookup_key);
    let _val = version.to_be_bytes();
    todo!("batch.put(Bytes::from(_system_prefixed), Bytes::copy_from_slice(&_val))")
}

/// Read the version. Returns:
///   - `Ok(0)` if not present (matches MyRocks contract — fresh tables);
///   - `Ok(u64::MAX)` (= `ulonglong(-1)` in C++) on read error or malformed
///     value (preserved for bug-for-bug equivalence);
///   - `Err(slatedb::Error)` only on a true SlateDB I/O failure.
///
/// Original: ha_rocksdb.cc:14696 — `get_table_version`.
pub async fn get_table_version(_db: &slatedb::Db, path: &str) -> Result<u64, Error> {
    let lookup_key = make_lookup_key(path);
    let _system_prefixed = prepend_system_cf_prefix(&lookup_key);
    todo!(
        "match db.get(Bytes::from(_system_prefixed)).await? {{\n\
         \tSome(v) if v.len() == 8 => Ok(u64::from_be_bytes(v.as_ref().try_into().unwrap())),\n\
         \tSome(_) => Ok(u64::MAX),  // bug-for-bug with C++ ulonglong(-1)\n\
         \tNone => Ok(0),\n\
         }}"
    )
}

/// Queue a delete of the version key in the given batch.
/// Original: ha_rocksdb.cc:14718 — `delete_table_version`.
pub fn delete_table_version(_batch: &mut slatedb::WriteBatch, path: &str) {
    let lookup_key = make_lookup_key(path);
    let _system_prefixed = prepend_system_cf_prefix(&lookup_key);
    todo!("batch.delete(Bytes::from(_system_prefixed))")
}

/// Prepend `varint(SYSTEM_CF_ID)` to put the lookup key into the system CF.
/// Per _DESIGN.md §2.
fn prepend_system_cf_prefix(key: &[u8]) -> Vec<u8> {
    // varint encoding of u32::MAX is 5 bytes (0xFF FF FF FF 0F).
    // TODO(human): factor this into `codec::prefix` and reuse.
    let mut out = Vec::with_capacity(5 + key.len());
    let mut v = SYSTEM_CF_ID;
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 { out.push(byte); break; }
        out.push(byte | 0x80);
    }
    out.extend_from_slice(key);
    out
}
