---
title: 體素遊戲的資訊、設定與存檔 UX 教訓
status: exploration
type: research
updated: 2026-08-19
---

# 體素遊戲的資訊、設定與存檔 UX 教訓

## 結論

调试信息、准星检查、package 设置与 world 管理不应各自长成互相覆盖的模组 UI。
它们需要三个共享而彼此分离的产品面：

1. **玩家信息面**：只回答当前目标是什么、能否交互、关键状态是什么；
2. **诊断工作台**：按需订阅指标与 world-space visualizer，服务性能、区块、物理与
   package 故障排查；
3. **设置与 world 控制面**：以明确 scope／authority／apply impact 管理配置，并在
   world 写入前完成兼容预检、checkpoint 与恢复选择。

Minecraft、Jade／WTHIT、Factorio 与其他引擎已经证明这些功能有真实需求；它们的长期
抱怨也说明，单纯加入更多开关、快捷键或任意 GUI hook 并不能形成可维护设计。

## 證據使用方式

- 官方 release note、API／engine 文档用于确认实际机制；
- 模组仓库／文档用于确认生态已经反复实现的扩充点；
- forum、feedback 与 Reddit 只作为定性痛点样本，不作为用户比例或普遍性统计；
- 下面的「对 Lattice Axiom 的含义」是从资料推导的设计结论，不代表来源本身赞同该方案。

调查截止日期为 2026-08-19。

## F3 類資訊顯示

Minecraft Java 1.21.9 把 debug overlay 改为默认更少信息、可逐项配置，并把 chunk border、
entity hitbox 与 octree visualization 放进同一个 Debug Options screen；它还明确说明旧实现
会在打开 F3 时每帧收集资料，导致查看 FPS 本身降低 FPS。这个版本加入 always-visible 项
与 Performance preset，证明「按需采集」「常驻少量指标」「成组 preset」都是基础需求。
[官方 1.21.9 release note](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-9)

这次改进并没有消除全部问题：

