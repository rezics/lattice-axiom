---
title: Shipped settings catalog 与 package ownership
document_id: package.latticeaxiom.settings.shipped-catalog
document_status: accepted
document_type: package-spec
owners:
  - "@latticeaxiom/settings"
tracks_implementation: true
requirements:
  - SETTINGS-CATALOG-001
  - SETTINGS-PACKAGE-OWNERSHIP-001
updated: 2026-08-24
decision:
  - ../../../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
  - ../../../decisions/0033-freeze-input-actions-bindings-and-contexts.md
---

# Shipped settings catalog 与 package ownership

## 目的与解释规则

本页冻结完整 settings 产品必须表达的内容、唯一 package owner 与最小 schema。它不是当前 demo
菜单的抄录，也不以实现仓暂时只有几个 controls 为范围上限。compiled registration image 仍是运行时
事实；本页是 package author、surface、CLI/tool、requirements 与 implementation evidence 共同使用的
产品目录。

每项标记一种 availability：

- **baseline**：选择官方 client foundation 的普通 playable profile 必须注册；
- **feature-conditional**：只有 owning gameplay/render/audio/provider capability 存在时注册；没有
  consumer 时不显示灰色假选项；
- **developer**：只有选择 dev-tools profile 且权限允许时注册；
- **operation**：设置页动作或链接，不是 persisted `SettingValue`。

Runtime apply impact 只使用 Preview、Immediate、WorldReactivate、ProcessRestart。new-world-only 和
graph-affecting值进入 New World／profile draft，不作为 runtime setting。所有 default 均先通过
platform/provider constraints；host clamp 后 surface 必须分别显示 requested、admitted、effective 与
clamp reason。

## Accessibility 与 Interface foundation

下列 rows 由 `@latticeaxiom/settings-ui` 拥有，shell 与 client-world 共用：

