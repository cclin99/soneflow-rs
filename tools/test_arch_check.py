"""BDD tests for the architecture gate (tools/arch_check.py).

Run: python -m unittest tools/test_arch_check.py
Feature: 架構閘門 (architecture gate).
"""

import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
REPO = TOOLS.parent
sys.path.insert(0, str(TOOLS))

import arch_check  # noqa: E402  (RED until tools/arch_check.py exists)

RULES = """
facade = ["crate"]
require_rules = true

[[layer]]
module = "tables"
allow = []

[[layer]]
module = "core"
allow = ["tables"]

[[layer]]
module = "app"
allow = ["core", "tables"]

[crap]
max = 30.0

[[crap.scope]]
path = "core"
cc_max = 8

[[size]]
path = "core"
code_lines_max = 200

[[forbid]]
path = "core"
pattern = 'allow\\(clippy::too_many_arguments\\)'
"""

CLEAN = {
    "lib.rs": "pub mod app;\npub mod core;\npub mod tables;\n",
    "tables.rs": "pub const T: [f64; 2] = [1.0, 2.0];\n",
    "core/mod.rs": "pub mod a;\n",
    "core/a.rs": "use crate::tables::T;\npub fn f() -> f64 { T[0] }\n",
    "app.rs": "use crate::core::a::f;\npub fn g() -> f64 { f() + crate::tables::T[1] }\n",
}


class Crate:
    """A synthetic crate in a temporary directory."""

    def __init__(self, files, rules=RULES):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.src = self.root / "src"
        for rel, text in files.items():
            p = self.src / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text, encoding="utf-8")
        self.rules = self.root / "rules.toml"
        self.rules.write_text(rules, encoding="utf-8")

    def crap(self, entries):
        p = self.root / "crap.json"
        for e in entries:
            e["file"] = str(self.src / e["file"])
        p.write_text(json.dumps({"entries": entries}), encoding="utf-8")
        return p

    def run(self, crap=None, skip_crap=None):
        # Scenarios that are not about metrics skip them explicitly.
        if skip_crap is None:
            skip_crap = crap is None
        return arch_check.run(self.src, self.rules, crap, skip_crap)

    def cli(self, *args):
        out = io.StringIO()
        with redirect_stdout(out):
            code = arch_check.main(["--src", str(self.src), "--rules", str(self.rules), *args])
        return code, out.getvalue()

    def close(self):
        self._tmp.cleanup()


def kinds(result):
    return {k for k, v in result["violations"].items() if v}


