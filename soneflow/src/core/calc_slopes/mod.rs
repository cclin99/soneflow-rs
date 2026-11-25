//! Piecewise slope traversal using a frozen, independently written implementation.
//!
//! With ordinary binary64 round-to-nearest arithmetic and finite core values in
//! `0.0..=1e306`, totals and samples are finite and nonnegative. All three APIs
//! use the same total arithmetic and return identical total bits without runtime
//! allocation. Sample positions are derived from their indices.
//!
//! Negative and nonfinite cores retain that implementation's transitional zero sanitation;
//! these inputs do not constitute a valid measurement guarantee. A separate
//! checked-input layer is available through the `try_calc_slopes*` APIs below.
//! Finite cores above `1e306` retain termination and no-panic behavior, without
//! a finite-output or numerical-accuracy guarantee.
mod checked;
pub use checked::{try_calc_slopes, try_calc_slopes_into, try_calc_slopes_n_only, SlopeInputError};
pub(crate) mod model;
mod positions;
mod product;
mod walk;

#[cfg(test)]
mod checked_tests;
#[cfg(test)]
mod tests;

/// Returns total loudness and 240 index-derived specific-loudness samples.
#[inline]
pub fn calc_slopes(nm: &[f64; 21]) -> (f64, [f64; 240]) {
    product::MODEL.evaluate(nm)
}

/// Writes specific loudness without allocation. Panics unless output length is 240.
#[inline]
pub fn calc_slopes_into(nm: &[f64; 21], output: &mut [f64]) -> f64 {
    product::MODEL.evaluate_into(nm, output)
}

/// Returns total loudness without constructing or filling a sample array.
#[inline]
pub fn calc_slopes_n_only(nm: &[f64; 21]) -> f64 {
    product::MODEL.total_only(nm)
}
