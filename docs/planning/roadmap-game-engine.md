---
title: 套件驅動的 Bevy 執行期整合路線
status: active
type: roadmap
updated: 2026-08-19
decision:
  - ../decisions/0008-static-and-dynamic-realizations-share-one-graph.md
  - ../decisions/0010-nickel-driven-package-system.md
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0017-versioned-native-module-abi.md
  - ../decisions/0018-package-kernel-from-first-vertical-slice.md
---

# 套件驅動的 Bevy 執行期整合路線

## 目标

这条路线把 package control plane、dual realization 与 Bevy runtime 合成一个可验证依赖链。它既不是「先造完整 package manager」，也不是「先让官方 Bevy Plugin 跑起来再补 ABI」。每个基础阶段都尽快进入[第一个可玩 demo](roadmap-first-demo.md)。

```text
Nickel / typed models
  → deterministic lock
  → generated registration
  → static + dynamic equivalence
  → package-driven Bevy App
  → playable world / persistence
  → render composition / upgrade rehearsal
  → ABI 1.0 evidence
```

## R0：冻结词汇、模型与最小 schema

### 交付

- workspace／Rust toolchain／CI；
- `CompositionSpec`、`LockedGameGraph`、`BuildPlan`、`RegistrationManifest`、`RegistrationImage`、`RuntimeImage` 的 crate／schema skeleton；
- root／scoped `PackageName`、SemVer、独立 `StableId`／namespace grant、realization 与 schema owner grammar；
- `SemanticTag`／`SemanticMap`／`StatePropertyKey`／`Affordance`／`ContentPredicate`／`ContentRole`／`ContentBundle` typed model skeleton；
- `SettingSpec`／scope／authority／apply impact、InfoItem／Metric／InspectFragment／DebugVisualizer typed model skeleton；
- `WorldId`／`WorldHeader`／`WorldOpenPlan`与shell／world双lock关系；
- canonical encoding／hash rules；
- `latticeaxiom.lib` package与semantic constructors、contracts、explicit concat、default／binding overlay；
- version coordinate／diagnostic code catalog。

### 出场条件

- Nickel source 在 CLI／embedded evaluator 得到语义相同 `CompositionSpec`；
- typed models 有 round-trip／golden fixtures；
- source path／record key reorder 不改变 canonical hash；
- Nickel普通merge不隐式拼接semantic arrays，相同优先级binding冲突保留双方source span；
- 模型没有 Bevy Entity／Handle／TypeId 或 function pointer；
- 每项 version field 都有明确 owner，未出现 global `engineVersion`；
- graph-affecting parameter与runtime setting在schema层可机械区分。

## R1：Local package graph、SemVer 与 lock

### 交付

- workspace／local directory／in-memory fixture sources；
- SemVer exact／caret／tilde／bounded range 与 pre-release tests；
- dependency、optional／feature、cycle 与唯一版本政策；
- versioned capability、exclusive／multi provider；
- realization／target selection；
- deterministic resolver／resolution explanation；
- `latticeaxiom.lock` read／write／frozen mode；
- `latticeaxiom-shell.lock`与per-world frozen game lock fixture。

### 出场条件

- randomized source discovery不改变 lock；
- missing range、cycle、exclusive conflict、unavailable realization 有 package-chain诊断；
- frozen lock 不因新增 source version改变；
- resolver 可在没有 Bevy／GPU 的 test process运行；
- 尚未实现 public registry／network fetch／general SAT scale。

## R2：SDK、Registration 與 Static Path

### 交付

- package SDK／proc macro prototype；
- 从单一业务声明生成 manifest 与 static Bevy glue；
- `HostTyped`／`GeneratedSharedSchema` component 注册与 generated schema crate prototype；
- stable IDs、components／messages／assets、system stage、read／write、capabilities；
- `SettingSpec`、diagnostic item／metric、inspect fragment与debug visualizer manifest fragments；
- semantic manifest fragments、typed Tag DAG、Map merger、Predicate checker、Role resolver与single-wave fallback compiler；
- manifest linter／hash／producer metadata；
- closure-wide numeric ID、schema owner 与 schedule compiler；
- closure-wide SemanticCatalog、active bundle／binding lock与hot-path bitset／table／predicate plan；
- closure-wide SettingsCatalog／observability catalog与fragment conflict compiler；
- `TestGameplayPackage` static realization。

### 出场条件

