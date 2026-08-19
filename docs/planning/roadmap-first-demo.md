---
title: 第一個套件驅動的 Bevy 可玩 demo 路線圖
status: active
type: roadmap
updated: 2026-08-19
decision:
  - ../decisions/0008-static-and-dynamic-realizations-share-one-graph.md
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0017-versioned-native-module-abi.md
  - ../decisions/0018-package-kernel-from-first-vertical-slice.md
  - ../decisions/0019-separate-package-and-registration-identities.md
---

# 第一個套件驅動的 Bevy 可玩 demo 路線圖

## Demo 定義

> 玩家从一个已锁定 package closure 进入由 `terrenia` 定义的 Terrenia Y-up 体素维度，挖掉并放回方块；退出再进入后世界仍保留修改。同一最小 gameplay package 可切换 static／portable dynamic realization，而 registration、玩法结果与存档不变。

这是可玩的 vertical slice，也是 package／ABI 的第一个真实 conformance consumer。它不是 package manager UI、空动态库 loader 或 renderer benchmark。

## 固定基線

- Nickel `package.ncl`／`game.ncl` → typed `CompositionSpec`；
- package SemVer、capability／realization resolution 与精确 lock；
- `RegistrationManifest`／`RegistrationImage`；
- Nickel-authored SemanticTag／Map／Predicate／Role 与 locked fallback ContentBundle；
- `NativeStatic` 与 `PortableNative` ABI `0.x`；
- Bevy `0.19.x` baseline，精确 patch／features 由 Cargo lock 固定；
- client `DefaultPlugins`，headless 使用必要标准 profile；
- Bevy 原生右手 Y-up；
- voxel／physics／input／assets／diagnostics 先采用 Bevy／生态 upstream；
- RocksDB 保存完整已物化 chunk snapshot；
- Terrenia／test content 都经 package graph；host 没有内建默认维度；
- `terrenia` 是普通可替换root模组；host与平台contract不引用Terrenia concrete content；
- 无自研 ECS／scheduler／renderer／asset server。

## 两条同时验收的循环

### 玩家循环

1. 选择／创建 world。
2. 从 lock 建立游戏 closure。
3. 在 Y-up voxel terrain出生。
4. walk／look／jump、raycast、break／place。
5. mesh／collision 在预算内更新。
6. 退出／crash fixture后重启。
7. 已 durable修改正确恢复。

### Package 循环

1. Nickel profile 选择 `@example/dual-gameplay`。
2. 以 `NativeStatic` 启动并记录 lock／registration／state hash。
3. 只切换 realization 为 `PortableNative`。
4. loader 验证 artifact／ABI／manifest。
5. 执行同一 action fixture。
6. 比较 IDs、schedule、state、save 与 diagnostics。

任一循环失败，demo 均未完成。

## D0：Composition／Lock／Bevy Smoke

### 交付

- local Nickel profile／contracts；
- `latticeaxiom.lib` semantic constructors／contracts／concat与binding overlay；
- core／empty game packages；
- deterministic SemVer／capability lock；
- client／headless profile；
- package-driven Bevy App；
- placeholder camera／light／cube；
- Y-up orientation、manual fixed-time、diagnostics／CI。

### 完成

- 同一 lock 可建立 client／headless的 compatible authoritative closure；
- client 显示场景并退出；headless无 GPU推进 fixed tick；
- 没有 hidden hand-written first-party plugin／dimension list；
- 没有自有 runner／ECS／renderer；
- dependency／license 与 lock metadata可重建。
- CLI／embedded Nickel对semantic intent产生相同typed output与source-aware conflict。

## D1：真实 Dual-Realization Gameplay Fixture

### 交付

- SDK／proc macro prototype；
- `@example/dual-gameplay` 的一个真实 component + fixed system + command；
- generated manifest／static glue／C binding／dynamic batch shim；
- portable ABI entry、instance lifecycle、batch／command／diagnostics；
- equivalence harness 与 static／dynamic／per-entity反例 benchmark。

### 完成

- single-source business code 同时构建两种 realization；
- registration hash、IDs、schedule 与 N ticks state hash一致；
- static direct path 不经 C ABI／可启用 LTO；
- dynamic FFI 次数随 system／batch，不随 entity线性往返；
- wrong ABI／manifest／panic／stop-after-callback 有 deterministic test；
- native library不卸载。

D1 不是空 `hello_plugin`；system 必须读写 gameplay-shaped data并发 command。

## D2：Upstream Voxel Playground

### 交付

- `bevy_voxel_world` 优先 spike；
- Avian 优先 physics spike；
- Bevy input／Leafwing 二选一；
- walk／look／jump、raycast、break／place；
- chunk／mesh／collider diagnostics；
- dual gameplay package驱动 break／place 的一项真实规则。

### 完成

- 另一位测试者可取得 build并完成操作；
- chunk boundary／six Y-up faces正确；
- edit-to-visible、frame、fixed tick、memory、queues 已量测；
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
- unload／reload、normal shutdown／crash fixtures；
- world metadata的 required package／schema closure。
- world metadata的authoritative semantic image、active bundle与frozen Role binding。

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
- missing authoritative content有明确 recovery／拒绝。

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
- `EngineCoupledNative` exact-build fixture；
- 一次 Bevy upgrade branch rehearsal；
- portable old binary保留不重编。

### 完成

- feature pair 顺序／resource graph稳定；
- exclusive provider conflict在 code load前失败；
- unsupported GPU选 fallback；
- render failure不改变 world hash；
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
- checkpoint restore。

### 人工验收

- 新测试者 5 分钟内完成 break／place／restart；
- package／ABI／missing content错误可理解且有修复动作；
- static／dynamic切换无需修改业务 package source；
- profiler清楚标示 dynamic bridge／batch overhead；
- build／profile／lock／world recovery说明可重复。

## Demo 明確不做

- public／federated registry、marketplace／auto-update；
- general large-scale resolver；
- multi-platform build farm／distributed cache；
- native hot unload；
- production WASM sandbox／untrusted mod market；
- multiplayer rollback／lockstep；
- custom renderer／physics／ECS／scheduler／asset system；
- 完整 world editor／civilization／hydrology；
- Godot runtime。

## Demo 完成定义

1. 真实玩家完成完整循环。
2. 所有 profile只从 package graph启动。
3. 一个真实 system 的 static／portable realization等价，static性能优势未被抹平。
4. save／crash证明权威 revision正确。
5. upstream adoption report没有猜测性自研替代。
6. render feature／provider证明机制级扩充可组合。
7. Bevy upgrade rehearsal证明 portable／engine-coupled分级。
8. 性能、memory、queue、FFI／commands有基线数据。
9. semantic input／output／fallback兼容闭环成立，且另一个测试维度可替换`terrenia`而无host special case。

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
