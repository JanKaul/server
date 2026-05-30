//! Interface stub for `ha_slatedb_binlog_h__types` (NEW unit).
//!
//! C++ source: synthesised — no direct C++ equivalent. Mirrors the
//! supporting structs in `sql/handler.h` lines 6027..6150 and the
//! on-disk format constants in `storage/innobase/include/fsp_binlog.h`
//! lines 50..170. This file is the single source of truth for every
//! type that crosses two or more sibling stubs in the
//! `ha_slatedb_binlog_*` family.
//!
//! v4 manifest sub-unit: NEW — added 2026-05-29 for the
//! SlateDB-as-binlog interface (`_DESIGN.md §14`).
//!
//! ## Mapping
//!
//! Per `_DESIGN.md §14` (binlog-engine role): the binlog lives as a
//! key prefix in the same SlateDB `Db` instance that hosts the data
//! engine. Every chunk is one KV pair:
//!
//! ```text
//!   key   = b"binlog:" || file_no_be8 || b":" || offset_be8
//!   value = chunk_header (3 bytes) || payload
//! ```
//!
//! `chunk_header` is laid out exactly like InnoDB's
//! (`fsp_binlog.h:66..86`):
//! `chunk_type | CONT_flag | LAST_flag` (byte 0); little-endian u16
//! `payload_len` (bytes 1..3). Preserved bit-for-bit so a future tool
//! could read both InnoDB and SlateDB binlogs with the same parser.
//!
//! ## Out-of-scope methods
//! None — this file is pure data declarations.

use bytes::Bytes;

// ---------------------------------------------------------------------------
// Chunk format constants (InnoDB-compatible)
// ---------------------------------------------------------------------------

/// Chunk type, stored as the first byte of every chunk value.
/// Mirrors `fsp_binlog_chunk_types` in `storage/innobase/include/fsp_binlog.h:66`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkType {
    /// Padding to end-of-page; reader skips.
    Filler     = 0xff,
    /// Normal commit event group.
    Commit     = 1,
    /// GTID state snapshot. Written periodically + at every file boundary.
    GtidState  = 2,
    /// Out-of-band event-group data (pre-commit spill).
    OobData    = 3,
    /// Heartbeat / no-op (e.g. emitted by `binlog_flush` on rotation).
    Dummy      = 4,
    /// User-XA prepared event group (Stage 2, currently stubbed).
    XaPrepare  = 5,
    /// User-XA commit or rollback completion record (Stage 2).
    XaComplete = 6,
}

/// Bit 7 of `chunk_header[0]`. Set when the chunk continues onto the
/// next key (multi-chunk record split across pages, mirroring the
/// InnoDB convention).
pub const CHUNK_FLAG_CONT: u8 = 0x80;
/// Bit 6 of `chunk_header[0]`. Set on the final chunk of a record.
pub const CHUNK_FLAG_LAST: u8 = 0x40;

/// Parsed 3-byte chunk header.
#[derive(Debug, Clone, Copy)]
pub struct BinlogChunkHeader {
    pub chunk_type: ChunkType,
    pub cont: bool,
    pub last: bool,
    pub payload_len: u16,
}

// ---------------------------------------------------------------------------
// Key encoding
// ---------------------------------------------------------------------------

/// Parsed form of a `binlog:<file_no>:<offset>` key. Encode/decode via
/// `to_bytes` / `from_bytes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BinlogKey {
    pub file_no: u64,
    pub offset: u64,
}

impl BinlogKey {
    /// Fixed-length encoded form (preserves sort order with bytewise compare).
    pub fn to_bytes(&self) -> Bytes {
        todo!("b\"binlog:\" || file_no.to_be_bytes() || b\":\" || offset.to_be_bytes()")
    }
    pub fn from_bytes(_bytes: &[u8]) -> Option<Self> {
        todo!("parse 23-byte fixed-len key; None on mismatch")
    }
}

