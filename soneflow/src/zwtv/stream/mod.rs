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
mod reference;
mod tol_stage;

pub use diagnostics::{DiagnosticSnapshotV1, FrameQualityV1};
pub use frame::{FrameFlags, StreamFrame, N_WARMUP_FRAMES};
pub use reference::zwtv_reference_zerostate;

use crate::error::StreamUsageError;
use crate::numeric_guard::{read_rounding_environment, DomainFault, RoundingEnvironment};
use crate::types::FieldType;
use crate::zwtv::grid::{DEC_FACTOR, OUT_DECIM};
use denormal::DenormalGuard;
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

/// Test-only Order 5 stream hook scheduler (Design r3 §6, §8, §9.3).
/// Completely absent from default product builds (`#[cfg(test)]`); exposed to external
/// verification drivers solely via the candidate-side overlay `instrument_stream_c9.py`.
#[cfg(test)] // c9_stream_hook
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

/// Local entry-validation expiry flags per public call (Design r3 §2.3, §6; Orders 4–6 rules a, b, c).
#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy)]
struct Due {
    tol: bool,
    held: bool,
    nl: bool,
    tw: bool,
}

#[cfg(target_arch = "x86_64")]
impl Due {
    const fn all_true() -> Self {
        Self {
            tol: true,
            held: true,
            nl: true,
            tw: true,
        }
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

    pub const fn latency_samples() -> usize {
        DEC_FACTOR
    }

    pub const fn max_frames_for_chunk(chunk_len: usize) -> usize {
        chunk_len / (DEC_FACTOR * OUT_DECIM) + 2
    }

    pub fn push(&mut self, chunk: &[f64], out: &mut [StreamFrame]) -> usize {
        #[cfg(test)] // c9_stream_hook
        stream_hooks::begin_public_call();
        assert!(!self.flushed, "push after flush requires reset");
        self.check_timeline(chunk.len())
            .expect("stream timeline exhausted");
        assert!(out.len() >= Self::max_frames_for_chunk(chunk.len()));
        self.push_impl(chunk, out, None)
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
        #[cfg(test)] // c9_stream_hook
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
        Ok(self.push_impl(chunk, out, Some(quality)))
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

    fn push_impl(
        &mut self,
        chunk: &[f64],
        out: &mut [StreamFrame],
        mut quality: Option<&mut [FrameQualityV1]>,
    ) -> usize {
        let _guard = DenormalGuard::new();
        let mut written = 0;
        #[cfg(test)] // c9_stream_hook
        let mut hook_cursor = stream_hooks::HookCursor::for_call(chunk.len());
        if chunk.is_empty() {
            return 0;
        }

        let creation_env = self.creation_environment;
        let ZwtvStream {
            tol:
                TolStage {
                    kernel,
                    shadow,
                    sample_phase,
                },
            emitter,
            consumed_samples,
            ..
        } = self;

        // Design r3 §8: creation != KnownRn -> entire public call runs R checked on kernel.
        if creation_env != RoundingEnvironment::KnownRn {
            #[cfg(test)]
            test_stream_hooks::record_env_fallback();
            #[cfg(test)] // c9_stream_hook
            push_checked_range_with_hooks(
                kernel,
                sample_phase,
                emitter,
                consumed_samples,
                chunk,
                out,
                &mut quality,
                &mut written,
                &mut hook_cursor,
            );
            #[cfg(not(test))] // c9_stream_hook_else
            push_checked_range_on_kernel(
                kernel,
                sample_phase,
                emitter,
                consumed_samples,
                chunk,
                out,
                &mut quality,
                &mut written,
            );
            return written;
        }

        // First block MXCSR read inside DenormalGuard simultaneously checks qualification (§8).
        let first_mxcsr = crate::numeric_guard::read_unit_mxcsr();
        let Some(mut block_entry_mxcsr) =
            first_mxcsr.filter(|&m| crate::numeric_guard::is_mxcsr_qualified(m))
        else {
            #[cfg(test)]
            test_stream_hooks::record_env_fallback();
            #[cfg(test)] // c9_stream_hook
            push_checked_range_with_hooks(
                kernel,
                sample_phase,
                emitter,
                consumed_samples,
                chunk,
                out,
                &mut quality,
                &mut written,
                &mut hook_cursor,
            );
            #[cfg(not(test))] // c9_stream_hook_else
            push_checked_range_on_kernel(
                kernel,
                sample_phase,
                emitter,
                consumed_samples,
                chunk,
                out,
                &mut quality,
                &mut written,
            );
            return written;
        };

        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = (shadow, block_entry_mxcsr);
            #[cfg(test)] // c9_stream_hook
            push_checked_range_with_hooks(
                kernel,
                sample_phase,
                emitter,
                consumed_samples,
                chunk,
                out,
                &mut quality,
                &mut written,
                &mut hook_cursor,
            );
            #[cfg(not(test))] // c9_stream_hook_else
            push_checked_range_on_kernel(
                kernel,
                sample_phase,
                emitter,
                consumed_samples,
                chunk,
                out,
                &mut quality,
                &mut written,
            );
            return written;
        }

        #[cfg(target_arch = "x86_64")]
        {
            // Order 4 expiry rule a: all due flags start true at every public push entry.
            let mut due = Due::all_true();
            let mut pos = 0usize;
            let mut have_block_mxcsr = true;
            let mut emissions = [SpecEmission::ZERO; STREAM_MAX_EMISSIONS];

            while pos < chunk.len() {
                #[cfg(test)] // c9_stream_hook
                if hook_cursor.peek_sample() == Some(pos) {
                    apply_stream_hooks_at_boundary(
                        &mut hook_cursor,
                        pos,
                        kernel,
                        *sample_phase,
                        emitter,
                        *consumed_samples,
                        Some(&mut due),
                    );
                }

                #[cfg(test)] // c9_stream_hook
                let end = (pos + STREAM_BLOCK_LEN)
                    .min(chunk.len())
                    .min(hook_cursor.peek_sample().unwrap_or(chunk.len()));
                #[cfg(not(test))] // c9_stream_hook_else
                let end = (pos + STREAM_BLOCK_LEN).min(chunk.len());
                let block = &chunk[pos..end];

                if !have_block_mxcsr {
                    block_entry_mxcsr = crate::numeric_guard::read_unit_mxcsr().unwrap_or(0);
                }
                have_block_mxcsr = false;

                // Design r3 §5 line 123 & §6: when due.tol is true, validate kernel before
                // reading raw PCM, modifying pending, or touching shadow.
                if due.tol {
                    if kernel.validate_numeric().is_err() {
                        #[cfg(test)]
                        test_stream_hooks::record_entry_due_replay();
                        #[cfg(test)] // c9_stream_hook
                        assert!(
                            hook_cursor.peek_sample().is_none_or(|s| s >= end),
                            "STOP CONDITION: hook at {:?} fell inside entry-due replay interval [{}..{})",
                            hook_cursor.peek_sample(),
                            pos,
                            end
                        );
                        push_checked_range_on_kernel(
                            kernel,
                            sample_phase,
                            emitter,
                            consumed_samples,
                            block,
                            out,
                            &mut quality,
                            &mut written,
                        );
                        // Order 4 expiry rule b: cold path end sets all due flags true.
                        due = Due::all_true();
                        pos = end;
                        continue;
                    }
                    #[cfg(test)]
                    test_stream_hooks::record_entry_due_passed();
                    due.tol = false;
                }

                let entry_ordinal = *consumed_samples;
                let entry_phase = *sample_phase;

                #[cfg(test)]
                test_stream_hooks::record_spec_block();

                let outcome = tol_stage::advance_block_spec(
                    kernel,
                    shadow,
                    block,
                    entry_phase,
                    entry_ordinal,
                    &mut emissions,
                );

                if !outcome.ok {
                    // D1/D2 failure: do NOT swap; restore block entry MXCSR and replay R checked
                    // directly on kernel. Cold interface hard rule (§4): shadow is NOT passed.
                    #[cfg(test)] // c9_stream_hook
                    assert!(
                        hook_cursor.peek_sample().is_none_or(|s| s >= end),
                        "STOP CONDITION: hook at {:?} fell inside cold replay interval [{}..{})",
                        hook_cursor.peek_sample(),
                        pos,
                        end
                    );
                    push_cold_replay_with_mxcsr_restore(
                        block_entry_mxcsr,
                        kernel,
                        sample_phase,
                        emitter,
                        consumed_samples,
                        block,
                        out,
                        &mut quality,
                        &mut written,
                    );
                    // Order 4 expiry rule b: cold replay end sets all due flags true.
                    due = Due::all_true();
                    pos = end;
                    continue;
                }

                // D1/D2 passed: swap kernel <-> shadow and install tail phase.
                std::mem::swap(kernel, shadow);
                *sample_phase = outcome.end_phase;
                #[cfg(test)]
                test_stream_hooks::record_swap();

                // Deliver buffered emissions in order (Design r3 §5).
                let mut restarted_at: Option<usize> = None;
                for emission in &emissions[..outcome.emission_count] {
                    if emission.nonfinite_since_prev {
                        emitter.raise(FrameFlags::NONFINITE_INPUT);
                    }
                    let mut transaction = false;
                    written += emitter.on_levels_d0(
                        emission.levels,
                        emission.sample_ordinal,
                        &mut transaction,
                        &mut due.held,
                        &mut due.nl,
                        &mut due.tw,
                        &mut out[written..],
                        quality.as_deref_mut().map(|q| &mut q[written..]),
                    );
                    if transaction {
                        // Order 4 (Design r3 §5): downstream fault -> discard all subsequent
                        // speculative TOL/emission/nonfinite records, zero BOTH kernel and shadow,
                        // set phase = (0 + 1) % DEC_FACTOR = 1, consumed = e.ordinal + 1,
                        // and restart a new block from the next sample.
                        kernel.reset();
                        shadow.reset();
                        *sample_phase = 1;
                        *consumed_samples = emission.sample_ordinal + 1;
                        #[cfg(test)]
                        test_stream_hooks::record_downstream_restart();
                        restarted_at =
                            Some(pos + (emission.sample_ordinal - entry_ordinal) as usize + 1);
                        break;
                    }
                }

                if let Some(next_pos) = restarted_at {
                    // All-zero reset TOL does not require TOL entry validation (Design r3 §5).
                    due.tol = false;
                    pos = next_pos;
                } else {
                    if outcome.trailing_nonfinite {
                        emitter.raise(FrameFlags::NONFINITE_INPUT);
                    }
                    *consumed_samples = entry_ordinal + block.len() as u64;
                    pos = end;
                }
            }
            #[cfg(test)] // c9_stream_hook
            debug_assert!(
                hook_cursor.is_exhausted(),
                "unconsumed stream hooks remained after push"
            );
            written
        }
    }

    pub fn flush(&mut self, out: &mut [StreamFrame]) -> usize {
        #[cfg(test)] // c9_stream_hook
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
        #[cfg(test)] // c9_stream_hook
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
        #[cfg(test)] // c9_stream_hook
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
/// Never receives `&mut ZwtvStream`, `&mut TolStage`, or any reference to `shadow` (Design r3 §4).
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn push_checked_range_on_kernel(
    kernel: &mut TolKernel,
    sample_phase: &mut usize,
    emitter: &mut FrameEmitter,
    consumed_samples: &mut u64,
    sub: &[f64],
    out: &mut [StreamFrame],
    quality: &mut Option<&mut [FrameQualityV1]>,
    written: &mut usize,
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
            *written += emitter.on_levels(
                levels,
                *consumed_samples,
                &mut transaction,
                &mut out[*written..],
                quality.as_deref_mut().map(|q| &mut q[*written..]),
            );
            if transaction && !was_fault {
                kernel.reset();
            }
        }
        *sample_phase = (*sample_phase + 1) % DEC_FACTOR;
        *consumed_samples += 1;
    }
}

/// Cold replay after D1/D2 failure: restores block-entry MXCSR before the first FP op of replay,
/// then runs R's checked controller on `kernel`. Never receives `shadow` (Design r3 §4, §8).
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
    out: &mut [StreamFrame],
    quality: &mut Option<&mut [FrameQualityV1]>,
    written: &mut usize,
) {
    #[cfg(test)]
    test_stream_hooks::record_cold_replay();
    crate::numeric_guard::restore_mxcsr(entry_mxcsr);
    push_checked_range_on_kernel(
        kernel,
        sample_phase,
        emitter,
        consumed_samples,
        sub,
        out,
        quality,
        written,
    );
}

