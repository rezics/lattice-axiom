---
title: 套件模組、註冊清單與靜動雙實現
status: proposed
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0008-static-and-dynamic-realizations-share-one-graph.md
  - ../decisions/0010-nickel-driven-package-system.md
  - ../decisions/0017-versioned-native-module-abi.md
  - ../decisions/0018-package-kernel-from-first-vertical-slice.md
  - ../decisions/0019-separate-package-and-registration-identities.md
  - ../decisions/0020-semantic-registration-and-content-selection.md
---

# 套件模組、註冊清單與靜動雙實現

## 結論

Lattice Axiom 的模组单位是逻辑 package，不是 Cargo crate、Bevy Plugin 或动态库。一个 package 可有多种 realization；它们共用 package graph、stable ID、schema、capability、registration 与 schedule 语义，却在 host 呼叫层选择最合适的路径：

- 静态：生成 typed Rust／Bevy glue，保留 inline、LTO、generic 与完整 system parameter；
- 动态：生成 C ABI callback／batch adapter，经 versioned capability table 进入 Bevy；
- 纯资料：生成 validated registry／asset entries，不加载 code。

同一份业务声明由受控 SDK／proc macro 产生这些 adapter。禁止作者维护「静态 API 版」与「动态 API 版」两套意义相近但会漂移的注册代码。

## 三層模型

```text
logical package
├── identity / SemVer / dependencies / capabilities
├── schemas / stable IDs / assets
└── realization candidates
      ├── NativeStatic
      ├── PortableNative
      ├── EngineCoupledNative
      └── data
             ↓ generated RegistrationManifest
      closure-wide RegistrationImage
             ↓ host adapters
          one Bevy App
```

### Logical package

回答「这是什么、依赖什么、提供什么、拥有哪项长期语义」。package version 不因选择 static／dynamic 自动改变。

### Realization

回答「在这个 target／profile／trust policy 下如何取得可执行或资料 artifact」。realization 可以有自己的 artifact hash、ABI requirement 与 exact build constraint，但不能改变 package 对外语义。

### Host adapter

把已验证的 registration image 安装到 Bevy。adapter 不是第二个 engine：static adapter 注册正常 Bevy systems；dynamic adapter 注册少数 bridge systems，它们在相同 Bevy schedule 位置调用 foreign batch callback。

## Package 類別不是生命週期

package 可以依职责分类，但全部遵守同一 graph／registration：

| 类别 | 典型内容 |
| --- | --- |
| foundation／domain | 世界、内容 registry、持久化、玩法语义与公开 stages |
| mechanic | 伤害、掉落、方块互动、流体、生成规则 |
| content | 方块、物品、群系、entity archetype 与资产 |
| provider | terrain、visibility、render backend／pass 等 exclusive capability |
| observer／feature | 多个可组合 post effect、诊断、消息 consumer |
| tooling | 仅 development profile 的 inspector、gizmo、visualizer |

分类不会创造另一套启动 API；差异由 dependency、capability、profile condition 与 registration items 表达。

## 受控 SDK 與 proc macro

SDK 提供稳定的 Lattice-owned business surface，并生成 static／dynamic adapter。概念输入如下：

```rust
#[latticeaxiom::package(name = "@example/marker")]
mod marker {
    #[component(id = "example:component/marked", schema = 1)]
    pub struct Marked { pub strength: f32 }

    #[system(stage = "gameplay.fixed", reads(Marked), commands)]
    pub fn decay(batch: Query<(&mut Marked,)>, commands: Commands) {
        // one business implementation; exact author API remains to prototype
    }
}
```

这段只表达目标体验，不冻结 macro syntax。codegen 产物至少包括：

1. canonical `RegistrationManifest`；
2. static Bevy system／registration glue；
3. dynamic C header／Rust binding 与 batch shim；
4. callback key map；
5. schema／layout descriptor 与 conformance fixtures；
6. manifest hash／producer metadata。

若某项业务逻辑使用只能静态实现的 Bevy system parameter，SDK 必须在 build 时把 realization 标成 `NativeStatic only`，不能生成表面可编译、实际不安全的 dynamic adapter。

## `RegistrationManifest`

manifest 是 code activation 前可读取的纯资料契约：

```text
RegistrationManifest
├── package / realization / producer
├── stable content IDs
├── components / resources / messages / assets
│   ├── schema owner + version
│   ├── ABI layout kind
│   └── persistence / replication / presentation flags
├── systems
│   ├── callback key
│   ├── stage / set
│   ├── reads / writes
│   ├── before / after
│   └── thread / command policy
├── capabilities required / provided
├── semantic registration
│   ├── Tag / Map definitions and contributions
│   ├── StateProperty / Affordance contracts
│   ├── Predicate / Role / offers
│   └── ContentBundles / fallback guards
├── render features / slots / fallback
└── canonical hash
```

