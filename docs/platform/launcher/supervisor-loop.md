---
title: Shell 与 game supervisor 提案
document_id: platform.launcher.supervisor-loop
document_status: proposed
document_type: platform-spec
owners:
  - "platform:launcher"
tracks_implementation: true
requirements:
  - LAUNCHER-SUPERVISOR-001
updated: 2026-08-22
---

# Shell 与 game supervisor 提案

## 问题

accepted process model 要求 shell 与 world 使用 replacement process；当前 report 确认 shell 写入
`LaunchIntentV1` 后退出，但没有监督者读取 intent 并启动 game，因此菜单不是产品入口。

## 提议循环

```text
launcher supervisor
  → spawn shell with shell lock
  → shell exits
  → atomically consume LaunchIntentV1
     ├── no intent: product exits
     └── intent: spawn game with selected frozen world lock
          → game durable shutdown
          → return to shell, carrying bounded crash/recovery result
```

监督者不创建 Bevy App 或 window，复用 `latticeaxiom-launcher` 的 lease、intent store 与 fail-closed
validation。任一 child 崩溃、intent stale、lock mismatch 或 shutdown timeout 都不能绕过下一轮
world preflight。

## 产品入口

新增 `task play` 作为菜单驱动的产品入口；`task dev` 可以保留直进开发世界，但 release/client
smoke 必须走一次 shell → world → shell。supervisor 只编排进程，不授予 world writer authority。

## 验收方向

- Home → select/create world → game → Save & Quit → Home 完成；
- 无 intent 的 shell exit 正常结束，不误启动旧 world；
- crash marker 在下轮 shell 形成 recovery surface；
- 同时最多一个 window App，lease 与 intent 均原子消费；
- product smoke 使用 reopened locks，而不是 test-only fixture。

