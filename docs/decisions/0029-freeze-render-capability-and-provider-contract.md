---
title: 凍結首版渲染能力與 Provider 契約
status: accepted
type: decision
updated: 2026-08-20
---

# 決策 0029：凍結首版渲染能力與 Provider 契約

## 背景

[決策 0014](0014-adopt-bevy-upstream-first.md)已經確立 Bevy upstream-first 邊界，
[決策 0017](0017-versioned-native-module-abi.md)則把 static、portable dynamic 與
engine-coupled native realization 分級。首個可玩 demo 仍需要一個比「package 可以插入
render code」更窄、可在載入 code 前驗證的首版契約：package 必須能宣告 presentation data、
可組合 feature、明確 pass 與 exclusive provider，同時不能把 Bevy RenderWorld 或 raw wgpu
提升成 portable API。

若不在 D5 前凍結這個邊界，semantic slot 會退化成 Bevy node 名稱，resource access 會依
load order 互相覆蓋，portable command list 會無界擴張，而假造的第二 provider 會讓 core
過早拆成沒有產品 consumer 的抽象 packages。這些結果都會破壞 deterministic registration、
headless graph validation、static／dynamic equivalence 與 Bevy upgrade rehearsal。

本決策具體化 `REND-01` 至 `REND-09`，只凍結 D5／R6 所需的第一版 contract。SDK 產生的
manifest、numeric handle、schedule 與 equivalence receipt 由[決策 0023](0023-freeze-sdk-registration-and-semantic-compilation.md)
治理；semantic binding 與 provenance 不另建第二套選擇機制。portable table、buffer ownership、
instance lifecycle 與 fault boundary 仍由[決策 0024](0024-freeze-portable-native-abi-0x.md)治理。
渲染預算使用[決策 0026](0026-freeze-first-demo-performance-budgets.md)定義的
target performance profile 與量測 receipt；本頁的數字是 portable command contract 的硬上限，
不是宣稱 target hardware 已通過效能 gate。

## 決策驅動因素

- Bevy 必須保持唯一 renderer、render scheduler、GPU device 與 asset runtime。
- package graph 必須在不載入 package code、不建立 GPU 的情況下驗證 slot、resource、provider
  與 fallback。
- 普通 content／gameplay package 應優先提供 typed presentation data，而不是 renderer hook。
- static path 保留 Bevy typed API 與最佳化；portable path 只暴露 bounded、可驗證的命令。
- render failure、fallback、device loss 與 provider 更換不得改變 authoritative world 或 save。
- 首版必須由真實 post effect、voxel LOD 與 material consumer 驗證，但不要求人工或視覺測試。

## 決策

### 1. 四級擴充模型與首批 consumer

首版固定四個彼此不可混用的 declaration kind：

| 層級 | 首版責任 | 首批 consumer |
| --- | --- | --- |
| `RenderData` | renderer 已理解的 typed presentation rows，不建立 pass | voxel chunk presentation descriptor、block material table |
| `RenderFeature` | 可加性的零至多個 pass 與其 shader／pipeline intent | depth fog、selection outline、block material extension |
| `RenderPass` | 每 camera／view／frame 在一個 semantic slot 執行一次的明確 graph node | fog composite、outline compute／composite |
| `RenderProvider` | `exactly-one` 的完整機制 owner | terrain mesh source；core visibility 與 terrain backend |

`RenderFeature` 可以產生零個 pass，例如 GPU 能力不足而選擇 `Disabled` fallback；也可以產生多個
有明文 resource edge 的 pass。`RenderPass` 不是 arbitrary Bevy system 或 wgpu callback，不能取得
未宣告的 resource，也不能按 package load order插入。

普通 block／item／entity content 預設只註冊 `RenderData` 與 asset reference。只有需要新增 shader、
compute 或 composite 行為的 package 才提升為 `RenderFeature`；只有擁有完整 exclusive mechanism
的實作才是 `RenderProvider`。

