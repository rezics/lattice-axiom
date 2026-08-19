---
title: 一人與 AI 協作的 package-first／Bevy-first 開發策略
status: accepted
type: overview
updated: 2026-08-19
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0018-package-kernel-from-first-vertical-slice.md
  - ../decisions/0020-semantic-registration-and-content-selection.md
---

# 一人與 AI 協作的 package-first／Bevy-first 開發策略

## 結論

Lattice Axiom 以一位主要开发者、AI 协作与成熟开源元件开始。策略有两条同时成立的轴：

- **Bevy-first**：通用 engine 能力直接 adopt／compose／extend upstream；
- **package-first**：所有游戏内容从第一天经 Nickel、SemVer graph、registration 与 static／dynamic realization 进入同一 Bevy App。

第一個验收不是一套空 package platform，而是：

> 玩家从已锁定游戏闭包进入程序生成的体素世界，挖掉并放回方块；重启后变更仍在。同一最小 gameplay package 的 static 与 portable dynamic realization 产生相同注册与权威结果。

## 两类工作，不同决策规则

### 通用引擎能力

依固定顺序：

1. **Adopt**：Bevy 内建能力；
2. **Compose**：plugin、feature、schedule、asset、setting；
3. **Extend**：公开 extension point／维护中的 Bevy生态 plugin；
4. **Contribute／fork**：回馈 upstream／最小同步 fork；
5. **Build**：可玩原型与证据都证明前四步不足后，建立最小替代。

例外证据见[决策 0014](../decisions/0014-adopt-bevy-upstream-first.md)。

### Lattice 产品契约

package identity、SemVer、capability、dual realization、ABI、stable ID、schema、semantic Tag／Map／Predicate／Role 与 world semantics 是明确产品需求，不能要求「先证明 Bevy 做不到」才开始。这里的规则是：

1. 采用 Nickel／Cargo／C ABI／Bevy 等成熟 building blocks；
2. 从一个真实 consumer 设计最小 contract；
3. static／dynamic／old-binary fixtures 同步建立；
4. ABI `0.x` 先迭代，证据齐全才冻结；
5. registry、marketplace、hot unload 等规模功能延后。

## 最小垂直依賴鏈

```text
Nickel contracts
  → typed CompositionSpec
  → SemVer / capability lock
  → RegistrationManifest codegen
  → static + dynamic equivalence fixture
  → Bevy host adapter / App profiles
  → voxel edit
  → authoritative snapshot / restart
  → render feature/provider composition
  → Bevy upgrade / old-binary rehearsal
```

每一步有下游 consumer。package work不能连续多个 milestone 只加载空 library；gameplay 也不能新增绕过 graph 的临时 plugin。

## Bevy-first 實作方式

- client 从 `DefaultPlugins` 开始；headless／test 从 `MinimalPlugins` 与必要标准 plugins 开始。
- static host／package 可直接使用 Bevy component、resource、query、asset 与 `RenderApp`。
- dynamic package 经 generated batch ABI／command buffer，不创建第二个 ECS／schedule。
- 权威 chunk 使用紧凑 domain storage；一个 voxel 不对应一个 Entity。
- worldgen、meshing、I/O 使用 Bevy task pools；结果按 epoch／revision 验证。
- rendering 优先 Mesh／Material／shader／Bevy render schedules；低层扩充由 capability/provider 管理。
- assets 优先 `AssetServer`／glTF／typed loader；package layer只管理 ownership／stable identity／artifact closure。
- physics／input／voxel 候选先 spike 与量测，不预先重写。

## Package-first 實作方式

- 所有 profile 都产生 `LockedGameGraph`，没有 hand-written first-party plugin／dimension list。
- SDK/proc macro 是 authoring surface；manifest、static glue、C binding 与 fixtures 由同一 schema 生成。
- static 直接 Bevy／LTO；不把它降为呼叫 C ABI。
- dynamic 一次 system callback 处理 batches；不逐 entity／voxel FFI。
- package SemVer、ABI、schema、`EngineBuildId` 分开；lock 记录精确组合。
- native code视为 trusted；WASM／process sandbox 在威胁模型与 consumer 出现后另做。
- v1 stop／restart closure，不做 native library unload。

