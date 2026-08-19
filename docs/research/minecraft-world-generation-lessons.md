---
title: Minecraft 世界生成與洞穴模組的設計教訓
status: exploration
type: research
updated: 2026-08-09
---

# Minecraft 世界生成與洞穴模組的設計教訓

> 本頁保存外部調查與由此推導的設計教訓，不是下載量排行榜，也不表示 Lattice Axiom 會複製任一模組。下載數跨平台、版本與專案年代不可直接比較；玩家回饋是質性樣本，不是統計調查。

## 調查問題

- 高影響力地形與群系模組分別在修正原版的哪一類問題？
- Minecraft 多次更新地下內容，反映洞穴生成包含哪些相互獨立的層？
- 玩家對洞穴大小、連通、內容與稀有度的回饋有哪些反覆出現的矛盾？
- 哪些相容性問題應在 Lattice Axiom 的數學與 API 設計中提前消除？

## 地表世界生成

| 專案 | 專案自述與功能重點 | 對 Lattice Axiom 的教訓 |
| --- | --- | --- |
| [Tectonic](https://modrinth.com/datapack/tectonic) | 大陸與島嶼、深海、延伸數千至數萬格的山脈、地下河、峽谷、沙丘、高原坡道與濕地 | 宏觀尺度與可通行性要成為顯式生成問題；河流跨越高山時改走地下，說明網路連續性優先於局部地貌 |
| [TerraForged](https://www.curseforge.com/minecraft/mc-mods/terraforged) | 新地形、較完整河流、自訂群系特徵與大量組態，並嘗試納入其他模組群系 | 地形、水文、侵蝕近似和群系內容要分層；昂貴模擬需要規劃格、快取或程序近似 |
| [Terralith](https://modrinth.com/datapack/terralith) | 大量新群系、峽谷、破碎地形、浮島、海溝、洞穴群系與結構 | 群系是地貌、內容、氣氛與探索承諾，不只是顏色；世界建立後難以增刪也提醒生成閉包必須版本化 |
| [William Wythers' Overhauled Overworld](https://modrinth.com/mod/wwoo) | 重做原版群系、增加群系過渡，並改善河流旅行與海岸 | 邊界和旅行成本是第一級輸出，不應等到裝飾階段補救 |
| [Biomes O' Plenty](https://modrinth.com/mod/biomes-o-plenty) | 大量新群系、植物、樹木與材料 | 內容多樣性與地形骨架可以由不同模組提供，核心需要通用整合能力 |
| [Regions Unexplored](https://modrinth.com/mod/regions-unexplored) | 大量地表與地獄群系、方塊與物品 | 群系內容包會持續增長；註冊系統不能依靠固定槽位或中央手工表格 |
| [Continents](https://modrinth.com/datapack/continents) | 拉開陸塊距離並改變大小與形狀 | 噪聲細節不能取代世界尺度的陸海組織 |

### TerraBlender 的相容性教訓

[TerraBlender 的文件](https://github.com/Glitchfiend/TerraBlender/wiki/Getting-started)說明，它把不同群系模組放進各自 region，再依 region 權重選擇。這是務實的相容層，但套件身分因此參與地理分割。

社群案例曾出現 Terralith 群系在與其他大型群系包同時使用時變得極小或幾乎找不到；單一案例不能證明普遍因果，但與「模組 region 加權、群系再於 region 內分配」的結構性風險一致：[案例](https://www.reddit.com/r/feedthebeast/comments/1qxiz6u/terralith_is_overshadowed_by_regions_unexplored/)。

由此得到的設計要求是：

- 機率份額可以按套件治理，但套件不能因此擁有連續地理邊界。
- 群系所有權在同一全域仲裁中決定。
- 增加內容數量不應自動增加套件的世界總份額。
- 邊界必須屬於世界語義，而不是載入器實作細節。

## 洞穴形態與地下內容

| 專案 | 修正的主要問題 | 可吸收的做法 |
| --- | --- | --- |
| [YUNG's Better Caves](https://www.curseforge.com/minecraft/mc-mods/yungs-better-caves) | 舊洞穴重複、死路、缺少形態和水體變化 | 多種洞穴與洞室混合、可調大小和頻率、地下湖河與跨維度組態 |
| [YUNG 作者的演算法說明](https://www.reddit.com/r/feedthebeast/comments/d1t5fg/introducing_yungs_better_caves_a_brand_new_mod/) | 單一噪聲不能產生完整形態節奏 | 使用 cubic、simplex、Perlin 與 Worley noise 形成不同洞型，再以 cellular／Voronoi 區域配置洞型 |
| [Worley's Caves](https://www.curseforge.com/minecraft/mc-mods/worleys-caves) | 長時間挖掘沒有發現、基本隧道重複、網路不連續 | Worley noise、巨大連續網路、分支房間、noise cutoff、扭曲與垂直壓縮組態 |
| [Alex's Caves](https://modrinth.com/mod/alexs-caves) | 洞穴即使改形仍缺少造訪目的 | 少量稀有地下目的地，各自擁有方塊、物品、生物、機制、氣氛與尋找線索 |
| [YUNG's Cave Biomes](https://modrinth.com/mod/yungs-cave-biomes) | 原版地下生態種類不足 | 採 vanilla+ 範圍加入生物、方塊、物品與音樂，顯示地下群系可以是獨立內容能力 |
| [Spelunkery](https://modrinth.com/mod/spelunkery) | 採礦流程重複、洞穴缺少地標與工具策略 | 把材料進度、礦物分布、地標與探索工具一起設計，而非只改幾何 |

## Minecraft 官方更新分別解決不同層

[Minecraft 1.18](https://feedback.minecraft.net/hc/en-us/articles/4414284658701-Minecraft-Caves-Cliffs-Part-II-1-18-0-Bedrock)擴展世界高度與深度，加入 cheese、spaghetti、noodle 噪聲洞穴、含水層、三維群系與大型礦脈。這主要修正空間形態、水體與垂直分布。

[Minecraft 1.19](https://feedback.minecraft.net/hc/en-us/articles/6731464524941-Minecraft-Java-Edition-1-19)加入 Deep Dark、Warden 與 Ancient City，讓地下擁有稀有目的地、特殊規則、風險和獎勵。

[Minecraft 1.21](https://feedback.minecraft.net/hc/en-us/articles/27547857163917-Minecraft-Java-Edition-1-21-Tricky-Trials)加入 Trial Chambers、Trial Spawner、Vault、鑰匙與專屬戰利品，再補上可重複遭遇與挑戰層。

因此「洞穴更新」至少包含五個彼此不能替代的問題：

```text
拓撲連通
幾何形態
地下生態與氣氛
資源與工具進度
目的地、遭遇與獎勵
```

只改其中一層，其他層仍會被玩家感受為「洞穴沒有完成」。

## 玩家回饋呈現的是節奏分歧

Minecraft 官方回饋同時出現偏好巨大洞窟與認為巨大洞窟過於常見的意見。有玩家指出大型空間帶來奇觀，但空曠不等於內容；若過於常見，會增加照明、移動和地表建造負擔，也失去稀有感：[大型洞穴回饋案例](https://feedback.minecraft.net/hc/en-us/community/posts/360077051751-Make-Large-Caves-less-common-Fix-Large-Caves)。另有回饋指出過多洞口會使地表像被穿孔，影響建造：[洞穴密度案例](https://feedback.minecraft.net/hc/en-us/community/posts/4409333938957-Too-much-caves)。

這些樣本不支持「所有玩家喜歡同一尺寸」，反而支持以下設計：

- 保存狹窄、中型、巨型與水下／熔岩等多種形態。
- 讓大型洞窟以稀有度和前後節奏維持奇觀感。
- 把地表開口密度與地下空洞體積分開治理。
- 讓死路、環路、垂直落差與淹水比例成為可量測參數。
- 稀有洞穴必須具有內容、線索與回報，不只增加空氣體積。
- 支援不同世界預設，而不是宣稱單一分布代表所有玩家。

## 對演算法的直接要求

### 地表群系

- 需要多尺度連續領地，不可逐區塊獨立分類。
- 需要可查詢的中心、邊界距離、鄰接與父子領地。
- 需要與註冊順序無關的仲裁，而不是增加環境維數。
- 群系產生環境意圖，跨群系系統負責守住連續性。

### 洞穴

- 拓撲圖和幾何場需要分離，才能同時治理連通與外觀。
- 洞型應使用混合分布，而非全世界共用單一噪聲閾值。
- 跨規劃格入口必須由雙方可重算的共同鍵決定。
- 巨型洞窟、地下河、目的地和普通通道應有不同尺度與稀有度。
- 洞穴群系需要三維領地或明確的父群系關係，不能只沿地表柱狀投影。
- 生成結果需要可輸出拓撲指標，否則只能以截圖評估。

## 仍需補充的調查

- 以相同版本、種子和觀測範圍比較主流地形模組的群系面積、旅行成本與結構相容性。
- 對玩家偏好做真正的問卷或遙測，而不只依靠論壇高互動案例。
- 研究可局部查詢的河網、侵蝕近似與無限三維洞穴圖。
- 比較領地圖譜、獨立標記點程序與相關隨機場在新增大量群系時的面積穩定性。

## 相關文件

- [可組合世界生成架構](../architecture/world-generation.md)
- [可組合洞穴生成架構](../architecture/cave-generation.md)
- [世界生成方向決策](../decisions/0001-territory-first-biome-driven-world-generation.md)
- [洞穴生成組合決策](../decisions/0002-hybrid-cave-generation-composition.md)
- [空間生成領地委派決策](../decisions/0004-territorial-delegation-for-spatial-generation.md)
- [現代地形與洞穴生成研究](modern-terrain-and-cave-generation.md)
- [待決問題](../planning/open-questions.md)
