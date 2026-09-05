---
title: Implementation workspace 与 package layout
document_id: meta.repository-and-package-layout
document_status: active
document_type: meta
tracks_implementation: false
updated: 2026-08-24
decision:
  - ../decisions/0008-static-and-dynamic-realizations-share-one-graph.md
  - ../decisions/0010-nickel-driven-package-system.md
  - ../decisions/0018-package-kernel-from-first-vertical-slice.md
  - ../decisions/0019-separate-package-and-registration-identities.md
  - ../decisions/0020-semantic-registration-and-content-selection.md
  - ../decisions/0032-freeze-local-package-acquisition-imports-and-product-lock.md
  - ../decisions/0034-freeze-package-local-code-and-locked-source-realization.md
---

# Implementation workspace 与 package layout

## 迁移事实基线

本页以 implementation commit `45fe974546e6d5fa61b8ea4bf4d736fce612565d` 为迁移起点；当前实现证据由
`502a262` 与 `a93d213` 提供。实现仓仍有 28 个 Cargo workspace crates，但已经有 21 个 logical package
source roots；它们不是一一对应关系。四个 code-bearing package root（dual gameplay、front-end、input、
settings-ui）现在同时持有 manifest、Rust source 与 data；其余 pure-data roots 不创建空 crate。这是
[决策0034](../decisions/0034-freeze-package-local-code-and-locked-source-realization.md) 所要求的
package-first 迁移结果。

### 2026-08-24 implementation evidence

- `packages/example/dual-gameplay/`、`packages/latticeaxiom/front-end/`、`packages/latticeaxiom/input/`
  与 `packages/latticeaxiom/settings-ui/` 是 workspace members，代码与 package manifest/data 同根维护；
- `latticeaxiom-compose` 从冻结 CAS source closure 生成 package-bound `RealizedDataRootV1`，NativeStatic
  staging 只接受受控 crate/package 路径，production host 只消费 lock-selected data roots；
- settings surface 的 catalog、slider、draft/apply/undo/cancel、safe-restart 与 persistence revision
  由 package/runtime contracts 共同拥有，pause 只做 Bevy projection；view-distance 是 typed `2..=32`
  slider，requested、admitted、effective 与 clamp reason 分开表达；
- host/input/catalog、voxel dispatch 与 mesh presentation 保持 capability/receipt 边界、bounded queues、
  stable ordering 与 shared mesh storage；
- reproducible checks：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、
  `cargo test -p latticeaxiom-compose --all-targets --all-features`、`cargo test -p latticeaxiom-packages --all-targets --all-features`、
  `cargo test -p latticeaxiom-engine --all-targets --features client`、`cargo test -p latticeaxiom-engine --all-targets --no-default-features`、
  `cargo test -p latticeaxiom-settings-ui --all-targets --all-features` 与 `cargo test -p latticeaxiom-start-ui --all-targets --all-features` 均通过。

这些证据证明 package-local code 已进入 source/build/runtime closure；它们不证明完整shipped settings
catalog、全部分层surface或`@latticeaxiom/client-presentation`已经实现。当前也尚未证明native static
已成为所有产品profile的默认realization，或把effective view-distance 32标记为性能certified。
这些缺口继续由roadmap gate管理。

```text
lattice-axiom-demo/
├── Cargo.toml                 Rust workspace；crate graph
├── latticeaxiom.toml          非可执行 root/source/profile inputs
├── latticeaxiom.lock          当前生成的 client-world product lock
├── crates/                    platform／host Rust implementation components
├── nickel/latticeaxiom/       versioned authoring contracts
├── packages/                  complete logical package source roots（code／data／manifest）
│   ├── latticeaxiom/
│   └── terrenia/
├── profiles/                  roots、source universe、policy 与 projection
├── catalog/                   local package release/CAS inputs
├── fixtures/                  conformance/fault corpus
├── run/                       untracked runtime state
└── target/                    generated/compiled artifacts
```

## 身份规则

- Cargo crate identity 来自 workspace `Cargo.toml`；
- logical `PackageName` 来自每个 source root 的 `latticeaxiom-package.toml`；
- stable registration identity 来自 manifest/registration rows；
- source directory 只表达维护关系，移动目录不自动改另外三个身份。

`packages/latticeaxiom` 和 `packages/terrenia` 是 family containers，不是 source roots。每个包含
manifest 的子目录是独立、非重叠 source root，避免递归 hash 或资产扫描把兄弟 package 算入。

## 冻结目标 layout

`crates/`与`packages/`按ownership区分，不按“Rust code或data”区分：

```text
lattice-axiom-demo/
├── crates/
│   ├── latticeaxiom-core/
│   ├── latticeaxiom-compose/
│   ├── latticeaxiom-packages/
│   ├── latticeaxiom-sdk/
│   ├── latticeaxiom-abi/
│   ├── latticeaxiom-voxel-runtime/
│   └── latticeaxiom-engine/
└── packages/
    ├── latticeaxiom/
    │   └── settings-ui/
    │       ├── latticeaxiom-package.toml
    │       ├── package.ncl
    │       ├── Cargo.toml
    │       ├── src/
    │       └── data/
    └── terrenia/
        └── worldgen/
            ├── latticeaxiom-package.toml
            ├── package.ncl
            ├── Cargo.toml
            ├── src/
            └── data/
```

