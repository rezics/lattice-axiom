---
title: 套件管理、Nickel 組合與雙實現路徑
status: proposed
type: explanation
locale: zh-Hant
updated: 2026-08-19
source:
  - conversation-2026-08-09-turn-1
  - conversation-2026-08-09-turn-2
  - implementation-plan-2026-08-19
decision:
  - ../decisions/0003-no-global-version-package-scoped-compatibility.md
  - ../decisions/0007-nickel-as-composition-language.md
  - ../decisions/0008-static-and-dynamic-realizations-share-one-graph.md
---

# 套件管理、Nickel 組合與雙實現路徑

> Nickel、正規化中間模型，以及靜態／動態路徑共用 `ResolvedModuleGraph` 已獲採納。本頁定義 v1 邊界；registry、一般 SemVer 求解、穩定第三方 ABI 與二進位快取仍未定稿。

## 目標

同一個內容包應能依發布需要成為：

- 靜態連結進完整遊戲閉包的可信原生模組；
- 使用者放入資料夾即可載入的預編譯原生模組；
- 不含程式碼、載入期編譯成登錄表與資產的純資料模組；
- 遠期由沙箱執行的 WASM 模組。

這些是模組的「實現方式」，不是不同的模組 API 或套件生態。發現方式也與實現方式分離：Nickel profile 可以選動態實現，資料夾掃描也可以找到純資料模組。

## 組合管線

```text
module.ncl + game.ncl + contracts
                ↓
         lattice-compose
   合約、合併、完整求值、正規化
                ↓
    versioned canonical JSON model
                ↓
      resolver + trust policy
                ↓
       ResolvedModuleGraph
                ↓
             lock graph
          ┌─────┴─────┐
          ▼           ▼
   static builder   dynamic loader
          └─────┬─────┘
                ▼
   common registration + numeric IDs
```

Nickel 合約回答「輸入是否合法」，resolver 回答「這組候選是否能共同形成遊戲」。兩者不能混成一層：合約可驗證欄位型別，卻不負責選能力提供者、檢查衝突或配置穩定 ID。

## Nickel manifest

模組 manifest 的概念形式：

```nickel
let lattice = import "contracts.ncl" in
{
  package = {
    id = "lattice.official",
    version = "0.1.0",
  },
  provides = {
    blocks = [
      { id = "stone" },
      { id = "dirt" },
      { id = "grass" },
    ],
  },
  realization = {
    supports = ["native-static", "native-dynamic", "data"],
  },
  dependencies = {
    "lattice.core" = "=0.1.0",
  },
} | lattice.Module
```

遊戲設定檔選擇具體套件與實現方式：

```nickel
let lattice = import "contracts.ncl" in
{
  modules = {
    "lattice.official" = {
      version = "=0.1.0",
      realization = "native-static",
    },
    "example.dragon" = {
      path = "./modules/dragon",
      realization = "native-dynamic",
    },
  },
  optimizations = {
    lto = true,
  },
} | lattice.GameProfile
```

合約庫與 manifest schema 一同版本化。Nickel 原始碼不是 lock graph；只有完整求值、去除非匯出欄位並通過正規化後的資料能進解析器。

## 正規化模型

v1 中間模型至少包含：

```text
CompositionModel {
  schemaVersion
  rootProfile
  candidatePackages[]
  requestedRealizations[]
  capabilityRequirements[]
  policy
  sourceIdentities[]
}
```

正規化必須固定物件鍵順序、集合排序規則、套件與能力識別大小寫政策、路徑基準，以及整數與字串表示。解析與內容雜湊只以正規化結果為輸入，不能以格式化後的 `.ncl` 文字作語義雜湊。

## Resolver 與 lock graph

v1 resolver 刻意只做：

1. 驗證精確套件要求與來源內容；
2. 展開直接與遞迴相依；
3. 選出具名能力提供者並檢查唯一性、衝突與 fallback；
4. 驗證所選實現方式、信任政策、平台與工具鏈；
5. 依穩定鍵配置登錄順序與數值 ID；
6. 輸出 `ResolvedModuleGraph` 與 lock graph。

