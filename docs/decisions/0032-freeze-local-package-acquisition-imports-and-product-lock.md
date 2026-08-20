---
title: 冻结本地包取得、Nickel package import 与产品 lock
status: accepted
type: decision
updated: 2026-08-20
---

# 决策 0032：冻结本地包取得、Nickel package import 与产品 lock

> 本决策修正决策 0010／0021／0022 中「完整 Nickel profile 先产生 package roots，再取得并解析
> package」的顺序。Nickel 仍是 package／profile／semantic authoring 语言，但外部 package 的发现、
> 取得与 package-aware import 必须由非可执行 bootstrap manifest 和 Rust package kernel 先行。

## 背景

D0 已经有 typed package／source／resolver intent、受控 Nickel staging、registration、runtime image、
launcher 与 Bevy client／headless 组件，但现有 resolution receipt 仍不是可独立消费的
`latticeaxiom.lock`。它没有封印 package manifest、不可变 source snapshot、toolchain、
`EngineBuildId` 与 artifact receipts，也无法让 `--frozen` 在不回退 mutable source 的条件下验证并
启动同一 closure。

同时，真正的 package-aware Nickel import 形成 bootstrapping 约束：若完整 `game.ncl` 必须先
`import terrain` 才能产生 root package request，kernel 就必须在知道要解析什么以前先取得 terrain。
把 network、filesystem search path 或 package discovery 放进 Nickel evaluator 会破坏 0021／0022
已经冻结的 source-table-only、无 ambient authority 与 fail-closed 边界。

因此，产品 lock 不能只是 resolver DTO 改名。D0 必须交付一个本地、内容寻址、可冻结的 package
management vertical slice；远程 registry、marketplace 与 production signing 仍按决策 0030 延后。

## 决策

### 1. D0 交付最小本地 package manager，而不是完整远程服务

Rust package kernel 从 D0 起负责：

- 读取静态 bootstrap／package source manifests；
- workspace／path source 与 local catalog 的发现、取得和验证；
- SemVer、feature、domain、capability、realization 与唯一版本解析；
- immutable source snapshot、content-addressed store、package alias map；
- candidate product lock、Nickel composition、artifact realization 与 final product lock transaction；
- `--locked`／`--offline`／`--frozen` 验证和稳定 diagnostics。

D0 不交付 public registry、账号、远程自动更新、排名、marketplace UI、透明日志或公共 build
service。local catalog 必须使用可被未来 remote transport 复用的 metadata／object 边界，但不能
假装已经具备 publisher authenticity。

Cargo 继续管理 host 与 workspace Rust build closure；Lattice package manager 管理游戏、
shell、world 与 runtime package closure。`Cargo.lock` 不被 `latticeaxiom.lock` 取代，后者通过
toolchain／producer／`EngineBuildId` receipt 指纹化前者产生的 host build。

### 2. 非可执行 bootstrap manifest 先于完整 Nickel composition

第一版增加两个 versioned Rust serde DTO：

| DTO／默认文件 | 责任 |
| --- | --- |
| `CompositionBootstrapV1`／`latticeaxiom.toml` | root requirements、source providers、projection、features／parameters、realization policy、evaluation policy 与 Nickel profile entry |
| `PackageSourceManifestV1`／`latticeaxiom-package.toml` | package identity／version、direct dependency aliases、graph-affecting feature／domain／capability／realization／trust projection、Nickel public entrypoints 与 source inclusion policy |

两者使用受限 TOML 作为 human-authored representation，解析后由 normative Rust DTO 验证；semantic
hash 与 lock 使用项目 canonical encoding，不对原始 TOML 排版作 identity。manifest 禁止脚本、
函数、import、环境展开、条件 I/O 与网络。

`PackageSourceManifestV1` 是 `PackageSpec` 的 graph-affecting 静态投影。package 自己的
`package.ncl` 可以相对 import 这份 TOML，并通过 `latticeaxiom.lib` constructor 产生完整
`PackageSpec`，再添加 registrations／semantic intent；两者重叠字段必须在 typed normalization 后
完全相等，否则以稳定 diagnostic 拒绝。Nickel 不能新增 bootstrap 未声明的 package dependency、
source provider、trust、namespace authority 或 realization。