class ArchitectureGateBehavior(unittest.TestCase):
    def setUp(self):
        self.crates = []

    def tearDown(self):
        for c in self.crates:
            c.close()

    def crate(self, files, rules=RULES):
        c = Crate(files, rules)
        self.crates.append(c)
        return c

    def test_clean_layered_crate_passes(self):
        """Given core uses only tables and app uses core, when checked, then no violation."""
        result = self.crate(CLEAN).run()
        self.assertEqual(kinds(result), set(), result["violations"])

    def test_parent_child_mutual_use_is_a_cycle(self):
        """Given parent uses a child item and the child uses super::K, then a cycle is reported."""
        files = dict(CLEAN)
        files["core/mod.rs"] = "pub mod a;\npub use a::f;\npub const K: f64 = 1.0;\n"
        files["core/a.rs"] = "use super::K;\npub fn f() -> f64 { K }\n"
        result = self.crate(files).run()
        self.assertIn("cycles", kinds(result))
        self.assertIn("crate::core::a", " ".join(result["violations"]["cycles"]))

    def test_lower_layer_using_upper_layer_is_a_violation(self):
        """Given core uses crate::app, then the edge core::a -> app is a layer violation."""
        files = dict(CLEAN)
        files["core/a.rs"] = "use crate::app::g;\npub fn f() -> f64 { g() }\n"
        result = self.crate(files).run()
        self.assertIn("layers", kinds(result))
        self.assertTrue(
            any("crate::core::a -> crate::app" in v for v in result["violations"]["layers"]),
            result["violations"]["layers"],
        )

    def test_module_without_rule_is_a_violation_when_rules_are_required(self):
        """Given a new top-level module without a layer rule, then it is reported."""
        files = dict(CLEAN)
        files["lib.rs"] += "pub mod extra;\n"
        files["extra.rs"] = "pub fn h() {}\n"
        result = self.crate(files).run()
        self.assertIn("unruled", kinds(result))

    def test_cc_limit_applies_only_inside_its_scope(self):
        """Given CC 9 in core and CC 20 (CRAP 20) in app, then only the core function is reported."""
        c = self.crate(CLEAN)
        crap = c.crap([
            {"file": "core/a.rs", "function": "f", "line": 2, "cyclomatic": 9.0, "coverage": 100.0, "crap": 9.0},
            {"file": "app.rs", "function": "g", "line": 2, "cyclomatic": 20.0, "coverage": 100.0, "crap": 20.0},
        ])
        result = c.run(crap)
        self.assertEqual(kinds(result), {"crap"})
        self.assertEqual(len(result["violations"]["crap"]), 1)
        self.assertIn("f", result["violations"]["crap"][0])

    def test_global_crap_limit_applies_everywhere(self):
        """Given a function outside every scope with CRAP 31, then the global limit reports it."""
        c = self.crate(CLEAN)
        crap = c.crap([
            {"file": "app.rs", "function": "g", "line": 2, "cyclomatic": 6.0, "coverage": 10.0, "crap": 31.0},
        ])
        result = c.run(crap)
        self.assertEqual(kinds(result), {"crap"})

    def test_file_size_counts_only_non_test_code(self):
        """Given 250 comment/blank lines, a test module and 10 code lines, then the size limit holds."""
        body = "//! doc\n" * 120 + "\n" * 130
        body += "".join(f"pub fn f{i}() {{}}\n" for i in range(10))
        body += "#[cfg(test)]\nmod tests {\n" + "    fn t() {}\n" * 60 + "}\n"
        files = dict(CLEAN)
        files["core/mod.rs"] = "pub mod a;\npub mod big;\n"
        files["core/big.rs"] = body
        result = self.crate(files).run()
        self.assertNotIn("size", kinds(result))
        self.assertEqual(result["info"]["code_lines"]["core/big.rs"], 10)

    def test_test_module_file_does_not_count_toward_size(self):
        """Given a 201-line file brought in by `#[cfg(test)] mod tests;`, then the size limit holds."""
        files = dict(CLEAN)
        files["core/mod.rs"] = "pub mod a;\n#[cfg(test)]\nmod tests;\n"
        files["core/tests.rs"] = "".join(f"fn t{i}() {{}}\n" for i in range(201))
        result = self.crate(files).run()
        self.assertNotIn("size", kinds(result))
        self.assertNotIn("core/tests.rs", result["info"]["code_lines"])

    def test_file_over_the_code_line_limit_is_reported(self):
        """Given 201 code lines in core, then the size limit reports the file."""
        files = dict(CLEAN)
        files["core/mod.rs"] = "pub mod a;\npub mod big;\n"
        files["core/big.rs"] = "".join(f"pub fn f{i}() {{}}\n" for i in range(201))
        result = self.crate(files).run()
        self.assertIn("size", kinds(result))

    def test_forbidden_pattern_is_reported_with_its_line(self):
        """Given #[allow(clippy::too_many_arguments)] in core, then file and line are reported."""
        files = dict(CLEAN)
        files["core/a.rs"] = (
            "use crate::tables::T;\n#[allow(clippy::too_many_arguments)]\npub fn f() -> f64 { T[0] }\n"
        )
        result = self.crate(files).run()
        self.assertIn("forbid", kinds(result))
        self.assertIn("core/a.rs:2", result["violations"]["forbid"][0])

    def test_cli_returns_one_on_a_violation_and_writes_json(self):
        """Given a layer violation, when run from the command line, then exit code 1 and a JSON report."""
        files = dict(CLEAN)
        files["core/a.rs"] = "use crate::app::g;\npub fn f() -> f64 { g() }\n"
        c = self.crate(files)
        out = c.root / "report.json"
        with redirect_stdout(io.StringIO()):
            code = arch_check.main(["--src", str(c.src), "--rules", str(c.rules), "--json", str(out)])
        self.assertEqual(code, 1)
        self.assertTrue(json.loads(out.read_text(encoding="utf-8"))["violations"]["layers"])

    def test_repository_rules_hold_for_layers_cycles_size_and_patterns(self):
        """Given this repository and tools/arch_rules.toml, then layers, cycles, size and patterns hold."""
        result = arch_check.run(REPO / "soneflow" / "src", TOOLS / "arch_rules.toml", None, skip_crap=True)
        self.assertEqual(kinds(result), set(), result["violations"])

    def test_repository_rules_give_every_core_module_its_own_rule(self):
        """R7a criterion 4': require_exact_rules covers core, and every core module has a rule of its own."""
        rules = arch_check.tomllib.loads((TOOLS / "arch_rules.toml").read_text(encoding="utf-8"))
        self.assertIn("core", rules.get("require_exact_rules", []))
        own = {r["module"] for r in rules["layer"]}
        modules, _ = arch_check.mg.build_graph(REPO / "soneflow" / "src")
        core = sorted("::".join(m) for m in modules if m[:1] == ("core",))
        self.assertGreaterEqual(len(core), 7, core)
        self.assertEqual([m for m in core if m not in own], [])


