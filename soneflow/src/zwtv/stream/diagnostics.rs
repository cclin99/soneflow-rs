//! Bounded, allocation-free quality bookkeeping. DSP integration is separate.

pub(crate) const FAULT_FALLBACK: u32 = 1;
pub(crate) const RECOVERING: u32 = 2;
pub(crate) const HAS_INCIDENT: u32 = 1;
pub(crate) const ACTIVE_RECOVERY: u32 = 2;
pub(crate) const TAIL_PENDING: u32 = 4;
pub(crate) const COALESCED: u32 = 8;
pub(crate) const COUNTER_SATURATED: u32 = 16;
pub(crate) const CREATION_ENV_INVALID: u32 = 32;
const CLEAN_TICKS: u16 = 2320;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameQualityV1 {
    pub version: u32,
    pub quality_bits: u32,
    pub t_frame_index: u64,
    pub session_id: u64,
    pub recovery_generation: u64,
    pub first_affected_internal: u64,
    pub through_internal: u64,
    pub stage_mask: u32,
    pub reason_mask: u32,
}

impl Default for FrameQualityV1 {
    fn default() -> Self {
        Self {
            version: 1,
            quality_bits: 0,
            t_frame_index: 0,
            session_id: 0,
            recovery_generation: 0,
            first_affected_internal: 0,
            through_internal: 0,
            stage_mask: 0,
            reason_mask: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagnosticSnapshotV1 {
    pub version: u32,
    pub struct_size: u32,
    pub status_bits: u32,
    pub stage_mask: u32,
    pub reason_mask: u32,
    pub first_stage: u32,
    pub first_band: u32,
    pub latest_stage: u32,
    pub latest_band: u32,
    pub reserved: u32,
    pub session_id: u64,
    pub recovery_generation: u64,
    pub sequence: u64,
    pub incident_count: u64,
    pub first_detection_sample: u64,
    pub last_detection_sample: u64,
    pub first_affected_internal: u64,
    pub covered_through_internal: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiagnosticError {
    StaleSequence,
    CounterSaturated,
    SessionExhausted,
}

mod coverage;
use coverage::Coverage;
pub(crate) use coverage::Incident;

/// Four separate channels: contributor taint is supplied by the emitter;
/// current recovery never reads pending output or unacknowledged history.
pub(crate) struct DiagnosticState {
    session_id: u64,
    generation: u64,
    sequence: u64,
    incident_count: u64,
    saturated: bool,
    creation_invalid: bool,
    recovering: bool,
    clean_ticks: u16,
    history: Coverage,
    recovery: Coverage,
    pending: Coverage,
}

impl DiagnosticState {
    pub(crate) fn new(creation_invalid: bool) -> Self {
        Self {
            session_id: 0,
            generation: 0,
            sequence: 0,
            incident_count: 0,
            saturated: false,
            creation_invalid,
            recovering: false,
            clean_ticks: 0,
            history: Coverage::default(),
            recovery: Coverage::default(),
            pending: Coverage::default(),
        }
    }

    /// Called exactly once per fault transaction. Additional observed reasons in
    /// that same transaction use merge_incident_masks, not another incident.
    pub(crate) fn record_incident(&mut self, incident: Incident) {
        self.sequence = self.sequence.saturating_add(1);
        self.generation = self.generation.saturating_add(1);
        self.incident_count = self.incident_count.saturating_add(1);
        self.saturated |= [self.sequence, self.generation, self.incident_count].contains(&u64::MAX);
        let scope = Coverage {
            first: incident.first_internal,
            through: incident.first_internal,
            quality: FAULT_FALLBACK | RECOVERING,
            stage_mask: incident
                .stage
                .checked_sub(1)
                .and_then(|n| 1_u32.checked_shl(n))
                .unwrap_or(0),
            reason_mask: incident.reason,
            first_origin: Some((self.sequence, incident)),
            last_origin: Some((self.sequence, incident)),
            coalesced: false,
        };
        if !self.recovering {
            self.recovery = Coverage::default();
        }
        // A saturated sequence cannot distinguish new transactions.
        if self.sequence == u64::MAX {
            self.history.coalesced |= self.history.quality != 0;
            self.recovery.coalesced |= self.recovery.quality != 0;
            self.pending.coalesced |= self.pending.quality != 0;
        }
        self.history.merge(scope);
        self.recovery.merge(scope);
        self.pending.merge(scope);
        self.clean_ticks = 0;
        self.recovering = true;
    }

    pub(crate) fn merge_incident_masks(&mut self, stages: u32, reasons: u32) {
        self.history.add_masks(stages, reasons);
        self.recovery.add_masks(stages, reasons);
        self.pending.add_masks(stages, reasons);
    }

    /// `eligible_clean` excludes artificial flush successors. A tainted held or
    /// next slot is never eligible, even if its placeholder value is finite.
    pub(crate) fn emission_quality(
        &mut self,
        first: u64,
        through: u64,
        eligible_clean: bool,
        tainted: bool,
    ) -> u32 {
        let local = if tainted {
            FAULT_FALLBACK | RECOVERING
        } else if self.recovering {
            RECOVERING
        } else {
            0
        };
        if self.recovering && eligible_clean && !tainted {
            self.clean_ticks += 1;
            self.recovering = self.clean_ticks < CLEAN_TICKS;
        }
        if local != 0 {
            let scope = Coverage {
                first: self.recovery.first.min(first),
                through,
                quality: local,
                ..self.recovery
            };
            self.recovery.through = self.recovery.through.max(through);
            self.pending.merge(scope);
            if self.history.quality != 0 {
                self.history.merge(scope);
            }
        }
        local
    }

    /// Deliver pending quality once. This does not acknowledge caller history.
    pub(crate) fn take_frame_quality(
        &mut self,
        frame: u64,
        first: u64,
        through: u64,
    ) -> FrameQualityV1 {
        let pending = std::mem::take(&mut self.pending);
        let affected = if pending.quality == 0 {
            first
        } else {
            pending.first.min(first)
        };
        let end = pending.through.max(through);
        if pending.quality != 0 && self.history.quality != 0 {
            self.history.through = self.history.through.max(end);
        }
        FrameQualityV1 {
            version: 1,
            quality_bits: pending.quality,
            t_frame_index: frame,
            session_id: self.session_id,
            recovery_generation: self.generation,
            first_affected_internal: affected,
            through_internal: end,
            stage_mask: pending.stage_mask,
            reason_mask: pending.reason_mask,
        }
    }

    pub(crate) fn snapshot(&self) -> DiagnosticSnapshotV1 {
        let mut scope = self.history;
        if self.recovering {
            scope.merge(self.recovery);
        }
        scope.merge(self.pending);
        let first = scope.first_origin.map(|x| x.1);
        let latest = scope.last_origin.map(|x| x.1);
        let status = (u32::from(self.history.quality != 0) * HAS_INCIDENT)
            | (u32::from(self.recovering) * ACTIVE_RECOVERY)
            | (u32::from(self.pending.quality != 0) * TAIL_PENDING)
            | (u32::from(scope.coalesced) * COALESCED)
            | (u32::from(self.saturated) * COUNTER_SATURATED)
            | (u32::from(self.creation_invalid) * CREATION_ENV_INVALID);
        DiagnosticSnapshotV1 {
            version: 1,
            struct_size: 104,
            status_bits: status,
            stage_mask: scope.stage_mask,
            reason_mask: scope.reason_mask,
            first_stage: first.map_or(0, |x| x.stage),
            first_band: first.map_or(u32::MAX, |x| x.band),
            latest_stage: latest.map_or(0, |x| x.stage),
            latest_band: latest.map_or(u32::MAX, |x| x.band),
            reserved: 0,
            session_id: self.session_id,
            recovery_generation: self.generation,
            sequence: self.sequence,
            incident_count: self.incident_count,
            first_detection_sample: first.map_or(0, |x| x.sample),
            last_detection_sample: latest.map_or(0, |x| x.sample),
            first_affected_internal: scope.first,
            covered_through_internal: scope.through,
        }
    }

    pub(crate) fn acknowledge(&mut self, expected_sequence: u64) -> Result<(), DiagnosticError> {
        if self.saturated {
            return Err(DiagnosticError::CounterSaturated);
        }
        if expected_sequence != self.sequence {
            return Err(DiagnosticError::StaleSequence);
        }
        self.incident_count = 0;
        self.history = Coverage::default();
        Ok(())
    }

    pub(crate) fn reset_with_diagnostics(
        &mut self,
    ) -> Result<DiagnosticSnapshotV1, DiagnosticError> {
        let next = self
            .session_id
            .checked_add(1)
            .ok_or(DiagnosticError::SessionExhausted)?;
        let before = self.snapshot();
        *self = Self::new(self.creation_invalid);
        self.session_id = next;
        Ok(before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(sample: u64, internal: u64) -> Incident {
        Incident {
            sample,
            first_internal: internal,
            stage: 3,
            reason: 1,
            band: 7,
        }
    }

    #[test]
    fn frozen_pod_layouts() {
        assert_eq!(std::mem::size_of::<FrameQualityV1>(), 56);
        assert_eq!(std::mem::align_of::<FrameQualityV1>(), 8);
        assert_eq!(std::mem::size_of::<DiagnosticSnapshotV1>(), 104);
        assert_eq!(std::mem::offset_of!(DiagnosticSnapshotV1, session_id), 40);
        assert_eq!(std::mem::offset_of!(FrameQualityV1, stage_mask), 48);
    }

    #[test]
    fn healthy_and_creation_environment_are_distinct() {
        assert_eq!(DiagnosticState::new(false).snapshot().status_bits, 0);
        assert_eq!(
            DiagnosticState::new(true).snapshot().status_bits,
            CREATION_ENV_INVALID
        );
    }

    #[test]
    fn incident_reaches_frame_and_sticky_independently() {
        let mut d = DiagnosticState::new(false);
        d.record_incident(origin(24, 0));
        assert_eq!(
            d.emission_quality(0, 1, false, true),
            FAULT_FALLBACK | RECOVERING
        );
        let f = d.take_frame_quality(0, 0, 1);
        assert_eq!(f.quality_bits, FAULT_FALLBACK | RECOVERING);
        assert_eq!((f.first_affected_internal, f.through_internal), (0, 1));
        let s = d.snapshot();
        assert_eq!((s.incident_count, s.sequence), (1, 1));
        assert_eq!(s.status_bits & TAIL_PENDING, 0);
        assert_ne!(s.status_bits & HAS_INCIDENT, 0);
    }

    #[test]
    fn clean_counter_does_not_read_pending_or_unacked_history() {
        let mut d = DiagnosticState::new(false);
        d.record_incident(origin(0, 0));
        d.take_frame_quality(0, 0, 1);
        for tick in 1..=2320 {
            assert_eq!(d.emission_quality(tick, tick + 1, true, false), RECOVERING);
            if tick % 4 == 0 {
                d.take_frame_quality(tick / 4, tick, tick + 1);
            }
        }
        assert_eq!(d.snapshot().status_bits & ACTIVE_RECOVERY, 0);
        for tick in 2321..=2324 {
            assert_eq!(d.emission_quality(tick, tick + 1, true, false), 0);
        }
        assert_eq!(d.take_frame_quality(581, 2324, 2325).quality_bits, 0);
        assert_ne!(d.snapshot().status_bits & HAS_INCIDENT, 0);
    }

    #[test]
    fn pending_recovery_delivers_once_without_feedback() {
        let mut d = DiagnosticState::new(false);
        d.record_incident(origin(0, 0));
        d.take_frame_quality(0, 0, 1);
        for tick in 1..=2320 {
            d.emission_quality(tick, tick + 1, true, false);
        }
        assert_eq!(d.emission_quality(2321, 2322, true, false), 0);
        assert_eq!(
            d.take_frame_quality(580, 2321, 2322).quality_bits,
            RECOVERING
        );
        assert_eq!(d.take_frame_quality(581, 2324, 2325).quality_bits, 0);
    }

    #[test]
    fn acknowledgement_does_not_erase_active_or_pending_coverage() {
        let mut d = DiagnosticState::new(false);
        d.record_incident(origin(71, 2));
        let before = d.snapshot();
        assert_eq!(d.acknowledge(0), Err(DiagnosticError::StaleSequence));
        assert_eq!(d.snapshot(), before);
        d.acknowledge(1).unwrap();
        let s = d.snapshot();
        assert_eq!(s.incident_count, 0);
        assert_eq!(s.status_bits & HAS_INCIDENT, 0);
        assert_eq!(
            s.status_bits & (ACTIVE_RECOVERY | TAIL_PENDING),
            ACTIVE_RECOVERY | TAIL_PENDING
        );
        assert_eq!(s.stage_mask, 1 << 2);
        assert_eq!(s.first_affected_internal, 2);
    }

    #[test]
    fn repeated_fault_restarts_quarantine_and_coalesces_history() {
        let mut d = DiagnosticState::new(false);
        d.record_incident(origin(0, 0));
        for tick in 1..2320 {
            d.emission_quality(tick, tick + 1, true, false);
        }
        d.record_incident(Incident {
            stage: 5,
            band: u32::MAX,
            ..origin(55680, 2320)
        });
        assert_eq!(d.emission_quality(2321, 2322, true, false), RECOVERING);
        let s = d.snapshot();
        assert_eq!((s.incident_count, s.recovery_generation), (2, 2));
        assert_ne!(s.status_bits & COALESCED, 0);
        assert_eq!(s.stage_mask, (1 << 2) | (1 << 4));
        assert_eq!((s.first_stage, s.latest_stage), (3, 5));
        assert_eq!(d.clean_ticks, 1);
    }

    #[test]
    fn flush_placeholder_never_increments_clean_counter() {
        let mut d = DiagnosticState::new(false);
        d.record_incident(origin(1, 0));
        for _ in 0..2400 {
            d.emission_quality(0, 1, false, false);
        }
        assert_eq!(d.clean_ticks, 0);
        assert_ne!(d.snapshot().status_bits & ACTIVE_RECOVERY, 0);
    }

    #[test]
    fn tail_and_multiple_peeks_do_not_consume_incident() {
        let mut d = DiagnosticState::new(false);
        d.record_incident(origin(95, 3));
        let a = d.snapshot();
        assert_eq!(a, d.snapshot());
        assert_ne!(a.status_bits & TAIL_PENDING, 0);
        assert_eq!(a.first_detection_sample, 95);
    }

    #[test]
    fn explicit_reset_returns_old_summary_and_preserves_creation_validity() {
        let mut d = DiagnosticState::new(true);
        d.record_incident(origin(95, 3));
        let old = d.snapshot();
        assert_eq!(d.reset_with_diagnostics().unwrap(), old);
        let s = d.snapshot();
        assert_eq!((s.session_id, s.sequence, s.incident_count), (1, 0, 0));
        assert_eq!(s.status_bits, CREATION_ENV_INVALID);
    }

    #[test]
    fn exhausted_session_reset_is_atomic() {
        let mut d = DiagnosticState::new(false);
        d.session_id = u64::MAX;
        d.record_incident(origin(0, 0));
        let old = d.snapshot();
        assert_eq!(
            d.reset_with_diagnostics(),
            Err(DiagnosticError::SessionExhausted)
        );
        assert_eq!(d.snapshot(), old);
        d.emission_quality(1, 2, true, false);
        assert_eq!(d.clean_ticks, 1);
    }

    #[test]
    fn saturated_sequence_cannot_be_acknowledged() {
        let mut d = DiagnosticState::new(false);
        d.sequence = u64::MAX - 1;
        d.record_incident(origin(0, 0));
        assert_ne!(d.snapshot().status_bits & COUNTER_SATURATED, 0);
        assert_eq!(
            d.acknowledge(u64::MAX),
            Err(DiagnosticError::CounterSaturated)
        );
        d.record_incident(origin(24, 1));
        assert_eq!(d.snapshot().sequence, u64::MAX);
    }

    #[test]
    fn merging_same_transaction_does_not_count_a_new_incident() {
        let mut d = DiagnosticState::new(false);
        d.record_incident(origin(0, 0));
        d.merge_incident_masks(1 << 4, 4);
        let s = d.snapshot();
        assert_eq!(
            (s.incident_count, s.sequence, s.recovery_generation),
            (1, 1, 1)
        );
        assert_eq!((s.stage_mask, s.reason_mask), ((1 << 2) | (1 << 4), 1 | 4));
    }

    #[test]
    fn acknowledged_then_new_fault_keeps_old_active_origin() {
        let mut d = DiagnosticState::new(false);
        d.record_incident(origin(24, 0));
        d.take_frame_quality(0, 0, 1);
        d.acknowledge(1).unwrap();
        d.record_incident(Incident {
            stage: 5,
            ..origin(48, 1)
        });
        let s = d.snapshot();
        assert_eq!(s.incident_count, 1);
        assert_eq!((s.first_detection_sample, s.first_stage), (24, 3));
        assert_ne!(s.status_bits & COALESCED, 0);
    }

    #[test]
    fn saturated_sequence_keeps_first_and_latest_origins() {
        let mut d = DiagnosticState::new(false);
        d.sequence = u64::MAX - 1;
        d.record_incident(origin(24, 0));
        d.take_frame_quality(0, 0, 1);
        d.record_incident(Incident {
            stage: 5,
            ..origin(48, 1)
        });
        let s = d.snapshot();
        assert_eq!(
            (s.first_detection_sample, s.last_detection_sample),
            (24, 48)
        );
        assert_eq!((s.first_stage, s.latest_stage), (3, 5));
        assert_ne!(s.status_bits & COALESCED, 0);
        assert_eq!(
            d.acknowledge(u64::MAX),
            Err(DiagnosticError::CounterSaturated)
        );
    }

    #[test]
    fn old_pending_origin_survives_new_recovery_until_delivery_only() {
        let mut d = DiagnosticState::new(false);
        d.record_incident(origin(0, 0));
        for tick in 1..=2320 {
            d.emission_quality(tick, tick + 1, true, false);
        }
        d.acknowledge(1).unwrap();
        d.record_incident(Incident {
            stage: 5,
            ..origin(100000, 2400)
        });
        let pending = d.snapshot();
        assert_eq!(pending.first_detection_sample, 0);
        assert_eq!(pending.incident_count, 1);
        assert_ne!(pending.status_bits & COALESCED, 0);
        d.take_frame_quality(600, 2400, 2401);
        let after = d.snapshot();
        assert_eq!(after.first_detection_sample, 100000);
        assert_eq!(after.status_bits & COALESCED, 0);
    }
}
