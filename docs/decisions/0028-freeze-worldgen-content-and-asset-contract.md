---
title: 凍結第一版世界生成、內容與資產契約
status: accepted
type: decision
updated: 2026-08-20
---

# 決策 0028：凍結第一版世界生成、內容與資產契約

## 背景

[領地優先世界生成](0001-territory-first-biome-driven-world-generation.md)、
[混合洞穴組合](0002-hybrid-cave-generation-composition.md)、
[領地遞迴委派](0004-territorial-delegation-for-spatial-generation.md)、
[語義註冊與內容選擇](0020-semantic-registration-and-content-selection.md)已決定長期方向，
但第一個可玩 demo 仍缺少足以直接實作的最小契約：誰協調生成、D4 的兩種真實
terrain／biome style 是什麼、seed 與 provider identity 如何進入 hash、舊／新 generation
epoch 如何相鄰、72 個方塊資料何時完整、流體如何持久化，以及 Bevy asset dependency
如何回到 package owner 診斷。

若這些問題留給各 crate 自行決定，chunk bytes 會依 package／task 順序漂移，content schema
會退化為無型別 properties bag，presentation reload 可能偷改 authoritative world，而 package
kernel 也可能複製 Bevy 的 asset dependency graph。本決策凍結 D4、D7、D8 與 D9 使用的第一版
邊界；它不宣稱完整 territory、通用材料 ontology、進階流體或模型創作工具已完成。

本決策中的 schema、hash、matrix 與 fixture 是 normative。若較早的 exploration 文件或示例
與本決策衝突，以本決策為準。`accepted` 表示契約已決定，不表示對應實作與自動證據已交付。

## 決策驅動因素

- 相同 seed、canonical config、locked semantic image 與 provider identity 必須在不同 chunk
  順序、task 平行度、registration 順序與重啟後產生相同 authoritative bytes。
- 已物化 snapshot 不因 generator、package 或 semantic catalog 更新而被隱式重算。
- D4 必須用兩個真實內容組合壓力測試公開 generation contract，但不能假裝已完成 D7
  Territory Atlas。
- block、item、biome 與 fluid 必須是 typed、versioned、owner-aware 資料；identity、definition、
  semantic contribution、instance state 與 gameplay rule 不得混成一個 bag。
- Bevy 繼續擁有 App、task、asset loader、handle、runtime dependency 與 hot reload；Lattice Axiom
  只增加 package ownership、stable identity、authoritative policy 與可行動診斷。
- 所有出場條件必須能由 headless／command-input fixture 自動重現，不依賴視覺或人工測試。

## 問題映射

| 問題 | 決策章節 |
| --- | --- |
| `GEN-01` | 第 1 節 |
| `GEN-02` | 第 2 節 |
| `GEN-03` | 第 3 節 |
| `GEN-04` | 第 4 節 |
| `GEN-05` | 第 5 節 |
| `GEN-06` | 第 6 節 |
| `CONTENT-01`、`CONTENT-02` | 第 7 節 |
| `CONTENT-03`、`CONTENT-04` | 第 8 節 |
| `CONTENT-05` | 第 9 節 |
| `CONTENT-06` | 第 10 節 |
| `CONTENT-07` | 第 11 節 |
| `CONTENT-08` | 第 12 節 |
| `CONTENT-09`、`CONTENT-10` | 第 13 節 |

## 決策

### 1. 每維度一個協調器，提供者按通道與領地擁有

每個 active dimension 必須解析出恰好一個 `generation.coordinator@1`。它在進入 `Playing`
以前，從 validated content／semantic catalog、frozen Role bindings、`WorldSeedV1`、
`WorldgenConfigV1` 與 provider manifests 編譯 immutable `GenerationPlanV1`。

協調器是依賴、所有權、預算與邊界的唯一編譯者，不是唯一 terrain 或 cave 演算法：

- `terrain.base` 在每個 `channel × territory ownership domain` 恰好一個主要 owner；
- `cave.topology` 在每個 topology ownership domain 恰好一個主要 owner；
- 更具體的 child territory 可以接管父域，否則繼承父域或 dimension default；
- detail、branch、destination 與 constraint 可以有多個 contributor，但每個 channel 必須指定
  typed compositor／arbiter、最大空間 influence 與硬 budget；
- 同域同層的兩個主要 owner、未知 compositor、無界 influence 或超出 budget 都在 module code
  與 world writer 啟動前失敗；不以 discovery、load、link 或 callback 順序解決。

