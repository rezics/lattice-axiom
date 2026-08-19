---
title: 以 Nickel 驅動套件系統並以 Rust 執行
status: accepted
type: decision
updated: 2026-08-19
---

# 決策 0010：以 Nickel 驅動套件系統並以 Rust 執行

## 背景

Lattice Axiom 的游戏不是一组由 Cargo 偶然拼在一起的 crate，而是一个可检查、可锁定、可用不同方式实现的套件闭包。套件要表达逻辑身分、SemVer、相依、capability、参数、内容与 realization；整合包与使用者还需要可组合的默认值、函数与 overlay。

Nickel 的 record、function、contract、merge 与嵌入式求值能力适合构造这个控制平面。但版本解析、来源取得、内容雜湊、信任、建置、动态加载和副作用治理更适合由 Rust 实现。Nickel 上游的 package manager 管理 Nickel library，且官方仍将其描述为实验功能；它不是 Lattice Axiom 游戏套件的现成内核。

## 決策

1. 套件系统是 Lattice Axiom 的产品核心。官方内容、测试内容、静态模组与动态模组都必须先成为 package graph 节点，不能绕过 package kernel 直接加入 Bevy App。
2. 逻辑套件版本遵守 [Semantic Versioning](https://semver.org/)。package manifest 声明 package ID、version、dependency range、capability、schema owner、realization、feature／parameter 与资产；lock 选择精确版本、来源与 artifact hash。
3. `package.ncl`、`game.ncl`、整合包与使用者 overlay 采用 Nickel。版本化 `latticeaxiom.lib` 提供 `Package`、`GameProfile`、`Dependency`、`Capability`、`Realization` 与相关 contract／组合函数。
4. `latticeaxiom-compose` 通过 `nickel-lang` Rust API完整求值，并直接反序列化为强型别 `CompositionSpec`；不以 canonical JSON 作为求值器、resolver、planner 与 loader 的无型别内部总线。
5. Rust package kernel 负责来源取得、SemVer／capability 解析、唯一版本政策、循环与冲突、信任、lock、store／cache、build、load、activation 与诊断。Nickel 不自行下载、执行 compiler 或加载动态库。
6. 管线的核心语义型别是 `CompositionSpec`、`LockedGameGraph`、`BuildPlan`、`RegistrationImage` 与 `RuntimeImage`。每一层只携带已满足的不变量，并有独立 schema／semantic version、round-trip 与 golden fixture。
7. `latticeaxiom.lock`、发布描述符与诊断可使用 canonical serialization。它们是强型别模型的持久化／交换表示；其 schema 与 producer version 必须记录，且可从 Nickel source 与受控来源重建。
8. Bevy 是 Lattice Axiom 核心套件内部使用的 engine／build tool，不是每个模组可各自解析的 crate 相依。package graph 可以包含逻辑 `latticeaxiom.engine` package／capability；它表达外部可观察契约，不把 `bevy = 0.x` 暴露成模组相容性的唯一判断。
9. Bevy 更新不自动造成所有 package major bump。但若更新改变了外部可观察的 Lattice API、capability 行为、schema、渲染语义或 engine-coupled interface，相关 owner 必须按 SemVer／ABI 规则诚实升级；不能以「Bevy 是内部工具」掩盖破坏。
10. Nickel 与 package kernel 不进入热路径。Bevy App 启动前得到已验证的 `RegistrationImage`／`RuntimeImage`；游戏 tick 使用 numeric ID、typed registry、连续资料、direct function 或批次 callback。
11. v1 实作真实 SemVer range 检查、确定性选择政策、精确 lock 与清楚冲突诊断；远端 registry、通用 PubGrub／SAT 规模、多来源 marketplace、签章透明日志与分散式 artifact cache 可延后，但不能用延后规模功能为理由省略核心 graph。

## 權責分界

| Nickel 控制平面 | Rust package kernel／host |
| --- | --- |
| record／function／contract／merge | 来源 I/O、hash、signature 与 trust |
| package 与 profile 宣告 | SemVer／capability 解析与冲突诊断 |
| defaults、overlay、参数化组合 | lock、store、cache 与 build execution |
| realization 候选与 policy intent | target／artifact 选择与 ABI validation |
| 纯资料 plan | dynamic load、instance lifecycle 与 Bevy activation |

## 管線

```text
package.ncl + game.ncl + overlays + latticeaxiom.lib
                         ↓
              Nickel contracts / merge
                         ↓ direct typed conversion
                 CompositionSpec
                         ↓ resolve + lock
                LockedGameGraph
                         ↓ realization planning
                    BuildPlan
                         ↓ build / verify / load
          RegistrationImage + RuntimeImage
                         ↓ host adapter
                      Bevy App
```

## 護欄

- Nickel 求值不得读取网络、时钟、随机数或未宣告主机状态。package kernel 先取得来源并提供受控 import root。
- import path、求值时间、递归、内存、输出大小与错误来源必须有政策；不可信 package 不能用 manifest 求值取得任意 filesystem 能力。
- `latticeaxiom.lib` contract 与 Rust `CompositionSpec` 由同一组 conformance fixtures 共同版本化。
- source graph、locked graph、build graph、registration image 与 runtime instance graph 必须分开；不能把未解析 range 或 Nickel function 带入 runtime。
- package SemVer 不代替 ABI、schema、`EngineBuildId` 或 artifact hash；这些是独立相容坐标。

## 結果

- Nickel 成为套件声明与游戏组合的强大语言，而 Rust 保留副作用、安全、性能与宿主整合的清楚边界。
- 所有内容都经同一 graph 与 lock，官方内容无法绕过相依、capability 或冲突规则。
- Bevy 可以在核心套件内部升级，同时外部相容性由 Lattice-owned contract 诚实表达。
- 项目承担 package kernel、SDK、lock format 与诊断的长期维护；这不是可延后的附属工具。

## 被否決的方案

### 只用 Cargo／Bevy Plugin 當套件系統

Cargo 能解决 Rust build 依赖，Bevy Plugin 能注册 runtime 系统，但两者都不表达游戏 profile overlay、动态 artifact、内容 capability、world 所需 closure 或跨 realization 身分。

### 把 resolver、下載與建置全部寫進 Nickel

这会把网络、信任、并发、取消与错误恢复塞进纯组合语言，模糊副作用与安全边界。

### 直接採用 Nickel 實驗性 package manager

它的目标与稳定性承诺不同，不能代理游戏 package、native ABI、资产、capability 与世界相容性。

### 用產品版本或 Bevy 版本取代 package SemVer

单一版本无法说明具体依赖、capability、schema 与 artifact 是否相容，也会让无关更新造成整个生态大版本断裂。

## 驗證

- CLI 与嵌入 host 对同一 Nickel source 得到语义相同的 `CompositionSpec`，不建立 JSON 中间总线。
- SemVer range、pre-release、capability provider、cycle、duplicate ID 与 incompatible realization 都有 deterministic golden tests 和可行动诊断。
- 随机化 source discovery 与 registration 顺序不改变 lock hash、numeric ID 或权威 state hash。
- package kernel 可在不启动 Bevy renderer 的 headless 测试中完成 compose／resolve／plan／manifest validation。
- runtime dependency graph 不含 Nickel evaluator；tick 不查询 package kernel。

## 外部依據

- [Nickel 原始碼倉庫](https://github.com/nickel-lang/nickel)
- [Nickel Rust API](https://docs.rs/nickel-lang/latest/nickel_lang/)
- [Nickel package management](https://nickel-lang.org/user-manual/package-management/)
- [Semantic Versioning 2.0.0](https://semver.org/)

## 相關文件

- [決策 0008：靜態與動態共用一圖](0008-static-and-dynamic-realizations-share-one-graph.md)
- [套件管理架構](../architecture/package-management.md)
- [版本與相容性](../architecture/versioning-and-compatibility.md)
