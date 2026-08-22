---
title: Package settings registry 与 transaction
document_id: package.latticeaxiom.settings.settings-and-configuration
document_status: proposed
document_type: package-spec
owners:
  - "@latticeaxiom/settings"
tracks_implementation: true
requirements: []
updated: 2026-08-22
decision:
  - ../../../decisions/0010-nickel-driven-package-system.md
  - ../../../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
---

# Package settings registry 与 transaction

## 结论

所有 package 通过 `RegistrationManifest.settings` 贡献 typed `SettingSpec`；locked graph 必须
选择恰好一个 `@latticeaxiom/settings` provider，编译 deterministic catalog，并在任何 client、
headless 或 tool surface 之前解析 effective values。

本 package 拥有 registry、scope、authority、validation、transaction、event 与 persistence
contract。UI 由 [`@latticeaxiom/settings-ui`](../settings-ui/README.md) 拥有。

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

首版 value vocabulary 包含 boolean、bounded integer/float、enum、string、path、color、
`InputBindingV1`、read-only value 与 command。unknown required type major 在 activation 前失败；
optional unsupported row 产生明确诊断。

registry 对整个 closure 验证 ID/owner、default、constraints、scope/authority 与 category，按稳定
规则生成密集 runtime index；热路径不查找字符串。

## Scope 与 authority

scope 至少区分 device、user、world、player-world 与 session。effective value 以明确 precedence
计算，但 higher-precedence layer 只有在 authority 允许时才可覆盖。

- client-local presentation/input 可由 device/user/session 修改；
- world-authoritative gameplay 值必须进入 world snapshot/hash，并由 writer/server 授权；
- locked composition values 不允许通过 runtime setting 覆盖；
- headless 与 client 对 authoritative effective snapshot 必须一致。

## Apply transaction

draft 先完成 typed validation、authority check 与 cross-setting constraint，再形成 deterministic
change set。每项声明 live-reversible、live-irreversible、restart-client、restart-world 或
new-world-only impact。

transaction 要么完整成功，要么恢复所有已 preview 的 reversible values；失败返回稳定、可定位
到 setting ID 的诊断。UI、CLI 与 API 消费同一 transaction path，不分别实现 apply 语义。

## Persistence、migration 与 orphan

device/user values 使用版本化 canonical 文件和原子 replace；world/player-world values 进入
authoritative storage transaction。读回时先验证 schema/version，migration 产生 receipt；未知或
暂时缺失 package 的 user values 保留为 orphan，不能静默删除。

secret/credential setting 不进入普通 export、diagnostic report 或 log。world setting 保存前仍须
通过 sealed writer authority。

## 首个 consumers

- `@latticeaxiom/settings-ui` 的 UI scale 与 Controls rows；
- `@latticeaxiom/dev-tools` 的 session/user visualizer settings；
- view distance 与性能 preset 的 host-clamped setting；
- `@terrenia/gameplay` 的明确 world-authoritative rules。

这些例子不表示对应实现已完成；状态由 requirements/evidence 计算。

## 验收方向

- 同一 locked registration image 在 client/headless 产生相同 authoritative catalog/fingerprint；
- duplicate provider、invalid default、unknown required type 与 unauthorized override fail closed；
- preview/apply/rollback 在故障注入下无半应用；
- user values 重启保留，world values 只经 writer transaction；
- package 移除/恢复不会丢失 orphan values；
- UI、CLI、headless API 不产生平行设置语义。

## 相关文件

- [Settings UI surface](../settings-ui/settings-surface.md)
- [Input binding proposal](../../../platform/input/input-binding-and-contexts.md)
- [World persistence](../../../platform/world-storage/world-persistence.md)
- [Versioning and compatibility](../../../platform/compatibility/versioning-and-compatibility.md)