- 平台／host code留在`crates/`；product、game与client policy code和其data共同位于logical package root；
- pure-data package不建立空crate；code-bearing package的root `Cargo.toml`是`source-build` entry；
- package root可以包含一个crate或显式内部workspace，因此logical package与crate仍非一对一；
- root Cargo workspace可以列出package-local crates供开发，但workspace membership没有产品选择语义；
- generated product root、static glue、bindings、CAS materialization与compiled artifacts仍位于hash-addressed
  store／`target/`，不写回`packages/`。

## 当前 logical packages

当前 lock 包含 settings、settings-ui、observability、inspect、dev-tools、Terrenia root、blocks、
worldgen、gameplay、tools 与 presentation。front-end、world-library、progress、relations、journey、
metallurgy、science 与 thaumaturgy 已有 source manifest，但不在这份 lock。完整导航见
[logical package index](../packages/README.md)。

`@latticeaxiom/client-presentation`已有accepted package contract，用来统一拥有跨shell/world的
Accessibility、Audio与Video设置，但目前没有implementation source manifest，也不在当前lock；不能把
settings-ui中的少数demo controls当成该package已经实现。

“在 lock”只证明 source/manifest/graph/realization 被选择，不证明 capability consumer 或完整产品
journey 已实现。

## Crate 与 package code 组织

现有 crates 按职责覆盖：

- composition/package/registration：core、compose、packages、registration；
- SDK/ABI/runtime contracts：sdk、sdk-macros、abi、runtime-contracts、dual-fixture；
- world/storage/generation：storage、world-wire、world-db、world-catalog、worldgen、territory；
- gameplay/player/content：gameplay、player、content；
- voxel/render：voxel-mesh、voxel-runtime、voxel-playground、render-contracts；
- product host：launcher、start-ui、engine。

这些是迁移前implementation boundaries。平台职责继续由top-level crates承载；settings、input、
front-end、Terrenia worldgen/gameplay等可选择product implementation必须迁入所属package root。
package README显式列出many-to-many implementation mapping；内部API reference应与代码同仓、从真实
public API生成。

平台crate不得依赖product package implementation。跨package Rust dependency必须同时有logical
dependency／capability edge，并由locked build plan指向exact package instance；普通workspace path
dependency不能成为隐藏graph edge。generic engine manifest也不得预依赖所有product packages或维护
feature-based package清单。

## Profile 与 source universe

profile 的 root requests 与可用 local sources 必须分开。source universe 中存在某个 package 不
表示它是 root 或进入 closure。shell graph 和 per-world graph 各自产生精确 lock，使用同一
resolver、registration compiler 与 artifact verification。

当前 `client-world` lock 仍是开发投影；accepted product model 还要求独立 shell lock 与
replacement-process launch loop。状态由 evidence 看板而不是本页 prose 追踪。

## Generated 与 runtime state

CAS、build plan、registration image、static glue、shared schema、ABI bindings 和 compiled artifacts
按输入 hash 生成，可删除后从 lock/source 重建，不进入 `packages/`，也不取得 `PackageName`。

world、checkpoint、crash marker、external acquired package 与本地 lease 属于 `run/` 或用户资料
目录，不得提交为规范或 fixture。fixture 必须按被验证职责命名，不能用 R0/R1 milestone 创建
永久目录身份。

## 迁移顺序

1. 先把`@example/dual-gameplay`做成真实package-root Cargo source，并从CAS建立static／portable artifacts；
2. 再迁移settings、settings-ui、client-presentation、input、front-end、world-library、observability、
   inspect与dev-tools，并以shipped catalog验证package ownership；
3. 拆分generic platform worldgen／gameplay／render contract与Terrenia-specific policy后迁移product code；
4. 最后删除engine里的product Cargo dependencies、features与手写plugin／setting/provider fallback；
5. 设置surface只消费compiled catalog；完成全部固定分类、分层layout与controls后，才在性能证据通过时
   扩大effective view-distance profile。

阶段完成不能以“新graph路径存在但旧engine路径仍可运行”证明；compatibility bridge必须列出owner、
适用package与删除gate。

## 新增边界的规则

- 新 logical package：先有 package contract/ADR、独立 source root 与 namespace/capability 责任；
- 新 crate：必须对应真实 implementation cohesion，不因“未来也许可替换”创造 backend；
- 新 generated crate：记录 producer/input/toolchain receipt，不成为 logical package；
- 新 docs package 目录：可以先于 manifest 表达 proposal，但必须明确 `proposed`、无 manifest、
  无 implementation evidence。
- 新 code-bearing package：source inclusion必须覆盖Cargo manifests、Rust source与所有behavior-affecting
  inputs；修改任一项都改变source digest。
- 新 product Cargo dependency：必须能追溯到locked logical edge；若只能通过engine manifest或workspace
  membership解释，就拒绝加入。
