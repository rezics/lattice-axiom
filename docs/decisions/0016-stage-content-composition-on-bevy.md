---
title: 以 Bevy 插件起步並延後自訂套件執行期
status: accepted
type: decision
updated: 2026-08-19
---

# 決策 0016：以 Bevy 插件起步並延後自訂套件執行期

## 背景

Lattice Axiom 希望官方與第三方內容走同一條公開路徑，但這個產品目標不要求第一天就建立宣告語言、一般版本求解器、內容 store、動態 Rust ABI、WASM 沙箱或自訂套件執行期。先做這些機制會同時創造包管理器與遊戲引擎兩個產品，卻沒有真實內容證明邊界正確。

Bevy 已以 `Plugin`／`PluginGroup`、typed asset、reflection、scene 與 Cargo feature 提供足以驗證內容組合的基線。

## 決策

1. 第一個可玩原型以 Cargo workspace、`Cargo.lock`、Bevy `Plugin`／`PluginGroup` 與 typed data asset 組合程式和內容。
2. 官方內容實作為獨立 plugin／asset 集合，透過與測試內容及未來社群內容相同的公開註冊函式加入；不得直接修改只對官方內容可見的全域表。
3. 內容註冊直接使用 Bevy 的 App 擴充點以及 Lattice Axiom 真正需要的少量 domain registry。專案不複製 Bevy plugin lifecycle，也不發明另一套通用 module lifecycle。
4. Bevy 與 Rust crate 版本由 Cargo 解決。存檔 schema、內容 ID、生成器修訂與資產格式各自版本化，不由假想的全域 engine version 或自訂 package graph 代理。
5. 自訂宣告語言、一般 package resolver、動態 Rust library ABI、執行期卸載、WASM 市場和 registry 不在首個 demo 或引擎整合路線內。
6. 只有在可玩原型與第二個獨立內容集合都完成後，若發佈、組合或安全需求仍無法由 Cargo、Bevy plugin、typed asset 與既有生態工具滿足，才可啟動一份新的套件系統研究；研究仍必須先比較 upstream。
7. 即使未來引入內容套件圖，它只描述內容與產品契約，不擁有或重新版本解析 Bevy 本身。

## 啟動後續研究的證據

至少要有一項真實工作流失敗：

- 非開發者無法分發或組合內容；
- 伺服器需要在不重新編譯客戶端的情況下選擇權威內容；
- 不可信內容需要明確沙箱；
- 大量內容集合使 Cargo／靜態 plugin 組合無法達到可接受的建置或營運成本。

提案必須包含使用者、威脅模型、相容性承諾、最小原型和退出策略。「未來可能需要 mods」不是證據。

## 結果

- 第一個 demo 不再被自製包管理器、動態 ABI 或組合語言阻塞。
- 官方內容仍可驗證公開擴充路徑，但最初的可分發單位是正常 Rust crate 與資料資產。
- 動態第三方內容暫時需要重新建置；這是有意接受的早期限制。
- 未來的套件方案會從已存在的內容、作者與部署問題反推，而不是從抽象圖模型反推。

## 驗證

- 官方內容與一個最小測試內容 plugin 都只使用公開 API 註冊方塊、物品或生成內容。
- 移除官方內容 plugin 後，runtime 仍能啟動並運行測試內容。
- 新增第二內容集合不需要改動 Bevy 啟動流程或 Lattice Axiom 核心私有表。
- 首個 demo 的建置與啟動路徑中不存在自訂組合語言、動態 library 掃描或自訂版本求解器。

## 相關文件

- [模組與內容組合](../architecture/module-composition.md)
- [內容分發的分階段邊界](../architecture/package-management.md)
- [第一個可玩 demo 路線圖](../planning/roadmap-first-demo.md)
