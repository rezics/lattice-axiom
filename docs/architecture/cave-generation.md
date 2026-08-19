---
title: 可組合洞穴生成架構
status: proposed
type: explanation
locale: zh-Hant
updated: 2026-08-09
decision:
  - ../decisions/0002-hybrid-cave-generation-composition.md
  - ../decisions/0004-territorial-delegation-for-spatial-generation.md
---

# 可組合洞穴生成架構

> [洞穴採共享拓撲與群系形態的混合模型](../decisions/0002-hybrid-cave-generation-composition.md)與[依領地遞迴委派地形／洞穴能力](../decisions/0004-territorial-delegation-for-spatial-generation.md)已獲採納。本頁把責任邊界展開成候選介面、資料流與可驗證原型；具體演算法和參數仍待實驗。

## 目標

洞穴生成必須同時做到四件事：跨區域保持連通、讓地下群系擁有鮮明形態、接受地質與水文約束，以及容納任意數量但行為有界的擴展模組。

關鍵不是尋找一個包辦全部責任的「洞穴演算法」，而是把不同數學問題放進具有明確基數與契約的組合通道。

## 「層」有三種不同含義

討論洞穴是否為「單獨一層」時，必須標明談的是哪個軸。

| 軸 | 問題 | 本架構的答案 |
| --- | --- | --- |
| 執行層 | 何時規劃與求值？ | 先有領地、地質與水文語義，再求拓撲和形態，最後放置內容 |
| 空間所有權層 | 誰定義某個位置或拓撲域？ | 維度提供預設，地下群系可在三維子領地接管拓撲與形態；每通道、每所有權域只有一個主要所有者 |
| 模組責任層 | 哪個提供者負責哪一種能力？ | 維度協調器解析委派，父域與子域使用同一拓撲契約，內容由地下群系或目的地模組提供 |

因此，「拓撲是獨立能力」「群系可提供完整局部拓撲與形態」和「每維度由單一協調器解析跨域約束」可以同時成立。

## 五種核心能力

### 地下語義領地

地下群系必須在空氣或洞壁生成以前存在。它是一個可查詢的三維領地場，回答「此處若形成洞穴，應由哪一種地下環境負責」，而不是掃描最終方塊後才貼上的分類。

任意位置的生成上下文可以表示為：

```text
WorldContext(p) = {
  surfaceBiome,
  subsurfaceBiome,
  geologyBody,
  hydrologicBasin,
  civilizationClaims,
  ...
}
```

這些語義不必互相排斥。它們透過不同通道影響生成：地下群系可以決定洞穴拓撲與形態，地質體限制可侵蝕性，水文區決定含水狀態，文明領地則提出礦道或遺跡目的地。

群系必須宣告空間影響形態：

```text
surface-only
surface-with-shallow-subsurface
bounded-3d-volume
vertical-column
whole-dimension
```

地表群系可以影響淺層入口，但除非明確宣告三維或柱狀領地，否則不自動接管深層拓撲。

### 洞穴拓撲

拓撲層產生低解析度、可跨規劃格持久化的圖：

```text
CaveGraph = Nodes + Edges + Portals + ConnectivityConstraints
```

它回答哪些空間必須相連、哪些入口必須在相鄰規劃格重現、哪些路徑具有特定通行能力，但不直接決定鐘乳洞、熔岩管或裂縫的最終表面。

每個維度選出唯一 `CaveTopologyCoordinator`，每個拓撲所有權域則選出唯一 `CaveTopologyProvider`。維度預設提供者負責未被子領地接管的根域；地下群系可以實作同一介面並接管自己的三維子域。

父域不在子域內繼續雕刻，而是提交 `CaveTopologyDelegation`。子域可以使用岩溶最短路徑、熔岩管生長、裂隙網路、文明交通圖或其他完全不同的演算法，只需履行必要入口、水文連接、保護體積、目的地與預算契約。

同一拓撲域不能同時有兩個主要提供者。多個支線與修改模組可以在主要拓撲下有界合成，但不能暗自建立另一套未協調主幹。

### 洞穴形態

形態提供者把拓撲邊與節點實現為有界 SDF／密度貢獻，例如通道、洞室、豎井、裂隙、熔岩管或地下河床。提供者主要由局部地下群系選出；在領地邊界可進行有限寬度的形態混合。

