---
title: 分離精確註冊身分、內容語義與候選選擇
status: accepted
type: decision
updated: 2026-08-19
---

# 決策 0020：分離精確註冊身分、內容語義與候選選擇

## 背景

[決策 0019](0019-separate-package-and-registration-identities.md)已经把 logical
package name 与 `<namespace>:<kind>/<path>` stable registration identity 分开。
这解决了「对象是谁」与「由哪个分发单元提供」混在一起的问题，却还没有回答：

- 不同 package 注册的方块／物品是否具有共同材料、用途或机制语义；
- 配方、结构与世界生成应按精确 ID、集合、带值属性还是上下文行为判断；
- 多个候选都满足需求时，应选择哪一个具体内容写入世界；
- 某项内容只作为缺省实现时，如何在已有兼容实现时不激活，又不引入注册顺序；
- 这些选择如何进入 lock、持久化与 static／dynamic equivalence。

Minecraft 的 registry／tag 分层证明精确 ID 与集合成员必须分开；NeoForge 的 data
map、item ability 与 block extension 又证明 boolean tag 无法独自承担带值资料和上下文
行为。直接在模组初始化时查询「是否已经注册铜」再决定注册，会把结果交给发现／加载
顺序，并使新增 package 能让旧世界所需 ID 消失。

Lattice Axiom 已采用 Nickel 作为 package／profile 组合语言。语义注册若退化为 Rust
builder 清单，就会绕过 Nickel 的 function、contract、recursive record、merge、default
与 overlay 能力，形成第二个不具来源诊断的组合平面。

## 決策驅動因素

- 结构、配方、世界生成与玩法不能从 registration namespace 推断内容意义。
- 多个同类内容必须能共存，且兼容性不能依赖 load order 或 last-writer-wins。
- 世界中已经物化的值必须继续保存具体 stable ID。
- 内容作者需要可复用的材料族、结构谓词与默认配置，而不是重复手写展开清单。
- client、headless、static 与 dynamic realization 必须得到相同语义目录与 binding。
- 游戏 tick 不得依赖 Nickel evaluator、字符串查询或 package resolver。
- Terrenia 只是一个可安装、可替换的普通聚合模组，不能成为平台语义的隐藏基础设施。

## 決策

1. 现有 `StableId` 继续只表达精确 registration identity。package、namespace 或路径
   名称不自动赋予材料、用途、结构或行为语义。
2. 在 registration identity 之上建立 versioned semantic registration layer，首批包含：
   `SemanticTag`、`SemanticMap<T>`、共享 `StatePropertyKey`、`Affordance`、
   `ContentPredicate`、`ContentRole` 与 `ContentBundle`。
3. 这些契约本身继续使用 `<namespace>:<kind>/<path>@<major>` stable ID；不增加第二套
   ID 文法。目标 registry kind 进入强型别 schema，例如 block tag 不能包含 item。
4. Tag 回答集合成员，Map 回答带类型值／关系，StateProperty 回答单一已放置实例的有限
   状态，Affordance 回答上下文行为，Role 回答多候选中这次组合选择哪一个具体对象。
   禁止以无 schema 的 `Map<String, Any>` 代理所有性质。
5. Tag／Map／Role contract 的定义权仍受 namespace grant 管理；向标记为 extensible 的
   contract 贡献自己拥有的对象，不等于注册该 contract，不要求取得其 namespace。
   修改外部对象的语义必须来自显式 integration package 或有 provenance 的 profile overlay。
6. 普通 package 的语义贡献默认只能增加；`remove`／`replace` 与强制 role binding 只属于
   composition root／profile policy。所有冲突都由声明的 merger 或明确 binding 解决，
   不能按文件、package、link 或 dynamic load 顺序覆盖。
7. 结构与世界查询使用可序列化、带 target kind 的 `ContentPredicate` AST。简单静态行为
   优先使用机制 owner 定义的 Tag／Map；需要位置、状态、邻居或 world context 的判断才
   使用 Affordance。材料分类本身不自动授予机制行为。
