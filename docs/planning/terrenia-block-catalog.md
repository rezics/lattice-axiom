---
title: Terrenia 方块内容规划
status: active
type: planning
updated: 2026-08-19
---

# Terrenia 方块内容规划

## 结论

Terrenia 的第一个完整内容基线规划为 **72 个方块定义与 2 个独立流体定义**。
它不是首个 demo 的一次性交付量，而是从 registration／持久化验证到自然世界、建造和
最小生存循环的分阶段目标：

| 阶段 | 累计方块数 | 目的 |
| --- | ---: | --- |
| D3：语义与持久化最小集 | 6 | 验证 StableId、Tag／Map／Role、挖放、掉落和存档 |
| D4：世界生成切片 | 18 | 让两个地表／群系风格和基础洞穴在材质上可辨识 |
| Demo 后自然世界基线 | 40 | 支撑地质、水文、地下群系、资源和植被原型 |
| Terrenia 第一内容基线 | 72 | 加入完整基础建材与最小功能方块，形成可持续扩展的玩法底座 |

数量统计只包含具有独立内容身份、掉落或语义用途的 block definition。朝向、原木轴向、
点燃、成长、上下半部与有限几何形态等 instance variation 使用 `BlockState`；流体液位使用
流体状态。不得为了达到数量目标把这些有限状态展开为大量 StableId。

## 当前状态

目前架构文档只把下列四个方块绑定为 Terrenia 的最小默认世界角色：

```text
terrenia:block/air
terrenia:block/grass
terrenia:block/dirt
terrenia:block/stone
```

`terrenia:block/copper-block` 曾用于 semantic registration 示例，但此前没有正式进入
Terrenia shipped content 清单。本页将它与 obsidian fixture 一并纳入 D3，后续实现仍须经
`@terrenia/blocks` manifest 明确注册；本文的清单不会让 host 获得 Terrenia 私有内容表。

## 72 个方块定义

所有名称都是完整 ID `terrenia:block/<path>` 的 `<path>` 部分。分组用于创作与审查，
不从 ID 路径推断材料、掉落、工具或机制语义。

### 空间基础：1

| 阶段 | ID | 用途 |
| --- | --- | --- |
| D3 | `air` | 空体素与默认 empty Role |

### 地表与土壤：14

| 阶段 | ID |
| --- | --- |
| D3 | `grass`、`dirt` |
| D4 | `clay`、`sand`、`red-sand`、`gravel` |
| 自然基线 | `coarse-dirt`、`rooted-dirt`、`mud`、`silt`、`peat`、`snow`、`ice`、`moss` |

这些方块为温带地表、旱地区域、河床、湿地与寒冷表面提供最小材料差异。`grass` 是现有
surface Role 的草土方块；非实心植被使用后文的 `tall-grass`。

### 岩石与地质：14

| 阶段 | ID |
| --- | --- |
| D3 | `stone`、`obsidian` |
| D4 | `limestone`、`basalt`、`sandstone`、`red-sandstone` |
| 自然基线 | `deepstone`、`granite`、`slate`、`tuff`、`calcite`、`dripstone` |
| 内容基线 | `cobblestone`、`marble` |

岩石清单用于验证连续岩层、侵入体、可侵蚀性、洞壁替换和建筑来源。`obsidian` 的一般材料
Tag 不自动授予任何 portal-frame affordance；机制资格必须由对应 mechanic contract 明确贡献。

### 矿物与储存块：12

| 阶段 | ID |
| --- | --- |
| D3 | `copper-block` |
| D4 | `copper-ore` |
| 自然基线 | `coal-ore`、`iron-ore`、`tin-ore`、`gold-ore`、`sulfur-ore`、`crystal-cluster` |
| 内容基线 | `coal-block`、`iron-block`、`tin-block`、`gold-block` |

矿物分布同时读取地质、群系与玩法条件，不由任一层最后写入覆盖。储存块与矿石通过明确的
Tag／Map／Role 和 recipe 关系连接，不依赖 `*-ore` 或 `*-block` 名称后缀。

### 植被：15

| 阶段 | ID |
| --- | --- |
| D4 | `oak-log`、`oak-leaves`、`tall-grass` |
| 自然基线 | `pine-log`、`pine-leaves` |
| 内容基线 | `stripped-oak-log`、`stripped-pine-log`、`fern`、`reed`、`cactus`、`vine`、`red-mushroom`、`brown-mushroom`、`glow-moss`、`wildflowers` |

