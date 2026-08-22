---
title: Typed settings surface
document_id: package.latticeaxiom.settings-ui.settings-surface
document_status: accepted
document_type: package-spec
owners:
  - "@latticeaxiom/settings-ui"
tracks_implementation: true
requirements:
  - SETTINGS-SURFACE-001
updated: 2026-08-22
---

# Typed settings surface

## 范围

本页定义 settings catalog 到 client/tool UI 的机械投影。setting 的 ID、scope、authority、
validation、effective value 与 transaction 语义由 [`@latticeaxiom/settings`](../settings/README.md)
拥有；本 package 只拥有 surface。

## 固定分类与控件词汇表

首版分类顺序固定为 Accessibility、Controls、Audio、Video、Interface、Gameplay、World、
Packages、Developer。package 可以把 typed `SettingSpec` 放入已有分类，但不能增加任意代码
editor 或直接绘制 overlay。

V1 控件来自版本化词汇表：toggle、bounded integer/float slider、enum cycle、text、path、
color、key binding、read-only value 与 command row。未知 required control major 必须在 surface
建立前失败；optional row 可以显示明确 unsupported 诊断。

## Draft、preview 与 apply

UI 维护 draft，不在编辑每一行时直接修改 authoritative state：

1. registry 产生 current/effective snapshot；
2. UI 在 scope/authority 允许时修改 draft；
3. live-reversible 项可以 preview，并保存 rollback value；
4. apply 发送 typed transaction；
5. registry 返回 applied/rejected/restart-required impact；
6. 成功后由 owning scope persistence 保存，而不是 UI 自行写文件。

离开页面、transaction 失败或 preview 超时必须 rollback。world-authoritative 项在没有 writer
authority 时只读显示。

## Controls 与 key binding

Controls 分类消费 input package 产生的 rebindable action rows。key-capture 控件进入独占捕获
状态，接收一个稳定 `InputBindingV1`，运行同 context 冲突检查，然后确认、取消或清除。
物理输入仍来自 Bevy／Leafwing；UI 不建立第二个 input backend。

在 `@latticeaxiom/input` ADR 与 package 尚未接受前，Controls rows 保持 proposed，不能因
`SettingValueV1::key-binding` DTO 已存在就宣称改键已实现。

## 可访问性与输入

surface 必须使用 Bevy UI、官方 focus/editable text 和 AccessKit 语义；鼠标、纯键盘和手柄均
可完成搜索、编辑、apply 与 cancel。800×600、UI scale 1.0/2.0、IME 与 CJK fallback 是首版
验收场景。Esc 保留 native safety fallback。

## 非目标

- 不拥有 setting persistence 或 migration。
- 不让 package 注入任意 Bevy systems/widgets。
- 不把 Feathers 或 developer-only widgets 引入玩家 surface。
- 不用硬编码页面替代 typed catalog。