`GameProfileSpec.roots`、`source_universe`、graph-affecting features／parameters 与 realization
policy 继续存在于 fully evaluated model，但它们只能是 `CompositionBootstrapV1` 已授权输入的
相同投影或更窄选择，不能成为取得外部 source 的新 authority。Nickel profile 可以组合配置、
overlay、semantic bindings 与 package exports，不能在求值过程中发现依赖。

### 3. 本地 source、catalog 与 CAS

D0 支持四种有界 source provider：

1. workspace package；
2. root-manifest-relative path package；
3. local catalog package；
4. explicit in-memory／filesystem fixture。

Path 只是一项 acquisition locator，不是 runnable identity。Kernel 按 0021 的 canonical path、
raw-byte SHA-256、NFC／case-fold collision 与 within-root follow-target symlink policy建立 source
table，随后把完整 source tree 快照为 immutable CAS object。Lock 记录 algorithm-tagged source
digest；运行期从 CAS snapshot 建立 source table，不直接读取原 path。

Source store 至少区分：

```text
source tree object
package manifest object
package archive object
realized artifact object
```

用户级共享 store 是可删除的 cache，不是 authority。权威来自 bootstrap manifest、package
manifests、product lock 与内容摘要；需要仓库自包含时可由未来 `vendor` action materialize exact
lock closure。`--frozen` 所需 object 缺失时必须失败，不能回读 path 重建。

Local catalog 至少提供：

- `PackageName + exact version` 到 manifest／source digest 与 object locator 的 deterministic index；
- deterministic pack／check／publish-to-directory／acquire fixture；
- 同一 `PackageName + version` 不得发布为不同 source digest；
- immutable version、显式 yank metadata fixture 与按 canonical bytes 排序的 candidate enumeration；
- 不执行 install／build lifecycle script。

未来 remote registry 在相同 manifest、index 与 object contract 外增加 transport、TUF-style metadata、
publisher／scope authority、revocation 与 transparency receipt；不得让 remote 支援要求重写 lock
或 Nickel import 语义。

### 4. Package-aware Nickel import 由 locked alias map 解析

Nickel authoring 的 public package import 语义冻结为 package-local alias：

```nickel
let terrain = import terrain in
let la = import latticeaxiom_lib_v2 in
...
```

`terrain` 必须是 importing package 的 `PackageSourceManifestV1.dependencies` 中声明的 direct
dependency alias；`latticeaxiom_lib_v2` 继续是 controller 明文授予的 versioned library alias。
Alias 在每个 importing package scope 内唯一，允许不同 package 对同一 dependency 使用不同局部名，
但不存在 ambient global alias table。

Import resolver 的输入是 `(current PackageInstanceId, ImportSpec)`：

- bare alias 只查 final／candidate lock 中当前 instance 的 direct dependency edge，并进入目标 package
  声明的 Nickel public entrypoint；
- quoted relative import 只在当前 package 的 immutable source tree 内解析；
- transitive dependency 不因存在于 closure 就自动可见；
- absolute／drive／UNC／device path、`NICKEL_IMPORT_PATH`、current directory、environment、network、
  registry 与原 mutable path 一律不可见；
- import cycle、missing／duplicate alias、entrypoint 缺失和 root escape 在执行 Nickel expression 前
  产生 stable diagnostic。

Worker 只接收 source bytes、logical source identity、alias edges 与 receipts。Diagnostic 使用稳定
`pkg://<package-instance>/<logical-path>` identity，并保留 byte range、line／column cache 与 origin
chain；CAS path、temporary staging path 和 local absolute origin 不进入 diagnostic、hash 或 lock
identity。

Lattice 不把 Nickel 上游实验性 package manager 的 downloader、registry 或 lock 设为产品 authority。
可复用其 `import alias` parser／resolver hook，但 production semantics、source grant、lock 与
`--frozen` 全部由 Lattice DTO 和 conformance corpus决定。没有 source-table-only loader、hard
memory／deadline／recursion capability 的 backend 继续按 0022 fail closed。

### 5. Resolution receipt、build intent 与 product lock 分层

现有 `ResolutionReceiptV1` 与 `BuildIntentV1` 保留诚实名称：

- `ResolutionReceiptV1` 证明某一 bootstrap input 与 candidate universe 如何选出 exact package graph；
- `BuildIntentV1` 描述 target／projection／toolchain 下需要 realization 的内容；
- 两者都是 final lock 的输入 receipt，不是可单独启动产品的 lock。

