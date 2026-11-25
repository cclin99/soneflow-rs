"""Per-function metrics from rust-code-analysis-cli, product code only.

Usage (from the repository root):
    python tools/fnmetrics.py [--out fn.json] [--check] [--max-cc 15] [--max-ploc 80]

Runs `rust-code-analysis-cli -m -O json` over the `src` directories of the
three crates (soneflow, soneflow-ffi, soneflow-py), collects every function
(`kind == "function"`; closures count toward the enclosing function's
cyclomatic `sum`), and marks test code:

- files under a `tests/` path segment, or named `*_tests.rs` / `tests.rs`;
- files brought in by `#[cfg(test)] mod name;` (or `cfg(all(test, ...))`);
- functions inside an inline `#[cfg(test)] mod name { ... }` block;
- functions directly preceded by `#[test]` or `#[cfg(test)]`.

Metric names follow rust-code-analysis: `cc` is `cyclomatic.sum`, `ploc` is
physical lines of code without comments and blank lines, `sloc` is the full
source span. `--check` exits 1 if any product function exceeds --max-cc or
--max-ploc (0.2.0 completion condition 4).
"""

import argparse
import json
import pathlib
import re
import subprocess
import sys
import tempfile

CRATES = ("soneflow", "soneflow-ffi", "soneflow-py")
CFG_TEST = re.compile(r"\s*#\[cfg\((test|all\(test)")
MOD_DECL = re.compile(r"\s*(pub(\([^)]*\))?\s+)?mod\s+(\w+)\s*;")
MOD_BLOCK = re.compile(r"\s*(pub(\([^)]*\))?\s+)?mod\s+\w+\s*\{")


def run_rca(repo: pathlib.Path, out: pathlib.Path) -> None:
    # Relative input paths: the CLI mirrors them under `out`; absolute
    # Windows paths (drive letters) produce no output files.
    for crate in CRATES:
        subprocess.run(
            ["rust-code-analysis-cli", "-m", "-O", "json",
             "-p", f"{crate}/src", "-o", str(out)],
            cwd=repo, check=True,
        )


def walk(node: dict, file: str, rows: list) -> None:
    if node.get("kind") == "function":
        m = node["metrics"]
        nargs = m["nargs"]
        rows.append(dict(
            file=file, name=node["name"],
            start=node["start_line"], end=node["end_line"],
            sloc=m["loc"]["sloc"], ploc=m["loc"]["ploc"], lloc=m["loc"]["lloc"],
            cc=m["cyclomatic"]["sum"], cog=m["cognitive"]["sum"],
            nargs=nargs["total"] if isinstance(nargs.get("total"), (int, float)) else nargs.get("functions", 0),
            nexits=m["nexits"]["sum"],
        ))
    for child in node.get("spaces", []):
        walk(child, file, rows)


def skip_attrs(lines: list, k: int) -> int:
    while k < len(lines) and (lines[k].strip().startswith("#[") or lines[k].strip().startswith("//")):
        k += 1
    return k


def test_files(repo: pathlib.Path) -> set:
    """Files included only via `#[cfg(test)] mod name;`."""
    found = set()
    for crate in CRATES:
        for src in (repo / crate / "src").rglob("*.rs"):
            lines = src.read_text(encoding="utf-8").splitlines()
            for i, line in enumerate(lines):
                if not CFG_TEST.match(line):
                    continue
                k = skip_attrs(lines, i + 1)
                m = MOD_DECL.match(lines[k]) if k < len(lines) else None
                if not m:
                    continue
                name = m.group(3)
                base = src.parent if src.name in ("mod.rs", "lib.rs", "main.rs") else src.parent / src.stem
                for cand in (base / f"{name}.rs", base / name / "mod.rs"):
                    if cand.exists():
                        found.add(cand.relative_to(repo).as_posix())
    return found


def test_block_ranges(lines: list) -> list:
    """1-based inclusive line ranges of inline `#[cfg(test)] mod x { ... }`."""
    ranges = []
    for i, line in enumerate(lines):
        if not CFG_TEST.match(line):
            continue
        k = skip_attrs(lines, i + 1)
        if k >= len(lines) or not MOD_BLOCK.match(lines[k]):
            continue
        depth = 0
        for e in range(k, len(lines)):
            depth += lines[e].count("{") - lines[e].count("}")
            if depth == 0 and e > k:
                ranges.append((k + 1, e + 1))
                break
    return ranges


def mark_tests(repo: pathlib.Path, rows: list) -> None:
    tfiles = test_files(repo)
    cache = {}
    for r in rows:
        f = r["file"]
        parts = f.split("/")
        if "tests" in parts or f.endswith("_tests.rs") or parts[-1] == "tests.rs" or f in tfiles:
            r["test"] = True
            continue
        if f not in cache:
            lines = (repo / f).read_text(encoding="utf-8").splitlines()
            cache[f] = (lines, test_block_ranges(lines))
        lines, ranges = cache[f]
        if any(a < r["start"] <= b for a, b in ranges):
            r["test"] = True
            continue
        j = r["start"] - 2
        r["test"] = False
        while j >= 0 and (lines[j].strip().startswith("#[") or lines[j].strip().startswith("//")):
            if "#[test]" in lines[j] or CFG_TEST.match(lines[j]):
                r["test"] = True
                break
            j -= 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--repo", default=".", help="repository root (default: .)")
    ap.add_argument("--out", help="write all rows (product and test) as JSON here")
    ap.add_argument("--check", action="store_true", help="fail on product functions over the limits")
    ap.add_argument("--max-cc", type=float, default=15)
    ap.add_argument("--max-ploc", type=float, default=80)
    ap.add_argument("--top", type=int, default=10, help="rows to list per metric")
    a = ap.parse_args()
    repo = pathlib.Path(a.repo).resolve()

    rows = []
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        run_rca(repo, tmp)
        for f in sorted(tmp.rglob("*.rs.json")):
            d = json.loads(f.read_text(encoding="utf-8"))
            walk(d, f.relative_to(tmp).as_posix()[: -len(".json")], rows)
    if not rows:
        print("no functions found: rust-code-analysis-cli produced no output", file=sys.stderr)
        return 2
    mark_tests(repo, rows)
    rows.sort(key=lambda r: (r["file"], r["start"]))
    if a.out:
        pathlib.Path(a.out).write_text(json.dumps(rows, indent=0) + "\n", encoding="utf-8")

    prod = [r for r in rows if not r["test"]]
    print(f"functions {len(rows)}  product {len(prod)}  test {len(rows) - len(prod)}")
    for key in ("cc", "ploc"):
        top = sorted(prod, key=lambda r: -r[key])[: a.top]
        print(f"\n== product, highest {key}")
        for r in top:
            print(f"  {r[key]:>5.0f}  {r['file']}:{r['start']} {r['name']}"
                  f"  (cc {r['cc']:.0f}, ploc {r['ploc']:.0f}, sloc {r['sloc']:.0f})")

    over = [r for r in prod if r["cc"] > a.max_cc or r["ploc"] > a.max_ploc]
    print(f"\nover limits (cc > {a.max_cc:g} or ploc > {a.max_ploc:g}): {len(over)}")
    for r in over:
        print(f"  {r['file']}:{r['start']} {r['name']}  cc {r['cc']:.0f}  ploc {r['ploc']:.0f}")
    return 1 if (a.check and over) else 0


if __name__ == "__main__":
    sys.exit(main())
