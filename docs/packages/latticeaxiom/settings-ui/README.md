---
title: "@latticeaxiom/settings-ui"
document_id: package.latticeaxiom.settings-ui.index
document_status: accepted
document_type: index
owners:
  - "@latticeaxiom/settings-ui"
tracks_implementation: true
requirements: []
updated: 2026-08-22
---

# `@latticeaxiom/settings-ui`

## 身份与边界

- Source lifecycle：在当前 `client-world` lock。
- Domains：client、tool。
- Provides：`latticeaxiom:capability/settings-surface@1` exactly-one。
- Requires：`@latticeaxiom/settings` 的 settings registry。

本 package 把 typed settings catalog 投影为搜索、分类、preview、draft/apply/reset 与无障碍
surface。它不重新定义 setting scope、authority、validation 或 persistence。

## 规范

- [Settings surface](settings-surface.md)
- [Client UI system proposal](../../../platform/client-ui/ui-system.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-start-ui` 与未来共享的 `latticeaxiom-ui` client crate。当前
暂停菜单中的 View± 不是完整 settings surface，也不能证明设置持久化或 Controls 改键已实现。

