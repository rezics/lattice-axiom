---
title: "@latticeaxiom/relations"
document_id: package.latticeaxiom.relations.index
document_status: proposed
document_type: index
owners:
  - "@latticeaxiom/relations"
tracks_implementation: true
requirements:
  - RELATIONS-GRAPH-001
updated: 2026-08-22
---

# `@latticeaxiom/relations`

## 身份与边界

- Source lifecycle：data-only manifest/source 已声明，不在当前产品 lock。
- Domain：authoritative。
- Provides：`latticeaxiom:capability/relations-graph@1` exactly-one。

该 package 预留 pet／friend／subject／companion 等通用关系类型与持久图语义。当前只有
skeleton data，没有 runtime consumer、交互或存档闭环。

## 规划来源

- [Terrenia science、magic 与 relations 计划](../../../delivery/plans/terrenia-science-magic-and-relations.md)

## Implementation mapping

尚无获验收的 runtime crate mapping。未来实现不得依赖 Terrenia concrete IDs，也不得把
process-local entity handles 写入存档。

