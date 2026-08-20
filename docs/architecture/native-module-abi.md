---
title: 原生模組 ABI、批次資料與生命週期
status: proposed
type: architecture
updated: 2026-08-20
decision:
  - ../decisions/0008-static-and-dynamic-realizations-share-one-graph.md
  - ../decisions/0017-versioned-native-module-abi.md
  - ../decisions/0023-freeze-sdk-registration-and-semantic-compilation.md
  - ../decisions/0024-freeze-portable-native-abi-0x.md
  - ../decisions/0026-freeze-first-demo-performance-budgets.md
  - ../decisions/0031-freeze-bevy-upgrade-dependency-and-supply-chain-policy.md
---

# 原生模組 ABI、批次資料與生命週期

## 結論

动态 native 模组使用稳定 C bootstrap 和独立版本化 capability tables。它不导出 Rust／Bevy object，也不把每个 entity 变成一次 FFI 呼叫。package SDK 生成 manifest、C layout、batch shim 与 bindings；host 在执行 code 前完成 graph、schema、layout、capability 与 callback 验证。

同一业务 package 的静态 realization 不经过这层 ABI。它使用相同 `RegistrationManifest`，但生成直接 Rust／Bevy glue，因此保留完整原生优化。ABI 是动态部署边界，不是全项目的最低共同分母。

## 範圍與信任

本页覆盖：

- `PortableNative`：只依赖稳定 Lattice ABI／capability；
- `EngineCoupledNative`：另依赖精确 `EngineBuildId` 的低层 interface；
- static／dynamic adapter 如何保持 registration 等价。

本页不承诺：

- Rust ABI、Bevy API 或 `.rlib` 稳定；
- native code 沙箱；
- runtime library unload；
- 任意旧 ABI 永久兼容；
- 第一版支持所有 Rust component layout。

所有 native dynamic code 都视为可信进程代码。未来不可信模组必须使用独立 `WasmComponent`／process realization。

## Artifact 內容

一个发布的 native realization 至少包含：

```text
marker-package/
├── package descriptor        PackageName / SemVer / source / realization
├── registration manifest     schemas / systems / capabilities / hash
├── target descriptor         OS / arch / ABI / EngineBuildId if coupled
├── library                   .dll / .so / .dylib
├── assets/                   content-addressed or manifest-listed assets
└── provenance                producer SDK / toolchain / artifact hash
```

loader 不通过扫描 export symbol 猜测能力，也不执行库内任意初始化函数读取 metadata。descriptor 与 manifest 是可先验证的纯资料；动态库只提供固定 entry symbol。

## Bootstrap ABI

### 固定 entry

```c
typedef uint32_t LaxStatus;

LaxStatus latticeaxiom_module_entry(
    const LaxEntryRequest* request,
    LaxEntryResponse* response);
```

只有这个 symbol 名称与 C calling convention 是 bootstrap。所有其他 function 经 response／interface table 取得，避免导出 symbol 集合随功能成长。

### 共同 header

```c
typedef struct LaxAbiHeader {
    uint32_t magic;
    uint16_t abi_major;
    uint16_t abi_minor;
    uint32_t struct_size;
    uint32_t flags;
} LaxAbiHeader;
```

验证次序不可改变：

1. pointer／minimum size；
2. magic；
3. ABI major；
4. 可接受 minor；
5. `struct_size` 与 alignment；
6. flags 中没有 required unknown bit；
7. target／endianness／pointer width；
8. package、artifact 与 manifest hash。

读取端只能访问 `struct_size` 已覆盖的字段。minor update 可以在尾部追加 optional field／function pointer；既有字段的意义、ownership 或 offset 不能改变。

### Entry request／response

概念字段：

```text
LaxEntryRequest
├── bootstrap header
├── host ABI range
├── EngineBuildId
├── target descriptor
├── locked package / realization ID
├── expected manifest hash
├── host query_interface
└── diagnostic sink

LaxEntryResponse
├── bootstrap header
├── module / realization ID
├── manifest hash
├── module flags
├── create_instance / destroy_instance
└── module query_interface
```

response 使用 caller-allocated struct；module 不返回 ownership 不明的 Rust object。

## Interface Discovery

每项 interface 由 128-bit stable ID（或等价 canonical ID）、major、minor 与 table size 识别：