chunk 熱路徑只查詢 `GenerationPlanV1` 內已解析的 numeric provider、typed channel plan 與空間
索引，不掃描 package、Nickel record 或原始 manifest。

### 2. D4 固定兩種真實 style，但不冒充完整 Territory Atlas

D4 固定下列兩個 Terrenia style identity：

| Style | 真實內容組合 |
| --- | --- |
| `terrenia:biome/temperate-woodland` | grass／dirt 表面與淺層、stone／limestone、oak family、clay／gravel |
| `terrenia:biome/arid-badlands` | sand／red-sand、sandstone／red-sandstone、basalt 與 copper 資源 |

精確 block identity 來自 D4 累計 18 個 Terrenia definitions，不從上述顯示名稱或 ID path
推導。兩種 style：

1. 實作相同的 `terrain.base` contract；
2. 由具有 stable identity、closed config 與 `algorithm_revision` 的 simple style selector 選擇；
3. 由另一個具 stable identity／revision／width 的 deterministic named transition 連接；
4. 共用 D4 minimal cave plan／void materializer、final occupancy 與 Role-driven materializer；
5. 以 Predicate 接受既有內容，以 frozen Role 取得要寫入 chunk 的 concrete block ID。

D4 selector 可以先使用 coarse deterministic cells 或等價的簡單純函式，但必須實作與 D7 相同的
`TerritoryQuery` result shape：stable domain ID、winner、runner-up、boundary distance、transition
metadata 與 provenance。D4 不要求三尺度 Atlas、水文、地質或完整 cave topology。

本 ADR 不猜測 frequency、amplitude、threshold 或 cave 數值。這些值由 D4 golden fixture 中的
closed canonical config 與對應 `algorithm_revision` 凍結；同一 revision 下不得改值或改輸出。

### 3. `WorldSeedV1`、canonical config 與兩種 hash

`WorldSeedV1` 恰好是 32 bytes。建立方式固定為：

- random seed：由作業系統 CSPRNG 產生 32 bytes，建立 world 時立即保存原始 bytes；
- integer seed：接受 canonical signed 64-bit decimal，去除輸入格式差異後計算
  `SHA-256("latticeaxiom.world-seed.integer.v1\0" || canonical_decimal_utf8)`；
- text seed：先驗證 UTF-8 並正規化為 Unicode NFC，再計算
  `SHA-256("latticeaxiom.world-seed.text.v1\0" || normalized_utf8)`。

integer canonical form 不含 `+`、前導零或負零，只有零本身寫作 `0`。text 與 integer namespace
分離，因此文字 `"42"` 與整數 `42` 不得得到相同 seed。UI 可以顯示 hex 或原始 author input，
但 generator 只取得已保存的 32 bytes。

`WorldgenConfigV1` 是 closed typed record：未知欄位失敗；所有 default 在 hashing 前 materialize；
map 依 canonical key 排序；enum 使用固定 discriminant；StableId 使用 canonical bytes。v1
output-affecting config 只使用 bounded integer、boolean、enum、StableId、fixed-point／有理數與
bounded list，不接受 NaN、infinity、locale number 或未排序 set。若後續真的需要 floating-point
author intent，必須先定義 canonical bits、範圍與跨 target determinism，再升 config schema。

每個 output-affecting provider identity 為：

```text
ProviderGenerationIdentityV1 {
  provider_stable_id,
  contract_major,
  algorithm_revision,
  implementation_fingerprint,
}
```

`algorithm_revision` 是 owner 單調管理的非負整數；`implementation_fingerprint` 是 manifest／
artifact 驗證過的 SHA-256。相同 provider／contract／revision 出現不同 fingerprint 時 fail closed，
不能宣稱兩者相容。static 與 portable realization 若來自同一業務實作，必須宣告並驗證相同
generation identity；realization 差異不得改 authoritative output。

兩種 hash 分開：

```text
GenerationInputHashV1 = SHA-256(
  "latticeaxiom.generation-input.v1\0" ||
  world_seed || canonical_config || generation_plan_revision ||
  provider_generation_identities || authoritative_semantic_receipt || role_bindings
)

GenerationProvenanceHashV1 = SHA-256(
  "latticeaxiom.generation-provenance.v1\0" ||
  generation_input_hash || locked package/source/artifact/realization receipts
)
```

