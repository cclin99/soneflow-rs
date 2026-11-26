use crate::dsp::checked;
use crate::dsp::sos::Sos;
use crate::error::{NumericFaultReason, NumericFaultStage, SoneflowError};
use crate::tables_noct::{CHEBY_QS, CHEBY_SOS, NOCT_DECIM_Q, NOCT_SOS};

/// Anti-alias filter for decimation by `q`: scipy.signal.decimate's default
/// Chebyshev-I order-8 design, baked for the factors in `NOCT_DECIM_Q`.
pub(super) fn cheby_sos(q: usize) -> &'static [Sos] {
    let index = CHEBY_QS
        .iter()
        .position(|&c| c == q)
        .unwrap_or_else(|| panic!("no baked Chebyshev SOS for q={q}"));
    &CHEBY_SOS[index]
}

fn fault(index: usize, band: usize) -> SoneflowError {
    SoneflowError::InternalNumericFault {
        stage: NumericFaultStage::Filter,
        reason: NumericFaultReason::NonFinite,
        index,
        band: Some(band),
    }
}

fn band_rms(signal: &[f64], band: usize) -> Result<f64, usize> {
    let q = NOCT_DECIM_Q[band];
    let mut x = if q > 1 {
        checked::decimate_with_sos(cheby_sos(q), signal, q)?
    } else {
        signal.to_vec()
    };
    let sections = &NOCT_SOS[band];
    checked::sos(sections, &mut x, &mut vec![[0.0; 2]; sections.len()]).map_err(|i| i * q)?;
    let mut sum = 0.0;
    for (i, value) in x.iter().enumerate() {
        let square = value * value;
        if !square.is_finite() {
            return Err(i * q);
        }
        sum += square;
        if !sum.is_finite() {
            return Err(i * q);
        }
    }
    let mean = sum / x.len() as f64;
    let rms = mean.sqrt();
    if !mean.is_finite() || !rms.is_finite() {
        return Err(0);
    }
    Ok(rms)
}

/// Errors use a common original-rate logical sample coordinate. Reverse-pass
/// positions are un-reversed and padding maps to the nearest endpoint. This
/// identifies the computation location, not a causal source in zero-phase DSP.
/// All bands are checked; the lowest (sample index, band) wins.
pub(super) fn spectrum(signal: &[f64]) -> Result<Vec<f64>, SoneflowError> {
    let mut values = Vec::with_capacity(28);
    let mut first = None;
    for band in 0..28 {
        match band_rms(signal, band) {
            Ok(value) => values.push(value),
            Err(index) => {
                if first.is_none_or(|old| (index, band) < old) {
                    first = Some((index, band));
                }
                values.push(0.0);
            }
        }
    }
    if let Some((index, band)) = first {
        return Err(fault(index, band));
    }
    Ok(values)
}

pub(super) fn levels(spec: &[f64]) -> Result<Vec<f64>, SoneflowError> {
    spec.iter()
        .enumerate()
        .map(|(band, &a)| {
            let a = if a == 0.0 { 2e-12 } else { a };
            let ratio = a / 2e-5;
            let level = 20.0 * ratio.log10();
            if !a.is_finite() || !ratio.is_finite() || !level.is_finite() {
                Err(fault(0, band))
            } else {
                Ok(level)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_decimation_preserves_normal_bits() {
        let x: Vec<_> = (0..4800)
            .map(|i| ((i % 71) as f64 - 35.0) * 0.001)
            .collect();
        for &q in &CHEBY_QS {
            let sections = cheby_sos(q);
            let old = crate::dsp::filtfilt::decimate_with_sos(sections, &x, q);
            let new = checked::decimate_with_sos(sections, &x, q).unwrap();
            assert_eq!(
                old.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                new.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn spectrum_and_levels_match_original_bits() {
        for amp in [0.0, 0.00001, 0.02] {
            let signal: Vec<_> = (0..4800).map(|i| ((i % 71) as f64 - 35.0) * amp).collect();
            let old = super::super::noct_spectrum_rms(&signal);
            let new = spectrum(&signal).unwrap();
            assert_eq!(
                old.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                new.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            );
            assert_eq!(
                super::super::spec_to_db(&old)
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                levels(&new)
                    .unwrap()
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>()
            );
        }
    }
}