# ---- The independent review's 16 controls (tools/architecture-controls.json) ----

CONTROLS = REPO / "tools/architecture-controls.json"

# Metrics per control, as in the review's architecture_controls.py: None = no
# --crap file; "valid" = one function in src/core.rs (CRAP 2, CC 1); "empty" = no
# entries; "outside" = one entry outside --src (CRAP 999); "high" = CRAP 100, CC 9.
CONTROL_METRICS = {
    "valid-layer": "valid",
    "core-direct-upward": "valid",
    "parent-child-cycle": None,
    "high-complexity-metric": "high",
    "missing-metrics-full-gate": None,
    "empty-metrics-full-gate": "empty",
    "wrong-root-metrics-full-gate": "outside",
    "forbid-exact": "valid",
    "forbid-whitespace": "valid",
    "forbid-list": "valid",
    "forbid-comment-false-positive": "valid",
    "forbid-mul-add-exact": "valid",
    "forbid-mul-add-whitespace": "valid",
    "inline-module-upward": None,
    "qualified-whitespace-path": "valid",
    "core-size-201": "valid",
}

# The violation each rejected control must be rejected *for* (not only "rejected":
# a missing --crap now rejects everything, which would hide the cause).
CONTROL_CAUSE = {
    "core-direct-upward": "layers",
    "parent-child-cycle": "cycles",
    "high-complexity-metric": "crap",
    "missing-metrics-full-gate": "crap",
    "empty-metrics-full-gate": "crap",
    "wrong-root-metrics-full-gate": "crap",
    "forbid-exact": "forbid",
    "forbid-whitespace": "forbid",
    "forbid-list": "forbid",
    "forbid-mul-add-exact": "forbid",
    "forbid-mul-add-whitespace": "forbid",
    "inline-module-upward": "layers",
    "qualified-whitespace-path": "layers",
    "core-size-201": "size",
}