| Stable setting ID | Type、default 与 constraint | Scope／authority | Impact | Availability |
| --- | --- | --- | --- | --- |
| `latticeaxiom:setting/interface/ui-scale` | enum `auto | 100 | 150 | 200`，default `auto` | device／local-user | Preview | baseline |
| `latticeaxiom:setting/interface/text-scale` | integer 75..200%，step 5，default 100 | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/accessibility/high-contrast` | bool，default false | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/accessibility/reduce-motion` | bool，default false | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/accessibility/notification-duration` | enum `short | normal | long`，default `normal` | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/interface/language` | locale enum from validated asset catalog，default `system`；apply triggers validated asset reload | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/interface/show-advanced-settings` | bool，default false | user／local-user | Immediate | baseline |

language、high contrast、text/UI scale 与 reduce motion 必须在首次启动、world preflight error 与 package
recovery 之前可达。`show-advanced-settings` 只改变 layout visibility，不授权 hidden developer、
admin 或 server rows。

## Client presentation foundation

`@latticeaxiom/client-presentation` 是跨 shell/world 的 client-only owner。headless 可以省略它，且
省略不得改变 authoritative hash。

### Accessibility presentation

| Stable setting ID | Type、default 与 constraint | Scope／authority | Impact | Availability |
| --- | --- | --- | --- | --- |
| `latticeaxiom:setting/accessibility/narrator-mode` | enum `off | system | all`，default `off` | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/accessibility/subtitles` | bool，default false | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/accessibility/subtitle-direction` | bool，default true；visible when subtitles | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/accessibility/subtitle-scale` | integer 75..200%，step 5，default 100 | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/accessibility/subtitle-background-opacity` | integer 0..100%，step 5，default 50 | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/accessibility/camera-shake-scale` | integer 0..100%，step 5，default 100 | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/accessibility/damage-tilt-scale` | integer 0..100%，step 5，default 100 | user／local-user | Immediate | feature-conditional |
| `latticeaxiom:setting/accessibility/distortion-scale` | integer 0..100%，step 5，default 100 | user／local-user | Immediate | feature-conditional |

### Audio

| Stable setting ID | Type、default 与 constraint | Scope／authority | Impact | Availability |
| --- | --- | --- | --- | --- |
| `latticeaxiom:setting/audio/master-volume` | integer 0..100%，default 100 | device／local-user | Preview | baseline |
| `latticeaxiom:setting/audio/music-volume` | integer 0..100%，default 50 | user／local-user | Preview | baseline |
| `latticeaxiom:setting/audio/ambient-volume` | integer 0..100%，default 100 | user／local-user | Preview | baseline |
| `latticeaxiom:setting/audio/weather-volume` | integer 0..100%，default 100 | user／local-user | Preview | feature-conditional |
| `latticeaxiom:setting/audio/blocks-volume` | integer 0..100%，default 100 | user／local-user | Preview | baseline |
| `latticeaxiom:setting/audio/hostile-volume` | integer 0..100%，default 100 | user／local-user | Preview | feature-conditional |
| `latticeaxiom:setting/audio/neutral-volume` | integer 0..100%，default 100 | user／local-user | Preview | feature-conditional |
| `latticeaxiom:setting/audio/players-volume` | integer 0..100%，default 100 | user／local-user | Preview | baseline |
| `latticeaxiom:setting/audio/ui-volume` | integer 0..100%，default 100 | user／local-user | Preview | baseline |
| `latticeaxiom:setting/audio/records-volume` | integer 0..100%，default 100 | user／local-user | Preview | feature-conditional |
| `latticeaxiom:setting/audio/narrator-volume` | integer 0..100%，default 100 | user／local-user | Preview | baseline |
| `latticeaxiom:setting/audio/output-device` | validated device enum，default `system` | device／local-user | ProcessRestart | baseline |
| `latticeaxiom:setting/audio/directional-audio` | enum `auto | off | on`，default `auto` | device／local-user | ProcessRestart | baseline |
| `latticeaxiom:setting/audio/mono-audio` | bool，default false | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/audio/background-volume` | integer 0..100%，default 25 | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/audio/music-frequency` | enum `rare | normal | frequent`，default `normal` | user／local-user | Immediate | feature-conditional |

调节 category volume 时，在没有 active world 的 shell 播放短的可取消 preview；不得依赖进入 world
才能验证声音。output device 消失时保留用户选择和诊断，effective 回退 system device，不覆写保存值。

### Video / General

| Stable setting ID | Type、default 与 constraint | Scope／authority | Impact | Availability |
| --- | --- | --- | --- | --- |
| `latticeaxiom:setting/video/performance-preset` | enum `safe | balanced | quality | custom`，default `balanced` | device／local-user | Preview | baseline |
| `latticeaxiom:setting/video/window-mode` | enum `windowed | borderless | exclusive`，default `windowed`；失败回退 | device／local-user | Preview | baseline |
| `latticeaxiom:setting/video/monitor` | validated monitor enum，default `primary`；失败回退 | device／local-user | Preview | baseline |
| `latticeaxiom:setting/video/resolution` | validated width × height enum，default `current`；失败回退 | device／local-user | Preview | baseline |
| `latticeaxiom:setting/video/refresh-rate` | validated Hz enum，default `current`；失败回退 | device／local-user | Preview | baseline |
| `latticeaxiom:setting/video/vsync` | bool，default true | device／local-user | Preview | baseline |
| `latticeaxiom:setting/video/frame-rate-limit` | enum `30 | 60 | 90 | 120 | 144 | 165 | 240 | unlimited`，default 120 | device／local-user | Immediate | baseline |
| `latticeaxiom:setting/video/background-frame-rate-limit` | integer 5..60，step 5，default 30 | device／local-user | Immediate | baseline |
| `latticeaxiom:setting/video/field-of-view` | integer 50..110 degrees，step 1，default 75 | user／local-user | Preview | baseline |
| `latticeaxiom:setting/video/fov-effects-scale` | integer 0..100%，step 5，default 100 | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/video/brightness` | integer 0..100%，step 5，default 50 | user／local-user | Preview | baseline |
| `latticeaxiom:setting/video/view-distance` | integer 2..32 chunks，step 1，default 8；受 host/performance clamp | user／local-user | Immediate | baseline |

`performance-preset` 是同一 transaction 内对明确 member settings 的 typed preset，不是隐藏 global
quality boolean。用户改动 member 后 effective preset 显示 `custom`；选择 preset 前必须展示 diff。

### Video / Quality