- host 不加载 code 即能发现所有 ID／schema／schedule／render conflict；
- host不加载code即能发现setting ID／scope与inspect fragment conflict；
- static package 直接成为 typed Bevy system，不经过 C ABI；
- static typed dependency 必须由 host／generated schema crate 在 build plan 中显式满足；
- manifest 与 static callback map 完全对应；
- discovery／registration order 不改变 IDs／schedule；
- randomized fragment order不改变semantic image；Tag cycle、Map conflict、Role ambiguity与fallback竞争fail-fast；
- optimized build 能启用 LTO，并有 static direct-call baseline。

## R3：Portable Native ABI `0.x`

### 交付

- generated C header／Rust bindings；
- single entry、ABI header、`query_interface`；
- module descriptor／manifest hash／callback map validation；
- per-EngineInstance create／start／stop／destroy；
- `ecs.batch@0.x`、command buffer、diagnostics、messages 最小 tables；
- batched setting read／change与subscribed diagnostic／inspect sample prototype；
- ABI-POD schema／opaque handle／scratch arena；
- `RuntimeDynamic` POD descriptor → process-local Bevy `ComponentId` 注册与 mapping；
- panic boundary／status／ownership rules；
- dynamic realization of the same `TestGameplayPackage`；
- no-unload policy enforced。

### 出场条件

- static／dynamic registration hash、numeric IDs、system order 与 tick state hash 相同；
- dynamic 每 system／batch 呼叫，不逐 entity；
- dynamic setting／diagnostic callback不逐item／entity／voxel往返；
- wrong magic／major／size／hash／layout／callback 在业务 code 前失败；
- 预编译 static consumer 对未知 runtime type 的 typed dependency 会在 compose／plan 阶段被拒绝，不会误认 layout 相同即 type 相同；
- panic 不跨 FFI，stop 后 callback／command 被拒绝；
- static benchmark 未经过 interface table；
- dynamic batch 与逐 entity反例 benchmark 有数据。

## R4：Package-driven Bevy Host

### 交付

- shell／client／headless／test Nickel profiles与独立shell lock；
- `RegistrationImage`／`RuntimeImage` → Bevy host adapter；
- `DefaultPlugins`／headless standard plugin mapping；
- static system／dynamic bridge 安装；
- semantic system stages → Bevy `SystemSet`；
- multi-EngineInstance test；
- package activation transaction／rollback；
- `@latticeaxiom/settings`／settings-ui／observability／inspect／dev-tools／front-end／world-library基础packages；
- SettingsCatalog transaction、package-injected设置页与CLI fallback；
- subscription-driven diagnostics workbench skeleton；
- WorldHeader catalog、开始页与write-before `WorldOpenPlan` preflight。

### 出场条件

- 不存在可运行官方游戏的隐藏 hand-written plugin list；
- 任一fixture package可注入setting／metric／inspect fragment而不修改surface package source；
- shell在world缺失／损坏时仍可进入settings与recovery，不执行world module code；
- client／headless 使用相同权威 package closure；
- package error 在 `Playing` 前失败；
- dynamic module 不取得 Bevy World；
- Bevy 是唯一 runner／ECS／scheduler／task runtime；
- multi-instance 不共享隐式 module／world global state。

## R5：可玩世界消费

本阶段由 demo 的 voxel／persistence milestones 驱动，不另建平台样例。

### 交付

- upstream voxel／physics／input adoption spike；
- stable block ID、chunk revision、world command；
- Tag input、Role output、shared state key与一个typed Map的真实block／structure consumer；
- new-world／frozen-world fallback bundle persistence；
- `terrenia` dimension closure／test content packages；
- dual-realization gameplay system 参与真实 break／place；
- RocksDB／memory storage、snapshot、shutdown；
- minimal worldgen／provenance；
- target block name／owner inspect overlay；
- Performance preset、chunk lifecycle／persistence／mesh／collision与physics shape visualizers；
- world list／Quick Create／Continue gate、checkpoint／clone／trash／restore与loading／durability UI。

### 出场条件

- 玩家完整挖掘／放置／重启循环只能从 lock 启动；
- static／dynamic realization切换不改变权威 state／save；
- stale mesh／collision／I/O result 不覆盖新 revision；
- package missing／schema mismatch 有 recovery／拒绝 policy；
- semantic binding／bundle mismatch在writable world前诊断，Role在world command前解析为concrete ID；
- 非Terrenia测试维度可替换`terrenia` closure而无host／platform semantic special case；
- package／ABI overhead 出现在真实 profiler capture，而不只 microbenchmark。
- unselected diagnostics不执行昂贵采集；chunk／collision visualizer受radius／primitive／upload budget约束。
- preflight不写world；missing／migration／unclean／low-disk fixtures给可恢复动作，损坏header不从catalog消失。

