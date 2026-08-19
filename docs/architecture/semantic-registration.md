---
title: 語義註冊、內容判定與候選選擇
status: proposed
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0010-nickel-driven-package-system.md
  - ../decisions/0019-separate-package-and-registration-identities.md
  - ../decisions/0020-semantic-registration-and-content-selection.md
---

# 語義註冊、內容判定與候選選擇

## 結論

Lattice Axiom 保留 `<namespace>:<kind>/<path>` 作为精确 identity，在其上编译一个
closure-wide `SemanticCatalog`。Catalog 不是第二个 registry，而是同一
`RegistrationImage` 中由 Tag、typed Map、StateProperty、Affordance、Predicate、Role
与 active ContentBundle 构成的语义索引。

Nickel 是 authoring 与组合语言：`latticeaxiom.lib` 用 contracts、functions、recursive
records、enum variants、merge priorities 与 overlay 生成规范化 `SemanticIntent`。Rust
package kernel 负责只有看见完整 graph 才能完成的验证、解析、lock 与 numeric compilation。
Nickel function 不进入 runtime，Rust builder 也不能成为绕过 Nickel 的第二组合入口。

`terrenia` 只是当前 demo 选择的普通 root package。它可以聚合 `@terrenia/*`、注册
`terrenia:*` 内容与机制 semantic contract，但平台 semantic model 不引用它、不给它
特殊默认，也必须能用另一个测试维度 closure 完整替换。

## 核心不變條件

1. identity 不推导 semantics；semantics 不合并 identity。
2. 相似内容可以共存；duplicate exact ID 仍然失败。
3. 所有 additive contribution、override、binding 与 fallback 都保留 package／source provenance。
4. 同一输入 source universe、profile 与 lock 必须得到相同 semantic image，不依赖发现或加载顺序。
5. authoritative output 在进入 world command 前已经解析成 concrete stable／numeric ID。
6. 已物化 world 不因 Tag、Role 或新 package 出现而隐式改写。
7. ordinary package 不能删除其他 package 的语义贡献或强制全局 binding。
8. static／dynamic realization 产生相同 semantic manifest；实现路径不能改变 Tag、Map、Role 或 bundle。
9. Nickel 只在 compose 阶段执行；runtime 只消费已验证的 compiled catalog。
10. Terrenia、测试维度与第三方内容使用完全相同的 contract、grant 与 diagnostics。

## 型別與 ID

所有公开 semantic contract 都复用 `StableId` parser；`kind` 同时约束 target registry：

| logical type | kind 示例 | ID 示例 |
| --- | --- | --- |
| block tag | `block-tag` | `latticeaxiom:block-tag/storage-blocks/copper@1` |
| item tag | `item-tag` | `latticeaxiom:item-tag/ingots/copper@1` |
| block map | `block-map` | `latticeaxiom:block-map/physical@1` |
| block state key | `block-state` | `latticeaxiom:block-state/axis@1` |
| block affordance | `block-affordance` | `terrenia:block-affordance/nether-portal-frame@1` |
| block role | `block-role` | `latticeaxiom:block-role/storage-block/copper@1` |
| content bundle | `content-bundle` | `example:content-bundle/copper-fallback@1` |

`@major` 版本化的是 contract meaning／value schema／operation signature，不是当前成员集合。
往 extensible Tag 增加合法成员通常不创建新 Tag major，但仍会改变当前 registration image hash；
改变「属于该 Tag 代表什么」则必须创建新 major。

## `SemanticTag`

### 定義

```text
SemanticTagDefinition
├── id: TagId
├── target_kind: RegistryKind
├── extensibility: Closed | OwnerAndSelfContribution | ProfileOverlay
├── authority: authoritative | presentation
├── missing_policy: error | empty
└── documentation / contract revision
```

Tag contribution 是独立 row：

```text
TagContribution
├── tag: TagId
├── add
│   ├── Exact(StableId)
│   ├── RequiredTag(TagId)
│   └── OptionalTag(TagId)
├── declared_by: PackageName
└── source span / fragment hash
```

规则：

- exact target 与 Tag target kind 必须相同；block Tag 不能包含 item。
- nested Tag 形成 DAG；cycle 在 activation 前失败，并列出完整 edge provenance。
- ordinary package 默认只能添加由自己注册／schema-own 的 exact entry。
- integration package 修改 foreign entry 时必须显式依赖相关 package，并把行为记录为 integration provenance。
- ordinary package manifest 不支持 `remove`／`replace`；profile overlay 可以，但结果进入 lock 和 hash。
- Tag definition 的 namespace grant 与 contribution permission 分开验证。