manifest 不包含 process-local Bevy `TypeId`、`Entity`、`Handle`、function address 或动态库 base address。

pure-data semantic rows 可以来自完全求值的 Nickel registration fragment；code-bound
schema／system／Affordance callback由 SDK 生成 fragment。两者进入同一 canonical manifest，
同一 stable row key若重复但不完全相同必须失败，不能让 Nickel 与 Rust annotation成为两份真相。

## Bevy Component 註冊模式

逻辑 component 的 stable ID 与 schema 可以跨 realization 共用，但「host 是否在编译时认识对应 Rust type」必须明确区分。`RegistrationImage` 为每个 `EngineInstance` 维护：

```text
stable component ID
  → schema / ABI layout descriptor
  → process-local Bevy ComponentId
  → permitted typed / batch / serialized access modes
```

Bevy `ComponentId` 只在当前 process／`World` 有意义，不能进入 lock、save、network protocol 或动态 ABI。首阶段支援三种注册模式：

### `HostTyped`

component type 已编进 host 或 core shared-schema crate，由 static glue 以 typed Bevy API 注册。static systems 使用正常 typed query；dynamic bridge 只有在 SDK 生成的 ABI-POD descriptor 与 host layout hash 完全一致时，才可借用对应 column。

### `GeneratedSharedSchema`

公开 schema 由独立、生成的 schema crate／artifact 表达，不与某个 gameplay implementation realization 绑定。`BuildPlan` 可把 schema crate 编入 host，同时让实现本身选择 static 或 dynamic。这样需要 rebuild 的 static consumer 仍可取得 typed Rust component，而不必把 dynamic implementation 一并静态链接。

### `RuntimeDynamic`

host 从已验证 manifest 取得 POD size、alignment 与 layout descriptor，再使用 Bevy 的 dynamic component registration 建立 process-local component。它可供 dynamic／untyped bridge 或 stable-ID adapter 使用，但**不能让已经编译完成的 static binary 凭空获得一个新的 Rust type**。

若 static consumer 要 typed access 一个在 compose 时才出现的 component，planner 必须选择以下其一：

1. 把该 component 的 generated shared-schema crate 纳入 host 并重建；
2. 选择具有同一 schema 的相容 static realization；
3. 使用 SDK 生成的 stable-ID／untyped adapter，明确接受其能力与成本；
4. 因无法满足 typed requirement 而在 activation 前拒绝 graph。

首个 native ABI 只允许无 drop glue 的 ABI-POD 走 `RuntimeDynamic` zero-copy。复杂 component 必须是 host-owned typed value、opaque handle 或 serialized／message payload，不能让第三方动态库提供 Rust destructor。这个限制既保护 Bevy storage，也让 static／dynamic 等价性成为可测试事实，而不是由名称暗示。

## Stable ID 與 numeric ID

稳定 ID 例：

```text
terrenia:block/stone
terrenia:item/stone
rezics:component/health
example:system/decay
example:render-feature/outline
```

规则：

- stable ID 的 namespace owner／schema owner 由显式授权与 manifest 记录，不从 package name 或 Rust type name 推导；
- registration row 另存 `declared_by: PackageName`，但 provider package 不成为 stable ID 的语法组成；
- duplicate ID 一律在 activation 前失败，不能 last-writer-wins；
- numeric ID 从完整 `LockedGameGraph`／manifest set 以规范规则产生，或由 snapshot local palette 保存；
- numeric ID 不依 source discovery、Cargo link、plugin add 或 dynamic load 顺序；
- realization 切换不改变 stable ID、schema owner 与 normative numeric mapping。

## Exact Registration 與 Semantic Catalog

exact ID 冲突规则不应用于 extensible semantic contribution。多个 package 可以向同一个公开
Tag 添加各自拥有的 entry，却不能重复定义 Tag contract。package kernel在numeric ID之后／
code activation之前编译：

```text
exact stable IDs
  → typed Tag bitsets / SemanticMap tables
  → StateProperty and Affordance signatures
  → checked Predicate plans
  → concrete Role bindings
  → active ContentBundle set
```

输入／结构匹配使用 Tag／Predicate，输出／world command使用已经解析的Role target。材料Tag
不会自动赋予机制行为；context behavior使用机制owner的Tag／Map或versioned Affordance。
完整schema、Nickel authoring与fallback算法见[语义注册架构](semantic-registration.md)。

## Schedule 編譯

package 不声明任意 Bevy schedule internals，而是对齐少量 versioned domain stages／sets，例如：

```text
input.sample
gameplay.fixed.pre
gameplay.fixed
physics.integrate
world.commands.apply
world.revision.commit
persistence.capture
presentation.update
```

每个 system manifest 声明 stage、read／write set、显式 correctness edge 与 thread policy。package kernel：

