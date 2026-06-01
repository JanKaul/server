//! MariaDB `handler` analog — per-table-open runtime state.
//!
//! Translated from the `ha_rocksdb` C++ class. MariaDB creates one
//! handler instance per (THD, opened table); methods on it implement
//! the storage-engine vtable that the SQL layer calls.
//!
//! ## Scope of this commit
//!
//! Only **`HaSlateDb::open`** and **`HaSlateDb::close`** from the
//! lifecycle bucket — the smallest meaningful slice. Both are
//! catalogue-only operations: bind / release a reference to the
//! `Arc<TblDef>` looked up from the global engine's
//! [`crate::engine::ddl_manager::DdlManager`]. No transactions, no
//! key buffers, no row I/O yet.
//!
//! ## What's NOT here (deferred)
//!
//! - `init_with_fields` — needs cached_table_flags + check_keyread_allowed
//! - `store_lock` — pure MariaDB THR_LOCK wiring; needs ThdRef plumbing
//! - `external_lock` — txn lifecycle (get_or_create_tx, commit-on-unlock);
//!   needs the per-THD txn registry surface, which lands with the
//!   transaction handlerton callbacks
//! - `extra(HaExtraFunction)` — capability toggles; lands when the read
//!   path needs the `HA_EXTRA_KEYREAD` / `HA_EXTRA_FLUSH` toggles
//! - Per-handler key buffers (`m_pk_packed_tuple`, `m_sk_packed_tuple`,
//!   `m_pk_unpack_info`); lands with `write_row` / `index_read`
//! - `update_row_stats` — lands with `GlobalStats` infrastructure
//!
//! ## Cxx surface
//!
//! Exposed via `[`crate::bridge`]:
//! - `new_ha_slatedb()` returns `Box<HaSlateDb>` (opaque on the C++ side)
//! - `ha_open(handler, name) -> i32` / `ha_close(handler) -> i32`
//!   with status codes from [`crate::bridge::status`] extended in
//!   [`status`].

use std::sync::Arc;

use slatedb::Error;

use crate::codec::tbl_def::TblDef;

/// Status codes specific to handler operations. Reuses the
/// [`crate::bridge::status`] codes for engine-not-installed; adds
/// table-lookup-specific codes here.
pub mod status {
    pub use crate::bridge::status::{
        ALREADY_INITIALISED, ENGINE_IO_FAILED, OK, RUNTIME_INIT_FAILED,
    };

    /// No engine is installed — caller must call `slatedb_init_in_memory`
    /// first.
    pub const NO_ENGINE: i32 = 10;
    /// `open` couldn't normalize the input path (not `./db/tbl` shape).
    pub const BAD_TABLE_PATH: i32 = 11;
    /// The catalogue has no entry for the requested table.
    pub const NO_SUCH_TABLE: i32 = 12;
}

/// MariaDB's `enum thr_lock_type` from `include/thr_lock.h`. Numeric
/// values are stable across versions — they're part of MariaDB's
/// internal ABI.
///
/// We translate this enum so [`HaSlateDb::store_lock`] can pattern-
/// match on the values the C++ side passes us. The cxx surface
/// marshals them as `i32` and the bridge maps to/from this enum.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrLockType {
    Ignore = -1,
    Unlock = 0,
    ReadDefault = 1,
    Read = 2,
    ReadHighPriority = 3,
    ReadNoInsert = 4,
    ReadWithSharedLocks = 5,
    WriteAllowWrite = 6,
    WriteConcurrentInsert = 7,
    WriteDelayed = 8,
    WriteDefault = 9,
    WriteLowPriority = 10,
    Write = 11,
    WriteOnly = 12,
}

impl ThrLockType {
    /// Map from the raw i32 the cxx bridge passes us. Unknown values
    /// (out of range for the enum) collapse to [`ThrLockType::Ignore`]
    /// — matches the C++'s `if (lock_type != TL_IGNORE)` gate, which
    /// effectively treats anything it doesn't recognise as "leave the
    /// decision alone".
    pub fn from_i32(v: i32) -> Self {
        match v {
            -1 => Self::Ignore,
            0 => Self::Unlock,
            1 => Self::ReadDefault,
            2 => Self::Read,
            3 => Self::ReadHighPriority,
            4 => Self::ReadNoInsert,
            5 => Self::ReadWithSharedLocks,
            6 => Self::WriteAllowWrite,
            7 => Self::WriteConcurrentInsert,
            8 => Self::WriteDelayed,
            9 => Self::WriteDefault,
            10 => Self::WriteLowPriority,
            11 => Self::Write,
            12 => Self::WriteOnly,
            _ => Self::Ignore,
        }
    }
}

/// MyRocks' internal row-lock mode parsed out of `store_lock` and
/// used by the rest of the handler. Mirrors C++ `enum {
/// RDB_LOCK_NONE, RDB_LOCK_READ, RDB_LOCK_WRITE }`. Per
/// `_DESIGN.md §5` this drives the SI vs SSI isolation choice and
/// whether scans tag the txn via `txn.mark_read(...)`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowLockMode {
    #[default]
    None = 0,
    Read = 1,
    Write = 2,
}

/// Subset of MariaDB's `enum ha_extra_function` from
/// `include/my_base.h:133`. Only the variants [`HaSlateDb::extra`]
/// acts on are listed here — the full enum has 50+ entries, most of
/// which collapse to a silent no-op (matching the C++'s
/// `default: break;` at `ha_rocksdb.cc:11968`).
///
/// Numeric values match MariaDB's `enum ha_extra_function` exactly
/// (part of the internal handler ABI). The bridge marshals the C++
/// value as `i32` and we map via [`HaExtraFunction::from_i32`].
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaExtraFunction {
    /// `HA_EXTRA_KEYREAD = 7` — next read only needs the keypart
    /// bytes, no row data. Drives the covered-read fast path.
    Keyread = 7,
    /// `HA_EXTRA_NO_KEYREAD = 8` — caller is done with the keyread
    /// fast path; reset.
    NoKeyread = 8,
    /// `HA_EXTRA_FLUSH = 22` — invalidate any per-handler cached row
    /// buffer. Does NOT trigger a SlateDB-side flush.
    Flush = 22,
    /// `HA_EXTRA_NO_IGNORE_DUP_KEY = 26` — end of the `INSERT … ON
    /// DUPLICATE KEY UPDATE` / `REPLACE` window.
    NoIgnoreDupKey = 26,
    /// `HA_EXTRA_INSERT_WITH_UPDATE = 41` — start of `INSERT … ON
    /// DUPLICATE KEY UPDATE`. Tells the handler to cache row lookups
    /// for the upcoming update.
    InsertWithUpdate = 41,
    /// Anything else (e.g. `HA_EXTRA_PREPARE_FOR_RENAME`,
    /// `HA_EXTRA_CACHE`, ...) — silent no-op in our handler. The
    /// `-1` sentinel doesn't collide with any real `ha_extra_function`
    /// value (the real enum starts at 0).
    Other = -1,
}

impl HaExtraFunction {
    /// Map from the raw C++ `enum ha_extra_function` `i32` to a Rust
    /// variant. Unknown values (which is most of them) collapse to
    /// [`HaExtraFunction::Other`], matching the C++ `default: break;`
    /// behaviour.
    pub fn from_i32(v: i32) -> Self {
        match v {
            7 => Self::Keyread,
            8 => Self::NoKeyread,
            22 => Self::Flush,
            26 => Self::NoIgnoreDupKey,
            41 => Self::InsertWithUpdate,
            _ => Self::Other,
        }
    }
}

/// MariaDB's `int lock_type` parameter to `external_lock`. Values
/// from `<sys/file.h>` (`F_RDLCK = 1`, `F_WRLCK = 2`, `F_UNLCK = 8`).
/// Pinned here so the bridge can pass the raw int and we map to
/// the typed enum.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalLockType {
    /// `F_RDLCK = 1` — table is being read.
    Read = 1,
    /// `F_WRLCK = 2` — table is being written.
    Write = 2,
    /// `F_UNLCK = 8` — table is being released. Maybe-commit point.
    Unlock = 8,
}

impl ExternalLockType {
    /// Map from the raw `lock_type` int. Unknown values return
    /// `None`; the caller treats that as an internal error (the
    /// SQL layer shouldn't be passing values outside the F_* set).
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            1 => Some(Self::Read),
            2 => Some(Self::Write),
            8 => Some(Self::Unlock),
            _ => None,
        }
    }
}

/// Subset of MariaDB `THD` state that [`HaSlateDb::store_lock`] needs.
/// Filled by the cxx side from a live `THD*` before calling in.
///
/// Currently carries only the two booleans that the simple decision
/// path consults. The `lock_scanned_rows` upgrade branch in the C++
/// (`ha_rocksdb.cc:11299..11323`) needs additional THD reads
/// (`thd_sql_command`, `thd_tx_isolation`, `thd_test_options`, sysvar
/// `lock_scanned_rows`); that path is deferred until we plumb those
/// sysvars across the cxx boundary.
#[derive(Debug, Clone, Copy, Default)]
pub struct StoreLockThd {
    /// True when MariaDB is inside an explicit `LOCK TABLES`.
    pub in_lock_tables: bool,
    /// True for tablespace DDL (DISCARD/IMPORT TABLESPACE).
    pub tablespace_op: bool,
}

/// Per-table-open handler state.
///
/// The C++ `ha_rocksdb` class accretes ~50 fields over its lifetime
/// (key buffers, transaction handle, lock mode, key-read mode, dup-PK
/// flag, row checksum buffer, etc.). We add fields only as the
/// corresponding handler-bucket methods land — keeps the type honest.
///
/// **Today** the only field is `tbl_def`: an `Option<Arc<TblDef>>` that
/// `open` binds and `close` clears.
pub struct HaSlateDb {
    /// `Some` once `open` succeeds, `None` after `close` (or before
    /// the first `open`). The Arc clones cheaply from
    /// [`crate::engine::ddl_manager::DdlManager`]'s in-memory
    /// catalogue.
    tbl_def: Option<Arc<TblDef>>,

