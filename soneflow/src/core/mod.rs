pub(crate) mod bark_axis;
pub mod calc_slopes;
pub use calc_slopes::{
    try_calc_slopes, try_calc_slopes_into, try_calc_slopes_n_only, SlopeInputError,
};
pub mod main_loudness;
