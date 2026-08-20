---
title: 凍結第一個 Demo 性能預算
status: accepted
type: decision
updated: 2026-08-20
---

# 決策 0026：凍結第一個 Demo 性能預算

## 背景

[第一個可玩 demo 路線圖](../planning/roadmap-first-demo.md)要求 D2 量測 frame、fixed tick、
edit-to-visible、memory 與 queues，並在 D10 以十分鐘固定路徑旅程凍結
active／resident／visible chunks、generation／mesh／collider／save P95、task／I/O／FFI queues
與 RAM／VRAM 預算。既有架構也已規定：

- gameplay 使用獨立於 render delta 的 Bevy `FixedUpdate`；
- background work、I/O、mesh、collider、command 與 upload 都必須有 backpressure；
- portable native gameplay 每 system／batch 跨 ABI，禁止逐 entity／voxel FFI；
- static realization 直接使用 typed Bevy system，不得為了共用 ABI 而失去 inline／LTO；
- budget 由 profile 治理，host 強制上限並發布 diagnostics。

但上述文件沒有給出第一版數字、量測端點或修改規則。若各子系統自行選擇 threshold，
同一 demo 可以在「60 FPS」、「fixed tick 沒有落後」、「已保存」或「dynamic overhead 可接受」
等詞上得到互相矛盾的結論。本決策凍結 PERF-01 至 PERF-08 的第一版共同基線。

## 問題映射

| 問題 | 決策章節 |
| --- | --- |
| `PERF-01` | 第 1、2、10–12 節 |
| `PERF-02` | 第 3 節 |
| `PERF-03` | 第 4 節 |
| `PERF-04` | 第 5 節 |
| `PERF-05` | 第 6 節 |
| `PERF-06` | 第 7 節 |
| `PERF-07` | 第 8 節 |
| `PERF-08` | 第 9 節 |

## 決策

### 1. `desktop-reference-v1` 與預算生命週期

第一個 demo 的 normative performance profile 名為 `desktop-reference-v1`。其目標硬體類別為：

| 項目 | 基線 |
| --- | --- |
| CPU | 6 個 physical cores／12 hardware threads、x86-64 AVX2 |
| system memory | 16 GiB |
| GPU | 6 GiB dedicated VRAM、支援目前 Bevy baseline 所需的 D3D12 或 Vulkan capability |
| storage | 本機 SSD；load／durability 報告必須記錄實際裝置與 sync policy |
| display | 1920×1080、60 Hz、demo `medium` presentation preset |
| OS | Windows x86-64 或 Linux x86-64；normative run 明文記錄 OS build、driver 與 graphics backend |

這是能力類別，不是以 CPU 名稱猜測等價。D2 第一次提交 baseline 時，必須指定一台不高於此
產品目標的 exact reference host，並記錄 CPU／GPU SKU、clock／power policy、RAM、storage、OS、
driver 與 backend。較快開發機的結果可作 diagnostics，不能取代 reference-host pass。

本頁數字有兩個明確階段：

1. **D2 provisional**：D2 playable spike 一出現就套用本頁全部適用的 hard caps 與量測端點。
   數字是可失敗的 provisional release gates，不是只記錄而不治理的觀察值。
2. **D10 freeze**：D10 的代表性 Terrenia 旅程在 exact reference host 上通過後，把實測 baseline、
   exact scene／lock 與本頁 threshold 一起標記為 frozen。沒有 D10 evidence 不得宣稱已凍結。

performance profile 是產品／測試 policy，不是 native ABI 常數、world state 或 package semantic hash。
package 可以請求較少資源，不能自行提高 host hard cap。

### 2. Render frame、fixed tick 與 memory

`desktop-reference-v1` 的 first-version budgets 為：

| 指標 | Budget |
| --- | --- |
| presented render frame | P95 `<= 16.67 ms`；P99 `<= 25 ms` |
| long frame | 十分鐘 steady traversal 中，非明文 loading／device-recovery transition 的 `> 50 ms` frame `<= 3` |
| authoritative fixed rate | `60 Hz`，即每 tick `16.67 ms` wall interval |
| complete `FixedUpdate` CPU | P95 `<= 8 ms`；P99 `<= 12 ms` |
| fixed catch-up | 每 render frame 最多執行 2 個 catch-up ticks；不得形成持續 backlog |
| physics step share | Avian／所選 physics 的 integrate＋broad／narrow phase＋solve，P95 `<= 3 ms`、P99 `<= 5 ms` |
| client process RAM | 十分鐘旅程 high-water `<= 4 GiB` |
| GPU memory | 十分鐘旅程 high-water `<= 3 GiB`；若 backend 無可靠數值，必須標為 unsupported，不能填 `0` |