8. 接受现有内容时可使用 Tag／Predicate；创建配方输出、结构 palette 或世界生成结果时，
   必须先把 `ContentRole` 解析为具体 stable ID。chunk、inventory 与 persistent entity
   只保存具体 stable ID，不保存抽象 Tag、Predicate 或未解析 Role。
9. 默认允许多个「铜」「黑曜石」或其他相似内容同时注册。确实只提供缺省实现的内容使用
   声明式 fallback `ContentBundle`；禁止 module code 在 registration callback 中查询当前
   registry 后临时跳过注册。
10. v1 fallback 只在已经选入 graph 的 package 内激活 registration bundle，不隐式增加
    新 package。它以未满足 Role／content contract 为 guard，采用固定两阶段解析，必须
    原子激活，并把结果写入 lock。package 级缺省 provider 仍由 dependency／capability
    resolver 处理。
11. `latticeaxiom.lib` 是 semantic authoring 的唯一公共组合入口。Nickel contracts 验证
    局部形状；functions／enum variants 构造 Tag、Map、Predicate、Role 与 bundle；recursive
    records／defaults 生成材料族和机制模板；profile overlay 选择 binding。Nickel 完整求值
    后只输出可序列化的 `CompositionSpec`，function／AST source 不进入 runtime。
12. Nickel 的普通 record merge 不承担 additive array 拼接。`latticeaxiom.lib` 提供明确的
    `concat`／`add` combinator；相同优先级的互斥 binding 让 Nickel merge 直接报来源冲突，
    package default 使用 `default`，`force` 只允许受信任 composition policy 使用。
13. Rust package kernel 负责跨 package 的 namespace、target kind、Tag DAG、Map merger、
    Predicate type、Role cardinality、fallback cycle、world lock 与 artifact manifest 验证，
    再把结果编译进 `RegistrationImage`。Nickel 不承担依赖全 source universe 的全局真值。
14. `RegistrationImage` 包含展开后的 semantic sets／maps、role bindings、active bundles、
    predicate plans、provenance 与 canonical hash；runtime 使用 numeric bitset、dense／sparse
    table、numeric role target 与已编译 predicate，不在热路径执行 Nickel 或字符串查找。
15. `latticeaxiom:*` 只定义平台级公开契约，不注册 Terrenia 私有内容。`terrenia` 与
    `@terrenia/*` 像任何第三方 package 一样贡献内容、定义 `terrenia:*` 机制语义、接受
    namespace 验证并由 profile 选择；host 不对它提供默认 binding、fallback 或特殊权限。

## 語義層次

| 层 | 示例 | 负责的问题 |
| --- | --- | --- |
| exact registration | `example:block/obsidian` | 这个对象是谁？ |
| semantic tag | `latticeaxiom:block-tag/obsidians/normal@1` | 它属于哪个共享集合？ |
| semantic map | `latticeaxiom:block-map/physical@1` | 它携带哪些 typed values／relations？ |
| state property | `latticeaxiom:block-state/axis@1` | 这个已放置实例当前是什么状态？ |
| affordance | `terrenia:block-affordance/nether-portal-frame@1` | 在这个 context 能否执行机制行为？ |
| predicate | `All(InTag(...), StateEq(...))` | consumer 如何组合判定？ |
| content role | `latticeaxiom:block-role/storage-block/copper@1` | 这次 closure 选哪一个具体候选？ |
| content bundle | `example:content-bundle/copper-fallback@1` | 哪组 registration 必须原子启用／停用？ |

## 正面結果

- namespace 保持长期身份治理，不再被误用为材料或玩法分类。
- 多个 package 的同类内容可以共存，配方输入、结构匹配和行为兼容具有明确公共契约。
- 生成与输出仍得到单一具体 ID，避免把抽象集合写进存档。
- Nickel 真正承担可组合 authoring，而 Rust 保留全图验证、锁定与高效执行。
- fallback 是可检查、可重现、可冻结的组合结果，不是初始化时序副作用。
- Terrenia 能被另一个维度／内容聚合 package 替换而无需改 host 或平台 semantic contract。

## 負面結果與成本

