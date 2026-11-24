#!/usr/bin/env python3
"""Architecture gate for one Rust crate.

Checks, from a rules file (TOML):

* cycles      - module-level cycles (strongly connected components, see
                module_graph.py), inline modules included;
* layers      - every dependency edge of a module must stay inside its own subtree
                or point to a module its layer rule allows; the most specific rule
                (longest module path) applies; the facade modules (crate root) are
                exempt;
* unruled     - with `require_rules = true`, every module needs a layer rule; with
                `require_exact_rules = ["core", ...]`, every module in those
                subtrees needs a rule for exactly that module (no inheriting);
* unsupported - module declarations the parser does not follow (`#[path]`, `mod x;`
                without a file);
* crap        - cargo-crap JSON (`--crap`): global CRAP maximum and per-scope
                cyclomatic-complexity maximum. The metrics must be usable: at least
                one entry under `--src`, and at least one function in every scope.
                Without `--crap` the gate is incomplete and fails ("SKIPPED"),
                unless `--skip-crap` is given. Functions in a file that is a
                `cfg(test)` module (or lies below one) are test code and skipped;
* size        - per-scope maximum of non-test code lines per file (comments, blank
                lines and `#[cfg(test)]` modules do not count);
* forbid      - per-scope forbidden code, after removing comments and string/char
                literals:
                  lint = "clippy::x"  the lint inside `allow(...)` / `expect(...)`,
                                      also within `cfg_attr(...)` and lint lists,
                                      any whitespace;
                  call = "name"       a call `.name(` or `Type::name(`, any
                                      whitespace;
                  pattern = 'regex'   a regular expression, per line.

    python tools/arch_check.py [--src soneflow/src] [--rules tools/arch_rules.toml] \
        (--crap crap.json | --skip-crap) [--json report.json]

Exit code 1 on any violation or an incomplete gate. Paths in the rules are relative
to `--src`. Text-based like module_graph.py: paths and attributes built by macros
are not seen; `cargo clippy` keeps its own lints.
"""
import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
REPO = TOOLS.parent
sys.path.insert(0, str(TOOLS))
import module_graph as mg  # noqa: E402

KINDS = ("cycles", "layers", "unruled", "unsupported", "crap", "size", "forbid")
DEFAULT_SRC = REPO / "soneflow" / "src"
DEFAULT_RULES = TOOLS / "arch_rules.toml"


def _qualified(module: str) -> str:
    return "crate" if module in ("", "crate") else "crate::" + module.removeprefix("crate::")


def _under(name: str, prefix: str) -> bool:
    return name == prefix or name.startswith(prefix + "::")


def _rel(src: Path, path: Path) -> str:
    return path.resolve().relative_to(src.resolve()).as_posix()


def _in_scope(rel: str, scope: str) -> bool:
    scope = scope.strip("/")
    return rel == scope or rel.startswith(scope + "/") or rel == scope + ".rs"


def code_lines(text: str) -> int:
    """Non-blank lines left after removing `#[cfg(test)]` modules and comments."""
    body = mg.strip_comments(mg.strip_tests(text))
    return sum(1 for line in body.splitlines() if line.strip())


def check_graph(graph: dict, rules: dict) -> dict:
    named = {mg.name(m): {mg.name(t) for t in ts} for m, ts in graph.items()}
    cycles = [
        "{" + ", ".join(sorted(mg.name(m) for m in c)) + "}"
        for c in mg.tarjan(graph)
        if len(c) > 1
    ]
    facade = {_qualified(f) for f in rules.get("facade", ["crate"])}
    layers = [
        (_qualified(r["module"]), [_qualified(a) for a in r.get("allow", [])])
        for r in rules.get("layer", [])
    ]
    exact_roots = [_qualified(r) for r in rules.get("require_exact_rules", [])]
    ruled = {own for own, _ in layers}
    layer_violations, unruled, inexact = [], [], []
    for module in sorted(named):
        if module in facade:
            continue
        for root in exact_roots:
            if _under(module, root) and module not in ruled:
                inexact.append(f"{module} (require_exact_rules {root}: no rule of its own)")
        rule = max(
            (lr for lr in layers if _under(module, lr[0])),
            key=lambda lr: len(lr[0]),
            default=None,
        )
        if rule is None:
            unruled.append(module)
            continue
        own, allow = rule
        for target in sorted(named[module]):
            if _under(target, own) or any(_under(target, a) for a in allow):
                continue
            layer_violations.append(f"{module} -> {target} (rule {own}: allow {allow})")
    unruled_report = list(unruled)
    if not rules.get("require_rules", False):
        unruled = []
    return {
        "cycles": cycles,
        "layers": layer_violations,
        "unruled": unruled + inexact,
        "_unruled_info": unruled_report,
        "_edges": sum(len(v) for v in named.values()),
        "_modules": len(named),
    }


