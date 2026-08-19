---
title: 可組合世界生成架構
status: proposed
type: explanation
updated: 2026-08-19
decision:
  - ../decisions/0001-territory-first-biome-driven-world-generation.md
  - ../decisions/0002-hybrid-cave-generation-composition.md
  - ../decisions/0003-no-global-version-switch.md
  - ../decisions/0004-territorial-delegation-for-spatial-generation.md
  - ../decisions/0015-bevy-native-y-up-world-coordinates.md
  - ../decisions/0020-semantic-registration-and-content-selection.md
---

# 可組合世界生成架構

> [領地優先、群系驅動、約束求解](../decisions/0001-territory-first-biome-driven-world-generation.md)、[混合洞穴組合模型](../decisions/0002-hybrid-cave-generation-composition.md)、[無全域版本開關](../decisions/0003-no-global-version-switch.md)、[領地遞迴委派](../decisions/0004-territorial-delegation-for-spatial-generation.md)與[Bevy 原生 Y-up](../decisions/0015-bevy-native-y-up-world-coordinates.md)的方向已採納。本頁提出具體責任、資料流與候選介面；演算法與參數仍需原型驗證。世界生成是versioned package／capability，经host adapter执行于Bevy，不建立第二套engine runtime。

## 目標與非目標

本架構需要同時支援：

- 可由任意座標開始查詢的無限世界。
- 跨大量區塊的山脈、河流、洞穴網路、城市與其他巨型計畫。
- 不依賴註冊順序的群系與生成模組組合。
- 群系可在自身領地內完整提供地形與洞穴能力，而不只調整維度預設參數。
- 地質、水文、文明與工業系統日後以相同公開機制疊加。
- 平行區塊生成、產物級重現、獨立版本識別與可診斷衝突。

目前不承諾真實地球模擬，也不要求所有生成能力都能在已建立世界中無縫熱插拔。

所有二維規劃使用水平座標 `(x, z)`，高度為 `y`；三維 chunk／voxel 使用 `(x, y, z)`。演算法輸出直接符合 Bevy Y-up，不維護軸轉換版本。

## 世界生成是依賴圖，不是單一演算法

概念上的資料流如下：

```text
Validated ContentCatalog／SemanticCatalog／RoleBindings + WorldgenConfig + 世界種子
        ↓
WorldgenPlugin 建立不可變 GenerationPlan resource
        ↓
空間骨架
領地圖譜／計畫格／跨邊界入口
        ↓
語義所有權
地表群系／地下群系／其他空間角色
        ↓
領地能力委派
地形／洞穴拓撲／洞穴形態／其他主要提供者
        ↓
目標場、計畫與約束提供者
        ↓
跨領地求解與通道合成
        ↓
地形密度、材質與內容實體化
        ↓
ChunkDraft + generation provenance
        ↓
Bevy task 結果驗證 → 權威區塊 → RocksDB snapshot
```

這不是固定的函式呼叫清單。每個節點宣告輸入、輸出、空間範圍、尺度、實現版本、能力契約與合成方式；載入階段把它們編譯成已解析生成圖。大型結構可以在地形密度前提交計畫，裝飾只能在表面完成後執行，而不相關的場可以並行求值。

核心不應硬編碼「森林之後生成洞穴」之類的內容關係，只應理解依賴、領地、能力與契約。

## 確定性與空間查詢契約

單一可重算產物至少由下列輸入識別：

```text
GenerationArtifact = f(
    worldSeed,
    artifactDependencyReceipt,
    coordinateOrBounds,
    queryKind
)
```

`artifactDependencyReceipt` 只記錄該產物實際讀取的精確提供者、能力契約、組態和上游產物，不把完整遊戲根版本當成每次生成的隱含輸入。

生成器不可把「鄰近區塊已經生成」當成必要前置狀態。跨區塊內容應先形成可由範圍查詢的計畫；任一區塊只實體化與自身邊界相交的部分。

候選查詢介面如下，名稱尚未定案：

```ts
worldgen.getTerritory(scope, position)
worldgen.getBiome(scope, position)
worldgen.resolveProvider(channel, position)
worldgen.sampleField(channel, position)
worldgen.getPlans(kind, bounds)
worldgen.sampleDensity(position)
worldgen.materializeChunk(chunkBounds)
```

每個提供者必須宣告最大影響範圍或可建立的空間索引，否則核心無法保證任意座標查詢是有界工作。

## 可組合生成原語

核心目前預計理解以下通用原語，而不是具體群系或地貌：

