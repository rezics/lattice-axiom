---
title: Package 可注入的設定與配置架構
status: proposed
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0010-nickel-driven-package-system.md
  - ../decisions/0018-package-kernel-from-first-vertical-slice.md
  - ../decisions/0019-separate-package-and-registration-identities.md
---

# Package 可注入的設定與配置架構

## 結論

所有 package 都可以通过 `RegistrationManifest.settings` 注册结构化 `SettingSpec`，由
`@latticeaxiom/settings` 提供 exactly-one settings registry／transaction capability。
client 使用 `@latticeaxiom/settings-ui` 渲染同一 registry，headless／tool 使用 CLI 或 API；
package 不需要、也不能各自注入任意 widget tree 才能被配置。

Bevy 0.19 app settings负责 machine-local持久化与 ECS resource integration；Lattice Axiom
只增加产品必需的 stable ID、package ownership、scope、authority、apply impact、migration、
world transaction 与跨 realization contract。它不是第二套通用配置语言。

## 為何必須是基礎 Package

如果设置页只是一个可选生态模组，会形成三种失败：作者退回手写 JSON／TOML、不同 loader／
profile入口不一致、headless无法验证同一配置。首阶段明确提供：

| Logical package | 责任 | Profile |
| --- | --- | --- |
| `@latticeaxiom/settings` | registry、schema validation、value resolution、transaction、events | client／headless／test |
| `@latticeaxiom/settings-ui` | 搜索、分类、preview、draft／apply／reset 与无障碍 UI | client／tool |

`@latticeaxiom/settings` 提供
`latticeaxiom:capability/settings-registry@1 exactly-one`。默认 profile template必须显式请求它，
不能由 host hidden plugin list偷偷安装；需要设置的 package依赖 capability range，而不是依赖
某个 UI realization。若 provider缺失，closure在code activation前失败。

## 兩種不可混合的輸入

### Composition parameter

会改变 dependency、feature、realization、provider、registration或schema的值属于
`CompositionSpec`／profile parameter。它在 resolve／lock 前生效；UI只能编辑一个 draft
profile，然后明确执行 recompose、显示graph diff并生成新 lock。

### Runtime setting

只改变已锁定closure内行为或表现的值使用 `SettingSpec`。runtime setting不能新增package、
替换provider或改变registration image；若 package把graph选择伪装成runtime bool，manifest
validation必须拒绝。

这条边界避免「世界用什么package」同时存在于lock与一个可变配置文件中。

## `SettingSpec`

首版字段：

| 字段 | 语义 |
| --- | --- |
| `id` | 完整stable ID，例如`terrenia:setting/gameplay/instant-break` |
| `declared_by` | owner `PackageName`，由kernel写入provenance |
| `schema_version` | value schema／migration owner版本，不等同package SemVer |
| `value_type` | bool、integer、number、string、enum、color、key binding等有限类型 |
| `default`／`constraints` | typed默认值、min／max／step／allowed values／长度 |
| `allowed_scopes`／`default_scope` | 哪些store可保存该值 |
| `authority` | local user、world owner／server、admin-only或fixed-by-profile |
| `apply_impact` | preview、immediate、world-reactivate、graph-recompose、process-restart |
| `category`／`order` | stable语义分类与owner内顺序；不使用load order |
| `label_key`／`description_key` | localization key；缺失时回退ID和诊断 |
| `visibility`／`enabled_when` | 只允许已验证声明式predicate；隐藏不等于无权限 |
| `sensitivity` | ordinary／private-path；secret不进入普通setting store |
| `deprecated`／`replacement` | 可诊断迁移，不把旧键悄悄复用 |

概念示例：

```text
id             = latticeaxiom:setting/debug/chunk-visualizer-radius
value_type     = integer(min=1, max=32, step=1)
allowed_scopes = [user, session]
authority      = local-user
apply_impact   = immediate-presentation
default        = 8
```

这是typed manifest schema示例，不冻结最终Nickel／Rust语法。

## Scope 與 Authority

scope回答「值保存在哪里」，authority回答「谁能改变它」，两者不得合并成`client/server`字串：

| Scope | 保存位置 | 适合 | 不允许 |
| --- | --- | --- | --- |
| `device` | Bevy app settings／平台资料目录 | GPU、window、audio device | 影响权威world |
| `user` | 用户设置store | accessibility、HUD、input、debug layout | server gameplay rule |
| `profile` | Nickel profile／lock intent | graph／provider／startup参数 | 作为runtime `SettingSpec`层、gameplay中暗改 |
| `world` | world metadata／RocksDB transaction | seed后规则、难度、authoritative开关 | client单方面override |
| `player-world` | world或server的per-player schema | 个人但跟world相关的规则 | 泄漏给其他world |
| `session` | memory only | 临时diagnostic／preview | 被UI误称已保存 |

`profile`列在这里是因为设置页可以同时编辑composition draft；它不属于runtime
`SettingSpec.allowed_scopes`。提交profile change必须重新resolve、显示graph diff并生成新lock。

同一个runtime spec可以允许多个scope，但每个effective value必须显示provenance。值优先级不是任意
last-writer-wins；registry按spec定义的允许链解析，例如`default → user → session`。world或
server-authoritative值不会被user层盖掉。

