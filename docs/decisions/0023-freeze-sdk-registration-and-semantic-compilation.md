---
title: 凍結首版 SDK、Registration 與語義編譯契約
status: accepted
type: decision
updated: 2026-08-20
---

# 決策 0023：凍結首版 SDK、Registration 與語義編譯契約

## 背景

[決策 0008](0008-static-and-dynamic-realizations-share-one-graph.md)已經要求 static 與
dynamic realization 共用邏輯 package graph、registration semantics 與單一業務定義；
[決策 0020](0020-semantic-registration-and-content-selection.md)則已經凍結 exact identity、
Tag、Map、StateProperty、Affordance、Predicate、Role 與 fallback ContentBundle 的分層。

R2／D1 開始實作前，仍有十六項會讓 SDK、manifest、numeric layout、Bevy adapter、
semantic hot path 或 world reopen 產生兩份真相的問題未定。若把這些問題留給各 crate 自行選擇，
最直接的結果會是：

- macro 與 builder 各自形成 authoring API；
- static system 與 dynamic callback 手寫兩套業務邏輯；
- process-local Bevy ID 或 producer metadata 混入 registration equivalence；
- numeric ID、schedule 與 access validation 依 registration／load order 漂移；
- Tag／Map／Predicate 的 runtime layout 無界，或在 tick 內保留字串與 `serde_json::Value`；
- active bundle、Role binding 與 resolution explanation 在 lock、world metadata 和 runtime image
  之間成為互相不一致的權威副本；
- 尚無真實 consumer 的 custom merger 與 Affordance callback 提前擴張 ABI。

本決策只凍結第一個可玩 vertical slice 所需的首版 contract。它不把尚未出現的通用 SDK、
任意 Bevy system parameter、任意 dynamic Rust type 或一般 semantic rule engine 提前設計完成。

## 核心不變條件

1. Rust serde model 與同版本 positive／negative golden corpus 繼續是 machine contract 的
   normative source；Nickel contract、SDK codegen 與 schema 文件共同驗證同一 corpus。
2. Nickel 是 pure-data registration、semantic intent、profile binding 與 overlay 的唯一公共組合入口。
   Rust builder 不得成為第二套 package／semantic composition language。
3. code-bound schema、system 與未來 callback 由受控 SDK 產生 manifest fragment；作者不手寫
   static／dynamic 兩份 registration 或業務函數。
4. Bevy 是唯一 App、ECS 與 scheduler。static adapter 直接使用 typed Bevy API；dynamic adapter
   只使用 Lattice-owned ABI、stable IDs、validated POD batches 與 command／message sinks。
5. `TypeId`、`ComponentId`、`Entity`、`Handle`、function address、pointer、allocator identity、
   dynamic-library base address與任意 process-local handle不進 manifest semantic hash、lock、save或ABI artifact。
6. 所有 graph、ID、schedule、semantic image 與 diagnostic 結果不依 source discovery、Cargo link、
   registration、archetype discovery 或 dynamic load order。
7. gameplay hot path 只消費已編譯 numeric tables、bitsets、packed values、predicate plans、直接函數
   或 bounded batch callbacks，不執行 Nickel、resolver、package 字串 lookup 或 JSON interpretation。
## 術語

| 術語 | 本決策中的唯一含義 |
| --- | --- |
| `SourceSpan` | 單一 source blob 內的 byte range；它本身不包含 `source_id`、logical path 或 origin chain。 |
| `SourceProvenance` | 完整來源位置與追溯物件，至少包含 `source_id`、logical path、`SourceSpan` 與 origin chain。實作若要強調這是帶來源的 span，可以把 concrete type 命名為 `ProvenancedSpan`，但 machine contract 的概念名稱仍是 `SourceProvenance`。 |

下文凡是要求完整來源、診斷位置或追溯鏈，一律指 `SourceProvenance`；不得再用
`SourceSpan` 指稱含 source identity 或 origin chain 的完整物件。


## 決策

### 1. `SDK-01`：Public proc macro 與 sealed typed IR 分層

首版 code-bound authoring 採用兩層：