```c
typedef struct LaxInterfaceRequest {
    uint64_t id_hi;
    uint64_t id_lo;
    uint16_t min_major;
    uint16_t max_major;
    uint16_t min_minor;
    uint16_t reserved;
} LaxInterfaceRequest;
```

`query_interface` 返回「一个选定 major 下 host 支援的最大相容 minor」及 table pointer。table 的共同前缀包含 header、interface ID／version、context pointer 与 release／retain policy（若需要）。

### 为什么按 capability 分 table

- log 更新不应提升 ECS ABI major；
- render extension 不应让纯 worldgen module 重编；
- optional feature 可以明确探测；
- portable host 只暴露稳定 interfaces；
- engine-coupled interfaces 可以单独标记 exact build。

首批候选：

| Interface | 责任 |
| --- | --- |
| `core.log@1` | structured log 与 package context |
| `core.diagnostics@1` | source／schema／runtime diagnostic records |
| `ecs.batch@1` | column／entity batch view |
| `ecs.command-buffer@1` | structural mutation 与 deferred writes |
| `messages@1` | typed message publish／consume |
| `assets@1` | stable asset ID、opaque handle 与 readiness |
| `tasks@1` | host-owned jobs、cancellation 与 completion |
| `render.feature@1` | semantic render feature registration |
| `render.command-list@1` | validated compact render commands |

具体 interface 只有在第一个 consumer／provider 都存在时冻结；这张表是设计分区，不是宣告所有 API 已完成。

## Registration 與 callback 綁定

dynamic library 不在 entry 中临时「注册想要的系统」。注册真相来自 build-generated manifest：

1. package kernel 读取所有 manifests；
2. 分配 numeric IDs、验证 schema／capability、编译 schedule；
3. 产生 closure-wide `RegistrationImage`；
4. loader 打开 library，取得 entry response；
5. 比对 manifest hash；
6. 逐 callback key 验证 signature ID、required interface 与 function pointer；
7. 全部通过后才建立 module instance／Bevy bridge systems。

callback key 是 stable manifest-local ID，不是 symbol name。static adapter 解析成 typed function，dynamic adapter 解析成 C function pointer，但二者必须覆盖同一组 required callbacks。

## Component 與資料 Layout

第一版明确区分三种资料：

### ABI-POD

可以直接作为连续 column 借用：

- fixed-width integer／float；
- 由 SDK 明确排列的 nested POD；
- 没有 pointer、drop glue、niche-dependent enum、Rust `bool`／`char` 或 allocator ownership；
- manifest 固定 size、alignment、field offset、endianness 与 layout hash。

schema version 与 ABI layout version 分开。新增 persistence optional field 不必然表示同一运行时 column layout 可原地兼容。

### Opaque host value

Mesh、Image、Asset、task、GPU resource、复杂 collection 与 host service 使用 generational handle：

```text
handle = { table_id, slot, generation }
```

host 每次使用都验证 table／generation／permission。handle 只在其声明的 EngineInstance／lifetime 内有效，不可持久化。

### Serialized／message payload

跨 tick 储存、异步边界或低频复杂资料可使用 versioned byte payload／schema codec。它适合 command、event、asset metadata，不适合每 entity 每 frame 解码。

第一版不允许任意第三方 Rust type 直接成为 dynamic zero-copy component。新增 layout class 需要独立 conformance 与 migration 规则。

### Bevy 註冊與跨 package typed access

host 在建立 `RegistrationImage` 时把 stable component ID／layout hash 映射到当前 `World` 的 Bevy `ComponentId`。这个 numeric ID 是 process-local 实现细节，绝不写入 package lock、save 或 ABI artifact。

- `HostTyped` component 已由 host／shared-schema crate 编译并以 typed Bevy API 注册；static system 保留正常 `Query` 与编译器优化。
- `GeneratedSharedSchema` 允许 schema crate 编入 host，而 gameplay implementation 仍部署为 dynamic realization。
- `RuntimeDynamic` component 由已验证的 POD descriptor 动态注册，只对 batch／untyped adapter 可见。

动态注册机制不会让预编译 static consumer 获得未知 Rust type。若 static package 声明 typed dependency，`BuildPlan` 必须纳入相同 generated schema crate 并重建、改选相容 realization、生成明确的 untyped adapter，或拒绝 compose。禁止把 layout 恰好相同当作 Rust type identity，也禁止 dynamic module 提供 component drop function。

