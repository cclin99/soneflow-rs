# soneflow 0.1.0 獨立複驗：PASS

驗收者：gpt-6-astra。日期：2026-11-25（Asia/Taipei）。固定快照 `f4e239bb7cf3f79426c8812d6f096455a3c41bfb`，父 commit `fe786a0e40cff532968204da70c7187ae2d5bc99`。只複驗授權交辦的 R1–R5。PASS 限於本輪允許的非 ISO 材料檢查；待使用者執行的材料檢查依 brief 不算失敗。

| 關卡 | 判定 | 證據 |
|---|---|---|
| R1 公開樹 | 允許檢查範圍內 PASS | `r1-scan.json`、`new-side-review.txt` 及上一輪 A1 掃描/判定。159 個追蹤路徑均列入清冊，掃描 157 個文字檔；checksum manifest 與 owned binary 本體排除。舊兩處問題已更正；無私人 email；路徑命中只屬去識別化工具的通用規則。 |
| R2 數值不變 | PASS | `token-comparison.json`：兩個變動 Rust 檔與 C header 去註解 token 完全相同，未做名稱或常數正規化；其他 Rust 檔未改。CI 非註解內容相同。cbindgen 0.29.4 重生 header 與 Git blob、工作樹檔案逐位元相同。精確快照 CI 及本機允許測試見下文。 |
| R3 印出精度 | PASS | 兩份 `r3-README*.json`：README 唯一變動為 1.8e-4 → 1.7e-4。上一輪合成訊號實測 1.748e-4 四捨五入兩位有效數字即 1.7e-4；其他精度與時間數字未改。本輪未重測數值或時間。 |
| R4 不下載 ISO 附件 | PASS | `r4-unchanged.json` 證明 setup/requirements 與父 commit 相同。沿用上一輪第二次無 data staging、具備文件列出的普通 MoSQITo tarball 前置條件：exit 0 並印出自備授權檔指引。見 `logs/prior-setup-no-data-with-prerequisite.log`、上一輪英文報告第 82 行。修正後條件允許安裝一般套件；腳本未包含 ISO 附件下載操作。本輪未重跑 setup。 |
| R5 發布前提 | PASS | [CI 45519392033](https://github.com/cclin99/soneflow-rs/actions/runs/45519392033) 為受驗 SHA，全 8 jobs success；`logs/ci.txt`。即時 GitHub API 確認 main 僅父 commit → 受驗 commit，父 commit 無 parent，所有作者/committer 皆 noreply；`logs/remote-history.json`。三個 manifest、lockfile 三個套件與 CITATION 都為 0.1.0；`versions.json`。 |

## 證據邊界與程序例外

未開啟授權官方程式、工作簿、標準本文或 data 檔；未執行禁用測試或私有檢查器；未載入驗證 skill。`tools/golden.sha256` 刻意排除不讀。binary 的相鄰 provenance 宣告為自有合成 oracle，且本輪未變；這不取代使用者的材料檢查。關鍵字掃描不能證明改寫官方碼、換欄名數列或二進位內容均無材料，這些仍留給使用者。

初次通用 `git diff` 檢視時，工具意外顯示已刪除的歷史 README 參考輸出及官方碼評論。已停止閱讀該處，未用作數值驗收依據，也未複製進交付；交付只保留新版本新增行。本輪因此不聲稱完全零接觸歷史 ISO 相關文字。此例外照實揭露，不改動使用者 R1–R5 條件。

10 個變動檔案段落均逐行檢查：低頻三處註解改為本 crate/MoSQITo 行為；crate README 使用一般 WAV 並移除參考輸出；兩語 README 精度更正；FFI field 說明翻成英文；SOP shell 註解翻成英文；checklist 文句與章節定位更正；CI 僅註解數字更正。未交付完整歷史 diff。

## 測試與警告

精確快照 CI 已驗證 Windows/Linux 無資料 Rust integration、doctest、FFI test-panic、C smoke、Python smoke/stream、工具、文件、fmt、Clippy 與架構 coverage。這是觀察到的 CI 證據，不冒稱所有平台都在本機重跑。Windows CI 使用 dump-only hash；本機額外以 `HASH_GATE_DUMP_ONLY=0`、`REQUIRE_AVX2=1` 執行 `hash_gate`，PASS，見 `hash-enforced-command.json` 及日誌。本機套件結果見 `test-summary.json`；既有 1 個 ignored diagnostic 與 6 個刻意過濾 csv_ 測試不算通過。

cbindgen 警告略過非 public 匿名項目，公開 header 仍逐位元一致。SSH `git ls-remote` 因環境 CreateFileMapping/access 錯誤 exit 128，改由已登入的唯讀 GitHub API 補足即時遠端證據。未觸發 CI、上傳產物或修改遠端。

## 待使用者執行

由使用者本人執行私有材料檢查器，並檢查工具未涵蓋的改寫碼、換欄名數列與 binary。本代理未讀取或執行該工具；本報告不宣稱 ISO 符合性或授權材料驗收。此待辦依 brief 不算 FAIL。

## 交付與停車場

repo 只新增本證據目錄；產品、測試、原文件未改，未 commit/push/tag。上一輪證據未覆寫。`prior-evidence.json` 記錄沿用證據來源。

已記錄去識別化寫入與 check 兩遍於 `redaction.json`，檢查 skipped/非 UTF-8 與警告；補充大小寫不敏感掃描涵蓋既知私有 repo 名稱及小寫 users 路徑缺口。`integrity.sha256` 針對最終交付位元組，以兩空白分隔 hash 與相對路徑，不含清單自己。來源 hash 保留原始意義。命令見 `commands.jsonl`、腳本及 `commands.md`。

停車場：既知去識別化路徑模式缺口未修。未新增或展開實作工作。
