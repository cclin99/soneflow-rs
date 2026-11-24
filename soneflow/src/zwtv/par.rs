//! Band-parallel scheduling shared by the batch stages.

use rayon::prelude::*;

/// 頻帶平行階段的排程模式。離線批次走 `Rayon`;
/// R5 串流路徑必須選 `Sequential`(音訊路徑不得觸發 thread pool)。
/// 注意:scalar 後備路徑在 `Rayon` 模式下的暫態配置隨執行緒數放大
/// (10 s 訊號 @12 緒:tol ~46 MB、nl ~138 MB),僅限離線批次使用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParMode {
    Rayon,
    Sequential,
}

/// Split `out` into `chunk`-sized pieces and run `f(index, piece)` on each,
/// on the Rayon pool or in order on the caller's thread.
pub(crate) fn chunks_dispatch<F>(out: &mut [f64], chunk: usize, mode: ParMode, f: F)
where
    F: Fn(usize, &mut [f64]) + Sync,
{
    if out.is_empty() {
        return;
    }
    match mode {
        ParMode::Rayon => out
            .par_chunks_mut(chunk)
            .enumerate()
            .for_each(|(index, piece)| f(index, piece)),
        ParMode::Sequential => {
            for (index, piece) in out.chunks_mut(chunk).enumerate() {
                f(index, piece);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_modes_visit_every_chunk_once_in_index_order() {
        for mode in [ParMode::Rayon, ParMode::Sequential] {
            let mut out = vec![0.0; 10];
            chunks_dispatch(&mut out, 4, mode, |index, piece| {
                piece.fill(index as f64 + 1.0);
            });
            assert_eq!(out, [1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0, 3.0, 3.0]);
        }
        chunks_dispatch(&mut [], 4, ParMode::Rayon, |_, _| unreachable!());
    }
}
