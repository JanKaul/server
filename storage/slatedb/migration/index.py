#!/usr/bin/env python3
"""
INDEX phase tool (§5 of SlateDB_storage_engine.md).

Reads build/compile_commands.json, walks storage/rocksdb/*.{cc,h},
computes per-file LoC + classes + #include dependencies + tags, topo-sorts,
and emits migration/manifest.json.

v1 scope: file-level translation units. Megafiles
(ha_rocksdb.cc, rdb_datadic.cc, rdb_i_s.cc) are emitted as single units
with a `needs_split` flag so the INTERFACE phase knows to split them by
class. AST-driven method-cluster splitting is v2.
"""
from __future__ import annotations

import json
import re
import sys
from collections import defaultdict, deque
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
ROCKSDB_DIR = REPO_ROOT / "storage" / "rocksdb"
OUT_PATH = REPO_ROOT / "storage" / "slatedb" / "migration" / "manifest.json"
COMPILE_COMMANDS = REPO_ROOT / "build" / "compile_commands.json"

# Files inside the MyRocks dir we do not want to translate
SKIP_PATTERNS = (
    "rocksdb/",          # the vendored RocksDB submodule
    "tools/",            # mysql_ldb CLI — out of scope for engine migration
    "unittest/",
)

LOC_TARGET = 500  # soft target per §5; megafiles flagged for split

CLASS_RE = re.compile(
    r"^\s*(?:class|struct)\s+([A-Za-z_][A-Za-z0-9_]*)"
    r"(?:\s*:\s*(?:public|protected|private)\s+([A-Za-z_:][A-Za-z0-9_:<>]*))?",
)
INCLUDE_RE = re.compile(r'^\s*#\s*include\s+[<"]([^>"]+)[>"]')
ROCKSDB_API_RE = re.compile(r"\brocksdb\s*::\s*|<rocksdb/")


def read_text(p: Path) -> str:
    try:
        return p.read_text(encoding="utf-8", errors="replace")
    except FileNotFoundError:
        return ""


def collect_files() -> list[Path]:
    """All .cc and .h files in storage/rocksdb/ except skipped subtrees."""
    out: list[Path] = []
    for p in sorted(ROCKSDB_DIR.rglob("*")):
        if not p.is_file() or p.suffix not in (".cc", ".h"):
            continue
        rel = p.relative_to(ROCKSDB_DIR).as_posix()
        if any(rel.startswith(s) for s in SKIP_PATTERNS):
            continue
        out.append(p)
    return out


def loc(p: Path) -> int:
    return sum(1 for _ in read_text(p).splitlines())


def classes_in(p: Path) -> list[dict]:
    """Forward-declarations and definitions. Cheap, not AST-perfect."""
    result: list[dict] = []
    for line in read_text(p).splitlines():
        m = CLASS_RE.match(line)
        if not m:
            continue
        name, base = m.group(1), m.group(2)
        # Skip forward decls "class Foo;" — no body opener on the line
        if line.rstrip().endswith(";"):
            continue
        result.append({"name": name, "base": base})
    return result


def includes_in(p: Path) -> list[str]:
    out: list[str] = []
    for line in read_text(p).splitlines():
        m = INCLUDE_RE.match(line)
        if m:
            out.append(m.group(1))
    return out


def touches_rocksdb_api(p: Path) -> bool:
    return bool(ROCKSDB_API_RE.search(read_text(p)))


def touches_handler_vtable(p: Path, klasses: list[dict]) -> bool:
    # Either the file IS the handler, or it declares a handler subclass.
    if p.name in ("ha_rocksdb.cc", "ha_rocksdb.h"):
        return True
    return any((c.get("base") or "").endswith("handler") for c in klasses)


def unit_id_for(p: Path) -> str:
    """One translation unit per file. .h and .cc are separate units — the .h is
    the interface (depended on by other units), the .cc is the implementation
    (depends on its own .h plus impl-side headers from other units). Splitting
    them breaks the .cc → other.h back-edges that otherwise turn nearly all of
    MyRocks into one mutually-recursive cluster (see reports/index-v1.md)."""
    return f"{p.stem}_{'h' if p.suffix == '.h' else 'cc'}"


def build_units(files: list[Path]) -> dict[str, dict]:
    units: dict[str, dict] = {}
    for p in files:
        uid = unit_id_for(p)
        ks = classes_in(p)
        units[uid] = {
            "id": uid,
            "kind": "header" if p.suffix == ".h" else "impl",
            "source_files": [p.relative_to(REPO_ROOT).as_posix()],
            "loc": loc(p),
            "classes": ks,
            "includes": sorted(set(includes_in(p))),
            "touches_rocksdb_api": touches_rocksdb_api(p),
            "touches_handler_vtable": touches_handler_vtable(p, ks),
            "needs_split": loc(p) > 2 * LOC_TARGET,
            "depends_on": [],
        }
    return units


