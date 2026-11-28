# Release 檢查清單

> 對照／Pair: [English](RELEASE-CHECKLIST.md) · 來源：en · 同步 2026-11-28

發布 release 或公開 README 之前，請立即執行這份檢查清單。

- [ ] 確認 `README.md` 與 `README.zh-TW.md` 都沒有審查草稿用語；任何公開的 README 都不得寫「not yet announced as a public release」或「尚未對外宣布發布」。
- [ ] 重跑 `tools/compare_mosqito.py`，並一併更新日期、環境、計時與直接 parity 數值。
- [ ] 從本機重新產生的 golden 環境執行「Measured results (0.2.1)」→「Reproduce」／「實測結果（0.2.1）」→「重現」下的指令。
- [ ] 確認 `docs/GOLDEN-REGEN-SOP.md` 仍涵蓋全新 clone 所需的每一項先決條件。
- [ ] 逐一核對每條能力與邊界敘述是否與已打 tag 的原始碼及公開產出物相符。
- [ ] 對照 release tag 與目前的 repository 設定，檢視 `THIRD_PARTY_NOTICES.md`、`CITATION.cff`、`SECURITY.md` 與社群範本。
- [ ] 在正常的防毒保護啟用下，建置並掃描 release 的 DLL／靜態函式庫、wheel 與任何外掛套件；任何偵測都要記錄產品、版本、產出物 hash 與確切的建置指令。不得將停用防毒下的建置當作 release 證據。
- [ ] 對 release 的每一個 `soneflow` rlib 執行 `llvm-objdump -d -r`（`target/release/deps/` 裡，建置過的每組 feature 各有一個 `libsoneflow-*.rlib`），確認每一個的 `init_mp1_table_rn` 裡都有 20 筆 `pow` 重定位（Windows COFF 要加 `-r` 才看得到；Linux ELF 的重定位型別不同）。`std::hint::black_box` 只是盡力而為，工具鏈升級後要用這一步重新確認編譯器沒有把這些 `pow` 在編譯期折掉。
- [ ] 為每個 release 產出物產生並發布 SHA-256 校驗值；smoke test 之前，先以這些校驗值驗證下載的產出物。
- [ ] 確認僅限本機的檔案、壓縮檔、產生的 `data/`、憑證與機器特定路徑未被 stage。
- [ ] 執行維護者本機的檢查，確認樹中沒有 ISO 532-1 電子附件的材料（Annex A 程式、Annex B 訊號或參考值），並人工檢查工具看不到的格式（改寫過的程式碼、換了欄名的數列、二進位檔、壓縮檔）。
- [ ] 審核與驗證交付：執行 `python tools/redact_paths.py --check docs`（必須輸出 `clean`）；報告所引用的每個證據壓縮檔，都列在該次交付於 `docs/verification/` 下的資料夾中，含大小、SHA-256 與可用的下載連結；壓縮檔本身不進庫。
