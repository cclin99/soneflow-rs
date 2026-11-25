# Commands and evidence scope

All product commands ran in an isolated clone at `fe786a0e40cff532968204da70c7187ae2d5bc99`. `commands.jsonl` records automatically logged invocations and exit codes. These supplemental commands record preparation and checks outside that runner.

| Check | Command / method | Result |
|---|---|---|
| Fixed source | `git checkout --detach fe786a0`; `git rev-parse HEAD HEAD^{tree}` | Fixed commit and tree; see release-metadata.json |
| Library | `REQUIRE_AVX2=1 HASH_GATE_DUMP_ONLY=0 cargo test -p soneflow --lib --offline` | 141 passed |
| Header | `cbindgen --config soneflow-ffi/cbindgen.toml --crate soneflow-ffi --output audit-header.h` | Same text; checkout line endings differ |
| Canonical header | Write an LF-only temporary copy of the unchanged cbindgen config; `cbindgen --config audit-cbindgen.toml --crate soneflow-ffi --output audit-header-lf.h` | Raw bytes equal the fixed Git header blob; see header-check.json |
| Versions | Parse three Cargo.toml files, Cargo.lock, CITATION.cff | All 0.1.0 |
| No-data setup, prerequisite missing | `bash tools/setup_env.sh` in initially data-free clone without local MoSQITo tarball | Exit 1 before Annex B guidance; environment prerequisite missing |
| No-data setup, prerequisite supplied | `PIP_VERBOSE=1 bash audit-setup/tools/setup_env.sh` in fresh staging containing only unchanged setup script, requirements lock and ordinary MoSQITo tarball | Exit 0; dependency cache used; empty licensed-data directory guidance printed |
| Synthetic harness preparation | Copy only the 20 exact synthetic files listed in synthetic-inputs.json plus the single approved private test and its manifest | No recursive data copy; no licensed inputs |
| Preliminary synthetic harness | First standalone resolution picked newer cached transitive dependencies; deliberately stopped before test execution | INTERRUPTED, not a numerical failure; original exit code retained |
| Authoritative synthetic accuracy | Seed isolated private Cargo.lock from fixed public Cargo.lock; `CARGO_TARGET_DIR=<AUDIT_ROOT>/target cargo test --offline --manifest-path private/tests-crate/Cargo.toml --release --test mosqito_stats_synthetic -- --nocapture` | Exit 0; 1 passed; every shared dependency version equals fixed lock |
| Public text scan | Tracked-path inventory and keyword/structure scan; own-project paragraphs reviewed, no official source followed | a1-scan.json gives only paths/line numbers; tools/golden.sha256 excluded for user review |
| Product comparison | 66 Rust src files versus local 0a3a7c9; lexical comparison ignores comments/whitespace, normalizes rename identifiers and import order | 64 equal; 2 contain only the explicitly allowed test assertion-message changes |
| GitHub | Read-only GitHub API selected fields: commits?sha=main, actions/runs?head_sha=fe786a0..., actions/runs/45423756240/jobs | Coordinator-collected live JSON; interpreted in this report |
| Source preservation | `git diff --exit-code --stat` | No tracked modifications |

No blanket workspace test command was executed. `clippy --all-targets` only compiled project test code; it did not run data-reading tests. `annexb_validator` always used `--skip csv_`. One ordinary `py_contract_dump` diagnostic is ignored by its existing test declaration; the actual Windows hash gate and Python frozen-hash checks were enforced, not dump-only.

The clone initially had no data directory. The 20 allowed synthetic input files were copied later, after their exact path/control flow was reviewed. The separate setup staging had no data directory before execution and received no data files.

No tag, push, commit, product/test edit, original-worktree edit, or ISO-material checker execution was performed.
