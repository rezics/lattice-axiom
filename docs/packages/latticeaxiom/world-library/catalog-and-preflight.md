---
title: World catalog、preflight 与恢复
document_id: package.latticeaxiom.world-library.catalog-and-preflight
document_status: accepted
document_type: package-spec
owners:
  - "@latticeaxiom/world-library"
tracks_implementation: true
requirements: []
updated: 2026-08-22
---

# World catalog、preflight 与恢复

## Catalog

world-library 只读取 bounded header、lock metadata、checkpoint/trash metadata 与 crash marker，
产生稳定排序的 world cards。扫描阶段不得加载 native modules、运行 worldgen 或打开 writer。

每张卡区分 ready-exact、ready-compatible、missing-package、migration-required、recoverable、
read-only 与 corrupt 等状态，并提供对应可行动诊断；“无法打开”不能退化成一个通用错误。

## Preflight

选择 world 后依次验证 exact/frozen lock、required packages、schema、semantic image、active bundle、
generation epoch、world health 与 recovery mode。所有 writer authority evidence 必须在打开 storage
writer 前闭合；缺失或 stale evidence fail closed。

compatible reopen 不隐式修改 frozen lock 或 materialized chunks。upgrade 必须先 checkpoint/clone，
显示完整 graph/schema/semantic/settings diff，并产生新的明确迁移结果。

## 恢复操作

- checkpoint：记录 durable point 与关联 lock/epoch；
- clone：产生新 world identity，不复制 process-local lease；
- export：包含 bounded manifest 和 checksum，不隐含导出 secret；
- trash/restore：可恢复移动并处理 identity collision；
- read-only recovery：绝不打开 writer。

## Shell 边界

front-end 负责页面和 routing；world-library 返回 typed catalog rows、preflight progress、operations
和 diagnostics。完成 launch intent 之前，shell 仍不能持有 world writer。
