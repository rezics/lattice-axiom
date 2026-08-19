---
title: Bevy 渲染能力、Pass 與 Provider 組合
status: proposed
type: architecture
updated: 2026-08-20
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0015-bevy-native-y-up-world-coordinates.md
  - ../decisions/0017-versioned-native-module-abi.md
---

# Bevy 渲染能力、Pass 與 Provider 組合

## 結論

Lattice Axiom 使用 Bevy renderer，并让 package 以 versioned render capability 扩充资料、shader、pass 与特定机制。普通模组不依赖 Bevy内部 Rust layout；package kernel 先把声明式 slot、resource、ordering、GPU requirement 与 provider conflict 编译为 `RegistrationImage`，host 再映射到 Bevy render-world ECS schedules。

这同时满足两种需要：

- **上游优先**：window、surface、device、asset、extraction、pipeline cache、render schedules 与大部分 PBR 由 Bevy 提供；
- **可扩充机制**：模组可以加入 post effect、特殊材质、compute、visibility／terrain pipeline，甚至替换一个明确 provider，而不是靠 patch／Mixin 改写任意 renderer 内部。

## 不變邊界

- Bevy renderer 是唯一 runtime renderer；不建立平行 `latticeaxiom-render` backend。
- 权威 world／gameplay 不直接呼叫 GPU，也不依赖 render success。
- Mesh、Material、Image、buffer、visibility 与 render entity 都是可重建 presentation。
- render package 必须声明 capability 与资源契约；不能按 load order 注入任意 code。
- static、portable dynamic 与 engine-coupled realization 可以有不同调用机制，但共用同一个 `RegistrationManifest`／render semantic contract。

## 預設資料流

```text
authoritative chunk / gameplay state
                 ↓ revision-checked derived work
       presentation / RenderData components
                 ↓ Bevy ExtractSchedule
              render world
                 ↓ registered feature / pass systems
       prepare → queue → render → cleanup
                 ↓
          Bevy renderer / wgpu
```

Bevy 0.19 已将许多 render graph 工作转为 ECS systems／schedules；Lattice 的 render contract 应编译到这些公开语义位置，不复制另一张通用 render graph。

## 四級擴充模型

### Level 0：`RenderData`

提供 renderer 已理解的资料，不改变 pipeline 机制：

- Mesh／Material／Transform／Visibility；
- voxel chunk presentation descriptor；
- light、camera、particle emitter、animation／audio presentation state；
- debug gizmo／overlay data。

这是普通 content／gameplay package 的默认。dynamic package 可以注册 ABI-POD presentation component 或通过 command buffer 提交 stable render-data command；host 转成 Bevy assets／components。

### Level 1：`RenderFeature`

增加可组合 shader／compute／post-processing 功能：

- Material extension；
- outline、fog、color grading、screen-space effect；
- particle simulation、procedural texture、specialized pipeline；
- extraction／prepare／queue systems。

feature 声明 input／output semantic slots、resource access、format／sample constraints、GPU features、ordering 与 fallback。多个 feature 默认可以共存，但 graph compiler 必须证明资源与顺序有效。

### Level 2：`RenderPass`

建立新的明确 pass／phase 或插入已知 slot：

- `after.opaque`、`before.tonemap`、`after.tonemap`；
- shadow／depth／visibility consumer；
- compute-to-indirect-draw；
- portal／reflection／custom transparency phase。

package 不引用「某个 Bevy私有 node index」。它依赖 versioned semantic slot；host adapter 负责映射当前 Bevy schedule／set。slot 不存在且无 fallback 时，closure 在启动前失败。

### Level 3：`RenderProvider`

替换一项 exclusive 机制，而不是整个 renderer：

- terrain mesh source；
- chunk visibility／occlusion；
- terrain draw backend；
- shadow provider；
- upscaler／anti-alias provider。

provider capability 通常是 `exactly-one`。两个套件同时提供同一 exclusive capability，resolver 必须要求 profile 明确选择，不能 last-loaded-wins。provider 可以依赖其他 stable capabilities，例如 terrain draw backend 消费 visibility provider 与 material table。

## Capability 與 Cardinality

候选命名：

```text
render.data.voxel-chunk@1          multi
render.material.block@1            multi
render.post-effect@1               multi
render.semantic-slot.core3d@1      singleton host capability
render.terrain.mesh-source@1       exactly-one
render.visibility@1                exactly-one
render.terrain.backend@1           exactly-one
render.debug-overlay@1             multi, dev profile
```

每项 declaration 至少含：

- required／provided capability range；
- cardinality 与 profile condition；
- inputs／outputs semantic IDs；
- resource read／write／create；
- stage／slot／before／after；
- GPU feature／limit／format requirement；
- fallback／degraded mode；
- static／portable／engine-coupled realization support。

## Render Graph 編譯

package kernel 在不加载 code 的情况下：

1. 选择 exclusive providers；
2. 验证 capability／slot version；
3. 建立 semantic resource graph；
4. 检测 writer conflict、cycle、format／sample mismatch；
5. 验证 GPU requirement 或选择 fallback；
6. 产生 deterministic render plan／numeric handles；
7. 把 plan 写入 `RegistrationImage`。

