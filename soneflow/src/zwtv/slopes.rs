//! Batch slope stage: total and specific loudness of the decayed core bands
//! (`crate::core::calc_slopes`) for every 2 kHz frame, frame-parallel on Rayon.

use super::grid::OUT_DECIM;
use crate::core::calc_slopes::{calc_slopes_into, calc_slopes_n_only};
use rayon::prelude::*;

const N_CORE: usize = 21;
/// Specific-loudness points per frame (0.1 Bark steps).
const N_BARK: usize = 240;

/// From band-major `(21, n_time)` decayed core loudness `nl`, write the total
/// loudness of every frame to `loudness` (`n_time`) and the specific loudness
/// of every output frame (each `OUT_DECIM`-th) to `spec_time_major` as
/// `(n_out, 240)`. Both buffers are resized in place, so their capacity is
/// reused across calls.
///
/// Each Rayon frame task reads its own MXCSR once before its first
/// `calc_slopes_into` walk; qualified tasks omit output domain validation (D0),
/// while unqualified tasks record their `out_idx` so the caller validates them
/// in R's exact `(frame, band)` order (terms: see `crate::numeric_guard`).
pub(crate) fn slopes_into_d0(
    nl: &[f64],
    n_time: usize,
    loudness: &mut Vec<f64>,
    spec_time_major: &mut Vec<f64>,
) -> Vec<usize> {
    let n_out = n_time.div_ceil(OUT_DECIM);
    let frame_at = |t: usize| -> [f64; N_CORE] { std::array::from_fn(|b| nl[b * n_time + t]) };
    loudness.resize(n_time, 0.0);
    spec_time_major.resize(N_BARK * n_out, 0.0);
    loudness
        .par_chunks_mut(OUT_DECIM)
        .zip_eq(spec_time_major.par_chunks_mut(N_BARK))
        .enumerate()
        .fold(Vec::new, |mut unqual, (out_idx, (loudness_chunk, spec))| {
            let t0 = out_idx * OUT_DECIM;
            let qualified = crate::numeric_guard::read_unit_mxcsr()
                .is_some_and(crate::numeric_guard::is_mxcsr_qualified);
            #[cfg(test)]
            super::batch_checked::test_counters::record_slopes(qualified);
            loudness_chunk[0] = calc_slopes_into(&frame_at(t0), spec);
            for (offset, loudness) in loudness_chunk.iter_mut().enumerate().skip(1) {
                *loudness = calc_slopes_n_only(&frame_at(t0 + offset));
            }
            if !qualified {
                unqual.push(out_idx);
            }
            unqual
        })
        .reduce(Vec::new, |mut a, mut b| {
            a.append(&mut b);
            a
        })
}

/// `spec_time_major` `(n_out, 240)` transposed to the public bark-major
/// `(240, n_out)` layout of `LoudnessTimeVarying::n_specific`.
pub(crate) fn specific_bark_major(spec_time_major: &[f64], n_out: usize) -> Vec<f64> {
    let mut n_specific = Vec::with_capacity(N_BARK * n_out);
    for bark in 0..N_BARK {
        for out_idx in 0..n_out {
            n_specific.push(spec_time_major[out_idx * N_BARK + bark]);
        }
    }
    n_specific
}