一次 non-frozen resolve transaction 为：

```text
CompositionBootstrapV1
  → acquire and snapshot candidate package manifests/sources
  → deterministic ResolutionReceiptV1
  → candidate alias/source-grant table
  → controlled Nickel composition
  → CompositionSpec + semantic/provenance/evaluation receipts
  → registration compile + BuildIntentV1
  → artifact realization and verification without native load
  → final latticeaxiom lock
  → atomic write, reopen and full verification
  → RuntimeImage construction / native load
  → launcher may create Bevy App
```

在 final lock 已原子写入并重新读取验证前，不得load native module、构造带process-local callback的
`RuntimeImage`、启动Bevy App／module business code或打开可写world。Candidate lock只存在于
resolve／realize transaction，不能被world metadata引用或冒充frozen lock。

### 6. 一个 lock schema 同时封印 portable resolution 与 target realization

`latticeaxiom.lock` schema 至少包含：

```text
LockV1
├── schema / producer
├── bootstrap manifest receipt
├── portable package resolution
│   ├── exact package instances and source/manifest digests
│   ├── dependency alias edges
│   ├── feature/domain/capability/provider selections
│   └── semantic/provenance/evaluation policy receipts
├── composition and registration receipts
├── target realizations
│   ├── projection / target / toolchain
│   ├── selected realization and artifact digests
│   ├── EngineBuildId where applicable
│   └── RegistrationImage receipt / RuntimeImage input fingerprint
└── resolution explanation identity
```

Portable resolution 与 target realization 分层，使相同 package/source graph 可拥有 Windows、
Linux、client 或 headless realization receipts，而不会让 platform path 污染 logical graph。Client
与 headless 从同一 lock 选择已封印 projection；它们不得各自重新 resolve。authoritative rows、
schemas、Role bindings 与 active bundles 必须具有相同 fingerprint，presentation rows可以不同。

`latticeaxiom-shell.lock` 使用同一 schema family，只把 scope／roots 固定为 client shell。
Per-world metadata 继续按 0027 保存 exact game lock bytes／content-addressed reference 与
`FrozenLockReceipt`；它不是第三种 resolver 或 lock schema。

Lock 使用 canonical encoding、stable byte ordering 与 versioned schema。它不包含 timestamp、
filesystem mtime、ambient current directory、local absolute path、随机 ID、Bevy `Entity`／`Handle`
或 process-local ABI handle。Source locator 可作为非权威、可省略的 acquisition hint receipt存在，
但不能参与 runnable source identity。

Lock 以同目录 temporary file、完整 flush／sync、atomic replace 与 reopen validation写入。Crash
fixture只能观察到旧完整 lock 或新完整 lock，不能接受 partial file。实现必须分别验证 manifest、
source、toolchain、`EngineBuildId`、registration 与 artifact receipt，不能只比较顶层 lock hash。

### 7. Locked、offline 与 frozen 的动作语义

- `resolve`／`upgrade`：允许改变package selection；它可以snapshot local changes，并在完成全部
  验证后原子发布包含所需target realizations的新lock。
- `realize`：保持portable resolution不变，为显式target／projection产生并验证artifact receipts，
  然后原子发布新final lock。
- `--locked`：要求现有lock bytes不变；可以按lock中的exact expected receipts补齐store object或
  重建可逐byte验证的artifact，但不能增加／改写resolution或realization receipt。
- `--offline`：禁止network provider；local path、local catalog与已有CAS仍按动作权限使用。
- `--frozen`：等价于locked + offline + read-only + exact-artifact。禁止fetch、path snapshot、
  build、source fallback、lock write与store mutation；缺失或不匹配立即失败。

普通 launch 先完整复用 lock，也不因 source universe出现新版本而隐式 update。即使是刚完成的
non-frozen resolve，launcher 也只能消费已经落盘、重新打开并验证过的 final lock。

### 8. 内容完整性不授予 activation authority

Package 出现在 CAS 或 lock 中只证明 exact bytes 与 resolution provenance，不证明 publisher 身份，
更不授予 namespace、native trust、world writer、network、persistence 或其他 capability。

Sealed catalog → world-db activation validation receipt 仍是独立的安全授权边界。普通 resolver、
local catalog 或 lock writer不得自行签发；产品 lock未来可以引用已由授权组件签发的 receipt digest，
但不能因 package 已安装或 hash 正确就合成 write authority。

