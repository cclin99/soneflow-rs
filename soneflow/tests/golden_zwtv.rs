//! MoSQITo 1.2.1 goldens. The stage tests (third-octave levels, nonlinear
//! decay, temporal weighting) stay tight. The slope stage follows ISO 532-1
//! Annex A (no per-frame rounding of the total, every band completed), so the
//! end-to-end N(t) comparisons below are a compatibility check against MoSQITo
//! (相容性對照) with a bound of 5e-3 sone. N'(t) is recorded, not gated: in the
//! cell where a slope ends this crate writes the core loudness (or the
//! containing segment), MoSQITo extrapolates the slope there (up to ~4e-2
//! sone/Bark).

mod common;
use common::*;
use soneflow::zwtv::nonlinear_decay::{nl_loudness_scalar, nl_loudness_scalar_with};
use soneflow::zwtv::temporal_weighting::temporal_weighting;
use soneflow::zwtv::third_octave_levels::third_octave_levels_scalar;
use soneflow::zwtv::NlInit;
use soneflow::{loudness_zwtv, FieldType, ZwtvProcessor};

// MoSQITo 1.2.1 seeds the nonlinear decay from the last frame; the
// golden fixtures therefore compare exactly only under `NlInit::MosqitoTail`.
// Fixtures whose signal ends in silence have a zero seed and are identical
// under both initialisations.
const SILENT_TAIL: [&str; 2] = ["pulse_1k_70", "annexb_sig10"];

/// MoSQITo compatibility bound for the total loudness, sone.
const COMPAT_TOL: f64 = 5e-3;

fn mosqito_compatible(sig: &[f64]) -> soneflow::LoudnessTimeVarying {
    ZwtvProcessor::new()
        .process_with(sig, 48000.0, FieldType::Free, NlInit::MosqitoTail)
        .unwrap()
}

#[test]
fn third_octave_levels_matches_mosqito() {
    for sig in ["sine_1k_60", "pulse_1k_70", "step_60_80", "white_60"] {
        let x = read_bin(sig, "sig.bin");
        let want = read_bin(sig, "third_octave_level.bin");
        let (got, n_time) = third_octave_levels_scalar(&x);
        assert_eq!(n_time, want.len() / 28, "{sig}: n_time");
        assert_close(
            &got,
            &want,
            1e-7,
            1e-12,
            &format!("{sig}/third_octave_level"),
        );
    }
}

#[test]
fn nl_loudness_matches_mosqito_under_tail_init() {
    for sig in ["sine_1k_60", "pulse_1k_70", "step_60_80", "white_60"] {
        let core = read_bin(sig, "core_loudness.bin");
        let want = read_bin(sig, "nl_loudness.bin");
        let n_time = core.len() / 21;
        let got = nl_loudness_scalar_with(&core, n_time, NlInit::MosqitoTail);
        assert_close(&got, &want, 1e-6, 1e-9, &format!("{sig}/nl_loudness"));
    }
}

#[test]
fn nl_loudness_zero_init_equals_golden_when_the_fixture_ends_in_silence() {
    for sig in SILENT_TAIL {
        let core = read_bin(sig, "core_loudness.bin");
        let n_time = core.len() / 21;
        let last_frame_max = (0..21)
            .map(|b| core[b * n_time + n_time - 1])
            .fold(0.0, f64::max);
        assert_eq!(last_frame_max, 0.0, "{sig}: fixture must end in silence");
        let zero = nl_loudness_scalar(&core, n_time);
        let tail = nl_loudness_scalar_with(&core, n_time, NlInit::MosqitoTail);
        assert_eq!(
            zero, tail,
            "{sig}: zero seed must make both paths identical"
        );
        let want = read_bin(sig, "nl_loudness.bin");
        assert_close(&zero, &want, 1e-6, 1e-9, &format!("{sig}/nl_loudness zero"));
    }
}

#[test]
fn temporal_weighting_matches_mosqito() {
    for sig in ["pulse_1k_70", "step_60_80", "white_60"] {
        let loud = read_bin(sig, "loudness_raw.bin");
        let want = read_bin(sig, "filt_loudness.bin");
        let got = temporal_weighting(&loud);
        assert_close(&got, &want, 1e-9, 1e-12, &format!("{sig}/temporal"));
    }
}

#[test]
fn zwtv_end_to_end_compat_mosqito_under_tail_init() {
    for sig in ["sine_1k_60", "pulse_1k_70", "step_60_80", "annexb_sig10"] {
        let x = read_bin(sig, "sig.bin");
        let want_n = read_bin(sig, "N_time.bin");
        let want_spec = read_bin(sig, "N_spec_time.bin");
        let r = mosqito_compatible(&x);
        assert_eq!(r.bark_axis.len(), 240, "{sig}: bark axis");
        assert_close(
            &r.n,
            &want_n,
            0.0,
            COMPAT_TOL,
            &format!("{sig}/N_time (compatibility)"),
        );
        record_max_abs_diff(
            &r.n_specific,
            &want_spec,
            &format!("{sig}/N_spec_time vs MoSQITo"),
        );
    }
}

