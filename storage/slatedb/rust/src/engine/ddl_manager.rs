//! DDL manager — table-name / index-id catalogue.
//!
//! Translated from `Rdb_ddl_manager` (`rdb_datadic.{h,cc}`).
//!
//! Owns the in-memory catalogue that maps:
//! - `dbname.tablename` (or `dbname.tablename#P#part`) → [`TblDef`]
//! - [`GlIndexId`] → the `(table_name, index_in_table)` pair that
//!   locates the [`KeyDef`]
//! - [`GlIndexId`] → uncommitted `KeyDef` (mid-ALTER staging area)
//!
//! …plus the sequence generator for fresh `index_number` allocation.
//!
//! ## What's translated
//!
//! All the pure in-memory state management:
//! - [`DdlManager::put`] — install (or replace) a TblDef in the
//!   catalogue and refresh `index_num_to_keydef` for its keys.
//! - [`DdlManager::find`] — look up by table name. Returns an `Arc`
//!   clone so the caller can outlive the read lock without the C++'s
//!   manual lifecycle dance.
//! - [`DdlManager::find_key_by_id`] /
//!   [`DdlManager::safe_get_table_name`] — `GlIndexId` lookups
//!   matching the C++ `safe_find` / `safe_get_table_name`. `find_key`
//!   gates KeyDef visibility on `maxlength != 0` (matches the
//!   C++ `kd->max_storage_fmt_length() != 0` check at
//!   `rdb_datadic.cc:4251`).
//! - [`DdlManager::remove`] — in-memory removal.
//! - [`DdlManager::rename`] — clones a TblDef under a new name,
//!   transferring KeyDef refs (cheap with `Arc<KeyDef>` — no manual
//!   raw-pointer null-out like the C++).
//! - [`DdlManager::add_uncommitted_keydefs`] /
//!   [`remove_uncommitted_keydefs`] — for the ALTER staging.
//! - [`DdlManager::erase_index_num`].
//! - [`SeqGenerator`] — fresh-index-id allocator (without the
//!   dict-write side effect; that part lands with `put_and_write`).
//!
//! ## What's deferred
//!
//! - **`init` (load from dict)** — needs `dict_manager` to scan the
//!   DDL entries (`DataDictType::DdlEntryIndexStartNumber`) plus the
//!   `cf_flags` / `index_info` rows for each table. Lands when we
//!   wire dict scanning + per-table reconstitution.
//! - **`put_and_write`** — calls `put` + `TblDef::put_dict`. Gated on
//!   `put_dict` (which itself needs the `cf_flags` dict consumer).
//! - **`set_stats` / `adjust_stats` / `persist_stats`** — stats
//!   handling via `m_stats2store`. The `index_statistics` dict
//!   consumer is landed; this layer's stats queue / drain isn't.
//! - **`scan_for_tables`** — visitor pattern. Add when a consumer
//!   needs it; today nothing does.
//! - **`validate_schemas` / `validate_auto_incr`** — startup
//!   integrity checks; land with `init`.
//!
//! ## Concurrency
//!
//! C++ uses a `mysql_rwlock_t`. We use `parking_lot::RwLock` for the
//! state map and a `parking_lot::Mutex` for the sequence generator.
//! Find returns an `Arc<TblDef>` so callers can drop the lock and
//! keep working — the C++ instead keeps its `Rdb_tbl_def*` valid by
//! holding the lock and relying on `put` deleting the old pointer
//! only after the new one is in place; same end-state, different
//! tactics.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};

use crate::codec::key::KeyDef;
use crate::codec::tbl_def::TblDef;
use crate::globals::GlIndexId;

/// Where a KeyDef lives within the catalogue: which table, which
/// position in that table's `key_descrs` array. Mirrors the C++
/// `std::pair<std::string, uint>` value of `m_index_num_to_keydef`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexLocation {
    pub table_name: String,
    pub index_in_table: u32,
}

