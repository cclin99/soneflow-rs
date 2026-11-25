//! C ABI v1 for the soneflow crate: batch and stateful streaming APIs.
//!
//! Every extern fn that can panic is wrapped in `catch_unwind` (panic -> -2);
//! this crate must never be built with `panic = "abort"`.

// The doc comments describe C buffers in C array notation (`out_n[frames]`)
// and cbindgen copies them verbatim into include/soneflow.h, so they are not
// rustdoc links.
#![allow(rustdoc::broken_intra_doc_links)]

pub const SNF_OK: i32 = 0;
pub const SNF_ERR_LEVEL_EXCEEDS_120DB: i32 = 1;
pub const SNF_ERR_SIGNAL_TOO_SHORT: i32 = 2;
pub const SNF_ERR_UNSUPPORTED_SAMPLE_RATE: i32 = 3;
/// A batch input (`snf_loudness_zwst`, `snf_loudness_zwtv`) contains a
/// NaN or infinite sample; nothing is written to the output buffers. The
/// stream functions never return this code: they zero the sample and set
/// SNF_STREAM_FLAG_NONFINITE_INPUT instead.
pub const SNF_ERR_NONFINITE_INPUT: i32 = 4;
pub const SNF_ERR_INTERNAL_NUMERIC_FAULT: i32 = 5;
pub const SNF_ERR_UNSUPPORTED_VERSION: i32 = -5;
pub const SNF_ERR_LIFECYCLE: i32 = -6;
pub const SNF_ERR_TIMELINE_EXHAUSTED: i32 = -7;
pub const SNF_ERR_SESSION_EXHAUSTED: i32 = -8;
pub const SNF_ERR_STALE_SEQUENCE: i32 = -9;
pub const SNF_ERR_COUNTER_SATURATED: i32 = -10;
pub const SNF_ERR_UNSUPPORTED_ROUNDING: i32 = -11;
pub const SNF_ERR_UNSUPPORTED_ENVIRONMENT: i32 = -12;
pub const SNF_QUALITY_FAULT_FALLBACK: u32 = 1;
pub const SNF_QUALITY_RECOVERING: u32 = 2;
pub const SNF_QUALITY_VERSION: u32 = 1;
pub const SNF_QUALITY_V1_SIZE: usize = 56;
pub const SNF_DIAGNOSTIC_V1_SIZE: usize = 104;
pub const SNF_DIAGNOSTIC_HAS_INCIDENT: u32 = 1;
pub const SNF_DIAGNOSTIC_ACTIVE_RECOVERY: u32 = 2;
pub const SNF_DIAGNOSTIC_TAIL_PENDING: u32 = 4;
pub const SNF_DIAGNOSTIC_COALESCED: u32 = 8;
pub const SNF_DIAGNOSTIC_COUNTER_SATURATED: u32 = 16;
pub const SNF_DIAGNOSTIC_CREATION_ENV_INVALID: u32 = 32;
pub const SNF_REASON_NONFINITE: u32 = 1;
pub const SNF_REASON_NEGATIVE_DOMAIN: u32 = 2;
pub const SNF_REASON_ABOVE_VALID_DOMAIN: u32 = 4;
pub const SNF_REASON_UNSUPPORTED_ROUNDING: u32 = 8;
pub const SNF_REASON_UNSUPPORTED_ENVIRONMENT: u32 = 16;
pub const SNF_ERR_NULL_POINTER: i32 = -1;
pub const SNF_ERR_PANIC: i32 = -2;
pub const SNF_ERR_INVALID_FIELD_TYPE: i32 = -3;
/// An internal invariant was broken. Stream push/flush also use this code to
/// reject an insufficient out_cap without writing a partial result.
pub const SNF_ERR_INTERNAL: i32 = -4;
/// Valid `field_type` value: free field.
pub const SNF_FIELD_FREE: i32 = 0;
/// Valid `field_type` value: diffuse field.
pub const SNF_FIELD_DIFFUSE: i32 = 1;
pub const SNF_STREAM_FLAG_CLAMPED_120DB: u32 = 1;
pub const SNF_STREAM_FLAG_NONFINITE_INPUT: u32 = 2;
pub const SNF_STREAM_FLAG_WARMUP: u32 = 4;
use std::panic::{catch_unwind, AssertUnwindSafe};

use soneflow::{loudness_zwst, loudness_zwtv, FieldType};

