---
title: 凍結首版權威世界與持久化契約
status: accepted
type: decision
updated: 2026-08-20
---

# 決策 0027：凍結首版權威世界與持久化契約

## 背景

[決策 0009](0009-rocksdb-authoritative-world-snapshots.md)已經決定以 RocksDB 保存已物化世界的
完整權威快照，Bevy World 只保存 active working set，並以 `MemoryWorldStorage` 作為相同 product
contract 的測試實作。[決策 0015](0015-bevy-native-y-up-world-coordinates.md)凍結了右手 Y-up，
[決策 0020](0020-semantic-registration-and-content-selection.md)則要求存檔保存 concrete stable ID、
frozen Role binding 與 active ContentBundle intent，而不是在重開時重新猜測內容。

R5／D3 開始實作前，權威 chunk、revision、durability、wire encoding、metadata projection、world
open、checkpoint、migration、low-disk 與 trash 仍有十八項可改變長期資料或玩家復原結果的問題未定。
若把它們留給 storage adapter、Bevy host 與 client shell 各自選擇，會產生下列第二份真相：

- runtime palette、RocksDB key 與 snapshot bytes 各有不同 coordinate／ordering；
- voxel、entity 與 scheduled work 各自宣稱「已保存」，但無完整 chunk durability point；
- header、RocksDB metadata 與 UI health 狀態無法在 crash 後判定誰是權威；
- exact lock、compatible package range 與實際 used content 被混成一個 hash；
- missing authoritative content 被 presentation placeholder 偷偷帶入 writable world；
- migration、clone、trash 與跨 volume move 直接改原目錄，失敗後沒有可證明的 publish point；
- low disk 只顯示警告，遊戲仍繼續產生無法 durable 的 authoritative mutation。

本決策凍結第一個可玩 vertical slice 的首版 contract 與 bootstrap safety values。D3 必須用真實
Terrenia snapshot 與 RocksDB prototype 驗證 physical tuning；D10 再以完整 sandbox traversal
凍結 release performance profile。若證據要求改變 on-disk meaning，必須建立新 schema／policy major
與 migration；不能在相同版本下靜默改寫。

## 核心不變條件

1. 已物化 snapshot 是權威；generator 只創造未物化空間，更新 package、generator 或 config
   不得隱式重生舊 chunk。
2. 一個 world 同時只有一個 authoritative writer。RocksDB、Bevy、dynamic module 與 client shell
   都不能形成第二個 writer。
3. snapshot、persistent entity index、simulation continuation、provenance 與 requirement-closure 更新
   在同一權威提交中原子發布。
4. Bevy `Entity`、`Handle`、`TypeId`、Rust layout、ABI handle、function pointer、allocator identity、
   physics／asset／GPU process-local ID 不進 key、snapshot、header、checkpoint 或 export。
5. save／key／palette／diagnostic 結果不依 package discovery、registration、task completion、
   `HashMap`、archetype 或 RocksDB iterator 的非規範順序。
6. `SourceSpan` 只表示同一 source 內的 byte range；完整 source ID、logical path、origin chain 與
   location receipt 統一稱為 `SourceProvenance`。world／migration diagnostic 不得把二者混稱。
7. preflight 不執行 module business code、不建立 Bevy world、不打開 writer；任何不安全或不完整的
   compatibility 結果在 mutation 前失敗。
8. presentation、mesh、collider、thumbnail、navigation 與 render cache 可重建，不進 authoritative
   world hash，也不能使 authoritative snapshot 失效。

## 決策

### 1. `WORLD-01`：Chunk、palette 與 Y-up key v1

首版 dimension layout 的 authoritative chunk edge 固定為 `32`，即 `32 × 32 × 32` voxels。
chunk coordinate 是 `(i32 x, i32 y, i32 z)`，其中 `y` 是垂直索引。相同 world 不能以 runtime
setting 改 chunk edge；layout 與 snapshot schema fingerprint 一起保存。

`ChunkSnapshotV1` 至少包含：

