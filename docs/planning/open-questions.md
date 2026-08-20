---
title: 待決問題決議索引
status: active
type: planning
updated: 2026-08-20
---

# 待決問題決議索引

本页记录原 `open-questions` 的稳定编号、已接受答案与后续自动证据。`[x]` 表示产品／public contract 已有明确决定；不表示对应 roadmap 交付已经完成。实现状态只由[执行期路线图](roadmap-game-engine.md)与[第一個可玩 demo](roadmap-first-demo.md)追踪。

明确延后的事项也必须有已决定的触发条件；没有满足 trigger 前不得扩大首个 demo 的 scope。决策基线另见 ADR [0008](../decisions/0008-static-and-dynamic-realizations-share-one-graph.md)、[0009](../decisions/0009-rocksdb-authoritative-world-snapshots.md)、[0014](../decisions/0014-adopt-bevy-upstream-first.md)、[0015](../decisions/0015-bevy-native-y-up-world-coordinates.md)、[0017](../decisions/0017-versioned-native-module-abi.md)、[0018](../decisions/0018-package-kernel-from-first-vertical-slice.md) 与 [0019](../decisions/0019-separate-package-and-registration-identities.md)、[0020](../decisions/0020-semantic-registration-and-content-selection.md)、[0021](../decisions/0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md)、[0022](../decisions/0022-freeze-controlled-nickel-evaluation-metering-and-worker-protocol.md)。

## P0：SDK 與 Registration

本组由[决策 0023](../decisions/0023-freeze-sdk-registration-and-semantic-compilation.md)冻结。

- [x] `SDK-01`：公开 authoring surface 采用 proc macro；macro 共用 sealed typed IR，Rust builder 不成为第二个组合入口。
- [x] `SDK-02`：作者只写一个 row kernel；生成器分别产生直接 typed Bevy query adapter 与每 system／batch 的动态 adapter。
- [x] `SDK-03`：dual whitelist只含ABI-POD read／write／optional、`With`／`Without`、stable entity key、fixed context、command sink、manifest-declared message sink／reader与已实现read-only POD input slice；其他`SystemParam`编译期static-only。
- [x] `SDK-04`：跨 package typed 或公开持久 component 使用 `GeneratedSharedSchema`；host／private 使用 `HostTyped`；仅 POD untyped consumer 可用 `RuntimeDynamic`，typed dependency 显式进入 `BuildPlan`。
- [x] `SDK-05`：unsafe descriptor 构造隔离在窄 bridge crate；`ValidatedRuntimePodDescriptor` 验证 layout／drop-free invariant，stable ID 到 `ComponentId` 仅属当前 `World`。
- [x] `SDK-06`：`registration_semantic_hash`、`provenance_hash`、callback-map hash 与 artifact hash 分离；producer／toolchain 只进 provenance receipt。
- [x] `SDK-07`：先以 stable ID 解析 fallback／active set，再按 registry kind 和 canonical stable ID 连续分配 `u32` numeric ID；numeric ID 不持久化。
- [x] `SDK-08`：v1 冻结十个 versioned semantic stages，从 `input.sample` 到 `presentation.update`；Bevy schedule mapping 是 host-private conformance。
- [x] `SDK-09`：唯一 `SystemSignature` 同时绑定 manifest、callback 与动态 columns；static glue 初始化后的 Bevy actual access 必须在 `Playing` 前逐项相等。
- [x] `SDK-10`：等价性比较 registration／semantic／schedule、canonical authoritative commands／messages、barrier state hash 与未压缩 snapshot bytes；不比较 pointer、batch 划分或 timing。
- [x] `SDK-11`：typed semantic ID 与 wire target-kind 由一个 Rust target declaration 生成；跨边界 `SchemaId` 必须带 `@major`。
- [x] `SDK-12`：Tag contribution authority 由 owner、resolved dependency／grant 与 profile layer 机械推导，不信任 package 自称。
- [x] `SDK-13`：v1 使用 dense Tag bitset、按 canonical byte cost 选择 dense／sparse Map、无后向跳转的 bounded predicate bytecode；首版 memory／node limits 写入 compile policy。
- [x] `SDK-14`：v1 Map merger 只有 `Conflict`；出现第二个真实多贡献 consumer 且满足交换／结合／总定义 property tests 后才增加内建 merger。
- [x] `SDK-15`：唯一 content-addressed `SemanticResolutionReceipt` 冻结 Role bindings、active bundles 与解释图；world metadata／header 只引用其 hash。
- [x] `SDK-16`：D0–D10 没有 Affordance callback consumer；首版只实现 static predicate，至少两个真实 consumer 复用同一 context contract 后再发布 callback ABI。

