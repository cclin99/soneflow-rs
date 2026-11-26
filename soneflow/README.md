# soneflow

Rust implementation of ISO 532-1:2017 Zwicker loudness for calibrated pressure signals.

## Scope

- Stationary loudness: `loudness_zwst`
- Time-varying loudness: `loudness_zwtv`
- Input sample rate: 48 kHz only
- Output units: total loudness in sone and specific loudness in sone/Bark
- SIMD: AVX2+FMA dispatch for the time-varying filter bank and nonlinear decay kernels when supported by the host CPU

The repository tests compare the implementation with MoSQITo-generated golden data. The ISO 532-1 Annex B checks run only with your own licensed copy of the Annex B files; the repository does not carry them.

## Usage

```rust
use soneflow::{loudness_zwst, FieldType};

# fn main() -> Result<(), soneflow::SoneflowError> {
let signal = vec![0.0; 48_000];
let loudness = loudness_zwst(&signal, 48_000.0, FieldType::Free)?;
println!("N = {:.3} sone", loudness.n);
# Ok(())
# }
```

```rust
use soneflow::{loudness_zwtv, FieldType};

# fn main() -> Result<(), soneflow::SoneflowError> {
let signal = vec![0.0; 48_000];
let loudness = loudness_zwtv(&signal, 48_000.0, FieldType::Free)?;
println!("{} time samples", loudness.n.len());
# Ok(())
# }
```

## CLI Example

```powershell
cargo run --example cli -- path/to/recording.wav --calib 2.8284271247461903
```

The example reads a 48 kHz WAV (several channels are averaged), multiplies the full-scale samples by `--calib` (pascals at digital full scale; default 1.0) and prints the stationary loudness `N` in sone.

## Validation

```powershell
cargo test
cargo doc --no-deps
cargo clippy -- -D warnings
cargo fmt -- --check
```

Golden data and generated coefficient tables are reproducible with:

```powershell
..\.venv\Scripts\python.exe ..\tools\gen_golden.py
..\.venv\Scripts\python.exe ..\tools\gen_tables.py
```

Run those commands from the `soneflow` crate directory or adjust paths from the repository root.

## Benchmarks

Run:

```powershell
cargo bench --bench loudness
cargo bench --features internals --bench core
```

`loudness` uses only the public API; `core` measures single stages and needs
the `internals` feature. The benchmark groups `filter_bank_10s` (in `core`)
and `zwtv_10s` compare forced scalar dispatch with automatic AVX2 dispatch. Results depend on the CPU, build settings and thread count; measure on the target machine and record the commit together with the numbers.

## Non-goals

- No non-48 kHz resampling in the public API
- No VST/DAW plug-in and no NEON kernel (ARM uses the scalar path)
