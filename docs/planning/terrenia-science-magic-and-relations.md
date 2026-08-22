---
title: Terrenia 科学／魔法双轨与关系包规划
status: proposed
type: planning
updated: 2026-08-22
decision:
  - ../decisions/0018-package-kernel-from-first-vertical-slice.md
  - ../decisions/0019-separate-package-and-registration-identities.md
  - ../decisions/0020-semantic-registration-and-content-selection.md
  - ../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
  - ../decisions/0028-freeze-worldgen-content-and-asset-contract.md
---

# Terrenia 科学／魔法双轨与关系包规划

## 范围

本页把「初步可玩」之后的 Terrenia 玩法扩张写成 **package 图**，而不是 host 内置模组列表。
它不改写 [第一個可玩 demo](roadmap-first-demo.md) 的 D0–D10 出场门禁，也不授权把 Minecraft
全局注册表、Mixin、或同进程 title+world 搬进 Lattice。

外部对照只吸收可验证原则。实现研究可在 demo 仓库 `.temp/reference/` 下阅读（gitignore，不入库）：

| 对照 | 本地对照 | 可吸收 | 不搬入 |
| --- | --- | --- | --- |
| Minecraft 生存循环 | — | 挖／放／背包／热键栏／视距／设定 | 引擎内置维度、Numeric block ID、同进程标题页 |
| Terraria | — | 双轴探索节奏、近战／工具手感（后 D10） | 2D 专用物理或第二套 renderer |
| GregTech CEu Modern | `GregTech-Modern` | **材料表 + 电压层 + 配方图** 作为 science package 的 typed model；铜／青铜／电缆是材料行，不是 host enum | Mixin、对原版 Tick 的机器硬编码、把 `gtceu:*` 写进 engine |
| Botania | `Botania` | 自然魔法：mana 是独立 `StatePropertyKey`，与电压不是同一个整数 | Mixin、refmap、把植物 ID 写进 host |
| FTB Quests | `FTB-Quests` | `Chapter → Quest → Task/Reward` 有向图、依赖环在 **compile** 失败（对照 `DependencyLoopException`）、task 种类由包注册 | 全局 `ServerQuestFile` 单例、team 进度绕过 world snapshot |
| Touhou Little Maid | `TouhouLittleMaid` | `IMaidTask` 式 **可注册工作** + 关系边；同伴包声明 task UID 与物品权限 | 全局 `TaskManager`、把女仆 AI／模型格式写进 core host |

## 初步可玩（本页不延期的玩家循环）

「初步可玩」不是空世界走两步。玩家循环必须在 **已锁定 terrenia 游戏闭包** 上达到 Minecraft
生存流程的可操作子集，同时机制走 Lattice 的 package／registration／Y-up Bevy，而不是 MC 的形状：

1. 准星、状态条（活力展示、挖掘进度、工具耐久）、9 格热键栏可切换。
2. 打开背包查看 36 格；热键栏是背包前缀。
3. 挖掘／破坏／放置走既有权威 DDA 与 gameplay catalog，不是 client 预测改世界。
4. Pause 打开设定：至少 **渲染距离**（chunk 半径）可调，且被 `PlayableWorldHardLimitsV1` 夹紧。
5. 地形：P6 baseline 已适用——palette 色块 atlas、nearest clamp；缺 PNG 走 fallback，engine 不读 `placeholders/`。P3 每 tick apply 上限 16 jobs／16 MiB 已适用；无 2 ms 墙钟；startup／edit／collider safety 仍 drain until idle。

开始页仍是独立 `ClientShellGraph` + `LaunchIntentV1` replacement process，见
[开始页](../architecture/world-lifecycle-and-start-ui.md)。本页不批准同 App 切 Playing。

## 两条玩法轨（post-D10 包，不是 host 分支）

科学与魔法是 **可替换 exactly-one 或 multi provider capability**，由 terrenia 聚合包选择。
host 只认识 capability 名，不认识 `gregtech:*` 或 `botania:*`。