### Material tag 與 mechanic tag

```text
latticeaxiom:block-tag/obsidians/normal@1
terrenia:block-tag/nether-portal-frames@1
```

前者表示平台约定的材料／配方集合；后者由 Terrenia 的 portal mechanic owner 定义，表示
明确参与该机制。加入前者不会自动加入后者。另一个 portal package 应定义自己的 Tag／
Affordance，而不是依赖 `terrenia:*`。

## `SemanticMap<T>`

Tag 只提供 boolean membership。带值属性、关系或参数使用 typed Map：

```text
SemanticMapDefinition<T>
├── id: MapId
├── target_kind
├── value_schema + schema owner
├── cardinality: zero-or-one | one
├── merger: conflict | owner-defined associative merger
├── authority
└── sync / reload policy

SemanticMapContribution<T>
├── map
├── target: Exact(StableId) | MembersOf(TagId)
├── value: T
├── mode: contribute | profile_replace
└── provenance
```

候选实例：

```text
latticeaxiom:block-map/physical@1
  target → { hardness, blast_resistance, friction }

latticeaxiom:block-map/waxable@1
  target → { waxed_into: BlockStableId }

terrenia:block-map/nether-portal-frame@1
  target → { portal_kind, allowed_state_predicate }
```

Map owner 必须定义 value contract 与 conflict policy。默认 conflict，不提供隐式
last-writer-wins。只有 merger 可证明 deterministic、associative，且顺序不改变结果时才允许
多 contribution；profile replacement 永远显式记录。

按 Tag 附值发生在 Tag DAG 展开之后。若 exact 与 Tag-derived value 同时命中而 Map 没有
声明 merger／specificity policy，必须失败，不能假设 exact 自动获胜。

## `StatePropertyKey`

StateProperty 描述一个已放置 block instance 的有限、可枚举状态：

```text
BlockStateSchema
├── supported keys
│   ├── latticeaxiom:block-state/axis@1: enum(x, y, z)
│   ├── latticeaxiom:block-state/lit@1: bool
│   └── example:block-state/charge@1: bounded integer
├── default values
└── canonical encoding / schema revision
```

- 跨 block／package 查询的 property key 必须是 versioned stable contract。
- package-private state 可以 definition-scoped，但外部 Predicate 不得通过名称猜测它。
- 状态组合必须有限且有 compact encoding；大量或接近无界资料使用 persistent component／
  block entity schema。
- Tag／Map 作用于 block definition；`StateEq`／`StateIn` 作用于当前 block state。

## `Affordance`

Affordance 表达「在给定 context 中是否／如何完成一项动作」，不是普通分类：

```text
AffordanceDefinition
├── id
├── target_kind
├── request / response schema
├── purity / read set / thread policy
├── static fast-path representation
└── optional callback interface
```

实现可以是：

1. static declaration：Tag／Map 与 state predicate 足以回答，编译成纯资料 fast path；
2. static typed handler：`NativeStatic` 直接系统／函数；
3. dynamic handler：manifest callback key + versioned ABI table，按查询批次调用。

只有真实 consumer 证明资料谓词不足时才新增 callback affordance。portal frame 若只需要集合
成员和 state 条件，应保持 static；若某个方块是否稳定依赖邻居、能量或 dimension context，
才进入 context handler。

## `ContentPredicate`

Predicate 是可序列化、带 target kind 的封闭 AST；Nickel function 用于构造 AST，本身不
进入 `CompositionSpec`：

```text
ContentPredicate<T>
  = Exact(StableId<T>)
  | InTag(TagId<T>)
  | MapPresent(MapId<T, _>)
  | MapMatches(MapId<T, V>, TypedCondition<V>)
  | StateEq(StatePropertyKey<V>, V)             // block state only
  | Supports(AffordanceId<T>, Parameters)
  | All(Vec<Predicate<T>>)
  | Any(Vec<Predicate<T>>)
  | Not(Box<Predicate<T>>)
```

验证：

- 所有 child target kind 一致；
- referenced Tag／Map／State／Affordance major 存在；
- typed condition 与 value schema 相容；
- static context 禁止引用只在 runtime 才存在的 query；
- worldgen deterministic stage 禁止调用未声明 nondeterministic／world-mutable affordance。

