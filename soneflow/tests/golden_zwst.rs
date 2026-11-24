//! MoSQITo 1.2.1 goldens. The stage tests (noct spectrum, dB conversion, main
//! loudness) stay tight. The slope stage follows ISO 532-1 Annex A (no
//! per-frame rounding of the total, every band completed), so the total
//! loudness comparisons below are a compatibility check against MoSQITo
//! (相容性對照) with a bound of 5e-3 sone. Where a slope ends inside a cell
//! this crate writes the core loudness, or the segment that contains the
//! sample, so the specific loudness differs from MoSQITo by up to ~4e-2
//! sone/Bark by design; it is recorded, not gated.

mod common;
use common::*;

/// MoSQITo compatibility bound for the total loudness, sone.
const COMPAT_TOL: f64 = 5e-3;
use soneflow::core::calc_slopes::calc_slopes;
use soneflow::core::main_loudness::main_loudness;
use soneflow::zwst::{noct_spectrum_rms, spec_to_db};
use soneflow::{loudness_zwst, FieldType, SoneflowError};

#[test]
fn noct_spectrum_matches_mosqito() {
    for sig in ["sine_1k_60", "sine_250_80", "white_60"] {
        let x = read_bin(sig, "sig.bin");
        let want = read_bin(sig, "spec_third_amp.bin");
        let got = noct_spectrum_rms(&x);
        assert_close(&got, &want, 1e-7, 1e-12, &format!("{sig}/noct"));
    }
}

#[test]
fn spec_to_db_matches_mosqito_amp2db() {
    for sig in ["sine_1k_60", "sine_250_80", "white_60"] {
        let spec = read_bin(sig, "spec_third_amp.bin");
        let want = read_bin(sig, "spec_third_db.bin");
        let got = spec_to_db(&spec);
        assert_close(&got, &want, 1e-12, 1e-12, &format!("{sig}/spec_to_db"));
    }
}

/// Core loudness against MoSQITo (binary64 constants) is a compatibility
/// check: this crate computes with single-precision constants (0.0635, A0,
/// DDF, DCB, 0.4, 0.32), measured up to 1.553e-7 relative here (sine_4k_60
/// diffuse, near threshold).
const MOSQITO_CORE_REL_TOL: f64 = 5e-7;

#[test]
fn main_loudness_matches_mosqito() {
    for sig in ["sine_1k_60", "sine_250_80", "sine_4k_60", "white_60"] {
        let spec_db = read_bin(sig, "spec_third_db.bin");
        let want = read_bin(sig, "nm_free.bin");
        let got = main_loudness(&spec_db, FieldType::Free).unwrap();
        assert_close(
            &got,
            &want,
            MOSQITO_CORE_REL_TOL,
            1e-15,
            &format!("{sig}/nm free"),
        );
        let want_d = read_bin(sig, "nm_diffuse.bin");
        let got_d = main_loudness(&spec_db, FieldType::Diffuse).unwrap();
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
fn main_loudness_rejects_over_120db() {
    let mut spec = vec![50.0; 28];
    spec[3] = 121.0;
    assert_eq!(
        main_loudness(&spec, FieldType::Free).unwrap_err(),
        SoneflowError::LevelExceeds120dB
    );
}

#[test]
fn calc_slopes_compat_mosqito() {
    for sig in ["sine_1k_60", "sine_250_80", "sine_4k_60", "white_60"] {
        let nm_v = read_bin(sig, "nm_free.bin");
        let nm: [f64; 21] = nm_v.as_slice().try_into().unwrap();
        let want_n = read_bin(sig, "N.bin")[0];
        let want_spec = read_bin(sig, "N_specific.bin");
        let (n, spec) = calc_slopes(&nm);
        assert!(
            (n - want_n).abs() <= COMPAT_TOL,
            "{sig}: N {n} vs {want_n} (compatibility)"
        );
        record_max_abs_diff(&spec, &want_spec, &format!("{sig}/N_specific vs MoSQITo"));
    }
}

#[test]
fn zwst_end_to_end_compat_mosqito() {
    for sig in [
        "sine_1k_60",
        "sine_250_80",
        "sine_4k_60",
        "white_60",
        "annexb_sig3",
        "annexb_sig5",
    ] {
        let x = read_bin(sig, "sig.bin");
        let want_n = read_bin(sig, "N.bin")[0];
        let want_spec = read_bin(sig, "N_specific.bin");
        let r = loudness_zwst(&x, 48000.0, FieldType::Free).unwrap();
        assert!(
            (r.n - want_n).abs() <= COMPAT_TOL,
            "{sig}: N {} vs {want_n} (compatibility)",
            r.n
        );
        record_max_abs_diff(&r.n_specific, &want_spec, &format!("{sig}/spec vs MoSQITo"));
        assert_eq!(r.bark_axis.len(), 240);
    }
}
