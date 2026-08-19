---
title: Bevy 模組與內容組合
status: proposed
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0016-stage-content-composition-on-bevy.md
---

# Bevy 模組與內容組合

## 結論

首階段的模組就是 Bevy plugin，內容就是 typed definition／asset。Cargo 選擇要編進產品的 crate，`LatticeAxiomPluginGroup` 組合預設 plugin，Bevy App 執行它們；不存在第二套 module lifecycle 或 runtime package graph。

這個簡單基線仍必須證明一項重要產品原則：官方內容不走私有捷徑。

## 組合層次

```text
Cargo workspace / Cargo.lock
        ↓ selects code and exact dependencies
Bevy App + Plugin / PluginGroup
        ↓ registers systems, resources and asset types
content plugins + typed assets
        ↓ register stable domain definitions
validated ContentCatalog
        ↓ runtime indices / ECS / worldgen / presentation
```

這些層次回答不同問題：

| 層 | 回答 | 不回答 |
| --- | --- | --- |
| Cargo | 哪些 Rust crate 與精確相依被建置？ | 玩家在執行期下載哪些 mods |
| Bevy plugin | 哪些 systems、resources、states、assets 被加入 App？ | 長期內容 ID 如何遷移 |
| content catalog | 方塊、物品、群系與實體定義是什麼？ | Bevy 本身如何版本解析 |
| world／save schema | 哪些內容已進入權威世界，如何恢復？ | crate 如何編譯 |

## Plugin 分類

### Foundation／domain plugin

提供一個產品領域的 resources、components、events／messages、systems、asset types 和 diagnostics，例如 World、Worldgen、Persistence。這些 plugin 應保持小而可單獨測試，不以「未來可換引擎」為由隱藏 Bevy。

### Mechanic plugin

在既有領域 API 上加入可重用玩法，例如方塊交互、生命值、掉落、流體或 cave destination。它可以依賴 domain plugin，但不擁有全域啟動流程。

### Content plugin

註冊具體方塊、物品、群系、實體 archetype 與資產引用。官方內容、測試內容和未來外部內容都屬此類。

### Tooling／diagnostic plugin

只服務開發 profile，例如 gizmo、inspector、worldgen visualization 和 save diagnostics。它不得成為權威世界正常載入的必要條件。

## 預設 PluginGroup

`LatticeAxiomPluginGroup` 提供官方產品的方便組合，但每個 plugin 仍可獨立加入：

```rust
App::new()
    .add_plugins(DefaultPlugins)
    .add_plugins(LatticeAxiomPluginGroup)
    .add_plugins(OfficialContentPlugin);
```

這段是組合意圖，不是承諾最終 API 名稱。headless app 使用不同的 Bevy standard plugin profile，但加入相同 World、Worldgen、Persistence、Gameplay 和 Content plugin。

## 內容註冊

內容定義需要 stable ID，例如：

```text
latticeaxiom:block/stone
latticeaxiom:item/stone
latticeaxiom:biome/highland
example.test:block/marker
```

content plugin 透過公開 App extension 或 domain registration function 提交 typed definition。所有 plugin 完成註冊後，catalog validation 至少檢查：

- stable ID 語法與唯一性；
- 必要資產／關聯內容是否存在；
- 方塊、物品、recipe、loot 和生成引用的型別正確性；
- schema／definition revision 是否受支援；
- server／headless profile 是否意外依賴純 client asset；
- duplicate、cycle 和缺失內容能否給出 owner-aware 診斷。

若熱路徑需要 numeric ID，只能在 validation 後由 stable ID 的規範排序導出，或由世界 snapshot 保存自己的 palette mapping；不得依 plugin 加入順序分配永久 ID。

## Typed assets 與 Rust registration

選擇標準：

- 只包含資料、希望作者不編譯 Rust：優先 typed asset。
- 需要 systems、queries 或新玩法行為：使用 Bevy plugin。
- 固定 schema 的大量內容：typed asset + 一個共用 loader／mechanic plugin。
- 只為一項官方內容而新增通用 hook：先用第二個內容 fixture 證明它真的是公共能力。

資料格式應從最小需求出發。若 serde + RON／JSON 或既有 Bevy loader 足夠，不建立宣告語言。

## Schedule 擴充

mechanic plugin 直接把 systems 加入 Bevy schedule，並對齊少量公開 domain `SystemSet`。公共 set 表達語義 barrier，例如「world command 已套用」或「persistence capture 前」，而不是允許 plugin 宣告任意全域 DAG。

規則：

- 只有 correctness dependency 才使用 `before`／`after`；
- 不以 plugin discovery 順序決定 gameplay；
- 系統的 conflict／ambiguity 要在測試中可見；
- background task 的結果仍需 revision validation；
- 外部內容不得把 system 插入會繞過權威 command／persistence 的私有位置。

## 啟動與失敗

Bevy plugin build、App state、asset load state 和 Startup／OnEnter schedule 已提供啟動工具。Lattice Axiom 只增加產品需要的狀態，例如 `LoadingWorld`、`Playing`、`ShuttingDown`。

內容 validation 在進入 `Playing` 前完成。duplicate stable ID、缺少必要 definition 或 schema 不受支援時，啟動應顯示 owner-aware 診斷並停止進入世界，不能以最後註冊者覆蓋。

## 官方內容測試

公開擴充邊界至少由兩個 consumer 驗證：

1. `OfficialContentPlugin`：提供 demo 的 stone／dirt、基本群系與物品。
2. `TestContentPlugin`：使用不同 namespace，替換或新增一個最小方塊／群系。

測試應能：

- 只使用 test content 啟動 headless world；
- 任意調換兩個 content plugin 的加入順序，catalog 結果相同；
- 對 duplicate ID 產生穩定錯誤；
- 保存、重啟並重新解析兩個 namespace 的 stable ID；
- 移除一個 content plugin 時產生明確 missing-content policy，而不是 silent remap。

## 動態內容不是當前需求

首個 demo 不支援執行期下載、動態 Rust library、WASM、卸載或一般版本求解。若第二內容集合與真實作者流程證明需要新的分發方式，另依[內容分發的分階段邊界](package-management.md)研究。

未來即使增加 package layer，它也應把內容實現成正常 Bevy plugin／asset，不能建立第二個 schedule、ECS 或 asset system。

## 相關文件

- [決策 0016：以 Bevy 插件起步](../decisions/0016-stage-content-composition-on-bevy.md)
- [Bevy 執行期架構](game-engine-runtime.md)
- [內容分發的分階段邊界](package-management.md)
- [版本與相容性](versioning-and-compatibility.md)
- [第一個可玩 demo 路線圖](../planning/roadmap-first-demo.md)
