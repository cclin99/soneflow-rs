//! Test-only counters of the paths each batch stage takes.

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct TolDispatchCounts {
    pub scalar_qualified: usize,
    pub scalar_unqualified: usize,
    pub avx2_qualified: usize,
    pub avx2_unqualified: usize,
    pub scalar_fast_blocks: usize,
    pub scalar_cold_replays: usize,
    pub avx2_fast_blocks: usize,
    pub avx2_cold_replays: usize,
    pub nl_scalar_qualified: usize,
    pub nl_scalar_unqualified: usize,
    pub nl_scalar_fast_frames: usize,
    pub nl_scalar_entry_fallbacks: usize,
    pub nl_avx2_qualified: usize,
    pub nl_avx2_unqualified: usize,
    pub nl_avx2_fast_frames: usize,
    pub nl_avx2_entry_fallbacks: usize,
    pub slopes_qualified: usize,
    pub slopes_unqualified: usize,
    pub slopes_unqual_validated: usize,
    pub tw_qualified: usize,
    pub tw_unqualified: usize,
    pub tw_fast_frames: usize,
    pub tw_entry_fallbacks: usize,
}

#[derive(Default)]
pub(super) struct Sink {
    scalar_qualified: AtomicUsize,
    scalar_unqualified: AtomicUsize,
    avx2_qualified: AtomicUsize,
    avx2_unqualified: AtomicUsize,
    scalar_fast_blocks: AtomicUsize,
    scalar_cold_replays: AtomicUsize,
    avx2_fast_blocks: AtomicUsize,
    avx2_cold_replays: AtomicUsize,
    nl_scalar_qualified: AtomicUsize,
    nl_scalar_unqualified: AtomicUsize,
    nl_scalar_fast_frames: AtomicUsize,
    nl_scalar_entry_fallbacks: AtomicUsize,
    nl_avx2_qualified: AtomicUsize,
    nl_avx2_unqualified: AtomicUsize,
    nl_avx2_fast_frames: AtomicUsize,
    nl_avx2_entry_fallbacks: AtomicUsize,
    slopes_qualified: AtomicUsize,
    slopes_unqualified: AtomicUsize,
    slopes_unqual_validated: AtomicUsize,
    tw_qualified: AtomicUsize,
    tw_unqualified: AtomicUsize,
    tw_fast_frames: AtomicUsize,
    tw_entry_fallbacks: AtomicUsize,
}

impl Sink {
    pub(super) fn snapshot(&self) -> TolDispatchCounts {
        TolDispatchCounts {
            scalar_qualified: self.scalar_qualified.load(Ordering::SeqCst),
            scalar_unqualified: self.scalar_unqualified.load(Ordering::SeqCst),
            avx2_qualified: self.avx2_qualified.load(Ordering::SeqCst),
            avx2_unqualified: self.avx2_unqualified.load(Ordering::SeqCst),
            scalar_fast_blocks: self.scalar_fast_blocks.load(Ordering::SeqCst),
            scalar_cold_replays: self.scalar_cold_replays.load(Ordering::SeqCst),
            avx2_fast_blocks: self.avx2_fast_blocks.load(Ordering::SeqCst),
            avx2_cold_replays: self.avx2_cold_replays.load(Ordering::SeqCst),
            nl_scalar_qualified: self.nl_scalar_qualified.load(Ordering::SeqCst),
            nl_scalar_unqualified: self.nl_scalar_unqualified.load(Ordering::SeqCst),
            nl_scalar_fast_frames: self.nl_scalar_fast_frames.load(Ordering::SeqCst),
            nl_scalar_entry_fallbacks: self.nl_scalar_entry_fallbacks.load(Ordering::SeqCst),
            nl_avx2_qualified: self.nl_avx2_qualified.load(Ordering::SeqCst),
            nl_avx2_unqualified: self.nl_avx2_unqualified.load(Ordering::SeqCst),
            nl_avx2_fast_frames: self.nl_avx2_fast_frames.load(Ordering::SeqCst),
            nl_avx2_entry_fallbacks: self.nl_avx2_entry_fallbacks.load(Ordering::SeqCst),
            slopes_qualified: self.slopes_qualified.load(Ordering::SeqCst),
            slopes_unqualified: self.slopes_unqualified.load(Ordering::SeqCst),
            slopes_unqual_validated: self.slopes_unqual_validated.load(Ordering::SeqCst),
            tw_qualified: self.tw_qualified.load(Ordering::SeqCst),
            tw_unqualified: self.tw_unqualified.load(Ordering::SeqCst),
            tw_fast_frames: self.tw_fast_frames.load(Ordering::SeqCst),
            tw_entry_fallbacks: self.tw_entry_fallbacks.load(Ordering::SeqCst),
        }
    }
}

