use super::*;

#[test]
fn valid_inputs_keep_all_three_api_bits() {
    for value in [0.0, -0.0, f64::from_bits(1), 1.0, 1e306] {
        let core = [value; 21];
        let expected = calc_slopes(&core);
        let checked = try_calc_slopes(&core).unwrap();
        let mut samples = [f64::NAN; 240];
        let total = try_calc_slopes_into(&core, &mut samples).unwrap();
        assert_eq!(expected.0.to_bits(), checked.0.to_bits());
        assert_eq!(total.to_bits(), expected.0.to_bits());
        assert_eq!(
            try_calc_slopes_n_only(&core).unwrap().to_bits(),
            total.to_bits()
        );
        assert_eq!(expected.1.map(f64::to_bits), checked.1.map(f64::to_bits));
        assert_eq!(expected.1.map(f64::to_bits), samples.map(f64::to_bits));
    }
}

#[test]
fn invalid_domain_preserves_output_and_reports_first_band() {
    for (value, error) in [
        (f64::NAN, SlopeInputError::NonFinite { band: 3 }),
        (f64::INFINITY, SlopeInputError::NonFinite { band: 3 }),
        (f64::NEG_INFINITY, SlopeInputError::NonFinite { band: 3 }),
        (-1.0, SlopeInputError::Negative { band: 3 }),
        (
            f64::from_bits(0x8000_0000_0000_0001),
            SlopeInputError::Negative { band: 3 },
        ),
        (
            f64::from_bits(1e306_f64.to_bits() + 1),
            SlopeInputError::AboveUpper { band: 3 },
        ),
    ] {
        let mut core = [0.0; 21];
        core[3] = value;
        core[20] = f64::NAN;
        let mut output = [42.0; 240];
        assert_eq!(try_calc_slopes_into(&core, &mut output), Err(error));
        assert_eq!(try_calc_slopes(&core), Err(error));
        assert_eq!(try_calc_slopes_n_only(&core), Err(error));
        assert_eq!(output, [42.0; 240]);
    }
}

#[test]
fn output_length_precedes_domain_and_never_writes() {
    for len in [0, 1, 239, 241] {
        let mut output = vec![42.0; len];
        assert_eq!(
            try_calc_slopes_into(&[f64::NAN; 21], &mut output),
            Err(SlopeInputError::OutputLength {
                got: len,
                expected: 240
            })
        );
        assert!(output.iter().all(|value| *value == 42.0));
    }
}

#[test]
fn error_messages_are_frozen() {
    for (error, message) in [
        (
            SlopeInputError::OutputLength {
                got: 239,
                expected: 240,
            },
            "output length 239; expected 240",
        ),
        (
            SlopeInputError::NonFinite { band: 3 },
            "nonfinite core at band 3",
        ),
        (
            SlopeInputError::Negative { band: 4 },
            "negative core at band 4",
        ),
        (
            SlopeInputError::AboveUpper { band: 5 },
            "core exceeds 1e306 at band 5",
        ),
        (
            SlopeInputError::UnsupportedRoundingMode,
            "round-to-nearest mode required",
        ),
        (
            SlopeInputError::UnsupportedEnvironment,
            "rounding environment cannot be verified",
        ),
    ] {
        assert_eq!(error.to_string(), message);
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn rounding_rejection_is_atomic_and_follows_domain_checks() {
    struct Restore(u32);
    impl Drop for Restore {
        fn drop(&mut self) {
            // SAFETY: restore the unchanged control word captured on this thread.
            unsafe {
                std::arch::asm!("ldmxcsr [{p}]", p = in(reg) &self.0, options(nostack, preserves_flags));
            }
        }
    }
    let mut original = 0u32;
    // SAFETY: x86_64 supports SSE and this is a writable u32 slot.
    unsafe {
        std::arch::asm!("stmxcsr [{p}]", p = in(reg) &mut original, options(nostack, preserves_flags));
    }
    let _restore = Restore(original);
    for mode in [0x2000, 0x4000, 0x6000] {
        let changed = (original & !0x6000) | mode;
        // SAFETY: only the two documented rounding bits differ from a valid word.
        unsafe {
            std::arch::asm!("ldmxcsr [{p}]", p = in(reg) &changed, options(nostack, preserves_flags));
        }
        let mut output = [42.0; 240];
        assert_eq!(
            try_calc_slopes_into(&[1.0; 21], &mut output),
            Err(SlopeInputError::UnsupportedRoundingMode)
        );
        assert_eq!(
            try_calc_slopes(&[1.0; 21]),
            Err(SlopeInputError::UnsupportedRoundingMode)
        );
        assert_eq!(
            try_calc_slopes_n_only(&[1.0; 21]),
            Err(SlopeInputError::UnsupportedRoundingMode)
        );
        assert_eq!(output, [42.0; 240]);
        assert_eq!(
            try_calc_slopes_n_only(&[f64::NAN; 21]),
            Err(SlopeInputError::NonFinite { band: 0 })
        );
        let mut observed = 0u32;
        // SAFETY: observe, without altering, the calling thread's control word.
        unsafe {
            std::arch::asm!("stmxcsr [{p}]", p = in(reg) &mut observed, options(nostack, preserves_flags));
        }
        assert_eq!(observed, changed);
    }
}