def run_control(control):
    """Run one control as the review did: fixture text verbatim, the repository rules."""
    with tempfile.TemporaryDirectory(prefix="r7a-control-") as td:
        base = Path(td)
        src = base / "src"
        src.mkdir()
        for rel, text in control["source"].items():
            p = src / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text, encoding="utf-8")
        metrics = CONTROL_METRICS[control["name"]]
        crap = None
        if metrics is not None:
            crap = base / "crap.json"
            # One low probe per CRAP scope of the repository rules (core first),
            # so a valid control is not rejected for an empty scope.
            rules = arch_check.tomllib.loads((TOOLS / "arch_rules.toml").read_text(encoding="utf-8"))
            scopes = [s["path"] for s in rules["crap"].get("scope", [])]
            assert scopes and scopes[0] == "core", scopes
            data = [
                dict(function="probe", file=str(src / f"{scope}.rs"), line=1, crap=2.0, cyclomatic=1, coverage=1.0)
                for scope in scopes
            ]
            if metrics == "empty":
                data = []
            if metrics == "outside":
                for i, d in enumerate(data):
                    d["file"] = str(base / f"elsewhere{i}.rs")
                    d["crap"] = 999.0
            if metrics == "high":
                data[0].update(crap=100.0, cyclomatic=9)
            crap.write_text(json.dumps(data), encoding="utf-8")
        return arch_check.run(src, TOOLS / "arch_rules.toml", crap)


class ReviewControls(unittest.TestCase):
    def test_all_sixteen_controls_meet_their_expectation(self):
        """Given the review's 16 control fixtures, then each is accepted or rejected as expected, for its own cause."""
        controls = json.loads(CONTROLS.read_text(encoding="utf-8"))
        self.assertEqual(sorted(c["name"] for c in controls), sorted(CONTROL_METRICS))
        for control in controls:
            with self.subTest(control["name"]):
                result = run_control(control)
                if control["expected_rejected"]:
                    self.assertIn(CONTROL_CAUSE[control["name"]], kinds(result), result["violations"])
                else:
                    self.assertEqual(kinds(result), set(), result["violations"])


# ---- R7a: new gate scenarios (I2-I5, exact rules) ----

RULES_R7A = """
facade = ["crate"]
require_rules = true

[[layer]]
module = "tables"
allow = []

[[layer]]
module = "types"
allow = []

[[layer]]
module = "core"
allow = ["tables", "types"]

[[layer]]
module = "zwtv"
allow = ["core", "tables", "types"]

[crap]
max = 30.0

[[crap.scope]]
path = "core"
cc_max = 8

[[forbid]]
path = "core"
lint = "clippy::too_many_arguments"

[[forbid]]
path = "core"
call = "mul_add"
"""

BASE_R7A = {
    "lib.rs": "pub mod core;\npub mod tables;\npub mod types;\npub mod zwtv;\n",
    "tables.rs": "pub const T: u32 = 1;\n",
    "types.rs": "pub struct U;\n",
    "core/mod.rs": "pub mod a;\n",
    "core/a.rs": "use crate::tables::T;\npub fn f() -> u32 { T }\n",
    "zwtv.rs": "pub const X: u32 = 1;\n",
}


