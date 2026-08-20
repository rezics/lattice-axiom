---
title: 冻结治理、分发与安全触发条件
status: accepted
type: decision
updated: 2026-08-20
---

# 决策 0030：冻结治理、分发与安全触发条件

## 背景

Package kernel、确定性 lock、registration、可信 native ABI 与 client／server domain projection
已经是第一个 vertical slice 的必要基础；public registry、多平台预建服务、production
`WasmComponent` 与多人网络运行期则明确延后。若只写「以后由真实需求决定」，实现者仍会在
publisher 身份、artifact fallback、support window、资源预算和握手 hash 上作出互不兼容的选择。

本决策关闭 `open-questions.md` 中「P2：治理、分发与安全」的七项问题。它冻结首版政策、数值和
自动证据，但不会把这些规模能力提前塞进 Terrenia D0–D10。除 native trust preflight 外，
public registry、production Wasm 与多人网络服务都必须满足本页的 D10 后触发 gate 才进入产品路线。

## 决策

### GOV-01：public registry 由可重现的 creator distribution workflow 触发

Production public registry 不是 D0–D10 的交付物。Contract type、fixture 和 local mock 可以提前
存在；公共服务、remote source 默认启用与 registry availability 承诺必须等 D10 自动门禁完成后，
并同时取得以下证据：

1. 至少三名不共享 Lattice Axiom workspace 的外部作者完成 scripted publish workflow。
2. 至少五个 package、累计十个 immutable versions 进入 conformance corpus；其中至少一个是
   data package，至少一个是 `PortableNative` package。
3. 每个示范 package 都从作者环境、第二个 clean client 环境和 dedicated／headless CI 环境安装；
   三者产生相同 logical resolution 与 authoritative fingerprint。
4. 至少两个 issue／workflow receipt 证明手工目录、zip 或 Git URL 已阻塞更新、撤回、重现或
   server 部署；抽象的未来需求不计数。
5. Local `check → build → manifest → sign → publish --dry-run → resolve --frozen` 全程已有
   non-interactive fixture。

Registry MVP 只提供 root／scope claim、immutable publish、content-addressed fetch、resolve、yank、
security revoke 与 metadata API／CLI。排名、评论、推荐、付费、marketplace layout 和 auto-update UI
不属于 MVP。Federation 只有在第二个独立 operator 出现，并通过相同 signed-metadata、namespace、
yank／revocation 与 deterministic-resolution corpus 后才触发。

Network fetch 属于 Rust package kernel。Kernel 先验证和固定 remote source snapshot，再把 immutable
root 授给 Nickel worker；Nickel evaluator仍不得访问 network、ambient filesystem、clock或environment。

### GOV-02：publisher、签章、delegation 与 revocation

Public distribution 使用 TUF-style versioned metadata 和签名的 canonical release envelope；不得发明
由 TLS、registry account session 或 artifact hash 单独冒充 publisher authenticity 的协议。

- `PublisherId` 是 registry 分配的 immutable opaque identity。Display name、email、package name、
  source path 与 registration namespace 都不是 publisher identity。
- Release envelope v1 以项目 canonical encoding 编码并用 SHA-256 寻址；签章算法为 Ed25519。
  Envelope 至少覆盖 package name／version、source hash、manifest hash、realization、target artifact
  hashes、producer／toolchain provenance、metadata schema version 与发布时间。
- Organization publisher root 默认使用三把 offline key 的 2-of-3 threshold。Individual publisher
  可以使用 1-of-1 root，但必须登记独立 recovery credential。Root 可以委派最长三十日的 online
  release key；delegation 必须限制到明确 root package／scope pattern。
- Registry root metadata最长有效三百六十五日，targets／delegation metadata三十日，snapshot七日，
  timestamp二十四小时。Online resolve／publish必须使用未过期metadata并拒绝version rollback、freeze
  与mix-and-match。
- Package root／scope ownership、publisher trust与registration namespace grant是三个独立检查。
  有效签章不能自动取得同名namespace，ordinary package也不能自授trust或namespace。
- `yanked`只把版本排除在新resolution之外；已存在的exact lock仍可启动并产生warning。
  `revoked-security`在客户端取得该receipt后禁止evaluate、build或load，即使lock是frozen。
  Recovery只能走不加载被撤回code的raw read-only inspect／export。
