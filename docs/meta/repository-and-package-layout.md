---
title: Implementation workspace 与 package layout
document_id: meta.repository-and-package-layout
document_status: active
document_type: meta
tracks_implementation: false
updated: 2026-08-22
decision:
  - ../decisions/0010-nickel-driven-package-system.md
  - ../decisions/0019-separate-package-and-registration-identities.md
  - ../decisions/0020-semantic-registration-and-content-selection.md
---

# Implementation workspace 与 package layout

## 事实基线

本页对照 implementation commit `653556ba7df5c5993f300bd40c1393c8221abafe`。实现仓当前有
25 个 Cargo workspace crates 与 19 个 logical package source roots；它们不是一一对应关系。

```text
lattice-axiom-demo/
├── Cargo.toml                 Rust workspace；crate graph
├── latticeaxiom.toml          非可执行 root/source/profile inputs
├── latticeaxiom.lock          当前生成的 client-world product lock
├── crates/                    Rust implementation components
├── nickel/latticeaxiom/       versioned authoring contracts
├── packages/                  shipped/declared logical package sources
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

## 当前 logical packages

当前 lock 包含 settings、settings-ui、observability、inspect、dev-tools、Terrenia root、blocks、
worldgen、gameplay、tools 与 presentation。front-end、world-library、progress、relations、journey、
metallurgy、science 与 thaumaturgy 已有 source manifest，但不在这份 lock。完整导航见
[logical package index](../packages/README.md)。

“在 lock”只证明 source/manifest/graph/realization 被选择，不证明 capability consumer 或完整产品
journey 已实现。

## Crate 组织

现有 crates 按职责覆盖：

- composition/package/registration：core、compose、packages、registration；
- SDK/ABI/runtime contracts：sdk、sdk-macros、abi、runtime-contracts、dual-fixture；
- world/storage/generation：storage、world-wire、world-db、world-catalog、worldgen、territory；
- gameplay/player/content：gameplay、player、content；
- voxel/render：voxel-mesh、voxel-runtime、voxel-playground、render-contracts；
- product host：launcher、start-ui、engine。

这些是 implementation boundaries，不应为每个 crate 建立平行产品规范树。package README 显式
列出 many-to-many implementation mapping；内部 API reference 应与代码同仓、从真实 public API
生成。

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

## 新增边界的规则

- 新 logical package：先有 package contract/ADR、独立 source root 与 namespace/capability 责任；
- 新 crate：必须对应真实 implementation cohesion，不因“未来也许可替换”创造 backend；
- 新 generated crate：记录 producer/input/toolchain receipt，不成为 logical package；
- 新 docs package 目录：可以先于 manifest 表达 proposal，但必须明确 `proposed`、无 manifest、
  无 implementation evidence。
