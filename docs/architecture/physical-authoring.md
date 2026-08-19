---
title: 物理資產與局部形變創作
status: exploration
type: explanation
updated: 2026-08-09
---

# 物理資產與局部形變創作

## 設計意圖

內容作者在 Blender 或其他 DCC 中描述模型各部位的物理性質；遊戲執行時，通用物理與表現模組根據接觸位置、衝量與這些性質，自動產生局部反應。

龍的腹部柔軟，不應被寫成：

```text
if sword hits belly:
    play belly wobble
```

而應是：

```text
劍與表面接觸
    ↓
取得接觸點與衝量
    ↓
取樣局部物理場
    ↓
選擇適當模擬層級
    ↓
局部凹陷、傳播、阻尼與回復
```

## 三種不同資訊

### 幾何與動畫

網格、法線、UV、材質、骨架、skin、動畫與 morph target。

### 物理性質

描述「這個區域如何反應」的連續或離散資料：

| 候選欄位 | 意義 |
| --- | --- |
| density / mass | 慣性與質量分布 |
| stiffness | 抵抗形變的程度 |
| compliance | 受力後允許形變的程度 |
| damping | 振動消散速度 |
| thickness | 薄膜或表面厚度 |
| friction | 切向接觸行為 |
| restitution | 回彈 |
| deformability | 是否與多大程度允許局部形變 |
| tear resistance | 是否與多難產生破裂；目前僅是候選 |

### 互動語義

描述區域「在遊戲設計上是什麼」，例如鱗片、皮膚、脂肪、肌肉、骨骼、布料或翼膜。

物理值與語義必須分開。兩隻都標記為「肉」的生物，仍可有完全不同的剛性與阻尼；語義不應偷偷帶入固定物理常數。

## 創作體驗候選

Blender add-on 可以沿用 weight paint 或 attribute paint 的思維：

- 選擇 `Stiffness`、`Damping`、`Density` 等欄位。
- 在模型表面繪製權重。
- 指定剛體、可形變、布料或停用等模擬區域。
- 預覽接觸衝量下的回應。
- 匯出交換格式與遊戲物理語義。

這不代表執行期直接載入或重現 Blender Soft Body。DCC 設定是創作語義，遊戲引擎可以把它編譯成完全不同的即時表示。

## 資產編譯與模擬代理

高解析度渲染網格不應逐頂點參與昂貴柔體模擬。資產編譯器可以：

```text
高解析度渲染網格
        ↑ 形變轉移
低解析度模擬 cage
        ↑
局部柔體、彈簧或 PBD 解算
```

不同區域可採不同表示：

- 頭骨與爪：剛體或骨架蒙皮
- 鱗片背部：微小程序性形變
- 腹部與肌肉：低解析度局部柔體
- 翼膜：布料或受限柔體

## 品質與成本分級

| 層級 | 可能實作 | 適用情境 |
| --- | --- | --- |
| 0 | 剛體／骨架反應 | 遠距離、低重要性實體 |
| 1 | 接觸點周圍的程序性位移與彈簧回復 | 一般敵人或中距離 |
| 2 | 低解析度 spring/PBD 柔體區域 | 近距離 Boss 與重要部位 |
| 3 | 更完整的可形變模擬 | 極少數特寫或離線效果；是否需要仍未證明 |

LOD 可以同時依距離、螢幕面積、互動重要性與效能預算選擇，不只依模型幾何距離。

## 混合姿勢流程

```text
骨架動畫姿勢
    ↓
附著／跟隨約束
    ↓
碰撞與外力
    ↓
次級形變解算
    ↓
形變轉移到渲染網格
    ↓
最終姿勢與頂點
```

走路造成的加速度也能進入相同系統，讓柔軟組織因慣性落後並回彈；受擊動畫只需補足 AI 硬直、表情、音效或鏡頭等物理模型無法表達的高階效果。

## Schema 候選範圍

```text
Game Physical Asset
├── Geometry and Animation
├── Skeleton Semantics
├── Physical Fields
├── Simulation Regions
├── Collision Representation
└── Interaction Semantics
```

原對話曾以 `EXT_game_physical_surface` 與 `EXT_game_deformable` 說明可能的 glTF 擴充，但這些名稱目前只是佔位符，不得當成已註冊的 Khronos 擴充。

## 最需要原型回答的問題

- 頂點、面、體積與 simulation cage 之間如何傳遞物理場？
- 局部柔體如何與骨架、碰撞、動畫重定向及破壞狀態組合？
- 多人遊戲同步的是輸入事件、低解析度狀態，還是只同步玩法結果？
- 不同硬體如何在不同 LOD 下維持可接受的遊戲公平性？
- 物理場的單位、範圍、預設值與材料校準方式是什麼？
- 模型拓樸改變後，作者繪製的物理資料如何保留？
- 通用系統能否在沒有逐怪物調參時產生穩定而不滑稽的結果？

## 初步來源

- [Blender：Vertex Groups](https://docs.blender.org/manual/en/latest/modeling/meshes/properties/vertex_groups/index.html)
- [Blender：Soft Body](https://docs.blender.org/manual/en/latest/physics/soft_body/index.html)
- [Jolt Physics：Soft Body 文件](https://jrouwe.github.io/JoltPhysicsDocs/5.5.0/index.html)
- [Khronos glTF Registry](https://registry.khronos.org/glTF/)
- [OpenUSD Physics](https://openusd.org/release/api/usd_physics_page_front.html)

來源只證明可借鑑的現有能力，不證明已存在一套滿足本頁全部需求的通用遊戲物理資產標準。

## 相關文件

- [資產管線與模型語義](asset-semantics.md)
- [實體、物理與表現層](entity-physics-presentation.md)
- [待決問題](../planning/open-questions.md)
