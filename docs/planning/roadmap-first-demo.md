---
title: 第一個套件驅動的 Bevy 可玩 demo 路線圖
status: active
type: roadmap
updated: 2026-08-21
decision:
  - ../decisions/0008-static-and-dynamic-realizations-share-one-graph.md
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0017-versioned-native-module-abi.md
  - ../decisions/0018-package-kernel-from-first-vertical-slice.md
  - ../decisions/0019-separate-package-and-registration-identities.md
  - ../decisions/0020-semantic-registration-and-content-selection.md
  - ../decisions/0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md
  - ../decisions/0022-freeze-controlled-nickel-evaluation-metering-and-worker-protocol.md
  - ../decisions/0023-freeze-sdk-registration-and-semantic-compilation.md
  - ../decisions/0024-freeze-portable-native-abi-0x.md
  - ../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
  - ../decisions/0026-freeze-first-demo-performance-budgets.md
  - ../decisions/0027-freeze-authoritative-world-and-persistence-contract.md
  - ../decisions/0028-freeze-worldgen-content-and-asset-contract.md
  - ../decisions/0029-freeze-render-capability-and-provider-contract.md
  - ../decisions/0030-freeze-governance-distribution-and-security-triggers.md
  - ../decisions/0031-freeze-bevy-upgrade-dependency-and-supply-chain-policy.md
  - ../decisions/0032-freeze-local-package-acquisition-imports-and-product-lock.md
---

# 第一個套件驅動的 Bevy 可玩 demo 路線圖

## Demo 定義

> 玩家从package-driven开始页选择或创建world，经frozen lock预检进入由`terrenia`定义的Terrenia Y-up体素维度，能看懂准星指向的方块、挖掉并放回方块；退出再进入后world仍保留修改。同一最小gameplay package可切换static／portable dynamic realization，而registration、settings、玩法结果、diagnostics与存档不变。

这是可玩的 vertical slice，也是 package／ABI 的第一个真实 conformance consumer。它包含最小world／profile控制面，但不是marketplace／完整package manager、空动态库 loader 或 renderer benchmark。

D0–D6定义这条vertical slice；同一active roadmap继续以D7–D10把它推进到Terrenia 40自然世界、
通用sandbox systems、72方块＋2流体内容基线与自动sandbox完成旅程。D11是D10后的群系扩充轨。
后续阶段继承而不旁路D0–D6的package、semantic、persistence、upstream-first与dual-realization门禁。

## v1 playable freeze (V0–V9)

**v1 playable** is D0–D10 sandbox completion on this roadmap. It is not ABI 1.0,
a public registry, multiplayer, or a complete content catalog. This document
does not grant world-writer authority and does not replace
`ActivationEvidenceUnavailable`.

V0–V9 are the implementation sequence for those existing gates. They do not
rewrite D0–D10, skip D1–D6, or pull D11 into v1:

- **V0** freezes the playable fixture, world-session contract, journey fixture,
  crate/package reuse-adapt-replace inventory, and this mapping. It is not a
  D-phase delivery.
- **V1** closes **D0** lock and launcher execution.
- **V2** is the production spine: **D1** dual-realization gameplay and **D2**
  voxel playground, replacing the in-memory fixture as the playable path.
- **V4** closes sparse **D4** streaming / operationally unbounded generation
  *before* durable save. Content stays thin; dirty chunks stay resident.
- **V3** closes **D3** durable world lifecycle on that already-streaming world.
  Writer activation remains unauthorized until a sealed receipt exists.
- **V5–V6** close **D7** complete terrain and caves on the V4 coordinator.
- **V7** closes **D8** tools and gameplay.
- **V8** closes **D9** content and UX.
- **V9** closes **D6** delivery/regression and **D10** budgets and release.
  **D5** render composition remains a D0–D6 gate and is accepted with D6 at V9.

Execution order: `V0 → V1 → V2 → V4 → V3 → V5 → V6 → V7 → V8 → V9`.

v1 also does not require infinite vertical terrain; dungeons, mobs, combat, or
seasons; or iron/copper tool ladders, weapons, armor, enchantments, or repair.
D11 remains a post-baseline biome expansion track after V9.

## 固定基線

