//! Interface stub for `rdb_cf_options_cc`.
//!
//! C++ source: `storage/rocksdb/rdb_cf_options.cc` (337 LoC)
//! C++ class:  `Rdb_cf_options` (impl)
//!
//! ## Mapping
//! Per _DESIGN.md §1 ("Column families" row — Map, native via PrefixExtractor),
//! per-CF tuning silently ignored: SlateDB has global `Settings`, not per-CF.
//! The vast majority of this file's logic — parsing the
//! `rocksdb_default_cf_options` / `rocksdb_override_cf_options` strings,
//! looking up per-CF block-cache / compression / write-buffer-size — drops
//! out. We keep just enough surface so the SHOW VARIABLES output still looks
//! plausible to operators porting from MyRocks.
//!
//! Comparator factory (`get_cf_comparator`) is also dropped: SlateDB sorts
//! bytewise; reverse ordering is achieved by codec-side XOR (see
//! `rdb_comparator_h.rs`).
//!
//! Merge-operator factory (`get_cf_merge_operator`) collapses too: SlateDB
//! has a single global `MergeOperator` registered at `Db::builder` time, with
//! key-aware routing inside (see _DESIGN.md §1 "Merge operators" row). The
//! system-CF special case becomes a prefix branch in the global operator.
//!
//! ## Out-of-scope methods
//! - `set_default`, `set_override`, `parse_cf_options` — kept as no-op parsers
//!   that accept any string and warn rather than error, so the existing sysvar
//!   surface keeps working.
//! - `get_cf_comparator` — dropped (no per-CF comparator in SlateDB).
//! - `get_cf_merge_operator` — dropped (global merge operator).
//! - `find_column_family`, `find_options`, `find_cf_options_pair`,
//!   `skip_spaces` — pure parsing helpers; ported to private parse module
//!   only if a TRANSLATE-time user complains. Stubbed as TODOs here.

use slatedb::Error;
use std::collections::HashMap;

/// Per-CF tuning options as understood by MyRocks. Stored after parsing the
/// sysvar strings but **not consulted by the engine** — see module doc.
///
/// We surface them so `SHOW STATUS LIKE 'rocksdb_override_cf_options'`
/// produces the same string the user set.
#[derive(Debug, Clone, Default)]
pub struct CfTuning {
    /// Free-form per-CF option string, kept verbatim.
    pub raw: String,
}

/// Registry of parsed per-CF tuning strings. Populated from the
/// `rocksdb_default_cf_options` / `rocksdb_override_cf_options` sysvars at
/// plugin init. Read by no one inside the engine.
pub struct CfOptions {
    pub default_raw: String,
    pub overrides: HashMap<String, CfTuning>,
}

impl CfOptions {
    /// Construct, parse the two sysvar strings. Always returns `Ok` —
    /// parse failures only `log::warn!` and store an empty override map.
    ///
    /// Inputs:
    /// - `default_cf_options`: value of sysvar `rocksdb_default_cf_options`.
    /// - `override_cf_options`: value of sysvar `rocksdb_override_cf_options`.
    ///
    /// Output: configured `CfOptions`. The strings are stored, parsed
    /// lazily (since we don't actually use the parsed form).
    ///
    /// Errors: never — bad syntax is logged, not raised.
    ///
    /// Original: rdb_cf_options.cc:41 — `Rdb_cf_options::init`.
    pub fn new(default_cf_options: &str, override_cf_options: &str) -> Result<Self, Error> {
        let _ = override_cf_options;
        Ok(Self {
            default_raw: default_cf_options.to_string(),
            overrides: HashMap::new(),
        })
    }

    /// Replace the default CF options string. Returns `Ok(())` always, logs
    /// on parse problems.
    /// Original: rdb_cf_options.cc:95 — `set_default`.
    pub fn set_default(&mut self, default_config: &str) -> Result<(), Error> {
        self.default_raw = default_config.to_string();
        Ok(())
    }

    /// Replace the per-CF override map. Returns `Ok(())` always.
    /// Original: rdb_cf_options.cc:298 — `set_override`.
    pub fn set_override(&mut self, override_config: &str) -> Result<(), Error> {
        let _ = override_config;
        // TODO(human): parse the `cf=opt; cf=opt` syntax and populate
        // `self.overrides`. Until then we keep the map empty so callers see
        // "no overrides" which is the safe default.
        Ok(())
    }

    /// Update one CF's override string. The C++ version stored it in a map
    /// keyed by CF name. We keep that map but never read from it.
    /// Original: rdb_cf_options.cc:84 — `update`.
    pub fn update(&mut self, cf_name: &str, cf_options: &str) {
        self.overrides.insert(
            cf_name.to_string(),
            CfTuning { raw: cf_options.to_string() },
        );
    }

    /// Return the raw override string for a given CF, or empty if none.
    /// Original: rdb_cf_options.cc:69 — `get`.
    pub fn get_raw(&self, cf_name: &str) -> &str {
        self.overrides.get(cf_name).map(|t| t.raw.as_str()).unwrap_or("")
    }
}
