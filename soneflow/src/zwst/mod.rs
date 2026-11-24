use crate::core::bark_axis::bark_axis;
use crate::core::calc_slopes::calc_slopes;
use crate::core::main_loudness::main_loudness;
use crate::dsp::filtfilt::decimate;
use crate::dsp::sos::sosfilt;
use crate::error::SoneflowError;
use crate::tables_noct::{NOCT_DECIM_Q, NOCT_SOS};
use crate::types::{FieldType, LoudnessStationary};
mod checked;

/// 1/3-octave band RMS amplitudes, 28 bands (24 Hz..12.6 kHz), fs = 48 kHz.
/// Mirrors mosqito noct_spectrum for a single-channel stationary signal.
pub fn noct_spectrum_rms(signal: &[f64]) -> Vec<f64> {
    (0..28)
        .map(|band| {
            let q = NOCT_DECIM_Q[band];
            let mut x = if q > 1 {
                decimate(signal, q)
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
pub fn spec_to_db(spec: &[f64]) -> Vec<f64> {
    spec.iter()
        .map(|&a| {
            let a = if a == 0.0 { 2e-12 } else { a };
            20.0 * (a / 2e-5).log10()
        })
        .collect()
}

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
