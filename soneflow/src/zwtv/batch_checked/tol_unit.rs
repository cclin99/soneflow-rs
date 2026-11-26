//! The control flow of the third-octave levels stage, shared by a scalar band
//! (`levels`) and an AVX2 group of four bands (`avx2`), and its hooks.
//!
//! Hook samples (unit tests only) split the signal into blocks; the product
//! passes [`NoTolHooks`] and so runs one block, the whole signal from phase 0.
use super::shared::Fault;
#[cfg(test)]
use super::{batch_hooks, test_counters};
use crate::zwtv::grid::DEC_FACTOR;

/// Position within the decimation: the phase of the next sample and the
/// output slot of the next emission (phase 0).
#[derive(Clone, Copy)]
pub(super) struct Cursor {
    pub(super) phase: usize,
    pub(super) emission: usize,
}

impl Cursor {
    /// The cursor at sample `pos` of the signal.
    pub(super) fn at(pos: usize) -> Self {
        Self {
            phase: pos % DEC_FACTOR,
            emission: pos.div_ceil(DEC_FACTOR),
        }
    }

    /// The cursor after `samples` more samples, of which `emitted` were at phase 0.
    #[inline(always)]
    pub(super) fn advanced(self, samples: usize, emitted: usize) -> Self {
        Self {
            phase: (self.phase + samples) % DEC_FACTOR,
            emission: self.emission + emitted,
        }
    }
}

/// One unit of the levels stage: a scalar band, or an AVX2 group of four bands.
pub(super) trait TolUnit {
    type Snapshot;
    /// Samples in the signal.
    fn len(&self) -> usize;
    /// Unchecked steps over samples `pos..end` from `at`. Returns the cursor
    /// after `end` and whether every emitted level is finite.
    fn scan(&mut self, pos: usize, end: usize, at: Cursor) -> (Cursor, bool);
    /// Checked steps (R) over samples `pos..end` from `at`.
    fn scan_checked(&mut self, pos: usize, end: usize, at: Cursor) -> Result<(), Fault>;
    /// The state passes `validate_numeric`.
    fn state_valid(&self) -> bool;
    /// Bit-exact copy of the state (coefficients and numeric values).
    fn snapshot(&self) -> Self::Snapshot;
    fn restore(&mut self, snapshot: Self::Snapshot);
    /// The state as constructed.
    fn reset(&mut self);
    /// Write raw state bits of `band` (test hooks).
    #[cfg(test)]
    fn inject(&mut self, band: usize, field: usize, raw: u64);
}

/// Observation points of one levels unit.
pub(super) trait TolHooks {
    /// Sample index of the next scheduled hook.
    fn next_sample(&self) -> Option<usize>;
    /// Apply the hooks scheduled at `pos`.
    fn apply_at<U: TolUnit>(&mut self, unit: &mut U, pos: usize);
    fn record_due(&self, pos: usize, due: bool);
    /// A replay over `[pos..end)` must not straddle a scheduled hook.
    fn check_none_inside(&self, pos: usize, end: usize, what: &str);
    fn on_unit(&self, qualified: bool);
    fn on_fast_block(&self);
    fn on_cold_replay(&self);
}

/// Run one unit (terms: see `crate::numeric_guard`). `entry_mxcsr` is the
/// worker MXCSR when it qualifies; otherwise every sample goes through R.
#[inline(always)]
pub(super) fn run_levels_unit<U: TolUnit, H: TolHooks>(
    unit: &mut U,
    entry_mxcsr: Option<u32>,
    mut hooks: H,
) -> Result<(), Fault> {
    hooks.on_unit(entry_mxcsr.is_some());
    match entry_mxcsr {
        #[cfg(target_arch = "x86_64")]
        Some(mxcsr) => run_fast(unit, mxcsr, &mut hooks),
        _ => run_checked(unit, &mut hooks),
    }
}

/// Every sample through R, in blocks split at hook samples.
fn run_checked<U: TolUnit, H: TolHooks>(unit: &mut U, hooks: &mut H) -> Result<(), Fault> {
    let mut pos = 0;
    while pos < unit.len() {
        if hooks.next_sample() == Some(pos) {
            hooks.apply_at(unit, pos);
        }
        let end = hooks.next_sample().unwrap_or(unit.len());
        unit.scan_checked(pos, end, Cursor::at(pos))?;
        pos = end;
    }
    Ok(())
}