- `RegistrationManifest`、`CompositionSpec`、lock 与 world metadata 都需要新增 schema。
- Tag、Map、Role 与 Affordance 语义需要治理、文档、版本与冲突诊断，不能任意造同义词。
- 不是所有行为都能 data-drive；复杂 Affordance 仍需 static system 或 dynamic callback。
- fallback 的负条件容易产生循环，因此 v1 有意限制为两阶段、package 内 bundle。
- role binding 与 authoritative semantic change 会改变 registration hash，开发期 reload 不能
  无条件应用到已开放写入的世界。

## 被否決的方案

### 把性質編進 StableId 或 namespace

`foo:block/copper` 仍只说明一个名字。由路径推断铜、矿石或传送门行为会让改名、翻译与
package 组织渗入长期语义，也无法表达一个对象同时属于多个集合。

### 單一無型別 properties 字典

它缺少 target kind、value schema、merge、version、owner 与 hot-path representation；最终
每个 consumer 都会重新解释字符串，冲突只能靠约定或 load order。

### 只實作 Tag

Tag 适合 boolean membership，却不能自然表达硬度、转换目标、有参数行为或「从多个候选选
一个作为输出」。把所有东西编码成 Tag 会产生组合爆炸与错误的行为推断。

### 自動把語義相同內容去重為同一 ID

两个铜块仍可能有不同资产、状态、掉落、版本与 owner。语义相近不构成 identity 相同；
自动 alias 会破坏存档 provenance 与迁移责任。

### 模組啟動時查 registry 再條件註冊

这使 registration image 取决于加载顺序，并使已有世界的 required content 因安装新模组
而消失；也无法在执行 native code 前验证 static／dynamic equivalence。

### 讓 Terrenia 擁有平台預設語義

Terrenia 是第一个内容 consumer，不是 host 或标准库。让它拥有全局材料 vocabulary 或
隐式默认会重新建立第一方私有路径，使其他维度无法以同一 package contract 替换它。

## 驗證

- 置换 source discovery、manifest merge 与 artifact load 顺序，semantic image／binding／
  active bundle hash 不变。
- 两个不同 ID 的铜内容同时进入同一共享 Tag；input predicate 接受二者，Role 只解析一个
  concrete output，存档仍区分二者。
- 第三方黑曜石不会因材料 Tag 自动成为传送门框架；明确贡献机制 Tag／Affordance 后才通过。
- Tag cycle、wrong target kind、Map conflict、Role ambiguity、unauthorized foreign amendment
  与 fallback cycle 都在 code activation 前失败并指向 Nickel source／package provenance。
- 新世界可因新增 provider 不激活 fallback；旧世界以 frozen lock 重开时仍保留原 active
  bundle 与 stable IDs。
- CLI／embedded Nickel、static／dynamic realization 与 client／headless 产生相同 semantic
  registration hash。
- 用非 Terrenia 的测试维度替换 `terrenia` closure，平台 Tag／Map／Role 与 host 无需修改。

## 外部依據

- [NeoForge Tags：typed registry membership、additive merge 与 `c` conventions](https://docs.neoforged.net/docs/resources/server/tags/)
- [NeoForge Data Maps：registry object → typed object 的资料驱动扩展](https://docs.neoforged.net/docs/1.21.1/resources/server/datamaps/)
- [Fabric Block States：单一已放置方块的有限 properties](https://docs.fabricmc.net/develop/blocks/blockstates)
- [Nickel record merge、recursive records 与 merge priorities](https://nickel-lang.org/user-manual/merging/)
- [Nickel contracts](https://nickel-lang.org/user-manual/contracts/)

## 相關文件

- [語義註冊、內容判定與選擇架構](../architecture/semantic-registration.md)
- [套件內核](../architecture/package-management.md)
- [模組與註冊清單](../architecture/module-composition.md)
- [Minecraft 註冊與語義相容性調查](../research/minecraft-registration-semantics.md)
- [決策 0010：Nickel 驅動套件系統](0010-nickel-driven-package-system.md)
- [決策 0019：Package 與 registration identity 分離](0019-separate-package-and-registration-identities.md)

