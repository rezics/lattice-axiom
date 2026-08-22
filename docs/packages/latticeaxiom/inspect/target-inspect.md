---
title: Target inspect surface
document_id: package.latticeaxiom.inspect.target-inspect
document_status: accepted
document_type: package-spec
owners:
  - "@latticeaxiom/inspect"
tracks_implementation: true
requirements: []
updated: 2026-08-22
---

# Target inspect surface

## Pipeline

1. authoritative/player targeting 产生 target receipt；
2. registration image 找到 target identity、owner package 与可用 typed fragment providers；
3. observability policy 过滤权限和成本；
4. inspect package 以稳定顺序组合 name、summary、detail 与 action hints；
5. client UI 投影为准星附近的单一 overlay。

inspect 不重新 raycast，不读取任意 package private ECS component，也不让 providers 直接生成
Bevy nodes。没有 target、资料未加载或权限不足是不同的 typed state，UI 必须可区分。

## First-playable 内容

首个 surface 至少显示 display name、icon/fallback、owner package 与 technical StableId；适用时
显示可挖掘性、当前工具效果与交互拒绝原因。玩家信息与 developer detail 分层，F3 workbench
不能成为查看基本名称的唯一方式。

## 输入与可访问性

inspect 默认只读，不占用 gameplay action。需要展开 detail 时通过 client surface action 与
统一 focus/context stack 进入 overlay；关闭后恢复 gameplay context。内容必须具有 AccessKit
语义，并在无纹理、无翻译或 provider fault 时显示 deterministic fallback。

