"""Unit tests for tools/contract_tests.py (contract table -> test names)."""

from __future__ import annotations

import io
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import contract_tests  # noqa: E402

RUST = """\
#[test]
fn chunking_is_invariant() {}

#[test]
#[ignore = "slow"]
fn slow_but_listed() {}

fn helper_not_a_test() {}
"""

PY = """\
def test_flags_nan():
    pass


def helper():
    pass
"""


def table(*refs: str) -> str:
    rows = "\n".join(f"| promise {i} | `{r}` |" for i, r in enumerate(refs))
    return "# Contracts\n\n| promise | tests |\n|---|---|\n" + rows + "\n"


class ContractTestsBehavior(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        (self.root / "crate" / "tests").mkdir(parents=True)
        (self.root / "crate" / "tests" / "stream.rs").write_text(RUST, encoding="utf-8")
        (self.root / "py").mkdir()
        (self.root / "py" / "test_stream.py").write_text(PY, encoding="utf-8")

    def tearDown(self):
        self._tmp.cleanup()

    def run_check(self, en: str, zh: str) -> tuple[int, str]:
        (self.root / "C.md").write_text(en, encoding="utf-8")
        (self.root / "C.zh-TW.md").write_text(zh, encoding="utf-8")
        out = io.StringIO()
        with redirect_stdout(out):
            code = contract_tests.main(
                ["--root", str(self.root), "--doc", str(self.root / "C.md")]
            )
        return code, out.getvalue()

    def test_listed_tests_that_exist_pass(self):
        """Given Rust (with an extra attribute) and Python tests that exist, then the check passes."""
        doc = table(
            "crate/tests/stream.rs::chunking_is_invariant",
            "crate/tests/stream.rs::slow_but_listed",
            "py/test_stream.py::test_flags_nan",
        )
        code, out = self.run_check(doc, doc)
        self.assertEqual(code, 0, out)
        self.assertIn("3 tests", out)

    def test_a_missing_test_name_fails(self):
        """Given a listed name with no test function, then the check fails and names it."""
        doc = table("crate/tests/stream.rs::renamed_away")
        code, out = self.run_check(doc, doc)
        self.assertEqual(code, 1)
        self.assertIn("crate/tests/stream.rs::renamed_away", out)

    def test_a_helper_that_is_not_a_test_fails(self):
        """Given a listed Rust fn without #[test] or a Python def without test_, then the check fails."""
        doc = table("crate/tests/stream.rs::helper_not_a_test", "py/test_stream.py::helper")
        code, out = self.run_check(doc, doc)
        self.assertEqual(code, 1)
        self.assertIn("helper_not_a_test", out)
        self.assertIn("py/test_stream.py::helper", out)

    def test_a_missing_file_fails(self):
        """Given a reference to a file that does not exist, then the check fails."""
        doc = table("crate/tests/gone.rs::chunking_is_invariant")
        code, out = self.run_check(doc, doc)
        self.assertEqual(code, 1)
        self.assertIn("gone.rs", out)

    def test_language_versions_must_list_the_same_tests(self):
        """Given an English table with a test the Chinese table lacks, then the check fails."""
        en = table("crate/tests/stream.rs::chunking_is_invariant", "py/test_stream.py::test_flags_nan")
        zh = table("crate/tests/stream.rs::chunking_is_invariant")
        code, out = self.run_check(en, zh)
        self.assertEqual(code, 1)
        self.assertIn("py/test_stream.py::test_flags_nan", out)

    def test_a_document_without_references_fails(self):
        """Given a table with no test references, then the check fails instead of passing empty."""
        doc = "# Contracts\n\nNothing here.\n"
        code, _ = self.run_check(doc, doc)
        self.assertEqual(code, 2)


if __name__ == "__main__":
    unittest.main()