## P0：Portable Native ABI `0.x`

本组由[决策 0024](../decisions/0024-freeze-portable-native-abi-0x.md)冻结。

- [x] `ABI-01`：bootstrap 使用 16-byte `LaxAbiHeader`、`LAXB` magic、fixed-width C layout；首版只支持 little-endian 64-bit target，并在读取尾字段前验证 size／flags／target／hash。
- [x] `ABI-02`：`InterfaceId` 由 canonical interface name 的 domain-separated SHA-256 截取 128 bit；manifest 保留全名并对 collision fail closed。
- [x] `ABI-03`：首批tables只有diagnostics、`ecs.batch`、command buffer与messages；tasks／assets／render只有在真实consumer、provider、budget与conformance fixture齐备后才冻结。
- [x] `ABI-04`：ABI-POD 只允许 fixed scalars、fixed arrays 与显式 nested POD，alignment 不超过 16；layout hash 包含 fields／offsets／size／align／endianness。
- [x] `ABI-05`：recoverable write 必须 staged；authoritative direct write 失败为 instance-fatal，不实现通用 rollback，也不允许半写 state 继续 tick／save。
- [x] `ABI-06`：handle 固定为 `{u64 table_id,u32 slot,u32 generation}`；零值无效，generation 溢出永久 retire slot，handle 不持久化。
- [x] `ABI-07`：borrowed span、callback-lifetime scratch arena 与 allocator-owned buffer 是三种独立类型；owned buffer 由分配方 exactly-once release。
- [x] `ABI-08`：零 flags 默认 main-thread、per-instance serialized、nonblocking、nonreentrant；native callback 只有 soft deadline，hang 的 hard recovery 是 process termination。
- [x] `ABI-09`：recoverable staged error 丢弃结果；panic／invalid output／contract violation使 instance failed；memory fault／hang终止 process；native trust profile 不改变此矩阵。
- [x] `ABI-10`：`0.x` 是实验线并保留当前与前两个不可重编 fixture；`1.x` 起支持 current 与 previous major，过期只精确拒绝。
- [x] `ABI-11`：ABI 1.0 gate 使用真实 D8／D9 system；dynamic P95 不得超过 `max(1.25 × static, static + 0.50 ms)`，bridge 不超过 fixed CPU 的 10%。

## P0：玩家與 Bevy Spike

本组由[决策 0025](../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md)冻结，性能阈值引用[决策 0026](../decisions/0026-freeze-first-demo-performance-budgets.md)。

- [x] `PLAY-01`：60 Hz fixed movement；1.8 m × 0.35 m capsule、4.5 m/s walk、45° slope、0.60 m step、约 1.25 m jump apex、100 ms buffer／coyote；fly／noclip 仅 detached debug spectator。
- [x] `PLAY-02`：break／place reach 5 m，authoritative deterministic voxel DDA，共用 12 fixed ticks cooldown；命令携带 chunk revision 并返回稳定 typed errors。
- [x] `PLAY-03`：首个可理解目标是 `Restore the Terrenia Waymark`：检查错误方块、取得 creative palette、break／place copper block、durable reopen。
- [x] `PLAY-04`：首个client reference是Windows 11 x64、6C／12T AVX2、16 GiB、6 GiB VRAM、1080p medium／D3D12；Linux x64负责headless CI。
- [x] `PLAY-05`：Bevy 精确锁定 `0.19.1`；client 从 `DefaultPlugins` 开始，headless 使用最小标准 plugins，feature selection 由 manifests／build receipt 记录。
- [x] `PLAY-06`：`bevy_voxel_world` 只可作为 working-set／presentation adapter；authoritative chunk、stable ID 与 snapshot 由 Lattice 持有。
- [x] `PLAY-07`：Avian `0.7` 是 provisional D2 选择；fixed tick、collider rebuild 与 local-origin 必须通过自动 benchmark，block selection 仍用权威 DDA。
- [x] `PLAY-08`：Leafwing `0.21`是adoption-gated首版输入选择；headless直接注入相同Lattice `PlayerActionV1`／authoritative command DTO，gate失败则回退Bevy native input而不改stable action contract。
- [x] `PLAY-09`：首版 surface 使用 Bevy UI／widgets／input focus／EditableText／AccessKit；Feathers 仅限 dev workbench，不进入 public surface contract。

