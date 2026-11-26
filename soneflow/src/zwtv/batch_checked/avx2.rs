#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "unsafe blocks from 0.1.0 without SAFETY comments; remove once documented"
)]

use super::nl_unit::{run_unit, NlHooks, NlUnit};
use super::shared::{mxcsr_qualified, Fault, StageCtx};
use crate::numeric_guard::DomainFault;
use crate::zwtv::grid::DEC_FACTOR;
use crate::zwtv::nonlinear_decay::{NlConsts, NlGroupState, NlInit};
use crate::zwtv::third_octave_levels::{intensity_to_db, TolGroupState};
use std::arch::x86_64::*;

#[inline(never)]
#[target_feature(enable = "avx2,fma")]
unsafe fn levels_checked(
    signal: &[f64],
    group: usize,
    out: &mut [f64],
    n: usize,
) -> Result<(), (usize, usize, DomainFault)> {
    let mut state = unsafe { TolGroupState::new(group) };
    for (i, &sample) in signal.iter().enumerate() {
        let value = unsafe { state.advance_checked(sample) }
            .map_err(|(lane, r)| (i, group * 4 + lane, r))?;
        if i % DEC_FACTOR == 0 {
            let mut lanes = [0.0; 4];
            unsafe {
                _mm256_storeu_pd(lanes.as_mut_ptr(), value);
            }
            for lane in 0..4 {
                let level = intensity_to_db(lanes[lane]);
                if !level.is_finite() {
                    return Err((i, group * 4 + lane, DomainFault::NonFinite));
                }
                out[lane * n + i / DEC_FACTOR] = level;
            }
        }
    }
    Ok(())
}

#[cold]
#[inline(never)]
#[target_feature(enable = "avx2,fma")]
unsafe fn levels_cold_replay(
    entry_mxcsr: u32,
    signal: &[f64],
    group: usize,
    out: &mut [f64],
    n: usize,
) -> Result<(), (usize, usize, DomainFault)> {
    #[cfg(test)]
    super::test_counters::record_avx2_cold_replay();
    crate::numeric_guard::restore_mxcsr(entry_mxcsr);
    unsafe { levels_checked(signal, group, out, n) }
}

#[inline(never)]
#[target_feature(enable = "avx2,fma")]
unsafe fn levels_fast(
    entry_mxcsr: u32,
    signal: &[f64],
    group: usize,
    out: &mut [f64],
    n: usize,
) -> Result<(), (usize, usize, DomainFault)> {
    #[cfg(test)]
    super::test_counters::record_avx2_fast_block();
    let mut state = unsafe { TolGroupState::new(group) };
    let mut nonfinite_acc = 0u64;
    {
        let (r0, rest) = out.split_at_mut(n);
        let (r1, rest) = rest.split_at_mut(n);
        let (r2, r3) = rest.split_at_mut(n);
        for (chunk, ((s0, s1), (s2, s3))) in signal.chunks(DEC_FACTOR).zip(
            r0.iter_mut()
                .zip(r1.iter_mut())
                .zip(r2.iter_mut().zip(r3.iter_mut())),
        ) {
            if let Some((&first, tail)) = chunk.split_first() {
                let value = unsafe { state.advance(first) };
                let mut lanes = [0.0; 4];
                unsafe {
                    _mm256_storeu_pd(lanes.as_mut_ptr(), value);
                }
                let l0 = intensity_to_db(lanes[0]);
                let l1 = intensity_to_db(lanes[1]);
                let l2 = intensity_to_db(lanes[2]);
                let l3 = intensity_to_db(lanes[3]);
                nonfinite_acc |= crate::numeric_guard::nonfinite_mask_bit(l0)
                    | crate::numeric_guard::nonfinite_mask_bit(l1)
                    | crate::numeric_guard::nonfinite_mask_bit(l2)
                    | crate::numeric_guard::nonfinite_mask_bit(l3);
                *s0 = l0;
                *s1 = l1;
                *s2 = l2;
                *s3 = l3;
                for &sample in tail {
                    let _ = unsafe { state.advance(sample) };
                }
            }
        }
    }
    if (nonfinite_acc as i64) < 0 || unsafe { state.validate_numeric() }.is_err() {
        return unsafe { levels_cold_replay(entry_mxcsr, signal, group, out, n) };
    }
    Ok(())
}

#[cfg(test)]
struct BatchTolAvx2Snapshot {
    state: TolGroupState,
    start_sample: usize,
    phase: usize,
    emission_idx: usize,
}

