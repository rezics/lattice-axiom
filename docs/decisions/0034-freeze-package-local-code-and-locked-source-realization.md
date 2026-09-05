---
title: 冻结 package-local code 与 locked source realization
document_id: decision.0034-freeze-package-local-code-and-locked-source-realization
document_status: accepted
document_type: decision
tracks_implementation: false
updated: 2026-08-23
---

# 决策 0034：冻结 package-local code 与 locked source realization

## 背景

[决策 0010](0010-nickel-driven-package-system.md)、[0018](0018-package-kernel-from-first-vertical-slice.md)
与[0032](0032-freeze-local-package-acquisition-imports-and-product-lock.md)已经要求：第一方代码也必须先
成为 package graph 节点，完整 package source tree 必须进入 immutable CAS，最终产品只能从重新打开并
验证过的 lock 启动。然而现有 implementation workspace 把几乎所有 Rust 业务代码放在顶层
`crates/`，`packages/` 主要保存 manifest、Nickel 与 data；product host 又通过固定 Cargo dependency
与手写安装路径取得 settings、worldgen、gameplay 等实现。

这形成两套互相矛盾的 authority：package graph 决定声明与资料，engine Cargo graph 决定实际进入
程序的业务代码。改变 package selection 不能可靠地增加或移除对应系统；source digest 也未必覆盖
真正业务实现。这样的 build 即使产生 lock，也不能证明运行中的 code 来自该 lock。

Logical package 与 Cargo crate 是不同身份，不代表它们必须存放在互相分离的 source roots。本决策冻结
code-bearing package 的物理来源、Cargo 边界、static／portable build authority 与迁移门禁；它不要求
所有 logical package 都有 Rust code，也不把平台基础设施伪装成产品 package。

## 决策

### 1. 两类维护根，而不是两套产品真相

Implementation workspace 固定区分：

- `crates/` 保存不由某个可选择 logical package 拥有的平台实现，例如 core typed model、compose、
  package kernel、registration compiler、SDK／codegen、native ABI／loader、CAS／storage、通用 voxel
  runtime／mesh、Bevy host 与 launcher；
- `packages/<family>/<package>/` 是可选择 logical package 的完整 source root。code-bearing package 在
  同一 root 保存 manifest、Nickel、Rust source、data 与 package-owned tests；pure-data package 继续
  只有 manifest／Nickel／data；
- Terrenia-specific generator、gameplay、presentation 或 policy 属于相应 `@terrenia/*` source root；
  可跨产品复用且不拥有 Terrenia stable IDs／defaults 的算法或 host adapter 才留在平台 crate；
- platform crate 不得依赖 product package implementation。package code 只经 SDK、stable DTO、generated
  schema 或明文 host capability 依赖平台。

首版 code-bearing source root 使用下列形状：

```text
packages/<family>/<package>/
├── latticeaxiom-package.toml
├── package.ncl
├── Cargo.toml
├── src/
├── data/          optional
└── tests/         optional
```

`Cargo.toml` 是 `source-build` 的 package-root build entry。它可以是一个 crate，也可以是显式列出
package-internal crates 的 Cargo workspace；不得靠递归目录扫描发现 build units。package-internal generated
files仍是 build output，不进入 authored source root。

### 2. Logical package 与 Cargo crate 保持独立身份

- 一个 logical package 可以是 pure data、一个 crate 或数个有共同发布／版本生命周期的内部 crates；
- 一个平台 crate 可以服务多个 packages；若它开始拥有某个 package 的 registrations、defaults 或
  可替换 policy，该部分必须移入对应 package source root；
- crate name 继续遵守 `latticeaxiom-<component>`，但 crate name、workspace member path 与
  `PackageName` 之间没有隐式转换；
- 不因“一个 package 不必对应一个 crate”而允许 package implementation 留在无 provenance 的顶层
  crate。many-to-many mapping 必须由 package manifest、build plan 与 generated glue 明文证明。