## P0：Settings、資訊 Surface 與 Client Shell

本组由[决策 0025](../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md)冻结。

- [x] `SET-01`：settings、settings-ui、observability、inspect、dev-tools、world-library、front-end 各有独立 logical package／capability；dev-tools 是 optional root，选中后提供 exactly-one capability。
- [x] `SET-02`：`SettingValueV1`是bool／i64／exact decimal／string／enum／linear RGBA／typed input binding；visibility与cross constraints是bounded typed expressions，无脚本或regex。
- [x] `SET-03`：LocalUser 与 WorldOwner 使用分离 authority lanes 和固定 precedence；provenance 保存 stable ID、scope、store／transaction revision、writer kind 与适用 lock／world hash。
- [x] `SET-04`：Bevy settings plugin 只承接 static device／user store；world／player-world 使用相同 codec、catalog validator 与 `WorldStorage` transaction，不镜像 Bevy API。
- [x] `SET-05`：apply 是 Immediate／WorldReactivate／ProcessRestart，preview 是独立 reversible policy；GraphRecompose 只属于 composition parameter；prepare 后才 persist／publish／commit。
- [x] `SET-06`：UI编辑基于base profile／lock的typed draft，产出overlay、candidate lock与完整diff；既有world只能继续frozen lock、为new world使用candidate，或checkpoint／clone后显式upgrade。
- [x] `SET-07`：SDK IR 同时生成 static typed handle 与 dynamic numeric／batched binding；numeric setting key 只在当前 catalog epoch 有效。
- [x] `SET-08`：sample batch 每 key 返回 Value／Unavailable／Error；freshness 由 host 根据 TTL 与 revision 推导，omitted key 是 protocol error。
- [x] `SET-09`：subscription plan使用refcount、generation与cancellation；最后subscriber关闭后昂贵query／callback／upload必须为零，首版overlay hard budgets由0025冻结，0026治理后续性能证据与调整。
- [x] `SET-10`：target inspect 先做 server-authoritative raycast／reach／permission；Hidden、RequiresTool／Progress、Unavailable 与 Error 是不同 wire states，cache key包含全部 policy／target revisions。
- [x] `SET-11`：visualizer 只发 host-validated primitive vocabulary，具有 coordinate space、legend、depth、pick、radius 与 hard budgets；超限按 stable key 截断并显示 partial。
- [x] `SET-12`：client v1 不在同一 process 创建两个 fresh `DefaultPlugins` Apps；shell ↔ world 先完成graceful durability／shutdown barrier，再写atomic `LaunchIntentV1`并由replacement process启动。headless 可同 process 多 `EngineInstance`。
- [x] `SET-13`：800×600／高缩放／IME／keyboard／controller／AccessKit tree 由自动 fixture 验证；首版随包提供 CJK fallback font，OS screen reader 人工检查不阻塞 CI。

## P0：性能预算

本组由[决策0026](../decisions/0026-freeze-first-demo-performance-budgets.md)冻结。D10前修改provisional数字须同时提交old／new reference-host evidence与correctness理由；D10 freeze后，收紧可升profile minor，放宽或改变端点／hardware／failure semantics须新profile major与ADR。

