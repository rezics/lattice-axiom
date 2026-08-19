---
title: Bevy 渲染、物理、輸入與體素生態調查
status: exploration
type: research
updated: 2026-08-19
---

# Bevy 渲染、物理、輸入與體素生態調查

## 結論

引擎選型已結束：Lattice Axiom 採用 Bevy。本頁不再比較替代 renderer／ECS／完整引擎，而是記錄 Bevy 內建能力與生態 plugin 能否用最少專案程式完成第一個可玩 demo。

截至 2026-08-19，文件基線為 Bevy `0.19.x`，當時 docs.rs 的最新 patch 為 `0.19.1`。Bevy 約每三個月可能有 breaking release，因此精確 patch 和生態 plugin 對應版本必須在實作時由 `Cargo.lock` 固定，不能只憑本頁自動升級。

## Bevy 已直接提供的能力

| 需求 | Bevy 上游能力 | 對 Lattice Axiom 的含義 |
| --- | --- | --- |
| App／組合 | `App`、`Plugin`、`PluginGroup`、custom runner、`SubApp` | 使用正常 App；不做 launcher／host runtime |
| ECS／排程 | Bevy ECS、schedules、SystemSet、states | 直接採用完整上游模型 |
| 固定時間 | `FixedUpdate`、`Time<Fixed>` | gameplay tick 直接使用上游 |
| Headless | `MinimalPlugins` 與可選標準 plugin | 不安裝 renderer；不做 null renderer |
| 非同步 | Bevy task pools | worldgen／mesh／I/O 使用同一 runtime |
| Assets | `AssetServer`、`AssetLoader`、typed assets、glTF | 不做平行 asset service |
| Rendering | Bevy Render／PBR／Material／`RenderApp` extraction | wgpu 是 Bevy 實作細節 |
| UI／診斷 | Bevy UI、gizmos、diagnostics | demo overlay 先用內建能力 |

官方來源：

- [Bevy repository and feature overview](https://github.com/bevyengine/bevy)
- [Bevy 0.19 release](https://bevy.org/news/bevy-0-19/)
- [Bevy `App` API](https://docs.rs/bevy/latest/bevy/app/struct.App.html)
- [`MinimalPlugins` API](https://docs.rs/bevy/latest/bevy/prelude/struct.MinimalPlugins.html)
- [`FixedUpdate` API](https://docs.rs/bevy/latest/bevy/prelude/struct.FixedUpdate.html)
- [render extraction](https://docs.rs/bevy/latest/bevy/render/extract_plugin/index.html)
- [`AssetServer` API](https://docs.rs/bevy/latest/bevy/asset/struct.AssetServer.html)

## 體素候選：`bevy_voxel_world`

[`bevy_voxel_world`](https://github.com/splashdust/bevy_voxel_world) 是第一個 spike 候選，因為它已提供與 Bevy 整合的 chunk spawn／despawn、多執行緒 meshing、voxel edit 和 raycast，且 repository 說明了 Bevy 版本對應。

### 先驗證

- 目前 Bevy 0.19 對應 release 是否能在目標平台建置；
- generator／voxel material API 是否能映射 stable block ID；
- chunk 尺寸、coordinate、view distance 與 streaming 是否可設定；
- edit 是否產生可觀測 revision，以及 mesh job 如何拒絕過期結果；
- collider integration 是否已有官方或生態路徑；
- headless profile 能否只使用資料／generator 部分；
- repository 自述的硬編碼假設是否會阻擋持久化與 Y-up。

### 可能結果

1. **完整採用**：它同時承擔 working set 與 presentation，專案只接 stable ID／persistence。
2. **作 adapter**：權威 chunk 在 domain storage，它承擔 streaming／meshing／raycast。
3. **上游擴充**：缺口適合 PR 或小型公開 hook。
4. **局部替換**：只替換被量測證明不足的 storage／mesher，不重做 renderer。

結果只能由 B1 可玩 spike 決定。

## 物理候選：Avian

[Avian](https://github.com/avianphysics/avian) 是 ECS-first、Bevy-native 物理候選，其 release table 提供 Bevy 版本對應。第一個 spike 要驗證：

- Y-up 重力與 character movement；
- Bevy FixedUpdate integration；
- chunk collider 建立、更新、卸載和 memory；
- ray／shape cast 支援 break／place；
- local-origin／large-world 行為；
- target hardware 上的 fixed tick P95。

採用 Avian 不表示持久化其內部 world。存檔只保存 Lattice Axiom 需要恢復的 pose／velocity／sleep 等穩定 DTO。

若 Avian 有局部缺口，先比較其公開 extension、issue／PR 和其他維護中的 Bevy physics plugin；「可能需要特殊物理」不構成自製 solver 的理由。

## 輸入候選：Bevy 原生或 Leafwing

[Leafwing Input Manager](https://github.com/Leafwing-Studios/leafwing-input-manager) 提供 action mapping、重綁與測試／網路友善的 action state，並列出 Bevy 版本對應。

比較矩陣：

| 驗收 | Bevy 原生 input | Leafwing |
| --- | --- | --- |
| movement／look／jump／break／place | 必須完成 | 必須完成 |
| action remap | 記錄自製資料量 | 驗證內建能力 |
| headless action injection | 寫最小 test | 驗證 mock／action API |
| 維護與升級成本 | 無額外 crate | 多一個版本耦合 |
| replay／network | 非 demo 需求 | 不因未來可能性加權 |

選擇能以更少總程式與測試滿足 demo 的方案；如果 Bevy 原生已足夠，就不加入 plugin。

## 資產狀態候選

[`bevy_asset_loader`](https://github.com/NiklasEi/bevy_asset_loader) 可提供 loading state 與 collection，但 Bevy `AssetServer` 已具備載入、狀態與 dependency 基礎。只有 D0／D1 顯示 loading orchestration 重複且容易錯，才做此 spike。

資產格式與 semantic sidecar 仍以 Bevy loader／glTF 為基線，詳見[資產語義](../architecture/asset-semantics.md)。

## 授權與供應鏈檢查

每個候選鎖定前記錄：

- repository、release／commit 與 Bevy compatibility；
- SPDX license 和 transitive license；
- 最近 release、issue／PR 活躍度與 bus factor；
- 支援 target／feature；
- advisory、unsafe surface 和 native dependency；
- 最小 fallback／移除路徑。

Bevy 本身採 MIT OR Apache-2.0；實際 binary 的完整第三方 notices 仍由 Cargo lock／SBOM 產生，不能只抄本頁。

## Spike 輸出格式

每個候選提交同一份 adoption report：

1. playable build／commit；
2. 使用的 exact versions／features；
3. 使用了哪些 upstream defaults；
4. 必要設定與專案 adapter 行數／責任；
5. 功能、性能、memory、failure 測量；
6. 已知限制和 upstream issue／PR；
7. adopt／extend／fork／reject 結論；
8. 升級、fallback 和 owner。

只有 report 中的 playable reproduction 可觸發自研例外。

## Godot 的邊界

Godot 不參與 renderer／physics runtime 選型。它只在實際作者工作流被 Blender + Bevy 工具阻塞時，作 editor／import pipeline 對照；詳見[Godot 工具鏈對照](godot-toolchain-comparison.md)。

## 相關文件

- [技術棧](../foundations/technology-stack.md)
- [Bevy 執行期架構](../architecture/game-engine-runtime.md)
- [渲染架構](../architecture/rendering.md)
- [Bevy 執行期整合路線](../planning/roadmap-game-engine.md)
