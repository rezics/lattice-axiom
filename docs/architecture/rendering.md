---
title: 渲染架構與擴充邊界
status: proposed
type: explanation
updated: 2026-08-19
decision:
  - ../decisions/0006-wgpu-behind-rendering-facade.md
---

# 渲染架構與擴充邊界

> demo 唯一 GPU 後端採 wgpu、第一階段直接使用 WGSL，並以 `lattice-render` 門面及 headless 實現隔離，已由[決策 0006](../decisions/0006-wgpu-behind-rendering-facade.md)採納。長期 shader IR、Render Feature 與 Render Graph 擴充仍在探索。

## 核心原則

**模組描述如何呈現，渲染器統一決定如何執行。**

普通內容模組不應直接發送 GPU 命令。若每個實體或模組自行 `draw()`，渲染器便難以全域排序、剔除、批次化、實例化、管理資源生命週期與安排同步。

## demo 邊界

```text
lattice-core / content
        │ produces RenderWorld
        ▼
   lattice-render
    ┌──────┴──────┐
    ▼             ▼
lattice-render-  lattice-render-
wgpu             headless
```

門面第一版只包含相機、區塊網格實例、一般實體實例、`MeshId`、`MaterialId`、網格與材質上傳，以及 `submit`。核心流程固定為「模擬 tick → 擷取 `RenderWorld` → `renderer.submit`」；渲染 crate 不訂閱玩法事件，內容 crate 編譯期依賴不到 wgpu。

區塊先用 greedy meshing、純色調色盤、每區塊網格緩衝與 CPU 視錐剔除；一般實體先用 instanced cube。demo 只有 chunk、entity 與 egui 所需的最少 pipeline。

## 模擬世界與渲染世界分離

```text
模擬世界
實體、生命值、AI、物品、群系
        ↓
渲染擷取
        ↓
渲染世界
網格實例、骨架姿勢、燈光、粒子、地形區塊
        ↓
剔除與排序
        ↓
批次化與 Render Graph
        ↓
GPU
```

例如「龍」是遊戲內容概念；渲染器只應收到蒙皮網格、骨架姿勢、材質參數、變換與粒子發射器。

這個分離帶來兩個目標：

- 官方與第三方內容在進入渲染世界後沒有天然差異。
- 渲染器可以根據整個畫面的資料做全域最佳化，而不是服從模組呼叫順序。

## 內容描述如何被消化

```text
模組中的 Render Description
        ↓ 載入與驗證
Render Archetype
        ↓ 配置控制代碼
Pipeline ID + Mesh ID + Material ID
        ↓ 每幀只更新必要資料
Instance Buffer
        ↓
批次繪製
```

方塊、群系與一般實體尤其適合資料化：

- 方塊描述幾何類型、材質與不透明性，區塊網格器再產生最佳化網格。
- 群系提供天空、霧、水體、環境光與地表參數。
- 實體提供網格、材質、骨架、動畫狀態與效果資料。

正式渲染時不需要再查詢「這個三角形來自哪個模組」。

## 擴充能力分級

### 第一級：渲染資料

適用於絕大多數內容：

- 網格與蒙皮網格
- 材質與貼圖
- 動畫
- 粒子
- 方塊模型與地形表面
- 燈光與環境參數

這一級應最穩定、最容易跨平台，也最容易與官方內容共用熱路徑。

### 第二級：渲染功能

供需要新表現方式的模組使用：

- 自訂材質節點或 shader
- 自訂幾何產生器
- 自訂粒子模擬
- 後處理效果

模組仍提供可編譯的描述，不直接擁有 GPU 裝置與底層資源。

### 第三級：Render Graph 擴充

處理遞迴傳送門、體積光線步進或特殊計算 pass 等需求。模組可以宣告：

- 輸入與輸出資源
- pass 之間的前後關係
- 所需格式、佇列與能力
- 可以接受的回退方案

渲染核心仍負責相依性分析、資源別名、生命週期、同步 barrier、排程與平台轉譯。

### 非一般 API：原始圖形存取

Raw Vulkan、Direct3D 12 或 Metal 存取若存在，應被視為不安全的原生擴充，而不是普通內容 API。它會破壞跨平台、同步、裝置遺失復原與效能保證。

## Shader 與建置

demo 直接提交專案控制的 WGSL。讓第三方模組提交統一 shader 來源或中介表示，再由建置與載入管線產生各平台程式，仍是長期候選；尚未決定使用 WGSL 子集、Slang、HLSL 衍生流程或自有 IR。

宣告式整合包在建置前已知完整功能集合，理論上可以：

- 移除不存在的 shader 變體
- 預先編譯 pipeline
- 建立 pipeline cache
- 合併或打包貼圖與材質資料
- 預先做網格、meshlet 與資源配置最佳化

動態資料夾掃描仍可走相同登錄流程，只是第一次載入時可能需要編譯新增的 shader 與 pipeline，再寫入快取。

## 渲染核心應理解什麼

合理的核心基元可能包括：

- Mesh Renderer
- Skinned Mesh Renderer
- Terrain Renderer
- Particle Renderer
- Lighting Renderer
- UI Renderer
- Material System
- Render Graph

核心不應內建 `ZombieRenderer`、`DragonRenderer` 或 `GrassBlockRenderer` 之類內容類別。

## 不變量

以下前四項是 demo 要驗證的已採納邊界；第五項在開放特殊渲染功能時再成為必要契約：

1. 普通內容模組無法直接發送 GPU 命令。
2. 渲染世界不包含具體玩法類型。
3. 所有內容先被轉成有限的渲染基元或經審核的渲染功能。
4. 渲染擷取有明確的記憶體所有權與同步邊界。
5. 無法使用特殊功能時，模組能宣告回退表現或明確拒絕載入。

## 主要風險

- 渲染擷取若複製過多資料，可能把模組抽象成本轉成記憶體頻寬成本。
- Render Graph 擴充的資源與排序衝突需要可診斷的錯誤模型。
- 自訂 shader 會引入編譯時間、變體爆炸、安全性與跨平台一致性問題。
- 靜態整合包的全域最佳化收益仍需與建置時間、快取命中率一起量測。
- 允許多少全新渲染基元，是自由度與可最佳化程度之間最關鍵的產品選擇。

## 原型量測建議

- 相同渲染基元下，官方與第三方內容的 CPU/GPU 成本是否一致。
- 1 萬、10 萬個實例的擷取、剔除與提交成本。
- 動態加入一個材質或 pass 後的首次啟動與快取啟動時間。
- 多個 Render Graph 擴充互相競爭資源時的診斷品質。
- 無特殊擴充、一般擴充與 raw-native 擴充的隔離程度。

## 相關文件

- [模組核心與宣告式組合](module-composition.md)
- [實體、物理與表現層](entity-physics-presentation.md)
- [渲染與物理技術地圖](../research/renderer-physics-landscape.md)
- [決策 0006：wgpu 與渲染門面](../decisions/0006-wgpu-behind-rendering-facade.md)
- [第一個 demo 路線圖](../planning/roadmap-first-demo.md)
