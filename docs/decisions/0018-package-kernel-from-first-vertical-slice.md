---
title: 從第一個垂直切片交付套件內核與雙實現
status: accepted
type: decision
updated: 2026-08-19
---

# 決策 0018：從第一個垂直切片交付套件內核與雙實現

## 背景

把 Nickel、package kernel 与动态 ABI 延后到可玩 demo 之后，适合一般「以后可能支持 mods」的游戏，却不适合 Lattice Axiom：逻辑套件、SemVer、可组合 closure 与静态／动态双 realization 是核心产品模型。如果先让官方内容直接成为 Bevy Plugin，等 domain API、schedule、ID 与渲染路径固定后再补 package graph，项目会被迫维护旧／新两条启动路径，或进行第二次全面重写。

最佳时机不是先完成一套抽象 package platform，也不是游戏完成以后，而是**第一个真实玩法垂直切片的最窄闭环**：让一个小而真实的玩法／内容 package 同时走静态与动态路径，再由同一个已锁定 graph 启动 Bevy。

## 決策

1. Nickel composition、Rust package kernel、`LockedGameGraph`、registration codegen 与 native ABI `0.x` 属于首阶段架构，不得以「先做 Bevy demo」为由绕过。
2. 所有 client、headless、test 与 tool profile 都从 `game.ncl`／受控测试 `CompositionSpec` 进入同一 compose／resolve／plan 流程。开发捷径可以使用 in-memory source／artifact，但不能直接手写另一套 `App::add_plugins` closure 作为产品真相。
3. 首个 vertical slice 必须包含至少一个真实 `TestGameplayPackage`，由同一 SDK 业务代码生成 `NativeStatic` 与 `PortableNative`，并证明 registration、ID、schedule、权威 state 与存档 schema 等价。
4. Bevy standard／ecosystem plugins 由核心 host adapter 按 `RegistrationImage` 与 profile 安装；Bevy 仍拥有 App、ECS、schedule、render、asset、input 与 tasks。package kernel 不复制这些 engine 能力。
5. 首阶段 package scope 有意缩小为：local／workspace source、一个确定性 SemVer policy、精确 lock、单一主要 target、可信 native code、无卸载、最小 artifact directory 与清楚诊断。
6. 首阶段不得省略：package identity／SemVer、capability conflict、realization selection、manifest hash、ABI validation、stable ID、schema owner、static／dynamic equivalence 与 `EngineBuildId`。
7. public registry、通用 PubGrub／SAT 规模、多平台预建矩阵、远端 marketplace、透明签章、分散式 cache、native hot unload 与 WASM 生态延后。它们扩展分发／安全／规模，不改变首阶段核心语义。
8. 可玩性与 package 证据并行推进：每个基础设施阶段都必须尽快由挖掘／放置／保存或同等真实 gameplay consumer 使用，不能长期停留在「能加载空动态库」的技术展示。

## 首階段依賴順序

```text
K0  Nickel contracts + typed CompositionSpec
 ↓
K1  SemVer lock + capability / realization planning
 ↓
K2  SDK/proc-macro + RegistrationManifest
 ↓
K3  NativeStatic + PortableNative ABI 0.x equivalence fixture
 ↓
K4  RegistrationImage / RuntimeImage → Bevy host adapter
 ↓
K5  playable voxel edit + persistence through the same graph
 ↓
K6  rendering composition + Bevy upgrade rehearsal
```

K0–K4 是最小架构闭环，不要求先建立 registry service。K5 防止 package platform 脱离游戏；K6 提供 ABI 1.0 冻结所需的真实压力。

## 結果

- package graph 从第一天就是唯一启动真相，不会在 Bevy API 固化后再补一层平行系统。
- 动态 ABI 可以在 package、system 与 render 契约仍可调整时以 `0.x` 迭代，修正成本最低。
- 首个 demo 的工作增加了 package kernel／SDK／loader，但范围由单一垂直切片限制，不等于先做完整 marketplace。
- Bevy upstream-first 与 Lattice package-first 不冲突：前者避免重做引擎能力，后者定义游戏如何组合、版本化与分发。

## 被否決的方案

### demo 完成後再設計 ABI

届时静态 Bevy type、私有 plugin 顺序与官方捷径已经渗入 domain API，动态适配会决定性地更贵，也无法证明同一业务代码双 realization。

### 先完成完整 package manager 再做遊戲

没有真实 system、schema、render feature 与性能资料，ABI 与 capability 会成为抽象猜测。最小 vertical slice 才是正确设计压力。

### 官方內容暫時繞過 graph

任何暂时捷径都会让公开路径失去首位 consumer；官方与测试 package 必须同样被锁定、验证与激活。

## 驗證

- 空 profile、client profile 与 headless profile 均能从 lock 重建相同 closure，不存在隐藏手写 plugin list。
- 同一 gameplay fixture 的 static／dynamic realization 产生相同 registration hash、numeric ID、system ordering、tick state hash 与 save round-trip。
- package／ABI 错误在建立 Bevy `Playing` state 前给出 package、range、capability、artifact 与修复建议。
- 第一版可玩循环从 `LockedGameGraph` 启动；移除 package kernel 后没有另一条仍可运行的官方启动路径。
- 明确列出的延后项目没有被暗中实现，且核心 graph 不依赖其存在。

## 相關文件

- [決策 0010：Nickel 驅動套件系統](0010-nickel-driven-package-system.md)
- [決策 0017：版本化原生模組 ABI](0017-versioned-native-module-abi.md)
- [第一個 demo 路線圖](../planning/roadmap-first-demo.md)
