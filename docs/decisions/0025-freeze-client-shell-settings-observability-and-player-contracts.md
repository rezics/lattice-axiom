---
title: 凍結 Client Shell、設定、可觀測性與玩家契約
status: accepted
type: decision
updated: 2026-08-20
---

# 決策 0025：凍結 Client Shell、設定、可觀測性與玩家契約

## 背景

R4 的 package-driven Bevy host 與第一個 demo 的 D2 可玩 voxel playground 必須共用一組
可執行契約。先前文件已確立 Bevy 是唯一 App／ECS／scheduler／renderer／input／UI runtime，
也已提出 settings、diagnostics、world lifecycle 與 upstream adoption 的方向；但 package
capability 邊界、setting transaction、diagnostic batch、client process lifecycle、玩家移動與
break／place 的首版數值仍可能被不同實作者各自解讀。

本決策一次凍結 open questions 的 PLAY-01..09 與 SET-01..13。它凍結的是第一個可玩版本的
產品語義、stable schema 邊界、保守安全上限與自動證據；不把未量測的 upstream candidate
宣告為已通過 adoption，也不提前建立通用 UI framework、physics abstraction 或 multiplayer
protocol。

## 決策

### 1. 首版基礎 package 與 capability

下列七項是 logical package，不要求一個 package 對應一個 Rust crate。首個 workspace
manifest version 均從 0.1.0 開始；capability contract 獨立從 1.0.0 開始，consumer 使用
相容範圍 ^1.0.0。Portable Native 的 callback table 仍是獨立的 ABI 0.x coordinate，不能因
capability 已是 1.0.0 而冒充穩定 C ABI。

| Package | Provides | Requires | 首版投影與責任 |
| --- | --- | --- | --- |
| @latticeaxiom/settings | latticeaxiom:capability/settings-registry@1 | — | shell／client-world／headless／test；catalog、validation、effective value、transaction、event |
| @latticeaxiom/settings-ui | latticeaxiom:capability/settings-surface@1 | settings-registry@1 | client／tool；search、draft、preview、apply、reset、accessibility |
| @latticeaxiom/observability | latticeaxiom:capability/diagnostic-registry@1 | — | shell／client-world／headless／test；item、metric、subscription、budget、report |
| @latticeaxiom/inspect | latticeaxiom:capability/target-inspect-surface@1 | diagnostic-registry@1 | client；target selection、permission-aware fragment composition |
| @latticeaxiom/dev-tools | latticeaxiom:capability/debug-workbench@1 | settings-registry@1、diagnostic-registry@1 | dev／tool client；workbench、graphs、world-space visualizers |
| @latticeaxiom/world-library | latticeaxiom:capability/world-catalog@1 | diagnostic-registry@1 | shell／tool；catalog、preflight、recovery actions |
| @latticeaxiom/front-end | latticeaxiom:capability/client-shell@1 | world-catalog@1、settings-surface@1、diagnostic-registry@1 | client-shell root；首頁、world flow 與 surface composition |

每個被要求的 capability 都是 exactly-one。dev-tools 的「可選」由 profile 是否選入 package
表示；選入後仍是 exactly-one，不新增 zero-or-one cardinality，也不靠 host discovery
偷偷啟用。settings-ui、inspect 與 dev-tools 是 presentation package，不進 authoritative
world hash。headless foundation 只要求 settings 與 observability。

### 2. Setting value、constraint 與 predicate schema

SettingValueV1 是版本化 tagged union；default、stored value、draft、callback batch 與
dynamic ABI 都使用同一語義：

| Kind | 首版規則 |
| --- | --- |
| bool | true／false |
| integer | signed 64-bit；spec 可給 min／max／step |
| number | exact finite decimal；canonical model限制為signed 18-decimal fixed-point，不經 binary64 |
| string | UTF-8 NFC；spec 必須給 Unicode scalar count 上限，batch另受byte budget限制 |
| enum | stable variant ID；localized label 不作 value |
| color | linear RGBA 四個exact finite、0..1 component |
| key-binding | versioned InputBindingV1，不接受自由格式 display string |

list、map、任意 JSON、filesystem path、credential 與 secret 不進 V1。private path 只能使用
專用 sensitivity 與本機 store；secret 未來使用 platform secret-store contract。

