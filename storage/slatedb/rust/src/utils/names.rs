//! Path / table-name parsing helpers and XID byte packing.
//!
//! Translated from the `*_helpers` clusters in `ha_rocksdb.cc`. All routines
//! are pure data manipulation — no SlateDB API involvement — and preserve
//! bit-for-bit format with MyRocks so we can read its on-disk metadata.

use bytes::Bytes;
use slatedb::Error;

/// Strip trailing `/` characters from a directory path.
pub fn normalize_dir(mut dir: String) -> String {
    while dir.ends_with('/') {
        dir.pop();
    }
    dir
}

/// Convert MySQL on-disk path `./dbname/tablename` → `dbname.tablename`.
pub fn normalize_tablename(tablename: &str) -> Result<String, Error> {
    let bytes = tablename.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'.' || (bytes[1] != b'/' && bytes[1] != b'\\') {
        return Err(Error::invalid(format!(
            "malformed table path: '{tablename}'"
        )));
    }
    let rest = &tablename[2..];
    let pos = rest
        .find('/')
        .or_else(|| rest.find('\\'))
        .ok_or_else(|| {
            Error::invalid(format!("missing separator in table path: '{tablename}'"))
        })?;
    Ok(format!("{}.{}", &rest[..pos], &rest[pos + 1..]))
}

#[derive(Debug, Clone, Default)]
pub struct SplitName {
    pub db: String,
    pub table: String,
    pub partition: Option<String>,
}

/// Split a `db.tbl[#P#part]` into components.
pub fn split_normalized_tablename(fullname: &str) -> Result<SplitName, Error> {
    let dotpos = fullname
        .find('.')
        .ok_or_else(|| Error::invalid(format!("missing '.' in table name: '{fullname}'")))?;
    let db = fullname[..dotpos].to_owned();
    let rest = &fullname[dotpos + 1..];
    let (table, partition) = match rest.find("#P#") {
        Some(p) => (rest[..p].to_owned(), Some(rest[p + 3..].to_owned())),
        None => (rest.to_owned(), None),
    };
    Ok(SplitName {
        db,
        table,
        partition,
    })
}

// --- XA XID packing ---

const RDB_FORMATID_SZ: usize = 8;
const RDB_GTRID_SZ: usize = 1;
const RDB_BQUAL_SZ: usize = 1;
const RDB_XIDHDR_LEN: usize = RDB_FORMATID_SZ + RDB_GTRID_SZ + RDB_BQUAL_SZ;

/// MariaDB caps both gtrid and bqual at 64 bytes (`XIDDATASIZE / 2`).
const MAX_XID_PART_LEN: usize = 64;

#[derive(Debug, Clone)]
pub struct Xid {
    pub format_id: i64,
    pub gtrid: Vec<u8>,
    pub bqual: Vec<u8>,
}

/// Pack `Xid` into bytes for use as a SlateDB key suffix.
/// Format: `u64_be(format_id) || u8(gtrid_len) || u8(bqual_len) || gtrid || bqual`.
pub fn xid_to_bytes(xid: &Xid) -> Bytes {
    let mut out = Vec::with_capacity(RDB_XIDHDR_LEN + xid.gtrid.len() + xid.bqual.len());
    let raw_fid8: u64 = xid.format_id as u64;
    out.extend_from_slice(&raw_fid8.to_be_bytes());
    out.push(xid.gtrid.len() as u8);
    out.push(xid.bqual.len() as u8);
    out.extend_from_slice(&xid.gtrid);
    out.extend_from_slice(&xid.bqual);
    Bytes::from(out)
}

