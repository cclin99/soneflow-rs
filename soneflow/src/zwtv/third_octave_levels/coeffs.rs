//! Per-band coefficients of the ISO 532-1 Annex A third-octave filter bank.

use crate::dsp::sos::Sos;
use crate::tables::TOB_DELTA;
use crate::zwtv::grid::FS;

const TINY: f64 = 1e-12;
const REF_INTENSITY: f64 = 4e-10;

/// Reference-filter b coefficients per section (ISO 532-1 Table A.1).
pub const TOB_B: [[f64; 3]; 3] = [[1.0, 2.0, 1.0], [1.0, 0.0, -1.0], [1.0, -2.0, 1.0]];

/// Feedback coefficients `(a1, a2)` of one biquad section: the reference
/// filter's `(-2, 1)` minus the band's Table A.1 deviation.
#[inline]
pub(crate) fn feedback_coeffs(band: usize, section: usize) -> (f64, f64) {
    (
        -2.0 - TOB_DELTA[band][section][0],
        1.0 - TOB_DELTA[band][section][1],
    )
}

pub(crate) fn band_sos(band: usize) -> [Sos; 3] {
    std::array::from_fn(|section| {
        let (a1, a2) = feedback_coeffs(band, section);
        Sos {
            b: TOB_B[section],
            a: [a1, a2],
        }
    })
}

pub fn center_frequency(band: usize) -> f64 {
    10.0_f64.powf((band as f64 - 16.0) / 10.0) * 1000.0
}

/// `(b0, a1)` of the three cascaded one-pole smoothers of a band.
pub fn smoothing_coeff(band: usize) -> (f64, f64) {
    let fc = center_frequency(band);
    let tau = 2.0 / (3.0 * fc.min(1000.0));
    let a1 = (-1.0 / (FS * tau)).exp();
    (1.0 - a1, a1)
}

/// Smoothed squared band signal to level in dB.
#[inline]
pub(crate) fn intensity_to_db(v: f64) -> f64 {
    10.0 * ((v + TINY) / REF_INTENSITY).log10()
}
