---
title: 一人與 AI 協作的 Bevy-first 開發策略
status: accepted
type: overview
updated: 2026-08-19
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0016-stage-content-composition-on-bevy.md
---

# 一人與 AI 協作的 Bevy-first 開發策略

## 結論

Lattice Axiom 以一位主要開發者、AI 協作和成熟開源元件開始。開發順序不是「先完成引擎」，而是把 Bevy 的完整上游能力組成最小可玩縱切，然後只對真實阻塞做局部擴充。

第一個驗收是：

> 玩家走進程序生成的體素世界，挖掉並放回一個方塊；退出再進入後，世界仍保留變更。官方內容經公開 Bevy plugin 路徑加入。

## 買／造決策梯

每個通用能力依固定順序處理：

1. **Adopt**：直接使用 Bevy 內建能力與慣用資料流。
2. **Compose**：用 plugin、feature、schedule、asset 或設定組合現有能力。
3. **Extend**：使用公開 extension point 或維護中的 Bevy 生態 plugin。
4. **Contribute／fork**：將缺口回饋 upstream；必要時維持最小、可同步的 fork。
5. **Build**：只有可玩原型與可重現證據都證明前四步無法滿足不可妥協需求時，才建立最小自研替代。

例外證據格式見[決策 0014](../decisions/0014-adopt-bevy-upstream-first.md)。抽象出一個與 Bevy 一一對應的 facade 不算降低風險；它只會把升級工作複製到兩層。

## Bevy-first 實作方式

- client 從 `DefaultPlugins` 開始，headless／測試從 `MinimalPlugins` 與必要標準 plugin 開始。
- 玩法拆成小型 Bevy `Plugin`，系統以 `SystemSet` 表達領域順序；不用自製 lifecycle 或 scheduler。
- 權威世界資料保留在 domain resource／storage；活躍實體使用 Bevy ECS，不把每個體素做成 entity。
- 非同步工作使用 Bevy task pools；結果經 epoch／版本檢查後在明確 schedule barrier 套用。
- 渲染先使用 Bevy Mesh、Material、camera、lighting、visibility 與 render extraction。
- 資產先使用 `AssetServer`、標準 loader 與 glTF；不要先建立完整內容編譯平台。
- 物理、輸入與體素世界先做 Bevy 生態候選 spike，量測後鎖定，不先重寫。

## 人與 AI 的責任

主要開發者決定：

- 玩家價值、範圍和不可妥協需求；
- 權威資料、持久化與相容性承諾；
- 性能預算、失敗模式與停止條件；
- 外部依賴、授權、維護狀態與 fork 成本；
- 何時有足夠證據啟動新的抽象。

AI 適合處理：

- Bevy API 與既有 plugin 的窄型整合；
- property、round-trip、golden、故障注入與 migration test；
- 資料型別、演算法、工具腳本和機械重構；
- profiling、診斷與文件同步；
- 有明確輸入、輸出和驗收的小型資產或 shader 工作。

AI 不替代產品判斷。若「什麼狀態必須跨重啟保存」尚未決定，增加程式碼只會更快固化錯誤方向。

## 工作單元契約

每個工作單元都要同時有：

1. 玩家可見結果或明確的下游 consumer；
2. 要證明的不變量；
3. 採用的 upstream 能力與版本；
4. 明確不做的範圍；
5. 自動化驗收與性能／失敗預算。

例如「做區塊系統」不夠具體；「玩家修改方塊後跨重啟保留，卸載再載入不重生，mesh job 的過期結果不覆蓋新 revision」才可交付。

## demo 刻意不做

- 自研 App、ECS、scheduler、renderer、asset server、input layer 或 task runtime；
- 動態 Rust ABI、自訂套件內核、一般版本求解器與模組市場；
- 多人同步、WASM 沙箱、運行期卸載；
- 柔體、完整角色動畫、高階 PBR 美術與自研 Render Graph；
- 第二渲染後端或為不存在的作者建立完整編輯器。

demo 可以使用方塊／膠囊模型、簡單材質、最低限度碰撞、Bevy diagnostics 和開發者 gizmo。

## 工具鏈原則

優先使用 Blender、glTF、Bevy loader 與現成資產工具。若真實創作流程需要視覺化場景、import 或關卡工具，可用 Godot 作限時對照 spike；只有資料往返、語義保存與作者效率都通過驗收，才寫正式工具鏈文件或橋接器。

## 風險控制

- **Bevy 版本演進**：精確鎖版，升級時閱讀官方 migration guide，執行可玩 smoke test 與存檔回歸。
- **抽象先於需求**：任何公共抽象至少由官方內容和第二內容集合共同使用。
- **生態 plugin 維護風險**：記錄 Bevy 對應版本、授權、活躍度與退出路徑；薄整合比全域 facade 更容易替換。
- **範圍膨脹**：以[第一個 demo 路線圖](../planning/roadmap-first-demo.md)的出場條件阻止平台工作提前進入。
- **美術阻塞**：先用可讀 placeholder 驗證玩法，再逐步換成正式資產。

## 相關文件

- [專案願景](project-vision.md)
- [技術棧](technology-stack.md)
- [Bevy 執行期架構](../architecture/game-engine-runtime.md)
- [Bevy 執行期整合路線](../planning/roadmap-game-engine.md)
- [第一個可玩 demo 路線圖](../planning/roadmap-first-demo.md)
