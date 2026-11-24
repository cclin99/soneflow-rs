//! AVX2+FMA filter-bank kernel: 28 bands as 7 independent f64x4 groups.

use super::coeffs::{feedback_coeffs, intensity_to_db, smoothing_coeff};
use crate::tables::{N_TOB_BANDS, TOB_GAIN};
use crate::zwtv::grid::DEC_FACTOR;
use crate::zwtv::par::{chunks_dispatch, ParMode};
use std::arch::x86_64::__m256d;

/// Filter and smoother state of one f64x4 group (bands `4v..4v+4`).
pub(crate) struct TolGroupState {
    gain: __m256d,
    sb0: __m256d,
    sa1: __m256d,
    a1: [__m256d; 3],
    a2: [__m256d; 3],
    z0: [__m256d; 3],
    z1: [__m256d; 3],
    sm: [__m256d; 3],
}

#[target_feature(enable = "avx2,fma")]
unsafe fn check_lanes(
    value: __m256d,
    nonnegative: bool,
) -> Result<(), (usize, crate::numeric_guard::DomainFault)> {
    // SAFETY: this function requires AVX2; f64::MAX is a positive finite bound.
    if unsafe { crate::numeric_guard::lanes_valid(value, nonnegative, f64::MAX) } {
        return Ok(());
    }
    unsafe { check_lanes_slow(value, nonnegative) }
}

#[cold]
#[inline(never)]
#[target_feature(enable = "avx2,fma")]
unsafe fn check_lanes_slow(
    value: __m256d,
    nonnegative: bool,
) -> Result<(), (usize, crate::numeric_guard::DomainFault)> {
    use crate::numeric_guard::{classify_nonnegative_bounded, DomainFault};
    let mut lanes = [0.0; 4];
    unsafe {
        std::arch::x86_64::_mm256_storeu_pd(lanes.as_mut_ptr(), value);
    }
    for (lane, v) in lanes.into_iter().enumerate() {
        if !v.is_finite() {
            return Err((lane, DomainFault::NonFinite));
        }
        if nonnegative {
            classify_nonnegative_bounded(v, f64::MAX).map_err(|e| (lane, e))?;
        }
    }
    Ok(())
}

fn merge_fault(
    first: &mut Option<(usize, crate::numeric_guard::DomainFault)>,
    result: Result<(), (usize, crate::numeric_guard::DomainFault)>,
) {
    if let Err(fault) = result {
        if first.is_none_or(|old| fault.0 < old.0) {
            *first = Some(fault);
        }
    }
}

impl TolGroupState {
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn numeric_reason_mask(&self) -> u32 {
        use crate::numeric_guard::{classify_nonnegative_bounded, DomainFault};
        let mut mask = 0;
        for (value, nonnegative) in self
            .z0
            .iter()
            .chain(&self.z1)
            .map(|v| (v, false))
            .chain(self.sm.iter().map(|v| (v, true)))
        {
            let mut lanes = [0.0; 4];
            unsafe {
                std::arch::x86_64::_mm256_storeu_pd(lanes.as_mut_ptr(), *value);
            }
            for v in lanes {
                if !v.is_finite() {
                    mask |= DomainFault::NonFinite.reason_mask();
                } else if nonnegative {
                    if let Err(e) = classify_nonnegative_bounded(v, f64::MAX) {
                        mask |= e.reason_mask();
                    }
                }
            }
        }
        mask
    }
    #[inline]
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn validate_numeric(
        &self,
    ) -> Result<(), (usize, crate::numeric_guard::DomainFault)> {
        let mut fault = None;
        for &value in self.z0.iter().chain(&self.z1) {
            merge_fault(&mut fault, unsafe { check_lanes(value, false) });
        }
        for &value in &self.sm {
            merge_fault(&mut fault, unsafe { check_lanes(value, true) });
        }
        fault.map_or(Ok(()), Err)
    }