/// Unchecked blocks; a block whose levels or end state fail is replayed
/// through R from its start (cold replay), and a hook-written state is
/// checked before use (due rule c).
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn run_fast<U: TolUnit, H: TolHooks>(
    unit: &mut U,
    entry_mxcsr: u32,
    hooks: &mut H,
) -> Result<(), Fault> {
    let mut pos = 0;
    let mut at = Cursor {
        phase: 0,
        emission: 0,
    };
    let mut due = false;
    let mut block_mxcsr = entry_mxcsr;
    while pos < unit.len() {
        let mut snapshot = None;
        if hooks.next_sample() == Some(pos) {
            hooks.apply_at(unit, pos);
            // Due rule c: a hook wrote TOL state.
            due = true;
            hooks.record_due(pos, due);
            snapshot = Some(unit.snapshot());
            // Capture the block entry MXCSR once per hook-delimited block.
            block_mxcsr = crate::numeric_guard::read_unit_mxcsr().unwrap_or(entry_mxcsr);
        }
        let end = hooks.next_sample().unwrap_or(unit.len());
        debug_assert!(end > pos && end <= unit.len());
        if due && !unit.state_valid() {
            hooks.check_none_inside(pos, end, "entry-due replay");
            unit.scan_checked(pos, end, at)?;
            at = Cursor::at(end);
        } else {
            hooks.on_fast_block();
            let (after, finite) = unit.scan(pos, end, at);
            if finite && unit.state_valid() {
                due = false;
                at = after;
            } else {
                hooks.check_none_inside(pos, end, "cold replay");
                cold_replay(unit, pos, end, at, block_mxcsr, snapshot, hooks)?;
                due = true;
                at = Cursor::at(end);
            }
        }
        pos = end;
    }
    Ok(())
}

/// Replay `[pos..end)` through R from the block's start state: the hook
/// snapshot, or the constructed state for the first block.
#[cfg(target_arch = "x86_64")]
#[cold]
#[inline(never)]
fn cold_replay<U: TolUnit, H: TolHooks>(
    unit: &mut U,
    pos: usize,
    end: usize,
    at: Cursor,
    block_mxcsr: u32,
    snapshot: Option<U::Snapshot>,
    hooks: &H,
) -> Result<(), Fault> {
    hooks.on_cold_replay();
    crate::numeric_guard::restore_mxcsr(block_mxcsr);
    match snapshot {
        Some(state) => unit.restore(state),
        None => {
            assert_eq!(pos, 0, "an unhooked batch TOL block must start at sample 0");
            unit.reset();
        }
    }
    unit.scan_checked(pos, end, at)
}

/// Product hooks: nothing scheduled, nothing recorded. Passed by value: a
/// `&mut` to it would still be a pointer argument.
pub(super) struct NoTolHooks;

impl TolHooks for NoTolHooks {
    #[inline(always)]
    fn next_sample(&self) -> Option<usize> {
        None
    }
    #[inline(always)]
    fn apply_at<U: TolUnit>(&mut self, _: &mut U, _: usize) {}
    #[inline(always)]
    fn record_due(&self, _: usize, _: bool) {}
    #[inline(always)]
    fn check_none_inside(&self, _: usize, _: usize, _: &str) {}
    #[inline(always)]
    fn on_unit(&self, _: bool) {}
    #[inline(always)]
    fn on_fast_block(&self) {}
    #[inline(always)]
    fn on_cold_replay(&self) {}
}

/// Test hooks of one unit: the scheduled writes for its bands, the stage's
/// due log and the path counters of its backend.
#[cfg(test)]
pub(super) struct CtxTolHooks<'a> {
    ctx: &'a batch_hooks::StageHookContext,
    events: Vec<batch_hooks::BatchHookEvent>,
    idx: usize,
    band: usize,
    avx2: bool,
}

#[cfg(test)]
impl<'a> CtxTolHooks<'a> {
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
impl TolHooks for CtxTolHooks<'_> {
    fn next_sample(&self) -> Option<usize> {
        self.events.get(self.idx).map(|e| e.sample)
    }
    fn apply_at<U: TolUnit>(&mut self, unit: &mut U, pos: usize) {
        while self.idx < self.events.len() && self.events[self.idx].sample == pos {
            let e = self.events[self.idx];
            unit.inject(e.band, e.field, e.raw_u64);
            self.ctx.record_executed_write();
            self.idx += 1;
        }
    }
    fn record_due(&self, pos: usize, due: bool) {
        self.ctx.record_due(1, self.band, pos, due);
    }
    fn check_none_inside(&self, pos: usize, end: usize, what: &str) {
        let backend = if self.avx2 { "AVX2" } else { "scalar" };
        assert!(
            self.next_sample().is_none_or(|s| s >= end),
            "STOP CONDITION: batch {backend} TOL hook fell inside {what} interval [{pos}..{end})"
        );
    }
    fn on_unit(&self, qualified: bool) {
        if self.avx2 {
            test_counters::record_avx2(qualified);
        } else {
            test_counters::record_scalar(qualified);
        }
    }
    fn on_fast_block(&self) {
        if self.avx2 {
            test_counters::record_avx2_fast_block();
        } else {
            test_counters::record_scalar_fast_block();
        }
    }
    fn on_cold_replay(&self) {
        if self.avx2 {
            test_counters::record_avx2_cold_replay();
        } else {
            test_counters::record_scalar_cold_replay();
        }
    }
}

/// Hooks for one unit: the recorder in unit tests, none in the product.
#[cfg(test)]
pub(super) fn tol_hooks(
    ctx: &batch_hooks::StageHookContext,
    band: usize,
    lanes: usize,
    avx2: bool,
) -> CtxTolHooks<'_> {
    CtxTolHooks::new(ctx, band, lanes, avx2)
}

#[cfg(not(test))]
#[inline(always)]
pub(super) fn tol_hooks(_band: usize, _lanes: usize, _avx2: bool) -> NoTolHooks {
    NoTolHooks
}