class GateHardening(unittest.TestCase):
    def setUp(self):
        self.crates = []

    def tearDown(self):
        for c in self.crates:
            c.close()

    def crate(self, files, rules=RULES_R7A):
        c = Crate(files, rules)
        self.crates.append(c)
        return c

    def test_missing_metrics_fail_unless_explicitly_skipped(self):
        """I2: without --crap the CLI prints crap: SKIPPED and exits 1; with --skip-crap it exits 0."""
        c = self.crate(BASE_R7A)
        code, out = c.cli()
        self.assertEqual(code, 1, out)
        self.assertIn("crap: SKIPPED", out)
        code, out = c.cli("--skip-crap")
        self.assertEqual(code, 0, out)
        self.assertIn("crap: SKIPPED", out)

    def test_a_crap_scope_without_functions_is_a_violation(self):
        """I2: metrics with functions outside core only, then scope core is reported as empty."""
        c = self.crate(BASE_R7A)
        crap = c.crap([
            {"file": "zwtv.rs", "function": "g", "line": 1, "cyclomatic": 1.0, "coverage": 100.0, "crap": 1.0},
        ])
        result = c.run(crap)
        self.assertEqual(kinds(result), {"crap"})
        self.assertIn("'core' has no functions", " ".join(result["violations"]["crap"]))

    def test_forbidden_lint_in_expect_and_cfg_attr_but_not_in_a_string(self):
        """I3: expect(...) and cfg_attr(..., allow(...)) are reported once each; a string literal is not."""
        files = dict(BASE_R7A)
        files["core/mod.rs"] = "pub mod a;\npub mod b;\npub mod c;\n"
        files["core/a.rs"] = "#[expect(clippy::too_many_arguments)]\npub fn f() {}\n"
        files["core/b.rs"] = "#[cfg_attr(test, allow(clippy :: too_many_arguments))]\npub fn g() {}\n"
        files["core/c.rs"] = 'pub const S: &str = "#[allow(clippy::too_many_arguments)]";\n'
        result = self.crate(files).run()
        self.assertEqual(sorted(result["violations"]["forbid"]), [
            "core/a.rs:1: lint clippy::too_many_arguments (core)",
            "core/b.rs:1: lint clippy::too_many_arguments (core)",
        ])

    def test_path_form_mul_add_is_forbidden(self):
        """I3: f64::mul_add(x, 2.0, 1.0) in core is a forbidden call."""
        files = dict(BASE_R7A)
        files["core/a.rs"] = "pub fn f(x: f64) -> f64 {\n    f64::mul_add(x, 2.0, 1.0)\n}\n"
        result = self.crate(files).run()
        self.assertEqual(result["violations"]["forbid"], ["core/a.rs:2: call mul_add (core)"])

    def test_literals_comments_and_lifetimes_do_not_confuse_the_scan(self):
        """I3: raw strings, block comments, char literals and lifetimes neither hide nor fake a hit."""
        files = dict(BASE_R7A)
        files["core/a.rs"] = (
            "/* x.mul_add(1.0, 2.0) /* nested */ still comment */\n"
            'pub const R: &str = r#"x.mul_add(1.0, 2.0) " still raw"#;\n'
            "pub const Q: char = '\\'';\n"
            "pub fn f<'a>(x: &'a f64) -> f64 { x.mul_add(2.0, 1.0) }\n"
        )
        result = self.crate(files).run()
        self.assertEqual(result["violations"]["forbid"], ["core/a.rs:4: call mul_add (core)"])

    def test_inline_test_module_is_not_a_dependency_and_nested_inline_modules_are_layered(self):
        """I4: crate::core::inner -> crate::zwtv is reported; the cfg(test) module's use is not."""
        files = {
            "lib.rs": (
                "pub mod tables { pub const T: u32 = 1; }\n"
                "pub mod types { pub struct U; }\n"
                "pub mod zwtv { pub const X: u32 = 1; }\n"
                "pub mod core {\n"
                "    pub mod inner { use crate::zwtv::X; pub fn f() -> u32 { X } }\n"
                "    #[cfg(test)]\n"
                "    mod tests { use crate::zwtv::X; }\n"
                "}\n"
            ),
        }
        result = self.crate(files).run()
        self.assertEqual(kinds(result), {"layers"}, result["violations"])
        self.assertEqual(len(result["violations"]["layers"]), 1)
        self.assertTrue(result["violations"]["layers"][0].startswith("crate::core::inner -> crate::zwtv"))

    def test_use_with_an_alias_resolves_to_the_aliased_module(self):
        """`use crate::zwtv as z;` and `use crate::{zwtv::X as Y}` are edges to crate::zwtv."""
        files = dict(BASE_R7A)
        files["core/mod.rs"] = "pub mod a;\npub mod b;\n"
        files["core/a.rs"] = "use crate::zwtv as z;\npub fn f() -> u32 { z::X }\n"
        files["core/b.rs"] = "use crate::{zwtv::X as Y};\npub fn g() -> u32 { Y }\n"
        result = self.crate(files).run()
        self.assertEqual(sorted(v.split(" (")[0] for v in result["violations"]["layers"]), [
            "crate::core::a -> crate::zwtv",
            "crate::core::b -> crate::zwtv",
        ])

    def test_path_attribute_and_missing_module_file_are_unsupported(self):
        """I4: #[path] and a `mod x;` without a file are each reported, exit code 1."""
        files = dict(BASE_R7A)
        files["core/mod.rs"] = 'pub mod a;\n#[path = "x.rs"]\nmod b;\nmod missing;\n'
        files["core/x.rs"] = "pub fn h() {}\n"
        c = self.crate(files)
        result = c.run()
        self.assertEqual(kinds(result), {"unsupported"}, result["violations"])
        self.assertEqual(len(result["violations"]["unsupported"]), 2)
        code, _ = c.cli("--skip-crap")
        self.assertEqual(code, 1)

    def test_bare_cli_uses_the_repository_defaults(self):
        """I5: with no --src / --rules the gate checks soneflow/src against tools/arch_rules.toml."""
        out = io.StringIO()
        with redirect_stdout(out):
            code = arch_check.main(["--skip-crap"])
        self.assertEqual(code, 0, out.getvalue())
        self.assertIn(f"src: {REPO / 'soneflow' / 'src'}", out.getvalue())
        self.assertIn(f"rules: {TOOLS / 'arch_rules.toml'}", out.getvalue())

    def test_exact_rules_require_a_rule_for_every_module_in_the_subtree(self):
        """require_exact_rules: core::b relies on core's rule, so it is reported; core::a is not."""
        files = dict(BASE_R7A)
        files["core/mod.rs"] = "pub mod a;\npub mod b;\n"
        files["core/b.rs"] = "pub fn g() {}\n"
        rules = RULES_R7A.replace(
            "require_rules = true", 'require_rules = true\nrequire_exact_rules = ["core"]'
        ) + '\n[[layer]]\nmodule = "core::a"\nallow = ["tables"]\n'
        result = self.crate(files, rules).run()
        self.assertEqual(kinds(result), {"unruled"}, result["violations"])
        self.assertEqual(len(result["violations"]["unruled"]), 1)
        self.assertTrue(result["violations"]["unruled"][0].startswith("crate::core::b "))

    def test_a_stricter_submodule_rule_takes_precedence(self):
        """core allows tables and types, core::a only tables; core::a using types is a violation."""
        files = dict(BASE_R7A)
        files["core/a.rs"] = "use crate::types::U;\npub fn f() -> U { U }\n"
        rules = RULES_R7A + '\n[[layer]]\nmodule = "core::a"\nallow = ["tables"]\n'
        result = self.crate(files, rules).run()
        self.assertEqual(kinds(result), {"layers"}, result["violations"])
        self.assertTrue(result["violations"]["layers"][0].startswith("crate::core::a -> crate::types"))


