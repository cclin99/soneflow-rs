# soneflow 0.2.1 打 tag 前獨立驗收

日期：2026-11-28（Asia/Taipei）。受驗：`ddbe9e2cc4676c2b8fdaf854725ca7107d267540`；基準：`82d1d61`（完整 SHA 見 `baseline-head.txt`）。指定的數值、測試及效能驗收均由獨立 `gpt-6-astra` 代理執行，協調代理整合與檢查交付。

**判定：PASS（A5 待使用者執行，不計失敗）。** A5 依交辦列為待使用者執行，不計失敗。本報告只判定本次固定快照的交辦條件，不宣稱 ISO 符合性或已核准發布。未 commit、push、打 tag，未修正產品；突變及觀測僅在隔離快照進行並已還原。

## 驗收矩陣

| 項目 | 結果 | 本輪獨立證據 |
|---|---|---|
| A1 RN 位元不變 | PASS | 本機預設 feature 的 `hash_gate` debug/release 均以 dump=0 斷言；SIMD parity 通過；受驗 wheel 三套 Python 合約 47 passed；凍結值與 API diff 為空。自選語料 debug/release 各 1068 列完全一致。見 `gates/02-product-debug.log`、`gates/03-hash-release.log`、`gates/04-internals-debug.log`、`gates/25-python-contract.log`、`numerical/comparison.json`、`frozen-contract-diff.txt`。 |
| A2 順序測試與 CI | PASS | 無條件取快取的突變 exit 101，在第二次呼叫（RN）的 band 0 比較（測試第 136 行）失敗；還原後 debug/release 均通過。CI test 與 arch coverage 清單均列入。見 `numerical/report.md`、`numerical/mutation-debug.log`、`numerical/restored-debug.log`、`numerical/restored-release.log`。 |
| A3 既有測試／架構／API／CI | PASS | 所有允許的本機 CI 命令通過；API check 266 lines；函式上限、架構及 CRAP 通過，最高 CRAP 20.0。線上指定 CI SHA 正確、8 jobs success。見 `gates/all-command-index.json`、`gates/method.md`、`gates/31-arch.log`、`ci-run.json`、`ci-jobs.json`。 |
| A4 四臂效能 | PASS | 完整重測一次；第二組四臂中位數皆改善，最大全距/中位數 3.13%。見下表與 `performance/decision.json`。 |
| A5 本機 ISO 測試／材料檢查 | 待使用者執行 | 未讀取原始 `data`，未執行禁跑清單或材料檢查。使用者指令見 `pending-user.md`；不計失敗。 |
| N1 非 RN 行為 | PASS | 三模式兩場型全語料與獨立 runtime reference 一致；debug 對基準全同，release 差異只在頻帶 0–2，最大 5 ULP。batch／quality 拒絕，legacy push／flush 可執行。見 `numerical/comparison.json`、`numerical/five-ulp-cases.json`。 |
| N2 無編譯期折疊 | PASS | 候選初始化 20 個 pow relocation；候選 main 14 pow／1 stmxcsr；基準 main 11／0。實際執行 release rlib 的 `llvm-objdump -d -r`。見 `performance/objdump-summary.json` 與函式摘錄、完整反組譯。 |
| N3 FTZ／DAZ 與預熱 | PASS | 四組 FTZ/DAZ、新 process 空快取起跑，兩 profile 位元一致；release 觀測四組均 before_new=false、after_new=true、after_push=true。見 `numerical/report.md`、`numerical/cache-*.log`。 |
| R1 版號與標頭 | PASS | 三 crate、lock、CITATION 均 0.2.1；cbindgen 0.29.4 使用 Git blob 設定重生之標頭，與 HEAD／baseline blob 位元完全相同。見 `gates/r1-check.json`、`gates/r1-header-final.json`。 |
| R2 CHANGELOG 逐句依據 | PASS | 所有 0.2.1 句子均有程式、獨立數值或效能依據；處理器效能依共享 batch 路徑推論，非另量 processor-only。見 `performance/changelog-audit.md`。 |
| R3 文件與契約 | PASS | doc_pairs、contract_tests、原始 docs redaction 通過；release checklist 反組譯指令已實際重現。最終交付亦執行兩階段去識別化。見 `gates/16-doc-pairs.log`、`gates/17-contracts.log`、`performance/method.md`、`delivery-audit.json`。 |