1. 公共層是 SDK attribute／derive proc macros，用於 package、component、schema 與 system declaration；
2. 內部層是版本化、typed、sealed 的 `RegistrationIr` builder，供 macro expansion、build tooling
   與 conformance tests 共用。

普通 package 不直接使用底層 builder 拼出任意 registration graph。底層 IR 不提供 runtime global
registry、linker inventory、constructor side effect 或「掃描已連結 crate」的 discovery path。
`BuildPlan` 明文列出 generated fragment、static glue、shared-schema unit 與 dynamic binding artifact。

Macro 的具體標點與 ergonomic spelling 可在相同 contract major 內經 prototype 改善；以下輸出語義
是首版固定表面：

- canonical `RegistrationManifest` fragment；
- static Bevy registration／system glue；
- dynamic C header／Rust binding／batch shim；
- callback key 與 signature map；
- schema／layout descriptor與conformance fixture metadata；
- semantic hash input與producer provenance receipt。

### 2. `SDK-02`：單一 row kernel 生成 typed Query 與 batch shim

首版 dual-realization business surface 使用單一 **row kernel**。作者函數只接收 SDK 生成的 typed
component borrows、有限 context 與 sinks，不直接把任意 Bevy `Query` 寫進可攜業務簽名。

SDK 對同一函數生成：

- static wrapper：正常 Bevy system，以 typed `Query`／filter 迭代，直接呼叫並可 inline row kernel；
- dynamic wrapper：一次 callback 接收一個 system 的一批 archetype column views，在 module 內迭代
  rows 並呼叫同一 kernel。

因此 FFI 次數隨 system／batch，而不是隨 entity；static path 不先把 typed query 打包成 C ABI view。
需要跨 row reduction、排序或 bulk algorithm 的公共 batch-kernel API，只有第二個真實 consumer
證明 row kernel 不足後才新增。首版不嘗試以 proc macro 改寫任意 Bevy system body 成兩種語言。

### 3. `SDK-03`：Dual-realization system parameter 白名單

首版可 dual-realization 的參數只有：

- ABI-POD component 的 required／optional read 與 write row；
- ABI-POD component 的 `With`／`Without` filter；
- 代表目前 row entity 的 Lattice opaque／generational key；
- by-value fixed tick／delta context；
- SDK command sink；
- 已實作、manifest-declared 的 authoritative message sink／reader；
- 已實作 interface 提供的 read-only POD input slice。

以下一律標為 `NativeStaticOnly`：

- `World`／`App`／`RenderApp` access 或 exclusive system；
- 任意 custom `SystemParam`、`Local`、`NonSend`、`ParamSet` 或 change-detection filter；
- Bevy／Rust `Entity`、`Handle<T>`、`AssetServer`、task、gizmo、render-world type；
- 非 POD、需要 drop glue、allocator ownership 或 Rust type identity 的 dynamic component；
- 尚未進入 versioned ABI table 的 resource、event、message 或 service parameter。

明文要求 dual realization 的 system 遇到不支援參數時是 compile error。`auto` policy 可以產生
帶 stable reason code 的 `NativeStaticOnly` registration，但不得生成表面存在、實際不安全的 callback。

### 4. `SDK-04`：Component mode 與 typed schema dependency

Component 的穩定 schema 與目前 host 如何取得 Rust type 分開。首版分類如下：

- `HostTyped`：host／platform 已編譯 type，或只由同一 static implementation 私用的 type；
- `GeneratedSharedSchema`：有跨 package static typed consumer，或是公開 authoritative
  persisted／replicated component；
- `RuntimeDynamic`：compose 時才出現、沒有 static typed consumer、只需 batch／untyped access，
  且通過 ABI-POD validation 的 component。

任何出現在 public registration／manifest、`BuildPlan`、lock、save／world metadata、network
或 ABI contract 的 `SchemaId`，都必須是帶明文 `@<major>` suffix 的 canonical versioned ID。
只驗證 kind、卻接受缺少 major 的 parser 結果不符合本決策；typed conversion／manifest validation
必須在 canonicalisation 前以 stable diagnostic 拒絕它。`GeneratedSharedSchema` 與 `RuntimeDynamic`
因此一律使用 versioned `SchemaId`，破壞 field 意義、layout、persistence 或 ABI contract 的變更
必須配置新的 major。

