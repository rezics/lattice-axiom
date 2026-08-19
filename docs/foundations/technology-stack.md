---
title: 已選技術棧與採用邊界
status: active
type: reference
updated: 2026-08-19
---

# 已選技術棧與採用邊界

本頁集中記錄 Lattice Axiom 已接受的技術選擇、預定用於第一個 demo 的函式庫，以及每項依賴不得越過的架構邊界。重大選擇仍以 ADR 為準；本頁是便於實作與審查的單一入口，不取代各決策的理由。

## 採用原則

- 套件系統、模組圖、區塊生命週期、世界生成與持久化語義是產品差異層，由專案掌握。
- Nickel 負責宣告、組合、合約與參數化；Rust 負責有副作用的解析、取得、建置、信任與執行。
- GPU、儲存引擎、視窗與其他第三方型別藏在專案門面後，不得滲入核心內容契約。
- 只有 `accepted` 選擇是實作基線；`demo baseline` 項目仍須以最小原型鎖定精確 crate 與版本。
- 每項外部依賴都要記錄版本、授權、來源與其替代的自研工作；升級需通過相應契約測試。

## 已接受的核心選擇

| 領域 | 技術 | 專案中的角色與邊界 | 狀態 | 上游 |
| --- | --- | --- | --- | --- |
| 實作語言與建置 | Rust + Cargo workspace | 實作套件核心、遊戲核心、工具與執行期；crate 邊界隔離第三方系統 | accepted | [rust-lang/rust](https://github.com/rust-lang/rust)、[rust-lang/cargo](https://github.com/rust-lang/cargo) |
| 套件宣告與遊戲組合 | Nickel；Rust 透過 `nickel-lang` 嵌入 | `package.ncl`、`game.ncl`、overlay、合約與宣告式建置描述；不進入遊戲熱路徑 | accepted | [nickel-lang/nickel](https://github.com/nickel-lang/nickel)、[`nickel-lang` Rust API](https://docs.rs/nickel-lang/latest/nickel_lang/) |
| 套件解析與實現 | Lattice 自研 Rust package kernel | 來源取得、版本與能力解析、信任政策、內容雜湊、lock、建置、載入與啟用；不依賴 Nickel 的實驗性 package manager | accepted | 專案內部實作；見[套件管理架構](../architecture/package-management.md) |
| GPU 與 shader | wgpu + WGSL | 唯一 demo GPU 後端；只由 `lattice-render-wgpu` 使用 | accepted | [gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) |
| 世界儲存 | RocksDB | `WorldStorage` 的 demo 生產實現；鍵、column family 與壓實細節不得穿過門面 | accepted | [facebook/rocksdb](https://github.com/facebook/rocksdb) |

## 第一個 demo 的函式庫基線

下列項目已被選為 demo 起點，但精確 crate、feature、版本與平台支援要在對應里程碑以原型鎖定。

| 領域 | 預定技術 | 邊界 | 狀態 | 上游 |
| --- | --- | --- | --- | --- |
| 視窗與輸入 | winit | 只存在於宿主與平台層 | demo baseline | [rust-windowing/winit](https://github.com/rust-windowing/winit) |
| 數學 | glam | 經核心型別或薄轉接層使用，避免成為持久格式 | demo baseline | [bitshifter/glam-rs](https://github.com/bitshifter/glam-rs) |
| ECS 儲存 | hecs | 提供資料儲存；主迴圈與排程由核心控制 | demo baseline | [Ralith/hecs](https://github.com/Ralith/hecs) |
| 體素網格 | block-mesh | 位於區塊網格編譯器內，不定義世界資料模型 | demo baseline | [bonsairobo/block-mesh-rs](https://github.com/bonsairobo/block-mesh-rs) |
| 動態原生載入 | libloading | 只負責載入；ABI、工具鏈與信任驗證由 Lattice 實作 | demo baseline | [nagisa/rust_libloading](https://github.com/nagisa/rust_libloading) |
| 序列化 | serde + bincode | serde 轉換強型別模型；bincode 只放在自有 schema envelope 內 | demo baseline | [serde-rs/serde](https://github.com/serde-rs/serde)、[bincode-org/bincode](https://github.com/bincode-org/bincode) |
| 開發者 UI | egui | 除錯與觀測疊加層，不成為玩家 UI 架構 | demo baseline | [emilk/egui](https://github.com/emilk/egui) |
| RocksDB Rust 綁定 | rust-rocksdb | `WorldStorage` 實現候選；須先驗證原生建置、備份與平台支援 | demo baseline | [rust-rocksdb/rust-rocksdb](https://github.com/rust-rocksdb/rust-rocksdb) |

## 刻意延後

- Rapier、glTF 角色管線與柔體能力等待垂直切片證明需求。
- WASM 元件模型、穩定第三方 ABI、一般版本求解器、registry、簽章與二進位快取不進入第一個 demo。
- Bevy、Godot 或其他完整引擎不作為宿主。
- Nickel 上游的 package manager 目前是實驗功能，不作為 Lattice 套件生態的解析與發布基礎。

## 版本與授權記錄

實作開始後，每個外部依賴都應在 workspace lock 與第三方 notices 中保存精確版本和授權。若上游倉庫、維護狀態、授權或 API 穩定性發生重大變化，先更新本頁與相關 ADR，再調整依賴。

## 相關文件

- [決策 0005：Rust 與自研邊界](../decisions/0005-rust-and-focused-build-boundary.md)
- [決策 0006：wgpu 渲染門面](../decisions/0006-wgpu-behind-rendering-facade.md)
- [決策 0009：RocksDB 權威世界快照](../decisions/0009-rocksdb-authoritative-world-snapshots.md)
- [決策 0010：以 Nickel 驅動套件系統](../decisions/0010-nickel-driven-package-system.md)

