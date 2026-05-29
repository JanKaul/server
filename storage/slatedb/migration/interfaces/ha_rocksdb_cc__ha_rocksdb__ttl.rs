//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__ttl`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (span 6399..13838, body ~90 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__ttl`
//!
//! ## Mapping
//!
//! **The whole bucket collapses to thin adapters.** Per _DESIGN.md §1 row
//! "Compaction filters (TTL)" + §3:
//!
//! - MyRocks embedded a TTL timestamp in the value bytes and then ran an
//!   external filter (`should_hide_ttl_rec`) on every iterator step plus a
//!   compaction-time sweep.
//! - SlateDB has **native TTL** via `PutOptions::ttl: Ttl` and stores
//!   `expire_ts: Option<i64>` on each `RowEntry` (`config.rs:553`,
//!   `types.rs`). Expired entries are filtered by SlateDB's iterators
//!   internally — we never see them.
//!
//! So the Rust equivalents are essentially **no-ops that exist only to keep
//! call sites in the read/scan/iter_setup buckets compiling unchanged**:
//!
//! - `should_hide_ttl_rec`     → always `Ok(false)` (SlateDB already filtered)
//! - `rocksdb_skip_expired_records` → always `Ok(false)` (no extra scan needed)
//! - `should_skip_invalidated_record` → preserves the MyRocks semantics
//!   (READ_COMMITTED race with concurrent DELETE) because that's an
//!   *isolation-level* concept, not a TTL one. Still in scope.
//!
//! The TTL prefix bytes in the value codec are also dropped (see
//! `codec::value` — A1 owns that change). At write time the engine will
//! call `WriteOptions { ttl: Ttl::ExpireAfter(secs) }` per row when
//! `kd.has_ttl()`.
//!
//! ## Out-of-scope methods
//! None — all three are in scope as thin shims; their *bodies* shrink to
//! nothing because the work moved into SlateDB.

use slatedb::Error;

/// Forward-declared. Owned by A2's `ha_rocksdb_h` stub.
pub struct HaSlateDb;

impl HaSlateDb {
    /// Migration shim. In MyRocks this decoded the 8-byte TTL prefix from
    /// the value bytes and compared it to `curr_ts + ttl_duration`. With
    /// SlateDB's native `expire_ts` filter we **never see expired rows**
    /// from the iterator — so this always returns `Ok(false)` ("do not
    /// hide; the row is already known-fresh").
    ///
    /// All inputs are accepted for source-compatibility but ignored. We
    /// preserve the doc-comment naming so a future grep against the MyRocks
    /// source line-number lands here cleanly.
    ///
    /// Original C++: ha_rocksdb.cc:6399 — `bool ha_rocksdb::should_hide_ttl_rec(...)`.
    pub fn should_hide_ttl_rec(
        &self,
        _index_has_ttl: bool,
        _ttl_rec_val: &[u8],
        _curr_ts: i64,
    ) -> Result<bool, Error> {
        // Rely on SlateDB's native expire_ts filtering. The row is fresh
        // by construction if we got here from a scan/get.
        Ok(false)
    }

    /// Migration shim. In MyRocks this advanced the iterator past any
    /// expired rows; with SlateDB the iterator already does this internally
    /// for us. Returns `Ok(false)` ("no rows were skipped on our account")
    /// so the caller's "valid?" check still works.
    ///
    /// `_seek_backward` is preserved in the signature so call-site signatures
    /// match MyRocks 1:1 — but it's a no-op for us.
    ///
    /// Original C++: ha_rocksdb.cc:6465 — `int ha_rocksdb::rocksdb_skip_expired_records(...)`.
    pub fn rocksdb_skip_expired_records(
        &mut self,
        _index_has_ttl: bool,
        _seek_backward: bool,
    ) -> Result<bool, Error> {
        // SlateDB iterators already skip expired entries via expire_ts.
        Ok(false)
    }

    /// **In scope** — this method is misnamed in the TTL bucket but is
    /// really an isolation-level helper, not a TTL one. It returns true
    /// when the engine should silently treat HA_ERR_KEY_NOT_FOUND as
    /// "row was deleted by another transaction" during a SELECT FOR UPDATE
    /// under `READ COMMITTED`. The semantics map cleanly: we check our
    /// shim's `m_lock_rows` + isolation level the same way MyRocks did.
    ///
    /// Inputs:
    ///   - `rc`: the HA_ERR code that just came back from a read attempt.
    ///
    /// Output: `true` iff the caller should swallow the not-found and seek
    /// to the next row instead of returning the error to MySQL.
    ///
    /// Original C++: ha_rocksdb.cc:13832.
    pub fn should_skip_invalidated_record(&self, rc: i32) -> bool {
        let _ = rc;
        todo!("port: lock_rows != NONE && rc == HA_ERR_KEY_NOT_FOUND && tx_isolation == READ_COMMITTED")
    }
}