两套树木足以验证轴向 state、植被点程序、不同群系冠层和建材来源。蘑菇、荧光苔藓及藤蔓
提供地下／附着表面 placement 的真实 consumer。

### 建筑材料：12

全部进入内容基线：

```text
oak-planks
pine-planks
stone-bricks
mossy-stone-bricks
bricks
polished-stone
polished-granite
polished-slate
basalt-tiles
glass
copper-grating
roof-tiles
```

初期每种材料只注册一个内容 ID。若 renderer、collision 与 placement prototype 证明统一状态
模型可行，`cube`、`slab`、`stair`、`wall` 等有限形态由 versioned `form` state 表达；如果
后续真实 consumer 证明形态需要独立物品身份、掉落或 package 兼容边界，再以单独提案拆成
StableId，不提前生成材料 × 形态的笛卡尔积。

### 功能方块：4

全部进入内容基线：

```text
torch
workbench
furnace
chest
```

`torch` 用于 `lit`／朝向和发光表现；`furnace` 用于 active／lit 状态与配方执行；`chest`
用于 block entity schema 和容器持久化。功能逻辑属于 gameplay／mechanic package，
`@terrenia/blocks` 只拥有内容定义与相邻资料，不把通用容器或配方系统私有化为 Terrenia API。

## 流体不计入方块总数

第一内容基线另注册：

```text
terrenia:fluid/water
terrenia:fluid/lava
```

流体使用独立 registry kind 和有限 `level`／`flow` state；静水、流水及不同液位不注册为
不同 block ID。voxel palette 如何同时编码 solid occupancy 与 fluid state 仍由首个流体
consumer 和 persistence prototype 决定，本文只冻结内容身份与“不以方块变体膨胀”的边界。

## Authoring 与语义要求

每个方块定义至少应能声明并由 schema 验证：

- 完整 StableId、内容 revision、拥有 package 与 presentation asset 引用；
- collision／occlusion／replaceable 等体素行为；
- hardness、摩擦、支撑或其他经真实 consumer 证明需要的 typed physical data；
- mining input、掉落、工具需求、recipe 与 placement item；
- definition-level SemanticTag／Map contribution 和需要的 ContentRole candidate；
- 可用的 versioned BlockState schema，例如 `axis`、`facing`、`lit`、`growth`、`half` 或 `form`；
- worldgen placement predicate、Role receipt 与生成 provenance。

具体字段与 `physical@1` Map 的归属仍由 block／item／biome 最小 schema prototype 决定；
本清单不以内容数量提前冻结平台通用 ontology。平台 contract 也不得把这 72 个 Terrenia
concrete ID 写成 host 默认值。

## Package 责任

- `@terrenia/blocks` 注册 72 个 block definitions、2 个 fluid definitions、相邻 item／drop 资料及 Terrenia 自有语义贡献。
- `@terrenia/worldgen` 通过 Predicate／Role 选择内容，不硬编码 runtime numeric block ID。
- `@terrenia/gameplay` 提供采掘、配方、容器、熔炼等 Terrenia 专属规则；可跨维度复用的机制应拆成独立 package。
- `@terrenia/presentation` 提供纹理、材质、声音与可选表现；headless 省略它时不得改变权威注册或 world hash。
- root `terrenia` 只聚合 closure、授予 namespace authority 并绑定维度级 fallback Role，不复制方块清单。

## 验收

- D3 的 6 个与 D4 的 18 个方块各自形成确定性、与注册顺序无关的 RegistrationImage。
- 两个 D4 terrain／biome styles 在没有调试 overlay 时也能由表面、浅层和岩石材料辨认。
- 已物化 chunk 保存完整 StableId 与 state；内容排序、static／dynamic realization 或重启不改变结果。
- 一般 obsidian Tag 不自动获得 portal 行为；copper input Tag 可以接受第三方铜而 Role output 仍解析为 frozen concrete ID。
- 72 个定义不包含朝向、液位、成长阶段、台阶或楼梯的 ID 复制品。
- 移除 Terrenia closure 后，host 不建立这些方块、流体或隐式 fallback。

## 相关文件

- [Demo workspace 与 Terrenia package 组织](../architecture/demo-workspace-organization.md)
- [语义注册、内容判定与选择](../architecture/semantic-registration.md)
- [可组合世界生成](../architecture/world-generation.md)
- [第一个可玩 demo 路线图](roadmap-first-demo.md)
- [待决问题](open-questions.md)