    /// Per-statement row-lock decision, set by [`Self::store_lock`].
    /// Drives the SI vs SSI isolation choice elsewhere in the handler
    /// (per `_DESIGN.md §5`). Defaults to `None` between statements.
    lock_rows: RowLockMode,

    /// Current MariaDB-level lock type for this handler instance
    /// (mirrors the C++ `m_db_lock.type`). Tracked so `store_lock`'s
    /// THR_LOCK downgrade only triggers on the `Unlock → request`
    /// transition — matches the C++ `m_db_lock.type == TL_UNLOCK`
    /// gate at `ha_rocksdb.cc:11327`.
    db_lock_type: ThrLockType,

    /// Set by `HA_EXTRA_KEYREAD`, cleared by `HA_EXTRA_NO_KEYREAD`.
    /// When true, the next read should only fetch keypart bytes and
    /// skip the row payload. Drives the covered-read fast path.
    keyread_only: bool,

    /// Set by `HA_EXTRA_INSERT_WITH_UPDATE`, cleared by
    /// `HA_EXTRA_NO_IGNORE_DUP_KEY`. When true, the handler caches
    /// row lookups during `INSERT … ON DUPLICATE KEY UPDATE` to
    /// avoid the second read on the update step.
    insert_with_update: bool,

    /// Cached pull-bytes from the most recent point lookup. Cleared
    /// by `HA_EXTRA_FLUSH` because if the table has BLOB columns the
    /// caller may have started reading them out of this buffer and
    /// MariaDB needs to invalidate that handle. C++ counterpart is
    /// `m_retrieved_record` at `ha_rocksdb.cc:11954`.
    retrieved_record: bytes::BytesMut,
}

impl Default for HaSlateDb {
    fn default() -> Self {
        Self::new()
    }
}

impl HaSlateDb {
    /// Construct a fresh handler. No I/O. The C++ constructor
    /// initialises ~30 default-valued fields; we'll add them as the
    /// corresponding methods land.
    pub fn new() -> Self {
        Self {
            tbl_def: None,
            lock_rows: RowLockMode::None,
            db_lock_type: ThrLockType::Unlock,
            keyread_only: false,
            insert_with_update: false,
            retrieved_record: bytes::BytesMut::new(),
        }
    }

    /// Bind the handler to the table at `name`. The C++ signature is
    /// `int open(const char *name, int mode, uint test_if_locked)`; we
    /// drop `mode` and `test_if_locked` because no current code path
    /// inspects them.
    ///
    /// `name` is the MariaDB on-disk path form (`./db/tbl` or
    /// `./db/tbl#P#part`). We normalise via
    /// [`crate::utils::names::normalize_tablename`] to the
    /// `db.tbl[#P#part]` catalogue key.
    ///
    /// Errors:
    /// - `Invalid` if `name` isn't `./db/tbl[#P#part]` shape (matches
    ///   MyRocks' `HA_ERR_ROCKSDB_INVALID_TABLE` at the same site).
    /// - `Data` if the catalogue has no entry for the normalised name
    ///   (corruption: caller has a `.frm` we don't know about). Matches
    ///   the C++ `HA_ERR_INTERNAL_ERROR` at `ha_rocksdb.cc:6711` plus
    ///   error log.
    /// - `Invalid` if no engine is installed (`slatedb_init_in_memory`
    ///   wasn't called yet) — a programmer / lifecycle bug rather than
    ///   a runtime condition.
    ///
    /// Translated from `ha_rocksdb::open` (`ha_rocksdb.cc:6711`). The
    /// big-bear C++ version also allocates key buffers, computes
    /// `m_pk_descr` / `m_sk_descr` pointers, sets up `m_pk_packed_tuple`
    /// scratch, binds the per-table `table_handler` (refcounted I/O
    /// stats), and pre-fetches the auto-increment value. All deferred.
    pub fn open(&mut self, name: &str) -> Result<(), Error> {
        let normalized = crate::utils::names::normalize_tablename(name)?;
        let ddl = crate::bridge::current_ddl().ok_or_else(|| {
            Error::invalid(
                "HaSlateDb::open: no engine installed — call slatedb_init_* first".into(),
            )
        })?;
        let engine = crate::bridge::current_engine().ok_or_else(|| {
            Error::invalid("HaSlateDb::open: no engine installed".into())
        })?;
        let tbl_def = ddl.find(&normalized).ok_or_else(|| {
            Error::data(format!(
                "HaSlateDb::open: no catalogue entry for table {normalized:?}"
            ))
        })?;
        self.tbl_def = Some(tbl_def);

        // Prime the auto-incr / hidden-pk counter from persisted dict
        // state. Mirrors the C++ `ha_rocksdb::open` calls to
        // `load_auto_incr_value` / `load_hidden_pk_value` (the C++
        // gates the auto-incr call on `table->found_next_number_field`
        // and the hidden-pk call on `has_hidden_pk()`; we don't have a
        // TableShareView yet, so we always attempt the relevant load
        // — a missing dict entry is a no-op).
        let db_ref = engine.db().clone();
        crate::runtime::block_on(async {
            if self.has_hidden_pk() {
                self.load_hidden_pk_value(&db_ref).await?;
            } else {
                self.load_auto_incr_value(&db_ref).await?;
            }
            Ok::<_, Error>(())
        })?;

        Ok(())
    }

    /// Release the handler's per-table state. Idempotent — calling on
    /// an already-closed handler is OK (matches `ha_rocksdb::close`
    /// at `ha_rocksdb.cc:6875`, which is also free of failure paths).
    ///
    /// Does NOT touch the global engine state. Returns the
    /// previously-bound `TblDef` `Arc` count to zero only when no other
    /// references remain.
    pub fn close(&mut self) -> Result<(), Error> {
        self.tbl_def = None;
        Ok(())
    }

    /// Whether `open` has been called and `close` hasn't yet. Useful
    /// for tests and for the cxx bridge to gate ops that require an
    /// open handler.
    pub fn is_open(&self) -> bool {
        self.tbl_def.is_some()
    }

    /// Snapshot the bound `TblDef`. Returns `None` if the handler
    /// isn't currently open.
    pub fn tbl_def(&self) -> Option<&Arc<TblDef>> {
        self.tbl_def.as_ref()
    }

    /// Decide our internal row-lock mode + the MariaDB THR_LOCK type
    /// to install on the handler. Pure decision function — no SlateDB
    /// I/O. Translated from `ha_rocksdb::store_lock` at
    /// `ha_rocksdb.cc:11283`.
    ///
    /// Two decisions, mirroring the C++ structure:
    ///
    /// 1. **`lock_rows`** (`m_lock_rows` in C++): the internal
    ///    READ/WRITE/NONE mode that drives later txn-tagging
    ///    decisions.
    /// 2. **THR_LOCK downgrade**: possibly weakens the lock_type the
    ///    SQL layer should install — `TL_WRITE_CONCURRENT_INSERT..=TL_WRITE`
    ///    collapses to `TL_WRITE_ALLOW_WRITE` outside `LOCK TABLES`
    ///    so concurrent writers aren't blocked; `TL_READ_NO_INSERT`
    ///    becomes `TL_READ` outside `LOCK TABLES` to allow inserts
    ///    into the read-locked table in INSERT-SELECT patterns.
    ///
    /// Returns the (possibly downgraded) `ThrLockType` for the SQL
    /// layer to install. The C++ also writes the lock into a
    /// `THR_LOCK_DATA**` cursor — that's a cxx-bridge-side concern.
    ///
    /// ## What's deferred
    ///
    /// The `lock_scanned_rows` upgrade path (`ha_rocksdb.cc:11299..11323`)
    /// — when the THD's `lock_scanned_rows` sysvar is on AND the
    /// isolation level is `>= REPEATABLE_READ` (or `SERIALIZABLE`),
    /// MyRocks upgrades a NONE-mode read to a READ-mode read so
    /// scanned rows hold their lock past the scan. We don't model
    /// sysvars or `thd_tx_isolation` across cxx yet; deferred until
    /// the sysvar plumbing lands.
    pub fn store_lock(&mut self, thd: StoreLockThd, requested: ThrLockType) -> ThrLockType {
        // ----- 1. row-lock-mode decision -----
        if (requested as i32) >= (ThrLockType::WriteAllowWrite as i32) {
            self.lock_rows = RowLockMode::Write;
        } else if requested == ThrLockType::ReadWithSharedLocks {
            self.lock_rows = RowLockMode::Read;
        } else if requested != ThrLockType::Ignore {
            self.lock_rows = RowLockMode::None;
            // lock_scanned_rows + tx_isolation upgrade is deferred —
            // see method docs.
        }

        // ----- 2. THR_LOCK downgrade -----
        //
        // The C++ guards this with `m_db_lock.type == TL_UNLOCK` so a
        // re-entrant store_lock on an already-locked handler doesn't
        // downgrade the active lock. Match by checking `db_lock_type`.
        if requested != ThrLockType::Ignore && self.db_lock_type == ThrLockType::Unlock {
            let mut chosen = requested;

            // Concurrent-writes downgrade: TL_WRITE_CONCURRENT_INSERT..=TL_WRITE
            // → TL_WRITE_ALLOW_WRITE when outside LOCK TABLES + not a
            // tablespace op.
            if (chosen as i32) >= (ThrLockType::WriteConcurrentInsert as i32)
                && (chosen as i32) <= (ThrLockType::Write as i32)
                && !thd.in_lock_tables
                && !thd.tablespace_op
            {
                chosen = ThrLockType::WriteAllowWrite;
            }

            // INSERT…SELECT pattern: TL_READ_NO_INSERT → TL_READ
            // outside LOCK TABLES so the source table doesn't block
            // inserts into itself.
            if chosen == ThrLockType::ReadNoInsert && !thd.in_lock_tables {
                chosen = ThrLockType::Read;
            }

            self.db_lock_type = chosen;
            return chosen;
        }

        requested
    }

