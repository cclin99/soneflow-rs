# Recheck commands

Run from the frozen repository root. `verify.py` records each command and exit code in commands.jsonl; ordinary local suites use REQUIRE_AVX2=1 and HASH_GATE_DUMP_ONLY=1. The separately recorded hash-enforced-command.json overrides dump-only to 0 and passed.

- python ../verify.py (delivered copy: verify.py)
- python ../audit.py (delivered copy: audit.py; Git blobs and metadata only; explicit material exclusions)
- gh api repos/cclin99/soneflow-rs/commits?sha=main&per_page=100
- git diff --name-only fe786a0 f4e239b
- git status --short; git diff --exit-code
- rustc -Vv; cargo -V
- cbindgen --version; generated header goes only to evidence folder
- python tools/redact_paths.py <evidence-folder>
- python tools/redact_paths.py --check <evidence-folder>

The initial full historical diff inspection caused the disclosed exposure and must not be repeated. Only new-side excerpts are delivered. Previous-round setup evidence was read and copied without changing its original; no setup rerun, dependency installation, private checker or prohibited test was performed this round.

Finalization uses finalize.py for supplementary path checks, redaction pass metadata and final SHA-256 manifest. All delivery files are UTF-8 text. The manifest excludes itself.