```text
ChunkSnapshotV1
├── dimension stable ID / ChunkCoord<i32>
├── snapshot schema ID / owner / version
├── WorldRevision / ChunkRevision / domain revisions
├── solid palette + bit-packed indices
├── optional fluid palette + bit-packed state indices
├── persistent entity / owner-component envelopes
├── scheduled work / simulation continuation
├── generation epoch / config / planning provenance
├── semantic image / Role binding / bundle receipts used by the chunk
├── required owner / schema receipt
└── integrity digest
```

palette entry 按以下 canonical tuple 的 byte order 升序排列，再重寫 cell indices：

```text
(concrete StableId, state schema ID, state schema version, canonical state bytes)
```

`solid` layer 必須存在。`fluid` 是獨立 optional layer；缺少 layer 表示全空，不以 water／lava
stable ID 塞入 solid occupancy。fluid palette index `0` 固定為 `Empty`，其餘 entry 才按上述 tuple
排序；D9 v1 非空 fluid 只與 `terrenia:block/air` solid occupancy 共存。palette index width 是可表示
palette length 的最小 bit width，最少一 bit；bit stream 以 little-endian byte／least-significant-bit first
保存。local voxel linear index 固定為：

```text
local_x + 32 * (local_z + 32 * local_y)
```

因此 `x` 最快、`z` 次之、`y` 最慢。runtime 可使用其他 mutable palette，但 normative write 前
必須 canonicalize。fluid definition／state與 occupancy限制由
[決策 0028](0028-freeze-worldgen-content-and-asset-contract.md)擁有；本決策只擁有 snapshot layer／index
encoding。

Chunk-record key v1 是 versioned binary tuple：

```text
u16be key_major = 1
WorldId raw bytes [16]
u16be dimension_id_length + canonical UTF-8 dimension StableId
u32be ordered_x
u32be ordered_y
u32be ordered_z
u16be record_kind
u16be owner_length + canonical UTF-8 owner StableId
```

signed coordinate 的 ordered value 固定為 `(value as u32) ^ 0x8000_0000`，再以 big-endian 寫入；
這使負數、零與正數保持數值 range order。禁止 native endian、zig-zag、Rust struct bytes、路徑分隔
字串或 package／directory name 拼接 key。未知 key major 或超長／malformed segment 必須可診斷拒絕；
未知 `record_kind` 在 envelope邊界可驗證時只能 opaque preserve／read-only，不能猜測或 writable alias。

### 2. `WORLD-02`：一個權威 revision 與多個派生 revision

首版使用三個層次，全部是 checked `u64`，初值為 `0`，overflow 時 world fail closed：

- `WorldRevision`：一次成功 authoritative command transaction 的全 world 單調序；跨 chunk transaction
  共用同一值；
- `ChunkRevision`：該 transaction 實際改變每個 chunk 的完整權威 revision；
- `VoxelRevision`、`PersistentEntityRevision`、`ContinuationRevision`：只在對應 domain 改變時增加，
  供 mesh／collider／entity job／scheduled work 判斷 stale。

snapshot、dirty、queued／written／durable、unload 與 crash recovery 只以完整 `ChunkRevision` 和其
capture 的 `WorldRevision` 判定。domain revision 不能各自宣稱 chunk 已完整保存。commit receipt 明文帶
world revision、chunk revision、domain revisions 與 changed-domain mask。

`last_durable_world_revision` 是所有不大於該值的 authoritative changes 都已達 durable 的 contiguous
frontier；不能把不同 chunk 的 local revision 直接比較成 world frontier。

### 3. `WORLD-03`：Chunk lifecycle、radius 與 backpressure

Residency 與 persistence 是兩個正交狀態機：

```text
Absent → Requested → Materializing → ResidentInactive ↔ Active → Evicting → Absent

Clean → Dirty → Captured → CommitInFlight → Written → Durable
```

首版 `world-streaming-policy@1` bootstrap defaults與
[決策 0026](0026-freeze-first-demo-performance-budgets.md)的 `desktop-reference-v1` 共用 chunk-space
Chebyshev horizontal radius 與 absolute vertical radius：

| 範圍 | horizontal | vertical |
| --- | ---: | ---: |
| active | 4 | 2 |
| resident | 6 | 3 |

distance 不是唯一 pin；player、authoritative scheduled work、persistent entity、explicit system pin
都可保持 active／resident。resident 與 active boundary 使用一個 chunk hysteresis，避免來回 thrash。