    /// Snapshot of [`Self::lock_rows`] — useful for tests and for the
    /// soon-to-land DML/read path that consumes it.
    pub fn lock_rows(&self) -> RowLockMode {
        self.lock_rows
    }

    /// Snapshot of the installed THR_LOCK type. Test/diagnostic accessor.
    pub fn db_lock_type(&self) -> ThrLockType {
        self.db_lock_type
    }

    /// Handler capability/hint toggle. Translated from
    /// `ha_rocksdb::extra` at `ha_rocksdb.cc:11939`.
    ///
    /// Five variants do real work; everything else falls through to
    /// a silent no-op (the C++ `default: break;`).
    ///
    /// Always returns `Ok(())` — toggles are infallible; the C++
    /// signature is `int` only because every MariaDB handler method
    /// returns `int`.
    pub fn extra(&mut self, op: HaExtraFunction) -> Result<(), Error> {
        match op {
            HaExtraFunction::Keyread => {
                self.keyread_only = true;
            }
            HaExtraFunction::NoKeyread => {
                self.keyread_only = false;
            }
            HaExtraFunction::Flush => {
                // If the table has BLOB columns they're part of the
                // retrieved-record buffer; flushing it invalidates
                // the BLOB handles the caller may still hold (C++
                // m_retrieved_record.Reset()).
                self.retrieved_record.clear();
            }
            HaExtraFunction::InsertWithUpdate => {
                // C++ gates this on the `rocksdb_enable_insert_with_update_caching`
                // sysvar (defaults true). We don't have sysvars across
                // cxx yet — hardcode the on-by-default behaviour;
                // sysvar override lands when sysvar plumbing does.
                self.insert_with_update = true;
            }
            HaExtraFunction::NoIgnoreDupKey => {
                self.insert_with_update = false;
            }
            HaExtraFunction::Other => {}
        }
        Ok(())
    }

    /// Snapshot of the keyread-only flag. Test/diagnostic accessor.
    pub fn keyread_only(&self) -> bool {
        self.keyread_only
    }

    /// Snapshot of the insert-with-update flag. Test/diagnostic accessor.
    pub fn insert_with_update(&self) -> bool {
        self.insert_with_update
    }

    /// Length of the retrieved-record buffer. Test/diagnostic accessor.
    pub fn retrieved_record_len(&self) -> usize {
        self.retrieved_record.len()
    }

    /// Push test bytes into the retrieved-record buffer. Only used
    /// by tests to verify `HA_EXTRA_FLUSH` clears it. Will be
    /// replaced by a real per-handler row-fetch path when DML/read
    /// lands.
    #[cfg(test)]
    pub(crate) fn set_retrieved_record_for_test(&mut self, bytes: &[u8]) {
        self.retrieved_record.clear();
        self.retrieved_record.extend_from_slice(bytes);
    }

    // ----- auto_incr + hidden_pk -----

    /// True iff this handler's table uses a hidden primary key (no
    /// declared PRIMARY KEY). Delegates to
    /// [`crate::codec::key::KeyDef::table_has_hidden_pk`] — but
    /// today we don't have a TableShareView for the bound table; we
    /// derive equivalently from `tbl_def.key_descrs` by checking the
    /// last index's type. Returns `false` if the handler isn't open
    /// (defensive — the C++ asserts).
    ///
    /// Translated from `ha_rocksdb::has_hidden_pk` at
    /// `ha_rocksdb.cc:9487`.
    pub fn has_hidden_pk(&self) -> bool {
        let Some(tdef) = &self.tbl_def else { return false };
        tdef.key_descrs()
            .last()
            .map(|kd| kd.index_type == crate::codec::key::IndexType::HiddenPrimary)
            .unwrap_or(false)
    }

    /// True iff `index` is the position of the hidden PK. Hidden PK
    /// is always the last entry in `tbl_def.key_descrs` (matches the
    /// C++ `m_key_count - 1`). Returns `false` if the table doesn't
    /// have a hidden PK or the handler isn't open.
    ///
    /// Translated from `ha_rocksdb::is_hidden_pk` at
    /// `ha_rocksdb.cc:9495`.
    pub fn is_hidden_pk(&self, index: u32) -> bool {
        let Some(tdef) = &self.tbl_def else { return false };
        if !self.has_hidden_pk() {
            return false;
        }
        (index as usize) + 1 == tdef.key_count()
    }

    /// Index-position of the PK (declared or hidden). Returns `None`
    /// if the handler isn't open or the catalogue's table has no
    /// keys at all (shouldn't happen — every table has at least a
    /// PK).
    ///
    /// Translated from `ha_rocksdb::pk_index` at
    /// `ha_rocksdb.cc:9504`. The C++ takes `table->s->primary_key`
    /// as input; we don't have that surfaced yet, so we walk
    /// `key_descrs` for a Primary or HiddenPrimary.
    pub fn pk_index(&self) -> Option<u32> {
        let Some(tdef) = &self.tbl_def else { return None };
        for (i, kd) in tdef.key_descrs().iter().enumerate() {
            if matches!(
                kd.index_type,
                crate::codec::key::IndexType::Primary
                    | crate::codec::key::IndexType::HiddenPrimary,
            ) {
                return Some(i as u32);
            }
        }
        None
    }

    /// True iff `index` is the PK (declared or hidden) position.
    /// Translated from `ha_rocksdb::is_pk` at `ha_rocksdb.cc:9513`.
    pub fn is_pk(&self, index: u32) -> bool {
        self.pk_index() == Some(index) || self.is_hidden_pk(index)
    }

    /// CAS-bump the in-memory auto-incr value to `val` if `val` is
    /// greater than the current. No-op on smaller values. Pure
    /// memory operation; no I/O. Returns `Err(Invalid)` if the
    /// handler isn't open.
    ///
    /// Translated from `ha_rocksdb::update_auto_incr_val` at
    /// `ha_rocksdb.cc:6191`. Implemented via
    /// [`crate::codec::tbl_def::TblDef::fetch_max_auto_incr_val`]
    /// (`AtomicU64::fetch_max`) — semantically identical to the
    /// C++'s `compare_exchange_weak` loop, fewer round trips.
    pub fn update_auto_incr_val(&self, val: u64) -> Result<(), Error> {
        let tdef = self.tbl_def.as_ref().ok_or_else(|| {
            Error::invalid("update_auto_incr_val: handler not open".into())
        })?;
        tdef.fetch_max_auto_incr_val(val);
        Ok(())
    }

    /// Allocate the next hidden-PK rowid: `fetch_add(1)` on the
    /// in-memory counter. Returns the **previous** value — that's
    /// the rowid to use for the new row, matching C++
    /// `m_hidden_pk_val++` (post-increment) at `ha_rocksdb.cc:6272`.
    ///
    /// Returns `Err(Invalid)` if the handler isn't open or the
    /// table doesn't have a hidden PK (the C++ asserts in the
    /// latter case).
    pub fn update_hidden_pk_val(&self) -> Result<i64, Error> {
        let tdef = self.tbl_def.as_ref().ok_or_else(|| {
            Error::invalid("update_hidden_pk_val: handler not open".into())
        })?;
        if !self.has_hidden_pk() {
            return Err(Error::invalid(
                "update_hidden_pk_val: table has no hidden PK".into(),
            ));
        }
        Ok(tdef.fetch_add_hidden_pk_val(1))
    }

    /// Decode the 8-byte hidden-PK rowid from a row-key. The key
    /// layout is `u32_be(index_id) || u64_be(hidden_pk)` — we skip
    /// the 4-byte index header and read the next 8 bytes as a u64.
    ///
    /// Translated from `ha_rocksdb::read_hidden_pk_id_from_rowkey`
    /// at `ha_rocksdb.cc:6277`. Returns the rowid as `i64` because
    /// the C++ stores hidden-PK values as `longlong` even though
    /// they're written as `u64_be` on the wire (sign matters for
    /// the wraparound semantic elsewhere in MyRocks).
    ///
    /// Errors with `Data` on a short rowkey (matches the C++
    /// `HA_ERR_ROCKSDB_CORRUPT_DATA`).
    pub fn read_hidden_pk_id_from_rowkey(rowkey: &[u8]) -> Result<i64, Error> {
        use crate::codec::key::INDEX_NUMBER_SIZE;
        const HIDDEN_PK_BYTES: usize = 8;
        if rowkey.len() < INDEX_NUMBER_SIZE + HIDDEN_PK_BYTES {
            return Err(Error::data(format!(
                "read_hidden_pk_id_from_rowkey: short rowkey \
                 (got {} bytes, need {})",
                rowkey.len(),
                INDEX_NUMBER_SIZE + HIDDEN_PK_BYTES,
            )));
        }
        let mut buf = [0u8; HIDDEN_PK_BYTES];
        buf.copy_from_slice(
            &rowkey[INDEX_NUMBER_SIZE..INDEX_NUMBER_SIZE + HIDDEN_PK_BYTES],
        );
        Ok(u64::from_be_bytes(buf) as i64)
    }