- 非可执行`latticeaxiom.toml`／`latticeaxiom-package.toml`先授权roots、sources与graph inputs；
- local path／catalog package快照进CAS，locked alias map再供Nickel `package.ncl`／`game.ncl` import；
- package SemVer、capability／realization resolution与包含source／manifest／toolchain／artifact receipts的精确产品lock；
- `RegistrationManifest`／`RegistrationImage`；
- Nickel-authored SemanticTag／Map／Predicate／Role 与 locked fallback ContentBundle；
- package-injected `SettingSpec`、observability items与统一settings／inspect／dev-tools surfaces；
- `NativeStatic` 与 `PortableNative` ABI `0.x`；
- Bevy `0.19.x` architecture baseline与精确 `0.19.1` first implementation；manifests／Cargo lock记录version、source、checksum，manifests／profile／build receipt记录features；
- client `DefaultPlugins`，headless 使用必要标准 profile；
- Bevy 原生右手 Y-up；
- voxel／physics／input／assets／diagnostics 先采用 Bevy／生态 upstream；
- RocksDB 保存完整已物化 chunk snapshot；
- package-driven client shell、WorldHeader／catalog与write-before preflight；
- Terrenia／test content 都经 package graph；host 没有内建默认维度；
- `terrenia` 是普通可替换root模组；host与平台contract不引用Terrenia concrete content；
- 无自研 ECS／scheduler／renderer／asset server。

## 两条同时验收的循环

### 玩家循环

1. 从开始页选择／创建world，并看到health／lock／durability摘要。
2. preflight从frozen lock建立游戏closure，任何写入前处理compatibility。
3. 在Y-up voxel terrain出生。
4. target inspect显示方块名称；walk／look／jump、raycast、break／place；9 格热键栏可切换；打开背包；Pause 设定可调渲染距离（受 host hard clamp）。
5. mesh／collision在预算内更新；状态条显示挖掘进度／工具耐久／活力展示；debug preset能解释chunk／collider状态。
6. 退出／crash fixture后回到world catalog。
7. 已durable修改正确恢复；checkpoint／trash可由UI恢复。

### Package 循环

1. Bootstrap manifest选择`@example/dual-gameplay` root、local source provider与projection。
2. Kernel取得／快照source、解析exact graph并建立package-local alias map。
3. Nickel profile从locked source table import该package并产生typed `CompositionSpec`。
4. 以`NativeStatic`启动并记录final lock／registration／state hash。
5. 只切换realization为`PortableNative`并显式产生candidate lock。
6. Kernel验证artifact／ABI／manifest但不load native code，原子写入并reopen final lock。
7. Loader只从reopened final lock建立`RuntimeImage`。
8. 执行同一action fixture。
9. 比较IDs、schedule、effective settings、state、save与diagnostics。

任一循环失败，demo 均未完成。

## D0：Composition／Lock／Bevy Smoke

### 交付

- `CompositionBootstrapV1`／`PackageSourceManifestV1`及受限TOML representations；
- local Nickel profile／contracts、package-local `import alias`与source-table-only grants；
- `latticeaxiom.lib` semantic constructors／contracts／concat与binding overlay；
- core／empty game packages；
- `@latticeaxiom/settings`／observability基础packages与manifest schema skeleton；
- workspace／path snapshot、immutable CAS、local catalog check／pack／publish／acquire；
- deterministic SemVer／capability resolution与`ResolutionReceiptV1`；
- portable resolution／target realization分层的产品lock、atomic read／write及locked／offline／frozen modes；
- shell／client／headless profile与同schema的独立shell lock；
- launcher／`LaunchIntentV1`／recovery shell process-transition state machine；
- package-driven Bevy App；
- package-driven开始页／设置页smoke、placeholder camera／light／cube；
- Y-up orientation、manual fixed-time、diagnostics／CI。

### 完成