package SemVer、source path 與 realization 只進 provenance；它們不直接成為 PRNG salt。真正改變
輸出的 config、semantic binding、algorithm revision 或 implementation fingerprint 已經進 input
hash。等價 static／portable realization應得到相同 input hash 與 chunk bytes，provenance仍可誠實
記錄不同 artifact。

### 4. D4 最小 generation DAG

D4 的 normative dependency order 為：

```text
validated ContentCatalog / SemanticCatalog / frozen Role bindings
  + WorldSeedV1 / WorldgenConfigV1
        ↓
simple two-style plan + named transition plan
        ↓
terrain height / density intent
        ↓
minimal coarse cave plan + stable shared-face keys
        ↓
bounded cave SDF / void contribution
        ↓
final solid occupancy
        ↓
surface / soil / stone materialization through frozen Roles
        ↓
bounded resource / vegetation point procedures
        ↓
ChunkDraftV1 + artifact dependency receipt
        ↓
snapshot-first atomic materialization
```

material／soil placement 必須在 cave／void 已形成 final occupancy 後執行，不能先填 block 再讓 cave
讀取或覆蓋鄰居的已寫 block array。cave/shared-face key 只依 seed、plan、canonical coordinates、
domain identity 與 declared dependencies；不得要求 neighbor chunk 已生成。

每個 node 聲明 typed input／output、owner、revision、bounds、budget 與 compositor。獨立 node 可以由
Bevy task pools 平行執行；依賴 edge 不能靠 system registration order 或 exclusive `World` access
暗示。task completion 必須帶 instance／plan／chunk revision，stale result 不得覆蓋新 revision。

`ChunkDraftV1` 完整驗證後與 generation receipt 原子寫入 snapshot，成功 durable publish 後才成為
authoritative working set。crash 不能留下對玩家可見但沒有 canonical snapshot 的半物化 chunk。

### 5. Generation epoch 以 planning cell 凍結，邊界有明確 receipt

`GenerationEpochIdV1` 為：

```text
SHA-256(
  "latticeaxiom.generation-epoch.v1\0" ||
  generation_plan_revision || authoritative_semantic_receipt ||
  worldgen_config_hash || provider_generation_identities
)
```

coarse planning cell 在第一次 materialization 前選定 epoch；第一次 durable materialization 後即
frozen。cell 尺寸、grid origin 與 coordinate encoding 屬於 `GenerationPlanV1` closed config。
generator 更新只可供尚未物化的新 cell 使用，或經顯式 clone／migration／regenerate workflow；
不得因新 package、Tag 成員或 Role candidate 出現而重寫既有 cell。

不同 epoch 相鄰時，writer 必須先建立並驗證：

```text
BoundaryReceiptV1 {
  boundary_id,
  epoch_a,
  epoch_b,
  adapter_id,
  adapter_version,
  adapter_hash,
  transition_width,
  terrain_boundary_signature,
  required_cave_portals,
}
```

`boundary_id` 由 canonical shared-boundary coordinate 與相鄰 epoch identity 建立，與查詢方向無關。
adapter 必須具有有限 width／work／memory、確定性 terrain signature 與每個 required portal 的位置、
tangent、clearance、fluid state contract。缺少 adapter、budget 超限、signature 不相容或 portal 驗證
失敗時，在打開該 cell writer 前拒絕；不得先寫一半再以 warning 繼續。

boundary fixture 必須證明 E0 cell A 先 durable、E1 cell B 後生成時，A 的 snapshot bytes、revision、
checksum 與 writer count 完全不變。epoch 是局部生成 provenance，不是 global world version。

### 6. Full territory prototype 的時程與 D7 gate

full `(x, z)` Territory Atlas 是 pure-data、headless prototype，可以與 D2／D3 並行，但不能阻塞
D0–D6。D4 使用前述 simple two-style provider；它實作相同 `TerritoryQuery` shape，卻不宣稱具有
full Atlas 的尺度、統計與 authoring 能力。

D7 開始前必須凍結 full prototype schema／algorithm revision，並把下列項目升為 hard gate：

- 至少三個尺度的 territory hierarchy；
- 1,000 個 synthetic biome candidates 與數十個 content packs；
- discovery／registration／query／task order 隨機化後 winner、runner-up、boundary 與 bytes 相同；
- boundary distance、transition width、territory area／fragmentation／connectivity 統計；
- cold／cached query、memory、parallel scaling 與 bounded-work machine-readable baseline；
- child delegation、same-level conflict、boundary adapter 與 diagnostics corpus。

