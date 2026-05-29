#!/usr/bin/env python3
"""
v4 AST-split: refines v3 with finer-grained classification.

Changes from v3:
1. HA_ROCKSDB_GROUPS expanded with 11 new buckets (auto_incr, read, iter_setup,
   ttl, error, metadata, write_path, bulk_load, buffer, key_compare, table_mgmt).
   Absorbs the 77 methods that v3 dumped into __other.
2. ha_rocksdb.cc free functions (199 of them, 3153 LoC) get a second-level
   classifier instead of all landing in ____free_functions.
3. Rdb_key_def (rdb_datadic.cc, 59 methods, 2015 LoC) gets a codec-direction
   classifier: encode / decode / meta.
4. rdb_i_s.cc (deferred in v3) gets per-information_schema-table extraction.
   13 tables, each with its fields_info array + fill_table fn + init fn +
   st_maria_plugin descriptor + RDB_<TABLE>_FIELD namespace.

This script reads the v3 manifest, replaces the affected sub_units, and writes
back. Idempotent — re-running produces the same result.
"""
from __future__ import annotations

import json
import shlex
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

import clang.cindex as cc

REPO = Path(__file__).resolve().parents[3]
MANIFEST = REPO / "storage" / "slatedb" / "migration" / "manifest.json"
COMPILE_COMMANDS = REPO / "build" / "compile_commands.json"


# --- classifier tables -----------------------------------------------------

# ha_rocksdb method → functional group. First match wins.
# A name `n` in the list matches a method `m` if: m == n, or m.startswith(n + "_"),
# or m.startswith("ha_" + n).
HA_ROCKSDB_GROUPS: list[tuple[str, list[str]]] = [
    # --- v3 groups, preserved ---
    ("lifecycle", ["open", "close", "external_lock", "lock_count", "store_lock", "extra", "init", "reset"]),
    ("ddl",       ["create", "delete_table", "rename_table", "truncate_table", "discard_or_import_tablespace", "set_keys_for_scanning"]),
    ("dml",       ["write_row", "update_row", "delete_row", "delete_all_rows", "bulk_insert", "start_bulk_insert", "end_bulk_insert", "is_using_full_unique_key"]),
    ("scan",      ["rnd_init", "rnd_next", "rnd_pos", "rnd_end", "position", "rnd_pos_by_record"]),
    ("index",     ["index_init", "index_read", "index_next", "index_prev", "index_first", "index_last", "index_end", "read_range_first", "read_range_next", "read_first_row", "ft_init", "ft_read", "ft_end", "calc_eq_cond_len"]),
    ("info",      ["info", "records_in_range", "scan_time", "keyread_time", "rnd_pos_time", "estimate", "table_flags", "index_flags", "max_supported", "get_real_row_type", "primary_key_is_clustered", "ha_table_flags", "key_used_on_scan", "extra_rec_buf_length", "table_cache_type"]),
    ("alter",     ["check_if_supported_inplace_alter", "prepare_inplace_alter_table", "inplace_alter_table", "commit_inplace_alter_table", "notify_table_changed"]),
    ("txn",       ["start_stmt", "end_stmt", "savepoint_set", "savepoint_rollback", "savepoint_release", "register_in_psi", "unregister_from_psi", "ha_rocksdb"]),
    ("repair",    ["check", "repair", "analyze", "optimize", "is_crashed", "auto_repair"]),
    ("convert",   ["convert_record_to_storage_format", "convert_record_from_storage_format", "convert_blob", "convert_field"]),
    ("locks",     ["check_keyread_allowed", "check_against_default_value", "check_if_incompatible_data", "build_decoder_for_pk", "build_decoder_for_sk"]),

    # --- v4 additions: absorb the 77 methods previously in __other ---
    # Use exact names (no prefix matching) to avoid stepping on existing groups.
    ("auto_incr", ["load_auto_incr_value", "load_auto_incr_value_from_index",
                   "update_auto_incr_val", "update_auto_incr_val_from_field",
                   "load_hidden_pk_value", "update_hidden_pk_val",
                   "read_hidden_pk_id_from_rowkey", "has_hidden_pk", "is_hidden_pk",
                   "pk_index", "is_pk", "get_pk_for_update", "get_auto_increment"]),
    ("read",      ["read_key_exact", "read_before_key", "read_after_key",
                   "read_row_from_primary_key", "read_row_from_secondary_key",
                   "secondary_index_read", "prepare_index_scan", "prepare_range_scan",
                   "find_icp_matching_index_rec", "get_row_by_rowid", "get_for_update"]),
    ("iter_setup", ["setup_iterator_bounds", "setup_scan_iterator",
                    "release_scan_iterator", "setup_iterator_for_rnd_scan"]),
    ("ttl",       ["should_hide_ttl_rec", "rocksdb_skip_expired_records",
                   "should_skip_invalidated_record"]),
    ("error",     ["get_error_message", "rdb_error_to_mysql", "print_error"]),
    ("metadata",  ["get_table_basename", "get_key_name", "get_key_comment",
                   "generate_cf_name", "get_table_comment", "get_table_if_exists",
                   "contains_foreign_key"]),
    ("write_path", ["update_write_pk", "update_write_sk", "update_write_indexes",
                    "update_write_row", "delete_or_singledelete", "skip_unique_check",
                    "can_use_single_delete", "can_assume_tracked", "unlock_row",
                    "is_blind_delete_enabled"]),
    ("bulk_load_helpers", ["bulk_load_key", "finalize_bulk_load",
                           "do_bulk_commit", "commit_in_the_middle"]),
    ("buffer",    ["alloc_key_buffers", "free_key_buffers",
                   "set_last_rowkey", "set_skip_unique_check_tables"]),
    ("key_compare", ["compare_keys", "compare_key_parts",
                     "get_old_key_positions", "is_using_full_key"]),
    ("table_mgmt", ["update_create_info", "update_stats", "calculate_stats_for_table",
                    "truncate", "remove_rows", "table_version", "calc_updated_indexes",
                    "get_range", "idx_cond_push", "inplace_populate_sk",
                    "should_recreate_snapshot", "can_use_bloom_filter",
                    "index_blocks", "read_thd_vars"]),
]