thread_local! {
    static ACTIVE_SINK: RefCell<Option<Arc<Sink>>> = const { RefCell::new(None) };
}

pub(super) fn attach_thread(sink: Option<Arc<Sink>>) -> Option<Arc<Sink>> {
    ACTIVE_SINK.with(|cell| cell.replace(sink))
}

#[inline]
pub(super) fn record_scalar(qualified: bool) {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            if qualified {
                sink.scalar_qualified.fetch_add(1, Ordering::Relaxed);
            } else {
                sink.scalar_unqualified.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

#[cfg(target_arch = "x86_64")]
#[inline]
pub(super) fn record_scalar_fast_block() {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            sink.scalar_fast_blocks.fetch_add(1, Ordering::Relaxed);
        }
    });
}

#[cfg(target_arch = "x86_64")]
#[inline]
pub(super) fn record_scalar_cold_replay() {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            sink.scalar_cold_replays.fetch_add(1, Ordering::Relaxed);
        }
    });
}

#[cfg(target_arch = "x86_64")]
#[inline]
pub(super) fn record_avx2(qualified: bool) {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            if qualified {
                sink.avx2_qualified.fetch_add(1, Ordering::Relaxed);
            } else {
                sink.avx2_unqualified.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

#[cfg(target_arch = "x86_64")]
#[inline]
pub(super) fn record_avx2_fast_block() {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            sink.avx2_fast_blocks.fetch_add(1, Ordering::Relaxed);
        }
    });
}

#[cfg(target_arch = "x86_64")]
#[inline]
pub(super) fn record_avx2_cold_replay() {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            sink.avx2_cold_replays.fetch_add(1, Ordering::Relaxed);
        }
    });
}

#[inline]
#[allow(dead_code)]
pub(super) fn record_nl_scalar(qualified: bool) {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            if qualified {
                sink.nl_scalar_qualified.fetch_add(1, Ordering::Relaxed);
            } else {
                sink.nl_scalar_unqualified.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

#[inline]
#[allow(dead_code)]
pub(super) fn record_nl_scalar_fast_frame() {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            sink.nl_scalar_fast_frames.fetch_add(1, Ordering::Relaxed);
        }
    });
}

#[inline]
#[allow(dead_code)]
pub(super) fn record_nl_scalar_entry_fallback() {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            sink.nl_scalar_entry_fallbacks
                .fetch_add(1, Ordering::Relaxed);
        }
    });
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[allow(dead_code)]
pub(super) fn record_nl_avx2(qualified: bool) {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            if qualified {
                sink.nl_avx2_qualified.fetch_add(1, Ordering::Relaxed);
            } else {
                sink.nl_avx2_unqualified.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[allow(dead_code)]
pub(super) fn record_nl_avx2_fast_frame() {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            sink.nl_avx2_fast_frames.fetch_add(1, Ordering::Relaxed);
        }
    });
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[allow(dead_code)]
pub(super) fn record_nl_avx2_entry_fallback() {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            sink.nl_avx2_entry_fallbacks.fetch_add(1, Ordering::Relaxed);
        }
    });
}

#[inline]
#[allow(dead_code)]
pub(in crate::zwtv) fn record_slopes(qualified: bool) {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            if qualified {
                sink.slopes_qualified.fetch_add(1, Ordering::Relaxed);
            } else {
                sink.slopes_unqualified.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

#[inline]
#[allow(dead_code)]
pub(super) fn record_slopes_unqual_validated(count: usize) {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            sink.slopes_unqual_validated
                .fetch_add(count, Ordering::Relaxed);
        }
    });
}

#[inline]
#[allow(dead_code)]
pub(super) fn record_tw(qualified: bool) {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            if qualified {
                sink.tw_qualified.fetch_add(1, Ordering::Relaxed);
            } else {
                sink.tw_unqualified.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

#[inline]
#[allow(dead_code)]
pub(super) fn record_tw_fast_frame() {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            sink.tw_fast_frames.fetch_add(1, Ordering::Relaxed);
        }
    });
}

#[inline]
#[allow(dead_code)]
pub(super) fn record_tw_entry_fallback() {
    ACTIVE_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            sink.tw_entry_fallbacks.fetch_add(1, Ordering::Relaxed);
        }
    });
}