## 批次 ECS 呼叫

### Call shape

```text
LaxSystemCall
├── header / signature ID
├── EngineInstance / tick / stage
├── system numeric ID
├── batch_count
├── batches[]
├── command sink
├── message / service interfaces
└── scratch arena

LaxBatchView
├── entity_count
├── entity handles or stable local IDs
├── column_count
└── columns[]

LaxColumnView
├── component numeric ID
├── layout hash
├── data pointer
├── count / stride / element size / alignment
└── read / write / optional flags
```

一个 bridge system 每次处理所有匹配 batches，或按有上限的大批次分段。禁止 host 对每个 entity 呼叫 module；也禁止 module 对每个字段反向调用 getter／setter。

### 借用規則

- pointer 只在 callback dynamic extent 内有效；
- read-only column 不得写入；
- writable column 的别名规则由 manifest／host 保证，module 必须遵守；
- module 不可缓存 pointer、slice 或 Bevy entity；
- host 可以为不满足 ABI layout 的资料提供 packed staging view；
- callback 返回前 host 验证 status／command buffer；必要时拒绝整批结果。

### Structural mutation

spawn、despawn、add／remove component、跨 archetype move、asset create 与 world command 经 command buffer：

1. module append fixed header + versioned payload；
2. host 检查 numeric ID、schema、handle generation、permission 与 size；
3. 在已声明 schedule barrier 依 deterministic policy 套用；
4. 失败产生 package-owned diagnostic，不让 foreign code留下半套 ECS mutation。

普通 ABI-POD writable column 可在 callback 内原地更新；是否允许 rollback／transaction 由 system policy 决定。

## Static Adapter 保留的性能

同一 SDK input 产生两个不同调用形态：

```text
business declaration
├── static glue
│   └── typed Bevy system → direct query / inlining / LTO
└── dynamic glue
    └── extern C callback → batch views / command buffer
```

static system 不先把 Bevy query 打包成 `LaxBatchView` 再调用自己。equivalence 发生在 stable ID、registration、可观察状态与测试 fixture，不要求机器指令路径一致。

若作者希望同一算法主体最大程度共享，可由 SDK 生成两层：

- 纯资料核心函数，接受 SDK 提供的 generic／slice-like view；
- static／dynamic 两个薄 adapter。

这仍必须由 codegen／compile-time check 保证，而不是手写复制。

## 記憶體與字串

- 谁分配，谁释放；每个 owned buffer 带同 allocator domain 的 release function。
- 高频临时输出使用 host-provided scratch arena；callback 返回即失效。
- UTF-8 用 pointer + byte length，不默认 NUL terminated。
- 不跨边界移动 Rust `String`／`Vec`／`Box`。
- size／count／stride 的乘法必须 checked，loader／host 拒绝 overflow。
- alignment 不符合时不得直接 cast；使用 SDK-generated accessor 或 staging copy。

## 錯誤、Panic 與診斷

所有 ABI function 返回小型 `LaxStatus`；丰富错误写入 host diagnostic sink：

```text
DiagnosticRecord
├── severity / stable code
├── package / realization / callback
├── interface / ABI / schema context
├── short message
├── structured fields
└── optional source span / remediation
```

- Rust SDK 在 exported function 外围 `catch_unwind`，禁止 unwind 穿越 C boundary；
- C／C++ module exception 同样必须在自己的 adapter 内终止；
- panic／exception 后 instance 是否可继续由 callback policy 决定，默认进入 failed／stop；
- module 不可把 pointer to thread-local error string 交给 host长期保存；
- invalid foreign memory 可能直接终止进程，这再次说明 native code 不是 sandbox。

## Instance 與 Library 生命週期

```text
library load (once per artifact, retained)
  → entry / descriptor negotiation
  → create_instance(EngineInstance context)
  → bind callbacks / install systems
  → start
  → callback / task / message activity
  → quiesce
  → stop
  → destroy_instance
  → library remains mapped in v1
```

规则：

- world／App state 存在 per-instance context，不在 writable process global；
- module-created task 要绑定 instance cancellation token；
- `stop` 后 host 不发新 callback，module 必须停止产生 command／message；
- outstanding callback／task／GPU command 完成前不 destroy；
- v1 永不 `FreeLibrary`／`dlclose`；新 closure 使用新 process／EngineInstance。

