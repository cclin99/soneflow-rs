//! Causal, allocation-free time-varying loudness for real-time use.
//!
//! [`ZwtvStream`] wires `tol_stage` (filter bank and decimation) to `emitter`
//! (everything after it, with `nl_stage` inside) under a `denormal` guard.
//! `frame` holds the frozen output ABI, `reference` an offline twin used by
//! the tests.

mod denormal;
mod diagnostics;
mod emitter;
mod frame;
mod nl_stage;
#[cfg(feature = "internals")]
mod reference;
mod tol_stage;

pub use diagnostics::{DiagnosticSnapshotV1, FrameQualityV1};
pub use frame::{FrameFlags, StreamFrame, N_WARMUP_FRAMES};
#[cfg(feature = "internals")]
pub use reference::zwtv_reference_zerostate;

use crate::error::StreamUsageError;
use crate::numeric_guard::{read_rounding_environment, DomainFault, RoundingEnvironment};
use crate::types::FieldType;
use crate::zwtv::grid::{DEC_FACTOR, OUT_DECIM};
use denormal::DenormalGuard;
#[cfg(target_arch = "x86_64")]
use emitter::DownstreamDue;
use emitter::FrameEmitter;
#[cfg(target_arch = "x86_64")]
use tol_stage::{SpecEmission, STREAM_BLOCK_LEN, STREAM_MAX_EMISSIONS};
use tol_stage::{TolKernel, TolStage};

#[cfg(test)]
mod test_stream_hooks {
    use std::cell::RefCell;

    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub(super) struct StreamTolCounts {
        pub env_fallbacks: usize,
        pub entry_due_passed: usize,
        pub entry_due_replays: usize,
        pub spec_blocks: usize,
        pub swaps: usize,
        pub cold_replays: usize,
        pub downstream_restarts: usize,
    }

    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub(super) struct StreamD0Counts {
        pub d0_on_levels: usize,
        pub d0_due_checks: usize,
        pub d0_due_fallbacks: usize,
        pub d0_calculate: usize,
        pub checked_calculate: usize,
    }

    thread_local! {
        static COUNTS: RefCell<StreamTolCounts> = const { RefCell::new(StreamTolCounts {
            env_fallbacks: 0,
            entry_due_passed: 0,
            entry_due_replays: 0,
            spec_blocks: 0,
            swaps: 0,
            cold_replays: 0,
            downstream_restarts: 0,
        }) };
        static D0_COUNTS: RefCell<StreamD0Counts> = const { RefCell::new(StreamD0Counts {
            d0_on_levels: 0,
            d0_due_checks: 0,
            d0_due_fallbacks: 0,
            d0_calculate: 0,
            checked_calculate: 0,
        }) };
    }

    pub(super) fn reset() {
        COUNTS.with(|c| *c.borrow_mut() = StreamTolCounts::default());
        D0_COUNTS.with(|c| *c.borrow_mut() = StreamD0Counts::default());
    }

    pub(super) fn snapshot() -> StreamTolCounts {
        COUNTS.with(|c| *c.borrow())
    }

    #[cfg(target_arch = "x86_64")]
    pub(super) fn snapshot_d0() -> StreamD0Counts {
        D0_COUNTS.with(|c| *c.borrow())
    }

