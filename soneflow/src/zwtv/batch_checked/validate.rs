//! Checks of stage outputs in R's reporting order (frame, then band).
#[cfg(test)]
use super::test_counters;
use crate::batch_guard;
use crate::error::{NumericFaultStage as Stage, SoneflowError};

pub(crate) fn validate_core(values: &[f64], n: usize) -> Result<(), SoneflowError> {
    for t in 0..n {
        let row = std::array::from_fn(|band| values[band * n + t]);
        batch_guard::core(&row, t)?;
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn validate_slopes(total: &[f64], specific: &[f64]) -> Result<(), SoneflowError> {
    let total_fault = total.iter().enumerate().find_map(|(t, &value)| {
        batch_guard::bounded(
            value,
            crate::numeric_guard::TW_INPUT_MAX,
            Stage::Slope,
            t,
            None,
        )
        .err()
        .map(|e| (t, usize::MAX, e))
    });
    let specific_fault = specific.iter().enumerate().find_map(|(i, &value)| {
        let t = (i / 240) * crate::zwtv::grid::OUT_DECIM;
        batch_guard::bounded(
            value,
            crate::numeric_guard::CORE_MAX,
            Stage::Slope,
            t,
            Some(i % 240),
        )
        .err()
        .map(|e| (t, i % 240, e))
    });
    [total_fault, specific_fault]
        .into_iter()
        .flatten()
        .min_by_key(|e| (e.0, e.1))
        .map_or(Ok(()), |(_, _, e)| Err(e))
}

/// Validate ONLY the outputs produced by unqualified slopes tasks, in R's exact
/// `(frame, band)` order (`total` has `band = usize::MAX`). Qualified tasks skip this:
/// with round-to-nearest arithmetic and validated core input, `calc_slopes` totals stay in
/// `[0, TW_INPUT_MAX)` and samples in `[0, CORE_MAX]` (see `core::calc_slopes`).
pub(crate) fn validate_slopes_chunks(
    total: &[f64],
    specific: &[f64],
    unqualified_chunks: &mut [usize],
) -> Result<(), SoneflowError> {
    if unqualified_chunks.is_empty() {
        return Ok(());
    }
    #[cfg(test)]
    test_counters::record_slopes_unqual_validated(unqualified_chunks.len());
    unqualified_chunks.sort_unstable();
    let total_fault = unqualified_chunks.iter().find_map(|&out_idx| {
        let t0 = out_idx * crate::zwtv::grid::OUT_DECIM;
        let t_end = (t0 + crate::zwtv::grid::OUT_DECIM).min(total.len());
        (t0..t_end).find_map(|t| {
            batch_guard::bounded(
                total[t],
                crate::numeric_guard::TW_INPUT_MAX,
                Stage::Slope,
                t,
                None,
            )
            .err()
            .map(|e| (t, usize::MAX, e))
        })
    });
    let specific_fault = unqualified_chunks.iter().find_map(|&out_idx| {
        let t0 = out_idx * crate::zwtv::grid::OUT_DECIM;
        let base = out_idx * 240;
        (0..240usize).find_map(|b| {
            batch_guard::bounded(
                specific[base + b],
                crate::numeric_guard::CORE_MAX,
                Stage::Slope,
                t0,
                Some(b),
            )
            .err()
            .map(|e| (t0, b, e))
        })
    });
    [total_fault, specific_fault]
        .into_iter()
        .flatten()
        .min_by_key(|e| (e.0, e.1))
        .map_or(Ok(()), |(_, _, e)| Err(e))
}
