//! Shared by the stages: the fault coordinate and band dispatch.
use crate::numeric_guard::DomainFault;
use crate::zwtv::par::ParMode;
use rayon::prelude::*;

pub(super) type Fault = (usize, usize, DomainFault);

/// What every band worker of one stage call shares.
#[derive(Clone, Copy)]
pub(super) struct StageCtx {
    /// The caller's MXCSR qualified when the stage started.
    pub(super) qualified: bool,
    /// The input was validated before the stage (per-frame input checks skipped).
    pub(super) validated: bool,
    /// Run the AVX2 kernels.
    pub(super) avx2: bool,
}

/// The current unit's MXCSR, when it qualifies for the unchecked path (see `crate::numeric_guard`).
#[inline(always)]
pub(super) fn qualified_mxcsr() -> Option<u32> {
    crate::numeric_guard::read_unit_mxcsr().filter(|&m| crate::numeric_guard::is_mxcsr_qualified(m))
}

/// The current unit's MXCSR qualifies for the unchecked path (see `crate::numeric_guard`).
#[inline(always)]
pub(super) fn mxcsr_qualified() -> bool {
    crate::numeric_guard::read_unit_mxcsr().is_some_and(crate::numeric_guard::is_mxcsr_qualified)
}

pub(super) fn dispatch<F>(out: &mut [f64], chunk: usize, mode: ParMode, f: F) -> Result<(), Fault>
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
