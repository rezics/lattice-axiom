---
title: Demo workspace 與 Terrenia 維度套件組織
status: proposed
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0010-nickel-driven-package-system.md
  - ../decisions/0019-separate-package-and-registration-identities.md
---

# Demo workspace 與 Terrenia 維度套件組織

## 結論

`lattice-axiom-demo` 继续是 Lattice Axiom 的 Rust 实作 workspace。Rust crates、
logical packages、profiles、Nickel contracts、测试 fixtures 与 generated artifacts
分开组织，任何目录名称都不承担 package 或 stable registration 身份。

Terrenia 是第一个维度的 package closure。`@rezics/terrenia` 是聚合 package，
不是产品 package；它让 Lattice Axiom profile 得到一个完整可运行的维度，并在
package graph 中选择方块、世界生成、玩法与表现实现。

## 建議目錄

```text
lattice-axiom-demo/
├── Cargo.toml
├── Cargo.lock
├── latticeaxiom.lock
├── crates/
│   ├── latticeaxiom-core/
│   ├── latticeaxiom-compose/
│   ├── latticeaxiom-packages/
│   ├── latticeaxiom-modules/
│   ├── latticeaxiom-render/
│   ├── latticeaxiom-render-wgpu/
│   ├── latticeaxiom-render-headless/
│   ├── latticeaxiom-storage/
│   ├── latticeaxiom-storage-rocksdb/
│   ├── latticeaxiom-voxel-mesh/
│   ├── latticeaxiom-cli/
│   └── latticeaxiom-demo/
├── nickel/
│   └── latticeaxiom/
│       ├── common.ncl
│       ├── package.ncl
│       ├── game.ncl
│       └── registration.ncl
├── packages/
│   ├── platform/
│   │   └── backend/
│   │       └── package.ncl
│   └── dimensions/
│       └── terrenia/
│           ├── package.ncl
│           ├── blocks/
│           │   ├── package.ncl
│           │   └── assets/
│           ├── worldgen/
│           │   └── package.ncl
│           ├── gameplay/
│           │   └── package.ncl
│           └── presentation/
│               ├── package.ncl
│               └── assets/
├── profiles/
│   ├── dev.ncl
│   ├── headless.ncl
│   └── test.ncl
├── fixtures/
│   ├── packages/
│   │   ├── marker/
│   │   └── dual-gameplay/
│   └── locks/
├── tools/
│   └── xtask/
├── run/
│   ├── worlds/
│   └── packages/
└── target/
    └── latticeaxiom/
        └── <graph-sha256>/
            ├── build-plan.json
            ├── registration-image.json
            ├── static-glue/
            ├── shared-schemas/
            └── abi-bindings/
```

目录表达维护关系，不表达身份。例如：

| Source path | `PackageName` | 可注册的 stable ID 示例 |
| --- | --- | --- |
| `packages/platform/backend` | `@rezics/backend` | `rezics:capability/backend@1` |
| `packages/dimensions/terrenia` | `@rezics/terrenia` | `terrenia:dimension/terrenia` |
| `packages/dimensions/terrenia/blocks` | `@rezics/terrenia-blocks` | `terrenia:block/stone` |
| `fixtures/packages/marker` | `@example/marker` | `example:component/marked` |

这张表是显式 manifest 关系，不是转换规则。把 source directory 移走不改变另外两列；
改 package name 也不会自动改 stable ID。

## Terrenia package closure

```text
@rezics/terrenia
├── @rezics/terrenia-blocks
├── @rezics/terrenia-worldgen
├── @rezics/terrenia-gameplay
└── @rezics/terrenia-presentation   optional in headless
```

### `@rezics/terrenia`

这是维度聚合 package，负责：

- 注册 `terrenia:dimension/terrenia`；
- 接受 source policy／registry 对 `terrenia` namespace 的 authority，并声明对子 package 的精确 grants；
- 声明维度依赖的 package／capability closure；
- 绑定 primary terrain／worldgen／spawn policy 等维度级语义角色；
- 提供 client 与 headless 都能验证的权威部分边界。

它不复制所有方块、system 或 asset 注册。各子 package 产生自己的
`RegistrationManifest`，package kernel 再合并 closure-wide `RegistrationImage`。

概念上的主 package：