    /// Build the row key for a hidden-PK row write. Writes
    /// `u32_be(index_number) || u64_be(hidden_pk_id)` (12 bytes
    /// total) into the front of `dst` and returns the byte count.
    ///
    /// Composes [`crate::codec::key::KeyDef::get_infimum_key`] +
    /// [`crate::codec::key::KeyDef::build_hidden_pk_id_buf`]. The
    /// future `write_row` path calls this once per insert on a
    /// hidden-PK table.
    ///
    /// Errors:
    /// - `Invalid` if the handler isn't open
    /// - `Invalid` if the bound table doesn't have a hidden PK
    ///   (`has_hidden_pk()` returned `false`)
    /// - `Invalid` if `dst` is shorter than 12 bytes
    pub fn pack_hidden_pk_row_key(
        &self,
        hidden_pk_id: i64,
        dst: &mut [u8],
    ) -> Result<usize, Error> {
        use crate::codec::key::INDEX_NUMBER_SIZE;
        use crate::globals::SIZEOF_HIDDEN_PK_COLUMN;
        const ROW_KEY_BYTES: usize = INDEX_NUMBER_SIZE + SIZEOF_HIDDEN_PK_COLUMN;

        if !self.has_hidden_pk() {
            return Err(Error::invalid(
                "pack_hidden_pk_row_key: handler not open or table \
                 has no hidden PK"
                    .into(),
            ));
        }
        if dst.len() < ROW_KEY_BYTES {
            return Err(Error::invalid(format!(
                "pack_hidden_pk_row_key: dst too short — have {} bytes, \
                 need {}",
                dst.len(),
                ROW_KEY_BYTES,
            )));
        }

        // `has_hidden_pk()` returned true above, so tbl_def + a PK
        // slot are present.
        let tdef = self.tbl_def.as_ref().expect("has_hidden_pk implies open");
        let pk_idx = self.pk_index().expect("has_hidden_pk implies PK present");
        let pk_kd = tdef
            .key(pk_idx as usize)
            .expect("pk_index returns valid slot");

        let mut written = 0usize;
        pk_kd.get_infimum_key(dst, &mut written);
        let added = pk_kd.build_hidden_pk_id_buf(hidden_pk_id, &mut dst[written..])?;
        Ok(written + added)
    }

    /// Prime the in-memory `auto_incr_val` from the persisted dict
    /// entry. No-op if the dict has no row for this table's PK
    /// (`gl_index_id` for the PK comes from
    /// [`TblDef::get_autoincr_gl_index_id`]). Returns `Err(Invalid)`
    /// if the handler isn't open.
    ///
    /// Translated from `ha_rocksdb::load_auto_incr_value` at
    /// `ha_rocksdb.cc:6099`. The C++ also falls back to a descending
    /// PK-index scan when the dict has no entry — that fallback is
    /// deferred until the read path (cxx-driven `Field` decode +
    /// descending iterator) lands. For a freshly-`CREATE TABLE`d
    /// table the dict entry is absent and the in-memory counter
    /// stays at the default (0); the first `get_auto_increment` call
    /// bumps it.
    pub async fn load_auto_incr_value(&self, db: &slatedb::Db) -> Result<(), Error> {
        let tdef = self.tbl_def.as_ref().ok_or_else(|| {
            Error::invalid("load_auto_incr_value: handler not open".into())
        })?;
        let gl = tdef.get_autoincr_gl_index_id();
        if let Some(val) = crate::codec::dict::autoinc::read(db, gl).await? {
            tdef.fetch_max_auto_incr_val(val);
        }
        Ok(())
    }

    /// Prime the in-memory `hidden_pk_val` from the persisted dict
    /// entry. Same dict slot as auto-incr (keyed by the PK's
    /// `gl_index_id`); the value is interpreted as the next-rowid
    /// high-water-mark. No-op if the table doesn't have a hidden PK
    /// or if the dict has no row. Returns `Err(Invalid)` if the
    /// handler isn't open.
    ///
    /// Translated from `ha_rocksdb::load_hidden_pk_value` at
    /// `ha_rocksdb.cc:6227`. The C++ fallback PK-desc scan is
    /// deferred along with [`load_auto_incr_value`]'s.
    pub async fn load_hidden_pk_value(&self, db: &slatedb::Db) -> Result<(), Error> {
        if !self.has_hidden_pk() {
            return Ok(());
        }
        let tdef = self.tbl_def.as_ref().ok_or_else(|| {
            Error::invalid("load_hidden_pk_value: handler not open".into())
        })?;
        let gl = tdef.get_autoincr_gl_index_id();
        if let Some(val) = crate::codec::dict::autoinc::read(db, gl).await? {
            // C++ stores hidden_pk as longlong; the on-disk value is
            // u64_be but the in-memory counter is i64. The cast is
            // intentional and matches the C++ `m_hidden_pk_val.store
            // (auto_incr)` at ha_rocksdb.cc:6240.
            tdef.fetch_max_hidden_pk_val(val as i64);
        }
        Ok(())
    }

    /// Auto-increment reservation. Returns `(first_value,
    /// nb_reserved_values)` — `nb_reserved_values` is always 1
    /// (matches MyRocks: "we will always tell MySQL that we only
    /// reserved 1 value" at `ha_rocksdb.cc:12297`).
    ///
    /// `max_val` is the largest legal value for the auto-incr
    /// column's type (the C++ derives it from `Field`; we accept it
    /// as a parameter so the handler doesn't need Field access).
    ///
    /// `inc == 1` is the fast path — a straight CAS bump capped at
    /// `max_val`. `inc != 1` follows the replication-multi-master
    /// arithmetic from `ha_rocksdb.cc:12339..12413`.
    ///
    /// If the in-memory counter is at `u64::MAX`, returns
    /// `(u64::MAX, 1)` — the SQL layer maps that to
    /// `ER_AUTOINC_READ_FAILED` for UNSIGNED BIGINT or to
    /// `ER_DUP_ENTRY` for other types.
    ///
    /// Translated from `ha_rocksdb::get_auto_increment` at
    /// `ha_rocksdb.cc:12282`. Returns `Err(Invalid)` if the handler
    /// isn't open.
    pub fn get_auto_increment(
        &self,
        mut off: u64,
        inc: u64,
        max_val: u64,
    ) -> Result<(u64, u64), Error> {
        let tdef = self.tbl_def.as_ref().ok_or_else(|| {
            Error::invalid("get_auto_increment: handler not open".into())
        })?;
        if off > inc {
            off = 1;
        }
        let atomic = tdef.auto_incr_atomic();

        let new_val = if inc == 1 {
            // Fast path: CAS bump capped at max_val. Matches the C++
            // inc==1 branch at ha_rocksdb.cc:12316..12338.
            let mut current = atomic.load(std::sync::atomic::Ordering::Relaxed);
            loop {
                if current == u64::MAX {
                    break u64::MAX;
                }
                let stored = std::cmp::min(current.saturating_add(1), max_val);
                match atomic.compare_exchange_weak(
                    current,
                    stored,
                    std::sync::atomic::Ordering::Relaxed,
                    std::sync::atomic::Ordering::Relaxed,
                ) {
                    Ok(_) => break current,
                    Err(actual) => current = actual,
                }
            }
        } else {
            // Replication-style sequence: off + N * inc.
            //
            // Same arithmetic as the C++ — see the long comment at
            // ha_rocksdb.cc:12339..12413 for the derivation.
            let mut last_val = atomic.load(std::sync::atomic::Ordering::Relaxed);
            if last_val > max_val {
                last_val = u64::MAX;
                atomic.store(last_val, std::sync::atomic::Ordering::Relaxed);
                last_val
            } else {
                loop {
                    debug_assert!(last_val > 0);
                    let last_minus_1 = last_val - 1;
                    let n = last_minus_1 / inc
                        + (last_minus_1 % inc + inc - off) / inc;
                    // Overflow check: if n * inc + off would overflow,
                    // bail with u64::MAX (C++ ha_rocksdb.cc:12376..12399).
                    if n > (u64::MAX - off) / inc {
                        debug_assert_eq!(max_val, u64::MAX);
                        atomic.store(u64::MAX, std::sync::atomic::Ordering::Relaxed);
                        break u64::MAX;
                    }
                    let candidate = n * inc + off;
                    let stored = std::cmp::min(candidate.saturating_add(1), max_val);
                    match atomic.compare_exchange_weak(
                        last_val,
                        stored,
                        std::sync::atomic::Ordering::Relaxed,
                        std::sync::atomic::Ordering::Relaxed,
                    ) {
                        Ok(_) => break candidate,
                        Err(actual) => last_val = actual,
                    }
                }
            }
        };

        Ok((new_val, 1))
    }

