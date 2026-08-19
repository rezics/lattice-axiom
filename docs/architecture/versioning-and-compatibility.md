---
title: 套件、ABI、Bevy 與持久化的相容性
status: proposed
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0003-no-global-version-switch.md
  - ../decisions/0010-nickel-driven-package-system.md
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0017-versioned-native-module-abi.md
---

# 套件、ABI、Bevy 與持久化的相容性

## 結論

Lattice Axiom 以 package SemVer 表达逻辑依赖相容，以 capability／ABI version 表达调用边界，以 `EngineBuildId` 表达 exact-build 耦合，以 schema／content／generator version 表达长期资料。Bevy 是核心 package 内部的 engine tool；它的升级不自动使所有模组 major bump，但只要外部可观察契约真的改变，就必须在正确 owner 上诚实版本化。

不存在一个能代替这些判断的全域 `engineVersion`。

## 相容性座標

| 坐标 | Owner | 回答的问题 | 不回答 |
| --- | --- | --- | --- |
| 产品 SemVer | release | 使用者取得哪次发行与支援线？ | package／save 是否相容 |
| package ID + SemVer | package owner | 逻辑 API／行为与 dependency range 是否相容？ | binary layout／精确 artifact |
| capability ID + interface version | capability owner | provider／consumer 可协商哪项服务？ | package source／save schema |
| native bootstrap／interface ABI | ABI owner | table、callback、batch layout 能否安全交换？ | gameplay behavior是否相容 |
| `EngineBuildId` | core build | engine-coupled artifact 是否针对同一 host？ | portable artifact 是否相容 |
| `latticeaxiom.lock` + artifact hash | package kernel | 这次 closure 精确选择了什么？ | 旧世界 record 如何迁移 |
| Cargo／Bevy lock | core build | host 使用哪些精确 Rust／Bevy 相依？ | 外部 package 的唯一版本语言 |
| stable content ID + revision | content owner | 方块／物品／entity type 是什么？ | code artifact identity |
| schema ID + version | schema owner | record／component 如何 decode 与 migrate？ | package dependency resolution |
| generator revision + config hash | worldgen owner | 未物化空间由什么算法／参数产生？ | 已物化 snapshot 是否重算 |
| asset format／semantic version | asset owner | importer／sidecar 如何解释资料？ | gameplay ABI |

这些坐标会在一次发布中共同出现，但不能互相代理。

## Package SemVer

package version 遵守 SemVer：

- `MAJOR`：公开 package contract／行为有破坏性变化；
- `MINOR`：向后相容新增 capability、definition、optional field 或行为；
- `PATCH`：不破坏 contract 的修正。

公开 contract 包括：

- dependency／capability；
- stable content／component／message ID；
- source SDK API 与生成的 registration semantics；
- 可观察 gameplay／worldgen／render behavior；
- package-owned schema 与 migration承诺；
- realization availability（若消费者依赖它）。

SemVer range 只表达 package owner 的逻辑承诺。lock 选择精确 version／source／artifact，正常启动不会因为 registry 出现新版本而偷偷重解。

### 同一 package 的 realization

`NativeStatic`、`PortableNative` 与 `EngineCoupledNative` 可以共享相同 package SemVer，只要它们的可观察 contract 与 registration 等价。artifact metadata 另记录：

- realization kind；
- target／profile；
- ABI／interface requirements；
- `EngineBuildId`（如适用）；
- manifest／artifact hash。

重新编译 engine-coupled artifact 不要求 package version 改变；它是同一逻辑 package 的新 exact-build artifact。若重编同时改变公开行为，则仍要按 SemVer 升级。

## Capability 與 ABI Version

capability version 用于 graph 解析与 runtime interface negotiation：

```text
package dependency: example.weather ^2.1
capability require: render.post-effect @1, minor >= 2
ABI require: native bootstrap 1, ecs.batch @1.3+
```

规则：

- capability major 改变语义／ownership／必需操作；
- minor 只 append optional 能力并保持旧消费者可工作；
- 每项 capability独立演进，不维护单一巨大 host API version；
- package 可以在一次 SemVer minor 中新增 optional capability provider；
- 如果既有 package contract 改为要求新 capability major，通常是 package major change；
- ABI compatibility 通过 header／table negotiation决定，不通过 package version 猜测。

## Bevy 是核心內部工具，但影響必須誠實

Bevy crate version 由核心 host 的 Cargo manifest／lock 固定。模组通常依赖：

- `latticeaxiom.engine` package／capability range；
- Lattice SDK／schema version；
- 对 dynamic realization 的 Lattice ABI／interfaces。

它们不直接声明「我需要 Bevy 0.19.1」作为 portable 相容契约。

### Bevy 更新不影响外部 contract

例如内部 API migration、renderer bugfix 或 scheduler implementation change，且：

- public Lattice package API／behavior 不变；
- portable ABI／capability不变；
- persisted schema／stable ID 不变；
- conformance 与性能预算通过。

此时更新 Cargo lock／`EngineBuildId` 即可；portable artifact 不需重编，相关 package 不需 major bump。

### Bevy 更新影响外部 contract

例如 Lattice 暴露的 schedule semantics、render slot、asset behavior、physics result 或 static SDK 必须破坏性改变。此时：

- 提升真正受影响的 package／capability／schema major；
- 提供 migration／adapter 或清楚拒绝；
- 重新生成 lock／registration fixtures；
- 不以「Bevy 是内部实现」否认外部 break。

### Engine-coupled artifact