/// Metadata key namespace (separate prefix so a scan over `binlog:`
/// never returns metadata rows).
pub const META_PREFIX: &[u8] = b"binlog_meta:";

// ---------------------------------------------------------------------------
// Cross-cutting handler types (Rust mirrors of C++ structs)
// ---------------------------------------------------------------------------

/// Rust mirror of `handler_binlog_event_group_info` (`sql/handler.h:6027..6077`).
/// One per cache; threaded through every event-group write call.
///
/// **`engine_ptr` ↔ `engine_data` parameter routing.** In C++ the struct
/// field `void *engine_ptr` is a *single* pointer (`handler.h:6036`). On
/// every OOB / XA call, the coordinator passes `&info->engine_ptr` as the
/// `void **engine_data` argument — i.e. the address of THIS field is what
/// the slot receives as its pointer-to-pointer. The cxx bridge handles
/// that indirection: the Rust slot signatures take
/// `&mut Option<Box<EngineDataPtr>>`, which is the bridge-side view after
/// the `*mut *mut` is collapsed.
#[derive(Debug)]
pub struct BinlogEventGroupInfo {
    /// Set by us in `*_ordered`; read by the coordinator for SHOW MASTER STATUS.
    pub out_file_no: u64,
    pub out_offset: u64,
    /// Opaque per-cache state. Heap-allocated `Box<EngineDataPtr>` on first
    /// OOB write; freed by `binlog_oob_free`. See the struct doc above for
    /// how the cxx bridge routes this as `void **engine_data`.
    pub engine_ptr: *mut EngineDataPtr,
    /// Secondary context — set only when statement + transactional caches
    /// are combined. Logically precedes `engine_ptr`.
    pub engine_ptr2: *mut EngineDataPtr,
    /// User-XA: non-NULL during XA PREPARE / XA COMMIT. Stage 2.
    pub xa_xid: Option<XidBytes>,
    /// End offset of already-binlogged OOB data; everything past is inline.
    pub out_of_band_offset: u64,
    /// Offset of the GTID event within the in-cache IO_CACHE buffer
    /// (the GTID is logically first but lives at the cache tail).
    pub gtid_offset: u64,
    /// `true` for internal 2PC (data engine + binlog engine), `false` for
    /// user-XA. Affects the recovery decision: internal_xa transactions
    /// resolve via `engine_count` cross-reference; user-XA via the
    /// pending-XID HASH that `binlog_init` populates.
    pub internal_xa: bool,
}

/// SAFETY: the raw pointers are managed by the cxx bridge across an
/// FFI boundary; the engine never dereferences them directly.
unsafe impl Send for BinlogEventGroupInfo {}

/// Opaque heap allocation behind `void **engine_data`. Holds the
/// per-transaction OOB tracking state.
#[derive(Debug, Default)]
pub struct EngineDataPtr {
    /// First file_no this txn's OOB chunks landed in.
    pub start_file_no: u64,
    /// Highest (file_no, offset) used so far.
    pub current_file_no: u64,
    pub current_offset: u64,
    /// Savepoint cursor: offset within the OOB stream to roll back to,
    /// if `binlog_savepoint_rollback(stmt_start_data)` fires.
    pub stmt_start_offset: Option<u64>,
    /// Savepoint cursor for named SAVEPOINTs (one-deep — MariaDB stacks
    /// them at the SQL layer, but the engine sees one at a time).
    pub savepoint_offset: Option<u64>,
}

/// One savepoint handle returned via `stmt_start_data` / `savepoint_data`
/// out-params. Opaque to the server. Structurally identical to
/// [`BinlogKey`] — we alias the type so an implementer doesn't have to
/// reconcile two near-identical structs.
pub type SavepointHandle = BinlogKey;

/// Opaque XID byte representation. We don't depend on MariaDB's `XID`
/// struct directly; the cxx shim packs it into `Bytes` for us.
///
/// Used as a `HashMap` key (via [`XidRecoveryHash`]). `Bytes` implements
/// `Hash` by content (not pointer), so this is correct.
pub type XidBytes = Bytes;

