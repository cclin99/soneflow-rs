//! Runtime choice between the AVX2+FMA and the scalar backend.
//!
//! Both backends give the same results within the documented tolerances; the
//! scalar one is the fallback on CPUs without AVX2 and FMA. Forcing it lets a
//! caller test or benchmark the fallback on any machine.

use std::sync::atomic::{AtomicBool, Ordering};

static FORCE_SCALAR: AtomicBool = AtomicBool::new(false);

/// Whether this CPU supports AVX2 and FMA (detected at run time). Always
/// `false` on targets other than x86_64.
#[inline]
pub fn avx2_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Force the scalar backend for the whole process (`true`), or return to
/// automatic selection (`false`).
///
/// Batch functions read the setting when they start; a `ZwtvStream` reads it
/// once, in `ZwtvStream::new`, and keeps that backend. Changing it while
/// another thread is inside a call affects only later calls.
pub fn set_force_scalar(value: bool) {
    FORCE_SCALAR.store(value, Ordering::Relaxed);
}

/// Whether a computation starting now uses the AVX2 backend:
/// [`avx2_available`] and not forced scalar by [`set_force_scalar`].
#[inline]
pub fn use_avx2() -> bool {
    avx2_available() && !FORCE_SCALAR.load(Ordering::Relaxed)
}
