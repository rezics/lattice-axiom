---
title: 以 Nickel 驅動套件系統並以 Rust 執行
status: accepted
type: decision
locale: zh-Hant
updated: 2026-08-19
supersedes:
  - 0007-nickel-as-composition-language.md
---

# 決策 0010：以 Nickel 驅動套件系統並以 Rust 執行

## 背景

[決策 0007](0007-nickel-as-composition-language.md)已選擇 Nickel 作為組合語言，但把它定位成可替換的輸入前端，並把「正規化 JSON」定為求值器與 resolver 之間的架構邊界。這使套件系統的語義被拆在 Nickel 合約、JSON schema 與 Rust 型別之間，也弱化了「遊戲本身是套件組合結果」這項核心假說。

Nix 與 NixOS 的重要啟發不是某個序列化格式，而是宣告語言直接參與構造套件、設定與建置圖；包管理器則掌握來源、store、derivation 與實現。Nickel 本身源自把 Nix 式語言能力與 Nix package manager 解耦，提供原生合併、合約與可嵌入求值器，適合成為 Lattice 套件系統的宣告與組合核心。

Nickel 上游另有 package manager，但目前仍標記為實驗功能，官方發行版預設停用。Lattice 不能把自己的套件身分、能力解析、信任、建置與發布語義委託給這項未穩定功能。

## 決策

1. 套件系統是 Lattice Axiom 的產品核心與控制平面之一，不只是載入模組前的配置工具。
2. `package.ncl`、`game.ncl`、整合包、使用者 overlay 與宣告式實現描述採 Nickel。專案提供版本化的 `lattice.lib`，包含 `Package`、`GameProfile`、`Capability`、`Realization` 與相關合約和組合函式。
3. `lattice-compose` 透過公開且以穩定為目標的 `nickel-lang` Rust API 嵌入求值器。完整求值後直接以 serde 轉成專案的 Rust 強型別，不產生供下一層消費的 JSON 中間檔。
4. 組合與實現管線使用四個有版本的語義型別：`CompositionSpec`、`LockedGameGraph`、`BuildPlan` 與 `RuntimeImage`。每一層只表達下一階段所需的不變量，不能以無型別字典取代。
5. Nickel 負責宣告、函式抽象、記錄合併、預設值、overlay、合約與純資料的實現描述；Rust package kernel 負責來源取得、版本與能力解析、循環與衝突、信任政策、內容雜湊、lock、store／快取、建置、載入、啟用與診斷。
6. v1 不依賴 Nickel 上游的實驗性 package manager。遠端來源、registry 與套件 lock 由 Lattice 定義；日後可以重用其穩定元件，但必須先經獨立決策與相容性層。
7. `lattice.lock`、發布描述符與診斷輸出可以使用 canonical JSON 或其他有版本格式。它們是強型別模型的持久化或交換表示，不是求值器、resolver、builder 與 loader 之間的語義 API。
8. 已發布套件可攜帶由強型別模型產生的封裝描述符，使一般遊戲啟動不必重新求值每個套件來源；描述符的 schema、內容雜湊與產生工具鏈必須寫入 lock 並可由原始 Nickel 重建。
9. Nickel 只存在於組合與規劃期。遊戲熱路徑只看 `RuntimeImage` 產生的數值 ID、登錄表、連續資料、排程與窄函式指標，不查詢 Nickel AST、字串套件鍵或 package kernel。
10. 本決策取代決策 0007 中「canonical JSON 是內部架構邊界」以及「Nickel 只是可替換來源前端」的部分；0007 其餘不在熱路徑求值、限制匯入與提供來源診斷的護欄繼續有效。

## 管線

```text
package.ncl + game.ncl + overlays + lattice.lib
                         ↓
            Nickel 合併、合約、完整求值
                         ↓ direct serde conversion
                CompositionSpec (Rust)
                         ↓
       source / dependency / capability resolver
                         ↓
                LockedGameGraph (Rust)
                         ↓ realization planner
                   BuildPlan (Rust)
                         ↓ build / load / register
                  RuntimeImage (Rust)
                         ↓
            IDs、tables、schedules、assets
```

