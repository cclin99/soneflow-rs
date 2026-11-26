use super::model::SlopeModel;
use super::positions::POSITIONS;
use crate::tables::{RNS, USL, ZUP};

const EXTENSION: f64 = 0.0001_f32 as f64;
pub(super) static MODEL: SlopeModel = product_model();

// Fixed product tables are checked against the validated constructor in tests.
// All additions and sample ownership comparisons run at compile time.
const fn product_model() -> SlopeModel {
    let mut edges = ZUP;
    let mut sample_ends = [0; 21];
    let mut i = 0;
    while i < 21 {
        edges[i] += EXTENSION;
        let mut k = 0;
        while k < POSITIONS.len() && POSITIONS[k] <= edges[i] {
            k += 1;
        }
        sample_ends[i] = k;
        i += 1;
    }
    SlopeModel {
        edges,
        sample_ends,
        thresholds: RNS,
        slopes: USL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn const_product_matches_validated_r11_constructor() {
        let validated = SlopeModel::new(ZUP, RNS, USL, EXTENSION)
            .expect("fixed product tables must form a valid slope model");
        assert_eq!(
            MODEL.edges.map(f64::to_bits),
            validated.edges.map(f64::to_bits)
        );
        assert_eq!(MODEL.sample_ends, validated.sample_ends);
        assert_eq!(
            MODEL.thresholds.map(f64::to_bits),
            validated.thresholds.map(f64::to_bits)
        );
        for (actual, expected) in MODEL.slopes.iter().zip(validated.slopes) {
            assert_eq!(actual.map(f64::to_bits), expected.map(f64::to_bits));
        }
    }
}
