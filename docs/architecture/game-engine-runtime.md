---
title: 套件驅動的 Bevy 執行期架構
status: proposed
type: architecture
updated: 2026-08-20
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0015-bevy-native-y-up-world-coordinates.md
  - ../decisions/0017-versioned-native-module-abi.md
  - ../decisions/0018-package-kernel-from-first-vertical-slice.md
  - ../decisions/0020-semantic-registration-and-content-selection.md
  - ../decisions/0023-freeze-sdk-registration-and-semantic-compilation.md
  - ../decisions/0024-freeze-portable-native-abi-0x.md
  - ../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
  - ../decisions/0026-freeze-first-demo-performance-budgets.md
  - ../decisions/0027-freeze-authoritative-world-and-persistence-contract.md
---

# 套件驅動的 Bevy 執行期架構

## 結論

Lattice Axiom runtime 是一个由 package closure 建立的正常 Bevy App：

- client先由`ClientShellGraph`建立package-driven开始页／settings／world catalog，再为所选world建立游戏closure；
- package kernel 在 App 进入玩法前完成 compose、resolve、lock、realization、registration 与 artifact validation；
- Bevy 拥有程序 runner、ECS、schedules、time、tasks、input、assets、window、render 与 diagnostics；
- host adapter 把 `RegistrationImage`／`RuntimeImage` 安装为 Bevy plugins、compiled SemanticCatalog、typed static systems 与 dynamic bridge systems；
- 权威世界、世界生成、持久化、内容身分与 ABI 是 Lattice Axiom 的产品语义。

package-first 与 Bevy upstream-first 不冲突。前者决定游戏闭包，后者避免重做游戏引擎。

## 啟動架構

```text
executable
└── Lattice bootstrap (bounded control plane)
    ├── evaluate game.ncl / read latticeaxiom.lock
    ├── resolve sources / SemVer / capabilities
    ├── select static / dynamic realizations
    ├── validate manifests / semantics / artifacts / ABI
    └── produce RegistrationImage + RuntimeImage
         └── EngineInstance
             └── Bevy App
                 ├── DefaultPlugins or headless standard profile
                 ├── selected ecosystem plugins
                 ├── static host/domain adapters
                 ├── dynamic bridge systems
                 └── package content / assets / render features
```

bootstrap 是短命的 control plane，不是第二套 game loop。进入 `Playing` 后，Nickel evaluator、resolver 与 build planner 不参与每 tick；仅保留 lock／package diagnostics 与 module instance lifecycle 所需的窄状态。

client shell本身也是独立locked package closure与EngineInstance。它只读取`WorldHeader`与package metadata；world选择后先产生不执行module business code的`WorldOpenPlan`，再完成settings flush与durability barrier、stop package instances、join有界tasks并关闭writer。只有这些步骤成功后才atomic写入`LaunchIntentV1`并退出；barrier失败不得发布intent。client v1由replacement process建立唯一fresh game `DefaultPlugins` App；返回开始页执行相反流程。这样遵守winit每个application只创建一个`EventLoop`的跨平台约束，也隔离mapped native library、window／GPU与process-global static state。headless／`MinimalPlugins`测试仍可在同一process建立多个EngineInstance；bootstrap只协调进程间顺序生命周期，不是第二个game runner。

## 唯一啟動真相

client、headless、server、test 与 tool 都以 profile 进入 package graph：

```text
client.ncl    → DefaultPlugins + presentation packages
headless.ncl  → MinimalPlugins + authoritative packages
test.ncl      → in-memory sources + selected fixtures
tool.ncl      → MinimalPlugins + tooling package closure
```

client profile另外锁定`@latticeaxiom/front-end`、settings、world-library与observability等shell packages；它们不是每个world的authoritative closure，也不能绕过world frozen lock。

profile 可以选择不同的 presentation／tooling realization，但以下条件必须成立：

- 相同 world 的权威 package／schema closure 可协商；
- `RegistrationImage` 清楚区分 client-only、server-only 与 authoritative items；
- 没有另一个手写 `LatticeAxiomPluginGroup` 清单绕过 lock；
- core host 的 Bevy standard plugins 是 package profile 的已知 adapter 结果，不是假装成可由任意模组替换的 package。

## Core Host 責任

