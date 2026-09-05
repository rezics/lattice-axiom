---
title: Input binding 与 context stack
document_id: platform.input.input-binding-and-contexts
document_status: accepted
document_type: platform-spec
owners:
  - "platform:input"
  - "@latticeaxiom/input"
tracks_implementation: true
requirements:
  - INPUT-RUNTIME-001
  - INPUT-BINDING-001
  - INPUT-CONTEXT-001
  - INPUT-ADAPTER-001
updated: 2026-08-22
decision:
  - ../../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
  - ../../decisions/0033-freeze-input-actions-bindings-and-contexts.md
---

# Input binding 与 context stack

## 状态与范围

本页落实 [ADR 0033](../../decisions/0033-freeze-input-actions-bindings-and-contexts.md)。实现仓当前的
Leafwing gameplay mapping、HUD `ButtonInput<KeyCode>` 与 shell 私有导航是迁移输入，不是允许长期
并存的产品架构。本系统统一 binding/action/context，但不建立自有 physical input backend。

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

## Stable binding 与编译

`InputBindingV1` 使用项目拥有的版本化 key/button/axis/stick 编码，不直接序列化 Bevy 或
Leafwing 类型。`BindingProfileV1` 只保存 user overrides；effective bindings 由 package defaults
与 profile 合并，未知 action overrides 保留以支持 package 暂时缺失后的恢复。

同一 context 内相同 button binding 的冲突产生稳定排序诊断并拒绝 apply；不同 context 可以
复用，例如 Esc 在 gameplay 为 pause、在 surface 为 back。

catalog 从 reopened product lock 中 exactly-one `input-actions@1` provider 编译。编译分为：

1. 验证 action ID、owner、kind、context 与 default bindings；
2. 以 user `BindingProfileV1` 覆盖 defaults，保留 unknown/orphan entries；
3. normalization 后检测同 context 冲突；
4. 生成 dense `CompiledActionIndex`、Leafwing gameplay map 与 client surface map。

生产启动不得从 `include_str!`、host 常量表或目录顺序补 action；package/catalog 与静态 client enum
的一致性由 golden test 锁死。

## Context stack

- `gameplay`：栈底，光标锁定，产生 live authoritative action frames；
- `surface`：独占，pause/shell/settings 使用，自动抑制 live gameplay；
- `hud-overlay`：overlay capture，但明确 suppress authoritative gameplay；inventory/workbench 只向
  下层透传 hotbar allowlist；
- `binding-capture`：独占捕获一个 candidate，只保留 confirm/cancel/clear safety actions。

push/pop 必须统一处理 gameplay suppression、cursor mode、focus 与 pressed-state 清理，避免关闭
菜单时把捕获键泄漏到下一 tick。`exclusive` 与 `suppress gameplay` 是两项不同政策，不能再用一个
boolean 推断。Esc native fallback 永久保留。

## Package 与 client adapter

`@latticeaxiom/input` 提供 action catalog/default bindings；headless core 负责编译、合并和冲突
检测；`client` adapter 生成 Leafwing maps 与 ClientSurfaceAction 状态。热路径使用密集 index，
不做 stable ID 字符串查找。

Controls settings rows 由 rebindable action specs 机械生成并交给 `@latticeaxiom/settings-ui`，
binding profile 使用 user scope 持久化。

业务 system 只消费 action state。原始 Bevy input 只能出现在 physical adapter、binding capture 与
Esc safety fallback。迁移范围至少覆盖 gameplay default map、HUD 的 inventory/workbench/hotbar、
shell navigation 与 pause/context integration；旧 playable fixture 冻结为测试，不提供生产 defaults。

## 验收方向

- 改键即时生效且重启保留；
- 同 context 冲突被拒绝并显示占用方；
- client input 与 headless command 产生相同 authoritative receipts；
- shell、HUD 和 gameplay 不再直接拥有平行 hard-coded mappings；
- inventory/settings 打开时无 gameplay input leakage，关闭后无 stuck input。

## 失败与恢复

- profile parse/migration 失败时保留原文件，加载 package defaults，并显示可行动诊断；
- duplicate provider、catalog/enum mismatch、unknown required binding major 在 App 建立前失败；
- rebind conflict、persistence failure 或 preview timeout 保持旧 effective map；
- missing binding 永远不能移除 Esc safety fallback；
- input failure 不得伪造 authoritative action frame 或直接执行 world command。

## 相关文件

- [ADR 0033](../../decisions/0033-freeze-input-actions-bindings-and-contexts.md)
- [`@latticeaxiom/input`](../../packages/latticeaxiom/input/README.md)
- [Game surface routing](../client-ui/game-surface-state.md)
- [Settings registry](../../packages/latticeaxiom/settings/settings-and-configuration.md)
