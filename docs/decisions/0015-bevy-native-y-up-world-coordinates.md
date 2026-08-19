---
title: 採用 Bevy 原生右手 Y-up 世界座標
status: accepted
type: decision
updated: 2026-08-19
---

# 決策 0015：採用 Bevy 原生右手 Y-up 世界座標

## 背景

專案尚無程式碼或既有存檔，因此現在選擇座標約定不需要資料遷移。若保留非 Bevy 原生的軸向，Transform、相機、物理、glTF、gizmo、導航與第三方 plugin 都會持續經過轉換層，增加錯誤與整合成本，卻不產生玩家價值。

Bevy 的 3D 世界採右手座標：`+X` 向右、`+Y` 向上，慣用前方為 `-Z`。

## 決策

1. 世界、物理、渲染、資產交換與持久化共同採 Bevy 原生右手 Y-up 座標。
2. `+X` 表示右方，`+Y` 表示上方，未旋轉物件的慣用 forward 為 `-Z`；距離使用公尺，角度使用弧度。
3. 體素與區塊座標寫作 `(x, y, z)`，其中 `y` 是垂直軸，`x-z` 是水平面。二維世界規劃、地形與領地座標寫作 `(x, z)`。
4. 面方向命名為 `east(+X)`、`west(-X)`、`up(+Y)`、`down(-Y)`、`south(+Z)`、`north(-Z)`。若遊戲設計日後需要不同羅盤語義，必須另行定義，但不得改變軸向。
5. glTF 與一般 3D 資產優先使用 Bevy 的 importer 與座標處理。其他來源格式只在匯入邊界轉換一次，不讓來源軸向滲入執行期。
6. 存檔、網路協定、測試 fixture 與生成 provenance 必須顯式標記座標 schema；不得序列化含混的未命名向量。

## 結果

- Bevy Transform、相機、gizmo、物理 plugin、glTF 與生態工具可以按預設使用。
- 世界生成文件與程式中的二維索引從 `(x, y)` 改為 `(x, z)`；高度統一稱 `y`。
- 區塊鍵仍是三個整數座標，但任何排序、序列化與鄰接測試都必須驗證 Y-up 語義。
- 不維護全域軸轉換矩陣或雙重座標 API。

## 驗證

- 原點放置的 Bevy 相機、光源、物理重力與 glTF fixture 在不加專案軸轉換的情況下方向一致。
- 六個體素面方向的 winding、normal、碰撞與 ray hit 測試一致。
- 區塊 `(0, 1, 0)` 位於 `(0, 0, 0)` 上方；地形高度只改變 `y`。
- 存檔 round-trip 不改變整數區塊座標或高精度世界位置。

## 相關文件

- [資產語義](../architecture/asset-semantics.md)
- [世界生成](../architecture/world-generation.md)
- [Bevy 執行期架構](../architecture/game-engine-runtime.md)
- [Bevy glTF coordinate conversion API](https://docs.rs/bevy/latest/bevy/gltf/convert_coordinates/struct.GltfConvertCoordinates.html)
