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
//! ```
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
//! ```
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
//! The examples here and on the functions, `ZwtvProcessor`, `ZwtvStream` and
//! the error types run as doctests. `docs/BEHAVIOUR-CONTRACTS.md` lists the
//! promises of the API and the tests that guard them.
//!
//! # Limitations
//!
//! The public signal APIs currently accept only 48 kHz input. Signals shorter
//! than 0.1 s are rejected, and ISO 532-1 low-frequency applicability checks
//! are surfaced as `SoneflowError`.
#![warn(clippy::undocumented_unsafe_blocks)]

/// The internal modules are not part of the public API; tests and benches
/// reach them through `soneflow::internals` (feature `internals`).
///
/// ```
/// use soneflow::simd::{avx2_available, set_force_scalar, use_avx2};
/// use soneflow::{loudness_zwst, loudness_zwtv, ZwtvStream};
/// ```
///
/// ```compile_fail
/// use soneflow::core;
/// ```
///
/// ```compile_fail
/// use soneflow::dsp;
/// ```
///
/// ```compile_fail
/// use soneflow::error;
/// ```
///
/// ```compile_fail
/// use soneflow::tables;
/// ```
///
/// ```compile_fail
/// use soneflow::tables_noct;
/// ```
///
/// ```compile_fail
/// use soneflow::zwst;
/// ```
///
/// ```compile_fail
/// use soneflow::zwtv;
/// ```
#[cfg_attr(
    not(feature = "internals"),
    doc = "```compile_fail
use soneflow::internals;
```"
)]
#[cfg_attr(
    feature = "internals",
    doc = "```
use soneflow::internals::zwtv::ParMode;
```"
)]
mod api_boundary {}

mod batch_guard;
mod core;
mod dsp;
mod error;
#[cfg(feature = "internals")]
pub mod internals;
mod numeric_guard;
pub mod simd;
mod sone2phon;
mod tables;
mod tables_noct;
mod types;
mod zwst;
mod zwtv;

pub use error::{NumericFaultReason, NumericFaultStage, SoneflowError, StreamUsageError};
pub use sone2phon::sone2phon;
pub use types::{FieldType, LoudnessStationary, LoudnessTimeVarying};
pub use zwst::loudness_zwst;
pub use zwtv::stream::{
    DiagnosticSnapshotV1, FrameFlags, FrameQualityV1, StreamFrame, ZwtvStream, N_WARMUP_FRAMES,
};
pub use zwtv::{loudness_zwtv, zwtv_out_frames, NlInit, ZwtvProcessor};
