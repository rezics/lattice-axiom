---
title: Lattice Axiom 文档
document_id: docs.index
document_status: active
document_type: index
tracks_implementation: false
updated: 2026-08-22
---

# Lattice Axiom 文档

Lattice Axiom 的规范按 logical package 组织；不属于某个 package 的 kernel、host、wire、
storage 与 rendering 契约按 platform system 组织。ADR 保存“为什么这样决定”，delivery
保存“先做什么”，两者都不替代实现证据。

## 先看哪里

1. [项目愿景](project/project-vision.md)与 [package-first 开发策略](project/development-strategy.md)
2. [Logical package 索引](packages/README.md)
3. [Platform contract 索引](platform/README.md)
4. [Delivery 路线与计划](delivery/README.md)
5. [文档组织与追踪契约](meta/documentation-organization.md)
6. [需求与实现证据规则](meta/traceability.md)

## 当前产品边界

- Bevy `0.19.x` 是 architecture baseline，首个实现精确锁定 `0.19.1`。
- runtime 使用一个正常 Bevy App；不重做 ECS、scheduler、renderer、assets、input 或 tasks。
- package manifest、Nickel composition、lock、registration image 与 runtime image 构成产品启动
  链，第一方 package 和 Terrenia 都不得绕过。
- logical `PackageName` 与 stable registration identity 相互独立，目录路径不承担任何一个身份。
- client shell graph 与 locked world graph 使用同一 resolver／lock／registration pipeline，但拥有
  不同 world-write 权限。
- RocksDB 保存完整已物化 world snapshots；process-local Bevy／ABI handles 不进入存档。
- static 与 portable native realization 共用同一 locked graph 和 authoritative contract。

这些是 accepted baseline，不表示对应实现已经完成。实际完成情况只看生成的
`delivery/status.md`；没有 evidence 的页面默认为 `not-started` 或 `in-progress`。

## Package 导航

### Lattice Axiom 平台 packages

- [front-end](packages/latticeaxiom/front-end/README.md)：client shell 与 world launch routing
- [world-library](packages/latticeaxiom/world-library/README.md)：world catalog、preflight 与恢复操作
- [settings](packages/latticeaxiom/settings/README.md)：typed registry、scope 与 apply transaction
- [settings-ui](packages/latticeaxiom/settings-ui/README.md)：client/tool settings surface
- [observability](packages/latticeaxiom/observability/README.md)：diagnostic registry 与 report
- [inspect](packages/latticeaxiom/inspect/README.md)：玩家 target inspect surface
- [dev-tools](packages/latticeaxiom/dev-tools/README.md)：开发者诊断 workbench
- [input](packages/latticeaxiom/input/README.md)：已接受、待实现的动作注册与绑定 package
- [progress](packages/latticeaxiom/progress/README.md)：提议后期使用的进度图 package
- [relations](packages/latticeaxiom/relations/README.md)：提议后期使用的关系图 package

### Terrenia packages

- [terrenia](packages/terrenia/main/README.md)：当前维度聚合 root
- [blocks](packages/terrenia/blocks/README.md)：方块、流体、物品与相邻内容定义
- [worldgen](packages/terrenia/worldgen/README.md)：Terrenia terrain provider
- [gameplay](packages/terrenia/gameplay/README.md)：Terrenia 掉落、配方与内容规则
- [tools](packages/terrenia/tools/README.md)：基础工具内容
- [presentation](packages/terrenia/presentation/README.md)：client-only 表现资料
- [metallurgy](packages/terrenia/metallurgy/README.md)、[science](packages/terrenia/science/README.md)、
  [thaumaturgy](packages/terrenia/thaumaturgy/README.md)、[journey](packages/terrenia/journey/README.md)：
  已声明但不在当前产品 lock 的后续 skeletons

## Platform 导航

- [Package kernel](platform/package-kernel/package-management.md)
- [Composition](platform/composition/module-composition.md)
- [Semantic registration](platform/registration/semantic-registration.md)
- [Runtime host](platform/runtime/game-engine-runtime.md)
- [Native ABI](platform/native-abi/native-module-abi.md)
- [World storage](platform/world-storage/world-persistence.md)
- [World generation](platform/world-generation/world-generation.md)
- [Rendering](platform/rendering/rendering.md)
- [Input runtime](platform/input/input-binding-and-contexts.md)
- [Client UI](platform/client-ui/ui-system.md) 与 [surface routing](platform/client-ui/game-surface-state.md)
- [Launcher supervisor](platform/launcher/supervisor-loop.md)

## 决策、研究与交付

- [Accepted Decisions](decisions/README.md)
- [研究](research/README.md)
- [Delivery](delivery/README.md)
- [问题决议索引](delivery/open-questions.md)

## 状态读法

`document_status` 表示规范成熟度；`implemented` 只可能由 requirements/evidence 聚合产生。
manifest、crate、DTO、fixture 或 roadmap checkbox 的存在都不等于实现。详细门槛见
[需求与实现证据](meta/traceability.md)。
