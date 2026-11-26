# 行為契約
> 對照／Pair: [English](BEHAVIOUR-CONTRACTS.md) · 來源：en · 同步 2026-11-26

下表每一列是本函式庫的一項承諾，以及承諾被打破時會失敗的測試。測試寫成 `路徑::名稱`（路徑從 repo 根目錄算起）。`python tools/contract_tests.py` 檢查表上每個測試都存在、中英兩版列的測試相同；CI 會跑這支腳本。

除了 §8 另有說明的以外，這裡列的測試都不需要外部資料，也都在 CI 裡跑。還沒有測試守住的承諾列在 §7，不放進表格。

## 1. 輸入

| 承諾 | 測試 |
|---|---|
| 只接受 48 kHz；其他取樣率是錯誤。 | `soneflow/tests/api_errors.rs::loudness_zwst_rejects_unsupported_sample_rate`、`soneflow/tests/api_errors.rs::loudness_zwtv_rejects_unsupported_sample_rate` |
| 批次輸入短於最短長度是錯誤；時變響度最短的合法輸入是 4,800 個樣本（50 個輸出點）。 | `soneflow/tests/api_errors.rs::loudness_zwst_rejects_short_signal`、`soneflow/tests/api_errors.rs::loudness_zwtv_rejects_short_signal`、`soneflow/tests/output_grid.rs::shortest_legal_input_is_4800_samples_and_50_points` |
| 輸入錯誤依固定順序回報：先取樣率，再長度，再非有限值。 | `soneflow/tests/api_errors.rs::sample_rate_and_length_errors_take_precedence_over_finiteness`、`soneflow/tests/l4_batch_fault.rs::stationary_input_error_precedence_stays_rate_length_finiteness` |
| 批次輸入有非有限值是錯誤（C ABI 代碼 4），並指出第一個出問題的樣本；次正規數、零和很大的有限值都接受。 | `soneflow/tests/api_errors.rs::batch_rejects_nan_and_infinities_at_head_middle_and_tail`、`soneflow/tests/api_errors.rs::batch_reports_the_first_offending_sample`、`soneflow/tests/api_errors.rs::nonfinite_error_code_is_frozen_at_4`、`soneflow/tests/api_errors.rs::subnormal_zero_and_large_finite_samples_are_not_rejected_as_nonfinite`、`soneflow-py/tests/test_smoke.py::test_batch_rejects_nonfinite_input` |
| 300 Hz 以下各三分之一倍頻帶的位準超過 120 dB 時，批次拒絕；第十二個頻帶起不設此上限。 | `soneflow/tests/core_characterization.rs::a_low_band_above_120_db_is_rejected_and_120_db_is_accepted`、`soneflow/tests/core_characterization.rs::bands_from_the_twelfth_up_are_not_limited_to_120_db` |

## 2. 輸出網格與因果性

| 承諾 | 測試 |
|---|---|
| `L` 個樣本的時變結果有 `ceil(ceil(L/24)/4)` 個點，第 `j` 點在 `96j/48000` 秒。 | `soneflow/tests/output_grid.rs::frame_count_is_ceil_of_ceil_for_every_field`、`soneflow/tests/output_grid.rs::time_axis_is_an_exact_2ms_grid_from_zero`、`soneflow-py/tests/test_smoke.py::test_zwtv_output_grid_contract`、`soneflow-ffi/tests/ffi.rs::out_frames_query_forwards_core_at_representative_boundaries` |
| 每個輸出點只取決於它的時間點以前的樣本：同一段前綴，後面接什麼都得到相同的點。 | `soneflow/tests/output_grid.rs::every_point_is_prefix_determined_under_zero_padding`、`soneflow/tests/output_grid.rs::every_point_is_prefix_determined_under_real_continuation`、`soneflow/tests/nl_zero_init.rs::batch_prefix_is_invariant_to_the_future_tail` |
| 批次的非線性時間衰減從零狀態開始；`NlInit::MosqitoTail` 重現 MoSQITo 的初值，只供比對用。 | `soneflow/tests/nl_zero_init.rs::silent_prefix_yields_exactly_zero_loudness_before_the_tone_starts`、`soneflow/tests/nl_zero_init.rs::mosqito_tail_init_reproduces_the_audited_pre_fix_numbers`、`soneflow/tests/nl_zero_init.rs::both_initialisations_agree_when_the_signal_ends_in_silence`、`soneflow-py/tests/test_smoke.py::test_bitwise_contract_mosqito_tail_init`、`soneflow-py/tests/test_smoke.py::test_nl_init_rejects_unknown_value` |
| 重複使用的 `ZwtvProcessor` 和新建的結果相同。 | `soneflow/tests/nl_zero_init.rs::processor_reuse_does_not_leak_state_between_calls` |

