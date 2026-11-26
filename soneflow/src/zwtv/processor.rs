//! Offline (batch) time-varying loudness with reusable scratch buffers.

use super::batch_checked;
use super::grid::decimate_to_output_grid;
use super::nonlinear_decay::NlInit;
use super::par::ParMode;
use super::slopes::{slopes_into_d0, specific_bark_major};
use super::third_octave_levels::time_axis;
use crate::core::bark_axis::bark_axis;
use crate::core::main_loudness::main_loudness_frames_into;
use crate::error::SoneflowError;
use crate::types::{FieldType, LoudnessTimeVarying};

const N_TOB: usize = 28;
const N_CORE: usize = 21;

/// Batch time-varying loudness that keeps its scratch buffers between calls.
///
/// A reused processor gives the same result as a new one and as
/// [`loudness_zwtv`]; it only saves the allocations.
///
/// # Examples
///
/// ```
/// use soneflow::{loudness_zwtv, FieldType, ZwtvProcessor};
///
/// let signal: Vec<f64> = (0..9_600).map(|i| 0.02 * (i as f64 * 0.13).sin()).collect();
/// let mut processor = ZwtvProcessor::new();
/// let first = processor.process(&signal, 48_000.0, FieldType::Free)?;
/// let again = processor.process(&signal, 48_000.0, FieldType::Free)?;
/// assert_eq!(first.n, again.n);
/// assert_eq!(first.n, loudness_zwtv(&signal, 48_000.0, FieldType::Free)?.n);
/// # Ok::<(), soneflow::SoneflowError>(())
/// ```
#[derive(Debug, Default)]
pub struct ZwtvProcessor {
    third_octave_frames: Vec<f64>,
    core: Vec<f64>,
    loudness: Vec<f64>,
    spec_time_major: Vec<f64>,
}

impl ZwtvProcessor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Batch time-varying loudness with the nonlinear decay started from zero
    /// state (`NlInit::Zero`, ISO 532-1 Annex A). Same as `loudness_zwtv`.
    pub fn process(
        &mut self,
        signal: &[f64],
        fs: f64,
        field: FieldType,
    ) -> Result<LoudnessTimeVarying, SoneflowError> {
        self.process_with(signal, fs, field, NlInit::Zero)
    }

    /// Batch time-varying loudness with an explicit nonlinear-decay initial
    /// state. `NlInit::MosqitoTail` reproduces MoSQITo 1.2.1 and exists for
    /// golden comparisons; see `NlInit`.
    pub fn process_with(
        &mut self,
        signal: &[f64],
        fs: f64,
        field: FieldType,
        nl_init: NlInit,
    ) -> Result<LoudnessTimeVarying, SoneflowError> {
        #[cfg(test)]
        batch_checked::batch_hooks::begin_public_call();
        SoneflowError::check_batch_input(signal, fs)?;
        crate::batch_guard::environment()?;

        let (tol, n_time) = batch_checked::levels(signal, ParMode::Rayon)?;
        self.store_levels_time_major(&tol, n_time);
        self.core.resize(N_CORE * n_time, 0.0);
        main_loudness_frames_into(&self.third_octave_frames, n_time, field, &mut self.core)?;
        batch_checked::validate_core(&self.core, n_time)?;

        let nl =
            batch_checked::nonlinear_validated_core(&self.core, n_time, ParMode::Rayon, nl_init)?;
        let mut unqualified_slopes =
            slopes_into_d0(&nl, n_time, &mut self.loudness, &mut self.spec_time_major);
        batch_checked::validate_slopes_chunks(
            &self.loudness,
            &self.spec_time_major,
            &mut unqualified_slopes,
        )?;

        let filt = batch_checked::temporal(&self.loudness)?;
        let (n, time_axis) = decimate_to_output_grid(&filt, &time_axis(signal.len(), n_time));
        Ok(LoudnessTimeVarying {
            n_specific: specific_bark_major(&self.spec_time_major, time_axis.len()),
            n,
            bark_axis: bark_axis(),
            time_axis,
        })
    }

    /// Transpose band-major `(28, n_time)` levels into `third_octave_frames`
    /// as time-major `(n_time, 28)`.
    fn store_levels_time_major(&mut self, tol: &[f64], n_time: usize) {
        self.third_octave_frames.resize(N_TOB * n_time, 0.0);
        for t in 0..n_time {
            for band in 0..N_TOB {
                self.third_octave_frames[t * N_TOB + band] = tol[band * n_time + t];
            }
        }
    }

    #[cfg(test)]
    fn scratch_capacities(&self) -> (usize, usize, usize, usize) {
        (
            self.third_octave_frames.capacity(),
            self.core.capacity(),
            self.loudness.capacity(),
            self.spec_time_major.capacity(),
        )
    }
}