單欄位 constraint 包含 range、step、allowed variants 與 length。CrossSettingConstraintV1
與 visibility／enabled predicate 使用有界 declarative AST：

- Eq、Ne、Lt、Le、Gt、Ge、In；
- All、Any、Not；
- setting reference 或同型別 literal；
- expression depth 最多 8，總 node 最多 32。

V1 不接受 script、regex、arithmetic、function callback 或隱式型別轉換。跨 package reference
必須有 manifest dependency；所有 reference 在 catalog compile 時解析、型別檢查並檢查 cycle。
一個 cross-setting constraint 只能跨同一 authority lane 與 durability transaction domain，
否則 manifest 在 code activation 前失敗。visibility 與 enabled_when 只控制呈現，不授權、不
繞過 validation；hidden value 仍按完整 catalog 驗證。

### 3. Scope、authority、precedence 與 provenance

profile 是 composition input，不是 Runtime Setting scope。RuntimeSettingScopeV1 只有 device、
user、world、player-world 與 session。每個 SettingSpec 選擇允許 scopes，但不得自訂任意
precedence；registry 固定使用下列 authority lane：

| Authority | Effective precedence，由低到高 |
| --- | --- |
| LocalUser | default → device → user → player-world → session |
| WorldOwner | default → world → player-world |
| Server | default → world → player-world |
| AdminOnly | default → world → player-world，且只有已驗證 admin command 可寫 |
| FixedByProfile | frozen lock value；沒有 runtime override |

LocalUser 的 player-world 層只在 active world 明確提供該玩家 store 時存在。WorldOwner、
Server 與 AdminOnly 不接受 device、user 或 session 覆蓋。FixedByProfile 的值來自 lock，而不是把
profile 偽裝成一個 mutable store。跨 scope 不使用 timestamp 或 last-writer-wins。

每個 effective value 都攜帶 EffectiveSettingProvenanceV1：

- setting ID 與 value schema version；
- winning scope、store revision 與 transaction revision；
- writer kind，以及 world／player key（適用時）；
- active shell lock 或 world lock hash；
- 被遮蔽的合法 scope revisions，供 UI explanation 使用。

wall-clock timestamp 可留在非權威 audit metadata，但不參與 effective resolution、
authoritative hash 或 deterministic comparison。

### 4. Bevy app settings 與 world store adapter

Client 明確啟用 Bevy 0.19.1 的 bevy_settings feature。Bevy SettingsPlugin 只承接一個靜態
LatticeLocalSettingsV1 group／resource；它的 device 與 user section 保存
StoredSettingEntryV1 tagged values。動態 package 不各自建立 SettingsGroup，因為 upstream
SettingsGroup 是靜態 Rust resource contract。

啟動順序固定為：

1. 讀取並驗證 local settings envelope；
2. 建立 device／user effective snapshot；
3. 安裝依賴這些值的 client plugin／surface；
4. graph activation 後才綁定 generated static handle 或 dynamic numeric key。

local store 使用 upstream debounced asynchronous save、exit sync-if-changed 與
temporary-file／atomic-replace 行為。world 與 player-world 使用相同
StoredSettingEntryV1 codec、catalog validator 與 migration owner，但經 WorldStorage／RocksDB
atomic batch 保存；它不鏡像 Bevy settings API。session 是記憶體內 Bevy Resource；profile
仍在 Nickel／lock。world preflight 只讀 header／metadata，不為讀設定而先開 writer。

### 5. Preview、apply transaction 與 rollback

RuntimeApplyImpactV1 只有 Immediate、WorldReactivate、ProcessRestart。Preview 不再是 impact；
PreviewPolicyV1 是 None 或 Reversible，後者宣告 timeout 與 generated participant。GraphRecompose
只屬於 composition draft row，manifest validator 必須拒絕 Runtime Setting 使用它。

一次 apply transaction 只跨一個 durability domain，依序執行：

1. authorize；
2. 以 base transaction revision 建立完整 proposed snapshot；
3. 驗證所有單欄位與 cross-setting constraints；
4. 依 owner stable ID／participant key 排序，批次 prepare；prepare 不得產生外部可見副作用；
5. 若有 preview，建立可逆 presentation preview；
6. 以該 store 的單一 atomic operation persist；
7. 原子 publish immutable effective snapshot；
8. 發出一個 SettingChangedBatchV1，再執行 non-rejecting、idempotent commit callback。