- 已安装exact lock离线时可以继续使用最后验证的metadata；过期metadata必须显示stale-security状态，
  不得执行remote resolve／upgrade。任何缓存中已知的security revocation仍然fail closed。
- Key rotation由旧key与新key共同签署，或由上级offline threshold授权。Publisher transfer同样产生
  显式、可审计的ownership transition，不改变PackageName或StableId。
- Public registry MVP保存append-only transparency entry与inclusion receipt；客户端lock／diagnostic
  保存receipt identity，但transparency可用性不进入authoritative gameplay hash。

签章验证发生在Nickel evaluation、build script和native load之前；它不替代manifest、ABI、schema、
semantic或capability validation。

### GOV-03：prebuilt target matrix 与 source fallback

Public build service v1提供以下machine-readable matrix：

| Tier | Target | Profile |
| --- | --- | --- |
| 1 | `x86_64-pc-windows-msvc` | client |
| 1 | `x86_64-unknown-linux-gnu` | client、dedicated server |
| 1 | `aarch64-apple-darwin` | client |
| 2 | `aarch64-unknown-linux-gnu` | dedicated server build／CI |
| 2 | `x86_64-apple-darwin` | client build／CI |

Tier 1 是 Lattice Axiom first-party game closure 的发布门禁；第三方 package 可以声明较窄的supported
target set，但resolver必须把缺失target报告为`realization unavailable`，不能静默换成不相容artifact。
Tier 2 要求build和conformance，不承诺每个release都提供下载artifact。

- x86预建使用通用architecture baseline，禁止`target-cpu=native`。额外SIMD成为显式artifact
  feature variant并有baseline fallback。
- Data artifact和未来`WasmComponent`是target-neutral。`PortableNative`由target triple、ABI／interface
  range和artifact hash选择。`EngineCoupledNative`只针对exact `EngineBuildId`，不承诺公共矩阵，且必须
  有portable、static、disabled或feature-specific fallback。
- `.rlib`不作为稳定distribution artifact。
- `auto`先选已验证prebuilt；只有publisher提供签名source hash、toolchain recipe与expected manifest，
  且profile明确允许`build` trust时，才可选择source build。
- Source fallback只发生在用户明确执行的non-frozen resolve／build action。它必须显示将执行build
  script，产生新artifact／producer provenance并写入新lock。
- `--frozen`与world exact-open只接受lock中的exact artifact；missing／wrong target／hash时精确失败，
  永远不在启动过程中source fallback。

### GOV-04：deprecation telemetry 与 support window

Package、ABI／capability和persistent schema继续独立版本化；它们使用以下首版窗口：

| Contract | v1 support policy |
| --- | --- |
| ABI `0.x` | 只承诺当前development minor；仍永久保留至少两代不可重编old-binary fixtures |
| ABI `1.x+` | current major和previous major；至少十八个月或两个stable product release trains，取较长者 |
| first-party package | current major和previous major接受security／critical correctness修复至少十二个月 |
| authoritative／world schema | current write major和前三个write majors，或三十六个月，取较长者 |

ABI major removal必须至少提前一个stable product release公告。Registry version与artifact保持immutable；
support结束改变metadata状态，不删除可审计的release。超出authoritative schema窗口的world仍必须获得
不加载unknown／revoked code的read-only inspect／export诊断路径，不能silent rewrite或silent remap。

Manifest／registry metadata v1增加：`deprecated_since`、`replacement`、`sunset_not_before`、
`support_status`与`last_supported_host_range`。Loader不写猜测性shim；每个supported ABI major有
old-binary fixture，每个supported schema write major有golden decode／migration／rejection fixture。

Compatibility report默认在本地生成。Outbound telemetry仅opt-in，并只发送package／ABI／schema major、
success／reject／migration result、host release与target的aggregate；不得发送WorldId、player identity、
absolute path、package contents或setting values。Raw event最多保留三十日，aggregate最多十三个月。
Support removal不能只由telemetry决定；自动gate至少检查window已经结束、replacement／recovery action存在、
deprecation metadata完整、old fixture给出预期结果且release notes包含sunset。

### GOV-05：`WasmComponent` threat model、capability 与预算

Production `WasmComponent`只在D10完成后进入路线，并还要同时满足：