    /// Statement-boundary hook. Translated (with significant scope
    /// reduction) from `ha_rocksdb::external_lock` at
    /// `ha_rocksdb.cc:11409`.
    ///
    /// - On `F_RDLCK` / `F_WRLCK` (lock acquire): get-or-create the
    ///   per-THD transaction in the global
    ///   [`crate::engine::txn_registry::TxnRegistry`]. For `F_WRLCK`
    ///   we additionally set `lock_rows = Write` (matches the C++).
    /// - On `F_UNLCK`: if `autocommit_boundary` is true (caller is
    ///   outside `BEGIN` and has autocommit on), commit the
    ///   transaction. Otherwise leave it open — the SQL layer will
    ///   either issue more statements in the same txn or `COMMIT` /
    ///   `ROLLBACK` later.
    ///
    /// `autocommit_boundary` is the cxx side's pre-computed answer
    /// to "should this F_UNLCK commit the txn?" — derived from
    /// `thd->variables.option_bits & (OPTION_NOT_AUTOCOMMIT |
    /// OPTION_BEGIN)` plus the `n_mysql_tables_in_use` counter the
    /// SQL layer maintains. We don't model those on the Rust side.
    ///
    /// ## What's deferred
    ///
    /// - Isolation-level selection — txn always begins at
    ///   SerializableSnapshot today; sysvar-driven Snapshot vs
    ///   SerializableSnapshot lands when sysvars do.
    /// - Isolation-level validation (reject SERIALIZABLE outside the
    ///   READ_COMMITTED..=REPEATABLE_READ band per the C++).
    /// - DDL tagging (mark txn as CREATE_INDEX/DROP_INDEX/ALTER
    ///   so commit can apply DDL-specific logic).
    /// - `register_tx(thd)` cxx callback to tell MariaDB to call our
    ///   commit/rollback at end-of-statement.
    pub async fn external_lock(
        &mut self,
        thd_id: u64,
        lock_type: ExternalLockType,
        autocommit_boundary: bool,
    ) -> Result<(), Error> {
        let registry = crate::bridge::current_txn_registry().ok_or_else(|| {
            Error::invalid(
                "HaSlateDb::external_lock: no engine installed — call slatedb_init_* first"
                    .into(),
            )
        })?;
        let db = crate::bridge::current_engine().ok_or_else(|| {
            Error::invalid("HaSlateDb::external_lock: no engine installed".into())
        })?;

        match lock_type {
            ExternalLockType::Write => {
                self.lock_rows = RowLockMode::Write;
                registry.get_or_create(thd_id, &db).await?;
            }
            ExternalLockType::Read => {
                registry.get_or_create(thd_id, &db).await?;
            }
            ExternalLockType::Unlock => {
                if autocommit_boundary {
                    registry.commit(thd_id).await?;
                }
                // else: txn stays open for the next statement.
            }
        }
        Ok(())
    }
}

