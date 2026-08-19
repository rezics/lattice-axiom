---
title: 靜態與動態實現收斂於同一套件圖
status: accepted
type: decision
updated: 2026-08-19
---

# 決策 0008：靜態與動態實現收斂於同一套件圖

## 背景

可信內容需要靜態連結、完整 Rust／Bevy 型別能力與 LTO；可安裝模組需要不重编整個游戏即可部署的动态实现。若两条路径各自定义 manifest、相依解析、ID、schema owner、schedule 与注册规则，第一方内容会拥有私有捷径，组合结果也无法重现。

Rust 没有稳定 ABI，Bevy 也不把动态 Rust plugin 作为稳定边界。因此「共用一切」既不可行，也会消灭静态实现的性能优势。正确的共同层是逻辑套件图与声明式注册语义，不是最低层呼叫约定。

## 決策

1. 套件的逻辑身分、SemVer、相依、capability、schema owner 与 realization 彼此分离。同一逻辑套件可提供 `NativeStatic`、`PortableNative`、`EngineCoupledNative` 或纯资料 realization；未来可增加 `WasmComponent`。
2. 所有来源与 overlay 先经 Nickel 产生 `CompositionSpec`，再由唯一 package kernel 解析成 `LockedGameGraph`。静态与动态路径不得拥有第二套 resolver、ID 分配或冲突规则。
3. `LockedGameGraph` 经 realization planner 产生 `BuildPlan`。plan 可以为不同节点选择不同 realization，但选择结果、target、artifact hash、ABI／capability 版本与 `EngineBuildId` 要写入 lock。
4. 受控 SDK／proc macro 从同一份业务注册定义生成 `RegistrationManifest` 与两类 adapter：静态 adapter 直接持有 typed Rust function；动态 adapter 输出 C ABI table、POD／batch view 与 callback map。作者不得手写两套业务语义。
5. 每个 realization 必须产生相同的稳定内容 ID、component／message schema、system stage、读写集合、ordering、asset 与 capability 声明。package kernel 合并并验证为 closure-wide `RegistrationImage`。
6. `NativeStatic` 是源码／可重建 realization，由 Cargo 编入 host。它直接调用生成的 Rust／Bevy glue，可接受 monomorphization、inlining、LTO 与 specialization；不承诺 `.rlib`、Rust symbol 或 layout 是可分发 ABI。
7. `PortableNative` 是可信原生动态库，只经[决策 0017](0017-versioned-native-module-abi.md)定义的 C ABI 与 versioned capability tables 访问 host，不直接引用 Bevy／Rust layout。只要所需 Lattice ABI 与 capability 相容，它不因 Bevy patch 自动失效。
8. `EngineCoupledNative` 使用相同 C bootstrap，但可查询仅对精确 `EngineBuildId` 开放的内部 capability。它换取更低层能力，也明确接受每次相关 Bevy／host 变更都可能重建。
9. 需要完整 `bevy::App`、`World`、`RenderApp` 或任意 Rust system parameter 的扩充必须选择 `NativeStatic`；不能以「动态」名义把不稳定 Bevy／Rust型别穿过 ABI。
10. 游戏热路径只使用已验证的 numeric ID、连续资料、直接函数或批次 callback，不回查 Nickel、resolver 或字串套件键。动态结构修改通过 command buffer，不在 foreign callback 中任意修改 Bevy World。
11. v1 不支持 native library 热卸载。变更 realization 或停用套件要重建／重启 closure；停止 instance 不等于安全卸载其 code pages。

## 共同與分離的邊界

| 契約 | 靜態與動態是否共用 | 說明 |
| --- | --- | --- |
| package ID／SemVer／dependency | 是 | 由 `LockedGameGraph` 决定 |
| capability 与 realization selection | 是 | 由 `BuildPlan` 决定并锁定 |
| stable content／component／message ID | 是 | 由 schema owner 与 manifest 决定 |
| registration／schedule semantics | 是 | 同一 SDK 输入产生 `RegistrationManifest` |
| persistence schema | 是 | 不得依 realization 改变资料解释 |
| Rust generic／Bevy system type | 否 | 只属于 `NativeStatic` |
| C ABI table／POD batch view | 否 | 只属于 dynamic native realization |
| LTO／inlining／monomorphization | 否 | 静态路径保留完整优势 |
| exact `EngineBuildId` | 仅 engine-coupled | portable path 不应无故耦合 |

## 管線

```text
package.ncl + game.ncl + overlays
                  ↓ Nickel
          CompositionSpec
                  ↓ package kernel
          LockedGameGraph
                  ↓ realization planner
             BuildPlan
            ┌─────┴──────────┐
            ▼                ▼
  NativeStatic glue   dynamic artifacts
            └─────┬──────────┘
                  ▼
       RegistrationImage + RuntimeImage
                  ↓ Bevy host adapter
               Bevy App
```

## 結果

- 同一份业务代码可产生静态与动态 realization，而不复制 package／registration 语义。
- 静态路径继续获得原生 Rust／Bevy API 与编译器跨 crate 优化；统一模型不会把它降成经 C ABI 呼叫的「伪静态」。
- 动态路径的可移植性来自较窄、显式版本化的 Lattice ABI，代价是不能任意取得 Bevy World。
- engine-coupled 动态路径诚实暴露升级成本，不把精确 build 相容误称为稳定 ABI。
- SDK、codegen 与 equivalence test 成为核心基础设施，而不是可选方便层。

## 被否決的方案

### 靜態與動態各自一套 API

这会让第一方内容、开发者模组与预编译模组形成三个生态，也无法证明 lock、ID、schedule 与存档一致。

### 所有 realization 都走同一个 C ABI

这会无谓放弃静态的 Rust type system、内联、LTO 与 Bevy-native system scheduling，且不能带来额外相容性。

### 動態庫直接暴露 Rust／Bevy 型別

Rust layout、trait object、panic、allocator 与 Bevy内部 API 都不是稳定跨库契约；同版本「能链接」不等于可长期分发。

### 以檔案枚舉或載入順序決定結果

发现顺序不是版本解析、冲突策略或稳定 ID 规则，不能重现。

## 驗證

- 同一 fixture package 分别以 `NativeStatic` 与 `PortableNative` 构建，产生相同 `LockedGameGraph`、`RegistrationImage`、stable／numeric ID、system order 与权威 state hash。
- 基准同时量测静态直接调用、动态批次调用与反例式逐 entity FFI；静态路径没有被强制绕经 ABI。
- 随机化来源发现、crate registration 与 dynamic load 顺序不改变结果。
- Bevy 升级演练中，portable fixture 不需重编即可加载；engine-coupled fixture 被明确拒绝并在重编后通过。
- 不相容 ABI、capability、manifest hash 或 artifact hash 在进入 `Playing` 前失败并给出 owner-aware 诊断。

## 相關文件

- [決策 0010：Nickel 驅動套件系統](0010-nickel-driven-package-system.md)
- [決策 0017：版本化原生模組 ABI](0017-versioned-native-module-abi.md)
- [模組組合](../architecture/module-composition.md)