struct DdlManagerState {
    /// name (`dbname.tablename[#P#part]`) → table descriptor.
    ddl_map: HashMap<String, Arc<TblDef>>,
    /// `GlIndexId` → location in the catalogue. Refreshed whenever
    /// `put` installs a TblDef. `BTreeMap` (not `HashMap`) to match
    /// the C++ `std::map` choice — ordered iteration is occasionally
    /// useful for diagnostics.
    index_num_to_keydef: BTreeMap<GlIndexId, IndexLocation>,
    /// Mid-ALTER staging — KeyDefs whose index ids have been
    /// allocated but whose table definition isn't committed yet.
    /// Populated by `prepare_inplace_alter_table`
    /// (`ha_rocksdb.cc:12637`); consumed by
    /// `commit_inplace_alter_table` / rollback.
    index_num_to_uncommitted_keydef: BTreeMap<GlIndexId, Arc<KeyDef>>,
}

impl DdlManagerState {
    fn new() -> Self {
        Self {
            ddl_map: HashMap::new(),
            index_num_to_keydef: BTreeMap::new(),
            index_num_to_uncommitted_keydef: BTreeMap::new(),
        }
    }
}

/// Fresh `index_number` allocator. The on-disk persistence of the
/// `max_index_id` watermark (the C++'s `dict->update_max_index_id`)
/// lands when [`DdlManager`] gains dict integration; today this is
/// the in-memory portion only.
pub struct SeqGenerator {
    next_number: u32,
}

impl SeqGenerator {
    pub fn new(initial_number: u32) -> Self {
        Self { next_number: initial_number }
    }

    /// Return the current value and increment. Matches the C++
    /// `m_next_number++` (post-increment).
    pub fn get_and_update(&mut self) -> u32 {
        let res = self.next_number;
        self.next_number = self.next_number.saturating_add(1);
        res
    }

    /// Peek the next value without consuming it. Used by tests.
    pub fn peek(&self) -> u32 {
        self.next_number
    }
}

pub struct DdlManager {
    state: RwLock<DdlManagerState>,
    sequence: Mutex<SeqGenerator>,
}

impl DdlManager {
    /// Empty catalogue. The C++ uses `Rdb_ddl_manager::init` to load
    /// from the dict; that's deferred, so this constructor is what
    /// callers use today (typically followed by `put` calls to install
    /// hand-built TblDefs in tests, or a future `init` once that lands).
    pub fn new() -> Self {
        Self {
            state: RwLock::new(DdlManagerState::new()),
            sequence: Mutex::new(SeqGenerator::new(1)),
        }
    }

    /// Construct with a specific initial `index_number` — useful when
    /// `init` (load-from-dict) reads the persisted `max_index_id` and
    /// seeds the sequence past it.
    pub fn with_initial_index_id(initial: u32) -> Self {
        Self {
            state: RwLock::new(DdlManagerState::new()),
            sequence: Mutex::new(SeqGenerator::new(initial)),
        }
    }

    // ----- table-name lookup -----

    /// Look up a `TblDef` by its full name (`dbname.tablename` or
    /// `dbname.tablename#P#part`). Returns an `Arc` clone so the
    /// caller can outlive the read lock.
    pub fn find(&self, table_name: &str) -> Option<Arc<TblDef>> {
        let st = self.state.read();
        st.ddl_map.get(table_name).cloned()
    }

    // ----- gl_index_id lookup -----

    /// Resolve a `GlIndexId` to its `KeyDef` (whether committed or
    /// uncommitted), gated on `KeyDef::maxlength != 0` — i.e. the
    /// KeyDef has been through `setup`. Matches the C++ `safe_find`
    /// at `rdb_datadic.cc:4240`.
    pub fn find_key_by_id(&self, gl_index_id: GlIndexId) -> Option<Arc<KeyDef>> {
        let st = self.state.read();
        if let Some(loc) = st.index_num_to_keydef.get(&gl_index_id) {
            let tdef = st.ddl_map.get(&loc.table_name)?;
            let kd = tdef.key(loc.index_in_table as usize)?;
            if kd.maxlength != 0 {
                return Some(kd.clone());
            }
            return None;
        }
        // Fall back to the uncommitted-keydef map.
        if let Some(kd) = st.index_num_to_uncommitted_keydef.get(&gl_index_id) {
            if kd.maxlength != 0 {
                return Some(kd.clone());
            }
        }
        None
    }

