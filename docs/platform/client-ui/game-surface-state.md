---
title: Client surface routing 与游戏内状态机
document_id: platform.client-ui.game-surface-state
document_status: accepted
document_type: platform-spec
owners:
  - "platform:client-ui"
tracks_implementation: true
requirements:
  - CLIENT-SURFACE-STATE-001
updated: 2026-08-22
decision:
  - ../../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
  - ../../decisions/0033-freeze-input-actions-bindings-and-contexts.md
---

# Client surface routing 与游戏内状态机

## 范围

本页定义 shell 和 game client 的唯一 screen/surface routing contract，消除 pause latch、
inventory/workbench boolean 与各 surface 私有 gameplay suppression。它只拥有 client presentation
状态，不改变 authoritative world、inventory、crafting 或 settings transaction 的所有权。

shell 与 game 位于不同 process，不能共享 Bevy resource；它们共享 versioned route vocabulary、
semantic commands、focus/context policy 与 headless transition fixtures。

## Game route model

game process 只有一个 `GameSurfaceRouter` resource。有效 route 是有界、类型化的三层结构：

```text
base:       Playing
overlay:    None | Inventory | Workbench
modal:      None | Pause | Settings | ConfirmSaveQuit | BindingCapture
transition: None | SavingAndExiting | FatalRecovery
```

约束：

- `Inventory` 与 `Workbench` 互斥；切换时先关闭旧 overlay，再打开新 overlay；
- modal 位于 overlay 之上并暂停其交互；Resume/Back 后恢复原 overlay，不复制其业务 draft；
- `Settings` 只能从 shell 或 Pause 进入；game 中 Back 返回 Pause；
- `BindingCapture` 只能作为 Settings 的 child，结束后返回原 Controls row；
- `ConfirmSaveQuit` 确认后进入 `SavingAndExiting`，所有可交互 surface 关闭且不能重新打开；
- transition 一旦开始只接受 cancel-safe shutdown signal 或结构化失败结果，不能返回 Playing
  伪装保存成功；
- route stack 最大深度为三；未知 route/version 在建立 Bevy surface 前失败。

route 由 typed `SurfaceCommandV1` 改变，例如 ToggleInventory、OpenWorkbench、Pause、Resume、
OpenSettings、Back、RequestSaveQuit、Confirm 与 Cancel。任意 HUD/widget system 只能发送 command，
不能直接修改 route、cursor、focus 或 gameplay suppression resource。

## Shell route model

shell 的 `ShellSurfaceRouter` 固定包含 Home、Worlds、NewWorld、PackagesProfiles、Settings、
DiagnosticsAbout、Loading、Recovery 与 QuitConfirm。Loading 在 writer 打开前可以按 stage policy
取消；writer 打开后 cancel 必须进入 shutdown/durability barrier。Home 信息架构和 Continue
enablement 继续由 front-end/world-library contract 拥有。

shell route 不创建 game route。选择 world 只产生 `LaunchIntentV1` 并进入不可重复提交的 Loading/
Exiting 状态；新 game process 从 Playing base 启动。

## Route projection

每次 accepted transition 增加 `surface_epoch`，并在一个 apply stage 中完成：

1. 验证 command 对当前 route 合法；
2. 提交新 route；
3. 由 route 机械派生 `ActiveInputContextStack`；
4. 选择唯一 focus owner 与 cursor policy；
5. diff semantic tree/widgets；
6. 清除被旧 epoch 消费的 pressed/capture state；
7. 发布 route receipt/diagnostic。

映射固定为：Playing → gameplay；Inventory/Workbench → gameplay + hud-overlay；Pause/Settings/
Confirm → gameplay + surface；BindingCapture → gameplay + surface + binding-capture；transition →
exclusive surface with no world commands。input policy 的精确定义由
[ADR 0033](../../decisions/0033-freeze-input-actions-bindings-and-contexts.md)拥有。

route transition 和 UI entity diff 不得跨多个 frame 留下“新 input context + 旧按钮”或“旧 context
+ 新按钮”的混合状态。异步 catalog/settings 数据只更新当前 epoch 的 projection；stale result 丢弃。

## 业务 draft 与 authority

- inventory/workbench selection 是 overlay-owned presentation draft；authoritative inventory 只由 typed
  command receipt 修改；
- settings draft、preview 与 rollback 由 settings transaction contract 拥有；route 关闭只发 cancel/
  rollback request；
- Save & Quit 只在 world durability barrier 返回 Durable 后允许 child process 正常退出；Written
  receipt 不能投影为已保存；
- FatalRecovery 保留 bounded diagnostic/crash marker，退出后由 supervisor 启动 recovery shell；
- route、focus entity、cursor slot、Bevy Entity 与 widget handle 不进入 world save。

## 失败行为

- 非法 transition 返回稳定诊断并保持原 route；不得 panic 或“尽量切换”；
- surface spawn/projection 失败时进入可退出的 Pause/FatalRecovery fallback，并保持 gameplay suppressed；
- focus target 消失时按 stable semantic key 找最近合法 target，不按 Bevy entity/index 猜测；
- catalog epoch 变化使打开的 recipe/setting row 无效时，保留 route、清除 stale selection 并显示原因；
- binding capture、modal 或 transition 崩溃时 Esc safety path 仍能 Cancel/Back 或进入安全退出。

## 自动验收

- transition table 覆盖每个合法/非法 command、最大深度、Back unwind 与 overlay restore；
- 每个 route 的 context、cursor、focus owner 和 gameplay suppression snapshot 精确匹配；
- inventory/workbench 打开、其上 Pause、进入 Settings/BindingCapture、逐层返回后无 input leakage、
  stuck key 或重复 command；
- `SavingAndExiting` 在 Written、Durable、timeout、storage failure 下分别投影正确状态，只有 Durable
  可以正常返回 shell；
- stale async epoch 不能重建已关闭 surface；
- keyboard-only 与 gamepad-only 完成 Playing → Inventory → Pause → Settings → Back → Save & Quit；
- semantic tree 和 AccessKit node 在 800×600、scale 1.0/2.0 保持可达。

## 相关文件

- [Shared client UI system](ui-system.md)
- [Input binding 与 contexts](../input/input-binding-and-contexts.md)
- [Client shell 与 world lifecycle](../../packages/latticeaxiom/front-end/world-lifecycle-and-start-ui.md)
- [Typed settings surface](../../packages/latticeaxiom/settings-ui/settings-surface.md)
- [Player surfaces](../gameplay/player-surfaces.md)

