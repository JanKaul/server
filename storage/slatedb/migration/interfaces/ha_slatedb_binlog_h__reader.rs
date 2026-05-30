//! Interface stub for `ha_slatedb_binlog_h__reader` (NEW unit).
//!
//! C++ source: `include/handler_binlog_reader.h` (lines 1..97)
//! v4 manifest sub-unit: NEW — added 2026-05-29 for the SlateDB-as-binlog
//! interface (`_DESIGN.md §14`).
//!
//! ## Mapping
//!
//! Mirror of the abstract C++ class `handler_binlog_reader`. The server
//! creates concrete reader objects polymorphically (one per dump thread
//! + one per `SHOW BINLOG EVENTS` invocation), so this is one of the
//! few genuine vtables in the API — we model it as a Rust trait.
//!
//! The factory (`get_binlog_reader`) and the concrete implementation
//! (`SlateDbBinlogReader`) live in sibling stubs; this file declares
//! the trait only.
//!
//! Per `_DESIGN.md §14`:
//! - `wait_durable=true` ⇒ reader returns data whose seqnum is
//!   ≤ `Db::status().last_durable_seq`. We subscribe to
//!   `DbMetadataOps::subscribe()` to advance this in real time.
//! - `wait_durable=false` ⇒ reader returns the latest writeable seq.
//!
//! ## Out-of-scope methods
//! - `read_log_event` (the C++ base class implements this on top of
//!   `read_binlog_data`). We keep it on the trait so the cxx shim sees
//!   the same vtable shape, but the default impl just calls
//!   `read_binlog_data` in a loop.

use crate::ha_slatedb_binlog_h__types::{
    RplBinlogState, SlaveConnectionState, ThdRef,
};
use bytes::BytesMut;
use slatedb::Error;

/// Outcome of `init_gtid_pos`. The full binlog state at the resolved
/// position is carried inside `Found` — there is no separate out-param.
#[derive(Debug)]
pub enum GtidInitResult {
    /// Position located and reader cursor placed.
    Found { file_no: u64, pos: u64, state: RplBinlogState },
    /// The needed binlogs have been purged. Caller treats as
    /// `ER_MASTER_FATAL_ERROR_READING_BINLOG`.
    Purged,
}

/// Reader trait. Subclassed by `SlateDbBinlogReader` in the sibling
/// stub file. Returned by `get_binlog_reader` as `Box<dyn BinlogReader>`.
///
/// Mirrors `class handler_binlog_reader` in
/// `include/handler_binlog_reader.h:29..95`.
#[async_trait::async_trait]
pub trait BinlogReader: Send {
    /// Current read position. Server reads these to know which file the
    /// dump thread is on so it can avoid purging an in-use file. Mirrors
    /// the `cur_file_no` / `cur_file_pos` public fields on the C++ base.
    fn cur_file_no(&self) -> u64;
    fn cur_file_pos(&self) -> u64;

    /// Read up to `buf.len()` bytes of binlog data. Returns the number
    /// of bytes written, or `0` for EOF.
    ///
    /// Mirrors `read_binlog_data(uchar *buf, uint32_t len) -> int`
    /// (`handler_binlog_reader.h:58`).
    async fn read_binlog_data(&mut self, buf: &mut [u8]) -> Result<usize, Error>;

    /// Whether the reader has data ready without blocking.
    /// Mirrors `data_available()` (`handler_binlog_reader.h:59`).
    fn data_available(&self) -> bool;

    /// Wait until data is available, the connection is killed, or the
    /// timeout fires. Returns `true` on timeout.
    ///
    /// `thd` is forwarded so the impl can check `thd.killed`. A stuck
    /// dump thread must be killable.
    ///
    /// Mirrors `wait_available(THD*, const timespec*)` (`handler_binlog_reader.h:65`).
    async fn wait_available(
        &mut self,
        thd: ThdRef,
        deadline: std::time::Instant,
    ) -> Result<bool, Error>;

    /// Seek to the slave's reported GTID position. The full binlog state
    /// at that position is returned inside `GtidInitResult::Found` — no
    /// out-param.
    ///
    /// `thd` is forwarded for kill-checking on long scans.
    ///
    /// Mirrors `init_gtid_pos(THD*, slave_connection_state*, rpl_binlog_state_base*)`
    /// (`handler_binlog_reader.h:80`).
    async fn init_gtid_pos(
        &mut self,
        thd: ThdRef,
        pos: &SlaveConnectionState,
    ) -> Result<GtidInitResult, Error>;

    /// Seek by legacy (filename, offset). Used for `SHOW BINLOG EVENTS`.
    /// `thd` is forwarded for kill-checking.
    ///
    /// Mirrors `init_legacy_pos(THD*, const char*, ulonglong)`
    /// (`handler_binlog_reader.h:86`).
    async fn init_legacy_pos(
        &mut self,
        thd: ThdRef,
        filename: &str,
        offset: u64,
    ) -> Result<(), Error>;

    /// Make the reader stop (EOF) at end of current file. For
    /// `SHOW BINLOG EVENTS` which has a per-file interface.
    /// Mirrors `enable_single_file()` (`handler_binlog_reader.h:93`).
    fn enable_single_file(&mut self);

    /// Default impl built on top of `read_binlog_data`. Mirrors
    /// `handler_binlog_reader::read_log_event` (a non-virtual helper on
    /// the C++ base class). Reads one complete log event into `packet`,
    /// up to `max_allowed` bytes.
    async fn read_log_event(
        &mut self,
        packet: &mut BytesMut,
        ev_offset: u32,
        max_allowed: usize,
    ) -> Result<usize, Error> {
        let _ = (packet, ev_offset, max_allowed);
        todo!(
            "loop: read_binlog_data into packet until one full event\n\
             is buffered or max_allowed exceeded; return bytes-read"
        )
    }
}
