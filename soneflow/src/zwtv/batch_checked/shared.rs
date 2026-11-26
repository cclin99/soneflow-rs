//! Shared by the stages: the fault coordinate and band dispatch.
use crate::numeric_guard::DomainFault;
use crate::zwtv::par::ParMode;
use rayon::prelude::*;

pub(super) type Fault = (usize, usize, DomainFault);

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
