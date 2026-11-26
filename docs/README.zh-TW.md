# 文件索引

> 對照／Pair: [English](README.md) · 來源：zh · 同步 2026-11-26

語言規則見 `docs/DOCUMENTATION-LANGUAGE-POLICY.zh-TW.md`。對照文件以 `NAME.md`（英文）與 `NAME.zh-TW.md`（繁體中文）成對存在，結構與數字完全相同。`python tools/doc_pairs.py --check` 檢查配對；`--list` 列出即時狀態。下表日期為該配對的「同步」日期。

## 從這裡開始

| 文件 | 內容 | en | zh |
|---|---|---|---|
| [`../README.md`](../README.md) | 專案 README：範圍、安裝、API、驗證範圍 | yes | [yes](../README.zh-TW.md) |
| [`ROADMAP.md`](ROADMAP.md) | 路線圖與延後項目 | yes | [yes](ROADMAP.zh-TW.md) |
| [`RELEASE-CHECKLIST.md`](RELEASE-CHECKLIST.md) | 每次發布的步驤 | yes | [yes](RELEASE-CHECKLIST.zh-TW.md) |
| [`GOLDEN-REGEN-SOP.md`](GOLDEN-REGEN-SOP.md) | MoSQITo golden 資料如何重生與驗證 | yes | [yes](GOLDEN-REGEN-SOP.zh-TW.md) |
| [`BEHAVIOUR-CONTRACTS.md`](BEHAVIOUR-CONTRACTS.md) | 函式庫的每項承諾與守住它的測試 | yes | [yes](BEHAVIOUR-CONTRACTS.zh-TW.md) |
| [`DOCUMENTATION-LANGUAGE-POLICY.md`](DOCUMENTATION-LANGUAGE-POLICY.md) | 本語言政策、術語表、撰寫原則 | yes | [yes](DOCUMENTATION-LANGUAGE-POLICY.zh-TW.md) |

## 發布紀錄

各版變更見 [`../CHANGELOG.md`](../CHANGELOG.md)。

## 獨立驗證

交付放在 `verification/<日期>-<議題>-<commit>/`，保留原撰寫語言；進庫前用 `tools/redact_paths.py` 去識別化路徑。
