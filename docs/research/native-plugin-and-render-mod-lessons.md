---
title: 原生外掛、Bevy 與 Minecraft 渲染模組的設計教訓
status: active
type: research
updated: 2026-08-19
---

# 原生外掛、Bevy 與 Minecraft 渲染模組的設計教訓

## 研究問題

Lattice Axiom 需要回答：

1. 为什么不能直接把 Bevy／Rust Plugin 放进动态库？
2. static 与 dynamic 如何共用业务语义，又保留静态性能？
3. 如何允许渲染模组增加 pass、compute、LOD 或 alternate backend，而不重演 Minecraft 大版本断裂与模组冲突？
4. Nickel 应负责哪一层，package kernel 又必须负责哪一层？

本页记录外部证据与可迁移的原则，不把任何现有项目当作 Lattice Axiom 的架构模板。

## 結論

- Bevy／Rust 生态本身证明：动态链接 Rust code 不等于稳定第三方 ABI。必须拥有独立的 C／WIT-like contract、versioned schema 与 generated adapter。
- static 与 dynamic 应共享 package graph、registration manifest 与业务 code generation，不共享最低层 calling convention。
- Minecraft 生态证明了高价值渲染扩充确实包括 chunk batching、culling、LOD、shader 与 alternate backend；也证明了依赖内部 method／shader／class transformation 会把兼容成本转嫁给每次大版本和每对模组组合。
- Lattice Axiom 应保留 Fabric 的显式 metadata／dependency／fail-fast 思路，但拒绝「所有 native mod 都可任意修改 host」作为可组合 API。
- 渲染扩充需要 semantic slots、resource contract、capability cardinality、hardware requirement 与 fallback；不是更多任意 hook。

## Bevy 的動態外掛教訓