## 數值驗證範圍

每場型 430 組頻譜，含每個臨界頻帶門檻附近、低頻 120 dB 的前一／本身／後一可表達值及 257 組固定隨機語料；低三頻帶透過二分法定位群組門檻。Free／Diffuse 均測。另用 9600 點自製雙音與分段振幅訊號，對照兩版 batch 的 n、specific loudness、time／bark axis，以及 137 點分塊 stream push／flush 的 frame index、n、n_phon、flags。兩版 debug/release 各 1068 列相同，不將有限語料稱為所有輸入的形式證明。探針未沿用實作者診斷程式或量測數字。

| 非 RN 模式 | release 與基準不同的頻帶及 ULP 範圍 |
|---|---|
| 向下 | 0：1–5；2：1–2 |
| 向上 | 0：1–5；1：2–5；2：1–2 |
| 向零 | 0：1–5；2：1–2 |

debug 全同。受驗版在三種模式下皆與獨立 runtime reference 逐位元相同。10 筆 5 ULP 案例含完整輸入與兩版 bits。這比交辦所述實作者窄語料的 1–3 ULP 大，但 CHANGELOG 沒有承諾該上限，仍符合「最低三個頻帶末位」的敘述，未放寬任何凍結斷言。

## 效能

第二組（依預定規則採用），單位 ms；base = 82d1d61，new = ddbe9e2。

| 測項 | 版本 | 三輪中位數 | 中位數的中位數 | 最小–最大 | 全距 |
|---|---|---|---|---|---|
| zwtv scalar | base | 189.087, 190.422, 188.371 | 189.087 | 188.371–190.422 | 2.051 (1.08%) |
| zwtv scalar | new | 180.716, 181.161, 177.746 | 180.716 | 177.746–181.161 | 3.415 (1.89%) |
| zwtv avx2 | base | 98.980, 98.603, 95.889 | 98.603 | 95.889–98.980 | 3.091 (3.13%) |
| zwtv avx2 | new | 89.074, 89.171, 86.774 | 89.074 | 86.774–89.171 | 2.397 (2.69%) |
| stream scalar | base | 181.028, 181.266, 182.852 | 181.266 | 181.028–182.852 | 1.824 (1.01%) |
| stream scalar | new | 171.660, 171.417, 172.523 | 171.660 | 171.417–172.523 | 1.106 (0.64%) |
| stream avx2 | base | 82.963, 82.894, 84.004 | 82.963 | 82.894–84.004 | 1.110 (1.34%) |
| stream avx2 | new | 73.218, 73.076, 74.386 | 73.218 | 73.076–74.386 | 1.309 (1.79%) |

四臂變化：zwtv scalar -4.427% / zwtv avx2 -9.663% / stream scalar -5.300% / stream avx2 -11.746%。全部中位數下降，四臂無退步，兩個 AVX2 必要改善條件成立。

首組 new zwtv avx2 全距/中位數為 6.49%，base stream avx2 為 6.90%，依事先 5% 規則完整重測一次。第二組最大 3.13%，無殘留超門檻雜訊。全部 12 輪與 48 份 estimates 保留；首組各版三輪與全距見 performance/batch1-summary.json，第二組見 batch2-summary.json；不是從兩組挑較有利的一組。

同一 AMD Ryzen 5 3600、Windows x86_64、Rust/Cargo 1.99.1，兩版使用相同預設 feature 與獨立 target。先完成全部建置與測試，再以前景命令固定 `RAYON_NUM_THREADS=1`，次序 base1/new1/base2/new2/base3/new3。直接取 Criterion 每輪 `new/estimates.json` 的 `median.point_estimate`，再算三輪中位數的中位數；不採 Criterion 對上次快取結果的相對 change 值。

無退步採候選減基準中位數不超過兩版同版三輪全距較大者；AVX2 兩臂另要求中位數下降。事先宣告：任一臂／版本的全距除中位數超過 5%，整組交錯六輪重跑一次，保留首組；不挑選單一有利輪次。無人值守，未詢問使用者保持安靜；無法排除使用者活動，實測全距及重測狀態就是本輪雜訊證據。

