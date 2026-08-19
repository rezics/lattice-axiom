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
| logical package | 以稳定 package ID／SemVer 识别的产品单元；拥有 dependency、capability、schema 与一种或多种 realization。不是 Cargo crate 或动态库的同义词。 |
| package kernel | 以 Rust 实作的控制平面；负责 source、SemVer／capability resolution、lock、realization planning、build、load、activation 与诊断。它不拥有游戏 ECS／renderer。 |
| `latticeaxiom.lib` | 提供 `Package`、`GameProfile`、`Capability`、`Realization` 等 contract／组合函数的 versioned Nickel library。 |
| `CompositionSpec` | Nickel 完整求值后直接转换的强型别组合意图；仍可含 version range 与 realization preference。 |
| `LockedGameGraph` | 已解析、确定的 package version／source／dependency／capability closure。每次游戏启动的逻辑真相。 |
| `BuildPlan` | 从 lock 选择 target、profile、realization、toolchain 与 artifact工作的强型别计划。 |
| `RegistrationManifest` | SDK 为单一 package realization 产生的纯资料注册声明；包含 stable IDs、schemas、systems、access、capabilities 与 render contracts。 |
| `RegistrationImage` | package kernel 合并整个 closure manifests 后产生的 IDs、schema、schedule 与 capability plan。code activation 前完成。 |
| `RuntimeImage` | 与 registration image 匹配的 static functions、dynamic callback tables、artifacts 与 instance factories。 |
| realization | 同一 logical package 的具体交付方式：data、`NativeStatic`、`PortableNative`、`EngineCoupledNative`，未来可有 `WasmComponent`。 |
| `NativeStatic` | 从 source 与 generated glue 编入 Bevy host 的 realization；直接使用 Rust／Bevy，保留 inline／LTO，不承诺 runtime ABI。 |
| `PortableNative` | 经 stable Lattice C ABI／versioned capability tables 加载的可信动态原生 realization；不直接依赖 Bevy type／version。 |
| `EngineCoupledNative` | 依赖精确 `EngineBuildId` 内部 interfaces 的动态原生 realization；接受随 host／Bevy 变化重建。 |
| `WasmComponent` | 未来用于 sandbox／跨语言的独立 realization 类别；当前只是预留方向，不是首阶段能力。 |
| package SemVer | logical package 的外部相容语言；不代替 ABI、schema、artifact hash 或 `EngineBuildId`。 |
| capability | versioned、具 cardinality 的 required／provided服务或机制，例如 `render.visibility@1`；不是任意字串 hook。 |
| provider | 提供一项 capability 的 package／realization。exclusive provider 必须由 graph 明确选择，不能 load-order-wins。 |

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

## 世界與持久化

| 词汇 | 本项目中的意思 |
| --- | --- |
| 权威世界（authoritative world） | 能决定玩法且必须正确恢复的 chunk、entity 与续行状态；不是 render world／cache。 |
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
- [原生 ABI](../architecture/native-module-abi.md)
- [版本與相容性](../architecture/versioning-and-compatibility.md)
