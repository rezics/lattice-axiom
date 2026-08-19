---
title: 模組核心與宣告式組合
status: proposed
type: explanation
locale: zh-Hant
updated: 2026-08-19
decision:
  - ../decisions/0003-no-global-version-package-scoped-compatibility.md
  - ../decisions/0010-nickel-driven-package-system.md
  - ../decisions/0008-static-and-dynamic-realizations-share-one-graph.md
---

# 模組核心與宣告式組合

> 模組組合採 Nickel，靜態建置與資料夾動態載入共用同一 `LockedGameGraph`，已由[決策 0010](../decisions/0010-nickel-driven-package-system.md)與[決策 0008](../decisions/0008-static-and-dynamic-realizations-share-one-graph.md)採納。長期穩定 ABI、WASM 與 registry 仍未決定。

## 核心命題

Lattice Axiom 不把遊戲理解為「一個完成的執行檔加上一批外掛」，而是把某次可玩的遊戲視為核心、機制模組、內容模組、資產與組態所形成的組合結果。

```text
遊戲核心
+ 官方內容
+ 第三方內容
+ 組態
+ 工具鏈輸入
        ↓
Nickel 組合 + package kernel 解析
        ↓
LockedGameGraph
        ↓
BuildPlan → RuntimeImage
        ↓
可執行的遊戲閉包
```

「模組」是原始碼與組織層級的概念，不必等於執行期的動態抽象。

## 核心與內容的邊界

核心應提供一般機制：

- 實體、元件、查詢與系統排程
- 世界、區塊與生成介面
- 物品、配方與登錄表
- 渲染、物理、網路與持久化的共通能力
- 資產生命週期
- 模組發現、解析與註冊政策；v1 不支援執行期卸載

內容模組則提供具體實例：

- 生物、群系、方塊、物品與結構
- AI、移動、戰鬥與表現設定
- 模型、材質、動畫、音效與粒子
- 可重用或專案專屬的遊戲規則

判斷方式不是「官方或第三方」，而是「通用機制或具體內容」。

## 模組層級

| 層級 | 典型內容 | 載入後的理想表示 | 主要風險 |
| --- | --- | --- | --- |
| 資料模組 | 物品、配方、群系參數、實體模板、動畫圖 | 一般資料、數值 ID、資產控制代碼 | 結構驗證、版本遷移 |
| 原生編譯模組 | ECS 系統、AI、特殊移動或生成器 | 排程項目、函式指標、已知型別 | ABI、信任、記憶體安全 |
| 沙箱模組 | WASM 或其他受限執行單元 | 批次呼叫、受控資源控制代碼 | 邊界成本、能力設計 |
| 直譯腳本 | 快速迭代與小型行為 | VM 內程式與粗粒度主機 API | 熱路徑成本、除錯 |

這些層級可以共存，不需要所有內容都採同一種執行模型。

## 發現與實現是兩個問題

### 模組如何被發現

- 宣告式遊戲定義
- 鎖定檔與套件管理器
- 建置時產生的靜態清單
- 啟動時掃描資料夾
- 開發期間的熱重載來源

### 模組如何被執行

- 靜態原生連結
- 動態原生載入
- 沙箱 WASM
- 直譯腳本
- 純資料編譯

所有發現方式最後必須產生相同的 `LockedGameGraph`。之後的建置計畫、登錄表、排程器、資產編譯器與渲染器不需要知道模組原本來自 Nickel profile、既有 lock 還是資料夾掃描。

## 效能模型

核心原則是：**模組邊界存在於組合與註冊階段，而不是大量重複的熱路徑。**

不理想的執行方式：

```text
每個實體 × 每個 tick
    ↓
查詢模組管理器
    ↓
雜湊表查找
    ↓
事件匯流排
    ↓
跨 VM 呼叫
```

理想方向：

```text
載入模組
    ↓
解析相依性與能力
    ↓
註冊元件、系統與資產
    ↓
配置數值 ID
    ↓
建立查詢、原型與排程
    ↓
遊戲迴圈直接批次執行系統
```

沙箱或腳本 API 應優先採用粗粒度呼叫，例如一次處理一批符合查詢的實體，而不是每個實體跨邊界一次。

「幾乎無可量測額外成本」是待基準測試的目標，不是由架構名稱自動保證的結果。

## 宣告式組合與遊戲閉包

官方遊戲、伺服器規則集與整合包是一份描述「這個遊戲由什麼構成」的 Nickel 根設定檔，而不只是 `mods/` 目錄：

