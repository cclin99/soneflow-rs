"""BDD tests for tools/redact_paths.py (directory mode covers every file)."""

import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))
import redact_paths  # noqa: E402

# Built from parts so this file never matches the patterns it tests.
DRIVE_USER = "C:" + "\\" + "Us" + "ers" + "\\" + "someone"
REPO_PATH = "D:" + "\\" + "ISO532"


def run(*args):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = redact_paths.main(list(args))
    return code, out.getvalue(), err.getvalue()


class DirectoryModeBehavior(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, rel, data):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(data, str):
            path.write_text(data, encoding="utf-8", newline="")
        else:
            path.write_bytes(data)
        return path

    def test_text_files_with_any_suffix_are_redacted(self):
        """Given text files whose suffixes were never listed, when a directory is redacted, then each one is."""
        names = ["arch/lcov.info", "probe.rs", "commands.jsonl", ".gitattributes", "DONE", "a.diff"]
        for name in names:
            self.write(name, f"SF:{REPO_PATH}\\iso532\\src\\lib.rs {DRIVE_USER}\\x\n")
        code, out, _ = run(str(self.root))
        self.assertEqual(code, 0)
        self.assertIn(f"{len(names)} file(s) changed", out)
        for name in names:
            text = (self.root / name).read_text(encoding="utf-8")
            self.assertEqual(text, "SF:<REPO>\\iso532\\src\\lib.rs <USER_HOME>\\x\n", name)

    def test_check_sees_paths_in_files_with_unlisted_suffixes(self):
        """Given a path left in lcov.info, when --check runs on the directory, then it is reported, not 'clean'."""
        self.write("report.md", "nothing here\n")
        self.write("arch/lcov.info", f"SF:{REPO_PATH}\\iso532\\src\\lib.rs\n")
        code, out, _ = run("--check", str(self.root))
        self.assertEqual(code, 1)
        self.assertIn("lcov.info: 1 identifying path(s) remain", out)
        self.assertNotIn("clean", out)

    def test_binary_and_non_utf8_files_are_left_alone_and_reported(self):
        """Given a binary and a non-UTF-8 file, when redacting, then neither changes and both are listed as skipped."""
        binary = self.write("img.png", b"\x89PNG\r\n\x1a\n\x00\x00" + REPO_PATH.encode())
        latin1 = self.write("old.txt", ("caf\xe9 " + REPO_PATH).encode("latin-1"))
        before = (binary.read_bytes(), latin1.read_bytes())
        code, out, err = run(str(self.root))
        self.assertEqual(code, 0)
        self.assertEqual((binary.read_bytes(), latin1.read_bytes()), before)
        self.assertIn("img.png: not UTF-8, skipped (inspect manually)", err)
        self.assertIn("old.txt: not UTF-8, skipped (inspect manually)", err)
        self.assertIn("0 file(s) changed, 2 skipped (inspect manually)", out)

    def test_check_is_not_clean_while_files_are_skipped(self):
        """Given only a skipped file, when --check runs, then it does not print a bare 'clean'."""
        self.write("img.png", b"\x89PNG\x00\xff")
        code, out, _ = run("--check", str(self.root))
        self.assertEqual(code, 0)
        self.assertIn("0 identifying path(s) remain; 1 file(s) skipped (inspect manually)", out)
        self.assertNotIn("\nclean", "\n" + out)

    def test_strict_fails_on_skipped_files(self):
        """Given a skipped file, when --strict is set, then the exit code is 2 (redaction and --check alike)."""
        self.write("img.png", b"\x89PNG\x00\xff")
        self.assertEqual(run("--strict", str(self.root))[0], 2)
        self.assertEqual(run("--check", "--strict", str(self.root))[0], 2)

    def test_clean_directory_prints_clean(self):
        """Given only redacted text, when --check runs, then it prints 'clean' and exits 0."""
        self.write("report.md", "<REPO>\\iso532\n")
        self.write("arch/lcov.info", "SF:<REPO>\\iso532\\src\\lib.rs\n")
        code, out, _ = run("--check", "--strict", str(self.root))
        self.assertEqual(code, 0)
        self.assertEqual(out.strip().splitlines()[-1], "clean")

    def test_build_and_vcs_directories_are_still_skipped(self):
        """Given target/ and .git/ inside the folder, when redacting, then files there are not touched."""
        inside = self.write("target/debug/x.d", f"{REPO_PATH}\\target\n")
        self.write(".git/config", f"{REPO_PATH}\n")
        code, out, _ = run(str(self.root))
        self.assertEqual(code, 0)
        self.assertIn("0 file(s) changed", out)
        self.assertIn(REPO_PATH, inside.read_text(encoding="utf-8"))

    def test_text_with_nul_bytes_is_still_text(self):
        """Given a log cut off mid-write (NUL bytes, valid UTF-8), when redacting, then it is processed as text."""
        f = self.write("perf/killed.log", "run\x00\x00 " + REPO_PATH + "\\x\n")
        code, _, err = run(str(self.root))
        self.assertEqual(code, 0)
        self.assertEqual(err, "")
        self.assertEqual(f.read_text(encoding="utf-8"), "run\x00\x00 <REPO>\\x\n")

    def test_second_pass_changes_nothing(self):
        """Given a redacted folder, when the tool runs again, then no file changes."""
        self.write("probe.rs", f"// {DRIVE_USER}\\scratch\n")
        run(str(self.root))
        code, out, _ = run(str(self.root))
        self.assertEqual(code, 0)
        self.assertIn("0 file(s) changed", out)

    def test_file_mode_is_unchanged(self):
        """Given a single file named on the command line, when redacting, then it is processed as before."""
        f = self.write("notes", f"{REPO_PATH}\\x\n")
        code, out, _ = run(str(f))
        self.assertEqual(code, 0)
        self.assertEqual(f.read_text(encoding="utf-8"), "<REPO>\\x\n")