- [x] `PERF-01`：`desktop-reference-v1` 以 60 Hz render／fixed tick为目标；frame P95 ≤ 16.67 ms、P99 ≤ 25 ms，fixed CPU P95 ≤ 8 ms、P99 ≤ 12 ms，RAM ≤ 4 GiB、VRAM ≤ 3 GiB。
- [x] `PERF-02`：在provisional 32³ chunk下active≤405、resident≤1,183、visible≤512、requested／materializing／prefetch合计≤1,536；改变edge时按覆盖距离与bytes重算。
- [x] `PERF-03`：edit→mesh P95／P99 ≤ 100／200 ms，edit→collider ≤ 150／300 ms，warm／cold chunk read P95 ≤ 50／250 ms，dirty→durable P95 ≤ 1 s。
- [x] `PERF-04`：worldgen／mesh 128 jobs／128 MiB，collider／persistence 64／64 MiB；全局 queued ≤ 512、in-flight ≤ 384 MiB、main-world apply ≤ 2 ms 与 16 MiB／frame。
- [x] `PERF-05`：典型dynamic row为64 B／entity，target batch 256（允许128–1,024）；单一`LaxBatchView`全部column payload合计≤64 KiB，单callback payload≤1 MiB。
- [x] `PERF-06`：每 tick callback soft／hard 64／128，indirect calls 256／512，commands 256 KiB／1 MiB 与 4,096／16,384；超限原子失败，不截断 authoritative mutation。
- [x] `PERF-07`：真实 dynamic batch 使用 ABI-11 overhead gate；steady-state shim／callback 零 heap allocation，分项记录 pack、FFI、body、validate、apply。
- [x] `PERF-08`：zero-copy 先看 layout／lifetime资格；≤16 KiB staging，≥64 KiB eligible payload优先 zero-copy，中间区由 benchmark 决定；validation ≤ 0.25 ms／tick。

## P1：Render Capability／Provider

本组由[决策 0029](../decisions/0029-freeze-render-capability-and-provider-contract.md)冻结。

- [x] `REND-01`：RenderData 是 typed presentation input；Feature 是可启停 additive unit；Pass 是 feature-owned execution node；Provider 完整提供 exactly-one mechanism。
- [x] `REND-02`：v1 slots 是 `prepass`、`after-main`、`early-post`、`before-tonemap`、`after-tonemap`；`after.opaque`语义含糊、耦合上游内部且没有真实consumer，因此不发布。
- [x] `REND-03`：resource graph 使用 logical SSA versions、唯一 writer 与显式 scope／lifetime／format／samples；host 推导 transient alias 与 barriers。
- [x] `REND-04`：D5 command list 只有 declared pipeline／resource set／parameter block、dispatch、fullscreen triangle 与 debug marker；无 raw resource creation／copy／barrier／readback。
- [x] `REND-05`：每 feature／view／frame lists ≤ 4、commands／list ≤ 256、encoded ≤ 64 KiB、upload ≤ 4 MiB、dispatch ≤ 32、fullscreen draw ≤ 8、readback = 0；整单先验证后提交。
- [x] `REND-06`：terrain mesh-source 是独立 logical package；visibility 与 backend 首版由 core Bevy adapter提供，fake fixture 不构成生产拆包证据。
- [x] `REND-07`：provider 按标准 GPU features／limits／formats 与 profile stable priority选择；vendor仅用于 denylist／workaround，fallback必须acyclic，结果不进 authoritative hash。
- [x] `REND-08`：压力 fixture 是 static depth fog + portable compute selection outline，并配 two-level CPU voxel LOD／material extension；oracle 比较 plan／command trace而非像素。
- [x] `REND-09`：没有产品级engine-coupled render consumer；D5只保留test-only exact-build fixture。真实provider须证明`NativeStatic`与portable contract均不足，并通过ADR0014 evidence及exact-build／fault／upgrade gates后才新增窄接口。

## P1：权威世界與持久化

本组由[决策 0027](../decisions/0027-freeze-authoritative-world-and-persistence-contract.md)冻结。

