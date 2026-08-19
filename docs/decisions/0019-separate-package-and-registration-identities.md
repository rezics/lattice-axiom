---
title: 分離 scoped package 名稱與統一註冊識別，並以 Terrenia 作為第一維度
status: accepted
type: decision
updated: 2026-08-19
supersedes:
  - 0012
---

# 決策 0019：分離 scoped package 名稱與統一註冊識別，並以 Terrenia 作為第一維度

## 背景

早期命名约定让 Rust crate、logical package 与 stable registration ID 共用
`latticeaxiom` 前缀。第一个 demo 又把 package-local block ID 机械扩展为
`<package>:<local-id>`。这种做法混合了三个本来独立的问题：代码由哪个 Rust
crate 实现、哪个分发单元提供它，以及世界／存档用什么稳定名称引用它。

这在第三方模组开始注册方块、维度、schema 与 system 后会形成错误边界：package
被迫拥有一个同名内容命名域；移动注册项到另一个 package 可能改变持久化 ID；
第一方 package 还会因 `official` 或 `latticeaxiom` 前缀而失去清楚的领域名称。

产品名称仍然是 **Lattice Axiom**。**Terrenia（泰拉尼亚）是由 package closure
定义的第一个维度，类似体素游戏的主世界；它不是产品的新名称，也不是 host
内建的特殊维度。**

## 決策

1. Rust 实现继续使用 `latticeaxiom-<component>` crate 名称；Rust 路径仍为
   `latticeaxiom_<component>`。动态 ABI 的 `latticeaxiom_module_entry` 与
   `Lax`／`lax_` 前缀也保持不变。
2. Logical package 使用 scoped package name：`@<scope>/<name>`。第一方示例为
   `@rezics/backend`、`@rezics/terrenia` 与 `@rezics/terrenia-worldgen`；第三方使用
   自己可治理的 scope，例如 `@example/marker`。
3. Package name 只标识可版本化、可解析、可分发的逻辑单元。它不是注册命名域，
   也不从源码目录、Cargo crate、publisher 显示名称或 stable ID 推导。
4. 所有跨重启稳定的内容、维度、schema、system、capability 与 render contract
   使用统一 registration ID：`<namespace>:<kind>/<path>`；需要 contract major 时
   可使用结尾的 `@<major>`。例如：
   `terrenia:dimension/terrenia`、`terrenia:block/stone`、
   `rezics:capability/content-blocks@1`。
5. `PackageName("@rezics/terrenia-blocks")` 与
   `StableId("terrenia:block/stone")` 是两个独立型别。实现中不得提供
   package name → namespace 的隐式转换，也不得以字符串拼接产生 stable ID。
6. Package manifest 直接声明完整 stable ID，并由 lock／registration manifest
   另外记录 `declared_by` package。这样移动实现边界不会自动改名，诊断与 provenance
   仍能指出实际提供者。
7. Namespace 使用显式授权。受信任的 local source policy／未来 registry 将
   `terrenia` namespace authority 授予 `@rezics/terrenia`，后者再按 kind／path
   grant 给 `@rezics/terrenia-blocks`、
   `@rezics/terrenia-worldgen` 等 package。package 名称相似本身不构成授权。
8. `official` 不进入 package name 或 stable ID。第一方身份由 publisher、签章、
   source policy 与 trust metadata 表达；第一方、测试与第三方 package 仍走相同 graph。
9. `@rezics/terrenia` 是 Terrenia 维度包族的聚合 package。它注册
   `terrenia:dimension/terrenia`、声明 namespace grants 与维度级 semantic bindings，
   并依赖方块、世界生成、玩法与表现 package。Lattice Axiom profile 选择这个 package；
   host 不手写 Terrenia 的方块或 generator 清单。
10. 从 lock／registration schema 生成的 static glue、shared-schema crates、C bindings
    与 `RegistrationImage` 是 build artifacts，不自动成为新的 logical packages。
    只有需要独立 SemVer、依赖选择与发布生命周期的生成结果才可提升为 package。

## 身分層次

| 层次 | 示例 | 负责的问题 |
| --- | --- | --- |
| 产品 | Lattice Axiom | 游戏／平台的对外名称 |
| Rust crate | `latticeaxiom-packages` | Rust 编译与代码消费 |
| Logical package | `@rezics/terrenia-blocks` | SemVer、依赖、来源、分发、realization |
| Registration namespace | `terrenia` | stable ID 的治理与授权 |
| Stable registration ID | `terrenia:block/stone` | 存档、schema、system 与 runtime registration 的长期引用 |
| Runtime numeric ID | `BlockId(…)` | 单次 runtime 的热路径 |

这些层之间可以有关联，但都必须由 manifest／lock 明文记录，不能靠名称相似自动转换。

## 結果

- Lattice Axiom 保留既有产品与 Rust 实现身份。
- Terrenia 能作为普通 package closure 安装、替换、测试或移除；它没有 host 私有路径。
- 一个 package 可注册多种 kind；多个获授权 package 也可共同维护同一 namespace。
- package 拆分、static／dynamic realization 切换与源码目录移动不会自动改变存档 ID。
- scoped package name 需要 registry／local source universe 保证唯一；namespace grant
  需要独立验证，不能再依赖 package ID 前缀完成。

## 被否決的方案

### 以 package name 自動產生 stable ID

例如把 `@rezics/terrenia-blocks` 与 local ID `stone` 拼成注册 key。这样分发边界会
渗入持久化身份，package 重组会成为内容迁移。

### 把 Terrenia 當成產品新名稱

Terrenia 是第一个维度及其 package closure，不是 Lattice Axiom 的替代品牌。
把二者合并会再次混淆 host／产品身份与可替换内容身份。

### 保留 `official` package

`official` 是来源与信任属性，不是领域身份。它不能说明 package 提供的是维度、
方块还是玩法，也会把发布政策写进长期 ID。

## 驗證

- parser 分别拒绝不符合 `@scope/name` 与 `<namespace>:<kind>/<path>` 的值。
- 没有 API 能从 `PackageName` 隐式构造 `StableId`；manifest 必须给完整 ID。
- 把 `terrenia:block/stone` 从一个获授权 package 移到另一个后，stable／numeric
  mapping 与存档引用保持不变，只有 provenance／graph hash 改变。
- 未获 grant 的 package 注册 `terrenia:*` 会在 code activation 前失败。
- Lattice Axiom 的 client、headless 与 test profile 均通过 `@rezics/terrenia`
  得到 `terrenia:dimension/terrenia`，host 中没有 Terrenia 私有清单。

## 相關文件

- [Demo workspace 与 Terrenia package 组织](../architecture/demo-workspace-organization.md)
- [套件内核](../architecture/package-management.md)
- [模组与注册组合](../architecture/module-composition.md)
- [决策 0012：旧命名约定](0012-latticeaxiom-naming-convention.md)
