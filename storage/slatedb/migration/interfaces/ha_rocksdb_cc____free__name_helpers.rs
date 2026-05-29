//! Interface stub for `ha_rocksdb_cc____free__name_helpers`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 367..372 + 3827..3854 + 4027..4046 + 7584..7710, ~165 LoC).
//! v4 manifest sub-unit: `ha_rocksdb_cc____free__name_helpers`
//!
//! ## Mapping
//! Path / table-name parsing helpers — completely engine-internal, no SlateDB
//! API involvement. Direct Rust translation.
//!
//!   - `rdb_normalize_dir(dir)` — strip trailing `/` from a directory path.
//!   - `rdb_normalize_tablename(./db/tbl, &mut "db.tbl")` — MySQL's on-disk
//!     `./dbname/tablename` → logical `dbname.tablename`.
//!   - `rdb_split_normalized_tablename("db.tbl#P#p0", db, tbl, part)` —
//!     parse partition suffix.
//!   - `rdb_xid_to_string(XID)` / `rdb_xid_from_string(str)` — XA xid
//!     packing. Format: `u64_be(formatID) || u8(gtrid_len) || u8(bqual_len) || gtrid_bytes || bqual_bytes`.
//!   - `corruption_marker_file_name()` — already lives in `error_helpers`;
//!     we don't duplicate.
//!
//! All preserve bit-for-bit format with MyRocks so we can read/write the
//! same on-disk data + the same XA-recovery records.
//!
//! ## Out-of-scope methods
//! - None — these are pure data manipulation.

use bytes::Bytes;
use slatedb::Error;

/// Strip trailing `/` characters from a directory path.
/// Original: ha_rocksdb.cc:367 — `rdb_normalize_dir`.
pub fn normalize_dir(mut dir: String) -> String {
    while dir.ends_with('/') { dir.pop(); }
    dir
}

/// Convert MySQL on-disk path "./dbname/tablename" → "dbname.tablename".
/// Returns `Err(invalid)` if `tablename` doesn't start with `./` or is
/// otherwise malformed (matches C++ `HA_ERR_ROCKSDB_INVALID_TABLE`).
/// Original: ha_rocksdb.cc:7584.
pub fn normalize_tablename(tablename: &str) -> Result<String, Error> {
    let bytes = tablename.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'.' || (bytes[1] != b'/' && bytes[1] != b'\\') {
        return Err(Error::invalid(format!("malformed table path: '{tablename}'")));
    }
    let rest = &tablename[2..];
    let pos = rest
        .find('/')
        .or_else(|| rest.find('\\'))
        .ok_or_else(|| Error::invalid(format!("missing separator in table path: '{tablename}'")))?;
    Ok(format!("{}.{}", &rest[..pos], &rest[pos + 1..]))
}

/// Parsed normalized table name: `<db>.<table>[#P#<partition>]`.
#[derive(Debug, Clone, Default)]
pub struct SplitName {
    pub db: String,
    pub table: String,
    pub partition: Option<String>,
}

/// Split a `db.tbl[#P#part]` into components. Returns `Err(invalid)` if no
/// `.` separator is present.
/// Original: ha_rocksdb.cc:7667 — `rdb_split_normalized_tablename`.
pub fn split_normalized_tablename(fullname: &str) -> Result<SplitName, Error> {
    let dotpos = fullname
        .find('.')
        .ok_or_else(|| Error::invalid(format!("missing '.' in table name: '{fullname}'")))?;
    let db = fullname[..dotpos].to_owned();
    let rest = &fullname[dotpos + 1..];
    let (table, partition) = match rest.find("#P#") {
        Some(p) => (rest[..p].to_owned(), Some(rest[p + 3..].to_owned())),
        None    => (rest.to_owned(), None),
    };
    Ok(SplitName { db, table, partition })
}

// --- XA XID packing ---

const RDB_FORMATID_SZ: usize = 8;
const RDB_GTRID_SZ: usize = 1;
const RDB_BQUAL_SZ: usize = 1;
const RDB_XIDHDR_LEN: usize = RDB_FORMATID_SZ + RDB_GTRID_SZ + RDB_BQUAL_SZ;

/// XID broken out of MariaDB's `XID` struct (we deliberately don't depend
/// on MariaDB types in the Rust crate).
#[derive(Debug, Clone)]
pub struct Xid {
    pub format_id: i64,
    pub gtrid: Vec<u8>,
    pub bqual: Vec<u8>,
}

/// Pack `Xid` into bytes for use as a SlateDB key suffix.
/// Format: `u64_be(format_id) || u8(gtrid_len) || u8(bqual_len) || gtrid || bqual`.
/// Original: ha_rocksdb.cc:3827 — `rdb_xid_to_string`.
pub fn xid_to_bytes(xid: &Xid) -> Bytes {
    let mut out = Vec::with_capacity(RDB_XIDHDR_LEN + xid.gtrid.len() + xid.bqual.len());
    // re-interpret format_id as u64 (matches the C++ which copies the bit pattern)
    let raw_fid8: u64 = xid.format_id as u64;
    out.extend_from_slice(&raw_fid8.to_be_bytes());
    out.push(xid.gtrid.len() as u8);
    out.push(xid.bqual.len() as u8);
    out.extend_from_slice(&xid.gtrid);
    out.extend_from_slice(&xid.bqual);
    Bytes::from(out)
}

/// Inverse of `xid_to_bytes`. Returns `Err(invalid)` on truncated input or
/// on `gtrid_len > MAXGTRIDSIZE (64)` / `bqual_len > MAXBQUALSIZE (64)`.
/// Original: ha_rocksdb.cc:4027 — `rdb_xid_from_string`.
pub fn xid_from_bytes(src: &[u8]) -> Result<Xid, Error> {
    if src.len() < RDB_XIDHDR_LEN {
        return Err(Error::invalid("xid: header truncated".into()));
    }
    let raw_fid8 = u64::from_be_bytes(src[..RDB_FORMATID_SZ].try_into().unwrap());
    let format_id = raw_fid8 as i64;
    let gtrid_len = src[RDB_FORMATID_SZ] as usize;
    let bqual_len = src[RDB_FORMATID_SZ + 1] as usize;
    if gtrid_len > 64 || bqual_len > 64 {
        return Err(Error::invalid(format!("xid: gtrid/bqual len out of range: {gtrid_len}/{bqual_len}")));
    }
    let need = RDB_XIDHDR_LEN + gtrid_len + bqual_len;
    if src.len() < need {
        return Err(Error::invalid("xid: body truncated".into()));
    }
    let gtrid = src[RDB_XIDHDR_LEN..RDB_XIDHDR_LEN + gtrid_len].to_vec();
    let bqual = src[RDB_XIDHDR_LEN + gtrid_len..need].to_vec();
    Ok(Xid { format_id, gtrid, bqual })
}

/// Find a token in a string ignoring case (used by `contains_foreign_key`).
/// We keep this in case ddl.rs needs it; otherwise consider it a leaf util.
pub fn find_in_string_case_insensitive(_haystack: &str, _needle: &str) -> Option<usize> {
    todo!("ascii-case-insensitive substring search, mirrors rdb_find_in_string in rdb_utils")
}