    #[inline]
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn advance_checked(
        &mut self,
        sample: f64,
    ) -> Result<__m256d, (usize, crate::numeric_guard::DomainFault)> {
        use std::arch::x86_64::*;
        unsafe {
            self.validate_numeric()?;
        }
        let xs = _mm256_set1_pd(sample);
        let mut y = _mm256_mul_pd(xs, self.gain);
        let mut fault = None;
        merge_fault(&mut fault, unsafe { check_lanes(y, false) });
        let b1s = [2.0, 0.0, -2.0];
        let b2s = [1.0, -1.0, 1.0];
        for section in 0..3 {
            let xin = y;
            y = _mm256_add_pd(xin, self.z0[section]);
            let b1v = _mm256_set1_pd(b1s[section]);
            let t = _mm256_fmadd_pd(b1v, xin, self.z1[section]);
            self.z0[section] = _mm256_fnmadd_pd(self.a1[section], y, t);
            let b2v = _mm256_set1_pd(b2s[section]);
            self.z1[section] = _mm256_fnmadd_pd(self.a2[section], y, _mm256_mul_pd(b2v, xin));
            for value in [y, self.z0[section], self.z1[section]] {
                merge_fault(&mut fault, unsafe { check_lanes(value, false) });
            }
        }
        y = _mm256_mul_pd(y, y);
        merge_fault(&mut fault, unsafe { check_lanes(y, true) });
        for stage in &mut self.sm {
            *stage = _mm256_fmadd_pd(self.sb0, y, _mm256_mul_pd(self.sa1, *stage));
            merge_fault(&mut fault, unsafe { check_lanes(*stage, true) });
            y = *stage;
        }
        fault.map_or(Ok(y), Err)
    }

    /// # Safety
    /// Caller must ensure AVX2 and FMA are available.
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn new(v: usize) -> Self {
        use std::arch::x86_64::*;

        let mut g = [0.0; 4];
        let mut b0s = [0.0; 4];
        let mut a1s = [0.0; 4];
        let mut a1c = [[0.0; 4]; 3];
        let mut a2c = [[0.0; 4]; 3];
        for lane in 0..4 {
            let band = 4 * v + lane;
            g[lane] = TOB_GAIN[band];
            let (b0, smooth_a1) = smoothing_coeff(band);
            b0s[lane] = b0;
            a1s[lane] = smooth_a1;
            for section in 0..3 {
                (a1c[section][lane], a2c[section][lane]) = feedback_coeffs(band, section);
            }
        }
        let mut a1 = [_mm256_setzero_pd(); 3];
        let mut a2 = [_mm256_setzero_pd(); 3];
        for section in 0..3 {
            a1[section] = _mm256_loadu_pd(a1c[section].as_ptr());
            a2[section] = _mm256_loadu_pd(a2c[section].as_ptr());
        }
        Self {
            gain: _mm256_loadu_pd(g.as_ptr()),
            sb0: _mm256_loadu_pd(b0s.as_ptr()),
            sa1: _mm256_loadu_pd(a1s.as_ptr()),
            a1,
            a2,
            z0: [_mm256_setzero_pd(); 3],
            z1: [_mm256_setzero_pd(); 3],
            sm: [_mm256_setzero_pd(); 3],
        }
    }

    /// # Safety
    /// Caller must ensure AVX2 and FMA are available.
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn reset(&mut self) {
        use std::arch::x86_64::_mm256_setzero_pd;
        self.z0 = [_mm256_setzero_pd(); 3];
        self.z1 = [_mm256_setzero_pd(); 3];
        self.sm = [_mm256_setzero_pd(); 3];
    }

    /// Bit-exact duplicate of creation coefficients and numeric vectors without extra FP arithmetic.
    ///
    /// # Safety
    /// Caller must ensure AVX2 and FMA are available.
    #[inline]
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn copy_exact(&self) -> Self {
        Self {
            gain: self.gain,
            sb0: self.sb0,
            sa1: self.sa1,
            a1: self.a1,
            a2: self.a2,
            z0: self.z0,
            z1: self.z1,
            sm: self.sm,
        }
    }

