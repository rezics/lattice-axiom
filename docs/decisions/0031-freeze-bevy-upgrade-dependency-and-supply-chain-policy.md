---
title: 冻结 Bevy 升级、依赖与供应链政策
status: accepted
type: decision
updated: 2026-08-20
---

# 决策 0031：冻结 Bevy 升级、依赖与供应链政策

## 背景

Bevy 是 core host 内部的 engine tool，而不是 package 的全域相容版本。项目已经要求以
`Cargo.lock` 精确锁定 host build、以 Lattice-owned contract 保护 portable boundary，并以
`EngineBuildId` 精确拒绝 engine-coupled artifact；但升级频率、生态相容矩阵、build identity
输入、old-binary 保留、external contract break 审查与供应链自动 gate 尚未形成一套可执行
政策。

如果这些值继续留到每次升级临时决定，项目会在两种错误之间摇摆：要么长期跳过 Bevy
release、累积跨版 migration 风险，要么把任何 Bevy 变化都误报成所有 package 的 major
break。依赖来源、license、advisory 与 release SBOM 也不能只依赖人工记忆。

本决策冻结首个 demo、D5 与 R7 所需的第一版治理值。它不改变 package SemVer、capability／
ABI、schema 与 `EngineBuildId` 彼此独立的既有决定。

## 问题映射与跨切不变量

| 待决问题 | 本决策的收口位置 |
| --- | --- |
| `DEP-01` | 第 1 节：Bevy baseline 与升级 cadence |
| `DEP-02` | 第 2 节：Bevy ecosystem compatibility matrix |
| `DEP-03` | 第 3 节：typed `EngineBuildId` 与 diagnostic receipt |
| `DEP-04` | 第 4 节：portable old-binary corpus 与保留期 |
| `DEP-05` | 第 5 节：external contract impact review |
| `DEP-06` | 第 6 节：license、SBOM、advisory 与 source pin |

另外冻结两个跨切语义：

- `PackageVersionReq` 的 parser 遇到任何 SemVer build metadata（`+...`）必须立即拒绝，
  不能先剥除再匹配，也不能让 matching 静默忽略这段误导性 metadata。此限制只针对
  requirement；合法 concrete package version 的 SemVer 语义由 package version contract 负责。
- `Cargo.lock` 只锁定 resolved package version、source 与 checksum，不记录本次 build 实际启用
  的 Cargo features。feature declarations／selection 分别由 Cargo manifests、composition profile
  与 versioned build receipt 记录；任何 compatibility、SBOM 或 build identity 检查都不得从
  `Cargo.lock` 猜测 feature selection。

## 决策

### 1. Bevy baseline 与升级 cadence

1. 首个实现 baseline 是 **Bevy `0.19.1`**。workspace manifest 声明精确版本；
   `Cargo.lock` 是 resolved Rust package versions、sources 与 checksums 的权威记录，Cargo
   manifests／profile／build receipt 另行记录实际 feature selection。
2. 每周自动检查一次 Bevy 与所有 adopted Bevy ecosystem dependencies 的稳定 release。
3. 每个 Bevy stable minor 都按相邻版本顺序演练，不采用“隔版升级”。新 minor 发布后
   **14 日内**建立独立 rehearsal branch 与 compatibility report，目标 **30 日内**通过 gate
   合并。每版演练不代表无条件合并；任何 gate 失败都必须先解决或形成有期限例外。
4. 普通 patch 采用 fix／security-driven policy：与当前 closure 有关的 patch 在确认后
   **14 日内**升级；修复已知可利用 high／critical vulnerability 的 patch 在 **72 小时内**
   进入 rehearsal branch。
5. 延后必须记录 blocker、owner、风险与退出日期。一次例外最多跨 **一个 stable minor** 或
   持续 **90 日**，以先到者为准；到期仍不能升级就必须重新审查 baseline，而不能静默累积。
6. 升级必须读取对应官方 migration guide，并完成 compose／lock／registration、portable
   ABI、engine-coupled reject／rebuild、client／headless、save／asset／worldgen 与 optimized
   performance gates。普通correctness CI保持headless且不需要窗口／GPU；render与normative
   performance gate必须按[决策0026](0026-freeze-first-demo-performance-budgets.md)另跑designated
   reference-host／GPU jobs。两类自动gate都不要求人工视觉验收。

### 2. Bevy ecosystem compatibility matrix

machine-readable single source 放在实现仓：

```text
compatibility/bevy-ecosystem.toml
```

core runtime maintainer 是 matrix owner；每个 dependency row 另有实际 capability owner。升级
PR 作者负责更新，受影响 capability owner 负责确认 evidence。若同一主要开发者兼任这些角色，
仍分别记录 author 与 owner decision，不虚构多人 quorum。

