---
title: "@latticeaxiom/dev-tools"
document_id: package.latticeaxiom.dev-tools.index
document_status: accepted
document_type: index
owners:
  - "@latticeaxiom/dev-tools"
tracks_implementation: true
requirements: []
updated: 2026-08-22
---

# `@latticeaxiom/dev-tools`

## 身份与边界

- Source lifecycle：在当前 `client-world` lock。
- Domains：client、tool；release profile 可省略。
- Provides：`latticeaxiom:capability/debug-workbench@1` exactly-one。
- Requires：settings 与 diagnostic registries。

本 package 拥有 F3 类 panel、graphs、world-space visualizer 选择和 developer-only settings。
它不改变 authoritative world，也不能让各 package 自行占用 HUD 角落。

## 规范

- [Debug workbench](debug-workbench.md)
- [Rendering debug boundary](../../../platform/rendering/rendering.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-runtime-contracts` 与 engine client adapter。当前诊断行和
指标 DTO 不能自动证明完整 workbench、subscription 或 visualizer 已实现。

