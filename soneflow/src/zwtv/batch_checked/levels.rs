//! Third-octave levels stage: a scalar band as a `TolUnit`, and the dispatch
//! of bands (scalar) or groups of four bands (AVX2).
#[cfg(target_arch = "x86_64")]
use super::avx2;
#[cfg(test)]
use super::batch_hooks;
use super::shared::{dispatch, qualified_mxcsr, Fault};
use super::tol_unit::{run_levels_unit, tol_hooks, Cursor, TolHooks, TolUnit};
use crate::batch_guard;
use crate::error::{NumericFaultStage as Stage, SoneflowError};
use crate::numeric_guard::DomainFault;
use crate::zwtv::grid::DEC_FACTOR;
use crate::zwtv::par::ParMode;
use crate::zwtv::third_octave_levels::{intensity_to_db, TolBandState};

/// Unchecked steps over `block` from `at`, writing one level per emission
/// into `row`: the samples before the next phase 0 first, then one
/// `DEC_FACTOR` chunk per output slot. Returns the cursor after the block and
/// whether every level is finite.
#[inline(always)]
fn scan_band(
    state: &mut TolBandState,
    block: &[f64],
    row: &mut [f64],
    at: Cursor,
) -> (Cursor, bool) {
    let lead = ((DEC_FACTOR - at.phase) % DEC_FACTOR).min(block.len());
    let (head, rest) = block.split_at(lead);
    for &sample in head {
        let _ = state.advance(sample);
    }
    let mut nonfinite_acc = 0u64;
    for (out_slot, chunk) in row[at.emission..].iter_mut().zip(rest.chunks(DEC_FACTOR)) {
        if let Some((&first, tail)) = chunk.split_first() {
            let level = intensity_to_db(state.advance(first));
            nonfinite_acc |= crate::numeric_guard::nonfinite_mask_bit(level);
            *out_slot = level;
            for &sample in tail {
                let _ = state.advance(sample);
            }
        }
    }
    let after = at.advanced(block.len(), rest.len().div_ceil(DEC_FACTOR));
    (after, (nonfinite_acc as i64) >= 0)
}

/// Checked steps (R) over `signal[pos..end]` from `at`; the first fault is
/// returned with its sample index.
#[inline(never)]
fn scan_band_checked(
    state: &mut TolBandState,
    signal: &[f64],
    (pos, end): (usize, usize),
    at: Cursor,
    row: &mut [f64],
    band: usize,
) -> Result<(), Fault> {
    let (mut phase, mut emission) = (at.phase, at.emission);
    for (offset, &sample) in signal[pos..end].iter().enumerate() {
        let i = pos + offset;
        let value = state.advance_checked(sample).map_err(|r| (i, band, r))?;
        if phase == 0 {
            let level = intensity_to_db(value);
            if !level.is_finite() {
                return Err((i, band, DomainFault::NonFinite));
            }
            row[emission] = level;
            emission += 1;
        }
        phase += 1;
        if phase == DEC_FACTOR {
            phase = 0;
        }
    }
    Ok(())
}

/// One scalar band: the signal, its output row and its filter state.
struct ScalarBand<'a> {
    state: TolBandState,
    signal: &'a [f64],
    row: &'a mut [f64],
    band: usize,
}

impl TolUnit for ScalarBand<'_> {
    type Snapshot = TolBandState;
    #[inline(always)]
    fn len(&self) -> usize {
        self.signal.len()
    }
    #[inline(always)]
    fn scan(&mut self, pos: usize, end: usize, at: Cursor) -> (Cursor, bool) {
        scan_band(&mut self.state, &self.signal[pos..end], self.row, at)
    }
    #[inline(always)]
    fn scan_checked(&mut self, pos: usize, end: usize, at: Cursor) -> Result<(), Fault> {
        scan_band_checked(
            &mut self.state,
            self.signal,
            (pos, end),
            at,
            self.row,
            self.band,
        )
    }
    #[inline(always)]
    fn state_valid(&self) -> bool {
        self.state.validate_numeric().is_ok()
    }
    fn snapshot(&self) -> TolBandState {
        self.state.copy_exact()
    }
    fn restore(&mut self, snapshot: TolBandState) {
        self.state = snapshot;
    }
    fn reset(&mut self) {
        self.state = TolBandState::new(self.band);
    }
    #[cfg(test)]
    fn inject(&mut self, _band: usize, field: usize, raw: u64) {
        self.state.inject_raw_bits(field, raw);
    }
}

/// One scalar band of the stage (terms: see `crate::numeric_guard`).
pub(super) fn levels_band<H: TolHooks>(
    signal: &[f64],
    band: usize,
    row: &mut [f64],
    hooks: H,
) -> Result<(), Fault> {
    // Read the worker MXCSR before constructing the band state.
    let entry_mxcsr = qualified_mxcsr();
    let mut unit = ScalarBand {
        state: TolBandState::new(band),
        signal,
        row,
        band,
    };
    run_levels_unit(&mut unit, entry_mxcsr, hooks)
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
        dispatch(&mut out, 4 * n, mode, |g, part| {
            let hooks = tol_hooks(
                #[cfg(test)]
                &hook_ctx,
                4 * g,
                4,
                true,
            );
            // SAFETY: caller ensures AVX2 and FMA are available.
            unsafe { avx2::levels(signal, g, part, n, hooks) }
        })
        .map_err(|(i, b, r)| batch_guard::fault(Stage::Filter, r, i, Some(b)))?;
        return Ok((out, n));
    }
    dispatch(&mut out, n, mode, |band, row| {
        let hooks = tol_hooks(
            #[cfg(test)]
            &hook_ctx,
            band,
            1,
            false,
        );
        levels_band(signal, band, row, hooks)
    })
    .map_err(|(i, b, r)| batch_guard::fault(Stage::Filter, r, i, Some(b)))?;
    Ok((out, n))
}

pub(crate) fn levels(signal: &[f64], mode: ParMode) -> Result<(Vec<f64>, usize), SoneflowError> {
    levels_with_backend(signal, mode, crate::simd::use_avx2())
}
