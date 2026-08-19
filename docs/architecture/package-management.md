---
title: Nickel 驅動的套件內核與分發邊界
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

# Nickel 驅動的套件內核與分發邊界

## 結論

Lattice Axiom 的 package kernel 从第一個垂直切片起就是唯一组合控制平面。Nickel 描述 package、profile、overlay、semantic registration 与 realization intent；Rust 负责 SemVer／capability 解析、全图 semantic validation、lock、build、加载与 Bevy activation。

它不是第二个 ECS、scheduler、renderer 或 asset server。所有已解析模组最终由 host adapter 安装进一个正常的 Bevy App；Bevy 提供通用引擎能力，package kernel 决定**哪些逻辑套件、哪些版本、以哪种 realization、携带哪些权限与 schema**构成这次游戏。

## 核心管線

```text
package.ncl / game.ncl / overlays / latticeaxiom.lib
                         │
                         ▼
             Nickel merge + contracts
                         │ direct typed conversion
                         ▼
                  CompositionSpec
                         │ source + SemVer + capability resolution
                         ▼
                 LockedGameGraph
                         │ realization and target planning
                         ▼
                     BuildPlan
             ┌───────────┴───────────┐
             ▼                       ▼
   static source/glue          data/dynamic artifacts
             └───────────┬───────────┘
                         ▼
          RegistrationImage + RuntimeImage
                         │ Bevy host adapter
                         ▼
                      Bevy App
```

没有任何第一方 `add_plugins(...)` 清单可以绕过这条管线成为第二份真相。测试可以用 in-memory source 与预构建 fixture 缩短 I/O，但仍产生相同语义型别与验证结果。

## Package 名稱與註冊 ID

Package identity 与 runtime／persistent registration identity 是两个正交坐标：

```text
PackageName("@terrenia/blocks")
StableId("terrenia:block/stone")
```

`PackageName` 使用 root `name` 或 scoped `@scope/name`，负责 SemVer、依赖、来源、
分发与 realization。`terrenia` 是聚合根，`@terrenia/blocks` 是其 scope 下的子包。
`StableId` 使用 `<namespace>:<kind>/<path>`，负责内容、维度、schema、system、
capability 与 render contract 的长期引用。manifest 必须直接声明完整 stable ID；
package kernel 只另外记录 `declared_by`，不得从 package name、source path 或 crate name
拼出注册 key。namespace 的使用权由显式 grant 验证。

完整规范见[决策 0019](../decisions/0019-separate-package-and-registration-identities.md)。

## 套件模型

每个逻辑 package 至少声明：

| 字段 | 语义 |
| --- | --- |
| `name` | root／scoped package 身分，例如 `terrenia`、`@terrenia/worldgen` |
| `version` | package 的 SemVer |
| `dependencies` | package name、SemVer range、optional／feature 条件 |
| `requires`／`provides` | versioned capability 与 cardinality |
| `realizations` | data、`NativeStatic`、`PortableNative`、`EngineCoupledNative` 等候选 |
| `schemas` | 持久化 component／message／asset schema owner |
| `content` | stable content IDs 与资产根 |
| `semantics` | Tag／Map contribution、StateProperty、Affordance、Predicate、Role 与 ContentBundle intent |
| `settings` | runtime `SettingSpec` registrations；graph-affecting选择仍放`parameters` |
| `observability` | info／metric、target inspect fragment与debug visualizer registrations |
| `parameters` | 由 Nickel contract 验证的纯资料输入 |
| `trust` | realization 所需信任等级与主机 capability policy |

概念上的 Nickel profile：

```nickel
let la = import "latticeaxiom/lib.ncl" in

la.GameProfile & {
  packages = [
    { name = "terrenia", version = "~0.1", realization = 'auto },
  ],
  semantic.bindings = {
    "latticeaxiom:block-role/storage-block/copper@1"
      = "terrenia:block/copper-block",
  },
  overlays = [import "./local-world.ncl"],
}
```

这只是语义示例，不冻结最终 Nickel API。最终 contract 必须由 `latticeaxiom.lib` 与 Rust conformance fixtures 共同定义。

## Nickel 與 Rust 的邊界

### Nickel 負責

- package／profile 的 record、function、defaults 与 overlay；
- contract validation 与 source-aware diagnostics；
- 纯资料参数化、capability／realization intent；
- material family／structure helper、semantic Predicate AST 与 Role／fallback intent；
- additive fragments 的显式 `concat`／`add`，以及 keyed binding 的 `default`／overlay merge；
- 构造完整且可序列化的 `CompositionSpec`。

### Rust package kernel 負責

