---
title: 實體、物理與表現層
status: exploration
type: explanation
updated: 2026-08-09
---

# 實體、物理與表現層

## 問題範圍

冒險內容需要加入全新的怪物、Boss、飛行、攀牆、彈跳與受擊效果，但這不代表每個模組都需要自己的渲染器或物理解算器。本頁整理三層責任：實體玩法、物理機制與感知表現。

## 自訂實體是通用基元的組合

一個內容模組中的龍可能由以下資料與系統構成：

```text
Ancient Dragon
├── 玩法
│   ├── Health
│   ├── Dragon AI
│   ├── Boss Combat
│   └── Flying Movement
├── 物理
│   ├── Body / Character Controller
│   ├── Collision Shape
│   └── Hit Zones
└── 視覺
    ├── Mesh
    ├── Skeleton
    ├── Material
    ├── Animation Graph
    └── Particle Emitters
```

「龍」由模組定義；碰撞體、骨架、材質與粒子等則是核心或共通機制提供的基元。

## 自訂移動不等於自訂物理解算器

多數內容需要的是特殊控制與施力邏輯：

| 內容需求 | 可重用機制 |
| --- | --- |
| 龍飛行 | 飛行移動系統，查詢碰撞並更新物理身體 |
| 蜘蛛攀牆 | 表面偵測、姿態對齊與攀爬控制器 |
| 史萊姆彈跳 | 接觸事件與彈性衝量策略 |
| 幽靈穿牆 | 無碰撞或選擇性碰撞政策 |
| 球形重力 | 依實體到行星中心計算重力方向 |

底層 broad phase、shape cast、ray cast、接觸、constraint 與剛體整合仍可交給共用物理引擎。

普通模組 API 可以提供：

- Collider、Rigid Body 與 Character Controller
- Force、Impulse 與 Constraint
- Ray Cast、Shape Cast 與 Trigger
- 可註冊的 Movement、Gravity、Buoyancy 或 Field System

只有布料、流體、破壞、特殊柔體或完全不同的模擬世界，才可能需要高權限 Physics Feature 或另一個 backend。

## 表現層把事件轉成感知結果

模擬只產生與玩法有關的事實：

```text
Damage Event
entity + amount + contact + impulse
```

表現層再把同一事件分派到可組合模組：

```text
Damage Event
├── Hit Reaction
├── Material Flash
├── Particle / Sound
├── Camera Feedback
└── Stagger Presentation
```

渲染器最後只收到已合成的姿勢、材質參數、網格與效果資料，不需要理解「受傷」「燃燒」或「中毒」等玩法詞彙。

## 姿勢與材質修飾器

通用表現可以形成有順序的修飾器堆疊：

```text
基礎動畫
  + 移動步態
  + 注視方向
  + 受擊反應
  + 後座與擊退
  + 程序性次級運動
        ↓
最終骨架姿勢
```

```text
基礎材質
  + 受傷閃爍
  + 燃燒
  + 冰凍
  + 中毒
        ↓
最終材質參數
```

內容作者只需提供必要差異，例如重型生物的反應設定，或宣告某些能力不適用。整合包也可以全域替換 `hit-reaction` 或 `locomotion-presentation`，讓官方與第三方實體一起改變風格。

## 模組組合而非繼承樹

原始討論曾用「Living Entity → Hostile Creature」說明預設值，但長期方向應優先考慮能力與資料組合，避免把所有生物塞入僵硬的類別繼承樹。

一個實體可以分別宣告：

- 可受傷
- 有骨架姿勢
- 有地面接觸點
- 可被擊退
- 支援局部柔性形變
- 有裝備插槽

每個表現或物理模組根據它需要的能力查詢實體，而不是辨識「這是不是龍」。

## 需要明確定義的衝突

- 多個姿勢修飾器如何排序、混合與限制權重？
- 遊戲規則上的硬直與純視覺後仰如何區分？
- 本地鏡頭與音效回饋是否需要網路同步？
- 表現模組缺少模型語義時，應使用回退、停用還是拒絕載入？
- 模組能否替換全域預設，以及伺服器是否有權強制表現設定？
- 高階物理 feature 如何隔離資源、執行時間與不穩定性？

## 相關文件

- [渲染架構與擴充邊界](rendering.md)
- [資產管線與模型語義](asset-semantics.md)
- [物理資產與局部形變創作](physical-authoring.md)
