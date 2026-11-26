//! Stream front end: the 28-band filter bank advanced sample by sample, with
//! the decimation phase that picks one 2 kHz frame every `DEC_FACTOR` samples.
//!
//! Dual-buffered TOL storage (`kernel` + `shadow`)
//! with immutable bit-copied creation coefficients, fixed 20-entry emission buffer for
//! up to 480-sample speculative blocks, and single `target_feature(avx2,fma)` AVX2 block loop.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "unsafe blocks from 0.1.0 without SAFETY comments; remove once documented"
)]

use crate::numeric_guard::DomainFault;
use crate::zwtv::grid::DEC_FACTOR;
#[cfg(target_arch = "x86_64")]
use crate::zwtv::third_octave_levels::TolGroupState;
use crate::zwtv::third_octave_levels::{intensity_to_db, TolBandState};

pub(super) const STREAM_BLOCK_LEN: usize = 480;
pub(super) const STREAM_MAX_EMISSIONS: usize = STREAM_BLOCK_LEN / DEC_FACTOR;

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct SpecEmission {
    pub(super) levels: [f64; 28],
    pub(super) sample_ordinal: u64,
    pub(super) nonfinite_since_prev: bool,
}

impl SpecEmission {
    pub(super) const ZERO: Self = Self {
        levels: [0.0; 28],
        sample_ordinal: 0,
        nonfinite_since_prev: false,
    };
}

#[cfg(target_arch = "x86_64")]
pub(super) struct FastBlockOutcome {
    pub(super) ok: bool,
    pub(super) emission_count: usize,
    pub(super) trailing_nonfinite: bool,
    pub(super) end_phase: usize,
}

pub(super) struct TolStage {
    pub(super) kernel: TolKernel,
    pub(super) shadow: TolKernel,
    pub(super) sample_phase: usize,
}

pub(super) enum TolKernel {
    Scalar(Box<[TolBandState; 28]>),
    #[cfg(target_arch = "x86_64")]
    Avx2(Box<[TolGroupState; 7]>),
}

#[inline]
fn validate_scalar_slice(states: &[TolBandState; 28]) -> Result<(), (usize, DomainFault)> {
    for (band, state) in states.iter().enumerate() {
        state.validate_numeric().map_err(|e| (band, e))?;
    }
    Ok(())
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "avx2,fma")]
unsafe fn validate_avx2_slice(groups: &[TolGroupState; 7]) -> Result<(), (usize, DomainFault)> {
    for (group, state) in groups.iter().enumerate() {
        unsafe { state.validate_numeric() }.map_err(|(lane, e)| (group * 4 + lane, e))?;
    }
    Ok(())
}

/// The 28 filters of one TOL kernel, advanced one sample at a time.
#[cfg(target_arch = "x86_64")]
trait FilterBank {
    /// Advance every band by `sample` and write the 28 levels; returns the OR
    /// of their non-finite mask bits (`crate::numeric_guard::nonfinite_mask_bit`).
    fn advance_emit(&mut self, sample: f64, levels: &mut [f64; 28]) -> u64;
    /// Advance every band by `sample` without reading the levels.
    fn advance(&mut self, sample: f64);
}

#[cfg(target_arch = "x86_64")]
impl FilterBank for [TolBandState; 28] {
    #[inline(always)]
    fn advance_emit(&mut self, sample: f64, levels: &mut [f64; 28]) -> u64 {
        let mut nonfinite = 0u64;
        for (slot, state) in levels.iter_mut().zip(self.iter_mut()) {
            let level = intensity_to_db(state.advance(sample));
            nonfinite |= crate::numeric_guard::nonfinite_mask_bit(level);
            *slot = level;
        }
        nonfinite
    }

    #[inline(always)]
    fn advance(&mut self, sample: f64) {
        for state in self.iter_mut() {
            let _ = state.advance(sample);
        }
    }
}

/// The seven AVX2 groups of one kernel. Built only inside
/// `run_avx2_fast_block`, which requires AVX2 and FMA; the `unsafe` calls
/// below rely on that.
#[cfg(target_arch = "x86_64")]
struct Avx2Bank<'a>(&'a mut [TolGroupState; 7]);

