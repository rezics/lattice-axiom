---
title: "@terrenia/tools"
document_id: package.terrenia.tools.index
document_status: accepted
document_type: index
owners:
  - "@terrenia/tools"
tracks_implementation: true
requirements:
  - TERRENIA-TOOLS-001
updated: 2026-08-22
---

# `@terrenia/tools`

## 身份与边界

- Source lifecycle：在当前 `client-world` lock。
- Domain：authoritative。
- Requires：`@terrenia/blocks` content capability。

本 package 只拥有木／石工具 item definitions、tier、class、durability、mining modifier 与
recipes。它是 data-only package，不实现 inventory、mining transaction 或 UI。

## 规范

- [v1 tool content and journey](../../../delivery/plans/v1-playable.md)
- [Terrenia block catalog](../blocks/catalog.md)

## Implementation mapping

内容由 package source 经 registration/content pipeline 消费；通用规则由 gameplay crates
实现。package 在 lock 中出现不证明所有工具可取得、durability 守恒或错误路径已验证。

