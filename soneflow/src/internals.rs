//! Internal modules, for the crate's own tests and benches and for research
//! code that needs a single pipeline stage (feature `internals`).
//!
//! **Not covered by semver.** Anything here may change or disappear in any
//! release, including patch releases. The stable API is what the crate root
//! exports (and `soneflow::simd`).
//!
//! Each module re-exports the public items of the internal module with the
//! same name, so `soneflow::zwtv::X` in 0.1.0 is `soneflow::internals::zwtv::X`.
//! The lists are explicit: a glob re-export would not mark the items as
//! reachable, and their own re-exports would then warn as unused.

/// Stage cores shared by both pipelines: main loudness and slopes.
pub mod core {
    pub use crate::core::{calc_slopes, main_loudness};
    pub use crate::core::{
        try_calc_slopes, try_calc_slopes_into, try_calc_slopes_n_only, SlopeInputError,
    };
}

/// Second-order-section filtering and zero-phase decimation.
pub mod dsp {
    pub use crate::dsp::{filtfilt, sos};
}

/// Loudness tables.
pub mod tables {
    pub use crate::tables::*;
}

/// Third-octave and decimation filter coefficients.
pub mod tables_noct {
    pub use crate::tables_noct::*;
}

/// Stationary pipeline stages.
pub mod zwst {
    pub use crate::zwst::{loudness_zwst, noct_spectrum_rms, spec_to_db};
}

/// Time-varying pipeline stages, scalar and AVX2 variants, and stream internals.
pub mod zwtv {
    pub use crate::zwtv::{
        loudness_zwtv, nonlinear_decay, stream, temporal_weighting, third_octave_levels,
        zwtv_out_frames, NlInit, ParMode, ZwtvProcessor,
    };
}
