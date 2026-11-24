//! Held lookahead, checked DSP emission, and per-frame diagnostic coverage.
use super::diagnostics::{DiagnosticState, FrameQualityV1, Incident};
use super::frame::{FrameFlags, StreamFrame, N_WARMUP_FRAMES};
use super::nl_stage::NlStage;
use crate::core::calc_slopes::calc_slopes_n_only;
use crate::core::main_loudness::main_loudness_clamped;
use crate::numeric_guard::{
    classify_nonnegative_bounded, validate_core, DomainFault, TW_INPUT_MAX,
};
use crate::sone2phon::sone2phon;
use crate::types::FieldType;
use crate::zwtv::grid::OUT_DECIM;
use crate::zwtv::temporal_weighting::TwState;

#[derive(Clone, Copy)]
struct CoreSlot {
    values: [f64; 21],
    tainted: bool,
}

pub(super) struct FrameEmitter {
    field: FieldType,
    nl: NlStage,
    tw: TwState,
    held_core: Option<CoreSlot>,
    emitted_internal: u64,
    pending: FrameFlags,
    pub(super) diagnostics: DiagnosticState,
}

pub(super) type Fault = (u32, usize, DomainFault);

fn core_reason_mask(core: &[f64; 21]) -> u32 {
    core.iter()
        .filter_map(|&value| {
            classify_nonnegative_bounded(value, crate::numeric_guard::CORE_MAX).err()
        })
        .fold(0, |mask, error| mask | error.reason_mask())
}

impl FrameEmitter {
    #[cfg(test)]
    pub(super) fn inject_numeric(&mut self, stage: u32, band: usize, field: usize, value: f64) {
        match stage {
            2 => {
                self.held_core
                    .as_mut()
                    .expect("test requires held slot")
                    .values[band] = value
            }
            3 => self.nl.inject_numeric(band, field, value),
            5 => self.tw.inject_numeric(field, value),
            _ => panic!("test stage"),
        }
    }
    pub(super) fn new(field: FieldType, avx2: bool, creation_invalid: bool) -> Self {
        Self {
            field,
            nl: NlStage::new(avx2),
            tw: TwState::new(),
            held_core: None,
            emitted_internal: 0,
            pending: FrameFlags::default(),
            diagnostics: DiagnosticState::new(creation_invalid),
        }
    }

    pub(super) fn reset(&mut self) {
        self.nl.reset();
        self.tw.reset();
        self.held_core = None;
        self.emitted_internal = 0;
        self.pending = FrameFlags::default();
    }

    pub(super) fn timeline_fits(&self, new_slots: u64) -> bool {
        self.emitted_internal
            .checked_add(new_slots)
            .and_then(|v| v.checked_add(u64::from(self.held_core.is_some())))
            .is_some()
    }

    pub(super) fn raise(&mut self, flag: FrameFlags) {
        self.pending.insert(flag);
    }
    pub(super) const fn pending(&self) -> FrameFlags {
        self.pending
    }

    pub(super) fn fault(
        &mut self,
        sample: u64,
        (stage, band, reason): Fault,
        transaction: &mut bool,
    ) {
        let (observed_stages, observed_reasons) = self.numeric_masks();
        let stages = observed_stages | (1 << (stage - 1));
        let reasons = observed_reasons | reason.reason_mask();
        let (stage, band, _) = self
            .validate_numeric()
            .err()
            .filter(|&(s, b, _)| (s, b) < (stage, band))
            .unwrap_or((stage, band, reason));
        if *transaction {
            self.diagnostics.merge_incident_masks(stages, reasons);
        } else {
            self.diagnostics.record_incident(Incident {
                sample,
                first_internal: self.emitted_internal,
                stage,
                reason: reasons,
                band: band as u32,
            });
            *transaction = true;
            self.diagnostics.merge_incident_masks(stages, reasons);
        }
        self.nl.reset();
        self.tw.reset();
        if let Some(held) = &mut self.held_core {
            held.values = [0.0; 21];
            held.tainted = true;
        }
    }

    pub(super) fn validate_numeric(&self) -> Result<(), Fault> {
        if let Some(held) = &self.held_core {
            validate_core(&held.values).map_err(|(b, e)| (2, b, e))?;
        }
        self.nl.validate_numeric().map_err(|(b, e)| (3, b, e))?;
        self.tw.validate_numeric().map_err(|e| (5, usize::MAX, e))
    }

    fn numeric_masks(&self) -> (u32, u32) {
        let core = self
            .held_core
            .as_ref()
            .map_or(0, |held| core_reason_mask(&held.values));
        let nl = self.nl.numeric_reason_mask();
        let tw = self.tw.numeric_reason_mask();
        let stages = if core != 0 { 1 << 1 } else { 0 }
            | if nl != 0 { 1 << 2 } else { 0 }
            | if tw != 0 { 1 << 4 } else { 0 };
        (stages, core | nl | tw)
    }

