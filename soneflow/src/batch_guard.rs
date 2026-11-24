//! Checked batch boundaries. These predicates never normalize numeric values.
use crate::error::{NumericFaultReason as Reason, NumericFaultStage as Stage, SoneflowError};
use crate::numeric_guard::{self, DomainFault, RoundingEnvironment};

pub(crate) fn fault(
    stage: Stage,
    reason: DomainFault,
    index: usize,
    band: Option<usize>,
) -> SoneflowError {
    let reason = match reason {
        DomainFault::NonFinite => Reason::NonFinite,
        DomainFault::Negative => Reason::NegativeDomain,
        DomainFault::AboveUpper => Reason::AboveValidDomain,
    };
    SoneflowError::InternalNumericFault {
        stage,
        reason,
        index,
        band,
    }
}

pub(crate) fn environment() -> Result<(), SoneflowError> {
    let reason = match numeric_guard::read_rounding_environment() {
        RoundingEnvironment::KnownRn => return Ok(()),
        RoundingEnvironment::NonRn => Reason::UnsupportedRounding,
        RoundingEnvironment::Unknown => Reason::UnsupportedEnvironment,
    };
    Err(SoneflowError::InternalNumericFault {
        stage: Stage::Environment,
        reason,
        index: 0,
        band: None,
    })
}

pub(crate) fn core(values: &[f64; 21], index: usize) -> Result<(), SoneflowError> {
    numeric_guard::validate_core(values)
        .map_err(|(band, reason)| fault(Stage::Core, reason, index, Some(band)))
}

pub(crate) fn bounded(
    value: f64,
    upper: f64,
    stage: Stage,
    index: usize,
    band: Option<usize>,
) -> Result<(), SoneflowError> {
    numeric_guard::classify_nonnegative_bounded(value, upper)
        .map_err(|reason| fault(stage, reason, index, band))
}
