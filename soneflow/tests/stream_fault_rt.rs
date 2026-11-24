//! Own synthetic fault/recovery trace; no material fixtures or timing gates.
use soneflow::{FieldType, FrameQualityV1, StreamFrame, ZwtvStream};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingAllocator;
thread_local! { static ACTIVE: Cell<bool> = const { Cell::new(false) }; }
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

fn count() {
    if ACTIVE.try_with(Cell::get).unwrap_or(false) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn fault_recovery_quality_ack_and_reset_allocate_nothing_and_do_not_enter_rayon() {
    let mut stream = ZwtvStream::new(FieldType::Free);
    let mut out = [StreamFrame::default(); 7];
    let mut quality = [FrameQualityV1::default(); 7];
    let mut signal = [0.0; 480];
    signal[23] = f64::MAX;
    ACTIVE.with(|flag| flag.set(true));
    stream
        .push_with_quality(&signal, &mut out, &mut quality)
        .unwrap();
    let summary = stream.diagnostics();
    stream.ack_diagnostics(summary.sequence).unwrap();
    for _ in 0..130 {
        stream
            .push_with_quality(&[0.0; 480], &mut out, &mut quality)
            .unwrap();
    }
    stream.flush_with_quality(&mut out, &mut quality).unwrap();
    stream.reset_with_diagnostics().unwrap();
    for _ in 0..10 {
        stream
            .push_with_quality(&[f64::MAX; 480], &mut out, &mut quality)
            .unwrap();
    }
    stream.flush_with_quality(&mut out, &mut quality).unwrap();
    ACTIVE.with(|flag| flag.set(false));
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), 0);
    assert!(stream.diagnostics().incident_count > 0);
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build_global()
        .expect("stream must not initialize the global Rayon pool");
}
