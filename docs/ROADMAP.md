# soneflow project roadmap
> 對照／Pair: [中文版](ROADMAP.zh-TW.md) · source: zh · synced 2026-11-26

Current state (0.2.0): the `soneflow` crate (ISO 532-1:2017 Zwicker loudness, Rust + AVX2) with batch and streaming APIs, phon conversion, a C ABI v1 and a Python binding. 0.2.0 narrowed the public Rust API to the crate root and `soneflow::simd` and restructured the internals without changing any output.

The items below are follow-on directions (recorded 2026-07-03). **None of them is in scope for 0.2.0.** They are listed here because they constrain the current implementation; stating the constraints now avoids rework later.

## 1. Layered architecture and further standards

soneflow is a psychoacoustic engine first. Physical and broadcast meters are optional add-ons on top of it.

```text
Core: psychoacoustic engines (how people hear)
  Zwicker loudness (ISO 532-1:2017) -> N(t) sone, N'(z) sone/Bark, loudness level in phon
    |-- sharpness (acum)            computed from the specific loudness N'
    |-- psychoacoustic annoyance    combines N5, sharpness, roughness, fluctuation strength
    `-- other engines: Moore-Glasberg loudness (ISO 532-2), Sottek hearing model (ECMA-418-2),
        roughness, tonality ...

Add-ons: physical and broadcast meters (what the signal is)
  dBFS, dB SPL (needs calibration), LUFS (ITU-R BS.1770 / EBU R128), true peak
```

- phon is part of the loudness engine output (loudness level, converted from sone), not an add-on.
- Engines are named after the model, not after the standards body: `soneflow` holds the Zwicker engine today; later engines become `soneflow-moore`, `soneflow-sottek`, and so on, and `soneflow` becomes the umbrella crate that re-exports them behind features. Documentation names each standard with its full reference, for example "ISO 532-1:2017".
- Add-on meters live outside the core crates (for example a `soneflow-meters` crate or the panel). LUFS and true peak can reuse the existing `ebur128` crate instead of a new implementation.
- `dsp/` (sosfilt, filtfilt, onepole, SIMD filter bank kernel) stays independent of any one standard so that later engines can reuse it.
- Sharpness can consume `n_specific` from `LoudnessStationary` / `LoudnessTimeVarying` directly; the output already contains the full 240-point specific loudness.

## 2. 64-track VST plug-in (real-time loudness metering)

The streaming API this section asks for shipped in 0.1.0 (`ZwtvStream`); the plug-in itself is still open.

**Requirements on the current implementation (largest impact):**
- **Streaming API**: the current API computes the whole signal as a batch. A real-time VST needs chunk-by-chunk processing with persistent filter state. The kernels already advance sample by sample with explicit state (biquad z0/z1, smoothing, nl uo/u2), so extracting `struct LoudnessStream { push(&mut self, chunk: &[f64]) -> ... }` later is a refactor, not a rewrite. **During implementation, do not bury state in globals outside function-local variables** (the current plan already complies).
- **Performance target**: 64 tracks × 48 kHz in real time = 3.07M samples per second; the zwtv hot spot is roughly 28 bands × 6 IIR per sample. The Phase 5 benchmark results must be evaluated against this target (a single-track 10 s signal must run far faster than 10 s wall-clock; 64 tracks can be parallelised across tracks with rayon, one thread per track is naturally parallel, to be added then).
- Note that the `sosfiltfilt` in zwst (zero phase, needs the full signal) **cannot be streamed**. Real-time scenarios use the zwtv path only, so this is not a problem.
- The VST framework will be evaluated at that time (nih-plug is the primary candidate).

## 3. dB SPL and sone display panel

The phon conversion shipped in 0.1.0 (`sone2phon`); the panel itself is still open.

- The engine output already contains the required data: `third_octave_levels` (dB SPL per band), `N(t)` (sone), `N_specific` (sone/Bark).
- What is missing is the phon conversion (`sone2phone`: N ≥ 1 → 40 + 10·log2(N); mosqito utils has a reference). It is tiny and will be added to the crate at that time.
- The panel itself is VST/GUI work. The engine side only has to guarantee that the output structure can be obtained incrementally (solved together with the streaming API).

## 4. C-ABI and Python bindings

Shipped in 0.1.0 (C ABI v1 and the Python binding).

- **C-ABI**: a separate `soneflow-ffi` crate (`cdylib` + `#[no_mangle] extern "C"`), flat f64 arrays plus length parameters. The current output deliberately uses flat row-major `Vec<f64>` (`n_specific` indexed as `bark_idx * n_frames + frame`) precisely to prepare for this. **Do not change it to nested Vec.**
- **Python**: pyo3 + maturin, returning numpy arrays; verification reuses the golden data directly (run mosqito and the Rust binding side by side on the Python side).
- Error handling: `SoneflowError` is already an enumerable named error; the FFI maps it to error codes.

## 5. Cross-platform: Linux (Ubuntu) and Apple M series

**The current plan already compiles and runs on both platforms** without changes:

- Pure Rust with pure Rust dependencies (thiserror, hound, criterion); no platform-specific system calls.
- The AVX2 kernel and its dispatch call sites are all inside `#[cfg(target_arch = "x86_64")]`; `avx2_available()` returns `false` at compile time on non-x86_64. On aarch64 (Apple M) the whole AVX module is excluded from compilation and the scalar path is taken automatically.
- Golden `.bin` files are f64 little-endian. Both x86-64 and Apple M (aarch64) are LE, so test data is shared across platforms.
- Toolchain scripts (`setup_env.sh`, `gen_golden.py`, `gen_tables.py`) are bash/Python and run natively on Linux/macOS.
- The later C-ABI (cdylib) and VST (nih-plug supports Win/macOS/Linux) hold on all three platforms.

**Per-platform status:**

| Platform | Build / correctness | SIMD acceleration |
|---|---|---|
| Windows / Linux x86-64 | ✅ | AVX2+FMA (runtime detection) |
| Linux aarch64, Apple M series | ✅ (scalar) | NEON kernel pending (see below) |

**Apple M series NEON kernel (future work):**

- NEON is 128-bit: `f64x2`, so 28 bands = 14 vectors (vs 7 for AVX2); FMA uses `vfmaq_f64`, branch elimination uses `vmaxq_f64`/`vminq_f64`/`vbslq_f64`. These map one-to-one onto the AVX2 kernel, so the port is mechanical.
- On aarch64 NEON is a baseline feature. **No runtime detection is needed**; compile it in directly under `#[cfg(target_arch = "aarch64")]`. Dispatch is simpler than on x86.
- Approach: copy the structure of the AVX2 kernel; the `simd_parity` test is reused as is (scalar vs NEON).
- Note: `simd_parity` skips on machines without AVX2. An all-green `cargo test` on an M-series machine only proves that scalar is correct; it does not prove that SIMD was verified.

**CI suggestion (at that time):** three GitHub Actions jobs: `windows-latest`, `ubuntu-latest` (x86-64, verifies the AVX2 path), `macos-latest` (M-series runner, verifies aarch64 scalar/NEON).

## Suggested execution order

1. Done in 0.1.0: batch ZWST/ZWTV with AVX2, C ABI v1 + Python binding, streaming API + phon conversion; 0.2.0: narrower public API, maintainability refactor, behaviour contracts
2. VST plug-in (64 tracks + panel)
3. NEON kernel for aarch64
4. Other standards (added one by one after the workspace split)
