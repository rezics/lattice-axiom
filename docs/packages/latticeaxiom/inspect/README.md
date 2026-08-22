---
title: "@latticeaxiom/inspect"
document_id: package.latticeaxiom.inspect.index
document_status: accepted
document_type: index
owners:
  - "@latticeaxiom/inspect"
tracks_implementation: true
requirements:
  - TARGET-INSPECT-001
updated: 2026-08-22
---

# `@latticeaxiom/inspect`

## 身份与边界

- Source lifecycle：在当前 `client-world` lock。
- Domain：client-only。
- Provides：`latticeaxiom:capability/target-inspect-surface@1` exactly-one。
- Requires：diagnostic registry。

本 package 将权威 target selection 与 package-contributed typed fragments 组合为玩家可理解的
overlay。它不拥有 DDA、方块内容、developer metrics 或任意 HUD 坐标。

## 规范

- [Target inspect](target-inspect.md)
- [Observability host boundary](../../../platform/observability/diagnostics-inspection-and-debug-visualization.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-runtime-contracts`、`latticeaxiom-player` 与 engine HUD
adapter。当前已有 inspect overlay fragment 的代码证据，但必须由 requirement/evidence 审计后
才能宣称完整 package surface 已实现。

