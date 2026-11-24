//! Micro-benchmarks of the core stages (R7 refactor guard).
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use soneflow::core::calc_slopes::{calc_slopes_into, calc_slopes_n_only};
use soneflow::core::main_loudness::main_loudness;
use soneflow::FieldType;

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

criterion_group!(benches, bench_main_loudness, bench_slopes);
criterion_main!(benches);