第一版不建立通用 registry 或 SAT／PubGrub 類版本求解器。profile 使用精確版本或已解析來源；等真實套件衝突出現後，再決定版本範圍與治理規則。

lock graph 至少記錄精確套件版本、來源、內容雜湊、相依邊、能力提供者、實現方式、正規化組態、合約版本、目標平台與工具鏈識別。它是機器產物，人工修改應在下一次 `lock` 時被覆寫或拒絕。

## 靜態建置

```text
game.ncl
   ↓ eval + resolve + lock
generated workspace / glue crate
   ↓ register_all() calls public module entrypoints
cargo build + selected profile + LTO
   ↓
game closure = binary + assets + lock + pipeline caches
```

靜態路徑讓編譯器看見完整可信模組集合，可內聯註冊之後的窄呼叫並移除未使用程式碼。可重現建置與二進位快取是目標；v1 先做到精確 lock、工具鏈記錄與閉包雜湊，不宣稱跨機器位元重現已完成。

## 資料夾動態載入

發行模組的候選形狀：

```text
modules/
└── dragon/
    ├── module.json
    ├── dragon.dll
    └── assets/
```

`module.json` 是從 `module.ncl` 匯出的有版本正規化模型。開發模式可以直接讀 `.ncl`，但載入前仍要經相同 `lattice-compose` 與 resolver。

啟動流程為：掃描候選目錄、解析同一模組圖、驗證內容與 ABI、載入動態函式庫、消化註冊載荷、建立 ID／查表／排程。使用者不需在本機編譯預編譯模組；代價是模組作者或發布者必須為目標平台與工具鏈提供產物。

### v1 原生 ABI

- 一個具版本的 `extern "C"` 入口；
- `#[repr(C)]` 描述符只承載窄、明確生命週期的資料與函式指標；
- 描述符包含 ABI 版本、目標平台與工具鏈雜湊；
- 大型或可演進註冊資料使用有 schema 的序列化載荷；
- 載入器在建立玩法狀態前拒絕不相容產物；
- 不支援執行期卸載。

這不是穩定第三方 ABI。面向陌生作者的長期方案需要在穩定 C ABI、WASM 元件模型或其他沙箱邊界之間另作決策。

## CLI 最小範圍

`lattice-cli` 第一版只需要：

- `eval`：驗證並輸出正規化模型；
- `lock`：解析精確來源並寫 lock graph；
- `build`：產生靜態膠水 workspace 並呼叫受控 Cargo 建置；
- `doctor`：檢查內容雜湊、平台、工具鏈與 ABI 診斷。

registry 發布、下載、簽章、撤銷、二進位快取與授權政策先保留在待決清單。

## 效能與正確性不變量

- 發現順序不影響解析圖、ID 或生成結果。
- Nickel 與字串套件鍵不進入每 tick、每實體或每體素熱路徑。
- 官方內容不使用第三方無法取得的註冊捷徑。
- 同一內容包的靜態／動態實現得到相同語義登錄表。
- lock graph 不相同只表示輸入有差異；實際相容性仍按能力與產物依賴子圖判定。

## 外部依據

- [Nickel：記錄合併](https://nickel-lang.org/user-manual/merging/)
- [Nickel：合約](https://nickel-lang.org/user-manual/contracts/)
- [Nickel：匯出與命令列介面](https://nickel-lang.org/user-manual/command-line-interface/)
- [Nickel 的 Rust 公開 crate](https://docs.rs/nickel-lang/latest/nickel_lang/)

## 相關文件

- [模組核心與宣告式組合](module-composition.md)
- [版本、相依性與相容性架構](versioning-and-compatibility.md)
- [決策 0007：採 Nickel 作為組合語言](../decisions/0007-nickel-as-composition-language.md)
- [決策 0008：雙實現路徑](../decisions/0008-static-and-dynamic-realizations-share-one-graph.md)
- [第一個 demo 路線圖](../planning/roadmap-first-demo.md)

