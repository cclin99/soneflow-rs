# soneflow 0.1.0 獨立驗收：FAIL

驗收者：Astra。日期：2026-11-24（Asia/Taipei）。固定快照 `fe786a0e40cff532968204da70c7187ae2d5bc99`，樹 `e02b3e42b1658c0500c2fdc329b27a1b7a6c134d`。依新版 soneflow acceptance brief；舊驗收結果未沿用。僅驗非 ISO 材料部分。

程序例外：依流程讀取既有驗證 skill 時，工具非預期顯示了一個舊 ISO 衍生 hash 範例；本輪未使用、比對、計算或納入交付，也未執行其禁止命令。不重刊其內容；因此本輪不宣稱對所有 ISO 衍生文字零接觸。沒有開啟官方材料或禁止資料檔，也沒有讀取/執行私有 ISO checker。此例外如實揭露，不改動 A1–A5 門檻。

## A1–A5 判定

| 條件 | 判定 | 理由 |
|---|---|---|
| A1 公開樹清理 | **FAIL** | 仍有官方程式評論與授權輸入的預期數值輸出殘留；下列只列路徑、行號、類型。完整 ISO 材料檢查交使用者。 |
| A2 數值不變與合法測試 | **PASS** | 66 個產品 Rust src 檔比對符合改名/排版/註解及兩個 test-only 訊息例外；合法必測全通過，Windows hash 實際斷言。 |
| A3 README 實測重現 | **FAIL（文件精度呈現）** | synthetic 非 step 的 tail max N 與 README 在通常四捨五入的印出位數不一致；不是演算法回歸。速度另列，超過 ±20% 本來就不算 FAIL。 |
| A4 無資料設定流程 | **FAIL（字面零下載條件）** | 自備普通第三方套件後會印授權檔缺失說明、且不下載 ISO 附件；但腳本仍執行網路可用的 pip 安裝，未實作「不下載任何東西」。實測套件使用 cache，並未證明本次下載套件位元組。 |
| A5 發布前提 | **PASS** | GitHub main 單一 root、noreply、8 CI jobs 成功、版本一致、cbindgen 標頭一致。 |

A1 已足以阻擋本次 PASS。以上沒有修改完成條件，也沒有把交使用者執行的項目當成失敗。未提交、推送、打 tag 或修改產品、測試、原受驗工作樹的追蹤檔。

## A1 阻擋位置與檢查範圍

- `soneflow/src/core/main_loudness/low_freq.rs:68`：對官方 Annex A 程式控制流程的評論。
- `soneflow/README.md:45`：與授權 Annex B 輸入連結的預期數值輸出。

不重刊上述命中文字或數值，不追讀官方來源，也不計算/比對其結果。`a1-scan.json` 僅記路徑與行號。名稱殘留只出現在 brief 允許的兩個 redaction 工具；可掃描的公開文字未命中私人 email 或未豁免工作站路徑。`tools/golden.sha256` 未讀取、未掃描，其內容與完整 ISO 清查交使用者。唯一追蹤 binary 的旁附 provenance 宣稱自有 synthetic fixture；本輪不以此取代使用者的材料來源審查。

## A2 證據

| Verification | Result |
|---|---|
| Rust library | 141 passed |
| CI data-free integration targets | 61 passed; 1 existing ignored diagnostic |
| annexb_validator synthetic portion | 18 passed; 6 csv_ tests deliberately filtered |
| Doctests | 2 passed |
| FFI including test-panic and quality_v1 | 23 passed |
| Python smoke + stream, newly built wheel | 42 passed |
| Repository tools | 43 passed |
| fmt / clippy / rustdoc / documentation pairs | PASS |
| Approved private synthetic statistics test | 1 passed; fixed dependency versions |