唯一例外是 package-private、ephemeral 的 `HostTyped` type：generator 可以在單一 package 的 static
implementation 內使用不可觀察的 internal identity，但它不是 `SchemaId`，不得寫入 public manifest、
lock、save，亦不得跨 package、process、network、persistence 或 ABI 邊界。一旦該 type 需要跨越上述
任何邊界，owner 必須先為它配置帶 `@<major>` 的 public `SchemaId`；不得由 Rust type name、`TypeId`
或 generator discovery order 隱式導出跨邊界 identity。

Manifest 宣告 component contract 與允許的 access modes；實際 mode 是 `BuildPlan`／realization 的結果，
不是另一份 schema。切換 realization 不得改變 stable component ID、schema owner、layout contract 或
持久化意義。

`BuildPlan` 必須表達：

```text
TypedSchemaRequirement
├── consumer package
├── versioned schema ID (`@major`)
├── Rust API fingerprint
└── required access: read | write

SharedSchemaUnit
├── schema owner package / contract major
├── generator contract version
├── source model hash
├── artifact hash
└── consuming build units
```

Generated schema crate 按 schema owner package 與 contract major 分組，不為每個 field／component
建立一個 logical package。layout hash 相同不構成 Rust type identity；planner 必須加入相同
shared-schema unit、選擇相容 static realization、選擇明文 untyped adapter，或在 activation 前拒絕 graph。

### 5. `SDK-05`：RuntimeDynamic descriptor 的唯一 unsafe 邊界

Bevy `ComponentDescriptor::new_with_layout` 的 unsafe construction 隔離在專用 ECS／ABI bridge crate；
該 crate 可局部覆寫 workspace 的 `unsafe_code` lint，但每個 unsafe block 必須有 `// SAFETY:` 證明。
其他 crate 繼續 deny unsafe。

公開安全 API 先把 wire descriptor 驗證成不可自行構造的 `ValidatedRuntimePodDescriptor`。首版只接受：

- fixed-width scalar與由同一規則遞迴組成的 nested POD；
- table storage、`drop = None`、無 relationship／foreign clone callback；
- 無 pointer、Rust `bool`／`char`、niche-dependent enum、allocator ownership；
- non-zero size；size／offset／stride checked，size 是 alignment 的倍數；
- alignment 是二次冪且不超過 host policy ceiling；
- 明文 endianness、field offsets、size、alignment 與 layout hash。

stable component ID 到 Bevy `ComponentId` 的映射只存在於每個 `EngineInstance` 的 runtime map。
不同 `World`、registration order 或 process 可以得到不同 `ComponentId` 數值；任何測試與程式碼都不得
把該數值當作穩定 identity。

### 6. `SDK-06`：Manifest semantic、provenance 與 artifact hash 分離

不再以含糊的 `manifest_hash` 代理三種用途：

1. `registration_semantic_hash` 包含 manifest schema major、logical package／version、normalised
   registration rows、stable IDs、schema／layout、system callback key／signature／stage／access／ordering、
   capability、semantic、setting與domain-effective rows；
2. [決策 0021](0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md)定義的
   `provenance_hash` 另外包含 source／origin chain 與 producer／toolchain receipt；
3. artifact hash 與 callback-map hash 綁定實際 static／dynamic build output。

Canonical producer receipt 至少包含：

```text
ProducerReceipt
├── SDK stable ID / exact version
├── generator contract version
├── input fragment hash
└── generated fragment hash
```

Producer、realization、`SourceProvenance`（含其中的 `SourceSpan`）、toolchain、target、artifact、timestamp、absolute path 與 process-local
資料不進 `registration_semantic_hash`。static 與 dynamic realization 的 semantic hash 必須相同；
provenance 或 artifact receipt 可以不同但必須可解釋。Loader 在執行業務 code 前分別核對 semantic、
callback map 與 artifact hashes。

### 7. `SDK-07`：Numeric ID 採 final active set canonical sort

Numeric registration ID 按 registry kind 分域。package kernel：