frame gate 使用關閉 VSync／frame limiter 的 raw timing run，另以 60 Hz capped run驗證玩家設定；GPU 與
main-world／render-world CPU time分開記錄。headless manual-time fixture驗證 fixed semantics與 throughput，
但不能宣告通過 render budget。

### 3. Active、resident 與 visible chunk working set

D2 provisional profile 以 `32×32×32` voxel chunk 作 sizing premise。chunk edge 是 world snapshot／voxel
spike 的 schema 決策，不由本頁永久凍結；若該決策改變，必須依本節 adjustment rule重新量測，而不是
沿用看似相同的 count。

| Working set | 範圍／hard cap |
| --- | --- |
| active | player 周圍水平 radius 4、垂直 radius 2，即最多 `9×5×9 = 405` chunks |
| resident | 水平 radius 6、垂直 radius 3，即最多 `13×7×13 = 1,183` chunks |
| visible | 每 frame 最多 `512` resident chunks 提交可見 presentation |
| requested／prefetch | requested、materializing 與 prefetch 合計最多 `1,536` chunks |

`active` 必須是 `resident` 的子集；visible chunk 必須引用有效 resident revision。距離不是唯一 pin：
authoritative scheduled work、player／entity pin 與明文 system pin 可以保持 active，但仍計入 hard cap。
超限時依 distance／priority class／stable chunk key 決定、合併同 chunk 的舊 revision、停止遠端 prefetch
並施加 backpressure；不得藉由無界擴容維持表面流暢。

若 chunk edge 改變，新的 profile 必須至少保持相同 world-space active／resident coverage，並以 decoded
bytes、mesh／collider bytes、P95 latency 和 queue high-water重新計算 count。只降低 count 來通過 RAM
budget，不算相容調整。

### 4. Chunk、world 與 persistence latency

所有 latency 使用具體起點、完成 revision 與 durability level。舊 revision completion、被取消工作或
fallback presentation 不得記為成功。

| Operation | 起點 → 終點 | P95 | P99 |
| --- | --- | ---: | ---: |
| edit-to-visible | authoritative block edit committed → matching revision mesh 首次 presented | `100 ms` | `200 ms` |
| collider rebuild | authoritative edit committed → matching occupancy revision collider active | `150 ms` | `300 ms` |
| warm chunk load | resident request accepted → authoritative snapshot decoded and ready | `50 ms` | `100 ms` |
| cold chunk load | resident request accepted → authoritative snapshot decoded and ready | `250 ms` | `500 ms` |
| world activation | user／command `Play` accepted → `Playing` 可接受第一個 action | `2 s` | `3 s` |
| save written | dirty authoritative revision → RocksDB `written` receipt | `250 ms` | `500 ms` |
| save durable | dirty authoritative revision → configured `durable` receipt | `1 s` | `2 s` |
| checkpoint | checkpoint request → independently restorable checkpoint published | `5 s` | `10 s` |

`queued`、`written`、`durable` 與 `checkpointed` 仍是不同語義；UI 的「已保存」不得藉由較短的
written P95 冒充 durable。world activation只涵蓋 local immutable package／artifact cache；download、
compile、migration與shader warm-up若是獨立明示階段，分項量測且不得隱藏在刪除樣本中。

warm load表示同一 process 的受控 cache-hot corpus；cold load表示新 process、以 benchmark runner執行
documented OS cache eviction後的同一 corpus。無法證明 cache state時，報告必須標為 `unknown`，不能把
該次 run算進 cold gate。

### 5. Task、I/O、mesh 與 collider queues

queue entry 是一項帶 world／chunk／revision、immutable input、cancellation token 與 owner 的 work item；
in-flight bytes計入 retained input、staging、result、command與尚未 apply／release 的 buffer。

| Queue kind | queued hard cap | in-flight bytes hard cap |
| --- | ---: | ---: |
| world generation | `128` jobs | `128 MiB` |
| mesh | `128` jobs | `128 MiB` |
| collider | `64` jobs | `64 MiB` |
| persistence I/O／commit | `64` jobs | `64 MiB` |
| other package／module tasks | `128` jobs | `64 MiB` |
| all kinds combined | `512` jobs | `384 MiB` |

