//! `#ifndef DBUG_OFF` test-instrumentation helpers.
//!
//! Translated from `ha_rocksdb.cc:5948..5971 + 6485..6512 + 8942..8952`.
//! Production-only code paths invoke these behind a feature switch to
//! simulate corruption / failures for MTR test runs. Gated to test builds
//! and the `dbug` cargo feature to keep them out of release binaries.

#![cfg_attr(not(any(test, feature = "dbug")), allow(dead_code))]

use bytes::{Bytes, BytesMut};
use slatedb::Error;

/// "Corruption detected" sentinel — used by tests to force the iterator-error
/// code path. Per `_DESIGN.md §4`, corruption maps to `ErrorKind::Data`.
pub fn change_status_to_corrupted() -> Error {
    Error::data("dbug: simulated corruption".into())
}

/// Append `b"abc"` to the end of a fetched row.
pub fn append_garbage_at_end(record: Bytes) -> Bytes {
    let mut buf = BytesMut::from(&record[..]);
    buf.extend_from_slice(b"abc");
    buf.freeze()
}

/// Truncate the entire record to zero length.
pub fn truncate_record(_record: Bytes) -> Bytes {
    Bytes::new()
}

/// Replace the value with `\0\x0C123456789ab` (a 12-byte VARCHAR(10) that
/// fails length validation on read-back).
pub fn modify_rec_varchar12() -> Bytes {
    let mut buf = BytesMut::with_capacity(14);
    buf.extend_from_slice(b"\0");
    buf.extend_from_slice(b"\x0C");
    buf.extend_from_slice(b"123456789ab");
    buf.freeze()
}

/// Synthesize the "Intentional failure in inplace alter occurred." error.
pub fn create_err_inplace_alter() -> Error {
    Error::invalid("Intentional failure in inplace alter occurred.".into())
}

/// Print a byte buffer with escapes for non-printable bytes.
pub fn dump_str(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    for &b in bytes {
        if b > 32 {
            out.push(b as char);
        } else {
            out.push('\\');
            out.push_str(&b.to_string());
        }
    }
    out.push('"');
    out
}

/// Per-call print buffer for debug dumps. Mirrors the 512-byte static buffer
/// at `ha_rocksdb.cc:12423`.
pub fn item_print_buf() -> String {
    String::with_capacity(512)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_appender_adds_three_bytes() {
        let out = append_garbage_at_end(Bytes::from_static(b"row"));
        assert_eq!(&out[..], b"rowabc");
    }

    #[test]
    fn truncate_yields_empty() {
        let out = truncate_record(Bytes::from_static(b"row"));
        assert!(out.is_empty());
    }

    #[test]
    fn dump_escapes_low_bytes() {
        let s = dump_str(b"a\0b");
        assert_eq!(s, "\"a\\0b\"");
    }
}
