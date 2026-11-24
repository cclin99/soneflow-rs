//! Metadata travels with each retained interval, independently of acknowledgement.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Incident {
    pub sample: u64,
    pub first_internal: u64,
    pub stage: u32,
    pub reason: u32,
    pub band: u32,
}

#[derive(Clone, Copy, Default)]
pub(super) struct Coverage {
    pub first: u64,
    pub through: u64,
    pub quality: u32,
    pub stage_mask: u32,
    pub reason_mask: u32,
    pub first_origin: Option<(u64, Incident)>,
    pub last_origin: Option<(u64, Incident)>,
    pub coalesced: bool,
}

impl Coverage {
    pub(super) fn merge(&mut self, other: Self) {
        if other.quality == 0 {
            return;
        }
        if self.quality == 0 {
            *self = other;
            return;
        }
        self.first = self.first.min(other.first);
        self.through = self.through.max(other.through);
        self.quality |= other.quality;
        self.stage_mask |= other.stage_mask;
        self.reason_mask |= other.reason_mask;
        if let Some(first) = other.first_origin {
            if self
                .first_origin
                .is_none_or(|current| (first.0, first.1.sample) < (current.0, current.1.sample))
            {
                self.first_origin = Some(first);
            }
        }
        if let Some(last) = other.last_origin {
            if self
                .last_origin
                .is_none_or(|current| (last.0, last.1.sample) >= (current.0, current.1.sample))
            {
                self.last_origin = Some(last);
            }
        }
        self.coalesced |=
            other.coalesced || self.first_origin.map(|x| x.0) != self.last_origin.map(|x| x.0);
    }

    pub(super) fn add_masks(&mut self, stages: u32, reasons: u32) {
        self.stage_mask |= stages;
        self.reason_mask |= reasons;
    }
}