    pub(super) fn on_levels(
        &mut self,
        levels: [f64; 28],
        sample: u64,
        transaction: &mut bool,
        out: &mut [StreamFrame],
        quality: Option<&mut [FrameQualityV1]>,
    ) -> usize {
        let mut current = CoreSlot {
            values: [0.0; 21],
            tainted: *transaction,
        };
        if !*transaction {
            if let Some(band) = levels.iter().position(|x| !x.is_finite()) {
                self.fault(sample, (2, band, DomainFault::NonFinite), transaction);
                current.tainted = true;
            } else {
                let (core, clamped) = main_loudness_clamped(&levels, self.field);
                if clamped {
                    self.raise(FrameFlags::CLAMPED_120DB);
                }
                match validate_core(&core) {
                    Ok(()) => current.values = core,
                    Err((band, e)) => {
                        let reasons = core_reason_mask(&core);
                        self.fault(sample, (2, band, e), transaction);
                        self.diagnostics.merge_incident_masks(1 << 1, reasons);
                        current.tainted = true;
                    }
                }
            }
        }
        if let Err(fault) = self.validate_numeric() {
            self.fault(sample, fault, transaction);
            current.tainted = true;
            current.values = [0.0; 21];
        }
        let written = match self.held_core.take() {
            Some(held) => self.emit(held, &mut current, true, sample, transaction, out, quality),
            None => 0,
        };
        self.held_core = Some(current);
        written
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    fn is_held_entry_valid(&self) -> bool {
        use crate::numeric_guard::{is_nonnegative_bounded, CORE_MAX};
        self.held_core.as_ref().is_none_or(|held| {
            held.values
                .iter()
                .all(|&v| is_nonnegative_bounded(v, CORE_MAX))
        })
    }

    #[cfg(target_arch = "x86_64")]
    #[cold]
    #[inline(never)]
    fn on_levels_due_fallback(
        &mut self,
        sample: u64,
        current: &mut CoreSlot,
        transaction: &mut bool,
    ) {
        #[cfg(test)]
        super::test_stream_hooks::record_d0_due_fallback();
        // D0 entry failure: no speculation occurred, no MXCSR restore; delegate to R's
        // exact `validate_numeric()` + `fault()` so coordinate, reason, masks, and reset match R.
        if let Err(fault) = self.validate_numeric() {
            self.fault(sample, fault, transaction);
            current.tainted = true;
            current.values = [0.0; 21];
        }
    }

    /// Order 6 (Design r3 §3, §5, §7, §8): qualified `push` / `push_with_quality` emission path.
    /// - Deduplicates `levels` non-finite rescan (proven finite by TOL D2 on the exact same bits).
    /// - Retains `main_loudness_clamped` and its `validate_core(&core)` output check.
    /// - Checks due persistent states (`held`, `nl`, `tw`) ONLY after `main_loudness` validation.
    /// - Omits NL/TW substeps, NL `validate_core`, stream total bound, and the extra `sone2phon`
    ///   finiteness check inside `calculate_d0`; retains public output `sone2phon` in `emit_d0`.
    /// - Never called by `flush`, `flush_with_quality`, unqualified calls, or cold replay.
    #[cfg(target_arch = "x86_64")]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn on_levels_d0(
        &mut self,
        levels: [f64; 28],
        sample: u64,
        transaction: &mut bool,
        due_held: &mut bool,
        due_nl: &mut bool,
        due_tw: &mut bool,
        out: &mut [StreamFrame],
        quality: Option<&mut [FrameQualityV1]>,
    ) -> usize {
        debug_assert!(!*transaction);
        #[cfg(test)]
        super::test_stream_hooks::record_d0_on_levels();
        let mut current = CoreSlot {
            values: [0.0; 21],
            tainted: false,
        };
        let (core, clamped) = main_loudness_clamped(&levels, self.field);
        if clamped {
            self.raise(FrameFlags::CLAMPED_120DB);
        }
        match validate_core(&core) {
            Ok(()) => current.values = core,
            Err((band, e)) => {
                let reasons = core_reason_mask(&core);
                self.fault(sample, (2, band, e), transaction);
                self.diagnostics.merge_incident_masks(1 << 1, reasons);
                current.tainted = true;
            }
        }
        if *due_held || *due_nl || *due_tw {
            if !*transaction {
                #[cfg(test)]
                super::test_stream_hooks::record_d0_due_check();
                let held_ok = !*due_held || self.is_held_entry_valid();
                let nl_ok = !*due_nl || self.nl.is_entry_valid();
                let tw_ok = !*due_tw || self.tw.is_entry_valid();
                if !(held_ok && nl_ok && tw_ok) {
                    self.on_levels_due_fallback(sample, &mut current, transaction);
                }
            }
            *due_held = false;
            *due_nl = false;
            *due_tw = false;
        }
        let written = match self.held_core.take() {
            Some(held) => self.emit_d0(held, &mut current, sample, transaction, out, quality),
            None => 0,
        };
        self.held_core = Some(current);
        written
    }

