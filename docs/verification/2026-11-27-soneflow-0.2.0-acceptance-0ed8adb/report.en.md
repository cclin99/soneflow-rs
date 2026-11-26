# soneflow 0.2.0 independent acceptance

**PASS: B1–B5 and R1–R3.** Delivery date: 2026-11-27, Asia/Taipei. Verifier: Astra. Under the brief, checks requiring ISO material are delegated to the user and are not failures. This result does not claim those checks ran or establish ISO conformity.

Candidate `0ed8adbb256ad29ba43781f57bd5ac370e549121`; baseline `v0.1.0` peeled to `15a2c076d3b4002b93e5380159a028fd056bdbb6`. The five completion conditions remain unchanged: bitwise output, existing permitted tests, performance ceiling, function/CRAP limits, and D1 API narrowing.

| Gate | Result | Current evidence |
|---|---|---|
| B1 | PASS | Default hash_gate, internals simd_parity, and 47 Python tests against the candidate-built wheel; HASH_GATE_DUMP_ONLY=0, REQUIRE_AVX2=1. Rust frozen-hash file changes only an import; three Python contract files unchanged. Linux CI enforces hashes. |
| B2 | PASS | 145 library tests for each feature set, 32 product + 43 internal integration tests, 20 doctests per feature set, CLI 6, FFI test-panic 23, Python 47. fmt, four clippy configurations, rustdoc, tool tests and MSVC C smoke pass. All 17 data-free integration files are listed in CI. Two existing ignored tests remain ignored. |
| B3 | PASS | Four median-of-three-median comparisons below +3%, table below. Individual estimates retained. |
| B4 | PASS | 614 product functions: max CC 14, max ploc 78, no violations. cargo-crap 0.4.3 and arch gate max CRAP 20.0. Coverage explicitly omits annexb_validator. |
| B5 | PASS | 266 API records match; default eight compile-fail tests pass. Removed canonical paths belong to the seven modules documented in Changed (breaking). Bindings use root paths. |
| R1 | PASS | All three crate versions, lockfile and CITATION are 0.2.0; cbindgen 0.29.4 output matches header bytes exactly. |
| R2 | PASS | All ten comparable timing rows within ±20%; 3-second numerical differences agree to README precision; table check PASS. Golden commands remain user-only. |
| R3 | PASS | doc_pairs, contract_tests and source-doc redaction pass; ten contract rows inspected. Full committed-doc check confirmed by same-SHA CI tools job; local selective snapshot omits historical verification folders. Final local docs/evidence check clean. |

CI [run 45851619042](https://github.com/cclin99/soneflow-rs/actions/runs/45851619042), workflow_dispatch: eight jobs successful on the exact candidate SHA. Only metadata and step states were fetched, not prohibited test output.

## Performance

Ryzen 5 3600, six cores / twelve logical CPUs; Windows 10 build 19045; rustc 1.99.1 / LLVM 23.1.1; Python 3.11.9, NumPy 2.4.6, MoSQITo 1.2.1. Separate clean source/build trees. Foreground serial benchmarks after all other checks/builds, RAYON_NUM_THREADS=1, order baseline1/candidate1/baseline2/candidate2/baseline3/candidate3. Values are medians of each snapshot's three Criterion medians, not Criterion's previous-run change estimate. Automatic sample-time extensions were accepted without changing parameters.

| Benchmark | v0.1.0 ms | 0.2.0 ms | Change | Gate |
|---|---:|---:|---:|---|
| zwtv_10s_t1/scalar/480000 | 192.963 | 191.739 | -0.634% | PASS |
| zwtv_10s_t1/avx2/480000 | 100.463 | 100.324 | -0.138% | PASS |
| stream_push_10s_480chunk/scalar | 183.748 | 183.920 | +0.094% | PASS |
| stream_push_10s_480chunk/avx2 | 83.894 | 84.661 | +0.914% | PASS |

README 12-thread batch: scalar 43.298 ms / AVX2 30.078 ms; filter bank 13.521 / 5.528 ms. Thirty Python runs after one warm-up: binding mean 8.584 ms, MoSQITo 7.134723 s, ratio 831.18x. Maximum differences: N `5.806071e-03`, specific `3.245252e-02`, matching README `5.8e-3` / `3.2e-2` precision. Five mosqito_tail iterations and table checks also completed; see their ready logs. Timing differences from README range from -7.20% to +0.190%, all within ±20% (the brief would record an out-of-range timing without treating it as failure).

## Evidence and boundaries

Snapshots were selected from enumerated tracked paths, excluding data/, private/ and historical docs/verification/. No blind checkout or licensed-material copy. Product/test/existing documentation content remains unchanged; see snapshot-immutability.json. No commits, pushes or tags. All numerical/test/performance evidence is fresh. No implementation or decision probes were needed.

commands.jsonl/commands.md, full logs, support runners, lcov.info, crap.json, fnmetrics.json, API/CI/version/contract inventories and final integrity hashes are included. Raw cargo-crap includes a test-only helper at 42; existing arch exclusions remove test code, leaving gate maximum 20.0. No thresholds were weakened. The candidate wheel was installed into a clean venv; its hash describes the original binary, while delivery hashes describe final redacted text.

Initial sandbox build stalled; only identified acceptance processes were stopped and the same checks succeeded outside the restrictive sandbox. Initial MoSQITo commands failed because matplotlib was missing; installing the repository lock's matplotlib 3.11.0 resolved the import, and original failure logs are retained. The initial docs redaction check found newly generated unredacted acceptance logs; final redaction/check passes resolve that preparation-stage failure. delivery-check.json records UTF-8/skipped/binary/ZIP/path review.

## User-only checks

pending-user-commands.md provides exact golden, Annex, stream_golden, Python parity and private ISO checker commands for the fixed snapshot. The agent did not execute them or obtain their results/statistics/derived outputs. PASS does not replace the user's licensed-material or release checks.

## Parking lot

- The cited MXCSR test compares control bits using `& !0x3f`, excluding six exception-status bits. DenormalGuard Drop restores the full saved register; no violation was observed. This narrower test coverage is noted without adding a gate or changing code/tests.
- A clean MoSQITo pip install needed the indirect matplotlib dependency, supplied in the isolated venv. Documentation changes are a separate user decision.
