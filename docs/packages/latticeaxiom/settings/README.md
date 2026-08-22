---
title: "@latticeaxiom/settings"
document_id: package.latticeaxiom.settings.index
document_status: accepted
document_type: index
owners:
  - "@latticeaxiom/settings"
tracks_implementation: true
requirements: []
updated: 2026-08-22
---

# `@latticeaxiom/settings`

## 身份与边界

- Source lifecycle：在当前 `client-world` lock。
- Domains：client、server、tool。
- Provides：`latticeaxiom:capability/settings-registry@1` exactly-one。

本 package 拥有 typed `SettingSpec` catalog、scope／authority、effective value resolution、
apply transaction 与保存迁移语义。它不拥有 Bevy widget、screen routing 或任意 package 的
setting ID。

## 规范

- [设置与配置契约](settings-and-configuration.md)
- [Settings UI package](../settings-ui/README.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-runtime-contracts`、`latticeaxiom-compose`、
`latticeaxiom-start-ui` 与 host integration。当前 lock 中的 data package 和 DTO 只证明 source/
registration surface 存在，不自动证明 user-scope 持久化或完整 transaction 已接入。