## 註冊與啟動

1. package kernel在不执行module code时收集全部`SettingSpec`。
2. 验证namespace grant、owner、ID唯一性、type、scope／authority与condition dependency。
3. 编译`SettingsCatalog`进`RegistrationImage`，产生canonical fingerprint。
4. settings provider载入device／user store；world scope只读取header作preflight，不写world。
5. module activation后把typed effective values映射为static Bevy resource或dynamic ABI view。
6. world通过兼容预检后，world／player-world值才随world activation transaction载入。

unknown setting不会在runtime以字符串随意查询。static path使用generated typed handle；dynamic
path使用numeric `SettingKey`与ABI-POD value，numeric mapping只在当前registration image有效。

## Draft、Preview 與 Apply Transaction

设置UI不直接边输入边覆盖权威状态：

```text
effective values
      ↓ edit
typed draft ── validate all dependencies ── impact summary
      ├─ reversible presentation preview ── confirm / timed rollback
      └─ apply transaction ── persist ── SettingChanged batch
```

- video mode、UI scale等可能让画面不可用的设置必须有倒计时rollback；
- slider可实时preview，但写盘使用debounce；退出时再做sync-if-changed；
- world-authoritative设置在fixed-tick barrier以一个batch验证／提交／revision；
- `world-reactivate`、`graph-recompose`、`process-restart`必须在Apply前列出影响；
- package callback只能在transaction提交后收到typed batch，不能在validation中产生副作用；
- 多个字段的cross-setting constraint失败时，整个draft不部分写入。

## UI 組合

`@latticeaxiom/settings-ui`从catalog机械生成可用界面：

- 顶层按玩家任务分类：Accessibility、Controls、Audio、Video、Interface、Gameplay、World、
  Packages、Developer；package只是secondary filter；
- 搜索匹配label、description、package与stable ID；
- 每项显示当前值、default、来源scope、authority、apply impact与validation error；
- 支持`Reset item`、`Reset category`、`Reset package`与`Reset all in scope`，并先显示diff；
- world／server值在无权限时保持可见但read-only，并解释由谁控制；
- advanced／technical项默认折叠，不用“隐藏”掩盖重要风险；
- mouse、keyboard、controller navigation与screen narration使用相同语义树。

首阶段不允许custom arbitrary editor。有限value type无法表达真实设置时，先扩充versioned
`SettingSpec` widget vocabulary；只有出现不可机械表达的真实consumer，才设计受限
`settings-editor` capability，且必须保留generic fallback。

## 保存、迁移與 Orphan

- device／user store采用Bevy app settings的crash-resistant write作为上游基础；
- world setting与其他world metadata使用RocksDB atomic batch和checkpoint策略；
- store保存stable setting ID、schema version、typed value、scope与last-writer provenance；
- owner package负责同一ID的schema migration；更换ID需显式replacement mapping；
- package缺失时值成为orphan，默认保留并在管理页显示，不激活、不丢弃；
- re-install compatible owner后才可恢复；purge orphan是独立可审查动作；
- secret／credential／token不进入普通settings或world backup，未来使用平台secret store contract。

## Multiplayer 與安全

- join／load preflight比较server／world authoritative setting fingerprint与本地closure；
- client UI可以显示server值，但只有授权command能提交修改；
- server发送spec允许公开的label／value，不发送private path或secret；
- presentation-only user设置无需进入authoritative state hash；
- authoritative world setting必须进入replay／save fixture与state hash；
- trusted native package仍可绕过进程边界，settings validation不是sandbox。

## 首個 Demo Consumer

至少同时实现：

1. `@latticeaxiom/settings-ui`自己的user-scope HUD scale；
2. `@latticeaxiom/dev-tools`的session／user-scope chunk visualizer radius；
3. `@terrenia/gameplay`的world-authoritative break cooldown；
4. 一个profile parameter修改realization preference，并明确走recompose而非runtime setting。

这四项覆盖不同owner、scope、authority与apply impact，能证明「所有package可注入」不是只有
core settings的硬编码表。

## 驗收

- 随机化package discovery／manifest顺序不改变catalog、UI order或effective values。
- 两个package注册同一SettingId在code load前失败并列出namespace／owner。
- client／headless验证相同authoritative specs；headless可经CLI读取／修改授权值。
- static／portable dynamic读取相同typed value并产生相同authoritative结果。
- graph-affecting值只能生成draft profile + lock diff，不能热改registration。
- invalid cross-setting draft不部分apply；crash during user／world save恢复旧或完整新batch。
- missing package留下可见orphan；reinstall compatible owner可恢复；purge需明确动作。
- world／server setting没有权限时UI可解释，不以disabled-unlabeled control表示。
- 关键设置可由keyboard／controller完整操作，screen narration读出label、value、scope与impact。

## 相關文件

- [套件內核](package-management.md)
- [模組與註冊組合](module-composition.md)
- [套件驅動的 Bevy runtime](game-engine-runtime.md)
- [World 目錄、開始頁與安全生命週期](world-lifecycle-and-start-ui.md)
- [外部調查](../research/debug-settings-and-world-ux-lessons.md)