matrix schema 至少包含：

- schema version、exact Bevy patch 与 Rust toolchain；
- crate／package、exact version、registry／git source、checksum／commit 与排序后的 features；
- supported target／profile；
- `candidate`、`adopted`、`forked`、`blocked` 或 `retired` 状态；
- playable spike／test／benchmark evidence、最近验证日期；
- SPDX license、advisory 状态、unsafe／native dependency／build-script 标记；
- owner、upstream issue、fallback、fork／remove exit path。

每个 adopted direct Bevy ecosystem dependency 必须恰有一行。version／source／checksum 与
`Cargo.lock`／`cargo metadata --locked` 对照，feature selection 与 Cargo manifests、profile、
build receipt 对照；不能把 lock 当成 feature record。candidate 只有在 playable spike 产生
可重现 evidence 后才能改为 adopted。每次 Bevy patch／minor 都重验 adopted row；无升级时
超过 **90 日**未验证产生 warning。文档可以生成或链接摘要，但不维护第二份手写版本表。

### 3. `EngineBuildId` 与 diagnostic receipt

Rust model、`BuildPlan` 与 lock 字段必须使用透明 `EngineBuildId(CanonicalHash)` typed
newtype，不得继续以裸 `CanonicalHash` 表示 engine identity；generated ABI DTO 使用对应的
稳定 typed ID。既有 wire representation 继续是 64 个小写十六进制 SHA-256 字符，避免无
必要改变已冻结的 package model。ID 由 domain-separated、versioned receipt 计算：

```text
SHA-256("latticeaxiom.engine-build/v1\0" || canonical_json(EngineBuildReceiptV1))
```

`EngineBuildReceiptV1` 的 normative inputs 是：

1. receipt schema version；
2. build-relevant core／static／generated source closure hash；
3. `Cargo.lock` byte hash；
4. `rustc -Vv` 的 release、commit、host、LLVM identity 与 Cargo version；
5. target triple、target CPU／features；
6. profile、opt-level、LTO、codegen units、panic、overflow／debug assertions；
7. 排序后的 Cargo features；
8. allowlist 后会影响 codegen／link 的 Rust 与 linker flags；
9. 每个 internal interface 的 ID、major／minor、schema／layout hash；
10. generated ABI／registration code hash；
11. 从 lock 展开的 Bevy／ecosystem exact versions 与 features；
12. build script 的声明式 input／output receipt hash。

所有 set／map／flag canonical sort，文字使用规范化编码。timestamp、hostname、PID、absolute
worktree path、branch name、无关 Git metadata 与 secret environment value 不进入 ID；实际
source content 已由 closure hash 表达。未声明但会影响 build 的环境输入必须 fail closed，不能
偷偷加入机器特有值。

完整 receipt 随 host build 保存。mismatch diagnostic 同时给出 expected／actual ID、按字段
路径稳定排序的 receipt diff、受影响 realization 与“rebuild this realization”动作。
`EngineBuildId` 只判断 `EngineCoupledNative` exact-build compatibility，不参与
`PortableNative` compatibility，也不自动提升 package SemVer。

### 4. Portable old-binary corpus 与保留期

portable fixture 使用稳定语义路径，不以 roadmap 阶段命名：

```text
fixtures/native-abi/portable/<target>/<abi-version>/<fixture>/
```

路径中不得出现 `r0`。每个 fixture 保存 immutable binary、source snapshot、SDK／compiler
receipt、descriptor、registration manifest 与 artifact hash。conformance test 必须直接加载
已提交 binary，不能在测试中以当前 toolchain 偷偷重编。

第一版 normative corpus 包含：

1. C reference bootstrap + diagnostics／capability negotiation；
2. `@example/dual-gameplay` Rust SDK old binary，覆盖 component、fixed system、command、
   authoritative state 与 save equivalence；
3. `render.command-list` 成为真实 D5 portable contract 后加入一个 compact-command binary，
   只要求 headless semantic validation；
4. 独立的 engine-coupled old artifact，验证 build-ID mismatch、fallback 与重编后通过。

首批 target 至少是 `x86_64-unknown-linux-gnu` 与 `x86_64-pc-windows-msvc`；之后每个 shipped
tier target 都要同等 corpus。fixture bytes 永久保留；停止支援后移到 `retired` corpus，而非
删除。