不存在「persistence radius」。每個 dirty chunk 無論距離都有 durability obligation。首版 safety ceilings：

| 項目 | hard ceiling |
| --- | ---: |
| requested／materializing／prefetch chunks combined | 1,536 |
| world-generation jobs | 128 |
| dirty chunks not yet captured | 256 |
| queued／in-flight persistence commit jobs | 64 |
| in-flight uncompressed commit bytes | 64 MiB |
| automatic retry attempts per commit | 3 |
| normal-shutdown I/O drain | 10 s |

active與resident（包含distance外pins）分別仍受`405`與`1,183` chunk hard cap。soft high-water 停止
遠端 prefetch並降低 generation／mesh concurrency；hard resident limit 只驅逐已達durability gate 的
chunk。達 dirty／I/O ceiling 時停止新 materialization並 coalesce 同 chunk 到最新尚未
capture 的 revision。storage failure 或 low disk 時停止新 authoritative mutation，不能丟棄 dirty state。

### 4. `WORLD-04`：RocksDB prototype physical profile

RocksDB physical options 不進 `WorldStorage` domain API。D3 prototype 的首選 profile 是：

- required `default` column family 保持空；
- `metadata` column family 保存 authoritative world metadata、active generation 與 receipts；
- `records` column family保存以 `record_kind` 邏輯分離的 chunk records；
- upper levels 使用 LZ4，bottommost 使用 Zstd，checksums 開啟；
- version／world／dimension／chunk prefix extractor 與 prefix bloom；
- shared block cache ceiling `256 MiB`；
- write buffer bootstrap value `64 MiB`；
- 所有同一 authoritative transaction 的 records 與 metadata 使用一個 `WriteBatch`。

禁止每 package、schema、owner 或 chunk 建 column family。D3 benchmark 必須比較 2-CF／split-index、
none／LZ4／Zstd、16³／32³／64³、batch bytes、sync policy、cold／warm read、prefix scan、WAL reopen、
checkpoint、sustained compaction 與 crash。physical profile 可由證據更改而不改 world schema；若改 key、
payload 或 logical atomicity，則需新 contract major。

### 5. `WORLD-05`：Durability 與「已保存」

首版固定四級：

| level | normative meaning | UI |
| --- | --- | --- |
| `Queued` | 只在 bounded memory queue | `Saving…` |
| `Written` | WAL enabled、RocksDB 接受 write，未要求同步介質 | 不得顯示「已保存」 |
| `Durable` | WAL enabled 且本次 commit 使用 sync write 成功 | `Saved／已保存` |
| `Checkpointed` | revision 已進獨立位置且驗證可還原的 checkpoint | `Recovery point created` |

UI 只有在目前 authoritative world frontier 不高於 `last_durable_world_revision` 時顯示「已保存」。
一個 chunk durable 不代表整個 world saved。手動 Save 與正常退出至少等待 `Durable`；checkpoint 是獨立、
較昂貴且明文命名的操作。任何 RocksDB callback success 都不能不經 policy receipt 冒充 durable。

### 6. `WORLD-06`：Snapshot wire encoding

首版 writer 只寫 `world-wire@1`。它由固定 envelope、versioned Rust serde DTO 與 pinned postcard payload
組成；voxel／fluid indices 是 DTO 內的自有 bit-packed byte sections，不逐 cell serde。

Outer envelope 固定為：

```text
magic [8] = "LAXWSNP\0"
u16be envelope_major = 1
u16be schema_id_length + canonical UTF-8 schema StableId
u16be owner_length + canonical UTF-8 PackageName
u32be schema_version
u64be uncompressed_payload_length
SHA-256 payload_digest [32]
postcard payload bytes
```

RocksDB compression 是 storage concern，不改 normative payload；digest 覆蓋未壓縮 postcard payload。
Authoritative DTO 禁止 `usize`／`isize`、`HashMap`／`HashSet`、implicit enum declaration order、未規範
NaN／negative zero、pointer、process-local handle或 Rust layout。collection 使用 stable sorted vector／
`BTreeMap`；wire enum 使用明文固定 numeric tag；每個 schema version 有獨立 decoder、golden 與 stepwise
migration。更新 postcard crate 只有在既有 golden bytes 全部保持相同時才可繼續寫 `world-wire@1`。

