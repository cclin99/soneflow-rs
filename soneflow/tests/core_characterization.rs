//! R7 BDD scenarios for the core stages (calc_slopes, main_loudness).
//!
//! Characterization: these describe the ISO 532-1 Annex A behaviour of the
//! public API and pass on the pre-refactor baseline by design; they guard the
//! refactor, they are not its RED. Scenarios that need crate-private items
//! (`core_loudness_of_band`, `excitation_level`, `band0_correction`,
//! `main_loudness_clamped`) are unit tests next to those items.

#[path = "common/core_corpus.rs"]
mod core_corpus;

use soneflow::core::calc_slopes::{
    calc_slopes_into, calc_slopes_n_only, try_calc_slopes_into, SlopeInputError,
};
use soneflow::core::main_loudness::main_loudness;
use soneflow::tables::ZUP;
use soneflow::{FieldType, SoneflowError};

/// Band-edge extension of the slope stage (0.0001, Zwicker et al. 1991; held in
/// single precision and widened), as `core::calc_slopes` uses it (crate-private).
const ZUP_EPS: f64 = 0.0001_f32 as f64;

fn cell_z(cell: usize) -> f64 {
    (cell + 1) as f64 / 10.0
}

/// The 0.1 Bark samples that belong to band `k` (edges at `ZUP + ZUP_EPS`).
fn band_cells(k: usize) -> impl Iterator<Item = usize> {
    let lo = if k == 0 {
        f64::NEG_INFINITY
    } else {
        ZUP[k - 1] + ZUP_EPS
    };
    let hi = ZUP[k] + ZUP_EPS;
    (0..240).filter(move |&c| cell_z(c) > lo && cell_z(c) <= hi)
}

