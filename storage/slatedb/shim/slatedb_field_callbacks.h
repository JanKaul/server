/*
  Copyright (c) 2026, MariaDB Corporation.

  This program is free software; you can redistribute it and/or modify
  it under the terms of the GNU General Public License as published by
  the Free Software Foundation; version 2 of the License.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU General Public License for more details.

  You should have received a copy of the GNU General Public License
  along with this program; if not, write to the Free Software
  Foundation, Inc., 51 Franklin St, Fifth Floor, Boston, MA 02110-1335 USA */

#ifndef SLATEDB_FIELD_CALLBACKS_H
#define SLATEDB_FIELD_CALLBACKS_H

/*
  Field/TABLE callback surface for the slatedb-engine Rust crate.

  Lifetime contract
  -----------------
  Memory is ALWAYS owned by MariaDB. `slatedb::FieldRef` and
  `slatedb::TableRef` are thin C++ wrappers around `Field*` / `TABLE*`
  pointers that MariaDB allocated. They cross the cxx boundary by
  shared reference only — never by value, never as a unique_ptr, never
  as a Box on the Rust side.

  The Rust side receives them as `&FieldRef` / `&TableRef` (or
  `Pin<&mut FieldRef>` for mutating ops). Rust must not retain the
  borrow past the cxx-callback that received it: the validity window
  is "while the MariaDB thread that handed us the pointer is blocked
  on the Rust call".

  Column data is exchanged by fill-buffer:
    * Read direction: Rust supplies `&mut [u8]`, C++ writes into it.
    * Write direction: Rust supplies `&[u8]`, C++ copies into Field
      storage.

  No allocation crosses the boundary in either direction.

  Why an empty wrapper struct
  ---------------------------
  cxx requires opaque C++ types to be expressible as a unique
  identity on both sides. We use a wrapper rather than a typedef so
  the cxx-generated code never sees `Field` / `TABLE` types directly
  (which would require pulling MariaDB's full include chain into the
  bridge .cc translation unit). The wrapper has zero overhead — it's
  one pointer, treated as an opaque handle.

  Why these specific callbacks
  ----------------------------
  Distilled from the MyRocks call sites at
  `storage/rocksdb/rdb_datadic.cc` and `storage/rocksdb/ha_rocksdb.cc`.
  Each callback corresponds to a `field->XXX()` call in MyRocks that
  the codec pack/unpack functions issue per keypart. Charsets are
  exchanged by collation id (`u32`) rather than `CHARSET_INFO*` to
  avoid another pointer lifetime to manage; lookup-by-id happens in
  a separate callback added when collation-aware unpack lands.

  Build-time toggle
  -----------------
  This header is meaningful only inside the MariaDB plugin build
  (it transitively pulls in `field.h` / `table.h`). The slatedb
  Rust crate enables the Field-callback bridge under the
  `field_callbacks` Cargo feature; CMake turns the feature on via
  `corrosion_import_crate(... FEATURES field_callbacks)`. The
  cxx-generated bridge .cc then `#include`s this header to find the
  symbol declarations + the opaque type definitions.

  Why a single header file
  ------------------------
  All callback bodies are short forwards to MariaDB Field methods —
  inlining them keeps the cxx-generated bridge as a single TU and
  avoids an extra .cc that would have to re-include all the MariaDB
  headers. The inline bodies also let MariaDB's optimiser inline
  across the cxx trampoline.
*/

#include "my_global.h"
#include "field.h"
#include "table.h"
#include "rust/cxx.h"

