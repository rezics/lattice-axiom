---
title: "@latticeaxiom/dev-tools"
document_id: package.latticeaxiom.dev-tools.index
document_status: accepted
document_type: index
owners:
  - "@latticeaxiom/dev-tools"
tracks_implementation: true
requirements:
  - DEV-TOOLS-001
updated: 2026-08-24
---

# `@latticeaxiom/dev-tools`

## 身份与边界

- Source lifecycle：在当前 `client-world` lock。
- Domains：client、tool；release profile 可省略。
- Provides：`latticeaxiom:capability/debug-workbench@1` exactly-one。
- Requires：settings 与 diagnostic registries。

本 package 拥有 F3 类 panel、graphs、world-space visualizer 选择和 developer-only settings。
它不改变 authoritative world，也不能让各 package 自行占用 HUD 角落。

## Settings contribution

- `latticeaxiom:setting/developer/overlay-enabled`、`overlay-detail`；
- `latticeaxiom:setting/developer/sample-period-ms`、`history-seconds`；
- `latticeaxiom:setting/developer/visualizer-radius`、`visualizer-depth-mode`、
  `visualizer-labels`；
- `latticeaxiom:setting/developer/log-level`；
- 每个 registered `DebugVisualizerSpec` 机械生成 enable row，并按 Grid、Lifecycle、Persistence、
  Mesh、Collision、Visibility/LOD、Worldgen 分组。

这些 rows 只在 dev-tools profile 与 permission 允许时出现，scope 为 session/user，且始终受 host
primitive、upload、history 与 sampling hard budgets 限制。完整 defaults/constraints 见
[shipped settings catalog](../settings/shipped-settings-catalog.md)。

## 规范

- [Debug workbench](debug-workbench.md)
- [Rendering debug boundary](../../../platform/rendering/rendering.md)
- [Shipped settings catalog](../settings/shipped-settings-catalog.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-runtime-contracts` 与 engine client adapter。当前诊断行和
指标 DTO 不能自动证明完整 workbench、subscription 或 visualizer 已实现。
