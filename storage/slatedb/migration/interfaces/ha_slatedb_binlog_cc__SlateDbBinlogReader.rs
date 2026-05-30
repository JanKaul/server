//! Interface stub for `ha_slatedb_binlog_cc__SlateDbBinlogReader` (NEW unit).
//!
//! C++ source: synthesised — the concrete impl that the InnoDB-side
//! reference implements as `ha_innodb_binlog_reader`
//! (`storage/innobase/handler/innodb_binlog.cc:266..314`).
//!
//! v4 manifest sub-unit: NEW — added 2026-05-29 for the SlateDB-as-binlog
//! interface (`_DESIGN.md §14`).
//!
//! ## Mapping
//!
//! Concrete implementation of the `BinlogReader` trait declared in
//! `ha_slatedb_binlog_h__reader.rs`. Backed by a SlateDB `DbIterator`
//! positioned over `binlog:<file_no>:<offset>` keys.
//!
//! State held:
//! - `db: Arc<Db>` — shared with the data engine.
//! - `wait_durable: bool` — gates visibility horizon.
//! - `cur_file_no` / `cur_file_pos` — exposed via trait accessors;
//!   server polls these to compute "earliest in-use file" for purge
//!   protection.
//! - `iter: Option<DbIterator>` — populated by `init_gtid_pos` /
//!   `init_legacy_pos`.
//! - `single_file: bool` — set by `enable_single_file`; reader returns
//!   EOF at file boundary instead of continuing.
//! - `partial_chunk: Option<Bytes>` — leftovers from the previous
//!   `read_binlog_data` call (chunk was bigger than caller buffer).
//! - `durable_watch: Option<watch::Receiver<DbStatus>>` — `Some(_)`
//!   when `wait_durable=true`, drives `wait_available`.
//!
//! ## Out-of-scope methods
//! None — full BinlogReader impl scoped here.

use crate::ha_slatedb_binlog_h__reader::{BinlogReader, GtidInitResult};
use crate::ha_slatedb_binlog_h__types::{SlaveConnectionState, ThdRef};
use bytes::Bytes;
use slatedb::Error;
use std::marker::PhantomData;
use std::sync::Arc;

/// Concrete reader. One per dump thread / `SHOW BINLOG EVENTS` call.
///
/// **Iterator-holding strategy is unresolved at the INTERFACE phase.**
/// SlateDB's `DbIterator` borrows from its owning snapshot/Db; we cannot
/// hold one directly inside a non-self-referential struct. The TRANSLATE
/// phase picks one of:
///   - hold `Arc<DbSnapshot>` and open a fresh iterator per `read_binlog_data`,
///   - hold `Box<dyn Stream<Item = KeyValue> + Send>` boxed into a fixed
///     lifetime via the snapshot Arc held alongside, or
///   - introduce an internal `Pin<Box<...>>` self-referential wrapper.
/// The `_iter_marker` field is a placeholder so the struct shape is
/// recognisable; the real field will be added in TRANSLATE.
pub struct SlateDbBinlogReader {
    pub db: Arc<slatedb::Db>,
    pub wait_durable: bool,

    /// Current cursor; exposed via trait accessors.
    pub cur_file_no: u64,
    pub cur_file_pos: u64,

    /// Placeholder for the iterator-holding strategy; see struct doc.
    pub _iter_marker: PhantomData<()>,

    /// `enable_single_file()` flag.
    pub single_file: bool,

    /// Carry-over from the previous `read_binlog_data` call when the
    /// caller's buffer was smaller than the next chunk.
    pub partial_chunk: Option<Bytes>,

    /// Subscribed to `DbMetadataOps::subscribe()` when `wait_durable`.
    pub durable_watch: Option<tokio::sync::watch::Receiver<slatedb::DbStatus>>,
}

impl SlateDbBinlogReader {
    pub fn new(db: Arc<slatedb::Db>, wait_durable: bool) -> Self {
        Self {
            db,
            wait_durable,
            cur_file_no: u64::MAX,
            cur_file_pos: u64::MAX,
            _iter_marker: PhantomData,
            single_file: false,
            partial_chunk: None,
            durable_watch: None,
        }
    }

    /// Common chunk-fetch helper used by `read_binlog_data` and
    /// `data_available`. Returns the next chunk's value bytes, or
    /// `None` if no more data is currently visible.
    async fn next_chunk(&mut self) -> Result<Option<Bytes>, Error> {
        todo!(
            "1. Open or reuse the iter (per the iterator-holding strategy\n\
                chosen at TRANSLATE — see struct doc).\n\
             2. iter.next().await:\n\
                  - Some(kv): respect wait_durable horizon (skip kv.seq > durable_seq).\n\
                  - None: return Ok(None) — caller decides to wait or EOF.\n\
             3. Parse BinlogKey from key; update cur_file_no/cur_file_pos.\n\
             4. Return Ok(Some(kv.value))."
        )
    }
}

#[async_trait::async_trait]
impl BinlogReader for SlateDbBinlogReader {
    fn cur_file_no(&self) -> u64 {
        self.cur_file_no
    }

    fn cur_file_pos(&self) -> u64 {
        self.cur_file_pos
    }

    async fn read_binlog_data(&mut self, _buf: &mut [u8]) -> Result<usize, Error> {
        todo!(
            "1. If partial_chunk: serve from it first; update partial_chunk if leftover.\n\
             2. Else: chunk = next_chunk().await?; if None return 0 (EOF).\n\
             3. Copy min(buf.len(), chunk.len()) bytes; stash remainder in partial_chunk.\n\
             4. Return bytes-copied."
        )
    }

    fn data_available(&self) -> bool {
        // Cheap synchronous check — no I/O. If partial_chunk is Some we
        // definitely have data; the buffered-iterator check is added in
        // TRANSLATE once the iter-holding strategy is decided.
        todo!("self.partial_chunk.is_some() || <iter has buffered KV?>")
    }

    async fn wait_available(
        &mut self,
        _thd: ThdRef,
        _deadline: std::time::Instant,
    ) -> Result<bool, Error> {
        todo!(
            "1. If wait_durable: select! on durable_watch.changed(), sleep_until(deadline),\n\
                AND a kill-check on _thd.killed.\n\
             2. Else: select! on the engine's commit notification channel, deadline, kill.\n\
             3. Return true on timeout, false on data or kill."
        )
    }

    async fn init_gtid_pos(
        &mut self,
        _thd: ThdRef,
        _pos: &SlaveConnectionState,
    ) -> Result<GtidInitResult, Error> {
        todo!(
            "1. Scan binlog:*:* for ChunkType::GtidState chunks (cheap because\n\
                states are written periodically + on file boundaries).\n\
                Honor _thd.killed in the scan loop.\n\
             2. Find the latest snapshot ≤ requested pos; if not found, Purged.\n\
             3. Replay GTID chunks from snapshot to find exact start position.\n\
             4. Open the internal iter at (file_no, offset); return Found{state}."
        )
    }

    async fn init_legacy_pos(
        &mut self,
        _thd: ThdRef,
        _filename: &str,
        _offset: u64,
    ) -> Result<(), Error> {
        todo!(
            "1. Parse 'slatedb-bin.NNNNNN' filename → file_no.\n\
             2. Open the internal iter at binlog:<file_no>:<offset>.\n\
             3. Update cur_file_no / cur_file_pos.\n\
             4. Honor _thd.killed."
        )
    }

    fn enable_single_file(&mut self) {
        self.single_file = true;
    }
}
