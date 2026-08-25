---
title: "@latticeaxiom/settings-ui"
document_id: package.latticeaxiom.settings-ui.index
document_status: accepted
document_type: index
owners:
  - "@latticeaxiom/settings-ui"
tracks_implementation: true
requirements:
  - SETTINGS-SURFACE-001
  - SETTINGS-SURFACE-LAYERING-001
  - SETTINGS-SURFACE-CONTROLS-001
updated: 2026-08-24
---

# `@latticeaxiom/settings-ui`

## 身份与边界

- Source lifecycle：在当前 `client-world` lock。
- Domains：client、tool。
- Provides：`latticeaxiom:capability/settings-surface@1` exactly-one。
- Requires：`@latticeaxiom/settings` 的 settings registry。

本 package 是官方提供、默认推荐的 settings surface foundation，把 typed settings catalog 投影为
搜索、分类、分组／子页、详情、preview、draft/apply/reset 与无障碍 surface。复杂 package 可以
使用声明式 layout 或经 capability gate 的 specialized editor；产品也可以选择另一个完整
`settings-surface@1` provider，但它们都不得重新定义 setting scope、authority、validation、
transaction 或 persistence。

## Settings contribution

- `latticeaxiom:setting/interface/ui-scale`：device scope，`auto | 100 | 150 | 200`，可回滚 preview；
- `latticeaxiom:setting/interface/text-scale`：user scope，75%..200%；
- `latticeaxiom:setting/accessibility/high-contrast`、`reduce-motion`：user scope，立即生效；
- `latticeaxiom:setting/accessibility/notification-duration`：user scope，`short | normal | long`；
- `latticeaxiom:setting/interface/language`：user scope，locale enum，资源重载后生效；
- `latticeaxiom:setting/interface/show-advanced-settings`：user scope，只改变 Advanced 可见性。

这些是 surface 自己拥有的 UI/accessibility preferences。Video、Audio、window、renderer 与 gameplay
设置分别由 client-presentation、render provider 与 gameplay packages 拥有；不能因为本 package
负责画页面就把其他 package 的 setting ID 归到这里。完整字段见
[shipped catalog](../settings/shipped-settings-catalog.md)。

## 规范

- [分层 Settings surface](settings-surface.md)
- [Shipped settings catalog](../settings/shipped-settings-catalog.md)
- [Client UI system](../../../platform/client-ui/ui-system.md)
- [Client surface routing](../../../platform/client-ui/game-surface-state.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-start-ui` 与共享的 `latticeaxiom-ui` client crate。当前
暂停菜单中的 View± 不是完整 settings surface，也不能证明设置持久化或 Controls 改键已实现。