#[cfg(target_arch = "x86_64")]
impl FilterBank for Avx2Bank<'_> {
    #[inline(always)]
    fn advance_emit(&mut self, sample: f64, levels: &mut [f64; 28]) -> u64 {
        // SAFETY: AVX2 and FMA are available (see `Avx2Bank`).
        unsafe { avx2_advance_emit(self.0, sample, levels) }
    }

    #[inline(always)]
    fn advance(&mut self, sample: f64) {
        // SAFETY: AVX2 and FMA are available (see `Avx2Bank`).
        unsafe { avx2_advance(self.0, sample) }
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "avx2,fma")]
unsafe fn avx2_advance_emit(
    groups: &mut [TolGroupState; 7],
    sample: f64,
    levels: &mut [f64; 28],
) -> u64 {
    use std::arch::x86_64::_mm256_storeu_pd;
    let mut nonfinite = 0u64;
    for (group, state) in groups.iter_mut().enumerate() {
        // SAFETY: this function's own target features are enabled.
        let values = unsafe { state.advance(sample) };
        let mut lanes = [0.0; 4];
        // SAFETY: `lanes` holds four f64; unaligned store.
        unsafe { _mm256_storeu_pd(lanes.as_mut_ptr(), values) };
        let l0 = intensity_to_db(lanes[0]);
        let l1 = intensity_to_db(lanes[1]);
        let l2 = intensity_to_db(lanes[2]);
        let l3 = intensity_to_db(lanes[3]);
        nonfinite |= crate::numeric_guard::nonfinite_mask_bit(l0)
            | crate::numeric_guard::nonfinite_mask_bit(l1)
            | crate::numeric_guard::nonfinite_mask_bit(l2)
            | crate::numeric_guard::nonfinite_mask_bit(l3);
        let base = group * 4;
        levels[base] = l0;
        levels[base + 1] = l1;
        levels[base + 2] = l2;
        levels[base + 3] = l3;
    }
    nonfinite
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "avx2,fma")]
unsafe fn avx2_advance(groups: &mut [TolGroupState; 7], sample: f64) {
    for state in groups.iter_mut() {
        // SAFETY: this function's own target features are enabled.
        let _ = unsafe { state.advance(sample) };
    }
}

/// The speculative pass over one block on `shadow` (already synced from
/// `kernel`): a non-finite sample counts as zero and is remembered for the
/// next emission; every `DEC_FACTOR`-th sample (phase 0) emits the 28 levels.
/// `ok` covers the levels only; the caller adds the D1 state check.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn scan_block<B: FilterBank>(
    shadow: &mut B,
    block: &[f64],
    entry_phase: usize,
    entry_ordinal: u64,
    emissions: &mut [SpecEmission; STREAM_MAX_EMISSIONS],
) -> FastBlockOutcome {
    let mut nonfinite_acc = 0u64;
    let mut emission_count = 0usize;
    let mut seg_nonfinite = false;
    let mut phase = entry_phase;
    for (offset, &raw) in block.iter().enumerate() {
        let sample = if raw.is_finite() {
            raw
        } else {
            seg_nonfinite = true;
            0.0
        };
        if phase == 0 {
            let entry = &mut emissions[emission_count];
            nonfinite_acc |= shadow.advance_emit(sample, &mut entry.levels);
            entry.sample_ordinal = entry_ordinal + offset as u64;
            entry.nonfinite_since_prev = seg_nonfinite;
            emission_count += 1;
            seg_nonfinite = false;
        } else {
            shadow.advance(sample);
        }
        phase += 1;
        if phase == DEC_FACTOR {
            phase = 0;
        }
    }
    FastBlockOutcome {
        ok: (nonfinite_acc as i64) >= 0,
        emission_count,
        trailing_nonfinite: seg_nonfinite,
        end_phase: phase,
    }
}

#[cfg(target_arch = "x86_64")]
#[inline(never)]
fn run_scalar_fast_block(
    kernel: &[TolBandState; 28],
    shadow: &mut [TolBandState; 28],
    block: &[f64],
    entry_phase: usize,
    entry_ordinal: u64,
    emissions: &mut [SpecEmission; STREAM_MAX_EMISSIONS],
) -> FastBlockOutcome {
    for (dst, src) in shadow.iter_mut().zip(kernel.iter()) {
        dst.sync_numeric_from(src);
    }
    let mut outcome = scan_block(shadow, block, entry_phase, entry_ordinal, emissions);
    let d1_ok = validate_scalar_slice(shadow).is_ok();
    outcome.ok &= d1_ok;
    outcome
}

