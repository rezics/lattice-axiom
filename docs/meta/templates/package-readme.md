---
title: <Package display name>
document_id: package.<scope>.<name>.index
document_status: proposed
document_type: index
owners:
  - "@scope/name"
tracks_implementation: true
requirements: []
updated: YYYY-MM-DD
---

# <Package display name>

## 身份

- PackageName：`@scope/name`
- 生命周期：proposed／declared／locked／shipped
- 权威边界：authoritative／client-only／tool-only

## 职责

说明 package 拥有什么，并列出明确非目标。

## Capability 与依赖

列出提供和消费的 versioned capabilities，以及依赖的 logical packages。

## Settings contribution

列出 package 拥有的 stable setting IDs、category、default/constraint、scope/authority、apply impact、
availability 与 primary catalog 链接。没有用户可配置 consumer 时必须明确写
`No user-configurable settings` 及理由；composition/new-world inputs、内部调参与 hard policy
不得伪装成 runtime settings。

## 规范

链接本目录单一问题页面，不复制其 MUST 条款。

## Implementation mapping

列出负责实现本 package contract 的 crates、production entry 和生成状态摘要。crate 名称不是
package 身份。

## 验收

链接 `requirements.json` 与全局 evidence/status 页面。
