mod common;
use common::*;
use soneflow::internals::core::calc_slopes::calc_slopes;
use soneflow::internals::core::main_loudness::main_loudness;
use soneflow::{FieldType, SoneflowError};

const SIGNALS: [&str; 4] = ["sine_1k_60", "sine_250_80", "sine_4k_60", "white_60"];

/// Core loudness against MoSQITo (binary64 constants) is a compatibility
/// check: this crate computes with single-precision constants (0.0635, A0,
/// DDF, DCB, 0.4, 0.32), measured up to 1.553e-7 relative here (sine_4k_60
/// diffuse, near threshold).
const MOSQITO_CORE_REL_TOL: f64 = 5e-7;

#[test]
fn main_loudness_matches_mosqito_free_and_diffuse_core_loudness() {
    for sig in SIGNALS {
        let spec = read_bin(sig, "spec_third_db.bin");
        let got = main_loudness(&spec, FieldType::Free).unwrap();
        let want = read_bin(sig, "nm_free.bin");
        assert_close(
            &got,
            &want,
            MOSQITO_CORE_REL_TOL,
            1e-15,
            &format!("{sig}/nm free"),
        );

        let got_d = main_loudness(&spec, FieldType::Diffuse).unwrap();
        let want_d = read_bin(sig, "nm_diffuse.bin");
        assert_close(
            &got_d,
            &want_d,
            MOSQITO_CORE_REL_TOL,
            1e-15,
            &format!("{sig}/nm diffuse"),
        );
    }
}

#[test]
fn main_loudness_rejects_low_frequency_third_octave_level_above_120_db() {
    let mut spec = vec![50.0; 28];
    spec[3] = 121.0;
    assert_eq!(
        main_loudness(&spec, FieldType::Free).unwrap_err(),
        SoneflowError::LevelExceeds120dB
    );
}

/// The slope stage follows ISO 532-1 Annex A (no per-frame rounding of the
/// total, every band completed, the specific value where a slope ends inside a
/// cell). Against MoSQITo the total is a compatibility check (相容性對照) with
/// a bound of 5e-3 sone and the specific loudness is recorded, not gated.
#[test]
fn calc_slopes_compat_mosqito_total_and_specific_loudness() {
    for sig in SIGNALS {
        let nm_v = read_bin(sig, "nm_free.bin");
        let nm: [f64; 21] = nm_v.as_slice().try_into().unwrap();
        let want_n = read_bin(sig, "N.bin")[0];
        let want_spec = read_bin(sig, "N_specific.bin");
        let (n, spec) = calc_slopes(&nm);
        assert!(
            (n - want_n).abs() <= 5e-3,
            "{sig}: N {n} vs {want_n} (compatibility)"
        );
        record_max_abs_diff(&spec, &want_spec, &format!("{sig}/N_specific vs MoSQITo"));
    }
}

#[path = "common/core_corpus.rs"]
mod core_corpus;

/// R7 refactor guard (not frozen: `main_loudness` uses `powf` / `log10`, so
/// the hashes belong to the platform libm). Run on the baseline worktree and
/// on the refactored tree and `diff` the outputs; any difference is a
/// behaviour change.
#[test]
#[ignore = "manual helper: bitwise core snapshot for refactor verification"]
fn dump_core_snapshot() {
    let levels = core_corpus::levels_corpus();
    for field in [FieldType::Free, FieldType::Diffuse] {
        let mut values = Vec::with_capacity(21 * levels.len());
        let mut errors = 0usize;
        for frame in &levels {
            match main_loudness(frame, field) {
                Ok(nm) => values.extend_from_slice(&nm),
                Err(_) => errors += 1,
            }
        }
        println!(
            "main_loudness {field:?}: frames={} errors={errors} nm={:016x}",
            levels.len(),
            fnv1a_f64(&values)
        );
    }
    let frames = core_corpus::slopes_corpus();
    let (mut totals, mut specs, mut n_only) = (Vec::new(), Vec::new(), Vec::new());
    for nm in &frames {
        let (n, spec) = calc_slopes(nm);
        totals.push(n);
        specs.extend_from_slice(&spec);
        n_only.push(soneflow::internals::core::calc_slopes::calc_slopes_n_only(
            nm,
        ));
    }
    println!(
        "calc_slopes: frames={} total={:016x} spec={:016x} n_only={:016x}",
        frames.len(),
        fnv1a_f64(&totals),
        fnv1a_f64(&specs),
        fnv1a_f64(&n_only)
    );
}
