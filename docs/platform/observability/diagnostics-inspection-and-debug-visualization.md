---
title: Observability host adapter
document_id: platform.observability.diagnostics-inspection-and-debug-visualization
document_status: proposed
document_type: platform-spec
owners:
  - "platform:observability"
tracks_implementation: true
requirements: []
updated: 2026-08-22
decision:
  - ../../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
---

# Observability host adapter

## 责任拆分

原先单页混合了 registry、玩家 inspect 与 developer workbench。现在三个 logical packages
分别拥有产品 surface：

| Package | 责任 | 规范 |
| --- | --- | --- |
| `@latticeaxiom/observability` | registry、subscription、budget、report | [registry](../../packages/latticeaxiom/observability/registry-and-reports.md) |
| `@latticeaxiom/inspect` | target selection 的玩家信息投影 | [target inspect](../../packages/latticeaxiom/inspect/target-inspect.md) |
| `@latticeaxiom/dev-tools` | F3 panel、graphs、world visualizers | [workbench](../../packages/latticeaxiom/dev-tools/debug-workbench.md) |

本 platform 页面只定义 host 如何激活 provider、采集资料并保护 authoritative simulation。

## Registration 与 activation

locked registration image 在 code activation 前合并 diagnostic/inspect/visualizer descriptors，验证
owner、schema、audience、cost 和 cardinality。host 按 runtime dense index 激活，不根据 Terrenia
或 crate 名称添加隐式 fallback。

client、headless 与 tool 可以选择不同 presentation packages，但 authoritative diagnostic IDs、
receipt fields 与 deterministic report ordering 保持一致。

## Data plane

host 暴露 bounded、typed snapshots 或 batches，不给 portable modules Bevy World、RenderWorld、
wgpu objects 或任意 component access。采集由 active subscriptions 驱动，按 cadence、CPU、memory、
readback 与 retained samples hard caps 合并。

昂贵 world-space geometry 在 owning subsystem 的安全边界产生，并通过 bounded descriptor 交给
client renderer；visualizer 不能改变 chunk residency、physics query、worldgen 或 render graph 的
authoritative decisions。

## Fault isolation

provider panic、invalid row、timeout、device loss、unsupported feature 或 budget rejection 产生结构化
diagnostic，并只禁用相关 item/surface。presentation fault 不改变 world state、snapshot bytes、
fixed tick 或 product lock。

## Implementation mapping

候选实现分布在 `latticeaxiom-runtime-contracts`、`latticeaxiom-compose`、
`latticeaxiom-render-contracts` 与 `latticeaxiom-engine` adapters。任何单一 crate 的 DTO/test 都不能
证明三个 package surface 或 host data plane 已完整实现。

## 验收方向

- duplicate/missing providers 在 activation 前失败；
- 无订阅时昂贵 metric/geometry 不采集；
- budget fault 只产生 degraded/rejected surface；
- headless report 与 client report 共享 stable schema/order；
- inspect、dev-tools 和 package-contributed rows 不直接占 HUD 坐标；
- fault corpus 证明 observability 不改变 authoritative hash。
