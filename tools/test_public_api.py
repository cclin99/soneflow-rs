"""Unit tests for tools/public_api.py (rustdoc HTML parsing, list comparison)."""

from __future__ import annotations

import io
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import public_api  # noqa: E402

ALL_HTML = (
    '<h1>List of all items</h1>'
    '<h3 id="structs">Structs</h3><ul class="all-items">'
    '<li><a href="struct.Frame.html">Frame</a></li></ul>'
    '<h3 id="enums">Enums</h3><ul class="all-items">'
    '<li><a href="enum.Mode.html">Mode</a></li></ul>'
    '<h3 id="functions">Functions</h3><ul class="all-items">'
    '<li><a href="simd/fn.use_avx2.html">simd::use_avx2</a></li></ul>'
)

STRUCT_PAGE = (
    '<section id="fields"></section>'
    '<span id="structfield.n"></span>'
    '<div id="implementations-list">'
    '<section id="impl-Frame"></section>'
    '<section id="method.new"></section>'
    '<section id="associatedconstant.WARMUP"></section>'
    '</div>'
    '<div id="trait-implementations-list">'
    '<section id="impl-Clone-for-Frame"></section><section id="method.clone"></section>'
    '<section id="impl-PartialEq%3CX%3E-for-Frame"></section>'
    '</div>'
    '<div id="synthetic-implementations-list">'
    '<section id="impl-Send-for-Frame"></section>'
    '<section id="impl-Freeze-for-Frame"></section>'
    '</div>'
    '<div id="blanket-implementations-list">'
    '<section id="impl-Any-for-T"></section><section id="method.type_id"></section>'
    '</div>'
)

ENUM_PAGE = (
    '<section id="variant.Zero"></section>'
    '<section id="variant.Tail"></section>'
    '<div id="variant.Tail.fields"></div>'
    '<section id="variant.Tail.field.len"></section>'
    '<div id="synthetic-implementations-list">'
    '<section id="impl-Sync-for-Mode"></section>'
    '</div>'
)


def write_docs(root: Path) -> None:
    (root / "simd").mkdir()
    (root / "all.html").write_text(ALL_HTML, encoding="utf-8")
    (root / "struct.Frame.html").write_text(STRUCT_PAGE, encoding="utf-8")
    (root / "enum.Mode.html").write_text(ENUM_PAGE, encoding="utf-8")


EXPECTED = sorted(
    [
        "struct soneflow::Frame",
        "field soneflow::Frame::n",
        "method soneflow::Frame::new",
        "assoc-const soneflow::Frame::WARMUP",
        "impl soneflow::Frame: Clone",
        "impl soneflow::Frame: PartialEq<X>",
        "impl soneflow::Frame: Send",
        "enum soneflow::Mode",
        "variant soneflow::Mode::Zero",
        "variant soneflow::Mode::Tail",
        "variant-field soneflow::Mode::Tail::len",
        "impl soneflow::Mode: Sync",
        "fn soneflow::simd::use_avx2",
    ]
)


class CollectTests(unittest.TestCase):
    def test_items_members_and_traits(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_docs(root)
            self.assertEqual(public_api.collect(root), EXPECTED)

    def test_trait_methods_and_blanket_impls_are_not_listed(self):
        lines = public_api.page_members(STRUCT_PAGE, "soneflow::Frame")
        self.assertNotIn("method soneflow::Frame::clone", lines)
        self.assertNotIn("method soneflow::Frame::type_id", lines)
        self.assertNotIn("impl soneflow::Frame: Any", lines)
        self.assertNotIn("impl soneflow::Frame: Freeze", lines)

    def test_unknown_section_is_an_error(self):
        html = '<h3 id="widgets">Widgets</h3><ul class="all-items"><li><a href="x.html">X</a></li></ul>'
        with self.assertRaises(SystemExit):
            public_api.all_items(html)


class CheckTests(unittest.TestCase):
    def run_check(self, listed: list[str]) -> tuple[int, str]:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_docs(root)
            listing = root / "api.txt"
            listing.write_text("# header\n" + "\n".join(listed) + "\n", encoding="utf-8")
            out = io.StringIO()
            with redirect_stdout(out):
                code = public_api.main(["--check", "--doc-dir", str(root), "--list", str(listing)])
            return code, out.getvalue()

    def test_matching_list_passes(self):
        code, _ = self.run_check(EXPECTED)
        self.assertEqual(code, 0)

    def test_extra_public_item_fails(self):
        code, out = self.run_check([l for l in EXPECTED if l != "fn soneflow::simd::use_avx2"])
        self.assertEqual(code, 1)
        self.assertIn("+ fn soneflow::simd::use_avx2", out)

    def test_missing_public_item_fails(self):
        code, out = self.run_check(EXPECTED + ["fn soneflow::gone"])
        self.assertEqual(code, 1)
        self.assertIn("- fn soneflow::gone", out)

    def test_write_then_check_round_trips(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_docs(root)
            listing = root / "api.txt"
            with redirect_stdout(io.StringIO()):
                self.assertEqual(public_api.main(["--write", "--doc-dir", str(root), "--list", str(listing)]), 0)
                self.assertEqual(public_api.main(["--check", "--doc-dir", str(root), "--list", str(listing)]), 0)


if __name__ == "__main__":
    unittest.main()
