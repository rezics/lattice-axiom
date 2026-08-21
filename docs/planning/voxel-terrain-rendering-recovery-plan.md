---
title: 體素地形渲染與區塊流送修復計畫
status: active
type: plan
updated: 2026-08-22
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0015-bevy-native-y-up-world-coordinates.md
  - ../decisions/0026-freeze-first-demo-performance-budgets.md
  - ../decisions/0027-freeze-authoritative-world-and-persistence-contract.md
  - ../decisions/0029-freeze-render-capability-and-provider-contract.md
---

# 體素地形渲染與區塊流送修復計畫

## 範圍與成熟度

本頁針對 `lattice-axiom-demo` production host 在診斷基線上出現的破面、視角改變後地形消失、移動時卸載／重建抖動，以及由此暴露的 mesh presentation、streaming、task、collider 與 terrain material 缺口，給出可依序落地的修復計畫。

診斷基線是 implementation commit `9e1fc9ddb67eba0646c47bbdf86ba65b99c10113`。外部對照是 Riverbed commit [`00005a3a6b8f181ca2641f046434a4397a7c4dca`](https://github.com/Inspirateur/riverbed/tree/00005a3a6b8f181ca2641f046434a4397a7c4dca)。Riverbed 是 MIT 專案；本計畫只吸收可驗證的設計思路，不把它的實作或 README 性能主張提升為 Lattice Axiom 的承諾。

本頁是 `active` implementation plan：P0–P2 已在 production host 落地，剩餘從 P3 起。它不新增 render provider、改寫 accepted performance profile，或授權自研 Bevy 以外的 renderer／scheduler／task runtime。若後續證據要求改變 accepted 邊界，必須另建 ADR。開始頁／`LaunchIntentV1` 進程切換不在本頁範圍，見[開始頁與安全生命週期](../architecture/world-lifecycle-and-start-ui.md)。

## 落地（P0–P2）

下列是 `lattice-axiom-demo` 已合併的 correctness blockers，不是 ADR 0026 量測，也不是 D2／D4 路線圖出場。

| 階段 | 實作 | 證據 |
| --- | --- | --- |
| P0 | `f2457b1` `test(voxel-mesh): lock outward winding and halo culling` | `geometry` indexed winding；六向 halo 同時覆蓋 `visible_faces` 與 `greedy_quads` |
| P1／P2 | `b686d3f` `fix(engine): present halo meshes and reconcile interest once` | adapter 只做 `MeshBuffer → Mesh`；`mesh_from_occupied`／`FACES` 已刪；`FixedUpdate` 只做 collider safety；`FixedPostUpdate` 以最終 pose reconcile 一次；headless：yaw／pitch 不變、每 tick 一次 reconcile、chunk 內移動不 distance-evict、折返 retain、sticky look-ahead |

診斷基線 `9e1fc9d` 的三個畫面缺陷因此在 production host 上關閉。下面「結論／證據與根因」保留該基線的定位，供回歸對照。下一步按原順序：P3 bounded Bevy task-pool → P5 collider 合併 → P6 deterministic terrain materials。P7 仍禁止提前。

## 結論

診斷基線 `9e1fc9d` 的畫面不是單一 shader 或設定問題，而是三個已定位的實作缺陷和數個後續規模問題疊加：

1. client 臨時 mesh adapter 的六個面都使用與 outward normal 相反的 triangle winding；Bevy 預設 back-face culling 因而剔除真正外表面。這直接解釋破面、巨大斜三角與轉動視角後幾何消失。
2. production derived path 已從 chunk＋一圈 halo 產生正確 renderer-independent geometry，卻只保留 receipt／source／face set；client 又從 `OccupiedCell` 建造第二份不含 halo 的 mesh。正確、已測試的 mesh 成果因此沒有進入 presentation。
3. 完整 streaming reconciliation 同時在移動前的 `FixedUpdate` 與移動後的 `FixedPostUpdate` 執行；每次都重算 look-ahead、更新 anchor 並立即驅逐 desired set 外的 chunk。這會在同一 fixed tick 內撤銷再建立前方工作，造成移動時的 unload／remesh 抖動。

修復順序必須是：

```text
幾何契約測試
  → 貫通既有 halo-aware derived mesh
  → 每 fixed tick 只 reconcile 一次 interest
  → bounded Bevy task-pool execution
  → collider 合併
  → deterministic terrain materials
  → 量測後才做 LOD／region batching／occlusion
```

在前兩項完成前，不得以關閉 back-face culling、加入 `NoFrustumCulling`、擴大視距或提高無界 queue 容量掩蓋錯誤。

## 證據與根因

### A. 六面 winding 與 normal 相反

`crates/latticeaxiom-engine/src/host/chunk_mesh.rs` 的 `mesh_from_occupied` 對每個 quad 固定發出：

```text
0, 1, 2, 0, 2, 3
```

但 `FACES` 中的 corner order 是從方塊內部看才是 counter-clockwise。例如 `+X` 面首個 triangle 的叉積指向 `-X`，而宣告 normal 是 `+X`。其餘五面有相同反轉。`StandardMaterial` 保持預設 back-face culling 時，外表面被剔除，觀察者只會在特定角度看到錯誤背面。

置信度：**確定**。這可由純 CPU cross／dot 測試重現，不需要 GPU 或 screenshot。

### B. 正確 geometry 被 production host 丟棄

`crates/latticeaxiom-engine/src/host/spine.rs` 的 `complete_derived` 呼叫 `latticeaxiom-voxel-mesh::visible_faces`，取得帶 `MeshReceipt` 的 `ChunkMesh`。但 `HostDerivedMesh`／`ChunkDerived` 只保存 receipt、source、face set 與 occupied cells，沒有保存 `MeshBuffer`。presentation delta 也只攜帶 collider、origin 與 occupied cells。

`chunk_mesh.rs` 隨後從 occupied cells 建立另一份 mesh，形成兩套互不相同的 truth：

```text
chunk + halo → latticeaxiom-voxel-mesh → receipt / face set（geometry 丟棄）
occupied cells → host/chunk_mesh.rs → Bevy Mesh（實際畫面）
```

第二條路徑沒有 halo，因此不能消除跨 chunk 內部面；也繞過 `latticeaxiom-voxel-mesh` 已有的 Y-up face axes、outward CCW winding、stable group order、bounds 與 greedy mesher。

置信度：**確定**。這是資料流直接可見的結構性缺陷。

### C. 同一 fixed tick 重複 reconcile streaming

`ProductionHostPlugin` 在 `FixedUpdate` 與 `FixedPostUpdate` 都排入：

```text
sync_player_pose → sync_chunk_stream → sync_chunk_colliders
```

`sync_interest_inner` 又以目前 translation 減去 `stream_anchor_xz` 算 look-ahead，然後立即覆寫 anchor。移動前呼叫可能得到零方向並移除前方預載列；移動後呼叫再得到非零方向並重新申請它。`evict_unwanted` 沒有 retain ring、minimum residency 或 grace ticks，desired set 外的 clean chunk 立即被驅逐。

置信度：**高**。需要 schedule-level regression fixture 量化每 tick 的 admission／eviction／remesh 次數，但現有控制流已足以證明重複 mutation 存在。

### D. 同步派生工作與 per-voxel collider 會放大卡頓

`drain_derived` 在持有 `ProductionSpineInner` mutex 時迴圈 dispatch 並同步完成全部可用 mesh／collider work。`compound_collider` 又為每個 occupied voxel 建立一個 cuboid。它們不是視角旋轉消失的第一根因，卻會把 chunk admission、edit 與移動邊界轉成 frame／fixed-tick burst。

置信度：**確定的成本形狀，尚待 profile 定量**。

### E. presentation semantics 仍是 debug baseline

`HostVoxel::face` 把所有 collision-occupied voxel 映射為 `MeshGroup::Opaque`、palette-index merge key 與 `FaceOcclusion::Full`；client 使用單一 `StandardMaterial` 加 hard-coded vertex color。fluid palette、cutout、translucent、emissive、face-specific texture、UV、AO 與正常 water semantics 尚未進入 production presentation。

這不造成 winding bug，但即使修好幾何，畫面仍只會是 debug-quality voxel world。

## Riverbed 對照結論

Riverbed 的價值在於展示一條已串通的 voxel presentation path，而不是提供可直接搬入的完整架構。

### 值得吸收

| Riverbed 機制 | 可吸收的原則 | Lattice Axiom 對應方式 |
| --- | --- | --- |
| chunk 使用 `interior + 1-voxel padding` | mesher 使用封閉 immutable input，不在 hot loop 跨 chunk 查詢 | 保留 `VoxelRuntime` 已捕獲的 chunk＋halo，直接呈現其 derived geometry |
| 邊界修改同步 padding 並通知兩側 remesh | boundary revision 必須雙向失效 | 沿用現有 neighbor revisions／halo invalidation，新增 presentation regression |
| mesh order 按玩家距離選最近項 | near／active work 優先於遠端 prefetch | 將 distance／core／retain／prefetch class 編入既有 bounded derived priority |
| main thread 對同 chunk／face 的完成結果取最新 | 合併重複 presentation apply | 以 `(chunk, kind, revision)` 做 bounded last-write-wins，仍由 receipt 拒絕 stale result |
| binary greedy meshing | 大幅減少 quad 與 upload bytes | 使用本專案既有 `GreedyMesher`，不新增第二個 mesher dependency |
| texture array、face-specific layer、nearest sampler | voxel material 應是 deterministic layered presentation | 由 stable content ID 編譯 layer table，按 `MeshGroup` 建立 material／pass |
| packed vertex attributes | 大視距下可降低 CPU→GPU 與 VRAM 壓力 | 先量測普通 Bevy mesh baseline；確認 vertex/upload 是瓶頸後再專項採用 |

### 不直接搬入

- 無界 `crossbeam` channels；
- 一個 detached 常駐 task 串行處理所有 mesh；
- startup busy wait；
- 缺少 chunk revision／neighbor receipt 的結果套用；
- `NoFrustumCulling` 加自建 per-face culling 作為預設；
- 只支援粗略 LOD 2 的 first-nonempty downsample；
- 一個 chunk 拆六個 face entities 的永久 draw architecture；
- wildcard dependencies、production `unwrap`／`panic` 與無 bound loop。

Riverbed README 的 2 km render-distance 敘述沒有附本專案 performance profile 所要求的 reference host、frame histogram、RAM／VRAM、queue high-water 或十分鐘 traversal evidence，因此只能視為研究線索。

## 不變實作約束

每個階段都必須保持：

- Bevy 是唯一 App、ECS、scheduler、renderer、assets、tasks、window 與 diagnostics runtime；
- authoritative chunk／world hash 不依賴 render success；
- GPU mesh、material、entity、visibility 與 collider 都是可重建 presentation；
- output-affecting order 使用 stable group／face／chunk／revision key；
- stale mesh／collider completion 不得覆蓋新 revision；
- hot path 不新增無界 allocation、queue、blocking I/O、全域鎖或 project-owned thread runtime；
- CI 保持 headless，不要求 window 或 GPU；GPU timing 由 reference-host runner 產生；
- performance gate 使用 [ADR 0026](../decisions/0026-freeze-first-demo-performance-budgets.md)，不以本頁另造較寬鬆 threshold。

## 可執行工作分解

### P0：建立會失敗的幾何與 schedule 契約測試

建議 commit：

```text
test(voxel-render): lock derived presentation invariants
```

修改範圍：

- `crates/latticeaxiom-engine/src/host/chunk_mesh.rs` 的 CPU-only adapter tests；
- `crates/latticeaxiom-voxel-mesh/src/geometry.rs`／`mesher.rs` 的現有 winding／halo corpus；
- `crates/latticeaxiom-engine/tests/headless_host.rs` 的 production-host schedule fixture。

新增門禁：

1. 所有 face 的兩個 triangles 都滿足 `dot(cross(edge_a, edge_b), outward_normal) > 0`。
2. renderer adapter 的 quad、vertex、index、group 與 bounds 數量等於來源 `MeshBuffer`。
3. 六個 halo 方向各只消除一個對應 boundary face。
4. 相機只改 yaw／pitch 時，resident set、chunk lifecycle、mesh receipt、entity count 與 queue counters 不變。
5. 同一 fixed tick 最多有一次完整 interest reconciliation。

P0 先合併測試，不以修改 material culling 讓錯誤測試假通過。

### P1：讓 halo-aware derived geometry 成為唯一 mesh truth

建議 commit：

```text
fix(voxel-render): present halo-aware derived chunk meshes
```

資料模型：

1. `HostDerivedMesh` 保存 `MeshReceipt`、`MeshSource` 與 `MeshBuffer<MergeKey>`。
2. `ChunkDerived` 保存最後 accepted mesh geometry／bounds／revision；`faces` 若只供 diagnostics，可從 geometry 推導或保留為快照，不能取代 geometry。
3. `PresentationDelta` 分離 `mesh_update`、`collider_update` 與 `removal`，避免 collider readiness 成為 mesh 傳遞的隱式條件。
4. `OccupiedCell` 只供 collider derivation；render path 不再從 occupied cells 重建 topology。

派生與 adapter：

1. production mesh job 改用既有 `GreedyMesher`／reusable scratch，保留 runtime 所發出的 source identity。
2. Bevy adapter 只做 `MeshBuffer → Mesh`：使用 `Quad::positions(face)`、`Face::quad_normals()`、`Face::quad_indices()` 與 geometry bounds。
3. 按 `MeshGroup` 保留分組邊界；P1 即使只有 opaque，也不能把未來 cutout／translucent／emissive 再壓回單一不透明語義。
4. 新 Bevy asset 完整建立後才替換 entity 上的舊 handle。pending、cancelled、stale、apply failure 都保留舊 presentation。
5. empty accepted geometry 才移除 mesh；chunk eviction 才移除整個 presentation entity。
6. 使用有效 Bevy `Aabb` 與原生 frustum culling；保留正常 back-face culling。
7. 刪除 `mesh_from_occupied`、`neighbor_occupied`、`FaceSpec` 與 `FACES`。

P1 出場條件：

- P0 全部通過；
- 畫面不再出現 inverted faces／巨大斜三角；
- 原地 360° rotation 不改變 world／stream／mesh state；
- chunk 邊界沒有因忽略 halo 產生的重複內部面；
- production client 不存在第二套 voxel face topology table。

### P2：每 fixed tick 只進行一次 interest reconciliation

建議 commit：

```text
fix(voxel-stream): reconcile player interest once per fixed tick
```

調度：

1. `FixedUpdate` 移動前只執行目前 player capsule 所需的 collider safety gate，不執行完整 generate／evict／presentation reconciliation。
2. 玩家 movement、Avian writeback 與 edit evaluation 完成後，在 `FixedPostUpdate` 記錄最終 pose。
3. 每個 fixed tick 以該最終 pose 執行一次 `reconcile_interest`，再消費一次 presentation delta。
4. camera `Update` 只同步 eye transform；camera rotation 不寫 chunk interest。

interest model：

```text
core      必須 resident／active，永不作距離型 victim
retain    core 外的滯回範圍，受 minimum residency／grace ticks 保護
prefetch  移動方向前方的低優先工作，queue 到 soft high-water 先停止
pins      authoritative work、player／entity、dirty／persistence 狀態的明文 pin
```

- 只在跨 chunk、方向 priority class 改變、edit invalidation、pin 改變或 completion 到達時重算集合。
- look-ahead 使用最後有效 movement direction／velocity bucket 與明確 expiry，不因一次零 delta 立即撤銷。
- admission、eviction、cancellation 與 coalescing 使用 stable chunk key 作 tie-break。
- hydrate 不等於永久 player edit。審計 `publish_hydrated` 對 `edited` 的寫入，將「從 storage 載入」「session 已修改」「尚未 durable」拆成不同狀態。
- dirty data 的 pin／writeback／eviction 必須服從 ADR 0027；不得為維持視距 silent drop dirty state。

P2 出場條件：

- 同一 chunk 內持續移動不產生 distance-only admission／eviction／remesh；
- 跨一個 boundary 後立刻折返，retain chunk 不重新 generate／hydrate；
- 持續前進時前方 core 在後方 retain 被驅逐前 active；
- 180° camera turn 不重算 authoritative interest；若未來 visibility priority 讀 camera，亦不能驅逐 core；
- request flood 到四倍 hard cap 時仍能 coalesce、cancel stale、backpressure 並有界 shutdown。

### P3：把 mesh／collider 工作移出 frame-critical mutex

建議拆成兩個 commits：

```text
refactor(voxel-runtime): separate derived dispatch from completion apply
perf(voxel-runtime): execute bounded derived work on Bevy task pools
```

執行模型：

1. 在短暫持有 spine lock 時 dispatch owned immutable `DerivedInput`，立即釋放 lock。
2. 使用 Bevy task pools 建立有限數量的短生命週期 jobs；不建立 detached infinite worker。
3. 每個 job 攜帶 world、chunk、kind、revision、neighbor revisions、owner、retained bytes 與 cancellation identity。
4. 每個 worker lane 可重用 `GreedyMesher` scratch；world／ECS／GPU handle 不進 task input。
5. completion 回到 main world 後先由 runtime receipt 驗證；accepted result 才進 presentation apply queue。
6. `(chunk, kind)` pending work 採 last-write-wins；in-flight 舊 revision 取消或完成後 stale-reject。
7. priority class 依 core、collider safety、edit-to-visible、retain、prefetch 排序，同 class 再依 distance 與 stable chunk key。

必須直接使用 ADR 0026 hard caps：

- mesh queue：`128 jobs / 128 MiB`；
- collider queue：`64 jobs / 64 MiB`；
- combined：`512 jobs / 384 MiB`；
- CPU-heavy concurrency：最多 `max(1, physical_core_count - 2)`；
- main-world completion apply：每 render frame `<= 2 ms` 且 `<= 16 MiB`；
- queue `75%` 時停止遠端 prefetch、coalesce pending revision 並降低非關鍵 priority。

P3 出場條件：

- fixed schedule 不再同步 drain 整條 derived queue；
- stale／cancelled completion 不建立或替換 Bevy asset；
- retained input、result 與 waiting-to-apply bytes 全部計入 diagnostics；
- optimized benchmark 顯示 hot path 無 per-job unbounded allocation、string lookup 或 coarse global lock serialization。

### P4：讓 chunk edge／working-set 配置重新符合 accepted profile

目前 production spike 使用 `8³` chunk、Y `0..31`、view／generation radius 2、resident 64、in-flight 4。ADR 0026 的 provisional sizing premise 是 `32³` chunk，active world-space 水平 coverage 128 m、resident coverage 192 m；chunk edge 改變時必須保持同等 world-space coverage並重新計算 bytes、latency 與 queue high-water。

因此不能把 `64 → 256` 當作正式修復，也不能只降低 radius 來通過 RAM gate。P4 先提交 evidence note，再選擇：

1. **建議 baseline**：把 production profile 收斂到 `32³` chunk premise，為尚未承諾相容的測試 world 建立明確 rebuild／migration policy；或
2. 保留 `8³`，但以同等 128 m／192 m coverage 建立新的 profile evidence，證明增加的 chunk count、entity count、metadata、mesh、collider 與 queue 成本仍通過 ADR 0026。若需要放寬 accepted threshold，另建 profile major 與 ADR。

在這個選擇完成前：

- P1／P2 使用小 deterministic fixture 驗證 correctness；
- 不宣稱目前 radius 1–2 已達 D2／D10 working-set gate；
- 不以 Riverbed 的大 chunk／LOD 數字替代本專案 reference-host evidence。

P4 出場條件是 machine-readable `PerformanceEvidenceV1` 能解釋 active、resident、visible、requested／prefetch counts 與相同 world-space coverage；不是只修改常數。

### P5：合併 collider，避免一 voxel 一 shape

建議 commit：

```text
perf(voxel-collider): merge occupied cells into bounded boxes
```

實作：

- render geometry 與 collision geometry 維持獨立 derived kinds；
- 對 collision-occupied volume 做 deterministic 3D box merge，輸出有限 compound cuboids；
- merge order 與 tie-break stable；
- collider receipt 仍對應 exact occupancy revision；
- cave-entry safety 在 matching collider active 前保持保守封閉；
- 記錄 voxel count、box count、retained bytes、build latency 與 Avian broad／narrow phase成本。

門禁：merged boxes 與逐 voxel occupancy 在 property corpus 上完全等價；solid chunk shape count 顯著低於 occupied count；collider rebuild P95／P99 通過 ADR 0026 的 150／300 ms gate。

### P6：建立 deterministic terrain material path

建議拆成：

```text
feat(voxel-render): compile deterministic terrain texture layers
feat(voxel-render): route terrain mesh groups to Bevy materials
```

實作：

1. 由 locked content／asset graph 按 stable ID 編譯 texture-layer table；不得依 filesystem、loaded-folder 或 `HashMap` iteration order。
2. merge key 包含所有會改變 emitted vertex／material 的值，例如 material layer、face variant、rotation、tint／AO class。
3. 支援 top／side／bottom 或完整 face-specific texture mapping。
4. opaque、cutout、translucent、emissive 使用現有 `MeshGroup` 分流；water／glass／leaves 不得繼續假裝 opaque full occluder。
5. voxel sampler 使用 nearest；mipmap、address mode、color space 與 array layer dimensions 由 asset validation 固定。
6. 若使用 `ExtendedMaterial`／custom shader，main、prepass、shadow、motion-vector 與 alpha-discard semantics 必須一致。
7. 先使用普通 Bevy attributes 建立可測 baseline；packed `u32x2` 只有在 profiler 證明 vertex／upload bandwidth 是瓶頸後才加入。

P6 出場條件：layer mapping 對 discovery order deterministic；每個 group 使用正確 depth／alpha／culling policy；missing／invalid texture 有 stable fallback diagnostic；render failure不改 authoritative world hash。

### P7：profile 驅動的遠景、LOD 與 visibility

P7 不是 P1 bugfix 的一部分。依[渲染架構](../architecture/rendering.md)既定順序，只在 CPU greedy mesh、async incremental path 與 material semantics 穩定後進行：

1. 先量測 Bevy native AABB／frustum baseline；
2. 若 draw-call／entity overhead 主導，評估 region batching／indirect draw；
3. 若 vertex／upload 主導，評估 packed vertices、coarse mesh 或 retained GPU representation；
4. LOD 必須定義 material selection、cave preservation、neighbor transition seam 與 edit invalidation；
5. occlusion／GPU-driven path 只有在較小方案無法通過 accepted budget時才進 provider spike；
6. 任何替換 Bevy／upstream機制的方案都要通過 ADR 0014 upstream deviation gate。

不得直接採用 Riverbed 的 `NoFrustumCulling`、六 face entities 或 LOD 2 downsample 作 production default。

## 驗收矩陣

| ID | 場景 | 必須成立 |
| --- | --- | --- |
| `VR-01` | 單 voxel／六面 | outward CCW winding、normal、bounds、indices 全部一致 |
| `VR-02` | 六方向 halo neighbor | 每個方向只消除對應 boundary face |
| `VR-03` | 原地 360° camera rotation | 無幾何消失；resident／receipt／queue 不變 |
| `VR-04` | 同 chunk 內移動 120 fixed ticks | 無 distance-only eviction／generation／remesh |
| `VR-05` | 跨 boundary 後立即折返 | retain 防止重新 materialize；畫面無空洞 |
| `VR-06` | chunk 邊界 edit | 本 chunk 與受影響 neighbor 各失效一次；stale completion不套用 |
| `VR-07` | queue flood 4× hard cap | coalescing、stable priority、cancellation、backpressure、有界 shutdown |
| `VR-08` | empty／solid／cave／transparent corpus | mesh group、collider safety、empty removal semantics 正確 |
| `VR-09` | 十分鐘 traversal | frame／tick／latency／RAM／VRAM／queue 符合 ADR 0026且無單調增長 |

## 驗證命令與證據

普通 CI／headless gate：

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p latticeaxiom-voxel-mesh
cargo test -p latticeaxiom-voxel-runtime
cargo test -p latticeaxiom-engine --no-default-features
cargo test -p latticeaxiom-engine --no-default-features --features client chunk_mesh::tests
cargo test -p latticeaxiom-engine --no-default-features --test headless_host
```

hot-path microbenchmarks：

```powershell
cargo bench -p latticeaxiom-voxel-mesh --bench meshing
cargo bench -p latticeaxiom-voxel-runtime --bench runtime
```

normative performance run 必須依 ADR 0026：optimized distributable build、關閉 development dynamic-linking、frozen locks、fixed seed／snapshot、2 分鐘 warm-up、10 分鐘 measurement、至少 1,000 個 edit／mesh／collider／chunk-load revision samples，並輸出 `PerformanceEvidenceV1`。普通開發機 FPS 或單張 screenshot 不能代替 gate。

GPU runner／developer smoke 另覆蓋：

- 原地完整 rotation；
- 同 chunk 移動與多 boundary traversal；
- 180° turn／折返；
- chunk 邊界 break／place；
- 地表、洞穴、水、cutout 與 translucent 同畫面；
- 長時間後 Mesh assets、entities、resident chunks、task/result bytes 沒有無界增長。

## 提交與停止規則

- P0–P2 是 correctness blockers；任何一項未通過，不開始以 LOD／culling／視距調參追求畫面數字。
- P3–P5 是 performance correctness；每個 hot-path commit 都要有 before／after benchmark或trace。
- P6 是正常 terrain presentation 的最低產品品質，但不得改變 authoritative block／fluid identity。
- P7 每個 provider spike 單獨提交，不把 refactor、行為變更與 dependency upgrade 混在同一 commit。
- 任一階段發現 accepted ADR 無法滿足，停止擴大實作，提交 reproduction／measurement／smallest deviation evidence，再走 ADR 0014／profile adjustment流程。

## 相關文件

- [第一個可玩 demo 路線圖](roadmap-first-demo.md)
- [套件驅動的 Bevy 執行期整合路線](roadmap-game-engine.md)
- [渲染 capability／pass／provider](../architecture/rendering.md)
- [Bevy 執行期](../architecture/game-engine-runtime.md)
- [實體、物理與呈現](../architecture/entity-physics-presentation.md)
- [資產語義](../architecture/asset-semantics.md)
- [診斷、檢查與除錯可視化](../architecture/diagnostics-inspection-and-debug-visualization.md)
- [ADR 0026：第一個 Demo 性能預算](../decisions/0026-freeze-first-demo-performance-budgets.md)
- [ADR 0027：Authoritative world 與 persistence](../decisions/0027-freeze-authoritative-world-and-persistence-contract.md)
- [ADR 0029：Render capability 與 provider contract](../decisions/0029-freeze-render-capability-and-provider-contract.md)
- [Riverbed](https://github.com/Inspirateur/riverbed)
