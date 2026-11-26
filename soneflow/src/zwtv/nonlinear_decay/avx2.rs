//! AVX2+FMA nonlinear-decay kernel: 21 bands as five f64x4 groups plus a
//! scalar tail band.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "unsafe blocks from 0.1.0 without SAFETY comments; remove once documented"
)]

#[cfg(any(test, feature = "internals"))]
use super::params::nl_coeffs;
use super::params::{NlInit, NL_EPS, NL_SUBSTEPS};
#[cfg(any(test, feature = "internals"))]
use super::scalar::nl_loudness_band_scalar;
#[cfg(any(test, feature = "internals"))]
use crate::zwtv::par::{chunks_dispatch, ParMode};
use std::arch::x86_64::__m256d;

/// AVX2+FMA nonlinear temporal decay kernel.
///
/// # Safety
/// Caller must ensure AVX2 and FMA are available before calling.
#[cfg(any(test, feature = "internals"))]
pub unsafe fn nl_loudness_avx2(
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
    if n_time == 0 {
        return Vec::new();
    }

    let b = nl_coeffs();
    let mut out = vec![0.0; core.len()];

    chunks_dispatch(&mut out, 4 * n_time, mode, |g, group| {
        let n_bands = if 4 * g + 4 <= 21 { 4 } else { 21 - 4 * g };
        // SAFETY: dispatch has already verified AVX2+FMA availability.
        unsafe { nl_group_avx2(core, group, n_time, 4 * g, n_bands, b, init) };
    });

    out
}

/// Dispatch one output group: full 4-band groups use AVX2, the final single-band
/// tail uses scalar processing.
///
/// # Safety
/// Caller must ensure AVX2 and FMA are available before calling.
#[cfg(any(test, feature = "internals"))]
unsafe fn nl_group_avx2(
    core: &[f64],
    group: &mut [f64],
    n_time: usize,
    band: usize,
    n_bands: usize,
    b: [f64; 6],
    init: NlInit,
) {
    debug_assert_eq!(group.len(), n_bands * n_time);
    if n_bands == 4 {
        // SAFETY: caller has verified AVX2+FMA availability.
        unsafe { nl_loudness_process4(core, group, n_time, band, b, init) };
    } else {
        debug_assert_eq!(n_bands, 1);
        nl_loudness_band_scalar(&core[band * n_time..(band + 1) * n_time], group, &b, init);
    }
}

#[inline]
#[target_feature(enable = "avx2,fma")]
#[cfg(any(test, feature = "internals"))]
unsafe fn nl_loudness_load4(core: &[f64], n_time: usize, band: usize, t: usize) -> __m256d {
    use std::arch::x86_64::*;

    _mm256_set_pd(
        core[(band + 3) * n_time + t],
        core[(band + 2) * n_time + t],
        core[(band + 1) * n_time + t],
        core[band * n_time + t],
    )
}

/// Broadcast network constants, see [`nl_coeffs`].
pub(crate) struct NlConsts {
    b0: __m256d,
    b1: __m256d,
    b2: __m256d,
    b3: __m256d,
    b4: __m256d,
    b5: __m256d,
    inv_iter: __m256d,
    eps: __m256d,
}

impl NlConsts {
    /// # Safety
    /// Caller must ensure AVX2 and FMA are available.
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn new(b: [f64; 6]) -> Self {
        use std::arch::x86_64::*;
        Self {
            b0: _mm256_set1_pd(b[0]),
            b1: _mm256_set1_pd(b[1]),
            b2: _mm256_set1_pd(b[2]),
            b3: _mm256_set1_pd(b[3]),
            b4: _mm256_set1_pd(b[4]),
            b5: _mm256_set1_pd(b[5]),
            inv_iter: _mm256_set1_pd(1.0 / NL_SUBSTEPS as f64),
            eps: _mm256_set1_pd(NL_EPS),
        }
    }
}

/// Capacitor voltages of four bands; lane-wise twin of `NlBandState`.
pub(crate) struct NlGroupState {
    prev_uo: __m256d,
    prev_u2: __m256d,
}

