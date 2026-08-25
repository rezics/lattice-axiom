---
title: Package settings registry 与 transaction
document_id: package.latticeaxiom.settings.settings-and-configuration
document_status: accepted
document_type: package-spec
owners:
  - "@latticeaxiom/settings"
tracks_implementation: true
requirements:
  - SETTINGS-REGISTRY-001
  - SETTINGS-TRANSACTION-001
  - SETTINGS-PERSISTENCE-001
updated: 2026-08-24
decision:
  - ../../../decisions/0010-nickel-driven-package-system.md
  - ../../../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
  - ../../../decisions/0033-freeze-input-actions-bindings-and-contexts.md
---

# Package settings registry 与 transaction

## 结论

所有 package 通过 `RegistrationManifest.settings` 贡献 typed `SettingSpec`；locked graph 必须
选择恰好一个 `@latticeaxiom/settings` provider，编译 deterministic catalog，并在任何 client、
headless 或 tool surface 之前解析 effective values。

本 package 拥有 registry、scope、authority、validation、transaction、event 与 persistence
contract。UI 由 [`@latticeaxiom/settings-ui`](../settings-ui/README.md) 拥有。
首个完整产品必须注册的 baseline、feature-conditional rows 与 package ownership 由
[shipped settings catalog](shipped-settings-catalog.md) 冻结；只有 registry 机制而没有目录，
不构成完整 settings 产品。

## Composition parameter 与 runtime setting

二者不得混用：

- composition parameter 影响 package resolution、features、provider、realization、schema 或
  lock fingerprint，只能通过 profile draft → resolve → candidate lock 改变；
- runtime setting 在已锁定 graph 内改变获授权行为，通过 settings transaction 生效。

任何会改变 authoritative package closure 的值都不是“实时设置”。UI 可以在同一 Settings
信息架构中链接 package/profile editor，但必须展示 lock diff 与 world compatibility impact。

## `SettingSpec`

每项至少包含稳定 setting ID、owner package、versioned value type、default、constraints、scope、
authority、category/order、apply impact、visibility、sensitivity 和 optional fallback。稳定 DTO 不
直接序列化 Bevy/Leafwing/widget 类型。

首版持久化 value vocabulary 包含 boolean、bounded integer/float、enum、string、path、color 与
`InputBindingV1`。read-only value、command、open subpage、import/export 与 reset 是 surface
operation，不进入 persisted `SettingValue`。unknown required type major 在 activation 前失败；
optional unsupported row 产生明确诊断。

registry 对整个 closure 验证 ID/owner、default、constraints、scope/authority 与 category，按稳定
规则生成密集 runtime index；热路径不查找字符串。

category、section、subpage、preset 与 dependency 等 layout metadata 进入独立、host 可验证的
`SettingsLayoutSpec`，不改变 setting value identity。普通 package 推荐只声明 `SettingSpec` 或
Tier 1 declarative layout；specialized editor 与 alternative surface 的 gate 由
[settings surface](../settings-ui/settings-surface.md) 定义。无论 surface 层级如何，CLI、tool 与
headless 仍可从 catalog 机械列出、验证和修改获授权值。

## Package ownership 与文档责任

每个 setting 只有一个 logical package owner。owner 必须拥有 stable ID、schema version、default、
constraints、migration、localization 与 apply callback；surface provider 只负责呈现，不能因为
画出 row 就取得该 setting 的语义所有权。

每个 shipped 或 proposed package README 必须有 `Settings contribution`：

- 有设置时，列出 stable IDs、category、scope/authority、default/constraint、apply impact 与
  availability，并链接 primary catalog；
- 没有用户可配置 consumer 时，明确写 `No user-configurable settings` 及理由；
- worldgen seed、provider selection、feature flag、realization 与 dependency 等 graph/new-world
  输入必须标为 composition/new-world parameters，不能伪装成 runtime settings；
- 内部调参、性能 hard limit、安全 policy 与作者数据不是因为“可配置”就自动成为玩家设置。

新增、删除或改变 shipped setting 的语义必须同时更新 owning package README、shipped catalog 与
requirement acceptance。目录扫描、host 常量或私有 JSON 不能补出 catalog 中不存在的生产 setting。

## Scope 与 authority

scope 至少区分 device、user、world、player-world 与 session。effective value 以明确 precedence
计算，但 higher-precedence layer 只有在 authority 允许时才可覆盖。

- client-local presentation/input 可由 device/user/session 修改；
- world-authoritative gameplay 值必须进入 world snapshot/hash，并由 writer/server 授权；
- locked composition values 不允许通过 runtime setting 覆盖；
- headless 与 client 对 authoritative effective snapshot 必须一致。

## Apply transaction

draft 先完成 typed validation、authority check 与 cross-setting constraint，再形成 deterministic
change set。V1 runtime impact 固定为 Preview、Immediate、WorldReactivate 或 ProcessRestart。
new-world-only 值属于 new-world form 或 composition/profile draft，不是 RuntimeApplyImpact。

transaction 要么完整成功，要么恢复所有已 preview 的 reversible values；失败返回稳定、可定位
到 setting ID 的诊断。UI、CLI 与 API 消费同一 transaction path，不分别实现 apply 语义。

## Persistence、migration 与 orphan

device/user values 使用版本化 canonical 文件和原子 replace；world/player-world values 进入
authoritative storage transaction。读回时先验证 schema/version，migration 产生 receipt；未知或
暂时缺失 package 的 user values 保留为 orphan，不能静默删除。

user/device file 的 publish protocol 固定为 write sibling temp → flush file → atomic replace → 按平台
能力 flush parent directory；启动只接受 old-complete 或 new-complete。corrupt/newer-required 文件原样
隔离并产生 recovery diagnostic，不能用 defaults 覆写原文件。UI scale、view distance 与
`BindingProfileV1` 是首批必须走该路径的 consumers。

secret/credential setting 不进入普通 export、diagnostic report 或 log。world setting 保存前仍须
通过 sealed writer authority。

## 首个完整 consumers

- `@latticeaxiom/settings-ui` 的 UI scale、accessibility 与分层 surface；
- `@latticeaxiom/client-presentation` 的 display、Video、Audio 与 presentation accessibility；
- `@latticeaxiom/input` 的 mouse/gamepad preferences、Controls rows 与 binding profile；
- `@latticeaxiom/dev-tools` 的 session/user visualizer settings；
- typed `2..=32` view distance、performance preset 与 render-provider advanced settings；
- `@latticeaxiom/world-library` 的 local world-management policy；
- `@terrenia/gameplay` 的明确 feature-conditional world-authoritative rules。

完整 IDs 与可见条件见 [shipped settings catalog](shipped-settings-catalog.md)。目录 accepted 不表示
对应实现已完成；状态仍由 requirements/evidence 计算。

## 验收方向

- 同一 locked registration image 在 client/headless 产生相同 authoritative catalog/fingerprint；
- duplicate provider、invalid default、unknown required type 与 unauthorized override fail closed；
- preview/apply/rollback 在故障注入下无半应用；
- user values 重启保留，world values 只经 writer transaction；
- package 移除/恢复不会丢失 orphan values；
- UI、CLI、headless API 不产生平行设置语义。

## 相关文件

- [Settings UI surface](../settings-ui/settings-surface.md)
- [Shipped settings catalog](shipped-settings-catalog.md)
- [Client presentation settings](../client-presentation/README.md)
- [Input binding contract](../../../platform/input/input-binding-and-contexts.md)
- [World persistence](../../../platform/world-storage/world-persistence.md)
- [Versioning and compatibility](../../../platform/compatibility/versioning-and-compatibility.md)
