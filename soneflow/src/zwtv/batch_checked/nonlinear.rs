//! Nonlinear temporal decay stage: per-band scalar path and dispatch to the AVX2 groups.
#[cfg(target_arch = "x86_64")]
use super::avx2;
use super::shared::dispatch;
use super::shared::Fault;
#[cfg(test)]
use super::{batch_hooks, test_counters};
use crate::batch_guard;
use crate::error::{NumericFaultStage as Stage, SoneflowError};
use crate::zwtv::nonlinear_decay::{nl_coeffs, NlBandState, NlInit};
use crate::zwtv::par::ParMode;

#[cold]
#[inline(never)]
fn nl_band_entry_fallback(
    state: &mut NlBandState,
    row_t: f64,
    next: f64,
    b: &[f64; 6],
    t: usize,
    band: usize,
) -> Result<f64, Fault> {
    #[cfg(test)]
    test_counters::record_nl_scalar_entry_fallback();
    // D0 entry failure: no speculation occurred, no MXCSR restore, delegate frame `t` to R
    // without modifying/zeroing state or seed so R determines exact coordinate, reason & state.
    state
        .advance_frame_checked(row_t, next, b)
        .map_err(|r| (t, band, r))
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn nl_band_checked_loop(
    state: &mut NlBandState,
    row: &[f64],
    out: &mut [f64],
    b: &[f64; 6],
    band: usize,
    #[cfg(test)] // c9_batch_hook
    band_hooks: &[batch_hooks::BatchHookEvent],
    #[cfg(test)] // c9_batch_hook
    mut due_nl: bool,
    #[cfg(test)] // c9_batch_hook
    hook_ctx: &batch_hooks::StageHookContext,
) -> Result<(), Fault> {
    #[cfg(test)] // c9_batch_hook
    let mut h_idx = 0usize;
    for t in 0..row.len() {
        #[cfg(test)] // c9_batch_hook
        {
            while h_idx < band_hooks.len() && band_hooks[h_idx].sample == t {
                state.inject_raw_bits(band_hooks[h_idx].field, band_hooks[h_idx].raw_u64);
                hook_ctx.record_executed_write();
                due_nl = true;
                h_idx += 1;
            }
            hook_ctx.record_due(3, band, t, due_nl);
        }
        out[t] = state
            .advance_frame_checked(row[t], row.get(t + 1).copied().unwrap_or(0.0), b)
            .map_err(|r| (t, band, r))?;
        #[cfg(test)] // c9_batch_hook
        {
            due_nl = false;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn nl_band(
    row: &[f64],
    out: &mut [f64],
    b: &[f64; 6],
    init: NlInit,
    band: usize,
    caller_qualified: bool,
    core_validated: bool,
    #[cfg(test)] // c9_batch_hook
    hook_ctx: &batch_hooks::StageHookContext,
) -> Result<(), Fault> {
    // Read the worker MXCSR once before seed and frame computation.
    let worker_qualified = crate::numeric_guard::read_unit_mxcsr()
        .is_some_and(crate::numeric_guard::is_mxcsr_qualified);
    let qualified = caller_qualified && worker_qualified;
    #[cfg(test)]
    test_counters::record_nl_scalar(qualified);

    let mut state = NlBandState::seed(init, row[row.len() - 1]);
    // Due rule d: NL state seeded from input (`MosqitoTail`) is due before first use.
    let mut due_nl = matches!(init, NlInit::MosqitoTail);
    #[cfg(test)] // c9_batch_hook
    let band_hooks = hook_ctx.for_range(band, band + 1);

    if !qualified {
        return nl_band_checked_loop(
            &mut state,
            row,
            out,
            b,
            band,
            #[cfg(test)] // c9_batch_hook
            &band_hooks,
            #[cfg(test)] // c9_batch_hook
            due_nl,
            #[cfg(test)] // c9_batch_hook
            hook_ctx,
        );
    }

    #[cfg(test)] // c9_batch_hook
    let need_hook_loop = !band_hooks.is_empty() || hook_ctx.has_due_log();
    #[cfg(not(test))] // c9_batch_hook_else
    let need_hook_loop = false;

    if core_validated && !need_hook_loop {
        let start = if due_nl && !state.is_entry_valid() {
            out[0] = nl_band_entry_fallback(
                &mut state,
                row[0],
                row.get(1).copied().unwrap_or(0.0),
                b,
                0,
                band,
            )?;
            1
        } else {
            0
        };
        for t in start..row.len() {
            #[cfg(test)]
            test_counters::record_nl_scalar_fast_frame();
            let next = if t + 1 < row.len() { row[t + 1] } else { 0.0 };
            out[t] = state.advance_frame(row[t], next, b);
        }
        return Ok(());
    }

    #[cfg(test)] // c9_batch_hook
    let mut h_idx = 0usize;
    for t in 0..row.len() {
        #[cfg(test)] // c9_batch_hook
        {
            while h_idx < band_hooks.len() && band_hooks[h_idx].sample == t {
                state.inject_raw_bits(band_hooks[h_idx].field, band_hooks[h_idx].raw_u64);
                hook_ctx.record_executed_write();
                // Due rule c: a hook wrote NL state.
                due_nl = true;
                h_idx += 1;
            }
            hook_ctx.record_due(3, band, t, due_nl);
        }
        let row_t = row[t];
        let next = if t + 1 < row.len() { row[t + 1] } else { 0.0 };
        let state_ok = !due_nl || state.is_entry_valid();
        let input_ok = core_validated
            || (crate::numeric_guard::is_nonnegative_bounded(
                row_t,
                crate::numeric_guard::CORE_MAX,
            ) && crate::numeric_guard::is_nonnegative_bounded(
                next,
                crate::numeric_guard::CORE_MAX,
            ));
        if !(state_ok && input_ok) {
            out[t] = nl_band_entry_fallback(&mut state, row_t, next, b, t, band)?;
        } else {
            due_nl = false;
            #[cfg(test)]
            test_counters::record_nl_scalar_fast_frame();
            out[t] = state.advance_frame(row_t, next, b);
        }
    }
    Ok(())
}

pub(super) fn nonlinear_with_backend(
    core: &[f64],
    n: usize,
    mode: ParMode,
    init: NlInit,
    core_validated: bool,
    use_avx2: bool,
) -> Result<Vec<f64>, SoneflowError> {
    #[cfg(test)] // c9_batch_hook
    let hook_ctx = batch_hooks::snapshot_stage(3, n);
    let mut out = vec![0.0; core.len()];
    if n == 0 {
        return Ok(out);
    }
    // The caller reads unit qualification before constructing `nl_coeffs()`.
    let caller_qualified = crate::numeric_guard::read_unit_mxcsr()
        .is_some_and(crate::numeric_guard::is_mxcsr_qualified);
    let b = nl_coeffs();
    #[cfg(not(target_arch = "x86_64"))]
    let _ = use_avx2;
    #[cfg(target_arch = "x86_64")]
    if use_avx2 {
        dispatch(&mut out, 4 * n, mode, |g, part| {
            let band = 4 * g;
            if part.len() == n {
                return nl_band(
                    &core[band * n..(band + 1) * n],
                    part,
                    &b,
                    init,
                    band,
                    caller_qualified,
                    core_validated,
                    #[cfg(test)] // c9_batch_hook
                    &hook_ctx,
                );
            }
            // SAFETY: runtime dispatch checks AVX2/FMA; remaining groups have four bands.
            unsafe {
                avx2::nonlinear(
                    core,
                    part,
                    n,
                    band,
                    b,
                    init,
                    caller_qualified,
                    core_validated,
                    #[cfg(test)] // c9_batch_hook
                    &hook_ctx,
                )
            }
        })
        .map_err(|(i, b, r)| batch_guard::fault(Stage::Nonlinear, r, i, Some(b)))?;
        return Ok(out);
    }
    dispatch(&mut out, n, mode, |band, part| {
        nl_band(
            &core[band * n..(band + 1) * n],
            part,
            &b,
            init,
            band,
            caller_qualified,
            core_validated,
            #[cfg(test)] // c9_batch_hook
            &hook_ctx,
        )
    })
    .map_err(|(i, b, r)| batch_guard::fault(Stage::Nonlinear, r, i, Some(b)))?;
    Ok(out)
}

#[allow(dead_code)]
pub(crate) fn nonlinear(
    core: &[f64],
    n: usize,
    mode: ParMode,
    init: NlInit,
) -> Result<Vec<f64>, SoneflowError> {
    nonlinear_with_backend(core, n, mode, init, false, crate::simd::use_avx2())
}

/// Called by `ZwtvProcessor::process_with` immediately after `validate_core(&self.core, n_time)?`
/// on immutable `&self.core`, so the per-frame `row`/`next` validation can be skipped.
pub(crate) fn nonlinear_validated_core(
    core: &[f64],
    n: usize,
    mode: ParMode,
    init: NlInit,
) -> Result<Vec<f64>, SoneflowError> {
    nonlinear_with_backend(core, n, mode, init, true, crate::simd::use_avx2())
}
