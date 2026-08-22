---
title: "@latticeaxiom/input"
document_id: package.latticeaxiom.input.index
document_status: proposed
document_type: index
owners:
  - "@latticeaxiom/input"
tracks_implementation: true
requirements:
  - INPUT-PACKAGE-001
updated: 2026-08-22
---

# `@latticeaxiom/input`

## 提案状态

实现仓当前没有该 package manifest，也没有 `latticeaxiom-input` crate。本目录记录 report
提出的 package 边界，不把它描述成 accepted 或 implemented。新增 package、capability、
`InputBindingV1` 编码与 context semantics 必须先由独立 ADR 冻结。

## 提议职责

- 声明 platform action catalog 与默认 bindings；
- 提供可版本化、可由 settings 保存的 binding profile；
- 区分 authoritative gameplay action 与 client surface action；
- 不读取 Bevy physical input，不取代 Leafwing，也不进入 authoritative fixed tick。

## 规范

- [Input binding 与 context proposal](../../../platform/input/input-binding-and-contexts.md)

## Implementation mapping

提议新增 headless core + `client` feature 的 `latticeaxiom-input` crate，并适配
`latticeaxiom-player`、`latticeaxiom-runtime-contracts` 与 engine HUD/shell。任何这些现有 crates
都不能在 package manifest 与验收建立前充当该 package 已存在的证据。

