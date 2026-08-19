---
title: 待決問題
status: active
type: planning
updated: 2026-08-19
---

# 待決問題

本頁只保留會改變產品、可玩原型或長期資料邊界的問題。Bevy 已有答案的通用引擎問題不再列為 Lattice Axiom 的自研工作。

## 已關閉的方向

- [x] 遊戲引擎採 Bevy；預設使用完整上游能力，見[決策 0014](../decisions/0014-adopt-bevy-upstream-first.md)。
- [x] 世界座標採 Bevy 原生右手 Y-up，水平面 `x-z`，見[決策 0015](../decisions/0015-bevy-native-y-up-world-coordinates.md)。
- [x] 第一階段使用 Cargo + Bevy plugin + typed asset；不建立自訂 package runtime，見[決策 0016](../decisions/0016-stage-content-composition-on-bevy.md)。
- [x] 已物化世界以 RocksDB 完整 snapshot 為權威，見[決策 0009](../decisions/0009-rocksdb-authoritative-world-snapshots.md)。
- [x] 相容性不由單一全域版本決定，見[決策 0003](../decisions/0003-no-global-version-switch.md)。
- [x] Godot 只作有觸發條件的工具鏈對照，不作第二 runtime。

## P0：第一個可玩 spike

### 玩家與操作

- [ ] 第一版玩家 movement 要驗收哪些行為：走、跳、坡面、台階、飛行／noclip debug？
- [ ] break／place 的距離、冷卻、選取回饋與錯誤回饋是什麼？
- [ ] demo 的最小「好玩／可理解」標準，除了技術循環還需要哪個玩家目標？
- [ ] 支援的第一個 OS／GPU／輸入裝置矩陣是什麼？

### Bevy 與生態 spike

- [ ] 實作時鎖定哪個 Bevy `0.19.x` patch 和 feature 集？
- [ ] `bevy_voxel_world` 能否直接承擔權威 chunk，還是只適合作 presentation／streaming adapter？
- [ ] 它的 chunk 尺寸、voxel type、material、raycast 和 async meshing 假設有哪些不可設定限制？
- [ ] Avian 的 Bevy 0.19 對應版本能否支援 character movement、voxel collider 更新與 FixedUpdate 預算？
- [ ] Bevy 原生 input 是否已足夠；Leafwing Input Manager 具體省下哪些 action mapping／test injection 工作？
- [ ] 標準 Bevy UI／diagnostics 是否足夠完成 crosshair、保存狀態和 debug overlay？

### 量測

- [ ] 目標硬體與最低可接受 frame／fixed-tick budget 是什麼？
- [ ] 需要同時 active、resident、visible 的 chunk 數是多少？
- [ ] edit-to-visible、collider rebuild、chunk load 和 save P95 預算是多少？
- [ ] queue depth、in-flight bytes、RAM／VRAM 高水位的第一版上限是多少？

P0 的回答必須來自可玩的 D1 build，不由文檔預先猜定。

## P1：權威世界與持久化

- [ ] B1 adoption report 後，權威 chunk 資料型別最小需要哪些欄位？
- [ ] chunk 尺寸、palette encoding 與 Y-up key encoding 如何選擇？
- [ ] stable block ID 的 namespace、alias、missing-content policy 是什麼？
- [ ] `ChunkRevision` 在 voxel、persistent entity、scheduled work 之間是一個 revision 還是分域 revision？
- [ ] active／resident／persistence radius 的狀態機和 backpressure 如何定義？
- [ ] RocksDB column family、prefix extractor、compression、cache 和 compaction 的 benchmark 結果？
- [ ] queued／written／durable／checkpointed 中，UI 的「已保存」對應哪一級？
- [ ] 正常 shutdown deadline 與 timeout UX 是什麼？
- [ ] 第一個 schema envelope／migration fixture 的實際 encoding 採何種 serde format？
- [ ] checkpoint 頻率、保留數、磁碟預算和 restore-time 目標？

## P1：最小世界生成

- [ ] 第一個 terrain spike 是否先使用 voxel plugin 的 generator hook，還是直接接 `WorldgenPlugin`？
- [ ] demo 的兩個 placeholder biome 要證明哪兩種真正不同的生成能力？
- [ ] world seed、`WorldgenConfig` 和 generator revision 如何 canonicalize／hash？
- [ ] terrain height、soil、cave／void 的最小 dependency graph 是什麼？
- [ ] 新舊 generation epoch 的邊界在 demo 中需要哪種最小驗收？
- [ ] 完整領地架構的第一個二維 `(x, z)` prototype 何時開始，不能阻塞哪個 demo gate？

## P1：公開內容 API

- [ ] `ContentCatalog` 的最小 definition：block、item、biome 各有哪些必填欄位？
- [ ] registration 直接用 Rust builder、typed asset，或兩者如何分工？
- [ ] validation 在 plugin build、Startup 或 loading state 的哪個階段完成？
- [ ] runtime compact ID 如何由 stable ID 建立，chunk palette 如何保存 mapping？
- [ ] `TestContentPlugin` 應刻意與官方內容有什麼差異，才能真正反證 API 偏見？
- [ ] content 移除時，placeholder、read-only recovery、migration 與拒絕載入如何選擇？

## P2：Bevy 升級與依賴治理

- [ ] Bevy minor 升級 cadence 是每版、隔版，還是只在需要 feature／fix 時？
- [ ] ecosystem plugin compatibility matrix 由誰、在哪份機器可讀資料維護？
- [ ] license／notice、SBOM、security advisory 與 source pin 如何自動檢查？
- [ ] upstream patch 何時用 git dependency，何時等待 release，何時維持最小 fork？
- [ ] 可玩 smoke、save fixture、asset fixture 和 performance benchmark 哪些是升級必過 gate？

## P2：資產與作者工具

- [ ] Blender → glTF → Bevy 能否保存 demo 需要的 socket、collider 與 semantic tags？
- [ ] 哪些語義真的需要 typed sidecar，而不是 node naming／glTF extras？
- [ ] Bevy hot reload 對 collision／gameplay asset 的安全邊界如何實作？
- [ ] 何種真實工作流會觸發 Godot toolchain spike？
- [ ] 若觸發，Godot 對照要節省多少作者時間，資料 round-trip 要保留哪些語義？

## 只有觸發後才重新開啟

以下不是目前待實作項目：

- 自訂 package manager／registry／resolver；
- dynamic Rust ABI、WASM host 或執行期卸載；
- 自研 renderer、physics、ECS、schedule、asset server；
- 第二 runtime 或 Godot runtime integration；
- multiplayer rollback／lockstep；
- 完整 visual world editor。

每一項都需要真實 playable／author workflow failure、新 ADR 和 upstream 調查才能重啟。

## 下一批要產生的證據

1. B0 Bevy client／headless smoke build。
2. B1 upstream voxel／physics 可玩 build 與 adoption report。
3. 一份含 frame、chunk、task、memory、collision 指標的 profiling capture。
4. D2 RocksDB crash／revision-race 測試結果。
5. Official／TestContent 亂序註冊和存檔 fixture。

## 相關文件

- [Bevy 執行期整合路線](roadmap-game-engine.md)
- [第一個可玩 demo 路線圖](roadmap-first-demo.md)
- [技術棧](../foundations/technology-stack.md)
- [渲染與物理候選](../research/renderer-physics-landscape.md)
- [Godot 工具鏈對照](../research/godot-toolchain-comparison.md)