### 7. `WORLD-07`：Exact lock 與 minimum requirement closure 並存

Authoritative RocksDB metadata 同時保存兩層，不能二選一：

1. `FrozenLockReceipt`：完整 exact `latticeaxiom.lock` bytes／content-addressed reference、package source與
   artifact hashes、realizations、ABI／EngineBuildId requirements、Registration／Semantic image、active
   bundles、frozen Role bindings 與 authoritative settings；
2. `WorldRequirementClosure`：實際持久資料需要的 package owner＋compatible range、schema ID／version／
   supported read contract、concrete stable content IDs、semantic contract majors、bundle／binding receipts、
   generator provenance 與 presentation optional policy。

open 先嘗試 exact frozen lock。exact artifact 不可用時，minimum closure 只可產生
`ReadyCompatible`／`NeedsMigration` 與展開 diff，不能自動改 frozen lock。used closure 在引入新 persistent
StableId／schema 的同一 authoritative batch 內單調擴張；首版不自動縮減。縮減需要離線全量掃描、
checkpoint 與新 migration transaction。

`WorldHeader` 只投影 hashes、counts與 bounded summaries；完整兩層 metadata 只以 RocksDB 為權威。

### 8. `WORLD-08`：Missing package／content policy

首版固定以下 matrix：

| 缺失／不相容 | 結果 |
| --- | --- |
| presentation asset／renderer-only package | degraded placeholder；可 writable，但不得改 authoritative hash |
| exact artifact缺失，verified compatible realization滿足全部 authoritative contract | `ReadyCompatible` + explicit diff |
| authoritative content缺失，但 envelope可完整 opaque preserve | `RecoverableReadOnly`；placeholder只供顯示 |
| verified owner migration chain可用 | `NeedsMigration`；checkpoint／clone後 staged migration |
| authoritative schema／content／semantic損壞或未知且無安全 decoder | `Blocked` |

authoritative writable placeholder 被禁止。read-only open 的 writer與 authoritative command count 必須為零，
且未知 bytes／owner／StableId／schema原樣保留。alias、名稱、Tag、Role current binding 或 registration order
都不能 silent remap concrete ID；alias仍必須由 owner 的 versioned migration 執行。

### 9. `WORLD-09`：Static／dynamic persistence equivalence

`@example/dual-gameplay` 的 persistent component 使用同一 `GeneratedSharedSchema` model／codec產生 static
typed adapter 與 portable dynamic batch adapter。Normative matrix 至少包含：

- static create／save → dynamic reopen／mutate／save；
- dynamic create／save → static reopen／mutate／save；
- `MemoryWorldStorage` 與 RocksDB；
- schema v1、ABI-layout-only change、schema v2 migration；
- client／headless與不同 process／build。

比較 schema identity、registration／schedule、commands、authoritative state hash、decoded values、canonical
component payload與完整 normative snapshot bytes。必須有兩個反證：layout 改變而 wire schema不變時 save
bytes不變；layout相同但 persistence meaning 改變時必須 migration／拒絕。ABI layout hash不能代理
persistence compatibility。

### 10. `WORLD-10`：RocksDB-first metadata publish 與 bounded header

RocksDB metadata 是唯一權威。catalog sidecar 固定為 canonical JSON `world-header.json`，上限 `64 KiB`，
至少帶 `WorldId`、`store_id`、`metadata_epoch u64`、authoritative metadata hash、exact lock／registration／
semantic／settings fingerprints、clean marker、durable frontier、checkpoint summary與自身 checksum。

sidecar 與 RocksDB 不可能跨系統原子提交，因此首版固定 DB-first recoverable protocol：

1. 在同一 RocksDB sync `WriteBatch` 寫 authoritative metadata、下一個 `metadata_epoch` 與預期 header
   projection hash；
2. 寫同目錄 temporary header，fsync file；
3. 以 platform-supported same-directory atomic replace發布 `world-header.json`，再 fsync directory；
4. 只有兩邊 epoch／hash／WorldId／store ID 相同才允許 writable open。

