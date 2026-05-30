//! Key-prefix encoding: `varint(cf_id) || u32_be(index_id) || memcmp_key`.
//!
//! Per `_DESIGN.md §2` and §11 Q1 (resolved). `cf_id` is LEB128-varint
//! encoded so the common case (cf_id < 128) costs one byte; the system CF
//! (`u32::MAX`) costs five bytes. `index_id` is fixed-width big-endian so
//! lexicographic key order matches numeric order within a CF.
//!
//! This module also exposes [`MyRocksPrefixExtractor`] — the
//! `slatedb::PrefixExtractor` impl that feeds the SST-level bloom filter
//! with `varint(cf_id) || u32_be(index_id)` prefixes for per-index bloom
//! lookups.

use bytes::Bytes;
use slatedb::{PrefixExtractor, PrefixTarget};

/// Width of the `u32_be index_id` portion of the key prefix.
pub const INDEX_ID_LEN: usize = 4;

/// Maximum encoded length of a u32 varint (5 bytes; `u32::MAX` needs the
/// full LEB128 expansion).
pub const MAX_VARINT_U32_LEN: usize = 5;

/// Encode `value` as an unsigned LEB128 varint, appending bytes to `out`.
/// Returns the number of bytes written.
pub fn encode_varint_u32(value: u32, out: &mut Vec<u8>) -> usize {
    let mut v = value;
    let mut written = 0;
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            written += 1;
            return written;
        }
        out.push(byte | 0x80);
        written += 1;
    }
}

/// Decode an unsigned LEB128 varint from the front of `input`. Returns
/// `Some((value, bytes_consumed))` on success, `None` on truncation or
/// overlong encoding (more than [`MAX_VARINT_U32_LEN`] continuation bytes).
pub fn decode_varint_u32(input: &[u8]) -> Option<(u32, usize)> {
    let mut value: u32 = 0;
    let mut shift = 0;
    for (i, &byte) in input.iter().enumerate() {
        if i >= MAX_VARINT_U32_LEN {
            return None;
        }
        let chunk = u32::from(byte & 0x7f);
        // Guard overflow: only the last legal byte may carry bits above 28.
        let shifted = chunk.checked_shl(shift)?;
        value |= shifted;
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
        shift += 7;
    }
    None
}

/// Encoded length of `value` as a u32 varint.
pub fn varint_u32_len(value: u32) -> usize {
    let mut v = value;
    let mut len = 1;
    while v >= 0x80 {
        v >>= 7;
        len += 1;
    }
    len
}

/// Build the per-index key prefix: `varint(cf_id) || u32_be(index_id)`.
pub fn build_key_prefix(cf_id: u32, index_id: u32) -> Bytes {
    let mut out = Vec::with_capacity(varint_u32_len(cf_id) + INDEX_ID_LEN);
    encode_varint_u32(cf_id, &mut out);
    out.extend_from_slice(&index_id.to_be_bytes());
    Bytes::from(out)
}

/// Parsed key-prefix components: `(cf_id, index_id, prefix_byte_len)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedPrefix {
    pub cf_id: u32,
    pub index_id: u32,
    pub prefix_len: usize,
}

/// Parse the `varint(cf_id) || u32_be(index_id)` head of `key`. Returns
/// `None` if `key` is too short or the varint is malformed.
pub fn parse_key_prefix(key: &[u8]) -> Option<ParsedPrefix> {
    let (cf_id, n) = decode_varint_u32(key)?;
    if key.len() < n + INDEX_ID_LEN {
        return None;
    }
    let mut idx_bytes = [0u8; INDEX_ID_LEN];
    idx_bytes.copy_from_slice(&key[n..n + INDEX_ID_LEN]);
    Some(ParsedPrefix {
        cf_id,
        index_id: u32::from_be_bytes(idx_bytes),
        prefix_len: n + INDEX_ID_LEN,
    })
}

/// `slatedb::PrefixExtractor` impl that extracts `varint(cf_id) || u32_be(index_id)`.
///
/// The Point and Prefix variants both return `Some(varint_len(cf_id) + 4)`
/// when the input is long enough — that's safe because the prefix bytes
/// depend only on those leading bytes (the `Prefix` invariant in the
/// SlateDB trait docs). If the input is shorter, both return `None` and
/// the bloom filter is skipped.
pub struct MyRocksPrefixExtractor;

impl PrefixExtractor for MyRocksPrefixExtractor {
    fn name(&self) -> &str {
        "myrocks-v1-cf-index"
    }