- source fetch／path normalization／content hash；
- SemVer range、pre-release 与唯一版本政策；
- dependency／capability graph、cycle、exclusive provider 与 conflict；
- semantic target kind、Tag DAG、typed Map merger、Role cardinality 与 fallback activation；
- trust policy、target／profile 与 realization selection；
- lock、artifact store／cache、Cargo build 与 dynamic load；
- registration manifest 验证、numeric ID、schedule compilation；
- lifecycle、诊断与 Bevy host activation。

Nickel evaluator 不直接访问网络、时钟、随机数或 compiler。package kernel 先准备受控 source root，再在资源上限内求值。Nickel function可构造 semantic资料，但求值后的 function／source AST 不进入 `CompositionSpec`；普通 record merge也不被误用来隐式拼接 additive arrays。

## 五個語義產物

### `CompositionSpec`

保留使用者意图与已通过 contract 的 package request、range、overlay、feature／parameter、realization preference、semantic bindings／bundle intent 与 policy。它不含已下载 artifact、numeric ID、未求值 Nickel function 或 loaded function pointer。

### `LockedGameGraph`

保存确定的 package version、source identity、content hash、dependency edge、capability provider、schema owner 与 resolution explanation。相同输入与 source universe 必须产生确定结果。

### `BuildPlan`

为每个节点选择 realization、target、profile、toolchain、预建 artifact 或 source build，并记录：

- `NativeStatic` glue／Cargo unit；
- dynamic ABI／required interface range；
- `EngineBuildId`（仅 engine-coupled）；
- asset processing 与 manifest generation；
- artifact dependency 与预期 hash。

### `RegistrationImage`

把所有 package 的 `RegistrationManifest` 合并为 closure-wide 的稳定／numeric ID table、component／message layout、system stages、read／write set、ordering、capability、render slot、schema owner、`SettingsCatalog`、observability catalog与`SemanticCatalog`。它保存展开后的Tag／Map、compiled Predicate、Role binding、active bundle与provenance，并在 code activation 前完成冲突验证。

### `RuntimeImage`

保存这次 EngineInstance 实际可执行的 static direct functions、dynamic callback tables、asset roots、instance factories 与 lifecycle state，并指向完全匹配的 `RegistrationImage`。它不保留 Nickel AST 或未解析 range。

## SemVer 與解析政策

SemVer 是逻辑 package 的外部版本语言。首阶段不需要先实现公开 registry 或通用求解器规模，但必须真实处理：

- exact／caret／tilde／bounded ranges 与 pre-release 规则；
- 一个 closure 内同 package ID 的唯一版本政策；
- optional dependency／feature activation；
- versioned capability provider 与 exclusive／multiple cardinality；
- cycle、missing provider、conflicting provider 与 realization unavailable；
- deterministic tie-break 与完整 resolution explanation；
- 精确 lock，后续启动默认不重新选择较新版本。

首阶段可以采用简单、可解释的 deterministic resolver；当真实 graph 证明 backtracking 成本或错误质量不足时，再引入 PubGrub／SAT 类实现。**延后一般求解器不等于延后 SemVer、lock 或 conflict semantics。**

## Capability 模型

dependency 表达「我需要某个 package 的公开 contract」；capability 表达「我需要一个提供者」。两者不可混为模糊字串 hook。

每个 capability 至少包含 stable ID、interface major／minor range、cardinality 与 profile／target 条件：

```text
latticeaxiom:capability/render-terrain-mesh-source@1  exactly-one
latticeaxiom:capability/render-visibility@1           exactly-one
latticeaxiom:capability/render-post-effect@1          zero-or-more
latticeaxiom:capability/worldgen-terrain-provider@2   exactly-one
latticeaxiom:capability/gameplay-damage-observer@1    zero-or-more
```

exclusive provider 冲突必须在 `LockedGameGraph` 阶段失败；multi-provider 的确定顺序来自显式 dependency／priority policy，不来自目录或动态库加载顺序。

## Realization 選擇

| realization | 构建／加载 | 相容边界 | 适合 |
| --- | --- | --- | --- |
| data | 验证并加载 assets／definitions | package／schema | 纯内容 |
| `NativeStatic` | Cargo 从 source 编入 host | source API + rebuild | 第一方／可信 code、最大性能、完整 Bevy 能力 |
| `PortableNative` | 验证后加载 `cdylib` | stable Lattice ABI／capability | 可分发可信 native mod |
| `EngineCoupledNative` | 精确 host build 加载 | `EngineBuildId` + internal interface | 低层 engine／renderer integration |
| `WasmComponent` | 未来独立 runtime | component／capability contract | 不可信或跨平台生态；不在首阶段 |