/// 統一 panic 邊界:所有 extern fn 的函式體都必須整體通過這裡。
fn guarded_or<T>(default: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(default)
}

fn guarded(f: impl FnOnce() -> i32) -> i32 {
    guarded_or(SNF_ERR_PANIC, f)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
/// One 2 ms stream output frame.
///
/// `flags` uses bit 1 for CLAMPED_120DB, bit 2 for NONFINITE_INPUT, and bit 4
/// for WARMUP. WARMUP is set while `t_frame_index < 580`.
pub struct SnfStreamFrame {
    pub t_frame_index: u64,
    pub n: f64,
    pub n_phon: f64,
    pub flags: u32,
    pub _reserved: u32,
}

/// Parallel quality record; version 1, exactly 56 bytes. Numeric fault and
/// recovery values are not valid measurements. Original frame flags still apply.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SnfFrameQualityV1 {
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

/// Non-consuming conservative incident summary; version 1, exactly 104 bytes.
/// Legacy callers must query after every processing call and after flush.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SnfDiagnosticSnapshotV1 {
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

macro_rules! check_layout {
    ($ffi:ty, $core:ty, $size:expr; $($field:ident),+ $(,)?) => {
        const _: () = {
            assert!(std::mem::size_of::<$ffi>() == $size);
            assert!(std::mem::size_of::<$core>() == $size);
            assert!(std::mem::align_of::<$ffi>() == std::mem::align_of::<$core>());
            $(assert!(std::mem::offset_of!($ffi, $field) == std::mem::offset_of!($core, $field));)+
        };
    };
}
check_layout!(SnfFrameQualityV1, soneflow::FrameQualityV1, 56;
    version, quality_bits, t_frame_index, session_id, recovery_generation,
    first_affected_internal, through_internal, stage_mask, reason_mask);
check_layout!(SnfDiagnosticSnapshotV1, soneflow::DiagnosticSnapshotV1, 104;
    version, struct_size, status_bits, stage_mask, reason_mask, first_stage, first_band,
    latest_stage, latest_band, reserved, session_id, recovery_generation, sequence,
    incident_count, first_detection_sample, last_detection_sample,
    first_affected_internal, covered_through_internal);

/// Opaque stream handle allocated by snf_stream_new and released by
/// snf_stream_free. A handle has no internal lock and must not be called
/// concurrently from multiple threads.
pub struct SnfStream {
    inner: soneflow::ZwtvStream,
}

const _: () = {
    assert!(std::mem::size_of::<SnfStreamFrame>() == std::mem::size_of::<soneflow::StreamFrame>());
    assert!(
        std::mem::align_of::<SnfStreamFrame>() == std::mem::align_of::<soneflow::StreamFrame>()
    );
    assert!(
        std::mem::offset_of!(SnfStreamFrame, t_frame_index)
            == std::mem::offset_of!(soneflow::StreamFrame, t_frame_index)
    );
    assert!(
        std::mem::offset_of!(SnfStreamFrame, n) == std::mem::offset_of!(soneflow::StreamFrame, n)
    );
    assert!(
        std::mem::offset_of!(SnfStreamFrame, n_phon)
            == std::mem::offset_of!(soneflow::StreamFrame, n_phon)
    );
    assert!(
        std::mem::offset_of!(SnfStreamFrame, flags)
            == std::mem::offset_of!(soneflow::StreamFrame, flags)
    );
    assert!(
        std::mem::offset_of!(SnfStreamFrame, _reserved)
            == std::mem::offset_of!(soneflow::StreamFrame, _reserved)
    );
    assert!(SNF_STREAM_FLAG_CLAMPED_120DB == soneflow::FrameFlags::CLAMPED_120DB.bits());
    assert!(SNF_STREAM_FLAG_NONFINITE_INPUT == soneflow::FrameFlags::NONFINITE_INPUT.bits());
    assert!(SNF_STREAM_FLAG_WARMUP == soneflow::FrameFlags::WARMUP.bits());
};

#[no_mangle]
/// Allocate a baked-in 48 kHz stream with 24 samples (one internal frame) of
/// latency. An invalid field_type returns NULL.
pub extern "C" fn snf_stream_new(field_type: i32) -> *mut SnfStream {
    let Ok(field) = FieldType::try_from(field_type) else {
        return std::ptr::null_mut();
    };
    guarded_or(std::ptr::null_mut(), || {
        Box::into_raw(Box::new(SnfStream {
            inner: soneflow::ZwtvStream::new(field),
        }))
    })
}

#[no_mangle]
pub extern "C" fn snf_stream_max_frames(chunk_len: usize) -> usize {
    soneflow::ZwtvStream::max_frames_for_chunk(chunk_len)
}

/// Return pending flags observed after the most recent output frame. Before
/// flush the value is provisional and will be attached to the next output
/// frame. Only after flush does it represent undelivered tail events. A null
/// handle returns zero. After a prior push or flush returned -2, the value is
/// undefined and only snf_stream_free may be called.
///
/// # Safety
/// A non-null handle must be live and must not be accessed concurrently.
#[no_mangle]
pub unsafe extern "C" fn snf_stream_residual_flags(handle: *const SnfStream) -> u32 {
    guarded_or(0, || {
        if handle.is_null() {
            return 0;
        }
        unsafe { (*handle).inner.residual_flags().bits() }
    })
}

/// Push a 48 kHz signal chunk into a stream handle.
///
/// `out` must hold at least `snf_stream_max_frames(chunk_len)` frames. An
/// insufficient out_cap returns -4, sets `*out_written` to zero, and writes no
/// partial result. A panic returns -2 and poisons the handle; only
/// snf_stream_free may be called afterward. A push after flush also returns
/// -2 through the internal assertion and has the same poisoned-handle rule.
///
/// # Safety
/// The handle must be live, chunk must contain chunk_len readable doubles,
/// out must contain out_cap writable frames, and out_written must be writable.
#[no_mangle]
pub unsafe extern "C" fn snf_stream_push(
    handle: *mut SnfStream,
    chunk: *const f64,
    chunk_len: usize,
    out: *mut SnfStreamFrame,
    out_cap: usize,
    out_written: *mut usize,
) -> i32 {
    guarded(|| {
        if out_written.is_null() {
            return SNF_ERR_NULL_POINTER;
        }
        unsafe { *out_written = 0 };
        if handle.is_null() || (chunk.is_null() && chunk_len > 0) || out.is_null() {
            return SNF_ERR_NULL_POINTER;
        }
        if out_cap < snf_stream_max_frames(chunk_len) {
            return SNF_ERR_INTERNAL;
        }
        let chunk = if chunk_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(chunk, chunk_len) }
        };
        let stream = unsafe { &mut (*handle).inner };
        let out =
            unsafe { std::slice::from_raw_parts_mut(out.cast::<soneflow::StreamFrame>(), out_cap) };
        let written = stream.push(chunk, out);
        unsafe { *out_written = written };
        SNF_OK
    })
}

