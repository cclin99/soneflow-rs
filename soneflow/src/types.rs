//! Public value types shared by the stationary and time-varying pipelines.
//! Kept out of `lib.rs` so inner modules never import through the crate
//! facade (which re-exports them), keeping the module graph acyclic.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Free,
    Diffuse,
}

impl TryFrom<i32> for FieldType {
    type Error = i32;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Free),
            1 => Ok(Self::Diffuse),
            other => Err(other),
        }
    }
}

impl std::str::FromStr for FieldType {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "free" => Ok(Self::Free),
            "diffuse" => Ok(Self::Diffuse),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod field_type_tests {
    use super::FieldType;

    #[test]
    fn try_from_i32_matches_abi_table() {
        assert_eq!(FieldType::try_from(0), Ok(FieldType::Free));
        assert_eq!(FieldType::try_from(1), Ok(FieldType::Diffuse));
        assert_eq!(FieldType::try_from(2), Err(2));
        assert_eq!(FieldType::try_from(-1), Err(-1));
    }

    #[test]
    fn from_str_matches_py_vocabulary() {
        assert_eq!("free".parse(), Ok(FieldType::Free));
        assert_eq!("diffuse".parse(), Ok(FieldType::Diffuse));
        assert!("FREE".parse::<FieldType>().is_err());
    }
}

#[derive(Debug, Clone)]
pub struct LoudnessStationary {
    /// Total loudness, in sone.
    pub n: f64,
    /// Specific loudness at 240 Bark steps [sone/Bark].
    pub n_specific: Vec<f64>,
    /// Bark axis from 0.1 to 24 Bark, 240 points.
    pub bark_axis: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct LoudnessTimeVarying {
    /// Total loudness over time, in sone, on the returned time axis.
    /// Length `zwtv_out_frames(signal.len()) = ceil(ceil(L/24)/4)` (output-grid contract).
    pub n: Vec<f64>,
    /// Specific loudness, row-major (240 Bark steps, frames) [sone/Bark].
    pub n_specific: Vec<f64>,
    /// Bark axis from 0.1 to 24 Bark, 240 points.
    pub bark_axis: Vec<f64>,
    /// Time axis, in seconds, on the ISO 532-1 2 ms output grid:
    /// `time_axis[j] = 96 * j / 48000` exactly, starting at 0.
    pub time_axis: Vec<f64>,
}
