//! Fallible batch stages sharing the checked scalar and AVX2 state kernels.
//!
//! One file per stage (`levels`, `nonlinear`, `temporal`) plus the output
//! checks (`validate`); `dispatch` runs a stage's bands or band groups on
//! Rayon or sequentially. `batch_hooks` and `test_counters` exist only in
//! test builds.
#[cfg(target_arch = "x86_64")]
mod avx2;
#[cfg(test)] // c9_batch_hook
#[allow(dead_code)]
pub mod batch_hooks;
mod levels;
mod nonlinear;
mod shared;
mod temporal;
#[cfg(test)]
pub(super) mod test_counters;
mod validate;

pub(super) use levels::levels;
pub(super) use nonlinear::nonlinear_validated_core;
pub(super) use temporal::temporal;
pub(super) use validate::{validate_core, validate_slopes_chunks};

// Used only by the unit tests in `tests.rs`.
#[cfg(test)]
use {nonlinear::nonlinear, validate::validate_slopes};

#[cfg(test)]
mod tests;
