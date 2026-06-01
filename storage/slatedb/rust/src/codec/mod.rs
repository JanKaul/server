//! Codec — translation between MariaDB row/key formats and SlateDB bytes.
//!
//! Per `_DESIGN.md §8`. This batch lands the key-prefix scaffolding (varint
//! `cf_id` + `u32_be index_id`) that everything else in the codec layer
//! ultimately produces / consumes.

pub mod comment_parser;
pub mod dict;
pub mod field_pack;
pub mod key;
pub mod prefix;
pub mod row_value;
pub mod tbl_def;
pub mod value;