    /// Synchronize only the 9 numeric vectors (`z0`, `z1`, `sm`) by lane bits;
    /// creation coefficients (`gain`, `sb0`, `sa1`, `a1`, `a2`) are never modified.
    ///
    /// # Safety
    /// Caller must ensure AVX2 and FMA are available.
    #[inline]
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn sync_numeric_from(&mut self, src: &Self) {
        self.z0 = src.z0;
        self.z1 = src.z1;
        self.sm = src.sm;
    }

    #[cfg(test)]
    pub(crate) fn coeff_bits(&self) -> [u64; 36] {
        let mut out = [0u64; 36];
        let vecs = [
            self.gain, self.sb0, self.sa1, self.a1[0], self.a1[1], self.a1[2], self.a2[0],
            self.a2[1], self.a2[2],
        ];
        for (i, v) in vecs.into_iter().enumerate() {
            // SAFETY: `__m256d` and `[u64; 4]` are both 32-byte plain bit containers.
            let lanes: [u64; 4] = unsafe { std::mem::transmute(v) };
            out[i * 4..i * 4 + 4].copy_from_slice(&lanes);
        }
        out
    }

    #[cfg(test)]
    pub(crate) fn inject_nan_for_test(&mut self) {
        let mut lanes: [u64; 4] = unsafe { std::mem::transmute(self.z0[0]) };
        lanes[0] = f64::NAN.to_bits();
        self.z0[0] = unsafe { std::mem::transmute::<[u64; 4], __m256d>(lanes) };
    }

    #[cfg(test)] // c9_stream_hook
    pub(crate) fn inject_raw_bits(&mut self, lane: usize, field: usize, bits: u64) {
        assert!(
            lane < 4 && field < 9,
            "TOL AVX2 lane/field out of range: lane={lane} field={field}"
        );
        let target = if field < 6 {
            if field.is_multiple_of(2) {
                &mut self.z0[field / 2]
            } else {
                &mut self.z1[field / 2]
            }
        } else {
            &mut self.sm[field - 6]
        };
        // SAFETY: `__m256d` and `[u64; 4]` are both 32-byte plain bit containers.
        let mut values: [u64; 4] = unsafe { std::mem::transmute(*target) };
        values[lane] = bits;
        *target = unsafe { std::mem::transmute::<[u64; 4], __m256d>(values) };
    }

    #[cfg(test)]
    #[allow(clippy::needless_range_loop)]
    pub(crate) fn numeric_bits(&self) -> [u64; 36] {
        let mut out = [0u64; 36];
        // Same per-band order as scalar: for lane in 0..4: z0[0..3]/z1[0..3], then sm[0..3].
        // SAFETY: `[__m256d; 3]` and `[[u64; 4]; 3]` are plain bit containers.
        let z0: [[u64; 4]; 3] = unsafe { std::mem::transmute(self.z0) };
        let z1: [[u64; 4]; 3] = unsafe { std::mem::transmute(self.z1) };
        let sm: [[u64; 4]; 3] = unsafe { std::mem::transmute(self.sm) };
        let mut idx = 0;
        for lane in 0..4 {
            for s in 0..3 {
                out[idx] = z0[s][lane];
                out[idx + 1] = z1[s][lane];
                idx += 2;
            }
            for s in 0..3 {
                out[idx] = sm[s][lane];
                idx += 1;
            }
        }
        out
    }

    /// # Safety
    /// Caller must ensure AVX2 and FMA are available.
    #[inline]
    #[target_feature(enable = "avx2,fma")]
    pub(crate) unsafe fn advance(&mut self, sample: f64) -> __m256d {
        use std::arch::x86_64::*;

        let xs = _mm256_set1_pd(sample);
        let mut y = _mm256_mul_pd(xs, self.gain);
        // `TOB_B[section][1..3]`; `TOB_B[section][0] == 1` is folded into the add.
        let b1s = [2.0, 0.0, -2.0];
        let b2s = [1.0, -1.0, 1.0];
        for section in 0..3 {
            let xin = y;
            y = _mm256_add_pd(xin, self.z0[section]);
            let b1v = _mm256_set1_pd(b1s[section]);
            let t = _mm256_fmadd_pd(b1v, xin, self.z1[section]);
            self.z0[section] = _mm256_fnmadd_pd(self.a1[section], y, t);
            let b2v = _mm256_set1_pd(b2s[section]);
            self.z1[section] = _mm256_fnmadd_pd(self.a2[section], y, _mm256_mul_pd(b2v, xin));
        }
        y = _mm256_mul_pd(y, y);
        for stage_state in &mut self.sm {
            *stage_state = _mm256_fmadd_pd(self.sb0, y, _mm256_mul_pd(self.sa1, *stage_state));
            y = *stage_state;
        }
        y
    }
}

