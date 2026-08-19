---
title: 渲染與物理技術地圖
status: exploration
type: research
locale: zh-Hant
updated: 2026-08-09
---

# 渲染與物理技術地圖

> 這是早期研究整理出的候選清單，不是選型結論。專案尚未逐項建立、整合或進行效能測試；版本、平台與授權狀態必須在評估時重新查證。

## 需求輪廓

目前想調查的不是完整商用遊戲引擎，而是可被自有遊戲核心、ECS、模組系統與資產管線使用的獨立渲染或物理底座。

理想候選應讓 Lattice Axiom 保有：

- 自己的模組發現、組合與信任模型
- 模擬世界與渲染世界的資料邊界
- 自訂資產編譯流程
- 官方與第三方內容共享的高效熱路徑
- 逐步增加高權限渲染或物理 feature 的可能性

## 渲染候選

| 專案 | 初步定位 | 可能適合的情境 | 先驗風險／待查 |
| --- | --- | --- | --- |
| [OGRE-Next](https://github.com/OGRECave/ogre-next) | 獨立 3D rendering engine | 希望直接取得場景、材質與較高階渲染能力 | 架構整合方式、現代 Render Graph 彈性、工具與平台需求 |
| [Filament](https://github.com/google/filament) | 跨平台即時 PBR renderer | 接受較明確的 PBR 與材質模型，重視多平台 | 客製渲染功能邊界與編輯工具整合 |
| [The Forge](https://github.com/ConfettiFX/The-Forge) | 跨平台 rendering framework | 願意自建較多引擎層，同時需要現代平台抽象 | 整合複雜度、授權與目標平台工具鏈 |
| [bgfx](https://github.com/bkaradzic/bgfx) | 跨圖形 API 的中階抽象 | 主要想避開逐一維護 Vulkan、D3D、Metal 等 backend | 場景、PBR、材質與高階 renderer 仍需自建 |
| [Diligent Engine](https://github.com/DiligentGraphics/DiligentEngine) | 現代圖形 API 抽象與渲染框架 | 想保留自有 renderer 架構並統一多 backend | 高階功能覆蓋、社群、工具與整合成本 |

## 物理候選

| 專案 | 初步定位 | 與本概念的關聯 | 待查 |
| --- | --- | --- | --- |
| [Jolt Physics](https://github.com/jrouwe/JoltPhysics) | C++ 多執行緒剛體與碰撞程式庫，另有柔體能力 | 可作為共用碰撞／剛體 backend，並原型化骨架與局部柔體混合 | 柔體成熟度、確定性、序列化、網路模型與授權細節 |
| [Rapier](https://rapier.rs/) | Rust 2D/3D 物理引擎 | 若核心採 Rust，可提供較自然的語言與型別整合 | 功能覆蓋、柔體需求、確定性與跨語言成本 |

## 評估矩陣

每個候選至少應用同一份矩陣評估，避免只靠功能清單選型。

### 架構相容

- 能否由外部 ECS 與排程器控制更新？
- 記憶體與資源所有權是否清楚？
- 是否能建立模擬世界 → 渲染擷取 → 渲染世界的邊界？
- 能否靜態連結，也能支援開發期快速迭代？
- 是否允許自有資產識別碼、快取與生命週期？

### 渲染能力

- GPU-driven rendering、indirect draw、meshlet 與大量實例
- 蒙皮、morph target、粒子、地形與材質系統
- Render Graph、compute pass 與受控擴充
- shader 編譯、反射、pipeline cache 與變體治理
- Vulkan、Direct3D、Metal、WebGPU 或目標平台覆蓋

### 物理能力

- 多執行緒、查詢、character controller 與 constraint
- 自訂重力、力場、移動系統與 collision filter
- 骨架、布料、局部柔體或外部 solver 的整合點
- 序列化、重播、伺服器權威與確定性選項

### 專案健康

- 授權與靜態再散布條件
- 發行節奏、維護者與 issue 回應
- 文件、範例、除錯器與 profiler
- 建置時間、二進位大小與依賴複雜度
- 重大版本升級與 API 穩定性

## 建議的最小原型

不要一開始做完整遊戲整合。用同一個垂直切片比較入選候選：

1. 載入一個 glTF/GLB 蒙皮模型。
2. 產生 1 萬個官方／第三方混合標記的實例。
3. 由模擬世界擷取到渲染世界並做剔除與批次。
4. 播放步態、注視與受擊姿勢修飾器。
5. 對一個低解析度腹部代理注入局部衝量。
6. 記錄建置時間、載入時間、CPU/GPU frame time、記憶體與整合程式碼量。

只有經過相同垂直切片，才能比較「現成高階能力」與「自建控制力」的真實成本。

## 尚未回答

- 核心語言是 C++、Rust 或其他選項？
- 目標平台與最低硬體是什麼？
- 是否真的需要可由第三方加入 Render Graph pass？
- 柔體局部形變是核心賣點、可選高階功能，還是過早最佳化？
- 多人確定性對物理與表現層的要求有多高？

## 相關文件

- [渲染架構與擴充邊界](../architecture/rendering.md)
- [實體、物理與表現層](../architecture/entity-physics-presentation.md)
- [物理資產與局部形變創作](../architecture/physical-authoring.md)
