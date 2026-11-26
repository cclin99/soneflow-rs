//! Nonlinear temporal decay stage: a scalar band as an `NlUnit`, and the
//! dispatch of bands (scalar) or groups of four bands (AVX2).
#[cfg(target_arch = "x86_64")]
use super::avx2;
#[cfg(test)]
use super::batch_hooks;
use super::nl_unit::{nl_hooks, run_unit, NlHooks, NlUnit};
use super::shared::{dispatch, mxcsr_qualified, Fault, StageCtx};
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
    // D0 entry failure: no speculation occurred, no MXCSR restore, delegate frame `t` to R
    // without modifying/zeroing state or seed so R determines exact coordinate, reason & state.
    state
        .advance_frame_checked(row_t, next, b)
        .map_err(|r| (t, band, r))
}

/// Core value of frame `t` and of the next frame (zero after the last).
#[inline(always)]
fn frame_inputs(row: &[f64], t: usize) -> (f64, f64) {
    let next = if t + 1 < row.len() { row[t + 1] } else { 0.0 };
    (row[t], next)
}

/// Unchecked steps of frames `start..row.len()`; the slices and coefficients
/// are arguments so the loop keeps them in registers.
#[inline(always)]
fn advance_band<F: Fn()>(
    state: &mut NlBandState,
    row: &[f64],
    out: &mut [f64],
    b: &[f64; 6],
    start: usize,
    each: F,
) {
    for t in start..row.len() {
        each();
        let next = if t + 1 < row.len() { row[t + 1] } else { 0.0 };
        out[t] = state.advance_frame(row[t], next, b);
    }
}

/// One scalar band: its core row, its output row and its state.
struct ScalarBand<'a> {
    state: NlBandState,
    row: &'a [f64],
    out: &'a mut [f64],
    b: &'a [f64; 6],
    band: usize,
}

impl NlUnit for ScalarBand<'_> {
    #[inline(always)]
    fn frames(&self) -> usize {
        self.row.len()
    }
    #[inline(always)]
    fn is_entry_valid(&self) -> bool {
        self.state.is_entry_valid()
    }
    #[inline(always)]
    fn inputs_ok(&self, t: usize) -> bool {
        let (row_t, next) = frame_inputs(self.row, t);
        let max = crate::numeric_guard::CORE_MAX;
        crate::numeric_guard::is_nonnegative_bounded(row_t, max)
            && crate::numeric_guard::is_nonnegative_bounded(next, max)
    }
    #[inline(always)]
    fn advance(&mut self, t: usize) {
        let (row_t, next) = frame_inputs(self.row, t);
        self.out[t] = self.state.advance_frame(row_t, next, self.b);
    }
    #[inline(always)]
    fn advance_from<F: Fn()>(&mut self, start: usize, each: F) {
        advance_band(&mut self.state, self.row, self.out, self.b, start, each);
    }
    #[inline(always)]
    fn advance_checked(&mut self, t: usize) -> Result<(), Fault> {
        let (row_t, next) = frame_inputs(self.row, t);
        self.out[t] = self
            .state
            .advance_frame_checked(row_t, next, self.b)
            .map_err(|r| (t, self.band, r))?;
        Ok(())
    }
    #[inline(always)]
    fn entry_fallback(&mut self, t: usize) -> Result<(), Fault> {
        let (row_t, next) = frame_inputs(self.row, t);
        self.out[t] = nl_band_entry_fallback(&mut self.state, row_t, next, self.b, t, self.band)?;
        Ok(())
    }
    #[cfg(test)]
    fn inject(&mut self, _band: usize, field: usize, raw: u64) {
        self.state.inject_raw_bits(field, raw);
    }
}

/// One scalar band of the stage (terms: see `crate::numeric_guard`).
pub(super) fn nl_band<H: NlHooks>(
    row: &[f64],
    out: &mut [f64],
    b: &[f64; 6],
    init: NlInit,
    band: usize,
    ctx: StageCtx,
    hooks: H,
) -> Result<(), Fault> {
    // Read the worker MXCSR once before seed and frame computation.
    let qualified = ctx.qualified && mxcsr_qualified();
    hooks.on_unit(qualified);
    let mut unit = ScalarBand {
        state: NlBandState::seed(init, row[row.len() - 1]),
        row,
        out,
        b,
        band,
    };
    // Due rule d: NL state seeded from input (`MosqitoTail`) is due before first use.
    let due = matches!(init, NlInit::MosqitoTail);
    run_unit(&mut unit, qualified, ctx.validated, due, hooks)
}

pub(super) fn nonlinear_with_backend(
    core: &[f64],
    n: usize,
    mode: ParMode,
    init: NlInit,
    core_validated: bool,
    use_avx2: bool,
) -> Result<Vec<f64>, SoneflowError> {
    #[cfg(test)]
    let hook_ctx = batch_hooks::snapshot_stage(3, n);
    let mut out = vec![0.0; core.len()];
    if n == 0 {
        return Ok(out);
    }
    // The caller reads unit qualification before constructing `nl_coeffs()`.
    let ctx = StageCtx {
        qualified: mxcsr_qualified(),
        validated: core_validated,
        avx2: use_avx2,
    };
    let b = nl_coeffs();
    #[cfg(target_arch = "x86_64")]
    if ctx.avx2 {
        dispatch(&mut out, 4 * n, mode, |g, part| {
            let band = 4 * g;
            let band_core = &core[band * n..(band + 1) * n];
            if part.len() == n {
                let hooks = nl_hooks(
                    #[cfg(test)]
                    &hook_ctx,
                    band,
                    1,
                    false,
                );
                return nl_band(band_core, part, &b, init, band, ctx, hooks);
            }
            let hooks = nl_hooks(
                #[cfg(test)]
                &hook_ctx,
                band,
                4,
                true,
            );
            // SAFETY: runtime dispatch checks AVX2/FMA; remaining groups have four bands.
            unsafe { avx2::nonlinear(core, part, n, band, b, init, ctx, hooks) }
        })
        .map_err(|(i, b, r)| batch_guard::fault(Stage::Nonlinear, r, i, Some(b)))?;
        return Ok(out);
    }
    dispatch(&mut out, n, mode, |band, part| {
        let hooks = nl_hooks(
            #[cfg(test)]
            &hook_ctx,
            band,
            1,
            false,
        );
        nl_band(
            &core[band * n..(band + 1) * n],
            part,
            &b,
            init,
            band,
            ctx,
            hooks,
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
