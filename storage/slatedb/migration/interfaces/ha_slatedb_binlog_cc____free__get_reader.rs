//! Interface stub for `ha_slatedb_binlog_cc____free__get_reader` (NEW unit).
//!
//! C++ source: `sql/handler.h` slot `get_binlog_reader` (line 1706).
//! Coordinator call sites: `sql/sql_repl.cc:2591` (dump thread —
//! `wait_durable=true`), `:4940` (`SHOW BINLOG EVENTS` —
//! `wait_durable=false`).
//!
//! v4 manifest sub-unit: NEW — added 2026-05-29 for the SlateDB-as-binlog
//! interface (`_DESIGN.md §14`).
//!
//! ## Mapping
//!
//! Factory function: allocates one `SlateDbBinlogReader`
//! (sibling stub) per caller. Reader lifetime is owned by the dump
//! thread / SQL command; we return `Box<dyn BinlogReader>` so the cxx
//! shim can hold it as an opaque pointer.
//!
//! `wait_durable=true` is the crash-safety contract: the reader must
//! not emit data past `db.last_durable_seq()`. We thread the flag into
//! the reader's construction; the reader uses it to pick a snapshot
//! seqnum at every `read_binlog_data` call.
//!
//! ## Out-of-scope methods
//! None — single-slot file.

use crate::ha_slatedb_binlog_cc__SlateDbBinlogReader::SlateDbBinlogReader;
use crate::ha_slatedb_binlog_h__reader::BinlogReader;

/// Coordinator entry point: factory. Sync (no I/O — reader does the
/// real work in its `init_*_pos` methods).
///
/// Original: `sql/handler.h:1706` —
/// `handler_binlog_reader * (*get_binlog_reader)(bool wait_durable);`
///
/// Returns `Box<dyn BinlogReader>` (not `Option`) — allocation failure
/// is fatal; we panic the runtime (matches the C++ which doesn't check
/// for null on the return path either).
pub fn get_binlog_reader(_wait_durable: bool) -> Box<dyn BinlogReader> {
    todo!(
        "Box::new(SlateDbBinlogReader::new(\n\
         \\tArc::clone(&shared_db_handle()),\n\
         \\twait_durable,\n\
         ))"
    )
}
