#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "unsafe blocks from 0.1.0 without SAFETY comments; remove once documented"
)]

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

#[cfg(test)] // c9_batch_hook
struct BatchTolAvx2Snapshot {
    state: TolGroupState,
    start_sample: usize,
    phase: usize,
    emission_idx: usize,
}

#[cfg(test)] // c9_batch_hook
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

#[cfg(test)] // c9_batch_hook
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

#[cfg(test)] // c9_batch_hook
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

#[cfg(test)] // c9_batch_hook
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

#[cfg(test)] // c9_batch_hook
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
            // c9_avx2_hook_due_check
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

        // c9_avx2_hook_d1_check
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
    #[cfg(test)] // c9_batch_hook
    hook_ctx: &super::batch_hooks::StageHookContext,
) -> Result<(), (usize, usize, DomainFault)> {
    let entry_mxcsr = crate::numeric_guard::read_unit_mxcsr();
    #[cfg(test)] // c9_batch_hook
    let group_hooks = hook_ctx.for_range(group * 4, group * 4 + 4);
    if let Some(entry_mxcsr) = entry_mxcsr.filter(|&m| crate::numeric_guard::is_mxcsr_qualified(m))
    {
        #[cfg(test)]
        super::test_counters::record_avx2(true);
        #[cfg(test)] // c9_batch_hook
        if !group_hooks.is_empty() {
            return unsafe {
                levels_fast_with_hooks(entry_mxcsr, signal, group, out, n, &group_hooks, hook_ctx)
            };
        }
        unsafe { levels_fast(entry_mxcsr, signal, group, out, n) }
    } else {
        #[cfg(test)]
        super::test_counters::record_avx2(false);
        #[cfg(test)] // c9_batch_hook
        if !group_hooks.is_empty() {
            return unsafe {
                levels_checked_with_hooks(signal, group, out, n, &group_hooks, hook_ctx)
            };
        }
        unsafe { levels_checked(signal, group, out, n) }
    }
}

#[target_feature(enable = "avx2,fma")]
unsafe fn load(core: &[f64], n: usize, band: usize, t: usize) -> __m256d {
    _mm256_set_pd(
        core[(band + 3) * n + t],
        core[(band + 2) * n + t],
        core[(band + 1) * n + t],
        core[band * n + t],
    )
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
) -> Result<__m256d, (usize, usize, DomainFault)> {
    #[cfg(test)]
    super::test_counters::record_nl_avx2_entry_fallback();
    // D0 entry failure: no speculation occurred, no MXCSR restore; delegate frame `t` to R.
    unsafe { state.advance_frame_checked(row, next, c) }.map_err(|(lane, r)| (t, band + lane, r))
}

#[inline(never)]
#[target_feature(enable = "avx2,fma")]
#[allow(clippy::too_many_arguments)]
unsafe fn nonlinear_checked_loop(
    state: &mut NlGroupState,
    core: &[f64],
    out: &mut [f64],
    n: usize,
    band: usize,
    c: &NlConsts,
    #[cfg(test)] // c9_batch_hook
    group_hooks: &[super::batch_hooks::BatchHookEvent],
    #[cfg(test)] // c9_batch_hook
    mut due_nl: bool,
    #[cfg(test)] // c9_batch_hook
    hook_ctx: &super::batch_hooks::StageHookContext,
) -> Result<(), (usize, usize, DomainFault)> {
    #[cfg(test)] // c9_batch_hook
    let mut h_idx = 0usize;
    for t in 0..n {
        #[cfg(test)] // c9_batch_hook
        {
            while h_idx < group_hooks.len() && group_hooks[h_idx].sample == t {
                let ev = group_hooks[h_idx];
                state.inject_raw_bits(ev.band - band, ev.field, ev.raw_u64);
                hook_ctx.record_executed_write();
                due_nl = true;
                h_idx += 1;
            }
            hook_ctx.record_due(3, band, t, due_nl);
        }
        let row = unsafe { load(core, n, band, t) };
        let next = if t + 1 < n {
            unsafe { load(core, n, band, t + 1) }
        } else {
            _mm256_setzero_pd()
        };
        let value = unsafe { state.advance_frame_checked(row, next, c) }
            .map_err(|(lane, r)| (t, band + lane, r))?;
        #[cfg(test)] // c9_batch_hook
        {
            due_nl = false;
        }
        let mut lanes = [0.0; 4];
        unsafe {
            _mm256_storeu_pd(lanes.as_mut_ptr(), value);
        }
        for lane in 0..4 {
            out[lane * n + t] = lanes[lane];
        }
    }
    Ok(())
}