```nickel
{
  package = { name = "@rezics/terrenia", version = "0.1.0" },

  dependencies = [
    { name = "@rezics/terrenia-blocks", version = "0.1.0" },
    { name = "@rezics/terrenia-worldgen", version = "0.1.0" },
    { name = "@rezics/terrenia-gameplay", version = "0.1.0" },
    { name = "@rezics/terrenia-presentation", version = "0.1.0", optional = true },
  ],

  namespaces = {
    authority = "terrenia",
    grants = [
      { package = "@rezics/terrenia-blocks", patterns = ["terrenia:block/**", "terrenia:item/**"] },
      { package = "@rezics/terrenia-worldgen", patterns = ["terrenia:worldgen/**", "terrenia:biome/**"] },
      { package = "@rezics/terrenia-gameplay", patterns = ["terrenia:system/**", "terrenia:schema/**"] },
      { package = "@rezics/terrenia-presentation", patterns = ["terrenia:asset/**", "terrenia:render-feature/**"] },
    ],
  },

  registrations = {
    dimensions = [{ id = "terrenia:dimension/terrenia" }],
    bindings = {
      empty_block = "terrenia:block/air",
      terrain_surface = "terrenia:block/grass",
      terrain_subsurface = "terrenia:block/dirt",
      terrain_stone = "terrenia:block/stone",
      worldgen = "terrenia:worldgen/default",
    },
  },
}
```

这只是字段责任示例；`authority` 必须由 profile 的受信任 local source policy 或未来
registry 证明，不能靠 package 自我声明取得。最终 Nickel contract 仍由 schema 与
conformance fixture 冻结。

### 子 package

- `@rezics/terrenia-blocks`：方块、物品与相邻内容资料；直接声明完整
  `terrenia:block/*` ID。
- `@rezics/terrenia-worldgen`：Terrenia generator、biome／terrain provider 与
  generation revision。
- `@rezics/terrenia-gameplay`：只属于该维度的玩法规则。跨维度通用规则应进入
  独立 package，不因目前只有一个维度就放进 Terrenia。
- `@rezics/terrenia-presentation`：材质、音效、天空与其他可选表现；headless
  profile 可以省略，但不能因此改变权威注册与 world hash。

初期不建立 `@rezics/terrenia-registry`。namespace grants 与维度级 bindings 数量很少，
由聚合 package 持有更直接；只有它们出现独立版本、消费者或发布生命周期时才拆包。

## Profile 與 source universe

Profile 的「根 package」与「可用 source」必须分开：

```text
root requests                         local source universe
@rezics/backend                  ←→  packages/platform/backend
@rezics/terrenia                 ←→  packages/dimensions/terrenia
                                    packages/dimensions/terrenia/blocks
                                    packages/dimensions/terrenia/worldgen
                                    ...
```

Lattice Axiom 的开发 profile 请求 backend 与 Terrenia 聚合 package；resolver 根据
dependencies 建立 closure。profile 列出本地 source 只是在尚无 registry 时提供候选，
不表示每个 source 都是 root dependency。

`dev.ncl` 与 `headless.ncl` 可选择不同 presentation realization，但必须解析出相同
Terrenia 维度身份、权威 package、schema 与 semantic bindings。

## Generated artifacts

`RegistrationImage`、static glue、shared-schema crates 与 ABI bindings 都以精确
`graph-sha256` 为输入，写入 `target/latticeaxiom/<graph-sha256>/`。它们：

- 不放进 `packages/`；
- 不取得 `@scope/name`；
- 不参与产生自己的 source graph；
- 记录 producer、schema version、target、toolchain 与 input hash；
- 可以删除并由 lock／source 重建。

生成的 Rust crate 仍可命名为 `latticeaxiom-static-glue`、
`latticeaxiom-terrenia-shared-schema` 等，因为它们属于 Rust build 层，而不是 Rezics
logical package 层。

## 從目前 demo 遷移

1. 保留现有 `crates/latticeaxiom-*` 结构。
2. 将 `PackageId` 改为 `PackageName`，验证 `@scope/name`；lock schema major 升级。
3. 新增独立 `StableId`／`NamespaceGrant` 型别；注册声明改为完整 ID。
4. 删除 package kernel 中 `<package>:<local-id>` 的 key 拼接，lock 同时保存
   stable ID 与 `declared_by`。
5. 用 `packages/dimensions/terrenia/*` 取代 `packages/official`，并建立
   `@rezics/terrenia` 聚合 package。
6. 把 Rust host 对 stone／dirt／grass 的硬编码查找改为读取 Terrenia bindings。
7. 将 profile 的 root requests 与 source universe 分开，再生成新的
   `latticeaxiom.lock`、registration golden 与存档迁移 fixture。

## 驗收

- 只请求 `@rezics/backend` 与 `@rezics/terrenia` 即可解析完整 playable closure。
- 删除 `@rezics/terrenia` 后，host 不会暗中建立默认维度。
- 测试 profile 可用另一个 dimension package 取代 Terrenia而无需修改 Rust host。
- package、source path 与 stable ID 任意一项改变时，另外两项不会被隐式改写。
- client／headless 对 Terrenia 的权威 registration hash 与 bindings 相同。
- generated directory 全部删除后可从 lock 与 source 确定性重建。

## 相關文件

- [决策 0019：Package 与 registration identity 分离](../decisions/0019-separate-package-and-registration-identities.md)
- [套件内核](package-management.md)
- [模组与注册组合](module-composition.md)
- [第一个 demo 路线图](../planning/roadmap-first-demo.md)
