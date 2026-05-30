//! Engine-layer modules: SlateDB `Db` wrapper, CF mapping, transactions,
//! snapshots, comparator/option helpers.
//!
//! Per `_DESIGN.md §8`. This batch lands the leaf-y subset: comparator
//! direction handling and CF-options parsing. The `Db` wrapper / CF
//! manager / transaction stubs follow once their dependencies are in.

pub mod cf;
pub mod cf_options;
pub mod compact_filter;
pub mod comparator;
pub mod db;
pub mod index_merge;
pub mod merge;
pub mod snapshot;
pub mod txn;
