---
title: 統一品牌識別字 latticeaxiom 與命名約定
status: accepted
type: decision
updated: 2026-08-19
---

# 決策 0012：統一品牌識別字 `latticeaxiom` 與命名約定

## 背景

專案正式名稱是 Lattice Axiom。早期文件曾以過度泛用的 `lattice-` 作 crate 前綴；對外發佈後容易與數學、密碼學及既有 Rust crate 混淆。名稱也必須能在 Rust crate、Bevy plugin、內容穩定 ID 與診斷輸出間一致搜尋。

## 決策

1. 專案唯一的機器識別字是小寫壓縮的 **`latticeaxiom`**；散文與標題寫完整名稱「Lattice Axiom」，不以「Lattice」單獨作專案簡稱。
2. Rust crate 命名為 `latticeaxiom-<component>`，元件段使用 kebab-case；對應 Rust 路徑為 `latticeaxiom_<component>`。
3. Bevy plugin 與 plugin group 的 Rust 型別採清楚的領域名稱，例如 `LatticeAxiomPluginGroup`、`WorldPlugin`、`PersistencePlugin` 與 `OfficialContentPlugin`；不在每個型別前重複建立引擎 facade 名稱。
4. 需要跨重啟穩定的內容 ID 使用 `<namespace>:<kind>/<path>` 形式；官方 namespace 是 `latticeaxiom`，例如 `latticeaxiom:block/stone`。外部內容使用自身可治理的 namespace。Rust 型別名、Bevy `Entity`、`Handle` 或 `TypeId` 不作穩定 ID。
5. CLI crate 命名為 `latticeaxiom-cli`；實際可執行檔可另定短別名，屬使用介面問題。
6. Git 倉庫名 `lattice-axiom` 保持不變；倉庫名稱不承擔程式碼或持久化識別字的職責。

## 結果

- crate、內容識別與文件共用可搜尋的品牌 token。
- Bevy 原生概念保留 Bevy 慣用名稱，不用品牌前綴重新包裝一遍。
- 穩定內容 ID 與 process-local Bevy 識別清楚分離。
- `latticeaxiom` 較長；接受這項成本換取無歧義。

## 被否決的方案

### 保留 `lattice-*`

`lattice` 不是完整專案名稱，泛用詞碰撞風險高。

### `lattice-axiom-*`

品牌與元件邊界不清楚，且名稱更長。

### 為所有 Bevy 能力建立 `LatticeAxiom*` 對應型別

這會形成無價值的 facade，增加升級與文件成本，違反上游優先原則。

## 驗證

- 新增 crate 可由搜尋 `latticeaxiom-` 枚舉。
- 存檔中的內容引用使用穩定命名空間，不出現 Bevy `Entity`、`Handle` 或 Rust `TypeId`。
- 程式碼直接使用 Bevy App、World、Transform、Mesh 等名稱，不建立一對一別名。

## 相關文件

- [技術棧](../foundations/technology-stack.md)
- [模組與內容組合](../architecture/module-composition.md)
- [決策 0014：採用 Bevy 並以上游能力為預設](0014-adopt-bevy-upstream-first.md)
