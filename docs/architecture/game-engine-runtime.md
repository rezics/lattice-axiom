---
title: Bevy 執行期架構
status: proposed
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0015-bevy-native-y-up-world-coordinates.md
  - ../decisions/0016-stage-content-composition-on-bevy.md
---

# Bevy 執行期架構

## 結論

Lattice Axiom 的 runtime 是一個正常的 Bevy App。Bevy 擁有程序生命週期、ECS、schedule、時間、工作池、輸入、資產、視窗、渲染與診斷；專案以 domain plugin 加入權威世界、世界生成、持久化、玩法與內容。

這份架構不定義第二套引擎 API。它只說明如何善用 Bevy，以及哪些產品資料不能與 Bevy 的 process-local 狀態混為一談。

## 最小組合

```text
client binary
└── Bevy App
    ├── DefaultPlugins
    ├── selected ecosystem plugins
    └── LatticeAxiomPluginGroup
        ├── ContentCorePlugin
        ├── WorldPlugin
        ├── WorldgenPlugin
        ├── PersistencePlugin
        ├── GameplayPlugin
        └── OfficialContentPlugin

headless test / server binary
└── Bevy App
    ├── MinimalPlugins + required standard plugins
    ├── the same domain plugins
    └── test content / test harness
```

`LatticeAxiomPluginGroup` 只負責提供一個可讀的預設組合。每個 domain plugin 仍是普通 Bevy `Plugin`，可以在整合測試中單獨加入。它不解析另一張 runtime 圖、不擁有 Bevy，也不引入平行 lifecycle。

## Plugin 責任

| Plugin | 擁有 | 不擁有 |
| --- | --- | --- |
| `ContentCorePlugin` | 穩定內容 ID、方塊／物品／實體定義 registry、衝突診斷 | 第三方分發與動態 ABI |
| `WorldPlugin` | 權威區塊工作集、區塊 revision、載入／卸載、世界 command | renderer、資料庫 handle 的公共暴露 |
| `WorldgenPlugin` | 領地、地形、洞穴、確定性 RNG 與 generation provenance | 已物化區塊的隱式重算 |
| `PersistencePlugin` | schema owner、snapshot envelope、RocksDB／memory storage、commit 狀態 | Bevy `Entity`／`Handle` 的序列化 |
| `GameplayPlugin` | 玩家動作、交互、方塊修改與玩法事件 | 平台輸入設備細節 |
| `OfficialContentPlugin` | 官方方塊、物品、群系與 placeholder 資產 | 私有核心註冊捷徑 |

渲染、資產、Transform、視窗、音訊、UI 與 diagnostics 優先由 Bevy 標準 plugin 提供；只有實際的 Lattice Axiom 行為才需要 domain plugin。

## App 建立與 profile

### Client

client 從 `DefaultPlugins` 開始。若要調整 window、asset root、image sampling 或 render feature，透過標準 plugin 設定完成。只有可玩原型證明某個預設 plugin 不需要或造成實際問題時才移除。

### Headless 與測試

headless app 不安裝 window／render plugin；它使用 `MinimalPlugins` 及測試真正需要的 asset、scene、diagnostic 或 input 能力。玩法與世界 plugin 不得要求 GPU 或 window resource 才能初始化。

單元測試可用 `App::new()`／`App::empty()` 組合更小環境；整合測試至少保留與產品相同的 FixedUpdate、world、worldgen 和 persistence plugin。

### 不建立自有 host

啟動、runner、sub-app 與 plugin lifecycle 直接使用 Bevy。除非未來有一個可重現平台需求必須使用 Bevy custom runner／`SubApp`，否則不建立 launcher-host handoff、runtime descriptor 或可熱換 engine instance。

## Schedule 與時間

玩法在 Bevy `FixedUpdate` 執行；顯示與純表現在 `Update`／`PostUpdate` 跟隨 frame。`Time<Fixed>` 的頻率由原型量測後鎖定，不能拿 render delta 直接推進權威模擬。

