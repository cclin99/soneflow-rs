"""Redact this delivery, inspect every format, and seal post-redaction hashes.

Run with the designated audit root as the sole argument. No material checker
is run. The manifest covers every file except itself, including this script.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

root = Path(sys.argv[1]).resolve()
delivery = Path(__file__).resolve().parent
repo = root / "repo"
assert delivery.is_relative_to(repo / "docs" / "verification")
os.environ.update(TEMP=str(root / "tmp"), TMP=str(root / "tmp"),
                  PYTHONDONTWRITEBYTECODE="1")

# The repository tool predates this audit-root name. Normalize this additional
# prefix in ordinary, slash, and JSON-escaped forms before its standard pass.
sep = r"(?:\\\\|\\|/)"
pattern = re.compile(sep.join(re.escape(p) for p in str(root).split("\\")), re.I)
extra_changes = []
for p in sorted(delivery.rglob("*")):
    if not p.is_file():
        continue
    assert not p.is_symlink()
    raw = p.read_bytes()
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        continue
    text, count = pattern.subn("<AUDIT_ROOT>", text)
    if count:
        p.write_bytes(text.encode("utf-8"))
        extra_changes.append({"file": p.relative_to(delivery).as_posix(), "count": count})

tool = repo / "tools" / "redact_paths.py"
passes = []
for args in [[str(delivery)], ["--check", "--strict", str(delivery)]]:
    proc = subprocess.run([sys.executable, str(tool), *args], cwd=repo,
                          capture_output=True, text=True, encoding="utf-8")
    output = pattern.sub("<AUDIT_ROOT>", proc.stdout + proc.stderr)
    passes.append({"arguments": [pattern.sub("<AUDIT_ROOT>", a) for a in args],
                   "exit": proc.returncode, "output": output})
    assert proc.returncode == 0, output
assert passes[-1]["output"].strip() == "clean"

non_utf8 = []
archives = []
bad_paths = []
extensions = {}
for p in sorted(delivery.rglob("*")):
    if not p.is_file():
        continue
    rel = p.relative_to(delivery).as_posix()
    extensions[p.suffix or "<none>"] = extensions.get(p.suffix or "<none>", 0) + 1
    raw = p.read_bytes()
    if raw.startswith((b"PK\x03\x04", b"MZ", b"\x7fELF")):
        archives.append(rel)
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        non_utf8.append(rel)
        continue
    if pattern.search(text):
        bad_paths.append(rel)
assert not non_utf8 and not archives and not bad_paths

audit = {"extra_prefix_redaction": extra_changes, "standard_redaction_passes": passes,
         "format_inspection": {"non_utf8": non_utf8, "archive_or_executable": archives,
                               "remaining_audit_root_paths": bad_paths,
                               "extensions_seen_before_sealing": extensions},
         "meaning": "Delivery hashes describe redacted files. Source and binary hashes in evidence retain their original-input meaning."}
(delivery / "delivery-audit.json").write_text(json.dumps(audit, indent=2) + "\n", encoding="utf-8")

manifest = delivery / "integrity.json"
entries = {}
for p in sorted(delivery.rglob("*")):
    if p.is_file() and p != manifest:
        raw = p.read_bytes()
        entries[p.relative_to(delivery).as_posix()] = {
            "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}
manifest.write_text(json.dumps({"algorithm": "SHA-256", "excludes": ["integrity.json"],
                                "files": entries}, indent=2) + "\n", encoding="utf-8")

# Independently reload the saved manifest and verify its coverage and bytes.
saved = json.loads(manifest.read_text(encoding="utf-8"))["files"]
actual = {p.relative_to(delivery).as_posix() for p in delivery.rglob("*")
          if p.is_file() and p != manifest}
assert set(saved) == actual
for name, entry in saved.items():
    raw = (delivery / name).read_bytes()
    assert len(raw) == entry["bytes"]
    assert hashlib.sha256(raw).hexdigest() == entry["sha256"]
print(json.dumps({"files_hashed": len(saved), "coverage_mismatches": 0,
                  "hash_mismatches": 0, "non_utf8": 0, "archives_or_executables": 0,
                  "redaction": "clean"}))