`HASH_GATE_DUMP_ONLY=0`、`REQUIRE_AVX2=1`。ignored 項為既有 `py_contract_dump` 診斷，非 hash gate。66 檔去註解/空白、對應改名及排序 import 後，64 檔 token 相同；餘 2 檔只變更 `#[cfg(test)]` 下斷言訊息，測試位置見 `token-comparison.json`。未輸出舊歷史或敏感 diff。

Python wheel 由本快照重建，裝入隔離 `.venv` 後測試；版本/實際模組位置見 `logs/python-environment.log`。私有 synthetic harness 僅複製 20 個明列 synthetic 檔案，不遞迴拷貝 data；正式執行依賴版本與受驗 Cargo.lock 相同。第一次尚未固定依賴的準備 build 因版本差異主動中止，記為 INTERRUPTED，未當成數值 FAIL 或正式結果。

## A3 精度與速度

| Synthetic quantity | README | Reproduced | Printed-digit check |
|---|---:|---:|---|
| ZWST max absolute N | 4.7e-4 | 4.738e-4 | PASS |
| ZWST max relative N | 6.2e-5 | 6.156e-5 | PASS |
| ZWST max specific N | 1.5e-4 | 1.501e-4 | PASS |
| ZWTV tail, non-step max N | 1.8e-4 | 1.748e-4 | FAIL: conventional two-significant-digit rounding is 1.7e-4 |
| ZWTV tail, step max N | 3.9e-3 | 3.941e-3 | PASS |
| ZWTV tail, max specific N | 2.2e-2 | 2.209e-2 | PASS |
| ZWTV zero, step max N | 5.6e-2 | 5.575e-2 | PASS |

`1.8e-4` 若解釋為向上保守界線可包住實測，但 brief 要求印出位數一致，未授權用保守界線替代通常四捨五入。這項是文件表述不一致，A2 計算等價與合法測試仍成立。Python mean 8.070 ms（README 8.10 ms）、MoSQITo mean 6.584678 s（README 6.587 s），均在 ±20%；SD 分別 1.151 ms 與 0.036543 s，高於 README 0.39 ms 與 0.025 s，這項變異如實保留，依 brief 不作 FAIL。其餘 Python 預設零狀態與表格對照結果如下，完整 raw log 位於 logs。

```text
soneflow Rust binding: mean=0.008070 s, sample_sd=0.001151 s, min=0.007364 s, max=0.014038 s, real_time=371.7385x
MoSQITo: mean=6.584678 s, sample_sd=0.036543 s, min=6.524867 s, max=6.698754 s, real_time=0.4556x
relative speed from arithmetic means: 815.93x
max abs N(t) difference: 5.806071e-03 sone
max abs N_specific(t) difference: 3.245252e-02 sone/Bark
```

| Rust benchmark | README ms | Median ms | Difference |
|---|---:|---:|---:|
| `filter_bank_10s_t12/avx2/480000` | 5.57 | 5.582 | +0.22% |
| `filter_bank_10s_t12/scalar/480000` | 14.4 | 14.673 | +1.89% |
| `stream_push_10s_480chunk/avx2` | 83.1 | 82.585 | -0.62% |
| `stream_push_10s_480chunk/scalar` | 184.2 | 180.490 | -2.01% |
| `zwtv_10s_t1/avx2/480000` | 100.1 | 97.868 | -2.23% |
| `zwtv_10s_t1/scalar/480000` | 192 | 187.769 | -2.20% |
| `zwtv_10s_t12/avx2/480000` | 30.1 | 30.052 | -0.16% |
| `zwtv_10s_t12/scalar/480000` | 41.9 | 44.335 | +5.81% |

Windows 10 build 19045、AMD family 23 model 113、12 logical CPUs、rustc 1.99.1、Python 3.11.9、NumPy 2.4.6、MoSQITo 1.2.1。全部 correctness 完成後串行量測；協調者未同時跑 CPU 密集工作。Rust 使用 Criterion 的 `median.point_estimate`，不是 console 時間估計；每組 10 samples，部分組有 outlier/延長取樣時間提示，CI 區間見 `benchmark-medians.json`。Python 30 次與 tail 5 次各先暖機一次，使用 arithmetic mean，不與 Rust median 混稱。

