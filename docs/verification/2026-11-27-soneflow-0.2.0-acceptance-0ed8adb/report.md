# soneflow 0.2.0 獨立驗收

判定：**PASS（B1–B5、R1–R3）**。交付日 2026-11-27（Asia/Taipei），驗收者 Astra。此判定依 brief 將需 ISO 材料的檢查列為「交使用者執行」，不代表那些檢查已完成或 ISO 符合性已獲證實。

受驗：`0ed8adbb256ad29ba43781f57bd5ac370e549121`；基準：`v0.1.0` → `15a2c076d3b4002b93e5380159a028fd056bdbb6`。固定完成條件只限 B1–B5；沒有修改、擴充或放寬門檻。主任務結果為輸出不變、既有允許測試通過、效能不超過 +3%、函式複雜度與 CRAP 過關、公開 API 按 D1 收窄。

| 項目 | 判定 | 本輪證據 |
|---|---|---|
| B1 | PASS | 預設 features 的 hash_gate、internals simd_parity 通過；固定 commit wheel 的 Python 47 tests 通過，均 `HASH_GATE_DUMP_ONLY=0`、`REQUIRE_AVX2=1`。Rust 凍結 hash 檔只改 import；三個 Python 測試檔與基準無差異。Linux Rust/Python CI hash 強制斷言成功。 |
| B2 | PASS | default / internals lib 各 145；產品整合 32、內部整合 43；兩組 doctest 各 20；CLI 6；FFI（含 test-panic）23；Python 47。四組 clippy、fmt、rustdoc、工具單元測試、C MSVC smoke 通過。17 個無 data/ 整合檔逐一列在 CI。2 個既有 ignored 未啟用。 |
| B3 | PASS | 各 3 輪交錯，四組三輪 median 的 median 均未超過基準 +3%，見下表與 `benchmark-rounds.json`、`criterion/`。 |
| B4 | PASS | `fnmetrics`：614 個產品函式，最大 CC 14、ploc 78，0 超標；`cargo-crap 0.4.3` / arch 閘門最大 CRAP 20.0，全部規則通過。coverage 明確省略 annexb_validator。 |
| B5 | PASS | public_api 266 records 一致；預設 8 個 compile_fail 通過。基準 canonical API 差異全部屬 CHANGELOG 明列的 7 模組；FFI/Python 只用根層。 |
| R1 | PASS | 三個 crate、Cargo.lock、CITATION 皆 0.2.0；cbindgen 0.29.4 重生 header 原始 bytes 相同。 |
| R2 | PASS | 全部 10 組可比速度在 README ±20% 內；3 秒差值在印出位數內一致；tables PASS。需 golden 的命令交使用者。 |
| R3 | PASS | doc_pairs、contract_tests、來源文件 redaction 通過；10 列契約抽查詳 `contract-sample.json`。完整原始 committed docs 的 redaction 由同 SHA CI tools job 確認；本機隔離快照不複製歷史驗證目錄。最終本輪交付與本機 docs 檢查 clean。 |

CI：[https://github.com/cclin99/soneflow-rs/actions/runs/45851619042](https://github.com/cclin99/soneflow-rs/actions/runs/45851619042)，workflow_dispatch，8 jobs 全數 success，head SHA 與受驗快照完全相同。只查 metadata/step 狀態，未讀取禁用測試輸出；禁止的 validator 本機不執行。

## 效能與 README

AMD Ryzen 5 3600，6 cores / 12 logical CPUs；Windows 10 build 19045；rustc 1.99.1 / LLVM 23.1.1；Python 3.11.9、NumPy 2.4.6、MoSQITo 1.2.1。每個快照各自乾淨 target；所有建置/其他測試完成後，前景串行、`RAYON_NUM_THREADS=1`，順序 B1,C1,B2,C2,B3,C3。每輪四個 `new/estimates.json` 均另存，不採 Criterion 的前次 change 判定。Criterion 自動延長部分 10 samples 的收集時間，沒有改量測參數。

| Benchmark | v0.1.0 ms | 0.2.0 ms | Change | Gate |
|---|---:|---:|---:|---|
| zwtv_10s_t1/scalar/480000 | 192.963 | 191.739 | -0.634% | PASS |
| zwtv_10s_t1/avx2/480000 | 100.463 | 100.324 | -0.138% | PASS |
| stream_push_10s_480chunk/scalar | 183.748 | 183.920 | +0.094% | PASS |
| stream_push_10s_480chunk/avx2 | 83.894 | 84.661 | +0.914% | PASS |

README 12-thread batch：scalar 43.298 ms、AVX2 30.078 ms；filter-bank：13.521 / 5.528 ms。Python 30 次（一次暖身後）binding 平均 8.584 ms、MoSQITo 7.134723 s，831.18x。N 最大差 `5.806071e-03`、specific `3.245252e-02`，分別與原文 `5.8e-3` / `3.2e-2` 的印出精度一致。另完成 5 次 mosqito_tail；完整時間、差值見 `logs/compare-tail-5-ready.log`。`logs/compare-tables-ready.log` 為 PASS。所有 README 時間差範圍 -7.20% 至 +0.190%；即使超出 ±20% brief 也只要求紀錄，本輪沒有超出。

## 證據界線與交付

選擇性快照只取已列舉的 tracked 路徑，排除 data/、private/、歷史 docs/verification/；沒有盲目 checkout、複製授權材料或讀取其數列。產品、測試、既有文件沒有改動（`snapshot-immutability.json`）；未 commit、push、tag。採樣、測試、coverage 均為本輪新結果，未沿用歷史驗收數字。沒有實作，也沒有決策探針；所有量測是 brief 指定驗收。

`commands.jsonl` / `commands.md` 記錄命令及退出碼；`logs/` 是每項完整輸出；`support/` 保留 runner；`crap.json`、`lcov.info`、`fnmetrics.json`、API/版本/契約/CI 清單供重現。原始 cargo-crap test-only helper 有 42，但既有 arch 規則排除測試，產品/閘門最大為 20.0；不是放寬限制。Python extension 用同 commit wheel 安裝至乾淨 venv，wheel hash 是 binary 原檔含義；交付文字 hash 由最後去識別化版本產生。

初次 sandbox build 卡住，僅停止本輪已識別程序並於正常權限重跑成功；未將此視作產品失敗。初次 MoSQITo 比較因缺 matplotlib import 失敗，補 repo lock 的 3.11.0 後重跑成功；原失敗 log 保留。初次 docs redaction 掃到尚在生成的本輪 logs，保留失敗紀錄；最終完成去識別化後重跑 clean。去識別化與補充檢查詳 `delivery-check.json`，無未處理 skipped/非 UTF-8/二進位/ZIP。

## 待使用者執行

見 `pending-user-commands.md`，涵蓋 golden/Annex/stream_golden、Python parity 與私有 ISO 材料 checker。這些未由代理執行，未取得其輸出、統計或衍生結果。此 PASS 不取代使用者的材料與發布檢查。

## 停車場

- MXCSR 契約所列測試只比較 `& !0x3f` 的控制位，未涵蓋 exception-status 六位。來源 DenormalGuard 的 Drop 實際恢復完整 saved register；沒有觀察到承諾遭違反。這是測試涵蓋較窄的註記，未新增 gate、未改程式或測試；由使用者決定是否補強。
- MoSQITo 的乾淨 pip 安裝缺間接 matplotlib 依賴；本輪已在隔離 venv 補齊。是否補充安裝文件由使用者另決定。
