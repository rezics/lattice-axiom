---
title: Package-first／Bevy-first 技術棧與採用邊界
status: active
type: reference
updated: 2026-08-19
---

# Package-first／Bevy-first 技術棧與採用邊界

本页是目前实作技术的单一入口。`accepted` 表示架构方向已采用；精确 crate patch、feature 与 source 仍需由开始实作时的 manifests／lock 固定。`spike candidate` 只允许进入有时限、可玩的评估。

## 採用原則

- Bevy 是游戏引擎，不在项目内建立一层对等 engine API。
- Nickel + Rust package kernel 是游戏组合控制平面，不复制 Bevy runtime。
- static host／realization 可直接使用 Rust／Bevy type；portable dynamic 边界只使用 generated C ABI／stable IDs。
- package SemVer、ABI、`EngineBuildId`、schema 与 artifact hash 分域管理。
- package metadata／Nickel 不进入 tick hot path。
- 先使用 upstream default、公开 extension、生态 plugin 与最小 fork；自研通用替代受[决策 0014](../decisions/0014-adopt-bevy-upstream-first.md)的证据门槛约束。

## 已接受基線

| 能力 | 选择 | 使用方式 | 状态 |
| --- | --- | --- | --- |
| 语言 | Rust stable | core host、package kernel、SDK、static／dynamic modules | accepted |
| 建置 | Cargo workspace、`Cargo.lock` | core／static source build；toolchain／target／features 精确锁定 | accepted |
| 组合语言 | [Nickel](https://nickel-lang.org/) | `package.ncl`／`game.ncl`／overlays／contracts；只在控制平面 | accepted |
| Package version | [SemVer 2.0.0](https://semver.org/) | logical package dependencies；lock 选择精确 version／source／artifact | accepted |
| Package kernel | project Rust crates | source、resolve、capability、lock、plan、build、load、activation | accepted |
| Code generation | Rust proc macro／build tooling | 单一业务声明生成 manifest、static glue、C binding／batch shim | accepted |
| Dynamic native | platform `cdylib` + C ABI | 单一 entry、versioned interface tables、POD batches、opaque handles | accepted |
| 游戏引擎 | [Bevy](https://github.com/bevyengine/bevy) `0.19.x` baseline | client `DefaultPlugins`；headless／test 使用最小标准 profile | accepted |
| ECS／App／schedule | Bevy ECS、App、Plugin、States、Schedules、SystemSet | 唯一 runtime model；dynamic bridge 安装进同一 schedule | accepted |
| 时间／任务 | Bevy Time、`FixedUpdate`、TaskPool | fixed gameplay；jobs 按 instance／revision／budget 治理 | accepted |
| Rendering／window | Bevy Render／PBR／UI／winit／wgpu | Bevy 拥有 device／render schedules；package 使用 feature／provider contract | accepted |
| App settings | Bevy `bevy_settings` | device／user store与ECS resource基础；package scope／authority／world transaction由Lattice contract补充 | accepted |
| Assets | Bevy `AssetServer`、`AssetLoader`、glTF、typed assets | package 拥有 stable identity；Bevy handle 只在 process 内 | accepted |
| 座标 | Bevy 原生右手 Y-up | `+X` 右、`+Y` 上、forward `-Z`；公尺／弧度 | accepted |
| 世界储存 | RocksDB + memory test implementation | 完整已物化 snapshot、atomic batch、schema owner | accepted |
| 序列化 | serde 生态 | typed model／schema envelope；具体 encoding 由 fixture锁定 | accepted |
| 观测／debug draw | Bevy diagnostics overlay、gizmos + `tracing` | package／ABI／world／render structured diagnostics与validated visualizer资料 | accepted |

Bevy `0.19.x` 是 2026-08-19 文件基线，不允许自动漂移。实作 commit 必须固定精确 patch；新的 Bevy release 经过 migration／ABI／save／performance gate 后才能成为基线。

## Package／ABI 產物

| 产物 | 格式／owner | 用途 |
| --- | --- | --- |
| `CompositionSpec` | typed Rust model from Nickel | 使用者组合意图 |
| `LockedGameGraph` | typed model + canonical lock encoding | 精确 package／capability closure |
| `BuildPlan` | typed Rust model | realization／target／artifact work |
| `RegistrationManifest` | SDK-generated versioned data | package schema／system／setting／observability／render declarations |
| `RegistrationImage` | package kernel | closure-wide IDs／schedule／settings／observability／capabilities |
| `RuntimeImage` | loader／host | static functions、dynamic tables、instances |
| C headers／Rust bindings | generated from ABI schema | portable／engine-coupled dynamic boundary |
| `EngineBuildId` | core build metadata | exact engine-coupled compatibility |

这些是分层语义，不以一份 arbitrary JSON dictionary 贯穿所有阶段。

## Realization 技術矩陣

| realization | 技术 | 优势 | 明确成本 |
| --- | --- | --- | --- |
| data | typed assets／definitions | 无 native code、容易组合 | 不能加入任意 behavior |
| `NativeStatic` | Rust source + generated Bevy glue + Cargo／LTO | 完整 Bevy、inline、generic、最佳性能 | 需要重建 host；不分发 `.rlib` ABI |
| `PortableNative` | `cdylib` + stable Lattice C tables | 不随 Bevy内部版本自动重建 | API 较窄、indirect／FFI、trusted code |
| `EngineCoupledNative` | `cdylib` + exact-build internal tables | 预编译部署与较低层能力 | `EngineBuildId` 改变即可能重建 |
| `WasmComponent` | 未选择 runtime／component model | 未来 sandbox／跨平台 | 不在首阶段，需威胁模型与性能证据 |

## ABI 基本型別邊界

可以跨 portable ABI：

- fixed-width scalar；
- SDK-defined C-layout headers／tables；
- pointer + length 的 callback-lifetime views；
- ABI-POD column batches；
- opaque／generational handles；
- versioned serialized payload；
- function pointer + context pointer。

不得跨 portable ABI：

- Rust `String`、`Vec`、slice reference、trait object、closure；
- panic／unwind；
- Bevy `World`、`Entity`、`Handle<T>`、`TypeId`、system parameter；
- wgpu／physics raw handles；
- 需要跨 allocator drop 的任意 Rust value。

## 首個可玩原型候選

| 能力 | 首选候选 | 原型要回答 | 回退顺序 | 状态 |
| --- | --- | --- | --- | --- |
| 体素世界 | [`bevy_voxel_world`](https://github.com/splashdust/bevy_voxel_world) 适配 Bevy 0.19 的 release | streaming、edit、raycast、meshing 能否接 stable ID／revision／package render data | 上游扩充 → 最小权威 adapter → 局部替换 | spike candidate |
| 物理 | [Avian](https://github.com/avianphysics/avian) 对应 Bevy 0.19 的 release | fixed tick、character、voxel collider、raycast 与预算 | 另一维护中 Bevy plugin → domain-specific collision | spike candidate |
| 动作输入 | [Leafwing Input Manager](https://github.com/Leafwing-Studios/leafwing-input-manager) 对应 release | action mapping／test injection 是否比原生 input 省总成本 | Bevy 原生 input | spike candidate |
| loading orchestration | [`bevy_asset_loader`](https://github.com/NiklasEi/bevy_asset_loader) 对应 release | package activation／asset state 是否真的需要额外层 | Bevy `AssetServer` | optional spike |

候选版本必须在 spike 当天重新核对，不凭本页自动加入 Cargo lock。

## 明確上游邊界

- executable 最终运行正常 Bevy App；package bootstrap 不拥有 game loop。
- ECS／schedule／fixed time／tasks 使用 Bevy。
- renderer／window／GPU resource 使用 Bevy；dynamic command 由 host 翻译。
- assets runtime graph 使用 Bevy；package lock管理 package ownership／artifact closure。
- physics／input／voxel 先采用维护中的 Bevy生态。
- Nickel evaluator 只构造 `CompositionSpec`；副作用与 runtime 由 Rust。
- platform loader 只负责打开已验证 artifact；所有 ABI／lifecycle policy 由 Lattice loader。

## Bevy Type 使用邊界

直接使用 Bevy type：

- core host 与 `NativeStatic` realization；
- ECS components／resources／systems；
- Transform、Mesh、Material、Image、Animation、Audio；
- render extraction／`RenderApp` static extension；
- diagnostics／development tools。
- Bevy app settings、UI／Feathers与input focus实现。

转换成 Lattice stable contract：

- dynamic native ABI；
- RocksDB records／backup；
- future network protocol；
- generation provenance；
- cross-restart content／asset references；
- package manifests／lock；
- 需要独立 migration 承诺的公开 formats。

这条边界按 lifetime／distribution contract 划分，不是假装 Bevy 可替换。

## 供應鏈與信任

- core／static source build：审查 source、build script、Cargo dependencies 与 license；
- dynamic native：验证 descriptor／hash／target／ABI，但仍标记 trusted process code；
- Nickel：受控 imports、evaluation limits、无隐式网络／host access；
- lock／artifact：记录 source、producer、toolchain、hash 与 future signature slot；
- public signature／registry policy 延后到有分发 consumer 时，但 data model预留明确 owner，不实现假安全。

## Godot 的位置

Godot 只作未来 authoring／import toolchain 对照，不作 runtime、renderer、package manager 或权威 scene format。触发 gate 见[Godot 工具链对照](../research/godot-toolchain-comparison.md)。

## 相關文件

- [套件內核](../architecture/package-management.md)
- [原生 ABI](../architecture/native-module-abi.md)
- [Bevy 執行期](../architecture/game-engine-runtime.md)
- [版本與相容性](../architecture/versioning-and-compatibility.md)
- [設定與配置](../architecture/settings-and-configuration.md)
- [診斷、檢查與除錯可視化](../architecture/diagnostics-inspection-and-debug-visualization.md)
- [World 生命週期與開始頁](../architecture/world-lifecycle-and-start-ui.md)
- [外掛／渲染模組調查](../research/native-plugin-and-render-mod-lessons.md)
