---
title: "@terrenia/gameplay"
document_id: package.terrenia.gameplay.index
document_status: accepted
document_type: index
owners:
  - "@terrenia/gameplay"
tracks_implementation: true
requirements:
  - TERRENIA-GAMEPLAY-001
updated: 2026-08-24
---

# `@terrenia/gameplay`

## 身份与边界

- Source lifecycle：在当前 `client-world` lock。
- Domain：authoritative。
- Depends：`@terrenia/blocks` 与 `@terrenia/tools`。

本 package 提供 Terrenia 的 drops、placement rules、recipes、workstation／furnace／container
bindings 与 content-specific gameplay contributions。inventory、command transaction、durability 和
crafting engine 等可跨维度机制属于 platform gameplay，不得私有化为 Terrenia host code。

## Settings contribution

以下 world/world-owner rows 只在对应 authoritative mechanism 真实存在时注册；均为 Immediate，
client 没有 writer/admin authority 时只读：

- `terrenia:setting/world/difficulty`：`peaceful | easy | normal | hard`，default `normal`；
- `terrenia:setting/world/keep-inventory`：default false；
- `terrenia:setting/world/block-drops`：default true；
- `terrenia:setting/world/mob-spawning`：default true；
- `terrenia:setting/world/mob-griefing`：default true；
- `terrenia:setting/world/fire-spread`：default true；
- `terrenia:setting/world/daylight-cycle`：default true；
- `terrenia:setting/world/weather-cycle`：default true；
- `terrenia:setting/world/fall-damage`：default true；
- `terrenia:setting/world/natural-regeneration`：default true；
- `terrenia:setting/world/immediate-respawn`：default false。

首个 demo 尚无 mob、weather、daylight、fire 或 respawn consumer 时，对应 rows 不得以 disabled
placeholder 假装已实现。reach、cooldown、tool tier 与 recipe costs 是 balance/authoritative
constants，不因存在常量就自动成为设置。

## 规范

- [Player gameplay surfaces](../../../platform/gameplay/player-surfaces.md)
- [v1 playable plan](../../../delivery/plans/v1-playable.md)
- [Shipped settings catalog](../../latticeaxiom/settings/shipped-settings-catalog.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-gameplay`、`latticeaxiom-player` 与 engine integration。当前
walk／mine／place／craft slice 只有在 production journey evidence 闭合后才能标为 implemented。