#[cfg(target_arch = "x86_64")]
#[inline(never)]
#[target_feature(enable = "avx2,fma")]
unsafe fn run_avx2_fast_block(
    kernel: &[TolGroupState; 7],
    shadow: &mut [TolGroupState; 7],
    block: &[f64],
    entry_phase: usize,
    entry_ordinal: u64,
    emissions: &mut [SpecEmission; STREAM_MAX_EMISSIONS],
) -> FastBlockOutcome {
    for (dst, src) in shadow.iter_mut().zip(kernel.iter()) {
        // SAFETY: this function's own target features are enabled.
        unsafe { dst.sync_numeric_from(src) };
    }
    let mut outcome = scan_block(
        &mut Avx2Bank(shadow),
        block,
        entry_phase,
        entry_ordinal,
        emissions,
    );
    // SAFETY: as above.
    let d1_ok = unsafe { validate_avx2_slice(shadow) }.is_ok();
    outcome.ok &= d1_ok;
    outcome
}

#[cfg(target_arch = "x86_64")]
#[inline]
pub(super) fn advance_block_spec(
    kernel: &TolKernel,
    shadow: &mut TolKernel,
    block: &[f64],
    entry_phase: usize,
    entry_ordinal: u64,
    emissions: &mut [SpecEmission; STREAM_MAX_EMISSIONS],
) -> FastBlockOutcome {
    match (kernel, shadow) {
        (TolKernel::Scalar(k), TolKernel::Scalar(s)) => {
            run_scalar_fast_block(k, s, block, entry_phase, entry_ordinal, emissions)
        }
        (TolKernel::Avx2(k), TolKernel::Avx2(s)) => unsafe {
            run_avx2_fast_block(k, s, block, entry_phase, entry_ordinal, emissions)
        },
        _ => unreachable!("kernel and shadow must share the same backend"),
    }
}

impl TolStage {
    pub(super) fn reset_numeric(&mut self) {
        self.kernel.reset();
    }

    pub(super) fn validate_numeric(&self) -> Result<(), (usize, DomainFault)> {
        self.kernel.validate_numeric()
    }

    pub(super) fn new(avx2: bool) -> Self {
        let (kernel, shadow) = TolKernel::new_pair(avx2);
        Self {
            kernel,
            shadow,
            sample_phase: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.kernel.reset();
        self.shadow.reset();
        self.sample_phase = 0;
    }

    #[cfg(test)]
    pub(super) fn kernel_ptr_and_align(&self) -> (usize, usize) {
        self.kernel.ptr_and_align()
    }

    #[cfg(test)]
    pub(super) fn shadow_ptr_and_align(&self) -> (usize, usize) {
        self.shadow.ptr_and_align()
    }

    #[cfg(test)]
    pub(super) fn kernel_coeff_bits(&self) -> Vec<u64> {
        self.kernel.coeff_bits()
    }

    #[cfg(test)]
    pub(super) fn shadow_coeff_bits(&self) -> Vec<u64> {
        self.shadow.coeff_bits()
    }

    #[cfg(test)]
    pub(super) fn kernel_numeric_bits(&self) -> [u64; 252] {
        self.kernel.numeric_bits()
    }

    #[cfg(test)]
    pub(super) fn shadow_numeric_bits(&self) -> [u64; 252] {
        self.shadow.numeric_bits()
    }
}

impl TolKernel {
    fn new_pair(avx2: bool) -> (Self, Self) {
        #[cfg(not(target_arch = "x86_64"))]
        let _ = avx2;
        #[cfg(target_arch = "x86_64")]
        if avx2 {
            let kernel = Box::new(std::array::from_fn(|group| unsafe {
                TolGroupState::new(group)
            }));
            let shadow = Box::new(std::array::from_fn(|group| unsafe {
                kernel[group].copy_exact()
            }));
            return (Self::Avx2(kernel), Self::Avx2(shadow));
        }
        let kernel = Box::new(std::array::from_fn(TolBandState::new));
        let shadow = Box::new(std::array::from_fn(|band| kernel[band].copy_exact()));
        (Self::Scalar(kernel), Self::Scalar(shadow))
    }