#[target_feature(enable = "avx2,fma")]
unsafe fn check_core_lanes(
    value: __m256d,
) -> Result<(), (usize, crate::numeric_guard::DomainFault)> {
    // SAFETY: this function requires AVX2; CORE_MAX is a positive finite bound.
    if unsafe { crate::numeric_guard::lanes_valid(value, true, crate::numeric_guard::CORE_MAX) } {
        return Ok(());
    }
    use crate::numeric_guard::{classify_nonnegative_bounded, CORE_MAX};
    let mut lanes = [0.0; 4];
    unsafe {
        std::arch::x86_64::_mm256_storeu_pd(lanes.as_mut_ptr(), value);
    }
    for (lane, v) in lanes.into_iter().enumerate() {
        classify_nonnegative_bounded(v, CORE_MAX).map_err(|e| (lane, e))?;
    }
    Ok(())
}

impl NlGroupState {
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn numeric_reason_mask(&self) -> u32 {
        use crate::numeric_guard::{classify_nonnegative_bounded, CORE_MAX};
        let mut mask = 0;
        for value in [self.prev_uo, self.prev_u2] {
            let mut lanes = [0.0; 4];
            unsafe {
                std::arch::x86_64::_mm256_storeu_pd(lanes.as_mut_ptr(), value);
            }
            for v in lanes {
                if let Err(e) = classify_nonnegative_bounded(v, CORE_MAX) {
                    mask |= e.reason_mask();
                }
            }
        }
        mask
    }
    #[cfg(test)]
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn inject_numeric(&mut self, field: usize, lane: usize, value: f64) {
        let mut values = [0.0; 4];
        let vector = match field {
            0 => &mut self.prev_uo,
            1 => &mut self.prev_u2,
            _ => panic!("test field"),
        };
        unsafe {
            std::arch::x86_64::_mm256_storeu_pd(values.as_mut_ptr(), *vector);
        }
        values[lane] = value;
        *vector = unsafe { std::arch::x86_64::_mm256_loadu_pd(values.as_ptr()) };
    }
    #[cfg(test)] // c9_batch_hook
    pub(crate) fn inject_raw_bits(&mut self, lane: usize, field: usize, bits: u64) {
        assert!(
            lane < 4 && field < 2,
            "NL AVX2 lane/field out of range: lane={lane} field={field}"
        );
        let target = match field {
            0 => &mut self.prev_uo,
            1 => &mut self.prev_u2,
            _ => unreachable!(),
        };
        // SAFETY: `__m256d` and `[u64; 4]` are both 32-byte plain bit containers.
        let mut values: [u64; 4] = unsafe { std::mem::transmute(*target) };
        values[lane] = bits;
        *target = unsafe { std::mem::transmute::<[u64; 4], __m256d>(values) };
    }
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn validate_numeric(
        &self,
    ) -> Result<(), (usize, crate::numeric_guard::DomainFault)> {
        let a = unsafe { check_core_lanes(self.prev_uo) }.err();
        let b = unsafe { check_core_lanes(self.prev_u2) }.err();
        a.into_iter()
            .chain(b)
            .min_by_key(|x| x.0)
            .map_or(Ok(()), Err)
    }

    #[inline]
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn is_entry_valid(&self) -> bool {
        use crate::numeric_guard::{lanes_valid, CORE_MAX};
        unsafe {
            lanes_valid(self.prev_uo, true, CORE_MAX) && lanes_valid(self.prev_u2, true, CORE_MAX)
        }
    }

    #[inline]
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn core_lanes_valid(v: __m256d) -> bool {
        unsafe { crate::numeric_guard::lanes_valid(v, true, crate::numeric_guard::CORE_MAX) }
    }

    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn advance_frame_checked(
        &mut self,
        row: __m256d,
        next: __m256d,
        c: &NlConsts,
    ) -> Result<__m256d, (usize, crate::numeric_guard::DomainFault)> {
        use std::arch::x86_64::*;
        unsafe {
            self.validate_numeric()?;
            check_core_lanes(row)?;
            check_core_lanes(next)?;
        }
        let delta = _mm256_mul_pd(_mm256_sub_pd(next, row), c.inv_iter);
        let mut out0 = _mm256_setzero_pd();
        for k in 0..NL_SUBSTEPS {
            let ui = _mm256_fmadd_pd(delta, _mm256_set1_pd(k as f64), row);
            // A valid interpolation may be a tiny negative subnormal. Only
            // selected persistent states have the nonnegative invariant.
            let uo = unsafe { self.next_uo(ui, c) };
            let u2 = unsafe { self.next_u2(ui, uo, c) };
            self.prev_uo = uo;
            self.prev_u2 = u2;
            let fault = unsafe { check_core_lanes(uo) }
                .err()
                .into_iter()
                .chain(unsafe { check_core_lanes(u2) }.err())
                .min_by_key(|x| x.0);
            if let Some(fault) = fault {
                return Err(fault);
            }
            if k == 0 {
                out0 = uo;
            }
            self.prev_uo = uo;
            self.prev_u2 = u2;
        }
        Ok(out0)
    }

