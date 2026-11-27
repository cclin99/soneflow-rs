# 獨立數值驗收（A1 自製差分、A2、N1、N3）

受驗 ddbe9e2；基準 82d1d61。Windows x86_64。本報告只判定指定子項；不代表整體發布 PASS。

## 結果

- A1 自製差分 PASS：debug 與 release 各 1068 列完全一致，0 個位元差異。每場型 430 組頻譜，共 860 次 main_loudness 呼叫（含超過 120 dB 的拒絕）。每個 critical band 的門檻附近、120 dB 前一/本身/後一可表達值、257 組固定 LCG 頻譜均涵蓋。低三頻帶以二分法定位群組門檻。Free/Diffuse 均有 9600 點自製雙音分段振幅訊號；batch 比較 n、n_specific、time_axis、bark_axis，stream 以 137 點分塊，比較 push/flush 的 frame index、n、n_phon、flags。這是兩版各自輸出的跨版比較，不宣稱 batch 與 stream 互相比位元。
- A2 PASS：隔離 candidate 快照把 rounding match 換為無條件 Some(mp1_table_rn())。順序測試 exit 101，在第 136 行 RN 第二次呼叫的 band 0 assertion 失敗；got bits 4604434098601348759，runtime bits 4604434098601348761。原檔逐位元還原後 debug/release 各 1 passed。ci.yml 的 test 與 arch/llvm-cov 清單均含 main_loudness_rounding。
- N1 PASS：三種非 RN 模式、兩場型、所有合法頻譜，candidate debug/release 與自製 runtime reference 完全一致。debug 與基準全同。release 的差異只在 critical bands 0–2，見下表。三模式下 loudness_zwst、loudness_zwtv、push_with_quality 均拒絕；legacy push/flush 成功。參考算式獨立撰寫，只讀取公開 crate table 與產品運算結構，未採用實作者診斷程式或其數字。
- N3 PASS：四種 FTZ/DAZ 組合分別從新 process 的空快取開始，debug/release 的 860 次 main_loudness 結果完全一致。另於隔離快照只增設 OnceLock.get().is_some() 唯讀觀測函式，release 四個新 process 均 before_new=false、after_new=true、after_push=true，證明 RN stream constructor 已填表；即使 FTZ/DAZ 開啟也成立。觀測函式已逐位元還原。

| 模式 | release 不同頻帶及 ULP 範圍 |
|---|---|
| 向下 | 0: 1–5；2: 1–2 |
| 向上 | 0: 1–5；1: 2–5；2: 1–2 |
| 向零 | 0: 1–5；2: 1–2 |

## 證據與重現

probe.rs 為自行撰寫的語料、runtime reference 與輸出探針；run.ps1 從獨立 git archive 快照建置及執行。compare.py 產生 comparison.json。mutate.ps1 記錄突變/還原測試及 cache_probe.rs 唯讀觀測。CSV 所有浮點數均為原始 binary64 bits 十六進位。inputs.csv 可對應每筆頻譜。guard 行是該模式拒絕/接受斷言通過後的記錄。

snapshot-restoration.json 比對 archive 中全部 425 / 426 個追蹤檔案，兩快照皆零變更。僅新增 example 探針留在隔離快照。original-source-sha256.txt / restored-source-sha256.txt 補充被觀測檔案還原證據。原 repo 產品 git diff 為空；repo-status.txt 記錄當時狀態。首次 local clone 因 Git sh CreateFileMapping 錯誤失敗，改以 git archive，不影響受驗來源。

未開啟/處理 ISO 材料，未執行禁列測試，未 commit/push。所有建置/暫存位於驗收根目錄。CARGO_HOME 使用隔離副本；RUSTUP_HOME 保持既有只讀位置。

## 觀察（不擴增主線）

廣語料找到 10 筆最大 5 ULP 差異；five-ulp-cases.json 列出 mode、field、case、band、兩版 bits、完整輸入 bits 與十進位值。此結果超過交辦中實作者窄語料的 1–3 ULP，但 CHANGELOG 並未承諾該上限，其「三個最低頻帶末位」敘述仍成立。沒有修改產品或放寬斷言。
