use soneflow_ffi::*;

#[test]
fn quality_layout_is_additive() {
    assert_eq!(std::mem::size_of::<SnfStreamFrame>(), 32);
    assert_eq!(std::mem::size_of::<SnfFrameQualityV1>(), 56);
    assert_eq!(std::mem::size_of::<SnfDiagnosticSnapshotV1>(), 104);
    assert_eq!(std::mem::align_of::<SnfFrameQualityV1>(), 8);
    assert_eq!(std::mem::align_of::<SnfDiagnosticSnapshotV1>(), 8);
}

#[test]
fn batch_numeric_error_uses_code_five_without_output_writes() {
    let signal = vec![f64::MAX; 48_000];
    let mut n = -123.0;
    let mut specific = [-123.0; 240];
    let mut bark = [-123.0; 240];
    let code = unsafe {
        snf_loudness_zwst(
            signal.as_ptr(),
            signal.len(),
            48_000.0,
            SNF_FIELD_FREE,
            &mut n,
            specific.as_mut_ptr(),
            bark.as_mut_ptr(),
        )
    };
    assert_eq!(code, SNF_ERR_INTERNAL_NUMERIC_FAULT);
    assert_eq!(n, -123.0);
    assert!(specific.iter().chain(&bark).all(|v| *v == -123.0));
}

#[test]
fn checked_buffers_versions_and_lifecycle_are_atomic() {
    unsafe {
        let handle = snf_stream_new(SNF_FIELD_FREE);
        assert!(!handle.is_null());
        let mut snapshot = SnfDiagnosticSnapshotV1::default();
        assert_eq!(snf_stream_diagnostics_v1(handle, &mut snapshot, 1, 104), 0);
        let before = snapshot;
        let chunk = [0.0; 480];
        let cap = snf_stream_max_frames(chunk.len());
        let mut out = vec![
            SnfStreamFrame {
                n: -123.0,
                ..Default::default()
            };
            cap
        ];
        let mut quality = vec![
            SnfFrameQualityV1 {
                version: 777,
                ..Default::default()
            };
            cap
        ];
        let mut written = 99;
        assert_eq!(
            snf_stream_push_with_quality_v1(
                handle,
                chunk.as_ptr(),
                chunk.len(),
                out.as_mut_ptr(),
                cap,
                quality.as_mut_ptr(),
                cap,
                2,
                56,
                &mut written
            ),
            -5
        );
        assert_eq!(written, 0);
        assert_eq!(
            snf_stream_push_with_quality_v1(
                handle,
                chunk.as_ptr(),
                chunk.len(),
                out.as_mut_ptr(),
                cap,
                quality.as_mut_ptr(),
                0,
                1,
                56,
                &mut written
            ),
            -4
        );
        assert_eq!(written, 0);
        assert!(out.iter().all(|f| f.n == -123.0));
        assert!(quality.iter().all(|q| q.version == 777));
        assert_eq!(snf_stream_diagnostics_v1(handle, &mut snapshot, 1, 104), 0);
        assert_eq!(snapshot, before);
        assert_eq!(snf_stream_diagnostics_v1(handle, &mut snapshot, 2, 104), -5);
        assert_eq!(snapshot, before);
        assert_eq!(
            snf_stream_push_with_quality_v1(
                handle,
                chunk.as_ptr(),
                chunk.len(),
                out.as_mut_ptr(),
                cap,
                quality.as_mut_ptr(),
                cap,
                1,
                56,
                &mut written
            ),
            0
        );
        assert!(written > 0);
        for (f, q) in out[..written].iter().zip(&quality[..written]) {
            assert_eq!(q.version, 1);
            assert_eq!(q.t_frame_index, f.t_frame_index);
            assert_eq!(q.quality_bits, 0);
        }
        assert_eq!(
            snf_stream_flush_with_quality_v1(
                handle,
                out.as_mut_ptr(),
                cap,
                quality.as_mut_ptr(),
                cap,
                1,
                56,
                &mut written
            ),
            0
        );
        assert_eq!(
            snf_stream_push_with_quality_v1(
                handle,
                std::ptr::null(),
                0,
                out.as_mut_ptr(),
                cap,
                quality.as_mut_ptr(),
                cap,
                1,
                56,
                &mut written
            ),
            -6
        );
        assert_eq!(written, 0);
        assert_eq!(snf_stream_diagnostics_v1(handle, &mut snapshot, 1, 104), 0);
        assert_eq!(
            snf_stream_ack_diagnostics_v1(handle, snapshot.sequence + 1),
            -9
        );
        assert_eq!(
            snf_stream_reset_with_diagnostics_v1(handle, &mut snapshot, 1, 104),
            0
        );
        let previous_session = snapshot.session_id;
        assert_eq!(snf_stream_diagnostics_v1(handle, &mut snapshot, 1, 104), 0);
        assert_eq!(snapshot.session_id, previous_session + 1);
        snf_stream_free(handle);
    }
}

#[test]
fn null_precedes_version_and_zeroes_only_written() {
    unsafe {
        let mut written = 13;
        assert_eq!(
            snf_stream_push_with_quality_v1(
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
                9,
                0,
                &mut written
            ),
            -1
        );
        assert_eq!(written, 0);
        assert_eq!(snf_stream_ack_diagnostics_v1(std::ptr::null_mut(), 0), -1);
    }
}