# ha_rocksdb.cc free-function name → group. First match wins.
# Names ending in "_" are treated as prefixes; bare names as exact matches.
FREE_FUNC_GROUPS: list[tuple[str, list[str]]] = [
    ("show_callbacks", ["rocksdb_show_", "myrocks_update_status", "myrocks_update_memory_status",
                        "show_myrocks_vars", "show_rocksdb_stall_vars",
                        "update_rocksdb_stall_status", "io_stall_prop_value"]),
    ("sysvar_set",     ["rocksdb_set_", "rdb_set_", "rocksdb_validate_",
                        "rocksdb_check_bulk_load", "rocksdb_check_bulk_load_allow_unsorted",
                        "mysql_value_to_bool"]),
    ("txn_handlers",   ["rocksdb_commit", "rocksdb_commit_ordered", "rocksdb_commit_by_xid",
                        "rocksdb_rollback", "rocksdb_rollback_by_xid",
                        "rocksdb_rollback_to_savepoint", "rocksdb_rollback_to_savepoint_can_release_mdl",
                        "rocksdb_savepoint", "rocksdb_prepare", "rocksdb_recover",
                        "rocksdb_register_tx", "rocksdb_start_tx_and_assign_read_view",
                        "rocksdb_close_connection", "rocksdb_checkpoint_request",
                        "get_tx_from_thd", "get_or_create_tx", "thd_mark_transaction_to_rollback"]),
    ("lifecycle",      ["rocksdb_init_func", "rocksdb_done_func", "rocksdb_create_handler",
                        "rocksdb_check_version", "rocksdb_update_optimizer_costs",
                        "check_rocksdb_options_compatibility"]),
    ("cf_ops",         ["rocksdb_compact_", "rocksdb_flush_", "rocksdb_force_flush_",
                        "rocksdb_create_checkpoint", "rocksdb_create_checkpoint_stub",
                        "rocksdb_drop_index_", "rocksdb_delete_column_family",
                        "rocksdb_delete_column_family_stub",
                        "rocksdb_remove_mariabackup_checkpoint", "getCompactRangeOptions",
                        "rdb_init_rocksdb_db_options", "rocksdb_smart_seek",
                        "rocksdb_smart_next", "rocksdb_perf_context_level",
                        "rdb_get_rocksdb_write_options"]),
    ("dbug_helpers",   ["dbug_", "rdb_dbug_"]),
    ("error_helpers",  ["print_keydup_error", "print_stats", "format_string",
                        "timeout_message", "rdb_get_error_messages", "rdb_handle_io_error",
                        "get_rdb_io_error_string", "rdb_xid_to_string", "rdb_xid_from_string",
                        "rdb_get_all_trx_info", "rdb_get_deadlock_info",
                        "sql_print_verbose_info"]),
    ("status_helpers", ["rocksdb_show_status", "rdb_get_open_table_names",
                        "rdb_update_global_stats", "rdb_get_table_perf_counters"]),
    ("name_helpers",   ["rdb_normalize_", "rdb_normalize_dir",
                        "rdb_split_normalized_tablename",
                        "rdb_field_uses_nopad_collation",
                        "rdb_is_index_collation_supported"]),
    ("table_version",  ["make_table_version_lookup_key", "save_table_version",
                        "get_table_version", "delete_table_version",
                        "rdb_queue_save_stats_request"]),
    ("accessors",      ["rdb_get_", "rdb_is_", "is_valid", "get_range",
                        "is_myrocks_index_empty", "calculate_stats",
                        "can_hold_read_locks_on_select", "rdb_corruption_marker_file_name",
                        "rmdir_force"]),
]