1. 以 stable IDs 驗證 exact rows、Tag／Map／Predicate／Role 與 fallback intent；
2. 完成 single-wave fallback，得到 final active exact registration set；
3. 在每個 registry kind 內按 canonical StableId UTF-8 bytes 升序分配連續 `u32`；
4. 以 typed numeric newtypes 編譯 bitset、map、predicate 與 runtime tables。

Inactive fallback rows 不取得該 `RegistrationImage` 的 runtime ID。Numeric ID 不由 graph hash 派生，
也不使用會累積 tombstone／安裝歷史的顯式 lock table。closure 改變可以讓 process-local numeric ID
重新編號；save、chunk palette 與長期 protocol 保存 stable IDs，或使用已明文協商且匹配 image hash
的 session table。

### 8. `SDK-08`：首版 schedule stage 與 set policy

首版固定十個 versioned semantic stages：

| display alias | StageId |
| --- | --- |
| `input.sample` | `latticeaxiom:system-stage/input/sample@1` |
| `gameplay.fixed.pre` | `latticeaxiom:system-stage/gameplay/fixed-pre@1` |
| `gameplay.fixed` | `latticeaxiom:system-stage/gameplay/fixed@1` |
| `physics.integrate` | `latticeaxiom:system-stage/physics/integrate@1` |
| `world.commands.apply` | `latticeaxiom:system-stage/world/commands-apply@1` |
| `world.revision.commit` | `latticeaxiom:system-stage/world/revision-commit@1` |
| `derived-work.queue` | `latticeaxiom:system-stage/derived-work/queue@1` |
| `persistence.capture` | `latticeaxiom:system-stage/persistence/capture@1` |
| `async-results.observe` | `latticeaxiom:system-stage/async-results/observe@1` |
| `presentation.update` | `latticeaxiom:system-stage/presentation/update@1` |

Host 將它們映射到 Bevy `PreUpdate`、`FixedUpdate`、`Update`／`PostUpdate` 的 `SystemSet`；該 mapping
是 host implementation，不寫入 portable manifest。Package 不建立跨 stage 的私有 schedule。
Package-local set 必須有 stable ID、綁定一個 stage，before／after edge 只引用同 stage system／set
或公開 barrier。

新增不改變既有 barrier meaning 的 stage 可以提升 catalog minor；移動 authority barrier、改變 thread／
command meaning 或既有 stage 順序，必須建立新 stage major。Bevy 升級必須重跑 mapping conformance。

### 9. `SDK-09`：Access declaration、callback 與 Bevy actual access 三方一致

Macro 從同一 parameter model 生成唯一 `SystemSignature`：

```text
SystemSignature
├── system / callback key
├── stage / set / correctness edges
├── query filters
├── stable component/resource read-write-optional sets
├── command/message/service permissions
└── canonical signature hash
```

Dynamic bridge query 與 column views只由該 signature 建立，module 不提交第二份 access declaration；
loader 驗證 callback key、signature hash 與所有 column layout hashes。Static glue 在 validation World
初始化成 Bevy System，取得 Bevy actual access，經 stable ID ↔ process-local component map 反投影後
與 manifest 比較。`Commands` 等已知 host-internal access 另行分類，不偽裝成業務 component access。

缺少 access、額外 access、read 升級為 write、unknown component、wrong callback 或 layout mismatch
均在 `Playing` 前失敗。Static scheduling 仍以 Bevy actual access 為執行期權威；manifest mismatch
不是警告，也不能靠降低平行度繼續啟動。

### 10. `SDK-10`：Static／dynamic equivalence receipt

Static 與 dynamic 不比較機器指令、FFI buffer 或 batch partition，而比較 versioned
`EquivalenceReceipt`：

- registration semantic／image hash；
- stable ↔ numeric tables、semantic catalog、Role binding／active bundle；
- deterministic schedule plan與effective authoritative settings；
- 每個 fixed tick 在 host validation 後、apply 前的 canonical authoritative command stream；
- replay／authoritative message stream；
- 每個 authority barrier 後的 state hash；
- canonical、未壓縮 normative snapshot DTO bytes。

Command order 使用 tick、stage、system、batch-local sequence 的 canonical envelope；message 必須明文標記
是否屬於 authoritative／replay contract。Callback count、batch切分、thread、duration、address、backtrace、
storage compression、RocksDB WAL與純 presentation data不進 normative comparison。

