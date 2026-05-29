//! Interface stub for `rdb_datadic_h__Rdb_collation_codec`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.h` (lines 894..905)
//! v4 manifest sub-unit: `rdb_datadic_h__Rdb_collation_codec`
//! Parent unit: `rdb_datadic_h`
//! Approx body LoC accounted for here: 12
//!
//! ## Mapping
//! In-scope codec types — preserved from MyRocks per _DESIGN.md §2/§3. This
//! `struct` holds the precomputed encode/decode tables for a "simple"
//! collation (where each source byte maps to one dest byte via `strnxfrm`).
//! Because the `strnxfrm` mapping is not injective, decode needs the side
//! index `m_dec_idx` to recover the original byte.
//!
//! **The on-disk encoded bytes are preserved bit-for-bit from MyRocks**, so
//! the tables themselves are identical — they're populated at startup from
//! the MySQL `CHARSET_INFO` registry. The Rust port replaces the raw function
//! pointers `m_make_unpack_info_func` / `m_unpack_func` with `fn` pointers of
//! matching signatures.
//!
//! Two globals declared alongside the struct are also moved here:
//! `rdb_collation_data_mutex` (replaced by `std::sync::Mutex` over the
//! collation table) and the table itself (`rdb_collation_data`, an array
//! indexed by charset id).
//!
//! `rdb_mem_cmp_space_mutex` is an unrelated lock for `space_xfrm` lazy
//! initialization in `Rdb_field_packing`; we move it to that unit.
//!
//! ## Out-of-scope items
//! None — pure data carrier.

/// Function pointer that emits unpack_info bytes for one field. Mirrors C++
/// `rdb_make_unpack_info_t` from rdb_datadic.h:145. Defined here as a Rust
/// `fn` alias so `CollationCodec` can hold the slot without a generic param.
pub type MakeUnpackInfoFn = fn(
    codec: &CollationCodec,
    field: &super::rdb_datadic_h__Rdb_convert_to_record_key_decoder::FieldView,
    pack_ctx: &mut super::rdb_datadic_h__Rdb_pack_field_context::PackFieldContext<'_>,
);

/// Function pointer that decodes one packed memcmp field into the record
/// buffer. Mirrors C++ `rdb_index_field_unpack_t` from rdb_datadic.h:148.
pub type IndexFieldUnpackFn = fn(
    fpi: &mut super::rdb_datadic_h__Rdb_convert_to_record_key_decoder::FieldPacking,
    field: &mut super::rdb_datadic_h__Rdb_convert_to_record_key_decoder::FieldView,
    field_ptr: &mut [u8],
    reader: &mut crate::rdb_buff_h::StringReader,
    unpack_reader: Option<&mut crate::rdb_buff_h::StringReader>,
) -> i32;

/// Per-collation pack/unpack table.
///
/// `m_cs` originally pointed to a `my_core::CHARSET_INFO`. We hold the
/// charset id (a `u32` — collations are addressed by id elsewhere in this
/// codebase) since the full struct is a MySQL-internal type that we do not
/// re-expose in our Rust API surface.
///
/// Original: rdb_datadic.h:894 — `struct Rdb_collation_codec`.
pub struct CollationCodec {
    /// MySQL charset id (`CHARSET_INFO::number`). The original pointer-based
    /// access is recovered by indexing the runtime charset table.
    pub charset_id: u32,

    /// First slot unpacks `VARCHAR(n)`; second slot unpacks `CHAR(n)`.
    pub make_unpack_info_func: [MakeUnpackInfoFn; 2],
    pub unpack_func: [IndexFieldUnpackFn; 2],

    /// `src_byte -> idx` table written into the sidechannel during encode.
    pub enc_idx: [u8; 256],
    /// `src_byte -> length` in bytes of the encoded form.
    pub enc_size: [u8; 256],

    /// `idx -> decoded_length` lookup used during decode.
    pub dec_size: [u8; 256],
    /// `dec_idx[idx][packed_byte] -> original_byte`. Variable outer length
    /// because the number of disambiguating indices is collation-dependent.
    pub dec_idx: Vec<[u8; 256]>,
}

/// Global collation table. Indexed by MySQL charset id. Entries are populated
/// lazily as collations are first referenced. `None` for ids that aren't a
/// "simple" collation (the codec falls back to the unknown-charset path).
///
/// In MyRocks this is `std::array<const Rdb_collation_codec *, MY_ALL_CHARSETS_SIZE>`
/// guarded by `rdb_collation_data_mutex`. We use `OnceLock` per slot plus an
/// outer `RwLock` to match the lazy-init pattern without holding the mutex
/// over the hot read path.
///
/// Original: rdb_datadic.h:909 — `rdb_collation_data`.
pub struct CollationDataTable {
    // TODO(human): pin MY_ALL_CHARSETS_SIZE — currently 2048 in 10.6 but the
    // exact constant should be re-exported from the mariadb-port unit.
    pub slots: Vec<Option<std::sync::Arc<CollationCodec>>>,
}

/// Mutex that guards `rdb_collation_data` mutations (insertions of new
/// slots). Reads are lock-free via `Arc::clone`.
/// Original: rdb_datadic.h:907 — `rdb_collation_data_mutex`.
pub static COLLATION_DATA_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
