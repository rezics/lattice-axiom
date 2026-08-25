---
title: "@terrenia/blocks"
document_id: package.terrenia.blocks.index
document_status: accepted
document_type: index
owners:
  - "@terrenia/blocks"
tracks_implementation: true
requirements:
  - TERRENIA-BLOCKS-001
updated: 2026-08-24
---

# `@terrenia/blocks`

## 身份与边界

- Source lifecycle：在当前 `client-world` lock。
- Domain：authoritative。
- Provides：`latticeaxiom:capability/content-blocks@1` exactly-one。

本 package 拥有 Terrenia 方块、流体、物品及相邻 intrinsic／state／mining／drop／placement
资料。状态和几何变体不得复制 StableId；host 不得持有 Terrenia 私有 fallback catalog。

## Settings contribution

No user-configurable settings。方块、流体、intrinsic/state、mining、drop 与 placement 数据属于
authoritative content schema；硬度、掉落率、材质或碰撞不能作为 client preference 暴露。
影响这些规则的 world policy 由 `@terrenia/gameplay` 以明确 world-authoritative setting 拥有。

## 规范

- [72 blocks 与两种流体内容清单](catalog.md)
- [Asset semantics](../../../platform/assets/asset-semantics.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-content`、`latticeaxiom-registration` 与 engine catalog adapter。
当前 lock 和少量 block rows 不是完整 72+2 内容基线的实现证据。
