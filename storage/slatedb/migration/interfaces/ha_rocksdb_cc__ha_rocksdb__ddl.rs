//! Interface stub for `ha_rocksdb_cc__ha_rocksdb__ddl`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (body LoC 561, span 7108..11912)
//! v4 manifest sub-unit: `ha_rocksdb_cc__ha_rocksdb__ddl`
//!
//! ## Mapping
//! Handler-vtable DDL bucket: CREATE TABLE, DROP TABLE, RENAME TABLE and the
//! private helpers that wire `KEY[]` arrays into our internal `Rdb_key_def`
//! shape. In MyRocks each call edits the data-dictionary CF via a
//! `rocksdb::WriteBatch`. In SlateDB we use the same shape, layered over
//! `DbTransactionOps::put/delete` against the SYSTEM CF prefix
//! (`cf_id = u32::MAX`, see _DESIGN.md §2 and `rdb_global_h::SYSTEM_CF_ID`).
//!
//! Per _DESIGN.md §1: the data-dictionary writes use a single `DbTransaction`
//! committed atomically (or `WriteBatch` for non-conflicting metadata edits) —
//! this matches MyRocks' `dict_manager.begin() ... dict_manager.commit(batch)`
//! pattern bit-for-bit.
//!
//! Per _DESIGN.md §1 (column-families row): user-visible CFs collapse to a
//! key-prefix scheme. `create_cfs` no longer registers a RocksDB ColumnFamily;
//! it only allocates a fresh `cf_id` in the dictionary if a new CF qualifier
//! is seen in a CREATE TABLE comment.
//!
//! ## Out-of-scope methods
//! None. All nine vtable methods translate to SlateDB primitives + our
//! dict_manager / ddl_manager (themselves backed by SlateDB SYSTEM CF).

use bytes::Bytes;
use slatedb::Error;

use crate::ha_rocksdb_cc__ha_rocksdb__lifecycle::{HaSlateDb, TableRef, ThdRef};

/// Forwarded to a future TABLE-shape unit. Mirrors `HA_CREATE_INFO`:
/// `AUTO_INCREMENT` start value, `COMMENT`, charset hints, and the
/// MariaDB-specific `DATA DIRECTORY` / `INDEX DIRECTORY` strings (ignored).
#[derive(Debug, Clone)]
pub struct HaCreateInfo {
    pub auto_increment_value: u64,
    pub table_comment: String,
    pub data_directory: Option<String>,
    pub index_directory: Option<String>,
}

/// Forwarded to a future TABLE-shape unit. Mirrors a `KEY` row from
/// `TABLE_SHARE::key_info[]`: index name, user-defined key parts, flags
/// (`HA_NOSAME` / `HA_NULL_PART_KEY` / ...), and comment.
#[derive(Debug, Clone)]
pub struct KeyInfo {
    pub name: String,
    pub flags: u32,
    pub comment: String,
    pub key_parts: Vec<KeyPart>,
}

/// One part of a `KeyInfo`. POD shape for the codec.
#[derive(Debug, Clone)]
pub struct KeyPart {
    pub field_index: u16,
    pub key_prefix_len: u16,
}

/// Forwarded to a future `rdb_datadic` unit (see `codec/key.rs` in
/// _DESIGN.md §8 module layout). For now an opaque placeholder.
#[derive(Debug, Clone)]
pub struct RdbKeyDef {
    pub gl_index_id: crate::rdb_global_h::GlIndexId,
}

/// Forwarded to a future `rdb_datadic` unit. Bundles per-table metadata:
/// `key_count`, `key_descr_arr`, `auto_incr_val`, `hidden_pk_val`.
#[derive(Debug, Clone)]
pub struct RdbTblDef {
    pub full_name: String,
    pub key_count: u32,
    pub key_descrs: Vec<RdbKeyDef>,
    pub auto_incr_val: u64,
    pub hidden_pk_val: u64,
}

impl HaSlateDb {
    /// `int ha_rocksdb::create_key_defs(const TABLE*, Rdb_tbl_def*,
    /// const TABLE* old_table_arg=nullptr, const Rdb_tbl_def* old_tbl_def=nullptr)`
    /// — original C++ source line 7108.
    ///
    /// Inputs: `table_arg` (new layout), `tbl_def` (output, mutated in place),
    /// optional `old_table_arg`/`old_tbl_def` for inplace-alter delta.
    /// Outputs: `Ok(())` once `tbl_def.key_descrs` is populated for every
    /// `KEY` in `table_arg`, plus the hidden PK if absent.
    /// Errors:
    /// - `ErrorKind::Invalid` if a CF qualifier comment fails to parse.
    /// - `ErrorKind::Internal` if `create_key_def` returns an unexpected
    ///   layout from the codec.
    /// Invariants: this routine does NOT write to SlateDB — it only fills the
    /// in-memory `tbl_def`. Persistence happens in `create_table`.
    pub fn create_key_defs(
        &mut self,
        table_arg: &TableRef,
        tbl_def: &mut RdbTblDef,
        old_table_arg: Option<&TableRef>,
        old_tbl_def: Option<&RdbTblDef>,
    ) -> Result<(), Error> {
        let _ = (table_arg, tbl_def, old_table_arg, old_tbl_def);
        todo!("walk table_arg.key_info; for each call create_key_def or copy from old_tbl_def")
    }

