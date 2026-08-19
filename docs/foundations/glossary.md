---
title: 詞彙表
status: active
type: reference
updated: 2026-08-19
---

# 詞彙表

## 套件與實現

| 词汇 | 本项目中的意思 |
| --- | --- |
| logical package | 以 root／scoped `PackageName`／SemVer 识别的产品单元；拥有 dependency、capability、schema 与一种或多种 realization。不是 Cargo crate、注册命名域或动态库的同义词。 |
| `PackageName` | package graph 使用的 root `name` 或 scoped `@scope/name`，例如 `terrenia` 与 `@terrenia/worldgen`；负责版本、依赖、来源与分发，不参与 stable ID 的语法。 |
| `StableId` | 统一 registration identity：`<namespace>:<kind>/<path>`，例如 `terrenia:block/stone`；由 manifest 完整声明，不从 package name 或目录推导。 |
| namespace grant | 授权某个 `PackageName` 注册指定 namespace／kind／path pattern 的 closure-wide 规则；名称相似不自动取得授权。 |
| Terrenia | Lattice Axiom 由 root `terrenia` 与 `@terrenia/*` package closure 定义的第一个维度；不是产品别名或 host 内建主世界。 |
| package kernel | 以 Rust 实作的控制平面；负责 source、SemVer／capability resolution、lock、realization planning、build、load、activation 与诊断。它不拥有游戏 ECS／renderer。 |
| `latticeaxiom.lib` | 提供 `Package`、`GameProfile`、`Capability`、`Realization` 等 contract／组合函数的 versioned Nickel library。 |
| `CompositionSpec` | Nickel 完整求值后直接转换的强型别组合意图；仍可含 version range 与 realization preference。 |
| `LockedGameGraph` | 已解析、确定的 package version／source／dependency／capability closure。每次游戏启动的逻辑真相。 |
| `BuildPlan` | 从 lock 选择 target、profile、realization、toolchain 与 artifact工作的强型别计划。 |
| `RegistrationManifest` | SDK 为单一 package realization 产生的纯资料注册声明；包含 stable IDs、schemas、systems、settings、observability、capabilities 与 render contracts。 |
| `RegistrationImage` | package kernel 合并整个 closure manifests 后产生的 IDs、schema、schedule、settings／observability catalog与capability plan。code activation 前完成。 |
| `RuntimeImage` | 与 registration image 匹配的 static functions、dynamic callback tables、artifacts 与 instance factories。 |
| realization | 同一 logical package 的具体交付方式：data、`NativeStatic`、`PortableNative`、`EngineCoupledNative`，未来可有 `WasmComponent`。 |
| `NativeStatic` | 从 source 与 generated glue 编入 Bevy host 的 realization；直接使用 Rust／Bevy，保留 inline／LTO，不承诺 runtime ABI。 |
| `PortableNative` | 经 stable Lattice C ABI／versioned capability tables 加载的可信动态原生 realization；不直接依赖 Bevy type／version。 |
| `EngineCoupledNative` | 依赖精确 `EngineBuildId` 内部 interfaces 的动态原生 realization；接受随 host／Bevy 变化重建。 |
| `WasmComponent` | 未来用于 sandbox／跨语言的独立 realization 类别；当前只是预留方向，不是首阶段能力。 |
| package SemVer | logical package 的外部相容语言；不代替 ABI、schema、artifact hash 或 `EngineBuildId`。 |
| capability | versioned、具 cardinality 的 required／provided服务或机制，例如 `latticeaxiom:capability/render-visibility@1`；不是任意字串 hook。 |
| provider | 提供一项 capability 的 package／realization。exclusive provider 必须由 graph 明确选择，不能 load-order-wins。 |

## 註冊與內容語義

