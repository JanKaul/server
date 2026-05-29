//! Interface stub for `rdb_cf_options_h`.
//!
//! C++ source: `storage/rocksdb/rdb_cf_options.h` (104 LoC)
//! C++ class: `Rdb_cf_options`
//!
//! ## Mapping
//! Per _DESIGN.md §1 ("Block cache / Bloom filters / Compression → Map
//! (degraded)") SlateDB has **one global** block cache, filter policy, and
//! compression codec — there is no per-CF tuning. The MyRocks per-CF options
//! parsing surface lives on for backwards compatibility, but the parsed
//! options are silently dropped (with a `WARN`-level log) when they conflict
//! with SlateDB's single-global model.
//!
//! What remains real:
//! - **CF name parser** — splits `rocksdb_default_cf_options` /
//!   `rocksdb_override_cf_options` sysvars into `{cf_name → opts_string}`
//!   pairs. We keep this because the sysvar grammar is documented and users
//!   set it.
//! - **`get_cf_options(name)`** — returns a `CfOptionsSnapshot` struct with
//!   the requested options _as far as SlateDB supports them_. Unsupported
//!   knobs are silently ignored (per design).
//! - **Comparator selection** — `Rdb_pk_comparator` vs `Rdb_rev_comparator`
//!   is now a `KeyDirection` flag (see `rdb_comparator_h`).
//!
//! ## Out-of-scope methods
//! - `get_cf_comparator` — SlateDB has no per-CF comparator; replaced by
//!   `KeyDirection` carried on the index def.
//! - `get_cf_merge_operator` — SlateDB takes ONE merge operator at
//!   `DbBuilder::with_merge_operator`. Per-CF merge operators are simulated
//!   by key-aware routing inside a single operator (see _DESIGN.md §0,
//!   `KeyPrefixMergeOperator`).
//! - `init(table_options, prop_coll_factory, ...)` — accepts a
//!   RocksDB-specific `BlockBasedTableOptions` + the deprecated properties
//!   collector. Replaced by `init_from_settings(slatedb::Settings)` below.

use slatedb::Error;
use std::collections::HashMap;

/// One CF's configured options, post-parse, in the subset SlateDB respects.
/// Unsupported MyRocks options (e.g., `write_buffer_size`,
/// `level0_file_num_compaction_trigger`) are accepted by the parser but
/// discarded — see the `silently_ignored` field for audit.
#[derive(Debug, Clone, Default)]
pub struct CfOptionsSnapshot {
    pub direction: crate::rdb_comparator_h::KeyDirection,
    /// TTL in seconds, if set via `ttl_duration=N`. Honoured via
    /// SlateDB's native `PutOptions.ttl` (_DESIGN.md §0 TTL bullet).
    pub ttl_seconds: Option<u64>,
    /// Per-CF option keys that we accepted but discarded. Logged at WARN at
    /// CF-create time.
    pub silently_ignored: Vec<String>,
}

/// Container for all CF options — both the `default_cf_options` value
/// (applies to CFs not in the map) and the `override_cf_options` map.
/// Replaces C++ `Rdb_cf_options`.
pub struct CfOptions {
    /// `cf_name -> raw_options_string`, populated from `rocksdb_override_cf_options`.
    name_map: HashMap<String, String>,
    /// Single default options string, applied to any CF not in `name_map`.
    default_config: String,
    /// Parsed default. Updated on `init()`/`update()` mutation.
    default_snapshot: CfOptionsSnapshot,
}

impl CfOptions {
    pub fn new() -> Self {
        Self {
            name_map: HashMap::new(),
            default_config: String::new(),
            default_snapshot: CfOptionsSnapshot::default(),
        }
    }

    /// Parse the two sysvars at startup, build the snapshot for the default
    /// CF, warn about silently-ignored knobs.
    ///
    /// Replaces C++ `Rdb_cf_options::init(table_options, prop_coll, default, override)`.
    pub fn init(
        &mut self,
        default_cf_options: &str,
        override_cf_options: &str,
    ) -> Result<(), Error> {
        self.set_default(default_cf_options)?;
        self.set_override(override_cf_options)?;
        Ok(())
    }

    /// Return the parsed options for a CF by name. Falls back to the default
    /// when the name isn't in the override map.
    /// Replaces C++ `Rdb_cf_options::get_cf_options(name, opts)`.
    pub fn get(&self, cf_name: &str) -> CfOptionsSnapshot {
        if let Some(raw) = self.name_map.get(cf_name) {
            Self::parse_cf_options(raw).unwrap_or_default()
        } else {
            self.default_snapshot.clone()
        }
    }

    /// Live-update one CF's options (called from `SET GLOBAL
    /// rocksdb_update_cf_options=...`). C++ `update(cf_name, options)`.
    pub fn update(&mut self, cf_name: &str, cf_options: &str) -> Result<(), Error> {
        let _ = Self::parse_cf_options(cf_options)?; // validate
        self.name_map.insert(cf_name.to_string(), cf_options.to_string());
        Ok(())
    }

    fn set_default(&mut self, s: &str) -> Result<(), Error> {
        self.default_snapshot = Self::parse_cf_options(s)?;
        self.default_config = s.to_string();
        Ok(())
    }

    fn set_override(&mut self, s: &str) -> Result<(), Error> {
        let map = Self::parse_cf_options_map(s)?;
        self.name_map = map;
        Ok(())
    }

    /// Parse a single CF-options string (`key1=val1;key2=val2;…`). C++
    /// `parse_cf_options(s, option_map)` plus our SlateDB filtering.
    pub fn parse_cf_options(s: &str) -> Result<CfOptionsSnapshot, Error> {
        let mut snap = CfOptionsSnapshot::default();
        for tok in s.split(';').map(str::trim).filter(|t| !t.is_empty()) {
            let Some((k, v)) = tok.split_once('=') else {
                return Err(Error::invalid(format!(
                    "cf_options token missing '=': {tok:?}"
                )));
            };
            match k.trim() {
                "comparator" => {
                    snap.direction = if v.trim().eq_ignore_ascii_case("reverse") {
                        crate::rdb_comparator_h::KeyDirection::Reverse
                    } else {
                        crate::rdb_comparator_h::KeyDirection::Forward
                    };
                }
                "ttl_duration" => {
                    snap.ttl_seconds = Some(v.trim().parse().map_err(|_| {
                        Error::invalid(format!("ttl_duration not u64: {v:?}"))
                    })?);
                }
                // _DESIGN.md §1: per-CF tuning is dropped. Track in `silently_ignored`.
                other => snap.silently_ignored.push(other.to_string()),
            }
        }
        Ok(snap)
    }

    /// Parse the whole `override_cf_options` sysvar:
    /// `cf1={key=val;…};cf2={key=val;…}`.
    fn parse_cf_options_map(_s: &str) -> Result<HashMap<String, String>, Error> {
        todo!("port the brace-balanced parser from rdb_cf_options.cc")
    }

    pub fn default_snapshot(&self) -> &CfOptionsSnapshot { &self.default_snapshot }
}

impl Default for CfOptions {
    fn default() -> Self { Self::new() }
}
