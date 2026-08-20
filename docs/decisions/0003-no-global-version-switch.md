---
title: 不以全域版本代替實際相容性
status: accepted
type: decision
updated: 2026-08-20
---

# 決策 0003：不以全域版本代替實際相容性

## 背景

套件、capability、原生 ABI、Bevy host、內容、資產、世界生成演算法與持久化 schema 會以不同速度演進。若用單一 `engineVersion`、`gameVersion` 或 `worldVersion` 決定所有相容性，局部變更會被誤報成整體不相容，真正需要遷移的資料反而難以定位。

這不禁止正常的產品版本或 Bevy 版本。產品版本回答「使用者取得哪次發行」，Cargo 依賴回答「這次建置使用哪些程式庫」；它們都不能取代資料擁有者的 schema 與生成 provenance。

## 決策

1. Lattice Axiom 可以發布正常的產品 SemVer，但不設一個控制全部執行、存檔、網路與生成相容性的全域大版本開關。
2. 每个逻辑 package 使用 SemVer 声明相容范围，`latticeaxiom.lock` 选择精确 package version、source、realization 与 artifact hash。package SemVer 是主要外部相容语言，但不代理 ABI、schema 或精确 build。
3. Bevy 與 Rust crate 的精確版本、來源與checksum由核心 host 的manifest／`Cargo.lock`记录；启用的features由workspace manifests、profile与build receipt记录。Bevy 升級是程式碼庫遷移，不是由存檔中的 `engineVersion` 在執行期分派兩套邏輯；它只在改变 Lattice 外部契约时触发相应 package／capability 版本变化。
4. Portable native ABI、每项 capability interface 与 engine-coupled `EngineBuildId` 独立版本化。`EngineBuildId` 不匹配只判定 exact-build interface 不相容，不能自动宣告整个 package graph 不相容。
5. 每個長期資料擁有者獨立定義 schema ID、schema version、讀取範圍與遷移函式；不得使用 package／產品／ABI version 代替 schema version。
6. 世界生成器保存具名 revision、正規化設定雜湊與必要的實作 fingerprint。相同 API 版本不保證程序輸出完全相同。
7. 已物化世界以保存的快照為權威。新生成內容使用目前生成器；舊內容不因依賴更新而隱式重算。
8. 內容 ID、資產格式、網路協定與存檔 schema 是不同識別軸。只有真實相依存在時，遷移或快取失效才沿該相依傳播。
9. 完整建置或內容集合可保存 fingerprint 供診斷與重現；fingerprint 不同只表示存在差異，不足以單獨判定不相容。
10. 程式碼不得以 `if product_version >= ...` 判斷資料形狀；應交由具名 package dependency、interface negotiation、schema owner 或協定協商處理。

## 識別層次

| 識別 | 回答的問題 | 不負責的問題 |
| --- | --- | --- |
| 產品版本 | 使用者取得哪次可支援發行？ | 某份存檔如何遷移 |
| package ID + SemVer | 一个逻辑套件声明与依赖哪些外部行为？ | 动态 binary layout 或 persisted schema |
| capability interface version | host／provider 提供哪组可调用能力？ | package 来源与 artifact identity |
| native ABI version | loader 与动态库如何安全交换 table／batch？ | gameplay behavior 是否 SemVer 相容 |
| `EngineBuildId` | engine-coupled artifact 是否针对同一精确 host？ | portable module 是否相容 |
| Cargo lock | 這次二進位使用哪些精確 Rust／Bevy 相依？ | 已物化區塊由哪個生成器產生 |
| schema ID + version | 某個資料 owner 如何讀取與遷移資料？ | 整個遊戲是否相容 |
| 內容穩定 ID | 存檔中的方塊、物品或實體類型是什麼？ | 其程式實作是否逐位元相同 |
| 生成 revision／fingerprint | 哪個演算法與設定產生這項空間資料？ | 是否必須自動重算 |
| 資產格式版本 | importer 或 loader 如何解讀資產？ | 玩法 schema 是否改變 |

## 結果

- Bevy 升級、內容更新與存檔遷移不再被綁成同一種版本問題。
- 持久化與生成 provenance 必須維護明確 owner，不能依賴一個方便但含混的整數。
- 精確診斷比比較單一版本複雜，但能指出真正受影響的資料與所需動作。
- 可分发套件、动态 ABI 与 exact engine build 已成为明确识别轴；它们能组合，但不得压扁成单一 `engineVersion`。

## 被否決的方案

### 單一遊戲／世界版本門檻

它把無關變更綁在一起，也會在程式碼中累積永久的版本條件分支。

### 只保存 Cargo lock 或產品版本

它們能識別二進位或發行，卻不能說明某段持久化資料的 owner、schema 與合法遷移。

### 只保存完整閉包雜湊

它能判斷兩次建置不完全相同，卻不能指出差異是否影響某個區塊、資產或存檔欄位。

## 驗證

- Bevy patch／minor 升級後，未變更 schema 的存檔仍通過 round-trip。
- 同一 package 的 portable artifact 在所需 ABI／capability range 仍成立时跨 Bevy 升级加载；engine-coupled artifact 以 `EngineBuildId` 精确拒绝。
- 生成器更新不改寫既有區塊，新區塊的 provenance 能指出新 revision。
- 每個持久化 record 都能由 schema ID 找到唯一 owner 與遷移測試。

## 相關文件

- [版本與相容性](../architecture/versioning-and-compatibility.md)
- [世界持久化](../architecture/world-persistence.md)
- [決策 0014：採用 Bevy 並以上游能力為預設](0014-adopt-bevy-upstream-first.md)
- [決策 0010：Nickel 驅動套件系統](0010-nickel-driven-package-system.md)
- [決策 0017：版本化原生模組 ABI](0017-versioned-native-module-abi.md)