| 原語 | 責任 | 典型輸出 |
| --- | --- | --- |
| 領地提供者 | 建立穩定空間單元與鄰接關係 | 領地 ID、邊界距離、父子領地 |
| 所有權解析器 | 從候選內容中決定領地語義 | 地表群系、洞穴群系、文明歸屬 |
| 能力協調器 | 解析領地委派、依賴、預算與跨域契約 | 每通道／每域主要提供者、邊界工作項 |
| 場提供者 | 對座標輸出連續或離散意圖 | 密度、高度、溫度、含水量、岩性 |
| 計畫提供者 | 先描述跨區塊語義物件 | 河網、城市、道路、洞穴圖、巨型結構 |
| 約束提供者 | 宣告必須共同滿足的不變條件 | 河流連續、坡度上限、入口對接、排斥體積 |
| 求解器／合成器 | 依通道規則整合多個提供者 | 最終水位、地形密度、岩層、邊界過渡 |
| 實體化器 | 把場與計畫轉成執行期世界資料 | 方塊、體素、網格、實體與索引 |
| 裝飾器 | 在既有語義與表面上配置局部內容 | 植被、岩石、小型遺跡、環境物件 |

模組邊界應在生成圖建立與索引階段被消化。區塊熱路徑查詢已解析提供者、連續資料與空間索引，不反覆掃描原始模組。

## 空間生成權的遞迴委派

每個維度只選出一個 `DimensionGenerationCoordinator`。它負責解析所有權、依賴、預算、邊界與跨領地約束，不代表維度只有一個地形或洞穴演算法。

生成提供者的唯一性作用於「通道 × 所有權域」：

```text
owner(channel, domainOrPosition)
  = mostSpecificDeclaredTerritory(channel, domainOrPosition)
  ?? inheritedParentProvider(channel)
  ?? dimensionDefaultProvider(channel)
```

領地委派在生成圖編譯時解析，不是先產生維度結果後再由群系以最後寫入者覆蓋。規則為：

1. 維度為每個可委派通道提供預設實作。
2. 領地可宣告接管某個通道；巢狀子領地比父領地更具體。
3. 同層領地核心區由 `Territory Atlas` 預先解析為互斥所有權。
4. 邊界只查詢實際相鄰的少量提供者，依通道使用過渡或入口契約。
5. 多個細節與約束提供者可以共存，但主要基礎提供者每域只有一個。

| 通道角色 | 基數 | 解析方式 |
| --- | --- | --- |
| 維度協調器 | 每維度一個 | `SelectOne` |
| 基礎地形 | 每地形所有權域一個 | `TerritorialDelegation` |
| 洞穴拓撲 | 每拓撲所有權域一個 | `TerritorialDelegation` |
| 局部形態 | 每領地核心區一個，邊界可有少量多個 | `TerritoryWinner`／`BoundaryBlend` |
| 有界細節 | 每域多個 | `BudgetedMerge`／`SpatialArbitration` |
| 長程約束 | 每域多個 | `ConstraintSet` |

### 群系擁有基礎地形

`terrain.base` 由當地群系或更具體的子領地提供。沙丘可以使用方向場高度模型，山地使用侵蝕脊線與體積懸崖，沼澤使用低起伏水網，浮島群系則可以提供完全不同的三維密度場。

維度與長程系統不先建立一份不可替換的最終地形。它們提供大陸範圍、山脈軸線、流域、岩層、道路與結構保護等計畫或約束；群系在自身領地內決定如何實現。跨群系山脈可以在高山群系形成尖峰，在森林群系變成緩和山脊，在沙漠群系變成風蝕台地，同時遵守同一條宏觀連續計畫。

### 空間影響不是預設垂直柱

群系必須宣告空間影響形態，例如 `surface-only`、`surface-with-shallow-subsurface`、`bounded-3d-volume`、`vertical-column` 或 `whole-dimension`。地表沙漠可以影響淺層砂岩洞穴，但不因此自動擁有直到世界底部的所有地下空間。

## 群系領地生成

### 空間領地圖譜

領地先於群系語義存在。候選實作可使用多尺度、帶抖動的 Voronoi 或冪圖，再施加有界的低頻空間扭曲，使領地保持連通而邊界不呈規則多邊形。

尺度是空間角色，不是固定結論。例如：

```text
省域 Province       8–32 km
區域 Region         1–8 km
局部生境 Ecotope    128 m–1 km
```

群系宣告適合的尺度、目標面積分布、形狀傾向與過渡寬度。大型群系參與較粗層級；子群系、綠洲或特殊林地可在父領地內以較細層級出現。這避免群系數增加時所有群系都被壓成細碎斑點。

