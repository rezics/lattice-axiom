---
title: Diagnostic registry 与报告
document_id: package.latticeaxiom.observability.registry-and-reports
document_status: accepted
document_type: package-spec
owners:
  - "@latticeaxiom/observability"
tracks_implementation: true
requirements: []
updated: 2026-08-22
---

# Diagnostic registry 与报告

## 职责

`@latticeaxiom/observability` 合并 locked graph 中的 `InfoItemSpec`、`DiagnosticMetricSpec`、
inspect fragment metadata 与 visualizer metadata，验证 stable ID、owner、cost、permission 和
fallback，然后编译 deterministic catalog。

registry 不渲染任何 UI。`@latticeaxiom/inspect` 与 `@latticeaxiom/dev-tools` 是不同的
presentation consumers。

## Subscription 与成本

metric 和昂贵 debug data 默认不采集。consumer 以稳定 ID 订阅，并声明 update cadence、sample
count、CPU/memory/readback budget 与权限。host 合并订阅、应用 hard caps，并向 consumer 返回
active/degraded/rejected 状态；package 不能通过注册永久开启昂贵采集。

## Diagnostic report

可导出的 report 使用稳定 schema 和 deterministic ordering，包含产品 lock、engine build、
world identity/epoch、active subscriptions、bounded metrics 与截断诊断。report 不包含 secret、
未授权 world content、process-local handles 或无限日志。

同一 report schema 可被 client、headless 和 tool 消费。presentation failure 不得改变
authoritative world state。

## 权限与失败

每个 item 声明 audience 与信息权限。缺失 provider、重复 exactly-one registry、unknown required
schema 或超出 hard budget 必须在 activation 或 subscription 时产生结构化诊断；不得静默退回
host hard-coded item。