def check_crap(src: Path, rules: dict, crap_path, skip: bool = False) -> tuple:
    if crap_path is None:
        if skip:
            return [], {"skipped": "--skip-crap"}
        return ["SKIPPED: no --crap metrics, the gate is incomplete (pass --skip-crap to accept)"], {
            "skipped": "no --crap file"
        }
    data = json.loads(Path(crap_path).read_text(encoding="utf-8"))
    entries = data["entries"] if isinstance(data, dict) else data
    crap_rules = rules.get("crap", {})
    max_crap = crap_rules.get("max")
    scopes = crap_rules.get("scope", [])
    violations = []
    if not entries:
        violations.append(f"metrics {crap_path} have no entries")
    top, per_scope = [], {s["path"]: 0 for s in scopes}
    test_only = mg.test_files(src)
    for e in entries:
        try:
            rel = _rel(src, Path(e["file"]))
        except ValueError:
            continue  # another crate of the same report
        if rel in test_only:
            continue  # a cfg(test) module file is test code
        where = f"{e['function']} {rel}:{e.get('line')}"
        top.append((e["crap"], e["cyclomatic"], e.get("coverage"), where))
        if max_crap is not None and e["crap"] > max_crap:
            violations.append(f"CRAP {e['crap']:.1f} > {max_crap} {where}")
        for s in scopes:
            if not _in_scope(rel, s["path"]):
                continue
            per_scope[s["path"]] += 1
            if "cc_max" in s and e["cyclomatic"] > s["cc_max"]:
                violations.append(f"CC {e['cyclomatic']:.0f} > {s['cc_max']} ({s['path']}) {where}")
    if entries and not top:
        violations.append(f"metrics {crap_path} have no entry under {src}")
    for path, count in per_scope.items():
        if top and count == 0:
            violations.append(f"crap scope {path!r} has no functions in the metrics")
    top.sort(key=lambda r: -r[0])
    return violations, {
        "functions": len(top),
        "per_scope": per_scope,
        "top": [f"CRAP {c:.1f} CC {cc:.0f} cov {cov} {w}" for c, cc, cov, w in top[:10]],
    }


def _line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


_ATTR = re.compile(r"#!?\[")
_LINT_GROUP = re.compile(r"\b(?:allow|expect)\(([^()]*)\)")


def _attributes(clean: str):
    """(offset, text) of every outer or inner attribute, brackets balanced."""
    for m in _ATTR.finditer(clean):
        depth, j = 1, m.end()
        while depth and j < len(clean):
            depth += {"[": 1, "]": -1}.get(clean[j], 0)
            j += 1
        yield m.start(), clean[m.start():j]


def find_lint(clean: str, lint: str) -> list:
    """Offsets of attributes that allow or expect `lint` (whitespace-insensitive)."""
    want = re.sub(r"\s+", "", lint)
    hits = []
    for at, attr in _attributes(clean):
        squeezed = re.sub(r"\s+", "", attr)
        if any(want in g.split(",") for g in _LINT_GROUP.findall(squeezed)):
            hits.append(at)
    return hits


def find_call(clean: str, fn: str) -> list:
    """Offsets of `.fn(` and `Type::fn(` calls (whitespace-insensitive)."""
    return [m.start() for m in re.finditer(r"(?:\.|::)\s*" + re.escape(fn) + r"\s*\(", clean)]