### 與註冊順序無關的仲裁

對領地槽位 `c` 與候選群系 `b`，由穩定完整識別碼產生：

```text
u(b,c) = hash01(worldSeed, territoryId, biomeId)
t(b,c) = -ln(u(b,c)) / weight(b)
winner = argmin t(b,c)
```

這是確定化的指數競賽，等價於使用 Gumbel-Max 從未正規化權重抽樣。連續分數使理論平手機率為零；實作仍以完整名稱作最後平手規則，不能只相信有限位元雜湊永不碰撞。

建議採兩級份額：

1. 世界或整合包先決定 `BiomePack` 的份額。
2. 獲勝內容集合再用集合內權重選擇群系。

这让含有一百个biomes的content package不会只因项目数量压倒含有五个biomes的package，同时不会在地图上制造连续的“package专属区域”。集合份额由Nickel world profile／locked graph治理；package不能以任意极大权重自行取得全世界。

### 數量上限的精確含義

架構不預先分配群系槽位，因而可以處理任意大的有限註冊表。真正的可數無限候選只有在總權重可求和，且存在可惰性取樣的結構時才有定義：

```text
sum(weight(b)) < infinity
```

無限個候選不可能各自擁有相同且固定的非零世界份額。實際執行仍只解析有限的活動依賴圖；「無硬上限」是資料模型性質，不是無限 CPU 與記憶體承諾。

## 群系是環境程式，不是分類標籤

群系不宣告「溫度在某區間時選我」，而是先取得領地，再輸出自身環境意圖並可提供領地能力：

```ts
BiomeProgram {
  id
  version
  scope
  role

  spatialInfluence {
    kind
    depthRange
    bounds
  }

  territory {
    scale
    localWeight
    areaDistribution
    morphology
  }

  boundary {
    profile
    transitionWidth
    adapters
  }

  provide {
    terrainBase
    caveTopology
    caveMorphology
  }

  emit {
    terrainIntent
    climate
    hydrology
    geology
    ecology
    resources
    structureAffinity
    ambience
  }

  fallback
}
```

`provide` 讓群系在領地內接管基礎生成能力；`emit` 則向可合成通道提交目標場與約束。兩者都不是固定長度向量。新系統可以註冊新通道，而不需要替所有舊群系增加一個共同選擇維度；缺少提供者時使用父領地、維度預設或明確 fallback。

群系是本地環境與基礎地形的主要定義者，但不是跨世界規律的唯一寫入者。例如火山群系可以提供局部三維地形與岩漿洞穴拓撲；獨立板塊模組則向地質與山脈計畫提交長程約束。協調器依公開所有權與合成規則組成它們，而不是由載入順序決定誰覆蓋誰。

同一位置的主要 `terrain.base` 或同一拓撲域的主要 `cave.topology` 只能有一個所有者；這不妨礙任意數量的有界細節、目的地和約束在其下組合。

## 邊界是一等生成區域

領地查詢必須同時回傳第一候選、相鄰候選、邊界距離與局部座標。邊界帶由雙方 `BoundaryProfile` 和可選轉接器共同生成。

不同通道不能共享同一種盲目線性插值：

| 通道 | 候選合成方式 |
| --- | --- |
| 溫度、濕度、霧與肥力 | 平滑插值或擴散約束 |
| 地形密度 | SDF／CSG、平滑聯集與坡度限制 |
| 水文 | 流量、集水區、水位與出口連續性求解 |
| 地質 | 地層延續、彎曲、侵入與斷層規則 |
| 離散表面材質 | 過渡材質或穩定藍噪聲混合 |
| 植被與散布物 | 點程序合併、排斥與稀疏化 |
| 結構計畫 | 邊界體積、相容性、排斥與轉接計畫 |

不要求為 `N` 個群系手寫 `N²` 個配對。一般邊界由少量可組合 profile 與語義標籤處理；只有海岸、火山口或其他特殊關係需要額外轉接器。多個轉接器同時適用時，也必須使用穩定仲裁而非載入順序。

## 其他生成能力

### 宏觀地理、水文與地質

空間骨架只提供尺度、領地與鄰接，不先決定某地必然炎熱或潮濕。群系與獨立系統提交目標場後，水文和地質求解器維持跨領地連續性。完整侵蝕或板塊模擬若無法有界查詢，可以在較大規劃格上執行、快取，或用距離場與程序函式近似。

### 大型結構、城市與文明