    /// Resolve a `GlIndexId` to its owning table's full name. Matches
    /// the C++ `safe_get_table_name` at `rdb_datadic.cc:4295`. Returns
    /// `None` if the id isn't in the committed map (the uncommitted
    /// map doesn't carry a table-name back-reference).
    pub fn safe_get_table_name(&self, gl_index_id: GlIndexId) -> Option<String> {
        let st = self.state.read();
        st.index_num_to_keydef
            .get(&gl_index_id)
            .map(|loc| loc.table_name.clone())
    }

    // ----- mutating operations -----

    /// Install (or replace) a `TblDef`. Updates `index_num_to_keydef`
    /// to point at the new descriptor's KeyDefs. C++ `put` at
    /// `rdb_datadic.cc:4394`.
    ///
    /// Replacement semantics: if a TblDef with the same full name
    /// already exists, it's dropped from the catalogue. Any
    /// `Arc<TblDef>` clones held outside the catalogue stay valid
    /// (they were the C++'s "concurrent reader holds the lock" case
    /// — we get the same safety via refcounting).
    pub fn put(&self, tbl: Arc<TblDef>) {
        // Snapshot the new TblDef's key locations under the lock.
        let new_locations: Vec<(GlIndexId, IndexLocation)> = tbl
            .key_descrs()
            .iter()
            .enumerate()
            .map(|(i, kd)| {
                (
                    kd.get_gl_index_id(),
                    IndexLocation {
                        table_name: tbl.full_tablename().to_owned(),
                        index_in_table: i as u32,
                    },
                )
            })
            .collect();

        let mut st = self.state.write();
        // Drop the old entry first so any stale GlIndexId → location
        // entries that pointed at it get cleared. (The new locations
        // for the same GlIndexIds will be reinstated below; this just
        // catches indexes that the new TblDef *removed*.)
        if let Some(old) = st.ddl_map.remove(tbl.full_tablename()) {
            for kd in old.key_descrs() {
                st.index_num_to_keydef.remove(&kd.get_gl_index_id());
            }
        }
        st.ddl_map.insert(tbl.full_tablename().to_owned(), tbl.clone());
        for (gl, loc) in new_locations {
            st.index_num_to_keydef.insert(gl, loc);
        }
        // C++: tbl->check_and_set_read_free_rpl_table();
        tbl.check_and_set_read_free_rpl_table();
    }

    /// Drop a `TblDef` from the catalogue (in-memory only). The C++
    /// version also writes a tombstone to the DDL entry via
    /// `m_dict->delete_key` and frees the Rdb_tbl_def*; the dict
    /// write is deferred along with `put_and_write`, and Arc handles
    /// the free.
    pub fn remove(&self, table_name: &str) -> Option<Arc<TblDef>> {
        let mut st = self.state.write();
        let removed = st.ddl_map.remove(table_name)?;
        for kd in removed.key_descrs() {
            st.index_num_to_keydef.remove(&kd.get_gl_index_id());
        }
        Some(removed)
    }

