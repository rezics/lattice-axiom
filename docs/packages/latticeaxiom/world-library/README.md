---
title: "@latticeaxiom/world-library"
document_id: package.latticeaxiom.world-library.index
document_status: accepted
document_type: index
owners:
  - "@latticeaxiom/world-library"
tracks_implementation: true
requirements:
  - WORLD-LIBRARY-001
updated: 2026-08-22
---

# `@latticeaxiom/world-library`

## 身份与边界

- Source lifecycle：manifest/source 已声明；不在当前审计的 `client-world` lock。
- Domains：client、tool。
- Provides：`latticeaxiom:capability/world-catalog@1` exactly-one。
- Requires：diagnostic registry。

本 package 拥有 world header scan、健康与 lock 摘要、preflight、checkpoint、clone、export、
trash 和 restore 的目录面。它不渲染首页，不解析 gameplay graph，也不能在 preflight 成功前
打开 world writer。

## 规范

- [World catalog、preflight 与恢复](catalog-and-preflight.md)
- [Shell 组合边界](../front-end/world-lifecycle-and-start-ui.md)
- [World storage contract](../../../platform/world-storage/world-persistence.md)

## Implementation mapping

主要候选 crates 为 `latticeaxiom-world-catalog`、`latticeaxiom-world-wire`、
`latticeaxiom-world-db` 和 client shell adapter。目录扫描或 DTO 单测不能单独证明完整
world-library product flow。