| 词汇 | 本项目中的意思 |
| --- | --- |
| exact registration | 由 `StableId` 识别的具体内容／schema／system／contract row；duplicate exact ID 一律失败。 |
| `SemanticCatalog` | `RegistrationImage` 中 closure-wide 的 Tag、typed Map、state key、Affordance、Predicate、Role、active bundle与provenance集合；不是第二个 registry。 |
| `SemanticTag` | 绑定 target registry kind 的 versioned集合contract，例如`latticeaxiom:block-tag/storage-blocks/copper@1`；回答 membership，不回答数值或动态行为。 |
| tag contribution | package 向 extensible Tag 添加自己拥有的 exact entry／nested Tag 的声明；与定义或注册 Tag contract 的 namespace authority 分开。 |
| `SemanticMap<T>` | 从某一 registry entry到versioned typed value／relation的语义映射；owner定义value schema、cardinality与deterministic conflict merger。 |
| `StatePropertyKey` | 已放置block instance的有限、可编码状态键；跨package查询的key使用stable／versioned contract，不与definition-level Tag／Map混用。 |
| `Affordance` | versioned、带request／response与context的行为contract；资料可回答时编译为static fast path，必要时才使用typed／ABI callback。 |
| `ContentPredicate` | 可序列化、带target kind的`Exact`／`InTag`／`MapMatches`／`StateEq`／`Supports`及boolean组合AST；Nickel构造，Rust验证／编译。 |
| `ContentRole` | 从active content candidates中选择具体stable ID的versioned选择点，例如默认铜储存块；不是package capability。 |
| role binding | profile／unique candidate／frozen world对Role作出的具体选择，含provenance与resolution explanation，并写入lock。 |
| `ContentBundle` | 必须原子激活的一组registration与semantic contributions；v1可作为已选package内的声明式fallback，不能隐式改变package graph。 |
| semantic image hash | authoritative／presentation semantic catalog、bindings与active bundles的canonical fingerprint；用于重现与诊断，不单独代理world compatibility。 |

## ABI 與執行期

| 词汇 | 本项目中的意思 |
| --- | --- |
| native ABI | dynamic native library 与 host 的 C calling／layout／ownership contract；由 bootstrap 与独立 versioned interface tables 组成。 |
| bootstrap entry | 动态库唯一固定 export：`latticeaxiom_module_entry`；只协商 descriptor、instance factory 与 interfaces。 |
| interface table | COM-like、独立 major／minor 与 `struct_size` 的 function table，例如 `ecs.batch@1`。 |
| ABI-POD | 经 SDK 明确规定 C layout、size、alignment、offset 与 layout hash，可安全作为 dynamic batch column 的 plain data。 |
| batch view | 一次 dynamic system callback 借用的 archetype／column集合；避免逐 entity FFI。 |
| command buffer | dynamic callback 请求 structural ECS／world mutation 的验证后 deferred command集合。 |
| opaque／generational handle | host-owned复杂对象的 instance-local识别；含 slot／generation，可防 stale use，但不可持久化。 |
| `EngineBuildId` | 覆盖 core source、Cargo lock、features、target 与 internal interface 的精确 build身分；只用于 engine-coupled compatibility。 |
| EngineInstance | 一个 lock／registration／runtime image 与 Bevy App、module contexts、world／task lifecycle 的绑定实例。 |
| old-binary fixture | 保存真实旧 dynamic artifact、不在测试时重编的 ABI 相容／拒绝测试输入。 |
| trusted native | 可读取／破坏整个进程的 static／dynamic native code。ABI validation 不是 sandbox。 |
| static／dynamic equivalence | 两种 realization 的 package、manifest、IDs、schedule、schema 与可观察权威结果相同；不要求机器调用路径相同。 |

## Bevy 與渲染

| 词汇 | 本项目中的意思 |
| --- | --- |
| Bevy runtime | 唯一游戏 runtime；由 Bevy App、ECS、schedules、assets、render 与 standard plugins 组成。 |
| host adapter | 把 registration／runtime image 安装进 Bevy 的 core implementation；不是第二个 engine facade。 |
| static system | `NativeStatic` 直接注册的 typed Bevy system，可使用完整 system parameters／LTO。 |
| dynamic bridge system | 在 Bevy schedule 中把 query 形成 batches、调用 foreign callback 并验证 command 的 host system。 |
| `RenderData` | 普通 presentation 资料层；使用现有 Mesh／Material／Transform／stable render data。 |
| `RenderFeature` | 可组合 shader／compute／post effect，声明 semantic inputs／outputs／resources／fallback。 |
| `RenderPass` | 插入 versioned semantic slot 的新 render phase／pass。 |
| `RenderProvider` | 替换一项 exclusive 渲染机制，例如 terrain backend／visibility；不是替换整个 Bevy renderer。 |
| semantic render slot | 不依赖 Bevy私有 node index 的 versioned 插入语义，例如 `before.tonemap`。 |
| presentation | 从权威状态衍生的 camera、mesh、material、particle、animation、audio 与 UI；可丢弃重建。 |
| Y-up | Bevy 原生右手坐标：`+X` 右、`+Y` 上、forward `-Z`，水平面 `x-z`。 |

