---
title: Bevy 實體、物理與表現層
status: exploration
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
---

# Bevy 實體、物理與表現層

## 問題範圍

冒險內容需要怪物、Boss、飛行、攀牆、彈跳與受擊效果，但不需要每種內容擁有自己的 ECS、物理解算器或渲染器。本頁定義玩法實體、物理和表現如何直接組合 Bevy 能力。

## Bevy ECS 是實體模型

活躍 gameplay entity 直接是 Bevy ECS entity，由小型 component 和 system 組成：

```text
Ancient Dragon
├── gameplay components
│   ├── Health
│   ├── BossState
│   └── FlyingController
├── physics plugin components
│   ├── rigid body / character body
│   ├── collider
│   └── collision layers
└── Bevy presentation components
    ├── Transform
    ├── Mesh / Scene handle
    ├── Material
    ├── Animation
    └── visibility / effects
```

專案不在前面建立 backend-neutral entity facade。Bevy `Entity` 可以在一次執行內被 component、event／message 與 query 使用；需要跨卸載或存檔的身分另用 stable entity key。

## 內容組合而非繼承樹

內容優先以能力與資料組合：

- 可受傷；
- 有地面或飛行移動；
- 有物理 body／collider；
- 有骨架姿勢；
- 可被擊退；
- 有裝備插槽；
- 支援某種 presentation reaction。

系统查询所需component，而不是识别“这是不是龙”。官方与外部content packages使用相同stable component／registration contract：static realization生成typed Bevy bundle／system，dynamic realization生成ABI-POD batch／command adapter；两者不要求共享Bevy Rust type。

## 物理採用順序

第一個可玩 spike 優先評估 Avian 的 Bevy-native integration。要驗證：

- Y-up 重力、地面接觸和 character movement；
- voxel chunk collider 的建立、更新與卸載；
- ray cast／shape cast 是否能支援挖掘和放置；
- 物理 step 與 Bevy `FixedUpdate` 的整合；
- fast-moving entity、坡面、台階與 chunk 邊界；
- active chunk 數下的 CPU、memory 和 collider rebuild latency。

Core host与`NativeStatic` gameplay可直接使用physics plugin的Bevy component／query API；portable dynamic gameplay经Lattice spatial／physics capability或稳定gameplay components访问所需资料，不穿越solver Rust layout。以下长期边界必须使用项目DTO／stable schema：

- 存檔中需要恢復的 pose、velocity、sleep 或 constraint 語義；
- 未來網路 snapshot；
- 物理 plugin 更新時的 migration fixture；
- chunk collider 的權威來源 revision。

solver handle、broadphase、contact cache 和 plugin-internal entity mapping 都不可持久化。

## 自訂移動不等於自訂 solver

| 內容需求 | Bevy／物理 plugin 上的可重用做法 |
| --- | --- |
| 龍飛行 | controller system 產生目標速度／力，使用既有 cast 與碰撞 |
| 蜘蛛攀牆 | surface query、姿態對齊和狀態 component |
| 史萊姆彈跳 | contact event／message 加 impulse policy |
| 幽靈穿牆 | collision layer 或 sensor policy |
| 球形重力 | domain system 計算重力方向，solver 繼續處理碰撞 |

布料、流體、破壞或特殊柔體可作日後獨立 feature，但必須先有可玩需求與 Bevy 生態調查；不能以可能性為由先抽象整個物理 backend。

## 權威玩法與表現

玩法只產生需要被模擬或保存的事實：

```text
DamageOccurred
stable target + amount + contact + impulse + tick
```

presentation system 再把它轉成：

- animation reaction；
- material flash；
- particle／sound；
- camera feedback；
- local-only UI。

純表現可以掉幀、停用或重建，不得改變傷害結果。若某效果影響 hit timing、visibility 規則或 movement，它就是 gameplay state，必須在 FixedUpdate 的權威系統中表達。

## 姿勢與材質

優先使用 Bevy animation、scene、material 和現有生態工具。多個修飾器需要時，以清楚的 Bevy SystemSet／animation graph 層次定義順序：

```text
base animation
  → locomotion
  → aim / look
  → hit reaction
  → secondary motion
  → final presentation pose
```

不要在第一個 demo 建立通用 animation framework。用膠囊或低多邊形 placeholder 驗證 movement、collision 和 reaction 已足夠。

## 無限世界位置

權威位置以整數 chunk anchor 加局部位置保存；active physics world 與 Bevy Transform 只覆蓋附近 working set。floating origin 是否必要由精度原型決定，並優先採用維護中的 Bevy 生態方案。

跨 rebase 時只改變 presentation／physics local origin，不改變 stable world coordinate。rebase 必須發生在明確 FixedUpdate barrier，且 collider、raycast 與相機在同一個 local frame。

## 驗收

- player 可在 Y-up voxel world 行走、跳躍、raycast、挖掘和放置。
- official entity与test content entity只经package manifest／公开registration contract；static／dynamic realization产生相同stable IDs与schema。
- render frame rate 改變不影響固定 tick gameplay 結果。
- collider rebuild 的過期結果不覆蓋新 chunk revision。
- 保存／載入不序列化 Bevy Entity 或物理 plugin handle。

## 相關文件

- [Bevy 執行期架構](game-engine-runtime.md)
- [渲染架構](rendering.md)
- [資產語義](asset-semantics.md)
- [物理資產與局部形變創作](physical-authoring.md)
- [渲染與物理候選](../research/renderer-physics-landscape.md)
