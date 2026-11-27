# soneflow 0.2.1 independent pre-tag acceptance

Date: 2026-11-28, Asia/Taipei. Candidate: `ddbe9e2cc4676c2b8fdaf854725ca7107d267540`; baseline: `82d1d61` (full SHA in `baseline-head.txt`). Independent `gpt-6-astra` agents performed the numerical, test/tooling, and performance reviews as required by the brief. The coordinating agent reconciled and checked the delivery.

**Verdict: PASS (A5 remains user-only and is excluded from failure determination).** A5 is user-only and does not count as a failure. This report evaluates the brief for these frozen snapshots; it is neither an ISO conformity claim nor release authorization. No commit, push, tag, or product fix was made. Mutation and observation changes were confined to isolated snapshots and restored.

## Acceptance matrix

| Item | Result | Independently collected evidence |
|---|---|---|
| A1 RN bitwise stability | PASS | Default-feature local hash_gate debug/release asserted with dump=0; SIMD parity passed; 47 Python contract tests passed against the candidate-built wheel; frozen-value/API diff empty. The independent corpus matched across versions in 1068 rows per debug/release. See `gates/02-product-debug.log`, `gates/03-hash-release.log`, `gates/04-internals-debug.log`, `gates/25-python-contract.log`, `numerical/comparison.json`, `frozen-contract-diff.txt`. |
| A2 ordering test and CI | PASS | Unconditionally taking the cache failed with exit 101 on band 0 in the second call, under RN, assertion at test line 136. Restored debug/release passed. CI test and arch coverage lists both include the test. See `numerical/report.en.md`, `numerical/mutation-debug.log`, `numerical/restored-debug.log`, `numerical/restored-release.log`. |
| A3 regression/architecture/API/CI | PASS | All permitted local CI commands passed; public API check: 266 lines. Function limits, architecture and CRAP passed, maximum CRAP 20.0. The specified online run has the exact SHA and 8 successful jobs. See `gates/all-command-index.json`, `gates/method.md`, `gates/31-arch.log`, `ci-run.json`, `ci-jobs.json`. |
| A4 four-arm performance | PASS | One complete repeat; all four second-set medians improved, with a maximum range/median of 3.13%. See below and `performance/decision.json`. |
| A5 local ISO tests/material check | USER ACTION | No source data directory access, prohibited tests, or material checker execution. Commands are in `pending-user.md`; excluded from failure determination. |
| N1 non-RN behavior | PASS | Candidate equals the independently written runtime reference in all three modes, both fields and all valid spectra. Debug equals baseline; release differences are confined to bands 0–2, at most 5 ULP. Batch/quality reject; legacy push/flush run. See `numerical/comparison.json`, `numerical/five-ulp-cases.json`. |
| N2 no compile-time folding | PASS | Candidate initializer: 20 pow relocations; candidate main: 14 pow/1 stmxcsr; baseline main: 11/0. Actual release rlibs were disassembled with `llvm-objdump -d -r`. See `performance/objdump-summary.json`, extracted function bodies and full disassemblies. |
| N3 FTZ/DAZ and warming | PASS | Four combinations start with an empty process-local cache; outputs match in both profiles. Release observations for all four show before_new=false, after_new=true, after_push=true. See `numerical/report.en.md`, `numerical/cache-*.log`. |
| R1 versions/header | PASS | Three crates, lock and CITATION are 0.2.1. cbindgen 0.29.4 with the Git-blob configuration generates a header byte-identical to both HEAD and baseline blobs. See `gates/r1-check.json`, `gates/r1-header-final.json`. |
| R2 CHANGELOG claims | PASS | Every 0.2.1 statement has code, independent numerical, or timing support. Processor speed is inferred from the shared batch path, not a separate processor-only benchmark. See `performance/changelog-audit.md`. |
| R3 documents/contracts | PASS | doc_pairs, contract_tests and original-docs redaction passed; the release-checklist disassembly command was executed. Final delivery also passed both redaction stages. See `gates/16-doc-pairs.log`, `gates/17-contracts.log`, `performance/method.md`, `delivery-audit.json`. |

