---
title: 2026-08-19 實作方案來源索引
status: active
type: source-index
locale: zh-Hant
updated: 2026-08-19
---

# 2026-08-19 實作方案來源索引

## 來源與確認

本輪方案綜合三份歷史對話、既有正式文件與工作草稿 `plan/init.md`，提出 Rust、wgpu 門面、Nickel 組合、靜／動雙路徑、世界持久化與第一個 demo 路線圖。

維護者於 2026-08-19 確認方案整體方向，並把儲存選擇修正為：**demo 直接採 RocksDB，不先建立自訂 Region File。** 正式文件完成後，歷史 `chat-history` 與已吸收的 `plan/init.md` 草稿已刪除。

## 決策對照

| 方案主題 | 正式文件 |
| --- | --- |
| Rust 與核心自研／外購邊界 | [決策 0005](../decisions/0005-rust-and-focused-build-boundary.md)、[開發策略](../foundations/development-strategy.md) |
| wgpu 唯一 demo 後端與 headless 門面 | [決策 0006](../decisions/0006-wgpu-behind-rendering-facade.md)、[渲染架構](../architecture/rendering.md) |
| Nickel 與正規化 JSON 邊界 | [決策 0007](../decisions/0007-nickel-as-composition-language.md)、[套件管理](../architecture/package-management.md) |
| 靜態建置與資料夾動態載入 | [決策 0008](../decisions/0008-static-and-dynamic-realizations-share-one-graph.md)、[套件管理](../architecture/package-management.md) |
| RocksDB、權威區塊快照與 regenerate | [決策 0009](../decisions/0009-rocksdb-authoritative-world-snapshots.md)、[世界持久化](../architecture/world-persistence.md) |
| 分階段 demo 驗收 | [第一個 demo 路線圖](../planning/roadmap-first-demo.md) |
