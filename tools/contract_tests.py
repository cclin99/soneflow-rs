#!/usr/bin/env python3
"""Check that every test named in docs/BEHAVIOUR-CONTRACTS.md exists.

The contract tables refer to tests as `path::name`, with `path` relative to
the repository root:

* `.rs` files: `name` must be a function marked `#[test]` (other attributes
  such as `#[ignore]` or `#[cfg(...)]` may sit between them);
* `.py` files: `name` must be a `def test_...` function.

The English document and its Chinese pair (`NAME.zh-TW.md`) must list the
same set of tests.

    python tools/contract_tests.py           # exit 1 if a test is missing
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DEFAULT_DOC = REPO / "docs" / "BEHAVIOUR-CONTRACTS.md"

_REF_RE = re.compile(r"`([A-Za-z0-9_./-]+\.(?:rs|py))::([A-Za-z_][A-Za-z0-9_]*)`")


def references(text: str) -> set[tuple[str, str]]:
    """(path, name) for every `path::name` in the document."""
    return set(_REF_RE.findall(text))


def _rust_test_exists(source: str, name: str) -> bool:
    attrs = r"(?:\s*#\[[^\]]*\]|\s*#\[[^\]]*\([^)]*\)[^\]]*\])*"
    pattern = r"#\[test\]" + attrs + r"\s*(?:pub\s+)?fn\s+" + re.escape(name) + r"\s*\("
    return re.search(pattern, source) is not None


def _python_test_exists(source: str, name: str) -> bool:
    if not name.startswith("test_"):
        return False
    return re.search(r"^\s*def\s+" + re.escape(name) + r"\s*\(", source, re.M) is not None


def missing(root: Path, refs: set[tuple[str, str]]) -> list[str]:
    out = []
    for path, name in sorted(refs):
        file = root / path
        if not file.is_file():
            out.append(f"{path}::{name}  (file not found)")
            continue
        source = file.read_text(encoding="utf-8")
        found = _rust_test_exists(source, name) if path.endswith(".rs") else _python_test_exists(source, name)
        if not found:
            out.append(f"{path}::{name}  (no such test)")
    return out


def pair_of(doc: Path) -> Path:
    return doc.with_name(doc.stem + ".zh-TW.md")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", type=Path, default=REPO, help="repository root the paths are relative to")
    ap.add_argument("--doc", type=Path, default=DEFAULT_DOC)
    args = ap.parse_args(argv)

    en = references(args.doc.read_text(encoding="utf-8"))
    zh = references(pair_of(args.doc).read_text(encoding="utf-8"))
    if not en:
        print(f"contract_tests: no `path::name` references in {args.doc.name}", file=sys.stderr)
        return 2

    problems = missing(args.root, en | zh)
    for ref in sorted(en - zh):
        problems.append(f"{ref[0]}::{ref[1]}  (in {args.doc.name}, not in {pair_of(args.doc).name})")
    for ref in sorted(zh - en):
        problems.append(f"{ref[0]}::{ref[1]}  (in {pair_of(args.doc).name}, not in {args.doc.name})")
    for p in problems:
        print(f"- {p}")
    if problems:
        print(f"contract_tests: {len(problems)} problem(s)", file=sys.stderr)
        return 1
    print(f"contract_tests: ok ({len(en)} tests)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
