---
title: Logical packages
document_id: packages.index
document_status: active
document_type: index
tracks_implementation: false
updated: 2026-08-22
---

# Logical packages

这里按产品 `PackageName` 导航。路径镜像 source family 仅为了可发现性；package identity、
version、dependency 与 domain 仍以实现仓的 `latticeaxiom-package.toml` 为准。

“当前 lock”表示 package 出现在本次审计的 `client-world` lock，不表示其 capability 已接入
production behavior。“已声明”表示 manifest/source 存在但不在该 lock。“提议”表示规范存在、
manifest 尚不存在。

| Package | Source lifecycle | 主要职责 |
| --- | --- | --- |
| [`@latticeaxiom/front-end`](latticeaxiom/front-end/README.md) | 已声明 | client shell 与 launch routing |
| [`@latticeaxiom/world-library`](latticeaxiom/world-library/README.md) | 已声明 | world catalog、preflight、恢复动作 |
| [`@latticeaxiom/settings`](latticeaxiom/settings/README.md) | 当前 lock | typed settings registry |
| [`@latticeaxiom/settings-ui`](latticeaxiom/settings-ui/README.md) | 当前 lock | settings surface |
| [`@latticeaxiom/observability`](latticeaxiom/observability/README.md) | 当前 lock | diagnostic registry |
| [`@latticeaxiom/inspect`](latticeaxiom/inspect/README.md) | 当前 lock | target inspect surface |
| [`@latticeaxiom/dev-tools`](latticeaxiom/dev-tools/README.md) | 当前 lock | debug workbench |
| [`@latticeaxiom/input`](latticeaxiom/input/README.md) | 已接受，待 manifest/lock | action catalog 与 binding profile |
| [`@latticeaxiom/progress`](latticeaxiom/progress/README.md) | 已声明 | progress graph skeleton |
| [`@latticeaxiom/relations`](latticeaxiom/relations/README.md) | 已声明 | relations graph skeleton |
| [`terrenia`](terrenia/main/README.md) | 当前 lock | Terrenia dimension root |
| [`@terrenia/blocks`](terrenia/blocks/README.md) | 当前 lock | content catalog |
| [`@terrenia/worldgen`](terrenia/worldgen/README.md) | 当前 lock | terrain provider |
| [`@terrenia/gameplay`](terrenia/gameplay/README.md) | 当前 lock | content-specific gameplay rules |
| [`@terrenia/tools`](terrenia/tools/README.md) | 当前 lock | basic tool content |
| [`@terrenia/presentation`](terrenia/presentation/README.md) | 当前 lock | client presentation data |
| [`@terrenia/metallurgy`](terrenia/metallurgy/README.md) | 已声明 | metallurgy skeleton |
| [`@terrenia/science`](terrenia/science/README.md) | 已声明 | science processing skeleton |
| [`@terrenia/thaumaturgy`](terrenia/thaumaturgy/README.md) | 已声明 | thaumaturgy skeleton |
| [`@terrenia/journey`](terrenia/journey/README.md) | 已声明 | Terrenia progress content skeleton |

完整实现状态不在本表手填，统一由 [delivery status](../delivery/status.md) 生成。
