//! ISO 532-1 time-varying loudness (Zwicker, ZWTV).
//!
//! Pipeline: [`third_octave_levels`] (48 kHz → 2 kHz band levels) → core
//! loudness (`crate::core::main_loudness`) → [`nonlinear_decay`] → `slopes`
//! (`crate::core::calc_slopes`) → [`temporal_weighting`] → 2 ms output grid.
//! `processor` runs it offline over a whole signal, [`stream`] causally per
//! chunk. `grid` and `par` are leaf modules shared by the stages; no child
//! module depends on this one.

mod batch_checked;
mod grid;
pub mod nonlinear_decay;
mod par;
mod processor;
mod slopes;
pub mod stream;
pub mod temporal_weighting;
pub mod third_octave_levels;

pub use grid::zwtv_out_frames;
pub use nonlinear_decay::NlInit;
#[cfg(feature = "internals")]
pub use par::ParMode;
pub use processor::{loudness_zwtv, ZwtvProcessor};
