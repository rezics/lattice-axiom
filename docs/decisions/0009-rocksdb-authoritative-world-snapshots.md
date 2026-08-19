---
title: demo 以 RocksDB 保存權威世界快照
status: accepted
type: decision
updated: 2026-08-19
---

# 決策 0009：demo 以 RocksDB 保存權威世界快照

## 背景

Lattice Axiom的世界生成由可更换的content／generator packages、locked capability provider、generator revision与config组成。若只保存seed与玩家差异，package／generator更新或内容移除后，同一coordinate可能得到完全不同terrain；已探索world便不稳定。另一方面，spatial data主要按chunk加载／卸载，不适合在demo中把每个block或一般world entity做成关系资料列。

先自研 Region File 再遷移到嵌入式資料庫，會建立一套只使用一個階段、仍需自行處理索引、原子寫入、損壞復原與備份的格式。既然 World Store 的長期本機實現已選定 RocksDB，demo 直接使用它。

## 決策

1. Bevy World／ECS 與 chunk working set 是正在執行的世界；RocksDB World Store 是已物化空間世界的權威；未來的 PostgreSQL 管理需要關聯查詢與跨世界交易的社會性資料；物件儲存只負責備份與快照分發。
2. 區塊第一次生成完成後立即保存完整可讀快照。之後只要快照存在便直接載入，不因目前生成器、內容或組態變更而隱式重算。
3. generator 是創造新世界資料的工具，不是已存在世界的資料來源。種子與 provenance 用於解釋、比較與顯式重建，不能取代已保存快照。
4. demo 的生產實現直接採 RocksDB，不建立自訂 Region File、SQLite 或其他過渡 World Store。另提供 `MemoryWorldStorage` 給單元與 property test。
5. Core／packages只依赖`WorldStorage` product contract；RocksDB key、column family、compaction与backup types不得渗入worldgen、ECS或dynamic ABI。
6. 以世界、維度與區塊座標形成穩定且可做前綴／範圍掃描的鍵空間。區塊快照、空間實體、模擬續行狀態與產物生成記錄是邏輯上分離的資料類別；實際 column family 數量由原型量測決定。
7. 一次區塊提交涉及的快照、空間實體索引、續行狀態與 provenance 使用同一原子 write batch。耐久等級與同步策略必須由 API 顯式表達並有崩潰測試。
8. 同一個世界在任一時刻只有一個權威寫入者。RocksDB 是嵌入式儲存引擎，不作為多個遊戲節點共同遠端寫入的共享資料庫。
9. `/regenerate` 是顯式破壞性 World Transaction：選定範圍、generator revision／config、保留政策與備份點後，建立新快照並處理該範圍的生成物件、玩家物件及邊界。demo 內同一 RocksDB 的變更以 write batch 原子提交。
10. RocksDB 的執行期 read snapshot 不等於可恢復備份。可攜備份使用 checkpoint／backup 流程，平台期再上傳物件儲存並驗證還原。
11. PostgreSQL 不進入 demo 的區塊載入路徑。平台期若將 Persistent World Object 放入關聯庫，必須按 `(world, dimension, chunk_x, chunk_y, chunk_z)` 批量載入，並另行設計跨 RocksDB／PostgreSQL 的協調、補償與恢復；不得假裝存在免費的跨庫原子交易。

## 快照內容

| 類別 | 是否持久化 | 例子 |
| --- | --- | --- |
| 已物化基線 | 是 | 方塊、群系、生成結構、第一次生成結果 |
| 持久世界狀態 | 是 | 玩家修改、容器、機器、空間實體與 schema-owned component |
| 模擬續行狀態 | 是 | 排定 tick、流體前緣、尚未完成的工作與必要 RNG 狀態 |
| 可重建暫態 | 否 | 網格、尋路快取、物理 broadphase、渲染資源 |

保存的是「恢復目前狀態與未完成工作所需的最小狀態」，不是永久事件日誌。

## Provenance

每個已生成區塊至少保存：

- 座標與建立時間；
- generation epoch 或等價標識；
- 正規化生成組態雜湊；
- 相关stable content／package IDs、generator revision、schema owner与locked realization／implementation fingerprint；
- 上游規劃產物與邊界契約識別；
- 快照 schema 與擁有者 schema 版本。

provenance 用來回答來源、差異、遷移與 regenerate 基線，不表示舊生成器必須永遠可執行。

## 結果

- 已探索區域不會因內容或生成器更新而突然改變；同一世界可合法包含不同 generation epoch。
- 直接使用 RocksDB 省去一次格式遷移，並取得排序鍵、範圍掃描、WAL、原子批次與 checkpoint 等基礎能力。
- 專案從 demo 起就承擔原生建置、壓實、寫入放大、磁碟用量、備份與損壞復原的營運成本。
- 完整快照比只存差異占用更多空間；壓縮、去重與增量備份必須以實際世界量測，而不是以改回隱式重生來解決。

## 被否決的方案

### demo 先寫自訂 Region File

它會重複建立索引、原子性、檢查碼、回收與備份機制，且使用者已決定直接採 RocksDB。

### 只存種子與玩家差異

在可更換生成器下，基線無法保證重現；移除舊生成器後甚至無法讀回世界。

### 把所有世界實體放入 PostgreSQL

一般實體主要隨區塊成批載入，逐物件關聯查詢會破壞空間局部性。PostgreSQL 留給真的需要跨世界查詢、交易與治理的資料。

## 驗證

- 第一次生成後更換生成器，舊區塊位元內容不變，新區塊使用新 generator revision／config fingerprint。
- 隨機程序終止後重開，已確認提交的 write batch 不出現半個區塊或孤兒索引。
- 量測不同區塊尺寸、壓縮、批次、快取與壓實設定下的讀寫延遲、放大與磁碟占用。
- checkpoint 能在獨立目錄還原，並由內容雜湊或巡檢工具驗證。
- `MemoryWorldStorage` 與 RocksDB 實現通過同一契約測試套件。