class RuleCoverage(unittest.TestCase):
    """Spellings the 0.2.0 parking lot found uncovered (lowercase profile, renamed working copies)."""

    def assertRedacts(self, before, after):
        self.assertEqual(redact_paths.redact_text(before)[0], after, before)

    def test_lowercase_user_profile_is_redacted(self):
        """Given a profile path printed in lower case, when redacted, then it becomes <USER_HOME>."""
        users = "us" + "ers"
        self.assertRedacts("c:\\" + users + r"\someone\x", r"<USER_HOME>\x")
        self.assertRedacts("C:/" + users + "/someone/x", "<USER_HOME>/x")
        self.assertRedacts("/c/" + users + "/someone/x", "<USER_HOME>/x")
        self.assertRedacts("c:\\" + users + r"\someone\documents\codex\a", r"<AUDIT_HOME>\a")

    def test_current_working_copy_name_is_redacted(self):
        """Given the iso532-1 working copy in every separator spelling, when redacted, then it becomes <REPO>."""
        name = "iso" + "532-1"
        self.assertRedacts("D:\\" + name + r"\soneflow\src", r"<REPO>\soneflow\src")
        self.assertRedacts("d:/" + name + "/soneflow", "<REPO>/soneflow")
        self.assertRedacts("D:\\\\" + name + r"\x", r"<REPO>\x")
        self.assertRedacts("cd /d/" + name + " && ls", "cd <REPO> && ls")

    def test_renamed_working_copies_are_redacted(self):
        """Given a clone named after the old or the new GitHub repo, when redacted, then it becomes <REPO>."""
        for name in ("sone" + "flow-rs", "iso" + "532-1-rs"):
            self.assertRedacts("D:\\" + name + r"\x", r"<REPO>\x")
            self.assertRedacts("/d/" + name + "/x", "<REPO>/x")

    def test_urls_and_bare_names_are_left_alone(self):
        """Given repository names in URLs and prose, when redacted, then nothing changes."""
        for text in ("https://github.com/cclin99/soneflow-rs/actions/runs/1",
                     "the old repo iso532-1-rs is archived",
                     "soneflow-rs/src/lib.rs"):
            self.assertRedacts(text, text)

    def test_wsl_drive_mounts_are_redacted(self):
        """Given WSL spellings (/mnt/<drive>/...), when redacted, then they get the placeholders of /<drive>/..."""
        mnt = "/m" + "nt"
        users = "Us" + "ers"
        self.assertRedacts(mnt + "/c/" + users + "/someone/x", "<USER_HOME>/x")
        self.assertRedacts("cd " + mnt + "/d/iso" + "532-1/soneflow && ls", "cd <REPO>/soneflow && ls")
        self.assertRedacts(mnt + "/d/iso" + "532-rs-panel/x", "<PANEL_REPO>/x")

    def test_sibling_work_directories_are_redacted(self):
        """Given a drive-root directory whose name only starts with a repository name, then it becomes <WORK_DIR>."""
        archive = "ISO" + "532-research-archive"
        audit = "sone" + "flow-astra-0.2.1"
        self.assertRedacts("D:\\" + archive + r"\x", r"<WORK_DIR>\x")
        self.assertRedacts("D:\\\\" + archive + r"\\x", r"<WORK_DIR>\\x")
        self.assertRedacts("d:/" + audit + "/repo/docs", "<WORK_DIR>/repo/docs")
        self.assertRedacts("ls /d/" + archive + "/x", "ls <WORK_DIR>/x")
        self.assertRedacts("/m" + "nt/d/" + audit + "/repo", "<WORK_DIR>/repo")
        self.assertRedacts('"D:\\' + audit + '"', '"<WORK_DIR>"')

    def test_working_copy_subdirectories_keep_their_names(self):
        """Given crate directories inside the working copy, when redacted, then only the working copy prefix changes."""
        name = "iso" + "532-1"
        self.assertRedacts("D:\\" + name + r"\soneflow-ffi\include", r"<REPO>\soneflow-ffi\include")
        self.assertRedacts("/d/" + name + "/soneflow-py/tests", "<REPO>/soneflow-py/tests")

    def test_relative_paths_and_hosts_are_left_alone(self):
        """Given crate-relative paths, diff prefixes and host names, when redacted, then nothing changes."""
        for text in ("soneflow-ffi/include/soneflow.h",
                     "--- a/soneflow/src/lib.rs",
                     "+++ b/soneflow-ffi/src/lib.rs",
                     "https://soneflow.example/x",
                     "scheme:/soneflow/x",
                     "/home/runner/work/soneflow-rs/soneflow-rs/soneflow",
                     "/m" + "nt/data/soneflow"):
            self.assertRedacts(text, text)


if __name__ == "__main__":
    unittest.main()