crash 造成 DB ahead 時，catalog 顯示 stale／NeedsRecovery；preflight可由DB顯式重建 sidecar。temporary
header不發布。Header中的 health 不是權威 persisted fact；catalog從 bounded validation與最近 cross-check
計算 health。header損壞不能隱藏 entry，也不能覆蓋 DB metadata。

### 11. `WORLD-11`：World identity、名稱與目錄

新 world identity **只**使用 UUIDv4。`WorldId` wire form 固定為 RFC 4122 network-order raw `16` bytes；
canonical text 固定為 lowercase hyphenated UUID：

```text
xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
```

所有 public／persistent text boundary 只接受完全 canonical text；uppercase、braced、URN、simple、mixed
或其他可被寬鬆 UUID parser理解的形式仍是 input error，不得 silently reformat。內部從已驗證 raw bytes
建構不重新解析文字。

world directory name固定為 canonical WorldId text，不建立可變 directory slug contract。display name
另作 NFC、trim、禁止 control／NUL／path separator驗證，上限 `128` Unicode scalar values且 UTF-8不超過
`512` bytes；同名允許並以 short WorldId消歧。rename只更新 metadata，不搬目錄、不改WorldId／keys。

跨 root move 必須先關 writer並drain。同 volume使用atomic rename；跨 volume使用temporary copy、fsync、
checksum＋headless preflight、target publish，成功後才移除 source。duplicate live WorldId一律隔離／Blocked；
只有 Clone／Restore-as-clone可產生新WorldId。

### 12. `WORLD-12`：WorldCatalog bounded scan

world roots 是 device／user setting 中明文列出的 canonical、non-overlapping allowlist。首版只掃每個 root
的直接 child world directories與managed trash；不遞迴任意filesystem，不follow symlink／junction／reparse
point。initial scan只讀 bounded header，不開RocksDB、不執行resolver、migration或module code。

Bootstrap scan limits：

| 項目 | 上限 |
| --- | ---: |
| concurrent header reads | 8 |
| header bytes | 64 KiB |
| thumbnail encoded bytes | 2 MiB |
| thumbnail dimensions | 512 × 512 |
| concurrent thumbnail decodes | 2 |
| concurrent recursive size jobs | 2 |

thumbnail與size延遲載入；watcher debounce且不能改變目前focus。每個 entry 使用 typed error：至少包含
Unreadable、PermissionDenied、HeaderTooLarge、BadChecksum、UnsupportedHeader、DuplicateWorldId、
MetadataMismatch、IncompleteTemporary、SymlinkEscape與TrashTombstoneError。單一損壞 entry 不阻塞或隱藏
其他world；完整DB cross-check屬 preflight。

### 13. `WORLD-13`：`WorldOpenStatus` normative 判定

首版狀態與判定固定如下：

| 狀態 | 必要條件／下一步 |
| --- | --- |
| `ReadyExact` | header↔DB一致、clean／durable、exact lock／artifact／trust／ABI／schema／semantic／settings全匹配，無write-before-open工作 |
| `ReadyCompatible` | exact不同，但全部authoritative read／write contract相容且無world-byte migration；顯示diff，確認前不改lock |
| `NeedsDownloadOrBuild` | 有可滿足graph，但下一個安全步驟是取得／build artifact；完成後重跑preflight |
| `NeedsMigration` | target closure已可用，但world bytes／schema／setting／binding需staged migration，checkpoint與disk gate可滿足 |
| `RecoverableReadOnly` | writer不安全，但可完整保留／inspect／export／restore |
| `Blocked` | read與opaque preservation也不能安全完成，或identity／trust／corruption不可恢復 |

多個問題並存時，status表示下一個可執行的安全步驟，所有 structured diagnostics仍保留。unclean／
non-durable world在 recovery驗證前不是 `ReadyExact`；low disk不能宣稱 `NeedsMigration` 可執行。

`writable` 不作可與status矛盾的獨立 input field；它由 accepted plan／action派生。diagnostics使用 stable
`DiagnosticCode`與structured fields，不使用無型別 `Vec<String>`。Plan action至少包含 UseFrozenLock、
ResolveCompatibleGraph、PreparePackage、OpenReadOnly、Export、RestoreCheckpoint、RepairHeader、
CloneAndMigrate。

