# Orchestration and evidence boundary

Candidate: `ddbe9e2cc4676c2b8fdaf854725ca7107d267540`. Baseline: `82d1d61`.
Delivery date: 2026-11-28, Asia/Taipei.

The numerical, test/tooling, and performance/release reviews were delegated to separate `gpt-6-astra` agents as explicitly required by the acceptance brief. The coordinating agent independently queried GitHub and will reconcile evidence, redact the delivery, and verify its integrity. No commit, push, tag, or product fix is authorized.

Only synthetic inputs and permitted repository tests are eligible. The source repository's `data` directory is forbidden. The brief's forbidden test list, including the validator with CSV tests skipped, takes precedence over CI and generic skills. No ISO material checker is executed. User-only tasks must be listed without waiting for a response.

An initial read of the generic `iso532-rust-verification` skill inadvertently exposed an old restricted-derived hash example in its prose. It was not used, copied into this delivery, compared, or used to design any probe. That skill's prohibited commands were not run. This is a disclosed process incident; it is not evidence of a product failure. The delegated agents were explicitly told not to read that skill. No source `data` directory was accessed.

Temporary files, Cargo home/cache, targets, virtual environments, isolated source snapshots, and reports are under the designated audit root. Existing tools outside it are read/executed only. Formal performance measurement starts only after other verification jobs stop. User activity on the machine cannot be excluded. A greater-than-5-percent full-range/median in any version/arm triggers one complete repeat of the interleaved three-round measurements, retaining the first set.

## Coordinator commands

```text
git rev-parse HEAD
git status --short
git remote -v
git diff --name-status 82d1d61 ddbe9e2
git diff 82d1d61 ddbe9e2 -- soneflow/tests/hash_gate.rs soneflow/tests/py_contract_dump.rs soneflow-py/tests/test_smoke.py soneflow-py/tests/test_stream.py soneflow-py/tests/test_quality_v1.py soneflow/public-api.txt
gh api repos/cclin99/soneflow-rs/actions/runs/46079306401
gh api repos/cclin99/soneflow-rs/actions/runs/46079306401/jobs --paginate
rustc -Vv
cargo -V
python --version
gh --version
```

The browser retrieval of the CI URL returned a cache miss. GitHub CLI API retrieval succeeded; `ci-run.json` and `ci-jobs.json` are the authoritative retrieved evidence. CI conclusions are not substituted for local execution. CI itself includes a validator step that this local run is explicitly prohibited from executing.
