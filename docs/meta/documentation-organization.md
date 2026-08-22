---
title: 文档组织与追踪契约
document_id: meta.documentation-organization
document_status: active
document_type: meta
tracks_implementation: false
updated: 2026-08-22
---

# 文档组织与追踪契约

## 目的

Lattice Axiom 采用 package-first、docs-as-code 的文档模型。规范必须能回答“哪个
package 拥有这项行为、它依赖什么、怎样验证”，实现状态必须来自可复现证据，不能由
roadmap 文案、Markdown checkbox 或文件成熟度推断。

本页规定 active 文档树的目录、身份、frontmatter 与维护流程。需求和实现证据的详细
格式见[需求与实现证据](traceability.md)。

## 三种身份不得混淆

| 身份 | 示例 | 文档位置 | 身份来源 |
| --- | --- | --- | --- |
| logical package | `@latticeaxiom/settings`、`@terrenia/blocks` | `docs/packages/<scope>/<name>/` | package manifest；路径只用于导航 |
| platform system | package kernel、runtime host、native ABI | `docs/platform/<system>/` | accepted system contract |
| Rust crate | `latticeaxiom-engine`、`latticeaxiom-world-db` | 不建立平行产品规范树 | implementation workspace manifest |

一个 logical package 可以由多个 crates 实现，一个 crate 也可以服务多个 packages。
package 文档必须显式列出 implementation mapping，但不得根据 crate 名称创造第二套产品身份。

## Active 目录契约

```text
docs/
├── README.md                  总入口与生成状态摘要
├── project/                   愿景、策略、技术基线与共同词汇
├── packages/                  按 logical package 组织的产品规范
│   ├── latticeaxiom/
│   └── terrenia/
├── platform/                  跨 package 的 host／kernel／wire／storage 契约
├── delivery/
│   ├── roadmaps/              依赖顺序与 release gates
│   ├── plans/                 有限期 implementation plans
│   ├── evidence/              固定到实现 commit 的机器可读证据
│   └── status.md              由证据生成的只读看板
├── decisions/                 ADR 与兼容性理由
├── research/                  外部调查；不自动成为产品承诺
└── meta/                      本契约、schema、模板与维护工具说明
```

`docs/packages/` 是产品规范的主要入口，但不是所有文件的容器。跨越多个 logical
packages 且由 host 强制执行的机制属于 `platform/`；决策理由、外部研究和 delivery
sequencing 分别保留独立目录，避免复制到每个 package。

根目录不得再建立第二个 `plan/` 文档树。

## Package 目录最低结构

```text
docs/packages/latticeaxiom/settings/
├── README.md          package 身份、职责、依赖、capability 与 implementation mapping
├── registry.md        单一主要问题的规范页
└── requirements.json  本目录规范的机器可读 MUST／SHOULD 验收清单
```

只有已有内容时才新增子页。提议中的 package 可以建立目录，但 `README.md` 必须明确
标为 `proposed`，requirements 的实现状态保持 `not-started`，不得让目录存在本身成为
package 已被接受或已被实现的证据。

## Frontmatter 契约

每份 Markdown 至少包含：

```yaml
---
title: 设置注册与解析
document_id: package.latticeaxiom.settings.registry
document_status: accepted
document_type: package-spec
owners:
  - "@latticeaxiom/settings"
tracks_implementation: true
requirements:
  - SETTINGS-REGISTRY-001
updated: 2026-08-22
---
```

字段规则：

- `document_id`：仓库内永久稳定、全局唯一；移动文件不改变。
- `document_status`：`exploration | proposed | accepted | active | superseded`，只描述
  文档或规范成熟度。
- `document_type`：`index | overview | package-spec | platform-spec | decision | research |
  roadmap | plan | reference | meta`。
- `owners`：规范责任主体；package 使用完整 `PackageName`，platform 文档使用
  `platform:<system>`。index／research／meta 可以省略。
- `tracks_implementation`：只有具有可验证产品要求的规范、roadmap 或 plan 才为 `true`。
- `requirements`：由本文档定义或汇总的稳定 requirement IDs；不追踪实现时省略。
- `updated`：规范内容最后变更日期，不是实现最后验证日期。

禁止继续使用含义模糊的裸 `status` 与 `type` 字段。

## 文档成熟度与实现状态

`document_status: accepted` 只表示规范已被采用。它不表示：

- 对应 crate 已存在；
- package manifest 已进入产品 lock；
- fixture 或 DTO 已接入 production path；
- roadmap milestone 已完成。

实现状态只允许由 requirement evidence 聚合产生：

| 状态 | 含义 |
| --- | --- |
| `not-started` | 没有符合要求的实现证据 |
| `scaffolded` | 只有 manifest、DTO、trait、fixture、generated skeleton 或测试空壳 |
| `in-progress` | 已有 production code，但仍有 MUST requirement 未验证，或产品入口尚未消费 |
| `implemented` | 全部 MUST requirements 在固定 commit 上通过，且真实产品入口消费该实现 |
| `not-applicable` | 该文档不描述可实施产品要求 |

人不得直接在规范 Markdown 中填写或修改实现状态。`docs/delivery/status.md` 及 package
README 中的状态摘要必须由 requirements 与 evidence 生成。

## 单一事实来源

- 规范和验收条件：本仓库的 normative Markdown 与 `requirements.json`。
- package 身份、版本、依赖和 source：实现仓 package manifest。
- crate graph：实现仓 Cargo workspace manifest。
- 实现结果：绑定实现仓 commit 的 evidence snapshot。
- 排期和依赖顺序：roadmap；它不覆盖前四项。

同一条规范只能有一个 primary owner 页面。其他页面用链接和 requirement ID 引用，不能
复制整段 MUST 规则。

## 变更流程

```text
package／platform 规范与 requirement
  → 必要时新增 ADR
  → 实现 PR 引用 requirement ID
  → production code、test 与 product-entry evidence
  → 固定 implementation commit
  → 更新 evidence snapshot
  → 自动生成 package 与全局状态
```

如果修改 writer authority、持久化相容性、trust policy、ABI、world coordinate 或冻结性能
budget，仍须遵守现有 ADR stop conditions；文件重组不能被用来绕开决策流程。

## 文档生命周期

```text
调查／讨论
  → exploration
  → proposed（已有具体可验收方案）
  → accepted（明确采用）
  → superseded（由新规范或 ADR 取代）
```

研究文件可以长期保持 `exploration`。roadmap 和 index 使用 `active`，但 active 仍不表示
其列出的交付已经实现。

## 禁止事项

- 不以目录、crate、类型或 fixture 的存在宣称 package 已实现。
- 不以 `[x]`、章节名“完成”或 prose 百分比作为实现事实。
- 不手填“90%”；显示已验证 MUST 数量，例如 `7/11`。
- 不把 implementation commit 的代码快照复制进本仓库。
- 不让同一规范在 architecture、planning 与 package 页面维护三份。
- 不保留旧路径下的平行 active 页面；迁移后由 Git history 追溯。

## 文档质量最低要求

- 标题直接说明页面回答的问题。
- 开头说明 scope、owner 与非目标。
- 每个 normative MUST 都有稳定 requirement ID 和可复现 acceptance。
- 外部事实链接原始或官方来源；推断明确标记。
- 大改使用可审查的独立 commit；移动与语义改写尽量分轮完成。
