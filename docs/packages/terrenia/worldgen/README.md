---
title: "@terrenia/worldgen"
document_id: package.terrenia.worldgen.index
document_status: accepted
document_type: index
owners:
  - "@terrenia/worldgen"
tracks_implementation: true
requirements:
  - TERRENIA-WORLDGEN-001
updated: 2026-08-24
---

# `@terrenia/worldgen`

## 身份与边界

- Source lifecycle：在当前 `client-world` lock。
- Domain：authoritative。
- Provides：`latticeaxiom:capability/worldgen-terrain-provider@2` exactly-one。
- Requires：`@terrenia/blocks` content capability。

本 package 选择 Terrenia terrain、biome、geology、resource、spawn 与 cave providers。通用
coordinator、chunk lifecycle、storage candidate protocol 和 deterministic execution 属于 platform。

## Settings contribution

No user-configurable runtime settings。seed、world preset、terrain/cave provider、generation feature、
world height、resource distribution 与其他生成参数属于 New World form、profile composition 或 frozen
world metadata；创建后不能用普通 Settings row 改写。若未来出现不会改变 authoritative generation
的纯预览偏好，应由其 presentation/tool owner 单独注册。

## 规范

- [World generation platform](../../../platform/world-generation/world-generation.md)
- [Cave composition](../../../platform/world-generation/cave-generation.md)
- [Physical authoring](../../../platform/content/physical-authoring.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-worldgen`、`latticeaxiom-territory`、
`latticeaxiom-voxel-runtime` 与 engine integration。当前 sparse generator 不等于完整 Territory
Atlas、hydrology 与 cave journey。