## 測試、工具與例外處理

允許的 CI 清單明列執行，未執行 annexb_validator（即使 skip CSV）、golden 系列、stream_golden、ignored／include-ignored、Python MoSQITo parity 或私有材料檢查。`pending-user.md` 中這些命令僅供使用者執行。測試編譯與所有 cache、venv、pip／Cargo、臨時檔均置於指定驗收根。

GitHub run [46079306401](https://github.com/cclin99/soneflow-rs/actions/runs/46079306401) 由 API 獨立查證：完整 head SHA 相符、workflow_dispatch、8 jobs 全 success；受驗 CI 設定在 Linux 以 dump=0 斷言。線上 CI 包含本輪本機禁止執行的 validator 步驟，僅記錄 CI 結論，不在此本機重跑。

CRAP 警告保留：64 analyzed files、55 LCOV files、54 matched；21 個 null coverage entry 全落於 10 個 cfg(test) 檔案，無產品 entry 為 null。唯一 unmatched LCOV 的 tables.rs 無產品函式。另有 15 個 coverage=0 entry，不宣稱 100% coverage。詳見 `gates/crap-scope-audit.json`；CRAP gate 仍按原門檻通過。

本機 `core.autocrlf=true` 使工作樹標頭為 CRLF，cbindgen 讀 CRLF 設定時輸出混合換行；兩次原始 byte 比較的失敗均保留。從 HEAD 取 LF 設定到交付區再重生，輸出與 HEAD／baseline Git blob 完全相同，未改原檔。C smoke 首次以相對命令啟動失敗，改明確 exe 路徑後通過，保留兩份日誌。Git clone／bundled tar 的環境啟動錯誤，以 git archive＋Windows 原生 tar 取得固定快照；未改產品以規避問題。Get-CimInstance 拒絕讀取時，CPU 資訊以唯讀 Registry 取得。

**限制事件揭露：** 協調代理開頭讀取通用 ISO 驗證技能時，意外接觸該技能中的舊受限衍生雜湊範例；未引用、複製至交付、比對或用於探針，也未執行技能中的禁跑命令。已告知分派代理不要讀該技能。原始 `data` 完全未讀。事件詳見 `orchestration.md`；不能將本輪描述成完全沒有任何受限衍生資訊接觸。

## 交付、去識別化與完整性

主報告為本檔與 `report.en.md`；子目錄分別保存數值探針／原始位元結果、測試／coverage／工具日誌、效能 estimates／反組譯與逐句 CHANGELOG 核對。`gates/all-command-index.json` 保留初次失敗與重試，不能只讀 exit0 的摘要。

`<AUDIT_ROOT>` 為指定驗收根，`<SOURCE_REPO>` 為使用者原始材料工作目錄；重現命令前需在自己的環境代入。既有 redact_paths.py 不涵蓋本輪新根名，因此先補足該前綴，再執行工具處理及 `--check --strict`。所有格式另行逐檔檢查，詳見 `delivery-audit.json`。`integrity.json` 是去識別化後所有交付檔案（自身除外）的 SHA-256／大小清單；原始 rlib、來源 archive 等 hash 維持原始輸入含義，不與文字副本混用。交付內容及原始 tracked diff 已檢查，產品／測試／既有文件未改。

## 待使用者執行

A5：在持有合法材料的本機環境，以兩個固定版本執行 ISO 測試與 `iso_material_check.py --check` 並比較結果。必要的 Python parity／私有材料測試亦由使用者本人執行。逐行指令見 [pending-user.md](pending-user.md)。本輪沒有等待人工答覆，也沒有替使用者執行受限工作。

## 停車場

- 廣語料的非 RN 最大 5 ULP 已保留，供日後描述範圍時參考；不改本輪已接受的非 RN 契約。
- 通用驗證技能仍含舊受限衍生雜湊範例，宜另案清理；本輪僅揭露，未修改技能。
- CRAP scope 警告、Windows 換行／命令啟動差異已記錄；本輪不改工具或產品。
- README 的 0.2.0 Measured results 是否重量、非 x86_64 加速與其他已知延後事項，維持使用者既有決策；不另開一輪。