fn pattern(nm: &[f64; 21]) -> (f64, [f64; 240]) {
    let mut spec = [f64::NAN; 240];
    let n = calc_slopes_into(nm, &mut spec);
    (n, spec)
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

// ---- Feature: specific loudness pattern (calc_slopes) ----

#[test]
fn equal_core_loudness_gives_a_flat_pattern() {
    for c in [0.05, 1.0, 8.0] {
        let (n, spec) = pattern(&[c; 21]);
        assert!(spec.iter().all(|&v| v == c), "c = {c}: pattern not flat");
        assert!(rel(n, c * (ZUP[20] + ZUP_EPS)) <= 1e-12, "c = {c}: N = {n}");
    }
}

#[test]
fn falling_core_loudness_descends_along_the_upper_slope() {
    for k in 0..20 {
        for (hi, lo) in [(8.0, 1.0), (25.0, 0.3), (2.0, 0.0)] {
            let nm: [f64; 21] = std::array::from_fn(|i| if i <= k { hi } else { lo });
            let (_, spec) = pattern(&nm);
            let cells: Vec<usize> = band_cells(k + 1).collect();
            for w in cells.windows(2) {
                assert!(
                    spec[w[1]] <= spec[w[0]],
                    "k = {k}: not non-increasing at {}",
                    w[1]
                );
            }
            for &c in &cells {
                assert!(spec[c] >= lo, "k = {k}: cell {c} below the core loudness");
            }
        }
    }
}

#[test]
fn rising_core_loudness_jumps_up_at_the_band_edge() {
    for k in 0..20 {
        let nm: [f64; 21] = std::array::from_fn(|i| if i <= k { 1.0 } else { 8.0 });
        let (_, spec) = pattern(&nm);
        for c in band_cells(k + 1) {
            assert_eq!(spec[c], 8.0, "k = {k}: cell {c}");
        }
    }
}

#[test]
fn total_only_and_with_pattern_give_bitwise_equal_totals() {
    let corpus = core_corpus::slopes_corpus();
    assert!(corpus.len() >= 2000);
    for (i, nm) in corpus.iter().enumerate() {
        let (n, _) = pattern(nm);
        assert_eq!(calc_slopes_n_only(nm).to_bits(), n.to_bits(), "frame {i}");
    }
}

#[test]
fn band_0_starts_from_zero_and_does_not_look_back_at_band_20() {
    let mut nm = [0.0; 21];
    nm[20] = 5.0;
    let (n, spec) = pattern(&nm);
    for k in 0..20 {
        for c in band_cells(k) {
            assert_eq!(spec[c], 0.0, "band {k} cell {c}");
        }
    }
    assert!(rel(n, 5.0 * (ZUP[20] - ZUP[19])) <= 1e-12, "N = {n}");
}

#[test]
fn total_loudness_is_never_negative() {
    // No per-frame rounding of the total, only the negative clamp.
    for nm in core_corpus::slopes_corpus() {
        assert!(calc_slopes_n_only(&nm) >= 0.0, "{nm:?}");
    }
}

// ---- Feature: applicability of calc_slopes ----
//
// Valid domain: all 21 core loudness values >= 0 and finite (Annex A). The
// normal pipelines are designed to produce only such input; the low-level
// `soneflow::core::calc_slopes` entry points accept anything. Four layers:
//
// 1. Valid domain values and Annex A rules (locked).
// 2. Algorithm rule: band 0 never continues a slope (any finite input).
// 3. Finite within the bounds: valid domain with max core <= B1 = 1e306, or
//    any finite input with max |core| <= B2 = 1e150. The call returns, N is
//    finite and >= 0, the 240 samples are finite, both entry points agree.
//    Out-of-domain values have no physical meaning and are not locked.
// 4. Beyond the bounds, or non-finite input: the call returns (no panic,
//    debug or release); the output is unspecified (+inf, or 0 when the
//    final clamp meets NaN or -inf).
//
// An earlier revision promised "finite in, finite out" without a bound; an
// independent review refuted it (1e308 in every band gives N = +inf). The
// bounds and their derivation are in the module docs of `core::calc_slopes`.
// Negative input has no meaning in the standard and is not frozen to N == 0.

/// Valid-domain bound: the total is at most 24.0001 * max core, so it stays
/// finite up to about f64::MAX / 24.0001 = 7.49e306.
const B1: f64 = 1e306;
/// Out-of-domain bound: one walk-back segment of area n1^2 / (2 usl) with an
/// intermediate product up to 50 * M^2 overflows from about 1.9e153.
const B2: f64 = 1e150;

fn band_0_cells(spec: &[f64; 240]) -> Vec<f64> {
    band_cells(0).map(|c| spec[c]).collect()
}

#[test]
fn band_0_stays_flat_in_the_valid_domain() {
    for c0 in [0.0, 0.05, 8.0] {
        for rest in [0.0, 1.0, 20.0] {
            let mut nm = [rest; 21];
            nm[0] = c0;
            let (_, spec) = pattern(&nm);
            assert!(
                band_0_cells(&spec).iter().all(|&v| v == c0),
                "c0 = {c0}, rest = {rest}"
            );
        }
    }
}

/// Algorithm rule: band 0
/// starts from N1 = 0 and never continues a slope, whatever the sign of its
/// core loudness.
#[test]
fn band_0_valid_values_stay_flat_and_negative_values_are_rejected() {
    for c0 in [0.0, 5.0] {
        let mut core = [1.0; 21];
        core[0] = c0;
        let (_, specific) = pattern(&core);
        assert!(band_0_cells(&specific).iter().all(|&v| v == c0));
    }
    for c0 in [-1e-9, -1.0, -30.0] {
        let mut core = [1.0; 21];
        core[0] = c0;
        let mut output = [42.0; 240];
        assert_eq!(
            try_calc_slopes_into(&core, &mut output),
            Err(SlopeInputError::Negative { band: 0 })
        );
        assert_eq!(output, [42.0; 240]);
        let (_, specific) = pattern(&core);
        assert!(band_0_cells(&specific).iter().all(|&v| v == 0.0));
    }
}

#[test]
fn a_negative_band_0_leaves_the_upper_bands_unchanged() {
    let mut negative = [1.0; 21];
    negative[0] = -1.0;
    let mut zero = [1.0; 21];
    zero[0] = 0.0;
    let (_, a) = pattern(&negative);
    let (_, b) = pattern(&zero);
    for k in 1..21 {
        for c in band_cells(k) {
            assert_eq!(a[c].to_bits(), b[c].to_bits(), "band {k} cell {c}");
        }
    }
}

fn out_of_domain_finite_inputs() -> Vec<(&'static str, [f64; 21])> {
    let mut band0_only = [0.0; 21];
    band0_only[0] = -1.0;
    vec![
        ("all -1e-9", [-1e-9; 21]),
        ("all -1", [-1.0; 21]),
        ("all -30", [-30.0; 21]),
        ("band 0 = -1, rest 0", band0_only),
        (
            "-1, -2, ..., -21",
            std::array::from_fn(|k| -((k + 1) as f64)),
        ),
        (
            "-30, -29, ..., -10",
            std::array::from_fn(|k| -30.0 + k as f64),
        ),
    ]
}

/// Layer 3: N finite and >= 0, 240 finite samples, both entry points agree.
fn assert_finite_contract(name: &str, nm: &[f64; 21]) -> f64 {
    let n_only = calc_slopes_n_only(nm);
    let (n, spec) = pattern(nm);
    assert!(n.is_finite() && n >= 0.0, "{name}: N = {n}");
    assert_eq!(n_only.to_bits(), n.to_bits(), "{name}: entry points differ");
    assert!(
        spec.iter().all(|v| v.is_finite()),
        "{name}: non-finite cell"
    );
    n
}

/// Walk-back shape: bands 0..flat_bands at -s/2 (flat, from band 0), then
/// band `flat_bands` at -s, below the carried-in -s/2: one slope segment that
/// walks back by (s/2) / usl Bark.
fn walk_back(s: f64, flat_bands: usize) -> [f64; 21] {
    let mut nm = [0.0; 21];
    for v in nm.iter_mut().take(flat_bands) {
        *v = -0.5 * s;
    }
    nm[flat_bands] = -s;
    nm
}

/// Deterministic generator for the random scenarios (LCG, top 53 bits).
struct Lcg(u64);

impl Lcg {
    fn unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[test]
fn finite_out_of_domain_input_within_b2_returns_a_finite_non_negative_total() {
    for (name, nm) in out_of_domain_finite_inputs() {
        assert_finite_contract(name, &nm);
    }
}

#[test]
fn the_valid_domain_is_finite_up_to_b1() {
    let shapes: [(&str, [f64; 21]); 4] = [
        ("all equal (largest total)", [B1; 21]),
        ("band 0 only", {
            let mut a = [0.0; 21];
            a[0] = B1;
            a
        }),
        (
            "alternating",
            std::array::from_fn(|i| if i % 2 == 0 { B1 } else { 0.0 }),
        ),
        (
            "descending",
            std::array::from_fn(|i| B1 * (21 - i) as f64 / 21.0),
        ),
    ];
    for (name, nm) in shapes {
        assert_finite_contract(name, &nm);
    }
    let mut rng = Lcg(0x5eed_7b00_c0de_0001);
    for i in 0..2000 {
        let scale = 10f64.powf(rng.unit() * 306.0);
        let nm: [f64; 21] = std::array::from_fn(|_| (rng.unit() * scale).min(B1));
        assert_finite_contract(&format!("random valid #{i}"), &nm);
    }
}

#[test]
fn out_of_domain_input_is_finite_up_to_b2() {
    let shapes: [(&str, [f64; 21]); 5] = [
        ("review mixed_negative_large at B2", {
            let mut a = [B2; 21];
            a[0] = -1.0;
            a
        }),
        ("walk-back usl 0.06 (band 1)", walk_back(B2, 1)),
        ("walk-back usl 0.02 (band 5)", walk_back(B2, 5)),
        ("walk-back then positive", {
            let mut a = walk_back(B2, 5);
            for v in a.iter_mut().skip(6) {
                *v = B2;
            }
            a
        }),
        ("minimised review counterexample at B2", {
            let mut a = [0.0; 21];
            a[0] = -B2;
            a[1] = -0.695 * B2;
            a[2] = -0.713 * B2;
            a
        }),
    ];
    for (name, nm) in shapes {
        assert_finite_contract(name, &nm);
    }
    let mut rng = Lcg(0x5eed_7b00_c0de_0002);
    for i in 0..2000 {
        let scale = 10f64.powf(rng.unit() * 150.0);
        let nm: [f64; 21] = std::array::from_fn(|_| {
            let v = (rng.unit() * scale).min(B2);
            if rng.unit() < 0.5 {
                -v
            } else {
                v
            }
        });
        assert_finite_contract(&format!("random signed #{i}"), &nm);
    }
}

/// Observable consequence of the derivation (invariant I: at most one
/// walk-back, area n1^2 / (2 usl)): with n1 = -s/2 the total is s^2 / (8 usl)
/// once the linear terms (|N| <= 24.0001 s) are negligible. A cross-check of
/// the derivation, not a numeric contract.
#[test]
fn negative_core_is_rejected_without_inventing_walk_back_area() {
    for flat_bands in [1, 5] {
        for scale in [1e50, 1e100, 1e150] {
            let core = walk_back(scale, flat_bands);
            let mut output = [42.0; 240];
            assert_eq!(
                try_calc_slopes_into(&core, &mut output),
                Err(SlopeInputError::Negative { band: 0 })
            );
            assert_eq!(output, [42.0; 240]);
            let (total, specific) = pattern(&core);
            assert_eq!(total, 0.0);
            assert!(specific.iter().all(|&v| v == 0.0));
        }
    }
}

/// Layer 4: beyond the bounds only termination is promised. These are
/// the independent review's counterexamples (N = +inf today) and the minimised
/// one at its original scale.
#[test]
fn finite_input_beyond_the_bounds_returns_without_panicking() {
    let inputs: [(&str, [f64; 21]); 4] = [
        ("all_MAX", [f64::MAX; 21]),
        ("all_1e308", [1e308; 21]),
        ("mixed_negative_large", {
            let mut a = [1e308; 21];
            a[0] = -1.0;
            a
        }),
        ("minimised counterexample, original scale", {
            let mut a = [0.0; 21];
            a[0] = -7.842355485875134e153;
            a[1] = -5.4501313297469415e153;
            a[2] = -5.595052342755576e153;
            a
        }),
    ];
    for (_name, nm) in inputs {
        let _ = calc_slopes_n_only(&nm);
        let _ = pattern(&nm);
    }
}

#[test]
fn non_finite_input_returns_without_panicking() {
    for band in [0, 5, 20] {
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut nm = [1.0; 21];
            nm[band] = v;
            // Output values are unspecified; the call must return.
            let _ = calc_slopes_n_only(&nm);
            let _ = pattern(&nm);
        }
    }
}

// ---- Feature: core loudness (main_loudness) ----

#[test]
fn a_low_band_above_120_db_is_rejected_and_120_db_is_accepted() {
    for band in 0..11 {
        let mut frame = [60.0; 28];
        frame[band] = 120.0;
        assert!(
            main_loudness(&frame, FieldType::Free).is_ok(),
            "band {band} at 120 dB"
        );
        frame[band] = 120.0_f64.next_up();
        assert_eq!(
            main_loudness(&frame, FieldType::Free),
            Err(SoneflowError::LevelExceeds120dB),
            "band {band} above 120 dB"
        );
    }
}

#[test]
fn bands_from_the_twelfth_up_are_not_limited_to_120_db() {
    for band in 11..28 {
        let mut frame = [60.0; 28];
        frame[band] = 130.0;
        assert!(
            main_loudness(&frame, FieldType::Free).is_ok(),
            "band {band}"
        );
    }
}

#[test]
fn levels_far_below_threshold_give_zero_core_loudness() {
    for field in [FieldType::Free, FieldType::Diffuse] {
        assert_eq!(main_loudness(&[-60.0; 28], field).unwrap(), [0.0; 21]);
    }
}

#[test]
fn core_loudness_is_never_negative() {
    // Backs the F-R7-1 reachability claim: the slope stage never receives a
    // negative core loudness from the main loudness stage.
    for frame in core_corpus::levels_corpus() {
        for field in [FieldType::Free, FieldType::Diffuse] {
            if let Ok(nm) = main_loudness(&frame, field) {
                assert!(nm.iter().all(|&v| v >= 0.0), "{field:?} {frame:?}");
            }
        }
    }
}

#[test]
fn raising_one_band_never_lowers_any_core_loudness() {
    for field in [FieldType::Free, FieldType::Diffuse] {
        for band in 0..28 {
            let mut prev = [0.0; 21];
            for step in 0..=280 {
                let mut frame = [50.0; 28];
                frame[band] = -20.0 + 0.5 * step as f64;
                let nm = main_loudness(&frame, field).unwrap();
                for k in 0..21 {
                    assert!(
                        nm[k] >= prev[k],
                        "{field:?} band {band} at {} dB lowers core band {k}",
                        frame[band]
                    );
                }
                prev = nm;
            }
        }
    }
}
