//! ISO 532-1 Annex A third-octave filter bank: 28 bands filtered at 48 kHz,
//! squared, smoothed and sampled every `DEC_FACTOR` samples as levels in dB,
//! returned row-major `(28, n_time)`.
//!
//! `coeffs` holds the band coefficients, `scalar` and `avx2` the two kernels;
//! this module only picks the backend.

#![cfg_attr(
    any(test, feature = "internals"),
    expect(
        clippy::undocumented_unsafe_blocks,
        reason = "unsafe blocks from 0.1.0 without SAFETY comments; remove once documented"
    )
)]

#[cfg(target_arch = "x86_64")]
mod avx2;
mod coeffs;
mod scalar;

#[cfg(target_arch = "x86_64")]
#[cfg(any(test, feature = "internals"))]
pub use avx2::third_octave_levels_avx2;
#[cfg(target_arch = "x86_64")]
pub(crate) use avx2::TolGroupState;
pub(crate) use coeffs::intensity_to_db;
#[cfg(feature = "internals")]
pub use coeffs::{center_frequency, smoothing_coeff, TOB_B};
#[cfg(feature = "internals")]
pub use scalar::third_octave_levels_scalar;
pub(crate) use scalar::TolBandState;

pub use crate::zwtv::grid::time_axis;
#[cfg(feature = "internals")]
pub use crate::zwtv::grid::DEC_FACTOR;
#[cfg(any(test, feature = "internals"))]
use crate::zwtv::par::ParMode;

#[cfg(any(test, feature = "internals"))]
pub(crate) fn third_octave_levels_with_mode_and_backend(
    sig: &[f64],
    mode: ParMode,
    avx2: bool,
) -> (Vec<f64>, usize) {
    #[cfg(not(target_arch = "x86_64"))]
    let _ = avx2;
    #[cfg(target_arch = "x86_64")]
    if avx2 {
        return unsafe { third_octave_levels_avx2(sig, mode) };
    }
    scalar::third_octave_levels_scalar_impl(sig, mode)
}

#[cfg(any(test, feature = "internals"))]
pub fn third_octave_levels_with_mode(sig: &[f64], mode: ParMode) -> (Vec<f64>, usize) {
    third_octave_levels_with_mode_and_backend(sig, mode, crate::simd::use_avx2())
}

#[cfg(feature = "internals")]
pub fn third_octave_levels(sig: &[f64]) -> (Vec<f64>, usize) {
    third_octave_levels_with_mode(sig, ParMode::Rayon)
}
