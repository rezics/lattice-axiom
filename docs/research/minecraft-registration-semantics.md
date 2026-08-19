---
title: Minecraft 註冊、Tag 與語義相容性的設計教訓
status: exploration
type: research
updated: 2026-08-19
---

# Minecraft 註冊、Tag 與語義相容性的設計教訓

## 調查範圍

本页调查 Minecraft Java 及当前 Fabric／NeoForge／Forge 生态如何区分精确 registry
identity、集合、typed data、block state、行为与条件资源。目的不是复制 loader API，
而是为 Lattice Axiom 的 semantic registration contract 找出成熟做法与长期缺口。

## Registry key 只解決精確身分

Minecraft 的 block、item、fluid、entity type 与许多 data-driven对象进入不同 registry，
每个对象取得 namespaced key。这个 key 适合存档、命令、network／datapack 引用与精确查找，
但 `example:copper_block` 的字符串本身不会证明它是铜、储存块或任何结构材料。

这与 Lattice Axiom 的 `StableId` 目标一致：identity 必须稳定且精确，但不能兼任开放世界
中的全部语义分类。

## Tag 是 typed、可增量合併的集合

[NeoForge Tag 文档](https://docs.neoforged.net/docs/resources/server/tags/)把 Tag 定义为
同一 registry 内注册对象的集合，可用于快速 membership check。Tag 文件默认增量合并，
而不是像多数同 ID data file 一样互相替换；Tag 也可以包含另一个 Tag、引用 optional entry，
循环依赖则使 datapack 无法载入。

NeoForge 与 Fabric 共用 `c` namespace 的 convention tags，例如 `c:ingots/gold`；当前
NeoForge common tag 源码实际定义了 `c:ingots/copper`、`c:ores/copper` 与
`c:storage_blocks/copper` 等材料／形态集合。这个模型带来两个重要性质：

1. `foo:copper_ingot` 与 `bar:copper_ingot` 仍是两个精确对象；
2. recipe／consumer 可以接受共同 Tag，而不必知道所有 provider ID。

文档建议会修改某个模组自身行为的 Tag 放在该模组 namespace；真正希望跨生态共用的
材料／宝石集合才使用 convention namespace。`replace=true` 主要留给 pack author，普通
模组应保持 additive。这证明 Tag contract 的 owner 与 membership contributor 必须分开。

## Data Map 補足 boolean Tag

[NeoForge Data Maps](https://docs.neoforged.net/docs/1.21.1/resources/server/datamaps/)
明确把 Tag 类比为 `registry object → boolean`，把 Data Map 类比为
`registry object → object`。Data Map value 由 codec 定义，可以同步、reload，并为冲突
定义 merger；entry 可以按精确对象或 Tag 批量附加。

内建例子包括 compost chance、waxed block target 等。这些不是单纯「属于哪一组」：它们
需要数值、结构或指向另一个 registry object 的关系。Lattice Axiom 因此需要 typed
`SemanticMap<T>`，不能把所有性质编码成更多 Tag 或无型别 JSON。

## Block State 是實例狀態，不是內容本體論

[Fabric Block States](https://docs.fabricmc.net/develop/blocks/blockstates)说明 block state
附着在世界中的单一方块上，保存 rotation、activated、age 等有限 property。NeoForge 的
blockstate 文档也强调有限状态适合 blockstate，接近无限的资料应使用 block entity。

因此：

- `axis=y`、`lit=true` 属于已放置实例；
- 「属于铜储存块」属于定义级 semantic tag；
- hardness／waxed target 属于 definition-level typed map；
- 「在这个 world position 能否作框架」可能是上下文 affordance。

把这四类资料都叫 property 会导致 persistence、merge 与查询边界混乱。

## 類別不等於行為

当前 NeoForge common tag 源码对多个 tool tags 明确注明：这些 Tag 用于分类，不应决定
工具能执行什么动作；实际行为应查询 `ItemAbility`。这是很有价值的反例：如果 consumer
从「像某种工具／材料」自动推出行为，新内容很容易得到作者没有承诺的能力。

传送门框架更能说明问题。Forge 的 `IForgeBlock#isPortalFrame` 提供上下文 hook，但默认
实现仍检查原版 `Blocks.OBSIDIAN`。这比完全硬编码可扩展，却依然依赖每种机制新增专用
method。Lattice Axiom 应从一开始让简单行为使用机制 owner 的 Tag／Map，让复杂行为通过
versioned Affordance，而不是从广义 `obsidians` Tag 自动推断。

来源：

- [NeoForge common tags 对分类与能力的区分](https://github.com/neoforged/NeoForge/blob/26.2.x/src/main/java/net/neoforged/neoforge/common/Tags.java)
- [Forge `IForgeBlock#isPortalFrame`](https://github.com/MinecraftForge/MinecraftForge/blob/26.2/src/main/java/net/minecraftforge/common/extensions/IForgeBlock.java)

## 條件資源有用，但不應成為任意晚期註冊

[Fabric Resource Conditions](https://docs.fabricmc.net/develop/resource-conditions)允许 recipe、
advancement、loot table、predicate 等资料根据 mod presence、registry entry 或 populated Tag
声明式载入。它证明 integration 资源确实需要条件，但也显示条件应在可检查的 resource
composition 阶段求值。

若 block registration 本身在 module initializer 内读取当前 registry 再决定是否执行，
结果会依赖先后顺序；新增模组还可能让旧世界 required ID 消失。Lattice Axiom 需要的是
pre-activation、declarative、locked fallback bundle，而不是推广 late imperative query。

## 可吸收與不複製的部分

| 机制 | 吸收 | 不直接复制 |
| --- | --- | --- |
| Registry key | 精确、namespaced、typed identity | 从路径猜材料／行为 |
| Tag | typed set、nested set、optional ref、additive contribution | 任意 package replace／remove |
| Convention tag | 跨 package 的材料／形态 vocabulary | 无治理的公共 namespace 抢注 |
| Data Map | typed value、relation、merger、按 Tag 批量附加 | last-writer-wins default |
| Block state | 有限 instance state | 把定义语义复制进每个 voxel |
| Ability／hook | 行为与分类分离 | 每个新机制不断扩大 monolithic block interface |
| Resource condition | 声明式 integration | module init 时观察 registry 顺序 |

## 對 Lattice Axiom 的直接要求

- `StableId`、Tag membership、typed Map、state 与 Affordance 是不同 schema。
- shared Tag 的定义权与添加成员的权限分离，并保存 contribution provenance。
- input／matching 使用 set／predicate；output／generation 使用 locked concrete binding。
- semantic catalogs、bindings 与 conditional bundle activation 进入 `RegistrationImage` hash。
- content package 可以全部被替换；平台 contract 不得暗中依赖 Terrenia 或任何第一方内容。
- runtime 将 Tag 编译为 bitset、Map 编译为 indexed table、Predicate 编译为 plan；不逐次扫描
  package metadata 或执行 Nickel。

## 相關文件

- [決策 0020：分離註冊身分、語義與選擇](../decisions/0020-semantic-registration-and-content-selection.md)
- [語義註冊架構](../architecture/semantic-registration.md)
- [模組與註冊清單](../architecture/module-composition.md)
- [Minecraft 世界生成教訓](minecraft-world-generation-lessons.md)

