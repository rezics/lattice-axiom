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

外部对照只吸收可验证原则：

| 对照 | 可吸收 | 不搬入 |
| --- | --- | --- |
| Minecraft 生存循环 | 挖／放／背包／热键栏／视距／设定 | 引擎内置维度、Numeric block ID、同进程标题页 |
| Terraria | 双轴探索节奏、近战／工具手感（后 D10） | 2D 专用物理或第二套 renderer |
| GregTech | 材料／电压／配方图作为 **science package** | 对 host 硬编码机器 Tick |
| Botania／奥术类魔法模组 | 植物／魔力／仪式作为 **thaumaturgy package** | Mixin 改原版配方 |
| FTB Quests / ATM 任务页 | 有向任务图、进度、成就 = **progress packages** | 任务 ID 写进 host |
| Touhou Little Maid | 作为 **relations consumer** 的同伴实体包 | 把女仆 AI 写进 core host |

## 初步可玩（本页不延期的玩家循环）

「初步可玩」不是空世界走两步。玩家循环必须在 **已锁定 terrenia 游戏闭包** 上达到 Minecraft
生存流程的可操作子集，同时机制走 Lattice 的 package／registration／Y-up Bevy，而不是 MC 的形状：

1. 准星、状态条（活力展示、挖掘进度、工具耐久）、9 格热键栏可切换。
2. 打开背包查看 36 格；热键栏是背包前缀。
3. 挖掘／破坏／放置走既有权威 DDA 与 gameplay catalog，不是 client 预测改世界。
4. Pause 打开设定：至少 **渲染距离**（chunk 半径）可调，且被 `PlayableWorldHardLimitsV1` 夹紧。
5. 地形可用色块／atlas fallback；缺 PNG 不得假装有文件。

开始页仍是独立 `ClientShellGraph` + `LaunchIntentV1` replacement process，见
[开始页](../architecture/world-lifecycle-and-start-ui.md)。本页不批准同 App 切 Playing。

## 两条玩法轨（post-D10 包，不是 host 分支）

科学与魔法是 **可替换 exactly-one 或 multi provider capability**，由 terrenia 聚合包选择。
host 只认识 capability 名，不认识 `gregtech:*` 或 `botania:*`。

```text
terrenia
├── @terrenia/blocks
├── @terrenia/worldgen
├── @terrenia/gameplay
├── @terrenia/tools
├── @terrenia/metallurgy          # 铜、合金、电压／热量材料图（science 输入）
├── @terrenia/science             # 机器、配方图、电缆；requires metallurgy
├── @terrenia/thaumaturgy         # 魔力、植物、仪式；requires blocks，不复制科学电压
└── @latticeaxiom/progress        # 任务／成就 DAG（平台，无 terrenia ID）
    └── @terrenia/journey         # Terrenia 章节页，引用 frozen Role 输出
@latticeaxiom/relations           # 有向关系图（平台）
└── @example/companion-maid       # TLM 类同伴，消费 relations，不进 host
```

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
| 铜、退火铜、青铜、黄铜 | `@terrenia/metallurgy` | 电缆、管道、工具头、魔法导体框 |
| 锡、锌 | 同一包的有限合金输入 | 不新增 host 金属表 |

配方与 GT 式处理图属于 science package；魔法只通过 Tag／Role 读取「导电／导热／封魔」资格。

## 进度、成就、任务编排

ATM／FTB Quests 的价值是 **可数据驱动的有向图**，不是任务 GUI 皮肤。

`@latticeaxiom/progress` 拥有：

- 稳定 `QuestId`／`ChapterId`／`AchievementId`；
- 有向边、互斥、隐藏、与 `ContentPredicate` 输入；
- 完成 receipt 进入 world snapshot（权威），UI 是 presentation。

`@terrenia/journey` 只贡献章节内容。发现顺序不得改变图 hash。禁止 host 写死「第一个任务是打树」。

## 关系网络

关系是一等 package，而不是实体组件上的临时 HashMap。

`@latticeaxiom/relations` 拥有：

- 有向边 `from`／`to`／`kind`／`strength`／`revision`；
- kind 由包注册（`pet`、`friend`、`subject`、`companion`），不是 host enum 穷举；
- 边的权威状态进 snapshot；AI／路径是 presentation／gameplay consumers。

Touhou Little Maid 类内容是 **consumer 包**：声明 companion kind、日程、物品权限，通过 relations
capability 绑到玩家。core host 不出现 `terrenia:maid` 或 `tlm:*`。

## 明确不做（在本页变成 ADR 之前）

- 不把 GregTech／Botania／TLM 源码或 ID 编进 `latticeaxiom-engine`。
- 不在 D0–D10 要求电压网络、魔力池、女仆 AI。
- 不新增自研任务引擎或关系 ECS 以绕过 Bevy。
- 不把渲染距离硬顶改成 Riverbed 式无界视距；夹紧仍服从 ADR 0026。

## 验证

- 本页任何新 logical package 都能从 terrenia 图中移除而不破坏 D9 72 方块基线。
- 科学／魔法交叉只通过 Tag／Role／Predicate 测试，不通过 host 字符串。
- 初步可玩 HUD／设定／背包由 production host 的 headless 或 CPU 契约测试覆盖，不拿「看起来像 MC」当证据。