## 設定、資訊與前端

| 词汇 | 本项目中的意思 |
| --- | --- |
| `SettingSpec` | package以stable ID注册的typed setting schema；声明scope、authority、default、constraint、apply impact、migration与localization，不是任意UI widget。 |
| composition parameter | 在resolve／lock前改变dependency、feature、realization或provider的profile输入；不能伪装成runtime setting热改graph。 |
| effective setting | registry按spec允许的scope／authority规则解析出的当前typed值，带default／user／world／session provenance。 |
| diagnostic item | package注册的结构化info／metric资料源；由subscription、permission与budget控制，不是package直接绘制的文字行。 |
| target inspect | 准星目标的玩家信息surface；组合name／summary／detail／technical fragments，并受server／world visibility policy控制。 |
| debug visualizer | 以stable ID注册、受radius／primitive／CPU／GPU预算约束的world-space除错资料；例如chunk lifecycle、collider、AABB或query。 |
| `ClientShellGraph` | world activation前运行开始页、settings、world catalog与diagnostics的独立locked package closure；与`LockedGameGraph`共用resolver／lock schema，只读world header，不是hidden host plugin list。 |

## 世界與持久化

| 词汇 | 本项目中的意思 |
| --- | --- |
| 权威世界（authoritative world） | 能决定玩法且必须正确恢复的 chunk、entity 与续行状态；不是 render world／cache。 |
| `WorldId` | 创建时生成、rename／move不变的world稳定身份；display name与directory slug都不是identity。 |
| `WorldHeader` | 可校验、大小有界、供catalog与preflight只读扫描的world metadata projection；不需要载入chunk或执行module code。 |
| `WorldCatalog` | shell使用的world header／checkpoint／trash索引；保留损坏entry与health状态，不是权威world store。 |
| `WorldPreflight` | world写入与module business code之前验证frozen lock、artifact、schema、semantic setting与migration并产生`WorldOpenPlan`的阶段。 |
| checkpoint | 与某个world revision／lock／header一起校验、可独立恢复的RocksDB恢复点；不同于process-local read snapshot。 |
| 已物化 chunk | 已完成第一次生成并保存完整 snapshot 的 chunk；之后不因 generator 更新隐式重算。 |
| active working set | 当前载入 RAM／Bevy ECS 供模拟与 presentation 的世界子集；可由 persistent data 重建。 |
| chunk revision | chunk 权威资料变更后递增的 revision，用来拒绝 stale mesh／collision／I/O result。 |
| generation provenance | generator revision、config 与上游规划 fingerprint 的长期记录。 |
| schema owner | 唯一负责一类 persistent／ABI-visible资料 schema、版本、migration 与 fixtures 的 package／domain。 |
| stable content ID | 跨重启、save 与未来 network 的具名识别；不得用 Bevy／ABI process-local ID 代替。 |
| process-local ID | Bevy Entity／Handle、ABI handle、GPU／physics handle 等只在当前 instance有效的识别。 |
| fixed tick | Bevy `FixedUpdate`／`Time<Fixed>` 推进的 gameplay步长，不等于 render frame。 |
| headless profile | package graph 选择不安装 window／render 的 Bevy standard profile；不是自制 null renderer。 |
| upstream deviation | 替换／fork／平行实现 Bevy或成熟通用依赖；须通过决策 0014 的证据门槛。 |

## 相關文件

- [專案願景](project-vision.md)
- [技術棧](technology-stack.md)
- [套件內核](../architecture/package-management.md)
- [語義註冊](../architecture/semantic-registration.md)
- [原生 ABI](../architecture/native-module-abi.md)
- [版本與相容性](../architecture/versioning-and-compatibility.md)
- [設定與配置](../architecture/settings-and-configuration.md)
- [診斷、檢查與除錯可視化](../architecture/diagnostics-inspection-and-debug-visualization.md)
- [World 生命週期與開始頁](../architecture/world-lifecycle-and-start-ui.md)
