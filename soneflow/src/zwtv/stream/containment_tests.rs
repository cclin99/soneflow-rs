use super::*;

#[test]
fn pending_fault_placeholder_covers_all_sample_phases_and_both_apis() {
    for avx2 in [false, crate::simd::use_avx2()] {
        for position in 0..96 {
            let mut signal = [0.0; 600];
            signal[position] = f64::MAX;
            let (frames, quality, _) = trace(&signal, signal.len(), avx2);
            assert_eq!(trace(&signal, 17, avx2).0, frames);
            let mut legacy = ZwtvStream::with_backend(FieldType::Free, avx2);
            let mut out = [StreamFrame::default(); 10];
            let count = legacy.push(&signal, &mut out);
            let tail = legacy.flush(&mut out[count..]);
            assert_eq!(&out[..count + tail], frames);
            let mut saw_fault = false;
            let mut saw_recovery_only = false;
            for (frame, record) in frames.iter().zip(quality) {
                if record.quality_bits & diagnostics::FAULT_FALLBACK != 0 {
                    saw_fault = true;
                    assert_eq!(
                        (frame.n.to_bits(), frame.n_phon.to_bits()),
                        (0, 0),
                        "backend={avx2} position={position} frame={}",
                        frame.t_frame_index
                    );
                } else if record.quality_bits == diagnostics::RECOVERING {
                    saw_recovery_only = true;
                    assert_eq!(
                        frame.n_phon.to_bits(),
                        crate::sone2phon::sone2phon(frame.n).to_bits()
                    );
                }
            }
            assert!(saw_fault && saw_recovery_only);
        }
    }
}

#[test]
fn same_transaction_all_observed_reasons_and_stages_are_unioned_once() {
    for avx2 in [false, crate::simd::use_avx2()] {
        let mut stream = ZwtvStream::with_backend(FieldType::Free, avx2);
        let mut out = [StreamFrame::default(); 4];
        let mut quality = [FrameQualityV1::default(); 4];
        stream
            .push_with_quality(&[0.0], &mut out, &mut quality)
            .unwrap();
        stream.emitter.inject_numeric(2, 20, 0, f64::MAX);
        stream.emitter.inject_numeric(3, 3, 0, f64::NAN);
        stream.emitter.inject_numeric(3, 0, 1, -1.0);
        stream.emitter.inject_numeric(5, 0, 2, f64::NAN);
        stream.flush_with_quality(&mut out, &mut quality).unwrap();
        let summary = stream.diagnostics();
        assert_eq!(summary.incident_count, 1);
        assert_eq!(summary.reason_mask, 1 | 2 | 4);
        assert_eq!(summary.stage_mask, (1 << 1) | (1 << 2) | (1 << 4));
        assert_eq!((summary.first_stage, summary.first_band), (2, 20));
        assert_eq!(quality[0].reason_mask, summary.reason_mask);
    }
}

#[test]
fn poisoned_held_nl_and_dormant_tw_are_visible_on_flush() {
    for avx2 in [false, crate::simd::use_avx2()] {
        for (stage, band, field) in [
            (2, 0, 0),
            (2, 20, 0),
            (3, 0, 0),
            (3, 3, 1),
            (3, 19, 0),
            (3, 20, 1),
            (5, 0, 0),
            (5, 0, 1),
            (5, 0, 2),
        ] {
            for value in [f64::NAN, f64::from_bits(0x8000000000000001), f64::MAX] {
                let mut stream = ZwtvStream::with_backend(FieldType::Free, avx2);
                let mut out = [StreamFrame::default(); 4];
                let mut quality = [FrameQualityV1::default(); 4];
                assert_eq!(
                    stream.push_with_quality(&[0.0], &mut out, &mut quality),
                    Ok(0)
                );
                stream.emitter.inject_numeric(stage, band, field, value);
                assert_eq!(stream.flush_with_quality(&mut out, &mut quality), Ok(1));
                assert_eq!(out[0].t_frame_index, 0);
                assert_eq!((out[0].n, out[0].n_phon), (0.0, 0.0));
                assert_ne!(quality[0].quality_bits & diagnostics::FAULT_FALLBACK, 0);
                assert_eq!(stream.diagnostics().incident_count, 1);
                assert_eq!(stream.diagnostics().first_stage, stage);
                assert!(stream.emitter.validate_numeric().is_ok());
            }
        }
    }
}

