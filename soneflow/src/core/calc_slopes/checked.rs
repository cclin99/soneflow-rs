use super::product::MODEL;
use crate::numeric_guard::{self, DomainFault, RoundingEnvironment};

/// A rejected checked slope call. No output is written on any error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlopeInputError {
    OutputLength { got: usize, expected: usize },
    NonFinite { band: usize },
    Negative { band: usize },
    AboveUpper { band: usize },
    UnsupportedRoundingMode,
    UnsupportedEnvironment,
}

impl std::fmt::Display for SlopeInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutputLength { got, expected } => {
                write!(f, "output length {got}; expected {expected}")
            }
            Self::NonFinite { band } => write!(f, "nonfinite core at band {band}"),
            Self::Negative { band } => write!(f, "negative core at band {band}"),
            Self::AboveUpper { band } => write!(f, "core exceeds 1e306 at band {band}"),
            Self::UnsupportedRoundingMode => f.write_str("round-to-nearest mode required"),
            Self::UnsupportedEnvironment => f.write_str("rounding environment cannot be verified"),
        }
    }
}

impl std::error::Error for SlopeInputError {}

fn validate(core: &[f64; 21]) -> Result<(), SlopeInputError> {
    numeric_guard::validate_core(core).map_err(|(band, reason)| match reason {
        DomainFault::NonFinite => SlopeInputError::NonFinite { band },
        DomainFault::Negative => SlopeInputError::Negative { band },
        DomainFault::AboveUpper => SlopeInputError::AboveUpper { band },
    })?;
    match numeric_guard::read_rounding_environment() {
        RoundingEnvironment::KnownRn => Ok(()),
        RoundingEnvironment::NonRn => Err(SlopeInputError::UnsupportedRoundingMode),
        RoundingEnvironment::Unknown => Err(SlopeInputError::UnsupportedEnvironment),
    }
}

/// Checks every core band is finite and in `0..=1e306`, and requires verified
/// round-to-nearest arithmetic. Valid inputs use the unchanged slope kernel.
pub fn try_calc_slopes(core: &[f64; 21]) -> Result<(f64, [f64; 240]), SlopeInputError> {
    validate(core)?;
    Ok(MODEL.evaluate(core))
}

/// Checks length first, then core bands, then rounding mode. On error the
/// caller's output is unchanged. Successful calls allocate no memory.
pub fn try_calc_slopes_into(core: &[f64; 21], output: &mut [f64]) -> Result<f64, SlopeInputError> {
    if output.len() != 240 {
        return Err(SlopeInputError::OutputLength {
            got: output.len(),
            expected: 240,
        });
    }
    validate(core)?;
    Ok(MODEL.evaluate_into(core, output))
}

/// Checks the same domain and environment without constructing sample output.
pub fn try_calc_slopes_n_only(core: &[f64; 21]) -> Result<f64, SlopeInputError> {
    validate(core)?;
    Ok(MODEL.total_only(core))
}
