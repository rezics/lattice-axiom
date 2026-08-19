---
title: Nickel 驅動的套件系統與雙實現路徑
status: proposed
type: explanation
locale: zh-Hant
updated: 2026-08-19
decision:
  - ../decisions/0003-no-global-version-package-scoped-compatibility.md
  - ../decisions/0008-static-and-dynamic-realizations-share-one-graph.md
  - ../decisions/0010-nickel-driven-package-system.md
---

# Nickel 驅動的套件系統與雙實現路徑

> 套件系統是 Lattice Axiom 的核心控制平面。Nickel 負責宣告與組合，Rust package kernel 負責解析、取得、信任、建置與載入；靜態／動態路徑共用同一 `LockedGameGraph`。registry、一般版本求解、穩定第三方 ABI 與二進位快取仍未定稿。

## 核心定位

Lattice Axiom 不是「先啟動一個固定遊戲，再從資料夾附加 mod」。一次可建置、可發布或可啟動的遊戲，是核心、官方內容、第三方內容、資產、工具鏈與政策經套件系統組合後的結果。官方遊戲本身是一個根套件／game profile，不享有繞過公開契約的私有路徑。

套件系統必須同時回答：

- 這個遊戲要求哪些套件、能力與實現方式？
- 每個來源實際取得了什麼內容，是否可信且可重建？
- 哪些套件與能力能共同形成一個合法閉包？
- 應該產生靜態建置、動態載入、純資料編譯，還是遠期的沙箱實現？
- 最終執行期需要哪些 ID、登錄表、排程、資產與持久化契約？

## 向 Nix／NixOS 學什麼

值得採用的是「宣告語言直接參與構造設定、套件與建置圖，package kernel 掌握來源、lock、store 與實現」的分工，而不是複製 Nix 語言、Nix store 路徑或 NixOS module library。

JSON 可以作為 lock file 或對外輸出；它不應成為求值器、resolver、builder 與 loader 之間的內部語義總線。Lattice 的內部邊界由有版本的 Rust 型別與其不變量定義。

## 四層語義模型

```text
package.ncl + game.ncl + overlays + lattice.lib
                         ↓
            Nickel 合併、合約、完整求值
                         ↓ direct serde conversion
                CompositionSpec (Rust)
                         ↓ source + graph resolution
                LockedGameGraph (Rust)
                         ↓ realization planning
                   BuildPlan (Rust)
                         ↓ build / load / register
                  RuntimeImage (Rust)
                         ↓
              IDs、tables、schedules、assets
```

### `CompositionSpec`

描述作者要求與已完成的 Nickel 組合結果，但不假裝來源已取得或版本已鎖定。至少包含：

```text
CompositionSpec {
  schema_version
  root_profile
  package_requests[]
  capability_requirements[]
  realization_preferences[]
  parameters
  policy
}
```

它由 `nickel-lang` 完整求值後透過 `Expr::to_serde<T>()` 直接轉成 Rust 型別。不存在需要保存或由 resolver 重新解析的 JSON 中間檔。

### `LockedGameGraph`

保存解析後的精確套件、來源、內容雜湊、相依邊、能力提供者、實現方式、規範化參數、合約版本、目標平台與工具鏈身分。它是所有後續建置與載入的共同語義來源。

### `BuildPlan`

把已鎖定圖轉成可執行但尚未執行的工作 DAG：來源準備、程式碼建置、資產編譯、膠水產生、封裝與快取查詢。每個工作節點宣告輸入、輸出、工具鏈、環境政策與可快取身分。

### `RuntimeImage`

只包含啟動與熱路徑需要的數值 ID、登錄表、排程、資產索引、窄函式入口與持久化 owner/schema 資訊。它不包含 Nickel 值、未鎖定版本、遠端來源或作者函式。

## Nickel 套件介面

專案提供版本化的 `lattice.lib`，而不是讓每個工具自行解讀欄位。概念上的套件宣告如下：

```nickel
let lattice = import "lattice/package.ncl" in
{
  package = {
    id = "lattice.official",
    version = "0.1.0",
  },
  provides = {
    capabilities = ["content.blocks@1", "worldgen.biomes@1"],
    blocks = [
      { id = "stone" },
      { id = "dirt" },
      { id = "grass" },
    ],
  },
  dependencies = {
    "lattice.core" = { version = "=0.1.0" },
  },
  realizations = ['NativeStatic, 'NativeDynamic, 'Data],
} | lattice.Package
```

