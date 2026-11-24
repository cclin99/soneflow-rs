//! Core loudness of one critical band (ISO 532-1 Annex A): excitation
//! level, threshold and DCB terms, and the band 0 correction.

use super::c_constants::{A0_C, DCB_C, DDF_C, KORRY_OFFSET, KORRY_SLOPE, MP1_FACTOR};
use crate::tables::LTQ;
use crate::types::FieldType;

/// Excitation level of critical band `band`: the level minus the transmission
/// term `A0`, plus the diffuse-field correction `DDF` in a diffuse field.
#[inline]
pub(super) fn excitation_level(level: f64, band: usize, field: FieldType) -> f64 {
    let mut le = level - A0_C[band];
    if field == FieldType::Diffuse {
        le += DDF_C[band];
    }
    le
}

/// Core loudness of critical band `band` from its excitation level: zero up
/// to the threshold in quiet `LTQ` (strict compare), then Zwicker's power law
/// after the `DCB` correction, clamped at zero. Operation order follows ISO 532-1 Annex A.
#[inline]
pub(super) fn core_loudness_of_band(le: f64, band: usize) -> f64 {
    if le > LTQ[band] {
        let le = le - DCB_C[band];
        let mp1 = MP1_FACTOR * 10.0_f64.powf(0.025 * LTQ[band]);
        let mp2 = (1.0 - 0.25 + 0.25 * 10.0_f64.powf(0.1 * (le - LTQ[band]))).powf(0.25) - 1.0;
        (mp1 * mp2).max(0.0)
    } else {
        0.0
    }
}

/// Band 0 correction `0.4 + 0.32 * N^0.2` (constants in single precision),
/// applied only while it is at most 1.
#[inline]
pub(super) fn band0_correction(nm0: f64) -> f64 {
    let korry = KORRY_OFFSET + KORRY_SLOPE * nm0.powf(0.2);
    if korry <= 1.0 {
        nm0 * korry
    } else {
        nm0
    }
}

#[cfg(test)]
mod tests {
    use super::super::c_constants::{A0_C, DCB_C, DDF_C, KORRY_OFFSET, KORRY_SLOPE};
    use super::{band0_correction, core_loudness_of_band, excitation_level};
    use crate::tables::LTQ;
    use crate::types::FieldType;

    #[test]
    fn free_field_excitation_subtracts_the_transmission_term() {
        for (band, a0) in A0_C.iter().enumerate() {
            let le = excitation_level(50.0, band, FieldType::Free);
            assert_eq!(le.to_bits(), (50.0 - a0).to_bits(), "band {band}");
        }
    }

    #[test]
    fn diffuse_and_free_field_differ_only_by_ddf() {
        for (band, ddf) in DDF_C.iter().enumerate() {
            let free = excitation_level(50.0, band, FieldType::Free);
            let diffuse = excitation_level(50.0, band, FieldType::Diffuse);
            assert_eq!(diffuse.to_bits(), (free + ddf).to_bits(), "band {band}");
            assert!((diffuse - free - ddf).abs() <= 1e-12, "band {band}");
        }
    }

    #[test]
    fn excitation_at_the_threshold_gives_zero_core_loudness() {
        for (band, &ltq) in LTQ.iter().enumerate() {
            assert_eq!(core_loudness_of_band(ltq, band), 0.0, "band {band}");
        }
        // The strict compare matters where DCB < 0: `>=` would give a value.
        let negative_dcb: Vec<usize> = (0..20).filter(|&b| DCB_C[b] < 0.0).collect();
        assert_eq!(negative_dcb, [0, 1, 2, 3, 4, 19]);
    }

    #[test]
    fn core_loudness_starts_above_threshold_plus_dcb() {
        for band in 0..20 {
            if DCB_C[band] <= 0.0 {
                assert!(
                    core_loudness_of_band(LTQ[band] + 1e-6, band) > 0.0,
                    "band {band}"
                );
            } else {
                let half = LTQ[band] + DCB_C[band] / 2.0;
                assert_eq!(core_loudness_of_band(half, band), 0.0, "band {band}");
                let above = LTQ[band] + DCB_C[band] + 1e-3;
                assert!(core_loudness_of_band(above, band) > 0.0, "band {band}");
            }
        }
    }

    #[test]
    fn nan_excitation_gives_zero_core_loudness() {
        assert_eq!(core_loudness_of_band(f64::NAN, 7), 0.0);
    }

    #[test]
    fn band0_correction_applies_only_while_korry_is_at_most_one() {
        let threshold = ((1.0 - KORRY_OFFSET) / KORRY_SLOPE).powi(5);
        let below = threshold * (1.0 - 1e-9);
        let korry = KORRY_OFFSET + KORRY_SLOPE * below.powf(0.2);
        assert!(korry < 1.0);
        assert_eq!(band0_correction(below), below * korry);
        let above = threshold * (1.0 + 1e-9);
        assert_eq!(band0_correction(above), above);
        assert_eq!(band0_correction(0.0), 0.0);
        assert!(band0_correction(f64::NAN).is_nan());
    }
}
