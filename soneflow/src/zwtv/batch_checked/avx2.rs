//! AVX2 kernels of the batch stages: a group of four bands for the levels
//! stage (`Avx2Levels`) and for the nonlinear decay stage (`Avx2Group`).
use super::nl_unit::{run_unit, NlHooks, NlUnit};
use super::shared::{mxcsr_qualified, qualified_mxcsr, Fault, StageCtx};
use super::tol_unit::{run_levels_unit, Cursor, TolHooks, TolUnit};
use crate::numeric_guard::DomainFault;
use crate::zwtv::grid::DEC_FACTOR;
use crate::zwtv::nonlinear_decay::{NlConsts, NlGroupState, NlInit};
use crate::zwtv::third_octave_levels::{intensity_to_db, TolGroupState};
use std::arch::x86_64::*;

/// The four lanes of `value` as levels.
#[target_feature(enable = "avx2,fma")]
#[inline]
unsafe fn lane_levels(value: __m256d) -> [f64; 4] {
    let mut lanes = [0.0; 4];
    // SAFETY: `lanes` holds four f64; unaligned store.
    unsafe { _mm256_storeu_pd(lanes.as_mut_ptr(), value) };
    lanes.map(intensity_to_db)
}

/// Unchecked steps over `block` from `at`, writing four levels per emission
/// into the band-major `out` (`n` slots per band): the samples before the
/// next phase 0 first, then one `DEC_FACTOR` chunk per output slot. Returns
/// the cursor after the block and whether every level is finite.
#[target_feature(enable = "avx2,fma")]
#[inline]
unsafe fn scan_group(
    state: &mut TolGroupState,
    block: &[f64],
    out: &mut [f64],
    n: usize,
    at: Cursor,
) -> (Cursor, bool) {
    let lead = ((DEC_FACTOR - at.phase) % DEC_FACTOR).min(block.len());
    let (head, rest) = block.split_at(lead);
    for &sample in head {
        // SAFETY: this function's own target features are enabled.
        let _ = unsafe { state.advance(sample) };
    }
    let mut nonfinite_acc = 0u64;
    let e = at.emission;
    let (r0, tail) = out.split_at_mut(n);
    let (r1, tail) = tail.split_at_mut(n);
    let (r2, r3) = tail.split_at_mut(n);
    for (chunk, ((s0, s1), (s2, s3))) in rest.chunks(DEC_FACTOR).zip(
        r0[e..]
            .iter_mut()
            .zip(r1[e..].iter_mut())
            .zip(r2[e..].iter_mut().zip(r3[e..].iter_mut())),
    ) {
        if let Some((&first, tail)) = chunk.split_first() {
            // SAFETY: this function's own target features are enabled.
            let [l0, l1, l2, l3] = unsafe { lane_levels(state.advance(first)) };
            nonfinite_acc |= crate::numeric_guard::nonfinite_mask_bit(l0)
                | crate::numeric_guard::nonfinite_mask_bit(l1)
                | crate::numeric_guard::nonfinite_mask_bit(l2)
                | crate::numeric_guard::nonfinite_mask_bit(l3);
            *s0 = l0;
            *s1 = l1;
            *s2 = l2;
            *s3 = l3;
            for &sample in tail {
                // SAFETY: as above.
                let _ = unsafe { state.advance(sample) };
            }
        }
    }
    let after = at.advanced(block.len(), rest.len().div_ceil(DEC_FACTOR));
    (after, (nonfinite_acc as i64) >= 0)
}

/// Checked steps (R) over `signal[pos..end]` from `at`; the first fault is
/// returned with its sample index and band.
#[inline(never)]
#[target_feature(enable = "avx2,fma")]
unsafe fn scan_group_checked(
    state: &mut TolGroupState,
    signal: &[f64],
    (pos, end): (usize, usize),
    at: Cursor,
    out: &mut [f64],
    (n, group): (usize, usize),
) -> Result<(), Fault> {
    let (mut phase, mut emission) = (at.phase, at.emission);
    for (offset, &sample) in signal[pos..end].iter().enumerate() {
        let i = pos + offset;
        // SAFETY: this function's own target features are enabled.
        let value = unsafe { state.advance_checked(sample) }
            .map_err(|(lane, r)| (i, group * 4 + lane, r))?;
        if phase == 0 {
            // SAFETY: as above.
            let lanes = unsafe { lane_levels(value) };
            for (lane, &level) in lanes.iter().enumerate() {
                if !level.is_finite() {
                    return Err((i, group * 4 + lane, DomainFault::NonFinite));
                }
                out[lane * n + emission] = level;
            }
            emission += 1;
        }
        phase += 1;
        if phase == DEC_FACTOR {
            phase = 0;
        }
    }
    Ok(())
}