ABI `1.0` 前只承诺 current development minor；conformance corpus永久保留current与前两个
`0.x`。旧fixture在manifest range声明compatible时必须加载，breaking epoch则必须精确拒绝；不能把
fixture retention误写成兼容承诺。ABI `1.0` 后，current major与previous major至少支援
**18个月或两个stable product release trains**，取较長者。始终保留至少一个retired incompatible
binary验证精确拒绝；artifact bytes永久保存。

### 5. External contract impact review

每个 Bevy upgrade 产生 machine-readable `ContractImpactReportV1`。报告逐一比较：

- package manifest／public API／observable behavior；
- capability／ABI layout／ownership；
- stable IDs／semantic image；
- schema／save／worldgen provenance；
- asset／render semantics；
- performance budgets；
- build-only `EngineBuildId` difference。

core runtime maintainer 起草报告；package、capability、ABI、schema、content、worldgen、asset
等实际 contract owner 分别决定自己的坐标；主要开发者对长期 public commitment 作最终决定。
AI 与自动工具可以生成 diff 和建议，但不能单独决定 public ABI。

升级合并要求 **零 unexplained diff**。分类规则固定为：

- 只有 core source／Cargo lock／Bevy internal／build receipt 改变时，只更新
  `EngineBuildId`；
- table 尾端 append-only optional operation 是 interface minor；
- field meaning、ownership 或 required operation 破坏是对应 interface major；
- package public API、dependency 或 observable behavior 破坏时，只提升实际受影响 package；
- schema、stable ID、semantic 或 generator break 由对应 owner version、migration／adapter
  或 explicit rejection 表达；
- 同一逻辑 package 的 engine-coupled rebuild 若行为不变，package SemVer 不变。

每个 major bump 必须链接一个具体失败的 contract fixture，以及 migration／adapter／explicit
rejection。Bevy version 改变本身不是 package major 的理由，禁止 blanket major bump。

### 6. License、SBOM、advisory 与 source pin

1. 实现仓根目录维护 `deny.toml`，使用精确 pin 的 `cargo-deny` 执行 licenses、advisories、
   bans 与 sources gate。所有 CI Cargo command 使用 `--locked`。
2. crates.io 是首版唯一默认 external registry。alternate／unknown registry 默认拒绝；git
   dependency 必须 pin 完整 commit SHA `rev`，禁止 branch／tag；path dependency 必须解析在
   workspace root 内。
3. GitHub Actions `uses:` pin 完整 commit SHA，并声明最小 permissions。
4. vulnerability、unsound 与 yanked dependency 默认 fail。临时 ignore 必须记录 advisory ID、
   exact package/version、owner、缓解、issue 与 expiry，最长 **30 日**。依赖变更 PR、每日
   scheduled scan 与每次 release 都执行；high／critical 修复 SLA 是 **72 小时**，其他已知
   vulnerability 是 **14 日**，无 severity 时按 high 处理。
5. license 必须是可解析 SPDX expression。unknown、unlicensed、non-commercial 与 field-of-use
   restriction 默认拒绝；permissive allowlist machine-readable，MPL／LGPL／GPL／AGPL／custom
   license 逐项审查。license exception 绑定 exact package version／checksum，升级后自动失效。
6. workspace crate metadata 记录 `AGPL-3.0-only`。package metadata 使用验证后的 SPDX type，
   不保留任意 string。Cargo dependency inventory 与 package／asset license inventory 分开，
   后者也必须记录 source、license 与 attribution；Cargo SBOM 不假装覆盖 content assets。
7. 每个 shipped target／profile／feature closure 从 exact lock 生成 CycloneDX JSON SBOM 与
   third-party notices。二者与 binary、build receipt 一起作为 release artifacts 永久保存，
   release manifest 记录各自 SHA-256。SBOM hash 不反向进入 `EngineBuildId`，避免循环 identity。
8. duplicate dependency 在首版为 warning；wildcard、unknown source、过期 exception 与
   manifest／lock drift 为 error。供应链工具自身也使用 exact version pin。

## 第一版冻结值

| 项目 | 第一版值 |
| --- | --- |
| Bevy baseline | `0.19.1` |
| release 检查 | 每 7 日 |
| minor rehearsal／合并目标 | 14 日／30 日 |
| 普通 patch／high-critical patch | 14 日／72 小时 |
| upgrade 延后上限 | 一个 minor 或 90 日，取先到者 |
| matrix path | `compatibility/bevy-ecosystem.toml` |
| matrix freshness warning | 90 日 |
| build ID | domain-separated canonical JSON + SHA-256，receipt schema 1 |
| 首批 old-binary targets | Linux x86-64、Windows MSVC x86-64 |
| pre-1.0 support／corpus | 只承诺current `0.x`；永久保留current + 前两个fixtures并load-or-reject |
| post-1.0 support | current + previous major，至少18个月或两个stable product release trains，取较長者 |
| advisory exception | 最长 30 日 |
| SBOM | 每个 shipped target/profile 的 CycloneDX JSON + notices |