namespace slatedb {

/*
  Opaque-on-Rust-side wrappers around MariaDB pointers. cxx sees the
  type by name only; the Rust crate cannot read `.ptr` or otherwise
  touch the underlying object — every interaction goes through the
  callbacks below.
*/
struct FieldRef {
  Field *ptr;
};

struct TableRef {
  TABLE *ptr;
};

/* --------------------------------------------------------------- */
/*  Field accessors — primitive return, const FieldRef             */
/* --------------------------------------------------------------- */

/* True iff this Field's storage has a null bit (NULLABLE).  */
inline bool field_real_maybe_null(const FieldRef &f) {
  return f.ptr->real_maybe_null();
}

/* True iff the Field is currently storing SQL NULL.  */
inline bool field_is_real_null(const FieldRef &f) {
  return f.ptr->is_real_null();
}

/* Storage-format length (the on-record number of bytes).  */
inline uint32_t field_pack_length(const FieldRef &f) {
  return f.ptr->pack_length();
}

/* Logical data length (e.g. VARCHAR runtime length).  */
inline uint32_t field_data_length(const FieldRef &f) {
  return f.ptr->data_length();
}

/* Character-unit length (charset-aware).  */
inline uint32_t field_char_length(const FieldRef &f) {
  return f.ptr->char_length();
}

/* `field_length` member — the declared column length.  */
inline uint32_t field_field_length(const FieldRef &f) {
  return static_cast<uint32_t>(f.ptr->field_length);
}

/* `real_type()` — the MYSQL_TYPE_* code (enum_field_types).  */
inline uint32_t field_real_type(const FieldRef &f) {
  return static_cast<uint32_t>(f.ptr->real_type());
}

/* `key_type()` — the HA_KEYTYPE_* code (enum ha_base_keytype).  */
inline uint32_t field_key_type(const FieldRef &f) {
  return static_cast<uint32_t>(f.ptr->key_type());
}

/* Position of this Field in its TABLE's `field[]` array.  */
inline uint32_t field_field_index(const FieldRef &f) {
  return static_cast<uint32_t>(f.ptr->field_index);
}

/* Collation id (charset->number). The full CHARSET_INFO* is not
   exposed — id-only avoids another opaque lifetime. A future
   `charset_by_id` callback will resolve the id when collation-aware
   unpack lands. */
inline uint32_t field_charset_number(const FieldRef &f) {
  return static_cast<uint32_t>(f.ptr->charset()->number);
}

/* Bit position within the null-byte for this Field.  */
inline uint32_t field_null_bit(const FieldRef &f) {
  return static_cast<uint32_t>(f.ptr->null_bit);
}

/* Null-byte offset (relative to record buffer start). -1 if the
   Field is non-nullable (no null_ptr).  */
inline int32_t field_null_offset(const FieldRef &f) {
  if (!f.ptr->real_maybe_null()) {
    return -1;
  }
  return static_cast<int32_t>(
      f.ptr->null_ptr - f.ptr->table->record[0]);
}

/* Bitmap of "which keys does this Field participate in".  */
inline uint64_t field_part_of_key(const FieldRef &f) {
  return f.ptr->part_of_key.to_ulonglong();
}

/* --------------------------------------------------------------- */
/*  Read column bytes — fill-buffer                                */
/* --------------------------------------------------------------- */

/* Encode the column's value into `dst` in memcmp (sort) form.
   `max_len` is the fixed output width (caller computed from the
   FieldPacking metadata). Mirrors MyRocks'
   `pack_with_make_sort_key` at `rdb_datadic.cc:1503`.

   The callback writes `min(max_len, dst.size())` bytes into the
   Rust-side slice. The `dbug_tmp_use_all_columns` wrapping that
   MyRocks uses to bypass read-set checks is applied here so the
   Rust caller doesn't see that detail. */
inline void field_sort_string(const FieldRef &f,
                              ::rust::Slice<uint8_t> dst,
                              uint32_t max_len) {
  size_t dst_len = dst.size();
  uint32_t len = max_len < dst_len
                     ? max_len
                     : static_cast<uint32_t>(dst_len);
  MY_BITMAP *old_map = dbug_tmp_use_all_columns(f.ptr->table,
                                                &f.ptr->table->read_set);
  f.ptr->sort_string(dst.data(), len);
  dbug_tmp_restore_column_map(&f.ptr->table->read_set, old_map);
}

/* Copy the raw on-record bytes (`field->ptr`) into `dst`. Writes
   `min(pack_length(), dst.size())` bytes. Used by codec paths that
   want the unencoded value (e.g. row-format pack for non-key
   columns). */
inline void field_ptr_bytes(const FieldRef &f,
                            ::rust::Slice<uint8_t> dst) {
  size_t n = f.ptr->pack_length();
  if (n > dst.size()) n = dst.size();
  memcpy(dst.data(), f.ptr->ptr, n);
}

/* --------------------------------------------------------------- */
/*  Write column state — mutating, non-const FieldRef              */
/* --------------------------------------------------------------- */

/* Copy `src` into the Field's storage. The caller is expected to
   have validated the size against `pack_length()`. Returns the
   number of bytes written. Used by unpack paths reconstructing a
   row.  */
inline uint32_t field_set_value(FieldRef &f,
                                ::rust::Slice<const uint8_t> src) {
  size_t n = f.ptr->pack_length();
  if (n > src.size()) n = src.size();
  memcpy(f.ptr->ptr, src.data(), n);
  return static_cast<uint32_t>(n);
}

/* Mark this Field as SQL NULL. Caller is responsible for ensuring
   the Field is NULLABLE (the C++ asserts internally). */
inline void field_set_null(FieldRef &f) {
  f.ptr->set_null();
}

/* Mark this Field as NOT NULL.  */
inline void field_set_notnull(FieldRef &f) {
  f.ptr->set_notnull();
}

/* --------------------------------------------------------------- */
/*  TABLE access                                                   */
/* --------------------------------------------------------------- */

/* Borrow the Field at `field_index` from a TABLE. The returned
   reference shares the TABLE's lifetime (which is bounded by the
   blocked MariaDB thread).

   The returned reference is to a static thread-local cache slot —
   cxx requires that `&FieldRef` returned from a callback be stable
   storage, but the underlying `Field*` it points to is short-lived
   (per-call). The Rust caller must not retain the reference past
   the cxx-callback boundary.

   Why thread-local: the cxx ABI for an `&T` return value expects T
   to live somewhere stable. We can't make `FieldRef` itself live in
   the TABLE (we don't own those), so we keep a per-thread scratch
   FieldRef that gets re-pointed on each lookup. Single-thread-per-
   call invariant from MariaDB makes this safe. */
inline const FieldRef &table_field_at(const TableRef &t,
                                      uint32_t field_index) {
  thread_local FieldRef scratch{nullptr};
  scratch.ptr = t.ptr->field[field_index];
  return scratch;
}

/* Borrow the in-progress row buffer (`record[0]`). The returned
   pointer is valid for the duration of the current handler call.
   Length is the table's stored-row size (`table->s->stored_rec_length`).

   Note: cxx's `&[u8]` (Rust-side `&[u8]`) is marshalled as
   `rust::Slice<const uint8_t>`. We construct it inline. */
inline ::rust::Slice<const uint8_t> table_record_buf(const TableRef &t) {
  return ::rust::Slice<const uint8_t>(
      t.ptr->record[0],
      t.ptr->s->stored_rec_length);
}

}  /* namespace slatedb */

#endif  /* SLATEDB_FIELD_CALLBACKS_H */