/// Wrap a `Result<(), Error>` from a handler method as an `i32`
/// status code for the cxx boundary. `Ok` → `status::OK`; the
/// `Error` kind drives the failure code:
/// - `Invalid` → [`status::BAD_TABLE_PATH`] (or `NO_ENGINE` /
///   `ENGINE_IO_FAILED` for non-path invalids; we collapse to
///   `BAD_TABLE_PATH` for the MVS).
/// - `Data` → [`status::NO_SUCH_TABLE`]
/// - others → [`status::ENGINE_IO_FAILED`]
///
/// Coarse-grained at this stage; same trade-off as the bridge's
/// `init_in_memory` return codes — richer info will surface via the
/// HA_ERR translation channel once we have one.
pub(crate) fn open_result_to_status(r: Result<(), Error>) -> i32 {
    match r {
        Ok(()) => status::OK,
        Err(e) => match e.kind() {
            slatedb::ErrorKind::Invalid => {
                // Differentiate "no engine" vs "bad path" by message
                // text — fragile, but acceptable until we plumb a
                // richer error variant. The two come from different
                // call sites in this module so the substring is
                // discriminating.
                if e.to_string().contains("no engine installed") {
                    status::NO_ENGINE
                } else {
                    status::BAD_TABLE_PATH
                }
            }
            slatedb::ErrorKind::Data => status::NO_SUCH_TABLE,
            _ => status::ENGINE_IO_FAILED,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::codec::key::{
        IndexType, KeyDef, INDEX_INFO_VERSION_LATEST, PRIMARY_FORMAT_VERSION_LATEST,
    };
    use crate::engine::ddl_manager::DdlManager;
    use parking_lot::Mutex;
    use std::sync::Arc;

    /// Bridge tests serialise on this for the same reasons as
    /// `bridge::tests::SERIALISE` — process-global state.
    static SERIALISE: Mutex<()> = Mutex::new(());

    fn pk(index_number: u32, cf_id: u32) -> Arc<KeyDef> {
        let mut kd = KeyDef::new_skeleton(
            index_number,
            cf_id,
            0,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Primary,
            PRIMARY_FORMAT_VERSION_LATEST,
            false,
            "pk",
        );
        kd.maxlength = 12;
        Arc::new(kd)
    }

    fn hidden_pk(index_number: u32, cf_id: u32) -> Arc<KeyDef> {
        let mut kd = KeyDef::new_skeleton(
            index_number,
            cf_id,
            0,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::HiddenPrimary,
            PRIMARY_FORMAT_VERSION_LATEST,
            false,
            "HIDDEN_PK_NAME",
        );
        kd.maxlength = 12;
        Arc::new(kd)
    }

    /// Install an engine and put a fixture table in its catalogue.
    /// Tests that need a bound handler use this.
    ///
    /// Synchronous wrapper because the bridge installs its own tokio
    /// runtime; nesting a `#[tokio::test]` runtime inside would
    /// panic with "cannot start a runtime from within a runtime".
    fn install_engine_with_fixture(name_in_dot: &str) {
        install_engine_with_keys(name_in_dot, vec![pk(100, 1)]);
    }

    /// Like [`install_engine_with_fixture`] but accepts a custom key
    /// layout — for tests that need a hidden-PK fixture or want to
    /// pin the PK's `(cf_id, index_id)` for dict writes.
    fn install_engine_with_keys(name_in_dot: &str, keys: Vec<Arc<KeyDef>>) {
        let _ = crate::bridge::slatedb_shutdown();
        assert_eq!(
            crate::bridge::slatedb_init_in_memory(format!("handler_test_{name_in_dot}")),
            status::OK,
            "init",
        );
        let db = crate::bridge::current_engine().expect("engine just installed");
        let ddl = crate::bridge::current_ddl().expect("ddl just installed");
        let tdef = Arc::new(TblDef::new(name_in_dot).unwrap().with_keys(keys));
        crate::runtime::block_on(async move {
            ddl.put_and_write(tdef, db.db()).await.expect("put_and_write");
        });
    }

    #[test]
    fn new_handler_is_not_open() {
        let h = HaSlateDb::new();
        assert!(!h.is_open());
        assert!(h.tbl_def().is_none());
    }

    #[test]
    fn open_rejects_malformed_path() {
        let mut h = HaSlateDb::new();
        let err = h.open("not_a_path").unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(!h.is_open());
    }

    #[test]
    fn open_then_close_round_trip() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.t1");

        let mut h = HaSlateDb::new();
        h.open("./appdb/t1").expect("open");
        assert!(h.is_open());
        assert_eq!(
            h.tbl_def().unwrap().full_tablename(),
            "appdb.t1",
            "tbl_def is bound correctly",
        );

        h.close().expect("close");
        assert!(!h.is_open());
        // Second close is OK.
        h.close().expect("close again");

        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn open_for_unknown_table_returns_data_error() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.exists");

        let mut h = HaSlateDb::new();
        let err = h.open("./appdb/does_not_exist").unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Data);
        assert!(!h.is_open());

        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn open_without_engine_returns_invalid() {
        let _g = SERIALISE.lock();
        let _ = crate::bridge::slatedb_shutdown();

        let mut h = HaSlateDb::new();
        let err = h.open("./appdb/anything").unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(err.to_string().contains("no engine installed"));
    }

    // ----- status code mapping -----

    #[test]
    fn open_result_to_status_maps_each_error_kind() {
        assert_eq!(open_result_to_status(Ok(())), status::OK);

        // Bad path (Invalid without "no engine" marker).
        let bad = HaSlateDb::new().open("not_a_path").unwrap_err();
        assert_eq!(open_result_to_status(Err(bad)), status::BAD_TABLE_PATH);

        // No engine (Invalid with "no engine" marker).
        let mut h = HaSlateDb::new();
        let no_eng = {
            let _g = SERIALISE.lock();
            let _ = crate::bridge::slatedb_shutdown();
            h.open("./appdb/x").unwrap_err()
        };
        assert_eq!(open_result_to_status(Err(no_eng)), status::NO_ENGINE);

        // Data error (no such table) — we synthesise one without
        // standing up a fixture engine.
        let data = slatedb::Error::data("no such table".into());
        assert_eq!(open_result_to_status(Err(data)), status::NO_SUCH_TABLE);

        // Other (Unavailable / Internal / etc.) → ENGINE_IO_FAILED.
        let unavail = slatedb::Error::unavailable("transient".into());
        assert_eq!(open_result_to_status(Err(unavail)), status::ENGINE_IO_FAILED);
    }

    #[test]
    fn ddl_manager_is_unused_import_silence() {
        // Tiny sanity: DdlManager is reachable from this module (we
        // use it transitively via bridge::current_ddl). This is here
        // so the import isn't flagged as unused when the doc-test for
        // DdlManager isn't compiled.
        let _ = DdlManager::new();
    }

    // ----- store_lock -----

    fn thd_default() -> StoreLockThd {
        StoreLockThd::default()
    }

    #[test]
    fn store_lock_write_set_lock_rows_write() {
        let mut h = HaSlateDb::new();
        for write_kind in [
            ThrLockType::WriteAllowWrite,
            ThrLockType::WriteConcurrentInsert,
            ThrLockType::WriteDefault,
            ThrLockType::WriteLowPriority,
            ThrLockType::Write,
            ThrLockType::WriteOnly,
        ] {
            let mut h2 = HaSlateDb::new();
            h2.store_lock(thd_default(), write_kind);
            assert_eq!(h2.lock_rows(), RowLockMode::Write, "{write_kind:?}");
        }
        // Sanity: parameterised over a clean handler each iteration; h
        // here is just to demonstrate the API also accepts a long-lived
        // handler.
        h.store_lock(thd_default(), ThrLockType::WriteDefault);
        assert_eq!(h.lock_rows(), RowLockMode::Write);
    }

    #[test]
    fn store_lock_read_with_shared_locks_sets_lock_rows_read() {
        let mut h = HaSlateDb::new();
        h.store_lock(thd_default(), ThrLockType::ReadWithSharedLocks);
        assert_eq!(h.lock_rows(), RowLockMode::Read);
    }

    #[test]
    fn store_lock_plain_read_sets_lock_rows_none() {
        let mut h = HaSlateDb::new();
        h.store_lock(thd_default(), ThrLockType::Read);
        assert_eq!(h.lock_rows(), RowLockMode::None);
    }

    #[test]
    fn store_lock_ignore_does_not_change_lock_rows() {
        let mut h = HaSlateDb::new();
        // Seed lock_rows = Write via an earlier call.
        h.store_lock(thd_default(), ThrLockType::Write);
        assert_eq!(h.lock_rows(), RowLockMode::Write);

        // TL_IGNORE must NOT clear it.
        h.store_lock(thd_default(), ThrLockType::Ignore);
        assert_eq!(h.lock_rows(), RowLockMode::Write);
    }

    #[test]
    fn store_lock_downgrades_write_range_outside_lock_tables() {
        for input in [
            ThrLockType::WriteConcurrentInsert,
            ThrLockType::WriteDelayed,
            ThrLockType::WriteDefault,
            ThrLockType::WriteLowPriority,
            ThrLockType::Write,
        ] {
            let mut h = HaSlateDb::new();
            let out = h.store_lock(thd_default(), input);
            assert_eq!(
                out,
                ThrLockType::WriteAllowWrite,
                "downgrade for {input:?}",
            );
            assert_eq!(h.db_lock_type(), ThrLockType::WriteAllowWrite);
        }
    }

    #[test]
    fn store_lock_does_not_downgrade_inside_lock_tables() {
        let thd = StoreLockThd {
            in_lock_tables: true,
            tablespace_op: false,
        };
        let mut h = HaSlateDb::new();
        let out = h.store_lock(thd, ThrLockType::WriteDefault);
        assert_eq!(out, ThrLockType::WriteDefault, "inside LOCK TABLES, no downgrade");
    }

    #[test]
    fn store_lock_does_not_downgrade_for_tablespace_op() {
        let thd = StoreLockThd {
            in_lock_tables: false,
            tablespace_op: true,
        };
        let mut h = HaSlateDb::new();
        let out = h.store_lock(thd, ThrLockType::WriteDefault);
        assert_eq!(out, ThrLockType::WriteDefault, "tablespace op blocks downgrade");
    }

    #[test]
    fn store_lock_downgrades_read_no_insert_outside_lock_tables() {
        let mut h = HaSlateDb::new();
        let out = h.store_lock(thd_default(), ThrLockType::ReadNoInsert);
        assert_eq!(out, ThrLockType::Read);
        // The lock_rows decision for plain reads is still None.
        assert_eq!(h.lock_rows(), RowLockMode::None);
    }

    #[test]
    fn store_lock_does_not_redowngrade_when_lock_already_installed() {
        // After the first store_lock installs a lock, a second call
        // must NOT re-downgrade it — matches the C++ `m_db_lock.type
        // == TL_UNLOCK` gate. Re-entrant call returns the input lock
        // unchanged.
        let mut h = HaSlateDb::new();
        let first = h.store_lock(thd_default(), ThrLockType::Write);
        assert_eq!(first, ThrLockType::WriteAllowWrite);

        // Now the handler holds WriteAllowWrite. A second call should
        // see db_lock_type != Unlock and return its input verbatim.
        let second = h.store_lock(thd_default(), ThrLockType::ReadNoInsert);
        assert_eq!(
            second,
            ThrLockType::ReadNoInsert,
            "no downgrade once a lock is installed",
        );
    }

    #[test]
    fn store_lock_returns_input_for_lock_types_outside_downgrade_set() {
        let mut h = HaSlateDb::new();
        let out = h.store_lock(thd_default(), ThrLockType::ReadHighPriority);
        // Not in the WriteConcurrentInsert..=Write range and not
        // ReadNoInsert ⇒ returned unchanged.
        assert_eq!(out, ThrLockType::ReadHighPriority);
    }

    #[test]
    fn thr_lock_type_from_i32_round_trips_known_values() {
        for v in [-1i32, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12] {
            assert_eq!(ThrLockType::from_i32(v) as i32, v);
        }
        // Unknown -> Ignore.
        assert_eq!(ThrLockType::from_i32(99), ThrLockType::Ignore);
        assert_eq!(ThrLockType::from_i32(-2), ThrLockType::Ignore);
    }

    // ----- extra -----

    #[test]
    fn extra_keyread_toggles_flag() {
        let mut h = HaSlateDb::new();
        assert!(!h.keyread_only());
        h.extra(HaExtraFunction::Keyread).unwrap();
        assert!(h.keyread_only());
        h.extra(HaExtraFunction::NoKeyread).unwrap();
        assert!(!h.keyread_only());
    }

    #[test]
    fn extra_flush_clears_retrieved_record() {
        let mut h = HaSlateDb::new();
        h.set_retrieved_record_for_test(b"row-bytes");
        assert_eq!(h.retrieved_record_len(), 9);
        h.extra(HaExtraFunction::Flush).unwrap();
        assert_eq!(h.retrieved_record_len(), 0);
    }

    #[test]
    fn extra_insert_with_update_pairs() {
        let mut h = HaSlateDb::new();
        assert!(!h.insert_with_update());
        h.extra(HaExtraFunction::InsertWithUpdate).unwrap();
        assert!(h.insert_with_update());
        h.extra(HaExtraFunction::NoIgnoreDupKey).unwrap();
        assert!(!h.insert_with_update());
    }

    #[test]
    fn extra_other_is_a_silent_noop() {
        let mut h = HaSlateDb::new();
        // Seed every flag in the "on" state.
        h.extra(HaExtraFunction::Keyread).unwrap();
        h.extra(HaExtraFunction::InsertWithUpdate).unwrap();
        h.set_retrieved_record_for_test(b"data");

        // Other must not touch any of them.
        h.extra(HaExtraFunction::Other).unwrap();
        assert!(h.keyread_only(), "keyread unchanged");
        assert!(h.insert_with_update(), "insert_with_update unchanged");
        assert_eq!(h.retrieved_record_len(), 4, "retrieved_record unchanged");
    }

    #[test]
    fn extra_flush_does_not_touch_other_flags() {
        let mut h = HaSlateDb::new();
        h.extra(HaExtraFunction::Keyread).unwrap();
        h.extra(HaExtraFunction::InsertWithUpdate).unwrap();
        h.set_retrieved_record_for_test(b"data");

        h.extra(HaExtraFunction::Flush).unwrap();
        assert_eq!(h.retrieved_record_len(), 0, "flush cleared the buffer");
        assert!(h.keyread_only(), "flush did not touch keyread");
        assert!(h.insert_with_update(), "flush did not touch insert_with_update");
    }

    // ----- external_lock -----

    // ----- auto_incr + hidden_pk -----

    fn hidden_pk_kd(index_number: u32, cf_id: u32) -> Arc<KeyDef> {
        let mut kd = KeyDef::new_skeleton(
            index_number,
            cf_id,
            0,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::HiddenPrimary,
            PRIMARY_FORMAT_VERSION_LATEST,
            false,
            "HIDDEN_PK",
        );
        kd.maxlength = 12;
        Arc::new(kd)
    }

    fn install_table_with_keys_for_test(name: &str, keys: Vec<Arc<KeyDef>>) {
        let _ = crate::bridge::slatedb_shutdown();
        assert_eq!(
            crate::bridge::slatedb_init_in_memory(format!("handler_aic_{name}")),
            status::OK,
        );
        let db = crate::bridge::current_engine().expect("engine");
        let ddl = crate::bridge::current_ddl().expect("ddl");
        let tdef = Arc::new(TblDef::new(name).unwrap().with_keys(keys));
        crate::runtime::block_on(async move {
            ddl.put_and_write(tdef, db.db()).await.expect("put_and_write");
        });
    }

    #[test]
    fn has_hidden_pk_true_when_last_index_is_hidden() {
        let _g = SERIALISE.lock();
        install_table_with_keys_for_test("appdb.hpk", vec![hidden_pk_kd(100, 1)]);
        let mut h = HaSlateDb::new();
        h.open("./appdb/hpk").expect("open");
        assert!(h.has_hidden_pk());
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn has_hidden_pk_false_when_only_declared_pk() {
        let _g = SERIALISE.lock();
        install_table_with_keys_for_test("appdb.pk_only", vec![pk(100, 1)]);
        let mut h = HaSlateDb::new();
        h.open("./appdb/pk_only").expect("open");
        assert!(!h.has_hidden_pk());
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn has_hidden_pk_false_when_handler_not_open() {
        let h = HaSlateDb::new();
        assert!(!h.has_hidden_pk());
    }

    #[test]
    fn is_hidden_pk_only_for_last_slot_of_hidden_pk_table() {
        let _g = SERIALISE.lock();
        install_table_with_keys_for_test(
            "appdb.hpk2",
            vec![
                Arc::new(KeyDef::new_skeleton(
                    50,
                    1,
                    0,
                    INDEX_INFO_VERSION_LATEST as u16,
                    IndexType::Secondary,
                    PRIMARY_FORMAT_VERSION_LATEST,
                    false,
                    "sk",
                )),
                hidden_pk_kd(51, 1),
            ],
        );
        let mut h = HaSlateDb::new();
        h.open("./appdb/hpk2").expect("open");
        assert!(!h.is_hidden_pk(0)); // SK slot
        assert!(h.is_hidden_pk(1));  // hidden PK at last slot
        assert!(!h.is_hidden_pk(99)); // out of range
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn pk_index_finds_declared_or_hidden() {
        let _g = SERIALISE.lock();
        install_table_with_keys_for_test(
            "appdb.pkfind",
            vec![pk(100, 1)],
        );
        let mut h = HaSlateDb::new();
        h.open("./appdb/pkfind").expect("open");
        assert_eq!(h.pk_index(), Some(0));
        assert!(h.is_pk(0));
        assert!(!h.is_pk(1));
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn update_auto_incr_val_cas_bumps_only_upward() {
        let _g = SERIALISE.lock();
        install_table_with_keys_for_test("appdb.aic", vec![pk(100, 1)]);
        let mut h = HaSlateDb::new();
        h.open("./appdb/aic").expect("open");

        let tdef = h.tbl_def().unwrap().clone();
        tdef.store_auto_incr_val(10);

        // Lower value is a no-op.
        h.update_auto_incr_val(5).unwrap();
        assert_eq!(tdef.auto_incr_val(), 10);

        // Higher value bumps.
        h.update_auto_incr_val(20).unwrap();
        assert_eq!(tdef.auto_incr_val(), 20);

        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn update_auto_incr_val_errors_when_handler_not_open() {
        let h = HaSlateDb::new();
        let err = h.update_auto_incr_val(1).unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
    }

    #[test]
    fn update_hidden_pk_val_fetch_adds_returning_old() {
        let _g = SERIALISE.lock();
        install_table_with_keys_for_test("appdb.hpkadd", vec![hidden_pk_kd(99, 1)]);
        let mut h = HaSlateDb::new();
        h.open("./appdb/hpkadd").expect("open");
        let tdef = h.tbl_def().unwrap().clone();
        tdef.store_hidden_pk_val(42);

        let first = h.update_hidden_pk_val().unwrap();
        assert_eq!(first, 42, "returns the old value");
        assert_eq!(tdef.hidden_pk_val(), 43);

        let second = h.update_hidden_pk_val().unwrap();
        assert_eq!(second, 43);
        assert_eq!(tdef.hidden_pk_val(), 44);

        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn update_hidden_pk_val_errors_when_no_hidden_pk() {
        let _g = SERIALISE.lock();
        install_table_with_keys_for_test("appdb.nohpk", vec![pk(100, 1)]);
        let mut h = HaSlateDb::new();
        h.open("./appdb/nohpk").expect("open");
        let err = h.update_hidden_pk_val().unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(err.to_string().contains("no hidden PK"));
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn read_hidden_pk_id_from_rowkey_decodes_u64_after_index_header() {
        // Rowkey: u32_be(index_id=0x1234) || u64_be(hidden_pk=0xDEADBEEF).
        let mut rowkey = Vec::new();
        rowkey.extend_from_slice(&0x1234_u32.to_be_bytes());
        rowkey.extend_from_slice(&0xDEAD_BEEF_u64.to_be_bytes());
        let id = HaSlateDb::read_hidden_pk_id_from_rowkey(&rowkey).unwrap();
        assert_eq!(id, 0xDEAD_BEEF);
    }

    #[test]
    fn read_hidden_pk_id_from_rowkey_short_input_is_data_error() {
        let err = HaSlateDb::read_hidden_pk_id_from_rowkey(&[1, 2, 3]).unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Data);
    }

    #[test]
    fn get_auto_increment_inc_eq_1_fast_path() {
        let _g = SERIALISE.lock();
        install_table_with_keys_for_test("appdb.gai1", vec![pk(100, 1)]);
        let mut h = HaSlateDb::new();
        h.open("./appdb/gai1").expect("open");
        let tdef = h.tbl_def().unwrap().clone();
        tdef.store_auto_incr_val(1);

        let (first, n) = h.get_auto_increment(1, 1, 1000).unwrap();
        assert_eq!(first, 1);
        assert_eq!(n, 1);
        assert_eq!(tdef.auto_incr_val(), 2);

        // Next call.
        let (first, _) = h.get_auto_increment(1, 1, 1000).unwrap();
        assert_eq!(first, 2);
        assert_eq!(tdef.auto_incr_val(), 3);

        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn get_auto_increment_caps_at_max_val() {
        let _g = SERIALISE.lock();
        install_table_with_keys_for_test("appdb.gai_cap", vec![pk(100, 1)]);
        let mut h = HaSlateDb::new();
        h.open("./appdb/gai_cap").expect("open");
        let tdef = h.tbl_def().unwrap().clone();
        tdef.store_auto_incr_val(100);

        // max_val = 100; the C++ stores std::min(new_val + 1, max_val)
        // so successive calls past max all see auto_incr stuck at 100
        // and the caller returns 100 repeatedly (which causes
        // ER_DUP_ENTRY in the SQL layer for unique-PK columns).
        let (first, _) = h.get_auto_increment(1, 1, 100).unwrap();
        assert_eq!(first, 100);
        assert_eq!(tdef.auto_incr_val(), 100);

        let (first2, _) = h.get_auto_increment(1, 1, 100).unwrap();
        assert_eq!(first2, 100, "stuck at max");

        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn get_auto_increment_u64_max_returns_max() {
        let _g = SERIALISE.lock();
        install_table_with_keys_for_test("appdb.gai_max", vec![pk(100, 1)]);
        let mut h = HaSlateDb::new();
        h.open("./appdb/gai_max").expect("open");
        let tdef = h.tbl_def().unwrap().clone();
        tdef.store_auto_incr_val(u64::MAX);

        let (first, _) = h.get_auto_increment(1, 1, u64::MAX).unwrap();
        assert_eq!(first, u64::MAX);

        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn get_auto_increment_inc_3_off_1_replication_sequence() {
        // off=1, inc=3 produces sequence 1, 4, 7, 10, ...
        let _g = SERIALISE.lock();
        install_table_with_keys_for_test("appdb.gai_rep", vec![pk(100, 1)]);
        let mut h = HaSlateDb::new();
        h.open("./appdb/gai_rep").expect("open");
        let tdef = h.tbl_def().unwrap().clone();
        tdef.store_auto_incr_val(1);

        let (a, _) = h.get_auto_increment(1, 3, 1000).unwrap();
        let (b, _) = h.get_auto_increment(1, 3, 1000).unwrap();
        let (c, _) = h.get_auto_increment(1, 3, 1000).unwrap();
        assert_eq!((a, b, c), (1, 4, 7));

        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn external_lock_type_from_i32_matches_sys_file_h() {
        assert_eq!(ExternalLockType::from_i32(1), Some(ExternalLockType::Read));
        assert_eq!(
            ExternalLockType::from_i32(2),
            Some(ExternalLockType::Write),
        );
        assert_eq!(
            ExternalLockType::from_i32(8),
            Some(ExternalLockType::Unlock),
        );
        // Unknown ints return None.
        assert_eq!(ExternalLockType::from_i32(0), None);
        assert_eq!(ExternalLockType::from_i32(99), None);
        assert_eq!(ExternalLockType::from_i32(-1), None);
    }

    #[test]
    fn external_lock_wrlck_sets_lock_rows_and_creates_txn() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.elt1");

        let mut h = HaSlateDb::new();
        crate::runtime::block_on(async {
            h.external_lock(1001, ExternalLockType::Write, false)
                .await
                .expect("write lock");
        });

        assert_eq!(h.lock_rows(), RowLockMode::Write);
        let reg = crate::bridge::current_txn_registry().expect("registry");
        assert!(reg.has(1001), "txn registered for thd_id=1001");

        // Clean up.
        reg.rollback(1001);
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn external_lock_rdlck_creates_txn_does_not_change_lock_rows() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.elt2");

        let mut h = HaSlateDb::new();
        // Pre-seed lock_rows to verify Read doesn't touch it.
        h.lock_rows = RowLockMode::None;
        crate::runtime::block_on(async {
            h.external_lock(2002, ExternalLockType::Read, false)
                .await
                .expect("read lock");
        });

        assert_eq!(h.lock_rows(), RowLockMode::None);
        let reg = crate::bridge::current_txn_registry().expect("registry");
        assert!(reg.has(2002));

        reg.rollback(2002);
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn external_lock_unlock_with_autocommit_commits_txn() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.elt3");

        let reg = crate::bridge::current_txn_registry().expect("registry");
        let mut h = HaSlateDb::new();
        crate::runtime::block_on(async {
            h.external_lock(3003, ExternalLockType::Read, false)
                .await
                .expect("acquire");
            assert!(reg.has(3003));

            h.external_lock(3003, ExternalLockType::Unlock, true)
                .await
                .expect("commit-on-unlock");
        });

        assert!(!reg.has(3003), "autocommit-boundary unlock dropped the txn");

        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn external_lock_unlock_without_autocommit_keeps_txn() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.elt4");

        let reg = crate::bridge::current_txn_registry().expect("registry");
        let mut h = HaSlateDb::new();
        crate::runtime::block_on(async {
            h.external_lock(4004, ExternalLockType::Read, false)
                .await
                .expect("acquire");

            // F_UNLCK inside an open BEGIN block — must NOT commit.
            h.external_lock(4004, ExternalLockType::Unlock, false)
                .await
                .expect("unlock-no-commit");
        });

        assert!(
            reg.has(4004),
            "txn stays open for the next statement in the BEGIN block",
        );

        reg.rollback(4004);
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn external_lock_distinct_thd_ids_get_distinct_txns() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.elt5");

        let reg = crate::bridge::current_txn_registry().expect("registry");
        let mut h1 = HaSlateDb::new();
        let mut h2 = HaSlateDb::new();
        crate::runtime::block_on(async {
            h1.external_lock(5001, ExternalLockType::Write, false)
                .await
                .expect("h1");
            h2.external_lock(5002, ExternalLockType::Write, false)
                .await
                .expect("h2");
        });

        assert!(reg.has(5001));
        assert!(reg.has(5002));
        assert_eq!(reg.len(), 2);

        reg.rollback(5001);
        reg.rollback(5002);
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn external_lock_without_engine_returns_invalid() {
        let _g = SERIALISE.lock();
        let _ = crate::bridge::slatedb_shutdown();

        let mut h = HaSlateDb::new();
        // Even though there's no engine, we still need a runtime to call
        // .await on. Boot it.
        if crate::runtime::get().is_none() {
            crate::runtime::init(2, 8).expect("rt");
        }
        let err = crate::runtime::block_on(h.external_lock(
            6006,
            ExternalLockType::Read,
            false,
        ))
        .unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(err.to_string().contains("no engine installed"));
    }

    #[test]
    fn ha_extra_function_from_i32_matches_mariadb_values() {
        // Pinned values from include/my_base.h:133.
        assert_eq!(HaExtraFunction::from_i32(7), HaExtraFunction::Keyread);
        assert_eq!(HaExtraFunction::from_i32(8), HaExtraFunction::NoKeyread);
        assert_eq!(HaExtraFunction::from_i32(22), HaExtraFunction::Flush);
        assert_eq!(
            HaExtraFunction::from_i32(26),
            HaExtraFunction::NoIgnoreDupKey,
        );
        assert_eq!(
            HaExtraFunction::from_i32(41),
            HaExtraFunction::InsertWithUpdate,
        );
        // Anything else → Other.
        for v in [0, 1, 2, 3, 4, 5, 6, 9, 10, 21, 23, 24, 25, 27, 40, 42, 99, -5] {
            assert_eq!(
                HaExtraFunction::from_i32(v),
                HaExtraFunction::Other,
                "{v} should be Other",
            );
        }
    }

    // ----- load_auto_incr_value / load_hidden_pk_value -----

    #[test]
    fn load_auto_incr_value_picks_up_persisted_dict_entry() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.t_ai");

        // Persist a counter to the autoinc dict slot. The PK is
        // pk(100, 1) per `install_engine_with_fixture`, so the gl is
        // `{cf_id:1, index_id:100}`.
        let db = crate::bridge::current_engine().expect("engine").db().clone();
        let gl = crate::globals::GlIndexId {
            cf_id: 1,
            index_id: 100,
        };
        crate::runtime::block_on(async {
            crate::codec::dict::autoinc::write(&db, gl, 5_000)
                .await
                .expect("write autoinc");
        });

        // Open primes the counter from the dict.
        let mut h = HaSlateDb::new();
        h.open("./appdb/t_ai").expect("open");
        assert_eq!(h.tbl_def().unwrap().auto_incr_val(), 5_000);
        // Hidden-PK counter stays untouched (explicit-PK table).
        assert_eq!(h.tbl_def().unwrap().hidden_pk_val(), 0);

        h.close().unwrap();
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn load_auto_incr_value_is_noop_when_dict_has_no_entry() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.t_empty");

        let mut h = HaSlateDb::new();
        h.open("./appdb/t_empty").expect("open");
        // Fresh table — no dict entry — counter stays at default 0.
        assert_eq!(h.tbl_def().unwrap().auto_incr_val(), 0);
        assert_eq!(h.tbl_def().unwrap().hidden_pk_val(), 0);

        h.close().unwrap();
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn load_hidden_pk_value_picks_up_persisted_dict_entry() {
        let _g = SERIALISE.lock();
        install_engine_with_keys("appdb.t_hpk", vec![hidden_pk(200, 2)]);

        let db = crate::bridge::current_engine().expect("engine").db().clone();
        let gl = crate::globals::GlIndexId {
            cf_id: 2,
            index_id: 200,
        };
        crate::runtime::block_on(async {
            crate::codec::dict::autoinc::write(&db, gl, 42_000)
                .await
                .expect("write autoinc");
        });

        let mut h = HaSlateDb::new();
        h.open("./appdb/t_hpk").expect("open");
        // Hidden-PK counter primed from the same autoinc slot.
        assert_eq!(h.tbl_def().unwrap().hidden_pk_val(), 42_000);
        // Auto-incr counter stays at 0 — open routes to the hidden-pk
        // loader for hidden-PK tables.
        assert_eq!(h.tbl_def().unwrap().auto_incr_val(), 0);

        h.close().unwrap();
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn load_hidden_pk_value_is_noop_on_explicit_pk_table() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.t_explicit");

        // Even if the dict slot has a value, an explicit-PK table's
        // `load_hidden_pk_value` skips the load entirely (early
        // return on `!has_hidden_pk()`).
        let db = crate::bridge::current_engine().expect("engine").db().clone();
        let gl = crate::globals::GlIndexId {
            cf_id: 1,
            index_id: 100,
        };
        crate::runtime::block_on(async {
            crate::codec::dict::autoinc::write(&db, gl, 9_999)
                .await
                .expect("write autoinc");
        });

        let mut h = HaSlateDb::new();
        h.open("./appdb/t_explicit").expect("open");
        // open() routed to load_auto_incr_value (explicit-PK).
        assert_eq!(h.tbl_def().unwrap().auto_incr_val(), 9_999);
        assert_eq!(h.tbl_def().unwrap().hidden_pk_val(), 0);

        // Direct call to load_hidden_pk_value is also a no-op.
        crate::runtime::block_on(async {
            h.load_hidden_pk_value(&db).await.expect("noop");
        });
        assert_eq!(h.tbl_def().unwrap().hidden_pk_val(), 0);

        h.close().unwrap();
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn load_auto_incr_value_errors_when_handler_not_open() {
        let _g = SERIALISE.lock();
        // Need an engine for the &Db; the function bails before
        // touching it on the not-open check.
        install_engine_with_fixture("appdb.t_closed");
        let db = crate::bridge::current_engine().expect("engine").db().clone();

        let h = HaSlateDb::new();
        let err = crate::runtime::block_on(async {
            h.load_auto_incr_value(&db).await
        })
        .unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(err.to_string().contains("handler not open"));

        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn load_auto_incr_value_only_bumps_upward() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.t_ratchet");

        let mut h = HaSlateDb::new();
        h.open("./appdb/t_ratchet").expect("open");
        // Force the in-memory counter higher than what we'll persist.
        h.tbl_def().unwrap().store_auto_incr_val(10_000);

        let db = crate::bridge::current_engine().expect("engine").db().clone();
        let gl = crate::globals::GlIndexId {
            cf_id: 1,
            index_id: 100,
        };
        crate::runtime::block_on(async {
            crate::codec::dict::autoinc::write(&db, gl, 500)
                .await
                .expect("write");
            h.load_auto_incr_value(&db).await.expect("load");
        });
        // fetch_max: smaller dict value does NOT lower the counter.
        assert_eq!(h.tbl_def().unwrap().auto_incr_val(), 10_000);

        h.close().unwrap();
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    // ----- pack_hidden_pk_row_key -----

    #[test]
    fn pack_hidden_pk_row_key_writes_prefix_plus_rowid() {
        let _g = SERIALISE.lock();
        install_engine_with_keys("appdb.t_hpk_pack", vec![hidden_pk(200, 2)]);

        let mut h = HaSlateDb::new();
        h.open("./appdb/t_hpk_pack").expect("open");

        let mut buf = [0u8; 16];
        let n = h.pack_hidden_pk_row_key(0x0102_0304_0506_0708, &mut buf).expect("pack");
        assert_eq!(n, 12);
        // index_number=200 → u32_be(0x000000c8)
        assert_eq!(&buf[..4], &[0x00, 0x00, 0x00, 0xc8]);
        assert_eq!(
            &buf[4..12],
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08],
        );
        // Trailing bytes untouched.
        assert_eq!(&buf[12..], &[0u8; 4]);

        h.close().unwrap();
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn pack_hidden_pk_row_key_round_trips_with_decoder() {
        let _g = SERIALISE.lock();
        install_engine_with_keys("appdb.t_hpk_round", vec![hidden_pk(0xCAFE, 2)]);

        let mut h = HaSlateDb::new();
        h.open("./appdb/t_hpk_round").expect("open");

        let mut buf = [0u8; 12];
        h.pack_hidden_pk_row_key(-7, &mut buf).expect("pack");
        // Negative rowid round-trips via i64 → u64 → i64 cast pair.
        let decoded = HaSlateDb::read_hidden_pk_id_from_rowkey(&buf).expect("decode");
        assert_eq!(decoded, -7);

        h.close().unwrap();
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn pack_hidden_pk_row_key_rejects_closed_handler() {
        let h = HaSlateDb::new();
        let mut buf = [0u8; 12];
        let err = h.pack_hidden_pk_row_key(1, &mut buf).unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
    }

    #[test]
    fn pack_hidden_pk_row_key_rejects_explicit_pk_table() {
        let _g = SERIALISE.lock();
        install_engine_with_fixture("appdb.t_explicit_pack");

        let mut h = HaSlateDb::new();
        h.open("./appdb/t_explicit_pack").expect("open");

        let mut buf = [0u8; 12];
        let err = h.pack_hidden_pk_row_key(1, &mut buf).unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(err.to_string().contains("no hidden PK"));

        h.close().unwrap();
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }

    #[test]
    fn pack_hidden_pk_row_key_rejects_short_dst() {
        let _g = SERIALISE.lock();
        install_engine_with_keys("appdb.t_hpk_short", vec![hidden_pk(7, 2)]);

        let mut h = HaSlateDb::new();
        h.open("./appdb/t_hpk_short").expect("open");

        let mut buf = [0u8; 8]; // less than 12 required
        let err = h.pack_hidden_pk_row_key(1, &mut buf).unwrap_err();
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);
        assert!(err.to_string().contains("dst too short"));

        h.close().unwrap();
        assert_eq!(crate::bridge::slatedb_shutdown(), status::OK);
    }
}