```nickel
let lattice = import "lattice/game.ncl" in
{
  packages = {
    "lattice.runtime" = {
      source = 'Registry { registry = "official", version = "=0.1.0" },
      realization = 'NativeStatic,
    },
    "lattice.official" = {
      source = 'Registry { registry = "official", version = "=0.1.0" },
      realization = 'NativeStatic,
    },
    "example.dragon" = {
      source = 'Path "./packages/dragon",
      realization = 'NativeDynamic,
    },
  },
} | lattice.GameProfile
```

Nickel 負責套件宣告、合約、合併、overlay 與參數化；求值結果透過嵌入 API 直接轉成有版本的 Rust `CompositionSpec`，再由 package kernel 解析為 `LockedGameGraph`。完整模型與雙路徑見[Nickel 驅動的套件系統與雙實現路徑](package-management.md)。

根 manifest 可以有自己的元套件發行版本，但它不支配內部套件、演算法、能力契約與資料 schema 的版本。Lattice Axiom 不使用一個 `gameVersion` 或 `worldVersion` 作為所有相容性的總開關。

| 層 | 作用 |
| --- | --- |
| `CompositionSpec` | 保存 Nickel 合約與合併後的套件要求、能力需求、實現偏好與根政策 |
| `LockedGameGraph` | 決定並鎖定實際套件、來源、內容雜湊、能力提供者、adapter 與 fallback |
| `BuildPlan` | 把已鎖定圖轉成可執行的建置、資產編譯、封裝與快取工作 DAG |
| `RuntimeImage` | 保存啟動與熱路徑使用的 ID、登錄表、排程、資產索引與窄入口 |
| Capability contract | 判斷提供者與使用者能否交換同一語義 |
| Owner schema | 由資料擁有者讀取與遷移自己的持久化資料 |
| Artifact receipt | 記錄某項編譯或生成產物真正依賴的子圖 |

package kernel 輸出的可識別輸入可能包括：

- 核心與模組版本或內容雜湊
- 能力契約版本與所選提供者
- 組態與資產雜湊
- 編譯器、資產工具與目標平台
- 信任政策與實現方式
- 授權與再散布條件

完整解析圖仍可計算根雜湊，以快速確認兩個集合完全相同；根雜湊不同只代表存在差異，不能直接宣布整體不相容。解析器必須沿差異子圖判斷受影響的能力、資料與產物。

對世界生成、資產編譯和其他可重算結果，快取鍵應由實際生產者、正規化組態與輸入產物雜湊組成。更新不相關套件不應使產物失效。

完整候選模型見[版本、相依性與相容性架構](versioning-and-compatibility.md)。

## 靜態、動態與沙箱的取捨

| 實現方式 | 可能優點 | 主要成本 |
| --- | --- | --- |
| 靜態原生 | LTO、跨模組內聯、移除未使用程式碼、已知完整功能集合 | 重建與連結時間、二進位快取、授權與完全信任 |
| 動態原生 | 更新與開發迭代較快、可掃描載入 | ABI 穩定性、平台差異、仍是完全信任程式碼 |
| 沙箱 WASM | 能力隔離、較可控的第三方執行 | 邊界與資料交換成本、API 粒度限制 |
| 直譯腳本 | 快速創作、熱重載與低門檻 | 效能、工具鏈與行為可預測性；不在 v1 範圍 |

套件與模組 API 可以一致，但實現方式不必一致。套件宣告應明確列出支援的實現方式與所需能力。

## 已知限制

- 靜態連結會把第三方原生程式碼變成遊戲本體的一部分，不能當成安全沙箱。
- 大型靜態整合包需要可信建置者、簽章、快取與供應鏈政策。
- 不同授權在靜態連結與再散布時可能產生不同義務，必須把授權中繼資料視為一等資訊。
- 動態卸載比動態載入更困難，牽涉實體、資產、排程與持久化資料的所有權；v1 明確不支援。
- 可重現建置不等於多人確定性；兩者需要分開設計與驗證。

## 待驗證假說

- 第一方內容只使用公開模組 API，是否仍能達到所需的開發效率與效能？
- 同一第二內容包以靜態與動態實現載入時，能否得到相同 ID、生成結果與可接受的效能差異？
- 粗粒度沙箱 API 是否能支援真正複雜的第三方機制？
- 模組解析與資產編譯能否讓執行期不再追蹤內容來源？

## 相關文件

- [專案願景與設計支柱](../foundations/project-vision.md)
- [版本、相依性與相容性架構](versioning-and-compatibility.md)
- [Nickel 驅動的套件系統與雙實現路徑](package-management.md)
- [渲染架構與擴充邊界](rendering.md)
- [待決問題](../planning/open-questions.md)
