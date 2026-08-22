---
title: Shared client UI system 提案
document_id: platform.client-ui.ui-system
document_status: proposed
document_type: platform-spec
owners:
  - "platform:client-ui"
tracks_implementation: true
requirements:
  - CLIENT-UI-001
updated: 2026-08-22
---

# Shared client UI system 提案

## 状态与目的

本页提出 client-only `latticeaxiom-ui` crate，供 front-end、settings-ui、inspect、dev-tools 与
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

## 验收方向

- 800×600、UI scale 1.0/2.0 无溢出；
- IME、CJK fallback、鼠标、纯键盘和手柄完整可操作；
- semantic tree snapshot 和 AccessKit node 自动检查；
- shell/pause/settings 不再各自实现 focus、theme 与 input latch；
- headless profile 可完全省略 client UI crate，而 authoritative registration 不变。

