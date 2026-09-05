---
title: "@terrenia/journey"
document_id: package.terrenia.journey.index
document_status: proposed
document_type: index
owners:
  - "@terrenia/journey"
tracks_implementation: true
requirements:
  - TERRENIA-JOURNEY-001
updated: 2026-08-24
---

# `@terrenia/journey`

- Source lifecycle：data-only manifest/source 已声明，不在当前产品 lock。
- Domains：authoritative、client。
- Requires：`@latticeaxiom/progress` progress graph。

该 package 只贡献 Terrenia chapter content，不拥有通用 progress engine。当前 chapters 为空、
没有 runtime consumer，也没有持久化或 UI evidence。

## Settings contribution

No user-configurable settings。该 package 只贡献 Terrenia chapters、objectives 与 hint content；
通用 tracker、notification 与 auto-pin preferences 由 `@latticeaxiom/progress` 拥有。未来只有
出现 Terrenia-specific、非 authoritative 的真实偏好时才新增 feature-conditional rows。

规划见[Terrenia science、magic 与 relations](../../../delivery/plans/terrenia-science-magic-and-relations.md)。
尚无获验收的 runtime crate mapping。
