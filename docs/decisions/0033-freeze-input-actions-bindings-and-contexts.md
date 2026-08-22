---
title: 冻结输入动作、绑定档案与 surface context 契约
document_id: decision.0033-freeze-input-actions-bindings-and-contexts
document_status: accepted
document_type: decision
tracks_implementation: false
updated: 2026-08-22
---

# 决策 0033：冻结输入动作、绑定档案与 surface context 契约

## 背景

[决策 0025](0025-freeze-client-shell-settings-observability-and-player-contracts.md)已经冻结
`PlayerActionV1`、`InputBindingV1`、Leafwing adoption、Bevy UI/focus 与设置值边界，但没有冻结
动作目录、用户绑定档案、surface action、context arbitration 和它们进入产品 lock 的方式。

当前实现因此形成三条生产输入路径：gameplay 使用 Leafwing，HUD 直接读取 Bevy key state，shell
维护私有导航键。pause、inventory 与 workbench 又分别管理 gameplay suppression、光标和 focus。
继续在 host 中增加快捷键会让改键、手柄、无障碍和 headless parity 永远无法形成一个可验证闭环。

## 决策

### 1. 一个动作目录、两个动作域

首版只有一个由 locked registration image 编译出的 action catalog。每个 `ActionSpecV1` 至少包含：

- versioned stable action ID；
- `Button` 或 `Axis2` kind；
- owning input context；
- rebindable policy；
- 有稳定顺序的平台默认 bindings。

动作分为两个域：

- `PlayerActionV1` 是既有 authoritative fixed-tick 动作；其 discriminant、
  `PlayerActionFrameV1` 与 headless command injection 不改变；
- `ClientSurfaceActionV1` 只用于 shell、HUD 与 UI，不进入 authoritative tick、save 或 portable ABI。

首版 `ClientSurfaceActionV1` 固定包含方向导航、next/previous focus、activate、back、pause、
inventory、workbench、hotbar slot 1–9 与 hotbar next/previous。package-defined dynamic actions延后到
新的 capability major；不得用 runtime 字符串动作绕过静态 client adapter。

首版 surface action IDs 与出厂默认如下；键盘名称表示物理 key usage，手柄名称使用 standard
gamepad mapping。package 数据必须与静态 enum golden 一致：

| Stable action ID | Context | Keyboard default | Gamepad default |
| --- | --- | --- | --- |
| `latticeaxiom:action/ui/nav-up@1` | surface | ArrowUp | DPadUp |
| `latticeaxiom:action/ui/nav-down@1` | surface | ArrowDown | DPadDown |
| `latticeaxiom:action/ui/nav-left@1` | surface | ArrowLeft | DPadLeft |
| `latticeaxiom:action/ui/nav-right@1` | surface | ArrowRight | DPadRight |
| `latticeaxiom:action/ui/nav-next@1` | surface | Tab | RightShoulder |
| `latticeaxiom:action/ui/nav-previous@1` | surface | Shift+Tab | LeftShoulder |
| `latticeaxiom:action/ui/activate@1` | surface | Enter | South |
| `latticeaxiom:action/ui/back@1` | surface | Escape | East |
| `latticeaxiom:action/gameplay/pause@1` | gameplay | Escape | Start |
| `latticeaxiom:action/hud/toggle-inventory@1` | gameplay | KeyE | Select |
| `latticeaxiom:action/hud/toggle-workbench@1` | gameplay | KeyC | North |
| `latticeaxiom:action/hud/hotbar-slot-1@1` … `hotbar-slot-9@1` | gameplay | Digit1 … Digit9 | — |
| `latticeaxiom:action/hud/hotbar-next@1` | gameplay | — | RightTrigger |
| `latticeaxiom:action/hud/hotbar-previous@1` | gameplay | — | LeftTrigger |

`hud-overlay` 的 lower-action allowlist 只包含打开该 overlay 的 toggle、hotbar slots、hotbar
next/previous 与 Back；它不等价于允许所有 gameplay actions。若未来改默认绑定，只改变 package
default revision；stable action meaning 不变，用户 override 继续优先。

### 2. `@latticeaxiom/input` 是 graph-selected foundation package

新增 logical package `@latticeaxiom/input`，提供 exactly-one
`latticeaxiom:capability/input-actions@1`。它通过版本化 registration batch 声明 action catalog 和
default bindings。产品只能从 reopened final lock 编译该目录；host 不得使用 `include_str!`、目录
扫描或隐藏 fallback 拼出第二份生产目录。

Rust 实现映射为 headless-first `latticeaxiom-input` crate 与可选 `client` feature。crate 名不取得
package identity；headless 可以完全省略 Bevy/Leafwing adapter，但仍消费同一 authoritative action
DTO。catalog 编译后使用密集 index，fixed-tick 热路径不得查找 stable ID 字符串。

### 3. 稳定 binding 与用户档案

`InputBindingV1` 是项目拥有的 versioned DTO，不序列化 Bevy、Leafwing、winit 或平台私有类型。
首版 vocabulary 覆盖：

- 物理键盘 usage 与明确 modifier set；文本输入仍走 IME/composition，不把字符绑定当文本编辑；
- standard mouse buttons、motion 与 wheel axis；
- standard gamepad buttons 与 sticks，包含有界 deadzone/sensitivity；
- 未知 newer-minor 项的保留与可诊断 round-trip。

`BindingProfileV1` 以 action stable ID 映射到有稳定顺序的 binding list。缺失 entry 表示继承
package default，存在的空 list 表示用户明确 unbind；未知 action entry 保留，package 暂时缺失不能
删除用户数据。effective bindings 固定为 defaults 与 user override 的确定性 overlay。