具體 performance threshold 由 D7 target hardware baseline 與 roadmap budget 凍結；本 ADR 不猜測
尚未量測的數字。D7 gate 失敗不能由 D11 biome expansion 隱藏補救。

### 7. Content definition schema 與 72-row authoring evidence

所有 definition 共用：

```text
ContentHeaderV1 {
  stable_id,
  schema_id_and_major,
  content_revision,
  declared_by_package,
}
```

`declared_by_package` 由 RegistrationManifest／namespace grant 驗證，不信任 asset 自我聲明。
display name 使用 stable localization key；實際 locale text、texture、audio 與純視覺 material 進
presentation binding，不因此改 authoritative world hash。

`BlockDefinitionV1` 只保存 block 本體契約：

- header；
- supported versioned state keys、每個 key 的 allowed subset、default 與 canonical encoding；
- solid occupancy、collision shape kind、selection shape kind、occlusion、replaceability 與
  fluid-occupancy policy；
- authoritative rule references：mining rule、drop table、placement item；
- optional presentation binding stable asset ID。

`ItemDefinitionV1` 只保存 header、non-zero bounded stack limit、placement/use action reference、
optional durability／persistent component schema reference 與 presentation binding。recipe、fuel、tool
class／tier 與 drop rule 是獨立 typed registrations，透過 exact ID、Tag、Predicate 與 Role 引用 item，
不塞成每個 item 都有的 optional bag。

`BiomeDefinitionV1` 保存 header、scope、spatial influence、territory participation、boundary contract、
provided channel offers、typed emitted intents、fallback 與 generation revision。它不是 temperature／
humidity 分類標籤；新 emit channel 必須有 owner、value schema、compositor 與 budget。

Tag／Map contribution、Role offer、worldgen placement、drop／tool／recipe rule 與 presentation binding
是獨立 rows。definition identity 不由 asset path、package name或顯示名稱推導，semantic contribution
也不自動合併 exact identity。

D8 自動出場證據必須包含一份 canonical、machine-validatable Terrenia 72-row authoring matrix。
每行至少包含：

```text
block stable ID / content revision / localization key / presentation asset ID
intrinsic block definition / physical-map contributions / state schema
mining rule / drop table / tool predicate / placement item / recipe references
semantic contributions / Role offers / worldgen Predicate and Role references
```

matrix 必須精確等於 [Terrenia 方塊內容規劃](../planning/terrenia-block-catalog.md)的 72 個 ID，
並同時驗證兩個 fluid definitions 與相鄰 item／drop／tool／recipe references。D8 evidence 把完整
authoring 與 gameplay schema 壓力提前；它不表示 D8 已把 72 個 block 全部註冊到 shipped profile。
D9 才要求 active manifest 精確得到 72 blocks + 2 fluids 並通過完整玩法／持久化門禁。

matrix 中的 v1 display binding、drop、tool requirement 與 balance value 是該 `content_revision` 的
normative first-version values。材料族可以用 authoring default 生成，但 lock／manifest必須保存展開後
的 canonical value；runtime 不查詢隱式 family default。調平不是永遠「最終」，任何 output-affecting
改值都提升 owner content revision並進 authoritative image／world compatibility。

### 8. 首批 semantic vocabulary、physical data 與 shared state

首批平台 material vocabulary 只凍結已有真實 D3 consumer／fixture 的三個 contract：

```text
latticeaxiom:block-tag/storage-blocks/copper@1
latticeaxiom:block-tag/obsidians/normal@1
latticeaxiom:block-role/storage-block/copper@1
```

`latticeaxiom:item-tag/ingots/copper@1` 只有在 D8 item／recipe consumer 與至少兩個 compatible item
fixtures 存在後才加入 shipped vocabulary。Terrenia 的 empty／terrain surface／subsurface／stone Role
與 portal-frame semantics 保持 `terrenia:*` domain contract，不提升為 platform default。

新增平台 Tag／Map／Role 必須同時具有：明確 owner、target kind、authoritative／presentation domain、
versioned meaning、extensibility／cardinality／conflict policy、至少一個真實 consumer，以及至少兩個
不同 exact target 的 compatibility fixture。未滿足時使用 exact ID、definition-scoped state 或 domain
private contract；不建立 generic wood／stone／metal ontology、字串相似推理或自動 alias。