## Numerical scope

Each field has 430 spectra covering every critical-band threshold, adjacent representable values around the low-band 120 dB boundary, and 257 fixed pseudorandom inputs. The lowest grouped-band thresholds are located by bisection. Both Free and Diffuse are covered. A separately synthesized 9600-sample two-tone signal with an amplitude step exercises batch n, specific loudness, time/bark axes and 137-sample stream chunks, comparing push/flush frame index, n, n_phon and flags across versions. All 1068 rows match in each profile. This is finite-corpus evidence, not a formal proof over all possible inputs. The implementer's diagnostics and measurements were not reused as evidence.

| Non-RN mode | Release bands that differ from baseline, and ULP ranges |
|---|---|
| Downward | 0: 1–5; 2: 1–2 |
| Upward | 0: 1–5; 1: 2–5; 2: 1–2 |
| Toward zero | 0: 1–5; 2: 1–2 |

Debug is identical. The candidate matches the independent runtime reference bit for bit in all modes. Ten 5-ULP cases retain full input and output bits. The maximum exceeds the implementer's narrower 1–3 ULP observation, but CHANGELOG promises no such maximum and correctly confines the change to the lowest three bands' final bits. No frozen assertion was weakened.

## Performance

Second set (selected under the preregistered rule), in ms; base = 82d1d61, new = ddbe9e2.

| Arm | Version | Three medians | Median of medians | Min–max | Full range |
|---|---|---|---|---|---|
| zwtv scalar | base | 189.087, 190.422, 188.371 | 189.087 | 188.371–190.422 | 2.051 (1.08%) |
| zwtv scalar | new | 180.716, 181.161, 177.746 | 180.716 | 177.746–181.161 | 3.415 (1.89%) |
| zwtv avx2 | base | 98.980, 98.603, 95.889 | 98.603 | 95.889–98.980 | 3.091 (3.13%) |
| zwtv avx2 | new | 89.074, 89.171, 86.774 | 89.074 | 86.774–89.171 | 2.397 (2.69%) |
| stream scalar | base | 181.028, 181.266, 182.852 | 181.266 | 181.028–182.852 | 1.824 (1.01%) |
| stream scalar | new | 171.660, 171.417, 172.523 | 171.660 | 171.417–172.523 | 1.106 (0.64%) |
| stream avx2 | base | 82.963, 82.894, 84.004 | 82.963 | 82.894–84.004 | 1.110 (1.34%) |
| stream avx2 | new | 73.218, 73.076, 74.386 | 73.218 | 73.076–74.386 | 1.309 (1.79%) |

Changes: zwtv scalar -4.427% / zwtv avx2 -9.663% / stream scalar -5.300% / stream avx2 -11.746%. Every median decreased; all four no-regression conditions and both required AVX2 improvements hold.

In the first set, new zwtv avx2 had a 6.49% range/median and base stream avx2 6.90%, triggering the predefined complete repeat. The second-set maximum is 3.13%, below the 5% threshold. All 12 runs and 48 estimates payloads are retained. First-set three-run values and ranges are in performance/batch1-summary.json; second-set values are in batch2-summary.json. The second set was selected by rule, not by choosing the more favorable set.

Both versions use the same AMD Ryzen 5 3600, Windows x86_64, Rust/Cargo 1.99.1, default features and separate targets. All compilation and tests stop before foreground benchmarking with `RAYON_NUM_THREADS=1`, in base1/new1/base2/new2/base3/new3 order. Each value is Criterion's `new/estimates.json` median.point_estimate; the comparison is the median of three such medians, not Criterion's change against cached previous runs.