// ---------------------------------------------------------------------------
// Recovery types
// ---------------------------------------------------------------------------

/// Rust mirror of `handler_binlog_xid_info` (`sql/handler.h:6119..6150`).
/// Engine subclasses this conceptually; we use it directly with the
/// per-XID state we read from the binlog scan.
#[derive(Debug, Clone)]
pub struct BinlogXidInfo {
    pub xid: XidBytes,
    pub engine_count: u32,
    pub engine_map: u32,
    pub state: XidState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XidState {
    Prepare,
    Commit,
    Rollback,
}

/// Hash returned from `binlog_init` and consumed by the server's
/// `ha_recover_engine_binlog`. Keyed by raw XID bytes (matching the
/// C++ HASH that uses `XID::key()`).
pub type XidRecoveryHash = std::collections::HashMap<XidBytes, BinlogXidInfo>;

// ---------------------------------------------------------------------------
// Admin types
// ---------------------------------------------------------------------------

/// Rust mirror of `binlog_file_entry` (`sql/handler.h:1257..1263`).
/// Server fills in `size`; we set `name`.
#[derive(Debug, Clone)]
pub struct BinlogFileEntry {
    pub file_no: u64,
    pub name: String,
}

/// Rust mirror of `handler_binlog_purge_info` (`sql/handler.h:6081..6108`).
#[derive(Debug, Default)]
pub struct BinlogPurgeInfo {
    pub limit_file_no: u64,
    pub limit_size: u64,
    pub limit_name: Option<String>,
    pub limit_date: i64,
    pub purge_by_date: bool,
    pub purge_by_size: bool,
    pub purge_by_name: bool,
    /// Out: engine sets if a requested purge could not proceed.
    pub nonpurge_reason: Option<String>,
    pub nonpurge_filename: Option<String>,
}

// ---------------------------------------------------------------------------
// GTID types (forwarded from server layer)
// ---------------------------------------------------------------------------

/// Single GTID — opaque to us; the codec lives in the server. We carry
/// the raw bytes through `binlog_write_direct(.., gtid: Option<&RplGtid>)`.
#[derive(Debug, Clone)]
pub struct RplGtid {
    pub domain_id: u32,
    pub server_id: u32,
    pub seq_no: u64,
}

/// Per-(domain, server) high-water seq_no map. Materialised on file
/// boundaries via inline `ChunkType::GtidState` chunks (see `_DESIGN.md §14`).
#[derive(Debug, Default, Clone)]
pub struct RplBinlogState {
    /// One entry per (domain_id, server_id) pair seen since binlog start.
    pub entries: Vec<RplGtid>,
}

/// Slave's reported position from `COM_BINLOG_DUMP_GTID`. Passed to
/// `BinlogReader::init_gtid_pos` to seek the read cursor.
#[derive(Debug, Clone)]
pub struct SlaveConnectionState {
    /// (domain_id, server_id) → seq_no the slave has already received.
    pub entries: Vec<RplGtid>,
}

// ---------------------------------------------------------------------------
// FFI carriers
// ---------------------------------------------------------------------------

/// Wraps the MariaDB `IO_CACHE*` that carries event-group data into the
/// engine. We only need to drain bytes from it; the cxx shim provides a
/// `read(buf)` helper.
#[derive(Debug)]
pub struct IoCacheRef {
    /// SAFETY: raw pointer to a server-allocated `IO_CACHE`; valid for
    /// the duration of the hton call only.
    pub raw: *mut std::ffi::c_void,
}

/// SAFETY: see the cxx bridge contract — the pointer is server-owned
/// and not aliased across threads while the hton call is on the stack.
unsafe impl Send for IoCacheRef {}

/// `THD *` opaque handle. Identical in shape to other stubs in the
/// project; re-declared here to keep the binlog unit import-free of
/// data-engine modules.
#[derive(Debug, Clone, Copy)]
pub struct ThdRef {
    pub raw: usize,
}
