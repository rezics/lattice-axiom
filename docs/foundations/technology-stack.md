---
title: Bevy-first 技術棧與採用邊界
status: active
type: reference
updated: 2026-08-19
---

# Bevy-first 技術棧與採用邊界

本頁是目前實作技術的單一入口。`accepted` 是已採納基線；`spike candidate` 只允許進入短期可玩原型，必須以相容性、維護狀態、授權和量測結果決定是否鎖定。

## 採用原則

- Bevy 是引擎，不在專案內再建立一層對等引擎 API。
- Rust／Bevy 型別可以在執行期內部自由使用；只有存檔、網路與穩定內容契約隔離 process-local 型別。
- 先使用 upstream 預設，再組合、擴充、最小 fork；自研替代受[決策 0014](../decisions/0014-adopt-bevy-upstream-first.md)的證據門檻約束。
- crate 精確版本、feature 與來源在開始實作時寫入 Cargo manifest／`Cargo.lock`；本頁只記錄選型與理由。
- 一個外部依賴若只服務單一領域，就在該領域做薄整合，不建立全專案 provider facade。

## 已接受基線

| 能力 | 選擇 | 使用方式 | 狀態 |
| --- | --- | --- | --- |
| 語言與建置 | Rust stable、Cargo workspace、`Cargo.lock` | 遊戲與內容 plugin 靜態組合；CI 鎖定 toolchain | accepted |
| 遊戲引擎 | [Bevy](https://github.com/bevyengine/bevy) `0.19.x` | client 使用 `DefaultPlugins`；headless／測試使用 `MinimalPlugins` 加必要標準 plugin | accepted |
| ECS 與 App | Bevy ECS、App、Plugin、States、Schedules、SystemSet | 直接使用，不建立 `latticeaxiom-ecs` 或自研 lifecycle／scheduler | accepted |
| 時間與工作池 | Bevy Time、`FixedUpdate`、TaskPool | 固定玩法 tick；背景 job 回主世界時驗證 revision／epoch | accepted |
| 渲染與視窗 | Bevy Render／PBR／UI／winit integration | wgpu、winit 是 Bevy 實作細節；專案不直接建立 render facade | accepted |
| 資產 | Bevy `AssetServer`、`AssetLoader`、glTF 與 typed asset | 依賴追蹤、載入狀態與熱重載沿 Bevy；權威資料不保存 `Handle` | accepted |
| 座標 | Bevy 原生右手 Y-up | `+X` 右、`+Y` 上、forward `-Z`；公尺／弧度 | accepted |
| 世界儲存 | RocksDB + memory test implementation | 完整已物化區塊快照與原子 write batch；由 domain storage 邊界隔離 | accepted |
| 序列化 | serde 生態 | schema owner 定義穩定 DTO；實際 wire／disk encoding 由原型鎖定 | accepted |
| 觀測 | Bevy diagnostics + `tracing` 生態 | 先用標準 diagnostics、log 與 profiler 整合 | accepted |

Bevy `0.19.x` 是本次文件重構的相容系列，不代表允許自動漂移。實作提交必須固定精確 patch；任何 minor 升級都視為顯式遷移工作。

## 首個可玩原型候選

| 能力 | 首選候選 | 原型要回答的問題 | 回退順序 | 狀態 |
| --- | --- | --- | --- | --- |
| 體素世界 | [`bevy_voxel_world`](https://github.com/splashdust/bevy_voxel_world) `0.17`（Bevy 0.19） | chunk streaming、編輯、raycast、mesh job 與材質能否支撐 demo；其硬編碼假設能否被設定或薄擴充吸收 | plugin 擴充／貢獻上游 → 直接採用 `block-mesh` 等小型 mesher | spike candidate |
| 物理 | [Avian](https://github.com/avianphysics/avian) `0.7`（Bevy 0.19） | 固定 tick、character movement、voxel collider、raycast 與 local-origin 能否達到預算 | Bevy 生態另一維護方案 → 最小 domain-specific collision | spike candidate |
| 動作輸入 | [Leafwing Input Manager](https://github.com/Leafwing-Studios/leafwing-input-manager) `0.21`（Bevy 0.19） | action mapping、重綁、測試注入是否比 Bevy 原生輸入更省工作 | Bevy 原生 input resources | spike candidate |
| 資產載入狀態 | [`bevy_asset_loader`](https://github.com/NiklasEi/bevy_asset_loader) `0.27`（Bevy 0.19） | 是否真的需要 state-aware collection；標準 `AssetServer` 是否已足夠 | Bevy `AssetServer` 直接管理 | optional spike |

候選不是承諾。第一個 spike 應盡量保持 upstream 原貌，先確認能力，再決定是否保留依賴。

## 明確採用的上游邊界

- 程序入口是正常 Bevy `App`。
- ECS 使用 Bevy ECS。
- 排程與固定時間使用 Bevy schedules、`FixedUpdate` 與 `Time<Fixed>`。
- 渲染使用 Bevy renderer；headless profile 不安裝 render plugin。
- 資產使用 Bevy `AssetServer` 與 typed assets。
- 非同步工作使用 Bevy task pools。
- 內容組合使用 Cargo、Bevy plugins 與 typed assets；後續分發需求另立研究。
- 所有空間資料直接使用 Bevy 原生 Y-up。

## Bevy 型別邊界

可以直接使用 Bevy 型別的區域：

- plugin 之間的執行期整合；
- ECS component、resource、system parameter 與 event／message；
- Transform、Mesh、Material、Image、Animation、Audio 與 asset handle；
- render extraction、diagnostics 與開發工具。

必須轉成專案穩定 DTO／ID 的邊界：

- RocksDB record 與備份；
- 未來網路協定；
- 生成 provenance；
- 跨重啟內容引用；
- 需要獨立遷移承諾的公開資料格式。

Bevy `Entity`、`Handle`、Rust `TypeId`、反射註冊順序與 GPU resource ID 都不得進入這些長期邊界。

## Godot 的位置

Godot 只作未來工具鏈對照組，不作 runtime、第二渲染後端或內容格式真相來源。若 Blender + glTF + Bevy 工具不足，依[Godot 工具鏈對照](../research/godot-toolchain-comparison.md)做限時 spike，再決定是否需要場景／資產橋接。

## 相關文件

- [決策 0014：採用 Bevy 並以上游能力為預設](../decisions/0014-adopt-bevy-upstream-first.md)
- [決策 0015：Bevy 原生 Y-up](../decisions/0015-bevy-native-y-up-world-coordinates.md)
- [決策 0016：以 Bevy 插件起步](../decisions/0016-stage-content-composition-on-bevy.md)
- [Bevy 執行期架構](../architecture/game-engine-runtime.md)
- [渲染與物理候選](../research/renderer-physics-landscape.md)