- 玩家继续要求把 targeted block 坐标与完整 block state 拆开，因为一个粗粒度开关仍可让
  面板占掉半个屏幕；
  [Minecraft Feedback：targeted block 应拆项](https://feedback.minecraft.net/hc/en-us/community/posts/38601482908301-Make-targeted-block-coordinates-a-separate-option-from-targeted-block-info)
- 新界面仍被抱怨把多个值绑成一个选项、帮助文字造成额外 clutter；
  [Minecraft Feedback：1.21.9 F3 建议](https://feedback.minecraft.net/hc/en-us/community/posts/39993473664525-1-21-9-New-F3-Menu-Suggestions)
- debug 字体跟随全局 GUI scale，使玩家必须在正常 UI 尺寸与可用 debug 尺寸之间二选一；
  [Minecraft Feedback：独立 debug 字体缩放](https://feedback.minecraft.net/hc/en-us/community/posts/360061243051-Adjustable-F3-debug-menu-font-size)
- 更早的官方 issue 已记录常见方块的 tags 可把 targeted-fluid 信息推出屏幕；这说明固定
  文本堆叠没有可靠 overflow policy。
  [MC-248192](https://bugs-legacy.mojang.com/browse/MC-248192)

BetterF3 用 module、逐行开关、左右位置、颜色、间距、背景与阴影回应这些缺口；但它在不同
loader 上还建议安装 Mod Menu 或 Cloth Config，或要求玩家寻找独立快捷键／编辑 TOML。
[BetterF3 repository](https://github.com/TreyRuffy/BetterF3)

### 對 Lattice Axiom 的含義

- 指标必须是有 stable ID 的细粒度 item，不以一整段 package-owned文字注册；
- 数据采集由可见 item 的 subscription 驱动，隐藏面板不得继续执行昂贵 query；
- `Pinned HUD`、`Compact` 与完整 workbench 是不同密度，不靠一个 F3 toggle循环；
- layout 由使用者明确选择 anchor／column／order，不能因为内容长度自动跳边；
- debug text scale、opacity 与背景独立于 gameplay UI scale；
- overflow 使用折叠、分页／滚动与预算警告，绝不静默把资料画出屏幕。

## 方塊名稱與目標檢查 Overlay

Jade 的定位就是显示准星所指对象；WTHIT 同样显示 block／fluid，并提供 API 让其他模组贡献
资料。[Jade repository](https://github.com/Snownee/Jade)、
[WTHIT project page](https://modrinth.com/mod/wthit)

WTHIT 把 server data 与 client tooltip 分开，并允许 provider 阻止资料下发；这证明容器
内容、NBT 类状态或其他权威资料不能由 client 任意读取。
[WTHIT server-data documentation](https://docs.bai.lol/wthit/plugin/server_data/)

生态痛点样本显示：

- 两个模组提供相似 overlay 时会重复绘制，玩家只能逐个猜测并关闭其中一个；
  [重复 overlay 排查样本](https://www.reddit.com/r/feedthebeast/comments/1v6noxh/what_could_be_causing_this_overlap_in_overlays/)
- 信息可能技术上可配置，却没有玩家能发现的表达方式，甚至需要手改 JSON 才知道为何不显示；
  [隐藏 scoping 规则样本](https://www.reddit.com/r/feedthebeast/comments/1qj6h8d/jade_not_working_raspberry_flavoured/)
- 有玩家不希望系统无条件剧透容器／配方／状态，期望信息受工具、进度或 server policy 限制；
  [“不想一直被告知一切”样本](https://www.reddit.com/r/feedthebeast/comments/1t4b736/is_there_any_way_to_restrict_jei_and_jade_to_only/)
- 容器逐 stack 展开会让 overlay失去摘要价值，且 client／server安装状态会造成行为差异；
  [容器信息密度样本](https://www.reddit.com/r/feedthebeast/comments/1g1xeax/compact_inventory_contents_in_jade_overlay/)

### 對 Lattice Axiom 的含義

- host 只提供一个 inspect surface，package 贡献结构化 fragment，不各画一张 panel；
- `name`、`summary`、`detail`、`technical` 分层，默认只显示最小玩家信息；
- fragment 声明 cost／refresh／authority／visibility，昂贵资料在 target 变化或限频时取得；
- server 决定可见的权威资料；client-only package 不得从 presentation cache 推断隐藏状态；
- UI 明示「因权限／进度不可用」，不能让“无资料”看起来像模组坏掉；
- package 来源与 stable ID 放在可展开 technical层，不占用默认名称行。

## Package 設定

Bevy 0.19 已加入 app settings，可把 Rust `SettingsGroup` 载入为 ECS resource，并提供
debounced asynchronous save、退出时同步保存及 temporary-file + atomic replace 的
crash-resistant写入。它适合做 Lattice Axiom device／user setting store 的上游基础，但它
不定义 package setting ID、world authority、server sync、兼容 migration 或通用设置 UI。
[Bevy 0.19 App Settings](https://bevy.org/news/bevy-0-19/#app-settings)、
[SettingsPlugin 文档](https://docs.rs/bevy/latest/bevy/settings/struct.SettingsPlugin.html)

Factorio 的 mod settings 把 `startup`、`runtime-global`、`runtime-per-user` 分开；startup
需要所有多人客户端一致，runtime-global 由管理员控制，per-user则保留个人值。这个模型
证明 scope 与 authority 必须是一等资料，而不是从文件名或 UI 所在页面猜测。
[Factorio mod settings](https://stable.wiki.factorio.com/Tutorial%3AMod_settings)、
[runtime prototype](https://lua-api.factorio.com/latest/classes/LuaModSettingPrototype.html)

NeoForge 也区分 client／common／server config，并把部分 server config 放进 world目录、同步
到 client；这进一步证明「同一键在哪一侧生效」需要明确 contract。
[NeoForge configuration](https://docs.neoforged.net/docs/1.21.4/misc/config/)

Stardew Valley 的 Generic Mod Config Menu 与 Minecraft 的 Cloth Config／Mod Menu 都回应了
「玩家不应手改 JSON／TOML」的需求；但 UI 是额外安装的模组或可选 API 时，作者采用不一致、
入口也会随 loader／平台变化。
[Generic Mod Config Menu](https://www.curseforge.com/stardewvalley/mods/generic-mod-config-menu)、
[Cloth Config FAQ](https://blog.curseforge.com/cloth-config-api-frequently-asked-questions/)

### 對 Lattice Axiom 的含義

- client与headless profile 都要有基础 settings registry package；GUI／CLI只是不同 surface；
- 任一 package 都能声明 `SettingSpec`，但不得注入任意 widget tree 来劫持设置页；
- resolution／realization 选择仍属于 pre-lock profile parameter；runtime setting 不得绕过 lock
  暗改 package graph；
- 每个 setting 显式声明 scope、authority、apply impact、default、constraint、migration 与
  localization；
- UI 必须能显示来源、当前值来源、修改后影响、是否需重新进 world／recompose／restart；
- 未安装 package 的值保留为可清理 orphan，不把未知键静默套给另一个 owner。

## 區塊、碰撞與渲染可視化

Bevy 0.19 已有 `DiagnosticsOverlayPlugin` presets／custom paths、3D gizmos、AABB gizmo与
dev-only text gizmos；Avian 的 debug plugin进一步区分 collider、AABB、contacts、joints、
ray／shape casts、simulation islands 与 collider tree nodes。首个原型应采用这些 building
blocks，而不是建立另一套 debug renderer。
[Bevy 0.19 Diagnostics Overlay／Text Gizmos](https://bevy.org/news/bevy-0-19/#diagnostics-overlay)、
[Bevy 3D gizmos](https://bevy.org/examples/gizmos/3d-gizmos/)、
[Avian PhysicsDebugPlugin](https://docs.rs/avian3d/latest/avian3d/debug_render/struct.PhysicsDebugPlugin.html)

Unity Physics Debugger 的成熟分类值得借鉴：它把真实 collider、broadphase AABB、contact、
trigger、joint、query shape分开，并提供 layer／type filter、overdraw、view distance、颜色、
透明度与最大 query数，而不是把所有线框称为“碰撞箱”。
[Unity Physics Debug window](https://docs.unity.cn/Manual/PhysicsDebugVisualization.html)

### 對 Lattice Axiom 的含義

- voxel selection、authoritative solid occupancy、generated collider、physics broadphase AABB、
  render bounds 与 ray／shape query 必须使用不同 ID、legend与默认颜色；
- chunk visualizer分别表达 materialized／resident／visible／meshing／collider-ready／dirty／
  durable状态，以及 LOD／occlusion／provider，不只画一圈固定网格；
- 每个 visualizer 有 radius、entity／primitive、CPU、upload预算和深度模式；超过预算时采样并
  显示 truncation，不允许 debug功能拖垮正常帧率；
- visualizer使用同一个选择／inspect surface，点击线框能看到 revision、owner、job age与
  rejection reason；
- 这些全是 presentation；启停不得改变权威 world hash。

## World 管理與開始頁

Factorio 把最新存档名称直接放进 `Continue`，因为继续上次游戏是最高频动作；同时把 new／
load、multiplayer、editor、settings 与 mods分组。
[Factorio main menu 设计说明](https://www.factorio.com/blog/post/fff-330)

它还能从 save 同步模组与 startup settings；精确版本同步却需要 `Ctrl + click`，社区讨论
反复指出这个入口容易错过。Lattice Axiom 应提供显式的 `Use frozen lock`／`Resolve compatible`
选择与完整 diff，不能把重要相容行为藏在修饰键中。
[Factorio 自动同步说明](https://wiki.factorio.com/index.php?title=Installing_Mods)、
[入口难发现样本](https://www.reddit.com/r/factorio/comments/l09qrv/ability_to_pin_mods_to_save_files/)

Minecraft 26.1 在高风险world格式升级时改成 `Upgrade and Play`、禁用其他编辑动作、显示升级
进度并强制备份。这是正确的风险承认，但备份不应只在官方预知一次大迁移时出现；玩家仍在
要求 world selection直接提供按需 backup。
[Minecraft 26.1 Snapshot 6](https://feedback.minecraft.net/hc/en-us/articles/43431470235021-Minecraft-Java-Edition-26-1-Snapshot-6)、
[world list备份建议](https://feedback.minecraft.net/hc/en-us/community/posts/26602147746317-Provide-a-world-backup-button-on-world-selection-screen)

社区支持帖持续出现低磁盘后存档无法载入、移除 worldgen／content mod 后局部重生或无法
deserialize，以及更新前是否备份只能靠玩家自行判断的案例。这些是定性样本，但与本项目已
采用的 exact lock、required-content closure、revision与checkpoint方向一致。
[低磁盘／invalid player data样本](https://www.reddit.com/r/MinecraftHelp/comments/1vmvjzy/please_help_my_modded_minecraft_world_is_bugged/)、
[移除模组后 world无法载入样本](https://github.com/neoforged/NeoForge/discussions/2986)

### 對 Lattice Axiom 的含義

- 开始页只对 exact-compatible、健康且 clean／recoverable 的最近 world提供一键 Continue；
- world card 显示 immutable world ID 之外的人类名称、最后游玩、截图、大小、durability、
  lock／package健康与最近checkpoint；目录名不承担身份；
- 所有载入先做不执行module code、不写world的 preflight，并给 package／schema／setting／
  migration diff与可行动选项；
- upgrade／migration默认先做checkpoint或clone，失败不替换原world；
- delete先进入可恢复 trash，purge 才是不可逆动作；
- 损坏或缺metadata的world仍显示在catalog中并标为需要恢复，不能静默消失；
- world list提供 search／sort／filter，扫描与缩略图异步且不阻塞输入。

## 可及性與破壞性操作

Microsoft Xbox Accessibility Guidelines要求菜单可由keyboard与controller digital input完整
导航、focus顺序稳定，且从首次启动即可进入accessibility settings；对设置影响，建议提供
真实语境preview。
[XAG 112：UI navigation](https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/112)、
[XAG 114：UI context](https://learn.microsoft.com/en-us/xbox/accessibility/xbox-accessibility-guidelines/114)

XAG 115要求修改／删除资料前允许review、correct或reverse，并明确提到覆盖save与丢失setting
configuration；它也不建议把长按作为唯一的破坏性确认方式。
[XAG 115：error messages and destructive actions](https://learn.microsoft.com/en-us/xbox/accessibility/xbox-accessibility-guidelines/115)

### 對 Lattice Axiom 的含義

- 首次启动的字体、对比、字幕／旁白与减动效设置必须在 world／package错误之前可达；
- 诊断HUD、inspect overlay和world list都有可朗读label／value／state，不只靠颜色；
- 危险操作说明对象、影响与恢复点；常规delete用trash + undo，永久purge再做明确review；
- loading／migration／checkpoint必须显示阶段、进度与是否仍可安全取消。

## 方案對照

| 已观察缺陷 | Lattice Axiom 设计回应 |
| --- | --- |
| 一个 F3 面板堆所有文字 | item registry + preset + pinned／compact／workbench 三密度 |
| 打开性能面板反而降低性能 | subscription-driven collection + cost／rate budget |
| 多个模组各画 overlay | 单一 inspect／diagnostic surface，package只贡献结构化fragment |
| block坐标与完整状态绑在一起 | name／summary／detail／technical独立item |
| 各模组要求手改配置或装另一个菜单模组 | 基础settings package + declarative `SettingSpec` + GUI／CLI fallback |
| client／server／world setting含义模糊 | scope、authority、apply impact与provenance为schema字段 |
| 改设置暗中改变模组组合 | graph-affecting值只能编辑draft profile并重新resolve／lock |
| “碰撞箱”混用 collider／AABB／query | 类型化visualizer、固定legend、filter与pick inspector |
| 只画chunk边界看不出pipeline状态 | chunk lifecycle／revision／queue／LOD／provider分层 |
| save与当前模组不匹配才在载入后报错 | header-only preflight + frozen lock／compatible diff |
| migration直接改原world | checkpoint／clone + staged migration + rollback |
| 删除或损坏后依赖手动复制目录 | world catalog + trash／restore + health／checkpoint UI |
| 关键动作藏在快捷键或修饰键 | 屏幕内可发现入口；快捷键只是加速路径 |

## 相關文件

- [Package 設定與配置](../architecture/settings-and-configuration.md)
- [診斷、檢查與除錯可視化](../architecture/diagnostics-inspection-and-debug-visualization.md)
- [World 目錄、開始頁與安全生命週期](../architecture/world-lifecycle-and-start-ui.md)
- [第一個可玩 demo 路線圖](../planning/roadmap-first-demo.md)
