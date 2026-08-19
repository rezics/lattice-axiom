---
title: 待決問題
status: active
type: planning
locale: zh-Hant
updated: 2026-08-19
---

# 待決問題

這不是承諾清單，而是尚需以討論、原型、故障測試或量測回答的問題。已形成選擇的項目保留在「本輪已關閉」並連到 ADR；細節仍未驗證，不表示整個領域已完成。

## 本輪已關閉

- [x] 實作語言採 Rust，核心自研集中在模組、區塊、世界生成與持久化語義；見[決策 0005](../decisions/0005-rust-and-focused-build-boundary.md)。
- [x] demo 的最小可玩驗收已寫入[第一個 demo 路線圖](roadmap-first-demo.md)。
- [x] 宣告式組合語言採 Nickel，完整求值後收斂到正規化 JSON；見[決策 0007](../decisions/0007-nickel-as-composition-language.md)。
- [x] 靜態建置與資料夾動態載入共用同一 `ResolvedModuleGraph`；見[決策 0008](../decisions/0008-static-and-dynamic-realizations-share-one-graph.md)。
- [x] demo GPU 後端採 wgpu，以 `lattice-render` 門面與 headless 實現隔離；見[決策 0006](../decisions/0006-wgpu-behind-rendering-facade.md)。
- [x] demo 直接以 RocksDB 保存權威區塊快照，不先建立自訂 Region File；見[決策 0009](../decisions/0009-rocksdb-authoritative-world-snapshots.md)。

## 優先級 0：第一個 demo 的實作契約

- [ ] `Module`、`GameProfile` Nickel 合約與正規化 JSON schema 的最小欄位是什麼？
- [ ] canonicalization 如何固定集合排序、套件識別、路徑、數值與錯誤來源？
- [ ] `ResolvedModuleGraph`、lock graph、登錄表與穩定數值 ID 的 Rust 資料模型為何？
- [ ] 模組衝突、能力提供者與缺少精確來源時，CLI 的診斷與修復建議如何呈現？
- [ ] 區塊尺寸、調色盤格式與 RocksDB 值 envelope 的第一組基準參數為何？
- [ ] 自動存檔的耐久等級、可接受資料損失視窗與明確 `fsync` 邊界是什麼？
- [ ] RocksDB 鍵編碼、column family、cache、compression、batch 與 compaction 的最小配置為何？
- [ ] 渲染擷取採複製、雙緩衝或所有權移交，如何限制每幀配置與記憶體頻寬？
- [ ] demo 的 CPU、記憶體、磁碟、啟動、存檔與建置時間預算各是多少？

## 優先級 0：遊戲定義仍缺的部分

- [ ] 除了技術縱切外，玩家每分鐘的核心循環與情緒目標是什麼？
- [ ] 哪一項玩家價值必須依賴模組架構，而不只是工程上的優雅？
- [ ] 官方最小內容包除了方塊、兩個群系與一個實體外，還需要什麼才能形成可評估體驗？
- [ ] 第一個可公開測試版本是純單機、合作預備，還是創作工具預備？

## 優先級 1：套件、ABI 與生態治理

- [ ] 面向陌生第三方的長期程式碼邊界採穩定 C ABI、WASM 元件模型，還是兩者分層？
- [ ] 同工具鏈 dylib 的「相同」如何識別：rustc、target、profile、feature、依賴 ABI 與標準庫需記錄哪些？
- [ ] registry 的命名、發布、所有權轉移、撤銷與惡意套件處理由誰治理？
- [ ] 何時需要一般版本求解器；精確 lock 不足時應採何種範圍與衝突模型？
- [ ] 二進位快取、簽章、來源證明、授權與再散布政策如何進入 lock graph？
- [ ] v1 不卸載的前提下，開發期內容迭代與重啟時間如何控制？
- [ ] 模組擁有的持久 schema 在移除、升級或重新加入時如何保留、遷移與診斷？

## 優先級 1：量測「熱路徑已消除模組邊界」

- [ ] 建立純資料、靜態原生與動態原生同功能基準測試。
- [ ] 比較逐實體呼叫、批次查詢與編譯排程的成本。
- [ ] 證明官方與第三方內容進入同一執行表示後沒有來源查詢成本。
- [ ] 量測 LTO 的實際收益、動態函式指標的成本與載入時間，而不是由部署名稱推論。
- [ ] 亂序發現、增刪無關套件與靜／動切換時，ID、區塊雜湊與產物記錄是否穩定？

## 優先級 1：渲染邊界

- [ ] `RenderWorld` 的實際最小資料模型、生命週期與增量更新策略是什麼？
- [ ] 區塊網格重建、上傳、淘汰與 RocksDB 區塊生命週期如何背壓？
- [ ] 自訂 shader 的長期來源語言、IR、驗證、能力與快取鍵是什麼？
- [ ] 普通材質、Render Feature 與 Render Graph 擴充的權限邊界在哪裡？
- [ ] 動態模組第一次載入 shader／pipeline 時，如何編譯、快取與提供可讀進度？
- [ ] 第二渲染後端出現什麼真實需求時才值得加入？

