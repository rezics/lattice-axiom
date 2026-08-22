---
title: Platform contracts
document_id: platform.index
document_status: active
document_type: index
tracks_implementation: false
updated: 2026-08-22
---

# Platform contracts

Platform 文档描述跨 logical package、由 kernel/host/wire/storage 强制执行的机制。它们不创造
logical package，也不让 Rust crate 取得产品 identity。

| System | Contract |
| --- | --- |
| Package acquisition/resolution/lock | [Package management](package-kernel/package-management.md) |
| Nickel composition 与 realization | [Module composition](composition/module-composition.md) |
| Stable registration 与 semantic compilation | [Semantic registration](registration/semantic-registration.md) |
| Bevy application host | [Game engine runtime](runtime/game-engine-runtime.md) |
| Portable native boundary | [Native ABI](native-abi/native-module-abi.md) |
| Authoritative snapshots | [World persistence](world-storage/world-persistence.md) |
| Generation coordinator | [World generation](world-generation/world-generation.md) |
| Assets/render graph | [Asset semantics](assets/asset-semantics.md)、[rendering](rendering/rendering.md) |
| Generic gameplay transactions | [Player surfaces](gameplay/player-surfaces.md) |
| Diagnostic host adapter | [Observability adapter](observability/diagnostics-inspection-and-debug-visualization.md) |
| Input runtime | [Bindings and contexts](input/input-binding-and-contexts.md) |
| Shared client UI | [UI system](client-ui/ui-system.md)、[surface routing](client-ui/game-surface-state.md) |
| Product process loop | [Launcher supervisor](launcher/supervisor-loop.md) |

每个 system 的实现状态由 requirements/evidence 计算；crate 已存在只说明有候选 mapping。