Fault fixtures 比較 stable diagnostic code 與 structured fields，但排除 timestamp、duration、address、
thread與platform backtrace。D1 equivalence 在同 target／engine baseline 內驗證；portable artifact 跨 Bevy
升級屬另一組 behavior conformance，不偷換成逐 byte process state 比較。

### 11. `SDK-11`：Typed semantic ID 與 wire discriminator single source

Rust serde model 以一個 `registry_targets!` 定義產生：

- sealed `RegistryTarget` marker；
- canonical `RegistryKind` discriminator；
- `TagId<T>`、`RoleId<T>` 與 `MapId<T, V>` typed newtypes；
- checked typed ↔ erased wire conversion。

`V` 實作 `SemanticValueSchema` 並提供 versioned schema ID。Wire form 明文序列化：

```text
TagId / RoleId  = { id, target_kind }
MapId           = { id, target_kind, value_schema }
```

反序列化時 target kind 或 value schema 不匹配即失敗，不能只依 ID 字串的 kind 猜測 generic type。
Runtime catalog 可以使用 erased wire ID；typed Rust API 必須經 checked conversion。首版不建立另一份
YAML／JSON type registry，也不做 Rust／Nickel 雙向 code generation；Nickel contract 與文件以 ADR 0021
要求的同一 golden corpus 跟隨 Rust model。

### 12. `SDK-12`：Extensible Tag authority 與 provenance

Tag contribution 首版 schema 至少包含：

```text
TagContribution
├── tag
├── member: Exact | RequiredTag | OptionalTag
├── intent
│   ├── SelfOwned
│   ├── Integration { target dependency, contract dependency }
│   └── ProfileOverlay { layer ID, operation }
├── declared_by
├── canonical fragment hash
└── SourceProvenance / origin chain
```

Kernel 根據 resolved graph 與 grants判定 authority，不信任 package 自稱 `SelfOwned`：

- `Exact` target 的 exact-registration owner 必須是 contributor；schema kind 可使用明文 schema owner grant；
- `SelfOwned` 不得藉 nested Tag 間接納入 foreign entries；foreign `RequiredTag`／`OptionalTag` nesting
  需要 contract owner 或 integration authority；
- integration package 必須直接依賴 foreign target owner 與 semantic contract owner，或持有等價明文 grant；
  dependency range 使用 graph 中已解析 edge，不在 contribution 重複一份字串真相；
- `add`／`remove`／`replace` profile amendment 只有 trusted composition layer 可授權，並記錄 profile、
  overlay order、operation、authorizing root、被修改 contribution references 與完整 source origin。

Contribution 的 semantic effect 進 semantic hash；source／authority evidence 進 provenance hash與lock。
Unauthorized contribution、missing integration dependency、wrong target kind與overlay authority failure
使用 stable diagnostics並輸出完整 resolution explanation。

### 13. `SDK-13`：首版 semantic runtime layout 與 bounded policy

首版使用 versioned `semantic-compile-policy@1`。Effective policy ID 與 limits 進 lock；同一 authoritative
closure 的 client／server 必須一致。

Runtime representation：

- target numeric ID 使用 `u32` typed newtype；
- Tag 固定編譯成 target-kind width 的 `Box<[u64]>` dense bitset；首版不引入 Roaring bitmap；
- Map value 先按 schema 編成 packed typed value／index，hot path 不保存 `serde_json::Value`；
- Map 在 `Dense { presence bitset, values }` 與 stable-ID-order `Sparse { (u32, value) }` 間選擇；
  若 canonical dense byte cost 小於等於 sparse byte cost 的兩倍就選 dense，否則 sparse；
- Predicate 編成 validated postfix／stack bytecode，operands 是 numeric IDs／constant-pool indices；
  只允許 forward short-circuit，不允許 backward jump、loop、recursion 或 runtime Nickel closure。

`semantic-compile-policy@1` 的 bootstrap hard limits：

| 項目 | 上限 |
| --- | ---: |
| registrations per target kind | 65,536 |
| Tag definitions | 4,096 |
| total Tag words | 4,194,304（32 MiB） |
| compiled Map bytes | 16 MiB |
| authoritative compiled semantic image | 64 MiB |
| Predicate source nodes | 256 |
| Predicate nesting | 32 |
| Predicate instructions | 512 |
| Predicate stack | 64 |
| Predicate constant pool | 64 KiB |

