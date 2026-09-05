---
title: "@latticeaxiom/observability"
document_id: package.latticeaxiom.observability.index
document_status: accepted
document_type: index
owners:
  - "@latticeaxiom/observability"
tracks_implementation: true
requirements:
  - OBSERVABILITY-REGISTRY-001
updated: 2026-08-24
---

# `@latticeaxiom/observability`

## 身份与边界

- Source lifecycle：在当前 `client-world` lock。
- Domains：client、server、tool。
- Provides：`latticeaxiom:capability/diagnostic-registry@1` exactly-one。

本 package 拥有 diagnostic item／metric registry、subscription budget、deterministic report 与
权限元数据。它不拥有玩家 tooltip、F3 panel 或 world-space debug drawing。

## Settings contribution

No user-configurable settings。本 package 提供 diagnostic/metric/visualizer schema、权限与 hard
budget，不拥有其显示偏好。subscription、report/export 与 permission request 是 typed operations；
overlay、sampling、history、visualizer 和 log-level preferences 由 `@latticeaxiom/dev-tools` 拥有。

## 规范

- [Diagnostic registry](registry-and-reports.md)
- [Host observability adapter](../../../platform/observability/diagnostics-inspection-and-debug-visualization.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-runtime-contracts`、`latticeaxiom-compose` 与 engine host
adapter。manifest registration 与 HUD 文本存在仍不足以证明 subscription、budget 和 report
契约已闭合。