结构匹配已存在世界使用 Predicate；生成结构或配方输出不能从多成员 Predicate 任意取第一项，
而必须使用 Role。

## `ContentRole` 與 binding

Role 是需要 concrete target 的选择点：

```text
ContentRoleDefinition
├── id
├── target_kind
├── accepts: ContentPredicate
├── cardinality: ExactlyOne | ZeroOrOne | OneOrMore
├── authority
└── owner / documentation

RoleOffer
├── role
├── target
├── class: normal | fallback
└── provenance

RoleBinding
├── role
├── selected target(s)
├── selected_by: unique | package-default | profile | frozen-world
└── resolution explanation
```

解析政策：

1. frozen world binding 若仍满足 contract，优先保持；不满足则进入 migration／拒绝流程。
2. composition profile 的显式 binding 优先于 default suggestion。
3. 没有显式 binding且只有一个 normal candidate 时自动选择。
4. package 可以用 Nickel `default` 提出建议；多个同优先级不同建议直接形成 merge conflict。
5. 多个合法候选且无明确选择时失败，stable ID 排序只用于诊断，不静默决定语义。
6. 仍未满足时才进入 fallback bundle 阶段。

Role 不是 capability。Capability 选择实现某个服务／机制的 package provider；Role 从已经
active 的 content entries 中选择一个／多个 concrete registry object。

## `ContentBundle` 與 fallback

默认策略是让相似内容全部注册，由 Tag 兼容、Role 选择。只有内容确实是替代缺省时才使用：

```text
ContentBundle
├── id
├── registrations
├── semantic contributions
├── role offers
├── activation
│   ├── Always
│   └── FallbackForMissingRoles([RoleId])
└── internal references / artifact fragment
```

v1 固定算法：

1. 合并所有 `Always` registrations／semantic contributions。
2. 展开 Tag、Map，验证 normal role candidates，并解析非 fallback bindings。
3. 对 missing roles 收集 graph 中已经存在的 fallback bundles。
4. profile 显式选择或只有一个完整满足集合的 bundle时，原子激活该 bundle；歧义失败。
5. 重新构建 Tag／Map／Role 一次，验证 bundle 声称提供的 target 实际满足 predicate。
6. 若仍缺失、引入新 fallback guard 或形成 fallback 间循环，则失败；v1 不做任意负条件 fixed point。
7. active bundle set、role binding 与 explanation 写入 lock／RegistrationImage。

fallback bundle 只控制同一个已选 package 的候选 registration。若缺少的是 package provider，
应拆出独立 fallback package并使用 dependency／capability resolver；semantic stage 不隐式更改
`LockedGameGraph`。

既有 world 重开时使用 frozen active bundle set。安装新的铜 provider 不会让旧 fallback
copper ID 消失；使用新组合建立 world 时才可得到不同 activation。

## Nickel Authoring Contract

### 為何不是 Rust builder 清單

Nickel 已经负责 package、profile 与 overlay。如果 semantic authoring 只存在于 Rust macro／
builder，data package 与整合包将无法使用同一函数、默认与来源诊断。规范因此要求：

- pure-data registration／semantic intent 直接由 `package.ncl` 产生；
- code-bound system／schema／affordance callback 由 SDK 产生 manifest fragment；
- 两者进入同一 canonical `RegistrationManifest` schema，不允许同一 row 有两份不同真相；
- generated fragment 可由 `latticeaxiom.lib` 的 typed constructor 引用／组合，artifact hash 必须匹配。

### 標準庫表面

概念 API（名称仍由 R0 prototype fixture 冻结）：

```nickel
la.Registration.concat : Array RegistrationFragment -> Registration

la.Tag.define          : TagDefinitionInput -> RegistrationFragment
la.Tag.add             : TagContributionInput -> RegistrationFragment
la.Map.define          : MapDefinitionInput -> RegistrationFragment
la.Map.put             : MapContributionInput -> RegistrationFragment
la.State.define        : StatePropertyInput -> RegistrationFragment
la.Affordance.define   : AffordanceInput -> RegistrationFragment
la.Role.define         : RoleInput -> RegistrationFragment
la.Role.offer          : RoleOfferInput -> RegistrationFragment
la.Bundle.fallback     : BundleInput -> RegistrationFragment

la.Predicate.exact
la.Predicate.in_tag
la.Predicate.map_matches
la.Predicate.state_eq
la.Predicate.supports
la.Predicate.all
la.Predicate.any
la.Predicate.not
```