1. 验证 stage／interface version；
2. 合并显式 dependency 与 capability provider edge；
3. 检测 cycle、unknown owner 与 ambiguous exclusive order；
4. 产生 deterministic schedule plan；
5. 让 static system 与 dynamic bridge system 安装到相同 Bevy set。

普通资料冲突仍交给 Bevy scheduler 安排平行度；Lattice 不复制 ECS scheduler。manifest read／write set 主要用于 ABI batch 准备、诊断、静态验证与确定语义 barrier。

## 靜態 realization

`NativeStatic` 由 package source、generated glue 与 core host 共同经 Cargo build：

- static callback 是直接 Rust function／Bevy system；
- 可以使用 typed query、resource、event、asset 与 render extension；
- compiler 可进行 monomorphization、inline、LTO 与 specialization；
- package 仍不能绕过 manifest 注册 stable ID／schema／capability；
- `.rlib`／Rust symbol 不对外承诺稳定；只分发 source 或 exact-build cache artifact。

因此「静态与动态共用一份业务代码」不会抹掉静态性能优势。共用的是 codegen 输入与 observable contract，不是强迫静态呼叫 C function table。

## 動態 realization

`PortableNative`／`EngineCoupledNative` 的 manifest 先被合并；host 之后加载 `cdylib`、协商 interface、比对 manifest hash，再建立 instance。

动态 system 的一个 Bevy bridge invocation 对应一个 system／多个 archetype batches，而不是一个 entity 一次 FFI。callback：

- 借用 ABI-POD column view；
- 以 opaque／generational handle 访问复杂 host object；
- 通过 command buffer 请求 structural mutation；
- 通过 versioned service tables 发送 message、载入 asset、排 task 或产生 render command；
- callback 返回后不保留 borrowed pointer。

需要任意 Bevy World 或 Rust generic API 的 package 只能 static；需要精确低层 host 能力但仍要预编译部署的 package 明确选择 engine-coupled。

## 啟動與生命週期

```text
discover / compose
  → resolve / lock
  → read manifests
  → validate / compile RegistrationImage
  → build or load artifacts
  → verify callback maps
  → create EngineInstance / Bevy App
  → create module instances
  → install adapters
  → start modules
  → load world / enter Playing
```

失败政策：

- package／range／capability conflict：不建立 runtime；
- manifest／artifact／ABI mismatch：不执行 module code；
- instance create／start 失败：以逆序 stop／destroy 已建立 instance，世界保持未开放写入；
- 运行中 callback 失败：依 system policy 禁用当前 tick／module 或安全退出，不 unwind 跨 FFI；
- v1 不卸载 native library；重组 package closure 需要新 EngineInstance／重启。

## Terrenia 與測試套件

至少维护三个 consumers：

1. `terrenia`：第一个维度的聚合 root package，依赖 `@terrenia/*` 方块／世界生成／玩法／表现 closure；
2. `@example/marker`：独立 package name 与 registration namespace 的测试 package；
3. `@example/dual-gameplay`：同一业务代码同时构建 static／portable dynamic 的 equivalence fixture。

测试必须覆盖：

- 只用 test package 启动 headless world；
- 调换 discovery／load 顺序不改变 lock、ID 与 schedule；
- static／dynamic 切换不改变 registration hash、tick state hash 与 save bytes；
- duplicate／missing ID、capability conflict 与 wrong ABI 产生稳定诊断；
- Tag cycle／Map conflict／Role ambiguity／fallback竞争产生带source provenance的稳定诊断；
- Terrenia普通内容与另一个测试维度可互换，host没有`terrenia:*` special case；
- 移除 package 时遵守 missing-content／schema migration policy。

## 不建立的平行系統

package kernel 不拥有：

- 第二个 ECS／World；
- 第二个 frame／fixed scheduler；
- 第二个 asset dependency graph；
- 第二个 renderer／render graph；
- per-tick package lookup；
- 任意 native hot unload；
- 第一方 package 私有 registration shortcut。

## 驗收

- 所有 App profile 都由 `LockedGameGraph`／`RegistrationImage` 建立。
- 同一业务 package 能由 SDK 产生 static／dynamic realization，不复制业务函数。
- static benchmark 保留 direct Bevy call／LTO 路径；dynamic benchmark按批次而非逐 entity。
- manifest 足以在不加载 code 的情况下发现 ID、schema、schedule、capability 与 render conflict。
- package activation 完成后，tick 不查询 Nickel／resolver，且 dynamic code 不直接取得 Bevy World。

## 相關文件

- [套件內核](package-management.md)
- [語義註冊、內容判定與選擇](semantic-registration.md)
- [原生模組 ABI](native-module-abi.md)
- [Bevy 執行期](game-engine-runtime.md)
- [版本與相容性](versioning-and-compatibility.md)