/// Flush the final lookahead frame. out_cap must be at least one. *out_written
/// is zero or one: one frame is written only when the final internal frame is
/// on the 2 ms output grid; zero is not an error. After flush, residual flags,
/// diagnostics/acknowledgment, checked reset and free remain available.
///
/// # Safety
/// The handle must be live, out must contain at least one writable frame,
/// and out_written must be writable.
#[no_mangle]
pub unsafe extern "C" fn snf_stream_flush(
    handle: *mut SnfStream,
    out: *mut SnfStreamFrame,
    out_cap: usize,
    out_written: *mut usize,
) -> i32 {
    guarded(|| {
        if out_written.is_null() {
            return SNF_ERR_NULL_POINTER;
        }
        unsafe { *out_written = 0 };
        if handle.is_null() || out.is_null() {
            return SNF_ERR_NULL_POINTER;
        }
        if out_cap < 1 {
            return SNF_ERR_INTERNAL;
        }
        let stream = unsafe { &mut (*handle).inner };
        let out =
            unsafe { std::slice::from_raw_parts_mut(out.cast::<soneflow::StreamFrame>(), out_cap) };
        let written = stream.flush(out);
        unsafe { *out_written = written };
        SNF_OK
    })
}

/// Checked processing with a parallel quality buffer. Prefer this API for
/// measurement validity; legacy callers must query diagnostics after each call.
/// Version must be 1 and struct_size 56. Usage errors leave payloads and state
/// unchanged; out_written is set to zero. Unexpected panic returns -2 and poisons
/// the handle. Only free is valid after -2.
///
/// # Safety
/// All pointers must be aligned, valid for their stated lengths and disjoint;
/// handle must be live and exclusively accessed. A null chunk is allowed only
/// when chunk_len is zero. Caller memory must remain valid for the entire call.
#[no_mangle]
pub unsafe extern "C" fn snf_stream_push_with_quality_v1(
    handle: *mut SnfStream,
    chunk: *const f64,
    chunk_len: usize,
    out: *mut SnfStreamFrame,
    out_cap: usize,
    quality: *mut SnfFrameQualityV1,
    quality_cap: usize,
    version: u32,
    struct_size: usize,
    out_written: *mut usize,
) -> i32 {
    guarded(|| {
        if out_written.is_null() {
            return SNF_ERR_NULL_POINTER;
        }
        unsafe {
            *out_written = 0;
        }
        if handle.is_null()
            || out.is_null()
            || quality.is_null()
            || (chunk.is_null() && chunk_len != 0)
        {
            return SNF_ERR_NULL_POINTER;
        }
        if version != 1 || struct_size != 56 {
            return SNF_ERR_UNSUPPORTED_VERSION;
        }
        // SAFETY: caller owns valid non-overlapping storage; layouts are asserted above.
        let (stream, chunk, out, quality) = unsafe {
            (
                &mut (*handle).inner,
                if chunk_len == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(chunk, chunk_len)
                },
                std::slice::from_raw_parts_mut(out.cast::<soneflow::StreamFrame>(), out_cap),
                std::slice::from_raw_parts_mut(
                    quality.cast::<soneflow::FrameQualityV1>(),
                    quality_cap,
                ),
            )
        };
        match stream.push_with_quality(chunk, out, quality) {
            Ok(written) => {
                unsafe {
                    *out_written = written;
                }
                SNF_OK
            }
            Err(error) => error.code(),
        }
    })
}