/// Time-varying loudness (ISO 532-1 Zwicker, ZWTV) of a 48 kHz pressure signal in Pa.
///
/// Output length is [`zwtv_out_frames`](super::zwtv_out_frames)`(signal.len())`
/// points on an exact 2 ms grid starting at 0 s (see that function for the
/// frozen output-grid contract). Input contract: `fs == 48000.0`, at least 4800
/// samples, every sample finite; otherwise `UnsupportedSampleRate`,
/// `SignalTooShort` or `NonFiniteInput` is returned and nothing is computed.
///
/// # Examples
///
/// ```
/// use soneflow::{loudness_zwtv, zwtv_out_frames, FieldType, SoneflowError};
///
/// let silence = vec![0.0; 9_600]; // 0.2 s
/// let result = loudness_zwtv(&silence, 48_000.0, FieldType::Free)?;
/// assert_eq!(result.n.len(), zwtv_out_frames(silence.len())); // 100 points
/// assert_eq!(result.time_axis[1], 0.002); // the 2 ms grid starts at 0 s
/// assert!(result.n.iter().all(|&n| n == 0.0));
///
/// // The input contract is checked before any work.
/// assert_eq!(
///     loudness_zwtv(&silence, 44_100.0, FieldType::Free).unwrap_err(),
///     SoneflowError::UnsupportedSampleRate(44_100.0)
/// );
/// # Ok::<(), SoneflowError>(())
/// ```
pub fn loudness_zwtv(
    signal: &[f64],
    fs: f64,
    field: FieldType,
) -> Result<LoudnessTimeVarying, SoneflowError> {
    ZwtvProcessor::new().process(signal, fs, field)
}

#[cfg(test)]
mod tests {
    use super::{loudness_zwtv, ZwtvProcessor};
    use crate::types::FieldType;
    use crate::zwtv::grid::zwtv_out_frames;

    #[test]
    fn zwtv_out_frames_matches_pipeline_output() {
        let signal: Vec<f64> = (0..48_000)
            .map(|i| (i % 480) as f64 / 480.0 * 0.02 - 0.01)
            .collect();
        for len in [4800usize, 4801, 4823, 4824, 4895, 4896, 4897, 48_000] {
            let result = loudness_zwtv(&signal[..len], 48_000.0, FieldType::Free).unwrap();
            assert_eq!(zwtv_out_frames(len), result.n.len(), "len={len}");
            assert_eq!(result.time_axis.len(), result.n.len(), "len={len}");
            assert_eq!(result.n_specific.len(), 240 * result.n.len(), "len={len}");
        }
    }

    #[test]
    fn processor_matches_free_function_and_reuses_scratch_capacity() {
        let signal: Vec<f64> = (0..48_000)
            .map(|i| (2.0 * std::f64::consts::PI * 1_000.0 * i as f64 / 48_000.0).sin() * 0.02)
            .collect();
        let mut processor = ZwtvProcessor::new();

        let first = processor
            .process(&signal, 48_000.0, FieldType::Free)
            .unwrap();
        let capacities = processor.scratch_capacities();
        let second = processor
            .process(&signal, 48_000.0, FieldType::Free)
            .unwrap();
        let expected = loudness_zwtv(&signal, 48_000.0, FieldType::Free).unwrap();

        assert_eq!(processor.scratch_capacities(), capacities);
        assert_eq!(second.n, first.n);
        assert_eq!(second.n_specific, first.n_specific);
        assert_eq!(second.n, expected.n);
        assert_eq!(second.n_specific, expected.n_specific);
    }
}
