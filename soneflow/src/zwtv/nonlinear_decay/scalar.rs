//! Scalar nonlinear-decay kernel: one band at a time.

#[cfg(any(test, feature = "internals"))]
use super::params::nl_coeffs;
use super::params::{NlInit, NL_EPS, NL_SUBSTEPS};
#[cfg(any(test, feature = "internals"))]
use crate::zwtv::par::{chunks_dispatch, ParMode};

#[cfg(any(test, feature = "internals"))]
pub fn nl_loudness_scalar(core: &[f64], n_time: usize) -> Vec<f64> {
    nl_loudness_scalar_with(core, n_time, NlInit::Zero)
}

#[cfg(any(test, feature = "internals"))]
pub fn nl_loudness_scalar_with(core: &[f64], n_time: usize, init: NlInit) -> Vec<f64> {
    nl_loudness_scalar_impl(core, n_time, ParMode::Sequential, init)
}

#[cfg(any(test, feature = "internals"))]
pub(super) fn nl_loudness_scalar_impl(
    core: &[f64],
    n_time: usize,
    mode: ParMode,
    init: NlInit,
) -> Vec<f64> {
    assert_eq!(
        core.len(),
        21 * n_time,
        "nl_loudness expects row-major (21, n_time) core loudness"
    );

    let b = nl_coeffs();
    let mut out = vec![0.0; core.len()];
    if n_time == 0 {
        return out;
    }

    chunks_dispatch(&mut out, n_time, mode, |band, out_row| {
        nl_loudness_band_scalar(&core[band * n_time..(band + 1) * n_time], out_row, &b, init);
    });

    out
}

/// The two capacitor voltages of one band after the previous sub-step
/// (ISO 532-1 Annex A).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NlBandState {
    prev_uo: f64,
    prev_u2: f64,
}

impl NlBandState {
    pub(crate) fn numeric_reason_mask(&self) -> u32 {
        use crate::numeric_guard::{classify_nonnegative_bounded, CORE_MAX};
        [self.prev_uo, self.prev_u2]
            .into_iter()
            .filter_map(|v| classify_nonnegative_bounded(v, CORE_MAX).err())
            .fold(0, |mask, error| mask | error.reason_mask())
    }
    #[cfg(test)]
    pub(crate) fn inject_numeric(&mut self, field: usize, value: f64) {
        match field {
            0 => self.prev_uo = value,
            1 => self.prev_u2 = value,
            _ => panic!("test field"),
        }
    }
    #[cfg(test)] // c9_batch_hook
    pub(crate) fn inject_raw_bits(&mut self, field: usize, bits: u64) {
        match field {
            0 => self.prev_uo = f64::from_bits(bits),
            1 => self.prev_u2 = f64::from_bits(bits),
            _ => panic!("NL scalar field out of range: {field}"),
        }
    }
    pub(crate) fn validate_numeric(&self) -> Result<(), crate::numeric_guard::DomainFault> {
        use crate::numeric_guard::{classify_nonnegative_bounded, CORE_MAX};
        classify_nonnegative_bounded(self.prev_uo, CORE_MAX)?;
        classify_nonnegative_bounded(self.prev_u2, CORE_MAX)
    }

    #[inline(always)]
    pub(crate) fn is_entry_valid(&self) -> bool {
        use crate::numeric_guard::{is_nonnegative_bounded, CORE_MAX};
        is_nonnegative_bounded(self.prev_uo, CORE_MAX)
            && is_nonnegative_bounded(self.prev_u2, CORE_MAX)
    }

    pub(crate) fn advance_frame_checked(
        &mut self,
        row_t: f64,
        next: f64,
        b: &[f64; 6],
    ) -> Result<f64, crate::numeric_guard::DomainFault> {
        use crate::numeric_guard::{classify_nonnegative_bounded, DomainFault, CORE_MAX};
        self.validate_numeric()?;
        classify_nonnegative_bounded(row_t, CORE_MAX)?;
        classify_nonnegative_bounded(next, CORE_MAX)?;
        let delta = (next - row_t) / NL_SUBSTEPS as f64;
        let mut out0 = 0.0;
        for k in 0..NL_SUBSTEPS {
            let ui = row_t + delta * k as f64;
            if !ui.is_finite() {
                return Err(DomainFault::NonFinite);
            }
            let uo = self.next_uo(ui, b);
            let u2 = self.next_u2(ui, uo, b);
            // Keep all selected values inspectable until the caller resets a
            // failed transaction; checked failure never promises state reuse.
            self.prev_uo = uo;
            self.prev_u2 = u2;
            classify_nonnegative_bounded(uo, CORE_MAX)?;
            classify_nonnegative_bounded(u2, CORE_MAX)?;
            if k == 0 {
                out0 = uo;
            }
            self.prev_uo = uo;
            self.prev_u2 = u2;
        }
        Ok(out0)
    }

    pub(crate) fn seed(init: NlInit, row_last: f64) -> Self {
        match init {
            NlInit::Zero => Self::default(),
            NlInit::MosqitoTail => Self::mosqito_seed(row_last),
        }
    }