    /// Rename a `TblDef` (in-memory). Clones the table descriptor
    /// under the new name with the same `Arc<KeyDef>` list and the
    /// same atomic counter snapshots. Returns `true` on success,
    /// `false` if the source name doesn't exist (matches C++
    /// `bool` rename's "return true == error" convention).
    pub fn rename(&self, from: &str, to: &str) -> bool {
        let mut st = self.state.write();
        let Some(rec) = st.ddl_map.get(from).cloned() else {
            return false;
        };

        let new_tdef = Arc::new(
            // Construction error is structural — `to` must be a valid
            // `db.tbl[#P#part]` name; the caller has already validated.
            // We surface a failure return rather than panicking.
            match TblDef::new(to) {
                Ok(t) => {
                    let t = t.with_keys(rec.key_descrs().to_vec());
                    t.store_hidden_pk_val(rec.hidden_pk_val());
                    t.store_auto_incr_val(rec.auto_incr_val());
                    t.store_update_time(rec.update_time());
                    if rec.is_read_free_rpl_table() {
                        // We re-evaluate via the post-construction
                        // hook; for now mirror the C++ field-copy.
                        // check_and_set_read_free_rpl_table pins it
                        // to false today anyway (MARIAROCKS_NOT_YET),
                        // so this is effectively a no-op until that
                        // surface lights up.
                        t.check_and_set_read_free_rpl_table();
                    }
                    t
                }
                Err(_) => return false,
            },
        );

        // Drop the old name, install the new one.
        if let Some(_old) = st.ddl_map.remove(from) {
            for kd in _old.key_descrs() {
                st.index_num_to_keydef.remove(&kd.get_gl_index_id());
            }
        }

        let new_locations: Vec<(GlIndexId, IndexLocation)> = new_tdef
            .key_descrs()
            .iter()
            .enumerate()
            .map(|(i, kd)| {
                (
                    kd.get_gl_index_id(),
                    IndexLocation {
                        table_name: to.to_owned(),
                        index_in_table: i as u32,
                    },
                )
            })
            .collect();
        st.ddl_map.insert(to.to_owned(), new_tdef);
        for (gl, loc) in new_locations {
            st.index_num_to_keydef.insert(gl, loc);
        }
        true
    }

    // ----- uncommitted-keydef map -----

    /// Stage KeyDefs as "uncommitted" — `safe_find` will resolve their
    /// GlIndexIds until they're either committed (moved into a TblDef
    /// via `put`) or removed via [`remove_uncommitted_keydefs`].
    /// C++ `add_uncommitted_keydefs` at `rdb_datadic.cc:3740`.
    pub fn add_uncommitted_keydefs(&self, indexes: &[Arc<KeyDef>]) {
        let mut st = self.state.write();
        for kd in indexes {
            st.index_num_to_uncommitted_keydef
                .insert(kd.get_gl_index_id(), kd.clone());
        }
    }

    /// Inverse of [`add_uncommitted_keydefs`].
    pub fn remove_uncommitted_keydefs(&self, indexes: &[Arc<KeyDef>]) {
        let mut st = self.state.write();
        for kd in indexes {
            st.index_num_to_uncommitted_keydef
                .remove(&kd.get_gl_index_id());
        }
    }

    /// Remove a single index from the committed map. The C++
    /// `erase_index_num` doesn't take the lock (it expects the caller
    /// to hold one); we acquire a write lock here for safety.
    pub fn erase_index_num(&self, gl_index_id: GlIndexId) {
        let mut st = self.state.write();
        st.index_num_to_keydef.remove(&gl_index_id);
    }

    // ----- dict-integrated installation -----

    /// Persist a `TblDef` to the dict and install it in the in-memory
    /// catalogue. Composition of [`TblDef::put_dict`] + [`Self::put`].
    /// Matches C++ `Rdb_ddl_manager::put_and_write` at
    /// `rdb_datadic.cc:4368`.
    ///
    /// If the dict write fails (e.g. `cf_flags` mismatch), the
    /// in-memory catalogue is NOT touched — same ordering as the C++
    /// (which writes first, then calls `put`).
    pub async fn put_and_write(
        &self,
        tbl: Arc<TblDef>,
        db: &slatedb::Db,
    ) -> Result<(), slatedb::Error> {
        tbl.put_dict(db).await?;
        self.put(tbl);
        Ok(())
    }

    // ----- sequence generator passthrough -----

    /// Allocate a fresh `index_number`. The on-disk `max_index_id`
    /// watermark update (C++ `dict->update_max_index_id` inside
    /// `Rdb_seq_generator::get_and_update_next_number`) is deferred
    /// to the dict-integration commit; today this is in-memory only.
    pub fn get_and_update_next_number(&self) -> u32 {
        self.sequence.lock().get_and_update()
    }

    #[cfg(test)]
    fn next_number_peek(&self) -> u32 {
        self.sequence.lock().peek()
    }

