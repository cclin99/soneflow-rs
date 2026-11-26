//! Scalar filter-bank kernel: one band at a time.

#[cfg(any(test, feature = "internals"))]
use super::coeffs::intensity_to_db;
use super::coeffs::{band_sos, smoothing_coeff};
use crate::dsp::sos::Sos;
#[cfg(any(test, feature = "internals"))]
use crate::tables::N_TOB_BANDS;
use crate::tables::TOB_GAIN;
#[cfg(any(test, feature = "internals"))]
use crate::zwtv::grid::DEC_FACTOR;
#[cfg(any(test, feature = "internals"))]
use crate::zwtv::par::{chunks_dispatch, ParMode};

/// Filter and smoother state of one band, advanced one 48 kHz sample at a time.
pub(crate) struct TolBandState {
    sos: [Sos; 3],
    z: [[f64; 2]; 3],
    gain: f64,
    sb0: f64,
    sa1: f64,
    sm: [f64; 3],
}

impl TolBandState {
    /// Failure-only inspection; it never advances or repairs DSP state.
    pub(crate) fn numeric_reason_mask(&self) -> u32 {
        use crate::numeric_guard::{classify_nonnegative_bounded, DomainFault};
        let delays = if self.z.iter().flatten().any(|v| !v.is_finite()) {
            DomainFault::NonFinite.reason_mask()
        } else {
            0
        };
        self.sm
            .iter()
            .filter_map(|&v| classify_nonnegative_bounded(v, f64::MAX).err())
            .fold(delays, |mask, error| mask | error.reason_mask())
    }
    #[inline]
    pub(crate) fn validate_numeric(&self) -> Result<(), crate::numeric_guard::DomainFault> {
        use crate::numeric_guard::{classify_nonnegative_bounded, DomainFault};
        for value in self.z.iter().flatten() {
            if !value.is_finite() {
                return Err(DomainFault::NonFinite);
            }
        }
        for &value in &self.sm {
            classify_nonnegative_bounded(value, f64::MAX)?;
        }
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn advance_checked(
        &mut self,
        sample: f64,
    ) -> Result<f64, crate::numeric_guard::DomainFault> {
        use crate::numeric_guard::{classify_nonnegative_bounded, DomainFault};
        self.validate_numeric()?;
        let mut v = sample * self.gain;
        if !v.is_finite() {
            return Err(DomainFault::NonFinite);
        }
        for (s, z) in self.sos.iter().zip(self.z.iter_mut()) {
            let xin = v;
            let y = s.b[0] * xin + z[0];
            z[0] = s.b[1] * xin - s.a[0] * y + z[1];
            z[1] = s.b[2] * xin - s.a[1] * y;
            if !y.is_finite() || !z[0].is_finite() || !z[1].is_finite() {
                return Err(DomainFault::NonFinite);
            }
            v = y;
        }
        v *= v;
        classify_nonnegative_bounded(v, f64::MAX)?;
        for stage in &mut self.sm {
            *stage = self.sb0 * v + self.sa1 * *stage;
            classify_nonnegative_bounded(*stage, f64::MAX)?;
            v = *stage;
        }
        Ok(v)
    }

    pub(crate) fn new(band: usize) -> Self {
        let (sb0, sa1) = smoothing_coeff(band);
        Self {
            sos: band_sos(band),
            z: [[0.0; 2]; 3],
            gain: TOB_GAIN[band],
            sb0,
            sa1,
            sm: [0.0; 3],
        }
    }

    pub(crate) fn reset(&mut self) {
        self.z = [[0.0; 2]; 3];
        self.sm = [0.0; 3];
    }

    /// Bit-exact duplicate of creation coefficients and numeric state without extra FP arithmetic.
    #[inline]
    pub(crate) fn copy_exact(&self) -> Self {
        Self {
            sos: self.sos,
            z: self.z,
            gain: self.gain,
            sb0: self.sb0,
            sa1: self.sa1,
            sm: self.sm,
        }
    }

    /// Synchronize only the 9 numeric state fields (`z` + `sm`) by raw bits;
    /// creation coefficients (`sos`, `gain`, `sb0`, `sa1`) are never modified.
    #[inline(always)]
    pub(crate) fn sync_numeric_from(&mut self, src: &Self) {
        self.z = src.z;
        self.sm = src.sm;
    }

    #[cfg(test)]
    pub(crate) fn coeff_bits(&self) -> [u64; 18] {
        let mut out = [0u64; 18];
        let mut idx = 0;
        for s in &self.sos {
            for &b in &s.b {
                out[idx] = b.to_bits();
                idx += 1;
            }
            for &a in &s.a {
                out[idx] = a.to_bits();
                idx += 1;
            }
        }
        out[15] = self.gain.to_bits();
        out[16] = self.sb0.to_bits();
        out[17] = self.sa1.to_bits();
        out
    }

    #[cfg(test)]
    pub(crate) fn inject_nan_for_test(&mut self) {
        self.z[0][0] = f64::NAN;
    }

    #[cfg(test)] // c9_stream_hook
    pub(crate) fn inject_raw_bits(&mut self, field: usize, bits: u64) {
        assert!(field < 9, "TOL scalar field out of range: {field}");
        let value = f64::from_bits(bits);
        if field < 6 {
            self.z[field / 2][field % 2] = value;
        } else {
            self.sm[field - 6] = value;
        }
    }

    #[cfg(test)]
    pub(crate) fn numeric_bits(&self) -> [u64; 9] {
        [
            self.z[0][0].to_bits(),
            self.z[0][1].to_bits(),
            self.z[1][0].to_bits(),
            self.z[1][1].to_bits(),
            self.z[2][0].to_bits(),
            self.z[2][1].to_bits(),
            self.sm[0].to_bits(),
            self.sm[1].to_bits(),
            self.sm[2].to_bits(),
        ]
    }

    #[inline]
    pub(crate) fn advance(&mut self, sample: f64) -> f64 {
        let mut v = sample * self.gain;
        for (s, z) in self.sos.iter().zip(self.z.iter_mut()) {
            let xin = v;
            let y = s.b[0] * xin + z[0];
            z[0] = s.b[1] * xin - s.a[0] * y + z[1];
            z[1] = s.b[2] * xin - s.a[1] * y;
            v = y;
        }
        v *= v;
        for stage in &mut self.sm {
            *stage = self.sb0 * v + self.sa1 * *stage;
            v = *stage;
        }
        v
    }
}

#[cfg(any(test, feature = "internals"))]
fn tol_band_scalar(sig: &[f64], band: usize, out_row: &mut [f64]) {
    let mut state = TolBandState::new(band);
    let mut frame = 0;
    for (i, &sample) in sig.iter().enumerate() {
        let v = state.advance(sample);
        if i % DEC_FACTOR == 0 {
            out_row[frame] = intensity_to_db(v);
            frame += 1;
        }
    }
}

#[cfg(feature = "internals")]
pub fn third_octave_levels_scalar(sig: &[f64]) -> (Vec<f64>, usize) {
    third_octave_levels_scalar_impl(sig, ParMode::Sequential)
}

#[cfg(any(test, feature = "internals"))]
pub(super) fn third_octave_levels_scalar_impl(sig: &[f64], mode: ParMode) -> (Vec<f64>, usize) {
    let n_time = sig.len().div_ceil(DEC_FACTOR);
    let mut out = vec![0.0; N_TOB_BANDS * n_time];
    if n_time == 0 {
        return (out, 0);
    }

    chunks_dispatch(&mut out, n_time, mode, |band, out_row| {
        tol_band_scalar(sig, band, out_row)
    });

    (out, n_time)
}

#[cfg(test)]
mod checked_tests {
    use super::*;
    use crate::numeric_guard::DomainFault;
    #[test]
    fn persistent_delays_and_smoothers_checked_before_update() {
        for index in 0..6 {
            let mut state = TolBandState::new(0);
            state.z[index / 2][index % 2] = f64::NAN;
            assert_eq!(state.advance_checked(0.0), Err(DomainFault::NonFinite));
        }
        for index in 0..3 {
            let mut state = TolBandState::new(0);
            state.sm[index] = f64::from_bits(0x8000000000000001);
            assert_eq!(state.advance_checked(0.0), Err(DomainFault::Negative));
        }
        let mut state = TolBandState::new(0);
        state.z[0][0] = -0.1;
        assert!(state.validate_numeric().is_ok());
    }
    #[test]
    fn checked_filter_preserves_normal_bits_and_reports_overflow() {
        for band in 0..28 {
            let mut original = TolBandState::new(band);
            let mut checked = TolBandState::new(band);
            for i in 0..200 {
                let x = (i as f64 * 0.37).sin() * 0.1;
                assert_eq!(
                    original.advance(x).to_bits(),
                    checked.advance_checked(x).unwrap().to_bits()
                );
            }
            assert!(checked.advance_checked(f64::MAX).is_err());
        }
    }
}
