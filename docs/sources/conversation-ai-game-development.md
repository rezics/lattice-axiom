---
title: AI 協作遊戲開發對話來源索引
status: active
type: source-index
locale: zh-Hant
updated: 2026-08-19
---

# AI 協作遊戲開發對話來源索引

## 來源

- 歷史原始檔名：`ChatGPT-AI做游戏可行性.md`
- 主要問題：一位主要開發者能否借助 AI 與開源基礎設施實作系統型遊戲
- 處理方式：按開發責任、買／造邊界與垂直切片驗收整理後，於 2026-08-19 依維護要求刪除原始 `chat-history` 檔

原始對話中的具體引擎偏好後來由 Rust 技術方案修正；正式決策以 ADR 為準。

## 主題對照

| 對話主題 | 整理後文件 |
| --- | --- |
| 一人 + AI 的可行邊界 | [一人與 AI 協作的開發策略](../foundations/development-strategy.md) |
| AI 適合實作，維護者負責語義與不變量 | [一人與 AI 協作的開發策略](../foundations/development-strategy.md) |
| 採 upstream 而非複製大型渲染器 | [決策 0005](../decisions/0005-rust-and-focused-build-boundary.md) |
| 粗糙美術、先做可玩垂直切片 | [第一個 demo 路線圖](../planning/roadmap-first-demo.md) |
| 渲染、物理與資產基礎設施延後自研 | [渲染架構](../architecture/rendering.md)、[決策 0005](../decisions/0005-rust-and-focused-build-boundary.md) |