def resolve_deps(units: dict[str, dict]) -> None:
    """Each unit's `depends_on` lists in-scope _h units it #includes from.
    Implementation units (_cc) always depend on their paired _h."""
    header_to_unit: dict[str, str] = {}
    for u in units.values():
        if u["kind"] == "header":
            header_to_unit[Path(u["source_files"][0]).name] = u["id"]
    for u in units.values():
        deps: set[str] = set()
        for inc in u["includes"]:
            base = Path(inc).name
            other = header_to_unit.get(base)
            if other and other != u["id"]:
                deps.add(other)
        # Each .cc implicitly depends on its own .h even if not listed (it always is, in practice)
        if u["kind"] == "impl":
            stem = u["id"][:-3]  # strip "_cc"
            own_h = f"{stem}_h"
            if own_h in units:
                deps.add(own_h)
        u["depends_on"] = sorted(deps)


def tarjan_sccs(units: dict[str, dict]) -> list[list[str]]:
    """Strongly-connected components of the include graph. Each SCC of size > 1
    is a tangle of units with mutually-recursive interfaces — INTERFACE must
    propose them together. Returned in reverse topological order (leaves last
    in Tarjan's output, which is what we want: a cluster's deps come *before*
    it in the result)."""
    index = [0]
    stack: list[str] = []
    on_stack: set[str] = set()
    indexes: dict[str, int] = {}
    lowlink: dict[str, int] = {}
    sccs: list[list[str]] = []

    def strongconnect(v: str) -> None:
        indexes[v] = lowlink[v] = index[0]
        index[0] += 1
        stack.append(v)
        on_stack.add(v)
        for w in sorted(units[v]["depends_on"]):
            if w not in indexes:
                strongconnect(w)
                lowlink[v] = min(lowlink[v], lowlink[w])
            elif w in on_stack:
                lowlink[v] = min(lowlink[v], indexes[w])
        if lowlink[v] == indexes[v]:
            scc: list[str] = []
            while True:
                w = stack.pop()
                on_stack.remove(w)
                scc.append(w)
                if w == v:
                    break
            sccs.append(sorted(scc))

    sys.setrecursionlimit(10000)
    for v in sorted(units):
        if v not in indexes:
            strongconnect(v)
    return sccs


def cluster_topo(units: dict[str, dict], sccs: list[list[str]]) -> list[list[str]]:
    """Tarjan emits SCCs in reverse topological order; just return as-is — earlier
    SCCs have no dependencies on later ones."""
    return sccs


def main() -> None:
    if not COMPILE_COMMANDS.exists():
        sys.exit(f"missing {COMPILE_COMMANDS} — run cmake -DCMAKE_EXPORT_COMPILE_COMMANDS=ON")
    files = collect_files()
    units = build_units(files)
    resolve_deps(units)
    sccs = tarjan_sccs(units)
    clusters = cluster_topo(units, sccs)
    clusters_out = []
    for i, scc in enumerate(clusters):
        clusters_out.append({
            "cluster_id": f"c{i:02d}",
            "size": len(scc),
            "loc": sum(units[u]["loc"] for u in scc),
            "members": scc,
            "needs_split_in_interface_phase": len(scc) > 1,
        })
    manifest = {
        "schema_version": 1,
        "generator": "storage/slatedb/migration/index.py",
        "source_root": str(ROCKSDB_DIR.relative_to(REPO_ROOT)),
        "loc_target": LOC_TARGET,
        "notes": [
            "v1: file-level units; megafiles flagged needs_split for INTERFACE phase.",
            "Deps are from #include graph only; no call-graph yet (TRANSLATE-phase concern).",
            "touches_rocksdb_api / touches_handler_vtable are heuristic; verify in INTERFACE.",
            "Clusters are SCCs of the include graph; size > 1 means the cluster's interfaces",
            "are mutually recursive and must be proposed together in one INTERFACE batch.",
            "Clusters are listed in reverse-topological order (deps before dependents).",
        ],
        "clusters": clusters_out,
        "units": {uid: units[uid] for uid in sorted(units)},
    }
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(manifest, indent=2) + "\n")
    big = [c for c in clusters_out if c["size"] > 1]
    print(f"wrote {OUT_PATH}: {len(units)} units in {len(clusters_out)} clusters "
          f"({len(big)} multi-unit, total {sum(u['loc'] for u in units.values())} LoC)")
    if big:
        for c in big:
            print(f"  cluster {c['cluster_id']}: {c['size']} units, {c['loc']} LoC — {c['members']}")


if __name__ == "__main__":
    main()