    /// `int ha_rocksdb::create_cfs(const TABLE*, Rdb_tbl_def*, std::array<...>&)`
    /// — original C++ source line 7209.
    ///
    /// Inputs: `table_arg` (new layout), `tbl_def`.
    /// Outputs: a `Vec<u32>` of `cf_id`s, one per `KEY` (PK first).
    /// Errors: `ErrorKind::Invalid` for malformed CF qualifier comment.
    /// Invariants: in SlateDB there are no physical CFs — this only allocates
    /// (and persists) a fresh `cf_id` in the dictionary when a CREATE TABLE
    /// comment names a new CF qualifier. Idempotent on existing qualifiers.
    ///
    /// Maps to: `dict_manager.lookup_or_create_cf_id(name).await` on the
    /// SYSTEM CF prefix.
    pub async fn create_cfs(
        &mut self,
        table_arg: &TableRef,
        tbl_def: &RdbTblDef,
    ) -> Result<Vec<u32>, Error> {
        let _ = (table_arg, tbl_def);
        todo!("parse cf qualifiers; for each: dict_manager.lookup_or_create_cf_id().await")
    }

    /// `int ha_rocksdb::create_inplace_key_defs(...)` — original C++ source
    /// line 7314. Used by inplace-alter to keep matching old indexes intact
    /// and only re-create the changed ones.
    ///
    /// Inputs: `altered_table`, `new_tdef`, `old_table`, `old_tdef`.
    /// Outputs: `Ok(())`; `new_tdef.key_descrs` filled with a mix of
    /// re-pointed `old_tdef` entries and freshly-created ones.
    /// Errors: same set as `create_key_defs`.
    /// Invariants: no SlateDB writes. Caller (`prepare_inplace_alter_table`)
    /// commits the dict edits via a `DbTransaction`.
    pub fn create_inplace_key_defs(
        &mut self,
        altered_table: &TableRef,
        new_tdef: &mut RdbTblDef,
        old_table: &TableRef,
        old_tdef: &RdbTblDef,
    ) -> Result<(), Error> {
        let _ = (altered_table, new_tdef, old_table, old_tdef);
        todo!("for each new key: match by KEY signature to old; clone-or-create_key_def")
    }

    /// `int ha_rocksdb::create_key_def(const TABLE*, uint i, const Rdb_tbl_def*,
    /// std::shared_ptr<Rdb_key_def>*, ...)` — original C++ source line 7522.
    ///
    /// Inputs: `table_arg`, `i` (key index in the new layout), `tbl_def`,
    /// allocated `cf_id`, `is_hidden_pk`, `is_per_partition_cf`.
    /// Outputs: a fully-built `RdbKeyDef` for slot `i`.
    /// Errors: `ErrorKind::Invalid` if the key columns aren't memcomparable.
    /// Invariants: index encoding direction is decided here per _DESIGN.md §2
    /// (`KeyDirection::Reverse` if the index is declared reverse-ordered) and
    /// flows into the codec at scan time.
    pub fn create_key_def(
        &mut self,
        table_arg: &TableRef,
        i: u32,
        tbl_def: &RdbTblDef,
        cf_id: u32,
        is_hidden_pk: bool,
        is_per_partition_cf: bool,
    ) -> Result<RdbKeyDef, Error> {
        let _ = (table_arg, i, tbl_def, cf_id, is_hidden_pk, is_per_partition_cf);
        todo!("compute index_id, key_direction, ttl flag; build memcomparable codec; assemble RdbKeyDef")
    }

    /// `int ha_rocksdb::create_table(const std::string&, const TABLE*, ulonglong)`
    /// — original C++ source line 7726.
    ///
    /// Inputs: `table_name` (normalized "dbname.tablename"), `table_arg`,
    /// `auto_increment_value`.
    /// Outputs: `Ok(())` after the dict-manager transaction commits.
    /// Errors:
    /// - `ErrorKind::Transaction` if the dict-write txn conflicts (rare).
    /// - `ErrorKind::Unavailable` on object-store I/O failure.
    /// - `ErrorKind::Invalid` if `create_key_defs` / `create_cfs` reject the
    ///   layout.
    /// Invariants: atomic — either all DDL bytes land in SlateDB or none do
    /// (via a single `DbTransaction.commit().await` on the SYSTEM CF prefix).
    /// On error, `self.tbl_def` is rolled back to `None` (matches the C++
    /// `error:` label cleanup).
    pub async fn create_table(
        &mut self,
        table_name: &str,
        table_arg: &TableRef,
        auto_increment_value: u64,
    ) -> Result<(), Error> {
        let _ = (table_name, table_arg, auto_increment_value);
        todo!("dict_manager.begin() -> create_key_defs -> ddl_manager.put_and_write -> commit().await")
    }

