---
title: 內容分發與套件系統的分階段邊界
status: exploration
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0016-stage-content-composition-on-bevy.md
---

# 內容分發與套件系統的分階段邊界

## 結論

Lattice Axiom 現在沒有自訂套件管理器。第一個可玩 demo 使用 Cargo、`Cargo.lock`、Bevy plugin 與 typed asset；在出現真實創作者、分發、安全或部署問題前，不設計 resolver、registry、動態 ABI、沙箱或新的組合語言。

「沒有自訂套件系統」不是放棄可組合內容，而是先用 Bevy 已有機制驗證內容邊界。

## Phase 0：可玩原型

| 問題 | 解法 |
| --- | --- |
| Rust 相依 | Cargo workspace、SemVer manifest、`Cargo.lock` |
| App 組合 | Bevy `Plugin`／`PluginGroup` |
| 資料內容 | Bevy typed asset、serde 支援格式與現有 loader |
| 官方／測試內容 | 兩個獨立 content plugin，共用公開 registration |
| 建置重現 | toolchain file、`Cargo.lock`、CI artifact metadata |
| 存檔相容 | stable content ID、schema owner 與 migration |

本階段不需要 manifest DSL、package store、lock graph 的第二份實現或 binary loader。

## Phase 1：真實第二內容集合

在第一個 demo 可玩後，由一個與官方內容不同的內容集合驗證：

- 能否只用公開 API 註冊內容；
- content ID、asset path 與 schema 是否足夠；
- 是否必須修改 domain core；
- Cargo build／feature 組合對主要開發者是否可接受；
- 非程式內容能否只靠 typed assets 加入；
- 缺失、衝突、升級與移除診斷是否可理解。

若這些問題可由改進 Bevy plugin／asset API 解決，就停在 Phase 1，不建立 package platform。

## Phase 2：只有證據才能啟動的研究

至少出現一項真實失敗後，才建立新的 ADR：

| 觀察到的需求 | 先調查的 upstream 類別 |
| --- | --- |
| 非開發者安裝資料內容 | Bevy asset／scene 生態、資料包工具、一般 archive／manifest 標準 |
| 伺服器選擇權威內容而不重編 client | 網路內容協商、server plugin deployment、container／artifact distribution |
| 不可信程式內容 | WASM／sandbox plugin 生態、能力安全模型 |
| 多平台預建內容 | Cargo distribution、artifact registry、簽章與供應鏈工具 |
| 大型內容集合組合 | 既有 package manager／dependency resolver，而非從零寫一般求解器 |

研究必須先寫清楚 user story、信任邊界、可分發單位、相容承諾、平台矩陣與退出策略。

## 即使未來建立套件層，也保持的邊界

- Bevy 是正常 Cargo 相依，不是內容 graph 裡可任意替換的 engine package。
- runtime extension 最終仍是 Bevy plugin、asset 或經明確 adapter 進入 Bevy。
- 存檔相容依 schema owner 與 stable content ID，不由 package manager 的單一版本判斷。
- package metadata 不進每 tick、每 entity 或每 voxel 熱路徑。
- 權威程式碼與不可信內容的能力必須在載入前確定，不能以字串在執行途中索取主機權限。
- 動態 Rust ABI 不作預設；Rust compiler／crate ABI 不是穩定 plugin ABI。

## 現在明確不設計

- 自訂 registry protocol；
- 一般版本求解器；
- 內容定址 store；
- 動態 native library 掃描與卸載；
- WASM host capability surface；
- 另一份 build graph 或 runtime image；
- 自訂宣告／組合語言；
- 用 Godot package／scene 取代 Bevy runtime。

這些項目只有在 Phase 2 ADR 中重新取得授權後才回到路線圖。

## 可逆性

Phase 0 的選擇應為未來保留合理出口，但不支付完整平台成本：

- stable content ID 不綁 Rust type name；
- schema owner 不綁 Cargo version；
- content definitions 可從 Rust literal 移到 typed asset；
- domain registration API 不依官方內容；
- Bevy `Entity`／`Handle` 不進存檔；
- build artifact 可記錄 Cargo lock hash 供診斷。

這些是正常的長期資料衛生，不是預先實作包管理器。

## 驗收

- 第一個 demo 不含自訂 package resolver 或 runtime loader。
- 官方與測試 content plugin 可以由 Cargo feature／workspace 組合。
- 新增純資料內容不需要修改 Bevy lifecycle。
- 只有可重現的 Phase 1 失敗才能建立 Phase 2 ADR。
- 任何未來方案都先列出已評估的 upstream 選擇。

## 相關文件

- [決策 0016：以 Bevy 插件起步](../decisions/0016-stage-content-composition-on-bevy.md)
- [模組與內容組合](module-composition.md)
- [版本與相容性](versioning-and-compatibility.md)
- [開發策略](../foundations/development-strategy.md)
