---
title: 空間生成採領地遞迴委派
status: accepted
type: decision
updated: 2026-08-09
---

# 決策 0004：空間生成採領地遞迴委派

## 背景

「每個維度只有一個地形或洞穴演算法」無法表達群系之間巨大的地貌與地下網路差異；但若所有群系都能直接覆寫相同密度場或拓撲圖，又會回到載入順序、優先級競賽和跨邊界斷裂。

地形密度可以在邊界局部混合，洞穴拓撲卻不能靠逐點平均連接。兩者需要共同的空間所有權規則，以及各自適合的邊界契約。

## 決策

1. 每個維度只有一個 `DimensionGenerationCoordinator`。它負責解析領地委派、共通通道、預算、確定性與跨領地約束，不代表維度只能使用一個內容生成演算法。
2. 生成提供者的唯一性以「生成通道 × 空間所有權域」為作用域，而不是以整個維度為作用域。
3. 領地形成可巢狀的委派樹。某通道在一個位置或拓撲域中的主要提供者，是包含該位置、已宣告該通道且最具體的領地；若沒有則繼承父領地或維度預設。
4. 同層領地的核心區由 `Territory Atlas` 先解析為互斥所有權，不能用模組註冊順序或最後寫入者決定勝負。
5. 地形群系可以在自己的領地內完整提供 `terrain.base`，而不只向維度地形增加少量噪聲。維度與長程系統提供山脈軸線、流域、地質和結構等計畫與約束，局部群系決定如何實現。
6. 地下群系可以在自己的三維拓撲所有權域內完整提供 `cave.topology`，而不只修改洞壁形態。
7. 洞穴父域以必要或可選入口、水文連接、保護體積與預算把子域委派給群系；子域可以使用完全不同的拓撲演算法，但必須回傳可驗證的圖與契約結果。
8. 地形、洞穴形態與其他連續場在邊界帶依具名 `BoundaryProfile` 合成；連通性、最小淨空、水量守恆等不變條件不得被數值平均掉。
9. 群系必須宣告空間影響形態，例如僅地表、地表加淺層、有界三維體、垂直柱或整個維度。地表群系不自動擁有整根地下柱。
10. 多個有界細節、支線、內容與約束提供者仍可在主要所有者之下合成，但它們不能暗自建立第二個未協調的基礎地形或拓撲主幹。

## 所有權解析

概念規則為：

```text
owner(channel, domainOrPosition)
  = mostSpecificDeclaredTerritory(channel, domainOrPosition)
  ?? inheritedParentProvider(channel)
  ?? dimensionDefaultProvider(channel)
```

「最具體」來自領地包含關係和已解析空間所有權，不來自任意數值優先級。同一查詢只評估勝出者、少量相鄰邊界提供者和具空間交集的有界貢獻者，因此已註冊群系總數不直接成為每點求值成本。

## 通道基數

| 通道 | 基數與作用域 | 合成方式 |
| --- | --- | --- |
| `generation.coordinator` | 每維度一個 | `SelectOne` |
| `terrain.base` | 每個地形所有權域一個 | `TerritorialDelegation` |
| `terrain.detail` | 每域多個有界貢獻者 | `BudgetedMerge` |
| `terrain.boundary` | 每條已解析領地鄰接邊一個結果 | `BoundaryResolve` |
| `cave.topology` | 每個拓撲所有權域一個 | `TerritorialDelegation` |
| `cave.morphology` | 每個地下群系核心區一個；邊界可有少量多個 | `TerritoryWinner`／`BoundaryBlend` |
| `cave.branch` | 每域多個有界貢獻者 | `SpatialArbitration` |
| `*.constraint` | 每域多個 | `ConstraintSet` |

## 洞穴委派契約

父拓撲域至少可向子域提供：

```text
CaveTopologyDelegation {
  domainId
  requiredPortals
  optionalPortals
  hydraulicConnections
  mandatoryDestinations
  protectedVolumes
  topologyTargets
  budget
}
```

子域回傳自身節點、邊、入口實現、掛接點、保留體積、未滿足的可選需求與診斷。必要入口若無法滿足，必須觸發父域重新路由或顯式失敗，不能在區塊末端強行鑿穿。

## 結果

- 沙丘、山地、沼澤、浮島等群系可以使用完全不同的地形表示。
- 岩溶、熔岩管、斷層裂隙與文明地下網路可以使用完全不同的局部拓撲演算法。
- 維度仍有一個可診斷協調點，跨領地系統仍能維持連續性。
- 「覆蓋」被改寫為載入期可解析的空間委派，不會退化為執行期最後寫入者。
- 需要為領地巢狀、入口協商、拒絕與重路由建立明確資料結構和驗證器。

## 被否決的方案

### 每個維度一個地形或洞穴演算法

它只能把群系差異壓縮成參數，無法允許真正不同的高度場、體積場或洞穴網路模型。

### 群系任意覆寫最終密度與拓撲

它無法解釋所有權，依賴順序且會破壞跨領地連續性。

### 所有提供者在邊界盲目混合

拓撲、互斥玩法規則與守恆約束沒有可平均的語義；它們需要入口契約或唯一所有者。

## 修正的既有決策

- [決策 0001](0001-territory-first-biome-driven-world-generation.md)中的「群系提交意圖」現在明確包含群系可在領地內提供完整的基礎生成能力。
- [決策 0002](0002-hybrid-cave-generation-composition.md)中的「每維度唯一洞穴主幹」修正為「每拓撲所有權域唯一提供者；每維度唯一協調器」。
