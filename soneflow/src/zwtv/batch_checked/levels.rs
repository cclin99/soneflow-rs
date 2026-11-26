//! Third-octave levels stage: checked controller (R), scalar fast path and dispatch.
#[cfg(target_arch = "x86_64")]
use super::avx2;
use super::shared::dispatch;
use super::shared::Fault;
#[cfg(test)]
use super::{batch_hooks, test_counters};
use crate::batch_guard;
use crate::error::{NumericFaultStage as Stage, SoneflowError};
use crate::numeric_guard::DomainFault;
use crate::zwtv::grid::DEC_FACTOR;
use crate::zwtv::par::ParMode;
use crate::zwtv::third_octave_levels::{intensity_to_db, TolBandState};

#[inline(never)]
fn levels_scalar_checked(signal: &[f64], band: usize, row: &mut [f64]) -> Result<(), Fault> {
    let mut state = TolBandState::new(band);
    for (i, &sample) in signal.iter().enumerate() {
        let value = state.advance_checked(sample).map_err(|r| (i, band, r))?;
        if i % DEC_FACTOR == 0 {
            let level = intensity_to_db(value);
            if !level.is_finite() {
                return Err((i, band, DomainFault::NonFinite));
            }
            row[i / DEC_FACTOR] = level;
        }
    }
    Ok(())
}

#[cfg(target_arch = "x86_64")]
#[cold]
#[inline(never)]
fn levels_scalar_cold_replay(
    entry_mxcsr: u32,
    signal: &[f64],
    band: usize,
    row: &mut [f64],
) -> Result<(), Fault> {
    #[cfg(test)]
    test_counters::record_scalar_cold_replay();
    crate::numeric_guard::restore_mxcsr(entry_mxcsr);
    levels_scalar_checked(signal, band, row)
}

#[cfg(target_arch = "x86_64")]
#[inline(never)]
fn levels_scalar_fast(
    entry_mxcsr: u32,
    signal: &[f64],
    band: usize,
    row: &mut [f64],
) -> Result<(), Fault> {
    #[cfg(test)]
    test_counters::record_scalar_fast_block();
    let mut state = TolBandState::new(band);
    let mut nonfinite_acc = 0u64;
    for (out_slot, chunk) in row.iter_mut().zip(signal.chunks(DEC_FACTOR)) {
        if let Some((&first, tail)) = chunk.split_first() {
            let v0 = state.advance(first);
            let level = intensity_to_db(v0);
            nonfinite_acc |= crate::numeric_guard::nonfinite_mask_bit(level);
            *out_slot = level;
            for &sample in tail {
                let _ = state.advance(sample);
            }
        }
    }
    if (nonfinite_acc as i64) < 0 || state.validate_numeric().is_err() {
        return levels_scalar_cold_replay(entry_mxcsr, signal, band, row);
    }
    Ok(())
}

#[cfg(test)] // c9_batch_hook
struct BatchTolScalarSnapshot {
    state: TolBandState,
    start_sample: usize,
    phase: usize,
    emission_idx: usize,
}

#[cfg(test)] // c9_batch_hook
#[inline(never)]
fn levels_scalar_checked_with_hooks(
    signal: &[f64],
    band: usize,
    row: &mut [f64],
    hooks: &[batch_hooks::BatchHookEvent],
    ctx: &batch_hooks::StageHookContext,
) -> Result<(), Fault> {
    let mut state = TolBandState::new(band);
    let mut h_idx = 0usize;
    for (i, &sample) in signal.iter().enumerate() {
        while h_idx < hooks.len() && hooks[h_idx].sample == i {
            state.inject_raw_bits(hooks[h_idx].field, hooks[h_idx].raw_u64);
            ctx.record_executed_write();
            h_idx += 1;
        }
        let value = state.advance_checked(sample).map_err(|r| (i, band, r))?;
        if i % DEC_FACTOR == 0 {
            let level = intensity_to_db(value);
            if !level.is_finite() {
                return Err((i, band, DomainFault::NonFinite));
            }
            row[i / DEC_FACTOR] = level;
        }
    }
    Ok(())
}