fn map_diagnostic_error(error: diagnostics::DiagnosticError) -> StreamUsageError {
    match error {
        diagnostics::DiagnosticError::StaleSequence => StreamUsageError::StaleSequence,
        diagnostics::DiagnosticError::CounterSaturated => StreamUsageError::CounterSaturated,
        diagnostics::DiagnosticError::SessionExhausted => StreamUsageError::SessionExhausted,
    }
}

#[cfg(test)] // c9_stream_hook
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
    // c9_hook_pre_snap
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
                    d.held = true;
                }
            }
            3 => {
                emitter.inject_numeric(3, ev.band, ev.field, f64::from_bits(ev.raw_u64));
                #[cfg(target_arch = "x86_64")]
                if let Some(d) = due.as_deref_mut() {
                    d.nl = true;
                }
            }
            5 => {
                emitter.inject_numeric(5, ev.band, ev.field, f64::from_bits(ev.raw_u64));
                #[cfg(target_arch = "x86_64")]
                if let Some(d) = due.as_deref_mut() {
                    d.tw = true;
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
                held: d.held,
                nl: d.nl,
                tw: d.tw,
            },
        );
    }
    // c9_hook_post_snap
}

#[cfg(test)] // c9_stream_hook
#[allow(clippy::too_many_arguments)]
fn push_checked_range_with_hooks(
    kernel: &mut TolKernel,
    sample_phase: &mut usize,
    emitter: &mut FrameEmitter,
    consumed_samples: &mut u64,
    chunk: &[f64],
    out: &mut [StreamFrame],
    quality: &mut Option<&mut [FrameQualityV1]>,
    written: &mut usize,
    hook_cursor: &mut stream_hooks::HookCursor,
) {
    let mut pos = 0usize;
    while pos < chunk.len() {
        if hook_cursor.peek_sample() == Some(pos) {
            apply_stream_hooks_at_boundary(
                hook_cursor,
                pos,
                kernel,
                *sample_phase,
                emitter,
                *consumed_samples,
                None,
            );
        }
        let end = hook_cursor.peek_sample().unwrap_or(chunk.len());
        debug_assert!(end > pos && end <= chunk.len());
        push_checked_range_on_kernel(
            kernel,
            sample_phase,
            emitter,
            consumed_samples,
            &chunk[pos..end],
            out,
            quality,
            written,
        );
        pos = end;
    }
    debug_assert!(hook_cursor.is_exhausted());
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
            // 1. All six Design §10.2 c downstream Core NonFinite cases (plus guaranteed-finite-suffix
            // first/diffuse/interleaved shapes that stay below TOL overflow through block end so
            // they exercise Order 4's speculative downstream restart path directly) must match
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

            // 1. Expiry rule c sets ONLY the written stage's due flag after block 0 clears all four.
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
