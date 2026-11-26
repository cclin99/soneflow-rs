//! Micro-benchmarks of single stages: the core stages (refactor guard) and
//! the time-varying third-octave filter bank. Needs feature `internals`.
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use soneflow::internals::core::calc_slopes::{calc_slopes_into, calc_slopes_n_only};
use soneflow::internals::core::main_loudness::main_loudness;
use soneflow::internals::zwtv::third_octave_levels::third_octave_levels;
use soneflow::{simd, FieldType};

#[path = "../tests/common/core_corpus.rs"]
mod core_corpus;

fn bench_main_loudness(c: &mut Criterion) {
    let levels = core_corpus::levels_corpus();
    let mut group = c.benchmark_group("core_main_loudness");
    group.measurement_time(Duration::from_secs(6));
    group.throughput(Throughput::Elements(levels.len() as u64));
    for field in [FieldType::Free, FieldType::Diffuse] {
        group.bench_function(format!("{field:?}"), |b| {
            b.iter(|| {
                let mut acc = 0.0;
                for frame in &levels {
                    if let Ok(nm) = main_loudness(black_box(frame), field) {
                        acc += nm[0];
                    }
                }
                acc
            })
        });
    }
    group.finish();
}

fn bench_slopes(c: &mut Criterion) {
    let frames = core_corpus::slopes_corpus();
    let mut group = c.benchmark_group("core_slopes");
    group.measurement_time(Duration::from_secs(6));
    group.throughput(Throughput::Elements(frames.len() as u64));
    group.bench_function("n_only", |b| {
        b.iter(|| {
            frames
                .iter()
                .map(|nm| calc_slopes_n_only(black_box(nm)))
                .sum::<f64>()
        })
    });
    group.bench_function("into", |b| {
        let mut spec = [0.0; 240];
        b.iter(|| {
            frames
                .iter()
                .map(|nm| calc_slopes_into(black_box(nm), &mut spec))
                .sum::<f64>()
        })
    });
    group.finish();
}

const FS: f64 = 48_000.0;
const BENCH_SECS: usize = 10;
const SIGNAL_LEN: usize = 48_000 * BENCH_SECS;

/// Same signal as `benches/loudness.rs`.
fn bench_signal() -> Vec<f64> {
    (0..SIGNAL_LEN)
        .map(|i| {
            let t = i as f64 / FS;
            0.25 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()
                + 0.10 * (2.0 * std::f64::consts::PI * 1_760.0 * t).sin()
                + 0.04 * (2.0 * std::f64::consts::PI * 6_400.0 * t).sin()
        })
        .collect()
}

fn auto_dispatch_label() -> &'static str {
    if simd::avx2_available() {
        "avx2"
    } else {
        "auto_scalar_fallback"
    }
}

fn thread_suffix() -> String {
    format!("t{}", rayon::current_num_threads())
}

fn bench_third_octave_filter_bank(c: &mut Criterion) {
    let signal = bench_signal();
    let mut group = c.benchmark_group(format!("filter_bank_10s_{}", thread_suffix()));
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(6));
    group.throughput(Throughput::Elements(signal.len() as u64));

    group.bench_function(BenchmarkId::new("scalar", signal.len()), |b| {
        simd::set_force_scalar(true);
        b.iter(|| {
            let (levels, n_time) = third_octave_levels(black_box(signal.as_slice()));
            black_box((levels, n_time));
        });
        simd::set_force_scalar(false);
    });

    group.bench_function(BenchmarkId::new(auto_dispatch_label(), signal.len()), |b| {
        simd::set_force_scalar(false);
        b.iter(|| {
            let (levels, n_time) = third_octave_levels(black_box(signal.as_slice()));
            black_box((levels, n_time));
        });
    });

    simd::set_force_scalar(false);
    group.finish();
}

criterion_group!(
    benches,
    bench_main_loudness,
    bench_slopes,
    bench_third_octave_filter_bank
);
criterion_main!(benches);