1. 至少一个真实外部untrusted package需要data package无法表达的behavior。
2. 同一非玩具package有static／portable／Wasm registration与authoritative-state conformance。
3. Hostile-code corpus、fuzzing和runtime security review通过。
4. 下列首版资源与performance gate在所有Tier 1 targets有machine-readable baseline。

威胁模型假定Wasm package恶意，必须保护host memory、其他package／world、filesystem、network、secret，
并限制CPU、memory、host-call和queue DoS；不承诺防御microarchitectural side channel或runtime 0-day。

Capability policy默认deny-all：不提供ambient WASI filesystem／network／environment；filesystem最多
preopen只读package assets与独立scratch；clock与random使用具名host capability，authoritative路径只能
取得deterministic tick与host-owned RNG；ECS、message、asset和task只经Lattice stable batch interface；
不得取得Bevy／Rust／OS handle或建立foreign thread。Async work进入Bevy task pool和有界queue。

`latticeaxiom:wasm-runtime-policy/default@1`的首版上限为：

| Limit | Value |
| --- | ---: |
| linear memory per instance | 64 MiB |
| aggregate memory per package | 256 MiB |
| fuel per authoritative callback | 10,000,000 |
| fuel per instance per second | 100,000,000 |
| input + output bytes per callback | 4 MiB |
| host calls per callback | 1,024 |
| queued async jobs per instance | 64 |
| synchronous wall deadline | 2 ms或fixed-tick budget的10%，取较小者 |

Package不能提高预算；profile policy可以选择另一个versioned policy ID。改变计量单位、终止语义或缩小
accepted input需要新policy major。Fuel、memory、deadline或output trap丢弃整个callback transaction，
不发布partial command，也不推进authoritative revision。

上线gate要求全部Wasm systems的P95不超过fixed-tick budget的10%，P99不超过20%；调用按system／batch，
不逐entity；十分钟固定旅程中memory、queue和in-flight bytes没有无界增长。PortableNative对照数据必须
记录，但absolute tick／latency budget而非固定倍率决定可否上线。

首版增加`WasmComponent` realization与`SandboxedCode` trust class；不得把Nickel evaluation worker、
native C ABI validation或普通process isolation宣传成这一sandbox。

### GOV-06：native full-process consent

所有`NativeStatic` build／build script、`PortableNative`和`EngineCoupledNative`在执行前都显示或输出相同
风险语义，不因artifact签名有效而隐藏：

> 此 package 获得完整进程权限：可读取或修改游戏内存，以你的操作系统账户访问文件和网络，并可使
> 程序崩溃。ABI 验证只检查兼容性，不是安全沙箱。

Preflight至少显示package／version、PublisherId与key fingerprint、source、signature／revocation状态、
realization、transitive dependency chain、source／artifact hash、将执行build script还是加载binary，
以及允许它的trust policy来源。

- Consent key是`(publisher key fingerprint, package root／scope, trust class)`。
- Unsigned→signed identity变化、signing key变化、scope扩大、trust升级到native或revocation状态变化都
  使旧consent失效。
- 每个artifact更新都显示version／hash diff；只有用户明确授予该publisher signed auto-update trust时，
  同scope、同trust class的更新才免逐次确认。
- Trusted developer local profile可以显式预授权workspace source，但receipt仍进入audit log。
- Headless默认fail closed，只接受精确policy file中的PublisherId／key／scope／trust allowlist；不提供
  `--yes`或`allow-all-native`产品选项。
- 每次build/load写machine-readable audit event。Risk summary不是advanced-only内容，并使用与其他
  package preflight相同的keyboard、controller和screen narration语义树。

这项preflight不等待public registry；local／unsigned native path在R4／R5出现时就必须执行。

### GOV-07：多人 authoritative closure 与 presentation negotiation

Multiplayer network runtime仍是D10后能力。开始产品实现前必须有D10 clean journey、一个真实dedicated
server、两个独立automated clients、versioned replication interface和下列join corpus；rollback／lockstep
不是closure negotiation的前置条件，也不能由本决策暗中承诺。

`JoinClosureOffer@1`／`AuthoritativeClosureReceipt@1`在任何world module business code执行前协商。
Authoritative receipt包含：

- protocol major、world identity／epoch；
- authoritative package name／version与source release hash；
- dependency／capability provider selection；
- authoritative registration、schema与semantic fingerprint；
- authoritative Role bindings、active bundles和settings fingerprint；
- required network／replication interface ranges。

