---
title: 2026-08-09 對話來源索引
status: active
type: source-index
locale: zh-Hant
updated: 2026-08-09
---

# 2026-08-09 對話來源索引

## 來源

- 原始檔名：`ChatGPT-插件架構與性能优化.md`
- 原始位置：`C:/Users/edge/Downloads/ChatGPT-插件架構與性能优化.md`
- 規模：7 輪提問與回覆
- 本次處理：按主題重寫為繁體中文 living docs

原始檔位於個人下載目錄，不是可攜的倉庫來源。本頁保存追溯關係；整理後的主題頁才是後續修訂入口。原始對話中的外部連結與技術敘述尚未全部逐項驗證。

## 回合對照

| 回合 | 討論焦點 | 整理後文件 |
| --- | --- | --- |
| 1 | 所有生物、群系與內容皆為模組；如何避免執行期額外成本 | [模組核心與宣告式組合](../architecture/module-composition.md) |
| 2 | 開源核心、第三方靜態連結、Nix 式整合包與可重現建置 | [模組核心與宣告式組合](../architecture/module-composition.md) |
| 3 | 模組化渲染、渲染擷取、批次化、Render Graph 與 shader 建置 | [渲染架構與擴充邊界](../architecture/rendering.md) |
| 4 | 獨立開源渲染器、自訂實體、自訂移動與物理能力 | [實體、物理與表現層](../architecture/entity-physics-presentation.md)、[技術地圖](../research/renderer-physics-landscape.md) |
| 5 | 受擊與移動效果成為可替換的通用表現模組 | [實體、物理與表現層](../architecture/entity-physics-presentation.md) |
| 6 | glTF/GLB、骨架語義、能力式模型規格、VRM 與 OpenUSD | [資產管線與模型語義](../architecture/asset-semantics.md) |
| 7 | 在 Blender 中創作局部物理性質，執行期自動產生柔性反應 | [物理資產與局部形變創作](../architecture/physical-authoring.md) |

## 整理原則

- 保留設計意圖與重要限制，不保留對話式重複。
- 把「建議」改寫為「候選」或「假說」，除非已有明確決策。
- 將跨回合逐漸修正的想法合併到最新主題頁。
- 把尚無答案的部分移入[待決問題](../planning/open-questions.md)。

