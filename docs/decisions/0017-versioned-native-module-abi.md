---
title: 原生動態模組採版本化 C ABI 與 capability table
status: accepted
type: decision
updated: 2026-08-19
---

# 決策 0017：原生動態模組採版本化 C ABI 與 capability table

## 背景

Lattice Axiom 需要让已编译模组跨游戏 build 工作，同时保留静态模组的原生性能。Rust 没有稳定 ABI；`String`、`Vec`、trait object、panic、allocator、generic 与大多数 layout 都不能作为长期动态库契约。Bevy 已在 0.14 弃用、0.15 移除原本面向模组加载的 `bevy_dynamic_plugin`；这不等于 Bevy 放弃开发期 dynamic linking，而是证明同 toolchain／crate graph 的动态链接本身不构成稳定的第三方分发 ABI。

因此必须区分两件事：

- **逻辑共同层**：package、SemVer、capability、stable ID、schema、registration 与 schedule 语义；
- **机器边界**：动态库如何进入 host、交换资料、调用系统与管理生命週期。

本决策只固定 native dynamic 边界；静态 realization 继续直接使用 Rust／Bevy glue，不经 FFI。

## 決策

### 1. 固定 C bootstrap，不承諾 Rust ABI

原生动态 artifact 是平台 `cdylib`，只要求导出一个稳定 C symbol：

```c
LaxStatus latticeaxiom_module_entry(
    const LaxEntryRequest* request,
    LaxEntryResponse* response);
```

所有跨边界 struct 使用 C layout、固定宽度整数、pointer + length 与显式 function pointer。Rust binding 由 SDK 生成并使用 `extern "C"`／`#[repr(C)]`。ABI 不传递 Rust `bool`、enum layout、slice、`String`、`Vec`、trait object、closure、`TypeId`、Bevy type 或会跨边界析构的任意 Rust value。

每个结构以共同 header 开始：

```c
typedef struct LaxAbiHeader {
    uint32_t magic;
    uint16_t abi_major;
    uint16_t abi_minor;
    uint32_t struct_size;
    uint32_t flags;
} LaxAbiHeader;
```

host 先验证 magic、major、可接受 minor、`struct_size`、target 与 artifact／manifest hash，再读取任何后续字段。ABI `0.x` 是首阶段实验契约；达到本决策的冻结 gate 后才发布 `1.0`。

### 2. Bootstrap 只協商，能力按 interface 獨立版本化

entry response 返回 module descriptor、instance factory 与 `query_interface`。每项 host 服务使用稳定 `InterfaceId + major + minor` 独立演进，采用 COM-like table：

- `core.log`
- `core.diagnostics`
- `ecs.batch`
- `ecs.command-buffer`
- `messages`
- `assets`
- `tasks`
- `render.feature`
- `render.command-list`
- 仅 engine-coupled 可见的内部 interfaces

minor 版本只能在 `struct_size` 末尾追加 optional function／field；breaking layout、语义或所有权变化提升该 interface major。module 必须声明 required／optional interface range；缺少 required capability 时在创建 instance 前失败。

### 3. Portable 與 engine-coupled 是兩個誠實等級

| realization | 相容承諾 | 可取得能力 | 典型用途 |
| --- | --- | --- | --- |
| `PortableNative` | Lattice ABI／capability range | 稳定 batch ECS、command、message、asset、task 与 render feature tables | 可分发 gameplay、worldgen、资料处理与常规 render extension |
| `EngineCoupledNative` | 精确 `EngineBuildId` + interface version | 上述能力加已标记的 host／renderer 内部接口 | 极低层 renderer、特殊 profiling 或实验性 engine integration |
| `NativeStatic` | source rebuild；没有 runtime ABI | 完整 Rust、Bevy App／World／RenderApp | 需要任意 Bevy system type 或最大性能的可信源码模块 |

两种动态等级使用同一 bootstrap 与 loader。`EngineCoupledNative` 不伪装成 portable；`EngineBuildId` 不匹配一律拒绝加载。真正需要直接操作 `bevy::App`／`World`／`RenderApp` 的代码选择 `NativeStatic`。

### 4. Registration 先於 code activation

SDK／proc macro 从同一业务定义生成 `RegistrationManifest`，至少包含：

- package／realization identity 与 manifest schema；
- stable component、resource、message、asset 与 content IDs；
- component layout kind、schema owner 与 migration identity；
- systems、stage／set、read／write set、ordering 与 thread policy；
- required／provided capabilities 与 exclusivity；
- render features、semantic slots、resource access、GPU requirement 与 fallback；
- callback key 与 expected signature；
- canonical manifest hash。

package kernel 在加载 code 以前合并、分配 numeric ID、检查冲突与编译 schedule，产生 `RegistrationImage`。dynamic library entry 必须回报完全相同的 manifest hash 与 callback map；不匹配表示 artifact 与 lock 不同，禁止启动。static glue 嵌入同一 manifest，但 callback 可直接解析为 typed Rust function。

### 5. 熱路徑按批次，不逐 entity FFI

动态 system callback 接收 `LaxSystemCall`，其中包含 tick／stage context、匹配 archetype batches 与 host command sink。每个 `LaxColumnView` 描述 numeric component ID、element size／alignment、count、stride、access flags 与 data pointer。

规则：