#[test]
fn ffi_quality_matches_rust_on_signed_synthetic_chunks() {
    unsafe {
        let handle = snf_stream_new(SNF_FIELD_FREE);
        let mut rust = soneflow::ZwtvStream::new(soneflow::FieldType::Free);
        for len in [1, 23, 97, 480, 0, 19] {
            let mut chunk: Vec<_> = (0..len).map(|i| ((i % 31) as f64 - 15.0) * 1e-4).collect();
            if len == 19 {
                chunk[0] = f64::MAX;
            }
            let cap = snf_stream_max_frames(len).max(1);
            let mut frames = vec![SnfStreamFrame::default(); cap];
            let mut quality = vec![SnfFrameQualityV1::default(); cap];
            let mut expected = vec![soneflow::StreamFrame::default(); cap];
            let mut expected_quality = vec![soneflow::FrameQualityV1::default(); cap];
            let expected_len = rust
                .push_with_quality(&chunk, &mut expected, &mut expected_quality)
                .unwrap();
            let mut written = 0;
            assert_eq!(
                snf_stream_push_with_quality_v1(
                    handle,
                    chunk.as_ptr(),
                    len,
                    frames.as_mut_ptr(),
                    cap,
                    quality.as_mut_ptr(),
                    cap,
                    1,
                    56,
                    &mut written
                ),
                0
            );
            assert_eq!(written, expected_len);
            for i in 0..written {
                assert_eq!(frames[i].n.to_bits(), expected[i].n.to_bits());
                assert_eq!(frames[i].n_phon.to_bits(), expected[i].n_phon.to_bits());
                assert_eq!(frames[i].t_frame_index, expected[i].t_frame_index);
                assert_eq!(frames[i].flags, expected[i].flags.bits());
                let actual = std::slice::from_raw_parts(
                    (&quality[i] as *const SnfFrameQualityV1).cast::<u8>(),
                    56,
                );
                let wanted = std::slice::from_raw_parts(
                    (&expected_quality[i] as *const soneflow::FrameQualityV1).cast::<u8>(),
                    56,
                );
                assert_eq!(actual, wanted);
            }
            let mut snapshot = SnfDiagnosticSnapshotV1::default();
            assert_eq!(snf_stream_diagnostics_v1(handle, &mut snapshot, 1, 104), 0);
            let expected_snapshot = rust.diagnostics();
            let actual = std::slice::from_raw_parts(
                (&snapshot as *const SnfDiagnosticSnapshotV1).cast::<u8>(),
                104,
            );
            let wanted = std::slice::from_raw_parts(
                (&expected_snapshot as *const soneflow::DiagnosticSnapshotV1).cast::<u8>(),
                104,
            );
            assert_eq!(actual, wanted);
        }
        snf_stream_free(handle);
    }
}

#[cfg(target_arch = "x86_64")]
mod environment {
    use super::*;

    fn control() -> u32 {
        let mut value = 0;
        unsafe {
            std::arch::asm!("stmxcsr [{p}]", p = in(reg) &mut value, options(nostack, preserves_flags));
        }
        value
    }
    fn set_control(value: u32) {
        unsafe {
            std::arch::asm!("ldmxcsr [{p}]", p = in(reg) &value, options(readonly, nostack, preserves_flags));
        }
    }
    struct Restore(u32);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_control(self.0);
        }
    }

    #[test]
    fn environment_errors_preserve_payloads_and_creation_is_sticky() {
        unsafe {
            let restore = Restore(control());
            set_control(restore.0 & !0x6000);
            let handle = snf_stream_new(SNF_FIELD_FREE);
            let mut frame = SnfStreamFrame {
                n: -7.0,
                ..Default::default()
            };
            let mut quality = SnfFrameQualityV1 {
                version: 777,
                ..Default::default()
            };
            let mut written = 17;
            set_control((restore.0 & !0x6000) | 0x2000);
            assert_eq!(
                snf_stream_flush_with_quality_v1(
                    handle,
                    &mut frame,
                    0,
                    &mut quality,
                    0,
                    1,
                    56,
                    &mut written
                ),
                -4
            );
            assert_eq!(
                snf_stream_flush_with_quality_v1(
                    handle,
                    &mut frame,
                    1,
                    &mut quality,
                    1,
                    1,
                    56,
                    &mut written
                ),
                -11
            );
            assert_eq!(written, 0);
            assert_eq!(frame.n.to_bits(), (-7.0f64).to_bits());
            assert_eq!(quality.version, 777);
            let mut snapshot = SnfDiagnosticSnapshotV1::default();
            assert_eq!(snf_stream_diagnostics_v1(handle, &mut snapshot, 1, 104), 0);
            assert_eq!(snapshot.incident_count, 0);
            snf_stream_free(handle);
            let created_non_rn = snf_stream_new(SNF_FIELD_FREE);
            set_control(restore.0 & !0x6000);
            assert_eq!(
                snf_stream_flush_with_quality_v1(
                    created_non_rn,
                    &mut frame,
                    1,
                    &mut quality,
                    1,
                    1,
                    56,
                    &mut written
                ),
                -11
            );
            assert_eq!(
                snf_stream_diagnostics_v1(created_non_rn, &mut snapshot, 1, 104),
                0
            );
            assert_ne!(snapshot.status_bits & 32, 0);
            assert_eq!(
                snf_stream_reset_with_diagnostics_v1(created_non_rn, &mut snapshot, 1, 104),
                0
            );
            assert_eq!(
                snf_stream_flush_with_quality_v1(
                    created_non_rn,
                    &mut frame,
                    1,
                    &mut quality,
                    1,
                    1,
                    56,
                    &mut written
                ),
                -11
            );
            snf_stream_free(created_non_rn);
        }
    }
}