## 3. 串流

| 承諾 | 測試 |
|---|---|
| 串流從第一幀起逐位元等於批次結果；不論怎麼切塊，幀數都和批次相同。 | `soneflow/tests/nl_zero_init.rs::batch_matches_the_causal_stream_bitwise_from_frame_zero`、`soneflow/tests/stream_contract.rs::batch_zerostate_reference_and_stream_are_bitwise_identical_from_frame_zero`、`soneflow/tests/output_grid.rs::stream_frame_count_equals_batch_for_any_chunking`、`soneflow-py/tests/test_stream.py::test_stream_matches_batch_after_warmup`、`soneflow-ffi/tests/ffi.rs::stream_matches_rust_stream_bitwise` |
| 輸出和輸入怎麼切塊無關。 | `soneflow/tests/stream_contract.rs::chunk_size_invariance_is_bitwise`、`soneflow-py/tests/test_stream.py::test_chunk_invariance_bitwise` |
| 延遲是 24 個輸入樣本；前 580 個輸出幀標為暖機。 | `soneflow/tests/stream_contract.rs::stream_constants_and_single_push_reference`、`soneflow/tests/stream_contract.rs::warmup_constant_is_frozen`、`soneflow-py/tests/test_stream.py::test_constants_are_frozen` |
| 建構之後，`push`、`flush`、故障復原、品質輸出、`ack_diagnostics` 和 `reset` 都不配置記憶體，也不啟動 Rayon。 | `soneflow/tests/stream_alloc.rs::push_and_flush_do_not_allocate`、`soneflow/tests/stream_fault_rt.rs::fault_recovery_quality_ack_and_reset_allocate_nothing_and_do_not_enter_rayon`、`soneflow/tests/stream_no_rayon.rs::stream_does_not_initialize_rayon_pool` |
| 串流的非有限樣本當成零並標旗（`NONFINITE_INPUT`），不是錯誤；最後一個輸出幀之後的旗標以殘留旗標回報。 | `soneflow/tests/api_errors.rs::stream_still_flags_instead_of_failing`、`soneflow/tests/stream_contract.rs::nonfinite_input_flags_and_recovers`、`soneflow/tests/stream_contract.rs::tail_nonfinite_input_is_reported_as_residual_flag`、`soneflow-py/tests/test_stream.py::test_nan_is_flagged_not_raised`、`soneflow-py/tests/test_stream.py::test_tail_nan_surfaces_in_residual_flags`、`soneflow-ffi/tests/ffi.rs::stream_tail_nonfinite_is_exposed_as_residual_flag` |
| 串流中低頻帶超過 120 dB 的位準會被夾住並標旗（`CLAMPED_120DB`），串流繼續。 | `soneflow/tests/stream_contract.rs::over_120db_is_clamped_and_stream_continues` |
| `reset` 讓串流回到新建時的狀態，flush 過一段部分非有限的輸入後也一樣；`flush` 之後要 `reset` 才能再 `push`。 | `soneflow/tests/stream_contract.rs::reset_matches_a_new_stream`、`soneflow/tests/stream_contract.rs::flush_reset_after_partial_nonfinite_chunk_matches_a_new_stream`、`soneflow-py/tests/test_stream.py::test_push_after_flush_raises_and_reset_recovers` |
| `push` 和 `flush` 結束時，呼叫端的 MXCSR 維持原樣。 | `soneflow/tests/stream_contract.rs::push_and_flush_restore_mxcsr` |
| `max_frames_for_chunk` 是一次 `push` 最多寫出的幀數。 | `soneflow-py/tests/test_stream.py::test_max_frames_for_chunk_bounds_push_output`、`soneflow-ffi/tests/ffi.rs::stream_max_frames_forwards_rust_contract`、`soneflow-ffi/tests/ffi.rs::stream_rejects_small_output_without_consuming_input` |

