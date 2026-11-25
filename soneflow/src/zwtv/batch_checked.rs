//! Fallible batch stages sharing the checked scalar and AVX2 state kernels.
#[cfg(target_arch = "x86_64")]
mod avx2;
use super::grid::DEC_FACTOR;
use super::nonlinear_decay::{nl_coeffs, NlBandState, NlInit};
use super::par::ParMode;
use super::temporal_weighting::TwState;
use super::third_octave_levels::{intensity_to_db, TolBandState};
use crate::batch_guard;
use crate::error::{NumericFaultStage as Stage, SoneflowError};
use crate::numeric_guard::DomainFault;
use rayon::prelude::*;

type Fault = (usize, usize, DomainFault);

/// Test-only Order 5 batch hook scheduler & due-rule tracker (Design r3 §6, §4, §8).
/// Completely absent from default product builds (`#[cfg(test)]`); exposed to external
/// verification drivers solely via candidate overlay `instrument_batch_c9.py`.
#[cfg(test)] // c9_batch_hook
#[allow(dead_code)]
pub mod batch_hooks {
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct BatchHookEvent {
        pub public_call_id: u64,
        pub sample: usize,
        pub stage: u32,
        pub band: usize,
        pub field: usize,
        pub raw_u64: u64,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct BatchDueRecord {
        pub public_call_id: u64,
        pub stage: u32,
        pub band: usize,
        pub index: usize,
        pub due: bool,
    }

    struct ScheduleStore {
        schedule: Vec<BatchHookEvent>,
        next_call_id: u64,
        active_call_id: u64,
        executed_writes: Option<Arc<AtomicUsize>>,
        due_log: Option<Arc<Mutex<Vec<BatchDueRecord>>>>,
    }

    impl ScheduleStore {
        const fn empty() -> Self {
            Self {
                schedule: Vec::new(),
                next_call_id: 0,
                active_call_id: 0,
                executed_writes: None,
                due_log: None,
            }
        }
    }

    static HAS_GLOBAL: AtomicBool = AtomicBool::new(false);
    static GLOBAL_DUE_ON: AtomicBool = AtomicBool::new(false);
    static GLOBAL: Mutex<ScheduleStore> = Mutex::new(ScheduleStore::empty());

    thread_local! {
        static LOCAL_ACTIVE: RefCell<bool> = const { RefCell::new(false) };
        static LOCAL: RefCell<ScheduleStore> = const { RefCell::new(ScheduleStore::empty()) };
    }

    /// Snapshot of hooks & counters for one batch stage, captured on the caller thread
    /// and shared (`Sync`) across Rayon worker threads with zero lock contention.
    pub(crate) struct StageHookContext {
        pub(crate) call_id: u64,
        pub(crate) events: Vec<BatchHookEvent>,
        executed_writes: Option<Arc<AtomicUsize>>,
        due_log: Option<Arc<Mutex<Vec<BatchDueRecord>>>>,
    }

    impl StageHookContext {
        pub(crate) fn empty() -> Self {
            Self {
                call_id: 0,
                events: Vec::new(),
                executed_writes: None,
                due_log: None,
            }
        }

        #[inline]
        pub(crate) fn has_hooks(&self) -> bool {
            !self.events.is_empty()
        }

        #[inline]
        pub(crate) fn has_due_log(&self) -> bool {
            self.due_log.is_some()
        }

        pub(crate) fn for_range(&self, band_start: usize, band_end: usize) -> Vec<BatchHookEvent> {
            if self.events.is_empty() {
                return Vec::new();
            }
            self.events
                .iter()
                .copied()
                .filter(|e| e.band >= band_start && e.band < band_end)
                .collect()
        }

        #[inline]
        pub(crate) fn record_executed_write(&self) {
            if let Some(ref c) = self.executed_writes {
                c.fetch_add(1, Ordering::Relaxed);
            }
        }

        #[inline]
        pub(crate) fn record_due(&self, stage: u32, band: usize, index: usize, due: bool) {
            if let Some(ref log) = self.due_log {
                log.lock().unwrap().push(BatchDueRecord {
                    public_call_id: self.call_id,
                    stage,
                    band,
                    index,
                    due,
                });
            }
        }
    }

    /// Install a process-global schedule (used by external single-case verification drivers
    /// whose `pool.install(...)` runs on a worker thread).
    pub fn install_schedule(mut events: Vec<BatchHookEvent>) {
        events.sort_by_key(|e| (e.public_call_id, e.stage, e.sample));
        let non_empty = !events.is_empty();
        let mut st = GLOBAL.lock().unwrap();
        st.schedule = events;
        st.next_call_id = 0;
        st.active_call_id = 0;
        st.executed_writes = Some(Arc::new(AtomicUsize::new(0)));
        HAS_GLOBAL.store(non_empty, Ordering::SeqCst);
    }

    pub fn clear_schedule() {
        HAS_GLOBAL.store(false, Ordering::SeqCst);
        GLOBAL_DUE_ON.store(false, Ordering::SeqCst);
        let mut st = GLOBAL.lock().unwrap();
        st.schedule.clear();
        st.next_call_id = 0;
        st.active_call_id = 0;
        st.executed_writes = None;
        st.due_log = None;
    }

    pub fn executed_writes() -> usize {
        if LOCAL_ACTIVE.with(|a| *a.borrow()) {
            return LOCAL.with(|s| {
                s.borrow()
                    .executed_writes
                    .as_ref()
                    .map_or(0, |c| c.load(Ordering::SeqCst))
            });
        }
        GLOBAL
            .lock()
            .unwrap()
            .executed_writes
            .as_ref()
            .map_or(0, |c| c.load(Ordering::SeqCst))
    }

    pub fn begin_due_log() {
        if LOCAL_ACTIVE.with(|a| *a.borrow()) {
            LOCAL.with(|s| {
                s.borrow_mut().due_log = Some(Arc::new(Mutex::new(Vec::new())));
            });
            return;
        }
        GLOBAL.lock().unwrap().due_log = Some(Arc::new(Mutex::new(Vec::new())));
        GLOBAL_DUE_ON.store(true, Ordering::SeqCst);
    }

    pub fn take_due_log() -> Vec<BatchDueRecord> {
        if LOCAL_ACTIVE.with(|a| *a.borrow()) {
            return LOCAL.with(|s| {
                s.borrow_mut()
                    .due_log
                    .take()
                    .map_or_else(Vec::new, |l| std::mem::take(&mut *l.lock().unwrap()))
            });
        }
        GLOBAL_DUE_ON.store(false, Ordering::SeqCst);
        GLOBAL
            .lock()
            .unwrap()
            .due_log
            .take()
            .map_or_else(Vec::new, |l| std::mem::take(&mut *l.lock().unwrap()))
    }

    /// Thread-isolated schedule install for parallel `cargo test` unit tests (no interference
    /// with concurrent tests). Call inside `pool.install(...)` on the caller thread.
    pub fn install_thread_schedule(mut events: Vec<BatchHookEvent>) {
        events.sort_by_key(|e| (e.public_call_id, e.stage, e.sample));
        LOCAL_ACTIVE.with(|a| *a.borrow_mut() = true);
        LOCAL.with(|s| {
            let mut st = s.borrow_mut();
            st.schedule = events;
            st.next_call_id = 0;
            st.active_call_id = 0;
            st.executed_writes = Some(Arc::new(AtomicUsize::new(0)));
        });
    }

    pub fn clear_thread_schedule() {
        LOCAL_ACTIVE.with(|a| *a.borrow_mut() = false);
        LOCAL.with(|s| {
            let mut st = s.borrow_mut();
            st.schedule.clear();
            st.next_call_id = 0;
            st.active_call_id = 0;
            st.executed_writes = None;
            st.due_log = None;
        });
    }

    pub(crate) fn begin_public_call() {
        if LOCAL_ACTIVE.with(|a| *a.borrow()) {
            LOCAL.with(|s| {
                let mut st = s.borrow_mut();
                st.active_call_id = st.next_call_id;
                st.next_call_id += 1;
            });
            return;
        }
        if !HAS_GLOBAL.load(Ordering::Relaxed) && !GLOBAL_DUE_ON.load(Ordering::Relaxed) {
            return;
        }
        let mut st = GLOBAL.lock().unwrap();
        st.active_call_id = st.next_call_id;
        st.next_call_id += 1;
    }

    fn build_stage_ctx(
        cid: u64,
        schedule: &[BatchHookEvent],
        executed_writes: Option<Arc<AtomicUsize>>,
        due_log: Option<Arc<Mutex<Vec<BatchDueRecord>>>>,
        stage: u32,
        limit_n: usize,
    ) -> StageHookContext {
        let mut events = Vec::new();
        for &ev in schedule {
            if ev.public_call_id != cid {
                continue;
            }
            assert!(
                matches!(ev.stage, 1 | 3 | 5),
                "unsupported/NA batch hook stage={} at call_id={cid} (valid batch stages: 1=TOL, 3=NL, 5=TW)",
                ev.stage
            );
            if ev.stage != stage {
                continue;
            }
            assert!(
                ev.sample < limit_n,
                "invalid batch hook coordinate: call_id={cid} stage={stage} s={} >= N={limit_n} (must satisfy 0 <= s < N)",
                ev.sample
            );
            match stage {
                1 => {
                    assert!(ev.band < 28, "batch TOL band out of range: {}", ev.band);
                    assert!(ev.field < 9, "batch TOL field out of range: {}", ev.field);
                }
                3 => {
                    assert!(ev.band < 21, "batch NL band out of range: {}", ev.band);
                    assert!(ev.field < 2, "batch NL field out of range: {}", ev.field);
                }
                5 => {
                    assert!(ev.band == 0, "batch TW band must be 0, got {}", ev.band);
                    assert!(ev.field < 3, "batch TW field out of range: {}", ev.field);
                }
                _ => unreachable!(),
            }
            events.push(ev);
        }
        StageHookContext {
            call_id: cid,
            events,
            executed_writes,
            due_log,
        }
    }

    pub(crate) fn snapshot_stage(stage: u32, limit_n: usize) -> StageHookContext {
        if LOCAL_ACTIVE.with(|a| *a.borrow()) {
            let (cid, schedule, executed_writes, due_log) = LOCAL.with(|s| {
                let st = s.borrow();
                (
                    st.active_call_id,
                    st.schedule.clone(),
                    st.executed_writes.clone(),
                    st.due_log.clone(),
                )
            });
            return build_stage_ctx(cid, &schedule, executed_writes, due_log, stage, limit_n);
        }
        if !HAS_GLOBAL.load(Ordering::Relaxed) && !GLOBAL_DUE_ON.load(Ordering::Relaxed) {
            return StageHookContext::empty();
        }
        let (cid, schedule, executed_writes, due_log) = {
            let st = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
            (
                st.active_call_id,
                st.schedule.clone(),
                st.executed_writes.clone(),
                st.due_log.clone(),
            )
        };
        build_stage_ctx(cid, &schedule, executed_writes, due_log, stage, limit_n)
    }
}

#[cfg(test)]
pub(super) mod test_counters {
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
}

pub(super) fn validate_core(values: &[f64], n: usize) -> Result<(), SoneflowError> {
    for t in 0..n {
        let row = std::array::from_fn(|band| values[band * n + t]);
        batch_guard::core(&row, t)?;
    }
    Ok(())
}

#[allow(dead_code)]
pub(super) fn validate_slopes(total: &[f64], specific: &[f64]) -> Result<(), SoneflowError> {
    let total_fault = total.iter().enumerate().find_map(|(t, &value)| {
        batch_guard::bounded(
            value,
            crate::numeric_guard::TW_INPUT_MAX,
            Stage::Slope,
            t,
            None,
        )
        .err()
        .map(|e| (t, usize::MAX, e))
    });
    let specific_fault = specific.iter().enumerate().find_map(|(i, &value)| {
        let t = (i / 240) * super::grid::OUT_DECIM;
        batch_guard::bounded(
            value,
            crate::numeric_guard::CORE_MAX,
            Stage::Slope,
            t,
            Some(i % 240),
        )
        .err()
        .map(|e| (t, i % 240, e))
    });
    [total_fault, specific_fault]
        .into_iter()
        .flatten()
        .min_by_key(|e| (e.0, e.1))
        .map_or(Ok(()), |(_, _, e)| Err(e))
}

/// Order 6 (Design r3 §3, §8): validate ONLY the outputs produced by unqualified slopes
/// tasks in exact R `(frame, band)` order (`total` has `band = usize::MAX`).
/// Qualified tasks' outputs are guaranteed in `[0, T)` and `[0, M]` by D0 Addendum §B.
pub(super) fn validate_slopes_chunks(
    total: &[f64],
    specific: &[f64],
    unqualified_chunks: &mut [usize],
) -> Result<(), SoneflowError> {
    if unqualified_chunks.is_empty() {
        return Ok(());
    }
    #[cfg(test)]
    test_counters::record_slopes_unqual_validated(unqualified_chunks.len());
    unqualified_chunks.sort_unstable();
    let total_fault = unqualified_chunks.iter().find_map(|&out_idx| {
        let t0 = out_idx * super::grid::OUT_DECIM;
        let t_end = (t0 + super::grid::OUT_DECIM).min(total.len());
        (t0..t_end).find_map(|t| {
            batch_guard::bounded(
                total[t],
                crate::numeric_guard::TW_INPUT_MAX,
                Stage::Slope,
                t,
                None,
            )
            .err()
            .map(|e| (t, usize::MAX, e))
        })
    });
    let specific_fault = unqualified_chunks.iter().find_map(|&out_idx| {
        let t0 = out_idx * super::grid::OUT_DECIM;
        let base = out_idx * 240;
        (0..240usize).find_map(|b| {
            batch_guard::bounded(
                specific[base + b],
                crate::numeric_guard::CORE_MAX,
                Stage::Slope,
                t0,
                Some(b),
            )
            .err()
            .map(|e| (t0, b, e))
        })
    });
    [total_fault, specific_fault]
        .into_iter()
        .flatten()
        .min_by_key(|e| (e.0, e.1))
        .map_or(Ok(()), |(_, _, e)| Err(e))
}

fn dispatch<F>(out: &mut [f64], chunk: usize, mode: ParMode, f: F) -> Result<(), Fault>
where
    F: Fn(usize, &mut [f64]) -> Result<(), Fault> + Sync,
{
    if out.is_empty() {
        return Ok(());
    }
    let results: Vec<_> = match mode {
        ParMode::Rayon => out
            .par_chunks_mut(chunk)
            .enumerate()
            .map(|(i, part)| f(i, part))
            .collect(),
        ParMode::Sequential => out
            .chunks_mut(chunk)
            .enumerate()
            .map(|(i, part)| f(i, part))
            .collect(),
    };
    results
        .into_iter()
        .filter_map(Result::err)
        .min_by_key(|e| (e.0, e.1))
        .map_or(Ok(()), Err)
}

#[inline(never)]
fn levels_scalar_checked(signal: &[f64], band: usize, row: &mut [f64]) -> Result<(), Fault> {
    let mut state = TolBandState::new(band);
    for (i, &sample) in signal.iter().enumerate() {
        let value = state.advance_checked(sample).map_err(|r| (i, band, r))?;
        if i % DEC_FACTOR == 0 {
            let level = intensity_to_db(value);
            if !level.is_finite() {
                return Err((i, band, DomainFault::NonFinite));
            }
            row[i / DEC_FACTOR] = level;
        }
    }
    Ok(())
}

#[cfg(target_arch = "x86_64")]
#[cold]
#[inline(never)]
fn levels_scalar_cold_replay(
    entry_mxcsr: u32,
    signal: &[f64],
    band: usize,
    row: &mut [f64],
) -> Result<(), Fault> {
    #[cfg(test)]
    test_counters::record_scalar_cold_replay();
    crate::numeric_guard::restore_mxcsr(entry_mxcsr);
    levels_scalar_checked(signal, band, row)
}

#[cfg(target_arch = "x86_64")]
#[inline(never)]
fn levels_scalar_fast(
    entry_mxcsr: u32,
    signal: &[f64],
    band: usize,
    row: &mut [f64],
) -> Result<(), Fault> {
    #[cfg(test)]
    test_counters::record_scalar_fast_block();
    let mut state = TolBandState::new(band);
    let mut nonfinite_acc = 0u64;
    for (out_slot, chunk) in row.iter_mut().zip(signal.chunks(DEC_FACTOR)) {
        if let Some((&first, tail)) = chunk.split_first() {
            let v0 = state.advance(first);
            let level = intensity_to_db(v0);
            nonfinite_acc |= crate::numeric_guard::nonfinite_mask_bit(level);
            *out_slot = level;
            for &sample in tail {
                let _ = state.advance(sample);
            }
        }
    }
    if (nonfinite_acc as i64) < 0 || state.validate_numeric().is_err() {
        return levels_scalar_cold_replay(entry_mxcsr, signal, band, row);
    }
    Ok(())
}

#[cfg(test)] // c9_batch_hook
struct BatchTolScalarSnapshot {
    state: TolBandState,
    start_sample: usize,
    phase: usize,
    emission_idx: usize,
}

#[cfg(test)] // c9_batch_hook
#[inline(never)]
fn levels_scalar_checked_with_hooks(
    signal: &[f64],
    band: usize,
    row: &mut [f64],
    hooks: &[batch_hooks::BatchHookEvent],
    ctx: &batch_hooks::StageHookContext,
) -> Result<(), Fault> {
    let mut state = TolBandState::new(band);
    let mut h_idx = 0usize;
    for (i, &sample) in signal.iter().enumerate() {
        while h_idx < hooks.len() && hooks[h_idx].sample == i {
            state.inject_raw_bits(hooks[h_idx].field, hooks[h_idx].raw_u64);
            ctx.record_executed_write();
            h_idx += 1;
        }
        let value = state.advance_checked(sample).map_err(|r| (i, band, r))?;
        if i % DEC_FACTOR == 0 {
            let level = intensity_to_db(value);
            if !level.is_finite() {
                return Err((i, band, DomainFault::NonFinite));
            }
            row[i / DEC_FACTOR] = level;
        }
    }
    Ok(())
}

#[cfg(test)] // c9_batch_hook
#[inline(never)]
fn levels_scalar_checked_from_snapshot(
    mut state: TolBandState,
    start_sample: usize,
    mut phase: usize,
    mut emission_idx: usize,
    signal_slice: &[f64],
    band: usize,
    row: &mut [f64],
) -> Result<TolBandState, Fault> {
    for (offset, &sample) in signal_slice.iter().enumerate() {
        let i = start_sample + offset;
        let value = state.advance_checked(sample).map_err(|r| (i, band, r))?;
        if phase == 0 {
            let level = intensity_to_db(value);
            if !level.is_finite() {
                return Err((i, band, DomainFault::NonFinite));
            }
            row[emission_idx] = level;
            emission_idx += 1;
        }
        phase += 1;
        if phase == DEC_FACTOR {
            phase = 0;
        }
    }
    Ok(state)
}

#[cfg(test)] // c9_batch_hook
#[cfg(target_arch = "x86_64")]
#[cold]
#[inline(never)]
fn levels_scalar_cold_replay_from_snapshot(
    entry_mxcsr: u32,
    snap: BatchTolScalarSnapshot,
    signal_slice: &[f64],
    band: usize,
    row: &mut [f64],
) -> Result<TolBandState, Fault> {
    #[cfg(test)]
    test_counters::record_scalar_cold_replay();
    crate::numeric_guard::restore_mxcsr(entry_mxcsr);
    levels_scalar_checked_from_snapshot(
        snap.state,
        snap.start_sample,
        snap.phase,
        snap.emission_idx,
        signal_slice,
        band,
        row,
    )
}

#[cfg(test)] // c9_batch_hook
#[cfg(target_arch = "x86_64")]
#[cold]
#[inline(never)]
fn levels_scalar_cold_replay_reconstruct(
    entry_mxcsr: u32,
    signal_slice: &[f64],
    band: usize,
    row: &mut [f64],
) -> Result<TolBandState, Fault> {
    #[cfg(test)]
    test_counters::record_scalar_cold_replay();
    crate::numeric_guard::restore_mxcsr(entry_mxcsr);
    let fresh = TolBandState::new(band);
    levels_scalar_checked_from_snapshot(fresh, 0, 0, 0, signal_slice, band, row)
}

#[cfg(test)] // c9_batch_hook
#[cfg(target_arch = "x86_64")]
#[inline(never)]
fn levels_scalar_fast_with_hooks(
    entry_mxcsr: u32,
    signal: &[f64],
    band: usize,
    row: &mut [f64],
    hooks: &[batch_hooks::BatchHookEvent],
    ctx: &batch_hooks::StageHookContext,
) -> Result<(), Fault> {
    let mut state = TolBandState::new(band);
    let mut pos = 0usize;
    let mut phase = 0usize;
    let mut emission_idx = 0usize;
    let mut h_idx = 0usize;
    let mut due_tol = false;
    let mut block_entry_mxcsr = entry_mxcsr;

    while pos < signal.len() {
        let mut hook_snap: Option<BatchTolScalarSnapshot> = None;
        if h_idx < hooks.len() && hooks[h_idx].sample == pos {
            while h_idx < hooks.len() && hooks[h_idx].sample == pos {
                state.inject_raw_bits(hooks[h_idx].field, hooks[h_idx].raw_u64);
                ctx.record_executed_write();
                h_idx += 1;
            }
            // Expiry rule c: hook makes TOL due.
            due_tol = true;
            ctx.record_due(1, band, pos, due_tol);
            // Post-hook snapshot preserves all 9 numeric values, all 18 construction coeff bits,
            // phase, and original sample / emission indices (Design r3 §4, §6).
            hook_snap = Some(BatchTolScalarSnapshot {
                state: state.copy_exact(),
                start_sample: pos,
                phase,
                emission_idx,
            });
            // Design r3 §8: capture block entry MXCSR once per hook-delimited block.
            block_entry_mxcsr = crate::numeric_guard::read_unit_mxcsr().unwrap_or(entry_mxcsr);
        }

        let end = hooks.get(h_idx).map_or(signal.len(), |h| h.sample);
        assert!(end > pos && end <= signal.len());
        let block = &signal[pos..end];

        if due_tol {
            // c9_scalar_hook_due_check
            if state.validate_numeric().is_err() {
                assert!(
                    hooks.get(h_idx).is_none_or(|h| h.sample >= end),
                    "STOP CONDITION: batch scalar TOL hook fell inside entry-due replay interval [{}..{})",
                    pos,
                    end
                );
                let snap = hook_snap.expect("due_tol entry fault must have snapshot");
                state = levels_scalar_checked_from_snapshot(
                    snap.state,
                    snap.start_sample,
                    snap.phase,
                    snap.emission_idx,
                    block,
                    band,
                    row,
                )?;
                pos = end;
                phase = pos % DEC_FACTOR;
                emission_idx = pos.div_ceil(DEC_FACTOR);
                due_tol = true;
                continue;
            }
            due_tol = false;
        }

        #[cfg(test)]
        test_counters::record_scalar_fast_block();
        let mut nonfinite_acc = 0u64;
        let mut work_phase = phase;
        let mut work_emission = emission_idx;
        for &sample in block {
            let v = state.advance(sample);
            if work_phase == 0 {
                let level = intensity_to_db(v);
                nonfinite_acc |= crate::numeric_guard::nonfinite_mask_bit(level);
                row[work_emission] = level;
                work_emission += 1;
            }
            work_phase += 1;
            if work_phase == DEC_FACTOR {
                work_phase = 0;
            }
        }

        // c9_scalar_hook_d1_check
        if (nonfinite_acc as i64) < 0 || state.validate_numeric().is_err() {
            assert!(
                hooks.get(h_idx).is_none_or(|h| h.sample >= end),
                "STOP CONDITION: batch scalar TOL hook fell inside cold replay interval [{}..{})",
                pos,
                end
            );
            state = if let Some(snap) = hook_snap {
                levels_scalar_cold_replay_from_snapshot(block_entry_mxcsr, snap, block, band, row)?
            } else {
                assert_eq!(
                    pos, 0,
                    "unhooked batch scalar TOL block must start at sample 0"
                );
                levels_scalar_cold_replay_reconstruct(block_entry_mxcsr, block, band, row)?
            };
            pos = end;
            phase = pos % DEC_FACTOR;
            emission_idx = pos.div_ceil(DEC_FACTOR);
            due_tol = true;
        } else {
            pos = end;
            phase = work_phase;
            emission_idx = work_emission;
        }
    }
    Ok(())
}

fn levels_with_backend(
    signal: &[f64],
    mode: ParMode,
    use_avx2: bool,
) -> Result<(Vec<f64>, usize), SoneflowError> {
    #[cfg(test)] // c9_batch_hook
    let hook_ctx = batch_hooks::snapshot_stage(1, signal.len());
    let n = signal.len().div_ceil(DEC_FACTOR);
    let mut out = vec![0.0; 28 * n];
    #[cfg(not(target_arch = "x86_64"))]
    let _ = use_avx2;
    #[cfg(target_arch = "x86_64")]
    if use_avx2 {
        // SAFETY: caller ensures AVX2 and FMA are available.
        dispatch(&mut out, 4 * n, mode, |g, part| unsafe {
            avx2::levels(
                signal,
                g,
                part,
                n,
                #[cfg(test)] // c9_batch_hook
                &hook_ctx,
            )
        })
        .map_err(|(i, b, r)| batch_guard::fault(Stage::Filter, r, i, Some(b)))?;
        return Ok((out, n));
    }
    dispatch(&mut out, n, mode, |band, row| {
        let entry_mxcsr = crate::numeric_guard::read_unit_mxcsr();
        #[cfg(test)] // c9_batch_hook
        let band_hooks = hook_ctx.for_range(band, band + 1);
        if let Some(entry_mxcsr) =
            entry_mxcsr.filter(|&m| crate::numeric_guard::is_mxcsr_qualified(m))
        {
            #[cfg(test)]
            test_counters::record_scalar(true);
            #[cfg(target_arch = "x86_64")]
            {
                #[cfg(test)] // c9_batch_hook
                if !band_hooks.is_empty() {
                    return levels_scalar_fast_with_hooks(
                        entry_mxcsr,
                        signal,
                        band,
                        row,
                        &band_hooks,
                        &hook_ctx,
                    );
                }
                levels_scalar_fast(entry_mxcsr, signal, band, row)
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                let _ = entry_mxcsr;
                #[cfg(test)] // c9_batch_hook
                if !band_hooks.is_empty() {
                    return levels_scalar_checked_with_hooks(
                        signal,
                        band,
                        row,
                        &band_hooks,
                        &hook_ctx,
                    );
                }
                levels_scalar_checked(signal, band, row)
            }
        } else {
            #[cfg(test)]
            test_counters::record_scalar(false);
            #[cfg(test)] // c9_batch_hook
            if !band_hooks.is_empty() {
                return levels_scalar_checked_with_hooks(signal, band, row, &band_hooks, &hook_ctx);
            }
            levels_scalar_checked(signal, band, row)
        }
    })
    .map_err(|(i, b, r)| batch_guard::fault(Stage::Filter, r, i, Some(b)))?;
    Ok((out, n))
}

pub(super) fn levels(signal: &[f64], mode: ParMode) -> Result<(Vec<f64>, usize), SoneflowError> {
    levels_with_backend(signal, mode, crate::simd::use_avx2())
}

#[cold]
#[inline(never)]
fn nl_band_entry_fallback(
    state: &mut NlBandState,
    row_t: f64,
    next: f64,
    b: &[f64; 6],
    t: usize,
    band: usize,
) -> Result<f64, Fault> {
    #[cfg(test)]
    test_counters::record_nl_scalar_entry_fallback();
    // D0 entry failure: no speculation occurred, no MXCSR restore, delegate frame `t` to R
    // without modifying/zeroing state or seed so R determines exact coordinate, reason & state.
    state
        .advance_frame_checked(row_t, next, b)
        .map_err(|r| (t, band, r))
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn nl_band_checked_loop(
    state: &mut NlBandState,
    row: &[f64],
    out: &mut [f64],
    b: &[f64; 6],
    band: usize,
    #[cfg(test)] // c9_batch_hook
    band_hooks: &[batch_hooks::BatchHookEvent],
    #[cfg(test)] // c9_batch_hook
    mut due_nl: bool,
    #[cfg(test)] // c9_batch_hook
    hook_ctx: &batch_hooks::StageHookContext,
) -> Result<(), Fault> {
    #[cfg(test)] // c9_batch_hook
    let mut h_idx = 0usize;
    for t in 0..row.len() {
        #[cfg(test)] // c9_batch_hook
        {
            while h_idx < band_hooks.len() && band_hooks[h_idx].sample == t {
                state.inject_raw_bits(band_hooks[h_idx].field, band_hooks[h_idx].raw_u64);
                hook_ctx.record_executed_write();
                due_nl = true;
                h_idx += 1;
            }
            hook_ctx.record_due(3, band, t, due_nl);
        }
        out[t] = state
            .advance_frame_checked(row[t], row.get(t + 1).copied().unwrap_or(0.0), b)
            .map_err(|r| (t, band, r))?;
        #[cfg(test)] // c9_batch_hook
        {
            due_nl = false;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn nl_band(
    row: &[f64],
    out: &mut [f64],
    b: &[f64; 6],
    init: NlInit,
    band: usize,
    caller_qualified: bool,
    core_validated: bool,
    #[cfg(test)] // c9_batch_hook
    hook_ctx: &batch_hooks::StageHookContext,
) -> Result<(), Fault> {
    // Design r3 §8: read worker MXCSR once before seed & frame computation.
    let worker_qualified = crate::numeric_guard::read_unit_mxcsr()
        .is_some_and(crate::numeric_guard::is_mxcsr_qualified);
    let qualified = caller_qualified && worker_qualified;
    #[cfg(test)]
    test_counters::record_nl_scalar(qualified);

    let mut state = NlBandState::seed(init, row[row.len() - 1]);
    // Expiry rule d: NL state seeded from input (`MosqitoTail`) is due before first use.
    let mut due_nl = matches!(init, NlInit::MosqitoTail);
    #[cfg(test)] // c9_batch_hook
    let band_hooks = hook_ctx.for_range(band, band + 1);

    if !qualified {
        return nl_band_checked_loop(
            &mut state,
            row,
            out,
            b,
            band,
            #[cfg(test)] // c9_batch_hook
            &band_hooks,
            #[cfg(test)] // c9_batch_hook
            due_nl,
            #[cfg(test)] // c9_batch_hook
            hook_ctx,
        );
    }

    #[cfg(test)] // c9_batch_hook
    let need_hook_loop = !band_hooks.is_empty() || hook_ctx.has_due_log();
    #[cfg(not(test))] // c9_batch_hook_else
    let need_hook_loop = false;

    if core_validated && !need_hook_loop {
        let start = if due_nl && !state.is_entry_valid() {
            out[0] = nl_band_entry_fallback(
                &mut state,
                row[0],
                row.get(1).copied().unwrap_or(0.0),
                b,
                0,
                band,
            )?;
            1
        } else {
            0
        };
        for t in start..row.len() {
            #[cfg(test)]
            test_counters::record_nl_scalar_fast_frame();
            let next = if t + 1 < row.len() { row[t + 1] } else { 0.0 };
            out[t] = state.advance_frame(row[t], next, b);
        }
        return Ok(());
    }

    #[cfg(test)] // c9_batch_hook
    let mut h_idx = 0usize;
    for t in 0..row.len() {
        #[cfg(test)] // c9_batch_hook
        {
            while h_idx < band_hooks.len() && band_hooks[h_idx].sample == t {
                state.inject_raw_bits(band_hooks[h_idx].field, band_hooks[h_idx].raw_u64);
                hook_ctx.record_executed_write();
                // Expiry rule c: hook makes NL due.
                due_nl = true;
                h_idx += 1;
            }
            hook_ctx.record_due(3, band, t, due_nl);
        }
        let row_t = row[t];
        let next = if t + 1 < row.len() { row[t + 1] } else { 0.0 };
        let state_ok = !due_nl || state.is_entry_valid();
        let input_ok = core_validated
            || (crate::numeric_guard::is_nonnegative_bounded(
                row_t,
                crate::numeric_guard::CORE_MAX,
            ) && crate::numeric_guard::is_nonnegative_bounded(
                next,
                crate::numeric_guard::CORE_MAX,
            ));
        if !(state_ok && input_ok) {
            out[t] = nl_band_entry_fallback(&mut state, row_t, next, b, t, band)?;
        } else {
            due_nl = false;
            #[cfg(test)]
            test_counters::record_nl_scalar_fast_frame();
            out[t] = state.advance_frame(row_t, next, b);
        }
    }
    Ok(())
}

fn nonlinear_with_backend(
    core: &[f64],
    n: usize,
    mode: ParMode,
    init: NlInit,
    core_validated: bool,
    use_avx2: bool,
) -> Result<Vec<f64>, SoneflowError> {
    #[cfg(test)] // c9_batch_hook
    let hook_ctx = batch_hooks::snapshot_stage(3, n);
    let mut out = vec![0.0; core.len()];
    if n == 0 {
        return Ok(out);
    }
    // Design r3 §8: caller reads unit qualification before constructing `nl_coeffs()`.
    let caller_qualified = crate::numeric_guard::read_unit_mxcsr()
        .is_some_and(crate::numeric_guard::is_mxcsr_qualified);
    let b = nl_coeffs();
    #[cfg(not(target_arch = "x86_64"))]
    let _ = use_avx2;
    #[cfg(target_arch = "x86_64")]
    if use_avx2 {
        dispatch(&mut out, 4 * n, mode, |g, part| {
            let band = 4 * g;
            if part.len() == n {
                return nl_band(
                    &core[band * n..(band + 1) * n],
                    part,
                    &b,
                    init,
                    band,
                    caller_qualified,
                    core_validated,
                    #[cfg(test)] // c9_batch_hook
                    &hook_ctx,
                );
            }
            // SAFETY: runtime dispatch checks AVX2/FMA; remaining groups have four bands.
            unsafe {
                avx2::nonlinear(
                    core,
                    part,
                    n,
                    band,
                    b,
                    init,
                    caller_qualified,
                    core_validated,
                    #[cfg(test)] // c9_batch_hook
                    &hook_ctx,
                )
            }
        })
        .map_err(|(i, b, r)| batch_guard::fault(Stage::Nonlinear, r, i, Some(b)))?;
        return Ok(out);
    }
    dispatch(&mut out, n, mode, |band, part| {
        nl_band(
            &core[band * n..(band + 1) * n],
            part,
            &b,
            init,
            band,
            caller_qualified,
            core_validated,
            #[cfg(test)] // c9_batch_hook
            &hook_ctx,
        )
    })
    .map_err(|(i, b, r)| batch_guard::fault(Stage::Nonlinear, r, i, Some(b)))?;
    Ok(out)
}

#[allow(dead_code)]
pub(super) fn nonlinear(
    core: &[f64],
    n: usize,
    mode: ParMode,
    init: NlInit,
) -> Result<Vec<f64>, SoneflowError> {
    nonlinear_with_backend(core, n, mode, init, false, crate::simd::use_avx2())
}

/// Called by `ZwtvProcessor::process_with` immediately after `validate_core(&self.core, n_time)?`
/// on immutable `&self.core`, enabling §3A / §7 per-frame `row`/`next` deduplication.
pub(super) fn nonlinear_validated_core(
    core: &[f64],
    n: usize,
    mode: ParMode,
    init: NlInit,
) -> Result<Vec<f64>, SoneflowError> {
    nonlinear_with_backend(core, n, mode, init, true, crate::simd::use_avx2())
}

#[cold]
#[inline(never)]
fn tw_entry_fallback(state: &mut TwState, value: f64, i: usize) -> Result<f64, SoneflowError> {
    #[cfg(test)]
    test_counters::record_tw_entry_fallback();
    // D0 entry failure: no speculation, no MXCSR restore, delegate frame `i` to R.
    state
        .advance_checked(value)
        .map_err(|r| batch_guard::fault(Stage::TemporalWeighting, r, i, None))
}

#[inline(never)]
fn temporal_checked_loop(
    state: &mut TwState,
    values: &[f64],
    #[cfg(test)] // c9_batch_hook
    tw_hooks: &[batch_hooks::BatchHookEvent],
    #[cfg(test)] // c9_batch_hook
    hook_ctx: &batch_hooks::StageHookContext,
) -> Result<Vec<f64>, SoneflowError> {
    #[cfg(test)] // c9_batch_hook
    let (mut h_idx, mut due_tw) = (0usize, false);
    let mut out = Vec::with_capacity(values.len());
    for (i, &value) in values.iter().enumerate() {
        #[cfg(test)] // c9_batch_hook
        {
            while h_idx < tw_hooks.len() && tw_hooks[h_idx].sample == i {
                state.inject_raw_bits(tw_hooks[h_idx].field, tw_hooks[h_idx].raw_u64);
                hook_ctx.record_executed_write();
                due_tw = true;
                h_idx += 1;
            }
            hook_ctx.record_due(5, 0, i, due_tw);
        }
        out.push(
            state
                .advance_checked(value)
                .map_err(|r| batch_guard::fault(Stage::TemporalWeighting, r, i, None))?,
        );
        #[cfg(test)] // c9_batch_hook
        {
            due_tw = false;
        }
    }
    Ok(out)
}

pub(super) fn temporal(values: &[f64]) -> Result<Vec<f64>, SoneflowError> {
    #[cfg(test)] // c9_batch_hook
    let hook_ctx = batch_hooks::snapshot_stage(5, values.len());
    // Design r3 §8: caller reads MXCSR once before `TwState::new()` coefficient construction.
    let qualified = crate::numeric_guard::read_unit_mxcsr()
        .is_some_and(crate::numeric_guard::is_mxcsr_qualified);
    #[cfg(test)]
    test_counters::record_tw(qualified);

    let mut state = TwState::new();
    #[cfg(test)] // c9_batch_hook
    let tw_hooks = hook_ctx.for_range(0, 1);

    if !qualified {
        return temporal_checked_loop(
            &mut state,
            values,
            #[cfg(test)] // c9_batch_hook
            &tw_hooks,
            #[cfg(test)] // c9_batch_hook
            &hook_ctx,
        );
    }

    #[cfg(test)] // c9_batch_hook
    let (mut h_idx, mut due_tw) = (0usize, false);
    #[cfg(not(test))] // c9_batch_hook_else
    let due_tw = false;

    let mut out = Vec::with_capacity(values.len());
    for (i, &value) in values.iter().enumerate() {
        #[cfg(test)] // c9_batch_hook
        {
            while h_idx < tw_hooks.len() && tw_hooks[h_idx].sample == i {
                state.inject_raw_bits(tw_hooks[h_idx].field, tw_hooks[h_idx].raw_u64);
                hook_ctx.record_executed_write();
                // Expiry rule c: hook makes TW due.
                due_tw = true;
                h_idx += 1;
            }
            hook_ctx.record_due(5, 0, i, due_tw);
        }
        let state_ok = !due_tw || state.is_entry_valid();
        let input_ok =
            crate::numeric_guard::is_nonnegative_bounded(value, crate::numeric_guard::TW_INPUT_MAX);
        if !(state_ok && input_ok) {
            out.push(tw_entry_fallback(&mut state, value, i)?);
        } else {
            #[cfg(test)] // c9_batch_hook
            {
                due_tw = false;
            }
            #[cfg(test)]
            test_counters::record_tw_fast_frame();
            out.push(state.advance(value));
        }
    }
    Ok(out)
}

#[cfg(test)]
#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "unsafe blocks from 0.1.0 without SAFETY comments; remove once documented"
)]
mod tests {
    use super::*;
    fn bits(x: &[f64]) -> Vec<u64> {
        x.iter().map(|v| v.to_bits()).collect()
    }

    #[test]
    fn c4_temporal_lengths_bits_and_first_error_index() {
        for len in [0, 1, 2, 17, 100] {
            let values: Vec<_> = (0..len).map(|i| (i % 7) as f64 * 0.1).collect();
            assert_eq!(
                bits(&temporal(&values).unwrap()),
                bits(&crate::zwtv::temporal_weighting::temporal_weighting(
                    &values
                ))
            );
            for index in 0..len {
                let mut bad = values.clone();
                bad[index] = f64::NAN;
                assert!(
                    matches!(temporal(&bad),Err(SoneflowError::InternalNumericFault {stage:Stage::TemporalWeighting,index:i,band:None,..}) if i==index)
                );
            }
        }
    }

    #[test]
    fn slope_faults_use_internal_time_then_band() {
        let mut total = [0.0; 12];
        let mut specific = [0.0; 720];
        total[8] = f64::NAN;
        specific[240 + 7] = f64::NAN;
        assert!(matches!(
            validate_slopes(&total, &specific),
            Err(SoneflowError::InternalNumericFault {
                index: 4,
                band: Some(7),
                ..
            })
        ));
        total[4] = f64::INFINITY;
        assert!(matches!(
            validate_slopes(&total, &specific),
            Err(SoneflowError::InternalNumericFault {
                index: 4,
                band: Some(7),
                ..
            })
        ));
        total[2] = f64::INFINITY;
        assert!(matches!(
            validate_slopes(&total, &specific),
            Err(SoneflowError::InternalNumericFault {
                index: 2,
                band: None,
                ..
            })
        ));
    }

    #[test]
    fn checked_stages_preserve_normal_backend_bits() {
        let signal: Vec<_> = (0..4800)
            .map(|i| ((i % 71) as f64 - 35.0) * 0.0001)
            .collect();
        for mode in [ParMode::Sequential, ParMode::Rayon] {
            let expected =
                crate::zwtv::third_octave_levels::third_octave_levels_with_mode(&signal, mode);
            let actual = levels(&signal, mode).unwrap();
            assert_eq!(expected.1, actual.1);
            assert_eq!(bits(&expected.0), bits(&actual.0));
            let n = 120;
            let core: Vec<_> = (0..21 * n).map(|i| (i % 17) as f64 * 0.1).collect();
            for init in [NlInit::Zero, NlInit::MosqitoTail] {
                let expected = crate::zwtv::nonlinear_decay::nl_loudness_with(&core, n, mode, init);
                assert_eq!(
                    bits(&expected),
                    bits(&nonlinear(&core, n, mode, init).unwrap())
                );
            }
        }
        let total = [0.0, 0.1, 1.0, 0.0, 2.0, 0.5];
        assert_eq!(
            bits(&crate::zwtv::temporal_weighting::temporal_weighting(&total)),
            bits(&temporal(&total).unwrap())
        );
    }

    #[cfg(target_arch = "x86_64")]
    struct MxcsrGuard(u32);
    #[cfg(target_arch = "x86_64")]
    impl MxcsrGuard {
        fn capture() -> Self {
            let mut v = 0u32;
            unsafe {
                std::arch::asm!("stmxcsr [{p}]", p = in(reg) &mut v, options(nostack, preserves_flags));
            }
            Self(v)
        }
        fn set(v: u32) {
            unsafe {
                std::arch::asm!("ldmxcsr [{p}]", p = in(reg) &v, options(nostack));
            }
        }
    }
    #[cfg(target_arch = "x86_64")]
    impl Drop for MxcsrGuard {
        fn drop(&mut self) {
            Self::set(self.0);
        }
    }

    #[test]
    fn order1_mxcsr_qualification_predicate_exhaustive() {
        for rc in [0x0000u32, 0x2000, 0x4000, 0x6000] {
            for ftz_daz in [0x0000u32, 0x0040, 0x8000, 0x8040] {
                for masks in 0u32..64 {
                    for sticky in [0x00u32, 0x01, 0x20, 0x3f] {
                        let word = rc | ftz_daz | (masks << 7) | sticky;
                        let expected =
                            rc == 0 && masks == 0x3f && (ftz_daz == 0x0000 || ftz_daz == 0x8040);
                        assert_eq!(
                            crate::numeric_guard::is_mxcsr_qualified(word),
                            expected,
                            "word={word:#06x}"
                        );
                    }
                }
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn order1_batch_tol_dispatch_counters_and_precedence() {
        use crate::{FieldType, NlInit, ZwtvProcessor};
        use std::sync::Arc;

        let _restore = MxcsrGuard::capture();

        let sig: Vec<f64> = (0..4800)
            .map(|i| ((i as f64) * std::f64::consts::TAU * 1000.0 / 48000.0).sin() * 0.02)
            .collect();

        let has_avx2 =
            std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
        let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };

        for &use_avx2 in backends {
            let expected_units = if use_avx2 { 7 } else { 28 };

            // 1. Qualified workers: RN(0,0) and RN(1,1), with and without sticky IE=1
            for wcsr in [0x1f80u32, 0x1f81, 0x9fc0, 0x9fc1] {
                let sink = Arc::new(test_counters::Sink::default());
                let s_worker = Arc::clone(&sink);
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .start_handler(move |_| {
                        MxcsrGuard::set(wcsr);
                        test_counters::attach_thread(Some(Arc::clone(&s_worker)));
                    })
                    .build()
                    .unwrap();
                let res = pool.install(|| levels_with_backend(&sig, ParMode::Rayon, use_avx2));
                assert!(res.is_ok(), "qualified worker {wcsr:#06x} failed: {res:?}");
                let c = sink.snapshot();
                if use_avx2 {
                    assert_eq!(c.avx2_qualified, expected_units, "wcsr={wcsr:#06x}");
                    assert_eq!(c.avx2_unqualified, 0, "wcsr={wcsr:#06x}");
                } else {
                    assert_eq!(c.scalar_qualified, expected_units, "wcsr={wcsr:#06x}");
                    assert_eq!(c.scalar_unqualified, 0, "wcsr={wcsr:#06x}");
                }
            }

            // 2. Unqualified workers: RZ, RD, RU, FTZ=1/DAZ=0, FTZ=0/DAZ=1, OM unmasked (0x1b80)
            for wcsr in [0x7f80u32, 0x3f80, 0x5f80, 0x9f80, 0x1fc0, 0x1b80] {
                let sink = Arc::new(test_counters::Sink::default());
                let s_worker = Arc::clone(&sink);
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .start_handler(move |_| {
                        MxcsrGuard::set(wcsr);
                        test_counters::attach_thread(Some(Arc::clone(&s_worker)));
                    })
                    .build()
                    .unwrap();
                let res = pool.install(|| levels_with_backend(&sig, ParMode::Rayon, use_avx2));
                assert!(
                    res.is_ok(),
                    "unqualified worker {wcsr:#06x} failed: {res:?}"
                );
                let c = sink.snapshot();
                if use_avx2 {
                    assert_eq!(c.avx2_qualified, 0, "wcsr={wcsr:#06x}");
                    assert_eq!(c.avx2_unqualified, expected_units, "wcsr={wcsr:#06x}");
                } else {
                    assert_eq!(c.scalar_qualified, 0, "wcsr={wcsr:#06x}");
                    assert_eq!(c.scalar_unqualified, expected_units, "wcsr={wcsr:#06x}");
                }
            }

            // 3. Mixed worker pool (2 threads: thread 0 qualified 0x1f80, thread 1 unqualified 0x9f80).
            // O12-C1: bind each worker thread to its own Sink, guarantee both workers execute units,
            // and assert worker 0 runs fast path exclusively while worker 1 runs R's checked loop.
            {
                let sink_q = Arc::new(test_counters::Sink::default());
                let sink_u = Arc::new(test_counters::Sink::default());
                let sq = Arc::clone(&sink_q);
                let su = Arc::clone(&sink_u);
                let tid = std::sync::atomic::AtomicUsize::new(0);
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(2)
                    .start_handler(move |_| {
                        let id = tid.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        if id == 0 {
                            MxcsrGuard::set(0x1f80);
                            test_counters::attach_thread(Some(Arc::clone(&sq)));
                        } else {
                            MxcsrGuard::set(0x9f80);
                            test_counters::attach_thread(Some(Arc::clone(&su)));
                        }
                    })
                    .build()
                    .unwrap();

                let mut calls = 0usize;
                let (mut q_ran, mut u_ran) = (0usize, 0usize);
                for _ in 0..64 {
                    let res = pool.install(|| levels_with_backend(&sig, ParMode::Rayon, use_avx2));
                    assert!(res.is_ok(), "mixed worker pool failed: {res:?}");
                    calls += 1;
                    let cq = sink_q.snapshot();
                    let cu = sink_u.snapshot();
                    let (q_qual, q_unqual, q_fast, u_qual, u_unqual, u_fast) = if use_avx2 {
                        (
                            cq.avx2_qualified,
                            cq.avx2_unqualified,
                            cq.avx2_fast_blocks,
                            cu.avx2_qualified,
                            cu.avx2_unqualified,
                            cu.avx2_fast_blocks,
                        )
                    } else {
                        (
                            cq.scalar_qualified,
                            cq.scalar_unqualified,
                            cq.scalar_fast_blocks,
                            cu.scalar_qualified,
                            cu.scalar_unqualified,
                            cu.scalar_fast_blocks,
                        )
                    };
                    assert_eq!(
                        q_unqual, 0,
                        "qualified worker (0x1f80) must never fall back to R (avx2={use_avx2})"
                    );
                    assert_eq!(
                        q_fast, q_qual,
                        "qualified worker (0x1f80) must execute fast path for every unit (avx2={use_avx2})"
                    );
                    assert_eq!(
                        u_qual, 0,
                        "unqualified worker (0x9f80) must never take fast path (avx2={use_avx2})"
                    );
                    assert_eq!(
                        u_fast, 0,
                        "unqualified worker (0x9f80) must execute R checked loop exclusively (avx2={use_avx2})"
                    );
                    assert_eq!(
                        q_qual + u_unqual,
                        expected_units * calls,
                        "total dispatched units mismatch (avx2={use_avx2})"
                    );
                    q_ran = q_qual;
                    u_ran = u_unqual;
                    if q_ran > 0 && u_ran > 0 {
                        break;
                    }
                }
                assert!(
                    q_ran > 0 && u_ran > 0,
                    "both workers must execute units in mixed pool (avx2={use_avx2}, q_ran={q_ran}, u_ran={u_ran}, calls={calls})"
                );
            }
        }

        // 4. Existing R usage & caller environment rejections happen BEFORE worker dispatch (0 units)
        let sink = Arc::new(test_counters::Sink::default());
        let prev = test_counters::attach_thread(Some(Arc::clone(&sink)));
        MxcsrGuard::set(0x7f80); // caller non-RN
        let bad_rate =
            ZwtvProcessor::new().process_with(&sig, 44100.0, FieldType::Free, NlInit::Zero);
        assert!(matches!(
            bad_rate,
            Err(SoneflowError::UnsupportedSampleRate(_))
        ));
        let bad_short =
            ZwtvProcessor::new().process_with(&sig[..100], 48000.0, FieldType::Free, NlInit::Zero);
        assert!(matches!(
            bad_short,
            Err(SoneflowError::SignalTooShort { .. })
        ));
        let bad_env =
            ZwtvProcessor::new().process_with(&sig, 48000.0, FieldType::Free, NlInit::Zero);
        assert!(matches!(
            bad_env,
            Err(SoneflowError::InternalNumericFault {
                stage: Stage::Environment,
                ..
            })
        ));
        test_counters::attach_thread(prev);
        assert_eq!(sink.snapshot(), test_counters::TolDispatchCounts::default());
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn order2_batch_tol_fast_blocks_cold_replay_and_mxcsr_restore() {
        use std::sync::Arc;

        let _restore = MxcsrGuard::capture();
        let has_avx2 =
            std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
        let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };

        let sig_norm: Vec<f64> = (0..4800)
            .map(|i| ((i as f64) * std::f64::consts::TAU * 1000.0 / 48000.0).sin() * 0.02)
            .collect();
        let mut sig_start = vec![0.01f64; 4800];
        sig_start[0] = f64::MAX;
        let mut sig_mid = vec![0.01f64; 4800];
        sig_mid[2400] = f64::MAX;
        let mut sig_end = vec![0.01f64; 4800];
        sig_end[4799] = f64::MAX;
        let sig_level = vec![1e155_f64; 4800];

        let mut sig_minnorm_tail = vec![0.0f64; 4800];
        sig_minnorm_tail[2400] = 1e200;
        for x in sig_minnorm_tail.iter_mut().skip(2401) {
            *x = f64::MIN_POSITIVE;
        }

        for &use_avx2 in backends {
            let expected_units = if use_avx2 { 7 } else { 28 };

            // 1. Normal signal under qualified workers: fast_blocks == units, cold_replays == 0
            for wcsr in [0x1f80u32, 0x1f81, 0x9fc0, 0x9fc1] {
                let sink = Arc::new(test_counters::Sink::default());
                let sw = Arc::clone(&sink);
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .start_handler(move |_| {
                        MxcsrGuard::set(wcsr);
                        test_counters::attach_thread(Some(Arc::clone(&sw)));
                    })
                    .build()
                    .unwrap();
                let res = pool.install(|| levels_with_backend(&sig_norm, ParMode::Rayon, use_avx2));
                assert!(res.is_ok(), "normal qualified {wcsr:#06x} failed: {res:?}");
                let c = sink.snapshot();
                if use_avx2 {
                    assert_eq!(c.avx2_fast_blocks, expected_units, "wcsr={wcsr:#06x}");
                    assert_eq!(c.avx2_cold_replays, 0, "wcsr={wcsr:#06x}");
                } else {
                    assert_eq!(c.scalar_fast_blocks, expected_units, "wcsr={wcsr:#06x}");
                    assert_eq!(c.scalar_cold_replays, 0, "wcsr={wcsr:#06x}");
                }
            }

            // 2. Natural overflow at start/mid/end & level overflow: triggers cold replay
            for (tag, sig) in [
                ("start", sig_start.as_slice()),
                ("mid", sig_mid.as_slice()),
                ("end", sig_end.as_slice()),
                ("level", sig_level.as_slice()),
            ] {
                let sink = Arc::new(test_counters::Sink::default());
                let sw = Arc::clone(&sink);
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .start_handler(move |_| {
                        MxcsrGuard::set(0x1f80);
                        test_counters::attach_thread(Some(Arc::clone(&sw)));
                    })
                    .build()
                    .unwrap();
                let res = pool.install(|| levels_with_backend(sig, ParMode::Rayon, use_avx2));
                assert!(
                    matches!(
                        res,
                        Err(SoneflowError::InternalNumericFault {
                            stage: Stage::Filter,
                            ..
                        })
                    ),
                    "{tag} expected Filter fault, got {res:?}"
                );
                let c = sink.snapshot();
                if use_avx2 {
                    assert_eq!(c.avx2_fast_blocks, expected_units, "{tag}");
                    assert!(c.avx2_cold_replays > 0, "{tag} expected cold_replays > 0");
                } else {
                    assert_eq!(c.scalar_fast_blocks, expected_units, "{tag}");
                    assert!(c.scalar_cold_replays > 0, "{tag} expected cold_replays > 0");
                }
            }

            // 3. Min-normal tail fault: cold replay MUST restore MXCSR so UE (0x10) is NOT raised
            for wcsr in [0x1f80u32, 0x1f81, 0x9fc0, 0x9fc1] {
                let sink = Arc::new(test_counters::Sink::default());
                let sw = Arc::clone(&sink);
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .start_handler(move |_| {
                        MxcsrGuard::set(wcsr);
                        test_counters::attach_thread(Some(Arc::clone(&sw)));
                    })
                    .build()
                    .unwrap();
                let (res, exit_csr) = pool.install(|| {
                    MxcsrGuard::set(wcsr);
                    let r = levels_with_backend(&sig_minnorm_tail, ParMode::Rayon, use_avx2);
                    let after = MxcsrGuard::capture().0;
                    (r, after)
                });
                assert!(
                    matches!(
                        res,
                        Err(SoneflowError::InternalNumericFault {
                            stage: Stage::Filter,
                            index: 2400,
                            band: Some(0),
                            ..
                        })
                    ),
                    "minnorm_tail {wcsr:#06x} got {res:?}"
                );
                assert_eq!(
                    exit_csr & 0x10,
                    0,
                    "minnorm_tail {wcsr:#06x} leaked UE flag: exit_csr={exit_csr:#06x}"
                );
                let c = sink.snapshot();
                if use_avx2 {
                    assert_eq!(c.avx2_fast_blocks, expected_units);
                    assert_eq!(c.avx2_cold_replays, expected_units);
                } else {
                    assert_eq!(c.scalar_fast_blocks, expected_units);
                    assert_eq!(c.scalar_cold_replays, expected_units);
                }
            }

            // 4. Unqualified worker: fast_blocks == 0, cold_replays == 0
            for wcsr in [0x7f80u32, 0x9f80, 0x1fc0, 0x1b80] {
                let sink = Arc::new(test_counters::Sink::default());
                let sw = Arc::clone(&sink);
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .start_handler(move |_| {
                        MxcsrGuard::set(wcsr);
                        test_counters::attach_thread(Some(Arc::clone(&sw)));
                    })
                    .build()
                    .unwrap();
                let res = pool.install(|| levels_with_backend(&sig_norm, ParMode::Rayon, use_avx2));
                assert!(res.is_ok());
                let c = sink.snapshot();
                if use_avx2 {
                    assert_eq!(c.avx2_unqualified, expected_units, "wcsr={wcsr:#06x}");
                    assert_eq!(c.avx2_fast_blocks, 0, "wcsr={wcsr:#06x}");
                    assert_eq!(c.avx2_cold_replays, 0, "wcsr={wcsr:#06x}");
                } else {
                    assert_eq!(c.scalar_unqualified, expected_units, "wcsr={wcsr:#06x}");
                    assert_eq!(c.scalar_fast_blocks, 0, "wcsr={wcsr:#06x}");
                    assert_eq!(c.scalar_cold_replays, 0, "wcsr={wcsr:#06x}");
                }
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn order5_batch_hooks_snapshots_due_cd_and_ie_case1() {
        use batch_hooks::BatchHookEvent;

        let _restore = MxcsrGuard::capture();
        let has_avx2 =
            std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
        let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };

        let calm: Vec<f64> = (0..4800)
            .map(|i| ((i as f64) * std::f64::consts::TAU * 1000.0 / 48000.0).sin() * 0.02)
            .collect();
        let hev = |sample, stage, band, field, raw_u64| BatchHookEvent {
            public_call_id: 0,
            sample,
            stage,
            band,
            field,
            raw_u64,
        };

        for &use_avx2 in backends {
            let pool1 = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .start_handler(|_| MxcsrGuard::set(0x1f80))
                .build()
                .unwrap();

            // 1. Review counterexamples (s=25, s=0): fault must be at s, NOT natural 100
            let mut sig_nat100 = calm.clone();
            sig_nat100[100] = f64::MAX;
            for s_pos in [0usize, 25] {
                let res = pool1.install(|| {
                    batch_hooks::install_thread_schedule(vec![hev(
                        s_pos,
                        1,
                        0,
                        0,
                        1e155_f64.to_bits(),
                    )]);
                    let r = levels_with_backend(&sig_nat100, ParMode::Rayon, use_avx2);
                    batch_hooks::clear_thread_schedule();
                    r
                });
                assert!(
                    matches!(
                        res,
                        Err(SoneflowError::InternalNumericFault {
                            stage: Stage::Filter,
                            index,
                            band: Some(0),
                            ..
                        }) if index == s_pos
                    ),
                    "counterexample s={s_pos} avx2={use_avx2}: got {res:?}"
                );
            }

            // 2. Multi-hook snapshot overwrite (hook1 at 25 valid, hook2 at 50 triggers 1e155 overflow)
            let res_m = pool1.install(|| {
                batch_hooks::install_thread_schedule(vec![
                    hev(25, 1, 0, 0, 0.5_f64.to_bits()),
                    hev(50, 1, 0, 0, 1e155_f64.to_bits()),
                ]);
                let r = levels_with_backend(&sig_nat100, ParMode::Rayon, use_avx2);
                batch_hooks::clear_thread_schedule();
                r
            });
            assert!(
                matches!(
                    res_m,
                    Err(SoneflowError::InternalNumericFault {
                        stage: Stage::Filter,
                        index: 50,
                        band: Some(0),
                        ..
                    })
                ),
                "multi-hook overwrite avx2={use_avx2}: got {res_m:?}"
            );

            // 3. Expiry rule c under DAZ (0x9fc0): neg min subnormal in sm[0] MUST be caught at s
            let pool_daz = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .start_handler(|_| MxcsrGuard::set(0x9fc0))
                .build()
                .unwrap();
            for s_pos in [0usize, 25] {
                let res_daz = pool_daz.install(|| {
                    batch_hooks::install_thread_schedule(vec![hev(
                        s_pos,
                        1,
                        0,
                        6,
                        0x8000_0000_0000_0001,
                    )]);
                    let r = levels_with_backend(&calm, ParMode::Rayon, use_avx2);
                    batch_hooks::clear_thread_schedule();
                    r
                });
                assert!(
                    matches!(
                        res_daz,
                        Err(SoneflowError::InternalNumericFault {
                            stage: Stage::Filter,
                            reason: crate::NumericFaultReason::NegativeDomain,
                            index,
                            band: Some(0),
                        }) if index == s_pos
                    ),
                    "DAZ negsub s={s_pos} avx2={use_avx2}: got {res_daz:?}"
                );
            }

            // 4. IE Case 1 (Amendment r8 §2.4): scalar R stops at sec 0 with IE=0; cold replay must restore MXCSR!
            if !use_avx2 {
                for init_csr in [0x1f80u32, 0x1f84, 0x9fc0, 0x9fc4] {
                    let pool_ie = rayon::ThreadPoolBuilder::new()
                        .num_threads(1)
                        .start_handler(move |_| MxcsrGuard::set(init_csr))
                        .build()
                        .unwrap();
                    let mut sig_ie = vec![0.0f64; 4800];
                    sig_ie[25] = f64::MAX;
                    let (r, exit_csr) = pool_ie.install(|| {
                        MxcsrGuard::set(init_csr);
                        batch_hooks::install_thread_schedule(vec![hev(
                            25,
                            1,
                            0,
                            0,
                            f64::MAX.to_bits(),
                        )]);
                        let res = levels_with_backend(&sig_ie, ParMode::Rayon, false);
                        let after = MxcsrGuard::capture().0;
                        batch_hooks::clear_thread_schedule();
                        (res, after)
                    });
                    assert!(matches!(
                        r,
                        Err(SoneflowError::InternalNumericFault {
                            stage: Stage::Filter,
                            index: 25,
                            band: Some(0),
                            ..
                        })
                    ));
                    assert_eq!(
                        exit_csr & 1,
                        0,
                        "IE Case 1 leaked IE flag under init={init_csr:#06x}: exit={exit_csr:#06x}"
                    );
                }
            }

            // 5. Expiry rule d (batch NL MosqitoTail due at t=0, cleared at t>=1; Zero not due unless hooked)
            let core_ok = vec![0.25f64; 21 * 8];
            let run_nl = |init: NlInit| {
                let b = nl_coeffs();
                let hook_ctx = batch_hooks::snapshot_stage(3, 8);
                let mut out = vec![0.0; 21 * 8];
                if use_avx2 {
                    for g in 0..5usize {
                        let band = 4 * g;
                        unsafe {
                            avx2::nonlinear(
                                &core_ok,
                                &mut out[band * 8..(band + 4) * 8],
                                8,
                                band,
                                b,
                                init,
                                true,
                                false,
                                &hook_ctx,
                            )
                            .unwrap();
                        }
                    }
                    nl_band(
                        &core_ok[20 * 8..21 * 8],
                        &mut out[20 * 8..21 * 8],
                        &b,
                        init,
                        20,
                        true,
                        false,
                        &hook_ctx,
                    )
                    .unwrap();
                } else {
                    for band in 0..21usize {
                        nl_band(
                            &core_ok[band * 8..(band + 1) * 8],
                            &mut out[band * 8..(band + 1) * 8],
                            &b,
                            init,
                            band,
                            true,
                            false,
                            &hook_ctx,
                        )
                        .unwrap();
                    }
                }
            };
            let (log_mosqito, log_zero, log_zero_hooked) = pool1.install(|| {
                batch_hooks::install_thread_schedule(Vec::new());
                batch_hooks::begin_due_log();
                run_nl(NlInit::MosqitoTail);
                let lm = batch_hooks::take_due_log();
                batch_hooks::begin_due_log();
                run_nl(NlInit::Zero);
                let lz = batch_hooks::take_due_log();
                batch_hooks::install_thread_schedule(vec![hev(3, 3, 0, 0, 0.5_f64.to_bits())]);
                batch_hooks::begin_due_log();
                run_nl(NlInit::Zero);
                let lzh = batch_hooks::take_due_log();
                batch_hooks::clear_thread_schedule();
                (lm, lz, lzh)
            });
            assert!(
                log_mosqito
                    .iter()
                    .filter(|r| r.stage == 3 && r.index == 0)
                    .all(|r| r.due)
                    && log_mosqito.iter().any(|r| r.stage == 3 && r.index == 0),
                "Rule d: MosqitoTail must set due_nl=true at t=0 (avx2={use_avx2})"
            );
            assert!(
                log_mosqito
                    .iter()
                    .filter(|r| r.stage == 3 && r.index > 0)
                    .all(|r| !r.due),
                "Rule d: MosqitoTail due_nl must clear after t=0 (avx2={use_avx2})"
            );
            assert!(
                log_zero.iter().filter(|r| r.stage == 3).all(|r| !r.due),
                "NlInit::Zero without hooks must not set due_nl (avx2={use_avx2})"
            );
            assert!(
                log_zero_hooked
                    .iter()
                    .any(|r| r.stage == 3 && r.band == 0 && r.index == 3 && r.due)
                    && log_zero_hooked
                        .iter()
                        .filter(|r| r.stage == 3 && !(r.band == 0 && r.index == 3))
                        .all(|r| !r.due),
                "Rule c: NL hook at t=3 band=0 must set due_nl ONLY at (band=0, t=3) (avx2={use_avx2})"
            );

            // 6. Expiry rule c for TW + invalid s = N rejection for TOL, NL, TW + NA stage rejection
            let tw_vals = vec![0.5f64; 8];
            let tw_log = pool1.install(|| {
                batch_hooks::install_thread_schedule(vec![hev(4, 5, 0, 0, 1.0_f64.to_bits())]);
                batch_hooks::begin_due_log();
                temporal(&tw_vals).unwrap();
                let l = batch_hooks::take_due_log();
                batch_hooks::clear_thread_schedule();
                l
            });
            assert!(tw_log.iter().any(|r| r.stage == 5 && r.index == 4 && r.due));
            assert!(tw_log
                .iter()
                .filter(|r| r.stage == 5 && r.index != 4)
                .all(|r| !r.due));

            // Invalid s == N MUST panic for TOL (4800), NL (8), TW (8), and NA stage (2)
            for (stage, bad_s) in [(1u32, 4800usize), (3, 8), (5, 8), (2, 0)] {
                let caught = pool1.install(|| {
                    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        batch_hooks::install_thread_schedule(vec![hev(bad_s, stage, 0, 0, 0)]);
                        let _ = match stage {
                            1 | 2 => levels_with_backend(&calm, ParMode::Sequential, use_avx2)
                                .map(|_| ()),
                            3 => nonlinear(&core_ok, 8, ParMode::Sequential, NlInit::Zero)
                                .map(|_| ()),
                            5 => temporal(&tw_vals).map(|_| ()),
                            _ => unreachable!(),
                        };
                    }));
                    batch_hooks::clear_thread_schedule();
                    res
                });
                assert!(
                    caught.is_err(),
                    "stage={stage} s={bad_s} must panic as invalid/NA coordinate"
                );
            }

            // 7. Unqualified worker (0x9f80) MUST still execute batch TOL hooks!
            let pool_unq = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .start_handler(|_| MxcsrGuard::set(0x9f80))
                .build()
                .unwrap();
            let res_unq = pool_unq.install(|| {
                batch_hooks::install_thread_schedule(vec![hev(25, 1, 0, 6, 0x8000_0000_0000_0001)]);
                let r = levels_with_backend(&calm, ParMode::Rayon, use_avx2);
                let w = batch_hooks::executed_writes();
                batch_hooks::clear_thread_schedule();
                (r, w)
            });
            assert_eq!(res_unq.1, 1);
            assert!(matches!(
                res_unq.0,
                Err(SoneflowError::InternalNumericFault {
                    stage: Stage::Filter,
                    index: 25,
                    band: Some(0),
                    ..
                })
            ));
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn order5_batch_avx2_lane_isolation_tol_and_nl() {
        use batch_hooks::BatchHookEvent;

        if !(std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma")) {
            return;
        }
        let _restore = MxcsrGuard::capture();
        let pool1 = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .start_handler(|_| MxcsrGuard::set(0x1f80))
            .build()
            .unwrap();

        let calm: Vec<f64> = (0..240)
            .map(|i| ((i as f64) * std::f64::consts::TAU * 1000.0 / 48000.0).sin() * 0.02)
            .collect();
        let hev = |sample, stage, band, field, raw_u64| BatchHookEvent {
            public_call_id: 0,
            sample,
            stage,
            band,
            field,
            raw_u64,
        };

        // 1. TOL AVX2 lane isolation: hooking one lane of group 0 (bands 0..4) must change ONLY
        //    that band's output; the 3 sibling lanes MUST remain bit-exact identical to unhooked.
        let (base_tol, n_tol) = pool1
            .install(|| levels_with_backend(&calm, ParMode::Sequential, true))
            .unwrap();
        for target_lane in 0..4usize {
            let (hooked_tol, n_h) = pool1
                .install(|| {
                    batch_hooks::install_thread_schedule(vec![hev(
                        25,
                        1,
                        target_lane,
                        6,
                        1.5_f64.to_bits(),
                    )]);
                    let r = levels_with_backend(&calm, ParMode::Sequential, true);
                    let w = batch_hooks::executed_writes();
                    batch_hooks::clear_thread_schedule();
                    assert_eq!(w, 1);
                    r
                })
                .unwrap();
            assert_eq!(n_h, n_tol);
            for lane in 0..4usize {
                let b_slice = bits(&base_tol[lane * n_tol..(lane + 1) * n_tol]);
                let h_slice = bits(&hooked_tol[lane * n_tol..(lane + 1) * n_tol]);
                if lane == target_lane {
                    assert_ne!(
                        h_slice, b_slice,
                        "TOL AVX2 hook on lane {target_lane} must affect target lane"
                    );
                } else {
                    assert_eq!(
                        h_slice, b_slice,
                        "TOL AVX2 hook on lane {target_lane} clobbered sibling lane {lane}"
                    );
                }
            }
        }

        // 2. NL AVX2 lane isolation: hooking one lane of group 0 (bands 0..4) must change ONLY
        //    that band's output; the 3 sibling lanes MUST remain bit-exact identical to unhooked.
        let n_nl = 16usize;
        let core_ok: Vec<f64> = (0..21 * n_nl)
            .map(|i| 0.25 + ((i % 13) as f64) * 0.05)
            .collect();
        let b_coeffs = nl_coeffs();
        let run_nl_g0 = |hooks: Vec<BatchHookEvent>| -> Vec<f64> {
            pool1.install(|| {
                batch_hooks::install_thread_schedule(hooks);
                let hook_ctx = batch_hooks::snapshot_stage(3, n_nl);
                let mut out = vec![0.0; 4 * n_nl];
                unsafe {
                    avx2::nonlinear(
                        &core_ok,
                        &mut out,
                        n_nl,
                        0,
                        b_coeffs,
                        NlInit::Zero,
                        true,
                        false,
                        &hook_ctx,
                    )
                    .unwrap();
                }
                batch_hooks::clear_thread_schedule();
                out
            })
        };
        let base_nl = run_nl_g0(Vec::new());
        for target_lane in 0..4usize {
            let hooked_nl = run_nl_g0(vec![
                hev(3, 3, target_lane, 0, 5.0_f64.to_bits()),
                hev(3, 3, target_lane, 1, 2.0_f64.to_bits()),
            ]);
            for lane in 0..4usize {
                let b_slice = bits(&base_nl[lane * n_nl..(lane + 1) * n_nl]);
                let h_slice = bits(&hooked_nl[lane * n_nl..(lane + 1) * n_nl]);
                if lane == target_lane {
                    assert_ne!(
                        h_slice, b_slice,
                        "NL AVX2 hook on lane {target_lane} must affect target lane"
                    );
                } else {
                    assert_eq!(
                        h_slice, b_slice,
                        "NL AVX2 hook on lane {target_lane} clobbered sibling lane {lane}"
                    );
                }
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn order6_batch_d0_nl_slopes_tw_dispatch_and_seed_entry() {
        use std::sync::Arc;

        let _restore = MxcsrGuard::capture();
        let has_avx2 =
            std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
        let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };
        let n_time = 200usize;
        let core_ok: Vec<f64> = (0..21 * n_time)
            .map(|i| 0.25 + ((i % 13) as f64) * 0.05)
            .collect();

        // 1. Qualified 1-thread pool: NL, slopes, and TW MUST take D0 fast paths!
        for &use_avx2 in backends {
            for wcsr in [0x1f80u32, 0x9fc0] {
                let sink = Arc::new(test_counters::Sink::default());
                let sw = Arc::clone(&sink);
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .start_handler(move |_| {
                        MxcsrGuard::set(wcsr);
                        test_counters::attach_thread(Some(Arc::clone(&sw)));
                    })
                    .build()
                    .unwrap();
                let res = pool.install(|| -> Result<(), SoneflowError> {
                    MxcsrGuard::set(wcsr);
                    let nl = nonlinear_with_backend(
                        &core_ok,
                        n_time,
                        ParMode::Rayon,
                        NlInit::Zero,
                        true,
                        use_avx2,
                    )?;
                    let mut loud = Vec::new();
                    let mut spec = Vec::new();
                    let mut unq =
                        crate::zwtv::slopes::slopes_into_d0(&nl, n_time, &mut loud, &mut spec);
                    validate_slopes_chunks(&loud, &spec, &mut unq)?;
                    let _ = temporal(&loud)?;
                    Ok(())
                });
                assert!(res.is_ok(), "avx2={use_avx2} wcsr={wcsr:#06x}: {res:?}");
                let c = sink.snapshot();
                if use_avx2 {
                    assert_eq!(c.nl_avx2_qualified, 5, "avx2 NL 5 groups");
                    assert_eq!(c.nl_avx2_unqualified, 0);
                    assert_eq!(c.nl_avx2_fast_frames, 5 * n_time);
                    assert_eq!(c.nl_scalar_qualified, 1, "avx2 NL scalar tail band 20");
                    assert_eq!(c.nl_scalar_fast_frames, n_time);
                } else {
                    assert_eq!(c.nl_scalar_qualified, 21, "scalar NL 21 bands");
                    assert_eq!(c.nl_scalar_unqualified, 0);
                    assert_eq!(c.nl_scalar_fast_frames, 21 * n_time);
                }
                assert_eq!(
                    c.slopes_qualified, 50,
                    "200 frames / OUT_DECIM(4) = 50 tasks"
                );
                assert_eq!(c.slopes_unqualified, 0);
                assert_eq!(c.slopes_unqual_validated, 0);
                assert_eq!(c.tw_qualified, 1);
                assert_eq!(c.tw_unqualified, 0);
                assert_eq!(c.tw_fast_frames, n_time);
            }
        }

        // 2. Unqualified worker (0x9f80 FTZ=1,DAZ=0):
        //    NL tasks, slopes tasks, and TW MUST fall back to R checked / R validate_slopes_chunks!
        for &use_avx2 in backends {
            let sink = Arc::new(test_counters::Sink::default());
            let sw = Arc::clone(&sink);
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .start_handler(move |_| {
                    MxcsrGuard::set(0x9f80);
                    test_counters::attach_thread(Some(Arc::clone(&sw)));
                })
                .build()
                .unwrap();
            let res = pool.install(|| -> Result<(), SoneflowError> {
                let nl = nonlinear_with_backend(
                    &core_ok,
                    n_time,
                    ParMode::Rayon,
                    NlInit::Zero,
                    true,
                    use_avx2,
                )?;
                let mut loud = Vec::new();
                let mut spec = Vec::new();
                let mut unq =
                    crate::zwtv::slopes::slopes_into_d0(&nl, n_time, &mut loud, &mut spec);
                validate_slopes_chunks(&loud, &spec, &mut unq)?;
                let _ = temporal(&loud)?;
                Ok(())
            });
            assert!(res.is_ok());
            let c = sink.snapshot();
            if use_avx2 {
                assert_eq!(c.nl_avx2_qualified, 0);
                assert_eq!(c.nl_avx2_unqualified, 5);
                assert_eq!(c.nl_avx2_fast_frames, 0);
                assert_eq!(c.nl_scalar_unqualified, 1);
                assert_eq!(c.nl_scalar_fast_frames, 0);
            } else {
                assert_eq!(c.nl_scalar_qualified, 0);
                assert_eq!(c.nl_scalar_unqualified, 21);
                assert_eq!(c.nl_scalar_fast_frames, 0);
            }
            assert_eq!(c.slopes_qualified, 0);
            assert_eq!(c.slopes_unqualified, 50);
            assert_eq!(c.slopes_unqual_validated, 50);
            assert_eq!(c.tw_qualified, 0);
            assert_eq!(c.tw_unqualified, 1);
            assert_eq!(c.tw_fast_frames, 0);
        }

        // 3. MosqitoTail 13*eta seed negative entry check under RN(0,0): MUST catch NegativeDomain at frame 0!
        for &use_avx2 in backends {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .start_handler(|_| MxcsrGuard::set(0x1f80))
                .build()
                .unwrap();
            let mut core = vec![0.0f64; 21 * 4];
            core[3] = f64::from_bits(13);
            let res = pool.install(|| {
                nonlinear_with_backend(
                    &core,
                    4,
                    ParMode::Rayon,
                    NlInit::MosqitoTail,
                    true,
                    use_avx2,
                )
            });
            assert!(
                matches!(
                    res,
                    Err(SoneflowError::InternalNumericFault {
                        stage: Stage::Nonlinear,
                        reason: crate::NumericFaultReason::NegativeDomain,
                        index: 0,
                        band: Some(0),
                    })
                ),
                "MosqitoTail 13*eta seed must fail NegativeDomain at frame 0 (avx2={use_avx2}): got {res:?}"
            );
        }

        // 4. validate_slopes_chunks preserves exact R (frame, band) ordering (total band = usize::MAX)
        {
            let mut total = [0.0; 12];
            let mut specific = [0.0; 720];
            total[8] = f64::NAN;
            specific[240 + 7] = f64::NAN;
            assert!(matches!(
                validate_slopes_chunks(&total, &specific, &mut [2, 1, 0]),
                Err(SoneflowError::InternalNumericFault {
                    stage: Stage::Slope,
                    index: 4,
                    band: Some(7),
                    ..
                })
            ));
            total[4] = f64::INFINITY;
            assert!(matches!(
                validate_slopes_chunks(&total, &specific, &mut [2, 1, 0]),
                Err(SoneflowError::InternalNumericFault {
                    stage: Stage::Slope,
                    index: 4,
                    band: Some(7),
                    ..
                })
            ));
            total[2] = f64::INFINITY;
            assert!(matches!(
                validate_slopes_chunks(&total, &specific, &mut [2, 1, 0]),
                Err(SoneflowError::InternalNumericFault {
                    stage: Stage::Slope,
                    index: 2,
                    band: None,
                    ..
                })
            ));
        }

        // 5. DAZ (0x9fc0): -0.0 accepted as legal; -1*eta rejected as NegativeDomain for NL & TW
        {
            let pool_daz = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .start_handler(|_| MxcsrGuard::set(0x9fc0))
                .build()
                .unwrap();
            let neg_zero_vals = vec![f64::from_bits(0x8000_0000_0000_0000); 4];
            let mut neg_sub_vals = vec![0.0f64; 4];
            neg_sub_vals[2] = f64::from_bits(0x8000_0000_0000_0001);
            assert!(pool_daz.install(|| temporal(&neg_zero_vals)).is_ok());
            assert!(matches!(
                pool_daz.install(|| temporal(&neg_sub_vals)),
                Err(SoneflowError::InternalNumericFault {
                    stage: Stage::TemporalWeighting,
                    reason: crate::NumericFaultReason::NegativeDomain,
                    index: 2,
                    band: None,
                })
            ));
            for &use_avx2 in backends {
                let core_nz = vec![f64::from_bits(0x8000_0000_0000_0000); 21 * 4];
                assert!(pool_daz
                    .install(|| nonlinear_with_backend(
                        &core_nz,
                        4,
                        ParMode::Rayon,
                        NlInit::Zero,
                        false,
                        use_avx2
                    ))
                    .is_ok());
                let mut core_ns = vec![0.0f64; 21 * 4];
                core_ns[20 * 4 + 1] = f64::from_bits(0x8000_0000_0000_0001);
                assert!(matches!(
                    pool_daz.install(|| nonlinear_with_backend(
                        &core_ns,
                        4,
                        ParMode::Rayon,
                        NlInit::Zero,
                        false,
                        use_avx2
                    )),
                    Err(SoneflowError::InternalNumericFault {
                        stage: Stage::Nonlinear,
                        reason: crate::NumericFaultReason::NegativeDomain,
                        index: 0,
                        band: Some(20),
                    })
                ));
            }
        }
    }
}