| Core host 拥有 | Package／module 拥有 |
| --- | --- |
| Bevy App／runner／standard plugin profile | package identity、SemVer、dependency |
| host capability interfaces 与 ABI loader | required／provided capabilities |
| registration merge、numeric IDs、schedule compilation | manifest 中的 stable IDs／schemas／systems |
| SemanticCatalog／Role／fallback compilation | Nickel-authored Tag／Map／Predicate／Role／bundle intent |
| settings／observability catalog compilation与surface adapter | package-authored SettingSpec／info／metric／inspect／visualizer declarations |
| dynamic batch bridge／command validation | business callbacks／data transforms |
| EngineInstance lifecycle 与 shutdown barrier | per-instance create／start／stop state |
| renderer／asset／task service adapters | semantic render／asset／task requests |
| world activation transaction | package-owned migration／missing content policy |

core host 不拥有第一方内容或 Terrenia 维度的私有注册表。第一方 package 与测试 package 均经同一 registration image。`terrenia` 是普通可替换模组；core host也不为它预设platform role binding、fallback或`terrenia:*` semantic contract。

## EngineInstance

一个 `EngineInstance` 绑定：

- `LockedGameGraph`／lock hash；
- `RegistrationImage`／manifest set hash；
- `RuntimeImage`／artifact handles；
- Bevy App／World；
- dynamic module instance contexts；
- optional world／save identity（shell为none）与 lifecycle state；
- cancellation／task／diagnostic roots。

禁止 module 用 process global 暗中绑定某个 App。测试可在同一 process 依序或并行创建多个 EngineInstance；所有 static resource／dynamic context 必须因此暴露错误的全域假设。

## App 建立交易

### Phase A：不執行 module code

1. 读取／产生 lock。
2. 选择 realization 与 artifacts。
3. 读取所有 registration manifests。
4. 验证 package／capability／schema／ID／schedule／settings／observability／render graph。
5. 编译Tag／Map／State／Affordance／Predicate／Role、SettingsCatalog与observability subscription plan，解析声明式fallback并建立`RegistrationImage`。
6. 对已选择world验证header／frozen lock／schema／setting／checkpoint并产生`WorldOpenPlan`；尚未选择world的shell跳过本步。

### Phase B：載入但不開放 world

7. 准备 static function map；加载 dynamic libraries 并协商 descriptor／interfaces。
8. 比对 callback map／manifest hash，产生 `RuntimeImage`。
9. 建立 Bevy App，安装 standard／ecosystem plugins 与 host adapters。
10. 建立 module instances，注册 static systems／dynamic bridges。

### Phase C：啟用

11. 依 graph 顺序启动 modules，但尚不开放writable gameplay。
12. 依已审核`WorldOpenPlan`在checkpoint／staging边界执行migration，载入assets／world并再次验证content。
13. 原子发布world activation后才进入`Playing`。

任一阶段失败都逆序释放已建立 instance，不让 world 进入 writable gameplay。启动不是「尽量加载能用的模组」；权威 closure 必须原子成立。

## Static 與 Dynamic 安裝

### Static

static glue 直接调用 `App::add_systems`、注册 Bevy component／resource／asset／message／render extension，并以生成的 stable↔Bevy type map 对齐 registration image。它可以使用完整 typed Bevy API。

### Dynamic

host 为每个 dynamic system 生成／选择 bridge system：

1. Bevy scheduler 在 manifest stage 调用 bridge；
2. bridge 根据已编译 query／read-write set 形成 archetype batches；
3. 一次 C callback 传递多个 column views 与 command sink；
4. callback 返回后验证 command／message／render output；
5. 在 schedule barrier 套用 structural mutation。

dynamic module 永远不取得 `&mut World`。因此 dynamic safety／compatibility boundary 不会迫使 static path放弃 Bevy system optimization。

## Schedule 與時間

Bevy `FixedUpdate` 推进权威玩法，`Update`／`PostUpdate` 负责 frame-rate presentation。package manifest 对齐 Lattice versioned semantic stages；host 把它们映射到 Bevy `SystemSet`：

```text
PreUpdate
└── input.sample

FixedUpdate
├── gameplay.fixed.pre
├── gameplay.fixed
├── physics.integrate
├── world.commands.apply
├── world.revision.commit
├── derived-work.queue
└── persistence.capture

Update / PostUpdate
├── async-results.observe
├── presentation.update
└── Bevy transform / visibility / render extraction
```

规则：

- package 只声明 correctness edge，不以 discovery／load order 排序；
- package kernel 在启动前拒绝 cycle／unknown stage／exclusive provider conflict；
- static system 的真实 access 由 Bevy 验证，dynamic system 的 read／write set 由 manifest + bridge 验证；
- 普通无冲突 system 仍由 Bevy scheduler 并行；
- fixed tick rate 是 profile／world contract，不能用 render delta 推进权威逻辑。