根 game profile 選擇套件、來源、實現偏好與政策：

```nickel
let lattice = import "lattice/game.ncl" in
{
  game = {
    id = "lattice.official-game",
    version = "2026.8",
  },
  packages = {
    "lattice.official" = {
      source = 'Registry { registry = "official", version = "=0.1.0" },
      realization = 'NativeStatic,
    },
    "example.dragon" = {
      source = 'Path "./packages/dragon",
      realization = 'NativeDynamic,
    },
  },
  policy = {
    native_code = 'TrustedOnly,
    authoritative = ["simulation.*", "worldgen.*"],
  },
} | lattice.GameProfile
```

函式、預設值、overlay 與合併只存在於求值過程；進入 `CompositionSpec` 的結果必須是有限、完整且符合合約的值。

## 來源取得與求值順序

遠端存取不是 Nickel import 的隱含能力。v1 採兩階段控制：

1. Rust package kernel 先求值只依賴本機根檔與 `lattice.lib` 的 game profile，再以其來源請求與既有 lock 找出要取得的本機路徑、registry 物件或 Git 內容，驗證雜湊後放入受控來源根。
2. 逐一求值已取得套件的 `package.ncl` 以發現相依要求；package kernel 取得新增來源並重複，直到候選圖閉合，再執行全圖解析。
3. Nickel 只能 import 根 profile、本套件內容、`lattice.lib` 與已解析的宣告式函式庫；求值器不能自行連網或讀取未宣告主機路徑。

套件 manifest 輸出相依要求，不以 `import` 執行跨遊戲套件載入。這避免「必須先執行不可信套件，才能知道要下載什麼」的循環。若未來需要可發布的 Nickel 函式庫，另設與遊戲套件圖分離的工具鏈輸入圖。

## Resolver 與 lock graph

Nickel 合約回答「單一宣告的形狀與局部語義是否合法」；Rust resolver 回答「這些精確來源能否共同形成遊戲」。v1 resolver 刻意只做：

1. 驗證精確套件要求、來源身分與內容雜湊；
2. 展開直接與遞迴相依並拒絕非法循環；
3. 選出具名能力提供者，檢查唯一性、衝突、adapter 與 fallback；
4. 驗證實現方式、信任政策、平台、ABI 與工具鏈；
5. 依穩定鍵配置登錄順序與數值 ID；
6. 輸出 `LockedGameGraph` 和持久化 `lattice.lock`。

第一版不建立通用 registry 或 SAT／PubGrub 類求解器。profile 使用精確版本或既有 lock；等真實套件衝突出現後，再決定版本範圍與治理規則。

`lattice.lock` 是機器產物，可以採 canonical JSON 以便審查、diff 與外部工具使用。雜湊以規範化的強型別內容計算；人工修改在下一次 `lock` 時被覆寫或拒絕。lock 的檔案格式不等同於內部 API，也不允許下游繞過型別驗證直接查任意 JSON 欄位。

## 實現方式

同一個邏輯套件可以依發布需要成為：

- 靜態連結進完整遊戲閉包的可信原生實現；
- 使用者放入資料夾即可載入的預編譯原生實現；
- 載入期編譯成登錄表與資產的純資料實現；
- 遠期由沙箱執行的 WASM 實現。

這些是 realization，不是不同的套件 API 或生態。發現方式也與實現方式分離：Nickel profile 可以選動態實現，資料夾掃描也可以發現純資料套件。

### 靜態建置

```text
LockedGameGraph
       ↓ realization planner
generated workspace / glue crate / asset jobs
       ↓ controlled Cargo + asset tools
game closure = binaries + assets + lock + descriptors
```

靜態路徑依已鎖定圖產生膠水 crate，由 `register_all()` 呼叫各套件的公開註冊函式，再交給 Cargo 建置與 LTO。可重現建置與二進位快取是目標；v1 先做到精確 lock、工具鏈記錄與閉包雜湊，不宣稱跨機器位元重現已完成。

### 資料夾動態載入

發布套件的概念形狀：

```text
packages/
└── dragon/
    ├── lattice-package.json
    ├── dragon.dll
    └── assets/
```