#[target_feature(enable = "avx2,fma")]
unsafe fn tol_group_avx2(sig: &[f64], v: usize, out_group: &mut [f64], n_time: usize) {
    use std::arch::x86_64::*;

    debug_assert_eq!(out_group.len(), 4 * n_time);
    let mut state = unsafe { TolGroupState::new(v) };

    let mut frame = 0usize;
    for (i, &sample) in sig.iter().enumerate() {
        let y = unsafe { state.advance(sample) };
        if i % DEC_FACTOR == 0 {
            let mut lanes = [0.0; 4];
            _mm256_storeu_pd(lanes.as_mut_ptr(), y);
            for lane in 0..4 {
                out_group[lane * n_time + frame] = intensity_to_db(lanes[lane]);
            }
            frame += 1;
        }
    }
}

/// AVX2+FMA filter bank: 28 bands as 7 independent f64x4 groups.
///
/// # Safety
/// Caller must ensure AVX2 and FMA are available before calling.
pub unsafe fn third_octave_levels_avx2(sig: &[f64], mode: ParMode) -> (Vec<f64>, usize) {
    let n_time = sig.len().div_ceil(DEC_FACTOR);
    let mut out = vec![0.0; N_TOB_BANDS * n_time];
    if n_time == 0 {
        return (out, 0);
    }

    chunks_dispatch(&mut out, 4 * n_time, mode, |v, group| {
        // SAFETY: dispatch has already verified AVX2+FMA availability.
        unsafe { tol_group_avx2(sig, v, group, n_time) };
    });

    (out, n_time)
}