- 同一final lock可建立client／headless的compatible authoritative closure，且两者不各自重新resolve；
- local A package经manifest alias import B；undeclared transitive、ambient、escape与missing alias错误保留exact `pkg://` source span；
- path source修改后旧lock仍读取原CAS bytes；`--frozen`对missing／tampered source、manifest、toolchain、`EngineBuildId`与artifact精确失败且不fallback；
- local check／pack／publish-to-directory／acquire／resolve／run-frozen fixture全程无network；
- lock fault corpus覆盖temporary write、flush、atomic replace、reopen与corruption，只接受旧完整或新完整lock；
- client 显示场景并退出；headless无 GPU推进 fixed tick；
- client process一次只建立一个`EventLoop`／fresh `DefaultPlugins` App；LaunchIntent fault corpus覆盖barrier失败、atomic write、stale／corrupt intent、spawn／boot失败与recovery loop prevention；
- 没有 hidden hand-written first-party plugin／dimension list；
- shell／headless都从graph选择exactly-one settings／observability provider；
- 没有自有 runner／ECS／renderer；
- dependency／license 与 lock metadata可重建。
- CLI／embedded Nickel对semantic intent产生相同typed output与source-aware conflict。

## D1：真实 Dual-Realization Gameplay Fixture

### 交付

- SDK／proc macro prototype；
- `@example/dual-gameplay` 的一个真实 component + fixed system + command；
- generated manifest／static glue／C binding／dynamic batch shim；
- portable ABI entry、instance lifecycle、batch／command／diagnostics；
- dual package贡献一个typed setting、metric与inspect fragment的generated static／dynamic callback；
- equivalence harness 与 static／dynamic／per-entity反例 benchmark。

### 完成

- single-source business code 同时构建两种 realization；
- registration hash、IDs、schedule 与 N ticks state hash一致；
- static direct path 不经 C ABI／可启用 LTO；
- dynamic FFI 次数随 system／batch，不随 entity线性往返；
- static／dynamic的setting read与diagnostic sample语义一致，不逐item跨FFI；
- wrong ABI／manifest／panic／stop-after-callback 有 deterministic test；
- native library不卸载。

D1 不是空 `hello_plugin`；system 必须读写 gameplay-shaped data并发 command。

## D2：Upstream Voxel Playground

### 交付

- `bevy_voxel_world` 优先 spike；
- Avian 优先 physics spike；
- Leafwing `0.21`作为adoption-gated首版action／input mapping；headless注入相同Lattice `PlayerActionV1`／authoritative command DTO，失败回退Bevy native input；
- walk／look／jump、raycast、break／place；
- `@latticeaxiom/inspect`显示target block名称、icon、owner与technical StableId；
- Performance preset与chunk Grid／Lifecycle／Mesh／Collision visualizers；
- selection shape、generated collider、broadphase AABB与break／place raycast分层显示；
- dual gameplay package驱动 break／place 的一项真实规则。

### 完成

- clean checkout可产生可分发build，command-input fixture完成相同操作；外部测试者体验作为可选研究；
- chunk boundary／six Y-up faces正确；
- edit-to-visible、frame、fixed tick、memory、queues 已量测；
- 关闭／未订阅panel不执行昂贵target／chunk／contact采集，overlay overhead已量测；
- 800×600／高UI scale无overflow，visualizer有legend、radius与primitive budget；
- 每个 upstream缺口有 reproduction／adoption结论；
- static／dynamic切换仍完成同一操作。

D2 前不自写 voxel renderer／physics solver。局部缺口依决策 0014 走 upstream gate。

## D3：Authoritative Chunk、Package Content 與 Persistence

### 交付

- stable block ID、chunk revision、dirty／durability state；
- Terrenia D3 六方块语义与持久化最小集：air／grass／dirt／stone／copper-block／obsidian；
- `terrenia` 维度 closure／`@example/marker` test package；
- validated content／schema registry；
- typed Tag DAG／bitset、一个typed Map、shared StateProperty、Predicate与Role binding；
- normal copper／obsidian fixture与一个声明式fallback ContentBundle；
- `MemoryWorldStorage`／RocksDB；
- snapshot envelope、atomic batch、checkpoint；
- WorldId／WorldHeader／catalog、health与last-played sort；
- package-driven首页／world list／Quick Create／loading stages；
- exact frozen-lock preflight，以及missing package／schema／unclean shutdown状态；
- checkpoint／clone／move-to-trash／restore最小操作；
- unload／reload、normal shutdown／crash fixtures；
- world metadata的 required package／schema closure；
- world metadata的authoritative semantic image、active bundle与frozen Role binding；
- recoverable read-only open与checksummed world export；export至少包含checkpoint、header与exact lock。