- [x] `WORLD-01`：D3 provisional chunk edge 为 32；key 是 versioned binary tuple，signed coordinates 用 sign-bit-flipped big-endian；palette按 stable ID／state schema／bytes排序并用最小 bit width。
- [x] `WORLD-02`：每 chunk 一个 authoritative `ChunkRevision u64`，sub-revision只用于 derived invalidation；另有 world-level `WorldRevision`／commit frontier。
- [x] `WORLD-03`：lifecycle 是 Absent→Requested→Materializing→ResidentInactive↔Active→Evicting；durability是正交状态，不存在可丢 dirty data 的“persistence radius”。
- [x] `WORLD-04`：RocksDB physical tuning不进入 domain API；D3比较 16／32／64 edge、CF布局、none／LZ4／Zstd、cache／compaction／crash，并把结果写入 baseline。
- [x] `WORLD-05`：UI `Saved` 只表示当前 world authoritative frontier 已 durable；written 与 checkpointed 使用不同文案。
- [x] `WORLD-06`：首版 writer 只写 `world-wire@1`：versioned envelope + pinned postcard payload；voxel／fluid是自有 packed sections，RocksDB compression不改变 normative bytes。
- [x] `WORLD-07`：metadata同时保存完整 `FrozenLockReceipt` 与实际资料的 `WorldRequirementClosure`；compatible reopen 不隐式替换 frozen lock。
- [x] `WORLD-08`：presentation缺失可 placeholder；compatible authoritative closure可显式 reopen；可 opaque preserve 才能 read-only；有 trusted migration 才 staged migrate；否则 Blocked。
- [x] `WORLD-09`：dual-gameplay fixture 覆盖 static↔dynamic、Memory／RocksDB、layout-only／schema migration与 canonical snapshot byte equality。
- [x] `WORLD-10`：metadata采用 DB-first publish：durable DB epoch／projection hash，再 fsync temp header 与 atomic replace；header health是可重建 projection，不是第二真相。
- [x] `WORLD-11`：`WorldId` 是 UUIDv4，wire为16 bytes，canonical text／directory为lowercase hyphenated UUID；display name与path不是identity，clone才创建新 ID。
- [x] `WORLD-12`：catalog只扫描显式非重叠 roots 的直接 children，默认不跟 symlink／reparse；header ≤ 64 KiB，thumbnail ≤ 2 MiB／512 px，损坏 entry 使用 typed error。
- [x] `WORLD-13`：open status 按 ReadyExact、ReadyCompatible、NeedsDownloadOrBuild、NeedsMigration、RecoverableReadOnly、Blocked 的“下一安全动作”算法判定，并保留全部 typed reasons。
- [x] `WORLD-14`：pure preflight只读 bounded metadata／external receipts且不 dlopen／entry／create／migration；activation重验 ABI 后仍先不开 writer，所有验证成功才进入 `Playing`。
- [x] `WORLD-15`：checkpoint区分protected与rotating automatic；后者最多保留3个且physical bytes≤min(10 GiB, 20% filesystem capacity)。clone从verified checkpoint建立新ID并re-key，不只改header。
- [x] `WORLD-16`：migration从不 in-place；小迁移写新 keyspace 后 metadata switch，大迁移写 sibling DB 后 atomic publish，所有 failpoint 保留原 world／checkpoint。
- [x] `WORLD-17`：low disk按 absolute reserve + in-flight + WAL／drain + 当前操作估算 admission；Warning停止扩张，Paused在 apply 前拒绝新 authoritative mutation并优先 drain。
- [x] `WORLD-18`：每 root 使用同 filesystem managed trash；restore先 preflight，live ID冲突只能 Blocked或 restore-as-clone；demo不自动 purge，purge拒绝路径逃逸。

## P1：最小世界生成

本组由[决策 0028](../decisions/0028-freeze-worldgen-content-and-asset-contract.md)冻结。