大型內容先產生語義計畫，而不是在區塊內擲機率：

```text
StructurePlan
├── 穩定 ID 與種子
├── 邊界體積
├── 語義子圖
├── block／entity ContentPredicate 與 placement Role
├── 地形與水文約束
└── 實體化提供者
```

任一區塊只查詢與自身相交的計畫。城市、道路與文明可以在同一機制上建立更高階圖，再交由幾何與體素化能力實現。

结构检查已有world时使用compiled `ContentPredicate`，例如机制owner定义的portal-frame Tag；
结构实际放置新内容时使用已经锁定的`ContentRole → StableId` binding。禁止从registration
namespace、package name或Tag成员顺序选择输出。计划／产物receipt记录实际predicate major、
role binding与相关Map revision，使同一生成epoch可重现。

### 地形密度與材質

核心以三維密度場而非純高度圖作為一般地形表示，使懸崖、拱門、浮島、多層地下空間和大型地下結構能使用同一合成模型。高度場仍可作為常見地表提供者或最佳化表示。

### 河流

河流是跨領地計畫或流網，不是每個群系各自放置的表面裝飾。群系可以定義河岸、濕地、地下段與材料反應；流向、連通、入口和出口由共享水文圖治理。

### 裝飾與資源

樹木、岩石、局部礦物和小型遺跡使用具穩定鍵的點程序。藍噪聲、Poisson disk、排斥半徑和點程序稀疏化比逐方塊獨立機率更適合組合多個提供者。資源分布應能同時讀取群系、地質與玩法進度，而不讓其中一層默默覆蓋其他層。

### 執行期修改

玩家建造、採掘、工業污染和文明演化不屬於可任意重算的基礎生成結果。區塊第一次生成並提交後，快照即成為該空間的權威；生成器更新只影響尚未物化或被顯式 regenerate 的範圍。持久化細節見[世界持久化與 RocksDB World Store](world-persistence.md)。

## 洞穴採共享協調與領地拓撲的混合模型

洞穴不由單一「群系層」或「獨立層」包辦，而是拆成可以分別組合的能力：

- 地下群系以三維語義領地描述「此處若形成空洞，應成為怎樣的環境」。
- 維度洞穴協調器維護委派、跨規劃格入口、可達性與共同約束。
- 維度預設或地下群系可以在各自拓撲所有權域內產生完整節點、邊與局部網路。
- 地下群系把自身或父域拓撲實現為洞室、通道、裂縫與壁面形態。
- 地質與水文提供岩性、斷層、地下水與流體連續性等正交約束。
- 地下群系與目的地模組提供生態、資源、遺跡、危險與玩法內容。

地下群系領地在空氣或洞壁生成以前就存在，不能由最終方塊事後反推。它可以接管領地內的完整 `cave.topology` 與 `cave.morphology`；父域以必要和可選入口、水文連接、保護體積、目的地與預算委派子域。

| 通道 | 基數與合成方式 |
| --- | --- |
| `cave.coordinator` | 每維度選出一個跨域協調器 |
| `cave.topology` | 每拓撲所有權域選出一個提供者；最具體領地接管，否則繼承父域 |
| `cave.intent` | 多個意圖在區域預算內合併 |
| `cave.branch` | 多個局部貢獻者經空間仲裁掛接到本域主要拓撲 |
| `cave.morphology` | 領地內唯一形態提供者，邊界帶允許有限混合 |
| `cave.destination` | 多個目的地先規劃空間，再向拓撲提出連接需求 |
| `cave.constraint` | 地質、水文、結構保護與玩法限制共同求解 |
| `terrain.density` | 僅合成已核准且具有有限包絡的 SDF／密度貢獻 |

這裡的可組合性包含「每域選擇唯一所有者」「向子領地委派」「邊界契約」與「有界合併多個貢獻」；並不要求所有洞穴演算法同時相加。核心內容與第三方模組遵守同一套通道、仲裁與診斷規則。

完整責任、候選契約與原型量測見[可組合洞穴生成架構](cave-generation.md)；採納理由見[決策 0002](../decisions/0002-hybrid-cave-generation-composition.md)與[決策 0004](../decisions/0004-territorial-delegation-for-spatial-generation.md)。

## 產物依賴與世界相容性

世界不綁定一個決定全部相容性的 `worldVersion`。它至少保存：

