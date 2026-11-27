# Changelog

All notable changes to this project will be documented here. The project follows
[Semantic Versioning](https://semver.org/) for public API and ABI changes.

This repository restarts the version series at 0.1.0. Earlier development
happened in a private repository and is not published.

## Unreleased

Maintenance after 0.2.0. Output and the public API do not change.

### Removed (`internals` only)

- `dsp::sos::onepole`, `tables::N_CRITICAL_BANDS` and `tables::N_BARK_STEPS`
  had no callers. They were reachable only with the `internals` feature, which
  semver does not cover.

### Tests

- `push_and_flush_restore_mxcsr` now compares the whole MXCSR, including the
  six exception-status flags, instead of the control bits only.

### CI and documentation

- CI checks the per-function limits on product code (`tools/fnmetrics.py
  --check`: CC at most 15, at most 80 lines of code).
- `docs/GOLDEN-REGEN-SOP` lists `stream_golden` among the local golden tests
  and, like the README, notes that MoSQITo 1.2.1 needs matplotlib although it
  does not declare it. `CONTRIBUTING.md` lists the known limitations of
  `cargo crap` and `tools/public_api.py`.

## 0.2.0 - 2026-11-27

Output is bit for bit that of 0.1.0 (frozen hashes, Python contract hashes and
scalar/AVX2 parity unchanged), and the measured batch and stream timings are
within 3% of 0.1.0.

### Changed (breaking)

- The public Rust API of `soneflow` is now what the crate root exports, plus
  `soneflow::simd`. The modules `core`, `dsp`, `error`, `tables`,
  `tables_noct`, `zwst` and `zwtv` are no longer public. Everything the
  bindings and the documented workflows use was already re-exported at the
  root and keeps working unchanged (`loudness_zwst`, `loudness_zwtv`,
  `ZwtvProcessor`, `ZwtvStream`, `NlInit`, the error and frame types, ...).
  Code that named an item through its module path, such as
  `soneflow::zwtv::NlInit` or `soneflow::error::SoneflowError`, uses the root
  path instead. The C ABI and the Python module do not change.
- Single pipeline stages (third-octave levels, nonlinear decay, temporal
  weighting, slopes, main loudness, filters and tables), `ParMode` and the
  checked slope functions (`try_calc_slopes*`, `SlopeInputError`) moved to
  `soneflow::internals`, behind the new `internals` feature. It exists for the
  crate's own tests and benches and for research code; it is not covered by
  semver and may change in any release.
- `dsp::filtfilt::decimate(x, q)` became
  `internals::dsp::filtfilt::decimate_with_sos(sections, x, q)`: the filter
  for `q` is chosen by the caller (`internals::tables_noct::CHEBY_SOS`)
  rather than looked up inside the filter module. Results are unchanged.

### Changed

- Internal restructuring for maintainability: the stream push path, the
  batch stages (one file per stage, one control flow for the scalar and AVX2
  nonlinear decay) and the stream filter bank's fast blocks were split or
  merged, and test-only code paths go through hook traits instead of
  `cfg(test)` parameters (except in the batch third-octave stage). No product
  function exceeds cyclomatic complexity 15 or 80 code lines.
- CI can be started by hand on any branch (`workflow_dispatch`).

### Added

- `soneflow/public-api.txt`, the committed list of public items, checked in
  CI by `tools/public_api.py`.
- Documentation for `soneflow::simd` and for the conditions under which the
  legacy `ZwtvStream::push`, `flush` and `reset` panic (`# Panics`).
- Runnable examples (doctests) for `loudness_zwst`, `loudness_zwtv`,
  `zwtv_out_frames`, `ZwtvProcessor`, the `ZwtvStream` lifecycle (push, flush,
  reset, quality records, diagnostics), `sone2phon`, `SoneflowError` and
  `StreamUsageError`; the two crate-level examples now run instead of only
  compiling.
- `docs/BEHAVIOUR-CONTRACTS.md` (and the Chinese version): each promise of the
  library with the tests that guard it, checked in CI by
  `tools/contract_tests.py`.

## 0.1.0 - 2026-11-25

First public release.

### Added

- ISO 532-1:2017 stationary loudness (`loudness_zwst`) and time-varying
  loudness (`loudness_zwtv`, `ZwtvProcessor`): total loudness in sone,
  specific loudness in sone/Bark on 240 Bark steps, free and diffuse field.
- Stateful 48 kHz stream (`ZwtvStream`) on the same 2 ms output grid as the
  batch path, with phon conversion (`sone2phon`), frame index and status
  flags. The Rust hot path does not allocate after construction and does not
  start Rayon.
- Output-grid contract: `ceil(ceil(L/24)/4)` points for `L` samples, point `j`
  at `96j/48000` s, each point determined by the samples up to its time
  (`zwtv_out_frames`).
- AVX2+FMA kernels selected at run time on x86-64, with a scalar fallback;
  scalar and AVX2 results are checked for parity.
- C ABI v1 (`soneflow-ffi`, header `soneflow-ffi/include/soneflow.h`) and a Python
  binding (`soneflow-py`, NumPy arrays, GIL released during batch work).
- Input checks: 48 kHz only, levels above 120 dB in the third-octave bands
  below 300 Hz are rejected in batch and clamped (flagged) in the stream; a
  non-finite sample is an error in batch (`NonFiniteInput`, C ABI code 4) and
  is zeroed and flagged in the stream.
- WAV CLI example with an explicit linear calibration factor to pascals.

### Behaviour that differs from MoSQITo 1.2.1

MoSQITo 1.2.1 is the compatibility baseline (goldens and a direct parity suite
run locally). This crate follows the standard where the two differ:

- The low-frequency DLL correction reaches its last row above the seventh
  threshold (MoSQITo stops one row early).
- The batch nonlinear temporal decay starts from zero state, so a result for a
  signal prefix does not depend on later samples. `NlInit::MosqitoTail`
  reproduces MoSQITo's seed for comparisons only.
- The slope stage follows ISO 532-1 Annex A: no per-frame rounding of the
  total, every band completed, and the core loudness written in the cell where
  a slope ends. Total loudness agrees with MoSQITo within 5e-3 sone on the
  compatibility fixtures; specific loudness differs by up to about 4e-2
  sone/Bark by design.
- The core loudness constants are held in single precision.

### Verification

- CI runs the tests that need no external data on Linux and Windows,
  including bitwise hash gates per platform, scalar/AVX2 parity, streaming
  allocation checks and the C ABI and Python smoke tests.
- MoSQITo golden tests and the ISO 532-1 Annex B tests run locally. Annex B
  files are supplied from your own licensed copy of the standard (see
  `README.md`, "Verification scope").
