//! Sample-rate and frame-grid constants of the time-varying pipeline, and the
//! frozen output-grid contract. A leaf module: every stage depends on
//! it and it depends on nothing.

/// Input sample rate (Hz). The only rate the ZWTV pipeline accepts.
pub(crate) const FS: f64 = 48_000.0;

/// Decimation from the 48 kHz input to the 2 kHz internal frame rate.
pub const DEC_FACTOR: usize = 24;

/// Internal frame rate (Hz), `FS / DEC_FACTOR`.
pub(crate) const INTERNAL_RATE: f64 = 2000.0;

/// 輸出格線相對 third-octave 框架的再抽取因子(2 ms 格線)。
pub(crate) const OUT_DECIM: usize = 4;

/// `loudness_zwtv` 對 `signal_len` 樣本輸入的輸出框架數。
/// 純函數、不驗證輸入；binding 必須轉發本函式，勿手抄公式。
///
/// Output-grid contract (part of C ABI v1): for `L` samples at 48 kHz the
/// result has `ceil(ceil(L/24)/4)` points; point `j` is internal 2 kHz frame `4j`,
/// its time is exactly `96j / 48000` s (grid starts at 0, 2 ms step), and its value
/// depends only on samples `<= 96j`. The batch and the stream produce the same count.
/// A caller that needs a shorter grid with the same step takes a prefix
/// (`n[..len]`); the values of the prefix do not change.
///
/// # Examples
///
/// ```
/// use soneflow::zwtv_out_frames;
///
/// assert_eq!(zwtv_out_frames(4_800), 50); // the shortest legal input
/// assert_eq!(zwtv_out_frames(4_801), 51); // a started frame adds a point
/// assert_eq!(zwtv_out_frames(48_000), 500); // 1 s
/// ```
pub fn zwtv_out_frames(signal_len: usize) -> usize {
    signal_len.div_ceil(DEC_FACTOR).div_ceil(OUT_DECIM)
}

/// Time (s) of each of the `n_time` internal 2 kHz frames.
pub fn time_axis(_sig_len: usize, n_time: usize) -> Vec<f64> {
    (0..n_time).map(|i| (i * DEC_FACTOR) as f64 / FS).collect()
}

/// Every `OUT_DECIM`-th internal frame of the weighted loudness and its time.
pub(crate) fn decimate_to_output_grid(filt: &[f64], time_axis: &[f64]) -> (Vec<f64>, Vec<f64>) {
    filt.iter()
        .zip(time_axis)
        .step_by(OUT_DECIM)
        .map(|(&n, &t)| (n, t))
        .unzip()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_rate_is_input_rate_over_decimation() {
        assert_eq!(FS / DEC_FACTOR as f64, INTERNAL_RATE);
    }
}
