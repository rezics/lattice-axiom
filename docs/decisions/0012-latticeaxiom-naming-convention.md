---
title: 統一品牌識別字 latticeaxiom 與命名約定
status: accepted
type: decision
updated: 2026-08-19
---

# 決策 0012：統一品牌識別字 `latticeaxiom` 與命名約定

## 背景

專案正式名稱是 Lattice Axiom，但早期文件以 `lattice-` 作 crate 前綴、以 `lattice.` 作套件命名空間。lattice 不是專案名稱，而是數學、密碼學與多個既有 Rust crate 使用的常用詞；一旦 crate、套件或產物對外發布，同名碰撞與指代混淆的風險最大，從名稱也無法回推專案。

命名同時牽動多個層面，若不一次定案，各層會各自漂移：

- Rust crate 與 workspace 資料夾；
- Nickel 合約庫的匯入路徑與官方套件命名空間；
- lock 檔、發布描述符等產物檔名；
- 散文中的專案簡稱。

Rust 生態對多詞品牌有兩種常見寫法：以連字符保留全名（`lattice-axiom-*`），或壓縮為單一 token（如 `opentelemetry-*`）。本專案的元件名本身就含連字符（`render-wgpu`、`storage-rocksdb`），品牌若也含連字符，名稱中品牌與元件的邊界將不可辨識。

## 決策

1. 專案唯一的機器識別字是小寫壓縮的 **`latticeaxiom`**；散文與標題一律寫完整名稱「Lattice Axiom」，不再以「Lattice」單獨作專案簡稱。
2. Rust crate 一律命名為 `latticeaxiom-<component>`，元件段用 kebab-case；對應 Rust 路徑為 `latticeaxiom_<component>`；workspace 資料夾名與 crate 名一致。
3. 識別字為單一 token 後，名稱中的第一個連字符即品牌與元件邊界；`latticeaxiom-render-wgpu` 的元件是 `render-wgpu`，無歧義。
4. Nickel 與套件生態沿用同一識別字：合約庫為 `latticeaxiom.lib`，匯入路徑為 `import "latticeaxiom/…"`；官方套件命名空間為 `latticeaxiom.`（如 `latticeaxiom.official`）；lock 檔為 `latticeaxiom.lock`；發布描述符為 `latticeaxiom-package.json`。
5. CLI 套件名為 `latticeaxiom-cli`；實際可執行檔可另定短別名，屬使用介面問題，不改變識別字。
6. 既有 Git 倉庫名 `lattice-axiom` 與 `lattice-axiom-demo` 保持不變；倉庫名屬 GitHub 命名空間，不承擔程式碼與產物識別字的職責。

## 結果

- crate、命名空間、產物與文件共用同一識別字，搜尋 `latticeaxiom` 能枚舉全部相關成員，對外發布幾乎不可能碰撞。
- 文件中約六十處識別字已從 `lattice-*`、`lattice.*` 更新；散文簡稱同步統一為 Lattice Axiom。
- 負面後果：識別字有 12 字元，程式碼、表格與記錄會更寬；接受此代價換取無歧義與可搜尋性。

## 被否決的方案

### 保留 `lattice-*`

lattice 不是專案名稱，泛用詞碰撞風險最大，文件與生態成長後只會更難改。

### `lattice-axiom-*`

與倉庫名一致，但品牌與元件邊界在名稱中無法辨識（`lattice-axiom-render-wgpu`），且是候選中最長的前綴。

### `axiom-*` 或另造短代號

axiom 同樣泛用且只覆蓋專案名一半；另造代號則要維護第二套名稱體系，違背單一識別字的目標。

## 驗證

- 全倉庫搜尋 `lattice[-./]` 不再有舊識別字（倉庫名與外部連結除外）。
- 大寫「Lattice」在文件中只以「Lattice Axiom」形式出現。
- 新增 crate、套件、產物與文件引用皆以 `latticeaxiom` 識別字命名；審查時以識別字搜尋可枚舉全部成員。

## 相關文件

- [已選技術棧與採用邊界](../foundations/technology-stack.md)
- [Nickel 驅動的套件系統與雙實現路徑](../architecture/package-management.md)
- [第一個可玩 demo 路線圖](../planning/roadmap-first-demo.md)