```text
terrenia                              # in lock；deps: blocks/worldgen/gameplay/tools/presentation
├── @terrenia/blocks                  # in lock
├── @terrenia/worldgen                # in lock
├── @terrenia/gameplay                # in lock
├── @terrenia/tools                   # in lock
├── @terrenia/presentation            # in lock；client；authored PNG 源列出 placeholders/，host P6 用色块 atlas
├── @terrenia/metallurgy              # data-only skeleton；not in lock；copper/annealed-copper/bronze/brass
├── @terrenia/science                 # data-only skeleton；not in lock；recipes []；voltage StatePropertyKey
├── @terrenia/thaumaturgy             # data-only skeleton；not in lock；rituals []；mana StatePropertyKey
└── @latticeaxiom/progress            # data-only skeleton；not in lock；chapters []；compile-fail cycles
    └── @terrenia/journey             # data-only skeleton；not in lock；chapters []；requires progress
@latticeaxiom/relations               # data-only skeleton；not in lock；kinds pet/friend/subject/companion
└── @example/companion-maid           # not in packages/
```

`latticeaxiom.toml` sources／`latticeaxiom.lock` 不含 metallurgy／science／thaumaturgy／progress／journey／relations。engine 无 `gtceu:`／`botania:`／`tlm:` ID。

### 有机结合，而不是两个互不说话的模组

结合点必须是 **semantic Role／Predicate／材料 Tag**，禁止「if science then skip magic」的 host 分支：

- 同一铜矿可同时满足科学电缆与魔法导体 Predicate；具体机器／祭坛在 command 前解析 frozen Role。
- 科学电压与魔法 mana 是不同 `StatePropertyKey`；不得共用一个「能量」整数冒充两种物理。
- 进度包可以要求「科学章节」与「魔法章节」的交叉节点（例如：用青铜外壳封一棵魔力花）。
- 去掉 `@terrenia/science` 或 `@terrenia/thaumaturgy` 后，另一方与已物化 snapshot 仍可载入。

### 铜与合金

在 D9 72 方块基线之后，冶金扩张优先铜，而不是立刻上铁／钢梯子：

| 材料 | 包 | 用途 |
| --- | --- | --- |
| 铜、退火铜、青铜、黄铜 | `@terrenia/metallurgy` `materials-v1.json` | `terrenia:tag/conductive`／`thermal` |
| 锡、锌 | 未出现在该 JSON | 不新增 host 金属表 |

配方与 GT 式处理图属于 science package；魔法只通过 Tag／Role 读取「导电／导热／封魔」资格。

## 进度、成就、任务编排

ATM／FTB Quests 的价值是 **可数据驱动的有向图**，不是任务 GUI 皮肤。

`@latticeaxiom/progress` 拥有：

- 稳定 `QuestId`／`ChapterId`／`AchievementId`（对照 FTB 的 Quest / Chapter，但 ID 走 Lattice `StableId`）；
- 有向边、互斥、隐藏、与 `ContentPredicate` 输入；compile 检测环，禁止 runtime 单例 quest file；
- Task／Reward **种类**由包注册（对照 FTB `TaskTypes`／`RewardTypes`），平台包不穷举「击杀僵尸」；
- 完成 receipt 进入 world snapshot（权威），UI 是 presentation。

`@terrenia/journey` 只贡献章节内容。发现顺序不得改变图 hash。禁止 host 写死「第一个任务是打树」。

## 关系网络

关系是一等 package，而不是实体组件上的临时 HashMap。

`@latticeaxiom/relations` 拥有：

- 有向边 `from`／`to`／`kind`／`strength`／`revision`；
- kind 由包注册（`pet`、`friend`、`subject`、`companion`），不是 host enum 穷举；
- 边的权威状态进 snapshot；AI／路径是 presentation／gameplay consumers。

Touhou Little Maid 类内容是 **consumer 包**：声明 companion kind、可注册工作（对照 `IMaidTask` UID）、
日程与物品权限，通过 relations capability 绑到玩家。core host 不出现 `terrenia:maid` 或 `tlm:*`，
也不实现全局 TaskManager。

## 明确不做（在本页变成 ADR 之前）

- 不把 GregTech／Botania／TLM 源码或 ID 编进 `latticeaxiom-engine`。
- 不在 D0–D10 要求电压网络、魔力池、女仆 AI。
- 不新增自研任务引擎或关系 ECS 以绕过 Bevy。
- 不把渲染距离硬顶改成 Riverbed 式无界视距；夹紧仍服从 ADR 0026。

## 验证

- 本页任何新 logical package 都能从 terrenia 图中移除而不破坏 D9 72 方块基线。
- 科学／魔法交叉只通过 Tag／Role／Predicate 测试，不通过 host 字符串。
- 初步可玩 HUD／设定／背包由 production host 的 headless 或 CPU 契约测试覆盖，不拿「看起来像 MC」当证据。
