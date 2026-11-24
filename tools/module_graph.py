#!/usr/bin/env python3
"""Module-level dependency graph and cycle check for one Rust crate.

Every `crate::`, `super::`, `self::` and child-relative path (in `use` items
and inline, with any whitespace around `::`) is resolved to the deepest module
that exists. Comments, string and char literals and test-only code are ignored: modules
under `#[cfg(test)]` or `#[cfg(all(test, ...))]` (inline or declared `mod x;`),
and items, statements, fields and parameters under such an attribute. Inline modules
(`mod x { ... }`, nested to any depth) are modules of their own. A parent
that only *declares* a child (`mod x;`) is containment, not a dependency; a
parent that *uses* a child's items, or a child that uses its parent's, is.
Strongly connected components with more than one module are reported as
cycles.

Not followed, reported as `unsupported` instead of passing silently: a
`#[path = ...]` attribute on a module, and `mod x;` without `x.rs` or
`x/mod.rs` where Rust would look for it.

`cargo coupling` reports "no circular dependencies" for parent/child and
crate-facade cycles, so this is the architecture gate for those.

    python tools/module_graph.py soneflow/src    # exit 1 on a cycle or unsupported
    python tools/module_graph.py soneflow/src --dot graph.dot

Text-based: paths built by macros are not seen.
"""
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path


def module_of(src: Path, f: Path) -> tuple:
    rel = f.relative_to(src).with_suffix("")
    parts = list(rel.parts)
    if parts[-1] in ("lib", "main", "mod"):
        parts = parts[:-1]
    return tuple(parts)


_RAW_OPEN = re.compile(r'b?r(#*)"')


def _blank(s: str) -> str:
    """Spaces for every character of `s` except newlines (keeps line numbers)."""
    return re.sub(r"[^\n]", " ", s)


def blank_comments_and_literals(text: str, literals: bool = True) -> str:
    """Replace comments (and, if `literals`, the contents of string, byte-string,
    raw-string and char literals) with spaces; newlines stay, so offsets and line
    numbers are those of the original text. Lifetimes (`'a`) are kept."""
    out, i, n = [], 0, len(text)

    def keep(open_end, body_end, close_end):
        """Emit delimiters as they are and the literal body blanked (or kept)."""
        body = text[open_end:body_end]
        out.append(text[i:open_end] + (_blank(body) if literals else body) + text[body_end:close_end])
        return close_end

    while i < n:
        c, two = text[i], text[i:i + 2]
        word_start = i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")
        raw = _RAW_OPEN.match(text, i) if word_start and c in "br" else None
        if two == "//":
            j = text.find("\n", i)
            j = n if j < 0 else j
            out.append(_blank(text[i:j]))
            i = j
        elif two == "/*":  # nested block comments
            depth, j = 1, i + 2
            while j < n and depth:
                if text[j:j + 2] == "/*":
                    depth, j = depth + 1, j + 2
                elif text[j:j + 2] == "*/":
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            out.append(_blank(text[i:j]))
            i = j
        elif raw:  # r"..", r#".."#, br".."
            closing = '"' + raw.group(1)
            end = text.find(closing, raw.end())
            end = n if end < 0 else end
            i = keep(raw.end(), end, min(n, end + len(closing)))
        elif c == '"' or (word_start and two == 'b"'):
            start = i + (2 if c == "b" else 1)
            j = start
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            j = min(j, n)
            i = keep(start, j, min(n, j + 1))
        elif c == "'" and text[i + 1:i + 2] == "\\":  # '\n', '\'', '\u{..}'
            j = text.find("'", i + 3)
            j = n if j < 0 else j
            i = keep(i + 1, j, min(n, j + 1))
        elif c == "'" and text[i + 2:i + 3] == "'":  # 'x'
            i = keep(i + 1, i + 2, i + 3)
        else:  # code, including lifetimes ('a)
            out.append(c)
            i += 1
    return "".join(out)


def strip_comments(text: str) -> str:
    """Comments replaced with spaces (literals kept)."""
    return blank_comments_and_literals(text, literals=False)


