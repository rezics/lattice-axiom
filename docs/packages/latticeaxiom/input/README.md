---
title: "@latticeaxiom/input"
document_id: package.latticeaxiom.input.index
document_status: accepted
document_type: index
owners:
  - "@latticeaxiom/input"
tracks_implementation: true
requirements:
  - INPUT-PACKAGE-001
updated: 2026-08-22
---

# `@latticeaxiom/input`

## 身份与边界

- Source lifecycle：ADR 已接受；实现仓 manifest 与产品 lock 选择尚未完成。
- Domains：client、headless、tool。
- Provides：`latticeaxiom:capability/input-actions@1` exactly-one。
- Requires：`@latticeaxiom/settings` 的 user-scope persistence contract。

实现仓当前没有该 package manifest，也没有 `latticeaxiom-input` crate；因此实现状态保持
`not-started`。规范采用与实现证据严格分离。

## 职责

- 声明 platform action catalog 与默认 bindings；
- 提供可版本化、可由 settings 保存的 binding profile；
- 区分 authoritative gameplay action 与 client surface action；
- 不读取 Bevy physical input，不取代 Leafwing，也不进入 authoritative fixed tick。

## 规范

- [Input binding 与 context](../../../platform/input/input-binding-and-contexts.md)
- [ADR 0033](../../../decisions/0033-freeze-input-actions-bindings-and-contexts.md)

## Implementation mapping

新增 headless core + `client` feature 的 `latticeaxiom-input` crate，并适配
`latticeaxiom-player`、`latticeaxiom-runtime-contracts` 与 engine HUD/shell。任何这些现有 crates
都不能在 package manifest 与验收建立前充当该 package 已存在的证据。
