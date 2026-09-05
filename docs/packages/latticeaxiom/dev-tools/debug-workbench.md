---
title: Developer diagnostic workbench
document_id: package.latticeaxiom.dev-tools.debug-workbench
document_status: accepted
document_type: package-spec
owners:
  - "@latticeaxiom/dev-tools"
tracks_implementation: true
requirements:
  - DEV-TOOLS-001
updated: 2026-08-22
---

# Developer diagnostic workbench

## 范围

本 package 提供 opt-in F3 类 surface：指标表、时间序列、chunk/physics/render visualizer 选择、
package/lock 摘要与 bounded report export。release profile 可以完全省略它。

## 组合规则

package 只能注册 typed metric、row、graph 或 world-space visualizer descriptor。dev-tools 统一
决定布局、focus、颜色和空间 drawing budget；package 不得直接占 HUD 坐标或插入任意 systems。

visualizer activation 通过 observability subscription 和 settings transaction，必须显示成本、
权限、degraded/rejected 原因与 active budget。device loss、missing GPU feature 或 provider fault
只能关闭该 visualizer，不得影响 authoritative simulation。

## 首个 workbench

首版至少覆盖 frame/fixed tick、resident/active/visible/in-flight chunks、mesh/collider queues、
worldgen/storage receipts、player target 与 lock/registration fingerprints。chunk bounds、colliders
和 generation regions 是 bounded visualizers，不是永久后台采集。

## 非目标

- 不把玩家 inspect 放进 developer panel。
- 不提供 unrestricted ECS inspector 或 arbitrary code execution。
- 不让 debug setting 改写 frozen world-authoritative settings。

