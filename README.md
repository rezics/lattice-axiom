# Lattice Axiom

Lattice Axiom（晶格公理）是建立在 [Bevy](https://github.com/bevyengine/bevy) 上、以
Nickel／SemVer package graph 与静态／动态双 realization 为核心的体素世界游戏。

本仓库是产品规范与决策仓；Rust、Nickel packages 和可运行 vertical slice 位于
[`rezics/lattice-axiom-demo`](https://github.com/rezics/lattice-axiom-demo)。规范成熟度与实现
进度严格分离：accepted ADR 是兼容性边界，只有绑定实现 commit、production path 和可复现
checks 的 evidence 才能把 requirement 标为已实现。

从[文档入口](docs/README.md)开始，或直接浏览 [logical packages](docs/packages/README.md)、
[platform contracts](docs/platform/README.md) 与 [delivery](docs/delivery/README.md)。

## License

Except where otherwise noted, REZICS is licensed under the
[GNU Affero General Public License v3.0 only](./LICENSE) (`AGPL-3.0-only`).
Copyright © 2026 Rezics Inc.

Third-party components remain under their respective terms; exact notices
will be added when implementation dependencies are locked. The AGPL grants
copyright permissions only and does not grant trademark rights in the REZICS
name, logos, or other brand identifiers.
