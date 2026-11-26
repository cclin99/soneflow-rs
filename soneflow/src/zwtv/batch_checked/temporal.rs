//! Temporal weighting stage (single sequential state), and its hooks.
//!
//! The product passes [`NoTwHooks`]; unit tests pass [`CtxTwHooks`], which
//! writes scheduled state and records due flags and path counters.
use super::shared::mxcsr_qualified;
#[cfg(test)]
use super::{batch_hooks, test_counters};
use crate::batch_guard;
use crate::error::{NumericFaultStage as Stage, SoneflowError};
use crate::zwtv::temporal_weighting::TwState;

/// Observation points of the temporal weighting stage.
pub(super) trait TwHooks {
    /// Before frame `i`: apply the writes scheduled at `i` (each sets `due`), then record `due`.
    fn before_frame(&mut self, state: &mut TwState, i: usize, due: &mut bool);
    fn on_stage(&self, qualified: bool);
    fn on_fast_frame(&self);
    fn on_entry_fallback(&self);
}

/// Product hooks: nothing scheduled, nothing recorded.
pub(super) struct NoTwHooks;

impl TwHooks for NoTwHooks {
    #[inline(always)]
    fn before_frame(&mut self, _: &mut TwState, _: usize, _: &mut bool) {}
    #[inline(always)]
    fn on_stage(&self, _: bool) {}
    #[inline(always)]
    fn on_fast_frame(&self) {}
    #[inline(always)]
    fn on_entry_fallback(&self) {}
}

/// Test hooks: the scheduled TW writes, the due log and the TW counters.
#[cfg(test)]
pub(super) struct CtxTwHooks<'a> {
    ctx: &'a batch_hooks::StageHookContext,
    events: Vec<batch_hooks::BatchHookEvent>,
    idx: usize,
}

#[cfg(test)]
impl<'a> CtxTwHooks<'a> {
    pub(super) fn new(ctx: &'a batch_hooks::StageHookContext) -> Self {
        Self {
            ctx,
            events: ctx.for_range(0, 1),
            idx: 0,
        }
    }
}

#[cfg(test)]
impl TwHooks for CtxTwHooks<'_> {
    fn before_frame(&mut self, state: &mut TwState, i: usize, due: &mut bool) {
        while self.idx < self.events.len() && self.events[self.idx].sample == i {
            let e = self.events[self.idx];
            state.inject_raw_bits(e.field, e.raw_u64);
            self.ctx.record_executed_write();
            // Due rule c: a hook wrote TW state.
            *due = true;
            self.idx += 1;
        }
        self.ctx.record_due(5, 0, i, *due);
    }
    fn on_stage(&self, qualified: bool) {
        test_counters::record_tw(qualified);
    }
    fn on_fast_frame(&self) {
        test_counters::record_tw_fast_frame();
    }
    fn on_entry_fallback(&self) {
        test_counters::record_tw_entry_fallback();
    }
}

#[cold]
#[inline(never)]
fn tw_entry_fallback(state: &mut TwState, value: f64, i: usize) -> Result<f64, SoneflowError> {
    // D0 entry failure: no speculation, no MXCSR restore, delegate frame `i` to R.
    state
        .advance_checked(value)
        .map_err(|r| batch_guard::fault(Stage::TemporalWeighting, r, i, None))
}

/// Every frame through R (unqualified MXCSR).
#[inline(never)]
fn temporal_checked_loop<H: TwHooks>(
    state: &mut TwState,
    values: &[f64],
    hooks: &mut H,
) -> Result<Vec<f64>, SoneflowError> {
    let mut due = false;
    let mut out = Vec::with_capacity(values.len());
    for (i, &value) in values.iter().enumerate() {
        hooks.before_frame(state, i, &mut due);
        out.push(
            state
                .advance_checked(value)
                .map_err(|r| batch_guard::fault(Stage::TemporalWeighting, r, i, None))?,
        );
        due = false;
    }
    Ok(out)
}

/// The stage (terms: see `crate::numeric_guard`): the unchecked step with
/// per-frame entry checks when the MXCSR qualifies, otherwise R.
pub(super) fn temporal_with<H: TwHooks>(
    values: &[f64],
    mut hooks: H,
) -> Result<Vec<f64>, SoneflowError> {
    // The caller reads MXCSR once before `TwState::new()` coefficient construction.
    let qualified = mxcsr_qualified();
    hooks.on_stage(qualified);
    let mut state = TwState::new();
    if !qualified {
        return temporal_checked_loop(&mut state, values, &mut hooks);
    }
    let mut due = false;
    let mut out = Vec::with_capacity(values.len());
    for (i, &value) in values.iter().enumerate() {
        hooks.before_frame(&mut state, i, &mut due);
        let state_ok = !due || state.is_entry_valid();
        let input_ok =
            crate::numeric_guard::is_nonnegative_bounded(value, crate::numeric_guard::TW_INPUT_MAX);
        if !(state_ok && input_ok) {
            hooks.on_entry_fallback();
            out.push(tw_entry_fallback(&mut state, value, i)?);
        } else {
            due = false;
            hooks.on_fast_frame();
            out.push(state.advance(value));
        }
    }
    Ok(out)
}

pub(crate) fn temporal(values: &[f64]) -> Result<Vec<f64>, SoneflowError> {
    #[cfg(test)]
    let hook_ctx = batch_hooks::snapshot_stage(5, values.len());
    #[cfg(test)]
    let hooks = CtxTwHooks::new(&hook_ctx);
    #[cfg(not(test))]
    let hooks = NoTwHooks;
    temporal_with(values, hooks)
}