/// Checked flush; version=1, struct_size=56, each output capacity at least one.
/// Repeated flushes still validate the call and return zero written on success.
///
/// # Safety
/// Same handle, alignment, exclusive-access and disjoint-buffer requirements as
/// snf_stream_push_with_quality_v1; out_written must be writable.
#[no_mangle]
pub unsafe extern "C" fn snf_stream_flush_with_quality_v1(
    handle: *mut SnfStream,
    out: *mut SnfStreamFrame,
    out_cap: usize,
    quality: *mut SnfFrameQualityV1,
    quality_cap: usize,
    version: u32,
    struct_size: usize,
    out_written: *mut usize,
) -> i32 {
    guarded(|| {
        if out_written.is_null() {
            return SNF_ERR_NULL_POINTER;
        }
        unsafe {
            *out_written = 0;
        }
        if handle.is_null() || out.is_null() || quality.is_null() {
            return SNF_ERR_NULL_POINTER;
        }
        if version != 1 || struct_size != 56 {
            return SNF_ERR_UNSUPPORTED_VERSION;
        }
        // SAFETY: valid caller storage and compile-time checked layout correspondence.
        let (stream, out, quality) = unsafe {
            (
                &mut (*handle).inner,
                std::slice::from_raw_parts_mut(out.cast::<soneflow::StreamFrame>(), out_cap),
                std::slice::from_raw_parts_mut(
                    quality.cast::<soneflow::FrameQualityV1>(),
                    quality_cap,
                ),
            )
        };
        match stream.flush_with_quality(out, quality) {
            Ok(written) => {
                unsafe {
                    *out_written = written;
                }
                SNF_OK
            }
            Err(error) => error.code(),
        }
    })
}

/// Peek without consuming diagnostics, including after flush. Version=1, size=104.
/// On error the snapshot payload is unchanged.
///
/// # Safety
/// Handle must be live, non-poisoned and exclusively accessed; snapshot must be
/// aligned writable storage, disjoint from handle.
#[no_mangle]
pub unsafe extern "C" fn snf_stream_diagnostics_v1(
    handle: *const SnfStream,
    snapshot: *mut SnfDiagnosticSnapshotV1,
    version: u32,
    struct_size: usize,
) -> i32 {
    guarded(|| {
        if handle.is_null() || snapshot.is_null() {
            return SNF_ERR_NULL_POINTER;
        }
        if version != 1 || struct_size != 104 {
            return SNF_ERR_UNSUPPORTED_VERSION;
        }
        // SAFETY: pointer contract above and identical POD layouts asserted above.
        unsafe {
            snapshot
                .cast::<soneflow::DiagnosticSnapshotV1>()
                .write((*handle).inner.diagnostics());
        }
        SNF_OK
    })
}

