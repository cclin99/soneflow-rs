#[allow(dead_code)]
mod common;

use soneflow::NlInit;
use soneflow::{simd, FieldType, ZwtvProcessor};

const FS: f64 = 48_000.0;

// Frozen bitwise snapshots of (fnv1a(n), fnv1a(n_specific), fnv1a(time_axis))
// for the synthetic signal below, one per backend, OS and nonlinear-decay
// initialisation. Scalar and AVX2 differ in ULP because FMA rounds once; libm
// (sin/powf/log10) differs in ULP across platforms as well. Each path must
// stay bitwise-stable against its own snapshot (refactor invariance).
//
// Scope of the contract: time_axis is bitwise-identical across every
// environment tested. n_specific and n carry libm ULP noise (the total is not
// rounded per frame, so nothing hides that noise). glibc values are stable
// across machines (frozen from an ubuntu:24.04 container, glibc 2.39, rustc
// 1.99.1; GitHub CI with rustc 1.100 prints the same values). Windows UCRT
// dispatches libm by CPU and varies by OS build, so the windows constants only
// hold on the machine that froze them — windows CI therefore runs dump-only
// via HASH_GATE_DUMP_ONLY=1 (see .github/workflows/ci.yml); a different dev
// machine should re-freeze locally.
//
// `*_MOSQITO_TAIL` pin `NlInit::MosqitoTail`; the others pin the default
// `NlInit::Zero`. Only `n` and `n_specific` differ between the two;
// `time_axis` is the same.
// Regenerate: set a pair to (0, 0, 0), run
// `cargo test --test hash_gate -- --nocapture`, copy the printed values.
#[cfg(target_os = "windows")]
const EXPECTED_SCALAR: (u64, u64, u64) =
    (0xa5c169e50eb05414, 0x63a0952e4d8f6bea, 0xf076bcb342595537);
#[cfg(target_os = "windows")]
const EXPECTED_AVX2: (u64, u64, u64) = (0x0ea36644a71c49e0, 0x316e5578a19148d7, 0xf076bcb342595537);
#[cfg(target_os = "windows")]
const EXPECTED_SCALAR_MOSQITO_TAIL: (u64, u64, u64) =
    (0x3b2600d6861db12e, 0x63fb11bfc7004645, 0xf076bcb342595537);
#[cfg(target_os = "windows")]
const EXPECTED_AVX2_MOSQITO_TAIL: (u64, u64, u64) =
    (0xb6ce4544b62cd31e, 0x8cc468510ee4babf, 0xf076bcb342595537);

#[cfg(target_os = "linux")]
const EXPECTED_SCALAR: (u64, u64, u64) =
    (0x924db9f750987806, 0x703fab1a5ecb2d74, 0xf076bcb342595537);
#[cfg(target_os = "linux")]
const EXPECTED_AVX2: (u64, u64, u64) = (0x4d03ad33f1b83dcf, 0x1f03d0e7af6dd0da, 0xf076bcb342595537);
#[cfg(target_os = "linux")]
const EXPECTED_SCALAR_MOSQITO_TAIL: (u64, u64, u64) =
    (0xcc54d94d2033a5ea, 0x4dcb1dda99efd42e, 0xf076bcb342595537);
#[cfg(target_os = "linux")]
const EXPECTED_AVX2_MOSQITO_TAIL: (u64, u64, u64) =
    (0x3ddcf3034b61da04, 0x01eaa3bce1557cde, 0xf076bcb342595537);

// unfrozen: regenerate per note above
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const EXPECTED_SCALAR: (u64, u64, u64) = (0, 0, 0);
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const EXPECTED_AVX2: (u64, u64, u64) = (0, 0, 0);
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const EXPECTED_SCALAR_MOSQITO_TAIL: (u64, u64, u64) = (0, 0, 0);
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const EXPECTED_AVX2_MOSQITO_TAIL: (u64, u64, u64) = (0, 0, 0);

fn run_hashes(signal: &[f64], init: NlInit) -> (u64, u64, u64) {
    let r = ZwtvProcessor::new()
        .process_with(signal, FS, FieldType::Free, init)
        .unwrap();
    (
        common::fnv1a_f64(&r.n),
        common::fnv1a_f64(&r.n_specific),
        common::fnv1a_f64(&r.time_axis),
    )
}

fn print(label: &str, h: (u64, u64, u64)) {
    eprintln!(
        "{label}: n={:#018x} spec={:#018x} time={:#018x}",
        h.0, h.1, h.2
    );
}

// FORCE_SCALAR is process-global. Keep this integration test file to one
// #[test] so flag changes cannot race another test in this binary.
#[test]
fn zwtv_backend_hashes_match_frozen_snapshot() {
    let signal = common::synth_signal();

    // Compute and print every value before asserting so a single failing
    // run (e.g. a new OS) still reveals every value needed to freeze.
    simd::set_force_scalar(true);
    let scalar = run_hashes(&signal, NlInit::Zero);
    let scalar_tail = run_hashes(&signal, NlInit::MosqitoTail);
    simd::set_force_scalar(false);
    print("scalar", scalar);
    print("scalar mosqito_tail", scalar_tail);

    let avx2 = if common::require_avx2_or_skip("zwtv_backend_hashes avx2") {
        let avx2 = run_hashes(&signal, NlInit::Zero);
        let avx2_tail = run_hashes(&signal, NlInit::MosqitoTail);
        print("avx2", avx2);
        print("avx2 mosqito_tail", avx2_tail);
        Some((avx2, avx2_tail))
    } else {
        None
    };

    let dump_only = EXPECTED_SCALAR == (0, 0, 0)
        || std::env::var("HASH_GATE_DUMP_ONLY").is_ok_and(|v| v == "1");
    if dump_only {
        eprintln!("hash gate: dump-only run (no frozen snapshot for this environment)");
        return;
    }
    assert_eq!(scalar, EXPECTED_SCALAR, "scalar backend hash drifted");
    assert_eq!(
        scalar_tail, EXPECTED_SCALAR_MOSQITO_TAIL,
        "scalar backend (mosqito tail) hash drifted"
    );
    if let Some((avx2, avx2_tail)) = avx2 {
        assert_eq!(avx2, EXPECTED_AVX2, "avx2 backend hash drifted");
        assert_eq!(
            avx2_tail, EXPECTED_AVX2_MOSQITO_TAIL,
            "avx2 backend (mosqito tail) hash drifted"
        );
    }
}
