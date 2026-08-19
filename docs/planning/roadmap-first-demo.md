---
title: 第一個可玩 demo 路線圖
status: active
type: planning
updated: 2026-08-19
---

# 第一個可玩 demo 路線圖

## 驗收目標

> 玩家在可延伸的體素世界中行走，穿過兩個群系與一條可見邊界，破壞並放置一種方塊；世界能保存與載入。官方內容是獨立模組，畫面可以只是純色方塊。

這份路線圖驗證可組合世界平台的最小縱切，不是完整遊戲發行計畫。每一步必須先通過自動驗收才進入下一步；時間是單人加 AI 的估算區間，不是承諾。

## 已固定的基線

- Rust + Cargo workspace；
- Nickel 組合後直接建立 Rust `CompositionSpec`；
- Rust package kernel 解析同一 `LockedGameGraph`，支援靜態建置與資料夾動態載入；
- wgpu 藏在渲染門面後，另有 headless 實現；
- RocksDB 從 demo 起就是生產 World Store，另有記憶體測試實現；
- 世界空間採用右手笛卡兒座標系，`XY` 為水平面、`+Z` 向上，長度單位為公尺；
- 官方內容只使用公開模組路徑。

## 里程碑

| 步驟 | 產物 | 出場驗收 | 估算 | 狀態 |
| --- | --- | --- | --- | --- |
| 0 | 決策、架構與本路線圖轉正 | 互鏈一致；暫時對話與草稿完成吸收並刪除 | 1–2 天 | 完成 |
| 1 | workspace、空宿主、渲染門面三件套與立方體 | headless CI 通過；核心／內容無法依賴 wgpu | 約 1 週 | 待開始 |
| 2 | `package.ncl` → `CompositionSpec` → `LockedGameGraph`／lock；穩定數值 ID；CLI 雛形 | 無 JSON 中間檔；亂序註冊不改 ID；熱路徑只見 `BlockId` 等數值型別 | 1–1.5 週 | 未開始 |
| 3 | 區塊／調色盤、串流、挖放、AABB 物理、RocksDB `WorldStorage`、egui | 陌生人可玩 3 分鐘；重開後坑仍在；kill 測試無半提交區塊 | 2–4 週 | 未開始 |
| 4 | 與主迴圈分離的領地生成 PNG 預覽 | 同種子同像素；仲裁與註冊順序無關 | 2–3 週 | 未開始 |
| 5 | `terrain.base`、`terrain.boundary`、`surface.content` 接入世界並保存產物記錄 | 邊界連續；種子可重現；並行生成接縫一致 | 2–3 週 | 未開始 |
| 6 | 第二內容包走靜態路徑 | 關掉官方包仍可啟動；無 `if pack == official` | 1–2 週 | 未開始 |
| 7 | 同一第二內容包以預編譯 dylib 放入 `packages/` | 靜／動實現得到相同 ID 與區塊雜湊；ABI 不符診斷可讀 | 1–2 週 | 未開始 |
| 8 | 最小混合洞穴與共享入口契約 | 以可達性與入口契約測試，不以截圖判定 | 2–3 週 | 未開始 |

走到第 8 步約需 5–6 個月，實際速度以每個出場驗收為準。

## 每一步的邊界

### 1：空宿主與渲染門面

建立 `latticeaxiom-core`、`latticeaxiom-render`、`latticeaxiom-render-wgpu`、`latticeaxiom-render-headless` 與 `latticeaxiom-demo`。能開窗、控制相機、上傳並畫一個立方體；headless 只驗證 `RenderWorld`，不建立 GPU。

不做區塊、模組與物理。若 crate 邊界或編譯時間已不合理，在這一步修正。

### 2：薄組合核心

加入 `latticeaxiom-compose`、`latticeaxiom-packages`、`latticeaxiom-modules` 與 `latticeaxiom-cli`：`latticeaxiom.lib` 合約、Nickel 完整求值、直接 serde 轉換、精確來源 lock、能力解析、`LockedGameGraph`、穩定數值 ID 與靜態膠水 crate 產生。

不做 registry、一般 SemVer 求解器、二進位快取、WASM 或卸載。

### 3：第一次可玩

實作區塊調色盤、玩家周圍串流、greedy meshing、AABB／重力／射線、挖放方塊、簡單高度圖與 egui 指標。第一次生成的區塊連同 provenance 直接提交 RocksDB；玩家修改、空間實體與續行狀態走同一 `WorldStorage` 契約。

除了正常重啟，還要在 write batch、checkpoint 與載入階段注入程序終止。這一步不做光照傳播、水體、美術資產、合成或多人。

### 4–5：領地生成進入世界

先以獨立程式比較兩種二維 Territory Atlas 候選，使用兩個真群系與大量合成群系測仲裁、索引與快取。原型通過後只開三個生成通道，並為實體化區塊保存實際生產者與輸入雜湊。

不做水文、侵蝕、大型洞穴網路或全部文檔通道。

### 6–7：模組架構期末考

第二內容包提供新方塊、新群系與可選盒子實體，只依賴公開 API。先靜態建置，再以相同來源產出動態函式庫；兩路必須收斂到同一解析圖與註冊結果。

不為動態路徑加入官方特例，不支援熱卸載或跨工具鏈 ABI。

### 8：洞穴第一口

建立一個根域預設拓撲與一個可由地下群系接管的子域提供者，透過共享入口契約銜接。驗收最小淨空、入口滿足與可達性，不追求完整地質或視覺品質。

## 刻意延後

- registry、一般版本求解器、簽章、撤銷與二進位快取；
- WASM、穩定第三方 ABI 與運行期卸載；
- 多人、帳號、經濟、PostgreSQL 與物件儲存；
- Rapier、柔體、完整 glTF 角色與第二渲染後端；
- 通用 shader IR、第三方 Render Graph 擴充與 PBR 美術；
- 水文、侵蝕、巨型結構與完整洞穴生態。

## 相關文件

- [一人與 AI 協作的開發策略](../foundations/development-strategy.md)
- [Nickel 驅動的套件系統與雙實現路徑](../architecture/package-management.md)
- [世界持久化與 RocksDB](../architecture/world-persistence.md)
- [渲染架構與擴充邊界](../architecture/rendering.md)
- [待決問題](open-questions.md)
