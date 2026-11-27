# 待使用者執行 / User-only actions

以下指令僅供使用者本人執行，本輪代理未執行。`<SOURCE_REPO>` 指使用者持有合法材料的原始工作目錄，不是本輪無資料驗收快照。`<VERSION_ROOT>` 指使用者自行準備、具有相同合法本機資料的各版工作目錄；分別在基準 `82d1d61` 與受驗 `ddbe9e2` 執行並保留結果以比較。不要在有未保存變更的目錄直接切換版本。

These commands are for the user alone and were not executed by the agents. Run the relevant checks in user-prepared licensed environments for both frozen versions and compare results. The audit snapshots intentionally contain no local ISO data.

## A5：本機 ISO 測試與材料檢查

```powershell
# 確認每份工作目錄的版本；兩版各執行一次 / repeat per version
git -C '<VERSION_ROOT>' rev-parse HEAD
cargo test --manifest-path '<VERSION_ROOT>/soneflow/Cargo.toml' --features internals --test annexb --test annexb_validator --test golden_core --test golden_dsp --test golden_zwst --test golden_zwtv --test stream_golden
# 如本機驗收規程要求 ignored tests，仍只由使用者本人執行
cargo test --manifest-path '<VERSION_ROOT>/soneflow/Cargo.toml' --features internals --test annexb --test annexb_validator --test golden_core --test golden_dsp --test golden_zwst --test golden_zwtv --test stream_golden -- --include-ignored
```

材料檢查須從使用者原始 repo 執行；它不在本輪公開 clone 中：

```powershell
Set-Location '<SOURCE_REPO>'
python private/tools/iso_material_check.py --check
```

如使用者的既有驗收包含 Python MoSQITo parity，請在安裝各受驗版本 wheel 的對應本機環境執行：

```powershell
python -m pytest '<VERSION_ROOT>/soneflow-py/tests/test_parity_mosqito.py' -v
```

私有 `tests-crate` 中其他 ISO 相關測試，仍依使用者既有本機程序執行。本輪不讀取或推測這些私有測試的內容。`mosqito_stats_synthetic` 雖是允許的例外，並非本輪交辦必要門檻，未因此擴大工作範圍。

A5 是待使用者執行，不算失敗，也不是 ISO 符合性已通過的證據。是否將交付資料進庫、合併、發布或打 tag，仍由使用者決定。

A5 is pending user execution and does not count as a failure. It is not an ISO conformity claim. Importing the report, merging, publishing, and tagging remain user decisions.
