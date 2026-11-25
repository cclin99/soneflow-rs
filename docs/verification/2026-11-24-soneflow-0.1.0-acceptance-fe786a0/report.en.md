# soneflow 0.1.0 independent acceptance: FAIL

Auditor: Astra. Date: 2026-11-24 (Asia/Taipei). Frozen commit `fe786a0e40cff532968204da70c7187ae2d5bc99`, tree `e02b3e42b1658c0500c2fdc329b27a1b7a6c134d`. This is a fresh run under the new soneflow brief, restricted to non-ISO-material checks.

Procedural exception: while reading the existing verification skill as instructed, the tool unexpectedly displayed an old ISO-derived hash example. It was not used, compared, calculated with, or included in this delivery, and its prohibited commands were not executed. Its contents are not reproduced. This run therefore does not claim zero exposure to all ISO-derived text. No official material or forbidden data file was opened, and the private ISO checker was neither read nor executed. The exception is disclosed without changing A1–A5.

## A1–A5 verdicts

| Gate | Verdict | Evidence |
|---|---|---|
| A1 Public-tree cleanup | **FAIL** | Two retained items listed below; complete ISO-material inspection remains user-only. |
| A2 Numerical equivalence and allowed tests | **PASS** | All 66 Rust source files match permitted change categories; all allowed required tests passed with enforced Windows hashes. |
| A3 README reproduction | **FAIL: accuracy presentation** | One synthetic maximum does not round to the stated printed value. This is not a computational regression. |
| A4 No-data setup | **FAIL: literal zero-download clause** | Missing licensed-input guidance works with documented third-party prerequisites; network-enabled pip installation does not implement a zero-download requirement. This run used cached packages, so package-byte downloads were not demonstrated. |
| A5 Release prerequisites | **PASS** | Root-only GitHub main, noreply identities, eight successful CI jobs, all versions 0.1.0 and regenerated header consistency. |

A1 alone prevents acceptance. User-only checks are not failures. No source/test edits, tracked original-worktree edits, commits, pushes or tags were made.

## Findings and boundaries

A1: `soneflow/src/core/main_loudness/low_freq.rs:68` retains commentary on the official Annex A program's control flow. `soneflow/README.md:45` retains an expected numerical output associated with a licensed Annex B input. Neither the matched text nor the value is reproduced, analyzed or compared here, and no official source was followed.

The permitted public text scan found old-name matches only in the two exempt redaction tools and no private email or non-exempt workstation path. `tools/golden.sha256` was excluded from reading/scanning for user-only review. The tracked binary has adjacent provenance declaring an owned synthetic fixture; this does not replace the user's independent material-provenance inspection.

## A2 evidence

| Verification | Result |
|---|---|
| Rust library | 141 passed |
| CI data-free integration targets | 61 passed; 1 existing ignored diagnostic |
| annexb_validator synthetic portion | 18 passed; 6 csv_ tests deliberately filtered |
| Doctests | 2 passed |
| FFI including test-panic and quality_v1 | 23 passed |
| Python smoke + stream, newly built wheel | 42 passed |
| Repository tools | 43 passed |
| fmt / clippy / rustdoc / documentation pairs | PASS |
| Approved private synthetic statistics test | 1 passed; fixed dependency versions |

Windows `HASH_GATE_DUMP_ONLY=0` and `REQUIRE_AVX2=1` were enforced. The one ignored diagnostic is the existing `py_contract_dump`, not the hash gate. After lexical comment/whitespace removal, name mapping and import-order normalization, 64 of 66 Rust src files match; the remaining two differ only in the explicitly allowed assertion messages inside `#[cfg(test)]`. No old history or sensitive diff is delivered.

The Python wheel was rebuilt from the frozen source and tested after installation in the isolated virtual environment. Only 20 individually named synthetic input files and the single approved private harness were copied. The authoritative harness run used the frozen lockfile's shared dependency versions. A preliminary unpinned dependency build was deliberately interrupted before test execution; it is retained as INTERRUPTED, not a numerical failure or acceptance result.

## A3 precision and performance

| Synthetic quantity | README | Reproduced | Printed-digit check |
|---|---:|---:|---|
| ZWST max absolute N | 4.7e-4 | 4.738e-4 | PASS |
| ZWST max relative N | 6.2e-5 | 6.156e-5 | PASS |
| ZWST max specific N | 1.5e-4 | 1.501e-4 | PASS |
| ZWTV tail, non-step max N | 1.8e-4 | 1.748e-4 | FAIL: conventional two-significant-digit rounding is 1.7e-4 |
| ZWTV tail, step max N | 3.9e-3 | 3.941e-3 | PASS |
| ZWTV tail, max specific N | 2.2e-2 | 2.209e-2 | PASS |
| ZWTV zero, step max N | 5.6e-2 | 5.575e-2 | PASS |

Treating `1.8e-4` as a conservative upper bound would contain the measurement, but the brief requires agreement at printed precision and does not substitute that interpretation for conventional rounding. The numerical equivalence verdict A2 remains PASS. Python means were 8.070 ms (README 8.10 ms) and 6.584678 s (README 6.587 s), both within 20%. Standard deviations were 1.151 ms and 0.036543 s, above the README 0.39 ms and 0.025 s; this variability is retained and does not fail the gate under the brief. Python default-zero results:

