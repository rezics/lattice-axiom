---
title: Lattice Axiom 文件
status: active
type: index
updated: 2026-08-19
---

# Lattice Axiom 文件

Lattice Axiom 是建立在 Bevy 上、以 Nickel／SemVer package graph 与静态／动态双 realization 为核心的体素世界游戏。文档保存产品架构、accepted decisions、外部研究与第一个可玩 vertical slice 的验收。

## 当前基线

> 默认使用完整上游能力；只有当一个真实可玩的原型证明现有方案无法满足不可妥协的产品需求时，才允许自研替代。

- Bevy `0.19.x` 是 engine baseline；精确 patch／features 由 Cargo lock 固定。
- runtime 使用一个正常 Bevy App；不重做 ECS、scheduler、renderer、assets、input 或 tasks。
- Nickel `package.ncl`／`game.ncl` 产生 typed `CompositionSpec`；Rust package kernel负责 SemVer／capability、lock、build、load与activation。
- 每次游戏由 `LockedGameGraph`／`RegistrationImage` 建立；第一方 package 与 Terrenia 维度不能绕过。
- Logical package 使用 root `name` 或 scoped `@scope/name`；stable registration 使用独立的 `<namespace>:<kind>/<path>`，两者不得互相推导。
- 精确 StableId、SemanticTag／Map、StateProperty、Affordance、Predicate、Role 与 fallback bundle 分层；Nickel 组合语义，Rust 验证并编译进 `RegistrationImage`。
- `terrenia` 只是当前选择的普通维度聚合模组；平台 `latticeaxiom:*` contract 与 host 不以 Terrenia 作为固定基础设施。
- Terrenia 内容按 6 → 18 → 40 → 72 个方块定义分阶段交付；第一完整内容基线另含 water／lava 两个独立流体定义，状态与几何变体不复制 StableId。
- 同一业务 package可生成 `NativeStatic`与`PortableNative`：static直接Bevy／LTO，dynamic经versioned C ABI／batch ECS。
- dynamic另有诚实的`EngineCoupledNative`等级，以精确`EngineBuildId`换取低层host能力。
- Bevy是core package内部tool；若外部contract真的因升级破坏，相关package／capability／schema仍按自己的版本规则升级。
- 所有package可经`RegistrationManifest.settings`注入typed settings；基础settings package统一处理scope、authority、apply与GUI／CLI surface。
- 玩家inspect与dev diagnostics共用结构化observability资料，但使用不同surface；package不能各自占用HUD角落。
- 世界坐标采用Bevy原生右手Y-up。
- RocksDB保存完整已物化world snapshots；process-local Bevy／ABI handles不进入存档。
- package-driven client shell在world写入前完成catalog、frozen-lock preflight、checkpoint／migration与恢复选择。
- 首阶段包含package kernel与ABI `0.x`；延后的是public registry、marketplace、general resolver规模、hot unload与WASM ecosystem。

## 建议阅读顺序

1. [专案愿景与设计支柱](foundations/project-vision.md)
2. [Package-first／Bevy-first 开发策略](foundations/development-strategy.md)
3. [技术栈与边界](foundations/technology-stack.md)
4. [套件内核](architecture/package-management.md)
5. [模组、注册清单与双实现](architecture/module-composition.md)
6. [语义注册、内容判定与候选选择](architecture/semantic-registration.md)
7. [Demo workspace 与 Terrenia package 组织](architecture/demo-workspace-organization.md)
8. [原生模组 ABI](architecture/native-module-abi.md)
9. [套件驱动的 Bevy runtime](architecture/game-engine-runtime.md)
10. [Package 设置与配置](architecture/settings-and-configuration.md)
11. [诊断、检查与除错可视化](architecture/diagnostics-inspection-and-debug-visualization.md)
12. [渲染 capability／pass／provider](architecture/rendering.md)
13. [版本与相容性](architecture/versioning-and-compatibility.md)
14. [World 目录、开始页与安全生命周期](architecture/world-lifecycle-and-start-ui.md)
15. [世界持久化](architecture/world-persistence.md)
16. [Terrenia 方块内容规划](planning/terrenia-block-catalog.md)
17. [第一个可玩 demo 路线图](planning/roadmap-first-demo.md)
18. [执行期整合路线](planning/roadmap-game-engine.md)
19. [可组合世界生成](architecture/world-generation.md)
20. [可组合洞穴生成](architecture/cave-generation.md)
21. [实体、物理与表现](architecture/entity-physics-presentation.md)
22. [资产语义](architecture/asset-semantics.md)
23. [待决问题](planning/open-questions.md)

## 文件地图

