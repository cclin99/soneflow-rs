"""Synthetic-only additive quality binding contract; no reference material."""
import numpy as np
import pytest
import soneflow


def test_quality_keeps_legacy_arrays_and_supplies_structured_rows():
    signal = ((np.arange(480) % 31).astype(np.float64) - 15.0) * 1e-4
    legacy = soneflow.ZwtvStream()
    checked = soneflow.ZwtvStream()
    old = legacy.push(signal)
    new = checked.push_with_quality(signal)
    assert len(old) == 4 and len(new) == 5
    for actual, expected in zip(new[:4], old):
        assert actual.dtype == expected.dtype
        assert actual.tobytes() == expected.tobytes()
    quality = new[4]
    assert quality.dtype.itemsize == 56
    assert quality.dtype.names == (
        "version", "quality_bits", "t_frame_index", "session_id", "recovery_generation",
        "first_affected_internal", "through_internal", "stage_mask", "reason_mask")
    np.testing.assert_array_equal(quality["t_frame_index"], new[2])
    assert np.all(quality["version"] == 1)
    assert np.all(quality["quality_bits"] == 0)


def test_diagnostics_peek_stale_ack_reset_and_flush():
    stream = soneflow.ZwtvStream()
    before = stream.diagnostics
    assert before == stream.diagnostics
    with pytest.raises(RuntimeError):
        stream.ack_diagnostics(before["sequence"] + 1)
    assert stream.diagnostics == before
    stream.push_with_quality(np.zeros(25))
    assert len(stream.flush_with_quality()) == 5
    assert all(len(a) == 0 for a in stream.flush_with_quality())
    with pytest.raises(RuntimeError):
        stream.push_with_quality(np.zeros(0))
    previous = stream.diagnostics
    assert stream.reset_with_diagnostics() == previous
    assert stream.diagnostics["session_id"] == previous["session_id"] + 1


def test_dtype_shape_rejection_does_not_change_diagnostics():
    stream = soneflow.ZwtvStream()
    before = stream.diagnostics
    with pytest.raises((TypeError, ValueError)):
        stream.push_with_quality(np.zeros(8, dtype=np.float32))
    with pytest.raises((TypeError, ValueError)):
        stream.push_with_quality(np.zeros((2, 2)))
    assert stream.diagnostics == before


def test_internal_numeric_batch_fault_is_runtime_error():
    with pytest.raises(RuntimeError):
        soneflow.loudness_zwst(np.full(48_000, np.finfo(np.float64).max), 48_000.0)
    with pytest.raises(ValueError):
        soneflow.loudness_zwst(np.full(48_000, np.inf), 48_000.0)


def test_numeric_fault_quality_and_sticky_snapshot_are_exposed():
    stream = soneflow.ZwtvStream()
    result = stream.push_with_quality(np.full(480, np.finfo(np.float64).max))
    assert np.all(np.isfinite(result[0]))
    assert np.any(result[4]["quality_bits"] & soneflow.QUALITY_FAULT_FALLBACK)
    snapshot = stream.diagnostics
    assert snapshot["status_bits"] & soneflow.DIAGNOSTIC_HAS_INCIDENT
    assert snapshot["incident_count"] > 0
    assert snapshot == stream.diagnostics
    assert snapshot["reason_mask"] != 0