def _forbid_rule(f: dict):
    if "lint" in f:
        return f"lint {f['lint']}", lambda clean: find_lint(clean, f["lint"])
    if "call" in f:
        return f"call {f['call']}", lambda clean: find_call(clean, f["call"])
    pat = re.compile(f["pattern"])

    def per_line(clean):
        offsets, pos = [], 0
        for line in clean.splitlines(keepends=True):
            if pat.search(line):
                offsets.append(pos)
            pos += len(line)
        return offsets

    return f"/{pat.pattern}/", per_line


def check_files(src: Path, rules: dict) -> tuple:
    sizes, size_v, forbid_v = {}, [], []
    forbids = [(f["path"], *_forbid_rule(f)) for f in rules.get("forbid", [])]
    for f in sorted(src.rglob("*.rs")):
        rel = _rel(src, f)
        text = f.read_text(encoding="utf-8")
        sizes[rel] = code_lines(text)
        for s in rules.get("size", []):
            if _in_scope(rel, s["path"]) and sizes[rel] > s["code_lines_max"]:
                size_v.append(f"{rel}: {sizes[rel]} code lines > {s['code_lines_max']} ({s['path']})")
        clean = mg.blank_comments_and_literals(text)
        for scope, label, find in forbids:
            if not _in_scope(rel, scope):
                continue
            for offset in find(clean):
                forbid_v.append(f"{rel}:{_line_of(clean, offset)}: {label} ({scope})")
    return size_v, forbid_v, sizes


def run(src=DEFAULT_SRC, rules_path=DEFAULT_RULES, crap_path=None, skip_crap=False) -> dict:
    src = Path(src)
    rules = tomllib.loads(Path(rules_path).read_text(encoding="utf-8"))
    _, graph, unsupported = mg.analyse(src)
    g = check_graph(graph, rules)
    crap_v, crap_info = check_crap(src, rules, crap_path, skip_crap)
    size_v, forbid_v, sizes = check_files(src, rules)
    largest = sorted(sizes.items(), key=lambda kv: -kv[1])[:10]
    return {
        "violations": {
            "cycles": g["cycles"],
            "layers": g["layers"],
            "unruled": g["unruled"],
            "unsupported": unsupported,
            "crap": crap_v,
            "size": size_v,
            "forbid": forbid_v,
        },
        "info": {
            "src": str(src),
            "rules": str(rules_path),
            "modules": g["_modules"],
            "edges": g["_edges"],
            "modules_without_rule": g["_unruled_info"],
            "crap": crap_info,
            "code_lines": sizes,
            "largest_files": [f"{n} {rel}" for rel, n in largest],
        },
    }


def report(result: dict) -> str:
    info, v = result["info"], result["violations"]
    out = [f"src: {info['src']}  rules: {info['rules']}", f"modules: {info['modules']}  edges: {info['edges']}"]
    crap = info["crap"]
    for kind in KINDS:
        items = v[kind]
        if kind == "crap" and "skipped" in crap:
            state = "SKIPPED (gate incomplete)" if items else "SKIPPED (--skip-crap)"
            out.append(f"crap: {state}")
            continue
        out.append(f"{kind}: {'PASS' if not items else f'FAIL ({len(items)})'}")
        out += [f"  {i}" for i in items]
    if "skipped" not in crap:
        out.append(f"crap: {crap['functions']} functions {crap['per_scope']}, highest:")
        out += [f"  {t}" for t in crap["top"]]
    out.append("largest files (non-test code lines):")
    out += [f"  {f}" for f in info["largest_files"]]
    return "\n".join(out)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--src", default=str(DEFAULT_SRC))
    ap.add_argument("--rules", default=str(DEFAULT_RULES))
    group = ap.add_mutually_exclusive_group()
    group.add_argument("--crap", help="cargo-crap JSON report")
    group.add_argument("--skip-crap", action="store_true", help="accept a gate without CRAP metrics")
    ap.add_argument("--json")
    a = ap.parse_args(argv)
    result = run(a.src, a.rules, a.crap, a.skip_crap)
    print(report(result))
    if a.json:
        Path(a.json).write_text(json.dumps(result, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    return 1 if any(result["violations"].values()) else 0


if __name__ == "__main__":
    sys.exit(main())
