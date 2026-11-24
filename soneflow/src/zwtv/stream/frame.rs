//! Output frame of the stream API. Part of the frozen stream / C ABI v1
//! contract: layout, flag bits and the warm-up length must not change.

/// Frozen v1 contract: output frames with `t_frame_index < 580` carry
/// `FrameFlags::WARMUP`. Derived as ceil((8*75 ms + 8*70 ms) / 2 ms) and
/// calibrated in July 2026 so that the stream had converged to the then
/// MoSQITo-seeded batch result within 1e-9 (first sustained frame 544 on the
/// synthetic gate; the 5-tau estimate of 363 was not enough).
///
/// The batch path also starts the nonlinear decay from
/// zero state and equals the stream bitwise from frame 0, so the flag no
/// longer marks a batch/stream difference. It remains a conservative
/// filter-bank settling marker and is unchanged for stream API / C ABI v1
/// stability.
pub const N_WARMUP_FRAMES: u64 = 580;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameFlags(u32);

impl FrameFlags {
    pub const CLAMPED_120DB: Self = Self(1);
    pub const NONFINITE_INPUT: Self = Self(1 << 1);
    pub const WARMUP: Self = Self(1 << 2);

    pub const fn bits(self) -> u32 {
        self.0
    }
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
    pub fn take(&mut self) -> Self {
        std::mem::replace(self, Self(0))
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StreamFrame {
    pub t_frame_index: u64,
    pub n: f64,
    pub n_phon: f64,
    pub flags: FrameFlags,
    pub _reserved: u32,
}

#[cfg(test)]
mod tests {
    use super::{FrameFlags, StreamFrame};

    #[test]
    fn flag_bits_and_frame_layout_are_frozen() {
        assert_eq!(FrameFlags::CLAMPED_120DB.bits(), 1);
        assert_eq!(FrameFlags::NONFINITE_INPUT.bits(), 2);
        assert_eq!(FrameFlags::WARMUP.bits(), 4);
        assert_eq!(std::mem::size_of::<StreamFrame>(), 32);
    }

    #[test]
    fn insert_contains_and_take() {
        let mut flags = FrameFlags::default();
        flags.insert(FrameFlags::CLAMPED_120DB);
        flags.insert(FrameFlags::WARMUP);
        assert!(flags.contains(FrameFlags::WARMUP));
        assert!(!flags.contains(FrameFlags::NONFINITE_INPUT));
        assert_eq!(flags.take().bits(), 5);
        assert_eq!(flags, FrameFlags::default());
    }
}