#[target_feature(enable = "avx2,fma")]
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn nonlinear(
    core: &[f64],
    out: &mut [f64],
    n: usize,
    band: usize,
    b: [f64; 6],
    init: NlInit,
    caller_qualified: bool,
    core_validated: bool,
    #[cfg(test)] // c9_batch_hook
    hook_ctx: &super::batch_hooks::StageHookContext,
) -> Result<(), (usize, usize, DomainFault)> {
    // Read the worker MXCSR once before seed and frame computation.
    let worker_qualified = crate::numeric_guard::read_unit_mxcsr()
        .is_some_and(crate::numeric_guard::is_mxcsr_qualified);
    let qualified = caller_qualified && worker_qualified;
    #[cfg(test)]
    super::test_counters::record_nl_avx2(qualified);

    let c = unsafe { NlConsts::new(b) };
    let mut state = unsafe { NlGroupState::seed(init, load(core, n, band, n - 1), &c) };
    // Due rule d: NL state seeded from input (`MosqitoTail`) is due before first use.
    let mut due_nl = matches!(init, NlInit::MosqitoTail);
    #[cfg(test)] // c9_batch_hook
    let group_hooks = hook_ctx.for_range(band, band + 4);

    if !qualified {
        return unsafe {
            nonlinear_checked_loop(
                &mut state,
                core,
                out,
                n,
                band,
                &c,
                #[cfg(test)] // c9_batch_hook
                &group_hooks,
                #[cfg(test)] // c9_batch_hook
                due_nl,
                #[cfg(test)] // c9_batch_hook
                hook_ctx,
            )
        };
    }

    #[cfg(test)] // c9_batch_hook
    let need_hook_loop = !group_hooks.is_empty() || hook_ctx.has_due_log();
    #[cfg(not(test))] // c9_batch_hook_else
    let need_hook_loop = false;

    if core_validated && !need_hook_loop {
        let start = if due_nl && !unsafe { state.is_entry_valid() } {
            let row0 = unsafe { load(core, n, band, 0) };
            let next0 = if 1 < n {
                unsafe { load(core, n, band, 1) }
            } else {
                _mm256_setzero_pd()
            };
            let value = unsafe { nonlinear_entry_fallback(&mut state, row0, next0, &c, 0, band) }?;
            let mut lanes = [0.0; 4];
            unsafe {
                _mm256_storeu_pd(lanes.as_mut_ptr(), value);
            }
            for lane in 0..4 {
                out[lane * n] = lanes[lane];
            }
            1
        } else {
            0
        };
        for t in start..n {
            #[cfg(test)]
            super::test_counters::record_nl_avx2_fast_frame();
            let row = unsafe { load(core, n, band, t) };
            let next = if t + 1 < n {
                unsafe { load(core, n, band, t + 1) }
            } else {
                _mm256_setzero_pd()
            };
            let value = unsafe { state.advance_frame(row, next, &c) };
            let mut lanes = [0.0; 4];
            unsafe {
                _mm256_storeu_pd(lanes.as_mut_ptr(), value);
            }
            for lane in 0..4 {
                out[lane * n + t] = lanes[lane];
            }
        }
        return Ok(());
    }

    #[cfg(test)] // c9_batch_hook
    let mut h_idx = 0usize;
    for t in 0..n {
        #[cfg(test)] // c9_batch_hook
        {
            while h_idx < group_hooks.len() && group_hooks[h_idx].sample == t {
                let ev = group_hooks[h_idx];
                state.inject_raw_bits(ev.band - band, ev.field, ev.raw_u64);
                hook_ctx.record_executed_write();
                // Due rule c: a hook wrote NL state.
                due_nl = true;
                h_idx += 1;
            }
            hook_ctx.record_due(3, band, t, due_nl);
        }
        let row = unsafe { load(core, n, band, t) };
        let next = if t + 1 < n {
            unsafe { load(core, n, band, t + 1) }
        } else {
            _mm256_setzero_pd()
        };
        let state_ok = !due_nl || unsafe { state.is_entry_valid() };
        let input_ok = core_validated
            || (unsafe { NlGroupState::core_lanes_valid(row) }
                && unsafe { NlGroupState::core_lanes_valid(next) });
        let value = if !(state_ok && input_ok) {
            unsafe { nonlinear_entry_fallback(&mut state, row, next, &c, t, band) }?
        } else {
            due_nl = false;
            #[cfg(test)]
            super::test_counters::record_nl_avx2_fast_frame();
            unsafe { state.advance_frame(row, next, &c) }
        };
        let mut lanes = [0.0; 4];
        unsafe {
            _mm256_storeu_pd(lanes.as_mut_ptr(), value);
        }
        for lane in 0..4 {
            out[lane * n + t] = lanes[lane];
        }
    }
    Ok(())
}