/// Inverse of `xid_to_bytes`.
pub fn xid_from_bytes(src: &[u8]) -> Result<Xid, Error> {
    if src.len() < RDB_XIDHDR_LEN {
        return Err(Error::invalid("xid: header truncated".into()));
    }
    let raw_fid8 = u64::from_be_bytes(
        src[..RDB_FORMATID_SZ]
            .try_into()
            .map_err(|_| Error::invalid("xid: format_id slice not 8 bytes".into()))?,
    );
    let format_id = raw_fid8 as i64;
    let gtrid_len = src[RDB_FORMATID_SZ] as usize;
    let bqual_len = src[RDB_FORMATID_SZ + 1] as usize;
    if gtrid_len > MAX_XID_PART_LEN || bqual_len > MAX_XID_PART_LEN {
        return Err(Error::invalid(format!(
            "xid: gtrid/bqual len out of range: {gtrid_len}/{bqual_len}"
        )));
    }
    let need = RDB_XIDHDR_LEN + gtrid_len + bqual_len;
    if src.len() < need {
        return Err(Error::invalid("xid: body truncated".into()));
    }
    let gtrid = src[RDB_XIDHDR_LEN..RDB_XIDHDR_LEN + gtrid_len].to_vec();
    let bqual = src[RDB_XIDHDR_LEN + gtrid_len..need].to_vec();
    Ok(Xid {
        format_id,
        gtrid,
        bqual,
    })
}

/// ASCII-case-insensitive substring search. Mirrors `rdb_find_in_string` from
/// `rdb_utils`. Returns the byte offset of `needle` within `haystack`, or
/// `None` if not present. Both arguments are interpreted as ASCII for the
/// purpose of folding case (matches MyRocks, which uses `strcasestr`).
pub fn find_in_string_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.len() > h.len() {
        return None;
    }
    'outer: for start in 0..=h.len() - n.len() {
        for i in 0..n.len() {
            if h[start + i].eq_ignore_ascii_case(&n[i]) {
                continue;
            }
            continue 'outer;
        }
        return Some(start);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_dir_strips_trailing_slashes() {
        assert_eq!(normalize_dir("/var/lib/mysql///".to_string()), "/var/lib/mysql");
        assert_eq!(normalize_dir("".to_string()), "");
        assert_eq!(normalize_dir("a".to_string()), "a");
    }

    #[test]
    fn normalize_tablename_round_trip() {
        assert_eq!(
            normalize_tablename("./testdb/users").expect("parse"),
            "testdb.users"
        );
        assert!(normalize_tablename("testdb/users").is_err());
        assert!(normalize_tablename("./testdb_no_slash").is_err());
    }

    #[test]
    fn split_with_and_without_partition() {
        let s = split_normalized_tablename("db.tbl").expect("parse");
        assert_eq!(s.db, "db");
        assert_eq!(s.table, "tbl");
        assert!(s.partition.is_none());

        let s = split_normalized_tablename("db.tbl#P#p0").expect("parse");
        assert_eq!(s.db, "db");
        assert_eq!(s.table, "tbl");
        assert_eq!(s.partition.as_deref(), Some("p0"));

        assert!(split_normalized_tablename("nodot").is_err());
    }

    #[test]
    fn xid_pack_unpack_round_trip() {
        let xid = Xid {
            format_id: 1234,
            gtrid: b"GTRID-bytes".to_vec(),
            bqual: b"BQUAL!!".to_vec(),
        };
        let packed = xid_to_bytes(&xid);
        let parsed = xid_from_bytes(&packed).expect("parse");
        assert_eq!(parsed.format_id, xid.format_id);
        assert_eq!(parsed.gtrid, xid.gtrid);
        assert_eq!(parsed.bqual, xid.bqual);
    }

    #[test]
    fn xid_rejects_oversize_lengths() {
        let mut bytes = vec![0u8; RDB_XIDHDR_LEN];
        bytes[RDB_FORMATID_SZ] = 65;
        assert!(xid_from_bytes(&bytes).is_err());
    }

    #[test]
    fn case_insensitive_search() {
        assert_eq!(find_in_string_case_insensitive("ABCdef", "cd"), Some(2));
        assert_eq!(find_in_string_case_insensitive("ABCdef", "XYZ"), None);
        assert_eq!(find_in_string_case_insensitive("anything", ""), Some(0));
        assert_eq!(find_in_string_case_insensitive("", "x"), None);
    }
}