#[test]
fn zwtv_default_equals_golden_for_silent_tail_fixtures() {
    for sig in SILENT_TAIL {
        let x = read_bin(sig, "sig.bin");
        let r = loudness_zwtv(&x, 48000.0, FieldType::Free).unwrap();
        let c = mosqito_compatible(&x);
        assert_eq!(r.n, c.n, "{sig}: N must be bitwise identical");
        assert_eq!(
            r.n_specific, c.n_specific,
            "{sig}: N' must be bitwise identical"
        );
        assert_close(
            &r.n,
            &read_bin(sig, "N_time.bin"),
            0.0,
            COMPAT_TOL,
            &format!("{sig}/N_time default (compatibility)"),
        );
    }
}

/// Spacing to the next larger f64 (`x >= 0`).
fn ulp(x: f64) -> f64 {
    let x = x.abs();
    f64::from_bits(x.to_bits() + 1) - x
}

#[test]
fn zwtv_default_converges_to_golden_once_the_tail_seed_has_decayed() {
    // The default (zero state) and the MoSQITo tail seed differ only through
    // the initial capacitor voltages of the NL stage. On the 10 s Annex B
    // fixtures the seed has decayed below the golden tolerance well before
    // output frame 600 (compatibility check with COMPAT_TOL from SETTLED).
    //
    // Convergence contract (no measured frame number is pinned):
    // 1. From ENVELOPE_FROM on, the specific loudness of the two paths is
    //    bitwise identical (measured: from frame 26 / 56), so the NL stage has
    //    converged and the remaining difference of N lives only in the state
    //    of the temporal weighting (ISO 532-1: 3.5 ms and 70 ms low passes).
    // 2. That difference therefore decays at least as fast as the slow low
    //    pass: |dN(t)| <= 2 |dN(t0)| exp(-(t - t0) 2 ms / 70 ms) + 8 ulp(N).
    //    Measured ratio to this bound <= 0.54.
    // 3. The residue is eventually absorbed by rounding: the last BITWISE_TAIL
    //    frames (2 s) are bitwise identical. Where exactly the absorption
    //    happens depends on the last bits of the pipeline and has moved with
    //    every change to them, which is why no measured frame number is pinned.
    const SETTLED: usize = 600;
    const ENVELOPE_FROM: usize = 200;
    /// Slow time constant of the temporal weighting, ISO 532-1 (written here
    /// as its own literal, not imported from the product).
    const TAU_SLOW_S: f64 = 70e-3;
    /// Output grid step (500 Hz).
    const OUT_STEP_S: f64 = 2e-3;
    const BITWISE_TAIL: usize = 1000;
    let decay = (-OUT_STEP_S / TAU_SLOW_S).exp();
    for sig in ["annexb_sig3", "annexb_sig5"] {
        let x = read_bin(sig, "sig.bin");
        let want_n = read_bin(sig, "N_time.bin");
        let r = loudness_zwtv(&x, 48000.0, FieldType::Free).unwrap();
        let n_out = r.n.len();
        assert!(
            n_out > SETTLED + BITWISE_TAIL,
            "{sig}: expected a 10 s fixture"
        );
        assert_close(
            &r.n[SETTLED..],
            &want_n[SETTLED..],
            0.0,
            COMPAT_TOL,
            &format!("{sig}/N_time default from {SETTLED} (compatibility)"),
        );
        let c = mosqito_compatible(&x);
        for bark in 0..240 {
            for t in ENVELOPE_FROM..n_out {
                let (a, b) = (
                    r.n_specific[bark * n_out + t],
                    c.n_specific[bark * n_out + t],
                );
                assert_eq!(a.to_bits(), b.to_bits(), "{sig}: N' bark {bark} frame {t}");
            }
        }
        let d0 = (r.n[ENVELOPE_FROM] - c.n[ENVELOPE_FROM]).abs();
        assert!(
            d0 > 0.0,
            "{sig}: the seed must still be visible at frame {ENVELOPE_FROM}"
        );
        let mut envelope = 2.0 * d0;
        for t in ENVELOPE_FROM..n_out {
            let (a, b) = (r.n[t], c.n[t]);
            let bound = envelope + 8.0 * ulp(a.abs().max(b.abs()));
            assert!(
                (a - b).abs() <= bound,
                "{sig}: frame {t}: |dN| {:.3e} above the 70 ms envelope {bound:.3e}",
                (a - b).abs()
            );
            envelope *= decay;
        }
        for t in n_out - BITWISE_TAIL..n_out {
            assert_eq!(
                r.n[t].to_bits(),
                c.n[t].to_bits(),
                "{sig}: frame {t}: {} vs {}",
                r.n[t],
                c.n[t]
            );
        }
        assert_ne!(
            r.n[0].to_bits(),
            c.n[0].to_bits(),
            "{sig}: the two initialisations must still differ at the start"
        );
    }
}