/// Acknowledge exactly the current sequence; stale/saturated errors change nothing.
/// Active recovery is not erased by acknowledgment.
///
/// # Safety
/// Handle must be live, non-poisoned and exclusively accessed.
#[no_mangle]
pub unsafe extern "C" fn snf_stream_ack_diagnostics_v1(
    handle: *mut SnfStream,
    expected_sequence: u64,
) -> i32 {
    guarded(|| {
        if handle.is_null() {
            return SNF_ERR_NULL_POINTER;
        }
        // SAFETY: live exclusive handle by caller contract.
        match unsafe { (*handle).inner.ack_diagnostics(expected_sequence) } {
            Ok(()) => SNF_OK,
            Err(error) => error.code(),
        }
    })
}

/// Atomically return the last snapshot and start a new session. Version=1,size=104.
/// Session exhaustion returns -8 without changing state or snapshot payload.
///
/// # Safety
/// Handle must be live, non-poisoned and exclusively accessed; snapshot must be
/// aligned writable storage disjoint from handle.
#[no_mangle]
pub unsafe extern "C" fn snf_stream_reset_with_diagnostics_v1(
    handle: *mut SnfStream,
    snapshot: *mut SnfDiagnosticSnapshotV1,
    version: u32,
    struct_size: usize,
) -> i32 {
    guarded(|| {
        if handle.is_null() || snapshot.is_null() {
            return SNF_ERR_NULL_POINTER;
        }
        if version != 1 || struct_size != 104 {
            return SNF_ERR_UNSUPPORTED_VERSION;
        }
        // SAFETY: live exclusive handle; layout correspondence checked at compile time.
        match unsafe { (*handle).inner.reset_with_diagnostics() } {
            Ok(previous) => {
                unsafe {
                    snapshot
                        .cast::<soneflow::DiagnosticSnapshotV1>()
                        .write(previous);
                }
                SNF_OK
            }
            Err(error) => error.code(),
        }
    })
}

/// Free a stream handle. A null handle is accepted.
///
/// # Safety
/// A non-null handle must have been returned by snf_stream_new and not
/// previously freed.
#[no_mangle]
pub unsafe extern "C" fn snf_stream_free(handle: *mut SnfStream) {
    if handle.is_null() {
        return;
    }
    guarded_or((), || unsafe {
        drop(Box::from_raw(handle));
    });
}

/// Number of output frames `snf_loudness_zwtv` will write for a signal of
/// `signal_len` samples, on the ISO 2 ms output grid. Pure; does not validate
/// (validation happens in the main call). Forwards `soneflow::zwtv_out_frames`.
///
/// Output-grid contract (part of C ABI v1): frames = ceil(ceil(L/24)/4);
/// frame j is at exactly 96*j/48000 s (grid starts at 0) and depends only on
/// samples <= 96*j. The stream API yields the same count. A caller that needs
/// a shorter grid with the same step takes a prefix; its values do not change.
#[no_mangle]
pub extern "C" fn snf_zwtv_out_frames(signal_len: usize) -> usize {
    soneflow::zwtv_out_frames(signal_len)
}

/// Time-varying (zwtv) loudness. Caller allocates every buffer:
/// out_n[frames], out_n_specific[240*frames] (bark-major, row-major),
/// out_bark[240], out_time[frames]; frames = snf_zwtv_out_frames(signal_len).
/// Returns 0 on success (see error-code defines). Uses a process-wide thread
/// pool (rayon).
///
/// # Safety
/// `signal` must be non-null, 8-byte aligned (a valid `double*`), and valid
/// for `signal_len` reads; each out pointer must be valid (and 8-byte
/// aligned) for the writes documented above. `field_type` must be
/// SNF_FIELD_FREE (0) or SNF_FIELD_DIFFUSE (1); other values return
/// SNF_ERR_INVALID_FIELD_TYPE.
#[no_mangle]
pub unsafe extern "C" fn snf_loudness_zwtv(
    signal: *const f64,
    signal_len: usize,
    fs: f64,
    field_type: i32,
    out_n: *mut f64,
    out_n_specific: *mut f64,
    out_bark: *mut f64,
    out_time: *mut f64,
) -> i32 {
    guarded(|| {
        if signal.is_null()
            || out_n.is_null()
            || out_n_specific.is_null()
            || out_bark.is_null()
            || out_time.is_null()
        {
            return SNF_ERR_NULL_POINTER;
        }
        let Ok(field) = FieldType::try_from(field_type) else {
            return SNF_ERR_INVALID_FIELD_TYPE;
        };
        // SAFETY: 呼叫端契約(見函式 Safety 註解);closure 不繼承 unsafe fn
        // 的 unsafe 語境,故此處需明確 unsafe 區塊。
        let signal = unsafe { std::slice::from_raw_parts(signal, signal_len) };
        match loudness_zwtv(signal, fs, field) {
            Ok(r) => {
                let frames = r.n.len();
                if frames != soneflow::zwtv_out_frames(signal_len)
                    || r.n_specific.len() != 240 * frames
                    || r.bark_axis.len() != 240
                    || r.time_axis.len() != frames
                {
                    return SNF_ERR_INTERNAL;
                }
                // SAFETY: 呼叫端契約——各緩衝大小如上;來源為剛建構的 Vec。
                unsafe {
                    std::ptr::copy_nonoverlapping(r.n.as_ptr(), out_n, frames);
                    std::ptr::copy_nonoverlapping(
                        r.n_specific.as_ptr(),
                        out_n_specific,
                        240 * frames,
                    );
                    std::ptr::copy_nonoverlapping(r.bark_axis.as_ptr(), out_bark, 240);
                    std::ptr::copy_nonoverlapping(r.time_axis.as_ptr(), out_time, frames);
                }
                SNF_OK
            }
            Err(e) => e.code(),
        }
    })
}