### 完成

- 修改跨重启保留，已物化 chunk不重生；
- stale async result不覆盖新 revision；
- Terrenia／test packages无私有 registration；
- 两个不同copper IDs可共享input Tag而保持identity；Role output在world command前解析成concrete ID；
- broad obsidian Tag不自动授予portal-frame行为，明确mechanic semantic后才通过；
- 新world可不激活fallback，旧world重开保持frozen activation；
- discovery／load order不改变 IDs／save；
- static／dynamic gameplay产生相同 snapshot bytes（排除允许的 diagnostic metadata）；
- save不含 Bevy／ABI／physics handles；
- missing authoritative content有明确 recovery／拒绝；
- Continue只对`ReadyExact`world启用；损坏header仍在catalog显示且不阻塞其他world；
- preflight不执行module business code、不打开writer；migration fixture先checkpoint／clone再发布；
- low-disk fixture阻止危险新mutation并提供persistent、可行动警告；
- `RecoverableReadOnly`不建立writer，export bundle可在独立临时目录验证checksum并由headless preflight读取。

## D4：最小程序世界

### 交付

- Terrenia world seed／`WorldgenConfig`；
- 两个 placeholder terrain／biome styles；
- Terrenia block catalog由D3的6个扩展到D4累计18个，使两种地表、浅层与基础洞穴可由材质辨认；
- deterministic height（`y`）与最小 cave／void；
- generator package／capability；
- revision／config hash／provenance；
- terrain／structure content Predicate与placement Role receipt；
- new chunk snapshot-first materialization。

### 完成

- 相同 seed／`(x, z)`／config／locked provider 产生相同 chunk；
- task／chunk ordering不改变结果；
- generator更新不改变 old snapshot；
- exclusive terrain provider conflict fail-fast；
- 玩家／package循环仍完整。

不要求第一版完成 territory、hydrology、geology 或完整 cave topology。
D3／D4的精确内容与后续40／72方块范围见[Terrenia 方块内容规划](terrenia-block-catalog.md)。

## D5：Render Composition 與 Upgrade Rehearsal

### 交付

- RenderData／Feature／Pass／Provider registration；
- 两个 composable post／compute fixtures；
- default + fake hardware-specific terrain backend providers；
- GPU requirement／fallback；
- portable dynamic compact render command prototype；
- 一个窄的`EngineCoupledNative` exact-build conformance fixture；它只证明
  `EngineBuildId`拒绝／重建与fallback生命周期，不因此建立通用renderer internal API；
- 一次 Bevy upgrade branch rehearsal；
- portable old binary保留不重编；
- chunk Rendering layer显示visibility／LOD／provider／GPU budget与stale mesh reason。

### 完成

- feature pair 顺序／resource graph稳定；
- exclusive provider conflict在 code load前失败；
- unsupported GPU选 fallback；
- render failure不改变 world hash；
- advanced chunk／render visualizer的启停、truncate与device failure不改变world hash；
- portable old binary跨升级按 contract工作；
- engine-coupled old artifact因 `EngineBuildId`拒绝，重编后通过；
- 未因 Bevy内部更新无故提升所有 package major。

## D6：交付與回歸

### 自动验收

- client playable／headless fixed-tick；
- Nickel／typed model／lock golden；
- SemVer／capability conflict matrix；
- static／dynamic equivalence；
- old-binary／bad-ABI／panic／lifecycle；
- save round-trip／old schema／crash atomicity；
- chunk revision race／queue bound；
- Y-up mesh／raycast／collider six faces；
- content discovery randomization；
- semantic Tag／Map／Role／fallback conflict与order-randomization corpus；
- frozen-world binding／bundle reopen；
- render feature／provider composition；
- 10 分钟 traversal memory／CPU／GPU／FFI metrics；
- checkpoint restore；
- recoverable read-only open／export bundle checksum与headless preflight；
- setting catalog order／scope／transaction／orphan golden；
- WorldHeader损坏、catalog scan、preflight、clone-migration、trash／restore；
- inspect permission／fragment conflict、subscription-off与visualizer budget；

### 可选人工研究（不阻塞自动出场）