形態可以改變截面、粗糙度、彎曲、分叉、洞室尺度和壁面特徵，但不得破壞拓撲契約要求的入口、最小淨空與關鍵可達性。

### 洞穴物理

地質與水文不只是裝飾參數。它們以約束集合參與規劃：

- 岩性和硬度調整可侵蝕方向、跨度和坍塌風險。
- 斷層與岩層界面提供長程方向場或裂隙候選。
- 地下水位、流域和壓力頭限制含水洞穴、泉口與地下河。
- 溫度、深度與熱源限制熔岩、冰洞及熱液環境。

拓撲協調器應讀取這些約束；它們不直接接管整個洞穴圖。

### 洞穴內容

地下群系和目的地模組提供生態、資源、危險、遺跡與玩法節點。大型內容先提交 `DestinationIntent`，由規劃階段預留空間並要求拓撲連接，不能在區塊生成末端任意鑿穿既有結構。

## 建議資料流

```text
World Seed + Resolved Profile
              │
              ▼
地表領地／地下領地／地質體／水文區
              │
              ▼
CaveTopologyCoordinator（每維度一個）
              │
              ├── 根域預設 CaveTopologyProvider
              ├── 收集 CaveIntent、目的地與約束
              └── 對地下群系子域建立委派契約
              │
              ▼
每拓撲域唯一 CaveTopologyProvider
              │
              ├── 節點、邊、入口實現與掛接點
              ├── 支洞／洞室／目的地貢獻者仲裁
              └── 未滿足需求與預算診斷
              │
              ▼
地下群系形態 + 地質／水文約束
              │
              ▼
有界 SDF／Density 合成與結構避讓
              │
              ▼
材質、生態、資源、玩法內容、區塊快取與驗證
```

## 候選資料契約

### `CaveTopologyDelegation`

父域把有界拓撲責任交給子域：

```text
CaveTopologyDelegation {
  domainId
  parentTopologyId
  requiredPortals
  optionalPortals
  hydraulicConnections
  mandatoryDestinations
  protectedVolumes
  topologyTargets
  budget
}
```

子域回傳自身圖、入口實現、掛接點、保留體積、未滿足的可選需求和診斷。必要入口失敗時，協調器必須請父域重新路由或輸出顯式錯誤，不能在實體化末端強行鑿穿。

### `CaveIntent`

群系與擴展模組提交願望，不直接改寫最終密度場。

```text
CaveIntent {
  stableId
  sourceId
  bounds
  intentKind
  preferredScale
  directionBias
  connectivityDemand
  destinationRefs
  costEstimate
  priorityClass
}
```

`stableId` 必須由世界種子、提供者識別碼和規劃格座標等穩定輸入導出，不能依註冊先後或執行緒排程產生。

### `CavePortal`

跨規劃格或所有權域的洞穴不能只約定一個點。入口契約至少需要：

```text
CavePortal {
  portalId
  position
  tangent
  radiusRange
  traversalClass
  clearance
  fluidState
  pressureHead
  contractVersion
}
```

相鄰規劃格與相鄰所有權域以相同穩定鍵重建入口；形態提供者可改變附近外觀，但必須在允許誤差內滿足位置、切向、淨空和流體狀態。

### `CaveMorphologyContribution`

```text
CaveMorphologyContribution {
  sourceId
  bounds
  signedDistanceField
  materialHints
  protectedClearance
  blendProfile
  estimatedCost
}
```

所有密度貢獻都要宣告有限 `bounds`。協調器只在有交集的查詢範圍求值，並在超出預算時輸出可重現的取捨診斷。

## 拓撲所有權域與局部貢獻者

每域唯一所有者不代表整個維度只能有一種洞穴。

根域預設與地下群系子域使用相同的拓撲提供者契約。父域只透過入口和約束觀察子域，不依賴其內部演算法；局部貢獻者則可以在當前所有權域新增：

- 掛接到本域主要拓撲的支洞；
- 群系特有洞室或裂縫；
- 城市、礦山和遺跡之間的目的地連接；
- 受地質界面引導的洞穴段；
- 需要水文連續性的地下河段。

局部貢獻者必須選擇既有掛接點或提出有成本的新增連接，不能默默建立第二套無人協調的主要拓撲。若模組需要完全不同的網路模型，它應取得子領地所有權並成為該域的唯一提供者。