    pub(super) fn numeric_reason_mask(&self) -> u32 {
        match self {
            Self::Scalar(states) => states
                .iter()
                .fold(0, |mask, s| mask | s.numeric_reason_mask()),
            #[cfg(target_arch = "x86_64")]
            Self::Avx2(groups) => groups
                .iter()
                .fold(0, |mask, s| mask | unsafe { s.numeric_reason_mask() }),
        }
    }

    pub(super) fn validate_numeric(&self) -> Result<(), (usize, DomainFault)> {
        match self {
            Self::Scalar(states) => validate_scalar_slice(states),
            #[cfg(target_arch = "x86_64")]
            Self::Avx2(groups) => unsafe { validate_avx2_slice(groups) },
        }
    }

    pub(super) fn advance_sample_checked(
        &mut self,
        sample: f64,
        emit: bool,
    ) -> Result<Option<[f64; 28]>, (usize, DomainFault)> {
        let mut levels = [0.0; 28];
        match self {
            Self::Scalar(states) => {
                for (band, state) in states.iter_mut().enumerate() {
                    let value = state.advance_checked(sample).map_err(|e| (band, e))?;
                    if emit {
                        levels[band] = intensity_to_db(value);
                        if !levels[band].is_finite() {
                            return Err((band, DomainFault::NonFinite));
                        }
                    }
                }
            }
            #[cfg(target_arch = "x86_64")]
            Self::Avx2(groups) => unsafe {
                for (group, state) in groups.iter_mut().enumerate() {
                    let values = state
                        .advance_checked(sample)
                        .map_err(|(lane, e)| (group * 4 + lane, e))?;
                    if emit {
                        let mut lanes = [0.0; 4];
                        std::arch::x86_64::_mm256_storeu_pd(lanes.as_mut_ptr(), values);
                        for lane in 0..4 {
                            levels[group * 4 + lane] = intensity_to_db(lanes[lane]);
                            if !levels[group * 4 + lane].is_finite() {
                                return Err((group * 4 + lane, DomainFault::NonFinite));
                            }
                        }
                    }
                }
            },
        }
        Ok(emit.then_some(levels))
    }

    pub(super) fn reset(&mut self) {
        match self {
            Self::Scalar(states) => states.iter_mut().for_each(TolBandState::reset),
            #[cfg(target_arch = "x86_64")]
            Self::Avx2(groups) => groups.iter_mut().for_each(|state| unsafe { state.reset() }),
        }
    }

    #[cfg(test)]
    pub(super) fn inject_raw_bits(&mut self, band: usize, field: usize, bits: u64) {
        assert!(band < 28, "TOL band out of range: {band}");
        match self {
            Self::Scalar(states) => states[band].inject_raw_bits(field, bits),
            #[cfg(target_arch = "x86_64")]
            Self::Avx2(groups) => groups[band / 4].inject_raw_bits(band % 4, field, bits),
        }
    }

    #[cfg(test)]
    pub(super) fn ptr_and_align(&self) -> (usize, usize) {
        match self {
            Self::Scalar(states) => (
                states.as_ptr() as usize,
                std::mem::align_of::<TolBandState>(),
            ),
            #[cfg(target_arch = "x86_64")]
            Self::Avx2(groups) => (
                groups.as_ptr() as usize,
                std::mem::align_of::<TolGroupState>(),
            ),
        }
    }

    #[cfg(test)]
    pub(super) fn coeff_bits(&self) -> Vec<u64> {
        match self {
            Self::Scalar(states) => states.iter().flat_map(|s| s.coeff_bits()).collect(),
            #[cfg(target_arch = "x86_64")]
            Self::Avx2(groups) => groups.iter().flat_map(|g| g.coeff_bits()).collect(),
        }
    }

    #[cfg(test)]
    pub(super) fn numeric_bits(&self) -> [u64; 252] {
        let mut out = [0u64; 252];
        match self {
            Self::Scalar(states) => {
                for (band, s) in states.iter().enumerate() {
                    out[band * 9..band * 9 + 9].copy_from_slice(&s.numeric_bits());
                }
            }
            #[cfg(target_arch = "x86_64")]
            Self::Avx2(groups) => {
                for (group, g) in groups.iter().enumerate() {
                    out[group * 36..group * 36 + 36].copy_from_slice(&g.numeric_bits());
                }
            }
        }
        out
    }
}
