use thiserror::Error;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumericFaultStage {
    Filter = 1,
    Core = 2,
    Nonlinear = 3,
    Slope = 4,
    TemporalWeighting = 5,
    Output = 6,
    Environment = 7,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumericFaultReason {
    NonFinite = 1,
    NegativeDomain = 2,
    AboveValidDomain = 4,
    UnsupportedRounding = 8,
    UnsupportedEnvironment = 16,
}

/// Usage errors that the checked stream calls (`push_with_quality`,
/// `flush_with_quality`, `reset_with_diagnostics`, `ack_diagnostics`) return
/// instead of panicking. `code` is the number the C ABI returns for each.
///
/// # Examples
///
/// ```
/// use soneflow::{FieldType, FrameQualityV1, StreamFrame, StreamUsageError, ZwtvStream};
///
/// let mut stream = ZwtvStream::new(FieldType::Free);
/// let mut out = [StreamFrame::default(); 4];
/// let mut quality = [FrameQualityV1::default(); 4];
/// stream.flush_with_quality(&mut out, &mut quality)?;
/// let err = stream.push_with_quality(&[0.0; 24], &mut out, &mut quality).unwrap_err();
/// assert_eq!(err, StreamUsageError::AfterFlush);
/// assert_eq!(err.code(), -6);
/// # Ok::<(), StreamUsageError>(())
/// ```
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum StreamUsageError {
    #[error("push after flush requires reset")]
    AfterFlush,
    #[error("output frame buffer is too small")]
    OutputCapacity,
    #[error("output quality buffer is too small")]
    QualityCapacity,
    #[error("the complete operation cannot be represented on the stream timeline")]
    TimelineExhausted,
    #[error("session identity is exhausted; create a new stream")]
    SessionExhausted,
    #[error("diagnostic sequence changed before acknowledgement")]
    StaleSequence,
    #[error("saturated diagnostics cannot be safely acknowledged")]
    CounterSaturated,
    #[error("unsupported rounding mode (invalid creation environment: {creation})")]
    UnsupportedRoundingMode { creation: bool },
    #[error("rounding environment cannot be checked (creation: {creation})")]
    UnsupportedEnvironment { creation: bool },
}

impl StreamUsageError {
    pub fn code(&self) -> i32 {
        match self {
            Self::AfterFlush => -6,
            Self::OutputCapacity | Self::QualityCapacity => -4,
            Self::TimelineExhausted => -7,
            Self::SessionExhausted => -8,
            Self::StaleSequence => -9,
            Self::CounterSaturated => -10,
            Self::UnsupportedRoundingMode { .. } => -11,
            Self::UnsupportedEnvironment { .. } => -12,
        }
    }
}

/// Errors of the batch functions (`loudness_zwst`, `loudness_zwtv`,
/// `ZwtvProcessor`). `code` is the number the C ABI returns for each.
///
/// # Examples
///
/// ```
/// use soneflow::{loudness_zwst, FieldType, SoneflowError};
///
/// let mut signal = vec![0.0; 4_800];
/// assert_eq!(
///     loudness_zwst(&signal[..100], 48_000.0, FieldType::Free).unwrap_err(),
///     SoneflowError::SignalTooShort { got: 100, need: 4_800 }
/// );
/// signal[7] = f64::NAN;
/// let err = loudness_zwst(&signal, 48_000.0, FieldType::Free).unwrap_err();
/// assert_eq!(err, SoneflowError::NonFiniteInput { idx: 7 });
/// assert_eq!(err.code(), 4);
/// ```
#[derive(Debug, Error, PartialEq)]
pub enum SoneflowError {
    #[error("1/3 octave band level exceeds 120 dB below 300 Hz; Zwicker method not applicable")]
    LevelExceeds120dB,
    #[error("signal too short: got {got} samples, need at least {need}")]
    SignalTooShort { got: usize, need: usize },
    #[error("sampling rate must be 48000 Hz, got {0}")]
    UnsupportedSampleRate(f64),
    /// A NaN or infinite sample in a batch input (`loudness_zwst`,
    /// `loudness_zwtv`, `ZwtvProcessor`). Batch functions have the whole
    /// signal and reject it; the stream path instead zeroes the sample and
    /// raises `FrameFlags::NONFINITE_INPUT` because it must never fail on the
    /// audio thread. `idx` is the first offending sample.
    #[error("non-finite sample (NaN or infinite) at index {idx}; batch input is rejected")]
    NonFiniteInput { idx: usize },
    #[error("internal numeric fault at {stage:?}/{reason:?}: index {index}, band {band:?}")]
    InternalNumericFault {
        stage: NumericFaultStage,
        reason: NumericFaultReason,
        index: usize,
        band: Option<usize>,
    },
}

impl SoneflowError {
    /// 穩定數值代碼；C ABI 的正值錯誤碼鏡像本表。
    pub fn code(&self) -> i32 {
        match self {
            Self::LevelExceeds120dB => 1,
            Self::SignalTooShort { .. } => 2,
            Self::UnsupportedSampleRate(_) => 3,
            Self::NonFiniteInput { .. } => 4,
            Self::InternalNumericFault { .. } => 5,
        }
    }

    /// Batch-input contract shared by `loudness_zwst` and `loudness_zwtv`:
    /// 48 kHz, at least 0.1 s (4800 samples), every sample finite, checked in
    /// that order.
    pub(crate) fn check_batch_input(signal: &[f64], fs: f64) -> Result<(), Self> {
        const FS: f64 = 48_000.0;
        const MIN_LEN: usize = 4800;
        if fs != FS {
            return Err(Self::UnsupportedSampleRate(fs));
        }
        if signal.len() < MIN_LEN {
            return Err(Self::SignalTooShort {
                got: signal.len(),
                need: MIN_LEN,
            });
        }
        Self::check_finite(signal)
    }

    /// Batch-input finiteness check shared by the stationary and time-varying
    /// entry points. Runs after the sample-rate and length checks.
    pub(crate) fn check_finite(signal: &[f64]) -> Result<(), Self> {
        match signal.iter().position(|v| !v.is_finite()) {
            Some(idx) => Err(Self::NonFiniteInput { idx }),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SoneflowError, StreamUsageError};

    /// Mirrors the negative C ABI codes in `soneflow.h` (`SNF_ERR_LIFECYCLE`
    /// = -6 … `SNF_ERR_UNSUPPORTED_ENVIRONMENT` = -12; both capacity errors
    /// map to -4, which the C ABI returns for an insufficient output buffer).
    /// The `creation` payload does not change the code.
    #[test]
    fn stream_usage_error_codes_are_frozen() {
        use StreamUsageError::*;
        let table = [
            (AfterFlush, -6),
            (OutputCapacity, -4),
            (QualityCapacity, -4),
            (TimelineExhausted, -7),
            (SessionExhausted, -8),
            (StaleSequence, -9),
            (CounterSaturated, -10),
            (UnsupportedRoundingMode { creation: false }, -11),
            (UnsupportedRoundingMode { creation: true }, -11),
            (UnsupportedEnvironment { creation: false }, -12),
            (UnsupportedEnvironment { creation: true }, -12),
        ];
        for (err, code) in table {
            assert_eq!(err.code(), code, "{err:?}");
        }
    }

    #[test]
    fn error_codes_are_frozen() {
        assert_eq!(SoneflowError::LevelExceeds120dB.code(), 1);
        assert_eq!(
            SoneflowError::SignalTooShort { got: 0, need: 4800 }.code(),
            2
        );
        assert_eq!(SoneflowError::UnsupportedSampleRate(44_100.0).code(), 3);
        assert_eq!(SoneflowError::NonFiniteInput { idx: 7 }.code(), 4);
    }

    #[test]
    fn batch_input_checks_rate_then_length_then_finiteness() {
        let ok = vec![0.0; 4800];
        assert_eq!(SoneflowError::check_batch_input(&ok, 48_000.0), Ok(()));
        let mut bad = vec![f64::NAN; 10];
        assert_eq!(
            SoneflowError::check_batch_input(&bad, 44_100.0),
            Err(SoneflowError::UnsupportedSampleRate(44_100.0))
        );
        assert_eq!(
            SoneflowError::check_batch_input(&bad, 48_000.0),
            Err(SoneflowError::SignalTooShort {
                got: 10,
                need: 4800
            })
        );
        bad.resize(4800, 0.0);
        assert_eq!(
            SoneflowError::check_batch_input(&bad, 48_000.0),
            Err(SoneflowError::NonFiniteInput { idx: 0 })
        );
    }

    #[test]
    fn check_finite_reports_first_offending_index() {
        assert_eq!(SoneflowError::check_finite(&[0.0, 1.0, -1.0]), Ok(()));
        assert_eq!(
            SoneflowError::check_finite(&[0.0, f64::NAN, f64::INFINITY]),
            Err(SoneflowError::NonFiniteInput { idx: 1 })
        );
        assert_eq!(
            SoneflowError::check_finite(&[f64::NEG_INFINITY]),
            Err(SoneflowError::NonFiniteInput { idx: 0 })
        );
        // Subnormals and the largest finite value are valid input.
        assert_eq!(
            SoneflowError::check_finite(&[f64::MIN_POSITIVE / 4.0, f64::MAX, -f64::MAX]),
            Ok(())
        );
    }
}
