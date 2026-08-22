---
title: Input binding 与 context stack 提案
document_id: platform.input.input-binding-and-contexts
document_status: proposed
document_type: platform-spec
owners:
  - "platform:input"
  - "@latticeaxiom/input"
tracks_implementation: true
requirements:
  - INPUT-RUNTIME-001
updated: 2026-08-22
---

# Input binding 与 context stack 提案

## 状态与范围

本页把 report 中的输入方案整理成可评审边界，尚未成为 accepted contract。实现仓目前有
Leafwing gameplay mapping、HUD 直接 `ButtonInput<KeyCode>` 与 shell 私有导航三条路径；本提案
统一 binding/action/context，但不建立自有 physical input backend。

新增 `@latticeaxiom/input`、`latticeaxiom-input` crate、action capability、binding encoding 与
context semantics 前必须新增 ADR。

## 分层

```text
Bevy／Leafwing physical input
  → stable binding profile
  → action catalog
     ├── PlayerActionV1（authoritative fixed tick）
     └── ClientSurfaceAction（UI／HUD，不进 authoritative tick）
  → active context stack
```

`PlayerActionV1` 的既有 discriminants、`PlayerActionFrameV1` 和 headless command injection 不变。
HUD 的 inventory/workbench/hotbar 与 UI navigation 不能塞进 authoritative action enum。

## Stable binding

`InputBindingV1` 使用项目拥有的版本化 key/button/axis/stick 编码，不直接序列化 Bevy 或
Leafwing 类型。`BindingProfileV1` 只保存 user overrides；effective bindings 由 package defaults
与 profile 合并，未知 action overrides 保留以支持 package 暂时缺失后的恢复。

同一 context 内相同 button binding 的冲突产生稳定排序诊断并拒绝 apply；不同 context 可以
复用，例如 Esc 在 gameplay 为 pause、在 surface 为 back。

## Context stack

- `gameplay`：栈底，光标锁定，产生 live authoritative action frames；
- `surface`：独占，pause/shell/settings 使用，自动抑制 live gameplay；
- `hud-overlay`：按声明选择是否独占，inventory/workbench 等不再各自管理 latch。

push/pop 必须统一处理 gameplay suppression、cursor mode、focus 与 pressed-state 清理，避免关闭
菜单时把捕获键泄漏到下一 tick。Esc native fallback 永久保留。

## Package 与 client adapter

`@latticeaxiom/input` 提供 action catalog/default bindings；headless core 负责编译、合并和冲突
检测；`client` adapter 生成 Leafwing maps 与 ClientSurfaceAction 状态。热路径使用密集 index，
不做 stable ID 字符串查找。

Controls settings rows 由 rebindable action specs 机械生成并交给 `@latticeaxiom/settings-ui`，
binding profile 使用 user scope 持久化。

## 验收方向

- 改键即时生效且重启保留；
- 同 context 冲突被拒绝并显示占用方；
- client input 与 headless command 产生相同 authoritative receipts；
- shell、HUD 和 gameplay 不再直接拥有平行 hard-coded mappings；
- inventory/settings 打开时无 gameplay input leakage，关闭后无 stuck input。

