//! Flush-to-zero / denormals-are-zero for the duration of a stream call, so
//! decaying filter states never hit the subnormal slow path on the audio
//! thread. The previous MXCSR is restored on drop.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "unsafe blocks from 0.1.0 without SAFETY comments; remove once documented"
)]

#[cfg(target_arch = "x86_64")]
pub(super) struct DenormalGuard {
    saved: u32,
}

#[cfg(target_arch = "x86_64")]
impl DenormalGuard {
    /// FTZ (bit 15) | DAZ (bit 6).
    const FTZ_DAZ: u32 = 0x8040;

    pub(super) fn new() -> Self {
        #[allow(deprecated)]
        let saved = unsafe { std::arch::x86_64::_mm_getcsr() };
        #[allow(deprecated)]
        unsafe {
            std::arch::x86_64::_mm_setcsr(saved | Self::FTZ_DAZ)
        };
        Self { saved }
    }
}

#[cfg(target_arch = "x86_64")]
impl Drop for DenormalGuard {
    fn drop(&mut self) {
        #[allow(deprecated)]
        unsafe {
            std::arch::x86_64::_mm_setcsr(self.saved)
        };
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub(super) struct DenormalGuard;

#[cfg(not(target_arch = "x86_64"))]
impl DenormalGuard {
    pub(super) fn new() -> Self {
        // TODO(aarch64): use the FPCR FZ bit.
        Self
    }
}