### 14. `WORLD-14`：Preflight 的純資料上限

Pure preflight可以：

- 驗證 bounded header、RocksDB read-only metadata、checkpoint與clean marker；
- 讀 frozen lock與available package／artifact catalog；
- 驗證 external manifest、artifact hash／trust、declared ABI／EngineBuildId；
- 編譯／比較 Registration、Semantic、Role／bundle、Settings與requirement-closure data；
- 讀 declarative migration descriptors，估算record數、bytes、disk與風險；
- 產生 immutable `WorldOpenPlan`。

Pure preflight不可以 `dlopen`、呼叫entry／query／create／start、執行owner decoder callback、建立Bevy App／
World、執行migration或打開writer。Artifact acquire／build是明文plan action，完成後重跑preflight。

玩家接受plan後進入 activation validation：可以map library並重驗真實ABI／instance lifecycle，但world writer
仍關閉；全部module與decoder validation通過後才開writer。Migration code只在checkpoint後的staging store
與受限migration context執行，不接觸原world。external ABI metadata必須由artifact hash綁定，actual loader
仍在activation二次驗證。

### 15. `WORLD-15`：Checkpoint rotation 與 Clone

每個 checkpoint 記錄 stable checkpoint ID、source WorldRevision、exact lock hash、header／metadata hash、
reason、physical bytes與restore verification receipt。若 source revision、lock hash與metadata hash全相同，
不得重複建立等價 checkpoint。

首版分兩類：

- protected：initial、latest-known-clean、pre-migration／pre-graph-change與manual pinned；
- rotating automatic：最多 `3` 個，physical retained bytes上限為 `min(10 GiB, 20% filesystem capacity)`。

protected checkpoint不因普通rotation刪除；若空間不足以保留最低安全點，拒絕migration／clone並進
low-disk流程。自動刪除前必須輸出可行動diagnostic；manual pinned只由明文Purge移除。

Clone從已驗證checkpoint建立temporary store，產生新 UUIDv4 WorldId、writer lease與
`source_world_id／source_checkpoint_id／source_revision／source_lock_hash` provenance。因key含WorldId，
禁止只copy folder再改header；clone必須重寫key prefix或經明文versioned store-generation transform，
再 checksum、headless preflight與atomic publish。

### 16. `WORLD-16`：Migration staging、publish 與 rollback

任何 migration 前先建立／驗證checkpoint，不得原地覆寫active records。首版分級：

- 若預估不超過 `64 MiB` uncompressed staged payload且不超過 `4,096` records，使用同DB新 migration
  epoch／staging keys；全量驗證後以一個 metadata switch發布；
- 超過任一上限，使用checkpoint建立 sibling RocksDB directory，寫migration receipt、record counts／hashes，
  關閉所有handles後以same-volume active-generation pointer／directory publish；
- cross-volume migration先完整stage到target volume，再執行同volume publish，絕不假裝跨volume原子rename。

Publish gate至少驗證target lock／trust、schema與content closure、record count／digest、revision、disk
headroom、header projection及headless reopen。任一failpoint都保留原world／checkpoint，半migration不發布。
發布後尚無新authoritative mutation時可切回舊generation；一旦新generation產生新資料，rollback語義是
restore／clone，不是無損downgrade。

### 17. `WORLD-17`：Low-disk admission 與 mutation pause

Storage pressure 使用有hysteresis的：

```text
Normal → Warning → MutationPaused → RecoverableReadOnly / ShutdownRequired
```

首版每秒poll filesystem free／quota，並在任何 write／WAL／checkpoint error後立即重驗。Bootstrap thresholds：

- Warning：usable free低於 `max(5 GiB, 10% volume capacity, 2 × required_headroom)`；
- MutationPaused：usable free低於 `max(2 GiB, 5% volume capacity, required_headroom)`；
- `required_headroom = 1 GiB reserve + bounded in-flight bytes + worst-case WAL/shutdown drain
  + estimated current checkpoint/migration bytes`；
- resume只有 usable free高於 Warning threshold並連續兩次poll成功，避免thrash。