class TestOnlyCode(unittest.TestCase):
    """Code compiled only under `cfg(test)` is not architecture: a test-only module
    written as `cfg(all(test, ...))`, and test-gated items, statements and
    parameters outside a test module, add no dependency edge. `cfg(any(test, ...))`
    and `cfg(not(test))` are product code and still count."""

    PARENT = "pub mod a;\npub use a::f;\n#[cfg(test)]\npub(crate) mod hooks { pub fn rec() {} pub struct Ctx; }\n"

    def setUp(self):
        self.crates = []

    def tearDown(self):
        for c in self.crates:
            c.close()

    def run_files(self, changes):
        files = dict(CLEAN)
        files.update(changes)
        c = Crate(files, RULES)
        self.crates.append(c)
        return c.run(skip_crap=True)

    def test_cfg_all_test_module_is_test_code(self):
        """Given core declares `#[cfg(all(test, target_arch = ...))] mod t;` that uses app, then no layer violation."""
        result = self.run_files({
            "core/mod.rs": 'pub mod a;\n#[cfg(all(test, target_arch = "x86_64"))]\nmod t;\n',
            "core/t.rs": "use crate::app::g;\n",
        })
        self.assertEqual(kinds(result), set(), result["violations"])

    def test_cfg_any_test_module_is_product_code(self):
        """Given `#[cfg(any(test, feature = "x"))] mod t;` uses app, then the layer violation is reported."""
        result = self.run_files({
            "core/mod.rs": 'pub mod a;\n#[cfg(any(test, feature = "x"))]\nmod t;\n',
            "core/t.rs": "use crate::app::g;\n",
        })
        self.assertEqual(kinds(result), {"layers"}, result["violations"])

    def test_test_gated_items_statements_and_parameters_are_not_dependencies(self):
        """Given a child reaches the parent's cfg(test) hooks only through a test-gated
        parameter, statement and function, then no cycle is reported."""
        result = self.run_files({
            "core/mod.rs": self.PARENT,
            "core/a.rs": (
                "pub fn f(\n"
                "    x: f64,\n"
                "    #[cfg(test)] // hook\n"
                "    ctx: &super::hooks::Ctx,\n"
                ") -> f64 {\n"
                "    #[cfg(test)]\n"
                "    super::hooks::rec();\n"
                "    x\n"
                "}\n"
                "#[cfg(test)]\n"
                "#[inline(never)]\n"
                "fn g(c: &super::hooks::Ctx) -> f64 { super::hooks::rec(); 0.0 }\n"
            ),
        })
        self.assertEqual(kinds(result), set(), result["violations"])

    def test_not_test_gated_statement_still_counts(self):
        """Given the child uses the parent through a `#[cfg(not(test))]` statement, then the cycle is reported."""
        result = self.run_files({
            "core/mod.rs": "pub mod a;\npub use a::f;\npub const K: f64 = 1.0;\n",
            "core/a.rs": (
                "pub fn f() -> f64 {\n"
                "    #[cfg(not(test))]\n"
                "    let _k = super::K;\n"
                "    0.0\n"
                "}\n"
            ),
        })
        self.assertIn("cycles", kinds(result))