以下项目用于后续体验研究；在当前允许跳过人工／视觉测试的交付策略下，不作为D6或D10
自动出场条件，也不得以“未发现问题”代替相应自动状态、输入与无障碍语义断言：

- 新测试者 5 分钟内完成 break／place／restart；
- 新测试者能从开始页create／continue，找到block name、Performance preset与chunk／collider visualizer；
- package／ABI／missing content错误可理解且有修复动作；
- world delete可撤销，migration／purge可读出对象、影响与恢复点；
- settings与world list可由keyboard／controller／screen narration完整导航，关键state不只靠颜色；
- static／dynamic切换无需修改业务 package source；
- profiler清楚标示 dynamic bridge／batch overhead；
- build／profile／lock／world recovery说明可重复。

## D7：Natural Terrenia 40

### 交付

- `@terrenia/blocks`由D4累计18个扩展到自然世界累计40个方块，精确集合以
  [Terrenia 方块内容规划](terrenia-block-catalog.md)为准；
- Terrenia production `DimensionGenerationCoordinator`、多尺度`Territory Atlas`与
  `terrain.base`递归领地委派；host不理解Terrenia biome或content ID；
- 至少三个可查询地表领地程序与两个在空洞生成前存在的地下群系领地；
- 一条跨地表领地连续的河流／流域计划、一个可查询地质体／岩层场、稳定资源分布与
  具排斥半径的植被点程序；
- 维度默认洞穴拓扑域、两个由地下群系接管且使用不同算法的子域、一个有界支洞贡献者；
- 跨至少四个规划格与两个拓扑域的稳定入口契约，以及地质方向场、地下河约束与一个
  必须连接的大型目的地；
- territory、boundary、worldgen provenance与cave arbitration diagnostic／visualizer资料源。

### 自动出场门禁

- 40个方块的完整ID集合、definition schema、semantic contribution与asset reference形成
  与发现／注册顺序无关的RegistrationImage；18→40只新增规定内容，不复制有限state为ID；
- 相同seed、lock、config与查询范围在chunk顺序、task并行度、package注册顺序和重启变化下
  得到相同territory、biome、river、geology、resource、vegetation与chunk bytes；
- 三尺度领地查询返回稳定第一／第二候选、边界距离与所有权；同层冲突不使用load order，
  子领地接管只作用于声明的空间影响与通道；
- 洞穴必要入口的位置、切向、净空与流体状态满足contract；连接率、可通行率、死路／环路、
  SDF求值、生成时间与拒绝原因均进入基线报告；
- river／geology／cave plan均有stable ID、有限bounds与dependency receipt；已物化D4 chunk不重算，
  新generation epoch在边界没有未解释接缝；
- headless固定输入遍历fixture能从地表进入两个地下群系并取得至少三类自然资源，期间queue、
  memory与生成时间不超过D10冻结前的临时安全上限。

## D8：Sandbox Systems

### 交付

- versioned item stack、player inventory、block drop、pickup与placement-item schema；
- mining hardness、tool class／tier、durability、break progress与明确失败原因；
- recipe contract、Tag／Predicate输入、frozen Role输出与一个无序／一个有序配方consumer；
- 通用workstation、furnace process与container owner-component schema；具体Terrenia方块在D9绑定，
  通用机制不进入`terrenia:*` host contract；
- authoritative scheduled processing、fuel／input／output slot transaction与离线／重载续行；
- `@terrenia/gameplay`的初始掉落、工具、配方与熔炼资料，以及独立fixture package证明机制可跨维度复用；
- inventory／recipe／machine／container inspect fragments、settings与diagnostics。

### 自动出场门禁

- command-input fixture完成“采集方块→拾取→合成工具→按工具规则加速采掘→放置产物”，
  所有quantity、durability与chunk revision守恒；
- recipe input接受显式兼容的第三方Tag成员，output在command前解析为frozen concrete ID；
  broad材料Tag不会自动取得tool、fuel或machine机制资格；
- inventory、掉落实体、container、furnace scheduled work在Memory／RocksDB、卸载／重载、正常退出／
  crash与static／portable dynamic切换后得到相同authoritative state与normative snapshot bytes；
- slot、pickup、craft与smelt transaction在invalid input、满背包、满output、stale revision和中途fault时
  保持原子，不复制或丢失item；
