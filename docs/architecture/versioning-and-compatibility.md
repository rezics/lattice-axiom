---
title: Bevy、內容與持久化的版本相容性
status: proposed
type: architecture
updated: 2026-08-19
decision:
  - ../decisions/0003-no-global-version-switch.md
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0016-stage-content-composition-on-bevy.md
---

# Bevy、內容與持久化的版本相容性

## 結論

相容性沿真正擁有資料或契約的邊界處理。Bevy 版本由 Cargo 鎖定並透過程式碼遷移；存檔、內容 ID、生成器、資產與未來網路協定各自版本化。產品版本可以供發布和支援使用，但不替代任何一項實際相容性判斷。

## 六種版本座標

| 座標 | Owner | 用途 | 不得代替 |
| --- | --- | --- | --- |
| 產品 SemVer | 發布流程 | 發行說明、更新與支援 | 存檔 schema |
| Rust／Bevy lock | Cargo | 精確 crate、feature、source 與建置重現 | world generation provenance |
| stable content ID + definition revision | content owner | 方塊、物品、群系、實體類型 | Bevy Entity／Handle |
| schema ID + version | persistence owner | record decode、validation 與 migration | 產品版本 |
| generator revision + config hash | worldgen owner | 新空間生成與 provenance | 已物化區塊的資料來源 |
| asset format／semantic version | asset owner | importer、sidecar 與 fixture migration | gameplay protocol |

未來若有網路協定或內容 package，會新增自己的座標；不把它們塞進 `engineVersion`。

## Bevy 與 Cargo

Bevy 是編譯期相依：

- `Cargo.toml` 描述允許的依賴與 feature；
- `Cargo.lock` 保存目前建置的精確解析結果；
- toolchain file 保存 Rust compiler channel／version；
- build metadata 可以記錄 commit、lock hash、target 和 profile 供診斷。

Bevy minor 升級視為明確遷移：

1. 閱讀官方 release notes／migration guide；
2. 確認所有 Bevy 生態 plugin 的對應系列；
3. 單獨更新 code 和 lock；
4. 執行 client／headless、可玩 smoke、存檔、asset fixture 與性能回歸；
5. 合併後才讓新建置成為支援基線。

既有世界不需要知道 Bevy 版本才能被讀取；它只需要其 record schema 和 content ID 能由目前程式遷移／解析。build metadata 可保存 Bevy 版本供故障診斷，但不是 decode switch。

## 持久化 envelope

每類長期 record 至少有：

- record kind；
- schema owner／schema ID；
- schema version；
- stable world／dimension／spatial key；
- payload；
- checksum 或 storage 層完整性資訊；
- 與該 record 真正相關的 provenance／content references。

一個 schema owner 必須提供：

- 目前可寫版本；
- 支援讀取的舊版本範圍；
- 逐步 migration 或明確拒絕；
- malformed／unknown-field policy；
- round-trip、golden 和 migration fixture。

不要在每個 record 複製完整 Cargo lock。建置 fingerprint 放在 world metadata／diagnostic manifest；record 只保留自己真正需要的版本與來源。

## Stable content ID

存檔引用 `latticeaxiom:block/stone` 之類穩定 ID，不引用 Rust type name、Bevy asset handle 或 plugin 註冊序號。

content catalog 對移除／更名至少選擇一個明確政策：

- 保留 alias 並遷移到新 ID；
- 以 versioned migration 改寫 snapshot；
- 使用明確 placeholder，但保留原 ID 供恢復；
- 拒絕載入並列出缺失 owner／ID。

不得默默把未知 numeric ID 映射到另一個方塊。

若 runtime 需要 compact numeric palette，每個 chunk snapshot 保存其 stable-ID-to-local-index mapping；local index 只在該 snapshot／working set 有意義。

## 世界生成

對未物化空間，生成輸入至少包含 world seed、座標、generator revision、正規化設定 hash 和真正相關的上游規劃 fingerprint。

對已物化空間，完整 snapshot 是權威。更新 generator：

- 不重算舊區塊；
- 新區塊使用新 revision；
- 邊界由明確規劃／過渡規則銜接；
- regenerate 是顯式、有備份與預覽的破壞性 transaction。

因此，同一世界可合法包含不同 generation epoch；相容性由 snapshot schema 與邊界契約決定，而不是要求全世界共享 generation version。

## 衍生 artifact 與 cache

mesh、collider、navigation、thumbnail 和其他 cache 保存最小 source fingerprint，例如 chunk revision + mesher revision。若來源不匹配就丟棄重建，不為 cache 維護昂貴 migration。

權威 snapshot 不可只因 renderer、Bevy patch 或純視覺 asset 更新而失效。

## Migration 執行

1. 在原資料旁建立 checkpoint／備份。
2. 先讀取並驗證 envelope，不直接原地猜測格式。
3. 依 schema owner 執行純函式 migration chain。
4. 在 staging keyspace／新 world copy 寫入目前版本。
5. 執行 referential、spatial 和 content validation。
6. 原子切換可行時才切換；否則使用可恢復 journal。
7. 記錄 migration receipt、來源版本、目標版本和工具 build。

小型 record 可 lazy migration，但只有 owner 已證明混合版本讀寫安全時才採用。大型世界預設提供離線或 staged migration，不在一個 frame 內卡住遊戲。

## Missing content 與部分相容

載入前先掃描 snapshot／world metadata 所需 stable content ID。診斷區分：

- 純表現資產缺失：可以 placeholder；
- 權威 definition 缺失但有 migration：先遷移；
- 權威 definition 缺失且無 migration：拒絕進入會修改世界的模式；
- 未知可選 component：只有 envelope policy 明確允許 opaque preservation 時才能保留。

「能勉強 decode」不等於「允許保存回去」。read-only recovery mode 與正常 writable mode 必須分開。

## 未來內容分發

若日後引入 package metadata，它可記錄 package version、來源、簽章與相依；這些資料用於安裝／建置。runtime 與存檔仍沿 stable content ID、schema 和 generator provenance 判斷相容。

Bevy 永遠由 Cargo／產品建置選定，不成為內容 package graph 裡可由單一 world 任意換掉的 runtime。

## 驗收

- Bevy patch／minor 升級不修改未變更的 save schema。
- 每個 schema owner 都有 golden old-version fixture 和 migration test。
- content plugin 加入順序改變不改變 stable ID 或 snapshot bytes（不含允許的非規範 metadata）。
- generator 更新後舊區塊保持不變，新區塊記錄新 provenance。
- 缺失權威內容時不 silent remap，也不把未知資料覆寫掉。
- cache fingerprint 不匹配時可安全重建，不啟動 snapshot migration。

## 相關文件

- [決策 0003：不以全域版本代替相容性](../decisions/0003-no-global-version-switch.md)
- [世界持久化](world-persistence.md)
- [模組與內容組合](module-composition.md)
- [內容分發邊界](package-management.md)
- [Bevy 執行期架構](game-engine-runtime.md)