### 2. 首版 semantic slots

Core 3D slot catalog major 1 只包含：

| SlotId | 保證的語義 |
| --- | --- |
| `latticeaxiom:render-slot/core3d/prepass@1` | host 已建立該 view 所需的 declared prepass outputs，main color 尚未執行 |
| `latticeaxiom:render-slot/core3d/after-main@1` | main 3D color 已完成，尚未進入 post-processing chain |
| `latticeaxiom:render-slot/core3d/early-post@1` | early post feature 的 deterministic composition point |
| `latticeaxiom:render-slot/core3d/before-tonemap@1` | HDR color 與已宣告的 view resources 可讀，tonemapping 尚未執行 |
| `latticeaxiom:render-slot/core3d/after-tonemap@1` | tonemapped LDR color 可讀寫，後續只允許 LDR-compatible feature |

Package 只引用上述 stable IDs、slot version 與 resource contract；目前 Bevy schedule、render graph
node、label 或 set 名稱由 core host adapter映射，不進 portable manifest。Bevy upgrade 可以改 mapping，
但必須保持 slot conformance 或提升正確 slot major。

`extract`、`prepare` 與 `queue` 是決策 0023 的 system／adapter stages，不是 frame graph slots。
首版不提供語義含糊、容易綁死上游實作的 `after.opaque`。需要 opaque-only insertion 的真實 consumer
必須先證明現有 `after-main`／prepass 資源不足，再提出新 slot contract。

### 3. Resource declaration 採 SSA

每個 logical render resource declaration 至少包含：

```text
RenderResourceDecl
├── resource stable ID / local version
├── kind: Texture | Buffer
├── scope: View | Frame | Feature
├── lifetime: Imported | Transient | PersistentPresentation
├── format
├── sample count
├── dimensions / extent policy
├── usage set
└── producer: HostSlot | FeaturePass
```

Format 與 usage 使用 versioned portable enum，不使用 wgpu integer。首批 fixture 至少覆蓋
`depth32float`、`rgba16float` 與 `r8unorm`，以及 sampled、storage read／write、color attachment、
depth read、uniform 與 upload destination usage。Device-specific physical format只能由 host依
manifest允許的 compatible set選擇，並寫入 presentation receipt。

Resource graph 使用 single-static-assignment 規則：

- read引用一個已存在的明確 version；
- 每次 write 產生新 version，不能原地改寫上一 version 的 identity；
- 每個 version恰有一個 writer，可以有多個 readers；
- format、sample count、extent、usage、scope與lifetime必須在code activation前相容；
- package不能宣告barrier、physical allocation、alias address或Bevy／wgpu handle；
- host依SSA dependency與lifetime推導barrier，且只有在lifetime不重疊時才可alias physical resource。

Cycle、missing producer、multiple writer、read-before-produce、format／sample mismatch或非法lifetime
一律讓render plan compilation失敗，不以排序或最後寫入者解決。

### 4. Portable render command interface

`RenderCommandList` schema major 1 由 canonical InterfaceId
`latticeaxiom:interface/render.command-list` 的 experimental version `0.1`提供。Interface canonical
name不含version；negotiation、table header與minor／major演進完全沿用決策0024。

D5 的 portable command vocabulary 只有：

1. `SelectDeclaredPipeline`；
2. `BindDeclaredResourceSet`；
3. `SetParameterBlock`；
4. `Dispatch`；
5. `DrawFullscreenTriangle`；
6. `DebugMarker`。

Pipeline、resource set與parameter layout必須已在manifest／render plan宣告。Command只引用目前
`RegistrationImage`分配的typed numeric handle；host驗證generation、owner、slot、usage、range、
alignment與pipeline compatibility。Dynamic module填充資料時只使用host-provided upload slice，
再以checked `commit`交回；foreign code不能保存slice或GPU handle到callback之後。

