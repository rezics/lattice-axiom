---
title: "@terrenia/science"
document_id: package.terrenia.science.index
document_status: proposed
document_type: index
owners:
  - "@terrenia/science"
tracks_implementation: true
requirements:
  - TERRENIA-SCIENCE-001
updated: 2026-08-24
---

# `@terrenia/science`

- Source lifecycle：data-only manifest/source 已声明，不在当前产品 lock。
- Domain：authoritative。
- Provides：`terrenia:capability/science-processing@1` exactly-one。
- Optional dependency：`@terrenia/metallurgy`。

该 package 预留 science processing、voltage state 与可选 metallurgy composition。当前 recipes
为空且没有 runtime consumer，因此属于 scaffold proposal。

## Settings contribution

No user-configurable settings。voltage、processing、recipe 与 optional metallurgy composition 是
authoritative schema/profile choice；当前 skeleton 没有 presentation 或 world-owner setting
consumer，不注册空 rows。

规划见[Terrenia science、magic 与 relations](../../../delivery/plans/terrenia-science-magic-and-relations.md)。
尚无获验收的 runtime crate mapping。