- [x] `GEN-01`：每dimension exactly-one coordinator；terrain按channel×territory ownership domain唯一，cave按topology ownership domain唯一；其他贡献只有在typed compositor与hard budget下可多选。
- [x] `GEN-02`：D4 两个真实 style 是 temperate woodland 与 arid badlands；共用 cave／materializer／Roles，并以 stable selector 与 named deterministic transition组合。
- [x] `GEN-03`：`WorldSeedV1` 为 32 bytes；UI int／NFC text分别 domain-separated hash；generator identity包含 stable ID、contract major、algorithm revision 与 implementation fingerprint，SemVer不作随机盐。
- [x] `GEN-04`：D4 DAG 固定为 semantics／seed→style→terrain field→coarse cave plan→bounded SDF→final occupancy→material→resource／vegetation→draft receipt→atomic snapshot；cave不读取邻 chunk mutable arrays。
- [x] `GEN-05`：epoch按 planning cell首次 materialization冻结；新旧边界必须有 versioned adapter／width／signature／portal receipt，缺失或超预算在 writer 前拒绝，旧 cell 零重写。
- [x] `GEN-06`：full territory pure-data prototype可在 D2／D3并行但不阻塞 D0–D6；D7前以三尺度、1,000 synthetic candidates、determinism／boundary／performance作为 promotion gate。

## P1：内容與资产

本组由[决策 0028](../decisions/0028-freeze-worldgen-content-and-asset-contract.md)冻结。

- [x] `CONTENT-01`：72 StableIds 保持既定清单；`@terrenia/blocks` 必须交付 canonical 72-row name／asset／intrinsic／physical／state／mining／drop／placement／recipe／worldgen matrix，D8 调平后冻结 content revision。
- [x] `CONTENT-02`：v1使用共同`ContentHeaderV1`；`BlockDefinitionV1`／`ItemDefinitionV1`／`BiomeDefinitionV1`只持有自身intrinsic字段，drop／tool／recipe／worldgen／semantic contribution使用独立typed rows，不使用万能property bag。
- [x] `CONTENT-03`：平台首批vocabulary只有copper storage block Tag／Role与normal obsidian Tag；copper ingot须等D8真实consumer、至少两个compatible item fixtures及完整owner／schema／conflict gate，Terrenia terrain／portal contract保持领域私有。
- [x] `CONTENT-04`：不发布composite `physical@1`；D8 mining与physics各由真实consumer冻结narrow typed map。v1共享state先做axis／facing／lit，form需mesh＋collision＋placement fixture，blast resistance延后。
- [x] `CONTENT-05`：每cell独立solid／fluid palettes，fluid index 0是Empty；`FluidDefinitionV1`引用`FluidStateV1`，finite level／flow属于`FluidStateV1`，scheduled frontier另存versioned continuation。
- [x] `CONTENT-06`：typed asset 是 logical package拥有的 versioned artifact，不是第三类 package；authoritative asset receipt进 lock，presentation只进 presentation fingerprint。
- [x] `CONTENT-07`：RegistrationImage索引 stable asset→owner／artifact／path#label／type／schema／authority；host只注解 Bevy dependency graph并可沿两个方向输出诊断。
- [x] `CONTENT-08`：只有纯 presentation dependency可热重载；collision／selection／gameplay／semantic／worldgen／fluid变化必须新 authoritative image并 reactivate／recompose，writable world拒绝原地应用。
- [x] `CONTENT-09`：glTF只保证普通 node name／transform／proxy mesh；socket／collider／semantic property由 pinned Blender exporter + Bevy loader的无 GPU round-trip fixtures验证并 canonicalize。
- [x] `CONTENT-10`：geometry／material／skeleton留在 glTF；asset-local socket／hit-zone／collider mapping进 versioned sidecar；跨 asset／package或权威语义进 package schema。

## P2：治理、分发與安全

本组由[决策 0030](../decisions/0030-freeze-governance-distribution-and-security-triggers.md)冻结。未满足 trigger 时保持 local／workspace distribution，不实现假 registry／sandbox／multiplayer。

