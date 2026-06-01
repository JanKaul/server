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
//! **Slice 1**: bridge surface (Rust `extern "C++"` + C++ shim).
//! **Slice 2**: first Rust caller — [`pack_with_sort_string`].
//! Wraps [`ffi::field_sort_string`] with size validation and a
//! `Result` return. This is the Rust counterpart of MyRocks'
//! `Rdb_key_def::pack_with_make_sort_key` (`rdb_datadic.cc:1489`)
//! — the universal "encode a field's value in memcmp form via
//! `field->sort_string`" pack routine.
//!
//! The function is not yet installed in [`FieldPacking::pack_func`].
//! That slot's current signature (`fn(&mut FieldPacking, &mut
//! FieldView, ...)`) was modelled before the cxx bridge existed
//! and uses a Rust-side metadata POD rather than a live
//! `&FieldRef`. Redesigning the slot's signature to accept
//! `&FieldRef` (and threading that through every call site) is a
//! separate slice that lands alongside the rest of the codec
//! pack pipeline (`KeyDef::pack_record`).

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

/// Pack one keypart's value into `dst` in memcmp (sort) form via
/// the C++ `field->sort_string` callback. Counterpart of MyRocks'
/// `Rdb_key_def::pack_with_make_sort_key` at `rdb_datadic.cc:1489`
/// — the universal pack routine for fixed-width key parts (every
/// integer, date, float family in MyRocks installs this as the
/// `pack_func` slot).
///
/// Writes exactly `fpi.max_image_len` bytes into the front of
/// `dst` and returns that byte count so the caller can advance
/// its write cursor.
///
/// ## Errors
///
/// - `Invalid` if `fpi.max_image_len < 0` (corruption — the
///   metadata wasn't initialised by [`FieldPacking::setup`])
/// - `Invalid` if `dst.len() < max_image_len` (the caller didn't
///   reserve enough output space)
///
/// ## Lifetime
///
/// `field` is borrowed from MariaDB and valid only for the
/// duration of this call. See the module doc for the retention
/// rule.
#[cfg(feature = "field_callbacks")]
pub fn pack_with_sort_string(
    fpi: &crate::codec::field_pack::FieldPacking,
    field: &ffi::FieldRef,
    dst: &mut [u8],
) -> Result<usize, slatedb::Error> {
    if fpi.max_image_len < 0 {
        return Err(slatedb::Error::invalid(format!(
            "pack_with_sort_string: invalid max_image_len {} \
             (FieldPacking::setup must have run first)",
            fpi.max_image_len,
        )));
    }
    let max_len = fpi.max_image_len as usize;
    if dst.len() < max_len {
        return Err(slatedb::Error::invalid(format!(
            "pack_with_sort_string: dst too short — have {} bytes, \
             need {} (max_image_len)",
            dst.len(),
            max_len,
        )));
    }
    ffi::field_sort_string(field, &mut dst[..max_len], max_len as u32);
    Ok(max_len)
}

#[cfg(all(test, not(feature = "field_callbacks")))]
mod tests {
    //! Without the `field_callbacks` feature there's no `FieldRef`
    //! to construct in Rust tests, so this module is intentionally
    //! empty when the feature is off. The function is exercised by
    //! the C++ build (slice 3 onward).
}