Warning停止遠端prefetch、新自動checkpoint與非必要worldgen，並顯示persistent action。MutationPaused在
break／place／craft等command套用前拒絕新authoritative mutation，優先drain既有dirty。若既有dirty無法
durable，停止simulation並轉read-only／shutdown recovery；不得繼續累積只在memory的新state。ENOSPC、
quota、WAL或checkpoint failure直接至少進 MutationPaused，不等待下一次poll。

### 18. `WORLD-18`：Managed Trash 與跨平台 restore

首版使用每個world root同filesystem的 managed trash，不依賴OS recycle bin。entry path為：

```text
<world-root>/.trash/<WorldId>/<trash-entry-id>/
```

每個entry有bounded tombstone：original root／path、WorldId、display name、deleted time、header／metadata
checksum、size、last checkpoint與retention policy。Move to Trash先關writer／drain；同volume atomic rename，
跨volume使用temporary copy、fsync、checksum＋headless preflight、publish後才移除source。

Restore先preflight。display-name conflict允許；live WorldId conflict預設 `Blocked`，可提供 Restore-as-clone，
依 `WORLD-15`產生新ID並re-key，絕不overwrite。首版 auto-purge **disabled**；只有明文Permanent Purge
刪資料。Purge顯示WorldId、name、size、checkpoint與不可恢復後果，只接受resolved且證明位於managed
trash內的literal path，拒絕symlink／junction／reparse escape。失敗保留可重試entry與typed diagnostic。

## D3 Prototype-selected values 與 evidence gate

本決策中的 `32³`、streaming radii、queue ceilings、RocksDB CF／compression／cache、checkpoint rotation、
migration threshold與disk watermarks是首版 bootstrap values，不是免驗證的性能結論。D3 出場前必須在
optimized build提交machine-readable evidence：

1. `16³／32³／64³` 的 generation、decode、edit、mesh／collider、snapshot size、read／write與memory比較；
2. canonical key／palette／postcard golden，以及正負Y-up coordinate、order randomization與malformed corpus；
3. RocksDB CF／compression／cache／batch matrix的P50／P95／P99、throughput、RSS、read／write／space
   amplification、compaction stall、WAL recovery與checkpoint restore；
4. queue／dirty／in-flight boundary與`boundary + 1` backpressure；
5. crash failpoint覆蓋write batch、metadata publish、header replace、checkpoint、clone、migration與trash；
6. Memory／RocksDB與static／dynamic相同 contract／normative bytes；
7. exact lock、missing owner、old schema、unclean與low-disk preflight corpus。

若 D3 evidence否定 `32³` 或 on-disk codec/key，必須在 D3 materialized-world fixture凍結前以新 ADR／schema
major取代本決策相應值。physical RocksDB option與runtime budget可在不改wire meaning時更新bootstrap
profile，但必須保存benchmark receipt與effective setting provenance。

## D10 Release freeze gate

D10 以72 blocks、兩種fluid、inventory／container／machine、normal／crash、static／portable與10分鐘固定
traversal重新量測並凍結 release profile：

- active／resident／visible chunks與RAM／VRAM；
- generation、decode、mesh、collider、save、checkpoint與migration P50／P95；
- task／I/O／FFI／fluid queue及in-flight bytes high-water；
- disk growth、WAL、checkpoint retention、trash與low-disk recovery；
- static／dynamic authoritative state、semantic report與normative save bytes；
- client／headless preflight、checkpoint restore、export與另一root dimension conformance。

任何 D10 budget變更不得放寬無界queue、把`Written`改稱`Saved`、允許authoritative placeholder、跳過
checkpoint或改變相同 schema major 的bytes。

## 延後 Gate

以下不進首版，只有真實consumer與新決策後才加入：

- multi-writer／network shared RocksDB、跨 RocksDB／PostgreSQL transaction；
- automatic used-content closure縮減與一般world vacuum；
- native／schema owner code在pure preflight執行；
- writable authoritative placeholder或名稱／Tag猜測式repair；
- in-place migration、未checkpoint的downgrade與跨volume假原子rename；
- OS recycle-bin integration與無通知auto-purge；
- cloud save、跨設備merge、event-sourced永久日誌與任意舊world通用修復器；
- 多種同時可寫的 snapshot wire format negotiation。

