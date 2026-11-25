//! Third-octave bands below 300 Hz (ISO 532-1 Annex A): DLL correction,
//! intensities and the levels of the three lowest critical bands.

use crate::tables::{DLL, RAP};

/// Third-octave bands below 300 Hz (25 Hz .. 250 Hz).
pub(super) const N_LOW_BANDS: usize = 11;

/// DLL row selection: the first
/// RAP range whose threshold `RAP[j] - DLL[j][band]` the level does not exceed,
/// saturating at the last row. The comparison is strict, so a level exactly on
/// a threshold stays in the lower row.
///
/// MoSQITo 1.2.1 vectorises this loop as `arange(1, 7)` and does not reach the
/// last row, which leaves a 0 dB correction above the seventh threshold (for
/// example > 118 dB on the 25 Hz band). This crate scans every row and reaches
/// the last one.
#[inline]
fn low_freq_dll_row(level: f64, band: usize) -> usize {
    let last = DLL.len() - 1;
    let mut row = 0;
    while row < last && level > RAP[row] - DLL[row][band] {
        row += 1;
    }
    row
}

/// Intensities of the 11 third-octave bands below 300 Hz after the DLL
/// correction of the selected row.
pub(super) fn low_band_intensities(spec_third: &[f64]) -> [f64; N_LOW_BANDS] {
    let mut ti = [0.0; N_LOW_BANDS];
    for band in 0..N_LOW_BANDS {
        let row = low_freq_dll_row(spec_third[band], band);
        let xp = spec_third[band] + DLL[row][band];
        ti[band] = 10.0_f64.powf(xp / 10.0);
    }
    ti
}

/// Levels of the three lowest critical bands from the summed intensities of
/// third-octave bands 0-5, 6-8 and 9-10; a band without energy stays at 0.
pub(super) fn lower_critical_band_levels(ti: &[f64; N_LOW_BANDS]) -> [f64; 3] {
    let mut lcb = [0.0; 3];
    let gi = [
        ti[0..6].iter().sum::<f64>(),
        ti[6..9].iter().sum::<f64>(),
        ti[9..11].iter().sum::<f64>(),
    ];
    for (idx, energy) in gi.into_iter().enumerate() {
        if energy > 0.0 {
            lcb[idx] = 10.0 * energy.log10();
        }
    }
    lcb
}

#[cfg(test)]
mod tests {
    use super::{low_band_intensities, low_freq_dll_row, lower_critical_band_levels};
    use crate::tables::{DLL, RAP};

    fn threshold(row: usize, band: usize) -> f64 {
        RAP[row] - DLL[row][band]
    }

    #[test]
    fn low_freq_thresholds_are_strictly_increasing_per_band() {
        // low_freq_dll_row scans rows in order and assumes the thresholds
        // increase; pin the table.
        for band in 0..11 {
            for row in 1..DLL.len() {
                assert!(
                    threshold(row, band) > threshold(row - 1, band),
                    "band {band}: threshold row {row} not above row {}",
                    row - 1
                );
            }
        }
    }

    #[test]
    fn low_freq_dll_row_matches_annex_a_at_every_threshold() {
        let last = DLL.len() - 1;
        for band in 0..11 {
            for row in 0..DLL.len() {
                let thr = threshold(row, band);
                assert_eq!(
                    low_freq_dll_row(thr.next_down(), band),
                    row,
                    "band {band} just below threshold {row}"
                );
                assert_eq!(
                    low_freq_dll_row(thr, band),
                    row,
                    "band {band} exactly on threshold {row} (strict compare)"
                );
                assert_eq!(
                    low_freq_dll_row(thr.next_up(), band),
                    (row + 1).min(last),
                    "band {band} just above threshold {row}"
                );
            }
            assert_eq!(low_freq_dll_row(-1000.0, band), 0, "band {band} floor");
            assert_eq!(low_freq_dll_row(1000.0, band), last, "band {band} ceiling");
            assert_eq!(low_freq_dll_row(f64::NAN, band), 0, "band {band} NaN");
        }
    }

    #[test]
    fn low_freq_last_row_is_reachable_below_120_db() {
        // Every band whose seventh threshold is below the 120 dB validity
        // limit must reach the last row inside the accepted input domain.
        // (MoSQITo 1.2.1 never selects it.)
        let last = DLL.len() - 1;
        for band in 0..11 {
            let thr = threshold(last - 1, band);
            if thr < 120.0 {
                assert_eq!(low_freq_dll_row(120.0, band), last, "band {band} at 120 dB");
            }
        }
        // band 0: 118 dB -> row 6 (-18 dB), 118 + ULP -> row 7 (-15 dB)
        assert_eq!(threshold(6, 0), 118.0);
        assert_eq!(DLL[low_freq_dll_row(118.0, 0)][0], -18.0);
        assert_eq!(DLL[low_freq_dll_row(118.0_f64.next_up(), 0)][0], -15.0);
    }

    #[test]
    fn no_energy_leaves_the_three_lowest_critical_bands_at_zero() {
        assert_eq!(lower_critical_band_levels(&[0.0; 11]), [0.0; 3]);
    }

    #[test]
    fn third_octave_bands_group_six_three_two_into_critical_bands() {
        for k in 0..11 {
            let mut ti = [0.0; 11];
            ti[k] = 10.0;
            let group = if k < 6 {
                0
            } else if k < 9 {
                1
            } else {
                2
            };
            let mut want = [0.0; 3];
            want[group] = 10.0;
            assert_eq!(lower_critical_band_levels(&ti), want, "band {k}");
        }
    }

    #[test]
    fn group_level_is_ten_log10_of_the_summed_intensity() {
        let ti = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0];
        let want = [
            10.0 * 21.0_f64.log10(),
            10.0 * 24.0_f64.log10(),
            10.0 * 21.0_f64.log10(),
        ];
        assert_eq!(lower_critical_band_levels(&ti), want);
    }

    #[test]
    fn intensity_applies_the_annex_a_dll_correction_of_the_selected_row() {
        let mut spec = [0.0; 28];
        for (band, level) in spec.iter_mut().take(11).enumerate() {
            *level = 30.0 + 7.5 * band as f64;
        }
        let ti = low_band_intensities(&spec);
        for band in 0..11 {
            let xp = spec[band] + DLL[low_freq_dll_row(spec[band], band)][band];
            assert_eq!(ti[band], 10.0_f64.powf(xp / 10.0), "band {band}");
        }
    }
}