Generated participant operations 是 prepare_batch、begin_preview_batch、
rollback_preview_batch 與 commit_batch。token 只在同一 EngineInstance、transaction generation
內有效，不持久化、不跨 restart。任何 prepare 失敗都 rollback 已建立 preview，且不 persist；
使用者取消或 preview timeout 也按反向 stable order rollback。rollback 無法證明完成時，client
進入 safe ProcessRestart，不能假裝舊畫面已恢復。persist 之後 callback crash 不撤回 durable
value；host 隔離該 source、報 stable diagnostic，並依 impact reactivate／restart。

Immediate 在 UI frame 或 fixed-tick owner barrier commit。WorldReactivate 先保存 staged value，
再走正常 world shutdown／reopen。ProcessRestart 使用 pending journal 與下一次 boot confirmation；
新程序無法確認時保留舊 confirmed snapshot並進 recovery shell。包含多個 durability domain 的
UI draft 必須在確認頁拆成多個明文 transaction，不能提供虛假的跨 store atomicity。

### 6. Graph-affecting profile draft

Settings surface 以 ProfileDraftV1 編輯 graph-affecting parameter。draft 至少記錄 base profile
hash、base lock hash、ordered generated overlay 與 author provenance；不原地改寫使用者原始
Nickel source。

Apply 執行 deterministic resolve，先產生 candidate lock 與下列完整 diff：

- package／capability provider 與 version；
- feature、parameter 與 realization；
- registration／schema／semantic／settings fingerprint；
- trust、source build、download、migration 與 EngineBuildId 影響。

candidate artifact acquisition、build、trust check 與 world preflight 在 publish 前完成。已存在
world 的 frozen lock 不會被靜默替換；使用者只能繼續 frozen lock、為新 world 使用 candidate，
或 checkpoint／clone 後明確 upgrade。成功 publish 新 lock 後建立新的 client process／
EngineInstance；GraphRecompose 不產生 SettingChangedBatch。首個 conformance consumer 是
@example/dual-gameplay 的 static／portable realization preference。

### 7. Static handle、numeric key 與 dynamic batch

SDK registration IR 是唯一 declaration source。它生成：

- manifest SettingSpec fragment；
- static typed SettingHandle<T>、codec 與 Bevy binding；
- RegistrationImage callback map；
- C header、Rust binding 與 portable batch adapter。

RegistrationImage 對 Setting StableId 做 canonical byte order 排序，再分配 dense
SettingKey(u32)。mapping row 同時攜帶 registration epoch 與 value-type fingerprint。
SettingKey 只在一個 EngineInstance／RegistrationImage epoch 內有效，不進 manifest、lock、
save、report identity 或跨 restart message。

static path 綁定一次後直接用 typed handle，不做 per-frame string lookup，也不經 C ABI。
portable dynamic module在 create／start 收到 ordinal-to-key mapping；讀取與 change callback
都按 request batch，一次跨 FFI 處理多個 key。逐 setting、逐 entity 或逐 voxel callback 是
conformance failure。

### 8. Diagnostic callback、freshness 與 error

InfoItem、DiagnosticMetric 與 InspectFragment source 共用 owner-level
SampleRequestBatchV1／SampleBatchV1 envelope。request 包含 EngineInstance epoch、world／target
epoch（適用時）、subscription generation、stable-sorted numeric keys、deadline 與 byte budget。
response 對每個 requested key 恰好回一項：

- Value：typed value、observed fixed tick／monotonic instant、source revision；
- Unavailable：typed reason，例如 no-world、no-target、headless、not-supported 或
  source-not-installed；
- Error：stable diagnostic code、bounded typed arguments 與 retryability。

省略 requested key、重複 key、wrong type、超 budget 或 unknown key 都是 source protocol
error；不能用 zero、empty string 或 stale previous value冒充 unavailable。host 依 spec TTL、
current world／target revision 與 observed metadata推導 fresh／stale；package不能自行把資料
標為 fresh。world／target epoch不符的 response直接丟棄，不顯示成 stale。metric history由
host擁有，module只回本批 sample。

### 9. Subscription planner 與首版 budget