同一 context 内，同一个 button-like binding 不能绑定到多个会同时触发的动作；apply 返回按 action
stable ID 排序的占用方诊断。跨 context 允许复用。axis/stick 冲突按 component、direction、deadzone
和 modifier 的规范化 tuple 判定，不能依赖 Leafwing debug string。

键鼠 binding 首版可重绑；standard gamepad 首版可以使用固定映射。未来开放手柄重绑不改变
`PlayerActionV1`，但必须通过新的 UI acceptance evidence。

### 4. Context policy 不用一个布尔值表达

`ActiveInputContextStack` 是 shell/game client 唯一的输入仲裁资源。每个 context 明确声明三项政策：

- capture：`Exclusive` 或 `Overlay`；
- authoritative gameplay：`Allow` 或 `Suppress`；
- cursor/focus：locked gameplay 或 visible surface ownership。

首版固定语义：

| Context | Capture | Authoritative gameplay | Surface actions |
| --- | --- | --- | --- |
| `gameplay` | base | allow | pause 与已声明 HUD actions |
| `hud-overlay` | overlay | suppress | inventory/workbench、hotbar allowlist、back |
| `surface` | exclusive | suppress | navigation、activate、back |
| `binding-capture` | exclusive | suppress | confirm/cancel/clear；被捕获输入不继续传播 |

因此 inventory 打开时 movement/break/place 不产生 live frame，但 1–9 仍可选择 hotbar；pause、
settings 与 shell 不透传下层动作。push/pop 原子更新 gameplay suppression、cursor、focus owner 和
pressed-state generation。pop 后必须清除被 surface 消费的 held/just-pressed state，不能把关闭键泄漏
到下一 fixed tick。

`ActionFrameInbox::suppress_live_gameplay()` 仍是 authoritative boundary 的执行机制，但只能由
context stack adapter 调用；单个 HUD/pause system 不得私自维护 suppression latch。Esc native
fallback 永久保留，只能产生安全的 Back/Pause/Cancel，不得直接执行 world mutation。

### 5. Settings 与 UI 集成

每个 rebindable action 机械产生稳定 Controls setting rows；key capture 只接收一个
`InputBindingV1` candidate，交由 input registry 做 normalization/conflict validation，再通过统一
settings transaction apply。成功的 `BindingProfileV1` 使用 user scope canonical file、flush 与
atomic replace；失败或 cancel 必须 rollback preview。重启后从同一 effective-settings snapshot
重建 Leafwing map 与 client surface map。

UI 不读取 `ButtonInput<KeyCode>` 来实现业务动作；只有 binding-capture adapter 和永久 Esc safety
fallback 可以读取原始物理事件。text field/IME 事件不进入 action rebinding，除非 key-capture 明确
处于 active 状态。

### 6. 迁移与禁止事项

- gameplay 默认 Leafwing map 改为 action catalog + effective bindings 的编译结果；
- HUD inventory/workbench/hotbar 与 shell navigation 改为 `ClientSurfaceActionV1`；
- pause/inventory/workbench/settings 的 suppression、cursor 与 focus 改由 context stack 拥有；
- playable fixture 可以保存冻结行为测试，但不得新增生产功能或成为 binding source；
- 不建立自有 physical input backend，不改变 Bevy/Leafwing 的事件来源；
- 不把 client-only UI/HUD 动作塞进 `PlayerActionV1`；
- 不允许 package 注入任意 Bevy systems、callback 或 raw widget/input types。

## 兼容性与版本

改变现有 action stable ID 的含义、`InputBindingV1` canonical meaning、context propagation、冲突
规则或 capability batch compatibility 必须提升对应 major 并提供 migration。增加 optional action、
binding vocabulary minor 或诊断字段可以向后兼容，但 unknown data 必须保留或 fail closed，不能静默
重解释。

## 自动验收

1. binding profile canonical round-trip、旧 minor migration、unknown action preservation 与显式
   unbind 通过；发现/注册顺序不改变 effective catalog。
2. 同 context 冲突正负例与跨 context 复用产生稳定诊断；key capture cancel/clear/apply 无半应用。
3. 改键立即生效，client process 重启后仍生效；键鼠与固定 gamepad 映射产生同一逻辑动作。
4. shell、HUD 与 gameplay production path 不再拥有平行 hard-coded mappings，product lock 中恰有一个
   input-actions provider，catalog 与 client enum 一致。
5. inventory/workbench/settings/pause 打开时没有 movement、break 或 place leakage；hotbar allowlist
   行为符合表格；关闭后没有 stuck input。
6. 同一逻辑输入通过 client physical adapter 与 headless command injection 得到相同 authoritative
   action receipts 与 state hash。
7. Esc safety fallback 在 missing/corrupt binding profile 下仍能暂停、返回或取消，且不能绕过
   settings/world authority。

## 后果

- 新增一个 foundation logical package 和 headless-first crate，但删除三套业务快捷键真相。
- context policy 比简单 exclusive boolean 更明确，避免“overlay 非独占却仍需抑制 gameplay”的歧义。
- 首版 dynamic package action 与 gamepad rebinding 延后；未来扩展不会改变 authoritative action DTO。
- 所有实现状态仍由 requirement evidence 计算；本 ADR accepted 不表示 package、crate 或迁移已完成。

## 相关文件

- [决策 0025：Client shell、设置、可观察性与玩家契约](0025-freeze-client-shell-settings-observability-and-player-contracts.md)
- [Input binding 与 context stack](../platform/input/input-binding-and-contexts.md)
- [`@latticeaxiom/input`](../packages/latticeaxiom/input/README.md)
- [Typed settings surface](../packages/latticeaxiom/settings-ui/settings-surface.md)
- [Shared client UI system](../platform/client-ui/ui-system.md)
- [v1 playable delivery plan](../delivery/plans/v1-playable.md)
