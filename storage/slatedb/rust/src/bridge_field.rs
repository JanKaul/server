//! Cxx bridge — Field/TABLE callback surface (`extern "C++"`).
//!
//! Counterpart of `storage/slatedb/shim/slatedb_field_callbacks.h`.
//! Declares the C++ functions the Rust codec will call to access
//! MariaDB `Field*` / `TABLE*` state.
//!
//! ## Lifetime contract
//!
//! Memory ownership stays on the MariaDB side throughout. Rust
//! never sees a `Box<FieldRef>`, never owns a `UniquePtr<FieldRef>`,
//! never frees a `Field` / `TABLE`. The bridge marshals borrowed
//! references (`&FieldRef`, `Pin<&mut FieldRef>`) — the underlying
//! `Field*` / `TABLE*` is pinned by the blocked MariaDB thread
//! that issued the Rust call.
//!
//! The Rust caller must NOT retain a `&FieldRef` past the cxx
//! callback that returned it.
//!
//! ## Buffer semantics
//!
//! Column data is exchanged by fill-buffer — no allocation crosses
//! the boundary:
//! - Read direction: Rust supplies `&mut [u8]`, C++ writes into it.
//! - Write direction: Rust supplies `&[u8]`, C++ copies into Field.
//!
//! ## Build gating
//!
//! This module is gated by the `field_callbacks` Cargo feature.
//! Default `cargo test --lib` doesn't enable it (no C++ build
//! context = no link target for the `extern "C++"` symbols).
//! CMake's `corrosion_import_crate(... FEATURES field_callbacks)`
//! turns it on for the plugin build, at which point the cxx-
//! generated bridge .cc `#include`s `slatedb_field_callbacks.h`
//! which inline-forwards to MariaDB's Field/TABLE methods.
//!
//! ## Status
//!
//! **Slice 1**: pure surface — no Rust callers yet. The
//! declarations land alongside the C++ shim so slice 2
//! (`pack_with_sort_string` — first real Rust caller) can wire
//! straight through without further bridge changes.

#[cfg(feature = "field_callbacks")]
#[cxx::bridge(namespace = "slatedb")]
pub mod ffi {
    unsafe extern "C++" {
        include!("slatedb_field_callbacks.h");

        /// Opaque wrapper around MariaDB's `Field*`. Lifetime is
        /// managed by MariaDB — Rust only ever sees `&FieldRef`
        /// (or `Pin<&mut FieldRef>`), never by value.
        type FieldRef;

        /// Opaque wrapper around MariaDB's `TABLE*`. Same
        /// non-ownership contract as [`FieldRef`].
        type TableRef;

        // ----- Field accessors (read-only, primitive return) -----

        /// True iff the Field is NULLABLE.
        fn field_real_maybe_null(f: &FieldRef) -> bool;

        /// True iff the Field currently holds SQL NULL.
        fn field_is_real_null(f: &FieldRef) -> bool;

        /// Storage-format length (on-record byte count).
        fn field_pack_length(f: &FieldRef) -> u32;

        /// Logical data length (e.g. VARCHAR runtime length).
        fn field_data_length(f: &FieldRef) -> u32;

        /// Character-unit length (charset-aware).
        fn field_char_length(f: &FieldRef) -> u32;

        /// Declared column length (`field_length` member).
        fn field_field_length(f: &FieldRef) -> u32;

        /// MYSQL_TYPE_* (enum_field_types) code from `real_type()`.
        fn field_real_type(f: &FieldRef) -> u32;

        /// HA_KEYTYPE_* (enum ha_base_keytype) code from `key_type()`.
        fn field_key_type(f: &FieldRef) -> u32;

        /// Position of this Field in its TABLE's `field[]` array.
        fn field_field_index(f: &FieldRef) -> u32;

        /// Collation id (`charset()->number`). The full
        /// `CHARSET_INFO*` is not exposed — id avoids another
        /// opaque lifetime to manage. A future `charset_by_id`
        /// callback resolves the id when collation-aware unpack
        /// lands.
        fn field_charset_number(f: &FieldRef) -> u32;

        /// Bit position within the null-byte for this Field.
        fn field_null_bit(f: &FieldRef) -> u32;

        /// Null-byte offset (relative to `record[0]` start).
        /// Returns `-1` if the Field is non-nullable.
        fn field_null_offset(f: &FieldRef) -> i32;

        /// Bitmap of "which keys does this Field participate in"
        /// (MariaDB caps at 64 keys per table).
        fn field_part_of_key(f: &FieldRef) -> u64;

        // ----- Read column bytes (fill-buffer) -----

        /// Encode the column's value into `dst` in memcmp (sort)
        /// form. Writes `min(max_len, dst.len())` bytes. The C++
        /// side wraps the call in `dbug_tmp_use_all_columns` to
        /// bypass read-set checks (matches the MyRocks
        /// `pack_with_make_sort_key` pattern).
        fn field_sort_string(f: &FieldRef, dst: &mut [u8], max_len: u32);

        /// Copy the raw on-record bytes into `dst`. Writes
        /// `min(field.pack_length(), dst.len())` bytes.
        fn field_ptr_bytes(f: &FieldRef, dst: &mut [u8]);

        // ----- Write column state (mutating) -----

        /// Copy `src` into the Field's storage. Returns the
        /// number of bytes written. Caller is responsible for
        /// validating `src.len()` against `pack_length()`.
        fn field_set_value(f: Pin<&mut FieldRef>, src: &[u8]) -> u32;

        /// Mark this Field as SQL NULL. Caller must ensure the
        /// Field is NULLABLE.
        fn field_set_null(f: Pin<&mut FieldRef>);

        /// Mark this Field as NOT NULL.
        fn field_set_notnull(f: Pin<&mut FieldRef>);

        // ----- TABLE access -----

        /// Borrow the Field at `field_index` from a TABLE. The
        /// returned reference shares the TABLE's lifetime (bounded
        /// by the blocked MariaDB thread); see module-doc for the
        /// retention rule.
        ///
        /// The C++ side returns a reference to a thread-local
        /// scratch `FieldRef` whose `.ptr` is re-pointed on each
        /// call. Single-thread-per-handler-call invariant from
        /// MariaDB makes this safe.
        fn table_field_at(t: &TableRef, field_index: u32) -> &FieldRef;

        /// Borrow the in-progress row buffer (`record[0]`).
        /// Length is `table->s->stored_rec_length`.
        fn table_record_buf(t: &TableRef) -> &[u8];
    }
}