建議的 domain `SystemSet` 順序是：

```text
PreUpdate
└── SampleActions

FixedUpdate
├── ApplyWorldCommands
├── Gameplay
├── PhysicsIntegration
├── ResolveInteractions
├── CommitWorldRevision
├── QueueDerivedWork
└── CapturePersistence

Update / PostUpdate
├── ObserveAsyncResults
├── Presentation
└── Bevy transform / visibility / render extraction
```

只有存在資料競爭或權威順序的不變量時才增加 ordering constraint；普通獨立系統交給 Bevy parallel scheduler。物理 plugin 的實際 system set 接點以其官方 integration API 為準，不複製一套 `sync → step → collect` scheduler。

測試使用 Bevy 的手動時間推進能力：給定相同初始 snapshot 與 tick action，推進固定數量的 `FixedUpdate`，比較權威 state hash。這是可重現測試，不是宣稱所有浮點物理跨平台逐位元確定。

## 權威世界與 Bevy ECS

### 區塊不是 Entity 集合

體素區塊以緊湊 domain 資料保存，例如 palette、voxel storage、metadata、revision 與 dirty flags。不要為每個方塊建立 Bevy `Entity`。`WorldPlugin` 可用 resource 或專用 arena 管理目前載入的 chunk working set。

Bevy ECS 用於：

- 玩家、生物、掉落物、投射物與其他活躍實體；
- chunk presentation／collision 的粗粒度 owner entity；
- 跨系統需要 query、change detection 或 lifecycle 的執行期狀態；
- task handle、載入請求與短期診斷元件。

### 穩定身分

Bevy `Entity` 只在目前 World 中有效。需要跨卸載或重啟的實體由 persistence domain 配置 stable entity key；內容類型使用具名 stable content ID。兩者與 Bevy entity／component registration ID 分離。

### 權威與衍生資料

| 類別 | 例子 | 可否丟棄重建 |
| --- | --- | --- |
| 權威 | voxel、玩家修改、持久實體、排定工作、必要 RNG state | 否 |
| provenance | generator revision、設定 hash、上游規劃識別 | 否 |
| 執行工作集 | ECS entity、載入 chunk arena、task state | 可由 snapshot 恢復 |
| 衍生 | mesh、collider cache、visibility、GPU asset、navigation cache | 是 |

renderer 或物理失敗不得反向修改權威區塊。衍生結果只在其來源 revision 仍匹配時套用。

## 非同步工作

使用 Bevy task pools 執行 worldgen、meshing、collision build、compression 與 I/O 等工作；不建立另一個全域 executor。每個 job 至少攜帶：

- world／dimension ID；
- chunk `(x, y, z)`；
- world epoch 與 chunk revision；
- job kind 和必要輸入 fingerprint；
- cancellation／supersession 所需 token。

背景 task 只接收 immutable input 並產生 immutable result。結果回到主 Bevy World 後，在系統中重新驗證 epoch／revision；callback 不直接持有可變 World，也不從背景執行 structural ECS mutation。

queue、並行數、in-flight bytes 和每 frame 套用成本都要有上限。視距快速移動時，過期工作應被取消或在完成時廉價丟棄，不能拖垮新區塊。

## Input 與 gameplay action

平台事件由 Bevy input plugin 收集。Gameplay 不查詢鍵盤 scan code，而消費具名 action，例如 `Move`、`Look`、`Jump`、`BreakBlock`、`PlaceBlock`。先比較 Bevy 原生 input 與 Leafwing Input Manager；若原生資源已足夠，不增加依賴。

只有當 replay、rollback 或網路成為真實需求時，才把每 tick action frame 定義為穩定協定。demo 只需能在測試中注入 action，不能先把輸入系統設計成未驗證的網路協定。

## Physics