#[test]
fn clean_recovery_counter_excludes_tainted_successor_and_flush() {
    let mut stream = ZwtvStream::with_backend(FieldType::Free, false);
    let mut out = [StreamFrame::default(); 4];
    let mut quality = [FrameQualityV1::default(); 4];
    stream
        .push_with_quality(&[f64::MAX], &mut out, &mut quality)
        .unwrap();
    // At sample24 emit the tainted slot0 with a healthy next slot1.
    stream
        .push_with_quality(&[0.0; 24], &mut out, &mut quality)
        .unwrap();
    for _ in 0..2319 {
        stream
            .push_with_quality(&[0.0; 24], &mut out, &mut quality)
            .unwrap();
    }
    assert_ne!(
        stream.diagnostics().status_bits & diagnostics::ACTIVE_RECOVERY,
        0
    );
    stream
        .push_with_quality(&[0.0; 24], &mut out, &mut quality)
        .unwrap();
    assert_eq!(
        stream.diagnostics().status_bits & diagnostics::ACTIVE_RECOVERY,
        0
    );
    // Pending window still contains the final recovering contribution.
    let mut saw_healthy = false;
    for _ in 0..8 {
        let count = stream
            .push_with_quality(&[0.0; 24], &mut out, &mut quality)
            .unwrap();
        if count > 0 && quality[0].quality_bits == 0 {
            saw_healthy = true;
        }
    }
    assert!(saw_healthy);
}

fn trace(
    signal: &[f64],
    chunk_size: usize,
    avx2: bool,
) -> (Vec<StreamFrame>, Vec<FrameQualityV1>, DiagnosticSnapshotV1) {
    let mut stream = ZwtvStream::with_backend(FieldType::Free, avx2);
    let mut frames = Vec::new();
    let mut records = Vec::new();
    let capacity = ZwtvStream::max_frames_for_chunk(chunk_size);
    let mut out = vec![StreamFrame::default(); capacity];
    let mut quality = vec![FrameQualityV1::default(); capacity];
    for chunk in signal.chunks(chunk_size) {
        let count = stream
            .push_with_quality(chunk, &mut out, &mut quality)
            .unwrap();
        frames.extend_from_slice(&out[..count]);
        records.extend_from_slice(&quality[..count]);
    }
    let count = stream.flush_with_quality(&mut out, &mut quality).unwrap();
    frames.extend_from_slice(&out[..count]);
    records.extend_from_slice(&quality[..count]);
    (frames, records, stream.diagnostics())
}

#[test]
fn finite_overflow_has_numeric_quality_without_input_sanitation_flag() {
    for avx2 in [false, crate::simd::use_avx2()] {
        let mut signal = vec![0.0; 2400];
        signal[23] = f64::MAX;
        let (frames, quality, summary) = trace(&signal, 7, avx2);
        assert_eq!(summary.incident_count, 1);
        assert_eq!(summary.first_stage, 1);
        assert_eq!(summary.first_detection_sample, 23);
        assert!(frames
            .iter()
            .all(|f| f.n.is_finite() && f.n_phon.is_finite()));
        assert!(frames
            .iter()
            .all(|f| !f.flags.contains(FrameFlags::NONFINITE_INPUT)));
        assert!(quality
            .iter()
            .any(|q| q.quality_bits & diagnostics::FAULT_FALLBACK != 0));
        for (i, frame) in frames.iter().enumerate() {
            assert_eq!(frame.t_frame_index, i as u64);
        }
    }
}

#[test]
fn fault_trace_is_chunk_invariant_and_recovers_without_ack() {
    let mut signal = vec![0.0; 24 * 2340];
    for index in [0, 23, 24, 95, 96] {
        signal[index] = f64::MAX;
    }
    for avx2 in [false, crate::simd::use_avx2()] {
        let expected = trace(&signal, signal.len(), avx2);
        for size in [1, 23, 24, 25, 96, 480] {
            assert_eq!(trace(&signal, size, avx2), expected, "chunk={size}");
        }
        assert!(expected.2.incident_count >= 5);
        assert_eq!(expected.2.status_bits & diagnostics::ACTIVE_RECOVERY, 0);
        assert_eq!(expected.1.last().unwrap().quality_bits, 0);
        assert!(expected.2.status_bits & diagnostics::HAS_INCIDENT != 0);
    }
}