    /// `int ha_rocksdb::create(const char*, TABLE*, HA_CREATE_INFO*)` —
    /// original C++ source line 7829. The SQL-layer entry; thin wrapper that
    /// validates options and delegates to `create_table`.
    ///
    /// Inputs: `name` ("./dbname/tablename"), `table_arg`, `create_info`.
    /// Outputs: `Ok(())` on success.
    /// Errors:
    /// - `ErrorKind::Invalid` for unsupported `VECTOR` keys, foreign keys, or
    ///   `DATA DIRECTORY`/`INDEX DIRECTORY` (we warn-and-ignore the latter;
    ///   error for `VECTOR` keys and FK constraints).
    /// - `ErrorKind::Data` if a stale entry already exists in the DDL manager
    ///   under this name and `sql_command != TRUNCATE`.
    /// - Any error from `create_table`.
    /// Invariants: `name` is converted via `rdb_normalize_tablename` before
    /// any SlateDB lookups (preserves the "./" prefix and "#P#" partition
    /// suffix handling identically to MyRocks).
    pub async fn create(
        &mut self,
        name: &str,
        table_arg: &TableRef,
        create_info: &HaCreateInfo,
    ) -> Result<(), Error> {
        let _ = (name, table_arg, create_info);
        todo!("validate options; if existing tbl_def + SQLCOM_TRUNCATE -> delete_table; else create_table().await")
    }

    /// `int ha_rocksdb::delete_table(Rdb_tbl_def *tbl)` (private) — original
    /// C++ source line 11737. Deletes the dictionary entry and signals the
    /// drop-index thread to sweep rows on the next compaction.
    ///
    /// Inputs: `tbl` — the tbl_def to remove (caller-owned).
    /// Outputs: `Ok(())` once the dict txn commits.
    /// Errors: `ErrorKind::Transaction` / `ErrorKind::Unavailable` on commit.
    /// Invariants: the actual row-sweep is asynchronous. Per _DESIGN.md §1
    /// (compaction filters row), we register the `gl_index_id` as
    /// `to_drop` and the `CompactionFilter` (feature `compaction_filters`)
    /// drops rows with that prefix during the next compaction.
    pub async fn delete_table_by_tbl_def(
        &mut self,
        tbl: &RdbTblDef,
    ) -> Result<(), Error> {
        let _ = tbl;
        todo!("dict_manager: add_drop_table + ddl_manager.remove; commit().await; signal drop_index_thread")
    }

    /// `int ha_rocksdb::delete_table(const char *tablename)` (the public
    /// vtable variant) — original C++ source line 11779.
    ///
    /// Inputs: `tablename` ("./dbname/tablename").
    /// Outputs: `Ok(())` on success.
    /// Errors:
    /// - `ErrorKind::Invalid` if `tablename` doesn't normalize.
    /// - `ErrorKind::Data` (maps to `HA_ERR_NO_SUCH_TABLE`) if the DDL manager
    ///   has no entry.
    /// - Any error from `delete_table_by_tbl_def`.
    pub async fn delete_table(&mut self, tablename: &str) -> Result<(), Error> {
        let _ = tablename;
        todo!("normalize name; lookup; delegate to delete_table_by_tbl_def")
    }

    /// `int ha_rocksdb::rename_table(const char *from, const char *to)` —
    /// original C++ source line 11861.
    ///
    /// Inputs: `from`, `to` (both unnormalized "./dbname/tablename").
    /// Outputs: `Ok(())` on success.
    /// Errors:
    /// - `ErrorKind::Invalid` on malformed names.
    /// - `ErrorKind::Data` (mapped to `HA_ERR_NO_SUCH_TABLE`) if `from`
    ///   doesn't exist.
    /// - `ErrorKind::Transaction` / `Unavailable` on commit failure.
    /// Invariants: cross-database renames check that the target db exists
    /// (matches `rdb_database_exists`); error code on miss matches InnoDB
    /// (`-1`, returned as `Error::invalid` with a sentinel message that the
    /// HA-error mapper turns back into a plain `-1`).
    pub async fn rename_table(&mut self, from: &str, to: &str) -> Result<(), Error> {
        let _ = (from, to);
        todo!("normalize from/to; cross-db existence check; dict_manager.begin() -> ddl_manager.rename -> commit().await")
    }
}

/// Helper marker — DDL writes use Bytes keys/values throughout. The unused
/// import is intentional: it documents the codec contract at file scope.
#[allow(dead_code)]
const _BYTES_USAGE_MARKER: Option<Bytes> = None;

/// Forwarded helper for the DDL path: ergonomic "&str → normalized String"
/// used by `create` / `delete_table` / `rename_table`. Real impl lives in the
/// utils unit; this is a forward decl so the trait shape stays in this file.
#[allow(dead_code)]
fn normalize_tablename(_path: &str) -> Result<String, Error> {
    todo!("strip leading ./ ; lowercase fs handling on case-insensitive volumes; reject .. segments")
}

/// THD is forwarded as POD; we never expose `THD*` to the engine layer.
#[allow(dead_code)]
fn current_thd() -> ThdRef {
    todo!("bridge into current SQL session id")
}