- [x] `GOV-01`：public registry只在 D10后且有 ≥3非核心作者、≥5 packages／10 immutable versions、三环境可复现与 ≥2次手工分发阻塞时启动；首版没有排名／付费／auto-update marketplace。
- [x] `GOV-02`：distribution使用 content-addressed release envelope与 TUF-style versioned Ed25519 metadata；yank只影响新 resolve，security revoke阻止 code build／load并保留只读恢复。
- [x] `GOV-03`：Tier 1 client targets为 Windows x64、Linux x64、Apple arm64，dedicated为 Linux x64；frozen world／`--frozen` 永不从缺失 prebuilt静默 source-build。
- [x] `GOV-04`：ABI、package、schema各有 machine-readable deprecation字段与独立 support window；telemetry默认本地，外传只能 opt-in aggregate且不能携带 world／player identity。
- [x] `GOV-05`：production Wasm不阻塞D10；D10完成后仍须有真实外部untrusted非玩具package的static／portable／Wasm等价、hostile／fuzz security corpus与Tier 1性能证据才实现；首版policy为64 MiB／instance、10M fuel／callback及bounded host calls／I/O。
- [x] `GOV-06`：任何 native build／load前 UI明确说明 full-process authority并显示publisher／source／hash／dependency chain；headless无 exact allowlist即 fail closed。
- [x] `GOV-07`：multiplayer须在D10 clean journey、真实dedicated server、两个automated clients与versioned replication corpus完成后才实现`JoinClosureOffer@1`／`AuthoritativeClosureReceipt@1`；不比较target artifact／realization／EngineBuildId，presentation按optional／recommended／required-interface分级。

## P2：Bevy 升级與依赖治理

本组由[决策 0031](../decisions/0031-freeze-bevy-upgrade-dependency-and-supply-chain-policy.md)冻结。

- [x] `DEP-01`：每个稳定 Bevy minor都顺序 rehearsal，14日内开分支、目标30日合并；延期最多一个 minor或90日，patch／security按风险 SLA处理，不无条件追新。
- [x] `DEP-02`：实现仓 `compatibility/bevy-ecosystem.toml` 是唯一 machine-readable matrix，由 runtime owner与capability owner共同维护并与 `cargo metadata --locked` 双向校验。
- [x] `DEP-03`：`EngineBuildId` 是 domain-separated SHA-256 `EngineBuildReceiptV1` newtype；receipt包含 source closure、lock、toolchain、target／profile／features／flags、internal interfaces与生态版本并提供字段级 diff。
- [x] `DEP-04`：portable old binaries按 target／ABI version永久保留 committed bytes、source snapshot与receipt；测试不得重编，active support window与永久 retention分离。
- [x] `DEP-05`：每次升级产生 `ContractImpactReportV1`；只有实际受影响 coordinate owner可决定 package／capability／ABI／schema major，build-only变化只更新 `EngineBuildId`。
- [x] `DEP-06`：D0启用 pinned `cargo-deny`、`--locked`、full-SHA source／Actions policy、SPDX validation、advisory SLA、CycloneDX SBOM与notices；例外必须 exact、owned且有 expiry。

## 证据与重开规则

上述决定只有在对应 ADR 的自动 evidence 或 trigger 失败时才重开；实现尚未完成不等于问题重新变成未决。D0–D10 必须依次交付：composition／resolver goldens、semantic compiler、dual gameplay与真实 ABI、player／voxel／physics spike、world persistence fault corpus、worldgen／content receipts、render graph／command fixtures、十分钟 representative journey，以及 Bevy old-binary upgrade rehearsal。逻辑／故障证据必须headless可执行；render与normative performance另由0026指定的reference-host／GPU jobs提供数值证据，均不要求视觉人工测试。

不得为 future trigger预先扩大 API：general SAT resolver、native hot unload、production Wasm、multiplayer rollback／lockstep、federated marketplace、distributed build farm与full visual editor均保持明确延后。

## 相關文件

- [執行期路線圖](roadmap-game-engine.md)
- [第一個可玩 demo](roadmap-first-demo.md)
- [套件內核](../architecture/package-management.md)
- [原生 ABI](../architecture/native-module-abi.md)
- [語義註冊](../architecture/semantic-registration.md)
- [World 持久化](../architecture/world-persistence.md)
- [World 生命週期](../architecture/world-lifecycle-and-start-ui.md)
- [世界生成](../architecture/world-generation.md)
- [渲染架構](../architecture/rendering.md)
- [設定與配置](../architecture/settings-and-configuration.md)
- [診斷與檢查](../architecture/diagnostics-inspection-and-debug-visualization.md)