subscription 只由 visible layout、pinned item 或明確 report request 建立，並按 source／key
reference-count。最後一個 consumer 離開時 generation 增加、async request 取消、專用 Bevy
system run condition 關閉；舊 generation sample 到達時丟棄。共享的 upstream low-cost base
counter可以繼續存在，但關閉 panel 後不得執行 target query、chunk scan、contact collection
或 dynamic callback。

第一版 D2 hard policy：

| 項目 | 值 |
| --- | --- |
| source／visualizer update rate | 最多 10 Hz |
| chunk visualizer radius | default 4 chunks；hard 8 chunks |
| physics visualizer radius | default 16 m；hard 32 m |
| commands | hard 8,192 primitives／update |
| verified upload | hard 512 KiB／update |
| enabled collection + layout CPU | P95 不超過 0.5 ms／update |
| overlay render CPU | P95 不超過 0.5 ms／frame |
| overlay GPU | P95 不超過 0.5 ms／frame |
| disabled dedicated work | callback／query count 0；upload 0；額外 CPU P95 不超過 0.05 ms／frame |

history 必須由 spec 宣告有界 sample count／bytes；D2 world-space visualizer預設只保留 current
sample。超 budget 時依序降低 update rate、radius、history，再按 stable primitive key
deterministically truncate；仍無法遵守時停用 source並顯示實際限制。不得 allocation-grow、
queue-grow 或移除 hard cap來維持畫面完整。D10 在 reference hardware 上重跑代表 workload；
若量測要求改數字，必須用後續 performance ADR，而不是靜默改 default。

### 10. Target inspect permission、gating 與 cache

target key 只能來自 authoritative voxel DDA／gameplay selection。server或單機 authoritative
host在執行 provider 前重驗 player pose、5 m reach、line of sight、target revision、permission
與 disclosure policy；不信任 client傳入的命中結果。

InspectRequestV1 包含 request ID、server／world／player identity、target key與revision、
policy revision、tool／progress revision、catalog fingerprint、disclosure level、subscription
generation與budget。每個 fragment回：

- Value；
- HiddenByWorldPolicy；
- RequiresTool；
- RequiresProgress；
- Unavailable；
- Error。

HiddenByWorldPolicy 與 gated state 不允許 client local fallback或cache旁路。Unavailable 只表示
技術上沒有provider／資料，不能替代policy denial。cache key包含上述 identity、revision、
policy、tool、progress、catalog與disclosure；不得跨player、server或world重用。target改變、
permission撤銷或任一revision變更即取消／失效。首個demo以loopback adapter走同一 protocol，
不能為single-player新增無權限的捷徑。

### 11. Debug visualizer command contract

DebugVisualizerSpecV1 必須宣告 coordinate space／unit、allowed primitives、structured legend、
default depth mode、pick schema、permission、radius unit／default／hard max、update rate、
primitive／upload／history budgets及headless fallback。

V1 primitives 只有 Line、Polyline、WireAabb、WireObb、Sphere、Capsule、TextLabel與
allowlisted MeshInstance。MeshInstance只能引用host已驗證的presentation asset，不能傳raw
Bevy handle或任意render command。depth-tested是default；x-ray須由使用者明確啟用且仍受
permission／budget。legend除顏色外同時提供line style、symbol或label。pick ID只在該次
sample內有效，選中後由typed inspector顯示stable source資料。

host先驗證全部command，再套用第9節hard policy。invalid command只停用該source並記錄
diagnostic，不改world。headless只允許 report-only 或 Unavailable，永遠不建立GPU resource，
upload為zero。

### 12. Client shell／world 一律使用 process restart

首版client不在同一process順序建立兩個 fresh DefaultPlugins App。winit的跨平台契約要求
EventLoop在主執行緒建立，且一個application只能建立一次；window、GPU device、event loop、
process-global plugin state與native realization teardown也沒有足夠證據支援可靠重建。

因此 shell → world、world → shell、world →另一world 的預設路徑都是：

1. 在目前process完成resolve、artifact acquisition與read-only preflight；
2. 到達正常shutdown barrier，flush settings，若有world writer則確認durable revision；
3. stop package instances，取消／join有界tasks並關閉writer；
4. atomic write LaunchIntentV1；
5. 結束目前process，由launcher啟動一個只建立一次 DefaultPlugins App 的新process。