```text
soneflow Rust binding: mean=0.008070 s, sample_sd=0.001151 s, min=0.007364 s, max=0.014038 s, real_time=371.7385x
MoSQITo: mean=6.584678 s, sample_sd=0.036543 s, min=6.524867 s, max=6.698754 s, real_time=0.4556x
relative speed from arithmetic means: 815.93x
max abs N(t) difference: 5.806071e-03 sone
max abs N_specific(t) difference: 3.245252e-02 sone/Bark
```

| Rust benchmark | README ms | Median ms | Difference |
|---|---:|---:|---:|
| `filter_bank_10s_t12/avx2/480000` | 5.57 | 5.582 | +0.22% |
| `filter_bank_10s_t12/scalar/480000` | 14.4 | 14.673 | +1.89% |
| `stream_push_10s_480chunk/avx2` | 83.1 | 82.585 | -0.62% |
| `stream_push_10s_480chunk/scalar` | 184.2 | 180.490 | -2.01% |
| `zwtv_10s_t1/avx2/480000` | 100.1 | 97.868 | -2.23% |
| `zwtv_10s_t1/scalar/480000` | 192 | 187.769 | -2.20% |
| `zwtv_10s_t12/avx2/480000` | 30.1 | 30.052 | -0.16% |
| `zwtv_10s_t12/scalar/480000` | 41.9 | 44.335 | +5.81% |

Host: Windows 10 build 19045, AMD family 23 model 113, 12 logical CPUs; rustc 1.99.1, Python 3.11.9, NumPy 2.4.6, MoSQITo 1.2.1. Benchmarks ran serially after correctness; the coordinator avoided concurrent CPU-intensive work. Rust values are Criterion median point estimates, not the console time estimate. Ten samples per group; outlier/extended-collection notices remain in the logs and median confidence intervals in `benchmark-medians.json`. Python uses arithmetic means over 30 runs, plus the separately requested five tail-initialized runs, each after a warm-up. Per the brief, timing deviations outside 20% would be recorded without failing acceptance.

The MoSQITo table check passed for `ZUP`, `RNS` and `USL`; only shapes/counts/difference counts were output. The five-run tail-initialized comparison also exited 0, with N difference `1.155051e-4` and specific difference `1.855872e-3`. This is a different three-second synthetic signal, not the one-second fixtures in the table.

## A4 and A5 detail

The initial data-free clone lacked the documented local MoSQITo tarball prerequisite and stopped before licensed-input guidance; that prerequisite failure is not independently classified as a product defect. A second fresh, data-free setup staging supplied the ordinary tarball and unchanged setup/requirements files: exit 0 and missing licensed-input guidance. Lines 36–37 and 58 of `tools/setup_env.sh` still perform default-index/build-isolated package installation, so the literal no-download requirement is not implemented. No ISO attachment download operation was found or executed. Forbidden missing-data test targets remain user-only even when no data is present.

Live GitHub [run 45423756240](https://github.com/cclin99/soneflow-rs/actions/runs/45423756240), attempt 1, has eight successful jobs at the frozen SHA. Main has exactly that root commit, empty parents, noreply author and committer. All three crate manifests, Cargo.lock and CITATION.cff use 0.1.0. The coordinator collected the live read-only JSON; this report interprets that fresh evidence.

cbindgen 0.29.4 with the Windows-checkout config differs only in line endings. Regenerating from a temporary LF-normalized copy of the otherwise identical config yields **raw bytes identical to the fixed Git header blob**. No tracked config/header was edited; see `header-check.json`.

## User-only work (not failures)

These commands are **for the user to run personally**, in the corresponding fixed-source/private-tool environment. They were not executed by the AI. Missing-input runs and licensed-input runs both remain user-only.

```text
python private/tools/iso_material_check.py --check
.venv/Scripts/python.exe tools/golden_manifest.py --verify
cargo test -p soneflow --test annexb --test golden_core --test golden_dsp --test golden_zwst --test golden_zwtv -- --nocapture
cargo test -p soneflow --test annexb_validator csv_ -- --nocapture
.venv/Scripts/python.exe -m pytest soneflow-py/tests/test_parity_mosqito.py -v
cargo test --manifest-path private/tests-crate/Cargo.toml --release
```

The user must also inspect `tools/golden.sha256` and any formats the private checker does not cover, including rewritten code, renamed columns, binary fixtures and archives. No official-source/hash-list content or derived Annex B result is included in this delivery. The deferred checks are not counted as failures. The user must interpret their results personally; do not send ISO inputs or derived outputs back to an AI agent for comparison.

## Delivery and parking lot

Commands, logs, finding locations, token-comparison summary, synthetic input identities, dependency versions and live GitHub metadata are included. `integrity.sha256` covers the final redacted delivery bytes only. Original input/source/binary hashes keep their original meaning and do not claim identity with redacted copies.

The required redactor write/check passes and additional path/UTF-8/file-type/diff inspection are recorded in `delivery-check.json`. Skipped files and warnings are explicitly reviewed. The ISO-material checker was not read or executed; it remains user-only.

Parking lot, for user decisions only: whether to revise the stale verification skill containing a prohibited example, and whether to extend the redactor for the suffixed original-repository path and lowercase Users forms. This delivery was manually sanitized; no skill/tool/product changes or additional acceptance round were opened. The work stayed within A1–A5.
