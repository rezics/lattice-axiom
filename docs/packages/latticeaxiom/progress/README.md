---
title: "@latticeaxiom/progress"
document_id: package.latticeaxiom.progress.index
document_status: proposed
document_type: index
owners:
  - "@latticeaxiom/progress"
tracks_implementation: true
requirements:
  - PROGRESS-GRAPH-001
updated: 2026-08-24
---

# `@latticeaxiom/progress`

## 身份与边界

- Source lifecycle：data-only manifest/source 已声明，不在当前产品 lock。
- Domains：authoritative、client。
- Provides：`latticeaxiom:capability/progress-graph@1` exactly-one。

该 package 预留通用进度图、稳定 chapter/state identity 与循环拒绝。当前只有 skeleton data，
没有 runtime consumer；因此不得从 manifest 推断 gameplay progression 已实现。

## Settings contribution

真实 progress runtime/UI consumer 建立后，本 package 贡献以下 feature-conditional user settings：

- `latticeaxiom:setting/interface/progress-tracker`：bool，default true；
- `latticeaxiom:setting/interface/progress-notifications`：`off | important | all`，default `all`；
- `latticeaxiom:setting/interface/auto-pin-active-objective`：bool，default true。

三者均为 user/local-user、Immediate；当前 skeleton 没有 consumer，因此不得注册或显示空 rows。
Terrenia chapter/hint policy 属于 `@terrenia/journey` content，不成为通用 UI 偏好。

## 规划来源

- [Terrenia science、magic 与 relations 计划](../../../delivery/plans/terrenia-science-magic-and-relations.md)
- [Shipped settings catalog](../settings/shipped-settings-catalog.md)

## Implementation mapping

尚无获验收的 runtime crate mapping。未来实现必须保持 graph deterministic、可持久化且不把
Terrenia 首章写死进 host。
