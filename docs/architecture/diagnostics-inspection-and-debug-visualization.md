---
title: Package 可組合的診斷、檢查與除錯可視化
status: proposed
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0015-bevy-native-y-up-world-coordinates.md
  - ../decisions/0017-versioned-native-module-abi.md
---

# Package 可組合的診斷、檢查與除錯可視化

## 結論

Lattice Axiom提供一个统一observability model、两个玩家surface：

- `@latticeaxiom/inspect` 是普通client package，显示准星目标的名称与少量可理解信息；
- `@latticeaxiom/dev-tools` 是dev／opt-in client package，提供F3类诊断workbench、指标图表、
  chunk／collision／render visualizer与可复制报告。

任一package都可以注册结构化`InfoItem`、`DiagnosticMetric`、`InspectFragmentProvider`或
`DebugVisualizer`，但不能直接占一个HUD角落或覆盖其他package。host采用Bevy diagnostics、
gizmos、UI与上游physics debug data；Lattice只负责编排、权限、成本和stable contract。

## 三個面不可混合

| Surface | 默认受众 | 典型内容 | 默认状态 |
| --- | --- | --- | --- |
| Pinned HUD | 玩家 | FPS、坐标、方向、save状态等少量favorite | 只显示用户明确pin的项 |
| Target Inspect | 玩家 | 方块／流体／实体名称、icon、可交互摘要 | client profile启用，compact |
| Diagnostic Workbench | 作者／技术玩家 | graph、charts、packages、queues、visualizers | dev profile或明确开启 |

打开workbench不应把inspect panel复制一遍；同一个item可以被引用到不同layout preset，但只有
一份数据source和permission判定。

## 基礎 Packages 與 Capabilities

| Package | Provides | 责任 |
| --- | --- | --- |
| `@latticeaxiom/observability` | `diagnostic-registry@1` exactly-one | item／metric registry、subscription、budget、report |
| `@latticeaxiom/inspect` | `target-inspect-surface@1` exactly-one client | target selection与结构化tooltip布局 |
| `@latticeaxiom/dev-tools` | `debug-workbench@1` zero-or-one client | F3类panel、graphs、world-space visualizers |

headless保留observability registry、日志与report export，不安装UI／gizmo。默认profile显式选择
这些package；host不保留hand-written第一方overlay list。

## 註冊模型

### `InfoItemSpec`

适合文字／数值：

- stable item ID、owner package、label／unit／format；
- value schema与unavailable／stale／error state；
- `basic`／`detail`／`technical` disclosure level；
- default anchor／group仅是建议，user layout具有最终决定权；
- update policy：on-change、target-change、fixed interval、manual；
- estimated CPU／allocation／I/O cost class；
- authority／privacy／server policy；
- static typed source或portable dynamic batch callback。

Package返回typed value，不返回任意rich text或绝对screen coordinates。host负责localization、
format、wrapping、contrast、screen narration与overflow。

### `DiagnosticMetricSpec`

metric映射到Bevy `DiagnosticPath`或project counter／histogram：

- stable metric ID、unit、aggregation（latest／mean／P50／P95／max／rate）；
- sampling interval、history length上限、cost；
- warning／critical threshold是profile policy，不由color单独表达；
- authoritative metric与presentation metric分域；metric值不进入world state。

### `InspectFragmentProviderSpec`

provider声明target kinds、required semantic data、priority relation与fragment keys，例如：

```text
header.name
header.icon
summary.harvestability
summary.fluid
detail.block-state
detail.container-fill
technical.stable-id
technical.declared-by
```

同一个key只有一个primary owner，其他package须通过明确extension slot追加；不能靠priority数字
覆盖别人的名称。graph compiler检查key conflict与before／after relation。

### `DebugVisualizerSpec`

world-space可视化声明：

- stable visualizer ID、data source、coordinate space与Y-up单位；
- primitive kinds、legend entries、depth mode、pick support；
- radius／max primitive／CPU／GPU upload／history budget；
- required capability与headless fallback（report-only／unavailable）；
-权限：local dev、world owner、server admin或unsafe developer build。

visualizer只产生host验证的line／box／text／mesh-instance command，不取得raw render pass。
需要特殊GPU机制时另走`RenderFeature`／`RenderPass` contract。

## Subscription-Driven 收集

```text
registered source
      ↓ visible preset / pinned item / report request
subscription plan
      ↓ cost + permission + rate validation
Bevy diagnostics / ECS query / dynamic batch / async sample
      ↓ typed sample + timestamp + freshness
layout / graph / export
```

- 未订阅item不运行专用system；共享基础counter可继续由upstream低成本采集；
- layout关闭、world离开或permission撤销时取消subscription；
- expensive source按target change或限频，不能每render frame做server round-trip／chunk scan；
- async sample携带EngineInstance／world epoch／target key；stale结果丢弃；
- budget超限时降低rate／radius或truncate，并在surface显示“部分资料”与实际上限；
- 查看FPS的preset本身必须有可测overhead budget。

## F3 類 Diagnostic Workbench

首批preset：

| Preset | 默认内容 |
| --- | --- |
| `Play` | 坐标、方向、target name、durability状态 |
| `Performance` | FPS／frame time、fixed tick、CPU／GPU、memory、queue P95 |
| `World` | dimension、chunk coordinate／revision、resident／dirty／durable、worldgen provenance |
| `Rendering` | visible chunks、triangles、mesh queue、LOD／occlusion／provider、GPU budgets |
| `Physics` | fixed step、bodies、contacts、collider queue、query与visualizer toggles |
| `Packages` | lock hash、registration hash、realizations、ABI callbacks／bytes／errors |