## 通道與仲裁

| 通道 | 允許的提供者 | 仲裁方式 |
| --- | --- | --- |
| `cave.coordinator` | 核心或第三方協調器 | 每維度 `SelectOne` |
| `cave.topology` | 維度預設或地下群系拓撲提供者 | 每所有權域 `TerritorialDelegation` |
| `cave.intent` | 地表群系、地下群系、結構、文明、模組 | `BudgetedMerge` |
| `cave.branch` | 群系與局部洞穴模組 | `SpatialArbitration` |
| `cave.morphology` | 地下群系與邊界混合器 | `TerritoryWinner`／`BoundaryBlend` |
| `cave.destination` | 結構、任務、文明與生態模組 | `PlannedPlacement` |
| `cave.constraint` | 地質、水文、保護區與玩法規則 | `ConstraintSet` |
| `terrain.density` | 已核准的地形與洞穴貢獻 | 有界 `SmoothCSG` |

每次仲裁至少輸出：候選、勝出或被拒原因、預算消耗、衝突空間與穩定排序鍵。如此「群系打架」或「洞穴模組互相挖穿」才是可定位事件，而不是只能目視猜測的結果。

## 群系邊界如何影響洞穴

地下群系領地提供延綿範圍、核心區與拓撲所有權。拓撲以入口契約連接，形態在邊界帶轉換；兩者不能使用同一種盲目插值：

- 必要入口、可達性與水文連接採硬契約，不可平均掉。
- 連通性與最小淨空採硬約束，不可平均掉。
- 截面、粗糙度、濕度和材質可按 `BoundaryProfile` 漸變。
- 互斥玩法規則由空間所有權選出唯一勝者。
- 稀有目的地不得因邊界混合重複生成。

地表群系可以對淺層入口與侵蝕風格施加影響，但不自動擁有整個深層柱狀空間。地下群系、地質體與深度帶共同決定深層環境。

若子域拒絕可選入口，父域可以繞行；必要入口無法實現則是可診斷的規劃失敗，不以區塊後處理掩蓋。

## 最小可驗證切片

第一個原型不需要完整生態與文明系統，但應包括：

1. 兩個地下群系領地，具有不同洞室截面和粗糙度。
2. 一個維度預設拓撲域與兩個由地下群系接管、使用不同演算法的子域。
3. 一個在本域主要拓撲下工作的支洞貢獻者。
4. 跨越至少四個規劃格和兩個拓撲域的固定入口契約。
5. 一個地質方向場、一條具流向的地下河約束與一個必須連接的大型目的地。
6. 區塊亂序、並行度變化、註冊順序變化與重啟後逐位元或容差內一致的回歸測試。

原型至少量測：

- 圖上連通率與實際可通行率；
- 死路、環路、垂直路徑與洞室尺度分布；
- 地下群系辨識度與邊界轉換長度；
- 入口位置、切向、淨空與流體狀態誤差；
- 每規劃格的意圖數、SDF 求值數、記憶體與生成時間；
- 被預算或衝突仲裁拒絕的貢獻及原因。

## 尚待實驗的問題

- 第一批 `CaveTopologyProvider` 應採哪兩種圖建構或生長演算法做對照？
- 地下領地場要與地表 `Territory Atlas` 共用資料結構，還是只共用查詢與仲裁介面？
- 洞穴預算應以體積、表面積、圖複雜度、求值成本或混合單位計算？
- 水文約束是在拓撲搜尋中共同求解，還是在主要拓撲後進行有界修正？
- 如何把玩家偏好的洞穴節奏轉成可版本化的目標分布，而不是固定噪聲參數？

## 相關文件

- [世界生成架構](world-generation.md)
- [決策 0001：領地優先、群系主動定義環境](../decisions/0001-territory-first-biome-driven-world-generation.md)
- [決策 0002：洞穴採共享拓撲與群系形態的混合組合模型](../decisions/0002-hybrid-cave-generation-composition.md)
- [決策 0004：空間生成採領地遞迴委派](../decisions/0004-territorial-delegation-for-spatial-generation.md)
- [現代地形與洞穴生成研究](../research/modern-terrain-and-cave-generation.md)
- [Minecraft 世界生成研究](../research/minecraft-world-generation-lessons.md)
- [待決問題](../planning/open-questions.md)
