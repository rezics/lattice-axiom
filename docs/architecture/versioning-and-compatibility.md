---
title: 版本、相依性與相容性架構
status: proposed
type: explanation
updated: 2026-08-19
decision:
  - ../decisions/0003-no-global-version-package-scoped-compatibility.md
  - ../decisions/0010-nickel-driven-package-system.md
---

# 版本、相依性與相容性架構

> [不以全域大版本決定相容性](../decisions/0003-no-global-version-package-scoped-compatibility.md)已獲採納。Nickel 驅動套件宣告與組合、求值後直接進入 Rust 強型別管線，已由[決策 0010](../decisions/0010-nickel-driven-package-system.md)採納；本頁其餘 lock schema、能力契約與遷移細節仍是提案。

## 核心模型

```text
官方遊戲／整合包 manifest
          │  宣告版本範圍、能力與政策
          ▼
      Dependency Resolver
          │
          ▼
       LockedGameGraph
          │  精確版本、來源、內容雜湊與所選提供者
          ├───────────────┐
          ▼               ▼
      建置／載入       lock graph
          │               │
          └───────┬───────┘
                  ▼
       執行期產物與生成記錄
```

官方集合可以像套件管理器的根專案一樣擁有自己的發布版本，但它只是依賴解析的根，不是所有相容性判斷的共同座標。

## 四種不能混用的版本

### 套件版本

識別一個可發布套件，供依賴範圍、更新政策與發行說明使用。同一套件版本的產物仍可能因目標平台或組態而不同，因此不能單獨作為精確建置識別。

### 能力契約版本

識別具名介面的語義，例如 `cave.portal@2`、`terrain.base@1` 或 `entity.health@3`。提供者與使用者依賴能力契約，而非依賴「官方遊戲版本」。Adapter 也以能力契約作為輸入與輸出。

### 資料 schema 版本

持久化資料由擁有者自行版本化。遷移函式的作用域是 `(ownerId, schemaId, from, to)`，不能有一個中央 `switch(worldVersion)` 解析所有歷史資料。

### 精確實現識別

程序生成、編譯快取與重現需要內容雜湊、演算法修訂、正規化組態雜湊和依賴產物雜湊。語義相容的 bug fix 仍可能改變生成結果，因此它可以保持能力契約版本，但必須得到新的精確實現識別。

## Nickel 根設定檔

```nickel
let latticeaxiom = import "latticeaxiom/game.ncl" in
{
  game = {
    id = "latticeaxiom.official-game",
    version = "2026.8",
  },
  packages = {
    "latticeaxiom.runtime" = {
      source = 'Registry { registry = "official", version = "=0.1.0" },
    },
    "latticeaxiom.cave.contract" = {
      source = 'Registry { registry = "official", version = "=2.1.0" },
    },
    "latticeaxiom.cave.topology.geodesic" = {
      source = 'Registry { registry = "official", version = "=1.4.0" },
    },
    "latticeaxiom.biomes.official" = {
      source = 'Registry { registry = "official", version = "=3.0.0" },
    },
  },
  capabilities = {
    require = {
      "generation.coordinator" = "1",
      "cave.portal" = "2",
    },
  },
  policy = {
    authoritative = ["simulation.*", "worldgen.*"],
    presentationMayDiffer = ["render.*", "audio.*"],
  },
} | latticeaxiom.GameProfile
```

`latticeaxiom.official-game@2026.8` 只識別這份根設定的發布。若兩個實例解析出不同根版本但權威能力子圖相容，它們不應只因標籤不同而被拒絕。

v1 profile 使用精確版本或已鎖定來源，不先承諾一般版本求解器。Nickel 完整求值後直接轉成 Rust `CompositionSpec`，再由 package kernel 建立 `LockedGameGraph`；`.ncl` 原始文字本身不是 lock graph。

## 鎖定圖

鎖定圖至少保存：

- 精確套件版本、來源與內容雜湊；
- 直接與遞迴相依性；
- 能力候選與最終所選提供者；
- root override、adapter、可選相依性與 fallback；
- 正規化組態與資產雜湊；
- 必要的工具鏈、平台與實現方式資訊；
- 信任、簽章、撤銷與取得來源。

