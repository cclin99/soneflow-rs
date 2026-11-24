/* Self-authored synthetic native consumer. No reference programs or data. */
#include "soneflow.h"
#include <assert.h>
#include <float.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

_Static_assert(sizeof(SnfStreamFrame) == 32, "legacy frame ABI");
_Static_assert(sizeof(SnfFrameQualityV1) == 56, "quality ABI");
_Static_assert(sizeof(SnfDiagnosticSnapshotV1) == 104, "diagnostic ABI");
_Static_assert(_Alignof(SnfStreamFrame) == 8, "frame alignment");
_Static_assert(_Alignof(SnfFrameQualityV1) == 8, "quality alignment");
_Static_assert(_Alignof(SnfDiagnosticSnapshotV1) == 8, "snapshot alignment");
_Static_assert(offsetof(SnfStreamFrame, n) == 8, "legacy n offset");
_Static_assert(offsetof(SnfStreamFrame, flags) == 24, "legacy flags offset");
_Static_assert(offsetof(SnfFrameQualityV1, t_frame_index) == 8, "quality index offset");
_Static_assert(offsetof(SnfFrameQualityV1, reason_mask) == 52, "quality tail offset");
_Static_assert(offsetof(SnfDiagnosticSnapshotV1, session_id) == 40, "session offset");
_Static_assert(offsetof(SnfDiagnosticSnapshotV1, covered_through_internal) == 96, "snapshot tail");
_Static_assert(SNF_STREAM_FLAG_CLAMPED_120DB == 1, "legacy clamp flag");
_Static_assert(SNF_STREAM_FLAG_NONFINITE_INPUT == 2, "legacy nonfinite flag");
_Static_assert(SNF_STREAM_FLAG_WARMUP == 4, "legacy warmup flag");

static SnfDiagnosticSnapshotV1 peek(SnfStream *stream) {
    SnfDiagnosticSnapshotV1 snapshot;
    memset(&snapshot, 0xa5, sizeof(snapshot));
    assert(snf_stream_diagnostics_v1(stream, &snapshot, 1, sizeof(snapshot)) == 0);
    assert(snapshot.version == 1);
    assert(snapshot.struct_size == sizeof(snapshot));
    assert(snapshot.reserved == 0);
    return snapshot;
}

int main(void) {
    SnfStream *stream = snf_stream_new(SNF_FIELD_FREE);
    SnfStreamFrame frames[32], saved_frames[32];
    SnfFrameQualityV1 quality[32], saved_quality[32];
    SnfDiagnosticSnapshotV1 before, after, previous;
    double signal[480];
    size_t cap = snf_stream_max_frames(480), written = 99, i;
    int saw_fallback = 0;
    assert(stream != NULL && cap <= 32);
    for (i = 0; i < 480; ++i) signal[i] = ((double)(i % 31) - 15.0) * 1e-4;

    before = peek(stream);
    memset(frames, 0xa5, sizeof(frames));
    memset(quality, 0x5a, sizeof(quality));
    memcpy(saved_frames, frames, sizeof(frames));
    memcpy(saved_quality, quality, sizeof(quality));
    assert(snf_stream_push_with_quality_v1(stream, signal, 480, frames, cap,
        quality, 0, 1, sizeof(*quality), &written) == SNF_ERR_INTERNAL);
    assert(written == 0);
    assert(memcmp(frames, saved_frames, sizeof(frames)) == 0);
    assert(memcmp(quality, saved_quality, sizeof(quality)) == 0);
    after = peek(stream);
    assert(memcmp(&before, &after, sizeof(before)) == 0);
    assert(snf_stream_push_with_quality_v1(stream, signal, 480, frames, cap,
        quality, cap, 2, sizeof(*quality), &written) == SNF_ERR_UNSUPPORTED_VERSION);
    assert(written == 0);
    assert(memcmp(frames, saved_frames, sizeof(frames)) == 0);
    assert(memcmp(quality, saved_quality, sizeof(quality)) == 0);

    assert(snf_stream_push_with_quality_v1(stream, signal, 480, frames, cap,
        quality, cap, 1, sizeof(*quality), &written) == SNF_OK);
    assert(written > 0 && written <= cap);
    for (i = 0; i < written; ++i) {
        assert(isfinite(frames[i].n) && frames[i].n >= 0.0);
        assert(frames[i]._reserved == 0);
        assert(quality[i].version == 1 && quality[i].quality_bits == 0);
        assert(quality[i].t_frame_index == frames[i].t_frame_index);
        assert(quality[i].stage_mask == 0 && quality[i].reason_mask == 0);
    }
    before = peek(stream);
    previous = before;
    assert(snf_stream_reset_with_diagnostics_v1(stream, &previous, 1, 103)
        == SNF_ERR_UNSUPPORTED_VERSION);
    assert(memcmp(&previous, &before, sizeof(before)) == 0);
    after = peek(stream);
    assert(memcmp(&after, &before, sizeof(before)) == 0);
    assert(snf_stream_reset_with_diagnostics_v1(stream, &previous, 1, sizeof(previous)) == 0);
    assert(memcmp(&previous, &before, sizeof(before)) == 0);
    after = peek(stream);
    assert(after.session_id == before.session_id + 1);

    for (i = 0; i < 480; ++i) signal[i] = DBL_MAX;
    assert(snf_stream_push_with_quality_v1(stream, signal, 480, frames, cap,
        quality, cap, 1, sizeof(*quality), &written) == 0);
    for (i = 0; i < written; ++i) {
        assert(isfinite(frames[i].n) && isfinite(frames[i].n_phon));
        assert((frames[i].flags & SNF_STREAM_FLAG_NONFINITE_INPUT) == 0);
        if (quality[i].quality_bits & SNF_QUALITY_FAULT_FALLBACK) {
            saw_fallback = 1;
            assert(frames[i].n == 0.0 && frames[i].n_phon == 0.0);
        }
    }
    assert(saw_fallback);
    before = peek(stream);
    assert(before.status_bits & SNF_DIAGNOSTIC_HAS_INCIDENT);
    assert(before.incident_count > 0 && before.reason_mask != 0);
    after = peek(stream);
    assert(memcmp(&before, &after, sizeof(before)) == 0);
    assert(snf_stream_ack_diagnostics_v1(stream, before.sequence + 1) == SNF_ERR_STALE_SEQUENCE);
    after = peek(stream);
    assert(memcmp(&before, &after, sizeof(before)) == 0);
    assert(snf_stream_flush_with_quality_v1(stream, frames, cap, quality, cap,
        1, sizeof(*quality), &written) == 0);
    snf_stream_free(stream);
    puts("PASS native C ABI/layout, quality, diagnostics/reset, atomic rejection, finite-MAX fallback");
    return 0;
}