    #[inline]
    pub(super) fn record_env_fallback() {
        COUNTS.with(|c| c.borrow_mut().env_fallbacks += 1);
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub(super) fn record_entry_due_passed() {
        COUNTS.with(|c| c.borrow_mut().entry_due_passed += 1);
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub(super) fn record_entry_due_replay() {
        COUNTS.with(|c| c.borrow_mut().entry_due_replays += 1);
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub(super) fn record_spec_block() {
        COUNTS.with(|c| c.borrow_mut().spec_blocks += 1);
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub(super) fn record_swap() {
        COUNTS.with(|c| c.borrow_mut().swaps += 1);
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub(super) fn record_cold_replay() {
        COUNTS.with(|c| c.borrow_mut().cold_replays += 1);
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub(super) fn record_downstream_restart() {
        COUNTS.with(|c| c.borrow_mut().downstream_restarts += 1);
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub(super) fn record_d0_on_levels() {
        D0_COUNTS.with(|c| c.borrow_mut().d0_on_levels += 1);
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub(super) fn record_d0_due_check() {
        D0_COUNTS.with(|c| c.borrow_mut().d0_due_checks += 1);
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub(super) fn record_d0_due_fallback() {
        D0_COUNTS.with(|c| c.borrow_mut().d0_due_fallbacks += 1);
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub(super) fn record_d0_calculate() {
        D0_COUNTS.with(|c| c.borrow_mut().d0_calculate += 1);
    }

    #[inline]
    pub(super) fn record_checked_calculate() {
        D0_COUNTS.with(|c| c.borrow_mut().checked_calculate += 1);
    }
}

/// Test-only stream hook scheduler.
/// Completely absent from default product builds (`#[cfg(test)]`).
#[cfg(test)]
pub mod stream_hooks {
    use std::cell::RefCell;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct StreamHookEvent {
        pub public_call_id: u64,
        pub sample: usize,
        pub stage: u32,
        pub band: usize,
        pub field: usize,
        pub raw_u64: u64,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct DueSnapshot {
        pub tol: bool,
        pub held: bool,
        pub nl: bool,
        pub tw: bool,
    }

    struct SchedulerState {
        schedule: Vec<StreamHookEvent>,
        next_call_id: u64,
        active_call_id: u64,
        executed_writes: usize,
        due_log: Option<Vec<(u64, usize, DueSnapshot)>>,
    }

    impl SchedulerState {
        const fn empty() -> Self {
            Self {
                schedule: Vec::new(),
                next_call_id: 0,
                active_call_id: 0,
                executed_writes: 0,
                due_log: None,
            }
        }
    }

    thread_local! {
        static SCHED: RefCell<SchedulerState> = const { RefCell::new(SchedulerState::empty()) };
    }

    /// Install a schedule of `(public_call_id, s, stage, band, field, raw_u64)` writes.
    /// Stable sort preserves exact schedule order for multiple writes at the same `(call_id, s)`.
    pub fn install_schedule(mut events: Vec<StreamHookEvent>) {
        events.sort_by_key(|e| (e.public_call_id, e.sample));
        SCHED.with(|s| {
            let mut st = s.borrow_mut();
            st.schedule = events;
            st.next_call_id = 0;
            st.active_call_id = 0;
            st.executed_writes = 0;
        });
    }

    pub fn clear_schedule() {
        SCHED.with(|s| {
            let mut st = s.borrow_mut();
            st.schedule.clear();
            st.next_call_id = 0;
            st.active_call_id = 0;
            st.executed_writes = 0;
            st.due_log = None;
        });
    }

    pub fn executed_writes() -> usize {
        SCHED.with(|s| s.borrow().executed_writes)
    }

    pub fn begin_due_log() {
        SCHED.with(|s| s.borrow_mut().due_log = Some(Vec::new()));
    }

    pub fn take_due_log() -> Vec<(u64, usize, DueSnapshot)> {
        SCHED.with(|s| s.borrow_mut().due_log.take().unwrap_or_default())
    }

    pub(super) fn record_due(call_id: u64, sample: usize, due: DueSnapshot) {
        SCHED.with(|s| {
            if let Some(log) = s.borrow_mut().due_log.as_mut() {
                log.push((call_id, sample, due));
            }
        });
    }

    pub(super) fn record_executed_write() {
        SCHED.with(|s| s.borrow_mut().executed_writes += 1);
    }

    pub(super) fn begin_public_call() {
        SCHED.with(|s| {
            let mut st = s.borrow_mut();
            st.active_call_id = st.next_call_id;
            st.next_call_id += 1;
        });
    }

    pub(super) fn assert_no_hooks_for_active_call() {
        SCHED.with(|s| {
            let st = s.borrow();
            if st.schedule.is_empty() {
                return;
            }
            let cid = st.active_call_id;
            if let Some(ev) = st.schedule.iter().find(|e| e.public_call_id == cid) {
                panic!(
                    "invalid stream hook on flush/zero-len call_id={cid}: sample={} (must satisfy 0 <= s < N)",
                    ev.sample
                );
            }
        });
    }

    pub(super) struct HookCursor {
        call_id: u64,
        events: Vec<StreamHookEvent>,
        idx: usize,
    }

    impl HookCursor {
        pub(super) fn for_call(chunk_len: usize) -> Self {
            SCHED.with(|s| {
                let st = s.borrow();
                let call_id = st.active_call_id;
                if st.schedule.is_empty() {
                    return Self {
                        call_id,
                        events: Vec::new(),
                        idx: 0,
                    };
                }
                let mut events = Vec::new();
                for &ev in &st.schedule {
                    if ev.public_call_id == call_id {
                        assert!(
                            ev.sample < chunk_len,
                            "invalid stream hook coordinate: call_id={call_id} s={} for chunk len N={chunk_len} (must satisfy 0 <= s < N)",
                            ev.sample
                        );
                        events.push(ev);
                    }
                }
                Self {
                    call_id,
                    events,
                    idx: 0,
                }
            })
        }

        #[inline]
        pub(super) fn call_id(&self) -> u64 {
            self.call_id
        }

        #[inline]
        pub(super) fn peek_sample(&self) -> Option<usize> {
            self.events.get(self.idx).map(|e| e.sample)
        }

        pub(super) fn drain_at(&mut self, sample: usize) -> &[StreamHookEvent] {
            let start = self.idx;
            while self.idx < self.events.len() && self.events[self.idx].sample == sample {
                self.idx += 1;
            }
            &self.events[start..self.idx]
        }

        #[inline]
        pub(super) fn is_exhausted(&self) -> bool {
            self.idx == self.events.len()
        }
    }
}

/// Entry-validation due flags, local to one public call (rules a–c: see `crate::numeric_guard`).
#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy)]
struct Due {
    tol: bool,
    downstream: DownstreamDue,
}

#[cfg(target_arch = "x86_64")]
impl Due {
    const fn all_true() -> Self {
        Self {
            tol: true,
            downstream: DownstreamDue::ALL,
        }
    }
}

/// The caller's output buffers for one public call, and how many frames are
/// already written to them.
struct FrameSink<'a> {
    out: &'a mut [StreamFrame],
    quality: Option<&'a mut [FrameQualityV1]>,
    written: usize,
}

impl FrameSink<'_> {
    /// The unwritten tails of both buffers.
    #[inline(always)]
    fn rest(&mut self) -> (&mut [StreamFrame], Option<&mut [FrameQualityV1]>) {
        let written = self.written;
        (
            &mut self.out[written..],
            self.quality.as_deref_mut().map(|q| &mut q[written..]),
        )
    }
}

/// Stateful 48 kHz ISO 532-1 time-varying loudness processor.
///
/// Push and flush allocate no memory and never enter Rayon. Output latency is
/// one 24-sample internal frame.
///
/// Numerical validity requires round-to-nearest ties-to-even at construction
/// and processing. Checked quality APIs verify that environment on supported
/// targets. Legacy callers must inspect sticky diagnostics: finite fallback
/// values are not valid loudness measurements, statistics, or AGC inputs.
///
/// # Examples
///
/// Push chunks of any size, flush at the end; the frames are the batch
/// result, bit for bit:
///
/// ```
/// use soneflow::{loudness_zwtv, FieldType, FrameFlags, StreamFrame, ZwtvStream};
///
/// let signal: Vec<f64> = (0..9_600).map(|i| 0.02 * (i as f64 * 0.13).sin()).collect();
/// let mut stream = ZwtvStream::new(FieldType::Free);
/// let mut out = vec![StreamFrame::default(); ZwtvStream::max_frames_for_chunk(480)];
/// let mut frames = Vec::new();
/// for chunk in signal.chunks(480) {
///     let written = stream.push(chunk, &mut out);
///     frames.extend_from_slice(&out[..written]);
/// }
/// let written = stream.flush(&mut out);
/// frames.extend_from_slice(&out[..written]);
///
/// let batch = loudness_zwtv(&signal, 48_000.0, FieldType::Free)?;
/// assert_eq!(frames.len(), batch.n.len());
/// assert!(frames.iter().zip(&batch.n).all(|(f, n)| f.n.to_bits() == n.to_bits()));
/// assert!(frames[0].flags.contains(FrameFlags::WARMUP)); // the first 580 frames
///
/// stream.reset(); // start a new time line, as a new stream would
/// # Ok::<(), soneflow::SoneflowError>(())
/// ```
///
/// With per-frame quality records and the diagnostic snapshot:
///
/// ```
/// use soneflow::{FieldType, FrameQualityV1, StreamFrame, ZwtvStream};
///
/// let mut stream = ZwtvStream::new(FieldType::Free);
/// let chunk = [0.0; 480];
/// let capacity = ZwtvStream::max_frames_for_chunk(chunk.len());
/// let mut out = vec![StreamFrame::default(); capacity];
/// let mut quality = vec![FrameQualityV1::default(); capacity];
/// let written = stream.push_with_quality(&chunk, &mut out, &mut quality)?;
/// assert!(quality[..written].iter().all(|q| q.quality_bits == 0)); // no fault
///
/// let snapshot = stream.diagnostics();
/// assert_eq!(snapshot.incident_count, 0);
/// stream.ack_diagnostics(snapshot.sequence)?;
/// # Ok::<(), soneflow::StreamUsageError>(())
/// ```
///
/// # Panics
///
/// The legacy `push`, `flush` and `reset` panic on usage errors (see each
/// method). `push_with_quality`, `flush_with_quality` and
/// `reset_with_diagnostics` return the same conditions as
/// [`StreamUsageError`](crate::StreamUsageError) instead.
pub struct ZwtvStream {
    tol: TolStage,
    emitter: FrameEmitter,
    flushed: bool,
    consumed_samples: u64,
    creation_environment: RoundingEnvironment,
}

impl ZwtvStream {
    /// Creates a stream.
    ///
    /// Two details depend on how the compiler built this crate, not on the
    /// source: with the scalar backend, construction may or may not leave the
    /// inexact (PE) status flag set in MXCSR; and a scalar stream created
    /// under a rounding mode other than round-to-nearest may or may not carry
    /// filter coefficients rounded in that mode, which then shows in the
    /// output of the legacy `push`/`flush`. `push_with_quality` and
    /// `flush_with_quality` reject streams created under such a rounding mode
    /// in either case.
    pub fn new(field: FieldType) -> Self {
        Self::with_backend(field, crate::simd::use_avx2())
    }

    fn with_backend(field: FieldType, avx2: bool) -> Self {
        let creation_environment = read_rounding_environment();
        crate::core::main_loudness::warm_mp1_table(creation_environment);
        Self {
            tol: TolStage::new(avx2),
            emitter: FrameEmitter::new(
                field,
                avx2,
                creation_environment != RoundingEnvironment::KnownRn,
            ),
            flushed: false,
            consumed_samples: 0,
            creation_environment,
        }
    }

    /// Begin a new time/session epoch, as `reset_with_diagnostics` without
    /// returning the old snapshot.
    ///
    /// # Panics
    ///
    /// If the session identity is exhausted
    /// ([`StreamUsageError::SessionExhausted`](crate::StreamUsageError::SessionExhausted)).
    pub fn reset(&mut self) {
        self.reset_with_diagnostics()
            .expect("session identity exhausted");
    }

    /// Return the old diagnostic snapshot and begin a new time/session epoch.
    /// Session exhaustion is atomic. This does not repair invalid creation
    /// coefficients or change the caller's floating-point environment.
    pub fn reset_with_diagnostics(&mut self) -> Result<DiagnosticSnapshotV1, StreamUsageError> {
        let before = self
            .emitter
            .diagnostics
            .reset_with_diagnostics()
            .map_err(map_diagnostic_error)?;
        self.tol.reset();
        self.emitter.reset();
        self.flushed = false;
        self.consumed_samples = 0;
        Ok(before)
    }

    /// Peek a bounded conservative coverage union without consuming it.
    pub fn diagnostics(&self) -> DiagnosticSnapshotV1 {
        self.emitter.diagnostics.snapshot()
    }

    /// Acknowledge sticky history at this sequence; active recovery and
    /// undelivered per-frame quality remain intact.
    pub fn ack_diagnostics(&mut self, sequence: u64) -> Result<(), StreamUsageError> {
        self.emitter
            .diagnostics
            .acknowledge(sequence)
            .map_err(map_diagnostic_error)
    }

    /// Output latency in input samples (one internal frame).
    pub const fn latency_samples() -> usize {
        DEC_FACTOR
    }

    /// Output capacity a `push` of `chunk_len` samples needs.
    pub const fn max_frames_for_chunk(chunk_len: usize) -> usize {
        chunk_len / (DEC_FACTOR * OUT_DECIM) + 2
    }

    /// Process `chunk` and write the finished frames to `out`; returns how many
    /// were written.
    ///
    /// # Panics
    ///
    /// - after `flush` without a `reset` in between;
    /// - if the chunk would run past the end of the stream timeline;
    /// - if `out` is shorter than [`max_frames_for_chunk`](Self::max_frames_for_chunk)`(chunk.len())`.
    ///
    /// `push_with_quality` returns these as errors instead.
    pub fn push(&mut self, chunk: &[f64], out: &mut [StreamFrame]) -> usize {
        #[cfg(test)]
        stream_hooks::begin_public_call();
        assert!(!self.flushed, "push after flush requires reset");
        self.check_timeline(chunk.len())
            .expect("stream timeline exhausted");
        assert!(out.len() >= Self::max_frames_for_chunk(chunk.len()));
        self.push_impl(chunk, out, None, push_hooks(chunk.len()))
    }

    /// Process with one quality record per output frame. Quality bits 1 and 2
    /// mean fault fallback and recovery respectively; neither is measurement
    /// validity. Both buffers require max_frames_for_chunk capacity. All usage
    /// checks precede mutation, including zero-length chunks.
    pub fn push_with_quality(
        &mut self,
        chunk: &[f64],
        out: &mut [StreamFrame],
        quality: &mut [FrameQualityV1],
    ) -> Result<usize, StreamUsageError> {
        #[cfg(test)]
        stream_hooks::begin_public_call();
        if self.flushed {
            return Err(StreamUsageError::AfterFlush);
        }
        self.check_timeline(chunk.len())?;
        let required = Self::max_frames_for_chunk(chunk.len());
        if out.len() < required {
            return Err(StreamUsageError::OutputCapacity);
        }
        if quality.len() < required {
            return Err(StreamUsageError::QualityCapacity);
        }
        self.check_environment()?;
        Ok(self.push_impl(chunk, out, Some(quality), push_hooks(chunk.len())))
    }

    fn check_timeline(&self, len: usize) -> Result<(), StreamUsageError> {
        let samples = u64::try_from(len).map_err(|_| StreamUsageError::TimelineExhausted)?;
        self.consumed_samples
            .checked_add(samples)
            .ok_or(StreamUsageError::TimelineExhausted)?;
        let slots = samples / DEC_FACTOR as u64 + u64::from(samples % DEC_FACTOR as u64 != 0);
        if !self.emitter.timeline_fits(slots) {
            return Err(StreamUsageError::TimelineExhausted);
        }
        Ok(())
    }

    fn check_environment(&self) -> Result<(), StreamUsageError> {
        for (environment, creation) in [
            (self.creation_environment, true),
            (read_rounding_environment(), false),
        ] {
            match environment {
                RoundingEnvironment::KnownRn => {}
                RoundingEnvironment::NonRn => {
                    return Err(StreamUsageError::UnsupportedRoundingMode { creation })
                }
                RoundingEnvironment::Unknown => {
                    return Err(StreamUsageError::UnsupportedEnvironment { creation })
                }
            }
        }
        Ok(())
    }

    fn push_impl<H: PushHooks>(
        &mut self,
        chunk: &[f64],
        out: &mut [StreamFrame],
        quality: Option<&mut [FrameQualityV1]>,
        mut hooks: H,
    ) -> usize {
        let _guard = DenormalGuard::new();
        if chunk.is_empty() {
            return 0;
        }
        let mut sink = FrameSink {
            out,
            quality,
            written: 0,
        };
        // A stream created outside round-to-nearest runs the whole call checked (R) on
        // kernel. Otherwise the first block's MXCSR read inside DenormalGuard also checks
        // qualification.
        let entry_mxcsr = if self.creation_environment == RoundingEnvironment::KnownRn {
            crate::numeric_guard::read_unit_mxcsr()
                .filter(|&m| crate::numeric_guard::is_mxcsr_qualified(m))
        } else {
            None
        };
        match entry_mxcsr {
            #[cfg(target_arch = "x86_64")]
            Some(mxcsr) => self.push_speculative(chunk, &mut sink, mxcsr, &mut hooks),
            #[cfg(not(target_arch = "x86_64"))]
            Some(_) => self.push_checked(chunk, &mut sink, &mut hooks),
            None => {
                hooks.on_env_fallback();
                self.push_checked(chunk, &mut sink, &mut hooks);
            }
        }
        sink.written
    }

    /// The whole call through the checked per-sample controller on `kernel`, split at
    /// hook samples.
    fn push_checked<H: PushHooks>(
        &mut self,
        chunk: &[f64],
        sink: &mut FrameSink<'_>,
        hooks: &mut H,
    ) {
        let mut pos = 0usize;
        while pos < chunk.len() {
            if hooks.next_sample() == Some(pos) {
                hooks.apply_at(
                    pos,
                    &mut self.tol.kernel,
                    self.tol.sample_phase,
                    &mut self.emitter,
                    self.consumed_samples,
                    None,
                );
            }
            let end = hooks.next_sample().unwrap_or(chunk.len());
            debug_assert!(end > pos && end <= chunk.len());
            push_checked_range_on_kernel(
                &mut self.tol.kernel,
                &mut self.tol.sample_phase,
                &mut self.emitter,
                &mut self.consumed_samples,
                &chunk[pos..end],
                sink,
            );
            pos = end;
        }
        hooks.finish();
    }

    /// Qualified (D0) path: blocks of up to `STREAM_BLOCK_LEN` samples, each computed
    /// speculatively on `shadow` and swapped in when it passes (terms: see
    /// `crate::numeric_guard`).
    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    fn push_speculative<H: PushHooks>(
        &mut self,
        chunk: &[f64],
        sink: &mut FrameSink<'_>,
        mut block_entry_mxcsr: u32,
        hooks: &mut H,
    ) {
        // Due rule a: all due flags start true at every public push entry.
        let mut due = Due::all_true();
        let mut pos = 0usize;
        let mut have_block_mxcsr = true;
        let mut emissions = [SpecEmission::ZERO; STREAM_MAX_EMISSIONS];

        while pos < chunk.len() {
            if hooks.next_sample() == Some(pos) {
                hooks.apply_at(
                    pos,
                    &mut self.tol.kernel,
                    self.tol.sample_phase,
                    &mut self.emitter,
                    self.consumed_samples,
                    Some(&mut due),
                );
            }
            let end = (pos + STREAM_BLOCK_LEN)
                .min(chunk.len())
                .min(hooks.next_sample().unwrap_or(chunk.len()));
            if !have_block_mxcsr {
                block_entry_mxcsr = crate::numeric_guard::read_unit_mxcsr().unwrap_or(0);
            }
            have_block_mxcsr = false;
            let block = BlockSpan {
                pos,
                end,
                entry_mxcsr: block_entry_mxcsr,
            };
            pos = self.push_block(chunk, block, &mut due, &mut emissions, sink, hooks);
        }
        hooks.finish();
    }

    /// One block `chunk[block.pos..block.end]`; returns where the next block starts.
    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    fn push_block<H: PushHooks>(
        &mut self,
        chunk: &[f64],
        block: BlockSpan,
        due: &mut Due,
        emissions: &mut [SpecEmission; STREAM_MAX_EMISSIONS],
        sink: &mut FrameSink<'_>,
        hooks: &mut H,
    ) -> usize {
        let BlockSpan {
            pos,
            end,
            entry_mxcsr,
        } = block;
        let samples = &chunk[pos..end];

        // When due.tol is true, validate kernel before
        // reading raw PCM, modifying pending, or touching shadow.
        if due.tol {
            if self.tol.kernel.validate_numeric().is_err() {
                hooks.on_entry_due_replay();
                hooks.check_none_inside(pos, end, "entry-due replay");
                push_checked_range_on_kernel(
                    &mut self.tol.kernel,
                    &mut self.tol.sample_phase,
                    &mut self.emitter,
                    &mut self.consumed_samples,
                    samples,
                    sink,
                );
                // Due rule b: cold path end sets all due flags true.
                *due = Due::all_true();
                return end;
            }
            hooks.on_entry_due_passed();
            due.tol = false;
        }

        let entry_ordinal = self.consumed_samples;
        hooks.on_spec_block();
        let outcome = tol_stage::advance_block_spec(
            &self.tol.kernel,
            &mut self.tol.shadow,
            samples,
            self.tol.sample_phase,
            entry_ordinal,
            emissions,
        );

        if !outcome.ok {
            // D1/D2 failure: do NOT swap; restore block entry MXCSR and replay R checked
            // directly on kernel. Cold-path rule: shadow is NOT passed.
            hooks.check_none_inside(pos, end, "cold replay");
            push_cold_replay_with_mxcsr_restore(
                entry_mxcsr,
                &mut self.tol.kernel,
                &mut self.tol.sample_phase,
                &mut self.emitter,
                &mut self.consumed_samples,
                samples,
                sink,
            );
            // Due rule b: cold replay end sets all due flags true.
            *due = Due::all_true();
            return end;
        }

        // D1/D2 passed: swap kernel <-> shadow and install tail phase.
        std::mem::swap(&mut self.tol.kernel, &mut self.tol.shadow);
        self.tol.sample_phase = outcome.end_phase;
        hooks.on_swap();

        let delivered = &emissions[..outcome.emission_count];
        match self.deliver_emissions(delivered, &mut due.downstream, sink, hooks) {
            Some(restart_ordinal) => {
                // All-zero reset TOL does not require TOL entry validation .
                due.tol = false;
                pos + (restart_ordinal - entry_ordinal) as usize
            }
            None => {
                if outcome.trailing_nonfinite {
                    self.emitter.raise(FrameFlags::NONFINITE_INPUT);
                }
                self.consumed_samples = entry_ordinal + samples.len() as u64;
                end
            }
        }
    }

    /// Deliver a block's buffered emissions in order. A downstream fault
    /// discards the rest and resets both TOL kernels; the returned ordinal
    /// (the sample after the faulting emission) is where the next block starts.
    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    fn deliver_emissions<H: PushHooks>(
        &mut self,
        emissions: &[SpecEmission],
        due: &mut DownstreamDue,
        sink: &mut FrameSink<'_>,
        hooks: &mut H,
    ) -> Option<u64> {
        for emission in emissions {
            if emission.nonfinite_since_prev {
                self.emitter.raise(FrameFlags::NONFINITE_INPUT);
            }
            let (out, quality) = sink.rest();
            let emitted = self.emitter.on_levels_d0(
                emission.levels,
                emission.sample_ordinal,
                *due,
                out,
                quality,
            );
            *due = DownstreamDue::NONE;
            sink.written += emitted.written;
            if emitted.transaction {
                // Downstream fault -> discard all subsequent
                // speculative TOL/emission/nonfinite records, zero BOTH kernel and shadow,
                // set phase = (0 + 1) % DEC_FACTOR = 1, consumed = e.ordinal + 1,
                // and restart a new block from the next sample.
                self.tol.kernel.reset();
                self.tol.shadow.reset();
                self.tol.sample_phase = 1;
                self.consumed_samples = emission.sample_ordinal + 1;
                hooks.on_downstream_restart();
                return Some(emission.sample_ordinal + 1);
            }
        }
        None
    }

    /// Finish the held slot (see `flush_with_quality`); returns the number of
    /// frames written to `out`.
    ///
    /// # Panics
    ///
    /// - if `out` is empty;
    /// - if the stream timeline is exhausted.
    ///
    /// `flush_with_quality` returns these as errors instead.
    pub fn flush(&mut self, out: &mut [StreamFrame]) -> usize {
        #[cfg(test)]
        stream_hooks::begin_public_call();
        assert!(!out.is_empty());
        self.check_timeline(0).expect("stream timeline exhausted");
        self.flush_impl(out, None)
    }

    /// Finish the existing held slot without inventing input samples. The
    /// artificial successor does not advance the 2320-clean-tick recovery.
    /// Both output buffers require at least one slot, even after prior flush.
    pub fn flush_with_quality(
        &mut self,
        out: &mut [StreamFrame],
        quality: &mut [FrameQualityV1],
    ) -> Result<usize, StreamUsageError> {
        #[cfg(test)]
        stream_hooks::begin_public_call();
        self.check_timeline(0)?;
        if out.is_empty() {
            return Err(StreamUsageError::OutputCapacity);
        }
        if quality.is_empty() {
            return Err(StreamUsageError::QualityCapacity);
        }
        self.check_environment()?;
        Ok(self.flush_impl(out, Some(quality)))
    }

    fn flush_impl(
        &mut self,
        out: &mut [StreamFrame],
        quality: Option<&mut [FrameQualityV1]>,
    ) -> usize {
        let _guard = DenormalGuard::new();
        #[cfg(test)]
        stream_hooks::assert_no_hooks_for_active_call();
        if self.flushed {
            return 0;
        }
        self.flushed = true;
        let mut transaction = false;
        if let Err((band, reason)) = self.tol.validate_numeric() {
            self.filter_fault(band, reason, &mut transaction);
        }
        let written = self
            .emitter
            .finish(self.consumed_samples, &mut transaction, out, quality);
        if transaction {
            self.tol.reset_numeric();
        }
        written
    }

    fn filter_fault(&mut self, band: usize, reason: DomainFault, transaction: &mut bool) {
        filter_fault_on_kernel(
            &self.tol.kernel,
            &mut self.emitter,
            self.consumed_samples,
            band,
            reason,
            transaction,
        );
    }

    /// Return pending flags observed after the most recent output frame.
    /// Before flush the value is provisional and will be attached to the next
    /// output frame. Only after flush does it represent undelivered tail events.
    pub const fn residual_flags(&self) -> FrameFlags {
        self.emitter.pending()
    }
}

fn filter_fault_on_kernel(
    kernel: &TolKernel,
    emitter: &mut FrameEmitter,
    consumed_samples: u64,
    band: usize,
    reason: DomainFault,
    transaction: &mut bool,
) {
    let reasons = kernel.numeric_reason_mask();
    emitter.fault(consumed_samples, (1, band, reason), transaction);
    emitter.diagnostics.merge_incident_masks(1, reasons);
}

/// Cold/fallback R per-sample checked controller operating strictly on `kernel` alone.
/// Never receives `&mut ZwtvStream`, `&mut TolStage`, or any reference to `shadow`.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn push_checked_range_on_kernel(
    kernel: &mut TolKernel,
    sample_phase: &mut usize,
    emitter: &mut FrameEmitter,
    consumed_samples: &mut u64,
    sub: &[f64],
    sink: &mut FrameSink<'_>,
) {
    for &raw in sub {
        let sample = if raw.is_finite() {
            raw
        } else {
            emitter.raise(FrameFlags::NONFINITE_INPUT);
            0.0
        };
        let mut transaction = false;
        if let Err((band, reason)) = kernel.validate_numeric() {
            filter_fault_on_kernel(
                kernel,
                emitter,
                *consumed_samples,
                band,
                reason,
                &mut transaction,
            );
            kernel.reset();
        }
        let emitting = *sample_phase == 0;
        let levels = match kernel.advance_sample_checked(sample, emitting) {
            Ok(value) => value,
            Err((band, reason)) => {
                filter_fault_on_kernel(
                    kernel,
                    emitter,
                    *consumed_samples,
                    band,
                    reason,
                    &mut transaction,
                );
                kernel.reset();
                emitting.then_some([0.0; 28])
            }
        };
        if let Some(levels) = levels {
            let was_fault = transaction;
            let (out, quality) = sink.rest();
            sink.written +=
                emitter.on_levels(levels, *consumed_samples, &mut transaction, out, quality);
            if transaction && !was_fault {
                kernel.reset();
            }
        }
        *sample_phase = (*sample_phase + 1) % DEC_FACTOR;
        *consumed_samples += 1;
    }
}

/// Cold replay after D1/D2 failure: restores block-entry MXCSR before the first FP op of replay,
/// then runs R's checked controller on `kernel`. Never receives `shadow`.
#[cfg(target_arch = "x86_64")]
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn push_cold_replay_with_mxcsr_restore(
    entry_mxcsr: u32,
    kernel: &mut TolKernel,
    sample_phase: &mut usize,
    emitter: &mut FrameEmitter,
    consumed_samples: &mut u64,
    sub: &[f64],
    sink: &mut FrameSink<'_>,
) {
    #[cfg(test)]
    test_stream_hooks::record_cold_replay();
    crate::numeric_guard::restore_mxcsr(entry_mxcsr);
    push_checked_range_on_kernel(kernel, sample_phase, emitter, consumed_samples, sub, sink);
}

fn map_diagnostic_error(error: diagnostics::DiagnosticError) -> StreamUsageError {
    match error {
        diagnostics::DiagnosticError::StaleSequence => StreamUsageError::StaleSequence,
        diagnostics::DiagnosticError::CounterSaturated => StreamUsageError::CounterSaturated,
        diagnostics::DiagnosticError::SessionExhausted => StreamUsageError::SessionExhausted,
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn apply_stream_hooks_at_boundary(
    cursor: &mut stream_hooks::HookCursor,
    sample: usize,
    kernel: &mut TolKernel,
    sample_phase: usize,
    emitter: &mut FrameEmitter,
    consumed_samples: u64,
    #[cfg(target_arch = "x86_64")] mut due: Option<&mut Due>,
    #[cfg(not(target_arch = "x86_64"))] _due: Option<&mut ()>,
) {
    let call_id = cursor.call_id();
    let _ = (call_id, sample_phase, consumed_samples);
    for &ev in cursor.drain_at(sample) {
        match ev.stage {
            1 => {
                kernel.inject_raw_bits(ev.band, ev.field, ev.raw_u64);
                #[cfg(target_arch = "x86_64")]
                if let Some(d) = due.as_deref_mut() {
                    d.tol = true;
                }
            }
            2 => {
                emitter.inject_numeric(2, ev.band, ev.field, f64::from_bits(ev.raw_u64));
                #[cfg(target_arch = "x86_64")]
                if let Some(d) = due.as_deref_mut() {
                    d.downstream.held = true;
                }
            }
            3 => {
                emitter.inject_numeric(3, ev.band, ev.field, f64::from_bits(ev.raw_u64));
                #[cfg(target_arch = "x86_64")]
                if let Some(d) = due.as_deref_mut() {
                    d.downstream.nl = true;
                }
            }
            5 => {
                emitter.inject_numeric(5, ev.band, ev.field, f64::from_bits(ev.raw_u64));
                #[cfg(target_arch = "x86_64")]
                if let Some(d) = due.as_deref_mut() {
                    d.downstream.tw = true;
                }
            }
            other => panic!("unsupported stream hook stage: {other}"),
        }
        stream_hooks::record_executed_write();
    }
    #[cfg(target_arch = "x86_64")]
    if let Some(d) = due.as_deref() {
        stream_hooks::record_due(
            call_id,
            sample,
            stream_hooks::DueSnapshot {
                tol: d.tol,
                held: d.downstream.held,
                nl: d.downstream.nl,
                tw: d.downstream.tw,
            },
        );
    }
}

/// Bounds of one speculative block within the call, and the MXCSR read at its entry.
#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy)]
struct BlockSpan {
    pos: usize,
    end: usize,
    entry_mxcsr: u32,
}

#[cfg(target_arch = "x86_64")]
type DueSlot<'a> = Option<&'a mut Due>;
#[cfg(not(target_arch = "x86_64"))]
type DueSlot<'a> = Option<&'a mut ()>;

/// Observation points of one public push call. The product passes
/// [`NoPushHooks`], whose methods are empty and inlined, so the product build
/// carries no test state; unit tests pass the hook cursor, which writes
/// scheduled state at block boundaries and counts the paths taken.
trait PushHooks {
    /// Sample index within this call where the next scheduled hook fires.
    fn next_sample(&self) -> Option<usize>;
    /// Apply the hooks scheduled at `pos`, setting the matching due flags.
    fn apply_at(
        &mut self,
        pos: usize,
        kernel: &mut TolKernel,
        sample_phase: usize,
        emitter: &mut FrameEmitter,
        consumed_samples: u64,
        due: DueSlot<'_>,
    );
    /// A replay over `[pos..end)` must not straddle a scheduled hook.
    fn check_none_inside(&self, pos: usize, end: usize, what: &str);
    fn on_env_fallback(&self);
    fn on_entry_due_replay(&self);
    fn on_entry_due_passed(&self);
    fn on_spec_block(&self);
    fn on_swap(&self);
    fn on_downstream_restart(&self);
    /// End of the call: every scheduled hook was applied.
    fn finish(&self);
}

/// Product hooks: nothing scheduled, nothing recorded. Passed by value: a
/// `&mut` to it would still be a pointer argument.
struct NoPushHooks;

impl PushHooks for NoPushHooks {
    #[inline(always)]
    fn next_sample(&self) -> Option<usize> {
        None
    }
    #[inline(always)]
    fn apply_at(
        &mut self,
        _: usize,
        _: &mut TolKernel,
        _: usize,
        _: &mut FrameEmitter,
        _: u64,
        _: DueSlot<'_>,
    ) {
    }
    #[inline(always)]
    fn check_none_inside(&self, _: usize, _: usize, _: &str) {}
    #[inline(always)]
    fn on_env_fallback(&self) {}
    #[inline(always)]
    fn on_entry_due_replay(&self) {}
    #[inline(always)]
    fn on_entry_due_passed(&self) {}
    #[inline(always)]
    fn on_spec_block(&self) {}
    #[inline(always)]
    fn on_swap(&self) {}
    #[inline(always)]
    fn on_downstream_restart(&self) {}
    #[inline(always)]
    fn finish(&self) {}
}

#[cfg(test)]
impl PushHooks for stream_hooks::HookCursor {
    fn next_sample(&self) -> Option<usize> {
        self.peek_sample()
    }
    fn apply_at(
        &mut self,
        pos: usize,
        kernel: &mut TolKernel,
        sample_phase: usize,
        emitter: &mut FrameEmitter,
        consumed_samples: u64,
        due: DueSlot<'_>,
    ) {
        apply_stream_hooks_at_boundary(
            self,
            pos,
            kernel,
            sample_phase,
            emitter,
            consumed_samples,
            due,
        );
    }
    fn check_none_inside(&self, pos: usize, end: usize, what: &str) {
        assert!(
            self.peek_sample().is_none_or(|s| s >= end),
            "STOP CONDITION: hook at {:?} fell inside {what} interval [{}..{})",
            self.peek_sample(),
            pos,
            end
        );
    }
    fn on_env_fallback(&self) {
        test_stream_hooks::record_env_fallback();
    }
    fn on_entry_due_replay(&self) {
        #[cfg(target_arch = "x86_64")]
        test_stream_hooks::record_entry_due_replay();
    }
    fn on_entry_due_passed(&self) {
        #[cfg(target_arch = "x86_64")]
        test_stream_hooks::record_entry_due_passed();
    }
    fn on_spec_block(&self) {
        #[cfg(target_arch = "x86_64")]
        test_stream_hooks::record_spec_block();
    }
    fn on_swap(&self) {
        #[cfg(target_arch = "x86_64")]
        test_stream_hooks::record_swap();
    }
    fn on_downstream_restart(&self) {
        #[cfg(target_arch = "x86_64")]
        test_stream_hooks::record_downstream_restart();
    }
    fn finish(&self) {
        debug_assert!(
            self.is_exhausted(),
            "unconsumed stream hooks remained after push"
        );
    }
}

/// Hooks for one public push call: the scheduled cursor in unit tests, none in the product.
#[cfg(test)]
fn push_hooks(chunk_len: usize) -> stream_hooks::HookCursor {
    stream_hooks::HookCursor::for_call(chunk_len)
}

#[cfg(not(test))]
#[inline(always)]
fn push_hooks(_chunk_len: usize) -> NoPushHooks {
    NoPushHooks
}

#[cfg(test)]
mod containment_tests;

#[cfg(test)]
#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "unsafe blocks from 0.1.0 without SAFETY comments; remove once documented"
)]
mod tests {
    use super::*;

    fn run(stream: &mut ZwtvStream, signal: &[f64]) -> Vec<StreamFrame> {
        let mut out = vec![StreamFrame::default(); ZwtvStream::max_frames_for_chunk(480)];
        let mut frames = Vec::new();
        for chunk in signal.chunks(480) {
            let k = stream.push(chunk, &mut out);
            frames.extend_from_slice(&out[..k]);
        }
        let k = stream.flush(&mut out);
        frames.extend_from_slice(&out[..k]);
        frames
    }

    #[test]
    fn reset_matches_a_new_stream_on_every_backend() {
        let signal: Vec<f64> = (0..9_600)
            .map(|i| (i as f64 * 0.13).sin() * 0.05 * f64::from(i % 2400 < 1200))
            .collect();
        let backends = [
            false,
            cfg!(target_arch = "x86_64") && crate::simd::use_avx2(),
        ];
        for avx2 in backends {
            let fresh = run(
                &mut ZwtvStream::with_backend(FieldType::Free, avx2),
                &signal,
            );
            let mut reused = ZwtvStream::with_backend(FieldType::Free, avx2);
            run(&mut reused, &signal[..5_000]);
            reused.reset();
            assert_eq!(run(&mut reused, &signal), fresh, "avx2={avx2}");
            assert_eq!(ZwtvStream::latency_samples(), 24);
        }
    }

    /// The product hooks and the test cursor with nothing scheduled take the
    /// same paths: same frames, same quality records.
    #[test]
    fn product_hooks_match_an_empty_hook_schedule() {
        let mut signal: Vec<f64> = (0..9_600).map(|i| (i as f64 * 0.07).sin() * 0.2).collect();
        signal[3_001] = f64::NAN;
        let backends = [
            false,
            cfg!(target_arch = "x86_64") && crate::simd::use_avx2(),
        ];
        let cap = ZwtvStream::max_frames_for_chunk(480);
        for avx2 in backends {
            let mut product = ZwtvStream::with_backend(FieldType::Free, avx2);
            let mut hooked = ZwtvStream::with_backend(FieldType::Free, avx2);
            for chunk in signal.chunks(480) {
                let (mut out_a, mut out_b) = (
                    vec![StreamFrame::default(); cap],
                    vec![StreamFrame::default(); cap],
                );
                let (mut q_a, mut q_b) = (
                    vec![FrameQualityV1::default(); cap],
                    vec![FrameQualityV1::default(); cap],
                );
                let k_a = product.push_impl(chunk, &mut out_a, Some(&mut q_a), NoPushHooks);
                let cursor = stream_hooks::HookCursor::for_call(chunk.len());
                let k_b = hooked.push_impl(chunk, &mut out_b, Some(&mut q_b), cursor);
                assert_eq!(k_a, k_b, "avx2={avx2}");
                assert_eq!(&out_a[..k_a], &out_b[..k_b], "avx2={avx2}");
                assert_eq!(&q_a[..k_a], &q_b[..k_b], "avx2={avx2}");
            }
        }
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

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn order3_dual_buffer_alignment_coeffs_swap_cold_reset_and_env_dispatch() {
        assert_eq!(std::mem::size_of::<SpecEmission>(), 240);
        assert_eq!(std::mem::align_of::<SpecEmission>(), 8);

        let _restore = MxcsrGuard::capture();
        let has_avx2 =
            std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
        let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };

        let sig_norm: Vec<f64> = (0..1000)
            .map(|i| ((i as f64) * std::f64::consts::TAU * 1000.0 / 48000.0).sin() * 0.02)
            .collect();
        let mut sig_fault = sig_norm.clone();
        sig_fault[500] = f64::MAX;

        for &avx2 in backends {
            MxcsrGuard::set(0x1f80);
            let mut s = ZwtvStream::with_backend(FieldType::Free, avx2);
            let (k_ptr0, k_align) = s.tol.kernel_ptr_and_align();
            let (s_ptr0, s_align) = s.tol.shadow_ptr_and_align();
            let expected_align = if avx2 { 32 } else { 8 };
            assert_eq!(k_align, expected_align, "avx2={avx2}");
            assert_eq!(s_align, expected_align, "avx2={avx2}");
            assert_eq!(k_ptr0 % expected_align, 0, "kernel misaligned avx2={avx2}");
            assert_eq!(s_ptr0 % expected_align, 0, "shadow misaligned avx2={avx2}");
            assert_ne!(k_ptr0, s_ptr0, "kernel and shadow must not alias");

            let coeffs0 = s.tol.kernel_coeff_bits();
            assert_eq!(s.tol.shadow_coeff_bits(), coeffs0);
            assert_eq!(s.tol.kernel_numeric_bits(), [0u64; 252]);
            assert_eq!(s.tol.shadow_numeric_bits(), [0u64; 252]);

            // 1. Normal 1000-sample push -> 3 blocks (480 + 480 + 40), 3 swaps, 0 cold replays
            let mut out = vec![StreamFrame::default(); ZwtvStream::max_frames_for_chunk(1000)];
            let mut qual = vec![FrameQualityV1::default(); out.len()];
            test_stream_hooks::reset();
            let w = s.push_with_quality(&sig_norm, &mut out, &mut qual).unwrap();
            assert!(w > 0);
            let c = test_stream_hooks::snapshot();
            assert_eq!(c.spec_blocks, 3, "avx2={avx2}");
            assert_eq!(c.swaps, 3, "avx2={avx2}");
            assert_eq!(c.cold_replays, 0, "avx2={avx2}");
            assert_eq!(c.env_fallbacks, 0, "avx2={avx2}");
            // Odd number of swaps (3) swaps the two Box addresses without allocating
            let (k_ptr1, _) = s.tol.kernel_ptr_and_align();
            let (s_ptr1, _) = s.tol.shadow_ptr_and_align();
            assert_eq!(k_ptr1, s_ptr0);
            assert_eq!(s_ptr1, k_ptr0);
            assert_eq!(s.tol.kernel_coeff_bits(), coeffs0);
            assert_eq!(s.tol.shadow_coeff_bits(), coeffs0);

            // 2. Reset zeros BOTH kernel and shadow without changing coeffs or allocating
            s.reset();
            assert_eq!(s.tol.kernel_numeric_bits(), [0u64; 252]);
            assert_eq!(s.tol.shadow_numeric_bits(), [0u64; 252]);
            assert_eq!(s.tol.kernel_coeff_bits(), coeffs0);
            assert_eq!(s.tol.shadow_coeff_bits(), coeffs0);

            // 3. TOL fault in middle block (sample 500 -> block [480, 960)) triggers 1 cold replay (no swap on block 1)
            let (k_before, _) = s.tol.kernel_ptr_and_align();
            let (s_before, _) = s.tol.shadow_ptr_and_align();
            test_stream_hooks::reset();
            let _ = s
                .push_with_quality(&sig_fault, &mut out, &mut qual)
                .unwrap();
            let c = test_stream_hooks::snapshot();
            assert_eq!(c.spec_blocks, 3, "avx2={avx2}");
            assert_eq!(c.swaps, 2, "block 0 and block 2 swap; block 1 cold-replays");
            assert_eq!(c.cold_replays, 1, "block 1 cold-replays");
            // Even number of swaps (2) returns handles to original pointers
            assert_eq!(s.tol.kernel_ptr_and_align().0, k_before);
            assert_eq!(s.tol.shadow_ptr_and_align().0, s_before);
            assert_eq!(s.tol.kernel_coeff_bits(), coeffs0);
            assert_eq!(s.tol.shadow_coeff_bits(), coeffs0);
            assert_eq!(s.diagnostics().incident_count, 1);

            // 4. Non-RN creation -> legacy push runs entire call on R fallback (0 spec_blocks)
            MxcsrGuard::set(0x7f80);
            let mut s_nonrn = ZwtvStream::with_backend(FieldType::Free, avx2);
            MxcsrGuard::set(0x1f80);
            test_stream_hooks::reset();
            let _ = s_nonrn.push(&sig_norm[..480], &mut out);
            let c = test_stream_hooks::snapshot();
            assert_eq!(c.env_fallbacks, 1, "non-RN creation must fall back to R");
            assert_eq!(c.spec_blocks, 0);
            assert_eq!(c.swaps, 0);

            // 5. Caller non-RN (RZ) or unmasked exception (0x1b80) -> legacy push falls back to R
            MxcsrGuard::set(0x1f80);
            let mut s_env = ZwtvStream::with_backend(FieldType::Free, avx2);
            for bad_csr in [0x7f80u32, 0x3f80, 0x5f80, 0x1b80] {
                test_stream_hooks::reset();
                MxcsrGuard::set(bad_csr);
                let _ = s_env.push(&sig_norm[..120], &mut out);
                let after = MxcsrGuard::capture().0;
                MxcsrGuard::set(0x1f80);
                assert_eq!(after, bad_csr, "DenormalGuard must restore caller MXCSR");
                let c = test_stream_hooks::snapshot();
                assert_eq!(c.env_fallbacks, 1, "bad_csr={bad_csr:#06x}");
                assert_eq!(c.spec_blocks, 0, "bad_csr={bad_csr:#06x}");
            }

            // 6. Empty push: 0 spec_blocks, 0 env_fallbacks, 0 swaps
            test_stream_hooks::reset();
            assert_eq!(s_env.push_with_quality(&[], &mut out, &mut qual), Ok(0));
            assert_eq!(
                test_stream_hooks::snapshot(),
                test_stream_hooks::StreamTolCounts::default()
            );
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn order4_downstream_restart_flush_and_due_ab() {
        let _restore = MxcsrGuard::capture();
        let has_avx2 =
            std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
        let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };

        let core_free: Vec<f64> = (0..400)
            .map(|i| ((i as f64) * std::f64::consts::TAU * 4000.0 / 48000.0).sin() * 4e149)
            .collect();
        let core_diff: Vec<f64> = (0..400)
            .map(|i| ((i as f64) * std::f64::consts::TAU * 6300.0 / 48000.0).sin() * 4e149)
            .collect();
        let mut core_nf = core_free.clone();
        core_nf[167] = f64::NAN;
        core_nf[168] = f64::INFINITY;
        core_nf[169] = f64::NEG_INFINITY;

        let run_chunks = |avx2: bool, field: FieldType, sig: &[f64], chunks: &[usize]| {
            MxcsrGuard::set(0x1f80);
            let mut s = ZwtvStream::with_backend(field, avx2);
            let mut frames = Vec::new();
            let mut quals = Vec::new();
            let mut pos = 0;
            for &clen in chunks {
                let end = (pos + clen).min(sig.len());
                let part = &sig[pos..end];
                let cap = ZwtvStream::max_frames_for_chunk(part.len());
                let mut out = vec![StreamFrame::default(); cap];
                let mut q = vec![FrameQualityV1::default(); cap];
                let w = s.push_with_quality(part, &mut out, &mut q).unwrap();
                frames.extend_from_slice(&out[..w]);
                quals.extend_from_slice(&q[..w]);
                pos = end;
            }
            let tol_before_flush = s.tol.kernel_numeric_bits();
            let shadow_before_flush = s.tol.shadow_numeric_bits();
            let phase_before_flush = s.tol.sample_phase;
            let consumed_before_flush = s.consumed_samples;
            let mut fout = [StreamFrame::default(); 2];
            let mut fq = [FrameQualityV1::default(); 2];
            let fw = s.flush_with_quality(&mut fout, &mut fq).unwrap();
            frames.extend_from_slice(&fout[..fw]);
            quals.extend_from_slice(&fq[..fw]);
            (
                frames,
                quals,
                s.diagnostics(),
                s.residual_flags(),
                tol_before_flush,
                shadow_before_flush,
                phase_before_flush,
                consumed_before_flush,
            )
        };

        for &avx2 in backends {
            // 1. All six downstream Core NonFinite cases (plus guaranteed-finite-suffix
            // first/diffuse/interleaved shapes that stay below TOL overflow through block end so
            // they exercise the speculative downstream restart path directly) must match
            // 1-sample chunking bit-for-bit.
            #[allow(clippy::type_complexity)]
            let cases: Vec<(&str, FieldType, &[f64], Vec<usize>, bool)> = vec![
                (
                    "pos_first",
                    FieldType::Free,
                    &core_free[..168 + 49],
                    vec![168, 49],
                    false,
                ),
                (
                    "pos_first_restart",
                    FieldType::Free,
                    &core_free[..168 + 25],
                    vec![168, 25],
                    true,
                ),
                (
                    "pos_mid",
                    FieldType::Free,
                    &core_free[..144 + 49],
                    vec![144, 49],
                    true,
                ),
                (
                    "pos_last",
                    FieldType::Free,
                    &core_free[..120 + 49],
                    vec![120, 49],
                    true,
                ),
                (
                    "last_tail",
                    FieldType::Free,
                    &core_free[..120 + 60],
                    vec![120, 60],
                    true,
                ),
                (
                    "diffuse",
                    FieldType::Diffuse,
                    &core_diff[..],
                    vec![400],
                    false,
                ),
                (
                    "diffuse_restart",
                    FieldType::Diffuse,
                    &core_diff[..144 + 49],
                    vec![144, 49],
                    true,
                ),
                (
                    "interleaved_nonfinite",
                    FieldType::Free,
                    &core_nf[..],
                    vec![400],
                    false,
                ),
                (
                    "interleaved_nonfinite_restart",
                    FieldType::Free,
                    &core_nf[..144 + 49],
                    vec![144, 49],
                    true,
                ),
            ];
            for (tag, field, sig, chunks, must_restart_spec) in cases {
                let single_chunks = vec![1usize; sig.len()];
                let ref1 = run_chunks(avx2, field, sig, &single_chunks);
                test_stream_hooks::reset();
                let cand = run_chunks(avx2, field, sig, &chunks);
                let cnt = test_stream_hooks::snapshot();
                if must_restart_spec {
                    assert!(
                        cnt.downstream_restarts > 0,
                        "expected speculative downstream_restarts > 0 on {tag} avx2={avx2}, got {cnt:?}"
                    );
                } else {
                    assert!(
                        cnt.downstream_restarts > 0 || cnt.cold_replays > 0,
                        "expected downstream restart or cold replay on {tag} avx2={avx2}, got {cnt:?}"
                    );
                }
                assert_eq!(cand.0, ref1.0, "frames mismatch on {tag} avx2={avx2}");
                assert_eq!(cand.1, ref1.1, "quality mismatch on {tag} avx2={avx2}");
                assert_eq!(cand.2, ref1.2, "diagnostics mismatch on {tag} avx2={avx2}");
                assert_eq!(cand.3, ref1.3, "residual mismatch on {tag} avx2={avx2}");
                assert_eq!(
                    cand.4, ref1.4,
                    "tol kernel bits mismatch on {tag} avx2={avx2}"
                );
                assert_eq!(cand.6, ref1.6, "phase mismatch on {tag} avx2={avx2}");
                assert_eq!(cand.7, ref1.7, "consumed mismatch on {tag} avx2={avx2}");
                if tag == "pos_last" {
                    // Fault occurred on the very last sample (168): BOTH kernel and shadow must be all-zero
                    assert_eq!(
                        cand.4, [0u64; 252],
                        "pos_last kernel not zeroed avx2={avx2}"
                    );
                    assert_eq!(
                        cand.5, [0u64; 252],
                        "pos_last shadow not zeroed avx2={avx2}"
                    );
                    assert_eq!(cand.6, 1, "pos_last phase must be 1 avx2={avx2}");
                }
                if tag == "last_tail" {
                    // 11 tail samples [169, 180) advance from zeroed TOL -> non-zero kernel at call return
                    assert_ne!(
                        cand.4, [0u64; 252],
                        "last_tail must advance tail after reset"
                    );
                }
            }

            // 2. Due rules a & b (public call entry + cold replay end)
            MxcsrGuard::set(0x1f80);
            let mut s = ZwtvStream::with_backend(FieldType::Free, avx2);
            let sig_1000: Vec<f64> = (0..1000)
                .map(|i| ((i as f64) * std::f64::consts::TAU * 1000.0 / 48000.0).sin() * 0.02)
                .collect();
            let mut out = vec![StreamFrame::default(); ZwtvStream::max_frames_for_chunk(1000)];
            let mut q = vec![FrameQualityV1::default(); out.len()];

            // 3 clean blocks -> rule a checks TOL ONCE at block 0, skips blocks 1 and 2
            test_stream_hooks::reset();
            s.push_with_quality(&sig_1000, &mut out, &mut q).unwrap();
            let c = test_stream_hooks::snapshot();
            assert_eq!(c.spec_blocks, 3, "avx2={avx2}");
            assert_eq!(c.entry_due_passed, 1, "rule a: only block 0 checks due.tol");
            assert_eq!(c.entry_due_replays, 0);

            // Second public call -> rule a re-arms due.tol
            test_stream_hooks::reset();
            s.push_with_quality(&sig_1000[..480], &mut out, &mut q)
                .unwrap();
            let c = test_stream_hooks::snapshot();
            assert_eq!(c.entry_due_passed, 1, "rule a re-armed on next public call");

            // Cold replay in block 1 -> rule b re-arms due.tol for block 2 (2 entry checks total: block 0 + block 2)
            s.reset();
            let mut sig_mid_fault = sig_1000.clone();
            sig_mid_fault[500] = f64::MAX;
            test_stream_hooks::reset();
            s.push_with_quality(&sig_mid_fault, &mut out, &mut q)
                .unwrap();
            let c = test_stream_hooks::snapshot();
            assert_eq!(c.cold_replays, 1, "avx2={avx2}");
            assert_eq!(
                c.entry_due_passed, 2,
                "rule a (block 0) + rule b after cold replay (block 2)"
            );

            // Poisoned TOL injected before multi-block push: block 0 triggers entry_due_replays (rule a)
            // without speculating, then rule b re-arms due.tol for block 1 (entry_due_passed == 1).
            s.reset();
            match &mut s.tol.kernel {
                TolKernel::Scalar(st) => st[0].inject_nan_for_test(),
                TolKernel::Avx2(gr) => gr[0].inject_nan_for_test(),
            }
            test_stream_hooks::reset();
            s.push_with_quality(&sig_1000, &mut out, &mut q).unwrap();
            let c = test_stream_hooks::snapshot();
            assert_eq!(
                c.entry_due_replays, 1,
                "block 0 routed directly to R on due.tol failure"
            );
            assert_eq!(
                c.entry_due_passed, 1,
                "rule b re-arms due.tol for block 1 (block 2 skips)"
            );
            assert_eq!(c.spec_blocks, 2, "blocks 1 and 2 speculate");
            assert_eq!(s.diagnostics().incident_count, 1);

            // 3. Flush stays R checked across all 4 rounding modes & non-RN creation; already-flushed early return
            // does NOT run TOL validation even when TOL is poisoned; quality rejection precedence is preserved.
            for rm_csr in [0x1f80u32, 0x3f80, 0x5f80, 0x7f80] {
                MxcsrGuard::set(rm_csr);
                let mut sf = ZwtvStream::with_backend(FieldType::Free, avx2);
                let mut fout = [StreamFrame::default(); 4];
                let mut fq = [FrameQualityV1::default(); 4];
                let _ = sf.push(&sig_1000[..48], &mut fout);
                if rm_csr != 0x1f80 {
                    // quality flush rejects OutputCapacity -> QualityCapacity -> UnsupportedRoundingMode { creation: true }
                    assert_eq!(
                        sf.flush_with_quality(&mut [], &mut fq),
                        Err(StreamUsageError::OutputCapacity)
                    );
                    assert_eq!(
                        sf.flush_with_quality(&mut fout, &mut []),
                        Err(StreamUsageError::QualityCapacity)
                    );
                    assert_eq!(
                        sf.flush_with_quality(&mut fout, &mut fq),
                        Err(StreamUsageError::UnsupportedRoundingMode { creation: true })
                    );
                }
                test_stream_hooks::reset();
                MxcsrGuard::set(rm_csr);
                let fw = sf.flush(&mut fout);
                let after = MxcsrGuard::capture().0;
                assert_eq!(after, rm_csr, "flush must restore caller MXCSR");
                assert_eq!(
                    test_stream_hooks::snapshot(),
                    test_stream_hooks::StreamTolCounts::default(),
                    "flush must not enter speculative TOL path"
                );
                assert!(fw <= 1);
                // Second flush after already-flushed returns 0 without new incident even when TOL is poisoned!
                let inc_before = sf.diagnostics().incident_count;
                match &mut sf.tol.kernel {
                    TolKernel::Scalar(st) => st[0].inject_nan_for_test(),
                    TolKernel::Avx2(gr) => gr[0].inject_nan_for_test(),
                }
                let fw2 = sf.flush(&mut fout);
                assert_eq!(fw2, 0);
                assert_eq!(sf.diagnostics().incident_count, inc_before);
                if rm_csr == 0x1f80 {
                    assert_eq!(sf.flush_with_quality(&mut fout, &mut fq), Ok(0));
                    assert_eq!(sf.diagnostics().incident_count, inc_before);
                }
            }
            MxcsrGuard::set(0x1f80);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn ac2_d1_catches_post_last_emission_tol_fault() {
        let _restore = MxcsrGuard::capture();
        let has_avx2 =
            std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
        let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };

        for &avx2 in backends {
            MxcsrGuard::set(0x1f80);
            // 1. len=24, overflow at sample 23 (after last emission at sample 0)
            let mut s1 = ZwtvStream::with_backend(FieldType::Free, avx2);
            let mut sig1 = vec![0.01f64; 24];
            sig1[23] = f64::MAX;
            let mut out = vec![StreamFrame::default(); ZwtvStream::max_frames_for_chunk(504)];
            let mut q = vec![FrameQualityV1::default(); out.len()];
            s1.push_with_quality(&sig1, &mut out, &mut q).unwrap();
            let d1 = s1.diagnostics();
            assert_eq!(d1.incident_count, 1, "avx2={avx2}");
            assert_eq!(d1.first_stage, 1, "avx2={avx2}");
            assert_eq!(d1.first_detection_sample, 23, "avx2={avx2}");

            // 2. len=504 (2 blocks [0,480) + [480,504)), overflow at sample 479
            let mut s2 = ZwtvStream::with_backend(FieldType::Free, avx2);
            let mut sig2 = vec![0.01f64; 504];
            sig2[479] = f64::MAX;
            s2.push_with_quality(&sig2, &mut out, &mut q).unwrap();
            let d2 = s2.diagnostics();
            assert_eq!(d2.incident_count, 1, "avx2={avx2}");
            assert_eq!(d2.first_stage, 1, "avx2={avx2}");
            assert_eq!(d2.first_detection_sample, 479, "avx2={avx2}");

            // 3. No-emission block starting at phase=1: push(1) + push(10 with fault at sample 6)
            let mut s3 = ZwtvStream::with_backend(FieldType::Free, avx2);
            let mut sig3 = [0.01f64; 11];
            sig3[6] = f64::MAX;
            s3.push_with_quality(&sig3[..1], &mut out, &mut q).unwrap();
            s3.push_with_quality(&sig3[1..], &mut out, &mut q).unwrap();
            let d3 = s3.diagnostics();
            assert_eq!(d3.incident_count, 1, "avx2={avx2}");
            assert_eq!(d3.first_stage, 1, "avx2={avx2}");
            assert_eq!(d3.first_detection_sample, 6, "avx2={avx2}");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn ac3_downstream_restart_discards_trailing_nonfinite_before_earlier_frame() {
        let _restore = MxcsrGuard::capture();
        let has_avx2 =
            std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
        let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };

        let core_free: Vec<f64> = (0..169)
            .map(|i| ((i as f64) * std::f64::consts::TAU * 4000.0 / 48000.0).sin() * 4e149)
            .collect();
        let core_diff: Vec<f64> = (0..169)
            .map(|i| ((i as f64) * std::f64::consts::TAU * 6300.0 / 48000.0).sin() * 4e149)
            .collect();

        for &avx2 in backends {
            for (field, base) in [
                (FieldType::Free, &core_free),
                (FieldType::Diffuse, &core_diff),
            ] {
                MxcsrGuard::set(0x1f80);
                let mut sig = vec![0.01f64; 260];
                sig[..169].copy_from_slice(base);
                sig[250] = f64::NAN;
                let mut s = ZwtvStream::with_backend(field, avx2);
                let cap = ZwtvStream::max_frames_for_chunk(260);
                let mut out = vec![StreamFrame::default(); cap];
                let mut q = vec![FrameQualityV1::default(); cap];
                let w = s.push_with_quality(&sig, &mut out, &mut q).unwrap();
                assert_eq!(
                    w, 3,
                    "expected 3 public frames (0, 1, 2) for {field:?} avx2={avx2}"
                );
                // Frame 2 is emitted at sample 216, strictly BEFORE the NaN at sample 250!
                assert!(
                    !out[2].flags.contains(FrameFlags::NONFINITE_INPUT),
                    "trailing NaN at 250 leaked onto frame 2 (sample 216): {:?}",
                    out[2].flags
                );
                // NaN at sample 250 must remain pending in residual_flags() until the next frame.
                assert!(s.residual_flags().contains(FrameFlags::NONFINITE_INPUT));
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn order5_stream_hooks_and_due_rule_c() {
        use stream_hooks::{DueSnapshot, StreamHookEvent};

        let _restore = MxcsrGuard::capture();
        let has_avx2 =
            std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
        let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };
        let calm: Vec<f64> = (0..120)
            .map(|i| ((i as f64) * std::f64::consts::TAU * 1000.0 / 48000.0).sin() * 0.02)
            .collect();

        for &avx2 in backends {
            MxcsrGuard::set(0x1f80);
            let mut out = vec![StreamFrame::default(); ZwtvStream::max_frames_for_chunk(240)];
            let mut q = vec![FrameQualityV1::default(); out.len()];

            // 1. Due rule c sets ONLY the written stage's due flag after block 0 clears all four.
            //    Block 0 = [0, 25) has emissions at 0 and 24 -> clears tol, held, nl, tw!
            //    Hook at s=25 writes NL only -> due must be {tol: false, held: false, nl: true, tw: false}.
            stream_hooks::install_schedule(vec![StreamHookEvent {
                public_call_id: 0,
                sample: 25,
                stage: 3,
                band: 0,
                field: 0,
                raw_u64: 0x7ff8_0000_1111_2222,
            }]);
            stream_hooks::begin_due_log();
            let mut s = ZwtvStream::with_backend(FieldType::Free, avx2);
            s.push_with_quality(&calm[..40], &mut out, &mut q).unwrap();
            let dlog = stream_hooks::take_due_log();
            assert_eq!(
                dlog,
                vec![(
                    0,
                    25,
                    DueSnapshot {
                        tol: false,
                        held: false,
                        nl: true,
                        tw: false,
                    }
                )],
                "rule c must set ONLY nl due flag (avx2={avx2})"
            );
            // No emission in [25, 40) -> NL must NOT have faulted early!
            assert_eq!(
                s.diagnostics().incident_count,
                0,
                "NL hook must not validate before next emission (avx2={avx2})"
            );
            s.flush_with_quality(&mut out, &mut q).unwrap();
            assert_eq!(s.diagnostics().incident_count, 1);
            assert_eq!(s.diagnostics().first_stage, 3);
            stream_hooks::clear_schedule();

            // 2. TOL hook at s=25 sets ONLY tol due flag; negative min subnormal sm is caught at s=25!
            stream_hooks::install_schedule(vec![StreamHookEvent {
                public_call_id: 0,
                sample: 25,
                stage: 1,
                band: 13,
                field: 7,
                raw_u64: 0x8000_0000_0000_0001,
            }]);
            stream_hooks::begin_due_log();
            let mut s2 = ZwtvStream::with_backend(FieldType::Free, avx2);
            s2.push_with_quality(&calm[..72], &mut out, &mut q).unwrap();
            assert_eq!(
                stream_hooks::take_due_log(),
                vec![(
                    0,
                    25,
                    DueSnapshot {
                        tol: true,
                        held: false,
                        nl: false,
                        tw: false,
                    }
                )]
            );
            assert_eq!(s2.diagnostics().incident_count, 1);
            assert_eq!(s2.diagnostics().first_stage, 1);
            assert_eq!(s2.diagnostics().first_band, 13);
            assert_eq!(s2.diagnostics().first_detection_sample, 25);
            stream_hooks::clear_schedule();

            // 3. No-emission block [1, 10) after Call 0 [0, 1): due.{held,nl,tw} stay true!
            stream_hooks::install_schedule(vec![StreamHookEvent {
                public_call_id: 1,
                sample: 5,
                stage: 1,
                band: 0,
                field: 0,
                raw_u64: 0.25_f64.to_bits(),
            }]);
            stream_hooks::begin_due_log();
            let mut s3 = ZwtvStream::with_backend(FieldType::Free, avx2);
            s3.push_with_quality(&calm[..1], &mut out, &mut q).unwrap();
            s3.push_with_quality(&calm[1..10], &mut out, &mut q)
                .unwrap();
            // In Call 1 [1, 6), zero emissions occurred -> held/nl/tw remain true at s=5!
            assert_eq!(
                stream_hooks::take_due_log(),
                vec![(
                    1,
                    5,
                    DueSnapshot {
                        tol: true,
                        held: true,
                        nl: true,
                        tw: true,
                    }
                )]
            );
            stream_hooks::clear_schedule();

            // 4. Usage-rejected call MUST NOT execute hooks!
            stream_hooks::install_schedule(vec![StreamHookEvent {
                public_call_id: 0,
                sample: 0,
                stage: 1,
                band: 0,
                field: 0,
                raw_u64: 0x7ff8_0000_0000_0001,
            }]);
            let mut s4 = ZwtvStream::with_backend(FieldType::Free, avx2);
            let err = s4.push_with_quality(&calm[..24], &mut [], &mut q);
            assert_eq!(err, Err(StreamUsageError::OutputCapacity));
            assert_eq!(
                stream_hooks::executed_writes(),
                0,
                "usage-rejected call must execute 0 hooks"
            );
            assert_eq!(s4.tol.kernel_numeric_bits(), [0u64; 252]);
            stream_hooks::clear_schedule();

            // 5. Invalid coordinate s = N MUST be rejected (not clamped to N - 1)!
            stream_hooks::install_schedule(vec![StreamHookEvent {
                public_call_id: 0,
                sample: 24,
                stage: 1,
                band: 0,
                field: 0,
                raw_u64: 0x7ff8_0000_0000_0001,
            }]);
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut s5 = ZwtvStream::with_backend(FieldType::Free, avx2);
                let mut o5 = vec![StreamFrame::default(); ZwtvStream::max_frames_for_chunk(24)];
                let mut q5 = vec![FrameQualityV1::default(); o5.len()];
                let _ = s5.push_with_quality(&calm[..24], &mut o5, &mut q5);
            }));
            stream_hooks::clear_schedule();
            assert!(
                caught.is_err(),
                "s = N must panic/reject, never clamp to N-1"
            );

            // Also s = 0 on empty chunk (N = 0) MUST be rejected!
            stream_hooks::install_schedule(vec![StreamHookEvent {
                public_call_id: 0,
                sample: 0,
                stage: 1,
                band: 0,
                field: 0,
                raw_u64: 0,
            }]);
            let caught_empty = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut s6 = ZwtvStream::with_backend(FieldType::Free, avx2);
                let mut o6 = vec![StreamFrame::default(); 2];
                let mut q6 = vec![FrameQualityV1::default(); 2];
                let _ = s6.push_with_quality(&[], &mut o6, &mut q6);
            }));
            stream_hooks::clear_schedule();
            assert!(caught_empty.is_err(), "s = 0 on N = 0 must panic/reject");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn order6_stream_d0_dispatch_due_flush_and_levels_dedup() {
        let calm: Vec<f64> = (0..480)
            .map(|i| ((i as f64) * std::f64::consts::TAU * 1000.0 / 48000.0).sin() * 0.02)
            .collect();
        let has_avx2 =
            std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
        let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };
        for &avx2 in backends {
            let mut s = ZwtvStream::with_backend(FieldType::Free, avx2);
            let mut out = vec![StreamFrame::default(); ZwtvStream::max_frames_for_chunk(480)];
            let mut qual = vec![FrameQualityV1::default(); out.len()];
            test_stream_hooks::reset();
            let w = s.push_with_quality(&calm, &mut out, &mut qual).unwrap();
            assert!(w > 0);
            let c = test_stream_hooks::snapshot_d0();
            assert_eq!(
                c.d0_on_levels, 20,
                "RED: qualified 480-sample push must execute D0 on_levels for all 20 emissions (avx2={avx2})"
            );
            assert_eq!(
                c.d0_due_checks, 1,
                "RED: due check must run only once on first emission of push (avx2={avx2})"
            );
            assert_eq!(
                c.d0_calculate, 19,
                "RED: D0 calculate must run for 19 emissions (first emission holds slot) (avx2={avx2})"
            );
            assert_eq!(
                c.checked_calculate, 0,
                "RED: qualified push must not use checked calculate (avx2={avx2})"
            );

            // Flush MUST stay on R checked path (never D0)!
            test_stream_hooks::reset();
            let mut fout = [StreamFrame::default(); 2];
            let mut fqual = [FrameQualityV1::default(); 2];
            let _ = s.flush_with_quality(&mut fout, &mut fqual).unwrap();
            let cf = test_stream_hooks::snapshot_d0();
            assert_eq!(cf.d0_on_levels, 0, "flush must never enter D0 on_levels");
            assert_eq!(cf.d0_calculate, 0, "flush must never enter D0 calculate");
            assert_eq!(
                cf.checked_calculate, 1,
                "flush must use R checked calculate for final held slot"
            );

            // No-emission block MUST NOT check downstream stages prematurely!
            let mut s_noem = ZwtvStream::with_backend(FieldType::Free, avx2);
            let mut o_ne = [StreamFrame::default(); 2];
            assert_eq!(s_noem.push(&calm[..1], &mut o_ne), 0); // phase becomes 1, held_core = Some
            s_noem.emitter.inject_numeric(3, 0, 0, f64::NAN);
            test_stream_hooks::reset();
            assert_eq!(s_noem.push(&calm[1..10], &mut o_ne), 0); // no emission in samples [1..10)
            let c_ne = test_stream_hooks::snapshot_d0();
            assert_eq!(c_ne.d0_on_levels, 0);
            assert_eq!(c_ne.d0_due_checks, 0);
            assert_eq!(s_noem.diagnostics().incident_count, 0);
            // Next push crosses sample 24 emission -> catches injected NL via D0 due fallback!
            test_stream_hooks::reset();
            let _ = s_noem.push(&calm[10..25], &mut o_ne);
            let c_hit = test_stream_hooks::snapshot_d0();
            assert_eq!(c_hit.d0_due_checks, 1);
            assert_eq!(c_hit.d0_due_fallbacks, 1);
            assert_eq!(s_noem.diagnostics().incident_count, 1);

            // Non-RN creation legacy push MUST stay on R checked path (never D0)!
            let saved_csr = crate::numeric_guard::read_unit_mxcsr().unwrap();
            let mut s_nonrn = {
                unsafe {
                    let rz = (saved_csr & !0x6000) | 0x6000;
                    std::arch::asm!("ldmxcsr [{p}]", p = in(reg) &rz, options(nostack));
                }
                let stream = ZwtvStream::with_backend(FieldType::Free, avx2);
                crate::numeric_guard::restore_mxcsr(saved_csr);
                stream
            };
            test_stream_hooks::reset();
            let _ = s_nonrn.push(&calm, &mut out);
            let c_nr_d0 = test_stream_hooks::snapshot_d0();
            let c_nr_tol = test_stream_hooks::snapshot();
            assert_eq!(
                c_nr_d0.d0_on_levels, 0,
                "non-RN creation must never enter D0"
            );
            assert_eq!(c_nr_d0.d0_calculate, 0);
            assert_eq!(c_nr_d0.checked_calculate, 19);
            assert_eq!(c_nr_tol.env_fallbacks, 1);

            // Unqualified caller (RZ) on RN-created stream legacy push MUST stay on R checked path!
            let mut s_unq = ZwtvStream::with_backend(FieldType::Free, avx2);
            test_stream_hooks::reset();
            unsafe {
                let rz = (saved_csr & !0x6000) | 0x6000;
                std::arch::asm!("ldmxcsr [{p}]", p = in(reg) &rz, options(nostack));
            }
            let _ = s_unq.push(&calm, &mut out);
            crate::numeric_guard::restore_mxcsr(saved_csr);
            let c_uq_d0 = test_stream_hooks::snapshot_d0();
            let c_uq_tol = test_stream_hooks::snapshot();
            assert_eq!(
                c_uq_d0.d0_on_levels, 0,
                "unqualified push must never enter D0"
            );
            assert_eq!(c_uq_d0.d0_calculate, 0);
            assert_eq!(c_uq_d0.checked_calculate, 19);
            assert_eq!(c_uq_tol.env_fallbacks, 1);
        }
    }
}