# attributes (one level of nested brackets), visibility, `mod name` and `{` or `;`
_MOD_ITEM = re.compile(
    r"((?:#!?\[(?:[^\[\]]|\[[^\[\]]*\])*\]\s*)*)"
    r"(?<![\w:])(?:pub\s*(?:\([^)]*\))?\s*)?mod\s+([A-Za-z_]\w*)\s*([{;])"
)
_CFG_OPEN = re.compile(r"#\[\s*cfg\s*\(")
_PATH_ATTR = re.compile(r"#\[\s*path\s*=")


def _close_paren(text: str, open_at: int) -> int:
    """Index one past the `)` matching the `(` at open_at (or len(text))."""
    depth, j = 1, open_at + 1
    while depth and j < len(text):
        depth += {"(": 1, ")": -1}.get(text[j], 0)
        j += 1
    return j


def _split_top(args: str) -> list:
    parts, depth, cur = [], 0, ""
    for ch in args:
        if ch == "," and depth == 0:
            parts.append(cur)
            cur = ""
            continue
        depth += {"(": 1, ")": -1}.get(ch, 0)
        cur += ch
    return parts + [cur]


def _test_only(pred: str) -> bool:
    """True when a cfg predicate can hold only in test builds: `test`, or
    `all(...)` with a test-only argument. `any(test, ...)` and `not(test)` are not."""
    pred = re.sub(r"\s+", "", pred)
    if pred == "test":
        return True
    if pred.startswith("all(") and pred.endswith(")"):
        return any(_test_only(a) for a in _split_top(pred[4:-1]))
    return False


def _test_cfgs(text: str):
    """Yield (start, end) of every outer `#[cfg(<test-only>)]` attribute."""
    for m in _CFG_OPEN.finditer(text):
        if text[m.start() + 1:m.start() + 2] == "!":
            continue
        close = _close_paren(text, m.end() - 1)
        end = re.match(r"\s*\]", text[close:])
        if end and _test_only(text[m.end():close - 1]):
            yield m.start(), close + end.end()


def _is_test_cfg(attrs: str) -> bool:
    return any(True for _ in _test_cfgs(attrs))


def blank_test_items(clean: str) -> str:
    """Blank what a test-only cfg attribute gates outside a test module: the
    attribute, further attributes, and the gated item, statement, field or
    parameter. It ends at a `;` or `,` at depth 0, after a `{...}` block at
    depth 0, or before an unmatched closing bracket. Text-based: in
    `#[cfg(test)] if a { } else { }` only `if a { }` is blanked."""
    out, i = [], 0
    for start, j in _test_cfgs(clean):
        if start < i:
            continue
        while True:  # further attributes on the same item
            m = re.match(r"\s*#\[", clean[j:])
            if not m:
                break
            depth, k = 1, j + m.end()
            while depth and k < len(clean):
                depth += {"[": 1, "]": -1}.get(clean[k], 0)
                k += 1
            j = k
        depth, k = 0, j
        while k < len(clean):
            ch = clean[k]
            if ch in "([":
                depth += 1
            elif ch in ")]" or (ch == "}" and depth == 0):
                if depth == 0:
                    break
                depth -= 1
            elif ch == "{":
                if depth == 0:
                    k = _matching_brace(clean, k)
                    break
                depth += 1
            elif ch == "}":
                depth -= 1
            elif ch in ";," and depth == 0:
                k += 1
                break
            k += 1
        out.append(clean[i:start] + _blank(clean[start:k]))
        i = k
    out.append(clean[i:])
    return "".join(out)


def _matching_brace(text: str, open_at: int) -> int:
    depth, j = 1, open_at + 1
    while depth and j < len(text):
        depth += {"{": 1, "}": -1}.get(text[j], 0)
        j += 1
    return j  # one past the closing brace


@dataclass
class FileModules:
    """Modules written in one file: own text per module (inline bodies cut out),
    child declarations `mod x;` and the test modules that were dropped."""

    texts: dict = field(default_factory=dict)
    decls: list = field(default_factory=list)  # (parent, name, line, attrs)
    tests: list = field(default_factory=list)  # module paths of cfg(test) modules


