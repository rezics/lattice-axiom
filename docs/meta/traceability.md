---
title: 需求与实现证据
document_id: meta.traceability
document_status: active
document_type: meta
tracks_implementation: false
updated: 2026-08-22
---

# 需求与实现证据

## 目标

本项目把“文档驱动代码”定义为可追踪闭环：规范产生稳定 requirement，代码和测试产生
绑定 commit 的 evidence，状态工具只根据两者计算结果。报告、commit message、文件存在与
人工印象均不是 implementation truth。

## Requirement 文件

每个 package 或 platform system 在自己的目录保存一个 `requirements.json`。结构必须通过
[`requirements.schema.json`](schemas/requirements.schema.json)。

最小示例：

```json
{
  "$schema": "../../../meta/schemas/requirements.schema.json",
  "schema_version": 1,
  "owner": "@latticeaxiom/settings",
  "requirements": [
    {
      "id": "SETTINGS-REGISTRY-001",
      "document_id": "package.latticeaxiom.settings.registry",
      "level": "must",
      "statement": "locked registration image produces one deterministic settings catalog",
      "acceptance": [
        "a production client and headless runtime consume the same catalog",
        "duplicate exactly-one providers fail before application startup"
      ]
    }
  ]
}
```

规则：

- ID 发布后不得改义；语义改变时新增 ID，并显式 supersede 旧项。
- `statement` 描述产品可观察结果，不描述“建立一个 struct”之类实现动作。
- `acceptance` 必须能由 test、smoke、benchmark 或有限人工验收重现。
- `must` 参与 implemented 聚合；`should` 显示但不阻塞 implemented。
- requirement 不保存 `status`；状态属于 evidence。

## Evidence 文件

证据快照位于 `docs/delivery/evidence/`，通过
[`implementation-evidence.schema.json`](schemas/implementation-evidence.schema.json)。每个文件
固定一个实现仓 commit，可以包含多个 requirements 的证据。

```json
{
  "$schema": "../../meta/schemas/implementation-evidence.schema.json",
  "schema_version": 1,
  "implementation": {
    "repository": "https://github.com/rezics/lattice-axiom-demo",
    "commit": "0123456789abcdef0123456789abcdef01234567"
  },
  "evidence": [
    {
      "requirement_id": "SETTINGS-REGISTRY-001",
      "status": "verified",
      "production_paths": ["crates/latticeaxiom-engine/src/..."],
      "product_entries": ["task play"],
      "checks": ["cargo test -p latticeaxiom-engine ..."]
    }
  ]
}
```

证据不得包含 secret、机器绝对路径、未提交 working-tree 状态或不可重现的临时端口。

## Evidence 状态

| evidence status | 使用条件 | 聚合结果 |
| --- | --- | --- |
| `scaffolded` | 只有声明、schema、fixture 或 isolated prototype | `scaffolded` |
| `partial` | production code 或 integration test 已存在，但 acceptance 未闭合 | `in-progress` |
| `verified` | acceptance 在固定 commit 通过，且 product entry 使用该路径 | 可计入 `implemented` |

`verified` 必须同时具有：

1. 至少一个 production path；
2. 至少一个真实 product entry 或 production integration gate；
3. 至少一个可复现 check；
4. 完整 40 位 implementation commit；
5. 与 requirement acceptance 相符的结果。

纯视觉或硬件相关要求可以使用 bounded manual check，但仍须记录场景、reference profile 和
保存的 artifact；“我看过了”不是证据。

## 文档状态聚合

对 `tracks_implementation: true` 的页面：

1. 读取 frontmatter 的 requirement IDs；
2. 找出所有 MUST requirements；
3. 按固定 commit 读取 evidence；
4. 有且仅有全部 MUST 为 `verified` 时显示 `implemented`；
5. 只有 scaffold evidence 时显示 `scaffolded`；
6. 任意 MUST 是 partial、缺失或多 commit 证据未形成一致快照时显示 `in-progress` 或
   `not-started`。

页面没有 requirement IDs 时不得显示 implemented。跨页面看板显示 `verified MUST / total
MUST`，而不是人工百分比。

## Evidence 新鲜度

证据只证明其固定 commit，不自动证明较新的 implementation HEAD。看板必须显示 commit；
当实现仓 manifest、package lock、相关 production path 或 check 定义发生变化时，应重新运行并
更新 evidence。无法取得新证据时保留旧 commit 标签，不把它描述成“当前 HEAD 已验证”。

## Review checklist

- requirement 属于正确 package 或 platform owner。
- statement 是产品结果，不是任务描述。
- acceptance 同时覆盖正常路径与关键失败路径。
- evidence 指向已提交代码和真实产品入口。
- fixture evidence 没有被误当成 production integration。
- `implemented` 是工具计算结果，而非 reviewer 主观批准。

## 本地与 CI 检查

更新 requirement 或 evidence 后运行：

```powershell
python tools/docs_status.py generate
python tools/docs_status.py check
```

`generate`只重写 `docs/delivery/status.md`；`check`验证 frontmatter、唯一 document/requirement
IDs、内部链接、ADR references、requirements/evidence 结构、verified evidence 门槛，以及生成页是否
过期。GitHub Actions 在每个 pull request 与 `main` push 上运行同一检查。