每次 host build 计算 `EngineBuildId`，至少覆盖 core source、Cargo lock、features、target、profile、internal interface schema 与 relevant build flags。任何不同都可以精确要求重建，而不用污染 portable package SemVer。

## Bevy 升級 Gate

1. 选择明确 Bevy release／migration guide；冻结这次升级范围。
2. 更新 core Cargo lock 与 ecosystem compatibility matrix。
3. 重建 static／engine-coupled artifacts；保留 portable old-binary fixtures不重编。
4. 执行 compose／lock／registration conformance。
5. 执行 portable ABI load、engine-coupled rejection／rebuild、client／headless smoke。
6. 执行 save migration、asset fixtures、worldgen provenance 与性能基准。
7. 审查是否有外部可观察 break，并只提升受影响 owner 的版本。
8. 生成新的 `EngineBuildId`、SBOM／notices 与 release diagnostics。

## Lock 與重現

`latticeaxiom.lock` 保存完整游戏 closure；core `Cargo.lock` 保存 host build closure。两者职责不同并互相指纹：

```text
latticeaxiom.lock
├── package versions / sources / hashes
├── dependency / capability resolution
├── realization / target / artifact hashes
├── manifest / ABI / EngineBuildId requirements
└── core package identity + supported engine capability

core build metadata
├── Cargo.lock hash / Rust toolchain
├── Bevy and ecosystem versions / features
├── ABI / capability implementation versions
└── EngineBuildId
```

lock hash 不等于相容判断。两个 closure hash 不同可能只是纯视觉 patch；是否能打开世界仍由 required package、stable content IDs 与 schemas 判断。

## World 所需套件閉包

world metadata 保存真正影响恢复的最小要求：

- required package ID／compatible range 或 locked identity；
- 使用到的 stable content IDs；
- schema ID／version 与 owner；
- generator provenance；
- optional presentation package policy；
- 建立／最近保存 closure fingerprint 供诊断。

加载流程先以当前 graph 解析 required owner，再选择：

- 正常 writable load；
- migration；
- placeholder（仅允许的 presentation／non-authoritative资料）；
- opaque preservation／read-only recovery；
- 明确拒绝。

不能仅因产品版本或 Bevy patch不同就拒绝，也不能仅因 package ID 相同就忽略 schema break。

## Stable Content ID

存档引用 `latticeaxiom:block/stone`，不引用 Rust type name、Bevy `Entity`／`Handle`、registration order 或 dynamic callback index。

移除／更名政策必须明确：

- alias + migration；
- versioned snapshot rewrite；
- placeholder + 原 ID 保留；
- 拒绝 writable load。

runtime numeric palette 是 closure／snapshot local optimization；每个 persistent palette 保存 stable ID mapping，不能让 package discovery order 改变意义。

## 持久化 Schema

每类 record／persistent component 至少有 schema ID、owner、version、payload 与 integrity。owner 必须维护：

- current write version；
- supported read range；
- stepwise migration／explicit rejection；
- malformed／unknown field policy；
- golden old-version fixtures；
- realization-independent round-trip。

ABI-POD layout version 与 persistence schema version 不是同一坐标。一个 struct 可以改变内存布局但保持 wire schema，也可以保持 layout却改变业务语义；两者分别测试。

## 世界生成與衍生 Cache

未物化空间保存 seed、coordinate、generator revision、canonical config hash 与相关上游 fingerprint。已物化 snapshot 是权威：

- generator 更新不隐式重算旧 chunk；
- 新 chunk 记录新 provenance；
- boundary／regenerate 是显式规则／transaction。

mesh、collider、navigation、thumbnail 与 shader cache 保存 source fingerprint；不匹配就重建，不为衍生 cache 承诺昂贵 migration。renderer／Bevy patch 不能单独让权威 snapshot 失效。

## 常見變更矩陣

| 变更 | package SemVer | ABI／capability | `EngineBuildId` | schema／data |
| --- | --- | --- | --- | --- |
| core 内部 Bevy patch，contract 不变 | 通常不变 | 不变 | 变 | 不变 |
| static SDK 新增向后相容 helper | SDK minor | 可不变 | 变 | 不变 |
| portable table 尾端新增 optional fn | package 可不变／minor | interface minor | 变 | 不变 |
| 改变 batch ownership semantics | 受影响 package major | interface major | 变 | 视资料而定 |
| engine-coupled 重编，无行为变化 | 不变 | internal可不变 | 变 | 不变 |
| 移除 stable block ID | owner major | 通常不相关 | 可能变 | migration／拒绝 |
| persistent component 新格式 | owner按行为判定 | layout可能变 | 可能变 | schema version + migration |
| generator algorithm 更新 | owner按contract判定 | 不必变 | 可能变 | generator revision |

## 驗收

- package resolver、ABI loader、world loader 分别测试自己的版本坐标，没有全域 version branch。
- Bevy upgrade 演练中 portable old binary 不重编，engine-coupled artifact 精确要求重建。
- static／dynamic realization 切换不改变 package SemVer、stable ID、schema owner 或 save bytes。
- 每个 schema owner 有 old-version fixture；每个 supported ABI major 有 old-binary fixture。
- missing authoritative package／content 不 silent remap，也不因 renderer-only package 缺失无故破坏世界。
- lock／build fingerprint 能重现与诊断，但不被当成唯一相容判据。

## 相關文件

- [決策 0003：不以全域版本代替相容性](../decisions/0003-no-global-version-switch.md)
- [套件內核](package-management.md)
- [原生模組 ABI](native-module-abi.md)
- [世界持久化](world-persistence.md)
