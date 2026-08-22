---
title: "@terrenia/presentation"
document_id: package.terrenia.presentation.index
document_status: accepted
document_type: index
owners:
  - "@terrenia/presentation"
tracks_implementation: true
requirements:
  - TERRENIA-PRESENTATION-001
updated: 2026-08-22
---

# `@terrenia/presentation`

## 身份与边界

- Source lifecycle：在当前 `client-world` lock。
- Domain：client-only；headless 可省略。

本 package 拥有 Terrenia texture/material/icon/display metadata 与 client render registrations。
省略它不得改变 authoritative registrations、action receipts、snapshot bytes 或 world hash。

## 规范

- [Rendering platform](../../../platform/rendering/rendering.md)
- [Asset semantics](../../../platform/assets/asset-semantics.md)
- [Voxel rendering recovery plan](../../../delivery/plans/voxel-terrain-rendering-recovery.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-render-contracts`、`latticeaxiom-voxel-mesh` 与 engine client
adapter。当前 color atlas 是可用 baseline，不是完整 authored presentation catalog。