    pub(crate) fn mosqito_seed(row_last: f64) -> Self {
        let delta = (0.0 - row_last) / NL_SUBSTEPS as f64;
        Self {
            prev_uo: row_last + delta * (NL_SUBSTEPS - 1) as f64,
            prev_u2: 0.0,
        }
    }

    /// Run the network over one 2 kHz frame: `NL_SUBSTEPS` sub-steps with the
    /// input interpolated linearly from `row_t` towards `next`. Returns `Uo`
    /// after the first sub-step.
    #[inline]
    pub(crate) fn advance_frame(&mut self, row_t: f64, next: f64, b: &[f64; 6]) -> f64 {
        let delta = (next - row_t) / NL_SUBSTEPS as f64;
        let mut out0 = 0.0;
        for k in 0..NL_SUBSTEPS {
            let ui = row_t + delta * k as f64;
            let uo = self.next_uo(ui, b);
            let u2 = self.next_u2(ui, uo, b);
            if k == 0 {
                out0 = uo;
            }
            self.prev_uo = uo;
            self.prev_u2 = u2;
        }
        out0
    }

    /// `Uo` follows the input `ui` unless a decay path (coupled network,
    /// `B[2]`/`B[3]`, while the first capacitor was above the second; single decay
    /// `B[4]` otherwise)
    /// still holds it higher.
    #[inline(always)]
    fn next_uo(&self, ui: f64, b: &[f64; 6]) -> f64 {
        let mut uo = ui;
        let coupled = self.prev_uo * b[2] - self.prev_u2 * b[3];
        if self.prev_uo > self.prev_u2 && coupled >= ui {
            uo = coupled;
        }
        let single = self.prev_uo * b[4];
        if self.prev_uo <= self.prev_u2 && single >= ui {
            uo = single;
        }
        uo
    }

    /// `U2` follows `uo`, except while `Uo` decays through the coupled network
    /// (`B[0]`/`B[1]`) and while a rising input charges it with `B[5]`.
    #[inline(always)]
    fn next_u2(&self, ui: f64, uo: f64, b: &[f64; 6]) -> f64 {
        let mut u2 = uo;
        let coupled = self.prev_uo * b[0] - self.prev_u2 * b[1];
        if ui < self.prev_uo && self.prev_uo > self.prev_u2 && coupled <= uo {
            u2 = coupled;
        }
        let charging = (self.prev_u2 - ui) * b[5] + ui;
        let level_and_not_above = (ui - self.prev_uo).abs() < NL_EPS && uo <= self.prev_u2;
        if ui >= self.prev_uo && !level_and_not_above {
            u2 = charging;
        }
        u2
    }
}

#[cfg(any(test, feature = "internals"))]
pub(super) fn nl_loudness_band_scalar(row: &[f64], out: &mut [f64], b: &[f64; 6], init: NlInit) {
    let n_time = row.len();
    assert_eq!(out.len(), n_time);
    if n_time == 0 {
        return;
    }
    let mut state = NlBandState::seed(init, row[n_time - 1]);
    for t in 0..n_time {
        let next = if t + 1 < n_time { row[t + 1] } else { 0.0 };
        out[t] = state.advance_frame(row[t], next, b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_init_default_and_mosqito_seed_formula() {
        assert_eq!(NlInit::default(), NlInit::Zero);
        let s = NlBandState::seed(NlInit::Zero, 2.4);
        assert_eq!((s.prev_uo, s.prev_u2), (0.0, 0.0));
        let s = NlBandState::seed(NlInit::MosqitoTail, 2.4);
        // core[-1] + (0 - core[-1]) / 24 * 23 == core[-1] / 24 up to rounding
        assert!((s.prev_uo - 0.1).abs() < 1e-15, "{}", s.prev_uo);
        assert_eq!(s.prev_u2, 0.0);
    }
}

#[cfg(test)]
mod checked_tests {
    use super::*;
    use crate::numeric_guard::{DomainFault, CORE_MAX};
    #[test]
    fn selected_states_and_normal_subnormal_interpolation() {
        let b = nl_coeffs();
        for value in [
            f64::NAN,
            -1.0,
            f64::from_bits(0x8000000000000001),
            f64::from_bits(CORE_MAX.to_bits() + 1),
        ] {
            for index in 0..2 {
                let mut state = NlBandState::default();
                if index == 0 {
                    state.prev_uo = value;
                } else {
                    state.prev_u2 = value;
                }
                assert!(state.advance_frame_checked(0.0, 0.0, &b).is_err());
            }
        }
        let mut original = NlBandState::default();
        let mut checked = NlBandState::default();
        for (row, next) in [
            (f64::from_bits(13), 0.0),
            (0.0, 0.0),
            (1.0, 0.0),
            (0.0, 10.0),
            (10.0, 1.0),
        ] {
            assert_eq!(
                original.advance_frame(row, next, &b).to_bits(),
                checked
                    .advance_frame_checked(row, next, &b)
                    .unwrap()
                    .to_bits()
            );
        }
        assert_eq!(
            checked.advance_frame_checked(-1.0, 0.0, &b),
            Err(DomainFault::Negative)
        );
    }
}