    fn prefix_len(&self, target: &PrefixTarget) -> Option<usize> {
        let bytes: &[u8] = match target {
            PrefixTarget::Point(b) | PrefixTarget::Prefix(b) => b,
        };
        let (_cf_id, n) = decode_varint_u32(bytes)?;
        let total = n.checked_add(INDEX_ID_LEN)?;
        if bytes.len() < total {
            return None;
        }
        Some(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_round_trips_at_each_length_boundary() {
        for v in [0u32, 1, 127, 128, 16383, 16384, 2_097_151, 2_097_152, u32::MAX] {
            let mut buf = Vec::new();
            let written = encode_varint_u32(v, &mut buf);
            assert_eq!(written, buf.len());
            assert_eq!(written, varint_u32_len(v), "len helper disagrees for {v}");
            let (decoded, consumed) = decode_varint_u32(&buf).expect("decode");
            assert_eq!(decoded, v);
            assert_eq!(consumed, written);
        }
    }

    #[test]
    fn varint_max_is_five_bytes() {
        assert_eq!(varint_u32_len(u32::MAX), MAX_VARINT_U32_LEN);
    }

    #[test]
    fn varint_decode_truncated_returns_none() {
        // Byte with continuation bit set but no follow-up.
        assert!(decode_varint_u32(&[0x80]).is_none());
        assert!(decode_varint_u32(&[]).is_none());
    }

    #[test]
    fn varint_decode_overlong_returns_none() {
        // Six continuation bytes — overlong for u32.
        let too_long = [0x80, 0x80, 0x80, 0x80, 0x80, 0x01];
        assert!(decode_varint_u32(&too_long).is_none());
    }

    #[test]
    fn build_key_prefix_round_trip_single_byte_cf() {
        let prefix = build_key_prefix(7, 42);
        assert_eq!(&prefix[..], &[0x07, 0x00, 0x00, 0x00, 0x2a]);
        let parsed = parse_key_prefix(&prefix).expect("parse");
        assert_eq!(parsed.cf_id, 7);
        assert_eq!(parsed.index_id, 42);
        assert_eq!(parsed.prefix_len, 5);
    }

    #[test]
    fn build_key_prefix_round_trip_system_cf() {
        // u32::MAX is the system CF id — exercises the 5-byte varint path.
        let prefix = build_key_prefix(u32::MAX, 0xdead_beef);
        let parsed = parse_key_prefix(&prefix).expect("parse");
        assert_eq!(parsed.cf_id, u32::MAX);
        assert_eq!(parsed.index_id, 0xdead_beef);
        assert_eq!(parsed.prefix_len, MAX_VARINT_U32_LEN + INDEX_ID_LEN);
    }

    #[test]
    fn parse_key_prefix_short_input_returns_none() {
        // varint OK (1 byte) but no room for index_id.
        assert!(parse_key_prefix(&[0x05]).is_none());
        assert!(parse_key_prefix(&[]).is_none());
    }

    #[test]
    fn parse_key_prefix_preserves_payload_tail() {
        let mut bytes = Vec::from(&build_key_prefix(3, 11)[..]);
        bytes.extend_from_slice(b"payload-bytes");
        let parsed = parse_key_prefix(&bytes).expect("parse");
        let tail = &bytes[parsed.prefix_len..];
        assert_eq!(tail, b"payload-bytes");
    }

    #[test]
    fn prefix_extractor_returns_prefix_length() {
        let ext = MyRocksPrefixExtractor;
        let mut key = Vec::from(&build_key_prefix(7, 42)[..]);
        key.extend_from_slice(b"row_data");
        let n = ext
            .prefix_len(&PrefixTarget::Point(Bytes::from(key.clone())))
            .expect("Some(n)");
        assert_eq!(n, 5);
        assert_eq!(&key[..n], &[0x07, 0x00, 0x00, 0x00, 0x2a]);

        // Same answer for the Prefix variant.
        let n2 = ext
            .prefix_len(&PrefixTarget::Prefix(Bytes::from(key)))
            .expect("Some(n)");
        assert_eq!(n2, 5);
    }

    #[test]
    fn prefix_extractor_returns_none_for_short_input() {
        let ext = MyRocksPrefixExtractor;
        // varint only, no index_id.
        let short = Bytes::from_static(&[0x05]);
        assert!(ext.prefix_len(&PrefixTarget::Prefix(short.clone())).is_none());
        assert!(ext.prefix_len(&PrefixTarget::Point(short)).is_none());
    }

    #[test]
    fn prefix_extractor_name_is_versioned() {
        assert_eq!(MyRocksPrefixExtractor.name(), "myrocks-v1-cf-index");
    }
}