## 護欄

- Nickel 求值不得自行存取網路、時鐘或未宣告的主機狀態。遠端來源先由 Rust package kernel 取得並放入受控來源根，再交給求值器。
- import 根目錄、求值時間、遞迴、記憶體與輸出大小必須有明確政策；不可信套件不能取得任意檔案系統能力。
- `lattice.lib` 的 Nickel 合約與對應 Rust 型別由同一組版本與 round-trip／golden test 維護，不再建立第三份手寫 JSON schema 作內部真相來源。
- 持久化格式的 canonicalization 只服務雜湊、重建與交換；Rust 型別和語義版本決定相容性，文字鍵順序不決定業務語義。
- source graph、locked graph 與 runtime graph 必須分開，避免把未取得來源、版本範圍或作者函式帶進執行期。

## 結果

- Nickel 從「產生 JSON 的配置前端」提升為套件宣告與遊戲組合的核心語言，符合專案的產品假說。
- resolver 與執行器保留 Rust 的型別、安全邊界、效能與副作用治理，不把下載、信任或建置塞進純配置求值。
- 移除 JSON 內部總線後，少一層 schema 漂移與序列化成本；代價是 `CompositionSpec` 與 `lattice.lib` 必須嚴格共同版本化。
- lock 與發布描述符仍可使用可讀格式，除錯與可重現性不因內部強型別化而消失。
- 專案承擔自己的套件解析與生態治理，不能直接把 Nickel 的函式庫 package manager 當成遊戲 package manager。

## 被否決的方案

### 保留 canonical JSON 作內部總線

JSON 適合交換與持久化，但若每一個內部階段都以它作 API，合約、schema 與 Rust 型別會成為三份真相，也無法表達各階段已證明的不變量。

### 把 resolver、下載與建置全部寫在 Nickel

這會把網路、檔案系統、信任、併發、取消與錯誤恢復塞進配置語言，削弱可測性與安全邊界。Nickel 產生宣告式計畫，Rust 負責解析和執行。

### 直接採用 Nickel 實驗性 package manager

它的目標是管理 Nickel 函式庫，且仍可能破壞性變更；Lattice 需要的是遊戲套件、能力、實現方式、工具鏈、資產與世界相容性，兩者不是同一問題。

### 執行期保留 Nickel 值

這會把求值器、動態錯誤與字串查詢帶進遊戲熱路徑，也使已解析圖、持久化與多人握手失去窄而穩定的表示。

## 驗證

- 同一組 Nickel 輸入在 CLI 與嵌入宿主直接得到語義相同的 `CompositionSpec`，不建立 JSON 中間檔。
- Nickel 合約錯誤指出來源檔、欄位、合併來源與期望語義；Rust 解析錯誤則指出套件、能力、來源與可行修復。
- `CompositionSpec`、`LockedGameGraph`、`BuildPlan` 與 `RuntimeImage` 各有 schema／語義版本、round-trip 與 golden test。
- 隨機化來源發現與登錄順序不改變 lock graph、數值 ID 或種子區塊雜湊。
- 執行期核心的相依圖不包含 Nickel 求值器，熱路徑不查詢 package kernel。

## 外部依據

- [Nickel 原始碼倉庫](https://github.com/nickel-lang/nickel)
- [Nickel 穩定 Rust 嵌入介面](https://docs.rs/nickel-lang/latest/nickel_lang/)
- [Nickel package management 的實驗狀態](https://nickel-lang.org/user-manual/package-management/)
- [Nix derivation](https://nix.dev/manual/nix/stable/language/derivations)
- [Nix flake lock files](https://nix.dev/manual/nix/stable/command-ref/new-cli/nix3-flake.html#lock-files)

## 相關文件

- [已選技術棧與採用邊界](../foundations/technology-stack.md)
- [套件管理架構](../architecture/package-management.md)
- [模組核心與宣告式組合](../architecture/module-composition.md)
- [版本、相依性與相容性架構](../architecture/versioning-and-compatibility.md)
