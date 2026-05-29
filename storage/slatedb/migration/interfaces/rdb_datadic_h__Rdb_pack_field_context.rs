//! Interface stub for `rdb_datadic_h__Rdb_pack_field_context`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (lines 87..97)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_pack_field_context`
//! Parent unit: `rdb_datadic_h`
//! Approx body LoC accounted for here: 11
//!
//! ## Mapping
//! In-scope codec types — preserved from MyRocks per _DESIGN.md §2/§3. This
//! is a trivial value-carrier struct that threads the unpack-info writer
//! between a call to `rdb_index_field_pack_t` and a follow-up
//! `rdb_make_unpack_info_t`. Translates directly to a tiny Rust struct.
//!
//! In MyRocks the pointer is `Rdb_string_writer *` and may be null when the
//! caller is not producing unpack_info. We model that with `Option<&mut
//! StringWriter>` for clarity; the null-case is statically distinguished.
//!
//! ## Out-of-scope methods
//! None — pure data carrier.

use crate::rdb_buff_h::StringWriter;

/// Field-pack context — carries the unpack_info writer (if any) across the
/// pack/make-unpack call pair. Single-shot, stack-only object; never stored
/// in long-lived state.
///
/// Lifetime `'w` ties this to the writer the caller passed in.
///
/// Original: rdb_datadic.h:87 — `class Rdb_pack_field_context`.
pub struct PackFieldContext<'w> {
    /// `None` means the caller is not producing unpack_info for this index
    /// (e.g. the index does not support covering reads). When `Some`, the
    /// pack routine MAY append bytes; the make_unpack_info routine reads
    /// these back on decode.
    pub writer: Option<&'w mut StringWriter>,
}

impl<'w> PackFieldContext<'w> {
    /// Construct with a writer. The C++ constructor takes a `Rdb_string_writer
    /// *const` (possibly null); we accept the optional reference directly.
    ///
    /// Original: rdb_datadic.h:92 — explicit ctor.
    pub fn new(writer: Option<&'w mut StringWriter>) -> Self {
        Self { writer }
    }
}