    /// # Safety
    /// Caller must ensure AVX2 and FMA are available.
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn zero() -> Self {
        use std::arch::x86_64::*;
        Self {
            prev_uo: _mm256_setzero_pd(),
            prev_u2: _mm256_setzero_pd(),
        }
    }

    /// # Safety
    /// Caller must ensure AVX2 and FMA are available.
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn seed(init: NlInit, last_row: __m256d, c: &NlConsts) -> Self {
        match init {
            NlInit::Zero => unsafe { Self::zero() },
            NlInit::MosqitoTail => unsafe { Self::mosqito_seed(last_row, c) },
        }
    }

    /// # Safety
    /// Caller must ensure AVX2 and FMA are available.
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn mosqito_seed(last_row: __m256d, c: &NlConsts) -> Self {
        use std::arch::x86_64::*;
        let last_delta = _mm256_mul_pd(_mm256_sub_pd(_mm256_setzero_pd(), last_row), c.inv_iter);
        Self {
            prev_uo: _mm256_fmadd_pd(
                last_delta,
                _mm256_set1_pd((NL_SUBSTEPS - 1) as f64),
                last_row,
            ),
            prev_u2: _mm256_setzero_pd(),
        }
    }

    /// Lane-wise `NlBandState::advance_frame`.
    ///
    /// # Safety
    /// Caller must ensure AVX2 and FMA are available.
    #[inline]
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn advance_frame(
        &mut self,
        row: __m256d,
        next: __m256d,
        c: &NlConsts,
    ) -> __m256d {
        use std::arch::x86_64::*;
        let delta = _mm256_mul_pd(_mm256_sub_pd(next, row), c.inv_iter);
        let mut out0 = _mm256_setzero_pd();
        for k in 0..NL_SUBSTEPS {
            let ui = _mm256_fmadd_pd(delta, _mm256_set1_pd(k as f64), row);
            let uo = unsafe { self.next_uo(ui, c) };
            let u2 = unsafe { self.next_u2(ui, uo, c) };
            if k == 0 {
                out0 = uo;
            }
            self.prev_uo = uo;
            self.prev_u2 = u2;
        }
        out0
    }

    /// Lane-wise `NlBandState::next_uo`.
    #[inline]
    #[target_feature(enable = "avx2,fma")]
    unsafe fn next_uo(&self, ui: __m256d, c: &NlConsts) -> __m256d {
        use std::arch::x86_64::*;
        let coupled = _mm256_fnmadd_pd(self.prev_u2, c.b3, _mm256_mul_pd(self.prev_uo, c.b2));
        let take_coupled = _mm256_and_pd(
            _mm256_cmp_pd(self.prev_uo, self.prev_u2, _CMP_GT_OQ),
            _mm256_cmp_pd(coupled, ui, _CMP_GE_OQ),
        );
        let uo = _mm256_blendv_pd(ui, coupled, take_coupled);
        let single = _mm256_mul_pd(self.prev_uo, c.b4);
        let take_single = _mm256_and_pd(
            _mm256_cmp_pd(self.prev_uo, self.prev_u2, _CMP_LE_OQ),
            _mm256_cmp_pd(single, ui, _CMP_GE_OQ),
        );
        _mm256_blendv_pd(uo, single, take_single)
    }

    /// Lane-wise `NlBandState::next_u2`.
    #[inline]
    #[target_feature(enable = "avx2,fma")]
    unsafe fn next_u2(&self, ui: __m256d, uo: __m256d, c: &NlConsts) -> __m256d {
        use std::arch::x86_64::*;
        let coupled = _mm256_fnmadd_pd(self.prev_u2, c.b1, _mm256_mul_pd(self.prev_uo, c.b0));
        let take_coupled = _mm256_and_pd(
            _mm256_and_pd(
                _mm256_cmp_pd(ui, self.prev_uo, _CMP_LT_OQ),
                _mm256_cmp_pd(self.prev_uo, self.prev_u2, _CMP_GT_OQ),
            ),
            _mm256_cmp_pd(coupled, uo, _CMP_LE_OQ),
        );
        let u2 = _mm256_blendv_pd(uo, coupled, take_coupled);
        let charging = _mm256_fmadd_pd(_mm256_sub_pd(self.prev_u2, ui), c.b5, ui);
        let diff_abs = _mm256_andnot_pd(_mm256_set1_pd(-0.0), _mm256_sub_pd(ui, self.prev_uo));
        let level_and_not_above = _mm256_and_pd(
            _mm256_cmp_pd(diff_abs, c.eps, _CMP_LT_OQ),
            _mm256_cmp_pd(uo, self.prev_u2, _CMP_LE_OQ),
        );
        let take_charging = _mm256_andnot_pd(
            level_and_not_above,
            _mm256_cmp_pd(ui, self.prev_uo, _CMP_GE_OQ),
        );
        _mm256_blendv_pd(u2, charging, take_charging)
    }
}

