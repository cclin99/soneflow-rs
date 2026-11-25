//! Frame-by-frame nonlinear decay of the 21 core bands, scalar or AVX2
//! (five f64x4 groups plus a scalar tail band). Always starts from zero state.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "unsafe blocks from 0.1.0 without SAFETY comments; remove once documented"
)]

use crate::zwtv::nonlinear_decay::{nl_coeffs, NlBandState};
#[cfg(target_arch = "x86_64")]
use crate::zwtv::nonlinear_decay::{NlConsts, NlGroupState};

pub(super) enum NlStage {
    Scalar {
        states: [NlBandState; 21],
        b: [f64; 6],
    },
    #[cfg(target_arch = "x86_64")]
    Avx2 {
        groups: Box<[NlGroupState; 5]>,
        consts: NlConsts,
        tail: NlBandState,
        b: [f64; 6],
    },
}

impl NlStage {
    pub(super) fn numeric_reason_mask(&self) -> u32 {
        match self {
            Self::Scalar { states, .. } => states
                .iter()
                .fold(0, |mask, s| mask | s.numeric_reason_mask()),
            #[cfg(target_arch = "x86_64")]
            Self::Avx2 { groups, tail, .. } => {
                groups.iter().fold(tail.numeric_reason_mask(), |mask, s| {
                    mask | unsafe { s.numeric_reason_mask() }
                })
            }
        }
    }
    #[cfg(test)]
    pub(super) fn inject_numeric(&mut self, band: usize, field: usize, value: f64) {
        match self {
            Self::Scalar { states, .. } => states[band].inject_numeric(field, value),
            #[cfg(target_arch = "x86_64")]
            Self::Avx2 { groups, tail, .. } => {
                if band == 20 {
                    tail.inject_numeric(field, value)
                } else {
                    unsafe { groups[band / 4].inject_numeric(field, band % 4, value) }
                }
            }
        }
    }
    pub(super) fn validate_numeric(
        &self,
    ) -> Result<(), (usize, crate::numeric_guard::DomainFault)> {
        match self {
            Self::Scalar { states, .. } => {
                for (band, state) in states.iter().enumerate() {
                    state.validate_numeric().map_err(|e| (band, e))?;
                }
            }
            #[cfg(target_arch = "x86_64")]
            Self::Avx2 { groups, tail, .. } => {
                for (group, state) in groups.iter().enumerate() {
                    unsafe { state.validate_numeric() }
                        .map_err(|(lane, e)| (group * 4 + lane, e))?;
                }
                tail.validate_numeric().map_err(|e| (20, e))?;
            }
        }
        Ok(())
    }

    /// Pure integer D0 entry predicate for all 21 bands (5 AVX2 groups + scalar tail or 21 scalar).
    /// Bit-exact equivalent to `self.validate_numeric().is_ok()`.
    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub(super) fn is_entry_valid(&self) -> bool {
        match self {
            Self::Scalar { states, .. } => states.iter().all(NlBandState::is_entry_valid),
            Self::Avx2 { groups, tail, .. } => {
                groups.iter().all(|g| unsafe { g.is_entry_valid() }) && tail.is_entry_valid()
            }
        }
    }

    pub(super) fn advance_checked(
        &mut self,
        held: &[f64; 21],
        next: &[f64; 21],
    ) -> Result<[f64; 21], (usize, crate::numeric_guard::DomainFault)> {
        let mut out = [0.0; 21];
        match self {
            Self::Scalar { states, b } => {
                for band in 0..21 {
                    out[band] = states[band]
                        .advance_frame_checked(held[band], next[band], b)
                        .map_err(|e| (band, e))?;
                }
            }
            #[cfg(target_arch = "x86_64")]
            Self::Avx2 {
                groups,
                consts,
                tail,
                b,
            } => unsafe {
                use std::arch::x86_64::{_mm256_loadu_pd, _mm256_storeu_pd};
                for (group, state) in groups.iter_mut().enumerate() {
                    let band = group * 4;
                    let value = state
                        .advance_frame_checked(
                            _mm256_loadu_pd(held[band..].as_ptr()),
                            _mm256_loadu_pd(next[band..].as_ptr()),
                            consts,
                        )
                        .map_err(|(lane, e)| (band + lane, e))?;
                    _mm256_storeu_pd(out[band..].as_mut_ptr(), value);
                }
                out[20] = tail
                    .advance_frame_checked(held[20], next[20], b)
                    .map_err(|e| (20, e))?;
            },
        }
        Ok(out)
    }

    pub(super) fn new(avx2: bool) -> Self {
        let b = nl_coeffs();
        #[cfg(not(target_arch = "x86_64"))]
        let _ = avx2;
        #[cfg(target_arch = "x86_64")]
        if avx2 {
            return unsafe {
                Self::Avx2 {
                    groups: Box::new(std::array::from_fn(|_| NlGroupState::zero())),
                    consts: NlConsts::new(b),
                    tail: NlBandState::default(),
                    b,
                }
            };
        }
        Self::Scalar {
            states: [NlBandState::default(); 21],
            b,
        }
    }

    pub(super) fn reset(&mut self) {
        match self {
            Self::Scalar { states, .. } => states.fill(NlBandState::default()),
            #[cfg(target_arch = "x86_64")]
            Self::Avx2 { groups, tail, .. } => {
                groups
                    .iter_mut()
                    .for_each(|state| *state = unsafe { NlGroupState::zero() });
                *tail = NlBandState::default();
            }
        }
    }

    /// Decay output of frame `held`, interpolating towards `next`.
    pub(super) fn advance(&mut self, held: &[f64; 21], next: &[f64; 21]) -> [f64; 21] {
        let mut out = [0.0; 21];
        match self {
            Self::Scalar { states, b } => {
                for band in 0..21 {
                    out[band] = states[band].advance_frame(held[band], next[band], b);
                }
            }
            #[cfg(target_arch = "x86_64")]
            Self::Avx2 {
                groups,
                consts,
                tail,
                b,
            } => unsafe { advance_avx2(groups, consts, tail, b, held, next, &mut out) },
        }
        out
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn advance_avx2(
    groups: &mut [NlGroupState; 5],
    consts: &NlConsts,
    tail: &mut NlBandState,
    b: &[f64; 6],
    held: &[f64; 21],
    next: &[f64; 21],
    out: &mut [f64; 21],
) {
    use std::arch::x86_64::{_mm256_loadu_pd, _mm256_storeu_pd};
    for (group, state) in groups.iter_mut().enumerate() {
        let band = group * 4;
        let row = _mm256_loadu_pd(held[band..].as_ptr());
        let next_row = _mm256_loadu_pd(next[band..].as_ptr());
        let value = state.advance_frame(row, next_row, consts);
        _mm256_storeu_pd(out[band..].as_mut_ptr(), value);
    }
    out[20] = tail.advance_frame(held[20], next[20], b);
}