Root Cargo workspace 可以把 package-local Cargo roots列为 members，供 IDE、lint、test与开发构建使用；
workspace membership只表示可编译 source universe，不表示 package 被产品选择或有 activation authority。

### 3. 完整 package source 是 source-build 的唯一输入

Code-bearing package 的 source inclusion 至少覆盖：

- `latticeaxiom-package.toml`、`package.ncl`；
- root `Cargo.toml`、被其显式引用且位于同一 package root 的 Rust manifests／`src`／build inputs；
- package data、schemas、assets 与 generated-input definitions；
- 会改变 registration、business behavior 或 artifact bytes 的 package-owned build configuration。

`target/`、CAS materialization、generated product glue 与编译产物不得进入 source inclusion。Kernel 在任何
compiler／build script执行前规范化并快照完整 source root；source code、Cargo manifest或 data 任一变化
都必须改变 source-tree digest。build script仍是 trusted build code，并受source root、toolchain、环境与
网络政策约束；它不能读取原 mutable package path来补齐未纳入digest的输入。

`--frozen`只消费 lock 指定的 immutable CAS source与exact artifact，禁止重新snapshot、build或回读
workspace path。non-frozen `resolve`／`realize`也必须从刚完成的CAS snapshot物化build root，不得直接以
原目录作为Cargo build authority。

### 4. Cargo 负责编译；product lock 负责选择

Cargo继续管理Rust compiler dependency closure、feature unification与`Cargo.lock`。Lattice package
kernel不复制Cargo resolver；它负责决定哪些logical packages与realizations进入一个具体产品。

对`NativeStatic`，realizer必须：

1. 从candidate/final transaction中已验证的CAS snapshots物化所有selected source-build packages；
2. 生成hash-addressed product Cargo workspace／root crate与static callback glue；
3. 只为locked graph中selected `NativeStatic`节点建立Cargo dependencies与registration adapters；
4. 使用锁定toolchain、target、features、producer与Cargo dependency receipt构建产品专属binary；
5. 验证registration／callback map并把product binary、source、toolchain、Cargo lock／producer与
   `EngineBuildId` receipts写入target realization；
6. 原子发布并重新打开final lock后，launcher才可执行该binary。

`NativeStatic` artifact是lock-specific product binary／可重建build output，不是可长期分发的`.rlib`。
static business call path继续使用直接Rust／Bevy glue、LTO与monomorphization，不经过C ABI。

对`PortableNative`，realizer从同一locked package source与SDK business declaration生成portable adapter，
构建真实`cdylib`，验证target、ABI、registration manifest与artifact digest后写入lock。prebuilt artifact仍需
exact descriptor与digest；它不能让未锁定source或另一份手写business implementation取得同一身份。

跨package的Rust source dependency必须同时由logical package dependency／capability edge授权，并由
generated build plan指向locked package instance。普通workspace path dependency、feature或root member
不能暗中把未选择package带入product binary。

### 5. Engine manifest 不是 package list

`latticeaxiom-engine`及任何generic host manifest只可固定依赖platform/runtime contracts与generated
product entry；不得：

- 直接依赖settings-ui、input、front-end、Terrenia worldgen/gameplay等product package implementation；
- 以feature matrix预声明所有可能first-party packages，再由runtime flag假装选择；
- 保存手写`App::add_plugins`、system、provider、dimension或content清单作为graph之外的fallback；
- 因package恰好是workspace member就自动链接或激活它。

保留永久Esc recovery、corrupt-lock diagnostics等host safety path不等于产品package fallback；它们不能
启动world、提供业务setting或注册Terrenia content。

### 6. Settings 与 client surface 迁移门禁

`@latticeaxiom/settings`、`@latticeaxiom/settings-ui`与`@latticeaxiom/input`是第一批迁移的code-bearing
packages。暂停菜单只路由到graph-selected settings surface；host不得维护第二份setting rows、范围、
按键或apply逻辑。

