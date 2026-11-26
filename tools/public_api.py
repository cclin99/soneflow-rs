#!/usr/bin/env python3
"""List the public API of the `soneflow` crate and compare it with a committed list.

The list is read from the HTML that stable rustdoc writes for the default
feature set (no `internals`), so it is what a downstream user can name:

* every item in `all.html` (structs, enums, functions, constants, ...),
  written with its rustdoc path (`soneflow::simd::use_avx2`);
* for each struct, enum and union page: fields, variants (and their fields),
  methods and associated items of inherent impls, and the traits it
  implements (trait impls, plus the auto traits in `AUTO_TRAITS`).

Blanket impls are left out, and so are auto traits outside `AUTO_TRAITS`,
because their set changes with the toolchain.

    python tools/public_api.py --check     # exit 1 if the API differs from the list
    python tools/public_api.py --write     # rewrite the list from the current API
    python tools/public_api.py --doc-dir target/doc/soneflow --check   # skip cargo doc

The comparison fails both ways: an item that is public but not listed, and a
listed item that is no longer public.
"""
from __future__ import annotations

import argparse
import html
import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import unquote

REPO = Path(__file__).resolve().parent.parent
CRATE = "soneflow"
DEFAULT_LIST = REPO / "soneflow" / "public-api.txt"
DEFAULT_DOC_DIR = REPO / "target" / "doc" / CRATE

# Section id in all.html -> kind written to the list.
SECTION_KIND = {
    "structs": "struct",
    "enums": "enum",
    "unions": "union",
    "functions": "fn",
    "constants": "const",
    "statics": "static",
    "traits": "trait",
    "macros": "macro",
    "types": "type",
    "trait-aliases": "trait-alias",
    "attributes": "attr",
    "derives": "derive",
}
AUTO_TRAITS = {"Send", "Sync", "Unpin", "UnwindSafe", "RefUnwindSafe"}
PAGE_KINDS = {"struct", "enum", "union"}

_SECTION_RE = re.compile(r'<h3 id="([^"]+)">.*?</h3>\s*<ul class="all-items">(.*?)</ul>', re.S)
_LINK_RE = re.compile(r'<a href="([^"]+)">(.*?)</a>', re.S)
_ID_RE = re.compile(r'id="([^"]+)"')
# Page regions, in the order rustdoc writes them.
_REGIONS = (
    "implementations-list",
    "trait-implementations-list",
    "synthetic-implementations-list",
    "blanket-implementations-list",
)


def all_items(all_html: str) -> list[tuple[str, str, str]]:
    """(kind, path, href) for every item listed in all.html."""
    out = []
    for section, body in _SECTION_RE.findall(all_html):
        kind = SECTION_KIND.get(section)
        if kind is None:
            raise SystemExit(f"public_api: unknown all.html section {section!r}; update SECTION_KIND")
        for href, text in _LINK_RE.findall(body):
            name = html.unescape(re.sub(r"<[^>]+>", "", text)).strip()
            out.append((kind, f"{CRATE}::{name}", href))
    return out


def _region_of(page: str) -> list[tuple[int, str]]:
    """Start offsets of the impl regions present on the page, sorted."""
    marks = []
    for region in _REGIONS:
        i = page.find(f'id="{region}"')
        if i >= 0:
            marks.append((i, region))
    return sorted(marks)


def _trait_name(impl_id: str) -> str:
    # impl-Clone-for-StreamFrame, impl-PartialEq%3CX%3E-for-Y, impl-Debug-for-X-1
    body = unquote(impl_id)[len("impl-"):]
    return body.rsplit("-for-", 1)[0]


def page_members(page: str, path: str) -> list[str]:
    """Members of a struct/enum/union page as list lines."""
    lines = []
    regions = _region_of(page)

    def region_at(pos: int) -> str | None:
        current = None
        for start, name in regions:
            if pos >= start:
                current = name
        return current

    for m in _ID_RE.finditer(page):
        ident = m.group(1)
        region = region_at(m.start())
        if region is None:
            # Before the impl sections: fields and variants.
            if ident.startswith("structfield."):
                lines.append(f"field {path}::{ident[len('structfield.'):]}")
            elif ident.startswith("variant.") and not ident.endswith(".fields"):
                rest = ident[len("variant."):]
                if ".field." in rest:
                    variant, field = rest.split(".field.", 1)
                    lines.append(f"variant-field {path}::{variant}::{field}")
                else:
                    lines.append(f"variant {path}::{rest}")
        elif region == "implementations-list":
            for prefix, kind in (
                ("method.", "method"),
                ("associatedconstant.", "assoc-const"),
                ("associatedtype.", "assoc-type"),
            ):
                if ident.startswith(prefix):
                    lines.append(f"{kind} {path}::{ident[len(prefix):]}")
        elif region == "trait-implementations-list":
            if ident.startswith("impl-"):
                lines.append(f"impl {path}: {_trait_name(ident)}")
        elif region == "synthetic-implementations-list":
            if ident.startswith("impl-"):
                trait = _trait_name(ident)
                if trait in AUTO_TRAITS:
                    lines.append(f"impl {path}: {trait}")
    return lines


def collect(doc_dir: Path) -> list[str]:
    all_html = (doc_dir / "all.html").read_text(encoding="utf-8")
    lines = []
    for kind, path, href in all_items(all_html):
        lines.append(f"{kind} {path}")
        if kind in PAGE_KINDS:
            page = (doc_dir / href).read_text(encoding="utf-8")
            lines.extend(page_members(page, path))
    return sorted(set(lines))


def build_docs() -> None:
    # Default features only: the list is the product API, not `internals`.
    subprocess.run(
        ["cargo", "doc", "--no-deps", "-p", CRATE, "--quiet"],
        cwd=REPO,
        check=True,
    )


def read_list(path: Path) -> list[str]:
    lines = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line and not line.startswith("#"):
            lines.append(line)
    return lines


HEADER = """\
# Public API of the soneflow crate (default features), one line per item.
# Generated by tools/public_api.py from rustdoc; CI fails if it drifts.
# Regenerate with `python tools/public_api.py --write` and review the diff:
# every added line is a new semver commitment, every removed line a break.
"""


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    ap.add_argument("--list", type=Path, default=DEFAULT_LIST)
    ap.add_argument("--doc-dir", type=Path, help="use existing rustdoc output instead of running cargo doc")
    args = ap.parse_args(argv)

    if args.doc_dir is None:
        build_docs()
        doc_dir = DEFAULT_DOC_DIR
    else:
        doc_dir = args.doc_dir
    current = collect(doc_dir)
    if not current:
        print("public_api: no items found; rustdoc layout changed?", file=sys.stderr)
        return 2

    if args.write:
        args.list.write_text(HEADER + "\n".join(current) + "\n", encoding="utf-8", newline="\n")
        print(f"public_api: wrote {len(current)} lines to {args.list}")
        return 0

    expected = read_list(args.list)
    added = sorted(set(current) - set(expected))
    removed = sorted(set(expected) - set(current))
    for line in added:
        print(f"+ {line}  (public, not in the list)")
    for line in removed:
        print(f"- {line}  (in the list, not public)")
    if added or removed:
        print(f"public_api: {len(added)} added, {len(removed)} removed vs {args.list.name}", file=sys.stderr)
        return 1
    print(f"public_api: ok ({len(current)} lines)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