`latticeaxiom:block-map/physical@1` 在既有文件中只是候選示例，本決策不以 accepted `@1` 凍結
`{ hardness, blast_resistance, friction }` bag：

- `blast_resistance` 沒有 D0–D10 consumer，不進 v1；
- mining hardness 在 D8 以 narrow typed mining map／rule由真實 break-progress consumer 凍結；
- surface friction 在 voxel collider／physics contact prototype 證明資料流後以 narrow typed map 凍結；
- collision、selection、occlusion、replaceability 與 fluid occupancy 是 intrinsic block definition，
  不重複放進 physical map；
- support 若只有 placement／structure consumer，使用獨立 typed contract，不塞進通用 physical bag。

D3 的「一個 numeric typed Map」可以使用 fixture-owned narrow Map 證明編譯器，不得反過來把測試
欄位宣稱為平台 ontology。

首批跨 package shared block state keys 為：

```text
latticeaxiom:block-state/axis@1   = enum(x, y, z)
latticeaxiom:block-state/facing@1 = enum(east, west, up, down, south, north)
latticeaxiom:block-state/lit@1    = bool
```

方向語義遵守 `east(+X)`、`west(-X)`、`up(+Y)`、`down(-Y)`、`south(+Z)`、`north(-Z)`。
每個 definition 宣告 allowed subset，不能假設所有 facing 都合法。`growth`、`half` 與 `form` 可以先是
definition-scoped state；只有兩個真實跨 package consumers需要相同 meaning時才提升為 public stable key。
`form` 尤其必須由 mesh、collision 與 placement prototype共同證明；否則拆成 exact content identity。

state 組合保持有限；palette 保存 concrete block ID + canonical state，不為 axis、facing、lit、growth、
half、form 或 fluid level 產生新的 StableId。接近無界資料進 block entity／persistent component schema。

### 9. Fluid definition、state 與雙層 palette

D9 的兩個 exact fluid identity 為 `terrenia:fluid/water` 與 `terrenia:fluid/lava`。它們共用：

```text
FluidDefinitionV1 {
  ContentHeaderV1,
  state_schema = FluidStateV1,
  replace_or_displace_predicate,
  collision_policy,
  selection_policy,
  fixed_tick_period,
  bounded_update_policy,
  optional presentation_binding,
}

FluidStateV1 {
  level: u8 in 0..=7,
  flow: Still | Down | East | West | South | North,
}
```

`level = 0` 表示 full/source level，`1..=7` 表示逐級降低的量；`flow` 是有限 authoritative state，
不由 renderer 臨時猜測。v1 不支援 upward／diagonal flow、pressure simulation、無界 propagation 或
fluid mixing。water／lava 的 tick period、update budget、damage／interaction rule 與 presentation value
由兩行 canonical fluid matrix凍結並提升 content revision後才能改變。

每個 voxel cell 使用兩個正交 layer，而不是把 fluid 變成 block variant：

```text
solid palette entry = (BlockStableId, canonical BlockState)
fluid palette entry = Empty | (FluidStableId, FluidStateV1)
cell                 = (solid_palette_index, fluid_palette_index)
```

fluid palette index 0 固定為 `Empty`。兩個 palette entry 依 stable ID + canonical state bytes 排序，
snapshot 保存 stable identity；process-local numeric ID、Bevy handle 與 plugin voxel ID 不落盤。
bit width、cell traversal order 與 envelope version 由 snapshot schema owner固定並以 golden bytes驗證。

D9 v1 fluid 只可與 `terrenia:block/air` 的 solid layer 共存；遇到其他 replaceable block 時先產生
authoritative replace command，再寫入 `(air, fluid)`，其餘 solid 拒絕 fluid。未來 waterlogging／
coexistence 可擴充 block fluid-occupancy policy，而無需把 fluid 改回 block ID。

fluid frontier、scheduled tick 與必要 RNG／revision 另存 versioned simulation continuation，不編進每個
cell state。unknown fluid/state/snapshot schema 在 writer 打開前拒絕或進入明確 read-only recovery。

### 10. Data package、typed asset 與 code package

data 與 code 是 logical package 的責任／realization；typed asset 是 owner package內由 Bevy loader
讀取的 versioned artifact，不是第三種 package identity或第二個 dependency graph。

