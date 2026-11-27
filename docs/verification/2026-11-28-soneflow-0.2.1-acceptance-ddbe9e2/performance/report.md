# 效能與反組譯獨立驗收

A4 PASS、N2 PASS；R2 的效能與反組譯句成立。受驗 ddbe9e2，基準 82d1d61，同機、同工具鏈、預設 features、release、RAYON_NUM_THREADS=1。其餘 R2 逐句證據見 changelog-audit.md；數值與既有測試依各獨立驗收子報告。

## A4

事先固定的雜訊規則與判定式見 method.md。待所有其他代理建置/測試停止後才啟動前景量測。首批 new zwtv avx2 與 base stream avx2 的三輪全距/中位數分別為 6.49% / 6.90%，超過預定 5%，因此完整重做六輪一次；首批全部原始證據保留。第二批最大比值 3.13%，不再觸發明顯雜訊。使用者可能使用機器，不能宣稱專用或完全空閒環境。

每輪直接取 Criterion median.point_estimate；以下是第二批結果，單位 ms。base/new 各三輪交錯。不是對六輪挑選最佳值；採事先規則完整重測的第二批。

| 臂 | 版本 | 三輪 median | 三輪中位數 | 全範圍 min–max | 全距及其比例 |
|---|---|---|---|---|---|
| zwtv_10s_t1/scalar/480000 | base | 189.087044, 190.422331, 188.371115 | 189.087044 | 188.371115–190.422331 | 2.051216 (1.085%) |
| zwtv_10s_t1/scalar/480000 | new | 180.716151, 181.160632, 177.746034 | 180.716151 | 177.746034–181.160632 | 3.414598 (1.889%) |
| zwtv_10s_t1/avx2/480000 | base | 98.979758, 98.602643, 95.888691 | 98.602643 | 95.888691–98.979758 | 3.091067 (3.135%) |
| zwtv_10s_t1/avx2/480000 | new | 89.074183, 89.171227, 86.774349 | 89.074183 | 86.774349–89.171227 | 2.396878 (2.691%) |
| stream_push_10s_480chunk/scalar | base | 181.028280, 181.266494, 182.851781 | 181.266494 | 181.028280–182.851781 | 1.823501 (1.006%) |
| stream_push_10s_480chunk/scalar | new | 171.659839, 171.417493, 172.523454 | 171.659839 | 171.417493–172.523454 | 1.105961 (0.644%) |
| stream_push_10s_480chunk/avx2 | base | 82.963193, 82.893734, 84.003706 | 82.963193 | 82.893734–84.003706 | 1.109972 (1.338%) |
| stream_push_10s_480chunk/avx2 | new | 73.218027, 73.076293, 74.385671 | 73.218027 | 73.076293–74.385671 | 1.309378 (1.788%) |

| 臂 | candidate 相對 baseline | 無退步 |
|---|---|---|
| zwtv_10s_t1/scalar/480000 | -4.427% | PASS |
| zwtv_10s_t1/avx2/480000 | -9.663% | PASS |
| stream_push_10s_480chunk/scalar | -5.300% | PASS |
| stream_push_10s_480chunk/avx2 | -11.746% | PASS |

四臂中位數皆下降，且改善幅度均大於同版全距雜訊界。兩個必須改善的 AVX2 臂成立。證據：decision.json、batch2-summary.json；每筆 raw estimates 路徑列於 summary rows[].evidence。首批在 batch1-summary.json 與其引用檔；12 輪 bench log 全保留。沒有以實作者百分比作證據。

## N2 / R3 指令重現

| 版本 / 函式 | pow relocation | stmxcsr |
|---|---|---|
| baseline main_loudness_unchecked | 11 | 0 |
| candidate main_loudness_unchecked | 14 | 1 |
| candidate init_mp1_table_rn | 20 | 0 |

實際執行 llvm-objdump -d -r release rlib（Windows COFF）。objdump-summary.json 保存完整原始 dump 行號：baseline 主函式 25845，candidate 主函式 23966，candidate initializer 38800。三份函式摘錄各從第 1 行開始；不是把呼叫點或 OnceLock wrapper 計入。這個工具鏈確實保留 20 個填表 pow 及非 RN runtime 分支。二進位檔名與原始 SHA256 見 binary-inputs.json；兩版 archive 完整 commit 與83檔產品src/bench/manifest相符證據見 source-integrity.json。原始binary/source hash與後續去識別化交付文字檔 hash 不同義。

## 邊界

未處理 ISO 材料、未執行禁列測試，未更改產品/既有文件、未 commit/push。量測全部完成，無待執行 benchmark。整體發布判定仍由總報告匯整其他門檻與 A5 待使用者事項。
