//! Internal domain checks; callers supply fixed finite, nonnegative bounds.
//! These checks neither normalize values nor change the floating-point mode.
#[cfg(all(test, target_arch = "x86_64"))]
mod c4_tests;
pub(crate) const CORE_MAX: f64 = 1e306;
pub(crate) const TW_INPUT_MAX: f64 = 2.402e307;
pub(crate) const TW_STATE_MAX: f64 = 2.0 * TW_INPUT_MAX;

/// Exact all-lane success test; failures still use the caller's detailed scan.
/// `upper` is one of the fixed positive finite product bounds (never -0).
/// Integer operations preserve negative subnormals under DAZ and MXCSR flags.
///
/// # Safety
/// The caller must ensure AVX2 is available.
#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn lanes_valid(
    value: std::arch::x86_64::__m256d,
    nonnegative: bool,
    upper: f64,
) -> bool {
    use std::arch::x86_64::*;
    let bits = _mm256_castpd_si256(value);
    let zero = _mm256_setzero_si256();
    let magnitude = _mm256_and_si256(bits, _mm256_set1_epi64x(i64::MAX));
    let mut invalid = _mm256_cmpgt_epi64(magnitude, _mm256_set1_epi64x(0x7fef_ffff_ffff_ffff));
    if nonnegative {
        let negative = _mm256_cmpgt_epi64(zero, bits);
        let is_zero = _mm256_cmpeq_epi64(magnitude, zero);
        invalid = _mm256_or_si256(invalid, _mm256_andnot_si256(is_zero, negative));
        invalid = _mm256_or_si256(
            invalid,
            _mm256_cmpgt_epi64(magnitude, _mm256_set1_epi64x(upper.to_bits() as i64)),
        );
    }
    _mm256_testz_si256(invalid, invalid) != 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DomainFault {
    NonFinite,
    Negative,
    AboveUpper,
}

impl DomainFault {
    pub(crate) const fn reason_mask(self) -> u32 {
        match self {
            Self::NonFinite => 1,
            Self::Negative => 2,
            Self::AboveUpper => 4,
        }
    }
}

/// `upper` is a fixed finite nonnegative product bound, not arbitrary user input.
pub(crate) fn classify_nonnegative_bounded(value: f64, upper: f64) -> Result<(), DomainFault> {
    let bits = value.to_bits();
    let magnitude = bits & 0x7fff_ffff_ffff_ffff;
    if magnitude >= 0x7ff0_0000_0000_0000 {
        Err(DomainFault::NonFinite)
    } else if bits >> 63 != 0 && magnitude != 0 {
        Err(DomainFault::Negative)
    } else if magnitude > (upper.to_bits() & 0x7fff_ffff_ffff_ffff) {
        Err(DomainFault::AboveUpper)
    } else {
        Ok(())
    }
}

/// Pure integer D0 entry predicate bit-exact equivalent to
/// `classify_nonnegative_bounded(value, upper).is_ok()` for any positive finite `upper`.
/// Accepts `+0.0`, `-0.0`, and finite positive values `<= upper`; rejects NaN, ±Inf,
/// negative non-zero (including negative subnormals under DAZ), and values `> upper`.
/// Uses integer bit operations only (zero FP comparisons, zero MXCSR flag effects).
#[inline(always)]
pub(crate) fn is_nonnegative_bounded(value: f64, upper: f64) -> bool {
    let bits = value.to_bits();
    bits <= upper.to_bits() || bits == 0x8000_0000_0000_0000
}

pub(crate) fn validate_core(core: &[f64; 21]) -> Result<(), (usize, DomainFault)> {
    for (band, &value) in core.iter().enumerate() {
        classify_nonnegative_bounded(value, CORE_MAX).map_err(|reason| (band, reason))?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RoundingEnvironment {
    KnownRn,
    NonRn,
    Unknown,
}

pub(crate) fn read_rounding_environment() -> RoundingEnvironment {
    match rounding_bits() {
        Some(0) => RoundingEnvironment::KnownRn,
        Some(_) => RoundingEnvironment::NonRn,
        None => RoundingEnvironment::Unknown,
    }
}

/// Single-read MXCSR capture for a C9 execution unit (Design r3 §8, Amendment r8 §2.5).
/// Returns `Some(mxcsr)` on x86_64 and `None` on non-x86_64 targets.
#[inline(always)]
pub(crate) fn read_unit_mxcsr() -> Option<u32> {
    #[cfg(target_arch = "x86_64")]
    {
        Some(read_mxcsr())
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        None
    }
}

/// Pure integer qualification predicate on a captured MXCSR word (Amendment r8 §2.5, Design r3 §8):
/// RN rounding (`bits 13..=14 == 0`), all six exception masks set (`bits 7..=12 == 0x3f`),
/// and `(FTZ, DAZ)` in `{(0, 0), (1, 1)}` (`bit 15 == bit 6`).
#[inline(always)]
pub(crate) const fn is_mxcsr_qualified(mxcsr: u32) -> bool {
    (mxcsr & 0x7f80) == 0x1f80 && ((mxcsr >> 15) & 1) == ((mxcsr >> 6) & 1)
}

/// Restore a previously captured MXCSR word on x86_64 (cold replay path only).
/// Must NOT declare `preserves_flags`: Rust treats MXCSR exception flags as flags
/// (Design r3 §8, Amendment r8 §6.6).
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(crate) fn restore_mxcsr(mxcsr: u32) {
    // SAFETY: `mxcsr` was captured by `read_unit_mxcsr()` on this x86_64 thread.
    unsafe {
        std::arch::asm!(
            "ldmxcsr [{ptr}]",
            ptr = in(reg) &mxcsr,
            options(nostack),
        );
    }
}

/// Branchless D2 non-finiteness accumulator word: bit 63 is set iff `value` is NaN or ±Inf.
/// Uses integer operations only (zero FP comparisons, zero MXCSR flag effects).
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(crate) fn nonfinite_mask_bit(value: f64) -> u64 {
    (value.to_bits() & 0x7fff_ffff_ffff_ffff).wrapping_add(0x0010_0000_0000_0000)
}

fn rounding_bits() -> Option<u32> {
    #[cfg(target_arch = "x86_64")]
    {
        Some(read_mxcsr() & 0x6000)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        None
    }
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn read_mxcsr() -> u32 {
    let mut control = 0u32;
    // SAFETY: x86_64 provides SSE; STMXCSR writes exactly this live u32 slot.
    unsafe {
        std::arch::asm!("stmxcsr [{ptr}]", ptr = in(reg) &mut control,
            options(nostack, preserves_flags));
    }
    control
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_and_adjacent_values() {
        for upper in [CORE_MAX, TW_INPUT_MAX, TW_STATE_MAX] {
            for bits in [0, 1, 1u64 << 63, upper.to_bits() - 1, upper.to_bits()] {
                let value = f64::from_bits(bits);
                assert_eq!(classify_nonnegative_bounded(value, upper), Ok(()));
                assert_eq!(value.to_bits(), bits);
            }
            assert_eq!(
                classify_nonnegative_bounded(f64::from_bits(upper.to_bits() + 1), upper),
                Err(DomainFault::AboveUpper)
            );
            assert_eq!(
                classify_nonnegative_bounded(f64::MAX, upper),
                Err(DomainFault::AboveUpper)
            );
        }
    }

    #[test]
    fn fault_precedence_and_first_band() {
        for bits in [
            0x7ff0_0000_0000_0000,
            0xfff0_0000_0000_0000,
            0x7ff0_0000_0000_0001,
            0xfff8_0000_0000_1234,
        ] {
            assert_eq!(
                classify_nonnegative_bounded(f64::from_bits(bits), CORE_MAX),
                Err(DomainFault::NonFinite)
            );
        }
        for value in [-1.0, -f64::MAX, f64::from_bits(0x8000_0000_0000_0001)] {
            assert_eq!(
                classify_nonnegative_bounded(value, CORE_MAX),
                Err(DomainFault::Negative)
            );
        }
        let mut core = [0.0; 21];
        assert_eq!(validate_core(&core), Ok(()));
        core[20] = f64::INFINITY;
        core[3] = -1.0;
        assert_eq!(validate_core(&core), Err((3, DomainFault::Negative)));
        core[3] = 0.0;
        assert_eq!(validate_core(&core), Err((20, DomainFault::NonFinite)));
        assert_ne!(RoundingEnvironment::Unknown, RoundingEnvironment::KnownRn);
    }

    #[cfg(target_arch = "x86_64")]
    struct Restore(u32);
    #[cfg(target_arch = "x86_64")]
    impl Drop for Restore {
        fn drop(&mut self) {
            set_mxcsr(self.0);
        }
    }
    #[cfg(target_arch = "x86_64")]
    fn set_mxcsr(control: u32) {
        // SAFETY: tests only restore a captured value or change its documented
        // rounding/FTZ/DAZ bits, leaving reserved bits and exception masks intact.
        unsafe {
            std::arch::asm!("ldmxcsr [{ptr}]", ptr = in(reg) &control,
                options(readonly, nostack, preserves_flags));
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn daz_does_not_hide_negative_subnormal() {
        let _restore = Restore(read_mxcsr());
        set_mxcsr((_restore.0 & !0x6000) | 0x8040);
        let value = std::hint::black_box(f64::from_bits(0x8000_0000_0000_0001));
        let naive_negative = value < 0.0;
        assert!(
            !naive_negative,
            "negative control must expose DAZ comparison masking"
        );
        assert_eq!(
            classify_nonnegative_bounded(value, CORE_MAX),
            Err(DomainFault::Negative)
        );
        assert!(
            !is_nonnegative_bounded(value, CORE_MAX),
            "D0 entry predicate must reject negative subnormal under DAZ"
        );
        assert!(
            is_nonnegative_bounded(f64::from_bits(0x8000_0000_0000_0000), CORE_MAX),
            "D0 entry predicate must accept -0.0 under DAZ"
        );
        for upper in [CORE_MAX, TW_INPUT_MAX, TW_STATE_MAX] {
            for bits in [
                0u64,
                1,
                0x0010_0000_0000_0000,
                1.0_f64.to_bits(),
                upper.to_bits() - 1,
                upper.to_bits(),
                upper.to_bits() + 1,
                f64::MAX.to_bits(),
                0x7ff0_0000_0000_0000,
                0x7ff0_0000_0000_0001,
                0x7ff8_0000_0000_1234,
                0x8000_0000_0000_0000,
                0x8000_0000_0000_0001,
                0x8010_0000_0000_0000,
                (-1.0_f64).to_bits(),
                0xfff0_0000_0000_0000,
                0xfff0_0000_0000_0001,
                0xfff8_0000_0000_5678,
            ] {
                let v = f64::from_bits(bits);
                assert_eq!(
                    is_nonnegative_bounded(v, upper),
                    classify_nonnegative_bounded(v, upper).is_ok(),
                    "bit-exact mismatch for bits={bits:#018x} upper={upper}"
                );
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn modes_are_read_only_and_restore_on_unwind() {
        let saved = read_mxcsr();
        let _restore = Restore(saved);
        for mode in [0, 0x2000, 0x4000, 0x6000] {
            let control = (saved & !0x6000) | mode;
            set_mxcsr(control);
            let expected = if mode == 0 {
                RoundingEnvironment::KnownRn
            } else {
                RoundingEnvironment::NonRn
            };
            assert_eq!(read_rounding_environment(), expected);
            assert_eq!(read_mxcsr(), control);
        }
        set_mxcsr(saved);
        let caught = std::panic::catch_unwind(|| {
            let _restore = Restore(read_mxcsr());
            set_mxcsr((saved & !0x6000) | 0x6000);
            panic!("exercise restoration");
        });
        assert!(caught.is_err());
        assert_eq!(read_mxcsr(), saved);
    }

    #[cfg(not(target_arch = "x86_64"))]
    #[test]
    fn unsupported_target_is_unknown() {
        assert_eq!(read_rounding_environment(), RoundingEnvironment::Unknown);
    }
}