LaunchIntentV1 至少包含 generation、target Shell／WorldId、shell lock hash、可選world lock hash、
WorldOpenPlan hash、confirmed setting transaction revision與attempt count；不含raw pointer、
Bevy handle、module handle或authoritative world data。新process先驗證intent與locks，再啟動
target；成功到達安全bootstrap state才ack。invalid／stale intent或一次world boot failure會
quarantine intent並進recovery shell，不能形成restart loop。

WorldReactivate 是setting的產品impact；在client上可以由process restart實作。ProcessRestart
則表示連shell/runtime bootstrap也必須重建。即使未來另以ADR允許某些same-process transition，
下列情況仍強制process restart：

- window／GPU backend或startup-only Bevy setting改變；
- EngineCoupledNative 的 EngineBuildId／artifact改變；
- realization宣告或偵測到process-global、TLS、foreign thread或unknown multi-instance safety；
- task、GPU、callback或package instance無法在shutdown barrier證明quiescent。

headless／test仍可用 MinimalPlugins 在同process建立多個有明確生命週期的 EngineInstance，
以驗證instance isolation；它不證明client DefaultPlugins App可重建。Portable native library
維持no-unload；client process exit才真正釋放它。

### 13. Bevy UI、focus、IME 與 accessibility

client固定使用 bevy = 0.19.1，default features加bevy_settings。DefaultPlugins已提供首版所需
Bevy UI、bevy_ui_widgets、bevy_input_focus、EditableText與AccessKit integration。
bevy_feathers與bevy_dev_tools不屬於shipped player surface contract；dev build可明確啟用，
Feathers只用於experimental developer workbench，不成為setting、world或inspect schema。

所有surface由catalog／layout schema組合。package提供typed rows／fragments，不提供任意screen
coordinate、raw rich text或自己的root overlay。上游 diagnostics overlay／gizmo是building
block，不能取代Lattice的stable ID、scope、permission、subscription、budget與composition。

V1 UI gate：

- 800 × 600 logical viewport、UI scale 1.0與2.0時，primary action與error recovery均可達；
- overflow使用scroll、tabs或collapse，不讓focusable control落在不可達viewport外；
- async world list更新後以stable item key恢復focus，不以entity／row index猜測；
- world name、search與setting string field接受composition／IME events；
- bundle具CJK glyph coverage的fallback font，不依賴Bevy ASCII-only default font；
- mouse、keyboard與controller可完成shell、settings與workbench核心journey；
-每個control產生role、name、value、description、state與action的AccessibilityNode語義。

CI使用layout bounds、focus traversal、IME event injection與accessibility-tree golden；首版不把
pixel screenshot、GPU visual comparison或特定OS screen reader人工操作設為blocking gate。

### 14. PlayerMovementProfileV1

權威movement只在60 Hz FixedUpdate前進。首版profile固定：

| Field | Value |
| --- | --- |
| capsule total height | 1.80 m |
| capsule radius | 0.35 m |
| eye height | 1.62 m |
| maximum walk speed | 4.50 m/s |
| walkable slope | ≤ 45 degrees |
| maximum step | ≤ 0.60 m |
| jump apex above takeoff | approximately 1.25 m |
| jump input buffer | 100 ms＝6 fixed ticks |
| coyote time | 100 ms＝6 fixed ticks |

world coordinates沿用Bevy原生Y-up、forward -Z。jump impulse／gravity等backend parameter由
adapter推導，但自動journey的apex與landing結果必須符合profile；不得把Avian component layout
變成save或portable contract。

V1只有walk、look、jump。sprint、crouch、prone、swim、climb、vehicle與player fly／noclip延後。
dev-tools可提供detached spectator camera：它不改player transform、velocity、collision、world
command origin、save bytes或authoritative state hash；關閉時camera回到目前player eye anchor。
spectator需要developer permission，release gameplay不以隱藏快捷鍵啟用。

### 15. Break／place command

break與place共享5.0 m maximum reach及12 fixed ticks（200 ms）successful-mutation cooldown。
cooldown在成功world mutation後開始，兩種action共用同一counter，不能交替繞過。無效request另受
bounded command rate限制，但不消耗successful-mutation cooldown。