布局规则：

- preset是可复制的默认配置，不冻结用户layout；
- item可pin到HUD、放进left／right column或workbench panel；不因文本宽度自动换边；
- text scale、line spacing、background opacity与graph scale独立于普通UI；
- narrow window使用scroll／tabs／collapse，不让行消失到viewport外；
- search／command palette可直接找到item和visualizer；快捷键不是唯一入口；
- help与快捷键放进可搜索页面，不常驻覆盖画面中心。

## Target Inspect

### Target pipeline

1. authoritative gameplay raycast产生`TargetKey`、hit position／face与可公开基础状态；
2. local presentation provider立即产生名称／icon fallback；
3. server-authoritative provider在permission与rate内返回typed fragments；
4. host按disclosure level、layout budget和stable order组合；
5. target改变即取消旧请求；相同target按revision cache。

默认compact只显示：localized display name、icon／kind、一个最重要交互状态。按住“详细检查”或
pin后才显示state、工具／硬度、流体、package来源；stable ID、numeric revision与raw predicate
只在technical层。

### 資訊權限

- client从已公开render data能知道的名称仍受world gameplay policy控制；
- inventory contents、machine internals、AI state等必须由server provider批准；
- provider可回`hidden-by-world-policy`、`requires-tool`、`requires-progress`，UI显示可理解原因；
- server不安装对应provider时显示“资料不可用”，不伪造零值；
- tooltip不能成为绕过anti-cheat／fog-of-war／discovery机制的通道。

## Chunk Visualizer

首阶段至少区分：

| Layer | 显示 |
| --- | --- |
| Grid | chunk／section边界、coordinate与local origin |
| Lifecycle | requested、materialized、resident、active、evicting |
| Persistence | clean、dirty、queued、written、durable、checkpointed |
| Mesh | source revision、queued／building／ready／stale-rejected、triangle count |
| Collision | occupancy revision、collider queued／ready／stale-rejected |
| Visibility | frustum、occluded、LOD、provider、last-visible frame |
| Worldgen | provider／revision／config hash、generation epoch／boundary |

颜色之外同时使用line style、label／legend与pick inspector。选择chunk显示revision chain、job age、
queue position、memory bytes与最近rejection reason；不把所有状态压成红／绿边框。

## Collision 與 Query Visualizer

“碰撞箱”必须拆成：

- block selection shape；
- authoritative solid／fluid occupancy；
- generated narrow-phase collider；
- broadphase AABB／tree node；
- trigger／sensor；
- character controller shape与step／slope probes；
- raycast／shapecast path、hit point／normal；
- contact／penetration／impulse；
- render bounds／occlusion bounds（明确不属于physics）。

每类有固定legend语义、filter、depth-tested／x-ray选项、view distance和primitive上限。默认只看
nearby selected layer；“show all”必须先显示成本警告且仍受hard budget。

## Package 與 Dynamic ABI

static source直接写Bevy diagnostics或generated typed sources；portable dynamic使用批量接口：

- host一次callback传入已订阅item keys和sample context；
- module返回bounded POD samples／visual commands；
- 不逐item、逐entity或逐voxel跨FFI；
- callback timing、bytes、truncation与invalid command本身进入package diagnostics；
- package panic／invalid output只禁用对应source并保留workbench错误，不改变world。

## 報告與可重現性

`Copy Diagnostic Report`／CLI export至少包含：

- app／platform、exact lock／registration／engine build fingerprints；
-启用preset与明确同意的metrics snapshot／history摘要；
- world ID默认redact，seed、路径、server address与player identity默认不收集；
- selected chunk／target coordinate需要显式勾选；
- active settings只列非secret、与问题相关的diff；
-预算截断与unavailable source。

报告先显示preview再复制／写出，不自动上传。

## 首個 Demo Consumer

- `@latticeaxiom/inspect`显示target block display name、icon、stable ID（technical）与owner package；
- Performance preset显示FPS／frame time／fixed tick／memory／mesh与collider queue；
- Chunk visualizer实现Grid、Lifecycle、Persistence、Mesh、Collision五层；
- Physics visualizer区分selection shape、collider、AABB与break／place raycast；
- `@example/dual-gameplay`从static／dynamic两条路径贡献一个metric与一个inspect fragment；
- headless导出相同权威metric names，不建立UI／GPU。

## 驗收

- 默认玩家画面没有技术洪流；新测试者能在设置页找到inspect与workbench入口。
- 只pin FPS时不采集target tags、chunk scan或physics contacts；overlay overhead有量测。
- 800×600与高UI scale下无item画出viewport；column不会因内容变化跳边。
- 两个package争用同一fragment key在activation前失败，不产生重复overlay。
- server隐藏资料时client无法从inspect API取得，UI能区分隐藏与故障。
- chunk／collision visualizer legend、filter、radius、budget与pick inspector可用。
- static／dynamic source产生相同typed sample；dynamic callback不逐entity往返。
- debug visualizer启停、截断、invalid command、device loss都不改变authoritative world hash。
- screen narration能读出item label／value／stale／warning，视觉状态不只靠颜色。

## 相關文件

- [渲染能力、Pass 與 Provider](rendering.md)
- [套件驅動的 Bevy runtime](game-engine-runtime.md)
- [Package 設定與配置](settings-and-configuration.md)
- [外部調查](../research/debug-settings-and-world-ux-lessons.md)