## 優先級 1：世界持久化

- [ ] `ChunkCommit` 的原子範圍要包含哪些空間實體、續行工作與 receipt？
- [ ] 區塊快照採完整狀態、分層 blob 或週期性基線加 delta，哪種在實際 workload 下最合適？
- [ ] checksum 錯誤、磁碟滿、background compaction error 與雙重世界掛載時如何安全降級？
- [ ] checkpoint、增量備份、物件儲存、保留期與實際還原演練如何設計？
- [ ] `/regenerate --preserve-player` 如何分類 generated、player-owned 與 system-persistent 物件？
- [ ] 新舊 generation epoch 接縫需要哪些邊界輸入，誰負責生成 transition？
- [ ] 什麼查詢壓力才足以把 Persistent World Object 提升到 PostgreSQL？
- [ ] 未來跨 RocksDB／PostgreSQL 的 staged transaction、journal/outbox 與補償流程為何？

## 優先級 1：可組合世界生成

- [ ] 第一批兩個 `CaveTopologyProvider` 應選哪些受約束幾何圖、方向場或生長模型做對照？
- [ ] 地表與三維地下領地使用相同圖譜抽象，還是只共用所有權仲裁介面？
- [ ] 洞穴預算如何在主要拓撲、支洞、洞室、目的地與水文修正間換算？
- [ ] 地表群系、地下群系、深度帶與地質體如何共同影響淺層入口和深層洞穴？
- [ ] 跨規劃格與拓撲所有權域的入口契約需要保存哪些流體狀態？
- [ ] 第一個領地原型採用 Voronoi、冪圖、扭曲網格或其他候選中的哪兩種比較？
- [ ] Province、Region 與 Ecotope 的尺度、面積分布及形狀參數如何由實際旅行速度決定？
- [ ] `BiomePack` 世界份額由整合包、伺服器、玩家預設或內容作者中的誰治理？
- [ ] `BoundaryProfile` 如何避免 `N²` 配對，同時處理海岸、峭壁、地下河與其他特殊接縫？
- [ ] 水文、岩層、侵蝕與大型結構要使用多大的規劃格及 halo 才能有界查詢？
- [ ] 產物生成記錄採多大的空間與語義粒度，才能兼顧局部失效、儲存成本與跨世代邊界？
- [ ] 洞穴原型用哪些拓撲與玩法指標判斷優劣，而不是只比較截圖？

## 優先級 1：模型與物理資產

- [ ] glTF/GLB 是否承擔主要交換格式，哪些資料需要 extension 或 sidecar？
- [ ] 第一版只支援哪些跨模型能力：注視、接觸點、裝備、受擊區或其他？
- [ ] 語義角色、能力、物理場與互動語義如何分層與版本化？
- [ ] Blender add-on 最小功能是匯出、驗證，還是也包含物理預覽？
- [ ] 高解析度網格與低解析度 simulation cage 如何建立與對應？
- [ ] 物理 LOD 在不同硬體上是否會影響命中判定或遊戲公平性？

## 優先級 2：多人與供應鏈

- [ ] 客戶端與伺服器握手比較哪些權威能力子圖、lock 與 schema 狀態？
- [ ] 哪些純表現差異可以協商，哪些碰撞、生成或玩法能力必須完全一致？
- [ ] 原生第三方模組能否進入伺服器，是否允許自動下載？
- [ ] 伺服器如何取得、驗證與隔離舊生成器或歷史內容？
- [ ] 撤銷惡意套件時，已物化世界與可重現建置如何同時維持安全？

## 下一批實作型文件

只在開始相應原型時建立：

1. Nickel `Module`／`GameProfile` 合約與正規化 JSON schema。
2. `ResolvedModuleGraph`、lock graph 與數值 ID 資料模型。
3. RocksDB 鍵格式、耐久政策與故障注入測試計畫。
4. 渲染擷取垂直切片與效能預算。
5. 二維多尺度領地與大量合成群系的可重複原型報告。
6. 一個根域、兩個子域拓撲提供者與共享入口的三維洞穴原型報告。

## 相關文件

- [第一個 demo 路線圖](roadmap-first-demo.md)
- [套件管理、Nickel 組合與雙實現路徑](../architecture/package-management.md)
- [世界持久化與 RocksDB World Store](../architecture/world-persistence.md)
- [渲染架構與擴充邊界](../architecture/rendering.md)
- [可組合世界生成架構](../architecture/world-generation.md)
- [可組合洞穴生成架構](../architecture/cave-generation.md)
- [詞彙表](../foundations/glossary.md)