- 目前供新規劃工作使用的 `WorldgenConfig`、validated content catalog fingerprint 與 `GenerationPlan` revision。
- 每個規劃域、生成計畫或可重算快取的產物級生成記錄。
- 實際生產者的 stable ID、generator revision、精確實現與組態雜湊。
- 該產物真正依賴的輸入產物雜湊、群系份額、權重與 fallback。
- 已實體化程序基線、玩家／模擬差異和各資料擁有者的 schema 狀態。

更新內容或生成器後，只沿真正受影響的依賴子圖使可重算產物失效。新規劃域可以使用新的 generation revision；舊域與已物化區塊保留原 provenance，並透過地形邊界、洞穴入口、水文出口或其他版本化契約連接。若沒有合法 adapter 或 migration，worldgen plugin 必須拒絕建立受影響的新邊界，而不是以整體遊戲版本掩蓋差異。

完整模型見[版本、相依性與相容性架構](versioning-and-compatibility.md)與[決策 0003](../decisions/0003-no-global-version-switch.md)。

## 必須保持的不變條件

- 註冊順序不影響語義結果。
- 區塊生成順序與平行排程不影響語義結果。
- 維度只有一個協調器，但每個通道與所有權域都有可遞迴委派的唯一主要提供者。
- 更具體的子領地接管父域能力，同層衝突由領地配置先解析，不使用任意全域優先級。
- 每個所有權、場與計畫衝突都有指定解析器。
- 不使用未宣告的「最後寫入者獲勝」。
- 每個跨區塊計畫都有穩定 ID、邊界與有界查詢方式。
- 第一方與第三方內容使用同一公開能力。
- 邊界能被直接查詢、視覺化與測試。
- 同一產物依賴子圖能被識別並重現；完整根雜湊不同不會自動被解讀為整體不相容。
- worldgen读取的authoritative Tag／Map／Role binding进入artifact dependency receipt；语义目录变化不隐式重算已物化chunk。
- Terrenia worldgen只是普通package consumer；另一个维度closure可提供自己的Role／Predicate而无需host理解`terrenia:*`。

## 第一批原型與量測

### 二維領地原型

1. 建立至少三個尺度的領地圖譜。
2. 註冊一千個合成群系與數十個合成內容集合。
3. 驗證註冊順序、增刪候選、權重與領地面積統計。
4. 視覺化第一與第二候選、邊界距離和過渡帶。
5. 測量冷查詢、快取查詢、記憶體與平行一致性。

### 三維洞穴原型

1. 由共享面鍵建立相鄰規劃格一致的入口。
2. 建立維度預設拓撲域與至少兩個由地下群系接管的子拓撲域。
3. 讓子域使用不同拓撲演算法，同時滿足父域入口委派。
4. 讓至少三種群系形態把拓撲轉為不同 SDF 洞穴。
5. 量測主連通分量、環路率、死路率、寬度分布、地表開口與淹水阻斷率。
6. 驗證任意區塊生成順序、提供者註冊順序與局部演算法替換不產生非預期接縫。

## 已知限制與待驗證假說

- 有界 domain warp 是否能在自然外觀、領地連通與查詢成本間取得平衡？
- 兩級內容集合／群系份額是否符合內容作者對公平與稀有度的直覺？
- 場通道的型別與合成規則能否保持可理解，而不演變成隱藏的全域優先級系統？
- 巨型結構、水文與侵蝕的規劃格需要多大 halo 才能避免接縫？
- 產物生成記錄應採多大的空間與語義粒度，才能兼顧局部失效、儲存成本與舊生成器封存？
- 洞穴拓撲、局部貢獻者、目的地與水文約束的共同預算如何量化，才能同時控制成本與探索節奏？

## 相關文件

- [世界生成方向決策](../decisions/0001-territory-first-biome-driven-world-generation.md)
- [洞穴生成組合決策](../decisions/0002-hybrid-cave-generation-composition.md)
- [無全域大版本決策](../decisions/0003-no-global-version-switch.md)
- [空間生成領地委派決策](../decisions/0004-territorial-delegation-for-spatial-generation.md)
- [可組合洞穴生成架構](cave-generation.md)
- [版本、相依性與相容性架構](versioning-and-compatibility.md)
- [世界持久化與 RocksDB World Store](world-persistence.md)
- [Bevy 模組與內容組合](module-composition.md)
- [Minecraft 世界生成與洞穴模組的設計教訓](../research/minecraft-world-generation-lessons.md)
- [現代地形與洞穴生成研究](../research/modern-terrain-and-cave-generation.md)
- [待決問題](../planning/open-questions.md)
- [詞彙表](../foundations/glossary.md)
- [語義註冊、內容判定與選擇](semantic-registration.md)
