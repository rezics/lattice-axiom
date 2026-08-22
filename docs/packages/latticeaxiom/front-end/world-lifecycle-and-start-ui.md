---
title: Client shell 与安全 world launch
document_id: package.latticeaxiom.front-end.world-lifecycle-and-start-ui
document_status: accepted
document_type: package-spec
owners:
  - "@latticeaxiom/front-end"
tracks_implementation: true
requirements:
  - FRONTEND-SHELL-001
updated: 2026-08-22
decision:
  - ../../../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
  - ../../../decisions/0027-freeze-authoritative-world-and-persistence-contract.md
---

# Client shell 与安全 world launch

## 结论

开始页不是 host 手写菜单，而是 `ClientShellGraph` 选择的 `@latticeaxiom/front-end` surface。
shell 只消费 typed world catalog、settings 和 diagnostics；选择 world 后写入原子
`LaunchIntentV1`，由 replacement-process supervisor 启动 locked game graph。

## 两个 graph、两种权限

```text
ClientShellGraph
  @latticeaxiom/front-end
  @latticeaxiom/world-library
  @latticeaxiom/settings
  @latticeaxiom/settings-ui
  @latticeaxiom/observability

LockedGameGraph
  exact per-world package closure
  exact registration/semantic/settings fingerprints
  writer authority only after preflight evidence
```

两者使用同一 resolver、lock schema 与 registration pipeline。shell graph 可以读 bounded world
metadata，但不能加载 world native code、运行 worldgen 或打开 writer；game graph 不能继承 shell
临时 draft 作为未锁定权威配置。

## 信息架构

Home 顺序固定为 Continue、Worlds、New World、Packages/Profiles、Settings、Diagnostics/About、
Quit。Continue 只对 ready-exact world 可用；其他状态进入明确的 preflight/recovery flow。

Worlds 和 New World 的资料、操作与诊断来自
[`@latticeaxiom/world-library`](../world-library/catalog-and-preflight.md)。Settings 使用
[`@latticeaxiom/settings-ui`](../settings-ui/settings-surface.md)。package 只能贡献 typed rows/
fragments，不能占据绝对屏幕坐标或替换安全操作。

## Launch 与返回

shell 完成 catalog selection 和 preflight 后写入一次性、原子、带 shell/world lock fingerprint 的
`LaunchIntentV1`，完成自己的 durable shutdown 再退出。supervisor 验证并消费 intent，启动
game；game Save & Quit 完成 world durability barrier、发布匹配的 Shell intent/child result 后退出，
supervisor 再启动 shell。

没有 intent 的 shell exit 结束产品；stale/tampered intent、child crash 或 shutdown timeout 进入
结构化诊断/recovery，不复用旧 intent。详细循环见
[supervisor contract](../../../platform/launcher/supervisor-loop.md)。

## Loading 与取消

surface 显示 checking → resolving → acquiring/building → validating → opening → playing 的阶段与
是否可安全取消。writer 打开后 cancel 必须走有界 shutdown/durability barrier；“已保存”文案
区分 written 与 durable。

## 首版验收方向

- `task play` 完成 Home → world → game → Save & Quit → Home；
- world/lock/package/schema failure 在第一项 mutation 前呈现；
- shell process 从不持有 world writer；
- crash marker、read-only recovery、checkpoint/clone/trash/restore 有明确 surface；
- mouse、keyboard、gamepad 与 AccessKit 语义完成全程；
- 开发直进世界不能成为 release smoke 的唯一入口。

shell 与 game 内部 surface transition 使用统一
[client surface router](../../../platform/client-ui/game-surface-state.md)，不能以 host boolean/latch
替代 route、focus 与 input context contract。

## 相关文件

- [World catalog 与 preflight](../world-library/catalog-and-preflight.md)
- [Settings UI](../settings-ui/settings-surface.md)
- [World persistence](../../../platform/world-storage/world-persistence.md)
- [Client UI system](../../../platform/client-ui/ui-system.md)