## 權威世界與 Bevy ECS

### 區塊不是 Entity 集合

体素区块以紧凑 domain data 保存：palette、voxel storage、metadata、revision、dirty／durability state。一个 voxel 不对应一个 Bevy Entity。Bevy ECS 用于 active gameplay entities、粗粒度 chunk presentation／collision owner、task／request 与跨系统工作集。

### 身分層次

| 身分 | 生命周期 | 可否持久化／跨 ABI |
| --- | --- | --- |
| stable content／component ID | package contract | 可以 |
| semantic contract ID／Role binding | registration／world lock | contract与binding可持久化；Tag／Role不能代替concrete voxel ID |
| stable entity key | world／schema contract | 可以 |
| chunk-local palette index | snapshot-local palette | 可以，但每个palette entry保存stable ID／schema／state；不是closure numeric ID |
| Bevy `Entity`／`Handle`／`TypeId` | 当前 App | 不可以 |
| ABI generational handle | EngineInstance／interface | 不可以 |
| GPU／physics handle | 当前 backend instance | 不可以 |

### 權威與衍生

| 类别 | 示例 | 可否丢弃重建 |
| --- | --- | --- |
| 权威 | voxel、player edit、persistent entity、scheduled work、必要 RNG | 否 |
| provenance | generator revision、config hash、package／schema owner | 否 |
| runtime working set | ECS entities、loaded chunk arena、module instance | 可由 snapshot／lock 恢复 |
| derived | mesh、collider cache、visibility、GPU asset、navigation | 是 |

renderer／module callback／物理失败不得反向篡改已确认权威 revision。derived result 只在 source epoch／revision 匹配时套用。

## 非同步工作

使用 Bevy task pools；package 通过 static Bevy API 或 dynamic `tasks` interface 提交 host-owned job。每个 job 至少携带：

- EngineInstance／world／dimension；
- chunk `(x, y, z)`；
- world epoch／chunk revision；
- job kind／source fingerprint；
- package／callback owner；
- cancellation token 与资源预算。

背景 job 只接收 immutable input 并产生 immutable result／command。回主 World 后重新验证 epoch／revision。dynamic foreign thread 不直接修改 World；instance stop 会取消任务并等待有界 quiescence。

queue depth、concurrency、in-flight bytes、CPU time 与 per-frame apply cost 都有上限。package capability 可以请求工作，不可建立无法治理的隐式全域 thread pool。

## Input 與 Gameplay Action

Bevy input plugin 收集平台事件。Gameplay 消费具名 action：`Move`、`Look`、`Jump`、`BreakBlock`、`PlaceBlock`。static package 可读 typed action resource；dynamic package 经稳定 action/message batch interface 取得同一语义资料，不接触 platform scan code。

demo 只需 test injection；replay／network action frame 在出现真实需求后独立版本化，不先把 input API 假装成网络协议。

## Physics

优先采用 Avian 等 Bevy-native physics plugin。core host／static domain adapter 可直接使用其 component／query；dynamic gameplay 通过 Lattice-owned physics／spatial capability 或稳定 gameplay component 取得必要结果，而不暴露 solver Rust layout。

持久化只保存项目语义 DTO，不保存 broadphase／solver handle。若体素碰撞有性能缺口，先限制在 collider generation／query adapter，再通过决策 0014 的上游例外门槛。

## Assets

Bevy `AssetServer`／`AssetLoader` 管理 runtime dependency、handle 与 hot reload。package lock／manifest 管理 package asset ownership、stable asset ID、source／artifact hash 与 profile availability。

```text
package asset reference
  → stable asset ID / package-relative source
  → RegistrationImage ownership
  → host asset adapter
  → Bevy AssetServer / typed Handle (process-local)
```

dynamic module 只经 `assets` interface 使用 opaque generational handle，不持有 Bevy `Handle<T>` layout。存档只保存 stable asset／content reference。

## Rendering

Bevy renderer 仍是唯一 renderer。package 注册 `RenderData`、`RenderFeature`、`RenderPass` 或 `RenderProvider` contract；host 在 activation 时验证 semantic slots／resource access并映射到 Bevy render schedules。

