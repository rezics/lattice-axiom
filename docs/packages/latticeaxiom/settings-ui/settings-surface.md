---
title: 分层 typed settings surface
document_id: package.latticeaxiom.settings-ui.settings-surface
document_status: accepted
document_type: package-spec
owners:
  - "@latticeaxiom/settings-ui"
tracks_implementation: true
requirements:
  - SETTINGS-SURFACE-001
  - SETTINGS-SURFACE-LAYERING-001
  - SETTINGS-SURFACE-CONTROLS-001
updated: 2026-08-24
decision:
  - ../../../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
  - ../../../decisions/0033-freeze-input-actions-bindings-and-contexts.md
---

# 分层 typed settings surface

## 结论与范围

`@latticeaxiom/settings-ui` 是 Lattice Axiom 官方提供、默认推荐的 settings surface foundation。
它把 compiled settings catalog 投影为完整的搜索、分类、分组／子页、详情、draft、preview、apply、
reset 与 accessibility 体验；它不是只有几行按钮的 demo menu。

产品 profile 可以选择另一个满足 `latticeaxiom:capability/settings-surface@1` 的 exactly-one
provider，但替代 provider 必须消费同一 `@latticeaxiom/settings` registry、通过同一 transaction、
persistence、authority 与 accessibility requirements。替代 UI 不能创造第二套 setting value、
配置文件或 apply 语义。

本 package 只拥有 surface。setting ID、owner、type、scope、authority、validation、effective value
与 transaction 仍由 [`@latticeaxiom/settings`](../settings/README.md) 拥有；首个完整产品目录见
[Shipped settings catalog](../settings/shipped-settings-catalog.md)。

## 页面信息架构

设置页固定使用以下层次，而不是把整个 catalog 平铺成一列：

1. **Settings route**：从 client shell、Pause 或 tool 进入同一逻辑 surface；返回目标由 route
   epoch 决定。
2. **Category**：一级导航固定为 Accessibility、Controls、Audio、Video、Interface、Gameplay、
   World、Packages、Developer。
3. **Section／subpage**：一个 category 可以按真实复杂度拆成稳定分组或子页。Video 至少包含
   General、Quality、Performance、Advanced；Controls 至少包含 Mouse、Keyboard bindings、
   Gamepad 与 package actions。
4. **Setting row**：显示 label、当前 draft value、机械控件、短说明、修改状态与必要的 impact badge。
5. **Detail**：显示完整说明、owner package、default、effective/requested/admitted 值、provenance、
   scope、authority、performance/compatibility impact、visibility dependency 与 apply impact。
6. **Transaction bar**：只要存在 dirty draft，底部持续提供 Apply、Undo/Cancel 与 Reset；离开 route
   前不得静默丢弃或隐式提交。

宽 viewport 使用 category navigation + content + detail 的三层布局。800×600 或 UI scale 2.0 时，
detail 折叠到当前 row 下方，category 变成可聚焦的一级列表／tabs，transaction bar 仍保持可达。
搜索跨 category、section、label、description、owner display name 与 stable setting ID；结果保留其
category／section breadcrumb，不能成为没有上下文的平铺清单。

## 官方基础与扩展层级

推荐路径是尽量使用官方 foundation 的共享 theme、focus、widgets、transaction bar 与
accessibility semantics。复杂设置不是拒绝统一 surface 的理由，而是使用更高的显式层级：

| Tier | 适用内容 | Package 可声明的内容 | 要求 |
| --- | --- | --- | --- |
| 0：mechanical row | boolean、bounded number、enum、text、path、color、binding | `SettingSpec` + localization | 默认路径；自动获得搜索、scope、authority、reset、CLI/tool fallback |
| 1：declarative layout | 大量相关 rows、presets、依赖项、Video/Controls 类复杂页 | category、section、subpage、group、summary、preset、dependency；不得含绝对坐标 | 仍只使用共享 widget vocabulary；必须能退化为稳定 row list |
| 2：specialized editor | binding matrix、颜色渐变、provider graph 等机械 row 明显不足的真实 consumer | versioned `settings-editor@1` capability 与 typed command/event contract | 必须有 generic fallback、完整键鼠/手柄/AccessKit 语义、独立预算与故障隔离 |
| 3：alternative surface | 产品需要完全不同的设置体验 | exactly-one `settings-surface@1` provider | 必须覆盖完整 catalog、transaction、recovery 与 accessibility acceptance，不能只覆盖自己的 package |

Tier 2 不是任意 Bevy widget/system 注入。它必须满足 ADR 0025 的 gate：至少一个 Tier 0/1 无法表达的
真实 consumer、generic fallback 和新的 versioned capability。manifest 只保存 host 可验证的 layout
schema、editor capability 与 callback key，不保存任意 widget tree、绝对屏幕坐标或 root overlay。

package 可以在设置页内提供 dedicated subpage，但不能劫持 Back、Apply、Quit、binding-capture、
authority prompt 或其他 package 的区域。unsupported optional editor 回退到 generic rows并显示诊断；
unsupported required editor major 在进入 surface 前 fail closed。

## 分类与稳定排序

一级分类顺序固定为 Accessibility、Controls、Audio、Video、Interface、Gameplay、World、Packages、
Developer。package 默认只能选择已有 category；新增一级 category 需要 settings-surface capability
minor、localization、800×600 navigation evidence 与至少一个 shipped consumer。

