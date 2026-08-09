---
title: Lattice Axiom 文件
status: active
type: index
locale: zh-Hant
updated: 2026-08-09
---

# Lattice Axiom 文件

這套文件是 Lattice Axiom 的共同思考空間。目前的主要讀者是參與遊戲策劃、架構討論與原型設計的人；現階段不是玩家手冊，也不是已可實作的完整規格。

## 建議閱讀順序

1. [專案願景與設計支柱](foundations/project-vision.md)
2. [模組核心與宣告式組合](architecture/module-composition.md)
3. [渲染架構與擴充邊界](architecture/rendering.md)
4. [實體、物理與表現層](architecture/entity-physics-presentation.md)
5. [資產管線與模型語義](architecture/asset-semantics.md)
6. [物理資產與局部形變創作](architecture/physical-authoring.md)
7. [待決問題](planning/open-questions.md)

## 文件地圖

| 分類 | 用途 | 現有內容 |
| --- | --- | --- |
| 基礎 | 說明遊戲目前想成為什麼，以及詞彙如何使用 | [專案願景](foundations/project-vision.md)、[詞彙表](foundations/glossary.md) |
| 架構 | 整理可被驗證的設計假說與責任邊界 | [模組組合](architecture/module-composition.md)、[渲染](architecture/rendering.md)、[實體與表現](architecture/entity-physics-presentation.md)、[資產語義](architecture/asset-semantics.md)、[物理創作](architecture/physical-authoring.md) |
| 調查 | 比較外部做法與候選技術，不代表已選用 | [渲染與物理技術地圖](research/renderer-physics-landscape.md) |
| 規劃 | 集中尚待回答、原型化或量測的問題 | [待決問題](planning/open-questions.md) |
| 元文件 | 說明文件本身如何整理與演進 | [文件組織方式](meta/documentation-organization.md) |
| 來源 | 保存討論與整理頁之間的追溯關係 | [2026-08-09 對話索引](sources/conversation-2026-08-09.md) |

## 成熟度標記

- `exploration`：探索中的想法，尚未形成具體提案。
- `proposed`：已有可評估方案，仍待討論或驗證。
- `accepted`：已明確採納；重大選擇應另有決策紀錄。
- `superseded`：已由另一份文件或決策取代。
- `active`：目前生效的索引、流程或維護規則。

除非頁首明確標為 `accepted`，否則不可把內容當作 Lattice Axiom 的最終規格。

## 目前刻意沒有的文件

倉庫還沒有可執行產品，因此暫不建立空的教學、操作指南與 API 參考目錄。等到第一個可重複執行的原型出現，再依實際讀者任務增加這些文件。