视距的首个产品surface必须是由`SettingSpec`机械生成的integer slider，而非`-／+`按钮。首个authoring
request范围为`2..=32` chunks、step `1`；requested render distance、effective render distance、simulation
distance与resident／prefetch working set是不同语义。host capability与performance profile可降低effective
值，但UI必须同时显示requested、effective与稳定限制原因，不得把静默hard clamp冒充用户选择。超出32的
Ultra／experimental范围只有在新的world-space coverage与performance evidence通过后才能加入profile。

draft、preview、Apply、Undo、cancel rollback、atomic persistence、restart后恢复、keyboard／mouse／gamepad、
800×600、UI scale 1.0／2.0、IME/CJK与AccessibilityNode继续遵守决策0025。performance preset只是多个typed
setting drafts的组合，不得绕过catalog validation或host hard caps。

### 7. Package-local code 不增加热路径间接层

源码位置改变不授权新增runtime package lookup、per-frame stable-ID string lookup、project-owned task
runtime、逐entity／voxel FFI或无界queue。BuildPlan在App建立前把graph编译为dense IDs、typed static
callbacks与有界dynamic batches；game tick不读取Cargo metadata、package source或resolver。

迁移前后必须用optimized build与相同workload比较。static path需证明仍为direct call／LTO，portable path
继续满足决策0024／0026的batch与overhead gate。streaming、mesh、collider与I/O继续使用Bevy task pools、
stable nearest-first priority、revision coalescing、cancellation与backpressure；code colocation不能用来
放宽frame、fixed-tick、memory、queue或apply预算。

视距32成为certified effective范围以前，必须在`desktop-reference-v1`或新accepted performance profile上
完成十分钟traversal，记录world-space coverage、active／resident／visible counts、RAM／VRAM、frame
P95／P99、generation／mesh／collider latency与queue high-water。若32只能作为request并被较低cap限制，
产品必须明确标记未认证，而不是提高slider后宣称已支持。

## 分阶段迁移

1. **M0 Contract／inventory**：更新layout、package↔crate inventory与source-inclusion validation；分类
   platform、code-bearing product与pure-data roots，禁止新增新的split implementation。
2. **M1 Proof package**：把`@example/dual-gameplay`变成含`Cargo.toml／src／data`的真实package root；
   compose／realizer从CAS分别构建actual static product与actual portable library。
3. **M2 Foundation packages**：迁移settings、settings-ui、input、front-end、world-library、observability、
   inspect与dev-tools；删除pause／HUD／shell里的平行catalog与hard-coded business wiring。
4. **M3 Product packages**：拆分通用platform worldgen/gameplay/render contracts与Terrenia-specific policy，
   再迁移`@terrenia/worldgen`、gameplay与presentation code；blocks／tools等确实pure-data的package不强制加code。
5. **M4 Generated product closure**：engine移除product package Cargo deps／features，NativeStatic由locked
   graph生成product root与callback table；launcher只运行sealed product artifact。
6. **M5 Settings／performance closure**：完成typed settings surface、requested／effective range与持久化，
   再按0026与terrain recovery gates扩大effective view distance、LOD与working set。

每阶段必须保持build、test与frozen lock可重现；迁移不得用同时保留旧engine path作为“临时fallback”来
冒充完成。尚未迁移的节点必须在inventory中明确标记compatibility bridge及删除条件。

## 自动验收

### Source 与 artifact proof

1. 修改package Rust source、Cargo manifest或data会改变source digest；旧lock仍只读取旧CAS bytes。
2. `--frozen`删除／篡改source、toolchain、Cargo dependency、registration或artifact任一object时精确失败，
   且不会读取mutable workspace path或启动App。
3. 相同locked inputs在干净materialization中产生相同registration与可验证artifact receipt；不能重现的
   platform bytes必须由明文producer nondeterminism policy说明，不能省略receipt。

### Graph authority proof