global cap優先於各 kind cap；不能同時把所有 per-kind bytes填滿。完成結果在 main world 的 apply budget
是每 render frame `<= 2 ms` 且 `<= 16 MiB`。CPU-heavy concurrency最多使用
`max(1, physical_core_count - 2)` 個同時工作，並且只經 Bevy task pools；這不是建立專案 thread runtime
的授權。

任一 queue 到 `75%` 是 soft high-water：停止遠端 prefetch、coalesce同 chunk到最新尚未執行的 revision、
取消 stale work並降低非玩家關鍵 priority。到 hard cap時返回 stable backpressure diagnostic／status。
authoritative work不得 silent drop；persistence持續失敗時應停止新的危險 mutation，而不是犧牲 dirty data。

D9 fluid、future render upload或 package-specific queue仍受 combined cap。其 consumer可另立較低的
cell／command budget；不得以新增 queue kind繞過本節總量。

### 6. Dynamic batch shape

D1 的 canonical dynamic gameplay benchmark 使用與 gameplay 相同的 generated adapter，而不是手寫
FFI microkernel：

- 3 至 5 個 SoA columns；canonical case 合計 `64 bytes/entity`；另有 `128 bytes/entity` stress case；
- target batch size `256 entities`，host可在 `128..=1,024` 間依 archetype調整；
- 單一 `LaxBatchView` 的 column payload合計 `<= 64 KiB`；因此 `128 bytes/entity` case最多 512 entities；
- 一次 callback 的全部 batch payload合計 `<= 1 MiB`；
- 同一 system 的小 archetype batches在同一 callback傳遞；不得為單一 entity建立一次 callback，
  也不得讓 module逐 field反向 getter／setter。

entity handle、descriptor、alignment padding與command output分別計量，不能藏進 nominal
`bytes/entity`。host可以為 layout／alignment／isolation提供 staging，但必須遵守第 9 節 crossover policy。

### 7. Dynamic callback、indirect call 與 command budgets

以下 soft／hard budgets適用於一個 authoritative fixed tick；dynamic presentation／render callback以一個
render frame作相同計量單位並分開報告：

| 指標 | Soft budget | Hard cap |
| --- | ---: | ---: |
| host → module system callbacks | `64` | `128` |
| 全部跨 ABI indirect／service calls | `256` | `512` |
| command count | `4,096` | `16,384` |
| command bytes（header、padding、payload） | `256 KiB` | `1 MiB` |

dynamic bridge 的全部 pack／FFI／unpack／validate CPU在真實旅程中 P95 `<= 1 ms/tick`。graph compiler
若發現 closure 的必執行 callback declarations已超過 hard cap，必須在 code activation前拒絕。
runtime command超限使該 callback／transaction失敗並產生 owner-aware diagnostic；不得截斷前半段後套用，
也不得靠跳過 validation通過預算。

### 8. Static direct 與 dynamic batch overhead

同一 SDK business declaration的 static與portable dynamic realization必須使用相同 input corpus、
action receipts、registration、state hash與normative save bytes。optimized exact-host benchmark的 gate為：

```text
dynamic_batch_P95
  <= max(static_direct_P95 * 1.25, static_direct_P95 + 0.50 ms)
```

此 gate適用於至少 `10,000` entities的 D1 gameplay-shaped system，以及 D8／D10 第一個真實
inventory／machine或等價 sandbox consumer；兩者仍須各自符合完整 fixed tick budget。十分鐘旅程中，
dynamic bridge CPU總和 `<= FixedUpdate CPU` 的 `10%`，steady-state generated shim／callback不得有 heap
allocation。

static path必須從 trace／symbol／generated plan證明沒有經 C table或 `LaxBatchView` repack，且 release build
保留 thin LTO。逐 entity FFI只作反例 benchmark，永遠不是可接受 fallback。對極小 static baseline，
`+0.50 ms` absolute allowance避免無意義ratio；這不放寬 `1 ms/tick` bridge與完整 fixed tick hard gate。

### 9. Zero-copy、staging 與 command validation crossover

先判定資格，再判定 size：

1. 只有 manifest已驗證的 ABI-POD、layout／alignment相符、callback lifetime內可借用、alias規則成立，
   且不要求 rollback的 column才可 zero-copy。