## 结果

- D0 的 lock、Nickel import 与 local package acquisition 形成一个真实闭环，而不是一组互不相连的
  DTO／fixture。
- Package dependencies 在 Nickel 求值前已经确定，消除 bootstrapping cycle 与求值期网络／filesystem
  authority。
- Path authoring 保持快速，但 runnable closure 永远来自 immutable CAS snapshot。
- Local catalog 可以验证 check／pack／publish／acquire／frozen workflow，未来 remote registry无需改变
  package identity、object或lock contract。
- `ResolutionReceiptV1`／`BuildIntentV1` 保持可审计的中间证据，final lock才是 launcher与world
  preflight的执行 authority。
- Shell、client、headless 与world共享 resolver／lock schema，同时保留各自明确的 closure scope。

## 被否决的方案

### 直接采用 Nickel 上游 package manager 与 lock

它只管理 Nickel library，且当前仍是实验功能；其 downloader、source policy、lock与Lattice的
realization、artifact、`EngineBuildId`、registration、world与activation receipts不等价。只复用
语法或 upstreamable resolver hook，不转移产品 authority。

### 让完整 Nickel profile 动态发现 source 或 dependency

这会形成 package import bootstrapping cycle，并允许未锁定代码决定自己可以读取或下载什么。
Graph-affecting source、root、feature、parameter 与 realization input必须先进入静态 bootstrap DTO。

### Lock path package 的绝对目录并直接运行

目录内容可在 resolve 后变化，absolute path不可跨机器，且会绕过 CAS、artifact receipt与
`--frozen`。Path只用于 acquisition，snapshot digest才是 runnable identity。

### 只实现 path dependency，不实现 local catalog

它无法证明 package version不可覆盖、跨workspace安装、deterministic pack／acquire或未来 remote
transport边界。D0 local catalog是文件系统服务，不是public registry，但仍是package manager闭环的
必要 fixture。

### Package 安装即授予 capability 或 world writer

内容存在、hash正确与获得安全授权是不同事实。把它们合并会让普通package resolution越过
namespace、trust与world activation边界。

## 验证

- A package通过manifest alias `import b`，B继续相对import自身文件；undeclared transitive import、
  missing／duplicate alias、root escape与ambient import均在求值前稳定失败。
- Path source被修改后，旧lock仍只读取原CAS bytes；store object缺失时`--frozen`失败且不回读path。
- Local `check → pack → publish-to-directory → acquire → resolve → run --frozen` fixture全程无网络、
  无window／GPU并产生稳定lock bytes。
- 同一`PackageName + version`以不同source digest发布时失败；candidate enumeration随机化不改变
  resolution或lock。
- Lock write fault corpus覆盖temporary write、flush、replace、reopen与corruption；只接受旧完整或
  新完整lock。
- Manifest、source、toolchain、`EngineBuildId`、registration或artifact任一被篡改时`--frozen`
  精确指出失败receipt。
- CLI与embedded worker对package imports产生相同`CompositionSpec`、stable `pkg://` source identity、
  byte range与diagnostic order。
- 同一final lock驱动client建立一次fresh `DefaultPlugins` App并退出，headless无GPU推进fixed ticks；
  两者authoritative fingerprint相同，settings／observability provider均来自graph exactly-one选择。
- Launcher在final lock原子写入并reopen验证前不能启动App；world writer在独立activation receipt缺失时
  继续返回`ActivationEvidenceUnavailable`。

## 相关文件

- [决策 0010：Nickel 驱动 package system](0010-nickel-driven-package-system.md)
- [决策 0021：R0／R1 Package、Nickel 契约与解析政策](0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md)
- [决策 0022：受控 Nickel 求值计量与 Worker 协议](0022-freeze-controlled-nickel-evaluation-metering-and-worker-protocol.md)
- [决策 0027：权威世界与持久化契约](0027-freeze-authoritative-world-and-persistence-contract.md)
- [决策 0030：治理、分发与安全触发条件](0030-freeze-governance-distribution-and-security-triggers.md)
- [Package 内核与分发边界](../architecture/package-management.md)
- [第一个可玩 Demo 路线图](../planning/roadmap-first-demo.md)
- [执行期整合路线](../planning/roadmap-game-engine.md)
