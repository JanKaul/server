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
        Self { tbl_def: None }
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
        let tbl_def = ddl.find(&normalized).ok_or_else(|| {
            Error::data(format!(
                "HaSlateDb::open: no catalogue entry for table {normalized:?}"
            ))
        })?;
        self.tbl_def = Some(tbl_def);
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

    /// Install an engine and put a fixture table in its catalogue.
    /// Tests that need a bound handler use this.
    ///
    /// Synchronous wrapper because the bridge installs its own tokio
    /// runtime; nesting a `#[tokio::test]` runtime inside would
    /// panic with "cannot start a runtime from within a runtime".
    fn install_engine_with_fixture(name_in_dot: &str) {
        // Clean slate.
        let _ = crate::bridge::slatedb_shutdown();
        assert_eq!(
            crate::bridge::slatedb_init_in_memory(format!("handler_test_{name_in_dot}")),
            status::OK,
            "init",
        );
        // Install via the live engine reference, blocking on the
        // bridge's runtime for the async put_and_write call.
        let db = crate::bridge::current_engine().expect("engine just installed");
        let ddl = crate::bridge::current_ddl().expect("ddl just installed");
        let tdef =
            Arc::new(TblDef::new(name_in_dot).unwrap().with_keys(vec![pk(100, 1)]));
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
}
