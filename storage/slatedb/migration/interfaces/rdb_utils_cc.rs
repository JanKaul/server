//! Interface stub for `rdb_utils_cc`.
//!
//! C++ source: `storage/rocksdb/rdb_utils.cc` (369 LoC)
//!
//! ## Mapping
//! Implementation of utility helpers declared in `rdb_utils_h.rs`. Most
//! are string-parsing / hexdump / cleanup helpers that translate
//! mechanically.
//!
//! ## Out-of-scope methods
//! None.

use slatedb::Error;

/// Hexdump bytes (debugging helper). Returns up to `maxsize` bytes; longer
/// inputs are truncated with "...".
/// Original: rdb_utils.cc — `rdb_hexdump`.
pub fn rdb_hexdump(data: &[u8], maxsize: usize) -> String {
    todo!("format bytes as hex pairs separated by spaces; truncate at maxsize")
}

/// Persist a corruption marker file in the data directory. Read at startup
/// by `rdb_check_rocksdb_corruption`.
/// Original: rdb_utils.cc — `rdb_persist_corruption_marker`.
pub fn rdb_persist_corruption_marker(data_dir: &std::path::Path) -> Result<(), Error> {
    todo!("create empty file at data_dir/SLATEDB_CORRUPTED")
}

/// Check whether the corruption marker file exists.
/// Original: rdb_utils.cc — `rdb_check_rocksdb_corruption`.
pub fn rdb_check_corruption(data_dir: &std::path::Path) -> bool {
    data_dir.join("SLATEDB_CORRUPTED").exists()
}

/// Log a SlateDB error with context. Logged at error level. Used in
/// background tasks where the error doesn't propagate naturally.
/// Original: rdb_utils.cc — `rdb_log_status_error`.
pub fn rdb_log_status_error(err: &Error, context: Option<&str>) {
    todo!("tracing::error!(error = ?err, context); preserves the original log format")
}

/// Check whether a database directory exists in the object store.
/// Original: rdb_utils.cc — `rdb_database_exists`.
pub async fn rdb_database_exists(db_name: &str) -> Result<bool, Error> {
    todo!("Db::manifest probe or object_store list_with_delimiter")
}

/// Return the list of compression codecs supported by SlateDB. Used in
/// SHOW VARIABLES for `rocksdb_supported_compression_types`.
/// Original: rdb_utils.cc — `get_rocksdb_supported_compression_types`.
pub fn get_supported_compression_types() -> &'static str {
    todo!("derived from slatedb::config::CompressionCodec variants compiled in")
}

// String-parsing helpers (rdb_skip_spaces, rdb_compare_strings_ic, etc.)
// translate mechanically — declared as fns in rdb_utils_h.rs. Their
// implementations follow the same pattern: byte-level parsers, returns
// the new cursor position or None on error.