- Nickel／RegistrationManifest 擁有 package graph、StableId、schema owner、semantic intent 與 asset
  references；
- data realization 提供 validated definitions、tables 與 asset entries，不載入 business code；
- 大型 structured content 可以保存在 typed asset，由 build tool驗證後生成唯一 manifest fragment；
  同一欄位不得同時在 `package.ncl` 與 typed asset 各有一份 source of truth；
- code package 提供 algorithm／system、capability、read／write set 與 schema migration，經 stable ref
  消費 data，不複製 content identity；
- presentation package提供texture、material、audio、animation與locale data，headless可省略且不得改
  authoritative registration／world hash。

authoritative typed asset 的 schema、source hash、artifact hash 與 availability 進 lock／preflight；
presentation-only asset 進分離的 presentation fingerprint。

### 11. Asset ownership 與 Bevy dependency 的雙向診斷

每個 package asset registration 至少包含：

```text
AssetRegistrationV1 {
  stable_asset_id,
  declared_by_package,
  package_version,
  package_relative_path_and_optional_label,
  asset_type_and_schema,
  authority,
  reload_impact,
  source_hash,
  artifact_hash,
  profile_availability,
}
```

`RegistrationImage` 建立 `StableAssetId → owner/artifact/path/type/policy`；artifact manifest建立每個
packaged path／label到 owner、hash 的反向索引；host asset adapter在目前 EngineInstance維護
`AssetPath／UntypedAssetId／Handle → stable root references`。Bevy `AssetServer` 仍是 runtime
dependency、load state、loader 與 hot reload 的唯一 owner；Lattice index只附加 package provenance，
不得再算一份依賴圖。

跨 package asset dependency 必須同時是 logical package dependency 與 stable asset reference，禁止
以 `../`、絕對路徑、source root重疊或 longest-path猜 owner。失敗診斷必須能輸出：

```text
content reference
  → stable asset ID
  → owner package/version/artifact hash
  → package-relative path#label and expected type
  → Bevy loader/load state
  → failed dependency
  → reverse dependent stable roots
```

首批 stable diagnostic codes 為：

```text
asset.unknown_stable_id
asset.owner_mismatch
asset.unregistered_path
asset.cross_package_dependency_missing
asset.source_hash_mismatch
asset.artifact_hash_mismatch
asset.loader_unavailable
asset.type_mismatch
asset.dependency_failed
asset.authoritative_reload_blocked
asset.sidecar_mismatch
```

authoritative asset缺失／hash錯誤在 `Playing` 與 writer 前失敗；presentation asset只有manifest明確
允許 placeholder時才可降級，並保留owner-aware diagnostic。

### 12. Hot reload 依 authority 與 dependency closure 決定

每個 asset明確宣告：

```text
authority   = presentation | authoritative
reload_impact = presentation_rebuild | world_reactivate | recompose_or_restart
```

reload planner沿 Bevy 回報的 dependency closure取最大 impact；不能按副檔名推斷。texture、純視覺
material／shader、animation、audio與locale data可以重建Bevy asset、chunk mesh或render cache，前提是
collision／selection／gameplay不由該資料派生，且world hash不變。

下列資料一律 authoritative：collision、selection、可通行性、gameplay socket／hit zone、block／state
schema、authoritative Tag／Map、drop、tool、recipe、worldgen、fluid definition／state／simulation。
變更需要新content／generator／semantic image revision；在 writable world內拒絕原地套用，只能經
world reactivate、new session、顯式 migration或recompose／restart。

同一檔案混合presentation與authoritative語義時應拆成visual asset + authoritative sidecar；無法拆分就
把整個dependency closure視為authoritative。client不得以本地reload改變server/headless判定。

### 13. Blender、glTF、sidecar 與 package schema

第一版只承諾 glTF／Bevy正常保存與載入mesh、material、skeleton、animation、node name與transform。
named empty可承載socket transform，普通proxy mesh可承載collider geometry，但glTF／Bevy不自動賦予
它們socket、collider或semantic tag的Lattice meaning。runtime不得從任意名稱前綴猜authoritative
behavior。

`AssetSemanticSidecarV1` 最小 envelope 為：

```text
AssetSemanticSidecarV1 {
  schema_id_and_major,
  target_stable_asset_id,
  target_source_hash,
  coordinate_schema = bevy-y-up-meters@1,
  bindings: sorted [
    stable_semantic_name,
    target_node_name,
    kind,
    typed_payload,
  ],
}
```