| Stable setting ID | Type、default 与 constraint | Scope／authority | Impact | Availability |
| --- | --- | --- | --- | --- |
| `latticeaxiom:setting/video/graphics-quality` | enum `fast | balanced | fancy`，default `balanced` | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/video/particles` | enum `minimal | decreased | all`，default `all` | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/video/smooth-lighting` | enum `off | low | high`，default `high` | user／local-user | WorldReactivate | baseline |
| `latticeaxiom:setting/video/biome-blend-radius` | integer 0..7 blocks，step 1，default 2 | user／local-user | WorldReactivate | baseline |
| `latticeaxiom:setting/video/entity-distance-scale` | integer 50..200%，step 25，default 100 | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/video/entity-shadows` | bool，default true | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/video/fog-quality` | enum `off | fast | fancy`，default `fancy` | user／local-user，world policy may raise minimum | Immediate | baseline |
| `latticeaxiom:setting/video/weather-quality` | enum `off | fast | fancy`，default `fancy` | user／local-user，world policy may raise minimum | Immediate | feature-conditional |
| `latticeaxiom:setting/video/chunk-fade-duration` | integer 0..2000 ms，step 50，default 500 | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/video/mipmap-levels` | integer 0..4，step 1，default 4；apply triggers asset reload | device／local-user | WorldReactivate | baseline |
| `latticeaxiom:setting/video/anisotropic-filtering` | enum `off | 2x | 4x | 8x | 16x`，default `off`；GPU-clamped | device／local-user | WorldReactivate | baseline |
| `latticeaxiom:setting/video/texture-filtering` | enum `nearest | linear`，default `nearest` | user／local-user | WorldReactivate | baseline |
| `latticeaxiom:setting/video/fluid-quality` | enum `fast | balanced | fancy`，default `balanced` | user／local-user | WorldReactivate | feature-conditional |

### Video / Performance 与 Advanced

| Stable setting ID | Type、default 与 constraint | Scope／authority | Impact | Availability |
| --- | --- | --- | --- | --- |
| `latticeaxiom:setting/video/chunk-update-mode` | enum `immediate | soon | deferred`，default `soon` | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/video/chunk-build-threads` | enum `auto` + bounded hardware count，default `auto` | device／local-user | WorldReactivate | baseline/Advanced |
| `latticeaxiom:setting/video/block-face-culling` | bool，default true | device／local-user | WorldReactivate | baseline/Performance |
| `latticeaxiom:setting/video/fog-occlusion` | bool，default true | device／local-user | Immediate | baseline/Performance |
| `latticeaxiom:setting/video/entity-culling` | bool，default true | device／local-user | Immediate | baseline/Performance |
| `latticeaxiom:setting/video/animate-visible-textures-only` | bool，default true | device／local-user | Immediate | baseline/Performance |
| `latticeaxiom:setting/video/cpu-render-ahead` | integer 1..4 frames，default 2 | device／local-user | ProcessRestart | baseline/Advanced |
| `latticeaxiom:setting/video/render-backend` | validated backend enum，default `auto` | device／local-user | ProcessRestart | feature-conditional/Advanced |
| `latticeaxiom:setting/video/gpu-adapter` | validated adapter enum，default `auto` | device／local-user | ProcessRestart | feature-conditional/Advanced |
| `latticeaxiom:setting/video/validation-mode` | enum `auto | off | on`，default `auto` | device／local-user | ProcessRestart | developer/Advanced |

LOD、upscaler、shader pack、dynamic resolution、specialized visibility、terrain backend 或 alternate
renderer optimization 不预占 foundation IDs。对应 provider 必须以自己的 namespace/owner 注册
feature-conditional rows、performance impact、dependency、fallback 与 restart behavior；普通选项进入
Quality/Performance，只有兼容性和危险调试项进入 Advanced。

## Controls

`@latticeaxiom/input` 拥有 input preferences 与 `BindingProfileV1`。基础 preferences：