超限在 code activation 前產生 stable diagnostic，不輸出部分 image。D3／D8 optimized benchmark 可以支持
放寬 policy；放寬既有 accepted set可提升policy minor，收緊 acceptance、改變計量單位或runtime layout
meaning必須建立新major。ABI 1.0前以真實Terrenia與fixture數據重新凍結performance gate。

### 14. `SDK-14`：首版 Map merger 只有 `Conflict`

首版 numeric Map 與 relation Map 只實作 `Conflict`。多個不完全相同 contribution 命中同一
`(map, target)`、或 exact 與 Tag-derived value 同時命中且沒有明文規則時，compose 失敗。
不提供 last-writer-wins、implicit exact specificity、float sum 或任意 Nickel／native merger callback。

新增 custom merger 的 gate 是：

1. 至少兩個獨立 package 對同一 `(map, target)` 有合法、真實的多 contribution consumer；
2. 單一 owner 的完整值無法表達需求；
3. operation 有 identity，且 deterministic、commutative、associative；
4. overflow、invalid value與float／NaN policy有total definition；
5. permutation與parenthesization property tests通過。

首個候選應是 bounded integer／fixed-point 的內建 `Max` 類操作，而不是公開自訂函數。沒有滿足
上述 gate 的 consumer，`Conflict` 保持唯一 merger，且不阻塞 D3。

### 15. `SDK-15`：Frozen semantic resolution 只有一份持久權威 receipt

Role binding、active bundle 與 explanation 使用單一 canonical receipt：

```text
SemanticResolutionReceipt
├── schema version
├── authoritative semantic image hash
├── active bundle stable IDs
├── Role bindings
│   ├── Role ID / contract major
│   ├── selected concrete target(s)
│   └── selected_by: unique | profile | default | frozen | fallback
└── deterministic explanation graph
```

Receipt content-address為 `semantic_receipt_hash`，存入 per-world frozen game／registration lock。
`WorldMetadata` 只保存 exact game-lock hash、semantic receipt hash與world migration／epoch state；
world directory 必須攜帶該完整 receipt。`WorldHeader` 只投影 hash與health，不是第二份權威資料。

`RegistrationImage` 保存 derived O(1) role lookup、active bundle set與receipt hash，作為 runtime cache；
它不是另一份持久真相。World reopen 先恢復 frozen receipt，再重建並逐 byte核對 compatible image，
不能重新以新增候選擇優。Active bundle不能只從Role binding反推，因bundle可以包含沒有Role offer的
registration。

Explanation graph deterministic排序並記錄：所有候選、predicate evidence、Tag／Map provenance references、
selection rule、missing roles、profile/default/frozen input與fallback activation wave。只保存人類字串摘要
不符合此 contract。

### 16. `SDK-16`：首版 Affordance 只有 static semantic fast path

D0–D10 沒有證明 Tag／Map／State + normal mechanic system 無法表達的 Affordance callback consumer。
Mining、tool、fuel、recipe、torch、container與fluid baseline因此只實作
`AffordancePlan::StaticPredicate`。Manifest 可以保留 versioned callback policy 的未來形狀，但首版
compiler拒絕callback realization，R3不發布Affordance callback table。

Container權限、furnace transaction或一般互動先屬於明確mechanic system，不能為了API完整性包成
萬用 `interact` callback。Context-dependent portal frame stability可以成為未來第一候選；只有第二個
獨立 consumer 證明相同 request／response／read-set contract 可重用後，才凍結 static direct 與
dynamic batch callback。屆時必須同時交付determinism、access、budget、fault與equivalence證據。

## Activation 與編譯順序

首版實作依賴順序固定為：

```text
manifest hash boundary + typed semantic IDs
  → component/schema modes + stages + contribution authority
  → SDK parameter whitelist + final numeric mapping + semantic compilation
  → frozen semantic resolution receipt
  → row-kernel static glue + dynamic descriptor/batch bridge
  → actual Bevy access / callback binding validation
  → static/dynamic equivalence receipt
```