Receipt不包含target-specific artifact hash、realization kind、`EngineBuildId`、client／server-only rows、
presentation asset或local user setting。因此相同签名source的static／portable和Windows／Linux artifacts
可以协商成功；authoritative source、package version、schema、semantic、binding、bundle或setting任一
不相容都必须精确拒绝。不得比较包含artifact与presentation provenance的完整`lock_hash`冒充协议。

Presentation offer只允许三类：

1. `optional-presentation`：缺失使用safe fallback／placeholder，不阻止join。
2. `recommended-presentation`：提示可安装，拒绝后仍可join。
3. `required-client-interface`：只用于network／input等确实必需的versioned capability，不能把纯shader、
   asset或HUD伪装成authoritative requirement。

Client-only package不得注册authoritative rows、schema或settings；manifest validation在code load前拒绝。
Server不信任client authoritative calculation，client presentation只能消费server明确公开的replication／
inspect fields，不能从presentation cache推断隐藏state。Remote acquisition完成signature／trust验证后重新
握手；join不得隐式修改world frozen lock。失败diagnostic展开package、schema、semantic、setting与
capability差异，不只报告fingerprint mismatch。

## 自动证据

实现任何一项production能力前，CI必须能在clean checkout、frozen inputs和无window／GPU／人工输入下
重现对应证据：

- Registry：publish／install／yank／revoke、randomized discovery、offline cache、rollback／freeze／
  mix-and-match与三环境lock parity。
- Signing：threshold不足、expiry、key rotation、scope／namespace越权、publisher transfer与security
  revocation在evaluate／build／load前失败。
- Artifact：Tier 1 matrix完整；无toolchain机器使用prebuilt；non-frozen source fallback与frozen exact
  failure；wrong target／ABI／hash不执行code。
- Support：ABI old binaries、schema old versions、sunset boundary、read-only export与telemetry privacy／
  opt-out fixture。
- Wasm：infinite loop、memory grow、oversized I/O、host-call storm、path／network escape、forged handle、
  trap-after-command、queue bound与P50／P95／P99 baseline。
- Native consent：interactive command-input与headless allow／deny、transitive native、key change、trust
  escalation、revocation和audit receipt。
- Multiplayer：不同target／realization接受；optional presentation增删接受；任一authoritative维度变化
  精确拒绝；hidden inspect不下发；握手bytes不受package discovery order影响。

人工creator研究可以补充可用性证据，但不能替代上述自动出场门禁。

## 结果

- D0–D10继续集中在package-driven Terrenia sandbox，不被registry、Wasm和multiplayer规模工作阻塞。
- D10后的实现触发条件不再由主观「生态够大」判断，而有creator、consumer、platform和自动证据。
- Publisher authenticity、namespace authority、native trust与semantic correctness保持独立。
- Frozen world不会因missing prebuilt、registry更新或client presentation差异静默改变authoritative closure。
- 首版resource、support与expiry值都有versioned owner；后续可依据真实测量另立决策调整，不能无记录漂移。

## 被否决的方案

### 在D10前先建立完整registry／marketplace

它不会增加当前voxel、persistence、dual-realization或Terrenia sandbox证据，却会提前锁定尚无creator
consumer的运营接口。

### 有效签章即视为安全package

签章只证明release envelope由获授权key发布；它不能证明native code安全，也不能取代ABI、schema、
semantic、capability和namespace validation。

### Frozen启动时自动从source重建missing artifact

这会执行新的build code、产生新的artifact并改变exact lock；正确动作是精确失败，再由用户执行
non-frozen resolve／build transaction。

### 用完整lock hash作multiplayer compatibility

完整lock包含target artifact、realization和presentation provenance，会错误拒绝语义相容的client／server；
反过来只比较package name又会漏掉authoritative source、schema、semantic与setting break。

## 相关文件

- [套件内核与分发边界](../architecture/package-management.md)
- [原生模组 ABI](../architecture/native-module-abi.md)
- [版本与相容性](../architecture/versioning-and-compatibility.md)
- [套件驱动的 Bevy runtime](../architecture/game-engine-runtime.md)
- [第一个可玩 demo 路线图](../planning/roadmap-first-demo.md)
- [执行期整合路线图](../planning/roadmap-game-engine.md)
- [待决问题](../planning/open-questions.md)
