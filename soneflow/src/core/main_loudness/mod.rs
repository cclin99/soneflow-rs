use crate::error::SoneflowError;
use crate::numeric_guard::{read_rounding_environment, RoundingEnvironment};
use crate::types::FieldType;
use rayon::prelude::*;

mod c_constants;
mod core_band;
mod low_freq;

use core_band::{band0_correction, core_loudness_of_band, excitation_level, mp1_table_rn};
use low_freq::{low_band_intensities, lower_critical_band_levels, N_LOW_BANDS};

/// Annex A applicability: third-octave bands below 300 Hz must not exceed
/// 120 dB (`main_loudness` rejects, `main_loudness_clamped` clamps).
const LOW_BAND_LIMIT_DB: f64 = 120.0;

pub fn main_loudness(spec_third: &[f64], field: FieldType) -> Result<[f64; 21], SoneflowError> {
    assert_eq!(
        spec_third.len(),
        28,
        "main_loudness expects 28 third-octave band levels"
    );

    if spec_third[..N_LOW_BANDS]
        .iter()
        .any(|&level| level > LOW_BAND_LIMIT_DB)
    {
        return Err(SoneflowError::LevelExceeds120dB);
    }
    Ok(main_loudness_unchecked(spec_third, field))
}

/// Fills the `mp1` table if the caller runs in round-to-nearest, so that a
/// stream created there does not fill it on its first push.
pub(crate) fn warm_mp1_table(environment: RoundingEnvironment) {
    if environment == RoundingEnvironment::KnownRn {
        mp1_table_rn();
    }
}

fn main_loudness_unchecked(spec_third: &[f64], field: FieldType) -> [f64; 21] {
    // The table holds round-to-nearest values; any other mode, or a mode that
    // cannot be read, computes `mp1` at run time as before.
    let mp1_table = match read_rounding_environment() {
        RoundingEnvironment::KnownRn => Some(mp1_table_rn()),
        RoundingEnvironment::NonRn | RoundingEnvironment::Unknown => None,
    };
    let ti = low_band_intensities(spec_third);
    let lcb = lower_critical_band_levels(&ti);

    let mut nm = [0.0; 21];
    for band in 0..20 {
        // Critical bands 0-2 come from the low-frequency grouping, the others
        // from third-octave bands 11-27.
        let level = if band < 3 {
            lcb[band]
        } else {
            spec_third[band + 8]
        };
        nm[band] = core_loudness_of_band(excitation_level(level, band, field), band, mp1_table);
    }
    nm[0] = band0_correction(nm[0]);
    nm
}

pub(crate) fn main_loudness_clamped(spec_third: &[f64; 28], field: FieldType) -> ([f64; 21], bool) {
    let mut frame = *spec_third;
    let mut clamped = false;
    for level in &mut frame[..N_LOW_BANDS] {
        if *level > LOW_BAND_LIMIT_DB {
            *level = LOW_BAND_LIMIT_DB;
            clamped = true;
        }
    }
    (main_loudness_unchecked(&frame, field), clamped)
}
pub fn main_loudness_frames_into(
    frames: &[f64],
    n_time: usize,
    field: FieldType,
    core: &mut [f64],
) -> Result<(), SoneflowError> {
    assert_eq!(
        frames.len(),
        28 * n_time,
        "main_loudness_frames_into expects frame-major (n_time, 28) levels"
    );
    assert_eq!(
        core.len(),
        21 * n_time,
        "main_loudness_frames_into expects band-major (21, n_time) output"
    );

    let per_frame = frames
        .par_chunks_exact(28)
        .map(|frame| main_loudness(frame, field))
        .collect::<Result<Vec<_>, _>>()?;

    for (t, nm) in per_frame.iter().enumerate() {
        for band in 0..21 {
            core[band * n_time + t] = nm[band];
        }
    }

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::{main_loudness, main_loudness_clamped, main_loudness_frames_into};
    use crate::types::FieldType;

    #[test]
    fn clamped_variant_shares_the_row_selection_below_the_clamp() {
        let mut frame = [-60.0; 28];
        frame[0] = 119.0;
        let (clamped_nm, clamped) = main_loudness_clamped(&frame, FieldType::Free);
        assert!(!clamped);
        assert_eq!(clamped_nm, main_loudness(&frame, FieldType::Free).unwrap());
    }

    #[test]
    fn frame_major_batch_matches_per_frame_main_loudness() {
        let n_time = 3;
        let mut frames = vec![0.0; 28 * n_time];
        for t in 0..n_time {
            for band in 0..28 {
                frames[t * 28 + band] = 35.0 + band as f64 * 0.75 + t as f64 * 1.25;
            }
        }
        let mut core = vec![f64::NAN; 21 * n_time];

        main_loudness_frames_into(&frames, n_time, FieldType::Free, &mut core).unwrap();

        for t in 0..n_time {
            let expected = main_loudness(&frames[t * 28..(t + 1) * 28], FieldType::Free).unwrap();
            for band in 0..21 {
                assert_eq!(
                    core[band * n_time + t],
                    expected[band],
                    "band {band} frame {t}"
                );
            }
        }
    }

    #[test]
    fn clamped_variant_flags_and_matches_at_120() {
        let mut frame = [60.0; 28];
        frame[3] = 130.0;
        let (nm, clamped) = main_loudness_clamped(&frame, FieldType::Free);
        assert!(clamped);
        let mut at_limit = frame;
        at_limit[3] = 120.0;
        assert_eq!(nm, main_loudness(&at_limit, FieldType::Free).unwrap());
        let (_, ok) = main_loudness_clamped(&[60.0; 28], FieldType::Free);
        assert!(!ok);
    }
}
