//! Stream against the zero-state reference and the batch path on the golden
//! signals in `data/golden` (gitignored, regenerated locally). Local only, not
//! in CI; the data-free stream contract tests are in `stream_contract.rs`.

#[allow(dead_code)]
mod common;

use common::run_chunked;
use soneflow::FieldType;

#[test]
fn nine_golden_signals_match_zerostate_and_batch_bitwise() {
    const NAMES: [&str; 9] = [
        "sine_1k_60",
        "sine_250_80",
        "sine_4k_60",
        "white_60",
        "pulse_1k_70",
        "step_60_80",
        "annexb_sig3",
        "annexb_sig5",
        "annexb_sig10",
    ];
    for name in NAMES {
        let one_second = common::read_bin(name, "sig.bin");
        let signal: Vec<f64> = one_second
            .iter()
            .copied()
            .cycle()
            .take(one_second.len() * 3)
            .collect();
        let reference =
            soneflow::internals::zwtv::stream::zwtv_reference_zerostate(&signal, FieldType::Free);
        let frames = run_chunked(&signal, std::iter::repeat(480));
        assert_eq!(frames.len(), reference.len(), "{name}");
        for (index, (frame, expected)) in frames.iter().zip(&reference).enumerate() {
            assert_eq!(
                frame.n.to_bits(),
                expected.to_bits(),
                "{name} E2 frame {index}"
            );
        }
        let batch = soneflow::loudness_zwtv(&signal, 48_000.0, FieldType::Free).unwrap();
        assert_eq!(batch.n.len(), frames.len(), "{name}");
        for (index, (batch_n, frame)) in batch.n.iter().zip(&frames).enumerate() {
            assert_eq!(
                batch_n.to_bits(),
                frame.n.to_bits(),
                "{name} E3 frame {index}: batch={batch_n} stream={}",
                frame.n
            );
        }
    }
}