    pub(super) fn finish(
        &mut self,
        sample: u64,
        transaction: &mut bool,
        out: &mut [StreamFrame],
        quality: Option<&mut [FrameQualityV1]>,
    ) -> usize {
        if let Err(fault) = self.validate_numeric() {
            self.fault(sample, fault, transaction);
        }
        let mut next = CoreSlot {
            values: [0.0; 21],
            tainted: false,
        };
        match self.held_core.take() {
            Some(held) => self.emit(held, &mut next, false, sample, transaction, out, quality),
            None => 0,
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[inline]
    fn calculate_d0(&mut self, held: &[f64; 21], next: &[f64; 21]) -> f64 {
        #[cfg(test)]
        super::test_stream_hooks::record_d0_calculate();
        let nl = self.nl.advance(held, next);
        let total = calc_slopes_n_only(&nl);
        self.tw.advance(total)
    }

    #[cfg(target_arch = "x86_64")]
    #[allow(clippy::too_many_arguments)]
    fn emit_d0(
        &mut self,
        held: CoreSlot,
        next: &mut CoreSlot,
        sample: u64,
        transaction: &mut bool,
        out: &mut [StreamFrame],
        quality: Option<&mut [FrameQualityV1]>,
    ) -> usize {
        let _ = sample;
        let tainted = held.tainted || next.tainted;
        let n = if tainted {
            0.0
        } else {
            self.calculate_d0(&held.values, &next.values)
        };
        let internal = self.emitted_internal;
        self.emitted_internal += 1;
        let through = internal + 1;
        self.diagnostics
            .emission_quality(internal, through, !*transaction, tainted);
        if !internal.is_multiple_of(OUT_DECIM as u64) {
            return 0;
        }
        let index = internal / OUT_DECIM as u64;
        let mut flags = self.pending.take();
        if index < N_WARMUP_FRAMES {
            flags.insert(FrameFlags::WARMUP);
        }
        let record = self.diagnostics.take_frame_quality(
            index,
            internal.saturating_sub(OUT_DECIM as u64 - 1),
            through,
        );
        let fallback = record.quality_bits & super::diagnostics::FAULT_FALLBACK != 0;
        out[0] = StreamFrame {
            t_frame_index: index,
            n: if fallback { 0.0 } else { n },
            n_phon: if fallback { 0.0 } else { sone2phon(n) },
            flags,
            _reserved: 0,
        };
        if let Some(quality) = quality {
            quality[0] = record;
        }
        1
    }

    fn calculate(&mut self, held: &[f64; 21], next: &[f64; 21]) -> Result<f64, Fault> {
        #[cfg(test)]
        super::test_stream_hooks::record_checked_calculate();
        let nl = self
            .nl
            .advance_checked(held, next)
            .map_err(|(b, e)| (3, b, e))?;
        validate_core(&nl).map_err(|(b, e)| (3, b, e))?;
        let total = calc_slopes_n_only(&nl);
        classify_nonnegative_bounded(total, TW_INPUT_MAX).map_err(|e| (4, usize::MAX, e))?;
        let n = self
            .tw
            .advance_checked(total)
            .map_err(|e| (5, usize::MAX, e))?;
        if !sone2phon(n).is_finite() {
            return Err((6, usize::MAX, DomainFault::NonFinite));
        }
        Ok(n)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        held: CoreSlot,
        next: &mut CoreSlot,
        real_successor: bool,
        sample: u64,
        transaction: &mut bool,
        out: &mut [StreamFrame],
        quality: Option<&mut [FrameQualityV1]>,
    ) -> usize {
        let mut tainted = held.tainted || next.tainted;
        let n = if tainted {
            0.0
        } else {
            match self.calculate(&held.values, &next.values) {
                Ok(n) => n,
                Err(fault) => {
                    self.fault(sample, fault, transaction);
                    next.values = [0.0; 21];
                    next.tainted = true;
                    tainted = true;
                    0.0
                }
            }
        };
        let internal = self.emitted_internal;
        self.emitted_internal += 1;
        let through = internal + u64::from(real_successor);
        self.diagnostics.emission_quality(
            internal,
            through,
            real_successor && !*transaction,
            tainted,
        );
        if !internal.is_multiple_of(OUT_DECIM as u64) {
            return 0;
        }
        let index = internal / OUT_DECIM as u64;
        let mut flags = self.pending.take();
        if index < N_WARMUP_FRAMES {
            flags.insert(FrameFlags::WARMUP);
        }
        let record = self.diagnostics.take_frame_quality(
            index,
            internal.saturating_sub(OUT_DECIM as u64 - 1),
            through,
        );
        // Visibility covers the complete decimation interval. A fault in an
        // earlier, hidden tick still requires a placeholder here. This output
        // decision must not feed back into contributor taint or clean ticks.
        let fallback = record.quality_bits & super::diagnostics::FAULT_FALLBACK != 0;
        out[0] = StreamFrame {
            t_frame_index: index,
            n: if fallback { 0.0 } else { n },
            n_phon: if fallback { 0.0 } else { sone2phon(n) },
            flags,
            _reserved: 0,
        };
        if let Some(quality) = quality {
            quality[0] = record;
        }
        1
    }
}
