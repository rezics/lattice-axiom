---
title: 世界持久化與 RocksDB World Store
status: proposed
type: explanation
updated: 2026-08-19
decision:
  - ../decisions/0003-no-global-version-package-scoped-compatibility.md
  - ../decisions/0009-rocksdb-authoritative-world-snapshots.md
  - ../decisions/0011-right-handed-z-up-world-coordinates.md
---

# 世界持久化與 RocksDB World Store

> 區塊快照是已存在空間世界的權威，且 demo 直接使用 RocksDB，已由[決策 0009](../decisions/0009-rocksdb-authoritative-world-snapshots.md)採納。本頁整理資料分層、儲存契約與恢復流程；鍵編碼、column family 與調校值仍需原型量測。

## 四層責任

```text
                   執行中的世界
                    RAM / ECS
                         │ load / commit
                         ▼
             RocksDB authoritative World Store
        chunks / spatial entities / continuation / receipts
                         │ checkpoint / restore
                         ▼
               Object Storage（平台期）

PostgreSQL（平台期）
accounts / ownership / economy / searchable persistent objects
```

| 層 | 權威範圍 | 不應承擔 |
| --- | --- | --- |
| RAM／ECS | 目前載入區塊的模擬狀態 | 跨重啟耐久性 |
| RocksDB World Store | 方塊、生成基線、空間實體、持久元件與模擬續行 | 跨世界社會查詢與多節點共同寫入 |
| PostgreSQL | 帳號、權限、經濟、所有權與需要關聯查詢的資料 | 每方塊或一般空間實體熱路徑 |
| 物件儲存 | checkpoint、備份、封存與分發 | 活躍世界逐 tick 寫入 |

SQLite 可在日後承擔客戶端本地索引或 UI 中繼資料，但不作為 demo World Store，也不是世界權威。

## 權威快照原則

請求區塊時只存在兩條路：

```text
requested chunk
       │
       ├─ snapshot exists ──→ load snapshot
       │
       └─ missing ──────────→ current generation lock
                                  ↓ generate
                                  ↓ validate boundaries
                                  ↓ commit snapshot + provenance
                                  ↓ return materialized chunk
```

「同一種子可以重算」只能用於尚未物化的空間。對已保存區塊：

- 更新生成套件不會隱式重寫舊區塊；
- 移除舊生成器不影響讀取已物化方塊與狀態；
- 新探索區域可以使用新 generation lock；
- 新舊區域靠明確的邊界契約與 transition 規則銜接；
- 只有顯式 regenerate 或 migration 能更換已存在快照。

## 恢復狀態模型

區塊提交保存目前狀態與尚未完成的必要工作，而不是每次變更的永久事件流：

```text
MaterializedBaseline
  voxel palette, biome, generated structures

PersistentState
  player edits, containers, machines, spatial entities,
  owner-defined persistent components

SimulationContinuation
  scheduled ticks, fluid frontier, pending jobs,
  deterministic continuation state

DerivedEphemeral
  meshes, pathfinding cache, broadphase, GPU resources
  → rebuild after load; never authoritative
```

流體造成的最終方塊狀態需要保存；若卸載時仍有未完成傳播，還要保存足以繼續模擬的邊界或排定工作，不必保存「每一步流動歷史」。

## `WorldStorage` 契約

核心依賴語義操作，不依賴 RocksDB API。概念介面如下：

```rust
trait WorldStorage {
    fn load_chunk(&self, key: ChunkKey) -> Result<Option<StoredChunk>>;
    fn commit_chunk(&self, commit: ChunkCommit) -> Result<CommitReceipt>;
    fn delete_or_replace_range(&self, tx: RegenerationTransaction) -> Result<()>;
    fn scan_receipts(&self, query: ReceiptQuery) -> Result<Vec<ArtifactReceipt>>;
    fn create_checkpoint(&self, target: CheckpointTarget) -> Result<CheckpointId>;
}
```

實際同步／非同步形式由 I/O 原型決定，但契約必須表達：

- 一次提交涵蓋哪些資料類別；
- 預期的耐久等級與確認時點；
- schema、所有者與 generation provenance；
- 冪等重試、取消及錯誤恢復語義；
- 讀取是否來自一致 view；
- checkpoint 完成後能否獨立還原。

`MemoryWorldStorage` 與 RocksDB 實現共用同一契約測試；前者不能以較弱原子性掩蓋生產路徑錯誤。

## RocksDB 邏輯資料模型

第一版先定義鍵空間，不預先承諾每個鍵空間各有 column family：

| 鍵空間 | 鍵的主要部分 | 值 |
| --- | --- | --- |
| World metadata | world | active generation lock、schema、時間與政策 |
| Chunk snapshot | world / dimension / chunk | 壓縮後調色盤、方塊、群系與基線 |
| Spatial entities | world / dimension / chunk / entity | 元件載荷、owner schema、生命週期 |
| Continuation | world / dimension / chunk / work kind | 排定工作、流體邊界、必要 RNG 狀態 |
| Artifact receipts | world / domain / artifact | 生產者、組態、輸入與內容雜湊 |
| Regeneration journal | world / transaction | 範圍、保留政策、階段與恢復資訊 |