这些 function 返回 enum variant／record 组成的纯资料 AST。`CompositionSpec` 的 Rust enum
与 Nickel output fixture 一一对应。

### 使用 function 與 recursive record 生成材料族

作者不应为每种材料重复写 item／block tags 与 roles。标准库可以提供高阶 helper：

```nickel
let la = import "latticeaxiom/lib.ncl" in

let copper = la.Material.family {
  material = "copper",
  forms = {
    ingot = "terrenia:item/copper-ingot",
    storage-block = "terrenia:block/copper-block",
  },
}
in

la.Package & {
  package = { name = "@terrenia/blocks", version = "0.1.0" },
  registration = la.Registration.concat [
    la.Block.define {
      id = "terrenia:block/copper-block",
      state = {},
    },
    copper,
  ],
}
```

`la.Material.family` 在 Nickel 求值时展开为规范 Tag contributions／Role offers；Rust 不认识
名为 copper 的特殊语法。另一个 package 可复用同一 function 得到相同 contract shapes。
helper version／output schema 进入 `latticeaxiom.lib` fingerprint。

### Predicate helper

```nickel
let frame = la.Predicate.all [
  la.Predicate.in_tag "terrenia:block-tag/nether-portal-frames@1",
  la.Predicate.not (
    la.Predicate.state_eq {
      property = "example:block-state/unstable@1",
      value = true,
    }
  ),
]
in
{
  frame-predicate = frame,
  placement-role = "terrenia:block-role/nether-portal-frame@1",
}
```

这是构造资料，不允许把任意 Nickel closure 作为 runtime predicate 序列化。

### Merge、default 與 overlay

Nickel 的 `&` 对 record 做 recursive merge，但 array 不会自动 concatenate。因此：

- additive registration fragments 必须经 `la.Registration.concat`／领域 `add` function；
- keyed role bindings 使用 record merge；
- package 提议使用 `default`；game／profile 明确值覆盖 default；
- 相同优先级的不同 binding 让 Nickel 报 non-mergeable terms，并保留双方 source span；
- `force` 只允许 trusted composition policy。package source 即使能写语法，也不能取得
  `policy_overrides` layer 的 authority；Rust 在 typed conversion 时验证 layer provenance。

概念 overlay：

```nickel
let base = {
  semantic.bindings."latticeaxiom:block-role/storage-block/copper@1"
    | default = "terrenia:block/copper-block",
}
in
let pack = {
  semantic.bindings."latticeaxiom:block-role/storage-block/copper@1"
    = "example:block/copper-block",
}
in
base & pack
```

### Contracts 與 Rust 驗證邊界

Nickel contracts／types负责：

- required／optional field、enum alternatives、local value ranges；
- Stable ID 字符串的局部格式 contract；
- builder input 与 normalized output shape；
- source-aware default／overlay merge error。

Rust负责：

- Stable ID parser 的 normative validation；
- package closure、namespace grant 与 foreign amendment authority；
- 所有 referenced ID 是否存在及 target kind 是否相同；
- Tag DAG、Map merger、Role cardinality、fallback stage 与 world compatibility；
- numeric layout、canonical ordering、hash 与 runtime budget。

两侧由同一 positive／negative golden corpus绑定；不能让 Nickel contract 和 Rust model 各自
演进成不同语言。

## Manifest 與 CompositionSpec

```text
CompositionSpec
└── semantic intent
    ├── profile role bindings / overlay policy
    ├── conditional bundle intent
    └── normalized Nickel fragments

RegistrationManifest (per package realization)
├── exact registrations / schemas / systems
├── tag definitions / contributions
├── map definitions / contributions
├── state property definitions
├── affordance definitions / callback keys
├── predicates / roles / offers
├── content bundles
└── producer / source spans / canonical hash
```

Pure-data package 的 manifest 可以完全来自 Nickel output。code package 的 SDK fragment 与
Nickel fragment 按 stable row key 合并；重复 row 必须 byte-for-byte canonical equal，否则
报告 two-sources-of-truth。static 与 dynamic realization 必须发布相同 semantic rows。

## RegistrationImage 編譯管線

```text
evaluated Nickel CompositionSpec + locked package manifests
                          ↓
exact ID / namespace / schema validation
                          ↓
unconditional ContentBundles
                          ↓
Tag DAG expansion → typed membership bitsets
                          ↓
SemanticMap target expansion / merge
                          ↓
StateProperty + Affordance signatures
                          ↓
Predicate type-check / deterministic-stage validation
                          ↓
Role candidates / explicit bindings
                          ↓
single fallback activation wave
                          ↓
revalidate semantic catalog and bindings
                          ↓
canonical numeric tables + provenance + hash
                          ↓
RegistrationImage
```

