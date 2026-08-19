---
title: Bevy 資產管線與模型語義
status: exploration
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0015-bevy-native-y-up-world-coordinates.md
  - ../decisions/0020-semantic-registration-and-content-selection.md
---

# Bevy 資產管線與模型語義

## 結論

资产runtime默认走Bevy `AssetServer`、standard loaders与glTF pipeline。package manifest／lock负责stable asset identity、owner、source／artifact hash与profile availability；它不复制Bevy asset dependency graph。只有游戏特有且无法由现有格式保存的语义，才加入小型typed asset、sidecar或importer extension。

## 三種資料不要混用

| 層 | 例子 | 目的 |
| --- | --- | --- |
| 創作來源 | Blender、Krita、音訊工程檔，或日後經驗證的 Godot 工具 | 保存作者可編輯資訊 |
| 交換／專案資產 | glTF、PNG／KTX2、WGSL、音訊、專案 typed asset | 由 Git、工具與 Bevy loader 交換 |
| 執行期資源 | Bevy Mesh、Image、Material、Animation、Scene、Audio handle | 在目前程序載入和呈現 |

創作來源不必直接進 runtime；Bevy handle 也不是可跨重啟保存的資產 ID。

## 預設載入路徑

```text
source authoring file
        ↓ export
glTF / image / audio / typed data asset
        ↓ package ownership / stable asset ID validation
        ↓ Bevy AssetServer + loaders
Bevy assets and dependency graph
        ↓ handles used by ECS presentation
```

優先使用：

- Bevy glTF scene／mesh／material／animation loader；
- Bevy Image 與 shader asset；
- `AssetServer` 的載入狀態、相依與 hot reload；
- 真的需要新副檔名時才實作 Bevy `AssetLoader`；
- 只有資產集合與 app state 管理成為重複問題時才評估 `bevy_asset_loader`。

## 座標與單位

執行期、物理與交換基線是 Bevy 原生右手 Y-up：`+X` 右、`+Y` 上、forward `-Z`，距離為公尺，角度為弧度。

glTF 優先交由 Bevy loader 按其官方座標處理。其他來源格式在 exporter／importer 邊界轉換一次；執行期只保留統一 Y-up 表示，不建立全域軸轉換 component。

資產 fixture 至少驗證：

- forward／up 朝向；
- skeleton bind pose、animation root motion；
- triangle winding、normal、tangent；
- collider 與視覺 mesh 對齊；
- 1 unit 對應的公尺尺度。

## 遊戲特有語義

普通模型先用 glTF node、material、animation 和清楚的命名約定。只有被兩個真實資產共同需要、且命名約定無法安全表達時，才定義 versioned semantic sidecar 或 typed asset。

可能的 domain 語義包括：

- hit zone 與 damage multiplier；
- 裝備 socket；
- foot／hand／eye 等 attachment point；
- collider intent 與 movement affordance；
- 可形變區域、材質物理參數或 interaction tag。

sidecar 應以 stable semantic name 對應 glTF node，不保存 Bevy `Entity`、`Handle` 或 importer 產生順序。loader 解析後可以轉成正常 Bevy component／resource。

## 穩定資產識別與 Handle

| 識別 | 生命週期 | 可否持久化 |
| --- | --- | --- |
| stable content／asset ID | 由logical package／schema owner维护，跨发行迁移 | 可以 |
| logical asset path／label | 專案資產圖內可診斷引用 | 視格式契約而定 |
| Bevy `Handle<A>` | 目前 App／AssetServer 執行期 | 不可以 |
| ABI opaque asset handle | 目前EngineInstance／assets interface | 不可以 |
| GPU resource／render entity | 目前 device／frame | 不可以 |

存档保存stable content／asset ID与必要schema；加载时由`RegistrationImage`解析owner／package-relative asset，再由Bevy AssetServer产生handle。dynamic module只取得ABI opaque handle。缺失内容要产生package-aware诊断或明确placeholder policy，不能保存raw handle期待下次有效。

## Hot reload

開發期可以熱重載純表現資產。若資產會改變 collision、可通行性、生成結果或玩法判定，它就是權威輸入：

- 修改後不直接重寫已物化世界；
- 需要新的 generator／content revision；
- 只有在安全 reload、顯式 migration 或新 session 後生效；
- client 與 headless 對同一權威語義有一致解析。

## 體素資產

第一個 demo 的方塊定義只需要 stable ID、可見材質、collision kind、破壞／放置規則、
有限state schema与简单音效引用。跨package内容语义不从asset路径或模型名称推断，而由
[SemanticTag／Map、StateProperty、Affordance、Predicate与Role](semantic-registration.md)
声明。atlas 或 texture array 的实际布局由 Bevy／voxel plugin spike 决定，不先定义自有
universal material format或无型别properties bag。

block definition、semantic contribution 与placement选择必须分开：

- definition拥有exact stable ID与本体schema；
- Tag／Map描述definition-level集合与typed values；
- StateProperty描述已放置instance的有限状态；
- structure／worldgen用Predicate接受候选，用Role取得要写入chunk的concrete ID。

若採 `bevy_voxel_world`，其 voxel type 與 material 設定可以是初始 adapter；權威存檔仍保存 Lattice Axiom stable block ID，而不是 plugin-private numeric ID。

## Godot 工具鏈對照

Godot 可以在未來作視覺 authoring／import workflow 對照，但不是 runtime 或權威場景格式。任何 bridge 只交換明確資料，例如 glTF 與 versioned sidecar；不得要求遊戲同時載入 Godot runtime。啟動條件與驗收見[Godot 工具鏈對照](../research/godot-toolchain-comparison.md)。

## 驗收

- 一個 glTF fixture 在 Bevy 中無額外軸修正即可正確朝向、播放動畫並對齊 collider。
- client 由 `AssetServer` 載入資產；headless 不必載入純視覺資產也能解析權威 content ID。
- hot reload 視覺材質不改變 world hash；修改 collision semantic 必須走顯式權威更新。
- 重啟後 stable content ID 能重新解析，存檔中不存在 Bevy handle。
- static／dynamic realization引用同一stable asset ID，manifest／lock可定位相同owner与artifact hash。
- semantic Tag／Map改变不偷偷覆盖asset identity；authoritative semantic change建立新registration image，presentation-only change才可安全client reload。
- custom asset type 必須有 malformed-input、version 和 round-trip test。

## 相關文件

- [決策 0015：Bevy 原生 Y-up](../decisions/0015-bevy-native-y-up-world-coordinates.md)
- [Bevy 執行期架構](game-engine-runtime.md)
- [渲染架構](rendering.md)
- [實體、物理與表現層](entity-physics-presentation.md)
- [物理資產與局部形變創作](physical-authoring.md)
- [語義註冊、內容判定與選擇](semantic-registration.md)