`auto` policy 可以偏好已验证 portable artifact、否则 source static build；最终选择必须写入 lock。服务器／client profile 可以选择不同 realization 或可选 presentation package，但所有权威 package／schema closure 必须明确协商。

## Source、lock 與 artifact

首阶段来源只需支持 workspace／local directory 与测试 fixture；每个 source 都规范化并 content-addressed。lock 至少保存：

- package name／version／source／source hash；
- dependency 与 capability resolution；
- selected realization／target／profile；
- manifest schema／hash；
- ABI／interface range、`EngineBuildId`（如适用）；
- artifact hash、producer／toolchain fingerprint；
- `latticeaxiom.lib` 与核心模型版本。

`.rlib` 不作为稳定分发 artifact。static cache 只能以完整 toolchain、target、features、source 与 `EngineBuildId` 作 exact cache key；cache miss 就重建。dynamic artifact 必须与发布描述符／manifest hash 一致。

## 啟動交易

1. 读取 profile／lock；需要时在受控 source universe 重新 resolve。
2. 验证 source／artifact hash、target、trust 与签章 policy（首阶段可只信任 local）。
3. 读取所有 registration manifests，不执行模组 code。
4. 合并 ID、schema、capability、schedule、settings、observability、render与semantic contracts；编译Tag／Map／Predicate／Role，解析单轮fallback；任何冲突均停止。
5. 构建／加载 realization；dynamic entry 只能协商 descriptor／interface。
6. 验证 callback map 与 manifest hash，建立 `RuntimeImage`。
7. 创建 EngineInstance 与 Bevy App，host adapter 安装 standard plugins、static systems 与 dynamic bridge systems。
8. 执行 module `create／start`，完成 assets／world validation 后才进入 `Playing`。

任何失败都必须保持「尚未进入可修改世界」；不能加载一半后以 last-writer-wins 继续。

## 信任與安全

首阶段 native realization 都是可信 process code。package metadata 与 ABI validation 只能防止误配，不能沙箱恶意动态库。

- `NativeStatic`：构建时信任 source 与 build script。
- `PortableNative`／`EngineCoupledNative`：加载即授予进程级能力；需要来源 allowlist 与清楚 UI／日志。
- Nickel：在受控 import root 与资源限制内求值，不继承 native code 信任。
- 未来 `WasmComponent`／process isolation：另行定义 capability security、资源计量与持久化 owner。

## 首階段與延後範圍

### 首階段必須完成

- Nickel contracts 与 typed `CompositionSpec`；
- Nickel semantic constructors／contracts／overlay 与 Rust typed model conformance；
- package SemVer、local source、deterministic resolution 与 lock；
- capability／realization selection；
- SDK-generated `RegistrationManifest`；
- static／portable dynamic equivalence fixture；
- ABI／manifest／artifact validation；
- closure-wide ID／schema／schedule；
- closure-wide SemanticCatalog、role bindings、active bundle lock与compiled hot-path tables；
- package-injected SettingsCatalog与subscription-driven observability catalog；
- Bevy host activation 与 diagnostics。

### 可延後

- public／federated registry；
- 通用大型 graph 求解器；
- remote marketplace／自动更新 UI；
- 签章透明日志、组织信任 delegation；
- 多平台预建服务与分散式 artifact cache；
- native hot unload；
- WASM sandbox ecosystem。

这条边界把核心语义与规模／营运功能分开，不再把整个 package system 延后。

## 驗收

- 同一 Nickel profile 在 CLI／headless／client 得到同一 lock 与 registration hash。
- 官方与 test package 都只能经 graph 启动；不存在隐藏 plugin list。
- static／dynamic realization 可互换且不改变 stable ID、schedule、save schema 与权威 state hash。
- 随机化 source discovery／artifact load 顺序不改变结果。
- 多个语义相似内容可共存；Tag input、Role output与fallback activation均不依赖注册顺序。
- 任一package可声明typed setting／diagnostic item而不自绘设置页或HUD；ID冲突与无provider在code activation前失败。
- 用非 Terrenia root package替换维度 closure时，host与`latticeaxiom:*` semantic contracts不变。
- range／capability／ABI／artifact 冲突在启动前给出 package chain、requested／available version 与可行动修复。
- runtime hot path 不依赖 Nickel evaluator 或 resolver。

## 相關文件

- [模組與內容組合](module-composition.md)
- [語義註冊、內容判定與選擇](semantic-registration.md)
- [原生模組 ABI](native-module-abi.md)
- [版本與相容性](versioning-and-compatibility.md)
- [首階段路線圖](../planning/roadmap-game-engine.md)
- [Package 設定與配置](settings-and-configuration.md)
- [診斷、檢查與除錯可視化](diagnostics-inspection-and-debug-visualization.md)