这些值是首版 policy，不从环境变量或 CI 默认值推导。更改 cadence、support window、build-ID
input schema、license/source trust boundary 或 SBOM contract 需要新的决策记录；只替换实现工具
且产出与 gate 等价，可以作为普通 maintenance change。

## 明确延后与触发 gate

- public registry service与remote distribution activation须等待[决策0030](0030-freeze-governance-distribution-and-security-triggers.md)的GOV-01 trigger；
  signing、publisher identity、revocation与transparency contract已由同一决策冻结，不再另作互斥政策。当前hash／source／SBOM gate不因此延后。
- Linux／Windows x86-64 以外的 prebuilt target 在对应 shipped target 被批准时加入 matrix 与
  old-binary corpus；不得先宣称支持而没有 fixture。
- ABI `1.0` 只有在真实 gameplay、render、前两个 old `0.x`、一次 Bevy upgrade rehearsal 与
  performance gate 都通过后才能冻结。
- source attestation、SBOM signing、organization trust delegation 与 `cargo vet` 类完整 source
  review 在公开 native artifact 分发或第三方 fork 成为 release dependency 时触发。
- distributed build farm、remote artifact cache 与 marketplace 不属于本决策；任何未来实现仍须
  消费相同 lock、receipt、SBOM 与 source policy。

## 自动证据与验收

1. matrix validator 双向核对 adopted direct dependency：version／source／checksum 对照
   `Cargo.lock` 与 `cargo metadata --locked`，features 对照 Cargo manifests／profile／build
   receipt；missing／extra／stale row 给出稳定诊断。
2. `EngineBuildReceiptV1` property corpus 证明 map／set 顺序和 worktree path 不影响 ID，且每个
   normative input 单独变化都会改变 ID；mismatch 展开字段级 diff。
3. CI 先校验 committed old-binary hash，再直接加载。portable binary 跨 Bevy upgrade 不重编，
   engine-coupled old artifact 在查询 internal interface 前拒绝，重编后通过。
4. contract-impact golden 覆盖 internal patch、optional ABI append、ownership break、stable-ID
   removal、schema change 与 coupled rebuild；未解释 diff 或 blanket major bump 失败。
5. `cargo-deny`、source policy 与 exception expiry 是 PR／daily／release gate。negative corpus
   覆盖 branch git dependency、unknown registry、wildcard、unlicensed package、known advisory
   与 expired ignore。
6. SBOM packages 与按 shipped target 取得的 `cargo metadata --locked` 双向一致；notices 覆盖
   所有要求 attribution 的 entry。相同 lock／target／profile 的规范化 SBOM 语义相同。
7. upgrade rehearsal自动执行package conformance、portable／coupled ABI、client／headless、
   save／asset／worldgen与optimized performance suite。普通suite无窗口／GPU；0026要求的
   reference-host／GPU jobs仍是merge gate，且只采集数值／resource evidence，不做人工视觉测试。

## 结果

- Bevy 快速演进以相邻 release、短期 rehearsal 和有期限例外处理，不累积隐性 migration debt。
- ecosystem compatibility、build identity 与 external contract impact 都有 machine-readable
  evidence，而不靠 release note 猜测。
- portable ABI 的承诺由真正未重编的 old binaries 证明；engine-coupled rebuild 不污染 package
  SemVer。
- license、advisory、source与SBOM从D0起成为自动gate；0030已经冻结future registry signing contract，
  但在其trigger前不部署public registry service或remote trust runtime。

## 相关文件

- [决策 0014：采用 Bevy 并以上游能力为默认](0014-adopt-bevy-upstream-first.md)
- [决策 0017：版本化原生模组 ABI](0017-versioned-native-module-abi.md)
- [决策 0021：冻结 R0／R1 package contract](0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md)
- [决策 0026：冻结 first-demo performance budgets](0026-freeze-first-demo-performance-budgets.md)
- [决策 0030：冻结治理、分发与安全 triggers](0030-freeze-governance-distribution-and-security-triggers.md)
- [版本与相容性](../architecture/versioning-and-compatibility.md)
- [原生模组 ABI](../architecture/native-module-abi.md)
- [技术栈](../foundations/technology-stack.md)
- [开发策略](../foundations/development-strategy.md)
- [执行期路线图](../planning/roadmap-game-engine.md)
- [第一个可玩 demo](../planning/roadmap-first-demo.md)
