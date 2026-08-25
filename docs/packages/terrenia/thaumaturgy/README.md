---
title: "@terrenia/thaumaturgy"
document_id: package.terrenia.thaumaturgy.index
document_status: proposed
document_type: index
owners:
  - "@terrenia/thaumaturgy"
tracks_implementation: true
requirements:
  - TERRENIA-THAUMATURGY-001
updated: 2026-08-24
---

# `@terrenia/thaumaturgy`

- Source lifecycle：data-only manifest/source 已声明，不在当前产品 lock。
- Domain：authoritative。
- Provides：`terrenia:capability/thaumaturgy@1` exactly-one。

该 package 预留 ritual、mana state 与独立 magic progression。当前 rituals 为空且没有 runtime
consumer，不得因 capability manifest 存在而宣称 magic gameplay 已实现。

## Settings contribution

No user-configurable settings。ritual、mana 与 magic progression 是 authoritative content/state；
当前 skeleton 没有 presentation 或 world-owner setting consumer，不注册空 rows。

规划见[Terrenia science、magic 与 relations](../../../delivery/plans/terrenia-science-magic-and-relations.md)。
尚无获验收的 runtime crate mapping。