## 自動驗證

- key codec property tests證明signed order、prefix scan、Y-up segment order與unknown-version拒絕。
- snapshot golden覆蓋palette reorder、state、solid／fluid、entity／continuation、integrity與兩代schema。
- random I/O completion不讓舊receipt清除新dirty；domain-only edit只失效相應derived job。
- kill／fault injection證明atomic batch沒有half snapshot／orphan index，DB-first header publish可恢復。
- catalog包含bad／oversized／unknown header、duplicate ID、symlink escape與慢size／thumbnail，不阻塞其他entry。
- preflight instrumentation證明module callback、Bevy World、writer與authoritative command count皆為零。
- status matrix覆蓋exact、compatible、missing artifact、migration、read-only、blocked與多重問題precedence。
- checkpoint可在獨立目錄restore；clone有new WorldId與完整provenance；半migration永不發布。
- low-disk `boundary`／`boundary + 1`在command apply前pause，保留最近durable world。
- trash move／restore／collision／cross-volume failpoint與path-containment corpus不overwrite live world。
- `@example/dual-gameplay` static↔dynamic與Memory↔RocksDB round-trip產生相同normative bytes。
- D3與D10完整evidence可由clean checkout、frozen locks與headless命令重現，不要求window、GPU或人工輸入。

## 取捨

- `32³`、postcard與明文key profile讓首版可建立byte-level golden，但若D3證據否定它們，必須付出
  明文schema replacement成本；這比已發布後靜默漂移安全。
- DB-first header publish承認sidecar與RocksDB無免費跨系統transaction；catalog可能短暫顯示NeedsRecovery，
  但不會把陳舊header當權威寫入。
- 一個完整ChunkRevision會讓entity-only edit形成新snapshot durability point；domain revisions保留derived
  work效率，避免分域durability造成部分保存。
- managed trash與sibling migration會增加暫時磁碟占用，因此low-disk admission必須在操作前阻止危險流程。
- exact lock與minimum requirement closure同時保存增加metadata，但分開了重現與兼容判斷，避免hash代理語義。

## 問題追蹤

| Open question | 本決策 |
| --- | --- |
| `WORLD-01` | 第1節 |
| `WORLD-02` | 第2節 |
| `WORLD-03` | 第3節 |
| `WORLD-04` | 第4節與D3 evidence gate |
| `WORLD-05` | 第5節 |
| `WORLD-06` | 第6節 |
| `WORLD-07` | 第7節 |
| `WORLD-08` | 第8節 |
| `WORLD-09` | 第9節 |
| `WORLD-10` | 第10節 |
| `WORLD-11` | 第11節 |
| `WORLD-12` | 第12節 |
| `WORLD-13` | 第13節 |
| `WORLD-14` | 第14節 |
| `WORLD-15` | 第15節 |
| `WORLD-16` | 第16節 |
| `WORLD-17` | 第17節 |
| `WORLD-18` | 第18節 |

## 相關文件

- [決策 0009：RocksDB 權威世界快照](0009-rocksdb-authoritative-world-snapshots.md)
- [決策 0015：Bevy 原生 Y-up 世界座標](0015-bevy-native-y-up-world-coordinates.md)
- [決策 0020：語義註冊與內容選擇](0020-semantic-registration-and-content-selection.md)
- [決策 0023：首版 SDK、Registration 與語義編譯契約](0023-freeze-sdk-registration-and-semantic-compilation.md)
- [決策 0026：第一個 Demo 性能預算](0026-freeze-first-demo-performance-budgets.md)
- [決策 0028：世界生成、內容與資產契約](0028-freeze-worldgen-content-and-asset-contract.md)
- [世界持久化](../architecture/world-persistence.md)
- [World 目錄、開始頁與安全生命週期](../architecture/world-lifecycle-and-start-ui.md)
- [套件、ABI、Bevy 與持久化相容性](../architecture/versioning-and-compatibility.md)
- [執行期路線圖](../planning/roadmap-game-engine.md)
- [第一個可玩 demo 路線圖](../planning/roadmap-first-demo.md)
- [待決問題](../planning/open-questions.md)
