//! Test-only batch hook scheduler and due-rule tracker (due rules: see `crate::numeric_guard`).
//! Completely absent from default product builds (`#[cfg(test)]`).

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