Canonical ordering 只影响 numeric layout 与 hash，不解决语义歧义。所有 diagnostics 必须列出
stable semantic ID、target、declaring／contributing package、Nickel source span、profile
override 与 dependency chain。

## Runtime Representation

| contract | runtime representation |
| --- | --- |
| exact ID | stable ↔ process-local numeric table |
| Tag | target-kind-sized bitset／sorted numeric set |
| Map | dense vector 或 sparse numeric-key table |
| StateProperty | block palette 内 compact state index + schema table |
| static Predicate | validated bytecode／decision DAG／specialized Rust matcher |
| Role | resolved numeric target(s)，O(1) lookup |
| Affordance | static plan、typed direct function 或 batched ABI callback |
| Bundle | activation metadata；tick 不查询 guard |

authoritative 与 presentation catalogs 分开 fingerprint。presentation-only Tag／Map 可在安全
client reload 中重建；任何会改变 collision、结构、loot、recipe、worldgen 或 gameplay 的
semantic change 必须建立新 authoritative image，不能在开放写入的 world 中偷换。

## 銅相容範例

三个 package 注册不同 ID：

```text
terrenia:block/copper-block
example:block/copper-block
other:block/engraved-copper-block
```

前两个贡献：

```text
latticeaxiom:block-tag/storage-blocks/copper@1
```

雕刻铜若不满足「可无损还原为标准材料单位」的 contract，则不应因为名字含 copper 就加入。

- recipe input 使用 Tag，可接受两个 storage blocks；
- recipe output 使用 `latticeaxiom:block-role/storage-block/copper@1`；
- profile 有明确 binding 时生成该 target；
- 无 binding且多候选时 compose 失败并要求选择；
- fallback copper bundle 只有在 normal role 未满足时才可能激活；
- world 中两种 ID 永远保持可区分。

## 結構與傳送門範例

结构 pattern 分离「匹配」与「放置」：

```text
frame matcher  = InTag(terrenia:block-tag/nether-portal-frames@1)
frame output   = Role(terrenia:block-role/nether-portal-frame@1)
interior       = All(InTag(...replaceable...), Not(StateEq(...occupied...)))
```

第三方 `example:block/obsidian` 可以加入平台的 normal obsidian Tag用于 recipe；这不会让它
自动成为 Terrenia portal frame。它可以由自身或明确 integration package贡献到 Terrenia
mechanic Tag；如果是否可用取决于 runtime context，则提供对应 Affordance。

另一个维度 package 可以定义 `example:*` portal semantics，完全不依赖 Terrenia。host 只
执行已经进入 `RegistrationImage` 的通用 Predicate／Role／Affordance contract。

## 持久化與版本

world metadata 至少保存：

- exact locked package／artifact closure；
- authoritative semantic image hash；
- active content bundle set；
- authoritative role bindings及其 contract major；
- worldgen artifact 实际使用的 predicate／role binding／map revision receipt；
- schema owner 与 migration state。

chunk palette、inventory 与 entity component 保存 concrete stable IDs。打开 world 时：

1. 先恢复 frozen package／bundle／binding intent；
2. 验证当前 manifests 能重建 compatible semantic image；
3. exact content missing 进入 placeholder／read-only recovery／migration policy；
4. Role 新增候选不会重写旧 concrete values；
5. authoritative Tag／Map meaning 不相容时要求 contract major／migration，不以 hash 不同一概拒绝。

变更分类：

| 变更 | contract／lock 后果 | world 后果 |
| --- | --- | --- |
| extensible Tag 增加成员 | image hash 变 | 旧 concrete data不变；新行为需新 session／epoch |
| Tag meaning 破坏性改变 | 新 Tag major／owner SemVer | adapter／migration／拒绝 |
| Map value调整 | contribution owner version + image hash | 依 authority决定新 session或presentation reload |
| Map schema改变 | Map major + schema migration | migration／拒绝 |
| Role rebind | lock／binding receipt变 | 已物化值不重写；新 output／generation使用新 target |
| fallback activation改变 | active bundle set变 | 仅新 world／显式 migration；旧 lock保持 |
| Affordance signature改变 | affordance major／ABI interface影响 | consumer与provider重新解析 |