`lattice-package.json` 是 `PublishedPackageDescriptor` 強型別的版本化封裝表示，不是另一套作者 manifest。發布工具由 `LockedGameGraph` 與建置結果生成它，並記錄 schema、套件內容雜湊、ABI、目標平台與工具鏈。loader 先解碼並驗證成 Rust 型別，再把候選交給同一 resolver；不能讓業務邏輯散落成 JSON path 查詢。

啟動流程為：掃描候選目錄、驗證描述符與內容、解析同一套件圖、載入動態函式庫或純資料、消化註冊載荷、建立 `RuntimeImage`。使用者不需在本機求值 Nickel 或編譯預建模組；代價是發布者必須為目標平台與工具鏈提供產物。

### v1 原生 ABI

- 一個具版本的 `extern "C"` 入口；
- `#[repr(C)]` 描述符只承載窄、明確生命週期的資料與函式指標；
- 描述符包含 ABI 版本、目標平台與工具鏈雜湊；
- 大型或可演進註冊資料使用有 schema 的序列化載荷；
- loader 在建立玩法狀態前拒絕不相容產物；
- 不支援執行期卸載。

這不是穩定第三方 ABI。面向陌生作者的長期方案需要在穩定 C ABI、WASM 元件模型或其他沙箱邊界之間另作決策。

## Nickel 上游 package manager 的邊界

Nickel 上游 package manager 管理的是可由 Nickel `import` 的函式庫，現階段仍屬實驗功能且官方發行版預設停用。Lattice v1 不使用它解析遊戲套件、下載原生產物或治理 registry。

可以借鑑或日後重用的部分包括 manifest 合約、Git／index 來源、精確 lock 與最小版本選擇；任何直接依賴都必須先證明能與 Lattice 的能力、realization、信任、資產與世界相容性模型對齊。

## CLI 最小範圍

`lattice-cli` 第一版需要：

- `check`：求值 Nickel、執行合約並建立 `CompositionSpec`；
- `lock`：取得精確來源、解析能力並寫 `lattice.lock`；
- `build`：建立 `BuildPlan`、產生靜態膠水並呼叫受控工具鏈；
- `pack`：產生發布描述符、內容雜湊與套件目錄；
- `doctor`：檢查來源、內容雜湊、平台、工具鏈、ABI 與 lock 漂移。

registry 發布、簽章、撤銷、二進位快取與授權政策先保留在待決清單。

## 效能與正確性不變量

- 發現順序不影響解析圖、ID、建置計畫或生成結果。
- 同一 lock 與工具鏈輸入必須建立語義相同的 `RuntimeImage`。
- Nickel、遠端來源與字串套件鍵不進入每 tick、每實體或每體素熱路徑。
- 官方內容不使用第三方無法取得的註冊捷徑。
- 同一內容包的靜態／動態實現得到相同語義登錄表。
- lock graph 不同只表示精確輸入不同；實際相容性仍按能力與產物依賴子圖判定。
- JSON、bincode 或其他編碼只能序列化有版本的強型別，不得成為無型別擴充後門。

## 外部依據

- [Nickel 原始碼倉庫](https://github.com/nickel-lang/nickel)
- [Nickel：記錄合併](https://nickel-lang.org/user-manual/merging/)
- [Nickel：合約](https://nickel-lang.org/user-manual/contracts/)
- [Nickel 的 Rust 公開 API](https://docs.rs/nickel-lang/latest/nickel_lang/)
- [Nickel package management](https://nickel-lang.org/user-manual/package-management/)
- [Nix：derivation](https://nix.dev/manual/nix/stable/language/derivations)
- [Nix：flake lock files](https://nix.dev/manual/nix/stable/command-ref/new-cli/nix3-flake.html#lock-files)

## 相關文件

- [已選技術棧與採用邊界](../foundations/technology-stack.md)
- [模組核心與宣告式組合](module-composition.md)
- [版本、相依性與相容性架構](versioning-and-compatibility.md)
- [決策 0010：以 Nickel 驅動套件系統](../decisions/0010-nickel-driven-package-system.md)
- [決策 0008：雙實現路徑](../decisions/0008-static-and-dynamic-realizations-share-one-graph.md)
- [第一個 demo 路線圖](../planning/roadmap-first-demo.md)
