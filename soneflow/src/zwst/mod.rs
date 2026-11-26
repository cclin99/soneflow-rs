use crate::core::bark_axis::bark_axis;
use crate::core::calc_slopes::calc_slopes;
use crate::core::main_loudness::main_loudness;
#[cfg(any(test, feature = "internals"))]
use crate::dsp::filtfilt::decimate_with_sos;
#[cfg(any(test, feature = "internals"))]
use crate::dsp::sos::sosfilt;
use crate::error::SoneflowError;
#[cfg(any(test, feature = "internals"))]
use crate::tables_noct::{NOCT_DECIM_Q, NOCT_SOS};
use crate::types::{FieldType, LoudnessStationary};
mod checked;

/// 1/3-octave band RMS amplitudes, 28 bands (24 Hz..12.6 kHz), fs = 48 kHz.
/// Mirrors mosqito noct_spectrum for a single-channel stationary signal.
#[cfg(any(test, feature = "internals"))]
pub fn noct_spectrum_rms(signal: &[f64]) -> Vec<f64> {
    (0..28)
        .map(|band| {
            let q = NOCT_DECIM_Q[band];
            let mut x = if q > 1 {
                decimate_with_sos(checked::cheby_sos(q), signal, q)
            } else {
                signal.to_vec()
            };
            sosfilt(&NOCT_SOS[band], &mut x);
            let ms = x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64;
            ms.sqrt()
        })
        .collect()
}

/// Convert pressure amplitudes to dB SPL with ref 2e-5, matching mosqito amp2db.
#[cfg(any(test, feature = "internals"))]
pub fn spec_to_db(spec: &[f64]) -> Vec<f64> {
    spec.iter()
        .map(|&a| {
            let a = if a == 0.0 { 2e-12 } else { a };
            20.0 * (a / 2e-5).log10()
        })
        .collect()
}

/// Stationary loudness (ISO 532-1 Zwicker, ZWST) of a 48 kHz pressure signal in Pa.
///
/// Input contract: `fs == 48000.0`, at least 4800 samples, every sample
/// finite; otherwise `UnsupportedSampleRate`, `SignalTooShort` or
/// `NonFiniteInput` is returned and nothing is computed. A third-octave level
/// above 120 dB below 300 Hz is `LevelExceeds120dB`.
///
/// # Examples
///
/// A 1 kHz tone at 40 dB SPL in a free field is close to 1 sone (40 phon):
///
/// ```
/// use soneflow::{loudness_zwst, FieldType};
/// use std::f64::consts::{PI, SQRT_2};
///
/// let amplitude = 2e-5 * 100.0 * SQRT_2; // 40 dB SPL re 20 µPa, as a sine peak
/// let signal: Vec<f64> = (0..24_000)
///     .map(|i| amplitude * (2.0 * PI * 1000.0 * i as f64 / 48_000.0).sin())
///     .collect();
/// let result = loudness_zwst(&signal, 48_000.0, FieldType::Free)?;
/// assert!((result.n - 1.0).abs() < 0.1, "N = {} sone", result.n);
/// assert_eq!(result.n_specific.len(), 240); // 0.1 to 24 Bark
/// # Ok::<(), soneflow::SoneflowError>(())
/// ```
pub fn loudness_zwst(
    signal: &[f64],
    fs: f64,
    field: FieldType,
) -> Result<LoudnessStationary, SoneflowError> {
    SoneflowError::check_batch_input(signal, fs)?;
    crate::batch_guard::environment()?;
    let spec = checked::spectrum(signal)?;
    let spec_db = checked::levels(&spec)?;
    let nm = main_loudness(&spec_db, field)?;
    crate::batch_guard::core(&nm, 0)?;
    let (n, n_specific) = calc_slopes(&nm);
    crate::batch_guard::bounded(
        n,
        crate::numeric_guard::TW_INPUT_MAX,
        crate::error::NumericFaultStage::Slope,
        0,
        None,
    )?;
    for (band, &value) in n_specific.iter().enumerate() {
        crate::batch_guard::bounded(
            value,
            crate::numeric_guard::CORE_MAX,
            crate::error::NumericFaultStage::Slope,
            0,
            Some(band),
        )?;
    }
    Ok(LoudnessStationary {
        n,
        n_specific: n_specific.to_vec(),
        bark_axis: bark_axis(),
    })
}