No regression means candidate minus baseline median does not exceed the larger same-version three-run full range. Both AVX2 arms must also have a lower median. Before measurement, any arm/version with full-range/median above 5% was defined as obvious noise, triggering one complete additional interleaved six-run set, retaining the first set. No favorable individual round is selected. Execution is unattended; the user was not asked to remain idle. User activity cannot be excluded, and the measured ranges and repeat status document that limitation.

## Tests, tools and exceptions

Only explicitly permitted CI tests ran. No annexb_validator (even with CSV skipped), golden suite, stream_golden, ignored/include-ignored execution, Python MoSQITo parity, or private material checker ran. Such commands in `pending-user.md` are instructions for the user only. Build outputs, caches, virtual environments and temporary files reside under the designated audit root.

GitHub run [46079306401](https://github.com/cclin99/soneflow-rs/actions/runs/46079306401) was independently queried through the API: exact head SHA, workflow_dispatch, and 8 successful jobs. The tested CI configuration asserts hashes with dump=0 on Linux. Online CI includes a validator step prohibited in this local acceptance; its run conclusion is recorded without locally repeating that step.

The CRAP scope warning is retained: 64 analyzed files, 55 LCOV files, 54 matched. All 21 null-coverage entries belong to 10 cfg(test) files; no product entry has null coverage. The sole unmatched LCOV file, tables.rs, has no product function. Fifteen numeric coverage=0 entries remain; this is not a 100%-coverage claim. See `gates/crap-scope-audit.json`. The original CRAP gate passes.

Local core.autocrlf=true produces CRLF working files, and cbindgen reading the CRLF configuration emits mixed endings. Two initial raw-byte comparison failures are retained. Regenerating with the LF configuration obtained from the HEAD Git blob produces an exact HEAD/baseline header match, without editing original files. C smoke initially failed to launch by relative command; an explicit executable path passed, with both logs retained. Environmental Git clone/bundled-tar failures were handled using git archive and native Windows tar. No product edits bypassed a failure. CPU information used a read-only registry fallback after Get-CimInstance access was denied.

**Disclosed restriction incident:** an initial coordinator read of the generic ISO verification skill inadvertently exposed an old restricted-derived hash example embedded in its prose. It was not quoted, copied into the delivery, compared, or used in a probe, and none of that skill's prohibited commands ran. Delegates were told not to read that skill. The original data directory was never accessed. See `orchestration.md`; this run must not be described as having had no exposure whatsoever to restricted-derived information.

## Delivery, redaction and integrity

This file and `report.md` are the main reports. Subdirectories retain numerical probes/raw bits, test/coverage/tool logs, and timing estimates/disassemblies/CHANGELOG review. `gates/all-command-index.json` preserves initial failures and retries rather than showing only successful commands.

`<AUDIT_ROOT>` denotes the designated audit root; `<SOURCE_REPO>` denotes the user's original material-bearing repository. Substitute local paths to reproduce commands. The existing redact_paths.py does not cover the new audit-root name, so that prefix is normalized additionally before the standard write and `--check --strict` passes. Every format is separately inspected; see `delivery-audit.json`. `integrity.json` covers post-redaction delivery files, excluding itself, with SHA-256 and byte counts. Original rlib/archive/source hashes retain their original-input meaning. Delivery content and tracked diff were reviewed; product, test and pre-existing documentation files are unchanged.

## User-only actions

A5: run the ISO tests and `iso_material_check.py --check` in user-prepared licensed environments for both fixed versions, then compare results. Required local Python parity/private material tests also remain user-only. Exact commands are in [pending-user.md](pending-user.md). No response was awaited and no restricted action was performed on the user's behalf.

## Parking lot

- Retain the broader non-RN maximum of 5 ULP for future descriptions; do not change the accepted non-RN contract in this round.
- The generic verification skill still contains an old restricted-derived hash example and merits a separate cleanup decision; it was not modified here.
- CRAP scope warnings and Windows newline/launch differences are documented; no tool or product changes are made here.
- Whether to remeasure README's 0.2.0 Measured results, non-x86_64 acceleration and other previously deferred work remain user decisions; no additional round is opened.