# Rdb_key_def method → codec direction. First match wins.
KEY_DEF_GROUPS: list[tuple[str, list[str]]] = [
    ("encode", ["pack_index_tuple", "pack_field", "pack_record", "pack_hidden_pk",
                "pack_with_make_sort_key", "pack_legacy_variable_format",
                "pack_variable_format", "pack_with_varchar_encoding",
                "pack_with_varchar_space_pad", "write_index_flag_field"]),
    ("decode", ["unpack_info_has_checksum", "unpack_record", "unpack_integer",
                "unpack_floating_point", "unpack_double", "unpack_float",
                "unpack_newdate", "unpack_binary_str", "unpack_utf8_str",
                "unpack_binary_or_utf8_varchar", "unpack_binary_or_utf8_varchar_space_pad",
                "unpack_unknown", "unpack_unknown_varchar",
                "unpack_simple_varchar_space_pad", "unpack_simple",
                "make_unpack_unknown", "make_unpack_unknown_varchar",
                "make_unpack_simple_varchar", "make_unpack_simple",
                "dummy_make_unpack_info", "calc_unpack_legacy_variable_format",
                "calc_unpack_variable_format", "skip_max_length",
                "skip_variable_length", "skip_variable_space_pad",
                "read_memcmp_key_part", "get_unpack_header_size",
                "get_lookup_bitmap", "covers_lookup", "can_cover_lookup"]),
    ("meta",   []),  # everything else
]


# rdb_i_s.cc tables. The decl-name prefix `rdb_i_s_<table>_` and the namespace
# `RDB_<TABLE.upper()>_FIELD` both map back to the table key.
I_S_TABLES = [
    "cfstats", "dbstats", "perf_context", "perf_context_global",
    "cfoptions", "global_info", "compact_stats", "ddl",
    "sst_props", "index_file_map", "lock_info", "trx_info", "deadlock_info",
]


# --- classifier helpers ----------------------------------------------------

def _match(name: str, patterns: list[str]) -> bool:
    """A pattern ending in '_' is a prefix; otherwise exact."""
    for p in patterns:
        if p.endswith("_"):
            if name.startswith(p):
                return True
        else:
            if name == p:
                return True
    return False


def classify_ha_rocksdb_method(klass: str, method: str) -> str:
    """For ha_rocksdb methods: HA_ROCKSDB_GROUPS first-match. Other classes pass through."""
    if klass != "ha_rocksdb":
        return klass
    for group, names in HA_ROCKSDB_GROUPS:
        for n in names:
            if method == n or method.startswith(n + "_") or method.startswith("ha_" + n):
                return f"ha_rocksdb__{group}"
    return "ha_rocksdb__other"


def classify_free_function(name: str) -> str:
    for group, patterns in FREE_FUNC_GROUPS:
        if _match(name, patterns):
            return f"__free__{group}"
    return "__free__misc"


def classify_datadic_method(klass: str, method: str) -> str:
    """For Rdb_key_def: codec direction split. Other classes pass through."""
    if klass != "Rdb_key_def":
        return klass
    for group, patterns in KEY_DEF_GROUPS:
        if _match(method, patterns):
            return f"Rdb_key_def__{group}"
    return "Rdb_key_def__meta"


# Three namespaces use abbreviated/legacy names that don't match table key.
I_S_NAMESPACE_ALIASES = {
    "RDB_LOCKS_FIELD":    "lock_info",
    "RDB_TRX_FIELD":      "trx_info",
    "RDB_DEADLOCK_FIELD": "deadlock_info",
}


