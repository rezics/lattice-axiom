---
title: 開源遊戲引擎提供什麼，以及如何接入 Bevy
status: active
type: research
updated: 2026-08-19
---

# 開源遊戲引擎提供什麼，以及如何接入 Bevy

## 結論

開源遊戲引擎不是只有 renderer，也不是必須接管產品的一個封閉黑盒。它是一組已被大量遊戲共同需要的 runtime、資料模型、平台整合、內容管線、診斷和擴充機制。

Lattice Axiom没有“无法接入完整引擎”的证据。Bevy本身以Rust crate、App与Plugin组合，正好允许项目采用完整上游能力，再由package-generated domain adapters加入worldgen、persistence与gameplay。过去需要自研engine的结论来自预先假设的抽象边界，不是可玩prototype证明的限制。

## 一個現代遊戲引擎通常提供什麼

| 能力層 | 典型責任 | Lattice Axiom 的選擇 |
| --- | --- | --- |
| App lifecycle | 啟動、runner、plugin 初始化、退出 | Bevy App／Plugin |
| 遊戲資料模型 | ECS、entity／component、resource、change detection | Bevy ECS |
| 時間與排程 | frame、fixed tick、system ordering、parallel execution | Bevy schedules／`FixedUpdate` |
| 非同步工作 | compute／I/O task、thread pools | Bevy task pools |
| 平台 | window、input、event loop、filesystem integration | Bevy standard plugins |
| Rendering | camera、mesh、material、lighting、visibility、GPU extraction | Bevy Render／PBR／`RenderApp` |
| Assets／scene | loader、dependency、handle、hot reload、glTF／scene | Bevy `AssetServer`／Scene |
| Presentation | animation、audio、UI、gizmo | Bevy 與維護中的生態 plugin |
| Observability | log、diagnostics、profiling hooks | Bevy diagnostics／tracing |
| Ecosystem | physics、input mapping、voxel、editor／inspector 等 | 先評估 Bevy-native plugin |

若專案逐項重做這些能力，就不只是「保留核心控制」，而是在維護一套引擎。每項都需要平台測試、性能工程、升級、文件、除錯與生態整合。

## 常見接入模式

### Engine-hosted

引擎／editor 是主要 host，遊戲以 scene、script 或 extension 加入。Godot 常用此模式，並提供 GDExtension、custom MainLoop 與低階 server API；它很適合完整 editor workflow，但會引入另一套 runtime／資料模型。

### Library／framework composition

遊戲 binary 正常依賴引擎 crate，自己建立 App 並選擇 plugin。Bevy 屬這種模式：

```rust
let images = package_kernel.realize(profile_or_lock)?;
let mut app = App::new();
host_adapter.install(&mut app, images)?;
app.run();
```

这不是把游戏“塞进外部主程序”，而是用upstream crates组成自己的Rust application。package kernel只在启动控制平面决定closure；host adapter仍建立正常Bevy App。client、headless、test与tool可选择不同package／standard-plugin profiles，authoritative gameplay仍是同一套。

### Source fork

只有 extension point 無法滿足已證明需求時，才對上游維持最小 patch／fork。fork 仍應保留同步上游、回饋修正和退出路徑；它不是預設接入方式。

## 為何 Bevy 特別適合

- Rust 原生，與預定語言、Cargo build 和 ownership 模型一致。
- App／Plugin 是公開的一級組合模型，不需要自建 plugin host。
- ECS、schedule、fixed time 和 task pool 已形成同一資料流。
- `DefaultPlugins` 提供標準 client；`MinimalPlugins` 和 feature 組合支援 headless／test。
- renderer 已把 main simulation world 與 render world extraction 分開，無需先造 render facade。
- AssetServer／AssetLoader 提供正常 typed asset extension。
- Core host与static package可以直接使用Bevy type；dynamic ABI、存档、网络与stable content ID使用Lattice-owned contract。
- Bevy 生態已有物理、輸入、體素、資產載入與 tooling 候選，可先 spike。

這些特性不是宣稱 Bevy 永遠沒有缺口，而是說「可以完整接入」已成立；缺口必須在接入後由可玩原型逐項證明。

## 責任分界

| Bevy／生態 upstream 擁有 | Lattice Axiom 擁有 |
| --- | --- |
| App、ECS、schedule、time、tasks | 玩家循環與玩法規則 |
| window、input device、render、audio、UI | action 語義和權威 gameplay command |
| Mesh、Material、Image、Scene、AssetServer | stable content ID 與遊戲特有 asset semantics |
| physics solver／query | 哪些物理結果影響玩法、哪些資料需保存 |
| presentation entity／handle | 權威 chunk、stable entity key、snapshot schema |
| generic diagnostics | worldgen／persistence／chunk domain 指標 |
| glTF／標準 loader | 領地、地形、洞穴與 generation provenance |

Package kernel是Lattice Axiom额外拥有的产品控制平面：它决定package／SemVer／capability／realization closure，但不取代表格左侧的Bevy engine能力。

边界依资料寿命、distribution与产品语义划分，不依“第三方型别一律不得出现”划分。Bevy type在core／static runtime内部是正确工具；跨dynamic library、restart／process与长期资料则隔离。

## 「無法接入」必須如何證明

以下都不是證據：

- 未來可能需要換 renderer；
- Bevy 更新較快；
- 沒有內建完整 editor；
- 某項 API 看起來不符合預想架構；
- 建立 facade 感覺更安全。

有效證據必須同時包含：

1. 已能實際遊玩的 Bevy build；
2. 一項可驗收的不可妥協產品需求；
3. 對上游設定、官方 extension point 和活躍 plugin 的實作嘗試；
4. 可重現的功能、性能或平台失敗；
5. 最小替換範圍和維護 owner；
6. upstream issue／PR／fork 與退出策略。

因此目前結論不是「Bevy 一定能做所有事」，而是「沒有證據允許我們在接入以前重做任何通用引擎能力」。

## Godot 的位置

Godot 提供另一種成熟的 engine-hosted／editor-first 模式，適合作為作者工具對照。若 Blender + glTF + Bevy tooling 被真實工作流證明不足，可比較 Godot editor／importer；仍以明確 export contract 把資料交給 Bevy，不同時維護兩個遊戲 runtime。

具體 gate 見[Godot 工具鏈對照](godot-toolchain-comparison.md)。

## 官方來源

- [Bevy repository／features／license](https://github.com/bevyengine/bevy)
- [Bevy `App`](https://docs.rs/bevy/latest/bevy/app/struct.App.html)
- [Bevy `MinimalPlugins`](https://docs.rs/bevy/latest/bevy/prelude/struct.MinimalPlugins.html)
- [Bevy `FixedUpdate`](https://docs.rs/bevy/latest/bevy/prelude/struct.FixedUpdate.html)
- [Bevy render extraction](https://docs.rs/bevy/latest/bevy/render/extract_plugin/index.html)
- [Bevy `AssetServer`](https://docs.rs/bevy/latest/bevy/asset/struct.AssetServer.html)
- [Godot Engine API](https://docs.godotengine.org/en/stable/engine_details/engine_api/index.html)

## 相關文件

- [決策 0014：採用 Bevy 並以上游能力為預設](../decisions/0014-adopt-bevy-upstream-first.md)
- [Bevy 執行期架構](../architecture/game-engine-runtime.md)
- [技術棧](../foundations/technology-stack.md)
- [Bevy 生態調查](renderer-physics-landscape.md)
- [原生外掛與渲染模組教訓](native-plugin-and-render-mod-lessons.md)
