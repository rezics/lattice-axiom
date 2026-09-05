---
title: "@latticeaxiom/front-end"
document_id: package.latticeaxiom.front-end.index
document_status: accepted
document_type: index
owners:
  - "@latticeaxiom/front-end"
tracks_implementation: true
requirements:
  - FRONTEND-SHELL-001
updated: 2026-08-24
---

# `@latticeaxiom/front-end`

## 身份与边界

- Source lifecycle：manifest/source 已声明；不在当前审计的 `client-world` lock。
- Domain：client-only。
- Provides：`latticeaxiom:capability/client-shell@1` exactly-one。
- Requires：world catalog、settings surface 与 diagnostic registry。

本 package 拥有 shell state、routing、loading/error surface 与 launch intent 的用户流程。它不
扫描 world 目录、不打开 writer、不拥有 settings registry，也不把 Terrenia 内容写死在 UI。

## Settings contribution

- `latticeaxiom:setting/gameplay/pause-on-focus-loss`：bool，default true，user scope，
  local-user authority，Immediate。

本 package 只拥有 shell/game route 与窗口 focus 交互的偏好。background FPS、background audio、
window mode 与 Video/Audio rows 属于 `@latticeaxiom/client-presentation`；world launch、profile
选择与 package graph 是 operation/composition，不伪装成 runtime setting。

## 规范

- [Shell 与 world lifecycle 组合](world-lifecycle-and-start-ui.md)
- [玩家 surface 原始整合计划](../../../delivery/plans/first-playable-player-surfaces.md)；其通用 gameplay 部分由 platform contract
  承担，front-end 只保留 shell 责任。
- [Launcher supervisor](../../../platform/launcher/supervisor-loop.md)
- [Client surface routing](../../../platform/client-ui/game-surface-state.md)
- [Shipped settings catalog](../settings/shipped-settings-catalog.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-start-ui`、`latticeaxiom-launcher` 与
`latticeaxiom-engine` 的 client shell adapter。crate 存在不代表本 package 已实现；当前 evidence
确认 shell 与 game 仍缺少监督者 executable 与 `task play` 产品闭环。