/// A group of four bands (`group * 4..group * 4 + 4`) and its output rows.
/// Built only by [`levels`], which requires AVX2 and FMA; every `unsafe`
/// call below relies on that.
struct Avx2Levels<'a> {
    state: TolGroupState,
    signal: &'a [f64],
    out: &'a mut [f64],
    n: usize,
    group: usize,
}

impl TolUnit for Avx2Levels<'_> {
    type Snapshot = TolGroupState;
    fn len(&self) -> usize {
        self.signal.len()
    }
    fn scan(&mut self, pos: usize, end: usize, at: Cursor) -> (Cursor, bool) {
        // SAFETY: AVX2 and FMA are available (see `Avx2Levels`).
        unsafe {
            scan_group(
                &mut self.state,
                &self.signal[pos..end],
                self.out,
                self.n,
                at,
            )
        }
    }
    fn scan_checked(&mut self, pos: usize, end: usize, at: Cursor) -> Result<(), Fault> {
        let (signal, n, group) = (self.signal, self.n, self.group);
        // SAFETY: AVX2 and FMA are available (see `Avx2Levels`).
        unsafe {
            scan_group_checked(
                &mut self.state,
                signal,
                (pos, end),
                at,
                self.out,
                (n, group),
            )
        }
    }
    fn state_valid(&self) -> bool {
        // SAFETY: AVX2 and FMA are available (see `Avx2Levels`).
        unsafe { self.state.validate_numeric() }.is_ok()
    }
    fn snapshot(&self) -> TolGroupState {
        // SAFETY: AVX2 and FMA are available (see `Avx2Levels`).
        unsafe { self.state.copy_exact() }
    }
    fn restore(&mut self, snapshot: TolGroupState) {
        self.state = snapshot;
    }
    fn reset(&mut self) {
        // SAFETY: AVX2 and FMA are available (see `Avx2Levels`).
        self.state = unsafe { TolGroupState::new(self.group) };
    }
    #[cfg(test)]
    fn inject(&mut self, band: usize, field: usize, raw: u64) {
        self.state
            .inject_raw_bits(band - self.group * 4, field, raw);
    }
}

/// One group of four bands of the levels stage (terms: see `crate::numeric_guard`).
#[target_feature(enable = "avx2,fma")]
pub(super) unsafe fn levels<H: TolHooks>(
    signal: &[f64],
    group: usize,
    out: &mut [f64],
    n: usize,
    hooks: H,
) -> Result<(), Fault> {
    // Read the worker MXCSR before constructing the group state.
    let entry_mxcsr = qualified_mxcsr();
    // SAFETY: this function's own target features are enabled.
    let state = unsafe { TolGroupState::new(group) };
    let mut unit = Avx2Levels {
        state,
        signal,
        out,
        n,
        group,
    };
    run_levels_unit(&mut unit, entry_mxcsr, hooks)
}

#[target_feature(enable = "avx2,fma")]
#[inline]
unsafe fn load(core: &[f64], n: usize, band: usize, t: usize) -> __m256d {
    _mm256_set_pd(
        core[(band + 3) * n + t],
        core[(band + 2) * n + t],
        core[(band + 1) * n + t],
        core[band * n + t],
    )
}

/// Core values of frame `t` and of the next frame (zero after the last), four lanes.
#[target_feature(enable = "avx2,fma")]
#[inline]
unsafe fn load_frame(core: &[f64], n: usize, band: usize, t: usize) -> (__m256d, __m256d) {
    // SAFETY: this function's own target features are enabled.
    let row = unsafe { load(core, n, band, t) };
    let next = if t + 1 < n {
        // SAFETY: as above.
        unsafe { load(core, n, band, t + 1) }
    } else {
        _mm256_setzero_pd()
    };
    (row, next)
}

/// Write the four lanes of frame `t` to the band-major output.
#[target_feature(enable = "avx2,fma")]
#[inline]
unsafe fn store_frame(out: &mut [f64], n: usize, t: usize, value: __m256d) {
    let mut lanes = [0.0; 4];
    // SAFETY: `lanes` holds four f64; unaligned store.
    unsafe { _mm256_storeu_pd(lanes.as_mut_ptr(), value) };
    for lane in 0..4 {
        out[lane * n + t] = lanes[lane];
    }
}

#[cold]
#[inline(never)]
#[target_feature(enable = "avx2,fma")]
unsafe fn nonlinear_entry_fallback(
    state: &mut NlGroupState,
    row: __m256d,
    next: __m256d,
    c: &NlConsts,
    t: usize,
    band: usize,
) -> Result<__m256d, Fault> {
    // D0 entry failure: no speculation occurred, no MXCSR restore; delegate frame `t` to R.
    // SAFETY: this function's own target features are enabled.
    unsafe { state.advance_frame_checked(row, next, c) }.map_err(|(lane, r)| (t, band + lane, r))
}

