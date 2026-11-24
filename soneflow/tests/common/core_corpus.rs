//! Deterministic input corpora for the core stages (R7 characterization).
//! Shared by `tests/core_characterization.rs`, `tests/golden_core.rs` and
//! `benches/core.rs` (included there with `#[path]`).

use soneflow::tables::{DLL, RAP, RNS};

fn lcg(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

fn spike(band: usize, value: f64) -> [f64; 21] {
    let mut nm = [0.0; 21];
    nm[band] = value;
    nm
}

fn ramp(from: f64, to: f64) -> [f64; 21] {
    std::array::from_fn(|i| from + (to - from) * i as f64 / 20.0)
}

/// Core loudness frames for the slope stage: random frames, spikes, ramps,
/// alternating bands, every RNS value exactly and at +-1e-9 / +-0.5e-8,
/// all zero, negative values and values above `RNS[0]`.
pub fn slopes_corpus() -> Vec<[f64; 21]> {
    let mut frames = vec![
        [0.0; 21],
        [-1.0; 21],
        [30.0; 21],
        ramp(0.0, 10.0),
        ramp(10.0, 0.0),
    ];
    for band in 0..21 {
        for value in [0.05, 1.0, 8.0, 40.0] {
            frames.push(spike(band, value));
        }
    }
    frames.push(std::array::from_fn(|i| if i % 2 == 0 { 8.0 } else { 0.5 }));
    frames.push(std::array::from_fn(|i| if i % 2 == 0 { 0.5 } else { 8.0 }));
    for &range in &RNS {
        for delta in [0.0, 1e-9, -1e-9, 0.5e-8, -0.5e-8] {
            let v = range + delta;
            frames.push([v; 21]);
            let mut step = [v; 21];
            step[3] = 8.0;
            frames.push(step);
            frames.push(std::array::from_fn(|i| if i < 10 { 25.0 } else { v }));
        }
    }
    let mut seed = 0x0052_3743_6f72_6531_u64;
    for _ in 0..2000 {
        frames.push(std::array::from_fn(|_| lcg(&mut seed) * 30.0 - 1.0));
    }
    frames
}

/// 28-band level frames for the main loudness stage: a 0.5 dB sweep of every
/// band from -20 to 140 dB over a 50 dB background, every low-frequency DLL
/// threshold at `next_down` / exact / `next_up`, and the 120 dB limit.
pub fn levels_corpus() -> Vec<[f64; 28]> {
    let mut frames = Vec::new();
    for band in 0..28 {
        for step in 0..=320 {
            let mut frame = [50.0; 28];
            frame[band] = -20.0 + 0.5 * step as f64;
            frames.push(frame);
        }
    }
    for band in 0..11 {
        for row in 0..DLL.len() {
            let thr = RAP[row] - DLL[row][band];
            for level in [thr.next_down(), thr, thr.next_up()] {
                let mut frame = [40.0; 28];
                frame[band] = level;
                frames.push(frame);
            }
        }
        for level in [119.9, 120.0, 120.0_f64.next_up(), 120.1] {
            let mut frame = [60.0; 28];
            frame[band] = level;
            frames.push(frame);
        }
    }
    let mut seed = 0x004c_6576_656c_7331_u64;
    for _ in 0..2000 {
        frames.push(std::array::from_fn(|_| lcg(&mut seed) * 130.0 - 10.0));
    }
    frames
}