/// Stationary (zwst) loudness. Caller allocates: out_n[1],
/// out_n_specific[240], out_bark[240]. Returns 0 on success.
///
/// # Safety
/// `signal` must be non-null, 8-byte aligned (a valid `double*`), and valid
/// for `signal_len` reads; each out pointer must be valid (and 8-byte
/// aligned) for the writes documented above. `field_type` must be
/// SNF_FIELD_FREE (0) or SNF_FIELD_DIFFUSE (1); other values return
/// SNF_ERR_INVALID_FIELD_TYPE.
#[no_mangle]
pub unsafe extern "C" fn snf_loudness_zwst(
    signal: *const f64,
    signal_len: usize,
    fs: f64,
    field_type: i32,
    out_n: *mut f64,
    out_n_specific: *mut f64,
    out_bark: *mut f64,
) -> i32 {
    guarded(|| {
        if signal.is_null() || out_n.is_null() || out_n_specific.is_null() || out_bark.is_null() {
            return SNF_ERR_NULL_POINTER;
        }
        let Ok(field) = FieldType::try_from(field_type) else {
            return SNF_ERR_INVALID_FIELD_TYPE;
        };
        // SAFETY: 呼叫端契約(見函式 Safety 註解)。
        let signal = unsafe { std::slice::from_raw_parts(signal, signal_len) };
        match loudness_zwst(signal, fs, field) {
            Ok(r) => {
                if r.n_specific.len() != 240 || r.bark_axis.len() != 240 {
                    return SNF_ERR_INTERNAL;
                }
                // SAFETY: 呼叫端契約——out_n 1 個、spec/bark 各 240 個 f64。
                unsafe {
                    *out_n = r.n;
                    std::ptr::copy_nonoverlapping(r.n_specific.as_ptr(), out_n_specific, 240);
                    std::ptr::copy_nonoverlapping(r.bark_axis.as_ptr(), out_bark, 240);
                }
                SNF_OK
            }
            Err(e) => e.code(),
        }
    })
}

// ---- panic 注入(僅 test-panic feature;不進 header)----

/// 驗證 guarded() 邊界。僅測試用;release 交付不含此符號。
#[cfg(feature = "test-panic")]
#[no_mangle]
pub extern "C" fn snf__test_panic() -> i32 {
    guarded(|| panic!("test-panic: direct"))
}

/// rayon 工作項 panic 在 join 點 resume——本函式證實它被 guarded() 接住。
#[cfg(feature = "test-panic")]
#[no_mangle]
pub extern "C" fn snf__test_panic_rayon() -> i32 {
    guarded(|| {
        use rayon::prelude::*;
        (0..64_i32).into_par_iter().for_each(|i| {
            if i == 33 {
                panic!("test-panic: inside rayon worker");
            }
        });
        SNF_OK
    })
}

#[cfg(feature = "test-panic")]
#[no_mangle]
pub extern "C" fn snf__test_panic_stream() -> i32 {
    guarded(|| {
        let _stream = soneflow::ZwtvStream::new(FieldType::Free);
        panic!("test-panic: stream path")
    })
}