首版明確沒有raw resource／pipeline creation、copy、readback、explicit barrier、indirect draw、
arbitrary vertex／index draw或raw shader module。缺少的操作優先用`NativeStatic`實作；不能把
`RenderCommandList`逐步擴張成另一套wgpu。

Host先完整decode並驗證整張list及其upload commits，成功後才原子接受；任何unknown opcode、
bad handle、overflow、quota或resource mismatch都拒絕整張list，不能提交前綴。拒絕只產生bounded
diagnostic與presentation fallback，不改authoritative state。

### 5. 首版 hard budgets

以下上限按 **每 feature／view／frame** 計算；profile可以調低，不能提高到超過contract maximum：

| 項目 | 上限 |
| --- | ---: |
| command lists | 4 |
| commands per list | 256 |
| total encoded command bytes | 64 KiB |
| committed upload bytes | 4 MiB |
| dispatch commands | 32 |
| cumulative workgroups | 1,048,576 |
| fullscreen triangle draws | 8 |
| readback bytes | 0 |

每個dispatch的三維workgroup數另外不得超過device advertised limit；checked multiplication後才加入
cumulative workgroups。所有feature合計的global upload ceiling是每frame **16 MiB**。Host在分配或
提交前執行quota validation；超限不得以分割多張list、重入callback或多次slice commit繞過。

這些值是防止無界CPU／memory／GPU工作量的D5 bootstrap hard caps。決策 0026 的target profile
另外量測encode、validation、upload、dispatch與draw的P50／P95／P99；D10可以依證據調低effective
budget。整個dynamic presentation frame仍同時受決策0026的aggregate callback／command hard caps治理；
兩組上限不同時採較嚴格者。放寬本表、加入readback或改變計量單位需要新的contract version與
benchmark，不是普通設定。

### 6. Provider ownership 與 GPU fallback

Bevy／core host adapter永久擁有window、surface、device、queue、Core3d mapping、asset integration與
physical resource state。首版provider seam如下：

```text
authoritative chunk snapshot
  → latticeaxiom:capability/render-terrain-mesh-source@1   separate logical package
  → latticeaxiom:capability/render-visibility@1            core Bevy adapter
  → latticeaxiom:capability/render-terrain-backend@1       core Bevy adapter
  → Core3d slots
```

Terrain mesh source已有CPU mesher、LOD與未來替換路徑的真實獨立生命週期，因此由一個logical package
提供。Visibility與terrain backend在首版沒有第二個產品provider，保留為core Bevy adapter；exclusive
conflict fixture不構成拆包理由。只有第二個真實provider同時具有獨立version、consumer、fallback、
owner與量測證據，並通過決策0014的upstream偏離gate，才可把其中一項拆成logical package。

Provider／realization selection只使用standard GPU features、limits、formats、target profile與profile
明文stable priority。Vendor／device ID不能作positive capability或tie-break；只可進入有upstream issue、
expiry、owner與fixture的denylist／workaround。Fallback graph必須acyclic，按manifest stable order選擇：

1.滿足required standard capabilities的preferred realization；
2.明文degraded realization；
3.`Disabled`，但只限presentation-optional feature；
4. required feature無fallback時在activation前失敗。

Selected provider、physical format、fallback原因與GPU receipt進presentation registration／diagnostics，
不進authoritative hash、world save或semantic resolution receipt。Render failure、device loss與provider
切換不得推進world revision。

### 7. Normative D5 fixtures

首版必須交付三組互補consumer：

1. **NativeStatic depth fog**：插入`before-tonemap@1`，讀取HDR color與depth，寫入新的HDR color
   version；證明static feature仍宣告完整slot／resource contract而不經portable command table。
2. **Portable selection outline**：compute pass讀depth與selection mask、寫edge mask，再以
   `DrawFullscreenTriangle` composite；selection mask缺失時選`Disabled` fallback；證明dispatch、
   parameter、resource binding、multi-pass SSA、budget與atomic command validation。