## 人與 AI 的責任

主要开发者决定：

- 玩家价值、产品范围与不可妥协需求；
- package／capability 的长期公共承诺；
- 哪些资料权威、可跨 ABI、可持久化；
- ABI performance／support window 与安全政策；
- Bevy upstream adoption／fork gate；
- 何时冻结 ABI／schema major。

AI 适合处理：

- Nickel contract、Rust typed model 与 codegen 的窄型实现；
- C header／Rust binding／ABI inspector 的机械生成；
- round-trip、golden、equivalence、old-binary 与故障注入测试；
- Bevy API／生态 plugin 的有边界 spike；
- property test、benchmark harness、profiling 与诊断；
- 文件、schema 与 generated output 一致性检查。

AI 不单独决定 public ABI。任何「方便先暴露整个 World／RenderDevice」的快速方案都要回到 owner、lifetime、version 与性能问题审查。

## 工作單元契約

每个工作单元同时有：

1. 玩家／作者可见结果或明确 consumer；
2. 要证明的不变量；
3. upstream／package／ABI inputs 与版本；
4. 明确不做的范围；
5. 自动验收、性能与失败预算；
6. static／dynamic／headless 中适用的矩阵。

例：

> “实现 decay system”不是完整工作单元。完整定义是：同一 SDK业务函数生成 static／portable callbacks，10k entities 的 state hash 相同；static direct 与 dynamic batch 在预算内；panic 不跨 ABI；save schema 不含 realization-specific handle。

## Demo 必須包含

- local Nickel package／game profile；
- deterministic SemVer／capability resolution 与 lock；
- `RegistrationManifest`／`RegistrationImage`；
- 由 Nickel constructors／contracts 产生、Rust 全图编译的 semantic Tag／Map／Predicate／Role／fallback 闭环；
- 一个真实 dual-realization gameplay package；
- C ABI `0.x` loader／batch／command／lifecycle；
- client／headless Bevy App profiles；
- package-injected settings／observability与统一settings／inspect／dev-tools surfaces；
- package-driven开始页、WorldHeader／catalog、write-before preflight与可恢复checkpoint／trash；
- voxel edit、worldgen 与 RocksDB persistence；
- portable／engine-coupled upgrade fixture；
- 最小 render feature／provider composition test。

## Demo 刻意不做

- 自研 App、ECS、scheduler、renderer、asset server、input 或 task runtime；
- public registry、marketplace、一般大型 graph resolver；
- native hot unload、不可信 native sandbox；
- 完整 WASM ecosystem；
- multiplayer rollback／lockstep；
- 完整 terrain civilization／hydrology／visual editor；
- second renderer／Godot runtime。

## 风险控制

- **ABI 过早冻结**：维持 `0.x`，以真实 gameplay、render、old-binary 与 Bevy upgrade gates 决定 1.0。
- **package platform 脱离游戏**：每个 kernel milestone 必须服务同一可玩 slice。
- **静态被最低共同分母拖慢**：static adapter 直接 Bevy，单独 benchmark／LTO 验证。
- **动态太 chatty**：默认 archetype batches、command buffer；逐 entity 只作为反例 benchmark。
- **Bevy 快速演进**：Cargo lock、migration branch、portable old binaries、engine-coupled rebuild 与 performance gates。
- **生态 plugin 风险**：记录版本、license、维护、fallback；以薄 adapter 隔离产品语义。
- **范围膨胀**：核心 graph 与分发规模分开；registry／hot unload／WASM 不进入首阶段。
- **surface碎片化**：package贡献typed settings／fragments／visualizers，由基础package统一布局、权限与预算；不各画一套菜单／HUD。
- **存档先改后救**：Continue只接受ReadyExact；升级先preflight与checkpoint／clone，delete先进trash。
- **native 安全误解**：UI／docs 明确 trusted code；ABI validation 不宣传为 sandbox。

## 相關文件

- [專案願景](project-vision.md)
- [技術棧](technology-stack.md)
- [套件內核](../architecture/package-management.md)
- [語義註冊與內容選擇](../architecture/semantic-registration.md)
- [原生 ABI](../architecture/native-module-abi.md)
- [執行期路線圖](../planning/roadmap-game-engine.md)
