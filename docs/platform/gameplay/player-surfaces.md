---
title: Inventory、hotbar、crafting 与 player commands
document_id: platform.gameplay.player-surfaces
document_status: accepted
document_type: platform-spec
owners:
  - "platform:gameplay"
tracks_implementation: true
requirements:
  - PLAYER-MECHANICS-001
updated: 2026-08-22
decision:
  - ../../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
  - ../../decisions/0027-freeze-authoritative-world-and-persistence-contract.md
  - ../../decisions/0033-freeze-input-actions-bindings-and-contexts.md
---

# Inventory、hotbar、crafting 与 player commands

## 边界

inventory、hotbar、drops、pickup、mining progress、tool durability、recipes、workstation 与
placement transaction 是可跨维度复用的 authoritative mechanics。Terrenia packages 贡献内容和
规则，不允许 host 写死 `terrenia:*` IDs。

client UI 只发送带 expected revision 的 typed commands，并投影 authoritative receipts；它不维护
第二份 inventory authority，也不因动画成功提前修改数量。

## Surface 行为

- hotbar 是 inventory container 的选中窗口，不是独立 client array；
- pick-block 选择已有匹配 stack，不创造 item；
- inventory move/swap/split 与 crafting 使用 atomic command；
- crafting surface 读取 locked recipe catalog，不把 3×3 猜配方写死在 UI；
- full inventory、wrong tool、broken tool、stale revision 与 failed craft 返回可行动拒绝。

打开 inventory/workbench 时由 input context stack 抑制 live gameplay；允许的 hotbar selection
必须作为显式 context rule，而不是 HUD 私有 `ButtonInput<KeyCode>`。

overlay 的互斥、Pause 嵌套、Back unwind、focus/cursor 与关闭后的 pressed-state cleanup 由统一
[client surface router](../client-ui/game-surface-state.md)拥有。inventory/workbench system 不能用
boolean 或 latch 直接修改 UI/gameplay state。

## 持久化与守恒

player inventory、selected slot、durability、drops 与使用中的 container/workstation state 属于
authoritative snapshot。command 成功、crash、unload/reload 和 stale retry 都不得复制或丢失
quantity/revision。

## Headless 等价

自动 journey 通过同一 public command DTO 完成 gather → pickup → craft → equip → mine → place；
client input 只是在上游生成相同 command。fixture dimension 必须证明通用 mechanics 不依赖
Terrenia concrete IDs。
