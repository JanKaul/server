//! Interface stub for `ha_rocksdb_cc__Rdb_index_collector`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 13652..13662, ~10 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__Rdb_index_collector`
//!
//! ## Mapping
//! This is a **local-scope class** declared inside
//! `Rdb_background_thread::run`. It implements `Rdb_tables_scanner` —
//! when the index-stats recalc queue is empty, `ddl_manager.scan_for_tables`
//! invokes `add_table(tdef)` on each open table, and `Rdb_index_collector`
//! appends every `(cf_id, index_id)` of the table's key-defs onto
//! `rdb_indexes_to_recalc` (the global queue).
//!
//! It is **trivial** — one method that pushes `tdef->m_key_descr_arr[i]->get_gl_index_id()`
//! into a `Vec<GlIndexId>`.
//!
//! Per _DESIGN.md §1 (information_schema re-impl): the recalc queue is fed
//! by our analogue of `Rdb_background_thread` (already a Tokio task; see
//! that stub). This collector is just a closure passed to the
//! `RdbDdlManager::scan_for_tables` walker.
//!
//! We model it as a free function rather than a class — Rust doesn't need
//! an interface here, just a `FnMut(&RdbTblDef)`.
//!
//! ## Out-of-scope methods
//! - The `Rdb_tables_scanner` interface (single virtual method) — collapses
//!   to a `FnMut` closure in Rust.

use slatedb::Error;

use crate::ha_rocksdb_cc__Rdb_transaction::RdbTblDef;
use crate::rdb_global_h::GlIndexId;

/// Append every index of `tdef` onto `out`. Used to seed the
/// `rdb_indexes_to_recalc` global queue on background-task wake-up.
///
/// Original: ha_rocksdb.cc:13652 — `Rdb_index_collector::add_table`.
pub fn collect_index_ids(tdef: &RdbTblDef, _out: &mut Vec<GlIndexId>) -> Result<(), Error> {
    let _ = tdef;
    todo!("for i in 0..tdef.key_count: out.push(tdef.key_descr_arr[i].gl_index_id())")
}

/// Convenience: scan all open tables, collect every `GlIndexId`. The DDL
/// manager exposes a `for_each_table` iterator; this just chains.
///
/// Returns a fresh `Vec` — caller (typically `RdbBackgroundThread::run`)
/// drains it.
pub fn collect_all_indexes_for_recalc(
    _ddl_manager: &dyn crate::ha_rocksdb_cc__Rdb_background_thread::DdlManagerRef,
) -> Result<Vec<GlIndexId>, Error> {
    todo!("ddl_manager.for_each_table(|tdef| collect_index_ids(tdef, &mut out)?)")
}