## R6：Render Feature 與 Provider 組合

### 交付

- RenderData／Feature／Pass／Provider manifest schema；
- semantic slots、resource access、GPU requirements 与 fallback；
- default terrain mesh／visibility／backend providers；
- 两个可组合 post／compute feature fixtures；
- 两个 mutually exclusive terrain backend fixtures；
- portable dynamic compact command list prototype；
- engine-coupled render interface prototype（只有真实 consumer才保留）；
- chunk Rendering visualizer layer：visibility／LOD／provider／GPU budget／stale reason。

### 出场条件

- feature pair graph deterministic、无隐式 load order；
- exclusive provider conflict 在加载 code 前失败；
- GPU feature不足时选 fallback／明确禁用；
- dynamic command bounds／handle／budget 可验证；
- headless 不建立 GPU仍可验证 graph；
- render failure 不改变 authoritative world hash。
- render visualizer enable／truncate／device failure不改变authoritative world hash。

## R7：Bevy Upgrade 與 ABI 冻结演练

### 交付

- 从当前 Bevy baseline 到下一个明确 release 的独立 migration；
- 新 `EngineBuildId`；
- static／engine-coupled rebuild；
- portable old binaries（不重编）；
- old manifest／ABI fixtures；
- client／headless／save／asset／render／performance suite；
- settings schema migration／orphan与WorldHeader／catalog／clone-migration／trash restore suite；
- ABI inspector／compatibility report。

### 出场条件

- portable artifact 按 interface range加载或因明确 contract break精确失败；
- engine-coupled artifact先因 build ID拒绝，重编后通过；
- 未改变 schema 的 world无需 migration；
- external observable break 已提升正确 package／capability owner version；
- 证据不足时 ABI 留在 `0.x`，不为了时间表虚假发布 1.0。

## ABI 1.0 冻结 Gate

全部成立才能冻结：

1. 一个真实 gameplay package static／portable 等价。
2. 至少两代不可重编 old-binary fixtures。
3. dynamic batch真实场景 overhead 在预算内，static direct 优势保留。
4. memory／panic／bad input／lifecycle 故障注入完整。
5. 一次 Bevy upgrade rehearsal 证明 portable／coupled 分级。
6. render feature pair 与 exclusive provider conflict／fallback 通过。
7. C header、Rust SDK、manifest、inspector 与 docs由同一 schema 可重建。
8. 错误诊断能让 package author实际修复，不只返回 numeric status。
9. semantic Tag／Map／Predicate／Role／fallback fixtures经过两代lock／world reopen，且runtime热路径不执行Nickel或字符串package查询。
10. 一个真实package的setting／metric／inspect fragment经过static／dynamic与两代schema fixture。
11. shell preflight、checkpoint／clone与world recovery证明升级失败不改坏原world。

## 上游偏離 Gate

若 Bevy／生态 plugin 阻塞任何阶段，另建证据包：

| 字段 | 必填 |
| --- | --- |
| playable reproduction | build、操作、seed／world／lock |
| non-negotiable requirement | 玩家／作者／产品层验收 |
| upstream attempts | settings、extension、plugin、issue／PR |
| measurements | profiler／platform／compatibility／failure |
| smallest deviation | 只替换哪个 function／data flow |
| ownership | maintainer、tests、upgrade、upstream／exit |

package／ABI 核心不需要通过此 gate证明「Bevy 做不到」，但其实现仍必须先采用成熟 building blocks，且不能借机复制通用 engine。

## 延後项目

- public／federated registry；
- 大型通用 PubGrub／SAT resolver；
- remote marketplace／auto-update；
- multi-platform build farm／distributed cache；
- signature transparency／organization trust delegation；
- native library hot unload；
- production `WasmComponent` ecosystem。

## 相關文件

- [第一個可玩 demo](roadmap-first-demo.md)
- [套件內核](../architecture/package-management.md)
- [原生 ABI](../architecture/native-module-abi.md)
- [Bevy 執行期](../architecture/game-engine-runtime.md)
- [語義註冊、內容判定與選擇](../architecture/semantic-registration.md)
- [待決問題](open-questions.md)
- [Package 設定與配置](../architecture/settings-and-configuration.md)
- [診斷、檢查與除錯可視化](../architecture/diagnostics-inspection-and-debug-visualization.md)
- [World 目錄、開始頁與安全生命週期](../architecture/world-lifecycle-and-start-ui.md)
