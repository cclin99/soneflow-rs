use super::grid::INTERNAL_RATE;

/// Sub-steps per 2 kHz frame of the two low-pass filters.
const LP_SUBSTEPS: usize = 24;

fn lp_coeff(tau: f64) -> (f64, f64) {
    let a1 = (-1.0 / (INTERNAL_RATE * LP_SUBSTEPS as f64 * tau)).exp();
    (1.0 - a1, a1)
}

pub(crate) struct TwState {
    y_fast: f64,
    y_slow: f64,
    bf: (f64, f64),
    bs: (f64, f64),
    prev: f64,
    has_prev: bool,
}

impl TwState {
    pub(crate) fn numeric_reason_mask(&self) -> u32 {
        use crate::numeric_guard::{classify_nonnegative_bounded, TW_INPUT_MAX, TW_STATE_MAX};
        [
            (self.y_fast, TW_STATE_MAX),
            (self.y_slow, TW_STATE_MAX),
            (self.prev, TW_INPUT_MAX),
        ]
        .into_iter()
        .filter_map(|(v, upper)| classify_nonnegative_bounded(v, upper).err())
        .fold(0, |mask, error| mask | error.reason_mask())
    }
    #[cfg(test)]
    pub(crate) fn inject_numeric(&mut self, field: usize, value: f64) {
        match field {
            0 => self.y_fast = value,
            1 => self.y_slow = value,
            2 => self.prev = value,
            _ => panic!("test field"),
        }
    }
    #[cfg(test)]
    pub(crate) fn inject_raw_bits(&mut self, field: usize, bits: u64) {
        match field {
            0 => self.y_fast = f64::from_bits(bits),
            1 => self.y_slow = f64::from_bits(bits),
            2 => self.prev = f64::from_bits(bits),
            _ => panic!("TW field out of range: {field}"),
        }
    }
    pub(crate) fn validate_numeric(&self) -> Result<(), crate::numeric_guard::DomainFault> {
        use crate::numeric_guard::{classify_nonnegative_bounded, TW_INPUT_MAX, TW_STATE_MAX};
        classify_nonnegative_bounded(self.y_fast, TW_STATE_MAX)?;
        classify_nonnegative_bounded(self.y_slow, TW_STATE_MAX)?;
        classify_nonnegative_bounded(self.prev, TW_INPUT_MAX)
    }

    #[inline(always)]
    pub(crate) fn is_entry_valid(&self) -> bool {
        use crate::numeric_guard::{is_nonnegative_bounded, TW_INPUT_MAX, TW_STATE_MAX};
        is_nonnegative_bounded(self.y_fast, TW_STATE_MAX)
            && is_nonnegative_bounded(self.y_slow, TW_STATE_MAX)
            && is_nonnegative_bounded(self.prev, TW_INPUT_MAX)
    }

    pub(crate) fn advance_checked(
        &mut self,
        loud_t: f64,
    ) -> Result<f64, crate::numeric_guard::DomainFault> {
        use crate::numeric_guard::{
            classify_nonnegative_bounded, DomainFault, TW_INPUT_MAX, TW_STATE_MAX,
        };
        self.validate_numeric()?;
        classify_nonnegative_bounded(loud_t, TW_INPUT_MAX)?;
        if self.has_prev {
            let delta = (loud_t - self.prev) / LP_SUBSTEPS as f64;
            for k in 1..LP_SUBSTEPS {
                let ui = self.prev + delta * k as f64;
                if !ui.is_finite() {
                    return Err(DomainFault::NonFinite);
                }
                self.y_fast = self.bf.0 * ui + self.bf.1 * self.y_fast;
                self.y_slow = self.bs.0 * ui + self.bs.1 * self.y_slow;
                classify_nonnegative_bounded(self.y_fast, TW_STATE_MAX)?;
                classify_nonnegative_bounded(self.y_slow, TW_STATE_MAX)?;
            }
        }
        self.y_fast = self.bf.0 * loud_t + self.bf.1 * self.y_fast;
        self.y_slow = self.bs.0 * loud_t + self.bs.1 * self.y_slow;
        self.prev = loud_t;
        self.has_prev = true;
        self.validate_numeric()?;
        let out = 0.47 * self.y_fast + 0.53 * self.y_slow;
        classify_nonnegative_bounded(out, TW_STATE_MAX)?;
        Ok(out)
    }

    pub(crate) fn new() -> Self {
        Self {
            y_fast: 0.0,
            y_slow: 0.0,
            bf: lp_coeff(3.5e-3),
            bs: lp_coeff(70e-3),
            prev: 0.0,
            has_prev: false,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.y_fast = 0.0;
        self.y_slow = 0.0;
        self.prev = 0.0;
        self.has_prev = false;
    }

    #[inline]
    pub(crate) fn advance(&mut self, loud_t: f64) -> f64 {
        if self.has_prev {
            let delta = (loud_t - self.prev) / LP_SUBSTEPS as f64;
            for k in 1..LP_SUBSTEPS {
                let ui = self.prev + delta * k as f64;
                self.y_fast = self.bf.0 * ui + self.bf.1 * self.y_fast;
                self.y_slow = self.bs.0 * ui + self.bs.1 * self.y_slow;
            }
        }
        self.y_fast = self.bf.0 * loud_t + self.bf.1 * self.y_fast;
        self.y_slow = self.bs.0 * loud_t + self.bs.1 * self.y_slow;
        self.prev = loud_t;
        self.has_prev = true;
        0.47 * self.y_fast + 0.53 * self.y_slow
    }
}

/// Duration-dependent loudness perception: 0.47*LP(3.5ms) + 0.53*LP(70ms).
#[cfg(any(test, feature = "internals"))]
pub fn temporal_weighting(loudness: &[f64]) -> Vec<f64> {
    let mut state = TwState::new();
    loudness.iter().map(|&l| state.advance(l)).collect()
}

#[cfg(test)]
mod checked_tests {
    use super::*;
    use crate::numeric_guard::{DomainFault, TW_INPUT_MAX, TW_STATE_MAX};
    #[test]
    fn dormant_and_active_state_prechecks_cannot_be_masked_by_output() {
        for index in 0..3 {
            for invalid in [f64::NAN, f64::from_bits(0x8000000000000001)] {
                let mut state = TwState::new();
                match index {
                    0 => state.y_fast = invalid,
                    1 => state.y_slow = invalid,
                    _ => state.prev = invalid,
                }
                assert!(state.advance_checked(0.0).is_err());
            }
        }
        let mut state = TwState::new();
        state.y_fast = f64::from_bits(TW_STATE_MAX.to_bits() + 1);
        assert_eq!(state.advance_checked(0.0), Err(DomainFault::AboveUpper));
        state.reset();
        state.prev = f64::from_bits(TW_INPUT_MAX.to_bits() + 1);
        assert_eq!(state.advance_checked(0.0), Err(DomainFault::AboveUpper));
    }
    #[test]
    fn normal_and_subnormal_bits_are_preserved() {
        let mut original = TwState::new();
        let mut checked = TwState::new();
        for input in [f64::from_bits(13), 0.0, 1.0, 10.0, 0.0, 0.2] {
            assert_eq!(
                original.advance(input).to_bits(),
                checked.advance_checked(input).unwrap().to_bits()
            );
        }
    }
}
