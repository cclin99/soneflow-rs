#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "unsafe blocks from 0.1.0 without SAFETY comments; remove once documented"
)]

use super::*;
use std::arch::x86_64::*;

struct Restore(u32);
impl Drop for Restore {
    fn drop(&mut self) {
        set_control(self.0);
    }
}
fn set_control(control: u32) {
    // SAFETY: only captured MXCSR or documented rounding/FTZ/DAZ/sticky bits.
    unsafe {
        std::arch::asm!("ldmxcsr [{ptr}]", ptr = in(reg) &control, options(readonly, nostack, preserves_flags));
    }
}

#[test]
fn c4_negative_subnormal_with_daz_is_rejected_in_each_lane() {
    if !is_x86_feature_detected!("avx2") {
        return;
    }
    let saved = read_mxcsr();
    let _restore = Restore(saved);
    let control = (saved & !0x6000) | 0x8040;
    set_control(control);
    for lane in 0..4 {
        let mut values = [0.0; 4];
        values[lane] = f64::from_bits(0x8000_0000_0000_0001);
        let before = values.map(f64::to_bits);
        let valid = unsafe { lanes_valid(_mm256_loadu_pd(values.as_ptr()), true, CORE_MAX) };
        assert!(!valid, "negative subnormal masked by DAZ at lane {lane}");
        assert_eq!(values.map(f64::to_bits), before);
        assert_eq!(read_mxcsr(), control);
    }
}

#[test]
fn c4_integer_predicate_matches_scalar_and_preserves_full_mxcsr() {
    if !is_x86_feature_detected!("avx2") {
        return;
    }
    let saved = read_mxcsr();
    let _restore = Restore(saved);
    for mode in [0, 0x40, 0x8000, 0x8040] {
        for sticky in [0, 0x3f] {
            let control = (saved & !(0x6000 | 0x8040 | 0x3f)) | mode | sticky;
            set_control(control);
            for upper in [f64::MAX, CORE_MAX, TW_INPUT_MAX, TW_STATE_MAX] {
                let edges = [
                    0,
                    1 << 63,
                    1,
                    0x000f_ffff_ffff_ffff,
                    0x0010_0000_0000_0000,
                    0x8000_0000_0000_0001,
                    0x800f_ffff_ffff_ffff,
                    upper.to_bits() - 1,
                    upper.to_bits(),
                    upper.to_bits() + 1,
                    f64::MAX.to_bits(),
                    (-f64::MAX).to_bits(),
                    0x7ff0_0000_0000_0000,
                    0xfff0_0000_0000_0000,
                    0x7ff0_0000_0000_0001,
                    0xfff0_0000_0000_0001,
                    0x7ff8_0000_1234_5678,
                    0xfff8_0000_1234_5678,
                ];
                for nonnegative in [false, true] {
                    for lane in 0..4 {
                        for edge in edges {
                            let mut values = [0.0; 4];
                            values[lane] = f64::from_bits(edge);
                            check(values, nonnegative, upper, control);
                        }
                    }
                    let mut rng = 0xdead_beef_1234_5678u64;
                    for _ in 0..4096 {
                        let values = std::array::from_fn(|_| {
                            rng ^= rng << 13;
                            rng ^= rng >> 7;
                            rng ^= rng << 17;
                            f64::from_bits(rng)
                        });
                        check(values, nonnegative, upper, control);
                    }
                }
            }
        }
    }
}

fn check(values: [f64; 4], nonnegative: bool, upper: f64, control: u32) {
    let expected = values.iter().all(|&v| {
        if nonnegative {
            classify_nonnegative_bounded(v, upper).is_ok()
        } else {
            v.to_bits() & 0x7fff_ffff_ffff_ffff < 0x7ff0_0000_0000_0000
        }
    });
    // SAFETY: test caller detects AVX2; all four f64 lanes are initialized.
    let actual = unsafe { lanes_valid(_mm256_loadu_pd(values.as_ptr()), nonnegative, upper) };
    assert_eq!(actual, expected, "{:x?}", values.map(f64::to_bits));
    assert_eq!(
        read_mxcsr(),
        control,
        "predicate changed MXCSR, including sticky flags"
    );
}

#[test]
fn c4_tw_prestate_fault_cannot_be_hidden_by_overwrite() {
    use crate::zwtv::temporal_weighting::TwState;
    let mut checked = TwState::new();
    checked.inject_numeric(2, f64::NAN);
    assert_eq!(checked.advance_checked(0.0), Err(DomainFault::NonFinite));
    let mut end_only = TwState::new();
    end_only.inject_numeric(2, f64::NAN);
    end_only.advance(0.0);
    // Negative control: the end-only checker incorrectly loses the entry fault.
    assert_eq!(end_only.validate_numeric(), Ok(()));
}