3. **Voxel LOD + material composition**：CPU terrain mesh source產生近／遠兩級LOD，與獨立block
   material extension共存；stale chunk／material/provider receipt不能提交，LOD切換不改authoritative
   chunk或save。

同一feature另提供paired `NativeStatic`／`PortableNative` realization，normative comparison是compiled
render plan hash、resource versions、validated command trace、fallback與diagnostic，不是機器指令、
wgpu handle或pixel-by-pixel visual output。兩個test-only terrain provider可以驗證exclusive conflict，
但不取得production ownership。

### 8. Engine-coupled interface 暫不建立產品 contract

D5只有一個test-only exact-build fixture，驗證`EngineBuildId` mismatch、instance lifecycle、rebuild與
fallback。它只能查詢最小test table，不暴露RenderWorld、wgpu object或通用renderer internals；
因此不算`EngineCoupledNative` render interface的產品consumer，也不能藉此發佈長期interface。

只有真實terrain／visibility／pass provider證明`NativeStatic`與本決策的portable contract都不足，
且提交決策0014要求的playable reproduction、upstream attempts、measurements、smallest deviation與owner，
才可另立窄的engine-coupled interface ADR。沒有該證據時，REND-09關閉為「不實作」。

## 自動證據

D5／R6／R7必須提供以下自動化、headless-friendly證據；人工或視覺測試不能替代：

- 隨機化source、manifest與feature discovery order，compiled render plan與diagnostic ordering逐byte相同；
- 每個slot在Bevy 0.19 baseline有mapping conformance，升級rehearsal證明相同semantic fixture能remap；
- missing producer、cycle、multiple writer、bad format／sample／usage／lifetime全部在載入code前失敗；
- unknown opcode、forged／stale／foreign handle、bad alignment、integer overflow與每項quota boundary／`+1`
  corpus皆原子拒絕，且在拒絕前GPU submission count為零；
- command decoder與validator接受structure-aware fuzzing，任何輸入都不panic、不越界、不partial submit；
- paired static／portable feature產生相同render plan、resource graph、validated command trace與fallback；
  static path不查詢portable table；
- fake GPU matrix覆蓋feature、limit、format缺失、degraded與Disabled fallback；vendor順序不改選擇結果；
- 兩個exclusive test provider在code activation前衝突，profile明文選擇後只有一個active；
- headless不建立Window、RenderDevice或GPU resource，仍可編譯graph、驗證command與CPU mesher／LOD；
- shader／pipeline failure、device loss、invalid command與visualizer truncation前後authoritative world hash相同；
- target performance profile記錄encode／validate CPU、command bytes、upload、workgroups、draw與GPU time，
  hard cap與profile budget regression都由CI判定。

## 明確延後的 Gate

| 延後能力 | 觸發證據 |
| --- | --- |
| copy／explicit barrier／indirect draw／readback opcode | 至少一個不能由static或現有commands表達的產品feature；ownership、atomic failure與target benchmark齊全 |
| 更多或更細的Core3d slot，包括`after.opaque` | 兩個consumer共用同一跨Bevy semantic，且upgrade mapping conformance成立 |
| visibility／terrain backend logical package | 第二個真實provider、獨立version／owner／fallback與ADR0014證據包 |
| general engine-coupled render table | 真實provider證明static／portable不足，並通過exact-build、fault與upgrade gate |
| vendor-specific positive selection | standard capability無法表達的可重現差異、upstream issue與有期限policy；首版只允許denylist workaround |
| GPU-driven terrain／occlusion／meshlet path | CPU two-level LOD baseline先有profile數據，且新provider保持snapshot、revision與save invariant |

滿足觸發證據不會自動擴張`@1`。新增opcode、resource meaning、slot ordering或ownership若改變既有
acceptance／failure semantics，必須提升正確major並建立accepted decision。

## 待決問題收口