#[target_feature(enable = "avx2,fma")]
#[cfg(any(test, feature = "internals"))]
unsafe fn nl_loudness_process4(
    core: &[f64],
    out_group: &mut [f64],
    n_time: usize,
    band: usize,
    b: [f64; 6],
    init: NlInit,
) {
    use std::arch::x86_64::*;

    debug_assert_eq!(out_group.len(), 4 * n_time);
    let c = unsafe { NlConsts::new(b) };
    let last = n_time - 1;
    let last_row = nl_loudness_load4(core, n_time, band, last);
    let mut state = unsafe { NlGroupState::seed(init, last_row, &c) };

    for t in 0..n_time {
        let row = nl_loudness_load4(core, n_time, band, t);
        let next = if t + 1 < n_time {
            nl_loudness_load4(core, n_time, band, t + 1)
        } else {
            _mm256_setzero_pd()
        };
        let uo = unsafe { state.advance_frame(row, next, &c) };
        let mut lanes = [0.0; 4];
        _mm256_storeu_pd(lanes.as_mut_ptr(), uo);
        for lane in 0..4 {
            out_group[lane * n_time + t] = lanes[lane];
        }
    }
}

#[cfg(test)]
mod checked_tests {
    use super::*;
    #[test]
    fn c4_field_and_lane_ties_preserve_all_reasons() {
        if !crate::simd::use_avx2() {
            return;
        }
        use crate::numeric_guard::DomainFault;
        unsafe {
            let mut state = NlGroupState::zero();
            state.inject_numeric(0, 3, f64::NAN);
            state.inject_numeric(1, 0, -1.0);
            assert_eq!(state.validate_numeric(), Err((0, DomainFault::Negative)));
            assert_eq!(state.numeric_reason_mask(), 3);
            state.inject_numeric(0, 0, f64::NAN);
            assert_eq!(state.validate_numeric(), Err((0, DomainFault::NonFinite)));
            assert_eq!(state.numeric_reason_mask(), 3);
        }
    }
    use crate::numeric_guard::DomainFault;
    #[test]
    fn lowest_lane_in_either_persistent_field_is_reported() {
        if !crate::simd::use_avx2() {
            return;
        }
        unsafe {
            let mut state = NlGroupState::zero();
            state.inject_numeric(0, 3, f64::NAN);
            state.inject_numeric(1, 0, -1.0);
            assert_eq!(state.validate_numeric(), Err((0, DomainFault::Negative)));
        }
    }
    #[test]
    fn vector_subnormal_interpolation_and_updates_preserve_bits() {
        if !crate::simd::use_avx2() {
            return;
        }
        unsafe {
            let constants = NlConsts::new(super::super::nl_coeffs());
            let mut old = NlGroupState::zero();
            let mut checked = NlGroupState::zero();
            for (row, next) in [
                (f64::from_bits(13), 0.0),
                (0.0, 0.0),
                (1.0, 0.0),
                (0.0, 10.0),
                (10.0, 1.0),
            ] {
                let a = std::arch::x86_64::_mm256_set1_pd(row);
                let b = std::arch::x86_64::_mm256_set1_pd(next);
                let mut x = [0.0_f64; 4];
                let mut y = [0.0_f64; 4];
                std::arch::x86_64::_mm256_storeu_pd(
                    x.as_mut_ptr(),
                    old.advance_frame(a, b, &constants),
                );
                std::arch::x86_64::_mm256_storeu_pd(
                    y.as_mut_ptr(),
                    checked.advance_frame_checked(a, b, &constants).unwrap(),
                );
                assert_eq!(x.map(f64::to_bits), y.map(f64::to_bits));
            }
        }
    }
}