- static render package 可直接使用 Material、shader、`RenderApp` 与公开 Bevy extension；
- portable dynamic package 产出验证过的 compact render command／buffer，不取得 raw device；
- engine-coupled dynamic package 只经 exact-build C tables；
- 必须操作任意 Bevy RenderWorld 的 package 选择 static。

详见[渲染架构](rendering.md)。

## Settings、Inspect 與 Diagnostics

Bevy app settings、diagnostics、UI与gizmos是上游机制；package通过`RegistrationManifest`贡献
typed schema／sources：

- `@latticeaxiom/settings`解析device／user／world／session scope并以transaction apply；
- `@latticeaxiom/inspect`组合准星目标的name／summary／detail fragments；
- `@latticeaxiom/dev-tools`按subscription显示F3类metrics与chunk／physics／render visualizers；
- headless保留相同authoritative settings与metric IDs，以CLI／report取代UI／GPU；
- hidden panel不会继续执行专用昂贵query，visualizer failure不改变world。

详见[设置架构](settings-and-configuration.md)与[诊断／检查架构](diagnostics-inspection-and-debug-visualization.md)。

## Persistence Barrier

`persistence.capture` 从一个已提交权威 revision 建立 immutable snapshot envelope，再交给 I/O task。完成结果只确认它实际写入／耐久的 revision；较旧 completion 不能清除较新 dirty state。

world metadata 同时保存 required package／content／schema closure 与 diagnostic lock fingerprint。存档不包含 Bevy ID、ABI handle、callback key、function pointer 或 physics／GPU handle。

## Shutdown

```text
Playing
  → stop accepting authoritative commands
  → module quiesce / cancel tasks
  → capture final persistence revisions
  → bounded I/O drain / checkpoint
  → stop dynamic and static module instances in reverse graph order
  → destroy EngineInstance resources
  → AppExit
```

若 deadline 到期，UI／log 明确报告未 durable revision。Drop 顺序不是保存协议。v1 dynamic library 继续保持 mapped 到 process 结束，不在 shutdown 中尝试危险卸载。

## Bevy 升級

1. 锁定明确 release、Rust toolchain 与 ecosystem matrix。
2. 单独迁移 core host／static adapters，不夹带 gameplay 重构。
3. 生成新 `EngineBuildId`，重建 static／engine-coupled artifacts。
4. portable old-binary fixtures不重编，验证 Lattice ABI／behavior contract。
5. 执行 client／headless、package lock、registration、save、asset、render 与性能回归。
6. 只有外部 Lattice contract 真正破坏时，提升相应 package／capability major。

## 禁止的鏡像抽象與捷徑

- 自有 App／World／Entity／Component facade；
- 自有 ECS scheduler／fixed-time runtime；
- 自有 renderer／asset dependency graph／platform input layer；
- dynamic module 直接取得 Bevy／Rust type；
- 第一方 package 或 Terrenia 维度绕过 `LockedGameGraph`；
- host写死`terrenia:*` content、semantic binding或fallback；
- tick 内求值 Nickel／解析 SemVer；
- 一个 global `EngineVersion` 代理所有相容性；
- v1 native hot unload。

## 驗收

- client／headless／test 都由 package profile 建立同一权威 closure。
- Bevy App 是唯一 game loop／ECS／scheduler；package kernel 在 `Playing` 前结束组合工作。
- static system 直接进入 Bevy，dynamic system 以每 system／batch bridge 进入相同 semantic stage。
- 多 EngineInstance 测试不泄漏 global module／world state。
- 随机化 background completion／package load 不改变权威 revision、ID 或 schedule。
- Tag／Map hot query使用compiled table，Role在world command前成为concrete ID，tick不执行Nickel或package lookup。
- 非Terrenia测试维度可替换整个`terrenia` closure而无需修改host。
- portable artifact 跨 Bevy 升级按 ABI contract 工作，engine-coupled artifact精确要求重建。
- profiler 能区分 static systems、dynamic bridge overhead、module task、chunk／persistence 与 render costs。

## 相關文件

- [套件內核](package-management.md)
- [模組組合](module-composition.md)
- [語義註冊、內容判定與選擇](semantic-registration.md)
- [原生模組 ABI](native-module-abi.md)
- [渲染架構](rendering.md)
- [世界持久化](world-persistence.md)
- [設定與配置](settings-and-configuration.md)
- [診斷、檢查與除錯可視化](diagnostics-inspection-and-debug-visualization.md)
- [World 生命週期與開始頁](world-lifecycle-and-start-ui.md)
- [執行期路線圖](../planning/roadmap-game-engine.md)
