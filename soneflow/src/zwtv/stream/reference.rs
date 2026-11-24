//! Offline reference for the stream: the same stages run over a whole signal
//! from zero state, without the stream's chunking. Test support only.

use super::denormal::DenormalGuard;
use super::nl_stage::NlStage;
use crate::core::calc_slopes::calc_slopes_n_only;
use crate::core::main_loudness::main_loudness_clamped;
use crate::types::FieldType;
use crate::zwtv::grid::OUT_DECIM;
use crate::zwtv::par::ParMode;
use crate::zwtv::temporal_weighting::TwState;
use crate::zwtv::third_octave_levels::third_octave_levels_with_mode_and_backend;

#[doc(hidden)]
pub fn zwtv_reference_zerostate(signal: &[f64], field: FieldType) -> Vec<f64> {
    let _guard = DenormalGuard::new();
    let avx2 = crate::simd::use_avx2();
    let core = core_frames(signal, field, avx2);
    let mut nl = NlStage::new(avx2);
    let mut tw = TwState::new();
    let mut out = Vec::with_capacity(core.len().div_ceil(OUT_DECIM));
    for (t, frame) in core.iter().enumerate() {
        let next = core.get(t + 1).copied().unwrap_or([0.0; 21]);
        let n = tw.advance(calc_slopes_n_only(&nl.advance(frame, &next)));
        if t % OUT_DECIM == 0 {
            out.push(n);
        }
    }
    out
}

/// Time-major core loudness of every 2 kHz frame.
fn core_frames(signal: &[f64], field: FieldType, avx2: bool) -> Vec<[f64; 21]> {
    let (tol, n_time) =
        third_octave_levels_with_mode_and_backend(signal, ParMode::Sequential, avx2);
    (0..n_time)
        .map(|t| {
            let frame = std::array::from_fn(|band| tol[band * n_time + t]);
            main_loudness_clamped(&frame, field).0
        })
        .collect()
}