host adapter 再映射到 Bevy：

| Lattice semantic | Bevy host mapping（当前基线） |
| --- | --- |
| main-world presentation data | Bevy components／assets |
| extraction | `ExtractSchedule`／extract systems |
| render-world prepare／queue | Bevy `Render` schedule sets |
| Core 3D slots | `Core3d` 相关公开 sets／resources |
| shader／pipeline | Bevy shader assets、pipeline cache、specialization |
| GPU resource | Bevy／wgpu-owned opaque handle |

映射是 core host implementation，可以随 Bevy 升级而改变；Lattice semantic contract 若保持不变，portable package 不需因此重编。

## Static Render Package

`NativeStatic` 可以直接：

- 注册 `Material`／`ExtendedMaterial`；
- 添加 Bevy extraction／render systems；
- 使用 shader asset、pipeline specialization 与公开 render resources；
- 在证据门槛下使用更低层 Bevy／wgpu extension。

它仍要输出 manifest，声明 capability、slot、resource access 与 fallback；完整 Rust 能力不等于可绕过 graph 冲突诊断。若实现需要任意 `RenderApp`／Rust generic，这就是正确的 realization。

## Portable Dynamic Render Package

portable dynamic module 不接收 `RenderDevice`、`RenderQueue`、wgpu handle 或 Bevy RenderWorld。它可经 ABI：

- 注册 stable RenderData／buffer schema；
- 处理 extraction 后的 ABI-POD batches；
- 产生 compact `RenderCommandList`；
- 填充 host-provided upload／storage slices；
- 选择已注册 shader／pipeline／resource opaque handles；
- 发布 feature-local diagnostics／timing markers。

command list 是受限指令，不是重新发明完整图形 API。候选指令仅覆盖真实 feature 所需的 bind semantic resource、dispatch、draw indirect、copy／barrier request；host 验证：

- handle generation／owner／usage；
- offset／size／alignment／bounds；
- pass 允许的 pipeline／resource；
- feature capability 与 GPU limits；
- command count／upload bytes／CPU time budget。

host 翻译成 Bevy／wgpu work，并保留实际 resource state 所有权。若需求无法在窄 command contract 中表达，先评估 static；不能无限扩张 portable API 直到等同 raw wgpu。

## Engine-Coupled Render Package

`EngineCoupledNative` 用于确实需要预编译动态部署、又必须接近当前 renderer internals 的可信代码。它：

- 仍只经 C table／opaque handle；
- 必须匹配精确 `EngineBuildId`；
- internal interface 另行版本化；
- 每次 Bevy／render host 变化可能重建；
- 必须提供 disabled／fallback 体验，避免整个 world 因纯 presentation provider 缺失而无法恢复。

若 C table 只是把每个 Bevy type 逐项镜像，说明这个 realization 选错了；应改为 `NativeStatic`。

D5／R6保留的exact-build conformance fixture只验证`EngineBuildId` mismatch、rebuild、instance
lifecycle与fallback，因此算**相容层级的测试consumer**，不算建立通用render-internal interface的
产品consumer。fixture只能查询最小test-only table，不能借此镜像RenderWorld／wgpu API。只有一个真实
terrain／visibility／pass provider证明static或portable contract不足时，才设计并保留对应的窄
engine-coupled render interface；没有该证据时，upgrade rehearsal完成后不扩大或发布它。

## 體素渲染機制

默认 baseline 先采用 Bevy／生态 voxel plugin，量测后才升级机制。package model 为优化保留明确 provider seam：

```text
authoritative chunks
  → render.terrain.mesh-source@1
  → render.visibility@1
  → render.terrain.backend@1
  → Bevy Core3d / presentation
```

可能的 provider 演进：

1. **CPU mesh baseline**：face culling／greedy meshing、chunk mesh、frustum culling；
2. **异步与增量**：revision-aware job、优先队列、upload budget、dirty subregion；
3. **LOD／远景**：层级 chunk representation、transition seam、impostor／coarse mesh；
4. **occlusion**：hierarchical visibility／software or GPU-assisted occlusion；
5. **GPU-driven**：compact chunk data、compute culling、indirect draw／meshlet-like path。

这些机制不能全在第一版同时实现。每次 provider 更换必须保持：

- 权威 world／save 不变；
- source chunk revision 验证；
- stable material／content semantics；
- clear fallback 与 target capability matrix；
- profiler／capture／visual regression 可比较。

### 區塊渲染除錯

高级chunk debug不是单一“显示边界”开关。统一debug workbench分别订阅Grid、Lifecycle、
Persistence、Mesh、Collision、Visibility／LOD与Worldgen layers，并能pick一个chunk查看source
revision、queue age、provider、triangle／memory bytes与stale rejection reason。renderer只发布
结构化状态与validated gizmo data；layout、legend、radius、primitive／upload budget由
`@latticeaxiom/dev-tools`治理。

