---
title: Bevy 渲染架構與擴充邊界
status: proposed
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0015-bevy-native-y-up-world-coordinates.md
---

# Bevy 渲染架構與擴充邊界

## 結論

Lattice Axiom 使用 Bevy renderer。wgpu、render graph、GPU device、window surface 和資源上傳是 Bevy 的實作與擴充領域；專案不在外面建立 renderer facade，也不維護 headless renderer。

第一個 demo 的渲染目標是讓體素世界清楚可玩並能量測 chunk streaming，不是證明專案能自行畫出立方體。

## 預設資料流

```text
authoritative chunk / gameplay state
                ↓
     revision-checked derived work
                ↓
 Bevy Mesh / Material / Transform / Visibility
                ↓
       Bevy render extraction
                ↓
        Bevy RenderApp + wgpu
```

玩法系統不呼叫 raw GPU API。它修改權威狀態、產生需要重建的 chunk revision 或更新正常 Bevy presentation component；Bevy 負責 extraction、prepare、queue 和 render。

## 體素 chunk presentation

首個 spike 優先使用 `bevy_voxel_world` 的既有 streaming、editing、raycast 與 meshing 路徑。如果它的資料模型無法承擔權威世界，應讓它作 presentation／interaction adapter，而不是立刻 fork 整個 plugin。

若需要自有 chunk mesh job，背景工作產生 CPU-side vertex attributes、index、bounds 和來源 `ChunkRevision`；主 World system 驗證 revision 後建立或更新 Bevy `Mesh` asset。每個可見 chunk 通常只需要粗粒度 presentation entity，不為每個 voxel 建 entity。

必須量測：

- visible／resident chunk 數；
- mesh job queue、取消率、P50／P95 latency；
- 每 chunk vertex／triangle／byte 數；
- 每 frame mesh apply／asset upload 預算；
- 編輯後可見更新延遲；
- 快速移動時的 frame time、顯存與 RAM 高水位。

greedy meshing、face culling、LOD、occlusion 與 meshlet 都是量測後的優化，不是第一階段自研平台。

## 材質與 shader

採用順序：

1. Bevy 內建 PBR／unlit material；
2. Bevy `Material` extension 與 WGSL shader asset；
3. instancing、specialized pipeline 或 render phase 等公開 Bevy extension；
4. `RenderApp` extraction／prepare／queue system；
5. 只有證據門檻通過後才接觸更低層 wgpu 或替換 renderer。

方塊 atlas、texture array、vertex packing 和材質分組由 spike 量測決定。不要為假想的第二後端建立自有 shader IR。

## 相機、座標與精度

所有 Transform 使用 Bevy 原生右手 Y-up：`+X` 右、`+Y` 上、forward `-Z`。資產、physics 和 raycast 共用這套空間。

無限世界可能超過單一全域 `f32` Transform 的精度。處理順序是：

1. 以 chunk 整數座標保存權威位置；
2. 只讓目前 working set 使用相機附近的局部 `Transform`；
3. 量測可見抖動、physics 穩定性與 rebase 成本；
4. 優先評估 Bevy 生態中維護中的 large-world／floating-origin 方案；
5. 只有候選都無法達成已定精度預算時才做專案擴充。

## 權威與表現隔離

Mesh、Material、Image、GPU buffer、visibility 和 render entity 都是可重建表現。device loss、shader reload、render frame drop 或 mesh job 失敗不得修改 voxel／gameplay state。

chunk presentation component 應記住來源 revision。收到舊 mesh 時直接丟棄；卸載 chunk 時撤除 presentation entity／asset 引用，但不刪除權威 snapshot。

## Headless

headless app 不加入 window／render plugin，也不建立 `Mesh`／GPU 資源。worldgen、gameplay、physics（若伺服器需要）與 persistence 必須能在這個組合運作。

需要測試 mesher 時，直接測試其純 CPU 輸出與 winding／normal／bounds，不以「null renderer 收到 submit」作替代驗證。

## Debug 與觀測

優先使用 Bevy diagnostics、gizmos、wireframe／debug render 和現有 inspector／profiler integration。開發 overlay 至少顯示：

- FPS／frame time 與 fixed tick debt；
- camera chunk 與 active／resident chunk；
- mesh queue、過期結果與 upload bytes；
- triangle／draw／material count；
- world load／save latency。

若標準 Bevy UI 足以呈現，不加入另一個 UI framework。只有真實工具工作流需要 richer widgets 時才評估 egui 生態。

## 何時允許客製 RenderApp

只有下列條件同時成立：

- 第一個可玩 demo 已存在；
- 需求不能由 Mesh／Material／shader／正常 Bevy plugin 實現；
- profiler 證明瓶頸位於 render pipeline；
- 有 render test 或可重現 capture；
- extension 僅接入必要的 extract／prepare／queue／render node；
- 有 Bevy 升級 owner 與 fallback。

客製 RenderApp system 仍是 Bevy extension，不自動構成自有渲染器。

## 驗收

- 不直接依賴 raw wgpu 即可完成可玩體素世界。
- 六個 Y-up voxel 面的 normal、winding、ray hit 與材質方向一致。
- 編輯方塊後，只有 matching revision 的 mesh 被顯示。
- headless 測試不載入 renderer，仍可完成相同世界修改與保存。
- device／window／presentation 重建不改變權威 world hash。

## 相關文件

- [Bevy 執行期架構](game-engine-runtime.md)
- [實體、物理與表現層](entity-physics-presentation.md)
- [資產語義](asset-semantics.md)
- [技術棧](../foundations/technology-stack.md)
- [渲染與物理候選](../research/renderer-physics-landscape.md)