## 4. 數值與後端

| 承諾 | 測試 |
|---|---|
| AVX2 和純量核心結果相同；`simd::set_force_scalar` 優先於執行期偵測。 | `soneflow/tests/simd_parity.rs::filter_bank_avx2_matches_scalar`、`soneflow/tests/simd_parity.rs::nl_loudness_avx2_matches_scalar`、`soneflow/tests/simd_dispatch.rs::force_scalar_overrides_runtime_detection`、`soneflow/tests/stream_scalar.rs::forced_scalar_stream_matches_scalar_zerostate_reference` |
| 結果逐位元可重現：重複執行一致，參考輸出的凍結雜湊依平台成立。 | `soneflow/tests/determinism.rs::zwtv_output_is_bitwise_deterministic_over_20_runs`、`soneflow/tests/hash_gate.rs::zwtv_backend_hashes_match_frozen_snapshot`、`soneflow-py/tests/test_smoke.py::test_bitwise_contract_n_and_time_axis` |
| 低頻修正在第七個門檻以上用到最後一列，自由場和擴散場都一樣。 | `soneflow/tests/low_freq_correction.rs::core_band0_118_000001_db_reaches_the_last_dll_row`、`soneflow/tests/low_freq_correction.rs::core_band0_threshold_118_db_is_strict_and_last_row_applies_above_it`、`soneflow/tests/low_freq_correction.rs::core_diffuse_field_uses_the_same_row_selection`、`soneflow/tests/low_freq_correction.rs::stream_25hz_119db_sine_matches_batch_and_is_not_clamped` |
| 核心響度不會是負數，提高一個頻帶不會讓任何核心響度下降；遠低於閾值的位準得到零。 | `soneflow/tests/core_characterization.rs::core_loudness_is_never_negative`、`soneflow/tests/core_characterization.rs::raising_one_band_never_lowers_any_core_loudness`、`soneflow/tests/core_characterization.rs::levels_far_below_threshold_give_zero_core_loudness` |
| 斜率階段：總響度不會是負數，只算總量與連分布一起算的兩種版本逐位元一致，定義域外的輸入也會正常返回、不 panic。 | `soneflow/tests/core_characterization.rs::total_loudness_is_never_negative`、`soneflow/tests/core_characterization.rs::total_only_and_with_pattern_give_bitwise_equal_totals`、`soneflow/tests/core_characterization.rs::finite_input_beyond_the_bounds_returns_without_panicking`、`soneflow/tests/core_characterization.rs::non_finite_input_returns_without_panicking`、`soneflow/tests/zup_eps.rs::complete_writes_repeatability_and_three_total_apis_remain_bitwise` |
| `sone2phon` 依凍結的公式計算，負數或非有限輸入會被拒絕（Python）。 | `soneflow/src/sone2phon.rs::anchors_and_branch_monotonicity`、`soneflow-py/tests/test_smoke.py::test_sone2phon_matches_frozen_formula`、`soneflow-py/tests/test_smoke.py::test_sone2phon_rejects_negative_sone`、`soneflow-py/tests/test_smoke.py::test_sone2phon_rejects_nonfinite_sone` |

## 5. 內部數值故障