| 編號 | 本決策的收口 |
| --- | --- |
| `REND-01` | 四級模型與depth fog、outline、voxel consumer |
| `REND-02` | 五個Core3d semantic slots及Bevy adapter boundary |
| `REND-03` | versioned resource declaration與SSA writer／lifetime規則 |
| `REND-04` | 六個portable opcodes、host upload slice與明確禁用面 |
| `REND-05` | per-feature與global hard caps、atomic validation及target benchmark gate |
| `REND-06` | terrain mesh-source package；visibility／backend暫由core擁有 |
| `REND-07` | standard capability selection、vendor denylist與acyclic fallback |
| `REND-08` | fog、outline、two-level voxel LOD／material fixtures |
| `REND-09` | 沒有產品consumer；只保留test-only exact-build fixture |

## 結果

- Package kernel能在不載入code、不建立GPU時編譯deterministic render plan並拒絕衝突。
- 普通content沿typed RenderData路徑；feature與provider只在真實機制需要時提升。
- Static保留Bevy typed path；portable command surface窄、bounded、atomically validated且不鏡像wgpu。
- SSA resource graph讓single-writer、format、sample、lifetime與barrier責任可驗證，load order不再有語義。
- Provider ownership遵守upstream-first，沒有因test fixture提前創造visibility／backend packages。
- D5可以用全自動semantic／command／fault／performance證據出場，不依賴視覺golden。

## 被否決的方案

### 把 Bevy render graph node／SystemSet 當成 portable slot

它會讓每次Bevy內部重排成為package break，且portable artifact無法跨upgrade rehearsal。

### 讓 package 宣告 barrier 或 raw wgpu command

Package看不到全圖physical state，無法安全決定alias與barrier；raw API也會繞過resource ownership、
quota與fallback validation。

### 用 load order 或priority解決 resource writer／provider衝突

這會讓filesystem、link與dynamic load順序改變frame結果。Writer必須唯一，exclusive provider必須由
profile明文選擇。

### 為 fixture 提前拆出 visibility／terrain backend packages

Fake provider只能證明resolver conflict，不能證明獨立產品生命週期。沒有第二個真實provider時，
抽象package只增加version與upgrade成本。

### 把所有 effect 都限制為 portable commands

需要完整Bevy generic、specialized pipeline或低層integration的可信source package應使用`NativeStatic`；
portable ABI不是全project的最低共同分母。

## 路線與依賴

- 決策0023先提供manifest hash、numeric handle、stages、access cross-check與EquivalenceReceipt。
- 決策0024提供portable table header、handle、buffer ownership、callback與failed-instance semantics。
- D3／D4先提供revision-checked authoritative chunk snapshot、block material identity與CPU mesher input。
- D5交付本決策三組fixtures、command validator、fake GPU matrix與Bevy exact-build rehearsal。
- R6只在上述fixtures通過後擴充provider composition；R7以同一portable fixture驗證Bevy remap。
- Render plan與presentation receipt不進authoritative semantic／world hash；決策0026的performance profile
  只治理量測與effective budget，不改這個資料邊界。

## 相關文件

- [採用 Bevy 並以上游優先](0014-adopt-bevy-upstream-first.md)
- [版本化原生模組 ABI](0017-versioned-native-module-abi.md)
- [首版 SDK、Registration 與語義編譯契約](0023-freeze-sdk-registration-and-semantic-compilation.md)
- [Portable Native ABI `0.x`](0024-freeze-portable-native-abi-0x.md)
- [第一個 Demo 性能預算](0026-freeze-first-demo-performance-budgets.md)
- [Bevy 渲染能力、Pass 與 Provider 組合](../architecture/rendering.md)
- [套件驅動的 Bevy 執行期整合路線](../planning/roadmap-game-engine.md)
- [第一個套件驅動的 Bevy 可玩 demo 路線圖](../planning/roadmap-first-demo.md)
