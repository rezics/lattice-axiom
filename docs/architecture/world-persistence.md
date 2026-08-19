---
title: Bevy 世界持久化與 RocksDB World Store
status: proposed
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0003-no-global-version-switch.md
  - ../decisions/0009-rocksdb-authoritative-world-snapshots.md
  - ../decisions/0010-nickel-driven-package-system.md
  - ../decisions/0015-bevy-native-y-up-world-coordinates.md
---

# Bevy 世界持久化與 RocksDB World Store

## 結論

RocksDB 保存已物化世界的权威快照；Bevy World／ECS 只保存目前 active working set。persistence domain package／host adapter透过Bevy schedule与task pool捕捉、写入与确认snapshot，但长期格式不序列化Bevy、dynamic ABI或RocksDB的process-local型别。world metadata记录实际需要的package／content／schema closure，而不是一个global engine version。

## 資料所有權

| 層 | 擁有 | 不擁有 |
| --- | --- | --- |
| Bevy World／chunk working set | 目前載入、正在模擬的 chunk 與 entity | 跨重啟耐久性 |
| RocksDB World Store | 已物化 voxel、持久 entity/component、必要續行狀態、provenance | 多節點共同寫入、帳號經濟 |
| PostgreSQL（未來可選） | 帳號、權限、交易、跨世界查詢 | 每 voxel／一般 chunk 熱路徑 |
| Object storage（未來可選） | checkpoint／backup 分發 | active mutable world |

一個 world 同一時間只有一個權威 writer。多節點寫入若成為產品需求，需要新的協調架構，不把嵌入式 RocksDB 當共享遠端資料庫。

## 保存什麼

| 類別 | 是否保存 | 例子 |
| --- | --- | --- |
| 已物化基線 | 是 | voxel、biome、結構、第一次生成結果 |
| 玩家／玩法狀態 | 是 | 修改、容器、機器、持久實體與 owner component |
| 模擬續行 | 是 | 排定 tick、流體前緣、必要 RNG state、未完成權威工作 |
| provenance | 是 | generator revision、config hash、規劃產物、schema |
| 可重建衍生 | 否 | mesh、collider cache、physics broadphase、GPU asset、navigation cache |

generator 只創造未物化空間。只要 snapshot 存在，載入就以 snapshot 為準；生成器更新或內容變更不得隱式重算。

## 穩定 key 與 Y-up

chunk key 的邏輯欄位為：

```text
world_id / dimension_id / chunk_x / chunk_y / chunk_z / record_kind / owner
```

`chunk_y` 是垂直索引，`chunk_x`／`chunk_z` 位於水平面。實際 byte encoding 必須：

- 對有號座標有明確、可測的 canonical encoding；
- 支援 world／dimension／chunk prefix scan；
- 不依 Rust struct memory layout；
- 帶 schema ID／version；
- 能在 diagnostics 中安全解碼；
- 允許日後增加 record kind 而不碰撞。

column family 數量、compression、prefix extractor 和 compaction 由 benchmark 決定，不寫入 domain API。

## Chunk snapshot

每個 chunk snapshot 至少包含：

- stable chunk coordinate；
- snapshot schema；
- chunk revision；
- stable block palette + compact voxel data；
- persistent spatial entities 和 owner component envelopes；
- 必要 scheduled work／simulation continuation；
- generation provenance；
- optional integrity hash。
- required package／schema owner identity（适合放在world／region metadata时不在每个chunk重复）。

Bevy `Entity`、`Handle`、Rust `TypeId`、ABI generational handle、dynamic callback key、物理solver handle、asset server index与GPU ID都不允许出现在payload。

## Bevy schedule 中的 commit

persistence host adapter定义少量versioned semantic stages，并映射到Bevy `SystemSet`：

```text
FixedUpdate
... gameplay mutates authoritative state ...
CommitWorldRevision
CapturePersistence
        ↓ immutable ChunkCommit
Bevy I/O task pool
        ↓ RocksDB atomic WriteBatch
Update
ObserveCommitResult
```

流程：

1. gameplay command 只在 FixedUpdate 的權威 set 修改 chunk，成功後遞增 `ChunkRevision` 並標 dirty。
2. `CapturePersistence` 從一致 revision 建立 immutable `ChunkCommit`；序列化輸入不再借用 Bevy World。
3. 有界 I/O task 將同一 chunk 的 snapshot、entity index、continuation 和 provenance 放入一個 RocksDB atomic write batch。
4. 完成訊息帶回 world／chunk／revision／durability level／error。
5. 主 World system 只有在目前 revision 不小於完成 revision 時更新 `last_committed_revision`；只有二者相等時才能清 dirty。
6. 若 chunk 已再次變更，舊 commit 仍是合法 durability 進度，但不能宣告最新狀態已保存。

callback 不直接修改 ECS。in-flight commits、bytes、retry 和 shutdown drain 都有上限。

## Durability 語義