座標編碼必須保留確定排序，讓維度、region 或區塊前綴掃描有界；不得把不穩定的 Rust 記憶體表示直接當永久鍵或值。所有值帶 envelope schema，內容模組的資料再帶 owner schema。

一次 `ChunkCommit` 使用原子 write batch 同步更新快照、空間實體索引、續行狀態與產物記錄。大量區塊生成可以批次化，但不能讓一個過大的 batch 造成無界停頓；大小與 flush 策略以壓力測試決定。

## 儲存與執行生命週期

```text
load snapshot + spatial entities + continuation
                    ↓
          validate envelopes / schemas
                    ↓
              instantiate ECS
                    ↓
               simulation ticks
                    ↓
          build immutable ChunkCommit
                    ↓
          write batch + durability policy
                    ↓
       only then acknowledge durable save
```

網格、GPU buffer、物理 broadphase 與尋路快取在載入後重建。內容模組卸載或升級時，未知 owner 資料要保留、遷移或明確拒絕載入，不能靜默丟棄。

## 耐久、壓實與備份

RocksDB 提供 WAL、原子 write batch、排序鍵與 point-in-time read snapshot，但專案仍需定義：

- 自動存檔可接受的資料損失視窗；
- 哪些管理操作要求同步耐久；
- 壓實是否會與區塊串流競爭延遲；
- 磁碟空間不足與 background error 如何使世界安全降級；
- checkpoint／backup 的保留、驗證與還原演練；
- schema 升級前後的回復政策。

RocksDB read snapshot 是程序內一致 view，重啟後不保留。可攜備份必須使用 checkpoint 或 backup 流程，複製到獨立目標後實際開啟驗證；平台期再把完成的備份送入物件儲存。

## Persistent World Object 與 PostgreSQL

demo 先把所有隨區塊載入的持久物件放在 RocksDB。平台期只有當物件需要下列能力時，才考慮同步到 PostgreSQL：

- 跨世界或跨區塊查詢；
- 所有權、交易、稽核與約束；
- 依 package/schema 找出全部待遷移物件；
- 平台服務在世界未載入時仍需查詢。

若採用 PostgreSQL，資料以 `PersistentWorldObject` 與有 owner schema 的 component 模型組織，並以 `(world_id, dimension_id, chunk_x, chunk_y, chunk_z)` 作主要載入索引，一次取得整個區塊的物件，避免 N+1 查詢。三軸含義、順序與負座標分區遵守[決策 0011](../decisions/0011-right-handed-z-up-world-coordinates.md)，其中 `chunk_z` 是垂直索引。

跨 RocksDB 與 PostgreSQL 的 regenerate 不是單一 write batch。平台期必須採 staged replacement、journal/outbox、冪等提交與補償恢復等明確協定；在此協定完成前，不把物件拆到兩個權威儲存。

## Regenerate

`/regenerate chunk|region|selection` 至少指定：

- 目標範圍與 generation lock；
- 是否保留玩家擁有、系統持久或 package 擁有的物件；
- 與現有鄰接區塊的邊界輸入；
- 預覽、備份點與確認；
- 失敗後回復或繼續的交易識別。

流程是先在隔離狀態產生並驗證替代資料，再以原子切換取代權威快照；不能先刪舊資料再嘗試生成。跨 generation epoch 的接縫由世界生成邊界契約處理，而不是由持久化層平均方塊。

## 量測與故障測試

- 典型與最壞區塊的壓縮大小、讀寫 P50/P95/P99；
- 初次生成持續寫入時的 write amplification、compaction stall 與磁碟峰值；
- 隨機 kill、磁碟滿、checksum 錯誤與 schema 不相容；
- 同一世界單寫者保證與錯誤雙重掛載；
- checkpoint 時繼續遊玩的延遲，以及從 checkpoint 實際還原；
- 生成器更新後舊快照不變、新區域 provenance 正確、邊界可診斷。

## 外部依據

- [RocksDB Overview：排序鍵、WAL、column family、write batch 與 snapshot](https://github.com/facebook/rocksdb/wiki/RocksDB-Overview)
- [RocksDB Basic Operations：原子批次與一致讀取](https://github.com/facebook/rocksdb/wiki/Basic-Operations)
- [RocksDB Checkpoints](https://github.com/facebook/rocksdb/wiki/Checkpoints)

## 相關文件

- [版本、相依性與相容性架構](versioning-and-compatibility.md)
- [可組合世界生成架構](world-generation.md)
- [決策 0009：RocksDB 權威世界快照](../decisions/0009-rocksdb-authoritative-world-snapshots.md)
- [第一個 demo 路線圖](../planning/roadmap-first-demo.md)
