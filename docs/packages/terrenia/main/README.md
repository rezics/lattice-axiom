---
title: terrenia
document_id: package.terrenia.main.index
document_status: accepted
document_type: index
owners:
  - "terrenia"
tracks_implementation: true
requirements:
  - TERRENIA-ROOT-001
updated: 2026-08-22
---

# `terrenia`

## 身份与边界

- Source lifecycle：当前 `client-world` lock 的 root package。
- Domains：authoritative、client。
- Depends：blocks、worldgen、gameplay、tools、presentation。

`terrenia` 是当前维度聚合 package，不是 Lattice Axiom core。它注册维度身份、namespace
grants、closure 与 semantic role bindings；具体方块、系统和资产继续由子 packages 拥有。

## 规范

- [Repository 与 package layout](../../../meta/repository-and-package-layout.md)
- [Semantic registration](../../../platform/registration/semantic-registration.md)
- [v1 playable plan](../../../delivery/plans/v1-playable.md)

## Implementation mapping

聚合由 package manifests、composition/registration crates 与 engine runtime image 消费实现。
当前 lock 包含该 root 只能证明 closure 可解析，不能证明完整 Terrenia v1 journey。

