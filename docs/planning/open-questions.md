---
title: 待決問題
status: active
type: planning
updated: 2026-08-20
---

# 待決問題

本页只保留会改变产品、public package／ABI contract、可玩原型或长期资料边界的问题。已经形成决策的方向不再以问题形式反复打开。

## 已关闭的方向

- [x] 游戏引擎采用 Bevy；完整 upstream 能力为默认，见[决策 0014](../decisions/0014-adopt-bevy-upstream-first.md)。
- [x] runtime／assets／physics／save 使用 Bevy原生右手 Y-up，见[决策 0015](../decisions/0015-bevy-native-y-up-world-coordinates.md)。
- [x] Nickel 是 package／profile组合语言，Rust package kernel负责副作用、SemVer graph、lock、build与load，见[决策 0010](../decisions/0010-nickel-driven-package-system.md)。
- [x] static／dynamic realization共用同一 graph／registration语义，但不共用最低调用约定，见[决策 0008](../decisions/0008-static-and-dynamic-realizations-share-one-graph.md)。
- [x] dynamic native使用 C bootstrap、versioned capability tables、batch ECS与command buffer；static直接Bevy/LTO，见[决策 0017](../decisions/0017-versioned-native-module-abi.md)。
- [x] package kernel与ABI `0.x` 从第一个 vertical slice开始；只延后registry／marketplace／hot unload／WASM规模，见[决策 0018](../decisions/0018-package-kernel-from-first-vertical-slice.md)。
- [x] logical package 使用 root `name`／scoped `@scope/name`，stable registration 使用独立的 `<namespace>:<kind>/<path>` 与显式 namespace grant；Terrenia 同时治理 root `terrenia` 与 scope `@terrenia`，并作为 package 定义的第一维度，见[决策 0019](../decisions/0019-separate-package-and-registration-identities.md)。
- [x] exact ID与SemanticTag／Map、StateProperty、Affordance、Predicate、Role、fallback ContentBundle分层；Nickel负责authoring／overlay，Rust负责全图验证／lock／runtime compilation；Terrenia没有平台特权，见[决策 0020](../decisions/0020-semantic-registration-and-content-selection.md)。
- [x] Terrenia 内容规模采用6 → 18 → 40 → 72个方块定义的分阶段基线，water／lava为两个独立流体定义，有限状态与几何变体不复制StableId，见[Terrenia方块内容规划](terrenia-block-catalog.md)。
- [x] package compatibility使用SemVer；Bevy是core package内部tool，外部break仍需诚实版本化。
- [x] 已物化world以RocksDB完整snapshot为权威，见[决策 0009](../decisions/0009-rocksdb-authoritative-world-snapshots.md)。
- [x] 没有global version开关，见[决策 0003](../decisions/0003-no-global-version-switch.md)。
- [x] native code是trusted process code；ABI validation不构成sandbox。
- [x] v1不卸载native library。
- [x] Godot只作有触发条件的toolchain对照，不作第二runtime。
- [x] R0／R1的`PackageSpec`／`GameProfileSpec`／`RealizationSpec`最小字段与版本规则已经冻结，见[决策 0021](../decisions/0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md#2-packagespec-最小欄位)、[profile contract](../decisions/0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md#3-gameprofilespec-最小欄位)与[realization contract](../decisions/0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md#4-realizationspec-最小欄位)。
- [x] `SourceSpan`／origin provenance进入diagnostics但不进入semantic hash；source rename只改变provenance，见[决策 0021](../decisions/0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md#6-sourcespansemantic-hash-與-provenance-hash-分離)。
- [x] Nickel只使用受控import roots；R0默认time／memory／recursion／import／output limits与version policy已经冻结，见[决策 0021](../decisions/0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md#7-受控-import-roots-與-nickel-resource-policy)。
- [x] R0 Nickel的entry／import计数、deadline起止、recursion frame、memory backend receipt、diagnostic排序截断与worker终止映射已经冻结；缺少hard capability时production policy必须fail closed，见[决策 0022](../decisions/0022-freeze-controlled-nickel-evaluation-metering-and-worker-protocol.md)。
- [x] Rust serde model与正／负golden corpus是single source；Nickel contracts与schema docs验证同一corpus，首版不做双向codegen，见[决策 0021](../decisions/0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md#1-rust-serde-model-與-golden-corpus-是-normative-source)。
- [x] local source使用canonical root-relative path与SHA-256；symlink采用follow-target hash且不得逃逸root，见[决策 0021](../decisions/0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md#8-canonical-pathsha-256-與-symlink-policy)。
- [x] package range使用项目公开的strict SemVer 2.0 grammar；`0.x`不沿Cargo left-most-nonzero caret，见[决策 0021](../decisions/0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md#9-strict-semver-20-與-range)。
- [x] resolver选择最高compatible version，以stable source／realization tuple打破等价候选；有效frozen lock优先且不隐式重解，见[决策 0021](../decisions/0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md#10-deterministic-resolution-與-frozen-lock)。
- [x] package domain使用`authoritative`／`client`／`server`／`tool` markers并由profile projection机械裁剪，见[决策 0021](../decisions/0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md#5-domain-markers-與-profile-projection)。

## P0：SDK 與 Registration

- [ ] 第一版code-bound system／schema SDK authoring API是proc macro、builder还是两者分层？
- [ ] 如何从一个business function生成typed Bevy query与dynamic batch adapter，不让作者手写两套？
- [ ] 哪些system parameters可dual-realization，哪些compile-time标为static-only？
- [ ] 哪些公开component必须生成`GeneratedSharedSchema` crate，哪些可保持`HostTyped`或`RuntimeDynamic`？typed dependency如何进入`BuildPlan`？
- [ ] Bevy dynamic component descriptor的安全封装、drop-free限制与stable ID → process-local `ComponentId` mapping如何做conformance test？
- [ ] `RegistrationManifest` canonical schema与hash包含哪些producer信息、不包含哪些process-local信息？
- [ ] numeric IDs由stable ID排序、graph hash派生还是显式lock table？
- [ ] system stage／set第一版最小集合，如何版本化？
- [ ] dynamic read／write declaration如何与实际callback binding／Bevy access交叉验证？
- [ ] static与dynamic equivalence的normative comparison：state hash、commands、messages、save bytes哪些必须相同？
- [ ] `TagId<T>`／`MapId<T,V>`／`RoleId<T>`的Rust泛型与serialized target-kind discriminator如何single-source生成？
- [ ] extensible Tag contribution的self-owned判定、integration dependency与profile foreign amendment provenance具体schema？
- [ ] Tag bitset、SemanticMap dense／sparse table与Predicate bytecode的首版layout／budget？
- [ ] 第一个numeric Map merger是否只做conflict，何种第二consumer证明需要associative custom merger？
- [ ] frozen Role binding与active ContentBundle在lock／world metadata之间如何去重并保留resolution explanation？
- [ ] Affordance callback的第一个真实consumer是什么；在出现前只实现static semantic fast path。

## P0：Native ABI `0.x`

- [ ] bootstrap request／response与`LaxAbiHeader`的最终C layout、magic、endianness／pointer-width policy？
- [ ] `InterfaceId`使用UUID、128-bit hash还是canonical string hash？collision／registry治理？
- [ ] 首批必须实现哪些tables：diagnostics、batch、commands、messages、tasks、assets？
- [ ] ABI-POD允许的scalar／nested／alignment集合；layout hash如何计算？
- [ ] writable batch原地修改失败时是否需要transaction／rollback，还是package failure结束tick？
- [ ] entity handle／asset handle的bit layout与generation overflow policy？
- [ ] scratch arena、owned buffer与allocator-side release的具体API？
- [ ] callback timeout／blocking／reentrancy／thread flags的默认值？
- [ ] panic／fault后instance进入failed、disable system还是终止process？不同trust profile是否不同？
- [ ] old ABI major支援窗口与deprecation telemetry？
- [ ] ABI 1.0的dynamic overhead预算，以哪些真实system作为gate？

## P0：玩家與 Bevy Spike

- [ ] 第一版movement验收：walk、jump、slope、step、fly／noclip debug？
- [ ] break／place距离、cooldown、selection／error反馈？
- [ ] 技术循环之外最小「可理解／好玩」目标？
- [ ] 第一个OS／GPU／input matrix？
- [ ] 实作时锁定哪个Bevy `0.19.x` patch／features？
- [ ] `bevy_voxel_world`适合authoritative chunk还是presentation adapter？
- [ ] Avian fixed tick／voxel collider／raycast／local-origin是否达预算？
- [ ] Bevy原生input或Leafwing，哪一个总实现／测试成本较低？
- [ ] Bevy UI／diagnostics是否足够package／save／performance overlay？

## P0：Settings、資訊 Surface 與 Client Shell

- [ ] `@latticeaxiom/settings`／settings-ui／observability／inspect／dev-tools／front-end／world-library首版各自package与capability version边界？
- [ ] `SettingSpec`首版value types、cross-setting constraint与declarative visibility predicate最小集合？
- [ ] device／user／profile／world／player-world／session store的exact precedence、authority与provenance encoding？
- [ ] Bevy 0.19 app settings只承接哪些device／user scopes；world store adapter如何共享typed schema又不镜像上游API？
- [ ] `immediate`／`world-reactivate`／`graph-recompose`／`process-restart`的apply transaction与rollback hooks最终schema？
- [ ] graph-affecting profile parameter从settings UI生成draft／diff／new lock的最小可用流程？
- [ ] static typed handle与portable dynamic numeric setting key／batched change interface如何single-source生成？
- [ ] diagnostic item／metric／inspect fragment的callback batch、freshness、unavailable与error encoding？
- [ ] subscription planner如何确保关闭panel后不执行昂贵chunk／physics／server query；overlay自身CPU／GPU预算？
- [ ] target inspect的server permission、progress／tool gating、cache revision与“隐藏 vs 不可用”protocol？
- [ ] chunk／physics visualizer的legend、depth mode、pick、radius、primitive与upload hard limit？
- [ ] shell／world在同process顺序重建fresh Bevy App是否能可靠重建window／GPU／static state；哪些realization会触发process restart fallback？
- [ ] Bevy UI／Feathers／input focus在800×600、高UI scale、IME、keyboard、controller与screen narration下能否支撑首版surface？

## P0：性能预算

- [ ] target hardware的render frame／fixed tick预算？
- [ ] active／resident／visible chunk数量？
- [ ] edit-to-visible、collider rebuild、load、save P95？
- [ ] task／I/O／mesh queue depth与in-flight bytes上限？
- [ ] 典型dynamic system每次entity／column bytes与batch size？
- [ ] 每tick dynamic callback count／indirect call／command bytes预算？
- [ ] static direct相对dynamic batch允许的真实场景overhead？
- [ ] command validation与staging copy何时超过zero-copy收益？

## P1：Render Capability／Provider

- [ ] RenderData／Feature／Pass／Provider的最终分界与首批consumer？
- [ ] semantic slots第一版名称／version与Bevy 0.19 mapping？
- [ ] resource access如何表达format、sample count、lifetime与exclusive writer？
- [ ] portable `RenderCommandList`最小指令集合；如何避免逐步扩张成raw wgpu？
- [ ] dynamic upload／dispatch／draw command的budget与validation？
- [ ] default terrain mesh／visibility／backend providers由core还是separate packages拥有？
- [ ] hardware-specific provider的GPU feature／vendor／fallback policy？
- [ ] 两个post effects与LOD／shader组合fixture选择什么，才能真正压力测试？
- [ ] engine-coupled render interface是否有真实consumer；若没有则不实现。

## P1：权威世界與持久化

- [ ] authoritative chunk最小字段、chunk size、palette encoding与Y-up key encoding？
- [ ] `ChunkRevision`是统一还是voxel／entity／scheduled work分域？
- [ ] active／resident／persistence radius state machine与backpressure？
- [ ] RocksDB column families、prefix、compression、cache／compaction benchmark？
- [ ] queued／written／durable／checkpointed中UI“已保存”对应哪一级？
- [ ] snapshot／component schema encoding采用何种serde format？
- [ ] world metadata保存exact lock、compatible ranges与used-content closure的组合？
- [ ] missing package／content的placeholder、read-only recovery、migration与拒绝策略？
- [ ] static↔dynamic切换后persistent component layout／schema如何fixture验证？
- [ ] `WorldHeader`怎样与RocksDB metadata原子更新／交叉校验，避免catalog健康状态成为第二份真相？
- [ ] WorldId、display name、directory slug与move／rename的canonical规则？
- [ ] catalog scan roots、bounded header／thumbnail、async size与损坏entry的错误模型？
- [ ] `ReadyExact`／`ReadyCompatible`／`NeedsMigration`／`RecoverableReadOnly`／`Blocked`的normative判定？
- [ ] preflight可以执行到哪个验证层而绝不运行module business code／打开writer？
- [ ] checkpoint轮替的数量／空间／changed-only policy；clone如何产生新WorldId且保留provenance？
- [ ] migration staging用RocksDB keyswitch、独立目录还是按迁移规模选择；publish／rollback gate？
- [ ] low-disk threshold何时暂停authoritative mutation，怎样避免警告出现后仍继续扩大损坏？
- [ ] trash root／restore冲突／retention／permanent purge的跨平台filesystem语义？

## P1：最小世界生成

- [ ] generator是一个exclusive capability还是可分层多个providers？
- [ ] demo两个terrain／biome证明哪两种真实组合能力？
- [ ] seed、config、package version、generator revision如何canonicalize／hash？
- [ ] terrain、soil、cave／void最小dependency graph？
- [ ] 新旧generation epochs的boundary最小验收？
- [ ] full territory `(x,z)` prototype何时开始，不能阻塞哪一demo gate？

## P1：内容與资产

- [ ] 72方块清单中各定义的最终显示名称、纹理、掉落、工具需求与数值平衡？
- [ ] block／item／biome definition的最小schema？
- [ ] 首批平台material Tag／Role vocabulary具体只包含哪些真实demo forms，如何避免提前建立通用ontology？
- [ ] `physical@1` Map与shared block state keys哪些字段真正被voxel／physics／structure两个consumer共同需要？
- [ ] fluid definition／state的最小schema，以及solid occupancy与fluid state在voxel palette／snapshot中的编码？
- [ ] data package、typed asset与code package如何分工？
- [ ] Bevy AssetServer dependency与package artifact ownership如何双向诊断？
- [ ] asset hot reload何时只重建presentation，何时需阻止authoritative state修改？
- [ ] Blender → glTF → Bevy能否保存socket、collider与semantic tags？
- [ ] 哪些semantic需要sidecar／package schema？

## P2：治理、分发與安全

- [ ] 何种真实creator workflow触发public registry？
- [ ] package signing、publisher identity、revocation与trust delegation模型？
- [ ] prebuilt artifact target matrix与source fallback？
- [ ] package／ABI／schema deprecation telemetry与support window？
- [ ] `WasmComponent`的威胁模型、host capabilities、fuel／memory与performance gate？
- [ ] native code UI如何明确告知“trusted full-process access”？
- [ ] 多人／server如何协商authoritative closure与client-only presentation packages？

## P2：Bevy 升级與依赖治理

- [ ] Bevy upgrade cadence：每版、隔版或feature／fix-driven？
- [ ] ecosystem compatibility matrix由谁维护，放在哪个machine-readable artifact？
- [ ] `EngineBuildId`精确输入与可诊断展开格式？
- [ ] 哪些portable fixtures保证不重编；old binaries保留多久？
- [ ] external contract break审查由谁判断，如何避免把所有Bevy变化提升成package major？
- [ ] license／SBOM／advisory／source pin自动检查？

## 明确延后、但不是已否定

以下不进入首个demo，只有对应consumer／threat／scale证据后排期：

- public／federated registry与marketplace；
- general large-scale PubGrub／SAT resolver；
- multi-platform build farm／distributed cache；
- native hot unload；
- production WASM sandbox／untrusted mod market；
- multiplayer rollback／lockstep；
- full visual world editor。

## 下一批证据

1. Nickel → typed `CompositionSpec` golden fixtures。
2. SemVer／capability resolver conflict corpus与frozen lock。
3. Nickel semantic constructors → typed Tag／Map／Predicate／Role golden与source-aware overlay conflict。
4. copper／obsidian／portal、Tag cycle、Map conflict、Role ambiguity与fallback frozen-world corpus。
5. `@example/dual-gameplay` static／dynamic registration与state equivalence。
6. static direct／dynamic batch／per-entity反例benchmark。
7. package-driven Bevy client／headless smoke与non-Terrenia dimension replacement。
8. upstream voxel／physics playable adoption report。
9. RocksDB crash／revision-race／realization-switch save fixtures。
10. render feature pair／exclusive provider fixtures。
11. portable old binary／engine-coupled Bevy upgrade rehearsal。
12. 三个owner注入SettingSpec的scope／transaction／orphan与static／dynamic golden。
13. target inspect permission／fragment conflict与diagnostic subscription-off overhead fixture。
14. chunk lifecycle／collision shape／AABB／query／render visibility visualizer budget fixture。
15. WorldHeader corruption、catalog scan、exact-lock preflight、clone migration、low-disk与trash／restore corpus。

## 相關文件

- [執行期路線圖](roadmap-game-engine.md)
- [第一個可玩 demo](roadmap-first-demo.md)
- [套件內核](../architecture/package-management.md)
- [原生 ABI](../architecture/native-module-abi.md)
- [語義註冊、內容判定與選擇](../architecture/semantic-registration.md)
- [渲染架構](../architecture/rendering.md)
- [Package 設定與配置](../architecture/settings-and-configuration.md)
- [診斷、檢查與除錯可視化](../architecture/diagnostics-inspection-and-debug-visualization.md)
- [World 目錄、開始頁與安全生命週期](../architecture/world-lifecycle-and-start-ui.md)
