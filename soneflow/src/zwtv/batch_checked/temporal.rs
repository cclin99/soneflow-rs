//! Temporal weighting stage (single sequential state).
#[cfg(test)]
use super::{batch_hooks, test_counters};
use crate::batch_guard;
use crate::error::{NumericFaultStage as Stage, SoneflowError};
use crate::zwtv::temporal_weighting::TwState;

#[cold]
#[inline(never)]
fn tw_entry_fallback(state: &mut TwState, value: f64, i: usize) -> Result<f64, SoneflowError> {
    #[cfg(test)]
    test_counters::record_tw_entry_fallback();
    // D0 entry failure: no speculation, no MXCSR restore, delegate frame `i` to R.
    state
        .advance_checked(value)
        .map_err(|r| batch_guard::fault(Stage::TemporalWeighting, r, i, None))
}

#[inline(never)]
fn temporal_checked_loop(
    state: &mut TwState,
    values: &[f64],
    #[cfg(test)] // c9_batch_hook
    tw_hooks: &[batch_hooks::BatchHookEvent],
    #[cfg(test)] // c9_batch_hook
    hook_ctx: &batch_hooks::StageHookContext,
) -> Result<Vec<f64>, SoneflowError> {
    #[cfg(test)] // c9_batch_hook
    let (mut h_idx, mut due_tw) = (0usize, false);
    let mut out = Vec::with_capacity(values.len());
    for (i, &value) in values.iter().enumerate() {
        #[cfg(test)] // c9_batch_hook
        {
            while h_idx < tw_hooks.len() && tw_hooks[h_idx].sample == i {
                state.inject_raw_bits(tw_hooks[h_idx].field, tw_hooks[h_idx].raw_u64);
                hook_ctx.record_executed_write();
                due_tw = true;
                h_idx += 1;
            }
            hook_ctx.record_due(5, 0, i, due_tw);
        }
        out.push(
            state
                .advance_checked(value)
                .map_err(|r| batch_guard::fault(Stage::TemporalWeighting, r, i, None))?,
        );
        #[cfg(test)] // c9_batch_hook
        {
            due_tw = false;
        }
    }
    Ok(out)
}

pub(crate) fn temporal(values: &[f64]) -> Result<Vec<f64>, SoneflowError> {
    #[cfg(test)] // c9_batch_hook
    let hook_ctx = batch_hooks::snapshot_stage(5, values.len());
    // The caller reads MXCSR once before `TwState::new()` coefficient construction.
    let qualified = crate::numeric_guard::read_unit_mxcsr()
        .is_some_and(crate::numeric_guard::is_mxcsr_qualified);
    #[cfg(test)]
    test_counters::record_tw(qualified);

    let mut state = TwState::new();
    #[cfg(test)] // c9_batch_hook
    let tw_hooks = hook_ctx.for_range(0, 1);

    if !qualified {
        return temporal_checked_loop(
            &mut state,
            values,
            #[cfg(test)] // c9_batch_hook
            &tw_hooks,
            #[cfg(test)] // c9_batch_hook
            &hook_ctx,
        );
    }

    #[cfg(test)] // c9_batch_hook
    let (mut h_idx, mut due_tw) = (0usize, false);
    #[cfg(not(test))] // c9_batch_hook_else
    let due_tw = false;

    let mut out = Vec::with_capacity(values.len());
    for (i, &value) in values.iter().enumerate() {
        #[cfg(test)] // c9_batch_hook
        {
            while h_idx < tw_hooks.len() && tw_hooks[h_idx].sample == i {
                state.inject_raw_bits(tw_hooks[h_idx].field, tw_hooks[h_idx].raw_u64);
                hook_ctx.record_executed_write();
                // Due rule c: a hook wrote TW state.
                due_tw = true;
                h_idx += 1;
            }
            hook_ctx.record_due(5, 0, i, due_tw);
        }
        let state_ok = !due_tw || state.is_entry_valid();
        let input_ok =
            crate::numeric_guard::is_nonnegative_bounded(value, crate::numeric_guard::TW_INPUT_MAX);
        if !(state_ok && input_ok) {
            out.push(tw_entry_fallback(&mut state, value, i)?);
        } else {
            #[cfg(test)] // c9_batch_hook
            {
                due_tw = false;
            }
            #[cfg(test)]
            test_counters::record_tw_fast_frame();
            out.push(state.advance(value));
        }
    }
    Ok(out)
}
