//! Parameters of the ISO 532-1 Annex A nonlinear temporal decay.

use crate::zwtv::grid::INTERNAL_RATE;

/// Initial state of the nonlinear temporal decay (ISO 532-1 Annex A, the two
/// capacitor voltages of each critical band) before the
/// first frame of a batch run.
///
/// The stream path always starts from zero state; this choice only affects
/// `loudness_zwtv` / `ZwtvProcessor`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NlInit {
    /// Zero state (ISO 532-1 Annex A). Default. The output for a
    /// signal prefix does not depend on the samples that follow it, and the
    /// batch result equals the causal stream from the first frame.
    #[default]
    Zero,
    /// Seed every band with `core[n_time - 1] / 24`, which is what MoSQITo
    /// 1.2.1 computes because its vectorised first column reads column `-1`.
    /// Kept so that the MoSQITo golden fixtures and
    /// the direct MoSQITo parity suite stay bitwise comparable. Not intended
    /// for measurement use: the first ~0.1-0.5 s of the result depend on the
    /// end of the signal.
    MosqitoTail,
}

/// Sub-steps per 2 kHz frame (the network runs at 48 kHz).
pub(crate) const NL_SUBSTEPS: usize = 24;

/// Tolerance of the test "the input equals the previous output" in the
/// update of the second capacitor (Annex A).
pub(crate) const NL_EPS: f64 = 1e-5;

/// Constants B[0..6] of the two-capacitor analog network.
pub fn nl_coeffs() -> [f64; 6] {
    let t_short: f64 = 0.005;
    let t_long: f64 = 0.015;
    let t_var: f64 = 0.075;
    let delta_t = 1.0 / (INTERNAL_RATE * NL_SUBSTEPS as f64);
    let p = (t_var + t_long) / (t_var * t_short);
    let q = 1.0 / (t_short * t_var);
    let lambda_1 = -p / 2.0 + (p * p / 4.0 - q).sqrt();
    let lambda_2 = -p / 2.0 - (p * p / 4.0 - q).sqrt();
    let den = t_var * (lambda_1 - lambda_2);
    let e1 = (lambda_1 * delta_t).exp();
    let e2 = (lambda_2 * delta_t).exp();
    [
        (e1 - e2) / den,
        ((t_var * lambda_2 + 1.0) * e1 - (t_var * lambda_1 + 1.0) * e2) / den,
        ((t_var * lambda_1 + 1.0) * e1 - (t_var * lambda_2 + 1.0) * e2) / den,
        (t_var * lambda_1 + 1.0) * (t_var * lambda_2 + 1.0) * (e1 - e2) / den,
        (-delta_t / t_long).exp(),
        (-delta_t / t_var).exp(),
    ]
}