    #[cfg(test)]
    fn committed_index_count(&self) -> usize {
        self.state.read().index_num_to_keydef.len()
    }

    #[cfg(test)]
    fn uncommitted_index_count(&self) -> usize {
        self.state.read().index_num_to_uncommitted_keydef.len()
    }
}

impl Default for DdlManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::key::{
        IndexType, INDEX_INFO_VERSION_LATEST, PRIMARY_FORMAT_VERSION_LATEST,
        SECONDARY_FORMAT_VERSION_LATEST,
    };

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
        // Pretend setup ran — flip maxlength so find_key_by_id
        // returns it. Tests that exercise the "skeleton" gate
        // construct their own KeyDefs.
        kd.maxlength = 12;
        Arc::new(kd)
    }

    fn pk_skeleton(index_number: u32, cf_id: u32) -> Arc<KeyDef> {
        Arc::new(KeyDef::new_skeleton(
            index_number,
            cf_id,
            0,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Primary,
            PRIMARY_FORMAT_VERSION_LATEST,
            false,
            "pk",
        ))
    }

    fn sk(index_number: u32, cf_id: u32, name: &str) -> Arc<KeyDef> {
        let mut kd = KeyDef::new_skeleton(
            index_number,
            cf_id,
            1,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Secondary,
            SECONDARY_FORMAT_VERSION_LATEST,
            false,
            name,
        );
        kd.maxlength = 16;
        Arc::new(kd)
    }

    fn tdef(name: &str, keys: Vec<Arc<KeyDef>>) -> Arc<TblDef> {
        Arc::new(TblDef::new(name).unwrap().with_keys(keys))
    }

    #[test]
    fn new_starts_empty_with_seq_at_1() {
        let m = DdlManager::new();
        assert!(m.find("anything").is_none());
        assert_eq!(m.next_number_peek(), 1);
        assert_eq!(m.committed_index_count(), 0);
    }

    #[test]
    fn put_round_trip_installs_table_and_indexes() {
        let m = DdlManager::new();
        let t = tdef("db.users", vec![pk(100, 1), sk(101, 1, "by_email")]);
        m.put(t.clone());

        let got = m.find("db.users").expect("present");
        assert!(Arc::ptr_eq(&got, &t));

        // Both KeyDefs are now reachable by their GlIndexId.
        let pk_loc = m
            .find_key_by_id(GlIndexId { cf_id: 1, index_id: 100 })
            .expect("PK by id");
        assert!(Arc::ptr_eq(&pk_loc, &t.key_descrs()[0]));
        let sk_loc = m
            .find_key_by_id(GlIndexId { cf_id: 1, index_id: 101 })
            .expect("SK by id");
        assert!(Arc::ptr_eq(&sk_loc, &t.key_descrs()[1]));

        assert_eq!(
            m.safe_get_table_name(GlIndexId { cf_id: 1, index_id: 100 }),
            Some("db.users".to_owned()),
        );
        assert_eq!(m.committed_index_count(), 2);
    }

    #[test]
    fn put_replacement_drops_old_index_entries() {
        let m = DdlManager::new();
        let old = tdef("db.t", vec![pk(100, 1), sk(101, 1, "old_sk")]);
        m.put(old);

        // Replace with a TblDef that has different keys — old SK
        // (index_id=101) must disappear from index_num_to_keydef;
        // new SK (index_id=200) must appear.
        let new = tdef("db.t", vec![pk(100, 1), sk(200, 1, "new_sk")]);
        m.put(new);

        assert!(m
            .find_key_by_id(GlIndexId { cf_id: 1, index_id: 101 })
            .is_none());
        assert!(m
            .find_key_by_id(GlIndexId { cf_id: 1, index_id: 200 })
            .is_some());
        assert_eq!(m.committed_index_count(), 2);
    }

    #[test]
    fn find_key_by_id_skips_keydef_that_isnt_set_up_yet() {
        // maxlength == 0 ⇒ KeyDef not through setup() ⇒ not visible.
        let m = DdlManager::new();
        let t = tdef("db.t", vec![pk_skeleton(100, 1)]);
        m.put(t);
        assert!(m
            .find_key_by_id(GlIndexId { cf_id: 1, index_id: 100 })
            .is_none());
    }

    #[test]
    fn safe_get_table_name_misses_on_unknown_id() {
        let m = DdlManager::new();
        m.put(tdef("db.t", vec![pk(100, 1)]));
        assert_eq!(
            m.safe_get_table_name(GlIndexId { cf_id: 9, index_id: 9 }),
            None,
        );
    }

    #[test]
    fn remove_clears_table_and_index_entries() {
        let m = DdlManager::new();
        m.put(tdef("db.t", vec![pk(100, 1), sk(101, 1, "s")]));
        assert_eq!(m.committed_index_count(), 2);

        let removed = m.remove("db.t").expect("removed");
        assert_eq!(removed.full_tablename(), "db.t");

        assert!(m.find("db.t").is_none());
        assert_eq!(m.committed_index_count(), 0);
        // Removing again is a miss.
        assert!(m.remove("db.t").is_none());
    }

    #[test]
    fn rename_moves_keys_under_new_name() {
        let m = DdlManager::new();
        let original = tdef("db.t_old", vec![pk(100, 1), sk(101, 1, "s")]);
        // Stamp auto_incr / hidden_pk so we can verify they're copied.
        original.store_auto_incr_val(42);
        original.store_hidden_pk_val(7);
        m.put(original);

        assert!(m.rename("db.t_old", "db.t_new"));

        assert!(m.find("db.t_old").is_none());
        let new = m.find("db.t_new").expect("renamed present");
        assert_eq!(new.auto_incr_val(), 42);
        assert_eq!(new.hidden_pk_val(), 7);

        // The same GlIndexIds now resolve to db.t_new.
        assert_eq!(
            m.safe_get_table_name(GlIndexId { cf_id: 1, index_id: 100 }),
            Some("db.t_new".to_owned()),
        );
    }

    #[test]
    fn rename_missing_source_returns_false() {
        let m = DdlManager::new();
        assert!(!m.rename("db.absent", "db.target"));
    }

    #[test]
    fn rename_with_malformed_target_returns_false() {
        let m = DdlManager::new();
        m.put(tdef("db.t", vec![pk(100, 1)]));
        // "no_dot" isn't a valid db.tbl name.
        assert!(!m.rename("db.t", "no_dot"));
        // Source untouched.
        assert!(m.find("db.t").is_some());
    }

    #[test]
    fn uncommitted_keydefs_resolve_via_find_key_by_id() {
        let m = DdlManager::new();
        let staged = sk(500, 1, "alter_staged");
        m.add_uncommitted_keydefs(&[staged.clone()]);

        let got = m
            .find_key_by_id(GlIndexId { cf_id: 1, index_id: 500 })
            .expect("present via uncommitted");
        assert!(Arc::ptr_eq(&got, &staged));
        assert_eq!(m.uncommitted_index_count(), 1);

        m.remove_uncommitted_keydefs(&[staged]);
        assert!(m
            .find_key_by_id(GlIndexId { cf_id: 1, index_id: 500 })
            .is_none());
        assert_eq!(m.uncommitted_index_count(), 0);
    }

    #[test]
    fn committed_keydef_takes_precedence_over_uncommitted_with_same_id() {
        let m = DdlManager::new();
        let id = GlIndexId { cf_id: 1, index_id: 700 };

        // Stage an uncommitted KeyDef with one name.
        let staged = sk(700, 1, "staged");
        m.add_uncommitted_keydefs(&[staged]);

        // Commit a different KeyDef (same id, different name) via a
        // TblDef. The C++ checks committed first.
        let committed = sk(700, 1, "committed");
        m.put(tdef("db.t", vec![pk(99, 1), committed.clone()]));

        let got = m.find_key_by_id(id).expect("present");
        assert!(Arc::ptr_eq(&got, &committed));
    }

    #[test]
    fn erase_index_num_clears_only_committed_entry() {
        let m = DdlManager::new();
        m.put(tdef("db.t", vec![pk(100, 1), sk(101, 1, "s")]));
        m.erase_index_num(GlIndexId { cf_id: 1, index_id: 101 });

        assert!(m
            .find_key_by_id(GlIndexId { cf_id: 1, index_id: 101 })
            .is_none());
        // PK still there.
        assert!(m
            .find_key_by_id(GlIndexId { cf_id: 1, index_id: 100 })
            .is_some());
    }

    #[test]
    fn seq_generator_is_monotonic_and_thread_safe() {
        use std::sync::Arc as StdArc;
        let m = StdArc::new(DdlManager::with_initial_index_id(100));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let m = m.clone();
                std::thread::spawn(move || {
                    let mut out = Vec::new();
                    for _ in 0..50 {
                        out.push(m.get_and_update_next_number());
                    }
                    out
                })
            })
            .collect();
        let mut all: Vec<u32> = handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect();
        all.sort_unstable();
        // 8 * 50 = 400 unique values starting from 100.
        assert_eq!(all.len(), 400);
        assert_eq!(all[0], 100);
        assert_eq!(all[399], 100 + 399);
        // No duplicates.
        let dedup: std::collections::HashSet<_> = all.iter().copied().collect();
        assert_eq!(dedup.len(), 400);
    }

    #[test]
    fn ddl_manager_is_send_sync() {
        // Callers expect to hand it around behind Arc to handler threads.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DdlManager>();
        assert_send_sync::<Arc<DdlManager>>();
    }

    // ----- put_and_write integration -----

    use crate::codec::dict::ddl_entry_index_start_number;
    use crate::engine::db::EngineDb;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn put_and_write_installs_catalogue_and_persists_to_dict() {
        let engine = EngineDb::open_in_memory("paw_ok").await.expect("open");
        let m = DdlManager::new();

        let tdef = Arc::new(
            TblDef::new("db.users")
                .unwrap()
                .with_keys(vec![pk(100, 1), sk(101, 1, "by_email")]),
        );

        m.put_and_write(tdef.clone(), engine.db())
            .await
            .expect("put_and_write");

        // In-memory catalogue carries the table.
        let found = m.find("db.users").expect("present");
        assert!(Arc::ptr_eq(&found, &tdef));

        // Dict was written: typed DDL-entry decodes to the same id list.
        let ddl_entry =
            ddl_entry_index_start_number::read_typed(engine.db(), "db.users")
                .await
                .expect("read ddl entry")
                .expect("present");
        assert_eq!(
            ddl_entry,
            vec![
                GlIndexId { cf_id: 1, index_id: 100 },
                GlIndexId { cf_id: 1, index_id: 101 },
            ],
        );

        engine.close().await.expect("close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn put_and_write_aborts_on_dict_failure_leaves_catalogue_clean() {
        let engine = EngineDb::open_in_memory("paw_fail").await.expect("open");
        let m = DdlManager::new();

        // Establish cf=7 with flags=0 (no reverse).
        let plain = Arc::new(
            TblDef::new("db.t1")
                .unwrap()
                .with_keys(vec![pk(100, 7)]),
        );
        m.put_and_write(plain, engine.db()).await.expect("t1");

        // Build a t2 KeyDef that flips REVERSE_CF_FLAG on the same cf=7.
        // put_dict must reject it; catalogue must stay clean.
        let mut reverse_kd = KeyDef::new_skeleton(
            200,
            7,
            0,
            INDEX_INFO_VERSION_LATEST as u16,
            IndexType::Primary,
            PRIMARY_FORMAT_VERSION_LATEST,
            true, // is_reverse_cf
            "pk",
        );
        reverse_kd.maxlength = 12;
        let t2 = Arc::new(
            TblDef::new("db.t2")
                .unwrap()
                .with_keys(vec![Arc::new(reverse_kd)]),
        );
        let err = m
            .put_and_write(t2, engine.db())
            .await
            .expect_err("should fail");
        assert_eq!(err.kind(), slatedb::ErrorKind::Invalid);

        // No db.t2 entry in the catalogue.
        assert!(m.find("db.t2").is_none());

        engine.close().await.expect("close");
    }
}