def classify_i_s_decl(name: str) -> str:
    """`rdb_i_s_<table>_*` or `rdb_i_s_<table>` → <table>.
    Also `rdb_<table>_*` (no i_s_) → <table>, which catches helpers like
    `rdb_global_info_fill_row`. Else __shared."""
    # Match longest table name (perf_context_global must beat perf_context).
    sorted_tables = sorted(I_S_TABLES, key=len, reverse=True)
    if name.startswith("rdb_i_s_"):
        rest = name[len("rdb_i_s_"):]
        for t in sorted_tables:
            if rest == t or rest.startswith(t + "_"):
                return t
        return "__shared"
    if name.startswith("rdb_"):
        rest = name[len("rdb_"):]
        for t in sorted_tables:
            if rest.startswith(t + "_"):
                return t
    return "__shared"


def classify_i_s_namespace(name: str) -> str:
    """`RDB_<TABLE.upper()>_FIELD` → <table>, plus three legacy aliases."""
    if name in I_S_NAMESPACE_ALIASES:
        return I_S_NAMESPACE_ALIASES[name]
    if not (name.startswith("RDB_") and name.endswith("_FIELD")):
        return "__shared"
    middle = name[len("RDB_"):-len("_FIELD")].lower()
    for t in sorted(I_S_TABLES, key=len, reverse=True):
        if middle == t:
            return t
    return "__shared"


# --- libclang plumbing -----------------------------------------------------

def parse_args_for(target: Path) -> list[str]:
    ccmd = json.loads(COMPILE_COMMANDS.read_text())
    lookup = target if target.suffix == ".cc" else target.with_suffix(".cc")
    entry = next((e for e in ccmd if e["file"] == str(lookup)), None)
    if not entry:
        raise SystemExit(f"no compile_commands entry for {lookup}")
    tokens = shlex.split(entry["command"])
    args = [a for a in tokens[1:] if a != entry["file"]]
    resource_dir = subprocess.check_output(["clang", "-print-resource-dir"]).decode().strip()
    return ["-resource-dir", resource_dir] + args


def parse_tu(target_abs: Path):
    args = parse_args_for(target_abs)
    is_impl = target_abs.suffix == ".cc"
    tu_target = target_abs if is_impl else target_abs.with_suffix(".cc")
    idx = cc.Index.create()
    tu = idx.parse(str(tu_target), args=args)
    errors = [d for d in tu.diagnostics if d.severity >= cc.Diagnostic.Error]
    if errors:
        print(f"  parse errors in {target_abs.name}:", file=sys.stderr)
        for d in errors[:3]:
            print(f"    {d.location}: {d.spelling}", file=sys.stderr)
    return tu, is_impl


def make_in_target(target_abs: Path):
    target_real = str(target_abs.resolve())

    def in_target(node) -> bool:
        f = node.location.file
        if f is None:
            return False
        return Path(str(f)).resolve().as_posix() == target_real

    return in_target


def empty_group() -> dict:
    return {"methods": [], "classes": set(), "body_loc": 0,
            "first_line": None, "last_line": None}


def record(g: dict, klass: str, name: str, start: int, end: int) -> None:
    g["methods"].append({"class": klass, "name": name, "line": start})
    g["classes"].add(klass)
    g["body_loc"] += end - start + 1
    g["first_line"] = start if g["first_line"] is None else min(g["first_line"], start)
    g["last_line"] = end if g["last_line"] is None else max(g["last_line"], end)


def group_to_subunit(parent_id: str, gid: str, info: dict, kind: str, source_file: str) -> dict:
    return {
        "id": f"{parent_id}__{gid}",
        "parent": parent_id,
        "kind": kind,
        "source_file": source_file,
        "body_loc": info["body_loc"],
        "span_range": [info["first_line"], info["last_line"]],
        "classes": sorted(info["classes"]),
        "method_count": len(info["methods"]),
        "methods": [m["name"] for m in info["methods"]],
        "needs_split": info["body_loc"] > 2 * 500,
    }


# --- per-parent splitters --------------------------------------------------

