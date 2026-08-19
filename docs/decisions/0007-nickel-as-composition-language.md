---
title: 採 Nickel 作為組合語言
status: accepted
type: decision
locale: zh-Hant
updated: 2026-08-19
source:
  - implementation-plan-2026-08-19
---

# 決策 0007：採 Nickel 作為組合語言

## 背景

遊戲設定檔需要描述模組、實現方式、相依性與建置政策，並支援基礎設定、整合包與使用者覆寫的組合。單純 TOML 或 JSON 足以攜帶資料，卻會把 schema 驗證、預設值、參數化與合併規則推回專案自行實作。

Nickel 以記錄、函式、合約與合併為核心，並能匯出 JSON。這些能力正好對應宣告式遊戲組合，但 Nickel 不應成為遊戲執行期資料模型或熱路徑依賴。

## 決策

1. `module.ncl` 與 `game.ncl` 採 Nickel；模組與遊戲設定檔分別套用專案提供的 `Module` 與 `GameProfile` 合約。
2. 基礎 profile、整合包與使用者覆寫使用 Nickel 的記錄合併、預設值與函式組合，不另造一套合併語言。
3. `lattice-compose` 是唯一 Nickel 求值邊界。CLI 與宿主共用同一 Rust 嵌入介面與同一組合測試，不各自呼叫不同語義的外部流程。
4. 求值結果立即轉成有版本的內部模型，並序列化為正規化 JSON。解析器、lock graph、膠水 crate 產生器與動態載入器只讀該模型。
5. Nickel 只存在於組合期；每 tick、每實體、每區塊取樣與渲染熱路徑不得求值 Nickel 或查詢 Nickel AST。
6. lock graph 保存正規化輸入、求值器版本、合約版本與內容雜湊，使組合結果可診斷與重建。
7. 發行的資料夾模組應攜帶已匯出的 `module.json`；開發模式可直接求值 `module.ncl`，但兩者必須得到相同的正規化模型。
8. 若 Nickel 工具鏈日後不再適合，只替換 `lattice-compose` 的來源語言前端；內部模型、解析器與執行期契約不隨之重寫。

## 護欄

- 專案合約是 manifest 語義的單一來源，不能在 CLI、宿主與文件中維護三份不同 schema。
- 匯出前必須完成合約驗證、完整求值與錯誤來源定位。
- 可匯出的模型只包含資料；函式、未求值值與不穩定的來源路徑不得進入 lock graph。
- 正規化規則必須固定物件鍵順序、數值與路徑表示，並以 golden test 驗證。
- 匯入範圍、來源根目錄與資源限制由組合器政策控制，不把任意主機存取當成 manifest 能力。

## 結果

- manifest 可重用 Nickel 現成的合約、合併、函式與工具支援。
- 專案增加一項相對小眾的工具鏈依賴，錯誤呈現、求值成本與嵌入 API 升級都需要專門測試。
- 正規化 JSON 提供穩定的除錯、快取與替換邊界，但也要求明確定義 canonicalization。

## 被否決的方案

### 先以 TOML 過渡

它會讓早期 profile 疊加與 schema 邏輯形成另一套暫時 API，之後仍需遷移。既然組合模型已是 demo 的驗證對象，第一版直接使用 Nickel。

### 執行期直接保留 Nickel 值

這會把求值器、字串鍵與動態錯誤帶入熱路徑，也使替換前端變得困難。

### 自訂一門組合語言

目前沒有足以抵銷 parser、LSP、合約、合併與診斷維護成本的專案特有需求。

## 驗證

- 同一 `game.ncl` 在 CLI 與嵌入宿主得到位元一致的正規化 JSON。
- 合約錯誤能指出來源檔、欄位與期望語義。
- 合併順序與預設值有 golden test，輸入欄位重新排序不改變 lock graph 雜湊。
- 執行期核心的相依圖中不包含 Nickel 求值器。

