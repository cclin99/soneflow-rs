//! The control flow of the nonlinear decay stage, shared by a scalar band
//! (`nonlinear`) and an AVX2 group of four bands (`avx2`), and its hooks.
//!
//! The product passes [`NoNlHooks`]; unit tests pass [`CtxNlHooks`], which
//! writes scheduled state and records due flags and path counters.
use super::shared::Fault;
#[cfg(test)]
use super::{batch_hooks, test_counters};

/// One unit of NL work: a scalar band, or an AVX2 group of four bands.
pub(super) trait NlUnit {
    /// Frames in the unit.
    fn frames(&self) -> usize;
    /// The state may take an unchecked step.
    fn is_entry_valid(&self) -> bool;
    /// The core inputs of frame `t` (and of `t + 1`, or zero) are in range.
    fn inputs_ok(&self, t: usize) -> bool;
    /// Unchecked step of frame `t`.
    fn advance(&mut self, t: usize);
    /// Unchecked steps of frames `start..frames()`; `each` runs before every frame.
    fn advance_from<F: Fn()>(&mut self, start: usize, each: F);
    /// Frame `t` through the checked controller (R).
    fn advance_checked(&mut self, t: usize) -> Result<(), Fault>;
    /// Frame `t` through R after its entry check failed (cold).
    fn entry_fallback(&mut self, t: usize) -> Result<(), Fault>;
    /// Write raw state bits of `band` (test hooks).
    #[cfg(test)]
    fn inject(&mut self, band: usize, field: usize, raw: u64);
}

/// Observation points of one NL unit.
pub(super) trait NlHooks {
    /// Hooks are scheduled or due flags are logged: take the per-frame loop.
    fn needs_loop(&self) -> bool;
    /// Before frame `t`: apply the writes scheduled at `t` (each sets `due`), then record `due`.
    fn before_frame<U: NlUnit>(&mut self, unit: &mut U, t: usize, due: &mut bool);
    fn on_unit(&self, qualified: bool);
    fn on_fast_frame(&self);
    fn on_entry_fallback(&self);
}

/// Run one unit (terms: see `crate::numeric_guard`).
///
/// - `qualified`: the caller and worker MXCSR qualify; otherwise every frame
///   goes through R.
/// - `validated`: the core input was checked before the stage, so the
///   per-frame input check is skipped.
/// - `due`: the seeded state must be checked before first use (due rule d).
#[inline(always)]
pub(super) fn run_unit<U: NlUnit, H: NlHooks>(
    unit: &mut U,
    qualified: bool,
    validated: bool,
    mut due: bool,
    mut hooks: H,
) -> Result<(), Fault> {
    if !qualified {
        return run_checked(unit, due, &mut hooks);
    }
    if validated && !hooks.needs_loop() {
        let start = if due && !unit.is_entry_valid() {
            hooks.on_entry_fallback();
            unit.entry_fallback(0)?;
            1
        } else {
            0
        };
        unit.advance_from(start, || hooks.on_fast_frame());
        return Ok(());
    }
    for t in 0..unit.frames() {
        hooks.before_frame(unit, t, &mut due);
        if (!due || unit.is_entry_valid()) && (validated || unit.inputs_ok(t)) {
            due = false;
            hooks.on_fast_frame();
            unit.advance(t);
        } else {
            hooks.on_entry_fallback();
            unit.entry_fallback(t)?;
        }
    }
    Ok(())
}

/// Every frame through R (unqualified MXCSR).
#[inline(never)]
fn run_checked<U: NlUnit, H: NlHooks>(
    unit: &mut U,
    mut due: bool,
    hooks: &mut H,
) -> Result<(), Fault> {
    for t in 0..unit.frames() {
        hooks.before_frame(unit, t, &mut due);
        unit.advance_checked(t)?;
        due = false;
    }
    Ok(())
}

/// Product hooks: nothing scheduled, nothing recorded. Passed by value: a
/// `&mut` to it would still be a pointer argument.
pub(super) struct NoNlHooks;

impl NlHooks for NoNlHooks {
    #[inline(always)]
    fn needs_loop(&self) -> bool {
        false
    }
    #[inline(always)]
    fn before_frame<U: NlUnit>(&mut self, _: &mut U, _: usize, _: &mut bool) {}
    #[inline(always)]
    fn on_unit(&self, _: bool) {}
    #[inline(always)]
    fn on_fast_frame(&self) {}
    #[inline(always)]
    fn on_entry_fallback(&self) {}
}

/// Test hooks of one unit: the scheduled writes for its bands, the stage's
/// due log and the path counters of its backend.
#[cfg(test)]
pub(super) struct CtxNlHooks<'a> {
    ctx: &'a batch_hooks::StageHookContext,
    events: Vec<batch_hooks::BatchHookEvent>,
    idx: usize,
    band: usize,
    avx2: bool,
}

#[cfg(test)]
impl<'a> CtxNlHooks<'a> {
    /// Hooks for bands `band..band + lanes` (`lanes` is 1 for scalar, 4 for AVX2).
    pub(super) fn new(
        ctx: &'a batch_hooks::StageHookContext,
        band: usize,
        lanes: usize,
        avx2: bool,
    ) -> Self {
        Self {
            ctx,
            events: ctx.for_range(band, band + lanes),
            idx: 0,
            band,
            avx2,
        }
    }
}

#[cfg(test)]
impl NlHooks for CtxNlHooks<'_> {
    fn needs_loop(&self) -> bool {
        !self.events.is_empty() || self.ctx.has_due_log()
    }
    fn before_frame<U: NlUnit>(&mut self, unit: &mut U, t: usize, due: &mut bool) {
        while self.idx < self.events.len() && self.events[self.idx].sample == t {
            let e = self.events[self.idx];
            unit.inject(e.band, e.field, e.raw_u64);
            self.ctx.record_executed_write();
            // Due rule c: a hook wrote NL state.
            *due = true;
            self.idx += 1;
        }
        self.ctx.record_due(3, self.band, t, *due);
    }
    fn on_unit(&self, qualified: bool) {
        #[cfg(target_arch = "x86_64")]
        if self.avx2 {
            test_counters::record_nl_avx2(qualified);
            return;
        }
        test_counters::record_nl_scalar(qualified);
    }
    fn on_fast_frame(&self) {
        #[cfg(target_arch = "x86_64")]
        if self.avx2 {
            test_counters::record_nl_avx2_fast_frame();
            return;
        }
        test_counters::record_nl_scalar_fast_frame();
    }
    fn on_entry_fallback(&self) {
        #[cfg(target_arch = "x86_64")]
        if self.avx2 {
            test_counters::record_nl_avx2_entry_fallback();
            return;
        }
        test_counters::record_nl_scalar_entry_fallback();
    }
}

/// Hooks for one unit: the recorder in unit tests, none in the product.
#[cfg(test)]
pub(super) fn nl_hooks(
    ctx: &batch_hooks::StageHookContext,
    band: usize,
    lanes: usize,
    avx2: bool,
) -> CtxNlHooks<'_> {
    CtxNlHooks::new(ctx, band, lanes, avx2)
}

#[cfg(not(test))]
#[inline(always)]
pub(super) fn nl_hooks(_band: usize, _lanes: usize, _avx2: bool) -> NoNlHooks {
    NoNlHooks
}