同一颜色不能同时表示dirty、occluded和collider missing；line style、label与legend也必须表达
状态。启停visualizer、截断budget或render failure都不改变authoritative world。完整contract见
[诊断、检查与除错可视化](diagnostics-inspection-and-debug-visualization.md)。

## 从 Minecraft 模组只吸收问题形状

Minecraft 渲染模组说明了玩家会要求批次、chunk rebuild、occlusion、LOD、shader pipeline 与 alternate backend；也说明了依靠 bytecode patch／Mixin、版本特定内部结构与模组间非结构化注入，会造成大版本破裂、组合冲突与高维护成本。

Lattice Axiom 不复制其 loader／patch 模型：

- 不允许 arbitrary method patch；
- 不以游戏／Bevy内部 class／Rust type 作为模组 API；
- 不按 load order 解决 renderer ownership；
- provider／feature 都经 capability graph、semantic slots 与 manifest；
- portable／engine-coupled 相容承诺明确分级；
- common optimization 进入 upstream／core capability，而不是每个模组各自 hook。

因此 Minecraft 只提供需求与失败案例，不是架构模板。相关调查见[原生模組與渲染模組調查](../research/native-plugin-and-render-mod-lessons.md)。

## 座標、精度與資產

所有 runtime Transform／physics／raycast 使用 Bevy 原生右手 Y-up：`+X` 右、`+Y` 上、forward `-Z`。package render schema 中的空间资料必须标记 coordinate semantic，不能带来源工具的隐式轴向。

无限世界：

1. 权威位置用整数 chunk anchor + local position；
2. render working set 使用 camera-local Bevy Transform；
3. provider 声明 origin／precision capability；
4. 先评估维护中的 Bevy large-world 方案；
5. 以可重现 jitter／physics／culling measurement 决定是否扩充。

## Headless

headless profile 不安装 window／render plugins，不建立 GPU resource。package graph 可保留 render declaration 作 closure validation／server-to-client negotiation，但不实例化 presentation callbacks。

纯 CPU mesher／command builder 可独立测试：vertex／index、bounds、winding、normal、revision 与 command validation。无需自制 null renderer 来证明 submit 次数。

## 故障與回退

| 失败 | 处理 |
| --- | --- |
| exclusive provider conflict | resolve 阶段失败并要求明确选择 |
| required semantic slot missing | activation 前失败或选择声明的 fallback |
| optional GPU feature unavailable | degraded realization／disable feature |
| dynamic command invalid | 拒绝当前 command list，记录 package diagnostic；不污染权威 world |
| shader／pipeline compile failure | feature-specific fallback／error material |
| device loss | 重建 presentation resources；权威 state 不变 |
| engine-coupled build mismatch | 不加载 artifact，提示 exact rebuild／fallback |

纯 presentation package 缺失时，world loader 应尽量允许 safe fallback；若 package 同时拥有权威 schema，则按权威 missing-content policy 处理，不能用「这是 renderer mod」笼统跳过。

## 觀測與預算

至少记录：

- per feature／pass CPU time；
- extraction／prepare／queue／render time；
- dynamic callback count／batch size／command bytes；
- visible／resident chunks、triangle／draw／material count；
- mesh queue、cancellation、P50／P95 latency；
- upload／readback bytes、VRAM／RAM high-water；
- occlusion／LOD effectiveness；
- invalid／fallback command count；
- edit-to-visible latency 与 stale revision drops。

budget 可以由 profile 声明，但 host 负责强制上限与 diagnostics。模组不能以无限 command／upload 绕过背压。

## 驗收

- 默认体素世界只使用 Bevy／生态公开能力即可运行；没有平行 renderer。
- 两个 post-effect packages 同时安装，semantic slot 顺序确定且资源无冲突。
- 两个 terrain backend packages 同时提供 exclusive capability 时，resolver 在加载 code 前给出清楚冲突；profile 选择后只有一个激活。
- 同一 render feature 的 static／portable dynamic realization 产生语义相同 frame fixture；static path 不经 command ABI。
- engine-coupled artifact 在 `EngineBuildId` 不匹配时精确失败并回退。
- headless 不建立 GPU，仍能验证 package／schema 与 CPU derived work。
- device loss、shader failure 或 render module failure 不改变权威 world hash。
- chunk visualizer可区分mesh／collision／visibility pipeline与revision，且受radius／primitive／upload budget限制。

## 外部依據

- [Bevy 0.19：Render Graph as ECS systems](https://bevy.org/news/bevy-0-19/#render-graph-as-systems)
- [Bevy render extraction](https://docs.rs/bevy/latest/bevy/render/extract_plugin/index.html)
- [Bevy 0.19 diagnostics overlay／text gizmos](https://bevy.org/news/bevy-0-19/#diagnostics-overlay)

## 相關文件

- [診斷、檢查與除錯可視化](diagnostics-inspection-and-debug-visualization.md)

- [Bevy 執行期](game-engine-runtime.md)
- [原生模組 ABI](native-module-abi.md)
- [資產語義](asset-semantics.md)
- [渲染生態調查](../research/renderer-physics-landscape.md)
