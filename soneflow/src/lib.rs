//! ISO 532-1:2017 Zwicker loudness calculation.
//!
//! This crate implements stationary (`loudness_zwst`) and time-varying
//! (`loudness_zwtv`) loudness for calibrated 48 kHz pressure signals. The
//! implementation follows the ISO 532-1 / Annex B validation path used by the
//! repository golden data and uses AVX2+FMA dispatch for the time-varying
//! filter bank when the host CPU supports it.
//!
//! # Stationary example
//!
//! ```no_run
//! use soneflow::{loudness_zwst, FieldType};
//!
//! # fn main() -> Result<(), soneflow::SoneflowError> {
//! let signal = vec![0.0; 48_000];
//! let result = loudness_zwst(&signal, 48_000.0, FieldType::Free)?;
//! println!("N = {:.3} sone", result.n);
//! # Ok(())
//! # }
//! ```
//!
//! # Time-varying example
//!
//! ```no_run
//! use soneflow::{loudness_zwtv, FieldType};
//!
//! # fn main() -> Result<(), soneflow::SoneflowError> {
//! let signal = vec![0.0; 48_000];
//! let result = loudness_zwtv(&signal, 48_000.0, FieldType::Free)?;
//! println!("{} loudness samples", result.n.len());
//! # Ok(())
//! # }
//! ```
//!
//! # Limitations
//!
//! The public signal APIs currently accept only 48 kHz input. Signals shorter
//! than 0.1 s are rejected, and ISO 532-1 low-frequency applicability checks
//! are surfaced as `SoneflowError`.
#![warn(clippy::undocumented_unsafe_blocks)]

mod batch_guard;
pub mod core;
pub mod dsp;
pub mod error;
mod numeric_guard;
pub mod simd;
mod sone2phon;
pub mod tables;
pub mod tables_noct;
mod types;
pub mod zwst;
pub mod zwtv;

pub use error::{NumericFaultReason, NumericFaultStage, SoneflowError, StreamUsageError};
pub use sone2phon::sone2phon;
pub use types::{FieldType, LoudnessStationary, LoudnessTimeVarying};
pub use zwst::loudness_zwst;
pub use zwtv::stream::{
    DiagnosticSnapshotV1, FrameFlags, FrameQualityV1, StreamFrame, ZwtvStream, N_WARMUP_FRAMES,
};
pub use zwtv::{loudness_zwtv, zwtv_out_frames, NlInit, ZwtvProcessor};