class CrapTestModuleFiles(unittest.TestCase):
    """CRAP metrics of a file that is a `cfg(test)` module (declared `mod x;`, at
    any depth below it) are test code and not gated; product files are."""

    FILES = dict(
        CLEAN,
        **{
            "core/mod.rs": "pub mod a;\n#[cfg(test)]\nmod tests;\n#[cfg(all(test, target_arch = \"x86_64\"))]\nmod x86_tests;\n",
            "core/tests/mod.rs": "mod helper;\n",
            "core/tests/helper.rs": "fn check() {}\n",
            "core/x86_tests.rs": "fn probe() {}\n",
        },
    )

    def run_crap(self, entries):
        c = Crate(self.FILES, RULES)
        self.addCleanup(c.close)
        return c.run(c.crap(entries))

    def test_functions_in_test_module_files_are_not_gated(self):
        """Given CRAP 42 in core/tests/helper.rs and 56 in core/x86_tests.rs, then crap passes."""
        result = self.run_crap([
            {"file": "core/a.rs", "function": "f", "line": 2, "cyclomatic": 1.0, "coverage": 100.0, "crap": 1.0},
            {"file": "core/tests/helper.rs", "function": "check", "line": 1, "cyclomatic": 6.0, "coverage": None, "crap": 42.0},
            {"file": "core/x86_tests.rs", "function": "probe", "line": 1, "cyclomatic": 7.0, "coverage": 0.0, "crap": 56.0},
        ])
        self.assertEqual(kinds(result), set(), result["violations"])

    def test_product_file_beside_test_modules_is_still_gated(self):
        """Given CRAP 56 in the product file core/a.rs, then crap reports it."""
        result = self.run_crap([
            {"file": "core/a.rs", "function": "f", "line": 2, "cyclomatic": 7.0, "coverage": 0.0, "crap": 56.0},
            {"file": "core/tests/helper.rs", "function": "check", "line": 1, "cyclomatic": 6.0, "coverage": None, "crap": 42.0},
        ])
        self.assertEqual(kinds(result), {"crap"})
        self.assertEqual(len(result["violations"]["crap"]), 1)
        self.assertIn("core/a.rs", result["violations"]["crap"][0])


if __name__ == "__main__":
    unittest.main()
