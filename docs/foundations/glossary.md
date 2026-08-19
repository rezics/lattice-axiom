---
title: 詞彙表
status: active
type: reference
updated: 2026-08-19
---

# 詞彙表

| 詞彙 | 本專案中的意思 |
| --- | --- |
| Bevy runtime | 由 Bevy App、ECS、schedule、asset、render 與標準 plugin 組成的遊戲執行期；不是 Lattice Axiom 自研引擎。 |
| `LatticeAxiomPluginGroup` | 組合遊戲 domain plugin 的專案入口；薄組合層，不複製 Bevy lifecycle。 |
| domain plugin | 擁有一項產品語義的 Bevy `Plugin`，例如世界、世界生成、持久化或官方內容。 |
| content plugin | 只經公開註冊路徑加入方塊、物品、實體或生成內容的 Bevy plugin。官方與測試內容遵守同一規則。 |
| 權威世界（authoritative world） | 能決定玩法且必須正確恢復的區塊、實體與續行狀態；不是 render world 或可重建快取。 |
| 已物化區塊 | 已完成第一次生成並保存完整快照的區塊。之後以快照為權威，不因生成器更新而隱式重算。 |
| active working set | 目前載入 RAM／Bevy ECS、供模擬與呈現的世界子集；可由持久化資料重建。 |
| chunk revision | 區塊權威資料每次變更後遞增的 revision，用來拒絕過期 mesh、collision 或 I/O job 結果。 |
| generation provenance | 描述某項世界資料由哪個生成器 revision、設定與上游規劃產物產生的持久化記錄。 |
| schema owner | 唯一負責一類持久化資料格式、版本和遷移的 domain component／plugin。 |
| 穩定內容 ID | 跨重啟、存檔與未來網路使用的具名識別；不得以 Bevy `Entity`、`Handle` 或 Rust `TypeId` 代替。 |
| process-local ID | 只在一次執行有效的 Bevy Entity、asset handle、GPU handle 等識別，不可持久化。 |
| presentation | 從權威狀態與玩法事件衍生的相機、mesh、材質、粒子、動畫、音訊和 UI；可丟棄重建。 |
| fixed tick | 在 Bevy `FixedUpdate` 中以 `Time<Fixed>` 推進的玩法模擬步長；不等同渲染 frame。 |
| headless app | 不安裝 window／render plugin 的 Bevy App 組合，用於伺服器、模擬測試和持久化測試；不是自製 null renderer。 |
| upstream deviation | 對 Bevy 或成熟依賴進行替換、fork 或平行實作；必須通過決策 0014 的可玩原型證據門檻。 |
| Y-up | Bevy 原生右手座標約定：`+X` 右、`+Y` 上、forward `-Z`，水平面是 `x-z`。 |
| toolchain comparison | 對作者工具的限時、可驗收比較；Godot 可作這類對照，但不因此成為遊戲 runtime。 |

## 相關文件

- [專案願景](project-vision.md)
- [技術棧](technology-stack.md)
- [Bevy 執行期架構](../architecture/game-engine-runtime.md)
- [版本與相容性](../architecture/versioning-and-compatibility.md)