- 一次 callback 处理一个 system 的一个或多个 batch；不得为每个 entity 往返 ABI。
- 第一版只允许经 ABI schema 明确标记的 POD／plain columns 直接 zero-copy；复杂或 host-owned 值使用 generational handle 或 opaque service API。
- foreign code 不能保留 batch pointer 到 callback 结束以后。
- structural mutation、spawn／despawn、component add／remove 与跨系统 message 通过 command buffer，在 host barrier 验证后统一套用。
- host 可以因 alignment、layout、isolation 或调度需要提供 staging buffer；zero-copy 是合法优化，不是所有 schema 的保证。

### 6. 所有權、錯誤與生命週期顯式化

- 分配者负责释放；每个 returned buffer 都带对应 `release` callback，或由 host scratch arena 限定生命週期。
- FFI function 不得 unwind；SDK 在每个 exported／callback boundary 捕获 panic 并转换为 `LaxStatus` 与 diagnostic record。
- UTF-8 以 pointer + byte length 表示，不要求 NUL terminator；任何 borrowed view 都带明确 lifetime 规则。
- 每个 `EngineInstance` 独立执行 `create → start → callbacks → stop → destroy`；禁止以隐式 process global 保存 world／host state。
- thread safety 与 callback 可重入性由 table flags 声明；host 不推断。
- v1 停止 instance 后仍不卸载 native library。热卸载要等 schema ownership、task cancellation、GPU fence、callback、TLS 与 foreign thread 都有证明后另立 ADR。

### 7. Native 是可信程式碼，不是假沙箱

C ABI 解决版本与资料边界，不限制动态库读取进程内存、调用 OS 或使 host 崩溃。`PortableNative` 与 `EngineCoupledNative` 都只用于可信原生代码。未来不可信生态使用独立 `WasmComponent`／process isolation realization 与 capability policy；不能把「ABI 验证通过」写成安全保证。

## 版本規則

| 变化 | 需要的动作 |
| --- | --- |
| table 尾端新增 optional function | interface minor |
| 改变既有 field／ownership／callback semantics | interface major |
| 新增独立 capability | 新 InterfaceId；不必提升 bootstrap major |
| package public behavior／dependency break | package SemVer major |
| 只改变 Bevy 内部且 portable contract 不变 | portable artifact 可不变；更新 `EngineBuildId` |
| engine-coupled internal interface 改变 | 新 `EngineBuildId`，必要时 interface version 也升级 |
| persisted component schema 改变 | schema owner version／migration；不由 ABI 自动代理 |

这些坐标可以同时变化，但不得互相代替。

## 結果

- 动态 ABI 可在不暴露 Rust／Bevy layout 的情况下长期演进。
- 每个 capability 独立版本化，避免一个巨大 `HostApiV37` 让所有模组同步升级。
- batch view 与 command buffer 让动态 gameplay 保持原生资料处理能力，同时接受无法跨边界 LTO／inline 的现实成本。
- static realization 不经 C ABI，继续拥有直接 Bevy integration 与编译器最佳化。
- SDK、bindings、ABI linter、old-binary fixtures 与 loader diagnostics 成为必须维护的产品资产。

## 被否決的方案

### Rust `dylib`／`abi_stable` 型別直接承載 Bevy Plugin

它仍会把 compiler、crate graph、allocator 与 Bevy内部 layout 变成分发契约，无法提供所需长期独立性。

### 單一巨大 Host API table

任何局部变化都会扩散成全局 ABI 版本，也难以表达 optional／exclusive capability。

### 逐 entity callback

FFI、indirect call 与 scheduler barrier 成本会被 entity 数量放大；批次才是可接受的默认资料路径。

### v1 支援 native hot unload

code pointer、task、TLS、GPU work、asset 与 persisted schema 的 owner 可能仍存活；只做 `FreeLibrary`／`dlclose` 不能证明安全。

## 冻结與驗證 Gate

ABI 只能在以下证据齐全后从 `0.x` 冻结为 `1.0`：

1. 同一非玩具 package 的 static／portable dynamic equivalence suite 通过。
2. old-binary fixtures 跨至少两次 host 变更仍按承诺加载或给出精确拒绝原因。
3. 静态、batch dynamic 与逐 entity 反例有基准；batch overhead 在已定预算内。
4. panic、bad pointer／length、wrong table size、unknown interface、allocator mismatch 与 callback-after-stop 有故障注入。
5. Bevy 升级演练证明 portable／engine-coupled 的不同重建政策。
6. 至少两个可组合 render feature 与两个 exclusive provider 完成冲突／fallback 测试。
7. C header、Rust SDK、示例、ABI inspector 与诊断格式可以从同一 schema 重建。

## 外部依據

- [Rust type layout reference](https://doc.rust-lang.org/stable/reference/type-layout.html)
- [Rust linkage reference](https://doc.rust-lang.org/reference/linkage.html)
- [Bevy 0.14：弃用 dynamic plugins](https://bevy.org/news/bevy-0-14/#deprecate-dynamic-plugins)
- [Bevy 0.14 → 0.15：移除 `bevy_dynamic_plugin`](https://bevy.org/learn/migration-guides/0-14-to-0-15/#remove-deprecated-bevy_dynamic_plugin)
- [Bevy dynamic linking discussion](https://github.com/bevyengine/bevy/discussions/24010)

## 相關文件

- [決策 0008：靜態與動態共用一圖](0008-static-and-dynamic-realizations-share-one-graph.md)
- [原生模組 ABI 架構](../architecture/native-module-abi.md)
- [版本與相容性](../architecture/versioning-and-compatibility.md)
