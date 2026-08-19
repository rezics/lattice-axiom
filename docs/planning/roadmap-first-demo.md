---
title: 第一個 Bevy 可玩 demo 路線圖
status: active
type: roadmap
updated: 2026-08-19
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0016-stage-content-composition-on-bevy.md
---

# 第一個 Bevy 可玩 demo 路線圖

## Demo 定義

> 玩家走進程序生成的體素世界，挖掉並放回一個方塊；退出再進入後，世界仍保留變更。官方內容經公開 Bevy plugin 路徑加入。

這是一個玩家可操作的 vertical slice，不是引擎展示、package manager 展示或純技術 benchmark。

## 固定基線

- Bevy `0.19.x`，精確 patch 與 feature 由 `Cargo.lock` 固定；
- client 使用 `DefaultPlugins`；
- headless／整合測試使用 `MinimalPlugins` 加必要標準 plugin；
- Bevy 原生右手 Y-up；
- voxel、physics、input、assets 和 diagnostics 先用 Bevy／生態 upstream；
- RocksDB 保存完整已物化 chunk snapshot；
- 官方與 test content 使用普通 Bevy plugin；
- 不含自研 engine facade、動態 ABI、套件內核或一般 resolver。

## 玩家循環

1. 建立或載入一個 world。
2. 在 Y-up 體素地形出生。
3. 使用滑鼠／鍵盤移動、觀看和跳躍。
4. raycast 指向方塊並挖掉它。
5. 選擇可放置方塊並放回。
6. 看見 mesh／collision 在合理延遲內更新。
7. 正常退出或被測試強制終止。
8. 重啟後看見已確認保存的修改。

任何里程碑若不能縮短到這個循環的距離，就不進 demo 範圍。

## D0：Bevy smoke

### 交付

- client／headless 兩個 App profile；
- placeholder camera、light、ground／cube fixture；
- manual fixed-time test；
- diagnostics、log 和 CI；
- Y-up glTF／voxel orientation tests。

### 完成

- client 可操作相機並退出；
- headless 不建立 GPU，可推進固定 tick；
- 無自有 lifecycle、renderer 或 ECS layer。

## D1：Upstream voxel playground

### 交付

- `bevy_voxel_world` 優先 spike；
- Avian 優先 physics spike；
- Bevy input 或 Leafwing 二選一；
- walk／look／jump、raycast、break／place；
- chunk／mesh／collider diagnostics。

### 完成

- 另一位測試者可取得 build 並完成挖掘／放置；
- chunk 邊界行為正確；
- edit-to-visible latency、frame time、memory 與 task queue 有測量；
- 對每個 upstream 缺口有可重現記錄。

D1 之前不寫自有 voxel renderer、mesher 或 physics。

## D2：權威 chunk 與保存

### 交付

- 根據 D1 adoption report 選擇直接採用或最小 adapter；
- stable block ID、chunk revision、dirty state；
- `MemoryWorldStorage`／RocksDB；
- snapshot envelope、atomic batch、durability；
- chunk unload／reload；
- normal shutdown 和 crash test。

### 完成

- 修改跨重啟保留；
- 舊 async result 不覆蓋新 revision；
- 已物化 chunk 不重新生成；
- checkpoint 可在獨立目錄恢復；
- 存檔沒有 Bevy Entity、Handle 或 solver handle。

## D3：公開內容路徑

### 交付

- validated `ContentCatalog`；
- `OfficialContentPlugin`：最小 stone／dirt／placeable item；
- `TestContentPlugin`：另一 namespace 的 marker block；
- stable ID duplicate／missing diagnostics；
- headless content composition test。

### 完成

- 官方內容只用公共 registration；
- plugin 加入順序不改變內容 ID；
- test content 可獨立啟動；
- 缺失權威 ID 不 silent remap。

## D4：最小程序世界

### 交付

- world seed 與 `WorldgenConfig`；
- 兩個 placeholder biome 或 terrain style；
- deterministic terrain height（`y`）與最小洞穴／空洞；
- generator revision、config hash 和 chunk provenance；
- 新區塊生成後立即 snapshot。

### 完成

- 相同 seed／`(x, z)`／config 在 headless 測試產生相同權威 chunk；
- 不同 task／chunk 順序不改變結果；
- generator 更新不改變舊 chunk；
- 玩家循環仍完整。

不要求第一版就實作完整領地、水文、地質或洞穴拓撲架構。

## D5：交付與回歸

### 自動驗收

- client 可玩 smoke；
- headless fixed-tick gameplay；
- save round-trip／old-schema fixture；
- RocksDB crash／atomicity；
- chunk revision race；
- Y-up six-face mesh／raycast／collider；
- official／test content order randomization；
- 10 分鐘 traversal memory／queue bound；
- checkpoint restore。

### 人工驗收

- 新測試者不看原始碼能在 5 分鐘內完成挖掘、放置、退出與重載；
- 畫面清楚區分目標方塊、選中方塊和保存狀態；
- crash／missing content／存檔失敗有可理解診斷；
- build 與操作說明可重複。

## Demo 明確不做

- 動態模組載入、registry、mod marketplace、WASM sandbox；
- 多人、rollback 或 lockstep；
- 自研 renderer／physics／ECS／scheduler／asset system；
- 完整 terrain graph、文明、水文或世界編輯器；
- 正式角色美術、複雜動畫、柔體與進階 PBR；
- 以 Godot 作第二 runtime。

## Demo 完成定義

以下條件全部成立才算完成：

1. 一個真實玩家可完成完整循環。
2. client 與 headless 使用相同權威 gameplay／world／persistence plugin。
3. 保存與 crash 測試證明已確認 revision 不遺失、不半寫。
4. 官方內容證明公共 Bevy plugin 路徑。
5. upstream adoption report 沒有以猜測支持的自研替代。
6. 性能、記憶體和 queue 有基線數據，可供下一階段比較。

## 相關文件

- [Bevy 執行期整合路線](roadmap-game-engine.md)
- [Bevy 執行期架構](../architecture/game-engine-runtime.md)
- [世界持久化](../architecture/world-persistence.md)
- [模組與內容組合](../architecture/module-composition.md)
- [待決問題](open-questions.md)
