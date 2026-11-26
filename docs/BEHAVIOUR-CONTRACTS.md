# Behaviour contracts
> 對照／Pair: [中文版](BEHAVIOUR-CONTRACTS.zh-TW.md) · source: en · synced 2026-11-26

Each row below is one promise this library makes, with the tests that would fail if the promise were broken. Tests are written as `path::name` (path from the repository root). `python tools/contract_tests.py` checks that every listed test exists and that both language versions list the same tests; CI runs it.

All tests listed here run without external data and run in CI, except where §8 says otherwise. A promise with no test yet is listed in §7, not in a table.

## 1. Input

| promise | tests |
|---|---|
| Only 48 kHz input is accepted; any other rate is an error. | `soneflow/tests/api_errors.rs::loudness_zwst_rejects_unsupported_sample_rate`, `soneflow/tests/api_errors.rs::loudness_zwtv_rejects_unsupported_sample_rate` |
| Batch input shorter than the minimum length is an error; the shortest legal time-varying input is 4,800 samples (50 output points). | `soneflow/tests/api_errors.rs::loudness_zwst_rejects_short_signal`, `soneflow/tests/api_errors.rs::loudness_zwtv_rejects_short_signal`, `soneflow/tests/output_grid.rs::shortest_legal_input_is_4800_samples_and_50_points` |
| Input errors are reported in a fixed order: sample rate, then length, then finiteness. | `soneflow/tests/api_errors.rs::sample_rate_and_length_errors_take_precedence_over_finiteness`, `soneflow/tests/l4_batch_fault.rs::stationary_input_error_precedence_stays_rate_length_finiteness` |
| A non-finite batch sample is an error (C ABI code 4) that names the first offending sample; subnormal, zero and large finite samples are accepted. | `soneflow/tests/api_errors.rs::batch_rejects_nan_and_infinities_at_head_middle_and_tail`, `soneflow/tests/api_errors.rs::batch_reports_the_first_offending_sample`, `soneflow/tests/api_errors.rs::nonfinite_error_code_is_frozen_at_4`, `soneflow/tests/api_errors.rs::subnormal_zero_and_large_finite_samples_are_not_rejected_as_nonfinite`, `soneflow-py/tests/test_smoke.py::test_batch_rejects_nonfinite_input` |
| Third-octave levels above 120 dB in the bands below 300 Hz are rejected in batch; bands from the twelfth up are not limited. | `soneflow/tests/core_characterization.rs::a_low_band_above_120_db_is_rejected_and_120_db_is_accepted`, `soneflow/tests/core_characterization.rs::bands_from_the_twelfth_up_are_not_limited_to_120_db` |

## 2. Output grid and causality

| promise | tests |
|---|---|
| A time-varying result for `L` samples has `ceil(ceil(L/24)/4)` points, point `j` at `96j/48000` s. | `soneflow/tests/output_grid.rs::frame_count_is_ceil_of_ceil_for_every_field`, `soneflow/tests/output_grid.rs::time_axis_is_an_exact_2ms_grid_from_zero`, `soneflow-py/tests/test_smoke.py::test_zwtv_output_grid_contract`, `soneflow-ffi/tests/ffi.rs::out_frames_query_forwards_core_at_representative_boundaries` |
| Each output point depends only on the samples up to its time: a prefix gives the same points whatever follows it. | `soneflow/tests/output_grid.rs::every_point_is_prefix_determined_under_zero_padding`, `soneflow/tests/output_grid.rs::every_point_is_prefix_determined_under_real_continuation`, `soneflow/tests/nl_zero_init.rs::batch_prefix_is_invariant_to_the_future_tail` |
| The batch nonlinear decay starts from zero state; `NlInit::MosqitoTail` reproduces MoSQITo's seed for comparisons only. | `soneflow/tests/nl_zero_init.rs::silent_prefix_yields_exactly_zero_loudness_before_the_tone_starts`, `soneflow/tests/nl_zero_init.rs::mosqito_tail_init_reproduces_the_audited_pre_fix_numbers`, `soneflow/tests/nl_zero_init.rs::both_initialisations_agree_when_the_signal_ends_in_silence`, `soneflow-py/tests/test_smoke.py::test_bitwise_contract_mosqito_tail_init`, `soneflow-py/tests/test_smoke.py::test_nl_init_rejects_unknown_value` |
| A reused `ZwtvProcessor` gives the same result as a new one. | `soneflow/tests/nl_zero_init.rs::processor_reuse_does_not_leak_state_between_calls` |

## 3. Stream

