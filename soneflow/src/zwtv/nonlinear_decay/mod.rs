//! ISO 532-1 Annex A nonlinear temporal decay of the 21 core loudness bands
//! (two-capacitor network, upsampled `NL_SUBSTEPS` times per 2 kHz frame).
//!
//! `params` holds the network constants and the initial-state choice,
//! `scalar` and `avx2` the two kernels; this module only picks the backend.

#![cfg_attr(
    any(test, feature = "internals"),
    expect(
        clippy::undocumented_unsafe_blocks,
        reason = "unsafe blocks from 0.1.0 without SAFETY comments; remove once documented"
    )
)]

#[cfg(target_arch = "x86_64")]
mod avx2;
mod params;
mod scalar;

#[cfg(target_arch = "x86_64")]
#[cfg(any(test, feature = "internals"))]
pub use avx2::nl_loudness_avx2;
#[cfg(target_arch = "x86_64")]
pub(crate) use avx2::{NlConsts, NlGroupState};
pub use params::{nl_coeffs, NlInit};
pub(crate) use scalar::NlBandState;
#[cfg(any(test, feature = "internals"))]
pub use scalar::{nl_loudness_scalar, nl_loudness_scalar_with};

#[cfg(any(test, feature = "internals"))]
use crate::zwtv::par::ParMode;

#[cfg(any(test, feature = "internals"))]
pub fn nl_loudness_with(core: &[f64], n_time: usize, mode: ParMode, init: NlInit) -> Vec<f64> {
    #[cfg(target_arch = "x86_64")]
    {
        if crate::simd::use_avx2() {
            return unsafe { nl_loudness_avx2(core, n_time, mode, init) };
        }
    }
    scalar::nl_loudness_scalar_impl(core, n_time, mode, init)
}

#[cfg(any(test, feature = "internals"))]
pub fn nl_loudness_with_mode(core: &[f64], n_time: usize, mode: ParMode) -> Vec<f64> {
    nl_loudness_with(core, n_time, mode, NlInit::Zero)
}

#[cfg(any(test, feature = "internals"))]
pub fn nl_loudness(core: &[f64], n_time: usize) -> Vec<f64> {
    nl_loudness_with_mode(core, n_time, ParMode::Rayon)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core_from_rows(rows: &[Vec<f64>]) -> (Vec<f64>, usize) {
        let n_time = rows[0].len();
        let mut core = Vec::with_capacity(21 * n_time);
        for band in 0..21 {
            let row = &rows[band % rows.len()];
            assert_eq!(row.len(), n_time);
            core.extend_from_slice(row);
        }
        (core, n_time)
    }

    #[test]
    fn single_and_two_frame_inputs_do_not_panic_and_start_from_zero() {
        for rows in [vec![vec![0.7]], vec![vec![0.7, 0.0]], vec![vec![0.0, 0.7]]] {
            let (core, n_time) = core_from_rows(&rows);
            let out = nl_loudness_scalar(&core, n_time);
            assert_eq!(out.len(), core.len());
            // first output equals the first input (zero state never exceeds it)
            assert_eq!(out[0], core[0]);
            assert!(out.iter().all(|v| v.is_finite()));
        }
        assert!(nl_loudness_scalar(&[], 0).is_empty());
    }

    #[test]
    fn silent_tail_makes_both_initialisations_identical() {
        let mut row = vec![1.5; 200];
        row[100..].fill(0.0);
        let (core, n_time) = core_from_rows(&[row]);
        let zero = nl_loudness_scalar(&core, n_time);
        let tail = nl_loudness_scalar_with(&core, n_time, NlInit::MosqitoTail);
        assert_eq!(zero, tail);
    }

    #[test]
    fn tail_seed_only_changes_the_start_of_the_output() {
        // constant input: zero state outputs the input from frame 0; the tail
        // seed (1.5 / 24) is below the input and is overridden immediately on
        // the output but lives on in the slow capacitor.
        let (core, n_time) = core_from_rows(&[vec![1.5; 400]]);
        let zero = nl_loudness_scalar(&core, n_time);
        let tail = nl_loudness_scalar_with(&core, n_time, NlInit::MosqitoTail);
        assert!(zero.iter().all(|&v| v == 1.5));
        assert_eq!(zero[0], 1.5);
        assert_ne!(
            zero.iter().zip(&tail).filter(|(a, b)| a != b).count(),
            zero.len(),
            "difference must not be everywhere"
        );
        // A loud first frame overwrites the seeded fast capacitor at once; only
        // a quiet start exposes it (the batch filter start-up frames are quiet,
        // which is why the seed showed up at the beginning of every signal).
        let mut row = vec![0.0; 400];
        row[20..60].fill(2.0);
        row[399] = 2.4; // non-silent tail -> seed 0.1
        let (core, n_time) = core_from_rows(&[row]);
        let zero = nl_loudness_scalar(&core, n_time);
        let tail = nl_loudness_scalar_with(&core, n_time, NlInit::MosqitoTail);
        assert_eq!(zero[0], 0.0);
        assert!(tail[0] > 0.0 && tail[0] < 0.1, "decaying seed: {}", tail[0]);
        assert_eq!(zero[20], 2.0);
        assert_eq!(tail[20], 2.0);
    }

    #[test]
    fn dispatching_entry_points_use_zero_state() {
        let (core, n_time) = core_from_rows(&[vec![0.0, 0.7, 1.4, 0.2, 0.0, 0.0]]);
        let zero = nl_loudness_with(&core, n_time, ParMode::Sequential, NlInit::Zero);
        assert_eq!(nl_loudness(&core, n_time), zero);
        assert_eq!(
            nl_loudness_with_mode(&core, n_time, ParMode::Sequential),
            zero
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_and_scalar_agree_for_both_initialisations() {
        if !crate::simd::use_avx2() {
            return;
        }
        let rows: Vec<Vec<f64>> = (0..21)
            .map(|b| {
                (0..300)
                    .map(|t| ((t * (b + 3)) % 17) as f64 * 0.1 + if t > 250 { 0.0 } else { 0.5 })
                    .collect()
            })
            .collect();
        let (core, n_time) = core_from_rows(&rows);
        for init in [NlInit::Zero, NlInit::MosqitoTail] {
            let s = nl_loudness_scalar_with(&core, n_time, init);
            let a = unsafe { nl_loudness_avx2(&core, n_time, ParMode::Sequential, init) };
            for (i, (x, y)) in s.iter().zip(&a).enumerate() {
                assert!(
                    (x - y).abs() <= 1e-12 * x.abs().max(1.0),
                    "{init:?} [{i}] {x} vs {y}"
                );
            }
        }
    }
}