#[cfg(test)]
#[inline(never)]
#[target_feature(enable = "avx2,fma")]
unsafe fn levels_checked_with_hooks(
    signal: &[f64],
    group: usize,
    out: &mut [f64],
    n: usize,
    hooks: &[super::batch_hooks::BatchHookEvent],
    ctx: &super::batch_hooks::StageHookContext,
) -> Result<(), (usize, usize, DomainFault)> {
    let mut state = unsafe { TolGroupState::new(group) };
    let mut h_idx = 0usize;
    for (i, &sample) in signal.iter().enumerate() {
        while h_idx < hooks.len() && hooks[h_idx].sample == i {
            let ev = hooks[h_idx];
            state.inject_raw_bits(ev.band - group * 4, ev.field, ev.raw_u64);
            ctx.record_executed_write();
            h_idx += 1;
        }
        let value = unsafe { state.advance_checked(sample) }
            .map_err(|(lane, r)| (i, group * 4 + lane, r))?;
        if i % DEC_FACTOR == 0 {
            let mut lanes = [0.0; 4];
            unsafe {
                _mm256_storeu_pd(lanes.as_mut_ptr(), value);
            }
            for lane in 0..4 {
                let level = intensity_to_db(lanes[lane]);
                if !level.is_finite() {
                    return Err((i, group * 4 + lane, DomainFault::NonFinite));
                }
                out[lane * n + i / DEC_FACTOR] = level;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[inline(never)]
#[target_feature(enable = "avx2,fma")]
#[allow(clippy::too_many_arguments)]
unsafe fn levels_checked_from_snapshot(
    mut state: TolGroupState,
    start_sample: usize,
    mut phase: usize,
    mut emission_idx: usize,
    signal_slice: &[f64],
    group: usize,
    out: &mut [f64],
    n: usize,
) -> Result<TolGroupState, (usize, usize, DomainFault)> {
    for (offset, &sample) in signal_slice.iter().enumerate() {
        let i = start_sample + offset;
        let value = unsafe { state.advance_checked(sample) }
            .map_err(|(lane, r)| (i, group * 4 + lane, r))?;
        if phase == 0 {
            let mut lanes = [0.0; 4];
            unsafe {
                _mm256_storeu_pd(lanes.as_mut_ptr(), value);
            }
            for lane in 0..4 {
                let level = intensity_to_db(lanes[lane]);
                if !level.is_finite() {
                    return Err((i, group * 4 + lane, DomainFault::NonFinite));
                }
                out[lane * n + emission_idx] = level;
            }
            emission_idx += 1;
        }
        phase += 1;
        if phase == DEC_FACTOR {
            phase = 0;
        }
    }
    Ok(state)
}

#[cfg(test)]
#[cold]
#[inline(never)]
#[target_feature(enable = "avx2,fma")]
unsafe fn levels_cold_replay_from_snapshot(
    entry_mxcsr: u32,
    snap: BatchTolAvx2Snapshot,
    signal_slice: &[f64],
    group: usize,
    out: &mut [f64],
    n: usize,
) -> Result<TolGroupState, (usize, usize, DomainFault)> {
    #[cfg(test)]
    super::test_counters::record_avx2_cold_replay();
    crate::numeric_guard::restore_mxcsr(entry_mxcsr);
    unsafe {
        levels_checked_from_snapshot(
            snap.state,
            snap.start_sample,
            snap.phase,
            snap.emission_idx,
            signal_slice,
            group,
            out,
            n,
        )
    }
}

#[cfg(test)]
#[cold]
#[inline(never)]
#[target_feature(enable = "avx2,fma")]
unsafe fn levels_cold_replay_reconstruct(
    entry_mxcsr: u32,
    signal_slice: &[f64],
    group: usize,
    out: &mut [f64],
    n: usize,
) -> Result<TolGroupState, (usize, usize, DomainFault)> {
    #[cfg(test)]
    super::test_counters::record_avx2_cold_replay();
    crate::numeric_guard::restore_mxcsr(entry_mxcsr);
    let fresh = unsafe { TolGroupState::new(group) };
    unsafe { levels_checked_from_snapshot(fresh, 0, 0, 0, signal_slice, group, out, n) }
}

#[cfg(test)]
#[inline(never)]
#[target_feature(enable = "avx2,fma")]
unsafe fn levels_fast_with_hooks(
    entry_mxcsr: u32,
    signal: &[f64],
    group: usize,
    out: &mut [f64],
    n: usize,
    hooks: &[super::batch_hooks::BatchHookEvent],
    ctx: &super::batch_hooks::StageHookContext,
) -> Result<(), (usize, usize, DomainFault)> {
    let mut state = unsafe { TolGroupState::new(group) };
    let mut pos = 0usize;
    let mut phase = 0usize;
    let mut emission_idx = 0usize;
    let mut h_idx = 0usize;
    let mut due_tol = false;
    let mut block_entry_mxcsr = entry_mxcsr;

    while pos < signal.len() {
        let mut hook_snap: Option<BatchTolAvx2Snapshot> = None;
        if h_idx < hooks.len() && hooks[h_idx].sample == pos {
            while h_idx < hooks.len() && hooks[h_idx].sample == pos {
                let ev = hooks[h_idx];
                state.inject_raw_bits(ev.band - group * 4, ev.field, ev.raw_u64);
                ctx.record_executed_write();
                h_idx += 1;
            }
            // Due rule c: a hook wrote TOL state.
            due_tol = true;
            ctx.record_due(1, group * 4, pos, due_tol);
            // Post-hook snapshot preserves all 36 numeric values, all 36 construction coeff bits,
            // phase, and original sample / emission indices.
            hook_snap = Some(BatchTolAvx2Snapshot {
                state: unsafe { state.copy_exact() },
                start_sample: pos,
                phase,
                emission_idx,
            });
            // Capture the block entry MXCSR once per hook-delimited block.
            block_entry_mxcsr = crate::numeric_guard::read_unit_mxcsr().unwrap_or(entry_mxcsr);
        }

        let end = hooks.get(h_idx).map_or(signal.len(), |h| h.sample);
        assert!(end > pos && end <= signal.len());
        let block = &signal[pos..end];

        if due_tol {
            if unsafe { state.validate_numeric() }.is_err() {
                assert!(
                    hooks.get(h_idx).is_none_or(|h| h.sample >= end),
                    "STOP CONDITION: batch AVX2 TOL hook fell inside entry-due replay interval [{}..{})",
                    pos,
                    end
                );
                let snap = hook_snap.expect("due_tol entry fault must have snapshot");
                state = unsafe {
                    levels_checked_from_snapshot(
                        snap.state,
                        snap.start_sample,
                        snap.phase,
                        snap.emission_idx,
                        block,
                        group,
                        out,
                        n,
                    )
                }?;
                pos = end;
                phase = pos % DEC_FACTOR;
                emission_idx = pos.div_ceil(DEC_FACTOR);
                due_tol = true;
                continue;
            }
            due_tol = false;
        }

        #[cfg(test)]
        super::test_counters::record_avx2_fast_block();
        let mut nonfinite_acc = 0u64;
        let mut work_phase = phase;
        let mut work_emission = emission_idx;
        for &sample in block {
            let value = unsafe { state.advance(sample) };
            if work_phase == 0 {
                let mut lanes = [0.0; 4];
                unsafe {
                    _mm256_storeu_pd(lanes.as_mut_ptr(), value);
                }
                let l0 = intensity_to_db(lanes[0]);
                let l1 = intensity_to_db(lanes[1]);
                let l2 = intensity_to_db(lanes[2]);
                let l3 = intensity_to_db(lanes[3]);
                nonfinite_acc |= crate::numeric_guard::nonfinite_mask_bit(l0)
                    | crate::numeric_guard::nonfinite_mask_bit(l1)
                    | crate::numeric_guard::nonfinite_mask_bit(l2)
                    | crate::numeric_guard::nonfinite_mask_bit(l3);
                out[work_emission] = l0;
                out[n + work_emission] = l1;
                out[2 * n + work_emission] = l2;
                out[3 * n + work_emission] = l3;
                work_emission += 1;
            }
            work_phase += 1;
            if work_phase == DEC_FACTOR {
                work_phase = 0;
            }
        }

        if (nonfinite_acc as i64) < 0 || unsafe { state.validate_numeric() }.is_err() {
            assert!(
                hooks.get(h_idx).is_none_or(|h| h.sample >= end),
                "STOP CONDITION: batch AVX2 TOL hook fell inside cold replay interval [{}..{})",
                pos,
                end
            );
            state = if let Some(snap) = hook_snap {
                unsafe {
                    levels_cold_replay_from_snapshot(block_entry_mxcsr, snap, block, group, out, n)
                }?
            } else {
                assert_eq!(
                    pos, 0,
                    "unhooked batch AVX2 TOL block must start at sample 0"
                );
                unsafe { levels_cold_replay_reconstruct(block_entry_mxcsr, block, group, out, n) }?
            };
            pos = end;
            phase = pos % DEC_FACTOR;
            emission_idx = pos.div_ceil(DEC_FACTOR);
            due_tol = true;
        } else {
            pos = end;
            phase = work_phase;
            emission_idx = work_emission;
        }
    }
    Ok(())
}

#[target_feature(enable = "avx2,fma")]
pub(super) unsafe fn levels(
    signal: &[f64],
    group: usize,
    out: &mut [f64],
    n: usize,
    #[cfg(test)] hook_ctx: &super::batch_hooks::StageHookContext,
) -> Result<(), (usize, usize, DomainFault)> {
    let entry_mxcsr = crate::numeric_guard::read_unit_mxcsr();
    #[cfg(test)]
    let group_hooks = hook_ctx.for_range(group * 4, group * 4 + 4);
    if let Some(entry_mxcsr) = entry_mxcsr.filter(|&m| crate::numeric_guard::is_mxcsr_qualified(m))
    {
        #[cfg(test)]
        super::test_counters::record_avx2(true);
        #[cfg(test)]
        if !group_hooks.is_empty() {
            return unsafe {
                levels_fast_with_hooks(entry_mxcsr, signal, group, out, n, &group_hooks, hook_ctx)
            };
        }
        unsafe { levels_fast(entry_mxcsr, signal, group, out, n) }
    } else {
        #[cfg(test)]
        super::test_counters::record_avx2(false);
        #[cfg(test)]
        if !group_hooks.is_empty() {
            return unsafe {
                levels_checked_with_hooks(signal, group, out, n, &group_hooks, hook_ctx)
            };
        }
        unsafe { levels_checked(signal, group, out, n) }
    }
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
