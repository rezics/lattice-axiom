---
title: Godot 作為作者工具鏈對照組
status: exploration
type: research
updated: 2026-08-19
---

# Godot 作為作者工具鏈對照組

## 結論

Godot 可以作未來視覺 authoring／asset-import 工具的對照組，但不是 Lattice Axiom 的遊戲 runtime、第二 renderer 或權威場景格式。現在不開發 Godot bridge；只有真實作者工作流證明 Blender + glTF + Bevy 工具不足時，才啟動限時 spike。

## 為何值得作對照

Godot 提供成熟 editor、scene／resource 工作流、import／export 擴充、EditorPlugin、GDExtension 和可由低階 server API 組成的工具。這使它適合回答「是否能用既有 editor 快速完成某項作者工作」，而不是假設 Lattice Axiom 必須自製完整編輯器。

官方參考：

- [Godot MIT license](https://godotengine.org/license/)
- [Godot Engine API overview](https://docs.godotengine.org/en/stable/engine_details/engine_api/index.html)
- [`MainLoop`](https://docs.godotengine.org/en/stable/classes/class_mainloop.html)
- [`RenderingServer`](https://docs.godotengine.org/en/stable/classes/class_renderingserver.html)
- [3D scene import workflow](https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/importing_3d_scenes/index.html)
- [dedicated server／headless export](https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_dedicated_servers.html)

這些能力只證明 Godot 可作候選工具；是否適合本專案仍需要資料 round-trip 與作者時間測試。

## 啟動條件

至少有一個真實、重複出現的工作流失敗：

- glTF + sidecar 無法可靠標註 socket、hit zone、collider intent 或 semantic region；
- 非程式作者無法預覽內容定義、碰撞、animation 或材質；
- worldgen／群系／結構 authoring 需要空間視覺化，現有 Bevy gizmo／專用小工具明顯不足；
- 資產匯入錯誤頻繁且缺乏可診斷 UI；
- 同一工作在 Blender／文字工具中有已量測的高成本。

「Godot 有 editor」或「未來可能需要工具」不是啟動條件。

## 基線與對照

### 基線

```text
Blender / ordinary DCC
        ↓ glTF + optional typed sidecar
Bevy AssetServer / AssetLoader
        ↓
Lattice Axiom runtime
```

### Godot 對照

```text
Godot editor plugin / import UI
        ↓ explicit export contract
glTF + versioned sidecar / project typed asset
        ↓
Bevy AssetServer / AssetLoader
        ↓
Lattice Axiom runtime
```

Godot 不輸出 Bevy `Entity`／`Handle`，也不把 `.tscn`／`.tres` 直接變成權威 runtime 依賴。若 spike 選擇其他交換格式，必須有 versioned schema、獨立 validator 和非 Godot reader。

## 第一個 spike 的代表任務

只選一個有痛點的任務，使用三個代表資產：

1. 靜態方塊／道具：mesh、material、collider intent。
2. 有骨架生物：animation、socket、hit zone。
3. 空間結構：placement anchor、bounds、semantic tags。

同一任務分別用基線和 Godot 對照完成，記錄：

- 第一次完成時間與第二次重複時間；
- 手動步驟、失敗點與可診斷性；
- export 的 deterministic／diff-friendly 程度；
- Y-up、unit、animation、node identity 與 semantic round-trip；
- CI 能否在無 GUI 下驗證／匯出；
- 工具版本升級與跨平台成本；
- Godot runtime 是否完全未進遊戲 binary。

## 採用門檻

只有以下條件全部成立，才進入正式工具鏈提案：

- 作者時間或錯誤率有明確改善；
- export contract 小而穩定，可由非 Godot validator 驗證；
- 產物能由 Bevy 標準／小型自訂 loader 載入；
- client／headless 權威語義一致；
- CI 可重現 export 或至少驗證 checked-in artifact；
- Godot 版本、plugin 維護者、license／notices 和退出策略清楚；
- 不要求遊戲同時嵌入 Godot runtime。

若只是一次性預覽更方便，保留為開發輔助，不建立正式 bridge。

## 不能跨越的邊界

- Bevy 保持唯一遊戲 runtime。
- Y-up、stable content ID、save schema 與 gameplay semantics 由 Lattice Axiom 定義。
- Godot scene node ID 不進存檔或網路。
- Godot physics／render 設定不自動成為權威資料；必須轉成專案 typed semantic。
- 不為了使用 editor 建立 Bevy ↔ Godot 雙向即時同步。
- 不在首個可玩 demo 前投入這項工作。

## 若需要正式文件

只有採用門檻通過後，才新增：

- authoring tutorial；
- export schema reference；
- Godot editor plugin 開發／版本支援指南；
- CI validation／migration runbook；
- toolchain ADR。

在此之前，本頁就是唯一的 Godot 工具鏈文件，避免把候選誤寫成承諾。

## 相關文件

- [資產語義](../architecture/asset-semantics.md)
- [技術棧](../foundations/technology-stack.md)
- [開發策略](../foundations/development-strategy.md)
- [待決問題](../planning/open-questions.md)