第一個 spike 優先採用 Avian 等 Bevy-native 物理 plugin，驗證 character movement、重力、voxel collider、raycast 和固定 tick integration。普通 gameplay 直接使用其 Bevy component／query API；持久化只保存專案語義 DTO，不序列化 solver 內部 handle 或 broadphase。

自製完整物理解算器不在路線內。若體素碰撞有明確性能缺口，先把缺口限制在 chunk collider 生成或查詢層，再走上游例外門檻。

## Assets 與 rendering

`AssetServer` 管理載入、相依與 handle；glTF、Image、Mesh、Material、shader 和 scene 優先走 Bevy 標準 loader。自訂 domain asset 透過 `AssetLoader` 接入，而不是建立平行 asset service。

普通畫面由 main world component 和 Bevy renderer 產生。需要客製 render feature 時，先使用 Material／shader，再使用 Bevy `RenderApp` 與 extraction；不在前面另造 `RenderWorld` facade。詳見[渲染架構](rendering.md)。

## Persistence barrier

`PersistencePlugin` 在 `CapturePersistence` system set 從一致的權威 revision 建立 immutable snapshot envelope，再交給 I/O task。完成結果回主 World 後，只能確認同一或較新的已提交 revision；不得把較舊完成訊息誤當成目前 chunk 已乾淨。

存檔只包含 schema owner 的穩定資料，不包含 Bevy `Entity`、`Handle`、schedule label、reflection index 或第三方物理 handle。完整協定見[世界持久化](world-persistence.md)。

## Shutdown

正常離開先進入明確的 shutting-down app state：停止接受新世界 command、為 dirty chunk 建立最後 commit、等待有界 I/O drain、寫入可驗證 checkpoint／metadata，最後才送出 `AppExit`。若超過 deadline，向使用者報告仍未耐久的 revision；不能假裝 Drop 順序等於保存協定。

程序崩潰仍以 RocksDB WAL／原子 batch 與既有已確認 commit 為恢復基線。正常 shutdown 不能取代 crash test。

## Bevy 升級政策

1. 日常開發不允許相依自動漂移；精確結果由 `Cargo.lock` 固定。
2. 升級先讀 Bevy 官方 migration guide 和生態 plugin compatibility table。
3. 在獨立變更中升級，不夾帶玩法重構。
4. 驗證 client、headless、第一個可玩流程、存檔 round-trip、資產 fixture 與性能預算。
5. 若生態 plugin 暫未跟上，先留在舊 Bevy 系列或提交 upstream 修正，不立即自製替代。

## 禁止的鏡像抽象

沒有證據例外時，不建立：

- 自有 App／World／Entity／Component facade；
- 自有 plugin lifecycle、schedule graph 或 fixed-time runtime；
- 自有 renderer facade、null renderer 或 GPU resource API；
- 自有 asset handle／dependency graph；
- 自有 platform input／window layer；
- 自有通用 task runtime；
- 將 Bevy 再包成可服務任意遊戲的通用引擎。

## 原型驗收

- client 只用 Bevy 標準啟動路徑進入可玩世界。
- headless app 不建立 window／GPU，仍能推進相同 FixedUpdate gameplay 並保存世界。
- 打亂背景 job 完成順序不改變最終權威 chunk revision。
- 過期 mesh／collision／save 結果不覆蓋新狀態。
- 官方與測試內容 plugin 可獨立替換，不修改 runtime 私有啟動碼。
- profiler 能看見 fixed tick debt、chunk queue、task latency、entity／chunk 數與 persistence latency。

## 相關文件

- [決策 0014：採用 Bevy 並以上游能力為預設](../decisions/0014-adopt-bevy-upstream-first.md)
- [技術棧](../foundations/technology-stack.md)
- [渲染架構](rendering.md)
- [世界持久化](world-persistence.md)
- [模組與內容組合](module-composition.md)
- [Bevy 執行期整合路線](../planning/roadmap-game-engine.md)
