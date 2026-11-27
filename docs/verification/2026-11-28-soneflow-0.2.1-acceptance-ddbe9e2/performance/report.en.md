# Independent performance and disassembly acceptance

A4 PASS; N2 PASS. The timing/disassembly claims in R2 are supported. Candidate ddbe9e2 versus baseline 82d1d61, same host/toolchain, default features, release, RAYON_NUM_THREADS=1. See changelog-audit.md for every 0.2.1 sentence and the other independent reports for numerical/test evidence.

The predeclared noise rule (method.md) triggered one complete repeat: first-batch candidate zwtv AVX2 range/median was 6.49%, baseline stream AVX2 6.90%, both above 5%. All first-batch evidence remains. The second batch's largest ratio is 3.13%. Foreground measurements started after other agents stopped builds/tests; possible user activity cannot be excluded.

Second-batch results in milliseconds; each version has three interleaved rounds. Values are Criterion median.point_estimate, followed by their median and full range. The complete second batch is selected by the predeclared repeat rule, not by choosing favorable individual rounds.

| Arm | Version | Three round medians | Median of medians | Min–max | Full range (ratio) |
|---|---|---|---|---|---|
| zwtv_10s_t1/scalar/480000 | base | 189.087044, 190.422331, 188.371115 | 189.087044 | 188.371115–190.422331 | 2.051216 (1.085%) |
| zwtv_10s_t1/scalar/480000 | new | 180.716151, 181.160632, 177.746034 | 180.716151 | 177.746034–181.160632 | 3.414598 (1.889%) |
| zwtv_10s_t1/avx2/480000 | base | 98.979758, 98.602643, 95.888691 | 98.602643 | 95.888691–98.979758 | 3.091067 (3.135%) |
| zwtv_10s_t1/avx2/480000 | new | 89.074183, 89.171227, 86.774349 | 89.074183 | 86.774349–89.171227 | 2.396878 (2.691%) |
| stream_push_10s_480chunk/scalar | base | 181.028280, 181.266494, 182.851781 | 181.266494 | 181.028280–182.851781 | 1.823501 (1.006%) |
| stream_push_10s_480chunk/scalar | new | 171.659839, 171.417493, 172.523454 | 171.659839 | 171.417493–172.523454 | 1.105961 (0.644%) |
| stream_push_10s_480chunk/avx2 | base | 82.963193, 82.893734, 84.003706 | 82.963193 | 82.893734–84.003706 | 1.109972 (1.338%) |
| stream_push_10s_480chunk/avx2 | new | 73.218027, 73.076293, 74.385671 | 73.218027 | 73.076293–74.385671 | 1.309378 (1.788%) |

| Arm | Candidate relative change | No regression |
|---|---|---|
| zwtv_10s_t1/scalar/480000 | -4.427% | PASS |
| zwtv_10s_t1/avx2/480000 | -9.663% | PASS |
| stream_push_10s_480chunk/scalar | -5.300% | PASS |
| stream_push_10s_480chunk/avx2 | -11.746% | PASS |

All four medians improve, by more than the larger same-version full-range noise bound; both required AVX2 improvements hold. decision.json and batch2-summary.json contain machine-readable decisions and original-estimate links. batch1-summary.json retains the first batch. All 12 benchmark logs and 48 raw estimates are retained.

N2: baseline main_loudness_unchecked has 11 pow relocations / 0 stmxcsr; candidate has 14 / 1; candidate init_mp1_table_rn has 20 pow relocations. The actual llvm-objdump -d -r release-rlib command reproduces the release checklist. objdump-summary.json records complete-dump start lines 25845 / 23966 / 38800 respectively; the three extracted function bodies start at line 1. Counts exclude call-site mentions and OnceLock wrappers. binary-inputs.json preserves original binary filenames and SHA256; source-integrity.json binds both full commit IDs, archive hashes, and 83 matching product-source/bench/manifest files per version. Original binary/source hashes are distinct from hashes of redacted delivery text.

No ISO material or prohibited tests were processed; no product changes, commit, or push. Benchmarks are complete. The overall verdict remains with the combined acceptance report and user-only A5 items.
