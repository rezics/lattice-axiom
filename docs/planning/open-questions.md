---
title: 待決問題
status: active
type: planning
updated: 2026-08-19
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
- [x] package compatibility使用SemVer；Bevy是core package内部tool，外部break仍需诚实版本化。
- [x] 已物化world以RocksDB完整snapshot为权威，见[决策 0009](../decisions/0009-rocksdb-authoritative-world-snapshots.md)。
- [x] 没有global version开关，见[决策 0003](../decisions/0003-no-global-version-switch.md)。
- [x] native code是trusted process code；ABI validation不构成sandbox。
- [x] v1不卸载native library。
- [x] Godot只作有触发条件的toolchain对照，不作第二runtime。

## P0：Package 语义與 Nickel Contract

- [ ] `latticeaxiom.lib` 第一版 `Package`／`GameProfile`／`Realization` contracts具体字段？
- [ ] Nickel field metadata／source span经typed conversion后如何保留到Role binding与semantic contribution diagnostics？
- [ ] import roots、evaluation time／memory／recursion／output size上限？
- [ ] `CompositionSpec`与Nickel contracts由何种single-source schema／fixture共同生成？
- [ ] local source的canonical path／content hash／symlink policy？
- [ ] package pre-1.0 SemVer是否完全沿Cargo左侧非零规则，还是严格SemVer ranges？
- [ ] v1 resolver的deterministic tie-break／minimal vs highest compatible policy？
- [ ] client-only／server-only／authoritative package与feature条件如何表达？

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

## P1：最小世界生成

- [ ] generator是一个exclusive capability还是可分层多个providers？
- [ ] demo两个terrain／biome证明哪两种真实组合能力？
- [ ] seed、config、package version、generator revision如何canonicalize／hash？
- [ ] terrain、soil、cave／void最小dependency graph？
- [ ] 新旧generation epochs的boundary最小验收？
- [ ] full territory `(x,z)` prototype何时开始，不能阻塞哪一demo gate？

## P1：内容與资产

- [ ] block／item／biome definition的最小schema？
- [ ] 首批平台material Tag／Role vocabulary具体只包含哪些真实demo forms，如何避免提前建立通用ontology？
- [ ] `physical@1` Map与shared block state keys哪些字段真正被voxel／physics／structure两个consumer共同需要？
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

## 相關文件

- [執行期路線圖](roadmap-game-engine.md)
- [第一個可玩 demo](roadmap-first-demo.md)
- [套件內核](../architecture/package-management.md)
- [原生 ABI](../architecture/native-module-abi.md)
- [語義註冊、內容判定與選擇](../architecture/semantic-registration.md)
- [渲染架構](../architecture/rendering.md)
