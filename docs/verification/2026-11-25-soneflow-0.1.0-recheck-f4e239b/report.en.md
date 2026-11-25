# soneflow 0.1.0 independent recheck: PASS

Auditor: gpt-6-astra. Date: 2026-11-25 (Asia/Taipei). Frozen snapshot: `f4e239bb7cf3f79426c8812d6f096455a3c41bfb`; parent `fe786a0e40cff532968204da70c7187ae2d5bc99`. Scope is exactly R1–R5 in the authorized recheck brief. PASS is within the brief's non-ISO-material boundary; user-only material checks remain pending and are not counted as failures.

| Gate | Verdict | Evidence |
|---|---|---|
| R1 Public tree | PASS within permitted inspection | `r1-scan.json`, `new-side-review.txt`, previous round's A1 scan/disposition. All 159 tracked paths inventoried; 157 text files scanned, checksum manifest and owned binary body excluded. The two prior finding locations are corrected. No private email; workstation matches are generic redaction patterns. |
| R2 Numerical invariance | PASS | `token-comparison.json`: both changed Rust files and C header have identical non-comment tokens without rename/constant normalization. All other Rust files unchanged. `snapshot-checks.json`: CI non-comment lines unchanged. `header-check.json`: cbindgen 0.29.4 output equals both checked-in Git blob and worktree bytes. Exact-snapshot CI and local data-free checks below. |
| R3 Printed precision | PASS | Both `r3-README*.json`: the only README change is 1.8e-4 to 1.7e-4. Prior synthetic measured 1.748e-4 rounds to 1.7e-4 at two significant digits. Other accuracy and timing figures unchanged; not remeasured. |
| R4 No ISO download | PASS | `r4-unchanged.json`: setup script and requirements lock identical to parent. Reused successful second prior no-data staging, with documented ordinary MoSQITo tarball prerequisite: exit 0, licensed-file guidance reached; `logs/prior-setup-no-data-with-prerequisite.log`, prior report line 82. Package installation is allowed by corrected criterion. No ISO download operation exists in reviewed script. Setup not rerun. |
| R5 Release prerequisites | PASS | CI run [45519392033](https://github.com/cclin99/soneflow-rs/actions/runs/45519392033) is success for exact snapshot, all eight jobs success (`logs/ci.txt`). Live GitHub commits API confirms main consists only of parent then tested commit, with root parent empty; all authors/committers noreply (`logs/remote-history.json`). Three manifests, three lockfile package entries and CITATION all 0.1.0 (`versions.json`). |

## Evidence boundary and procedural exception

No licensed official program, workbook, standard text or data file was opened; no prohibited material-reading test or private checker was executed. No verification skill was loaded. `tools/golden.sha256` was deliberately excluded from reading and scanning. The binary's adjacent provenance declares an owned synthetic frozen oracle; provenance declaration and unchanged Git identity do not replace user material inspection. Keyword scanning cannot establish complete absence of rewritten official material, renamed reference tables or embedded binary content. These remain user-only checks.

During the initial generic `git diff` inspection, the tool unexpectedly displayed deleted historical README reference-output text and deleted commentary about official code. Reading that location stopped; the removed content was not used as numerical evidence and is not reproduced in this delivery. New-side-only excerpts are delivered instead. This run therefore does not claim zero exposure to historical ISO-related text. The incident does not change the user's R1–R5 conditions.

Changed paragraphs in all ten files were inspected: three low-frequency comments now describe this crate/MoSQITo behavior; the crate README uses a generic WAV example and no expected reference output; two README precision cells corrected; FFI field comments translated; SOP shell comments translated; checklist wording/anchors corrected; CI comment corrected. Full historical diff is deliberately absent.

## Tests and warnings

Exact-snapshot CI verifies Windows/Linux data-free Rust integration suites, doctests, FFI test-panic, C smoke, Python smoke/stream, tools, documentation, formatting, Clippy and architecture coverage. This is observed CI evidence, not a claim that every CI platform was rerun locally. Windows CI uses dump-only hashes. Local Windows additionally runs `hash_gate` with `HASH_GATE_DUMP_ONLY=0` and `REQUIRE_AVX2=1`: PASS (`hash-enforced-command.json`, `logs/hash-enforced.txt`). Local suite results are in `test-summary.json`; one pre-existing ignored diagnostic and six filtered csv_ tests are not passes.

cbindgen emitted `Skip soneflow-ffi::_ - (not pub)`; generated public header is byte-identical, so this warning does not indicate a missing public declaration. SSH `git ls-remote` failed due to the environment's CreateFileMapping/access error (exit 128); authenticated read-only GitHub API supplied current remote history instead. No CI rerun, artifact upload or repository mutation was requested.

## Pending user execution

Run the private material checker yourself and inspect formats beyond its coverage, including rewritten code, renamed reference columns and binaries. The agent did not read or execute that checker. No ISO conformance or licensed-material acceptance is claimed here. This pending user action is expressly excluded from FAIL by the brief.

## Delivery and parking lot

Only this new evidence directory was added under the repository. Product code, tests and original documentation remain unchanged; no commit, push or tag. Prior evidence was referenced, never overwritten. `prior-evidence.json` records provenance of reused setup and synthetic precision evidence.

Path redaction write pass and check pass are recorded in `redaction.json`; skipped/non-UTF-8 formats and warnings were inspected. Supplemental case-insensitive checks cover the known private-repo-name and lowercase-user-path gaps. `integrity.sha256` hashes final delivered bytes, excludes itself, and uses two spaces before each relative path. Input/source hashes retain their original meaning. `commands.jsonl`, the audit/verification scripts and `commands.md` describe execution.

Parking lot: the redaction tool's already-known path-pattern gaps remain unfixed. No new implementation task was opened.