def split_ha_rocksdb_cc(target_abs: Path, parent_id: str) -> list[dict]:
    """ha_rocksdb.cc: methods → HA_ROCKSDB_GROUPS, free fns → FREE_FUNC_GROUPS."""
    tu, _ = parse_tu(target_abs)
    in_target = make_in_target(target_abs)
    groups: dict[str, dict] = defaultdict(empty_group)

    for node in tu.cursor.walk_preorder():
        if not in_target(node):
            continue

        if node.kind == cc.CursorKind.CXX_METHOD and node.is_definition():
            sem = node.semantic_parent
            owner = sem.spelling if sem and sem.kind in (cc.CursorKind.CLASS_DECL,
                                                          cc.CursorKind.STRUCT_DECL) else "(free)"
            gid = classify_ha_rocksdb_method(owner, node.spelling)
            record(groups[gid], owner, node.spelling,
                   node.extent.start.line, node.extent.end.line)

        elif node.kind == cc.CursorKind.FUNCTION_DECL and node.is_definition():
            sem = node.semantic_parent
            if sem and sem.kind in (cc.CursorKind.CLASS_DECL, cc.CursorKind.STRUCT_DECL):
                continue
            gid = classify_free_function(node.spelling)
            record(groups[gid], "(free)", node.spelling,
                   node.extent.start.line, node.extent.end.line)

    source = str(target_abs.relative_to(REPO))
    return [group_to_subunit(parent_id, gid, info, "sub_impl", source)
            for gid, info in sorted(groups.items(), key=lambda kv: (kv[1]["first_line"] or 0))
            if info["first_line"] is not None]


def split_rdb_datadic_cc(target_abs: Path, parent_id: str) -> list[dict]:
    """rdb_datadic.cc: methods grouped by class, with Rdb_key_def split by codec direction."""
    tu, _ = parse_tu(target_abs)
    in_target = make_in_target(target_abs)
    groups: dict[str, dict] = defaultdict(empty_group)

    for node in tu.cursor.walk_preorder():
        if not in_target(node):
            continue

        if node.kind == cc.CursorKind.CXX_METHOD and node.is_definition():
            sem = node.semantic_parent
            owner = sem.spelling if sem and sem.kind in (cc.CursorKind.CLASS_DECL,
                                                          cc.CursorKind.STRUCT_DECL) else "(free)"
            gid = classify_datadic_method(owner, node.spelling)
            record(groups[gid], owner, node.spelling,
                   node.extent.start.line, node.extent.end.line)

        elif node.kind == cc.CursorKind.FUNCTION_DECL and node.is_definition():
            sem = node.semantic_parent
            if sem and sem.kind in (cc.CursorKind.CLASS_DECL, cc.CursorKind.STRUCT_DECL):
                continue
            record(groups["__free_functions"], "(free)", node.spelling,
                   node.extent.start.line, node.extent.end.line)

    source = str(target_abs.relative_to(REPO))
    return [group_to_subunit(parent_id, gid, info, "sub_impl", source)
            for gid, info in sorted(groups.items(), key=lambda kv: (kv[1]["first_line"] or 0))
            if info["first_line"] is not None]


def split_header(target_abs: Path, parent_id: str) -> list[dict]:
    """Header (.h): each class/struct definition becomes one sub-unit."""
    tu, _ = parse_tu(target_abs)
    in_target = make_in_target(target_abs)
    groups: dict[str, dict] = defaultdict(empty_group)

    for node in tu.cursor.walk_preorder():
        if not in_target(node):
            continue
        if node.kind in (cc.CursorKind.CLASS_DECL, cc.CursorKind.STRUCT_DECL) and node.is_definition():
            name = node.spelling or "(anonymous)"
            record(groups[name], name, name, node.extent.start.line, node.extent.end.line)

    source = str(target_abs.relative_to(REPO))
    return [group_to_subunit(parent_id, gid, info, "sub_header", source)
            for gid, info in sorted(groups.items(), key=lambda kv: (kv[1]["first_line"] or 0))
            if info["first_line"] is not None]


