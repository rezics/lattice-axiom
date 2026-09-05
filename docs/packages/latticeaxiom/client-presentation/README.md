---
title: "@latticeaxiom/client-presentation"
document_id: package.latticeaxiom.client-presentation.index
document_status: accepted
document_type: index
owners:
  - "@latticeaxiom/client-presentation"
tracks_implementation: true
requirements:
  - CLIENT-PRESENTATION-SETTINGS-001
updated: 2026-08-24
---

# `@latticeaxiom/client-presentation`

## 身份与边界

- Source lifecycle：产品契约已接受；实现仓 manifest、source 与 product lock 选择尚未建立。
- Domain：client-only；headless 可省略。
- Provides：`latticeaxiom:capability/client-presentation-settings@1` exactly-one。
- Requires：`@latticeaxiom/settings` registry，以及 selected window/render/audio host capabilities。

本 package 拥有跨 shell 与 client-world 的通用 device/user presentation preferences：display、
Video、Audio、字幕、旁白与镜头视觉可访问性。它不拥有 settings surface、Terrenia-specific
texture/material/effect policy、render provider 私有算法设置或 authoritative gameplay。

将这些 rows 放进独立 logical package，避免 `@latticeaxiom/settings-ui` 因为负责绘制页面而错误
取得 video/audio 语义，也避免 `@terrenia/presentation` 把通用 client 偏好绑死到一个维度。

## Settings contribution

- Accessibility：narrator mode、subtitles、subtitle direction/scale/background、camera shake、
  damage tilt、distortion；
- Audio：master、music、ambient、weather、blocks、hostile、neutral、players、UI、records、
  narrator volumes，以及 output device、directional/mono audio、background volume、music frequency；
- Video / General：performance preset、window mode、monitor、resolution、refresh rate、VSync、
  foreground/background frame limit、FOV、FOV effects、brightness、typed `2..=32` view distance；
- Video / Quality：graphics、particles、smooth lighting、biome blend、entity distance/shadow、fog、
  weather、chunk fade、mipmap、anisotropy、texture filtering、fluid quality；
- Video / Performance/Advanced：chunk update mode/threads、face/fog/entity culling、visible texture
  animation、render-ahead、selected backend/GPU 与 validation mode。

完整 stable IDs、defaults、constraints、scope、impact 与 availability 由
[shipped settings catalog](../settings/shipped-settings-catalog.md) 定义。optional GPU/audio feature
缺失时对应 row 不注册；device enumeration 失败时保留 requested value，显示 effective fallback 与
diagnostic，不能覆写 user preference。

selected render、upscaler、shader、visibility 或 terrain provider 可以用自己的 namespace 添加
feature-conditional Video rows。普通 rows 与 declarative subpages 使用官方 settings foundation；
需要 specialized editor 时遵守 [settings surface layering](../settings-ui/settings-surface.md)，
不得直接写私有配置文件或占用 root overlay。

## 规范

- [Shipped settings catalog](../settings/shipped-settings-catalog.md)
- [分层 settings surface](../settings-ui/settings-surface.md)
- [Rendering platform](../../../platform/rendering/rendering.md)
- [Shared client UI system](../../../platform/client-ui/ui-system.md)

## Implementation mapping

新增 package source root 和 client adapter，复用 Bevy window/render/audio resources、app settings 与
`@latticeaxiom/settings` transaction。现有 engine host 中的 window、view-distance 或 audio 常量
不是本 package 已实现的证据；实现必须来自 reopened lock 的 registration rows，并同时进入 shell 与
client-world projections。

