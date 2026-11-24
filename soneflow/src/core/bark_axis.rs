/// Bark axis of the specific-loudness output: 240 points from 0.1 to 24 Bark.
/// Shared by the stationary and time-varying results.
pub(crate) fn bark_axis() -> Vec<f64> {
    (0..240)
        .map(|i| 0.1 + (24.0 - 0.1) * i as f64 / 239.0)
        .collect()
}