def split_rdb_i_s_cc(target_abs: Path, parent_id: str) -> list[dict]:
    """Per-information_schema-table extraction. Walks top-level FUNCTION_DECL,
    VAR_DECL, NAMESPACE, and classifies by name → table key."""
    tu, _ = parse_tu(target_abs)
    in_target = make_in_target(target_abs)
    groups: dict[str, dict] = defaultdict(empty_group)

    # Walk only top-level decls (skip recursing into namespace contents we don't care about,
    # except for the myrocks namespace which holds all the per-table decls).
    def visit(node, depth: int) -> None:
        if depth > 2:  # TU → namespace myrocks → decl is enough
            return
        if not in_target(node) and node.kind != cc.CursorKind.TRANSLATION_UNIT:
            # File-level locations: keep traversing into namespaces even if their
            # opening token is the file we want.
            f = node.location.file
            if f is None or Path(str(f)).resolve().as_posix() != str(target_abs.resolve()):
                # Only skip if we're outside the file entirely
                return

        for child in node.get_children():
            if not in_target(child):
                continue
            k = child.kind
            name = child.spelling or ""
            if k in (cc.CursorKind.FUNCTION_DECL, cc.CursorKind.VAR_DECL):
                tbl = classify_i_s_decl(name)
                record(groups[tbl], "(top)", name,
                       child.extent.start.line, child.extent.end.line)
            elif k == cc.CursorKind.NAMESPACE:
                if name == "":
                    # Anonymous namespace — recurse into it, treat children as top-level.
                    visit(child, depth)
                elif name.startswith("RDB_") and name.endswith("_FIELD"):
                    tbl = classify_i_s_namespace(name)
                    record(groups[tbl], "(ns)", name,
                           child.extent.start.line, child.extent.end.line)
                else:
                    # myrocks namespace (or any other named one) — recurse.
                    visit(child, depth + 1)
            elif k == cc.CursorKind.STRUCT_DECL and name.startswith("rdb_i_s_"):
                tbl = classify_i_s_decl(name)
                record(groups[tbl], "(plugin)", name,
                       child.extent.start.line, child.extent.end.line)

    visit(tu.cursor, 0)

    source = str(target_abs.relative_to(REPO))
    return [group_to_subunit(parent_id, gid, info, "sub_impl", source)
            for gid, info in sorted(groups.items(), key=lambda kv: (kv[1]["first_line"] or 0))
            if info["first_line"] is not None]


# --- driver -----------------------------------------------------------------

SPLITTERS = [
    ("ha_rocksdb_h",   REPO / "storage/rocksdb/ha_rocksdb.h",   split_header),
    ("ha_rocksdb_cc",  REPO / "storage/rocksdb/ha_rocksdb.cc",  split_ha_rocksdb_cc),
    ("rdb_datadic_h",  REPO / "storage/rocksdb/rdb_datadic.h",  split_header),
    ("rdb_datadic_cc", REPO / "storage/rocksdb/rdb_datadic.cc", split_rdb_datadic_cc),
    ("rdb_i_s_cc",     REPO / "storage/rocksdb/rdb_i_s.cc",     split_rdb_i_s_cc),
]


def main() -> None:
    manifest = json.loads(MANIFEST.read_text())
    sub_units: dict[str, list[dict]] = {}
    for parent_id, target, splitter in SPLITTERS:
        print(f"AST-splitting {parent_id} ({target.name})...", flush=True)
        sub_units[parent_id] = splitter(target, parent_id)
        print(f"  -> {len(sub_units[parent_id])} sub-units")

    # Drop the old v3 sub_units and replace.
    manifest["sub_units"] = {s["id"]: s for subs in sub_units.values() for s in subs}
    for parent_id, subs in sub_units.items():
        if parent_id in manifest["units"]:
            manifest["units"][parent_id]["v3_split"] = True
            manifest["units"][parent_id]["v4_split"] = True
            manifest["units"][parent_id]["sub_units"] = [s["id"] for s in subs]
    manifest["v3"] = {
        "split_parents": [p for p, _, _ in SPLITTERS if p != "rdb_i_s_cc"],
        "deferred_to_v4": ["rdb_i_s_cc"],
        "deferred_reason": "rdb_i_s.cc was per-information_schema-table via macros; v4 handles it.",
    }
    manifest["v4"] = {
        "split_parents": [p for p, _, _ in SPLITTERS],
        "refinements": [
            "HA_ROCKSDB_GROUPS expanded to 22 buckets (11 new for ha_rocksdb__other absorption).",
            "ha_rocksdb.cc free functions classified into 11 functional groups.",
            "Rdb_key_def split by codec direction: encode / decode / meta.",
            "rdb_i_s.cc split per-information_schema-table (13 tables).",
        ],
    }
    MANIFEST.write_text(json.dumps(manifest, indent=2) + "\n")
    total_sub = sum(len(s) for s in sub_units.values())
    print(f"wrote {MANIFEST}: {total_sub} sub-units across {len(sub_units)} parents")

    over = [s for subs in sub_units.values() for s in subs if s["needs_split"]]
    if over:
        print("\nSub-units still over 1000-LoC target:")
        for s in sorted(over, key=lambda x: -x["body_loc"]):
            print(f"  {s['body_loc']:5d} LoC  {s['method_count']:4d} methods  {s['id']}")
    else:
        print("\nAll sub-units within 1000-LoC target.")


if __name__ == "__main__":
    main()