| promise | tests |
|---|---|
| The stream gives the batch result bit for bit, from the first frame, and the same number of frames for any chunking. | `soneflow/tests/nl_zero_init.rs::batch_matches_the_causal_stream_bitwise_from_frame_zero`, `soneflow/tests/stream_contract.rs::batch_zerostate_reference_and_stream_are_bitwise_identical_from_frame_zero`, `soneflow/tests/output_grid.rs::stream_frame_count_equals_batch_for_any_chunking`, `soneflow-py/tests/test_stream.py::test_stream_matches_batch_after_warmup`, `soneflow-ffi/tests/ffi.rs::stream_matches_rust_stream_bitwise` |
| Output does not depend on how the input is split into chunks. | `soneflow/tests/stream_contract.rs::chunk_size_invariance_is_bitwise`, `soneflow-py/tests/test_stream.py::test_chunk_invariance_bitwise` |
| Latency is 24 input samples; the first 580 output frames are marked as warm-up. | `soneflow/tests/stream_contract.rs::stream_constants_and_single_push_reference`, `soneflow/tests/stream_contract.rs::warmup_constant_is_frozen`, `soneflow-py/tests/test_stream.py::test_constants_are_frozen` |
| After construction, `push`, `flush`, fault recovery, quality output, `ack_diagnostics` and `reset` allocate nothing and never start Rayon. | `soneflow/tests/stream_alloc.rs::push_and_flush_do_not_allocate`, `soneflow/tests/stream_fault_rt.rs::fault_recovery_quality_ack_and_reset_allocate_nothing_and_do_not_enter_rayon`, `soneflow/tests/stream_no_rayon.rs::stream_does_not_initialize_rayon_pool` |
| A non-finite stream sample is counted as zero and flagged (`NONFINITE_INPUT`), not an error; a flag after the last output frame is reported as a residual flag. | `soneflow/tests/api_errors.rs::stream_still_flags_instead_of_failing`, `soneflow/tests/stream_contract.rs::nonfinite_input_flags_and_recovers`, `soneflow/tests/stream_contract.rs::tail_nonfinite_input_is_reported_as_residual_flag`, `soneflow-py/tests/test_stream.py::test_nan_is_flagged_not_raised`, `soneflow-py/tests/test_stream.py::test_tail_nan_surfaces_in_residual_flags`, `soneflow-ffi/tests/ffi.rs::stream_tail_nonfinite_is_exposed_as_residual_flag` |
| Levels above 120 dB in the low bands are clamped and flagged (`CLAMPED_120DB`) in the stream, which keeps running. | `soneflow/tests/stream_contract.rs::over_120db_is_clamped_and_stream_continues` |
| `reset` returns the stream to the state of a new one, also after a flush of a partly non-finite chunk; `push` after `flush` needs a `reset`. | `soneflow/tests/stream_contract.rs::reset_matches_a_new_stream`, `soneflow/tests/stream_contract.rs::flush_reset_after_partial_nonfinite_chunk_matches_a_new_stream`, `soneflow-py/tests/test_stream.py::test_push_after_flush_raises_and_reset_recovers` |
| `push` and `flush` leave the caller's MXCSR as they found it. | `soneflow/tests/stream_contract.rs::push_and_flush_restore_mxcsr` |
| `max_frames_for_chunk` bounds the frames one `push` can write. | `soneflow-py/tests/test_stream.py::test_max_frames_for_chunk_bounds_push_output`, `soneflow-ffi/tests/ffi.rs::stream_max_frames_forwards_rust_contract`, `soneflow-ffi/tests/ffi.rs::stream_rejects_small_output_without_consuming_input` |

## 4. Numerics and backends

| promise | tests |
|---|---|
| AVX2 and scalar kernels give the same result; `simd::set_force_scalar` overrides the runtime detection. | `soneflow/tests/simd_parity.rs::filter_bank_avx2_matches_scalar`, `soneflow/tests/simd_parity.rs::nl_loudness_avx2_matches_scalar`, `soneflow/tests/simd_dispatch.rs::force_scalar_overrides_runtime_detection`, `soneflow/tests/stream_scalar.rs::forced_scalar_stream_matches_scalar_zerostate_reference` |
| Results are bitwise reproducible: repeated runs agree, and frozen hashes of reference outputs hold per platform. | `soneflow/tests/determinism.rs::zwtv_output_is_bitwise_deterministic_over_20_runs`, `soneflow/tests/hash_gate.rs::zwtv_backend_hashes_match_frozen_snapshot`, `soneflow-py/tests/test_smoke.py::test_bitwise_contract_n_and_time_axis` |
| The low-frequency correction reaches its last row above the seventh threshold, in free and diffuse field. | `soneflow/tests/low_freq_correction.rs::core_band0_118_000001_db_reaches_the_last_dll_row`, `soneflow/tests/low_freq_correction.rs::core_band0_threshold_118_db_is_strict_and_last_row_applies_above_it`, `soneflow/tests/low_freq_correction.rs::core_diffuse_field_uses_the_same_row_selection`, `soneflow/tests/low_freq_correction.rs::stream_25hz_119db_sine_matches_batch_and_is_not_clamped` |
| Core loudness is never negative and never falls when one band rises; levels far below threshold give zero. | `soneflow/tests/core_characterization.rs::core_loudness_is_never_negative`, `soneflow/tests/core_characterization.rs::raising_one_band_never_lowers_any_core_loudness`, `soneflow/tests/core_characterization.rs::levels_far_below_threshold_give_zero_core_loudness` |
| The slope stage: total loudness is never negative, the total-only and pattern variants agree bit for bit, and out-of-domain input returns without panicking. | `soneflow/tests/core_characterization.rs::total_loudness_is_never_negative`, `soneflow/tests/core_characterization.rs::total_only_and_with_pattern_give_bitwise_equal_totals`, `soneflow/tests/core_characterization.rs::finite_input_beyond_the_bounds_returns_without_panicking`, `soneflow/tests/core_characterization.rs::non_finite_input_returns_without_panicking`, `soneflow/tests/zup_eps.rs::complete_writes_repeatability_and_three_total_apis_remain_bitwise` |
| `sone2phon` follows its frozen formula and rejects negative or non-finite input (Python). | `soneflow/src/sone2phon.rs::anchors_and_branch_monotonicity`, `soneflow-py/tests/test_smoke.py::test_sone2phon_matches_frozen_formula`, `soneflow-py/tests/test_smoke.py::test_sone2phon_rejects_negative_sone`, `soneflow-py/tests/test_smoke.py::test_sone2phon_rejects_nonfinite_sone` |

