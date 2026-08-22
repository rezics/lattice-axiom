---
title: Shared client UI system
document_id: platform.client-ui.ui-system
document_status: accepted
document_type: platform-spec
owners:
  - "platform:client-ui"
tracks_implementation: true
requirements:
  - CLIENT-UI-001
updated: 2026-08-22
decision:
  - ../../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
  - ../../decisions/0033-freeze-input-actions-bindings-and-contexts.md
---

# Shared client UI system

## 状态与目的

本页定义 client-only `latticeaxiom-ui` crate，供 front-end、settings-ui、inspect、dev-tools 与
in-game surfaces 复用。它不是 logical package，不拥有 package graph identity；新增 crate 不改变
各 surface package 的产品职责。

技术边界沿用 accepted ADR 0025：Bevy UI、官方 widgets/focus/editable text 与 AccessKit；
Feathers 和 `bevy_dev_tools` 不进入玩家 UI。

## Crate 边界

```text
latticeaxiom-ui/
├── theme              token、字号、间距、focus ring
├── focus              键盘／手柄方向导航
├── widgets            button、toggle、slider、cycle、list、text、modal、toast
├── key-capture        InputBindingV1 捕获与冲突确认
├── semantic-projection headless semantic tree → Bevy entity tree
├── forms              typed SettingSpec → widget row
└── a11y               AccessKit semantics 与检查 helpers
```

package 贡献 typed row/fragment，不能提供任意 widgets、system callbacks 或绝对屏幕坐标。

## 两种投影

shell、pause、settings、inventory/workbench 等阻塞 surface 使用可 headless snapshot 的 semantic
tree projection；crosshair、hotbar、target summary 与 bounded diagnostics 等高频 HUD 元素直接使用
共享 theme/widgets。两者消费同一 input context/focus 与 a11y contract。

## State ownership

共享 UI crate 不让每个 screen 自建状态机。shell/game 的 screen routing、modal/overlay 互斥、Back
unwind、cursor/focus/context 派生统一由
[Client surface router](game-surface-state.md)拥有。widgets 只发 typed semantic command；它们不能
直接切换 Bevy states、修改 gameplay suppression 或抓取 cursor。

`semantic-projection` 以 stable semantic key + surface epoch 做 diff。旧 epoch 的 async list、focus
或 setting result 不得重新生成已经关闭的 screen。高频 HUD 直接控件也必须挂在当前 route epoch，
并在 owner route 退出时完整移除。

## 验收方向

- 800×600、UI scale 1.0/2.0 无溢出；
- IME、CJK fallback、鼠标、纯键盘和手柄完整可操作；
- semantic tree snapshot 和 AccessKit node 自动检查；
- shell/pause/settings 不再各自实现 focus、theme 与 input latch；
- headless profile 可完全省略 client UI crate，而 authoritative registration 不变。

## 失败行为

- required widget/control vocabulary major 不支持时，在 surface 建立前 fail closed；
- optional fragment 不支持时显示 bounded diagnostic，不执行 package callback/widget code；
- layout、focus 或 projection 失败保持 gameplay suppressed，并提供 Back/Quit safety action；
- package 不得贡献 raw Bevy systems、absolute coordinates、rich-text executable content 或第二个 root。

## 相关文件

- [Client surface routing](game-surface-state.md)
- [ADR 0025](../../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md)
- [ADR 0033](../../decisions/0033-freeze-input-actions-bindings-and-contexts.md)