#[test]
fn sanitized_input_bits_match_explicit_zero_and_never_start_numeric_recovery() {
    let clean: Vec<_> = (0..2400).map(|i| (i as f64 * 0.13).sin() * 0.01).collect();
    for avx2 in [false, crate::simd::use_avx2()] {
        for index in [0, 1, 23, 24, 95, 96, 2399] {
            for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut bad = clean.clone();
                bad[index] = invalid;
                let mut zero = clean.clone();
                zero[index] = 0.0;
                let (frames, q, summary) = trace(&bad, 17, avx2);
                let (expected, _, _) = trace(&zero, 17, avx2);
                assert_eq!(summary.incident_count, 0);
                assert!(q.iter().all(|v| v.quality_bits == 0));
                for (a, b) in frames.iter().zip(expected) {
                    assert_eq!(
                        (a.n.to_bits(), a.n_phon.to_bits(), a.t_frame_index),
                        (b.n.to_bits(), b.n_phon.to_bits(), b.t_frame_index)
                    );
                }
            }
        }
    }
}

#[test]
fn quality_capacity_and_timeline_errors_are_atomic() {
    let mut stream = ZwtvStream::with_backend(FieldType::Free, false);
    let mut frames = [StreamFrame::default(); 4];
    let mut quality = [FrameQualityV1::default(); 4];
    let snapshot = stream.diagnostics();
    assert_eq!(
        stream.push_with_quality(&[1.0], &mut frames, &mut []),
        Err(StreamUsageError::QualityCapacity)
    );
    assert_eq!(stream.consumed_samples, 0);
    assert_eq!(stream.diagnostics(), snapshot);
    stream.consumed_samples = u64::MAX;
    assert_eq!(
        stream.push_with_quality(&[1.0], &mut [], &mut []),
        Err(StreamUsageError::TimelineExhausted)
    );
    assert_eq!(stream.consumed_samples, u64::MAX);
    assert_eq!(stream.diagnostics(), snapshot);
    stream.consumed_samples = 0;
    assert_eq!(
        stream.push_with_quality(&[], &mut frames, &mut quality),
        Ok(0)
    );
    assert_eq!(stream.diagnostics(), snapshot);
}

#[test]
fn tail_fault_peek_ack_and_explicit_session_reset() {
    let mut stream = ZwtvStream::with_backend(FieldType::Free, false);
    let mut out = [StreamFrame::default(); 4];
    let mut quality = [FrameQualityV1::default(); 4];
    stream
        .push_with_quality(&[0.0, f64::MAX], &mut out, &mut quality)
        .unwrap();
    let before = stream.diagnostics();
    assert_eq!(before.incident_count, 1);
    assert_eq!(stream.diagnostics(), before);
    assert_eq!(
        stream.ack_diagnostics(0),
        Err(StreamUsageError::StaleSequence)
    );
    stream.ack_diagnostics(before.sequence).unwrap();
    assert!(stream.diagnostics().status_bits & diagnostics::ACTIVE_RECOVERY != 0);
    assert_eq!(stream.flush_with_quality(&mut out, &mut quality), Ok(1));
    assert_eq!(out[0].n, 0.0);
    assert_eq!(out[0].n_phon, 0.0);
    assert_ne!(quality[0].quality_bits & diagnostics::FAULT_FALLBACK, 0);
    let after = stream.diagnostics();
    assert_eq!(stream.flush_with_quality(&mut out, &mut quality), Ok(0));
    assert_eq!(stream.diagnostics(), after);
    assert_eq!(stream.reset_with_diagnostics(), Ok(after));
    assert_eq!(stream.diagnostics().session_id, after.session_id + 1);
    assert_eq!(stream.diagnostics().status_bits, 0);
}
