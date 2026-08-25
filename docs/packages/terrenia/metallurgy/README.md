---
title: "@terrenia/metallurgy"
document_id: package.terrenia.metallurgy.index
document_status: proposed
document_type: index
owners:
  - "@terrenia/metallurgy"
tracks_implementation: true
requirements:
  - TERRENIA-METALLURGY-001
updated: 2026-08-24
---

# `@terrenia/metallurgy`

- Source lifecycle：data-only manifest/source 已声明，不在当前产品 lock。
- Domain：authoritative。
- Provides：`terrenia:capability/metallurgy@1` exactly-one。

该 package 预留 copper、annealed copper、bronze、brass 与相关材料语义。当前仅有 data
skeleton，没有 production processing、recipe journey 或 persistence evidence。

## Settings contribution

No user-configurable settings。材料性质、配方、温度或 processing balance 属于 authoritative
content/rules；当前 skeleton 没有可配置 runtime consumer，不注册空 rows。

规划见[Terrenia science、magic 与 relations](../../../delivery/plans/terrenia-science-magic-and-relations.md)。
尚无获验收的 runtime crate mapping。
