---
title: Shell 与 game supervisor
document_id: platform.launcher.supervisor-loop
document_status: accepted
document_type: platform-spec
owners:
  - "platform:launcher"
tracks_implementation: true
requirements:
  - LAUNCHER-SUPERVISOR-001
  - LAUNCHER-RESULT-001
updated: 2026-08-22
decision:
  - ../../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
---

# Shell 与 game supervisor

## 问题

accepted process model 要求 shell 与 world 使用 replacement process；当前 report 确认 shell 写入
`LaunchIntentV1` 后退出，但没有监督者读取 intent 并启动 game，因此菜单不是产品入口。

## 监督循环

```text
launcher supervisor
  → spawn shell with reopened shell lock
  → atomically consume child exit + optional LaunchIntentV1
     ├── shell + no intent: product exits
     ├── shell + World intent: spawn game with selected frozen world lock
     ├── game + Shell intent after durable barrier: spawn normal shell
     └── crash/timeout/missing game intent: quarantine generation, spawn recovery shell
```

监督者不创建 Bevy App 或 window，复用 `latticeaxiom-launcher` 的 lease、intent store 与 fail-closed
validation。任一 child 崩溃、intent stale、lock mismatch 或 shutdown timeout 都不能绕过下一轮
world preflight。

## Child result 与 intent protocol

每个 child generation 有唯一 lease。child 正常退出前原子发布 `ChildExitReportV1`，至少包含 child
generation、Shell/World role、exit kind、intent generation、最后确认的 setting revision，以及 game
适用的 last-written/last-durable world revision。report 只保存 bounded diagnostic reference，不复制
world data 或 secret。

`LaunchIntentV1` 对 shell→world 与 world→shell 都是一次性 transition：

- shell 只有在 preflight 成功、自己的 settings flush 完成后才能发布 World intent；
- game 只有在 Save & Quit durability barrier 返回 Durable、writer/task/package shutdown 达到安全点后
  才能发布 Shell intent；
- Written receipt、窗口强杀、panic、timeout 或 report/intent 不匹配不能伪装正常退出；
- supervisor 以 generation + role + lock hashes + open-plan hash 验证并原子 ack/quarantine；旧 generation
  永远不能再次启动 world。

crash/timeout 只允许同 generation 一次 recovery-shell hop。recovery shell 若无新用户选择则退出产品；
不得自动重试旧 World intent。spawn failure、recovery shell failure 与连续 child failure 形成 bounded
terminal diagnostic，而不是无限 restart loop。

## 产品入口

新增 `task play` 作为菜单驱动的产品入口；`task dev` 可以保留直进开发世界，但 release/client
smoke 必须走一次 shell → world → shell。supervisor 只编排进程，不授予 world writer authority。

`task play` 启动 supervisor，而不是直接启动 shell profile。release bundle、桌面快捷方式和 CI product
smoke 必须指向同一 executable entry；`profiles/shell.toml` 只能作为 child/debug entry。

## 验收方向

- Home → select/create world → game → Save & Quit → Home 完成；
- 无 intent 的 shell exit 正常结束，不误启动旧 world；
- crash marker 在下轮 shell 形成 recovery surface；
- 同时最多一个 window App，lease 与 intent 均原子消费；
- product smoke 使用 reopened locks，而不是 test-only fixture。

## 失败行为

- corrupt/stale/tampered intent 在 child spawn 前 quarantine，并进入 recovery shell；
- lease conflict 不杀死未知 owner，也不并发打开第二个 window App；
- game shutdown timeout 保留最新 Durable revision，标记 NeedsRecovery，不能显示“已保存”；
- supervisor 自身崩溃后可从 lease、intent quarantine 和 crash marker 恢复，但绝不推断 writer authority；
- shell Quit、game Save & Quit、OS close 与 crash 在 report 中是不同 exit kind。

## 相关文件

- [ADR 0025 process restart](../../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md)
- [Client shell 与 world lifecycle](../../packages/latticeaxiom/front-end/world-lifecycle-and-start-ui.md)
- [Client surface routing](../client-ui/game-surface-state.md)
- [World persistence](../world-storage/world-persistence.md)
