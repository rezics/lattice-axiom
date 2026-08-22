---
title: Architecture Decision Records
document_id: decisions.index
document_status: active
document_type: index
tracks_implementation: false
updated: 2026-08-22
---

# Architecture Decision Records

ADR 保存 accepted compatibility boundary、理由与后果。ADR 的 `document_status` 不是实现状态；
compliance 由 package/platform requirements 与 evidence 证明。

- [0001 Territory-first biome-driven world generation](0001-territory-first-biome-driven-world-generation.md)
- [0002 Hybrid cave generation composition](0002-hybrid-cave-generation-composition.md)
- [0003 No global version switch](0003-no-global-version-switch.md)
- [0004 Territorial delegation](0004-territorial-delegation-for-spatial-generation.md)
- [0008 Static/dynamic realizations share one graph](0008-static-and-dynamic-realizations-share-one-graph.md)
- [0009 RocksDB authoritative snapshots](0009-rocksdb-authoritative-world-snapshots.md)
- [0010 Nickel-driven package system](0010-nickel-driven-package-system.md)
- [0012 Naming convention — superseded scope](0012-latticeaxiom-naming-convention.md)
- [0014 Bevy upstream first](0014-adopt-bevy-upstream-first.md)
- [0015 Bevy-native Y-up coordinates](0015-bevy-native-y-up-world-coordinates.md)
- [0017 Versioned native ABI](0017-versioned-native-module-abi.md)
- [0018 Package kernel from first vertical slice](0018-package-kernel-from-first-vertical-slice.md)
- [0019 Separate package and registration identities](0019-separate-package-and-registration-identities.md)
- [0020 Semantic registration and content selection](0020-semantic-registration-and-content-selection.md)
- [0021 R0/R1 package and Nickel contract](0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md)
- [0022 Controlled Nickel evaluation](0022-freeze-controlled-nickel-evaluation-metering-and-worker-protocol.md)
- [0023 SDK registration and semantic compilation](0023-freeze-sdk-registration-and-semantic-compilation.md)
- [0024 Portable native ABI 0.x](0024-freeze-portable-native-abi-0x.md)
- [0025 Client shell, settings, observability and player](0025-freeze-client-shell-settings-observability-and-player-contracts.md)
- [0026 First-demo performance budgets](0026-freeze-first-demo-performance-budgets.md)
- [0027 Authoritative world and persistence](0027-freeze-authoritative-world-and-persistence-contract.md)
- [0028 Worldgen, content and asset](0028-freeze-worldgen-content-and-asset-contract.md)
- [0029 Render capability and provider](0029-freeze-render-capability-and-provider-contract.md)
- [0030 Governance, distribution and security triggers](0030-freeze-governance-distribution-and-security-triggers.md)
- [0031 Bevy upgrade and supply chain](0031-freeze-bevy-upgrade-dependency-and-supply-chain-policy.md)
- [0032 Local package acquisition and product lock](0032-freeze-local-package-acquisition-imports-and-product-lock.md)

编号空缺只通过 Git history 追溯，不在 active tree 保留互斥方案。