Package kernel 的 activation pipeline 是：

```text
read canonical manifests without code
  → validate exact IDs / schemas / grants / system signatures
  → validate Tag / Map / Predicate / Role intent using stable IDs
  → resolve one fallback wave and freeze SemanticResolutionReceipt
  → assign final per-kind numeric IDs
  → compile bitsets / packed maps / predicate plans / schedule
  → create RegistrationImage
  → build/load artifacts and verify callback/artifact hashes
  → register typed/dynamic Bevy components
  → initialize static systems and cross-check actual access
  → create RuntimeImage / install adapters
  → enter Playing only after world preflight succeeds
```

這個順序明確解決 conditional fallback registration：fallback 必須在 final active numeric mapping 前完成，
而 hot-path bitset與tables在numeric mapping後編譯。

## 自動證據

R2／R3／D1／D3 至少需要以下完全自動化的證據；人工或視覺測試不能替代：

### SDK 與 manifest

- proc-macro compile-pass／compile-fail corpus覆蓋全部 dual白名單與static-only reason；
- 同一row kernel只存在一份source，生成static wrapper與dynamic batch shim；
- generated manifest、C binding、Rust binding與schema docs通過同一golden corpus；
- public／persistent／ABI `SchemaId` golden corpus拒絕缺少 `@major` 的值，且證明 ephemeral
  `HostTyped` internal identity不能逸出generator／package-private static implementation；
- `SourceSpan` corpus只接受單一source內byte range；完整位置與origin chain使用`SourceProvenance`；
- producer／`SourceProvenance`／toolchain改變不改semantic hash，但按ADR0021改變provenance receipt；
- callback map、artifact或layout hash mismatch在任何業務callback前失敗。

### Component 與 Bevy access

- C reference descriptor與Rust SDK descriptor逐field得到相同layout hash；
- bad size／stride／offset／alignment／endianness、pointer、drop、ZST與overflow fixture全部拒絕；
- 多個fresh `World`與randomized registration order不改stable mapping語義，且測試不比較`ComponentId`數值；
- static system初始化後的Bevyactual access與manifest逐項相同；額外write／missingread fixture fail-fast；
- dynamic callback只取得signature允許的columns、commands、messages與services。

### Determinism 與 equivalence

- randomize source、fragment、registration、package、component與load order，不改numeric tables、schedule、
  semantic image與diagnostic ordering；
- static／dynamic registration、IDs、schedule、authoritative command／message stream、N tick state hash
  與normative snapshot bytes相同；
- static path不經C ABI並可啟用LTO；dynamic FFI隨system／batch而非entity線性增加；
- panic、wrong callback、stop-after-callback與invalid command不留下半套authoritative mutation。

### Semantic compilation 與 world reopen

- typed-ID wrong-kind／wrong-value-schema corpus在typed conversion失敗；
- self-owned、integration、profile overlay、foreign nested Tag與missing dependency均有positive／negative fixture；
- Tag DAG／cycle、Map conflict、Predicate type／budget、Role unique／ambiguous與fallback competition完整；
- semantic limits在boundary通過、`+1`以stable diagnostic失敗；tick benchmark證明無Nickel、JSON、字串package lookup；
- new-world與frozen-world fixture證明新增候選不改舊binding／bundle；receipt與world metadata mismatch在writer前失敗；
- 移除Terrenia並換入非Terreniaroot package時，typed semantic compiler與host不需special case。

## 明確延後的 Gate

以下能力不屬於首版缺漏，而是需要新證據才能進入後續 contract：

| 延後能力 | 觸發證據 |
| --- | --- |
| 公共 bulk／reduction batch kernel | 第二個row kernel無法合理表達的真實system |
| 更多 dual SystemParam | 對應ABI table、access model、batch形狀與equivalence fixture |
| non-POD／drop-bearing RuntimeDynamic | allocator、ownership、drop、panic與migration獨立ABI proposal |
| custom Map merger | `SDK-14`五項gate與兩個獨立package consumer |
| Affordance callback | 兩個共用request／response／read-set的context consumer |
| runtime semantic hot reload | authoritative epoch／world transaction／multiplayer協商proposal |