category 内排序依次为：foundation section order、package-declared section order、setting order、stable
setting ID。discover/load order 不参与排序。package display name 只用于归属和筛选，不把同一玩家任务
拆成多个模组菜单。

Video 的 Advanced 只放 compatibility、provider 或专家设置，默认折叠；普通质量与性能选项不能因为
来自 render package 就全部塞进 Advanced。每个性能相关 row 应声明 Low／Medium／High／Extreme／
Varies 一类稳定 impact hint，并在 detail 说明影响 CPU、GPU、VRAM、内存、延迟或视觉正确性的哪一项。

## 控件词汇与 surface operation

持久化 `SettingValue` 的 V1 机械控件为 toggle、bounded integer/float slider、enum cycle/dropdown、
text、path、color 与 key binding。read-only value、open subpage、import/export、reset package、open
profile draft 与 diagnostic action 是 surface operations，不伪装成可持久化 setting value。

未知 required control/layout/editor major 必须在 surface 建立前失败；optional row 或 editor 可以使用
声明的 generic fallback。控件必须显示 formatted value，slider 不能只有加／减按钮；requested、
admitted、effective 与 clamp reason 不相同时必须分别展示。

## Draft、preview 与 apply

UI 维护 typed draft，不在编辑每一行时直接修改 authoritative state：

1. registry 产生 current/effective snapshot；
2. surface 在 scope/authority 允许时修改 draft并持续显示完整 diff；
3. reversible presentation 项可以 preview，并保存 rollback value和超时；
4. Apply 发送一个 durability-domain transaction；跨 domain draft 必须明确拆分并说明顺序；
5. registry 返回 applied、rejected、world-reactivate 或 process-restart impact；
6. 成功后由 owning scope persistence 保存，surface 不自行写 package 配置文件。

离开页面、transaction 失败或 preview 超时必须 rollback。world/server-authoritative 项在没有 writer/
admin authority 时只读显示并说明控制方。ProcessRestart 或 WorldReactivate 在提交前显示影响；需要
显示模式确认的 preview 必须提供倒计时和永久安全回退。

## Controls 与 binding capture

Controls 消费 input package 的 action catalog 和 rebindable policy，不维护手写快捷键清单。所有标记为
rebindable 的 keyboard/mouse actions 必须生成 row；standard gamepad 首版固定映射也必须可查看，
开放 rebind 后使用同一路径。

key-capture 是 Settings 的 child route，进入独占 `binding-capture` context，只保留 confirm、cancel、
clear 与 Esc safety fallback。candidate 先经 `InputBindingV1` normalization 和同 context 冲突检查，
再进入 settings transaction。冲突必须列出所有占用 action，并允许 Replace、Cancel 或 Clear；不能
静默覆盖。按 action、context、device 与 owning package 可搜索／筛选，支持单 row、section、device
profile 与完整 binding profile reset。

## Packages 与 composition

Packages category 不是另一套 package manager。它提供：

- 按 owning package 筛选当前 catalog；
- 查看 package setting count、scope、authority、provenance 与 unsupported diagnostics；
- reset 单一 package、导入／导出非敏感 user/device values、查看并保留 orphan values；
- 链接 package/profile editor处理 graph-affecting composition parameters。

composition parameter 不能伪装成 runtime toggle。进入 profile editor 必须展示 candidate graph diff、
world compatibility 与 re-lock/restart impact，再由 resolver 产生新 lock。

## 可访问性与输入

官方 foundation 和替代 provider 均必须使用 Bevy UI、官方 focus/editable text 与 AccessKit 语义；
鼠标、纯键盘和 standard gamepad 均可完成 category navigation、搜索、编辑、详情、apply、cancel、
reset 与 binding capture。800×600、UI scale 1.0/2.0、IME、CJK fallback、stable focus restoration 和
无 gameplay input leakage 是首版 gate。Esc safety fallback 永久保留。

每个 control 产生 role、name、value、description、state 与 action。颜色、impact 与 validation 不得
只靠颜色表达；disabled/hidden row 必须能解释 authority、dependency 或 capability 原因。

## 失败与恢复

- catalog、layout 或 editor capability 验证失败时，不执行 package UI callback；保留 Back/Quit 安全路径；
- corrupt user setting 保留原文件、加载可说明来源的 fallback，不用 defaults 覆写坏文件；
- package 暂时缺失时保留 orphan values，恢复 package 后再验证和迁移；
- specialized editor 失败时回到 generic rows；generic fallback 也不可用时仅隔离该 package section；
- alternative surface 不满足完整 capability major 时，profile resolve 失败，不能在运行期偷偷换回另一 UI。

## 非目标

- 不拥有 setting persistence、migration、world writer 或 package resolution。
- 不允许普通 `SettingSpec` 携带任意 widget tree、Bevy system、absolute coordinates 或第二个 root。
- 不把 Feathers、developer-only widgets 或 package 私有配置文件作为玩家设置兼容性契约。
- 不把“有 Settings 按钮”或单一 view-distance control 当成完整 surface evidence。

## 相关文件

- [Shipped settings catalog](../settings/shipped-settings-catalog.md)
- [Settings registry 与 transaction](../settings/settings-and-configuration.md)
- [Input binding 与 context](../../../platform/input/input-binding-and-contexts.md)
- [Shared client UI system](../../../platform/client-ui/ui-system.md)
- [Client surface routing](../../../platform/client-ui/game-surface-state.md)