#[cfg(test)]
mod checked_tests {
    use super::*;
    use crate::numeric_guard::DomainFault;
    // Fixed C4 scan for floating-environment regression comparison only.
    // Result expectations below are independently decoded from raw bits.
    #[target_feature(enable = "avx2,fma")]
    unsafe fn c4_check_lanes_reference(
        value: __m256d,
        nonnegative: bool,
    ) -> Result<(), (usize, DomainFault)> {
        if unsafe { crate::numeric_guard::lanes_valid(value, nonnegative, f64::MAX) } {
            return Ok(());
        }
        use crate::numeric_guard::classify_nonnegative_bounded;
        let mut lanes = [0.0; 4];
        unsafe {
            std::arch::x86_64::_mm256_storeu_pd(lanes.as_mut_ptr(), value);
        }
        for (lane, v) in lanes.into_iter().enumerate() {
            if !v.is_finite() {
                return Err((lane, DomainFault::NonFinite));
            }
            if nonnegative {
                classify_nonnegative_bounded(v, f64::MAX).map_err(|e| (lane, e))?;
            }
        }
        Ok(())
    }
    #[test]
    fn c5_cold_boundary_routes_fault_vector() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("fma") {
            return;
        }
        unsafe {
            let value = std::arch::x86_64::_mm256_set_pd(0.0, 0.0, 0.0, f64::INFINITY);
            assert_eq!(check_lanes(value, false), Err((0, DomainFault::NonFinite)));
        }
    }
    #[test]
    fn c5_cold_boundary_routes_nonnegative_flag() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("fma") {
            return;
        }
        unsafe {
            let value = std::arch::x86_64::_mm256_set_pd(0.0, 0.0, 0.0, -1.0);
            assert_eq!(check_lanes(value, true), Err((0, DomainFault::Negative)));
        }
    }
    #[test]
    fn c5_cold_boundary_preserves_vector_flag_and_complete_mxcsr() {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("fma") {
            return;
        }
        struct Restore(u32);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    std::arch::asm!("ldmxcsr [{p}]",p=in(reg) &self.0,options(readonly,nostack,preserves_flags));
                }
            }
        }
        unsafe {
            let mut saved = 0u32;
            std::arch::asm!("stmxcsr [{p}]",p=in(reg) &mut saved,options(nostack,preserves_flags));
            let _restore = Restore(saved);
            for rounding in [0, 0x2000, 0x4000, 0x6000] {
                for mode in [0, 0x40, 0x8000, 0x8040] {
                    for sticky in 0..=0x3f {
                        let control =
                            (saved & !(0x6000 | 0x8040 | 0x3f)) | rounding | mode | sticky;
                        std::arch::asm!("ldmxcsr [{p}]",p=in(reg) &control,options(readonly,nostack,preserves_flags));
                        for nonnegative in [false, true] {
                            for lane in 0..4 {
                                for raw in [
                                    0,
                                    1 << 63,
                                    1,
                                    0x000f_ffff_ffff_ffff,
                                    0x8000_0000_0000_0001,
                                    0x800f_ffff_ffff_ffff,
                                    0xbff0_0000_0000_0000,
                                    0x7fef_ffff_ffff_ffff,
                                    0xffef_ffff_ffff_ffff,
                                    0x7ff0_0000_0000_0000,
                                    0xfff0_0000_0000_0000,
                                    0x7ff0_0000_0000_0001,
                                    0xfff0_0000_0000_0001,
                                    0x7ff8_0000_1234_5678,
                                    0xfff8_0000_1234_5678,
                                ] {
                                    let mut values = [0.0; 4];
                                    values[lane] = f64::from_bits(raw);
                                    // A valid subnormal before a later bad lane exercises
                                    // the old scan's reachable sticky-flag behavior.
                                    if lane > 0 {
                                        values[0] = f64::from_bits(1);
                                    }
                                    let expected = if (raw >> 52) & 0x7ff == 0x7ff {
                                        Err((lane, DomainFault::NonFinite))
                                    } else if nonnegative
                                        && raw >> 63 != 0
                                        && raw & 0x7fff_ffff_ffff_ffff != 0
                                    {
                                        Err((lane, DomainFault::Negative))
                                    } else {
                                        Ok(())
                                    };
                                    let vector =
                                        std::arch::x86_64::_mm256_loadu_pd(values.as_ptr());
                                    std::arch::asm!("ldmxcsr [{p}]",p=in(reg) &control,options(readonly,nostack,preserves_flags));
                                    let result = check_lanes(vector, nonnegative);
                                    let mut actual = 0u32;
                                    std::arch::asm!("stmxcsr [{p}]",p=in(reg) &mut actual,options(nostack,preserves_flags));
                                    std::arch::asm!("ldmxcsr [{p}]",p=in(reg) &control,options(readonly,nostack,preserves_flags));
                                    let reference = c4_check_lanes_reference(vector, nonnegative);
                                    let mut reference_control = 0u32;
                                    std::arch::asm!("stmxcsr [{p}]",p=in(reg) &mut reference_control,options(nostack,preserves_flags));
                                    assert_eq!(
                                        result, expected,
                                        "wrapper raw={raw:x} lane={lane} nonnegative={nonnegative}"
                                    );
                                    assert_eq!(reference, expected);
                                    assert_eq!(
                                        actual, reference_control,
                                        "complete MXCSR differs from fixed C4 scan"
                                    );
                                    if expected.is_ok() {
                                        assert_eq!(
                                            actual, control,
                                            "fast-success changed complete MXCSR"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn c4_negative_smoother_is_checked_before_daz_overwrite() {
        if !crate::simd::use_avx2() {
            return;
        }
        struct Restore(u32);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    std::arch::asm!("ldmxcsr [{p}]",p=in(reg) &self.0,options(readonly,nostack,preserves_flags));
                }
            }
        }
        unsafe {
            let mut saved = 0u32;
            std::arch::asm!("stmxcsr [{p}]",p=in(reg) &mut saved,options(nostack,preserves_flags));
            let _restore = Restore(saved);
            let control = (saved & !0x6000) | 0x8040;
            std::arch::asm!("ldmxcsr [{p}]",p=in(reg) &control,options(readonly,nostack,preserves_flags));
            for group in 0..7 {
                for lane in 0..4 {
                    let mut values = [0.0; 4];
                    values[lane] = f64::from_bits(0x8000_0000_0000_0001);
                    let mut checked = TolGroupState::new(group);
                    let mut end_only = TolGroupState::new(group);
                    checked.sm[2] = std::arch::x86_64::_mm256_loadu_pd(values.as_ptr());
                    end_only.sm[2] = checked.sm[2];
                    assert!(
                        matches!(checked.advance_checked(0.0),Err((l,DomainFault::Negative)) if l==lane)
                    );
                    assert_eq!(checked.numeric_reason_mask(), 2);
                    let mut after = [0.0; 4];
                    std::arch::x86_64::_mm256_storeu_pd(after.as_mut_ptr(), checked.sm[2]);
                    assert_eq!(after.map(f64::to_bits), values.map(f64::to_bits));
                    end_only.advance(0.0);
                    // Removing entry validation would silently lose this fault.
                    assert_eq!(end_only.validate_numeric(), Ok(()));
                }
            }
        }
    }
    #[test]
    fn c4_field_first_selection_is_not_lane_precedence() {
        if !crate::simd::use_avx2() {
            return;
        }
        unsafe {
            let mut state = TolGroupState::new(0);
            state.z0[0] = std::arch::x86_64::_mm256_set_pd(f64::NAN, 0.0, 0.0, 0.0);
            state.sm[2] = std::arch::x86_64::_mm256_set_pd(0.0, 0.0, 0.0, -1.0);
            assert_eq!(state.validate_numeric(), Err((0, DomainFault::Negative)));
            assert_eq!(state.numeric_reason_mask(), 3);
            let field_first = check_lanes(state.z0[0], false);
            // Negative control: field-first incorrectly selects lane 3.
            assert_eq!(field_first, Err((3, DomainFault::NonFinite)));
            state.z0[0] = std::arch::x86_64::_mm256_set_pd(0.0, 0.0, 0.0, f64::NAN);
            assert_eq!(state.validate_numeric(), Err((0, DomainFault::NonFinite)));
            assert_eq!(state.numeric_reason_mask(), 3);
        }
    }
    #[test]
    fn lowest_lane_wins_across_all_persistent_fields() {
        if !crate::simd::use_avx2() {
            return;
        }
        unsafe {
            let mut state = TolGroupState::new(0);
            state.z0[0] = std::arch::x86_64::_mm256_set_pd(f64::NAN, 0.0, 0.0, 0.0);
            state.z1[2] = std::arch::x86_64::_mm256_set_pd(0.0, 0.0, 0.0, f64::INFINITY);
            assert_eq!(state.validate_numeric(), Err((0, DomainFault::NonFinite)));
            state.reset();
            state.z0[0] = std::arch::x86_64::_mm256_set_pd(f64::NAN, 0.0, 0.0, 0.0);
            state.sm[2] = std::arch::x86_64::_mm256_set_pd(0.0, 0.0, -1.0, 0.0);
            assert_eq!(state.validate_numeric(), Err((1, DomainFault::Negative)));
        }
    }
    #[test]
    fn normal_vector_updates_keep_all_lane_bits() {
        if !crate::simd::use_avx2() {
            return;
        }
        unsafe {
            for group in 0..7 {
                let mut old = TolGroupState::new(group);
                let mut checked = TolGroupState::new(group);
                for i in 0..200 {
                    let sample = (i as f64 * 0.13).sin() * 0.2;
                    let mut a = [0.0_f64; 4];
                    let mut b = [0.0_f64; 4];
                    std::arch::x86_64::_mm256_storeu_pd(a.as_mut_ptr(), old.advance(sample));
                    std::arch::x86_64::_mm256_storeu_pd(
                        b.as_mut_ptr(),
                        checked.advance_checked(sample).unwrap(),
                    );
                    assert_eq!(a.map(f64::to_bits), b.map(f64::to_bits));
                }
            }
        }
    }
}
