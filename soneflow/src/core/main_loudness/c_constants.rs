//! The constants of the core loudness stage, held in single precision.

use crate::tables::{A0, DCB, DDF};

/// The value the core loudness stage computes with for a constant held in
/// single precision: the decimal rounded to binary32, then widened to `f64`.
/// `crate::tables` keeps the decimal values of the standard; computing with
/// the binary64 decimals instead moves the core loudness by up to 2.7e-8
/// relative.
pub(super) const fn c_float_literal(value: f64) -> f64 {
    value as f32 as f64
}

pub(super) const fn c_float_table(table: [f64; 20]) -> [f64; 20] {
    let mut out = [0.0; 20];
    let mut i = 0;
    while i < 20 {
        out[i] = c_float_literal(table[i]);
        i += 1;
    }
    out
}

/// `A0`, `DDF`, `DCB` held in single precision, widened to `f64`.
pub(super) const A0_C: [f64; 20] = c_float_table(A0);
pub(super) const DDF_C: [f64; 20] = c_float_table(DDF);
pub(super) const DCB_C: [f64; 20] = c_float_table(DCB);
/// Factor 0.0635 of the core loudness formula, in single precision.
pub(super) const MP1_FACTOR: f64 = c_float_literal(0.0635);
/// Band 0 correction `0.4 + 0.32 * N^0.2`, both constants in single precision.
pub(super) const KORRY_OFFSET: f64 = c_float_literal(0.4);
pub(super) const KORRY_SLOPE: f64 = c_float_literal(0.32);

#[cfg(test)]
mod tests {
    use super::{c_float_table, A0_C, DCB_C, DDF_C, KORRY_OFFSET, KORRY_SLOPE, MP1_FACTOR};
    use crate::tables::{A0, DCB, DDF};

    /// The tables are built at compile time; calling the builder at run time
    /// gives the same bits.
    #[test]
    fn run_time_table_equals_the_const_table() {
        for (decimal, c) in [(A0, A0_C), (DDF, DDF_C), (DCB, DCB_C)] {
            let run_time = c_float_table(std::hint::black_box(decimal));
            for (r, c) in run_time.iter().zip(c) {
                assert_eq!(r.to_bits(), c.to_bits());
            }
        }
    }

    /// Every C constant equals the float32 literal parsed from its decimal
    /// text, widened (no double rounding through binary64 for these values).
    #[test]
    fn c_constants_are_the_widened_float32_literals() {
        let literal = |v: f64| format!("{v}").parse::<f32>().unwrap() as f64;
        for (decimal, c) in [(A0, A0_C), (DDF, DDF_C), (DCB, DCB_C)] {
            for (d, c) in decimal.iter().zip(c) {
                assert_eq!(c.to_bits(), literal(*d).to_bits(), "{d}");
            }
        }
        assert_eq!(MP1_FACTOR, 0.0635_f32 as f64);
        assert_eq!(
            (KORRY_OFFSET, KORRY_SLOPE),
            (0.4_f32 as f64, 0.32_f32 as f64)
        );
        assert_ne!(
            MP1_FACTOR, 0.0635,
            "the single-precision constant is not the binary64 decimal"
        );
    }
}