MoSQITo 表格檢查：`ZUP`、`RNS`、`USL` 全部相等（PASS，僅輸出形狀／數量／差異筆數）。tail 5 次比較亦 exit 0，N 差異 `1.155051e-4`、specific 差異 `1.855872e-3`；這是另一個 3 秒 synthetic 訊號，不能與上表 1 秒 fixtures 混為同一結果。

## A4 與 A5 證據

第一次新 clone 無 data、也無文件明訂的 MoSQITo tarball 前置，setup exit 1，尚未到授權檔說明；這只是前置缺失，不獨立當產品失敗。第二次新 staging 備齊普通 tarball，執行未改 setup/requirements，exit 0，印出空授權資料目錄說明。`tools/setup_env.sh:36–37,58` 的 pip 安裝仍使用預設 index／build isolation；一般依賴可下載，與字面零下載條件不一致。這不表示有 ISO 附件下載。禁止 targets 的 missing 行為亦交使用者，不因沒有 data 而擅跑。

GitHub run [45423756240](https://github.com/cclin99/soneflow-rs/actions/runs/45423756240) attempt 1：8 jobs success，head 與本快照一致。main 只有此一 commit、parents 為空、author/committer 為 noreply。三個 crate、Cargo.lock、CITATION.cff 皆 0.1.0。live JSON 由協調者唯讀收集，本報告據此判讀，不沿用舊輪。

cbindgen 0.29.4 直接使用 Windows checkout 設定時，輸出與 Git blob 僅換行不同；暫存設定僅正規化 LF 後再產生，輸出與 Git blob **原始 bytes 完全相同**。原 tracked 設定/標頭未改。詳 `header-check.json`。

## 待使用者本人執行（不算 FAIL）

以下命令僅由**使用者本人**在對應固定快照及私有工具環境執行；AI 未執行。無資料時的 missing 流程與放入授權檔後的流程皆交使用者。

```text
python private/tools/iso_material_check.py --check
.venv/Scripts/python.exe tools/golden_manifest.py --verify
cargo test -p soneflow --test annexb --test golden_core --test golden_dsp --test golden_zwst --test golden_zwtv -- --nocapture
cargo test -p soneflow --test annexb_validator csv_ -- --nocapture
.venv/Scripts/python.exe -m pytest soneflow-py/tests/test_parity_mosqito.py -v
cargo test --manifest-path private/tests-crate/Cargo.toml --release
```

另請本人檢視 `tools/golden.sha256`，以及私有 checker 未涵蓋的改寫程式碼、改欄名數列、binary fixture、壓縮檔。本交付不包含官方來源／雜湊清單內容或 Annex B 衍生結果。這些待執行項目不算失敗；請使用者本人判讀，不將 ISO 輸入或衍生輸出傳回 AI 代理進行比對。

## 交付與停車場

命令：`commands.md`、`commands.jsonl`；明細：`findings.json`、`token-comparison.json`、`synthetic-inputs.json`、`synthetic-dependencies.json`、GitHub JSON、logs。完整性清單只涵蓋去識別化後交付檔；synthetic input、原來源與 binary hash 保留原始輸入意義，並非交付副本相同位元組的宣稱。

交付前執行 `python tools/redact_paths.py <delivery>` 與 `--check`，並補查工具未匹配的原 repo 路徑形式；檢查 skipped/warnings、UTF-8、檔案類型與交付 diff。詳 `delivery-check.json`。ISO checker **未執行，交使用者**。

停車場（僅記錄，由使用者決定）：既有 verification skill 仍含禁止範例，是否修訂；redaction 工具未涵蓋帶 -1 的原 repo 路徑及部分小寫 Users 形式，是否補規則。本輪只人工處理本次交付，未修改 skill／工具、產品或擴張驗收矩陣。