2. layout／alignment不符、需要 packing／isolation、存在 opaque／host-owned value或 transaction rollback時，
   一律 staging；size不能讓不合格資料變安全。
3. 合格 payload `<= 16 KiB` 時 staging可作預設；`16..64 KiB` 由 reference-host sweep凍結 crossover；
   `>= 64 KiB` 預設 zero-copy。
4. fixed-tick callback需要 staging而 payload `> 256 KiB` 時拒絕該 shape；改為較小batch、host handle、
   async task或 `NativeStatic`，不能在 frame-critical path複製無界資料。

staging＋descriptor／column validation P95必須同時 `<= 0.10 ms/callback` 且 `<= 該 system CPU 的 10%`。
command buffer無論 column是否zero-copy都必須完整驗證；全部 dynamic commands的 validation P95
`<= 0.25 ms/tick`。超出時只能擴大語義 batch、預先驗證 immutable descriptor、縮小 command format、
移出 fixed path或選 static realization，不能跳過 bounds／schema／permission／generation檢查。

crossover是 `performance-profile-v1` 的穩定資料。runtime不得依瞬時 timing自動改變可能影響
transaction／failure semantics的路徑。

### 10. Normative measurement protocol

所有 D2／D10 performance evidence遵守同一 protocol：

1. 使用 optimized distributable build；workspace current baseline是 release profile、thin LTO、
   `codegen-units = 1`，且關閉 development-only dynamic-linking feature。
2. 使用 frozen package locks、fixed seed、canonical world snapshot與versioned command-input path；
   static／dynamic比較只改 realization。
3. reference host使用固定 power policy，記錄 background load；raw frame run關閉 VSync／limiter。
4. 完整旅程先 warm up 2 分鐘，再量測 10 分鐘。frame／tick／queue sample不得刪除 outlier；只有
   schema列出的 loading／device-recovery transition可分項，不進 steady frame histogram。
5. edit／mesh／collider／chunk load事件每種至少 1,000 個成功 revision samples；world activation、
   durable save與checkpoint至少 30 次獨立 runs。樣本不足是 `insufficient-evidence`，不是 pass。
6. percentile使用固定、版本化 histogram與 nearest-rank語義；至少輸出 count、P50、P95、P99、max、
   mean、high-water與拒絕／取消／stale數。以至少 3 次完整旅程的 run-level P95中位數判定；任一 run
   超過 hard threshold `20%` 仍使 gate失敗並要求調查。
7. CPU time分 static systems、dynamic pack／callback／command validation／apply、worldgen、physics、
   mesh、persistence與render；GPU time、RAM／VRAM、bytes與allocation另列，不能只報總 FPS。
8. cold／warm cache、client／headless、static／portable是不同 scenario，不可混成同一 percentile。

microbenchmark可使用更長 warm-up與更多 iterations，但不能取代真正 cdylib、真實 command buffer與
D10 traversal。普通共享 CI 的 wall-time數字只作趨勢；normative performance pass只由 designated
reference-host runner產生。

### 11. Machine-readable evidence 與自動 Gate

每次 normative run產生 versioned `PerformanceEvidenceV1`，至少包含：

- profile／budget schema與 status（`d2-provisional`或`d10-frozen`）；
- repository commit、Cargo lock、game／shell lock、registration hash與 `EngineBuildId`；
- exact hardware／OS／driver／backend／power／storage與 build receipt；
- world seed、snapshot hash、scene、action fixture、realization與 sample protocol；
- 本頁每項 budget的 observed P50／P95／P99／max／high-water、pass／fail／unsupported；
- queue cancellation／stale／backpressure、ABI calls／bytes、allocation與 fault counts；
- measurement tool／histogram version與 raw artifact hashes。

自動證據分層：

- 所有 CI 都執行 headless manual-time correctness、queue／bytes hard-cap boundary＋1、stale revision、
  callback／command hard-cap、static／dynamic state equivalence與 report schema validation；不需要 window、
  GPU或人工輸入。
- reference-host CI執行 release 10-minute traversal、cold／warm storage、edit burst、queue saturation、
  true dynamic-library comparison與 256 B 到 1 MiB staging／zero-copy sweep。
- GPU runner只作數值 timing／resource evidence；本決策不要求 screenshot或人工視覺驗收。
- D10 release gate要求 report完整且所有 required項為 pass；`unsupported`只允許 backend確實無該
  counter的觀測欄位，不允許 frame／tick／latency／queue／ABI gate。