## 權限與治理

- `latticeaxiom:*` semantic contracts 由平台版本化；平台 package不得用它们偷渡 Terrenia
  concrete ID 作为 host 默认。
- domain mechanic contract 由 consumer domain 拥有，例如 `terrenia:*` portal semantics。
- package 可以为自己对象贡献公开 extensible contract。
- integration package 修改双方语义时声明双方 dependency 与 supported range。
- profile overlay 是玩家／整合包政策，允许 remove／replace／binding，但必须进入 lock、诊断
  和 multiplayer authoritative closure协商。
- publisher trust 不等于 semantic correctness；所有 native／data package 都经过同一 validation。

## 失敗診斷

最低 diagnostic codes：

```text
semantic.unknown_contract
semantic.target_kind_mismatch
semantic.unauthorized_contribution
semantic.foreign_amendment_missing_dependency
semantic.tag_cycle
semantic.map_conflict
semantic.map_merger_invalid
semantic.predicate_type_mismatch
semantic.affordance_unavailable
semantic.role_unsatisfied
semantic.role_ambiguous
semantic.binding_rejected
semantic.fallback_ambiguous
semantic.fallback_cycle
semantic.frozen_world_mismatch
```

每个错误包含 resolution explanation，而非只给 ID。例如 role ambiguity 要列出所有候选、
各自满足哪个 predicate、谁贡献了 membership，以及可以在哪个 Nickel profile field绑定。

## 首階段實作切片

### S0：Model 與 Nickel contracts

- Rust newtypes／enums：Tag、Map、StateKey、Predicate、Role、Bundle；
- `latticeaxiom.lib` constructors、contracts、concat 与 binding overlay；
- Nickel CLI／embedded → typed `CompositionSpec` golden corpus；
- 明确禁止 plain merge 拼接 additive arrays。

### S1：Tag、Predicate 與 Role

- typed Tag DAG、bitset compilation；
- `Exact`／`InTag`／`All`／`Any`／`Not` predicate；
- ExactlyOne／ZeroOrOne Role 与 profile binding；
- copper／obsidian／portal fixtures。

### S2：Typed Map 與 shared state key

- 一个 numeric physical Map；
- 一个 relation Map（例如 waxed target）；
- bool／enum shared StatePropertyKey；
- `MapMatches`／`StateEq` predicate compilation。

### S3：Fallback bundle 與 persistence

- fixed two-stage activation；
- active bundle／binding lock schema；
- new-world vs frozen-world fixtures；
- missing content／migration diagnostics。

### S4：Affordance escape hatch

- 只有 static predicate 无法表达的第二个真实 consumer出现后实现；
- static direct 与 portable dynamic batch adapter；
- determinism／read set／budget validation。

## 驗收矩陣

- randomized package／fragment／load order不改变 catalog hash。
- Tag nested／optional／cycle／wrong-kind fixture完整。
- ordinary package 无法 remove／replace foreign contribution。
- Map conflict与合法 associative merger结果稳定。
- Role unique／explicit／ambiguous／frozen binding均有测试。
- 两个 fallback竞争时失败；新世界选择与旧世界 frozen activation行为不同且可解释。
- structure matcher接受明确兼容的第三方 frame，不接受只有 broad material tag 的对象。
- role output永远在 world command前成为 concrete ID。
- static／dynamic、client／headless、CLI／embedded semantic hash一致。
- 以非 Terrenia测试维度替换整个 `terrenia` closure，无 host special case。
- runtime profile证明 Tag／Map／Role hot queries无 Nickel／string／package lookup。

## 不在首階段建立

- 通用 OWL／RDF-like ontology 或自动语义推理；
- 按字符串相似度自动归类；
- 任意 runtime Nickel predicate；
- 多轮负条件 fixed-point／SAT content activation；
- package 自动 alias／deduplicate exact content ID；
- gameplay-open world 的 authoritative semantic hot reload；
- Terrenia 专属 host registry 或默认表。

## 相關文件

- [決策 0020](../decisions/0020-semantic-registration-and-content-selection.md)
- [套件內核](package-management.md)
- [模組與註冊清單](module-composition.md)
- [版本與相容性](versioning-and-compatibility.md)
- [世界持久化](world-persistence.md)
- [世界生成](world-generation.md)
- [資產語義](asset-semantics.md)
- [Minecraft 語義相容性調查](../research/minecraft-registration-semantics.md)