`kind` 首批只允許真實 asset-local用途：socket／attachment、hit zone、collider proxy、selection proxy、
physical／simulation region與animation event anchor。node name在target asset內必須唯一；sidecar不保存
glTF array index、import order、Bevy `Entity`／`Handle`或GPU／physics handle。target hash不符、node
缺失／重複、單位／軸向不符與unknown payload major都fail closed。

mesh、material、skeleton、animation與node transform留在glTF；asset-local stable mapping留在sidecar；
跨asset／跨package或影響authoritative world的StableId、owner、Tag／Map／Role、collision／gameplay
contract、reload policy與migration留在package schema。glTF extras可以是authoring input，但只有經
versioned importer canonicalize並驗證後的typed sidecar／manifest才是runtime contract。

Blender exporter、export config、Bevy/glTF loader與sidecar importer版本都進fixture receipt。正式採用
任何命名／extras convention前，command-only、無window／GPU fixture必須驗證socket、collider proxy、
custom property、Y-up、meters、animation、node identity與semantic round-trip。這項fixture不要求視覺
人工驗收。

## 延後與升級 Gate

下列項目不在本決策建立：

- D4 的完整 Territory Atlas、水文、地質、侵蝕、完整 cave topology與D11群系擴充；
- 超過首批 copper／obsidian consumer 的平台材料 ontology或自動語義推理；
- 沒有兩個真實consumer的composite `physical@1`、public `growth`／`half`／`form` contract；
- diagonal／upward／pressure fluid、fluid mixing、無界simulation或一般waterlogging；
- 自有 asset dependency graph、native asset hot unload、任意 authoritative live reload；
- 自訂 Khronos glTF extension、完整editor、Godot bridge與Bevy↔DCC即時同步；
- deformable physical fields／soft-body通用schema；
- 將一次人工調平宣稱為永遠不變的最終balance。

新增項目必須有真實player／author／package consumer、versioned schema owner、bounded runtime cost、
malformed／migration／determinism fixtures，並遵守 Bevy upstream-first gate。Godot 只有在固定 Blender +
glTF + sidecar baseline被量測證明無法完成重複工作流時才可作限時對照。

## 自動驗證

### World generation

- coordinator missing／duplicate、同domain主要owner衝突、unknown compositor、unbounded influence與budget
  超限全部在 `Playing`／writer前失敗並列出package provenance。
- integer／text/random seed有known-answer vectors；integer `42`與text `"42"`不同；等值config reorder／
  explicit default得到相同bytes，unknown field與invalid numeric失敗。
- 相同provider revision但不同implementation fingerprint失敗；只改等價realization或package SemVer時
  output不被當成PRNG salt，provenance仍記錄差異。
- D4 fixed seed／config／lock在provider order、chunk order、task parallelism與restart隨機化後得到逐byte
  相同style plan、transition、ChunkDraft、receipt與snapshot。
- temperate woodland與arid badlands golden覆蓋各自surface／subsurface／rock／resource／vegetation Role，
  並跨一條named transition；移除任一required Role時fail closed。
- shared-face cave fixture從任一側、任一chunk順序得到相同key、portal與SDF；generator不讀neighbor已寫
  block array。
- E0先materialize A、E1後materialize B的fixture驗證boundary receipt且A的bytes／revision／checksum／
  writer count為零變更；missing adapter、portal mismatch與fault injection均零部分寫入。
- D7以前保留三尺度、1,000 candidates、boundary statistics與cold／cached／parallel performance corpus；
  D7出場時它們成為hard gate。

### Content and fluids

- block／item／biome／fluid schema有canonical round-trip、golden bytes、unknown field、range、wrong kind、
  owner/grant與old-schema migration fixtures。
- D8 matrix精確有72個unique block IDs、2個fluid IDs與完整refs；catalog漏項、多項、state展開成ID、
  missing asset／drop／tool／recipe／Role／Predicate均失敗。
- copper Tag接受兩個不同exact IDs而Role輸出在world command前成concrete ID；normal obsidian Tag不自動
 取得Terrenia portal行為；非Terrenia fixture維度不需host special case。
- shared axis／facing／lit encoding與每definition allowed subset有golden；未通過consumer gate的state不會
 偷偷成為platform contract。
- D9 manifest精確註冊72 blocks + water／lava；solid＋fluid palette在正負Y chunk、chunk邊界、卸載／
  重載、crash atomicity與schema migration中保持canonical。
