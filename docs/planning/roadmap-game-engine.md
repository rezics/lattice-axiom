---
title: Bevy 執行期整合路線
status: active
type: roadmap
updated: 2026-08-19
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0015-bevy-native-y-up-world-coordinates.md
  - ../decisions/0016-stage-content-composition-on-bevy.md
---

# Bevy 執行期整合路線

## 目標

這不是遊戲引擎實作計畫，而是把 Lattice Axiom 做成一個可測、可玩的 Bevy 遊戲的依賴順序。每一階段都先採用 upstream 完整能力；任何自研替代都必須離開本路線，先通過[決策 0014](../decisions/0014-adopt-bevy-upstream-first.md)的證據門檻。

最重要的流程是：

```text
Bevy 能跑
  → upstream 體素 spike 可玩
  → 世界修改可保存
  → 官方／測試內容共用公開路徑
  → Lattice Axiom 世界生成接入
  → 依 profiler 做最小優化
```

## B0：鎖定 Bevy 並建立兩種 App profile

### 交付

- Rust workspace、toolchain 與 CI；
- Bevy `0.19.x` 精確 patch／feature 寫入 manifest 和 `Cargo.lock`；
- client：`DefaultPlugins` + 空 `LatticeAxiomPluginGroup`；
- headless test：`MinimalPlugins` + 必要標準 plugin；
- Bevy diagnostics／tracing 基線；
- Y-up 相機、光源、glTF orientation fixture。

### 出場條件

- client 可建立 window、顯示 Bevy 場景並乾淨退出；
- headless app 不建立 window／GPU，可手動推進固定 tick；
- 同一個空 gameplay plugin 能加入兩種 profile；
- 沒有自有 App、host、scheduler、renderer 或 asset facade；
- dependency／license 清單可由 Cargo metadata 重建。

## B1：完整 upstream 體素可玩 spike

### 交付

- 優先以 `bevy_voxel_world` 原貌建立可編輯 voxel world；
- 以 Avian 候選建立 player collision、重力與 raycast；
- 比較 Bevy 原生 input 和 Leafwing Input Manager，只保留有實際價值的一種；
- placeholder material、camera、crosshair、diagnostics；
- 玩家可走、看、挖、放。

### Spike 規則

- 先使用 upstream 預設，不先包裝；
- 對每個改動記錄「設定即可」「需要 public extension」「需要 upstream issue／patch」「確實阻塞」；
- profiler 收集 frame time、chunk mesh latency、memory、collider update 和 edit-to-visible latency；
- 此階段不接 RocksDB、不重寫 chunk storage、不實作自有 mesher。

### 出場條件

- 有一個可由其他人實際操作的 build；
- client 連續遊玩 10 分鐘無 crash／無界 queue；
- 挖掘與放置在 chunk 邊界正確；
- 六個 Y-up 面的 raycast、normal 和 collision 一致；
- 形成一份 adoption report，逐項決定 voxel plugin 採用、擴充、貢獻或最小替換範圍。

沒有可玩 build 和量測，不允許宣告 upstream 不適用。

## B2：固定權威世界邊界

這一階段根據 B1 證據選擇，不預設一定要重做 chunk 系統：

1. 若 voxel plugin 可直接承擔權威資料，薄接 stable content ID、revision 與 snapshot。
2. 若它適合作 presentation，保留其 rendering／streaming，建立最小 authoritative chunk adapter。
3. 若只有局部缺口，向 upstream 貢獻或只替換該局部。

### 交付

- stable block ID 與 runtime／snapshot palette；
- chunk coordinate／revision／dirty state；
- async result 的 epoch／revision validation；
- active／resident／persistence radius 與有界 queue；
- headless world edit test。

### 出場條件

- 打亂 mesh／collision job 完成順序不覆蓋新 revision；
- 一個 voxel 不對應一個 Bevy Entity；
- 權威 chunk 不保存 Mesh、Handle 或物理 solver handle；
- B1 的可玩操作保持成立，性能没有越过已记录预算。

## B3：RocksDB 持久化縱切

### 交付

- `WorldStorage` domain contract、`MemoryWorldStorage` 與 RocksDB implementation；
- snapshot envelope、schema owner、stable palette、generation provenance；
- Bevy `CapturePersistence`／`ObserveCommitResult` system sets；
- atomic batch、durability level、shutdown drain；
- checkpoint／restore 與 crash harness。

### 出場條件

- 玩家挖掉／放回方塊，重啟後結果保留；
- chunk 卸載再載入不隱式重生；
- I/O completion 亂序不清除較新的 dirty revision；
- 強制終止不產生半個 chunk／orphan index；
- headless 與 client 讀取同一存檔，權威 hash 相同；
- 存檔不含 Bevy process-local ID。

## B4：公開內容路徑

### 交付

- `ContentCorePlugin` 與 validated catalog；
- `OfficialContentPlugin`；
- `TestContentPlugin`，使用不同 namespace；
- stable ID conflict、missing-content 和 migration diagnostics；
- typed data asset 最小 fixture。

### 出場條件

- 兩個 content plugin 都不修改私有啟動碼；
- 調換加入順序不改變 catalog；
- duplicate stable ID 一律失敗，不 last-writer-wins；
- 只用 test content 可啟動 headless world；
- 保存／重載可重新解析兩個 namespace。

這個 gate 通過前不討論動態 ABI、package registry 或內容市場。

## B5：Lattice Axiom 世界生成縱切

### 交付

- `WorldgenPlugin`；
- 兩個 placeholder surface biome；
- deterministic Y-up terrain；
- 最小洞穴形態或既有 upstream terrain hook；
- generator revision／config fingerprint；
- 新 chunk 的 snapshot-first materialization。

### 出場條件

- 相同 seed、`(x, z)` 規劃座標和 config 產生相同 chunk；
- chunk generation／task 完成順序不改變結果；
- 更換 generator 後舊 snapshot 不變，新 chunk 使用新 provenance；
- 官方與 test biome 使用同一 registration path；
- 玩家仍能完成挖掘、放置和重啟驗收。

完整領地／水文／洞穴圖譜是後續 worldgen roadmap，不阻塞首個 demo。

## B6：Production hardening

只修正在 B0–B5 被量測到的問題：

- load radius、task／I/O backpressure；
- checkpoint／backup、corruption diagnostics；
- asset／content missing recovery；
- long-session memory／resource leak；
- Bevy upgrade rehearsal；
- supported platform build／input；
- profiling 和可重現 benchmark scene。

不以 hardening 名義加入未被使用的通用抽象。

## 上游偏離 gate

任何階段若遇到阻塞，先建立一頁證據記錄：

| 欄位 | 必填內容 |
| --- | --- |
| playable reproduction | 可取得的 build、操作步驟與 seed／world |
| non-negotiable requirement | 玩家／產品層驗收，不是技術偏好 |
| upstream attempts | 設定、官方 extension point、生態 plugin、issue／PR |
| measurements | profiler、平台、相容性或故障證據 |
| smallest deviation | 哪個函式／資料流必須替換，哪些仍由 Bevy 提供 |
| ownership | 維護者、升級、測試、回饋上游與退出策略 |

證據審查通過後另建 ADR；不能直接把替換工作加回本路線。

## 相關文件

- [Bevy 執行期架構](../architecture/game-engine-runtime.md)
- [第一個可玩 demo 路線圖](roadmap-first-demo.md)
- [技術棧](../foundations/technology-stack.md)
- [待決問題](open-questions.md)