## 5. Internal numeric faults

| promise | tests |
|---|---|
| A value that leaves the valid range inside the pipeline is an error (`InternalNumericFault`, C ABI code 5) with no partial output, and the processor stays usable. | `soneflow/tests/l4_batch_fault.rs::temporal_extreme_fault_does_not_poison_reusable_processor`, `soneflow/tests/l4_batch_fault.rs::stationary_finite_extremes_are_internal_faults`, `soneflow-py/tests/test_quality_v1.py::test_internal_numeric_batch_fault_is_runtime_error`, `soneflow-ffi/tests/quality_v1.rs::batch_numeric_error_uses_code_five_without_output_writes` |
| In the stream, a numeric fault gives placeholder frames with per-frame quality bits and a sticky diagnostic snapshot; it recovers without acknowledgement. | `soneflow/src/zwtv/stream/containment_tests.rs::fault_trace_is_chunk_invariant_and_recovers_without_ack`, `soneflow-py/tests/test_quality_v1.py::test_numeric_fault_quality_and_sticky_snapshot_are_exposed`, `soneflow-py/tests/test_quality_v1.py::test_diagnostics_peek_stale_ack_reset_and_flush`, `soneflow-ffi/tests/quality_v1.rs::ffi_quality_matches_rust_on_signed_synthetic_chunks` |

## 6. C ABI and Python

| promise | tests |
|---|---|
| The C ABI gives the Rust results bit for bit. | `soneflow-ffi/tests/ffi.rs::zwtv_happy_path_matches_rust_api_bitwise`, `soneflow-ffi/tests/ffi.rs::zwst_happy_path_matches_rust_api_bitwise`, `soneflow-ffi/tests/ffi.rs::zwtv_diffuse_field_matches_rust_api` |
| C ABI v1 error codes, constants and struct layouts are frozen; null pointers return an error. | `soneflow-ffi/tests/ffi.rs::error_mapping_matches_spec_table`, `soneflow-ffi/tests/ffi.rs::field_constants_are_frozen`, `soneflow-ffi/tests/ffi.rs::null_pointers_return_err_null`, `soneflow-ffi/tests/ffi.rs::stream_layout_flags_and_null_paths_are_frozen`, `soneflow-ffi/tests/quality_v1.rs::quality_layout_is_additive` |
| A Rust panic never crosses the C ABI; it becomes an error code, also from a Rayon worker or the stream path. | `soneflow-ffi/tests/ffi.rs::injected_panic_returns_err_panic_not_abort`, `soneflow-ffi/tests/ffi.rs::rayon_worker_panic_is_caught_at_ffi_boundary`, `soneflow-ffi/tests/ffi.rs::stream_path_panic_is_caught_at_ffi_boundary` |
| The checked C stream calls validate versions and buffers before changing state. | `soneflow-ffi/tests/quality_v1.rs::checked_buffers_versions_and_lifecycle_are_atomic`, `soneflow-ffi/tests/quality_v1.rs::null_precedes_version_and_zeroes_only_written`, `soneflow-ffi/tests/quality_v1.rs::environment_errors_preserve_payloads_and_creation_is_sticky` |
| Python maps each error to a fixed exception type, and rejects a wrong dtype or shape without touching the stream's diagnostics. | `soneflow-py/tests/test_smoke.py::test_error_mapping`, `soneflow-py/tests/test_smoke.py::test_strict_input_contract`, `soneflow-py/tests/test_quality_v1.py::test_dtype_shape_rejection_does_not_change_diagnostics` |

## 7. Promises without a test

- The Python binding releases the GIL during batch work. This is visible in `soneflow-py/src/lib.rs` (`allow_threads`) but no test checks it.
- The C header `soneflow-ffi/include/soneflow.h` matches the Rust declarations. CI regenerates it with cbindgen and fails on a difference; that is a CI step, not a named test.

## 8. Tests that need local data

Agreement with MoSQITo 1.2.1 (`soneflow/tests/golden_*.rs`, `soneflow-py/tests/test_parity_mosqito.py`) and with the ISO 532-1 Annex B test signals (`soneflow/tests/annexb.rs`, `soneflow/tests/stream_golden.rs`) is checked by tests that read `data/`, which is not in the repository. They run locally; see `README.md`, "Verification scope".