- fixture dimension可复用通用inventory／recipe／container机制而不依赖`terrenia:*`，移除Terrenia后
  host不保留其recipe、drop或tool默认值；
- 未订阅machine／inventory inspect时不执行专用扫描，所有dynamic callback按batch而非逐slot／item往返。

## D9：Terrenia 72 + Fluids

### 交付

- `@terrenia/blocks`扩展到精确72个方块定义与独立的`terrenia:fluid/water`、
  `terrenia:fluid/lava`；
- 每个定义的display、presentation asset、collision／occlusion／replaceable、physical data、
  mining／drop／tool／recipe／placement与worldgen语义资料；
- `axis`、`facing`、`lit`、`growth`、`half`与经prototype保留的`form`等有限BlockState schema，
  不展开为材料×状态StableId；
- oak／pine建材族、石材建材族、glass、copper-grating与roof-tiles的可取得配方；
- torch、workbench、furnace与chest接入D8机制；chest block entity与furnace continuation持久化；
- solid occupancy与fluid state的versioned palette／snapshot encoding，water／lava的有限level／flow state、
  collision／selection／inspect与最小有界流动规则；
- 完整Terrenia presentation assets；headless省略presentation package时权威registration与world hash不变。

### 自动出场门禁

- manifest检查精确得到72个block与2个fluid ID；朝向、点燃、成长、half、form、流体液位与流向均不
  产生重复content ID；所有asset／drop／recipe／Role／Predicate引用闭包完整；
- 从自然资源可经确定配方取得全部12种建材与4种功能方块；workbench、furnace、chest与torch的
  state／entity／scheduled-work round-trip通过；
- solid + fluid palette在正负Y-up chunk坐标、边界流动、卸载／重载、crash atomicity与schema migration
  fixture中保持canonical；未知fluid schema在writer打开前拒绝或进入read-only recovery；
- static／portable dynamic、client／headless、content discovery随机化均不改变authoritative IDs、
  semantics、recipe results或snapshot bytes；presentation-only差异不进入world hash；
- 72内容随机采样的break／drop／pickup／place round-trip不产生missing definition，工具门槛、collision、
  selection与occlusion均与definition schema一致；
- water／lava更新有每tick cell、queue depth与in-flight bytes硬上限，stale fluid task不覆盖新chunk revision。

## D10：Sandbox Completion

### 交付

- 一个固定seed、只依赖shipped package closure的自动玩家旅程：create → spawn → 地表探索 →
  进入洞穴 → 采集木／石／铜 → 合成工具与建材 → 熔炼 → 建造可辨识小型结构 → 使用容器 →
  checkpoint／退出／重进；
- normal、crash-recovery、static→portable realization switch与compatible reopen四条旅程；
- release profile的frame、fixed tick、active／resident／visible chunk、generation／mesh／collider／save P95、
  task／I/O／FFI queue与RAM／VRAM预算；
- package／world错误的可行动CLI与client semantic output，以及可复制的diagnostic report；
- 键盘／controller command-injection与UI semantic-tree回归；视觉辨认与五分钟新手体验保留为可选人工研究，
  不以未执行的视觉测试冒充自动证明。

### 自动出场门禁

- 完整旅程在fresh world、durable reopen、checkpoint restore和crash recovery后满足同一显式状态断言；
  已durable玩家建筑、inventory、container、machine与chunk revision逐项恢复；
- 10分钟固定路径traversal在目标测试profile内不超过冻结预算，不出现无界chunk／task／mesh／collider／
  persistence／fluid queue增长；所有P50／P95与high-water写入machine-readable baseline；
- static与portable旅程产生相同action receipt、authoritative state hash、semantic report与normative save bytes；
  profiler能分离static、dynamic bridge、worldgen、physics、render与persistence成本；
- low disk、missing package／content、bad schema、bad ABI、device loss、stale async与shutdown timeout的fault
  corpus均保持最近durable world可恢复，纯presentation失败不改变world hash；
- 另一root dimension package可执行同一通用inventory／recipe／persistence conformance子集，无host、platform
  semantic或foundation package引用`terrenia:*`；
- 所有D0–D10自动门禁由clean checkout、frozen locks和文档化命令重现，CI不要求window、GPU或人工输入。

