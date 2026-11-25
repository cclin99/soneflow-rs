# Changelog

All notable changes to this project will be documented here. The project follows
[Semantic Versioning](https://semver.org/) for public API and ABI changes.

This repository restarts the version series at 0.1.0. Earlier development
happened in a private repository and is not published.

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
