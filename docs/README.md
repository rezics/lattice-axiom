---
title: Lattice Axiom 文件
status: active
type: index
locale: zh-Hant
updated: 2026-08-19
---

# Lattice Axiom 文件

這套文件是 Lattice Axiom 的架構、決策與原型驗收基線。目前的主要讀者是參與遊戲策劃、實作與審查的人；現階段不是玩家手冊，也不是完整產品規格。

## 建議閱讀順序

1. [專案願景與設計支柱](foundations/project-vision.md)
2. [一人與 AI 協作的開發策略](foundations/development-strategy.md)
3. [模組核心與宣告式組合](architecture/module-composition.md)
4. [套件管理、Nickel 組合與雙實現路徑](architecture/package-management.md)
5. [版本、相依性與相容性架構](architecture/versioning-and-compatibility.md)
6. [世界持久化與 RocksDB World Store](architecture/world-persistence.md)
7. [可組合世界生成架構](architecture/world-generation.md)
8. [可組合洞穴生成架構](architecture/cave-generation.md)
9. [渲染架構與擴充邊界](architecture/rendering.md)
10. [實體、物理與表現層](architecture/entity-physics-presentation.md)
11. [資產管線與模型語義](architecture/asset-semantics.md)
12. [物理資產與局部形變創作](architecture/physical-authoring.md)
13. [第一個可玩 demo 路線圖](planning/roadmap-first-demo.md)
14. [待決問題](planning/open-questions.md)

## 文件地圖

| 分類 | 用途 | 現有內容 |
| --- | --- | --- |
| 基礎 | 說明遊戲目前想成為什麼、如何開發，以及詞彙如何使用 | [專案願景](foundations/project-vision.md)、[開發策略](foundations/development-strategy.md)、[詞彙表](foundations/glossary.md) |
| 架構 | 整理可被驗證的設計假說與責任邊界 | [模組組合](architecture/module-composition.md)、[套件管理](architecture/package-management.md)、[版本與相容性](architecture/versioning-and-compatibility.md)、[世界持久化](architecture/world-persistence.md)、[世界生成](architecture/world-generation.md)、[洞穴生成](architecture/cave-generation.md)、[渲染](architecture/rendering.md)、[實體與表現](architecture/entity-physics-presentation.md)、[資產語義](architecture/asset-semantics.md)、[物理創作](architecture/physical-authoring.md) |
| 決策 | 保存已明確採納的重大方向、理由與後果 | [0001：領地優先](decisions/0001-territory-first-biome-driven-world-generation.md)、[0002：混合洞穴](decisions/0002-hybrid-cave-generation-composition.md)、[0003：無全域大版本](decisions/0003-no-global-version-package-scoped-compatibility.md)、[0004：領地委派](decisions/0004-territorial-delegation-for-spatial-generation.md)、[0005：Rust 與自研邊界](decisions/0005-rust-and-focused-build-boundary.md)、[0006：wgpu 門面](decisions/0006-wgpu-behind-rendering-facade.md)、[0007：Nickel](decisions/0007-nickel-as-composition-language.md)、[0008：雙實現路徑](decisions/0008-static-and-dynamic-realizations-share-one-graph.md)、[0009：RocksDB 權威快照](decisions/0009-rocksdb-authoritative-world-snapshots.md) |
| 調查 | 比較外部做法與候選技術，不代表已選用 | [渲染與物理技術地圖](research/renderer-physics-landscape.md)、[Minecraft 世界生成與洞穴模組教訓](research/minecraft-world-generation-lessons.md)、[現代地形與洞穴生成](research/modern-terrain-and-cave-generation.md) |
| 規劃 | 集中 demo 執行順序與尚待回答、原型化或量測的問題 | [第一個 demo 路線圖](planning/roadmap-first-demo.md)、[待決問題](planning/open-questions.md) |
| 元文件 | 說明文件本身如何整理與演進 | [文件組織方式](meta/documentation-organization.md) |
| 來源 | 保存討論、草稿與整理頁之間的追溯關係 | [2026-08-09 對話](sources/conversation-2026-08-09.md)、[資料庫對話](sources/conversation-game-database.md)、[AI 開發對話](sources/conversation-ai-game-development.md)、[2026-08-19 實作方案](sources/implementation-plan-2026-08-19.md) |

## 成熟度標記

- `exploration`：探索中的想法，尚未形成具體提案。
- `proposed`：已有可評估方案，仍待討論或驗證。
- `accepted`：已明確採納；重大選擇應另有決策紀錄。
- `superseded`：已由另一份文件或決策取代。
- `active`：目前生效的索引、流程或維護規則。

`accepted` 決策是目前實作基線，但仍可由後續 ADR 明確取代；`proposed` 頁面中引用的 accepted 決策與尚待量測細節應分開閱讀。

## 目前刻意沒有的文件

倉庫還沒有可執行產品，因此暫不建立空的教學、操作指南與 API 參考目錄。等到第一個可重複執行的原型出現，再依實際讀者任務增加這些文件。