### 12. 調整、版本化與延後

在 D10 前修改 provisional數字，必須同時提交：舊／新 reference-host evidence、玩家或correctness理由、
最小變更、對 RAM／latency／quality的影響，以及不採用 backpressure／batching／upstream修正的原因。
單純「目前實作超出預算」不是放寬理由。

D10 freeze後：

- 收緊 threshold或新增不影響舊欄位語義的 metric，可增加 performance profile minor；
- 放寬 threshold、改 percentile／端點／sample exclusion、改 hardware class或改 hard-cap failure semantics，
  必須建立新的 profile major與 ADR；
- Bevy／plugin／driver upgrade必須以原 frozen fixture先跑 before／after，不能原地覆寫舊 baseline；
- D11 biome expansion與任何 shipped package仍須通過同一 frozen profile，不能藉新內容降低 D10定義。

第一版明確延後多平台最低硬體矩陣、handheld／mobile、multiplayer server scale、production
`WasmComponent`、general render command與fluid cell的獨立產品 budgets。它們出現真實 consumer時建立
新 profile；在此之前仍受本頁 combined memory／queue／tick／ABI hard caps。

若 upstream voxel／physics／renderer不能通過，本決策只構成量測證據；是否 fork／replace仍必須通過
[決策 0014](0014-adopt-bevy-upstream-first.md)的 upstream deviation gate，不能直接授權自研第二 runtime。

## 取捨

- 60 Hz fixed tick與保留一半以上 tick interval給非固定工作，換取較清楚的 movement／physics響應和
  catch-up邊界；代價是 worldgen、mesh與I/O必須真正非同步且受 backpressure。
- chunk數以 provisional `32³` premise量化，使 D2 可立即驗證 memory與streaming；它不偷渡永久
  snapshot schema，chunk edge改變時必須重新建立同等 coverage證據。
- dynamic overhead同時使用 relative、absolute與whole-tick gates，避免極小 benchmark的ratio失真，
  也避免大型system以absolute allowance掩蓋退化。
- committed old baseline與 raw evidence會增加 repository／artifact storage，但可防止硬體、driver或
  benchmark protocol變動被誤認為程式改善。
- 嚴格 sample count與reference host使完整 gate較慢；普通 CI保留短、headless、deterministic tests，
  把昂貴 timing放在專用 runner與 release／scheduled workflow。

## 驗證

- `desktop-reference-v1` manifest與 `PerformanceEvidenceV1` 有正／負 golden、unknown-field、missing-field、
  threshold boundary與 `boundary + 1` fixtures。
- 十分鐘 traversal同時覆蓋 steady movement、chunk boundary、180° camera turn、edit burst、save、
  checkpoint與返回已訪問區域；任何 queue／RAM／VRAM不得單調無界增長。
- request flood以至少四倍hard cap 驗證 coalescing、stable priority、cancellation、backpressure與有界
  shutdown；舊 mesh／collider／I/O completion不得覆蓋新 revision。
- D1 benchmark掃描 entity count `1／64／256／1K／10K／100K`、column count `1／4／8`、
  `16／64／128 bytes/entity`、read／read-write與 contiguous／strided layouts。
- zero-copy sweep逐項拆出 descriptor validation、copy、module body、command parse與apply時間，實測
  crossover若相對 frozen值移動超過 `20%`，upgrade report必須調查。
- static／portable旅程比較 action receipts、authoritative state hash、semantic report與normative save
  bytes；performance report不得以結果不等價的較少工作量換取 pass。
- world save fault、slow I/O、cold/warm load、device counter unsupported與benchmark process crash都有
  stable machine-readable failure，不產生 partial pass report。

## 相關文件

- [第一個可玩 demo 路線圖](../planning/roadmap-first-demo.md)
- [Bevy 執行期整合路線](../planning/roadmap-game-engine.md)
- [Bevy 執行期](../architecture/game-engine-runtime.md)
- [原生模組 ABI](../architecture/native-module-abi.md)
- [渲染架構](../architecture/rendering.md)
- [實體、物理與呈現](../architecture/entity-physics-presentation.md)
- [世界持久化](../architecture/world-persistence.md)
- [診斷、檢查與除錯可視化](../architecture/diagnostics-inspection-and-debug-visualization.md)
- [待決問題](../planning/open-questions.md)
