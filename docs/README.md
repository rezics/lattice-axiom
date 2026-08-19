---
title: Lattice Axiom 文件
status: active
type: index
updated: 2026-08-19
---

# Lattice Axiom 文件

Lattice Axiom 是建在 Bevy 上的體素世界遊戲，不是自研遊戲引擎專案。這套文件保存目前的產品架構、決策、調查與第一個可玩原型驗收。

## 目前基線

> 默认使用完整上游能力；只有当一个真实可玩的原型证明现有方案无法满足不可妥协的产品需求时，才允许自研替代。

- Bevy `0.19.x` 是引擎基線，精確 patch 由實作時的 `Cargo.lock` 固定。
- runtime 直接使用 Bevy App、Plugin、ECS、schedule、asset、render、input、task 和 diagnostics。
- 世界座標採 Bevy 原生右手 Y-up。
- 第一階段內容用 Cargo + Bevy plugin + typed asset 組合。
- RocksDB 保存完整已物化世界 snapshot。
- Godot 只作未來工具鏈對照，不作第二 runtime。

## 建議閱讀順序

1. [專案願景與設計支柱](foundations/project-vision.md)
2. [Bevy-first 開發策略](foundations/development-strategy.md)
3. [Bevy-first 技術棧](foundations/technology-stack.md)
4. [開源遊戲引擎提供什麼，以及如何接入 Bevy](research/open-source-game-engine-adoption.md)
5. [Bevy 執行期架構](architecture/game-engine-runtime.md)
6. [第一個 Bevy 可玩 demo 路線圖](planning/roadmap-first-demo.md)
7. [Bevy 執行期整合路線](planning/roadmap-game-engine.md)
8. [Bevy 模組與內容組合](architecture/module-composition.md)
9. [世界持久化與 RocksDB World Store](architecture/world-persistence.md)
10. [可組合世界生成](architecture/world-generation.md)
11. [可組合洞穴生成](architecture/cave-generation.md)
12. [Bevy 渲染架構](architecture/rendering.md)
13. [Bevy 實體、物理與表現](architecture/entity-physics-presentation.md)
14. [Bevy 資產與模型語義](architecture/asset-semantics.md)
15. [版本與相容性](architecture/versioning-and-compatibility.md)
16. [待決問題](planning/open-questions.md)

## 文件地圖

| 分類 | 用途 | 現有內容 |
| --- | --- | --- |
| 基礎 | 說明遊戲要成為什麼、如何開發與採用哪些技術 | [願景](foundations/project-vision.md)、[開發策略](foundations/development-strategy.md)、[技術棧](foundations/technology-stack.md)、[詞彙表](foundations/glossary.md) |
| Bevy runtime | 說明 App、plugin、world、render、physics 與 asset 如何整合 | [執行期](architecture/game-engine-runtime.md)、[渲染](architecture/rendering.md)、[實體／物理／表現](architecture/entity-physics-presentation.md)、[資產語義](architecture/asset-semantics.md) |
| 內容與資料 | 說明公開內容路徑、分發邊界、版本與持久化 | [模組組合](architecture/module-composition.md)、[內容分發](architecture/package-management.md)、[版本相容](architecture/versioning-and-compatibility.md)、[世界持久化](architecture/world-persistence.md) |
| 世界生成 | 保存 Lattice Axiom 真正的產品差異層 | [世界生成](architecture/world-generation.md)、[洞穴生成](architecture/cave-generation.md)、[物理創作](architecture/physical-authoring.md) |
| 決策 | 保存目前已接受的重大方向 | [0001：領地優先世界生成](decisions/0001-territory-first-biome-driven-world-generation.md)、[0002：混合洞穴](decisions/0002-hybrid-cave-generation-composition.md)、[0003：無全域版本開關](decisions/0003-no-global-version-switch.md)、[0004：領地委派](decisions/0004-territorial-delegation-for-spatial-generation.md)、[0009：RocksDB 權威快照](decisions/0009-rocksdb-authoritative-world-snapshots.md)、[0012：命名約定](decisions/0012-latticeaxiom-naming-convention.md)、[0014：採用 Bevy／上游優先](decisions/0014-adopt-bevy-upstream-first.md)、[0015：Bevy Y-up](decisions/0015-bevy-native-y-up-world-coordinates.md)、[0016：Bevy 插件式內容組合](decisions/0016-stage-content-composition-on-bevy.md) |
| 調查 | 記錄引擎接入、upstream 候選與外部設計教訓，不自動成為承諾 | [開源引擎接入基線](research/open-source-game-engine-adoption.md)、[Bevy 生態候選](research/renderer-physics-landscape.md)、[Godot 工具鏈對照](research/godot-toolchain-comparison.md)、[Minecraft 世界生成教訓](research/minecraft-world-generation-lessons.md)、[現代地形與洞穴生成](research/modern-terrain-and-cave-generation.md) |
| 規劃 | 以可玩結果排序工作和證據 | [第一個 demo](planning/roadmap-first-demo.md)、[Bevy 整合路線](planning/roadmap-game-engine.md)、[待決問題](planning/open-questions.md) |
| 元文件 | 說明文件如何維護 | [文件組織方式](meta/documentation-organization.md) |

## 成熟度

- `exploration`：調查或尚未由原型驗證的方向。
- `proposed`：具體可評估架構，細節仍需實作證據。
- `accepted`：已明確採納的決策或策略。
- `active`：目前生效的索引、規劃或參考。

目前尚無程式實作。accepted ADR 是開工邊界，architecture 內容仍會由第一個可玩 Bevy 原型修正；任何自研 upstream 替代需要新的 accepted ADR。

## 目前刻意沒有

倉庫尚未建立 tutorial、操作指南或 API reference。等第一個可重複執行的 build 存在，再依真實玩家、作者和維護者任務增加，不先建立空目錄。
