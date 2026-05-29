//! Interface stub for `ha_rocksdb_cc____free__dbug_helpers`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 5948..5971 + 6485..6512 + 8942..8952, ~50 LoC).
//! v4 manifest sub-unit: `ha_rocksdb_cc____free__dbug_helpers`
//!
//! ## Mapping
//! `#ifndef DBUG_OFF` test-instrumentation helpers used by `DBUG_EXECUTE_IF`
//! to simulate failures during MTR test runs:
//!
//!   - `dbug_change_status_to_corrupted` — overwrite a `rocksdb::Status` with
//!     `Status::Corruption()` to test the corruption-handling path.
//!   - `dbug_append_garbage_at_end` / `dbug_truncate_record` /
//!     `dbug_modify_rec_varchar12` — mutate a pinned row buffer in flight to
//!     simulate corrupt-on-read scenarios.
//!   - `dbug_create_err_inplace_alter` — fail an in-place ALTER on demand.
//!   - `dbug_dump_str` — debug print of a non-printable byte buffer.
//!
//! These are debug-only and small; we keep one-to-one Rust analogues guarded
//! by `cfg(any(test, feature = "dbug"))`. The MTR triggers (`DBUG_EXECUTE_IF`)
//! become small helper functions invoked from the production code paths
//! behind the same feature flag.
//!
//! Per _DESIGN.md §13 (Stage 0 substrate-test implications) — these are
//! Stage 1 MTR support; safe to leave as `todo!()` in the stub.
//!
//! ## Out-of-scope methods
//! - The `DBUG_EXECUTE_IF("foo", action)` macro itself — Rust uses
//!   `if cfg!(feature = "dbug") && some_global_switch.contains("foo") { action }`.
//!   The harness is built in TRANSLATE; this stub just declares the
//!   simulated-failure helpers.

#![cfg_attr(not(any(test, feature = "dbug")), allow(dead_code))]

use bytes::{Bytes, BytesMut};
use slatedb::Error;

/// Replace `status` with a "corruption detected" Error. Used by tests to
/// force the iterator-error code path.
///
/// Per _DESIGN.md §4: `slatedb::ErrorKind::Data` is the corruption variant.
///
/// Original: ha_rocksdb.cc:5948.
pub fn change_status_to_corrupted() -> Error {
    Error::data("dbug: simulated corruption".into())
}

/// Append "abc" to the end of a fetched row. Caller swaps in the result.
/// Original: ha_rocksdb.cc:6485 — `dbug_append_garbage_at_end`.
pub fn append_garbage_at_end(record: Bytes) -> Bytes {
    let mut buf = BytesMut::from(&record[..]);
    buf.extend_from_slice(b"abc");
    buf.freeze()
}

/// Truncate the entire record to zero length.
/// Original: ha_rocksdb.cc:6492 — `dbug_truncate_record`.
pub fn truncate_record(_record: Bytes) -> Bytes { Bytes::new() }

/// Replace the value with `\0\x0C123456789ab` (a 12-byte VARCHAR(10) that
/// will fail length validation on read-back). Original: ha_rocksdb.cc:6496.
pub fn modify_rec_varchar12() -> Bytes {
    let mut buf = BytesMut::with_capacity(14);
    buf.extend_from_slice(b"\0");
    buf.extend_from_slice(b"\x0C");
    buf.extend_from_slice(b"123456789ab");
    buf.freeze()
}

/// Emit an "Intentional failure in inplace alter occurred." error. The error
/// surfaces through the `my_error(ER_UNKNOWN_ERROR, …)` path on the C++
/// side; on the Rust side we return an `Error::invalid` and let the shim
/// map it.
/// Original: ha_rocksdb.cc:6509 — `dbug_create_err_inplace_alter`.
pub fn create_err_inplace_alter() -> Error {
    Error::invalid("Intentional failure in inplace alter occurred.".into())
}

/// Print a byte buffer with escapes for non-printable bytes. Used by various
/// debug-dump helpers (e.g. `dbug_dump_database`). Returns the rendered
/// string so we can either log it (preferred) or feed it to stdout from C++.
/// Original: ha_rocksdb.cc:8942 — `dbug_dump_str`.
pub fn dump_str(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    for &b in bytes {
        if b > 32 { out.push(b as char); } else { out.push_str(&format!("\\{}", b)); }
    }
    out.push('"');
    out
}

/// Dummy item-print buffer hook — the C++ code uses a 512-byte static buffer
/// at ha_rocksdb.cc:12423. The Rust equivalent is a thread-local
/// `RefCell<String>` to avoid allocating per-call. Stub: just returns a
/// fresh `String`.
pub fn item_print_buf() -> String { String::with_capacity(512) }