Bevy 曾提供 `bevy_dynamic_plugin`。官方 [0.14 release note](https://bevy.org/news/bevy-0-14/#deprecate-dynamic-plugins)说明它使用率低、API 本质上难以安全使用并造成实际故障，因此弃用；官方 [0.14 → 0.15 migration guide](https://bevy.org/learn/migration-guides/0-14-to-0-15/#remove-deprecated-bevy_dynamic_plugin)记录其在 0.15 被移除。

Bevy repository 中关于 [WIT-based extension host 的讨论](https://github.com/bevyengine/bevy/discussions/24010)进一步列出 Rust stable ABI、`TypeId`、vtable 与 foreign initialization 等困难。该讨论不是 Bevy 已接受的 Lattice 方案，但问题诊断与 Rust 语言边界一致。

### 对 Lattice Axiom 的含义

- 不复活「dynamic Bevy Plugin」；
- dynamic artifact 不导出 `Box<dyn Plugin>`／trait object；
- 不跨库共享 Bevy `World`／`TypeId`／Rust container；
- static package 仍可直接成为 Bevy plugin／system；
- dynamic package 使用生成的稳定 Lattice interface；
- old binary 是 ABI conformance fixture，不能在测试时偷偷重编。

### Dynamic ECS 是 host 機制，不是模組 ABI

Bevy 0.19 官方的 [dynamic ECS example](https://github.com/bevyengine/bevy/blob/v0.19.0/examples/ecs/dynamic.rs)展示 host 可以用 runtime descriptor 注册没有 compile-time Rust type 的 component，并通过 dynamic query／observer 访问它。这给 Lattice 的 `RuntimeDynamic` adapter 提供了实现落点，但没有解决跨动态库 Rust ABI，也不会让已编译的 typed system 认识新 type。

因此 Lattice 必须分别处理两件事：host 用 Bevy descriptor 建立 process-local storage；package SDK 用 stable ID、layout hash、generated shared-schema crate 与 batch adapter 定义跨 realization contract。两者不能混称「动态 component 支援」。

## Rust ABI 的底線

[Rust type layout reference](https://doc.rust-lang.org/stable/reference/type-layout.html)明确说明 type layout 可以在每次编译改变；相同 layout 也不必然代表 function-call ABI 相容。closure 没有 layout guarantee，default Rust representation 也不是跨版本 C contract。

[Rust linkage reference](https://doc.rust-lang.org/reference/linkage.html)区分 `rlib`、`dylib`、`cdylib` 等 artifact，但 linkage 类型本身不为任意 Rust public API 提供长期稳定 ABI。

### 对 Lattice Axiom 的含义

- dynamic 使用 `cdylib` + `extern "C"` + C layout；
- 固定宽度 scalar、pointer + length、function table 与显式 ownership；
- `#[repr(C)]` 只解决生成 binding 的 layout 前提，不自动让 `String`／`Vec`／panic／allocator 安全；
- interface 独立 version、append-only minor、breaking major；
- 高频 ECS 以 column batches 交换，不能逐 entity 呼叫。

## Nickel 的能力與邊界

Nickel 的 merge、function、contract 与 modular configuration 适合构造 package／profile control plane。[Nickel package management manual](https://nickel-lang.org/user-manual/package-management/)同时明确标示其 package management 仍是实验功能，官方 release 默认停用；它管理的是 Nickel library，并有自己的 source／version policy。

### 对 Lattice Axiom 的含义

- 采用 Nickel 语言能力与稳定 Rust embedding API；
- 不把 Nickel 上游实验 package manager 当作游戏 package kernel；
- Nickel 产生 typed `CompositionSpec`；
- Rust 负责 source、SemVer／capability resolution、trust、lock、build、load 与 Bevy activation；
- manifest 求值不取得网络／compiler／任意 host I/O；
- Nickel 不进入 runtime hot path。

## Minecraft：值得借鑑的需求

### Fabric Loader 的 metadata／dependency／entrypoint

[Fabric Loader documentation](https://docs.fabricmc.net/develop/loader/)记录：mod 以 metadata 声明 ID、version、entrypoint、dependency／conflict；同 ID 一次只载入一个版本，无法满足依赖时启动失败。这个结构证明 modpack 需要显式身分、版本、冲突与 fail-fast，而不是文件夹顺序。

可迁移：

- stable package ID／SemVer；
- explicit dependency／conflict；
- one resolved closure；
- code activation 前失败；
- metadata 与 entrypoint 分离。

不迁移：Fabric 明确让所有 mod 都具备修改游戏的同等能力，并支援 class transformation。Lattice native code虽然同样无法真正沙箱，但其**受支援组合 API**不能等同 arbitrary host patch。

### Sodium：高性能 renderer replacement

[Sodium repository](https://github.com/CaffeineMC/sodium)展示玩家真正需要的 renderer 优化规模：替换 chunk rendering architecture、降低 micro-stutter、改善相容与硬体路径。其 source／options 也体现 face／entity／fog occlusion 等早期剔除价值。

可迁移的问题形状：

- chunk 编译与上传需有界队列；
- 批次、资料 layout 与剔除是整体 pipeline 问题；
- visibility results 可以被其他 presentation consumer 重用；
- performance feature 要有硬体 matrix 与 fallback；
- renderer 不是「画一个 cube」就结束，而是持续 profiling／compatibility 产品。

Lattice 不复制 Sodium 对 Minecraft internal renderer 的替换方式，而把 terrain mesh、visibility 与 backend 变成显式 providers。

### FRAPI：稳定公共表面比 internal hook 更重要

Sodium source 中有 [Fabric Rendering API adapter](https://github.com/CaffeineMC/sodium/tree/dev/frapi)，说明 alternate renderer 仍需要实现生态认可的公共 rendering surface，普通模组才不必针对每个 backend 写路径。

可迁移：

- 普通内容依赖 semantic render API；
- alternate backend 实现／消费相同 capability；
- provider replacement 不应迫使所有 content package 改写。

但 Lattice 还需更严格：interface version、resource ownership、provider cardinality 与 graph validation 都进入 package contract。

### Nvidium：alternate backend 與硬體限制

[Nvidium](https://github.com/MCRcortex/nvidium)明确定位为 Sodium 的 alternate rendering backend，并只支援特定 NVIDIA Turing+ 硬体。这提供两个直接启发：

- renderer mechanism 的确应允许独立 provider；
- provider 必须声明 GPU feature／vendor／fallback，不能让不相容硬体在运行中才崩溃。

对应 Lattice capability：`render.terrain.backend@1 exactly-one`，profile／host 在 activation 前选择 default 或 hardware-specific provider。

### Iris／Distant Horizons：两个机制模组需要共同语义

[Iris issue #1289](https://github.com/IrisShaders/Iris/issues/1289)记录 shader 与 LOD／远景模组的组合故障。单一模组各自工作不代表二者能共享 depth、远景 geometry、shader stage 与资源 ownership。

可迁移：

- post effect、LOD、terrain provider 与 shader pipeline 要声明 semantic input／output；
- pass 插入点不能只靠 method hook；
- resource format／depth／sample／ordering 必须启动前验证；
- 至少用两个真实 render features 做组合测试，单一 sample 没有足够压力。

### Internal shader／nightly dependency 的警告

Sodium 的 [resource-pack compatibility documentation](https://github.com/CaffeineMC/sodium/wiki/Resource-Packs)说明依赖 internal shader 的替换不稳定，alternate renderer 也无法普遍兼容；其 [nightly build documentation](https://github.com/CaffeineMC/sodium/wiki/Nightly-Builds)提醒紧耦合 internals 的模组很容易在更新时破坏。

这正对应 Lattice 的两级动态路径：

- portable package 只依赖 stable semantic capability；
- 必须触及 internals 的 package 标成 engine-coupled，并接受精确 `EngineBuildId` 重建；
- 不把 internal path 宣传成普通兼容承诺。

## Minecraft：明確拒絕的相容模型

### Mixin／Overwrite

[Sponge Mixin overwrite documentation](https://github.com/SpongePowered/Mixin/wiki/Introduction-to-Mixins---Overwriting-Methods)明确列出 overwrite 会抹去先前 transformation、与其他 overwrite 冲突、并需在 target method 改变后维护复制代码。Mixin 有 fail-fast／injector 等改善工具，但本质仍是对非专用扩充点做代码 transformation。

Lattice Axiom 不提供：

- method／system body patch；
- priority-wins overwrite；
- 以 private Bevy／core symbol 作为 portable API；
- load-order composition；
- 模组自行猜测另一个模组是否已 patch。

替代机制：

- versioned semantic stages／slots；
- explicit observer／multi-provider／exclusive-provider cardinality；
- resource read／write declaration；
- capability dependency 与 deterministic graph；
- engine-coupled escape hatch，而不是假装 internals 稳定。

### 大版本分支與高移植成本

Sodium 的 [support policy](https://github.com/CaffeineMC/sodium/wiki/Support-Policy)列出多个 Minecraft／loader 分支与 deprecated series。这不是 Sodium 的设计错误，而是 host 没有稳定官方 mod contract、renderer replacement 紧密跟随游戏版本的现实后果。

Lattice 不能保证永不破坏；能做得更好的是把破坏限制在准确边界：

- package SemVer 处理逻辑 contract；
- capability／ABI major 处理调用 break；
- `EngineBuildId` 处理 intentional internal coupling；
- schema migration 处理长期资料；
- static source rebuild 处理完整 Bevy integration；
- old-binary fixture 验证 portable 承诺。

## 落地到 Lattice Axiom

| 外部教训 | Lattice 设计动作 |
| --- | --- |
| Rust／Bevy dynamic plugin 不稳定 | C bootstrap + generated tables，不暴露 Bevy type |
| package metadata／dependency 必须 fail-fast | Nickel + Rust kernel + `LockedGameGraph` |
| static code 需要完整引擎与最佳化 | static adapter 直接 Bevy／LTO，不走 C ABI |
| chatty reflection／FFI 不可行 | archetype column batches + command buffer |
| renderer mods 会新增机制 | RenderData／Feature／Pass／Provider 四级模型 |
| alternate backend 有硬体限制 | GPU capability + fallback + exactly-one provider |
| shader／LOD 组合会争夺资源 | semantic slots + resource graph + pairwise fixtures |
| internal patch 版本易碎 | portable／engine-coupled 分级 + exact `EngineBuildId` |
| native code 权限无限 | 诚实标为 trusted；不可信路径日后用 WASM／process |

## 首個證據包

1. 同一 gameplay package：`NativeStatic` 与 `PortableNative`。
2. 静态 direct、dynamic batch、逐 entity反例 benchmark。
3. 一个普通 post effect + 一个 compute／LOD-like feature 的组合。
4. default terrain backend + hardware-specific fake provider 的 exclusive conflict／fallback。
5. portable old binary 跨 Bevy upgrade；engine-coupled artifact exact rejection。
6. manifest mismatch、wrong table size、panic、task-after-stop 故障注入。

这些 fixture 比继续研究更多 Minecraft 模组更重要；外部项目已经足以指出问题，Lattice 需要用自己的真实 vertical slice 验证契约。

## 相關文件

- [原生模組 ABI](../architecture/native-module-abi.md)
- [渲染架構](../architecture/rendering.md)
- [套件內核](../architecture/package-management.md)
- [Bevy 生態調查](renderer-physics-landscape.md)