4. 从profile移除code-bearing package会从generated Cargo closure、callback map与runtime systems同时移除；
   添加或替换provider不修改engine source。
5. engine manifest／feature graph中不存在product package list；root workspace membership变化不改变既有
   final lock选择或frozen runtime closure。
6. 跨package static dependency缺少logical edge、版本不符或指向未锁定workspace path时在build前失败。

### Realization proof

7. package-root`@example/dual-gameplay`的static／portable由同一business declaration构建，并产生相同
   registration hash、IDs、schedule、N-tick state、diagnostics与normative save bytes。
8. trace／symbol／benchmark证明static direct path不经C ABI；portable callback按system／batch而非entity增长。

### Settings 与 performance proof

9. settings UI只消费compiled catalog；视距slider的min／max／step来自typed spec，Apply／Undo／cancel／restart
   round-trip通过，pause host没有`View -／+`或另一个范围常数。
10. requested／effective视距与clamp reason有typed golden；host cap改变不需修改settings-ui source，且UI不把
    request成功误报为effective working set已经达到。
11. package移动前后optimized static baseline不回归；async queue hard-cap、stale revision、coalescing与
    apply-budget boundary／`+1` tests通过。
12. effective视距32只有在accepted reference profile的frame、tick、memory、queue与latency evidence全部
    required项通过后才标为certified；更高范围需要新的profile／evidence而不是只改catalog maximum。

## 结果

- package source root成为声明、代码、data、测试、hash、build与分发的共同维护边界；
- Cargo继续发挥Rust dependency与optimization能力，但不再替代产品package selection；
- static path保留最大性能，portable path保留稳定ABI，两者都能证明来自locked source；
- pure-data packages保持轻量，平台crates保持通用，Terrenia与client product policy不再渗入engine；
- settings与高视距不再由互相矛盾的catalog、pause UI与host常数分别决定。

Accepted状态只冻结contract，不表示现有implementation已经合规；完成状态仍由requirements与固定commit
evidence计算。

## 被否决的方案

### 继续让packages只有manifest／data，代码全部留在crates

这会保留package graph与Cargo product closure两套authority，无法证明lock选择了实际执行code。

### 把所有platform crates移动进packages

package source root表示可选择、可版本化的产品单元，不是顶层源码目录换名。compose、SDK、ABI、storage、
generic runtime与host没有必要伪装成logical packages。

### Engine预依赖所有packages，再用feature或runtime flag选择

未选择code仍进入compiler closure，feature list成为隐藏package registry，替换provider仍需改engine；
这不满足0010／0018的唯一graph要求。

### NativeStatic直接从mutable workspace path构建

path内容可在resolve后改变，source receipt不能证明artifact输入，也使`--frozen`失去意义。

### 先把slider上限调高，再补streaming证据

这只改变请求UI，不证明effective coverage、frame time、memory与queues可承受；产品必须分别表达请求、
capability与认证证据。

## 相关文件

- [决策 0008：静态与动态共用一图](0008-static-and-dynamic-realizations-share-one-graph.md)
- [决策 0010：Nickel驱动package system](0010-nickel-driven-package-system.md)
- [决策 0018：首个vertical slice交付package kernel](0018-package-kernel-from-first-vertical-slice.md)
- [决策 0024：Portable Native ABI 0.x](0024-freeze-portable-native-abi-0x.md)
- [决策 0025：Client shell、settings与player contract](0025-freeze-client-shell-settings-observability-and-player-contracts.md)
- [决策 0026：First-demo performance budgets](0026-freeze-first-demo-performance-budgets.md)
- [决策 0032：Local acquisition与product lock](0032-freeze-local-package-acquisition-imports-and-product-lock.md)
- [Implementation workspace与package layout](../meta/repository-and-package-layout.md)
- [Package kernel](../platform/package-kernel/package-management.md)
- [Runtime integration roadmap](../delivery/roadmaps/game-engine.md)
- [First demo roadmap](../delivery/roadmaps/first-demo.md)