def split_modules(clean: str, here: tuple, base_line: int = 1, acc: FileModules = None) -> FileModules:
    """Split comment- and literal-free text into its inline modules (recursive).
    Test modules are dropped; their text is blanked."""
    acc = acc or FileModules()
    own, i = [], 0
    while True:
        m = _MOD_ITEM.search(clean, i)
        if not m:
            own.append(clean[i:])
            break
        attrs, name, kind = m.group(1), m.group(2), m.group(3)
        line = base_line + clean.count("\n", 0, m.start(2))
        is_test = _is_test_cfg(attrs)
        if kind == ";":
            own.append(clean[i:m.end()])
            if is_test:
                acc.tests.append(here + (name,))
            else:
                acc.decls.append((here, name, line, attrs))
            i = m.end()
            continue
        close = _matching_brace(clean, m.end() - 1)
        own.append(clean[i:m.start()] + _blank(clean[m.start():close]))
        if is_test:
            acc.tests.append(here + (name,))
        else:
            child = here + (name,)
            if _PATH_ATTR.search(attrs):
                acc.decls.append((here, name, line, attrs))
            body_line = base_line + clean.count("\n", 0, m.end())
            split_modules(clean[m.end():close - 1], child, body_line, acc)
        i = close
    acc.texts[here] = acc.texts.get(here, "") + "".join(own)
    return acc


def strip_tests(text: str) -> str:
    """The text with every `#[cfg(test)]` inline module blanked (comments kept)."""
    clean = blank_comments_and_literals(text, literals=False)
    out, i = [], 0
    for m in _MOD_ITEM.finditer(clean):
        if m.start() < i or m.group(3) != "{" or not _is_test_cfg(m.group(1)):
            continue
        close = _matching_brace(clean, m.end() - 1)
        out.append(text[i:m.start()] + _blank(text[m.start():close]))
        i = close
    out.append(text[i:])
    return "".join(out)


def expand_use_groups(text: str) -> list:
    """Return every path written in the text, with `use a::{b, c::d}` expanded."""
    text = re.sub(r"\s*::\s*", "::", text)
    paths = []
    for m in re.finditer(r"\buse\s+([^;]+);", text):
        tree = re.sub(r"\s+as\s+\w+", "", m.group(1))  # `use a::b as c` -> a::b
        paths += _expand(re.sub(r"\s+", "", tree))
    body = re.sub(r"\buse\s+[^;]+;", "", text)
    paths += re.findall(r"(?<![:\w])([a-z_]\w*(?:::\w+)+)", body)
    return paths


def _expand(tree: str) -> list:
    i = tree.find("{")
    if i < 0:
        return [tree]
    prefix, inner = tree[:i], tree[i + 1:tree.rfind("}")]
    parts, depth, cur = [], 0, ""
    for ch in inner:
        if ch == "," and depth == 0:
            parts.append(cur)
            cur = ""
            continue
        depth += {"{": 1, "}": -1}.get(ch, 0)
        cur += ch
    parts.append(cur)
    return [p for part in parts if part for p in _expand(prefix + part)]


def resolve(path: str, here: tuple, modules: set):
    segs = path.split("::")
    if segs[0] == "crate":
        base, segs = (), segs[1:]
    elif segs[0] in ("super", "self"):
        base = here
        while segs and segs[0] in ("super", "self"):
            if segs[0] == "super":
                base = base[:-1]
            segs = segs[1:]
    elif here + (segs[0],) in modules:  # 2018-edition relative path to a child
        base = here
    else:
        return None
    best = base
    for k in range(1, len(segs) + 1):
        cand = base + tuple(segs[:k])
        if cand in modules:
            best = cand
        else:
            break
    return best