- fluid level／flow、per-tick cell、queue depth與in-flight bytes有硬上限；stale fluid task不覆蓋新chunk
  revision；unknown schema在writer前拒絕或只讀恢復。

### Assets

- stable asset→owner/path/hash/type與Bevy path/load-state→stable roots雙向查詢round-trip；cross-package缺少
  dependency、path escape、wrong label/type/hash與transitive dependency failure產生固定diagnostic code。
- static／portable與client／headless解析相同authoritative asset IDs、owner、hash與content definition；
  headless省略presentation不改authoritative registration或world hash。
- presentation texture/material reload只重建derived presentation且world hash不變；把同一變更標記或傳遞
  到collision／gameplay dependency時，writable world得到`asset.authoritative_reload_blocked`。
- glTF fixture由固定export receipt載入named socket、collider proxy與custom property，驗證Y-up、meter、
  animation、unique node mapping與sidecar hash；malformed、stale hash、missing／duplicate node失敗。

所有測試可使用command-input、headless Bevy standard profile與fixture assets；CI不需要window、GPU或人工
視覺判斷。performance與queue high-water寫入machine-readable baseline，不以文字「看起來足夠快」代替。

## 取捨

- D8 必須提早完成72-row authoring matrix，增加前期資料工作，但讓D9不再一邊啟用內容一邊發明schema。
- generation receipt、epoch boundary與asset provenance增加metadata和diagnostic實作成本，換取局部升級、
  crash recovery與可重現性。
- simple D4 style與full D7 territory共用query shape，會要求一個窄adapter，但避免D4過度建設或D7全面
  換介面。
- presentation／authoritative分離可能要求拆asset；這是安全hot reload與headless equivalence的必要成本。
- 本決策刻意不凍結猜測性的terrain數字、通用physical bag與進階sidecar semantic；真實consumer與
  golden fixture先於新public contract。

## 被否決的方案

### 把 generator 當成每維度唯一演算法

它無法讓terrain、cave、detail與constraint以typed channel組合，也會迫使群系以last-writer覆蓋。
唯一的是coordinator與每channel×domain主要owner，不是全部演算法。

### 讓 D4 先完成 full Territory Atlas

這會阻塞第一個可玩vertical slice，且在沒有D7統計／性能壓力前過度凍結。D4只證明兩style、transition
與相同query shape；full Atlas在D7前成為hard gate。

### 用 package SemVer、load order或global game version作seed salt

無關更新會改寫新chunk，static／portable也難以等價。只有canonical output inputs進generation hash；
package與artifact座標保留在provenance。

### generator更新後重生舊chunk或直接混寫epoch

這會破壞玩家修改與durability。舊cell保持snapshot權威，新cell以versioned boundary adapter連接，失敗時
在writer前拒絕。

### 一個無型別content／physical properties bag

它沒有owner、target kind、merge、version、canonical encoding與consumer gate；最終只能靠字串與load
order。definition、semantic、state、gameplay rule與asset binding保持分離。

### 把fluid level展開成block ID或只保存process numeric ID

它會放大StableId、阻擋solid＋fluid共存並破壞跨重啟identity。snapshot保存兩個stable palette與有限
state。

### Package kernel複製Bevy asset dependency graph

兩份load state與hot reload真相必然漂移。Lattice只維護stable owner／artifact索引並為Bevy graph附加
provenance。

### 只靠glTF node命名承載authoritative semantic

名稱可作authoring convention，但沒有schema major、owner、target hash與migration。authoritative或跨asset
meaning必須進typed sidecar／package schema。

## 相關文件

- [可組合世界生成架構](../architecture/world-generation.md)
- [可組合洞穴生成架構](../architecture/cave-generation.md)
- [世界持久化與 RocksDB World Store](../architecture/world-persistence.md)
- [語義註冊、內容判定與候選選擇](../architecture/semantic-registration.md)
- [Bevy 資產管線與模型語義](../architecture/asset-semantics.md)
- [Demo workspace 與 Terrenia 維度套件組織](../architecture/demo-workspace-organization.md)
- [Terrenia 方塊內容規劃](../planning/terrenia-block-catalog.md)
- [第一個可玩 demo 路線圖](../planning/roadmap-first-demo.md)
- [套件驅動的 Bevy 執行期整合路線](../planning/roadmap-game-engine.md)
