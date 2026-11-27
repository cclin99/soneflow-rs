//! `main_loudness` under a non-default rounding mode, then under
//! round-to-nearest, in one process with a fixed order.
//!
//! The stage may cache values that do not depend on the input. Such a cache
//! must not remember the rounding mode of the call that filled it: every call
//! gives the bits that the stage computed at run time in the caller's rounding
//! mode would give. This file holds a single test so that its first call is
//! the first call of the process.
//!
//! The expected values are computed here from the published tables, in the
//! same operation order as the stage, and not through another path of the
//! crate (a shared path would fail together with the one under test).
//! Constant operands of inexact operations go through `black_box`, so the
//! compiler cannot fold them in round-to-nearest at build time; the
//! single-precision constants are converted in `const` items, at build time,
//! as the crate does.

#[cfg(target_arch = "x86_64")]
mod reference {
    use soneflow::internals::tables::{A0, DCB, DLL, LTQ, RAP};
    use std::hint::black_box;

    const fn single(value: f64) -> f64 {
        value as f32 as f64
    }

    const fn single_table(table: [f64; 20]) -> [f64; 20] {
        let mut out = [0.0; 20];
        let mut i = 0;
        while i < 20 {
            out[i] = single(table[i]);
            i += 1;
        }
        out
    }

    const A0_S: [f64; 20] = single_table(A0);
    const DCB_S: [f64; 20] = single_table(DCB);
    const MP1_FACTOR_S: f64 = single(0.0635);
    const KORRY_OFFSET_S: f64 = single(0.4);
    const KORRY_SLOPE_S: f64 = single(0.32);

    fn low_critical_band_levels(spec: &[f64; 28]) -> [f64; 3] {
        let mut ti = [0.0; 11];
        for band in 0..11 {
            let last = DLL.len() - 1;
            let mut row = 0;
            while row < last && spec[band] > RAP[row] - DLL[row][band] {
                row += 1;
            }
            let xp = spec[band] + DLL[row][band];
            ti[band] = 10.0_f64.powf(xp / 10.0);
        }
        let gi = [
            ti[0..6].iter().sum::<f64>(),
            ti[6..9].iter().sum::<f64>(),
            ti[9..11].iter().sum::<f64>(),
        ];
        let mut lcb = [0.0; 3];
        for (idx, energy) in gi.into_iter().enumerate() {
            if energy > 0.0 {
                lcb[idx] = 10.0 * energy.log10();
            }
        }
        lcb
    }

    /// Free-field main loudness, every term computed at run time.
    pub fn main_loudness_free(spec: &[f64; 28]) -> [f64; 21] {
        let spec = black_box(*spec);
        let lcb = low_critical_band_levels(&spec);
        let mut nm = [0.0; 21];
        for band in 0..20 {
            let level = if band < 3 { lcb[band] } else { spec[band + 8] };
            let le = level - A0_S[band];
            if le > LTQ[band] {
                let le = le - DCB_S[band];
                let mp1 = MP1_FACTOR_S * 10.0_f64.powf(0.025 * black_box(LTQ[band]));
                let mp2 =
                    (1.0 - 0.25 + 0.25 * 10.0_f64.powf(0.1 * (le - LTQ[band]))).powf(0.25) - 1.0;
                nm[band] = (mp1 * mp2).max(0.0);
            }
        }
        let korry = KORRY_OFFSET_S + KORRY_SLOPE_S * nm[0].powf(0.2);
        if korry <= 1.0 {
            nm[0] *= korry;
        }
        nm
    }
}

#[cfg(target_arch = "x86_64")]
mod mxcsr {
    pub const ROUNDING_BITS: u32 = 0x6000;
    pub const TOWARD_ZERO: u32 = 0x6000;

    pub fn read() -> u32 {
        let mut control = 0u32;
        // SAFETY: x86_64 provides SSE; STMXCSR writes exactly this u32 slot.
        unsafe {
            std::arch::asm!("stmxcsr [{p}]", p = in(reg) &mut control,
                options(nostack, preserves_flags));
        }
        control
    }

    pub fn write(control: u32) {
        // SAFETY: the test only writes back a captured word or that word with
        // its rounding-control bits changed; reserved bits and masks stay.
        unsafe {
            std::arch::asm!("ldmxcsr [{p}]", p = in(reg) &control, options(nostack));
        }
    }

    /// Restores the captured word when dropped, also while unwinding.
    pub struct Restore(pub u32);

    impl Drop for Restore {
        fn drop(&mut self) {
            write(self.0);
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn toward_zero_first_then_round_to_nearest_match_run_time_values() {
    use soneflow::internals::core::main_loudness::main_loudness;
    use soneflow::FieldType;

    // Every critical band above its threshold: low bands near 55-59 dB,
    // the bands from 300 Hz up at 59-65 dB.
    let spec: [f64; 28] = std::array::from_fn(|i| 55.0 + 0.37 * i as f64);
    let assert_bitwise = |got: &[f64; 21], want: &[f64; 21], mode: &str| {
        for band in 0..21 {
            assert_eq!(
                got[band].to_bits(),
                want[band].to_bits(),
                "{mode}: band {band}: got {:e}, run-time value {:e}",
                got[band],
                want[band]
            );
        }
    };

    let entry = mxcsr::read();
    assert_eq!(entry & mxcsr::ROUNDING_BITS, 0, "test thread starts in RN");
    let _restore = mxcsr::Restore(entry);

    mxcsr::write((entry & !mxcsr::ROUNDING_BITS) | mxcsr::TOWARD_ZERO);
    let got_rz = main_loudness(&spec, FieldType::Free).unwrap();
    let want_rz = reference::main_loudness_free(&spec);
    assert_bitwise(&got_rz, &want_rz, "toward zero");

    mxcsr::write(entry);
    let got_rn = main_loudness(&spec, FieldType::Free).unwrap();
    let want_rn = reference::main_loudness_free(&spec);
    assert_bitwise(&got_rn, &want_rn, "round to nearest");

    assert!(
        want_rn[..20].iter().all(|&n| n > 0.0),
        "every critical band is above its threshold: {want_rn:?}"
    );
    assert!(
        (0..20).any(|band| want_rz[band].to_bits() != want_rn[band].to_bits()),
        "the two rounding modes give different bits, so the order matters"
    );
}

#[cfg(not(target_arch = "x86_64"))]
#[test]
fn toward_zero_first_then_round_to_nearest_match_run_time_values() {
    // The rounding mode is read from MXCSR, which exists only on x86_64.
}