BlockEditIntentV1只表達action、input generation、可選placement content與client observation；
authoritative host以fixed-tick player eye pose重新執行Y-up voxel DDA，選第一個符合selection
policy的cell，驗證line of sight、reach、permission與expected chunk revision。break修改命中cell；
place修改命中face相鄰cell，並驗證replaceable／fluid occupancy、placement rule、support及不與
player authoritative capsule相交。presentation raycast可以預覽，不能決定結果。

拒絕使用stable typed result，V1 code至少包含 NoTarget、OutOfReach、Occluded、Cooldown、
StaleRevision、NotBreakable、NotReplaceable、WouldIntersectActor、RequiresTool、
RequiresProgress、NoPlacementContent、PermissionDenied、ContentUnavailable與StorageUnavailable。
typed arguments包含remaining ticks、expected／actual revision或required tool等適用資料；不解析
上游英文error string。成功結果包含coordinate、old／new stable content與committed chunk revision。

selection surface明確區分 valid、invalid、pending-authority與stale；聲音、outline、particles及
error toast只由typed result派生，prediction被拒絕時撤回。static與portable
@example/dual-gameplay realization必須對同一intent產生相同command／state hash。

### 16. 最小可理解目標：Restore the Terrenia Waymark

第一個world template在spawn附近放置一個由D3六方塊集合構成、缺少copper-block中心的obsidian
waymark，並在旁邊放一個錯誤stone center。@terrenia/gameplay提供可localize、可dismiss的
「Restore the Terrenia Waymark」journey：

1. inspect waymark並取得「remove the stone center」提示；
2. 以break command移除stone；
3. 從首版sandbox placement palette選擇copper-block並place到標記cell；
4. authoritative predicate確認指定cell與waymark structure後完成。

這不是D8 inventory／drop系統的替代；首版placement palette不宣告物品擁有或轉移語義。goal
state由Terrenia package擁有、進world snapshot並在restart後保持；UI celebration是presentation，
不進world hash。goal不封鎖free-build sandbox，也不硬編碼在host。headless command-input fixture
必須完成同一路徑、save、restart、preflight與reopen。

### 17. 首個OS／GPU／input matrix

首個client performance reference是：

- Windows 11 x86_64；
- 6 physical cores／12 threads；
- 16 GiB system memory；
- 6 GiB VRAM；
- 1920 × 1080；
- D3D12。

Linux x86_64是required headless CI target；Windows x86_64至少執行compile、unit、headless
command-input與save／reopen corpus。reference GPU上的client性能capture進D2 adoption report，
但GPU／visual test不進CI。macOS graphical certification、Linux graphical certification、
integrated GPU、handheld、touch與mobile延後到有明確distribution target時決策。

InputBindingV1至少覆蓋keyboard／mouse與standard gamepad。兩者都能完成move、look、jump、
break、place、inspect、pause與surface navigation；keyboard-only可完成全部shell／settings／
recovery。CI對兩種action stream注入相同logical actions並比較fixed-tick command與state hash；
physical controller與OS IME的人工smoke是可選證據，不是blocking gate。

### 18. Bevy、voxel、physics 與 input adoption

Bevy精確鎖定0.19.1；任何升級走獨立migration。首版client使用DefaultPlugins +
bevy_settings，headless使用滿足實際consumer的最小標準Bevy plugin composition。

bevy_voxel_world只作working-set／presentation adapter候選：streaming、meshing與可丟棄的local
raycast acceleration可以使用它；authoritative chunk、stable block ID、revision、snapshot、
world command與DDA result由Lattice schema／storage擁有。plugin numeric material、entity、
handle、chunk cache或job ID不進save。每個async mesh／collider result攜帶world epoch與source
chunk revision；stale result必須拒絕。若candidate無法維持此邊界，先上游extend或換adapter，
不能把authority轉交plugin以求通過spike。

Avian 0.7是第一個physics candidate；exact patch、source與checksum由workspace manifest及
Cargo.lock鎖定，features由workspace manifest與adoption receipt記錄。
它只有在下列gate全部通過後才算adopted：