/// Unchecked steps of frames `start..n`; the slices and constants are
/// arguments so the loop keeps them in registers.
#[target_feature(enable = "avx2,fma")]
#[allow(clippy::too_many_arguments)]
unsafe fn advance_group<F: Fn()>(
    state: &mut NlGroupState,
    core: &[f64],
    out: &mut [f64],
    n: usize,
    band: usize,
    c: &NlConsts,
    start: usize,
    each: F,
) {
    for t in start..n {
        each();
        // SAFETY: this function's own target features are enabled.
        let (row, next) = unsafe { load_frame(core, n, band, t) };
        // SAFETY: as above.
        let value = unsafe { state.advance_frame(row, next, c) };
        // SAFETY: as above.
        unsafe { store_frame(out, n, t, value) };
    }
}

/// A group of four bands starting at `band`, read from the band-major core.
/// Built only by [`nonlinear`], which requires AVX2 and FMA; every `unsafe`
/// call below relies on that.
struct Avx2Group<'a> {
    state: NlGroupState,
    c: NlConsts,
    core: &'a [f64],
    out: &'a mut [f64],
    n: usize,
    band: usize,
}

impl NlUnit for Avx2Group<'_> {
    fn frames(&self) -> usize {
        self.n
    }
    fn is_entry_valid(&self) -> bool {
        // SAFETY: AVX2 and FMA are available (see `Avx2Group`).
        unsafe { self.state.is_entry_valid() }
    }
    fn inputs_ok(&self, t: usize) -> bool {
        // SAFETY: AVX2 and FMA are available (see `Avx2Group`).
        unsafe {
            let (row, next) = load_frame(self.core, self.n, self.band, t);
            NlGroupState::core_lanes_valid(row) && NlGroupState::core_lanes_valid(next)
        }
    }
    fn advance(&mut self, t: usize) {
        // SAFETY: AVX2 and FMA are available (see `Avx2Group`).
        unsafe {
            let (row, next) = load_frame(self.core, self.n, self.band, t);
            let value = self.state.advance_frame(row, next, &self.c);
            store_frame(self.out, self.n, t, value);
        }
    }
    fn advance_from<F: Fn()>(&mut self, start: usize, each: F) {
        // SAFETY: AVX2 and FMA are available (see `Avx2Group`).
        unsafe {
            advance_group(
                &mut self.state,
                self.core,
                self.out,
                self.n,
                self.band,
                &self.c,
                start,
                each,
            );
        }
    }
    fn advance_checked(&mut self, t: usize) -> Result<(), Fault> {
        // SAFETY: AVX2 and FMA are available (see `Avx2Group`).
        unsafe {
            let (row, next) = load_frame(self.core, self.n, self.band, t);
            let value = self
                .state
                .advance_frame_checked(row, next, &self.c)
                .map_err(|(lane, r)| (t, self.band + lane, r))?;
            store_frame(self.out, self.n, t, value);
        }
        Ok(())
    }
    fn entry_fallback(&mut self, t: usize) -> Result<(), Fault> {
        // SAFETY: AVX2 and FMA are available (see `Avx2Group`).
        unsafe {
            let (row, next) = load_frame(self.core, self.n, self.band, t);
            let value =
                nonlinear_entry_fallback(&mut self.state, row, next, &self.c, t, self.band)?;
            store_frame(self.out, self.n, t, value);
        }
        Ok(())
    }
    #[cfg(test)]
    fn inject(&mut self, band: usize, field: usize, raw: u64) {
        self.state.inject_raw_bits(band - self.band, field, raw);
    }
}

/// One group of four bands of the NL stage (terms: see `crate::numeric_guard`).
#[target_feature(enable = "avx2,fma")]
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn nonlinear<H: NlHooks>(
    core: &[f64],
    out: &mut [f64],
    n: usize,
    band: usize,
    b: [f64; 6],
    init: NlInit,
    ctx: StageCtx,
    hooks: H,
) -> Result<(), Fault> {
    // Read the worker MXCSR once before seed and frame computation.
    let qualified = ctx.qualified && mxcsr_qualified();
    hooks.on_unit(qualified);
    // SAFETY: this function's own target features are enabled.
    let (c, state) = unsafe {
        let c = NlConsts::new(b);
        let state = NlGroupState::seed(init, load(core, n, band, n - 1), &c);
        (c, state)
    };
    let mut unit = Avx2Group {
        state,
        c,
        core,
        out,
        n,
        band,
    };
    // Due rule d: NL state seeded from input (`MosqitoTail`) is due before first use.
    let due = matches!(init, NlInit::MosqitoTail);
    run_unit(&mut unit, qualified, ctx.validated, due, hooks)
}