def tarjan(graph: dict) -> list:
    index, low, stack, on, sccs, n = {}, {}, [], set(), [], [0]

    def visit(v):
        index[v] = low[v] = n[0]
        n[0] += 1
        stack.append(v)
        on.add(v)
        for w in graph.get(v, ()):
            if w not in index:
                visit(w)
                low[v] = min(low[v], low[w])
            elif w in on:
                low[v] = min(low[v], index[w])
        if low[v] == index[v]:
            comp = []
            while True:
                w = stack.pop()
                on.discard(w)
                comp.append(w)
                if w == v:
                    break
            sccs.append(comp)

    for v in graph:
        if v not in index:
            visit(v)
    return sccs


def name(m: tuple) -> str:
    return "crate" + "".join("::" + p for p in m)


def _under_any(m: tuple, roots) -> bool:
    return any(m[:len(r)] == r for r in roots)


def analyse(src: Path) -> tuple:
    """Return (modules, graph, unsupported): every non-test module as a path
    tuple, its dependency set, and the module declarations that are not
    followed (`#[path]`, or no file where Rust would look)."""
    files = sorted(src.rglob("*.rs"))
    texts, tests, unsupported = {}, [], []
    for f in files:
        here = module_of(src, f)
        clean = blank_comments_and_literals(f.read_text(encoding="utf-8"))
        found = split_modules(clean, here)
        for m, t in found.texts.items():
            texts[m] = texts.get(m, "") + t
        tests += found.tests
        rel = f.relative_to(src).as_posix()
        for parent, child, line, attrs in found.decls:
            where = f"{rel}:{line}: mod {child} in {name(parent)}"
            if _PATH_ATTR.search(attrs):
                unsupported.append(f"{where}: #[path] is not followed")
                continue
            folder = src.joinpath(*parent)
            if not ((folder / f"{child}.rs").is_file() or (folder / child / "mod.rs").is_file()):
                unsupported.append(f"{where}: no {child}.rs or {child}/mod.rs")
    modules = {m for m in texts if not _under_any(m, tests)}
    graph = {m: set() for m in modules}
    for here in modules:
        for p in expand_use_groups(blank_test_items(texts[here])):
            target = resolve(p, here, modules)
            if target is not None and target != here:
                graph[here].add(target)
    return modules, graph, sorted(unsupported)


def test_files(src: Path) -> set:
    """Paths (relative to `src`, posix) of the files that are a `cfg(test)`
    module or lie below one, e.g. `core/x/tests/case.rs` after
    `#[cfg(test)] mod tests;` in `core/x/mod.rs`."""
    files = sorted(src.rglob("*.rs"))
    tests = []
    for f in files:
        clean = blank_comments_and_literals(f.read_text(encoding="utf-8"))
        tests += split_modules(clean, module_of(src, f)).tests
    return {f.relative_to(src).as_posix() for f in files if _under_any(module_of(src, f), tests)}


def build_graph(src: Path) -> tuple:
    """Return (modules, graph): every module as a path tuple and its dependency set."""
    modules, graph, _ = analyse(src)
    return modules, graph


def main():
    src = Path(sys.argv[1])
    modules, graph, unsupported = analyse(src)
    print(f"modules: {len(modules)}  edges: {sum(len(v) for v in graph.values())}")
    for m in sorted(graph):
        if graph[m]:
            print(f"  {name(m)} -> " + ", ".join(sorted(name(t) for t in graph[m])))
    cycles = [c for c in tarjan(graph) if len(c) > 1]
    print(f"\ncycles (strongly connected components > 1): {len(cycles)}")
    for c in cycles:
        print("  {" + ", ".join(sorted(name(m) for m in c)) + "}")
    if unsupported:
        print(f"\nunsupported module declarations: {len(unsupported)}")
        for u in unsupported:
            print(f"  {u}")
    if "--dot" in sys.argv:
        out = Path(sys.argv[sys.argv.index("--dot") + 1])
        lines = ["digraph modules {", "  rankdir=LR;"]
        for m in sorted(graph):
            for t in sorted(graph[m]):
                lines.append(f'  "{name(m)}" -> "{name(t)}";')
        out.write_text("\n".join(lines + ["}"]) + "\n", encoding="utf-8")
    sys.exit(1 if cycles or unsupported else 0)


if __name__ == "__main__":
    main()