| Stable setting ID | Type、default 与 constraint | Scope／authority | Impact | Availability |
| --- | --- | --- | --- | --- |
| `latticeaxiom:setting/input/mouse-sensitivity` | integer 1..200%，default 100 | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/invert-mouse-x` | bool，default false | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/invert-mouse-y` | bool，default false | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/raw-mouse-input` | bool，default true | device／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/scroll-sensitivity` | integer 25..200%，step 5，default 100 | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/discrete-scroll` | bool，default false | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/attack-mode` | enum `hold | toggle`，default `hold` | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/use-mode` | enum `hold | toggle`，default `hold` | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/sneak-mode` | enum `hold | toggle`，default `hold` | user／local-user | Immediate | feature-conditional |
| `latticeaxiom:setting/input/sprint-mode` | enum `hold | toggle`，default `hold` | user／local-user | Immediate | feature-conditional |
| `latticeaxiom:setting/input/sprint-window` | integer 0..20 ticks，default 7 | user／local-user | Immediate | feature-conditional |
| `latticeaxiom:setting/input/gamepad-look-x` | integer 1..200%，default 100 | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/gamepad-look-y` | integer 1..200%，default 100 | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/gamepad-invert-x` | bool，default false | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/gamepad-invert-y` | bool，default false | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/gamepad-deadzone` | integer 0..50%，step 1，default 10 | device/user／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/gamepad-response-curve` | enum `linear | relaxed | precise`，default `linear` | user／local-user | Immediate | baseline |
| `latticeaxiom:setting/input/gamepad-vibration` | integer 0..100%，step 5，default 100 | user／local-user | Immediate | feature-conditional |
| `latticeaxiom:setting/input/gyro-enabled` | bool，default false | device/user／local-user | Immediate | feature-conditional |
| `latticeaxiom:setting/input/gyro-sensitivity` | integer 1..200%，default 100；visible when gyro | user／local-user | Immediate | feature-conditional |

Controls 必须机械投影 locked action catalog，而不是另写一份 setting ID 表。首版至少覆盖：move、look、
jump、break/attack、place/use、inspect、pause、UI directional navigation、next/previous focus、
activate、back、inventory、workbench、hotbar slots 1..9 与 hotbar next/previous。pick block、screenshot、
fullscreen、debug workbench、package gameplay actions 等一旦进入 action catalog且标记 rebindable，也
必须自动出现。每行显示 action stable ID、context、owner、default bindings、effective bindings、
conflicts、clear 与 reset。

## Gameplay、World、Interface 与 package rows

这些 rows 只有真实 consumer 存在时注册：

| Owner | Stable setting ID 或 row family | Contract |
| --- | --- | --- |
| `@latticeaxiom/front-end` | `latticeaxiom:setting/gameplay/pause-on-focus-loss` | user/local-user，bool default true，Immediate，baseline |
| `@latticeaxiom/inspect` | `latticeaxiom:setting/interface/inspect-visible`、`inspect-detail`、`inspect-pin-mode` | user/local-user；default true、compact、hold；Immediate |
| `@latticeaxiom/progress` | `latticeaxiom:setting/interface/progress-tracker`、`progress-notifications`、`auto-pin-active-objective` | user/local-user；feature-conditional，Immediate |
| `@latticeaxiom/world-library` | `latticeaxiom:setting/world/default-root` | private path，device/local-user，default platform location，ProcessRestart |
| `@latticeaxiom/world-library` | `latticeaxiom:setting/world/autosave-interval` | integer 1..30 minutes，default 5，world/world-owner，Immediate |
| `@latticeaxiom/world-library` | `latticeaxiom:setting/world/backup-before-migration` | bool default true，user/local-user，Immediate |
| `@latticeaxiom/world-library` | `latticeaxiom:setting/world/trash-retention-days` | integer 1..90，default 30，user/local-user，Immediate |
| `@latticeaxiom/world-library` | `latticeaxiom:setting/world/confirm-destructive-actions` | bool default true，user/local-user；不能关闭不可逆 authority confirmation |
| `@terrenia/presentation` | `terrenia:setting/video/ambient-particles`、`fluid-animation-quality`、`environment-animations` | user/local-user；feature-conditional presentation only；Immediate或WorldReactivate |
| `@terrenia/gameplay` | `terrenia:setting/world/difficulty` | enum `peaceful | easy | normal | hard`，default `normal`，world/world-owner，Immediate |
| `@terrenia/gameplay` | `terrenia:setting/world/keep-inventory`、`immediate-respawn` | bool，default false；world/world-owner；Immediate；仅在对应机制真实存在时注册 |
| `@terrenia/gameplay` | `terrenia:setting/world/block-drops`、`mob-spawning`、`mob-griefing`、`fire-spread`、`daylight-cycle`、`weather-cycle`、`fall-damage`、`natural-regeneration` | bool，default true；world/world-owner；Immediate；仅在对应机制真实存在时注册 |

World rows 在 client没有 writer/admin authority时仍显示 effective value、控制方与原因，但不可编辑。
server policy可以限制 client presentation（例如要求保留 gameplay-relevant fog），必须产生明确
Hidden/DisabledByWorldPolicy，而不是静默覆盖本地值。

## Developer

`@latticeaxiom/dev-tools` 在 dev profile 中至少贡献：

| Stable setting ID / family | Default 与 contract |
| --- | --- |
| `latticeaxiom:setting/developer/overlay-enabled` | false，session/user，Immediate |
| `latticeaxiom:setting/developer/overlay-detail` | `minimal | normal | verbose`，default normal |
| `latticeaxiom:setting/developer/sample-period-ms` | 100..5000，default 500 |
| `latticeaxiom:setting/developer/history-seconds` | 5..300，default 30 |
| `latticeaxiom:setting/developer/visualizer-radius` | provider-declared unit，default/provider max 受 host hard cap |
| `latticeaxiom:setting/developer/visualizer-depth-mode` | `depth-tested | x-ray`，default depth-tested；x-ray需权限 |
| `latticeaxiom:setting/developer/visualizer-labels` | true，session/user |
| `latticeaxiom:setting/developer/log-level` | `error | warn | info | debug | trace`，default info；trace可受 profile policy禁止 |

每个 registered DebugVisualizerSpec 机械产生 enable row，并按 Grid、Lifecycle、Persistence、Mesh、
Collision、Visibility/LOD、Worldgen 分组。radius、update rate、primitive/upload/history budget 不得
超过 host hard policy；关闭 workbench 后专用 sampling/query/upload 必须回到 zero。

## Packages category operations

下列都是 operation，不保存成 setting value：

- search/filter by owner package；
- reset current row、section、category、package 或全部 non-authoritative user/device values；
- export/import non-sensitive canonical values，并预览完整 diff、migration 与 unsupported rows；
- inspect orphan values、保留来源并按 package 明确删除；
- show provenance、scope、authority、apply impact、schema version 与 stable ID；
- open profile draft 处理 provider、feature、realization、package version 与其他 composition values。

reset/import 不得越过 world/server authority，private path 在 export/report 中默认 redacted。profile draft
必须通过 resolve → graph diff → candidate lock → explicit confirmation，不能把 package enable/disable
做成即时 toggle。

## Package 明确无设置

并非每个 package 都要制造设置。下列 package 当前必须明确声明无用户可配置 runtime settings：

- `@latticeaxiom/settings`、`@latticeaxiom/observability`：foundation registries，不拥有玩家偏好；
- `terrenia`：聚合 closure，只拥有 grants/role bindings；
- `@terrenia/blocks`、`@terrenia/tools`：authoritative content data，不把材料或平衡常量暴露为偏好；
- `@terrenia/worldgen`：seed、preset、provider 与生成参数属于 New World/profile/world freeze；
- `@latticeaxiom/relations`、`@terrenia/metallurgy`、`@terrenia/science`、
  `@terrenia/thaumaturgy`、`@terrenia/journey`：尚无真实可配置 runtime consumer。

未来新增真实 consumer 时，由 owning package 增加 stable rows、migration、availability 与 requirement；
不能因 package 有配置文件或内部常量就自动出现在玩家设置页。

## 外部产品依据

- [Sodium option groups、impact、dependency 与 apply flags](https://github.com/CaffeineMC/sodium/blob/dev/common/src/main/java/net/caffeinemc/mods/sodium/client/gui/SodiumConfigBuilder.java)
- [Sodium 配置扩展 API](https://github.com/CaffeineMC/sodium/blob/dev/common/src/api/java/net/caffeinemc/mods/sodium/api/config/USAGE.md)
- [Controlify sensitivity、invert、deadzone 与 response curve](https://github.com/isXander/Controlify/blob/main/src/main/java/dev/isxander/controlify/config/settings/profile/InputSettings.java)
- [Dynamic FPS 的 focus/idle/battery frame 与 volume policy](https://github.com/juliand665/dynamic-fps)
- [Iris package-defined shader setting groups、sub-screens 与 sliders](https://github.com/IrisShaders/docs/blob/main/src/content/docs/current/Reference/Shaders.Properties/shader_settings.mdx)
- [Minecraft Java 1.21.9 Controls 与 Accessibility 变更](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-9)

这些来源用于证明玩家需要的设置问题形状与复杂度，不复制 Minecraft 的配置文件、Mixin 或私有
renderer API。Lattice 的 stable owner、scope、authority、transaction、layout tier 与 fallback 仍由
本项目契约定义。

## 相关文件

- [Settings registry 与 transaction](settings-and-configuration.md)
- [分层 settings surface](../settings-ui/settings-surface.md)
- [Client presentation package](../client-presentation/README.md)
- [Input binding contract](../../../platform/input/input-binding-and-contexts.md)
- [Rendering platform](../../../platform/rendering/rendering.md)