滿足觸發證據也不自動授權實作；breaking schema、acceptance set、ownership或hot-path meaning仍需提升
正確contract major，必要時建立新的accepted decision。

## 結果

- R2 可以在沒有native code的情況下編譯完整RegistrationImage、schedule與semantic catalog。
- SDK authoring有一個公共入口，static與dynamic不複製業務函數或registration truth。
- Static保留typed Bevy/LTO路徑；dynamic限制在可驗證、bounded、batch-oriented contract。
- Numeric ID、schedule、Tag／Map／Predicate layout與world semantic receipt都有canonical、order-independent規則。
- World只凍結一份可重建且可解釋的semantic resolution，不因新package或候選出現而漂移。
- 首版承擔macro/codegen、unsafe bridge隔離、golden corpus與equivalence harness成本，以換取可測試的ABI與save contract。
- Custom merger與Affordance callback不再是假裝「稍後補完」的首版需求；它們有明確、不阻塞roadmap的證據gate。

## 被否決的方案

### 公開 macro 與 builder 兩套等價 authoring API

這會讓pure-data、static與dynamic package各自選一套truth，也會繞過Nickel source-aware merge與policy。

### 直接把任意 Bevy Query／SystemParam 自動翻譯成 C ABI

Bevy／Rust type、borrow、change detection與custom parameter沒有portable ABI；表面自動化只會把不安全或
無法驗證的差異推到runtime。

### 以 graph hash 或永久 lock table 分配 numeric ID

Hash需要collision／sparse policy；永久table讓安裝歷史與tombstone進入process-local layout。Canonical
active-set sort更直接，且長期資料本來就保存stable IDs。

### 讓 dynamic module 提供 destructor 或 ComponentId

Foreign drop、allocator與process-local ID都無法成為portable storage contract，且會污染Bevy World安全邊界。

### 以 load order、exact specificity 或 last-writer-wins 合併 Map

這些規則無法由manifest順序無關地重建，也會掩蓋owner與integration conflict。

### 在 lock 與 world metadata 各保存一份 bindings／bundles

兩份可修改權威資料必然漂移。World保存content-addressed frozen receipt reference，runtime image只保留派生cache。

### 為了 schema 完整提前發布 Affordance callback

沒有真實consumer就無法凍結request、read set、thread、budget與failure semantics，且會無謂擴大ABI 0.x。

## 路線與依賴

- R0／R1 提供typed models、canonical hash、golden corpus、deterministic graph、BuildPlan與frozen lock。
- R2 實作本決策的SDK、manifest、static glue、schema plan、numeric/schedule compiler與semantic compiler。
- R3只在本決策白名單與validated POD descriptor上加入portable batch／command／message ABI。
- D1以`@example/dual-gameplay`交付row-kernel、access validation與EquivalenceReceipt。
- D3以Terrenia／test content交付typed Tag／Map／Predicate／Role、fallback與frozen world reopen。
- Native ABI header、interface、ownership、panic與command transaction仍由[決策 0017](0017-versioned-native-module-abi.md)
  及其後續ABI decision治理；本決策不重複定義C layout。
- Bevy dynamic registration、schedule mapping與upgrade受[決策 0014](0014-adopt-bevy-upstream-first.md)約束；
  API差異先由host adapter吸收，不能滲入portable manifest。

## 相關文件

- [靜態與動態實現收斂於同一套件圖](0008-static-and-dynamic-realizations-share-one-graph.md)
- [分離 package 與 registration identity](0019-separate-package-and-registration-identities.md)
- [語義註冊與內容選擇](0020-semantic-registration-and-content-selection.md)
- [R0／R1 Package、Nickel 契約與解析政策](0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md)
- [套件模組、註冊清單與靜動雙實現](../architecture/module-composition.md)
- [語義註冊、內容判定與候選選擇](../architecture/semantic-registration.md)
- [原生模組 ABI、批次資料與生命週期](../architecture/native-module-abi.md)
- [Bevy 世界持久化](../architecture/world-persistence.md)
- [套件驅動的 Bevy 執行期整合路線](../planning/roadmap-game-engine.md)
- [第一個套件驅動的 Bevy 可玩 demo 路線圖](../planning/roadmap-first-demo.md)