鎖定圖是可重現解析結果，不是宣稱所有節點必須永遠一起升級的單體版本。

## 產物級生成記錄

世界不只保存一個全域生成版本。每個可重算產物保存自己的依賴子圖：

```text
ArtifactReceipt {
  artifactId
  artifactKind
  spatialDomain
  producerCapability
  producerImplementationHash
  normalizedConfigHash
  inputArtifactHashes[]
  contractVersions[]
  deterministicInputHash
}
```

產物識別可以表示為：

```text
artifactHash = H(
  producerImplementationHash,
  normalizedConfigHash,
  sorted(inputArtifactHashes),
  deterministicInputHash
)
```

這形成 Merkle 式依賴圖。某個群系裝飾器更新時，失效沿真正依賴它的後繼產物傳播；洞穴拓撲、地質和渲染器若不依賴它則不受影響。

## 世界目前設定與歷史產物

```text
World {
  activeProfileLock
  artifactReceiptIndex
  materializedBaselines
  runtimeDeltas
  ownerSchemaStates
}
```

- `activeProfileLock` 是今後新規劃或明確重算時的預設依賴圖。
- `artifactReceiptIndex` 保存歷史產物的精確來源子圖。
- `materializedBaselines` 保存已落地且不再需要舊生成器即可讀取的基線。
- `runtimeDeltas` 保存玩家與模擬對基線的修改。
- `ownerSchemaStates` 由各資料擁有者獨立遷移。

同一世界可以包含不同生成記錄的規劃域；跨域不變條件由地形邊界、洞穴入口、水文出口或其他版本化契約銜接。第一次生成的區塊快照是已存在空間世界的權威，詳見[世界持久化與 RocksDB World Store](world-persistence.md)。

## 相容性不是布林值

比較兩個依賴圖時，解析器應按能力與產物輸出結果：

| 結果 | 含義 |
| --- | --- |
| `Identical` | 精確子圖與內容雜湊相同，可直接重用 |
| `ContractCompatible` | 實現不同但能力契約相容，可供新工作使用 |
| `Recomputable` | 受影響產物可安全重算，沒有不可覆寫狀態 |
| `Migratable` | 擁有者提供完整資料遷移路徑 |
| `Bridgeable` | 舊新空間或協定可由 adapter／邊界契約連接 |
| `MaterializedOnly` | 舊產物可讀但已無法以原生成器重算 |
| `Incompatible` | 權威語義或必要資料沒有合法轉換 |

根閉包雜湊適合快速確認 `Identical`，但不能取代其餘結果。

## 多人與建置

多人握手應比較伺服器權威能力子圖，而不是要求所有客戶端閉包完全相同。純表現模組可以不同；會改變碰撞、玩法、世界生成或權威資料的模組必須相容或由伺服器提供結果。

可重現建置、多人確定性與資料相容是三個不同問題：

- 可重現建置關心相同輸入能否得到相同產物。
- 多人確定性關心權威模擬是否得到相同語義結果。
- 資料相容關心舊狀態是否能由新擁有者讀取或遷移。

## 尚待設計

- `CompositionSpec`、`LockedGameGraph` 與持久化 `latticeaxiom.lock` 的實際 schema、canonicalization 與演進規則。
- 能力契約使用 SemVer、結構相容或自訂版本代數的範圍。
- 舊原生生成器的封存與安全執行方式。
- 產物生成記錄的儲存粒度、去重與垃圾回收。
- 伺服器權威子圖與客戶端純表現差異的握手協定。

## 相關文件

- [模組核心與宣告式組合](module-composition.md)
- [Nickel 驅動的套件系統與雙實現路徑](package-management.md)
- [世界持久化與 RocksDB World Store](world-persistence.md)
- [可組合世界生成架構](world-generation.md)
- [決策 0003：不以全域大版本決定相容性](../decisions/0003-no-global-version-package-scoped-compatibility.md)
- [詞彙表](../foundations/glossary.md)
- [待決問題](../planning/open-questions.md)