API 必須區分：

- queued：只進記憶體 queue；
- written：RocksDB 已接受 write，但未必要求同步介質；
- durable：按選定 sync policy 確認；
- checkpointed：已進可獨立還原的 checkpoint／backup。

玩家 UI 顯示「已保存」時對應哪一級必須固定。自動保存、手動保存與正常退出可以有不同 batch／sync 策略，但不能以模糊 boolean 表達。

## 載入

```text
request chunk
    ↓
I/O task reads and validates envelope
    ↓
locked package / schema owners migrate to current DTO
    ↓
content IDs resolve against RegistrationImage
    ↓
main-world system instantiates chunk working set / ECS
    ↓
derived mesh and collider jobs start
```

unknown schema、missing authoritative package／content、checksum error与migration failure要在修改Bevy World前识别。schema owner与migration来自已锁定package closure；realization可以改变，但logical owner／schema contract不能因此改变。可恢复情况可进入read-only recovery／placeholder policy；不可恢复时保持world未载入并提供诊断。

## Chunk 卸載

chunk 只有在以下條件成立時才能離開 working set：

- 不在必須保持 active 的 simulation／player radius；
- 沒有未套用的權威 command；
- 最新 revision 已達要求的 durability；
- 持久 entity 已轉成 stable envelope；
- 衍生 task 已取消或其結果會因 epoch／revision 自動失效；
- presentation／physics resource 已撤除。

卸載 ECS／mesh 不等於刪除 RocksDB snapshot。

## 正常退出與崩潰

正常退出進入 `ShuttingDown` state：

1. 停止接收會產生新 dirty state 的 gameplay action；
2. quiesce package instances并取消会产生新权威command的tasks；
3. capture 所有 dirty chunk 的最新 revision；
4. 等待有界 I/O drain；
5. 寫入 world metadata／clean-shutdown marker；
6. 到達 durability 要求後才停止module instances并发送Bevy `AppExit`。

若 deadline 超時，保留 WAL 可恢復狀態並報告未確認 revision。程序 crash／kill test 必須證明每個 atomic batch 要麼完整存在、要麼完全不存在；正常退出流程不能替代這項測試。

## Regenerate

`/regenerate` 是顯式破壞性 world transaction，不是 cache refresh。它必須指定：

- world／dimension／chunk 範圍；
- generator revision 和 config；
- 玩家／系統擁有內容的保留政策；
- 邊界輸入與鄰接 validation；
- 預覽／dry run；
- checkpoint／backup；
- staged 新 snapshot 和原子切換策略。

對單一 RocksDB 範圍，盡量用 staging keys + atomic metadata switch 或有界 write batch；跨 RocksDB／未來 PostgreSQL 時另需 journal／outbox／補償協定，不能假裝有免費跨庫 transaction。

## Backup

RocksDB read snapshot 只提供程序內一致讀視圖，不是可攜備份。備份使用 checkpoint／backup engine 等上游機制，複製到獨立位置後驗證 restore。

至少定期做：

- checkpoint 建立與獨立目錄 restore；
- manifest／world metadata checksum；
- 抽樣 chunk decode 與 content reference validation；
- 舊 schema fixture migration；
- 恢復時間與儲存放大量測。

## WorldStorage 邊界

`WorldStorage` 是合理的產品邊界，因為它描述權威 snapshot 的 load／commit／checkpoint 語義並支援 `MemoryWorldStorage` 測試；它不是為任意資料庫建立的通用 facade。

規則：

- domain crate 看不到 RocksDB key／column family／iterator type；
- RocksDB implementation 可以完整善用 transaction batch、prefix scan、WAL、checkpoint 和 tuning API；
- 不因假想替換而限制到最低共同分母；
- 第二 production backend 只有真實部署需求時才加入。

## 驗收

- 挖掘／放置後重啟，stable block ID 與 voxel revision 正確恢復。
- 更換 generator 後舊 chunk bytes 不變，新 chunk 保存新 provenance。
- 在 write batch 各階段強制終止，重啟不出現半個 snapshot 或 orphan index。
- 打亂 I/O completion 順序，舊完成訊息不清除新 dirty revision。
- `MemoryWorldStorage` 與 RocksDB 通過相同 domain contract tests。
- checkpoint 可在獨立目錄恢復並進入 headless world。
- 存檔掃描不出現 Bevy Entity／Handle 或第三方 solver handle。
- 同一gameplay package从static切换到portable dynamic realization后读取／写回相同schema fixture，normative snapshot bytes不变。
- 缺少required package／schema时在world进入writable Bevy state前失败，并列出owner与相容range。

## 相關文件

- [決策 0009：RocksDB 權威世界快照](../decisions/0009-rocksdb-authoritative-world-snapshots.md)
- [Bevy 執行期架構](game-engine-runtime.md)
- [套件與 ABI 版本](versioning-and-compatibility.md)
- [世界生成](world-generation.md)
