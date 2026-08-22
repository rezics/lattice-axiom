---
title: "@terrenia/gameplay"
document_id: package.terrenia.gameplay.index
document_status: accepted
document_type: index
owners:
  - "@terrenia/gameplay"
tracks_implementation: true
requirements: []
updated: 2026-08-22
---

# `@terrenia/gameplay`

## 身份与边界

- Source lifecycle：在当前 `client-world` lock。
- Domain：authoritative。
- Depends：`@terrenia/blocks` 与 `@terrenia/tools`。

本 package 提供 Terrenia 的 drops、placement rules、recipes、workstation／furnace／container
bindings 与 content-specific gameplay contributions。inventory、command transaction、durability 和
crafting engine 等可跨维度机制属于 platform gameplay，不得私有化为 Terrenia host code。

## 规范

- [Player gameplay surfaces](../../../platform/gameplay/player-surfaces.md)
- [v1 playable plan](../../../delivery/plans/v1-playable.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-gameplay`、`latticeaxiom-player` 与 engine integration。当前
walk／mine／place／craft slice 只有在 production journey evidence 闭合后才能标为 implemented。