- 60 Hz Y-up character、45°／46° slope與0.60／0.61 m step boundary；
- jump、chunk boundary、voxel collider create／replace／unload；
- ray／shape query與authoritative voxel DDA結果一致；
- collider result用chunk revision拒絕stale job；
- reference workload的fixed-step、collider latency、memory與queue符合已接受performance budget；
- local-origin precision報告證明D2範圍可用，或提供不改stable world coordinate的FixedUpdate
  rebase adapter。

任一gate失敗先比較另一個維護中的Bevy-native physics plugin與upstream extension，再考慮窄的
domain adapter；本決策不授權自製solver。

Leafwing Input Manager 0.21是首版action mapping選擇；exact patch、source與checksum由
workspace manifest及Cargo.lock鎖定，features由adoption receipt記錄。Lattice擁有
PlayerActionV1與InputBindingV1；Leafwing ActionState／Rust type只留在static client adapter。
headless test injection與portable package只看Lattice action／command DTO。若0.21在Bevy 0.19.1
build、rebind、gamepad或headless injection gate失敗，adoption report可退回Bevy native input，
而不改setting、action或save contract。

### 19. Bevy UI／diagnostics的採用結論

Bevy UI、widgets、focus、EditableText、AccessKit、diagnostics與gizmos足以作首版implementation
building blocks；它們不單獨滿足package／save／performance overlay的產品語義。Lattice只增加
本決策凍結的薄層：stable IDs、catalog/schema、scope/authority、permission、subscription、
budget、layout composition與portable batches。不得再建立平行widget tree、input focus runtime、
diagnostics scheduler或debug renderer。

Feathers只允許dev workbench實驗，不作player-facing compatibility promise。任何未來custom
settings editor、rich inspect fragment或GPU debug pass都必須有至少一個無法由V1機械widget／
primitive表達的真實consumer、generic fallback及新的versioned capability。

## 明確延後與後續 Gate

- client same-process sequential DefaultPlugins App、SubApp shell或長存event-loop App需要獨立ADR、
  Windows／Linux／macOS teardown matrix與process-global realization audit；不是R4優化捷徑。
- Avian、bevy_voxel_world與Leafwing的exact patch／source／checksum、features、license、
  advisory、unsafe與fallback由各自adoption report、workspace manifests及Cargo.lock共同證明；只有Bevy 0.19.1在本決策中已精確凍結。
- D10以第17節reference hardware凍結全局frame／fixed-tick／chunk／I/O budget；第9節debug
  surface保守hard limit在後續ADR明文修改前有效。
- full multiplayer transport、remote inspect server、prediction／rollback與anti-cheat實作延後；
  loopback仍須遵守同一authority／permission schema。
- sprint、crouch、swim、climb、fly／noclip gameplay、完整inventory／drop／tool progression與
  新biome不阻塞D2；它們依roadmap在D7..D11加入。
- pixel-perfect visual golden、特定screen reader人工認證與實體controller lab不作首版CI gate；
  layout、focus、IME、accessibility tree與logical action parity仍是自動gate。

## 取捨

- process restart增加shell／world切換時間，但把winit EventLoop、GPU、native no-unload與
  process-global state的風險變成可恢復、可測的LaunchIntent，而不是假設teardown正確。
- 固定setting precedence與有界predicate限制了任意配置語言，但保住deterministic catalog、
  headless parity與portable batch。
- debug overlay的保守上限可能截斷資料；顯示「部分資料」比讓diagnostics本身破壞frame budget更
  誠實。
- 首版movement與waymark goal刻意狹窄；它先建立可重播、可保存、可由兩種realization驅動的
  playable contract，再由後續sandbox systems擴充。
- 選擇Leafwing與Avian candidate降低自有控制碼，但stable action／physics boundary仍由Lattice
  擁有，因此adoption失敗不會污染save或portable ABI。

## 自動驗證

下列證據是本決策的出場條件；不要求pixel或人工視覺測試：

1. package graph golden證明七個package、capability range、exactly-one conflict、optional
   dev-tools與client／headless projection；隨機discover order不改lock。
2. SettingValue／Spec／predicate／constraint positive與negative corpus涵蓋boundary、wrong type、
   cycle、undeclared cross-package ref、depth 8／9與node 32／33。
3. precedence table與provenance golden涵蓋每個authority lane；device／user不能覆蓋world，
   timestamp與record order不改effective hash。
