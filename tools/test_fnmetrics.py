"""BDD tests for tools/fnmetrics.py (the per-function CC / ploc gate).

rust-code-analysis-cli is replaced by a stub that writes its JSON layout, so
these tests need neither the CLI nor a Rust build.
"""

import contextlib
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

TOOLS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))
import fnmetrics  # noqa: E402

LIB_RS = """\
pub fn product(x: u32) -> u32 {
    x + 1
}

#[cfg(test)]
mod tests {
    #[test]
    fn big_test() {
        assert_eq!(super::product(1), 2);
    }
}
"""


def function(name, start, end, cc, ploc):
    return {
        "kind": "function", "name": name, "start_line": start, "end_line": end,
        "metrics": {
            "loc": {"sloc": end - start + 1, "ploc": ploc, "lloc": ploc},
            "cyclomatic": {"sum": cc}, "cognitive": {"sum": 0},
            "nargs": {"total": 1}, "nexits": {"sum": 1},
        },
        "spaces": [],
    }


class CheckGate(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name)
        for crate in fnmetrics.CRATES:
            (self.repo / crate / "src").mkdir(parents=True)
        (self.repo / "soneflow" / "src" / "lib.rs").write_text(LIB_RS, encoding="utf-8")

    def tearDown(self):
        self.tmp.cleanup()

    def run_tool(self, functions, *args):
        def fake_rca(repo, out):
            target = out / "soneflow" / "src" / "lib.rs.json"
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(json.dumps({"kind": "unit", "spaces": functions}), encoding="utf-8")

        argv = ["fnmetrics.py", "--repo", str(self.repo), *args]
        out = io.StringIO()
        with mock.patch.object(fnmetrics, "run_rca", fake_rca), mock.patch.object(sys, "argv", argv), \
                contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            code = fnmetrics.main()
        return code, out.getvalue()

    def test_product_function_over_cc_fails_the_check(self):
        """Given a product function with CC 16, when --check runs, then it exits 1 and names the function."""
        code, out = self.run_tool([function("product", 1, 3, cc=16, ploc=3)], "--check")
        self.assertEqual(code, 1)
        self.assertIn("over limits (cc > 15 or ploc > 80): 1", out)
        self.assertIn("soneflow/src/lib.rs:1 product", out)

    def test_product_function_over_ploc_fails_the_check(self):
        """Given a product function with 81 ploc, when --check runs, then it exits 1."""
        code, _ = self.run_tool([function("product", 1, 3, cc=2, ploc=81)], "--check")
        self.assertEqual(code, 1)

    def test_functions_at_the_limits_pass(self):
        """Given a product function at exactly CC 15 and 80 ploc, when --check runs, then it exits 0."""
        code, out = self.run_tool([function("product", 1, 3, cc=15, ploc=80)], "--check")
        self.assertEqual(code, 0)
        self.assertIn("over limits (cc > 15 or ploc > 80): 0", out)

    def test_test_functions_do_not_count(self):
        """Given an oversized function inside #[cfg(test)] mod tests, when --check runs, then it exits 0."""
        rows = [function("product", 1, 3, cc=2, ploc=3), function("big_test", 8, 10, cc=40, ploc=200)]
        code, out = self.run_tool(rows, "--check")
        self.assertEqual(code, 0)
        self.assertIn("functions 2  product 1  test 1", out)

    def test_without_check_an_excess_is_reported_but_exit_is_zero(self):
        """Given a product function over the limits, when run without --check, then it is listed and exits 0."""
        code, out = self.run_tool([function("product", 1, 3, cc=16, ploc=3)])
        self.assertEqual(code, 0)
        self.assertIn("over limits (cc > 15 or ploc > 80): 1", out)

    def test_no_analyser_output_is_an_error(self):
        """Given the analyser writes nothing, when --check runs, then it exits 2 instead of passing."""
        argv = ["fnmetrics.py", "--repo", str(self.repo), "--check"]
        with mock.patch.object(fnmetrics, "run_rca", lambda repo, out: None), \
                mock.patch.object(sys, "argv", argv), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(fnmetrics.main(), 2)


if __name__ == "__main__":
    unittest.main()