#[cfg(test)] // c9_batch_hook
#[inline(never)]
fn levels_scalar_checked_from_snapshot(
    mut state: TolBandState,
    start_sample: usize,
    mut phase: usize,
    mut emission_idx: usize,
    signal_slice: &[f64],
    band: usize,
    row: &mut [f64],
) -> Result<TolBandState, Fault> {
    for (offset, &sample) in signal_slice.iter().enumerate() {
        let i = start_sample + offset;
        let value = state.advance_checked(sample).map_err(|r| (i, band, r))?;
        if phase == 0 {
            let level = intensity_to_db(value);
            if !level.is_finite() {
                return Err((i, band, DomainFault::NonFinite));
            }
            row[emission_idx] = level;
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
#[cfg(target_arch = "x86_64")]
#[cold]
#[inline(never)]
fn levels_scalar_cold_replay_from_snapshot(
    entry_mxcsr: u32,
    snap: BatchTolScalarSnapshot,
    signal_slice: &[f64],
    band: usize,
    row: &mut [f64],
) -> Result<TolBandState, Fault> {
    #[cfg(test)]
    test_counters::record_scalar_cold_replay();
    crate::numeric_guard::restore_mxcsr(entry_mxcsr);
    levels_scalar_checked_from_snapshot(
        snap.state,
        snap.start_sample,
        snap.phase,
        snap.emission_idx,
        signal_slice,
        band,
        row,
    )
}

#[cfg(test)] // c9_batch_hook
#[cfg(target_arch = "x86_64")]
#[cold]
#[inline(never)]
fn levels_scalar_cold_replay_reconstruct(
    entry_mxcsr: u32,
    signal_slice: &[f64],
    band: usize,
    row: &mut [f64],
) -> Result<TolBandState, Fault> {
    #[cfg(test)]
    test_counters::record_scalar_cold_replay();
    crate::numeric_guard::restore_mxcsr(entry_mxcsr);
    let fresh = TolBandState::new(band);
    levels_scalar_checked_from_snapshot(fresh, 0, 0, 0, signal_slice, band, row)
}

#[cfg(test)] // c9_batch_hook
#[cfg(target_arch = "x86_64")]
#[inline(never)]
fn levels_scalar_fast_with_hooks(
    entry_mxcsr: u32,
    signal: &[f64],
    band: usize,
    row: &mut [f64],
    hooks: &[batch_hooks::BatchHookEvent],
    ctx: &batch_hooks::StageHookContext,
) -> Result<(), Fault> {
    let mut state = TolBandState::new(band);
    let mut pos = 0usize;
    let mut phase = 0usize;
    let mut emission_idx = 0usize;
    let mut h_idx = 0usize;
    let mut due_tol = false;
    let mut block_entry_mxcsr = entry_mxcsr;

    while pos < signal.len() {
        let mut hook_snap: Option<BatchTolScalarSnapshot> = None;
        if h_idx < hooks.len() && hooks[h_idx].sample == pos {
            while h_idx < hooks.len() && hooks[h_idx].sample == pos {
                state.inject_raw_bits(hooks[h_idx].field, hooks[h_idx].raw_u64);
                ctx.record_executed_write();
                h_idx += 1;
            }
            // Due rule c: a hook wrote TOL state.
            due_tol = true;
            ctx.record_due(1, band, pos, due_tol);
            // Post-hook snapshot preserves all 9 numeric values, all 18 construction coeff bits,
            // phase, and original sample / emission indices.
            hook_snap = Some(BatchTolScalarSnapshot {
                state: state.copy_exact(),
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
            // c9_scalar_hook_due_check
            if state.validate_numeric().is_err() {
                assert!(
                    hooks.get(h_idx).is_none_or(|h| h.sample >= end),
                    "STOP CONDITION: batch scalar TOL hook fell inside entry-due replay interval [{}..{})",
                    pos,
                    end
                );
                let snap = hook_snap.expect("due_tol entry fault must have snapshot");
                state = levels_scalar_checked_from_snapshot(
                    snap.state,
                    snap.start_sample,
                    snap.phase,
                    snap.emission_idx,
                    block,
                    band,
                    row,
                )?;
                pos = end;
                phase = pos % DEC_FACTOR;
                emission_idx = pos.div_ceil(DEC_FACTOR);
                due_tol = true;
                continue;
            }
            due_tol = false;
        }

        #[cfg(test)]
        test_counters::record_scalar_fast_block();
        let mut nonfinite_acc = 0u64;
        let mut work_phase = phase;
        let mut work_emission = emission_idx;
        for &sample in block {
            let v = state.advance(sample);
            if work_phase == 0 {
                let level = intensity_to_db(v);
                nonfinite_acc |= crate::numeric_guard::nonfinite_mask_bit(level);
                row[work_emission] = level;
                work_emission += 1;
            }
            work_phase += 1;
            if work_phase == DEC_FACTOR {
                work_phase = 0;
            }
        }

        // c9_scalar_hook_d1_check
        if (nonfinite_acc as i64) < 0 || state.validate_numeric().is_err() {
            assert!(
                hooks.get(h_idx).is_none_or(|h| h.sample >= end),
                "STOP CONDITION: batch scalar TOL hook fell inside cold replay interval [{}..{})",
                pos,
                end
            );
            state = if let Some(snap) = hook_snap {
                levels_scalar_cold_replay_from_snapshot(block_entry_mxcsr, snap, block, band, row)?
            } else {
                assert_eq!(
                    pos, 0,
                    "unhooked batch scalar TOL block must start at sample 0"
                );
                levels_scalar_cold_replay_reconstruct(block_entry_mxcsr, block, band, row)?
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

pub(super) fn levels_with_backend(
    signal: &[f64],
    mode: ParMode,
    use_avx2: bool,
) -> Result<(Vec<f64>, usize), SoneflowError> {
    #[cfg(test)] // c9_batch_hook
    let hook_ctx = batch_hooks::snapshot_stage(1, signal.len());
    let n = signal.len().div_ceil(DEC_FACTOR);
    let mut out = vec![0.0; 28 * n];
    #[cfg(not(target_arch = "x86_64"))]
    let _ = use_avx2;
    #[cfg(target_arch = "x86_64")]
    if use_avx2 {
        // SAFETY: caller ensures AVX2 and FMA are available.
        dispatch(&mut out, 4 * n, mode, |g, part| unsafe {
            avx2::levels(
                signal,
                g,
                part,
                n,
                #[cfg(test)] // c9_batch_hook
                &hook_ctx,
            )
        })
        .map_err(|(i, b, r)| batch_guard::fault(Stage::Filter, r, i, Some(b)))?;
        return Ok((out, n));
    }
    dispatch(&mut out, n, mode, |band, row| {
        let entry_mxcsr = crate::numeric_guard::read_unit_mxcsr();
        #[cfg(test)] // c9_batch_hook
        let band_hooks = hook_ctx.for_range(band, band + 1);
        if let Some(entry_mxcsr) =
            entry_mxcsr.filter(|&m| crate::numeric_guard::is_mxcsr_qualified(m))
        {
            #[cfg(test)]
            test_counters::record_scalar(true);
            #[cfg(target_arch = "x86_64")]
            {
                #[cfg(test)] // c9_batch_hook
                if !band_hooks.is_empty() {
                    return levels_scalar_fast_with_hooks(
                        entry_mxcsr,
                        signal,
                        band,
                        row,
                        &band_hooks,
                        &hook_ctx,
                    );
                }
                levels_scalar_fast(entry_mxcsr, signal, band, row)
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                let _ = entry_mxcsr;
                #[cfg(test)] // c9_batch_hook
                if !band_hooks.is_empty() {
                    return levels_scalar_checked_with_hooks(
                        signal,
                        band,
                        row,
                        &band_hooks,
                        &hook_ctx,
                    );
                }
                levels_scalar_checked(signal, band, row)
            }
        } else {
            #[cfg(test)]
            test_counters::record_scalar(false);
            #[cfg(test)] // c9_batch_hook
            if !band_hooks.is_empty() {
                return levels_scalar_checked_with_hooks(signal, band, row, &band_hooks, &hook_ctx);
            }
            levels_scalar_checked(signal, band, row)
        }
    })
    .map_err(|(i, b, r)| batch_guard::fault(Stage::Filter, r, i, Some(b)))?;
    Ok((out, n))
}

pub(crate) fn levels(signal: &[f64], mode: ParMode) -> Result<(Vec<f64>, usize), SoneflowError> {
    levels_with_backend(signal, mode, crate::simd::use_avx2())
}
