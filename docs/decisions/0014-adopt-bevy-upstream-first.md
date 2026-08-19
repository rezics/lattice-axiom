---
title: 採用 Bevy 並以上游能力為預設
status: accepted
type: decision
updated: 2026-08-19
---

# 決策 0014：採用 Bevy 並以上游能力為預設

## 背景

Lattice Axiom 曾規劃自行建立生命週期、ECS 門面、排程器、輸入、資產服務、渲染抽象、headless backend 與版本化引擎執行期。這些工作大多是成熟遊戲引擎已提供的通用能力，會讓第一個可玩原型在驗證玩家價值以前，先承擔一套引擎的設計、測試、平台支援與長期維護。

[Bevy](https://github.com/bevyengine/bevy) 已提供 Rust 原生的 App／Plugin 模型、ECS、排程、固定時間、工作池、輸入、資產、場景、UI、診斷、視窗與基於 wgpu 的 2D／3D 渲染。它允許以 `DefaultPlugins` 建立標準客戶端，也允許用 plugin feature 與 `MinimalPlugins` 組合 headless 或測試程式；Lattice Axiom 不需要先複製這些邊界才能開始做遊戲。

## 決策

> 默认使用完整上游能力；只有当一个真实可玩的原型证明现有方案无法满足不可妥协的产品需求时，才允许自研替代。

1. Bevy 是 Lattice Axiom 的遊戲引擎與執行期基礎。開始實作時以 Bevy `0.19.x` 為基線，精確 patch、feature 與所有 Rust 相依由 `Cargo.lock` 鎖定。
2. 客戶端預設從 Bevy `DefaultPlugins` 開始，只移除經原型證明不適用的 plugin。headless、伺服器與測試使用 Bevy 支援的 plugin 組合，例如 `MinimalPlugins` 加上實際需要的標準 plugin；不另造「空 renderer」或第二套主迴圈。
3. Lattice Axiom 的執行期擴充直接使用 Bevy 的 `Plugin`／`PluginGroup`、`Component`、`Resource`、typed asset、event／message、state、schedule 與 `SystemSet`。不建立逐項映射 Bevy API 的自有 App、World、ECS、scheduler、render、asset、input 或 task facade。
4. 通用渲染走 Bevy renderer；普通擴充先使用 Mesh、Material、shader、visibility、camera 與 Bevy render extraction。只有內建 API 和正常的 Bevy render extension point 都無法達成已證明需求時，才考慮更深層替換。
5. Lattice Axiom 自己擁有的是產品語義：權威區塊資料、世界生成、已物化世界持久化、玩法規則、內容識別，以及存檔或網路等長期資料契約。這些邊界可以隔離 Bevy 的 process-local handle；引擎內部程式則應善用 Bevy 型別，不為假想替換成本建立包裝層。
6. Bevy 是 Cargo 相依，不進入任何未來的內容套件圖。Bevy 升級是整個程式碼庫的受控遷移：先讀官方 migration guide、在專用分支更新、通過可玩 smoke test 與存檔回歸，再合併。
7. 選擇外部能力時依序採用：Bevy 內建能力、維護中的 Bevy 生態 plugin、小型 upstream 擴充、最小且可回饋上游的 fork，最後才是自研替代。
8. Godot 不作為第二個遊戲執行期。只有真實資產或關卡創作流程證明現有工具不足時，才把 Godot editor／importer 當工具鏈對照組做限時原型。

## 自研例外門檻

任何替換 Bevy 或成熟 upstream 的提案，必須同時附上：

- 一個可重複執行、已能實際遊玩的原型；
- 一項可由玩家結果或產品約束驗收的「不可妥協需求」；
- 對 Bevy 內建能力、官方擴充點與活躍生態方案的實作記錄；
- profiler、相容性測試或平台測試等可重現證據，而不是預期中的限制；
- 為何設定、組合、貢獻上游或最小 fork 仍不足的說明；
- 最小替換範圍、維護 owner、升級與退出策略。

缺少任何一項，提案不得進入實作路線圖。

## 結果

- 第一個里程碑從「建立引擎骨架」改成「建立可玩的世界縱切」。
- ECS、排程、渲染、資產、輸入、時間、工作池、視窗與診斷直接跟隨 Bevy 的模型與工具。
- 專案承擔 Bevy 快速演進與尚未提供完整視覺編輯器的成本；以鎖版、升級 gate、可玩 smoke test 和外部創作工具降低風險。
- 若未來真的需要替換某項能力，替換範圍由原型證據決定，而不是由預先建立的全引擎 facade 決定。

## 驗證

- 使用標準 Bevy app 組合完成第一個可玩 demo，沒有自有 launcher、host、scheduler 或 renderer facade。
- client 與 headless 測試共用同一套玩法 plugin 與權威世界系統。
- 升級一次 Bevy minor 後，能依官方 migration guide 完成遷移，且 demo、存檔 round-trip 與核心性能預算仍通過。
- 每項新基礎設施工作都能指出已先評估的 Bevy／upstream 能力；自研例外有完整證據包。

## 相關文件

- [開源遊戲引擎提供什麼，以及如何接入 Bevy](../research/open-source-game-engine-adoption.md)
- [Bevy 執行期架構](../architecture/game-engine-runtime.md)
- [開發策略](../foundations/development-strategy.md)
- [技術棧](../foundations/technology-stack.md)
- [Bevy 執行期整合路線](../planning/roadmap-game-engine.md)