即使未来支持 unload，也必须先证明 function pointer、TLS、foreign thread、GPU fence、asset、schema owner 与 persisted entity 都已清空，不能只增加一个 `unload()` 函数。

## Threading 與 Reentrancy

manifest／interface flags 明确声明：

- main-thread only；
- host-worker safe；
- single-instance serialized；
- concurrent batches allowed；
- callback may re-enter interface；
- interface function may block／schedule only。

默认最保守：module callback 不重入、instance 内串行、不能阻塞。只有 conformance／race tests 后才放宽。host-owned task 以 completion message／command 回到 schedule barrier，不让 background foreign code直接修改 Bevy World。

## Render ABI

portable dynamic render code不取得 wgpu device／Bevy RenderWorld。它可以：

1. 注册 `RenderData` schema；
2. 声明 `RenderFeature`、semantic input／output slots 与 resource access；
3. 在 batch callback 中产出 compact `RenderCommandList`／buffer payload；
4. 由 host 验证 handle、bounds、resource state 与 feature support，再翻译为 Bevy render systems／resources。

`EngineCoupledNative` 可以查询 exact-build render interface，但仍使用 opaque handles／C tables，不传 Bevy Rust type。必须直接写 `RenderApp` 的扩充使用 `NativeStatic`。

## 版本與升級流程

### Portable module

1. host 读取 required ABI／interfaces；
2. 若 major 与 minimum minor 均满足，继续验证 manifest／schema；
3. Bevy 版本只写 diagnostic metadata，不直接判定失败；
4. 行为 conformance fixture 决定 host 是否仍实现相同 Lattice contract。

### Engine-coupled module

1. 先比对 `EngineBuildId`；
2. 不同则无需尝试内部 interface，直接提示精确重建；
3. 相同仍要完成 ABI／manifest／artifact 验证。

### ABI evolution

- append-only optional field：minor；
- changed ownership／meaning／required field：major；
- 新独立服务：新 interface ID；
- 旧 major 的 support window 由明确政策与 fixture 决定，不在 loader 写猜测性 shim。

## Conformance 與故障注入

ABI test kit 至少包含：

- C reference module 与 Rust SDK module；
- old-binary fixtures（不可在 test 中偷偷重编）；
- static／portable／engine-coupled equivalence package；
- unknown tail field／short struct／wrong magic／wrong major；
- optional interface missing／required interface missing；
- wrong manifest／artifact／layout hash；
- bad count／stride／alignment／UTF-8／handle generation；
- panic／error／timeout／callback-after-stop；
- task cancellation、quiesce 与 repeated EngineInstance create／destroy；
- static direct、dynamic batch、per-entity反例的微基准与真实 gameplay benchmark。

ABI `1.0` 前，CI 必须能用至少前两个 ABI `0.x` fixture 清楚证明「可加载」或「因哪条规则被拒绝」。

## 明確禁止

- `extern "Rust"` 作为分发 ABI；
- 跨边界 `String`、`Vec`、trait object、panic、Bevy type；
- 逐 entity／逐 voxel FFI；
- foreign callback 任意保存 World pointer；
- ABI minor 改变既有 field 语义；
- 以相同 package SemVer 掩盖 breaking ABI／schema change；
- 将 native ABI 验证描述为安全 sandbox；
- v1 native hot unload。

## 驗收

- SDK 从单一 schema 产生 C header、Rust bindings、manifest 与 ABI inspector metadata。
- portable fixture 跨 Bevy host upgrade 不重编仍通过；engine-coupled fixture 精确拒绝并在重编后通过。
- dynamic gameplay 每 system／batch 跨 ABI，性能预算与 allocation profile可观察。
- invalid artifact 在任何业务 callback 前失败；失败诊断包含 package、realization、expected／actual version 与修复动作。
- static realization benchmark 未经过 C table，并能从 LTO／inline 获益。

## 外部依據

- [Rust type layout](https://doc.rust-lang.org/stable/reference/type-layout.html)
- [Rust linkage](https://doc.rust-lang.org/reference/linkage.html)
- [Bevy dynamic linking discussion](https://github.com/bevyengine/bevy/discussions/24010)

## 相關文件

- [套件內核](package-management.md)
- [模組組合](module-composition.md)
- [渲染架構](rendering.md)
- [版本與相容性](versioning-and-compatibility.md)