4. local file與RocksDB fault injection證明old-or-complete-new transaction；cross-domain draft會
   明確拆分，orphan／migration不丟資料。
5. prepare／preview／cancel／timeout／rollback／persist crash／callback panic fixture證明沒有
   partial apply；world-reactivate與process-restart journal可恢復。
6. ProfileDraft fixture產生deterministic graph diff與新lock；frozen world不被靜默改寫，
   static／portable realization preference走同一preflight。
7. generated manifest、typed handle、C／Rust binding與numeric mapping hash一致；restart後舊
   SettingKey被epoch拒絕，dynamic callback數隨batch而非item增加。
8. sample batch涵蓋Value／Unavailable／Error、omission、wrong type、fresh／stale、world／target
   epoch race與stable truncation。
9. 關閉workbench後專用callback／query與upload計數為zero；enabled／disabled CPU、render CPU／GPU、
   primitive與upload limit在optimized reference capture中有P95 evidence。
10. inspect loopback涵蓋hidden、tool、progress、unavailable、permission revoke、target change與
    cache revision；client無法從presentation fallback取得hidden fragment。
11. visualizer command fuzzer拒絕invalid primitive／asset／depth／pick；budget truncation排序穩定，
    headless不建立GPU resource。
12. LaunchIntent fault corpus涵蓋atomic write、stale／corrupt intent、spawn failure、world boot
    failure、attempt-loop prevention與recovery shell；client process一次只建立一個EventLoop／App。
13. 800 × 600、scale 1.0／2.0 layout bounds、keyboard／controller focus traversal、IME event與
    AccessibilityNode golden在無GPU test通過。
14. movement command-input測試涵蓋60 Hz、speed、jump apex、100 ms buffer／coyote、45°／46°坡面、
    0.60／0.61 m台階、chunk seam及detached spectator零authoritative diff。
15. break／place測試涵蓋5 m boundary、12／11 tick cooldown、DDA occlusion、face placement、
    actor overlap、stale revision、全部typed errors及static／portable equivalence。
16. Restore the Terrenia Waymark由keyboard／mouse與gamepad logical stream完成；save bytes、
    restart後goal與world state、headless state hash一致。
17. Windows x86_64與Linux x86_64執行locked build／test corpus；reference Windows D3D12的
    Avian／voxel／overlay optimized report記錄exact lock、features、workload與P50／P95／P99。

## 問題追蹤

| Open question | 本決策 |
| --- | --- |
| SET-01 | 第1節 |
| SET-02 | 第2節 |
| SET-03 | 第3節 |
| SET-04 | 第4節 |
| SET-05 | 第5節 |
| SET-06 | 第6節 |
| SET-07 | 第7節 |
| SET-08 | 第8節 |
| SET-09 | 第9節 |
| SET-10 | 第10節 |
| SET-11 | 第11節 |
| SET-12 | 第12節 |
| SET-13 | 第13節 |
| PLAY-01 | 第14節 |
| PLAY-02 | 第15節 |
| PLAY-03 | 第16節 |
| PLAY-04 | 第17節 |
| PLAY-05 | 第13、18節 |
| PLAY-06 | 第18節 voxel boundary |
| PLAY-07 | 第18節 Avian gate |
| PLAY-08 | 第18節 Leafwing decision |
| PLAY-09 | 第9、11、13、19節 |

## 相關文件

- [採用 Bevy upstream-first](0014-adopt-bevy-upstream-first.md)
- [Bevy 原生 Y-up 世界座標](0015-bevy-native-y-up-world-coordinates.md)
- [版本化 Native Module ABI](0017-versioned-native-module-abi.md)
- [凍結 R0／R1 Package、Nickel 契約與解析政策](0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md)
- [Package 可注入的設定與配置架構](../architecture/settings-and-configuration.md)
- [Package 可組合的診斷、檢查與除錯可視化](../architecture/diagnostics-inspection-and-debug-visualization.md)
- [World 目錄、開始頁與安全生命週期](../architecture/world-lifecycle-and-start-ui.md)
- [Entity、物理與表現](../architecture/entity-physics-presentation.md)
- [第一個完整 Demo 路線圖](../planning/roadmap-first-demo.md)
- [執行期路線圖](../planning/roadmap-game-engine.md)
