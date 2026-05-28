#!/usr/bin/env python3
"""
v3 AST-split: for each needs_split unit in manifest.json (v2), use libclang
to extract per-class sub-units. For ha_rocksdb_cc's giant ha_rocksdb class
(~155 methods), further split by method-name prefix into functional groups.

rdb_i_s.cc is deliberately not split here — its structure is per-information_schema
table, driven by macros, not per-class. Separate v4 needed; flagged in manifest.
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

# For each method belonging to ha_rocksdb (the 155-method handler subclass),
# the first matching group wins. Prefixes / exact names from the MariaDB handler
# vtable; anything not matched lands in __other.
HA_ROCKSDB_GROUPS: list[tuple[str, list[str]]] = [
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
]


def classify_method(klass: str, method: str) -> str:
    """For ha_rocksdb's methods, sub-split into functional groups; pass through otherwise."""
    if klass != "ha_rocksdb":
        return klass
    for group, names in HA_ROCKSDB_GROUPS:
        for n in names:
            if method == n or method.startswith(n + "_") or method.startswith("ha_" + n):
                return f"ha_rocksdb__{group}"
    return "ha_rocksdb__other"


def parse_args_for(target: Path) -> list[str]:
    """Look up the compile_commands entry for `target`. For .h files (which never
    appear in compile_commands directly), use the paired .cc's args."""
    ccmd = json.loads(COMPILE_COMMANDS.read_text())
    lookup = target
    if target.suffix == ".h":
        lookup = target.with_suffix(".cc")
    entry = next((e for e in ccmd if e["file"] == str(lookup)), None)
    if not entry:
        raise SystemExit(f"no compile_commands entry for {lookup}")
    tokens = shlex.split(entry["command"])
    args = [a for a in tokens[1:] if a != entry["file"]]
    resource_dir = subprocess.check_output(["clang", "-print-resource-dir"]).decode().strip()
    return ["-resource-dir", resource_dir] + args


def split_unit(target_abs: Path, parent_id: str) -> list[dict]:
    """Returns a list of sub-unit dicts for the given file.

    For .cc files (impl): groups CXX_METHOD definitions by their semantic_parent class
    (with the ha_rocksdb special-case for functional sub-grouping).
    For .h files: groups CLASS_DECL/STRUCT_DECL definitions; each class is one sub-unit
    sized by the byte-extent of its definition. We parse the paired .cc as the TU but
    filter AST nodes by the header's path."""
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

    target_real = str(target_abs.resolve())  # filter AST to nodes in *this* file (header or impl)

    def in_target(node) -> bool:
        f = node.location.file
        if f is None:
            return False
        # libclang may report paths with "./" components from include directives;
        # normalize both sides before comparing.
        return Path(str(f)).resolve().as_posix() == target_real

    # group_id -> {methods: [{class,name,start,end}], classes: set, body_loc: int}
    groups: dict[str, dict] = defaultdict(lambda: {"methods": [], "classes": set(), "body_loc": 0,
                                                    "first_line": None, "last_line": None})

    def record(g: dict, klass: str, name: str, start: int, end: int) -> None:
        g["methods"].append({"class": klass, "name": name, "line": start})
        g["classes"].add(klass)
        g["body_loc"] += end - start + 1
        g["first_line"] = start if g["first_line"] is None else min(g["first_line"], start)
        g["last_line"] = end if g["last_line"] is None else max(g["last_line"], end)

    for node in tu.cursor.walk_preorder():
        if not in_target(node):
            continue

        if is_impl and node.kind == cc.CursorKind.CXX_METHOD and node.is_definition():
            sem = node.semantic_parent
            owner = sem.spelling if sem and sem.kind in (cc.CursorKind.CLASS_DECL, cc.CursorKind.STRUCT_DECL) else "(free)"
            gid = classify_method(owner, node.spelling)
            record(groups[gid], owner, node.spelling, node.extent.start.line, node.extent.end.line)

        elif not is_impl and node.kind in (cc.CursorKind.CLASS_DECL, cc.CursorKind.STRUCT_DECL) and node.is_definition():
            name = node.spelling or "(anonymous)"
            record(groups[name], name, name, node.extent.start.line, node.extent.end.line)

    if is_impl:
        for node in tu.cursor.walk_preorder():
            if not in_target(node):
                continue
            if node.kind == cc.CursorKind.FUNCTION_DECL and node.is_definition():
                sem = node.semantic_parent
                if sem and sem.kind in (cc.CursorKind.CLASS_DECL, cc.CursorKind.STRUCT_DECL):
                    continue
                record(groups["__free_functions"], "(free)", node.spelling,
                       node.extent.start.line, node.extent.end.line)

    out: list[dict] = []
    for gid, info in sorted(groups.items(), key=lambda kv: (kv[1]["first_line"] or 0)):
        if info["first_line"] is None:
            continue
        out.append({
            "id": f"{parent_id}__{gid}",
            "parent": parent_id,
            "kind": "sub_impl" if is_impl else "sub_header",
            "source_file": str(target_abs.relative_to(REPO)),
            "body_loc": info["body_loc"],
            "span_range": [info["first_line"], info["last_line"]],
            "classes": sorted(info["classes"]),
            "method_count": len(info["methods"]),
            "methods": [m["name"] for m in info["methods"]],
            "needs_split": info["body_loc"] > 2 * 500,
        })
    return out


# Map manifest unit id -> source path (the file we'll AST-parse)
TARGETS_FOR = {
    "ha_rocksdb_h":   REPO / "storage/rocksdb/ha_rocksdb.h",
    "ha_rocksdb_cc":  REPO / "storage/rocksdb/ha_rocksdb.cc",
    "rdb_datadic_h":  REPO / "storage/rocksdb/rdb_datadic.h",
    "rdb_datadic_cc": REPO / "storage/rocksdb/rdb_datadic.cc",
    # rdb_i_s_cc intentionally skipped — needs per-table extraction, not per-class
}


def main() -> None:
    manifest = json.loads(MANIFEST.read_text())
    sub_units: dict[str, list[dict]] = {}
    for parent_id, target in TARGETS_FOR.items():
        print(f"AST-splitting {parent_id} ({target.name})...", flush=True)
        sub_units[parent_id] = split_unit(target, parent_id)
        print(f"  -> {len(sub_units[parent_id])} sub-units")

    # Mark parents in the original units dict, and store sub-units alongside
    for parent_id, subs in sub_units.items():
        if parent_id in manifest["units"]:
            manifest["units"][parent_id]["v3_split"] = True
            manifest["units"][parent_id]["sub_units"] = [s["id"] for s in subs]

    manifest["sub_units"] = {s["id"]: s for subs in sub_units.values() for s in subs}
    manifest["v3"] = {
        "split_parents": list(TARGETS_FOR.keys()),
        "deferred_to_v4": ["rdb_i_s_cc"],
        "deferred_reason": "rdb_i_s.cc is per-information_schema-table via macros, not per-class; needs different heuristic",
    }
    MANIFEST.write_text(json.dumps(manifest, indent=2) + "\n")
    total_sub = sum(len(s) for s in sub_units.values())
    print(f"wrote {MANIFEST}: {total_sub} sub-units added across {len(sub_units)} parents")


if __name__ == "__main__":
    main()