| 承諾 | 測試 |
|---|---|
| 管線內部的值超出有效範圍時是錯誤（`InternalNumericFault`，C ABI 代碼 5），不輸出部分結果，processor 仍可繼續使用。 | `soneflow/tests/l4_batch_fault.rs::temporal_extreme_fault_does_not_poison_reusable_processor`、`soneflow/tests/l4_batch_fault.rs::stationary_finite_extremes_are_internal_faults`、`soneflow-py/tests/test_quality_v1.py::test_internal_numeric_batch_fault_is_runtime_error`、`soneflow-ffi/tests/quality_v1.rs::batch_numeric_error_uses_code_five_without_output_writes` |
| 串流發生數值故障時輸出佔位幀，附每幀品質位元和會保留的診斷快照；不需要確認就會復原。 | `soneflow/src/zwtv/stream/containment_tests.rs::fault_trace_is_chunk_invariant_and_recovers_without_ack`、`soneflow-py/tests/test_quality_v1.py::test_numeric_fault_quality_and_sticky_snapshot_are_exposed`、`soneflow-py/tests/test_quality_v1.py::test_diagnostics_peek_stale_ack_reset_and_flush`、`soneflow-ffi/tests/quality_v1.rs::ffi_quality_matches_rust_on_signed_synthetic_chunks` |

## 6. C ABI 與 Python

| 承諾 | 測試 |
|---|---|
| C ABI 的結果逐位元等於 Rust。 | `soneflow-ffi/tests/ffi.rs::zwtv_happy_path_matches_rust_api_bitwise`、`soneflow-ffi/tests/ffi.rs::zwst_happy_path_matches_rust_api_bitwise`、`soneflow-ffi/tests/ffi.rs::zwtv_diffuse_field_matches_rust_api` |
| C ABI v1 的錯誤碼、常數和結構配置都已凍結；空指標回傳錯誤。 | `soneflow-ffi/tests/ffi.rs::error_mapping_matches_spec_table`、`soneflow-ffi/tests/ffi.rs::field_constants_are_frozen`、`soneflow-ffi/tests/ffi.rs::null_pointers_return_err_null`、`soneflow-ffi/tests/ffi.rs::stream_layout_flags_and_null_paths_are_frozen`、`soneflow-ffi/tests/quality_v1.rs::quality_layout_is_additive` |
| Rust 的 panic 不會穿過 C ABI，會變成錯誤碼，從 Rayon 工作執行緒或串流路徑發生的也一樣。 | `soneflow-ffi/tests/ffi.rs::injected_panic_returns_err_panic_not_abort`、`soneflow-ffi/tests/ffi.rs::rayon_worker_panic_is_caught_at_ffi_boundary`、`soneflow-ffi/tests/ffi.rs::stream_path_panic_is_caught_at_ffi_boundary` |
| 有檢查的 C 串流呼叫在改變狀態前先檢查版本和緩衝區。 | `soneflow-ffi/tests/quality_v1.rs::checked_buffers_versions_and_lifecycle_are_atomic`、`soneflow-ffi/tests/quality_v1.rs::null_precedes_version_and_zeroes_only_written`、`soneflow-ffi/tests/quality_v1.rs::environment_errors_preserve_payloads_and_creation_is_sticky` |
| Python 把每種錯誤對應到固定的例外型別；dtype 或 shape 不對時拒絕，不動串流的診斷。 | `soneflow-py/tests/test_smoke.py::test_error_mapping`、`soneflow-py/tests/test_smoke.py::test_strict_input_contract`、`soneflow-py/tests/test_quality_v1.py::test_dtype_shape_rejection_does_not_change_diagnostics` |

## 7. 還沒有測試的承諾

- Python binding 在批次計算期間釋放 GIL。這可以在 `soneflow-py/src/lib.rs`（`allow_threads`）看到，但沒有測試檢查。
- C 標頭 `soneflow-ffi/include/soneflow.h` 與 Rust 宣告一致。CI 用 cbindgen 重新產生並在有差異時失敗；那是 CI 步驟，不是具名的測試。

## 8. 需要本機資料的測試

與 MoSQITo 1.2.1 的一致性（`soneflow/tests/golden_*.rs`、`soneflow-py/tests/test_parity_mosqito.py`），以及與 ISO 532-1 Annex B 測試訊號的一致性（`soneflow/tests/annexb.rs`、`soneflow/tests/stream_golden.rs`），由讀取 `data/` 的測試檢查；`data/` 不在 repo 裡。這些測試在本機跑，見 `README.zh-TW.md` 的「驗證範圍」。