/// Bitwise output snapshots on the AVX2 auto-dispatch path (fnv1a of n,
/// n_specific, time_axis). Rayon/Sequential are bitwise-equal (see determinism
/// tests), so these values do not depend on thread count.
///
/// `MOSQITO_TAIL` pins `NlInit::MosqitoTail`, `ZERO` pins the default
/// `NlInit::Zero`. The two tables agree on the silent-tail fixtures, where the
/// seed is zero: only the seed differs between them.
/// Regenerate with
/// `cargo test --test golden_zwtv dump_zwtv_output_hashes -- --ignored --nocapture`.
const SNAPSHOT_AVX2_MOSQITO_TAIL: [(&str, u64, u64, u64); 4] = [
    (
        "sine_1k_60",
        0x9fe43c9a9c3cb073,
        0x61bb8b39cec4f6be,
        0xf076bcb342595537,
    ),
    (
        "pulse_1k_70",
        0x58f8a0ffb99cda3d,
        0x7a0d6668b5f56708,
        0xf076bcb342595537,
    ),
    (
        "step_60_80",
        0x5e3bea7739aeedce,
        0x60bb208a119eecbd,
        0xf076bcb342595537,
    ),
    (
        "annexb_sig10",
        0x5cab264aa8762197,
        0x4d4f56bcd5c88c09,
        0xf076bcb342595537,
    ),
];

const SNAPSHOT_AVX2_ZERO: [(&str, u64, u64, u64); 4] = [
    (
        "sine_1k_60",
        0xd75f14159e6d5655,
        0xfef1705ea0a1a2bc,
        0xf076bcb342595537,
    ),
    (
        "pulse_1k_70",
        0x58f8a0ffb99cda3d,
        0x7a0d6668b5f56708,
        0xf076bcb342595537,
    ),
    (
        "step_60_80",
        0x81626bb171d11764,
        0x33f5ef37629aedb4,
        0xf076bcb342595537,
    ),
    (
        "annexb_sig10",
        0x5cab264aa8762197,
        0x4d4f56bcd5c88c09,
        0xf076bcb342595537,
    ),
];

fn assert_snapshot(
    table: &[(&str, u64, u64, u64); 4],
    run: impl Fn(&[f64]) -> soneflow::LoudnessTimeVarying,
    label: &str,
) {
    for (sig, want_n, want_spec, want_time) in table {
        let x = read_bin(sig, "sig.bin");
        let r = run(&x);
        assert_eq!(fnv1a_f64(&r.n), *want_n, "{label} {sig}: N hash drifted");
        assert_eq!(
            fnv1a_f64(&r.n_specific),
            *want_spec,
            "{label} {sig}: N_specific hash drifted"
        );
        assert_eq!(
            fnv1a_f64(&r.time_axis),
            *want_time,
            "{label} {sig}: time hash drifted"
        );
    }
}

#[test]
fn zwtv_output_hashes_match_frozen_snapshots() {
    if !require_avx2_or_skip("zwtv_output_hashes_match_frozen_snapshots") {
        return;
    }
    assert_snapshot(
        &SNAPSHOT_AVX2_ZERO,
        |x| loudness_zwtv(x, 48000.0, FieldType::Free).unwrap(),
        "zero (default)",
    );
    assert_snapshot(
        &SNAPSHOT_AVX2_MOSQITO_TAIL,
        mosqito_compatible,
        "mosqito tail",
    );
    for (a, b) in SNAPSHOT_AVX2_ZERO.iter().zip(&SNAPSHOT_AVX2_MOSQITO_TAIL) {
        assert_eq!(a.0, b.0);
        let silent = SILENT_TAIL.contains(&a.0);
        assert_eq!(
            (a.1, a.2) == (b.1, b.2),
            silent,
            "{}: snapshots must agree exactly when (and only when) the tail is silent",
            a.0
        );
    }
}

#[test]
#[ignore = "manual helper: bitwise output snapshot for refactor verification"]
fn dump_zwtv_output_hashes() {
    for sig in ["sine_1k_60", "pulse_1k_70", "step_60_80", "annexb_sig10"] {
        let x = read_bin(sig, "sig.bin");
        for (label, r) in [
            ("zero", loudness_zwtv(&x, 48000.0, FieldType::Free).unwrap()),
            ("mosqito_tail", mosqito_compatible(&x)),
        ] {
            println!(
                "{sig} [{label}]: n={:016x} spec={:016x} time={:016x}",
                fnv1a_f64(&r.n),
                fnv1a_f64(&r.n_specific),
                fnv1a_f64(&r.time_axis),
            );
        }
    }
}