| 分类 | 用途 | 入口 |
| --- | --- | --- |
| 基础 | 愿景、策略、技术与共同词汇 | [愿景](foundations/project-vision.md)、[策略](foundations/development-strategy.md)、[技术栈](foundations/technology-stack.md)、[词汇表](foundations/glossary.md) |
| Package／ABI | 游戏如何组合、锁定、生成与加载 | [套件内核](architecture/package-management.md)、[模组组合](architecture/module-composition.md)、[语义注册](architecture/semantic-registration.md)、[设置](architecture/settings-and-configuration.md)、[demo组织](architecture/demo-workspace-organization.md)、[原生 ABI](architecture/native-module-abi.md)、[版本相容](architecture/versioning-and-compatibility.md) |
| Bevy runtime／前端 | package closure如何成为一个Bevy App并提供一致surface | [执行期](architecture/game-engine-runtime.md)、[诊断／检查／可视化](architecture/diagnostics-inspection-and-debug-visualization.md)、[渲染](architecture/rendering.md)、[开始页／world lifecycle](architecture/world-lifecycle-and-start-ui.md)、[实体／物理／表现](architecture/entity-physics-presentation.md)、[资产](architecture/asset-semantics.md) |
| 世界 | Lattice Axiom的权威资料与生成差异层 | [持久化](architecture/world-persistence.md)、[world lifecycle](architecture/world-lifecycle-and-start-ui.md)、[世界生成](architecture/world-generation.md)、[洞穴](architecture/cave-generation.md)、[物理创作](architecture/physical-authoring.md) |
| 研究 | 外部证据、候选与失败模式，不自动成为承诺 | [信息／设置／存档UX](research/debug-settings-and-world-ux-lessons.md)、[引擎采用](research/open-source-game-engine-adoption.md)、[原生外挂机制／渲染模组](research/native-plugin-and-render-mod-lessons.md)、[Bevy生态](research/renderer-physics-landscape.md)、[Godot工具对照](research/godot-toolchain-comparison.md)、[Minecraft注册语义](research/minecraft-registration-semantics.md)、[Minecraft世界生成](research/minecraft-world-generation-lessons.md)、[现代地形／洞穴](research/modern-terrain-and-cave-generation.md) |
| 规划 | 内容范围、依赖顺序、可玩验收、待决问题 | [Terrenia方块](planning/terrenia-block-catalog.md)、[第一个demo](planning/roadmap-first-demo.md)、[runtime路线](planning/roadmap-game-engine.md)、[待决问题](planning/open-questions.md) |
| 元文件 | 文档维护规则 | [组织方式](meta/documentation-organization.md) |

## Accepted Decisions

| ADR | 决策 |
| --- | --- |
| [0001](decisions/0001-territory-first-biome-driven-world-generation.md) | 领地优先／群系驱动世界生成 |
| [0002](decisions/0002-hybrid-cave-generation-composition.md) | 混合洞穴组合 |
| [0003](decisions/0003-no-global-version-switch.md) | 不以全域版本代理相容性 |
| [0004](decisions/0004-territorial-delegation-for-spatial-generation.md) | 领地委派空间生成能力 |
| [0008](decisions/0008-static-and-dynamic-realizations-share-one-graph.md) | 静态／动态 realization共用同一package graph |
| [0009](decisions/0009-rocksdb-authoritative-world-snapshots.md) | RocksDB权威完整snapshot |
| [0010](decisions/0010-nickel-driven-package-system.md) | Nickel驱动package system，Rust执行 |
| [0014](decisions/0014-adopt-bevy-upstream-first.md) | 采用Bevy／上游优先原则 |
| [0015](decisions/0015-bevy-native-y-up-world-coordinates.md) | Bevy原生右手Y-up |
| [0017](decisions/0017-versioned-native-module-abi.md) | Versioned C ABI／capability tables |
| [0018](decisions/0018-package-kernel-from-first-vertical-slice.md) | 首个vertical slice交付package kernel与双实现 |
| [0019](decisions/0019-separate-package-and-registration-identities.md) | Root／scoped package 与 stable registration identity 分离；Terrenia 为第一维度 |
| [0020](decisions/0020-semantic-registration-and-content-selection.md) | 精确注册身份、内容语义、判定、候选选择与 fallback 分层 |

### Superseded Decisions

| ADR | 已被取代的范围 |
| --- | --- |
| [0012](decisions/0012-latticeaxiom-naming-convention.md) | logical package 与 stable ID 共用 `latticeaxiom` 前缀；Rust crate／ABI 命名由 0019 保留 |

编号空缺表示错误方案已从当前文档树删除；需要追溯时使用 Git 历史，不在有效文档中并列互斥架构。

## 文档成熟度

- `exploration`：调查或仍未由prototype验证。
- `proposed`：可实施架构，具体layout／API仍需证据。
- `accepted`：已明确采用的decision／strategy。
- `active`：当前有效index／roadmap／reference。

目前尚无程序实作。accepted ADR 是开工边界；architecture中的具体API名称仍可由第一个vertical slice修正，但不能绕过accepted package／ABI／Bevy原则。

## 当前刻意没有

仓库尚未建立tutorial、操作指南或API reference。等第一个可重复build、SDK与ABI inspector存在后，再按真实player／package author／maintainer任务增加。