## D11：Post-Baseline Biome Expansion

D10之后继续使用同一territory／boundary／cave／semantic公开契约扩充群系，不改变Terrenia 72基线
的完成定义。第一批扩充以湿地／河岸、寒冷高地、火山地表与发光深洞四个环境程序验证水文、
高度、lava／basalt、snow／ice、glow-moss／crystal等既有内容的重新组合；只有真实环境需要时才新增
方块或机制package。

### 自动出场门禁

- 四个环境均由package registration与领地仲裁产生，无host biome列表或ID分支；移除任一扩充package
  后其他环境、已有snapshot与frozen world binding保持可载入；
- 每个环境有稳定出现率／面积／边界／资源与洞穴形态统计，package顺序、并行度与重启不改变结果；
- river、geology与cave portal跨新增边界保持连续，预算拒绝与fallback可由diagnostics解释；
- 新内容若被引入，必须先通过D9 definition／semantic／persistence门禁，不以名称或load order取得机制。

## Demo 明確不做

- public／federated registry、marketplace／auto-update；
- general large-scale resolver；
- multi-platform build farm／distributed cache；
- native hot unload；
- production WASM sandbox／untrusted mod market；
- multiplayer rollback／lockstep；
- custom renderer／physics／ECS／scheduler／asset system；
- 完整 world editor／civilization simulation／侵蚀级全球水文模拟；D7只交付可验证的河流／流域连续性；
- cloud save／跨设备同步、server browser与通用旧world修复器；
- Godot runtime。

## Demo 完成定义

1. 玩家循环可由真实输入或确定性command-input fixture完整完成。
2. 所有 profile只从 package graph启动。
3. 一个真实 system 的 static／portable realization等价，static性能优势未被抹平。
4. save／crash证明权威 revision正确。
5. upstream adoption report没有猜测性自研替代。
6. render feature／provider证明机制级扩充可组合。
7. Bevy upgrade rehearsal证明 portable／engine-coupled分级。
8. 性能、memory、queue、FFI／commands有基线数据。
9. semantic input／output／fallback兼容闭环成立，且另一个测试维度可替换`terrenia`而无host special case。
10. 任一fixture package可注入typed setting／metric／inspect fragment，而不修改基础UI source。
11. F3类metrics、block inspect、chunk／collision／render visualizer可解释且有采集／绘制预算。
12. world catalog、frozen-lock preflight、checkpoint／clone／trash保证高风险动作在写入前可审核、失败后可恢复。
13. D7的40方块自然世界、territory delegation与hybrid cave slice通过确定性、边界与预算门禁。
14. D8的inventory／drop／tool／recipe／workstation／furnace／container形成可持久化、跨维度机制闭环。
15. D9精确交付72方块与2种独立流体，所有有限变化使用state而非复制StableId。
16. D10自动旅程与10分钟traversal证明探索、采集、制作、建造、保存和恢复在冻结预算内完整成立。

D0–D6完成第一個package-driven playable vertical slice；只有D0–D10全部完成才达到Terrenia
第一内容基线的可玩sandbox完成定义。D11是完成后的群系扩充轨，不阻塞D10发布。

完成 demo 不自动等于 ABI 1.0；冻结仍需[执行期路线图](roadmap-game-engine.md)的完整 gates。

## 相關文件

- [執行期整合路線](roadmap-game-engine.md)
- [套件內核](../architecture/package-management.md)
- [Demo workspace 与 Terrenia package 组织](../architecture/demo-workspace-organization.md)
- [原生 ABI](../architecture/native-module-abi.md)
- [Bevy 執行期](../architecture/game-engine-runtime.md)
- [待決問題](open-questions.md)
- [語義註冊、內容判定與選擇](../architecture/semantic-registration.md)
- [Terrenia 方块内容规划](terrenia-block-catalog.md)
- [Terrenia 科学／魔法双轨与关系包](terrenia-science-magic-and-relations.md)
- [Package 设置与配置](../architecture/settings-and-configuration.md)
- [诊断、检查与除错可视化](../architecture/diagnostics-inspection-and-debug-visualization.md)
- [World目录、开始页与安全生命周期](../architecture/world-lifecycle-and-start-ui.md)
