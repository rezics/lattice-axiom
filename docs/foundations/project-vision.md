---
title: 專案願景與設計支柱
status: exploration
type: overview
updated: 2026-08-19
---

# 專案願景與設計支柱

## 一句話願景

Lattice Axiom 是一款以可演进体素世界、可组合套件生态与长期世界状态为核心的游戏：它以 Nickel／SemVer 定义游戏闭包，以静态与动态双 realization 交付内容，并建立在 Bevy 的完整上游引擎能力之上。

项目不以重做通用游戏引擎为目标；package identity、组合、ABI、world semantics 与 persistence 则是产品本身，不能被降级成未来再补的外围工具。

## 最高工程原則

> 默认使用完整上游能力；只有当一个真实可玩的原型证明现有方案无法满足不可妥协的产品需求时，才允许自研替代。

这条原则原文是所有 upstream 例外申请的硬门槛。它要求我们直接采用 Bevy 的 App、ECS、schedule、render、asset、input 与 tasks，也要求 package kernel／ABI 尽量采用 Nickel、Cargo、平台 C ABI 与成熟库。它不表示放弃 Lattice Axiom 独有的产品契约。

## 設計支柱

### 游戏是一个已锁定套件闭包

每次 client、server、headless test 与 tool 启动，都对应一个可审查的 `LockedGameGraph`。Terrenia、其他第一方内容、测试内容与第三方模组拥有相同 root／scoped package name、SemVer、dependency、capability、schema 与 conflict rules；不存在第一方私有加载捷径。

### 静态与动态是 realization，不是两个生态

同一逻辑 package 可静态编入 host，也可由已验证的动态 artifact 加载。二者共用 registration manifest、stable ID、schedule 与持久化语义；静态直接使用 Rust／Bevy 与 LTO，动态使用批次 C ABI。统一不以牺牲静态性能为代价。

### Bevy 是引擎，也是核心 package 的内部工具

游戏内部与 static realization 善用 Bevy type，不建立逐项镜像 facade。portable dynamic contract 则属于 Lattice Axiom，不能暴露 Bevy／Rust ABI。Bevy 升级通常只是 core host 的受控迁移；若外部行为真的破坏，必须在相关 package／capability／schema owner 上诚实版本化。

### 玩家价值与基础契约同步验证

第一个纵切同时证明两件事：

1. 玩家能进入程序体素世界、挖掘、放置并跨重启保存；
2. 一个真实 gameplay package 可由同一业务代码生成 static／portable dynamic realization，并得到相同注册与权威结果。

只做 package manager demo 不够；先让 Terrenia／第一方内容绕过 graph 做完游戏也不够。正确顺序是最窄基础闭环立即服务真实玩法。

### 只自研产品差异层

Lattice Axiom 自行定义：

- Nickel package／profile contract 与 Rust package kernel；
- package SemVer、capability、realization 与 native ABI；
- stable content／schema identity、独立 registration namespace 与 first-party／external parity；
- 权威体素区块、世界生成、provenance 与持久化；
- render feature／provider 的可组合产品契约；
- 无限世界精度、串流与玩家体验预算。

Bevy 提供 App、ECS、scheduler、renderer、window、input、assets、task pools、UI 与 diagnostics；通用物理优先采用 Bevy 生态。

### 作者描述语义，系统产生机制

作者声明 package、依赖、capability、component／message schema、system access、render slot 与 fallback。package kernel 验证并编译 closure；Bevy host 执行。模组不靠 load order、method patch 或私有 type 猜测另一个模组行为。

精确 StableId 只回答对象身份；SemanticTag／Map、StateProperty、Affordance、Predicate 与 Role
分别表达集合、带值资料、实例状态、上下文行为、判定和 concrete candidate选择。作者通过
Nickel functions／contracts／overlay组合这些语义，Rust把全图编译进 `RegistrationImage`。
Terrenia 只是使用同一机制的普通内容聚合模组，不是平台 vocabulary 或 host default。

### 权威资料与可重建资料分离

已物化 chunk、玩家修改、persistent entity 与必要续行状态是权威资料。Mesh、GPU resource、physics broadphase、runtime handle 与 cache 都可重建。Bevy ECS 是 active working set，不是永久存档格式；ABI handle 也不能持久化。

### 相容性按 owner 分域

package SemVer、capability／ABI、`EngineBuildId`、schema、stable content ID、generator revision 与 asset format 各自版本化。没有全域 `engineVersion` 能替代实际相容判断。

## 首階段核心與延後規模

### 首階段核心

- Nickel composition 与 typed `CompositionSpec`；
- SemVer graph、capability／realization selection 与精确 lock；
- SDK／proc macro 与 `RegistrationManifest`；
- `NativeStatic` + `PortableNative` ABI `0.x`；
- registration／artifact／ABI validation；
- semantic Tag／Map／Predicate／Role 与 locked fallback bundle；
- Bevy host adapter 与可玩／持久化纵切；
- render feature／provider 最小组合 fixture。

### 延後規模／營運能力

- public／federated registry；
- 通用大型 graph resolver；
- marketplace／自动更新 UI；
- 多平台预建 farm 与分散式 cache；
- native hot unload；
- 不可信 WASM／process ecosystem；
- 完整视觉 world editor。

延后这些项目不会延后核心 package identity、ABI 或统一 graph。

## 目前尚未形成的產品定義

- 长期核心循环、世界观与社交形态；
- 第一目标 OS／GPU／server matrix；
- 对第三方 native code 的发布／信任政策；
- ABI 1.0 的性能与 support window；
- 哪些 component 可成为 ABI-POD；
- creator tooling 与 registry 的最小可用体验；
- 哪些 rendering mechanism 应成为 stable provider capability。

这些问题集中在[待决问题](../planning/open-questions.md)，由 vertical slice、old-binary fixtures 与创作者测试回答。

## 相關文件

- [決策 0010：Nickel 套件系統](../decisions/0010-nickel-driven-package-system.md)
- [決策 0014：Bevy／上游優先](../decisions/0014-adopt-bevy-upstream-first.md)
- [決策 0017：原生模組 ABI](../decisions/0017-versioned-native-module-abi.md)
- [決策 0018：首階段交付套件內核](../decisions/0018-package-kernel-from-first-vertical-slice.md)
- [決策 0020：語義註冊與內容選擇](../decisions/0020-semantic-registration-and-content-selection.md)
- [語義註冊架構](../architecture/semantic-registration.md)
- [開發策略](development-strategy.md)
- [執行期架構](../architecture/game-engine-runtime.md)
