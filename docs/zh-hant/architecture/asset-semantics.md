---
title: 資產管線與模型語義
status: exploration
type: explanation
locale: zh-Hant
updated: 2026-08-09
source:
  - conversation-2026-08-09-turn-6
---

# 資產管線與模型語義

> glTF/GLB、VRM 與 OpenUSD 在本頁都是候選或參考，不是已接受的格式決策。

## 核心問題

通用受擊、步態、注視、裝備與粒子模組若要處理任意第三方模型，就不能只知道頂點與骨骼；它們還需要知道「哪裡是頭、手、地面接觸點、受擊區與效果插槽」。

因此需要同時解決三個層級：

1. 幾何與動畫如何交換。
2. 遊戲語義如何附著在模型上。
3. 交換資產如何編譯成高效的執行期資產。

## 三段式資產管線

```text
創作格式
.blend / USD / Maya project / 其他 DCC
        ↓ 匯出
開放交換格式
glTF / GLB + 遊戲語義擴充
        ↓ 驗證與資產編譯
執行期格式
平台專屬網格、貼圖、動畫、碰撞與語義表
```

創作格式保存編輯器所需的高階資訊；交換格式是工具與引擎的契約；執行期格式則可以自由做壓縮、meshlet 產生、貼圖轉碼、動畫壓縮與碰撞烘焙。

開放交換格式與引擎專屬執行格式並不衝突。

## 為何討論 glTF/GLB

glTF 2.0 能攜帶網格、PBR 材質、貼圖、節點階層、skin/joint、骨架動畫與 morph target，且有公開規格與擴充機制。它適合作為 runtime delivery 導向的交換格式候選。

`.blend` 更適合作為 Blender 創作來源，不適合直接成為所有工具都必須理解的遊戲 ABI。

OpenUSD 的 scene composition、layering、reference 與大型 DCC 管線能力值得研究；它可能位於大型內容製作的上游，而非每個一般模組的最終交付格式。這個角色分工仍需用實際工作流驗證。

## 幾何資料不等於遊戲語義

模型檔即使有 37 根骨骼，也不一定說明哪一根代表頭部、右手或胸腔。不能依賴節點名稱恰好是 `Head`：

```text
作者名稱：DragonSkull
        ↓ 語義映射
引擎角色：head
```

名字屬於作者；語義角色屬於跨工具契約。

## 能力式語義

固定的 `humanoid`、`quadruped` 或 `flying` 類別可作為範本，但不應限制六翼龍、八足生物或機械體。模型應以能力描述它能參與哪些通用機制。

```yaml
semantics:
  root: Root
  head: DragonSkull
  chest: Torso

capabilities:
  gaze:
    origin: head
    forward_axis: +Z
  locomotion:
    ground_contacts:
      - Foot.FrontLeft
      - Foot.FrontRight
      - Foot.BackLeft
      - Foot.BackRight
  equipment:
    right_hand: Claw.Right
  projectile:
    origin: MouthSocket
```

這只是說明形狀的草圖，不是 schema。

## VRM 可提供的啟發

VRM 建立在 glTF 上，為 humanoid 定義骨骼角色與可重定向的共同語義。Lattice Axiom 所需範圍更廣，但可以借鑑「交換格式 + 領域語義層」的模式，而不是照搬 humanoid 欄位。

語義層若足夠穩定，模型、動畫、AI、物理與表現包便可分開組合：

```text
Dragon Model
+ Quadruped Locomotion
+ Heavy Hit Reaction
+ Boss Health
+ Dragon AI
```

## 執行期編譯

交換資產進入建置流程後，至少需要：

- schema 與必要能力驗證
- 節點／骨骼語義解析成數值控制代碼
- 網格與材質最佳化
- 動畫壓縮與可選重定向資料
- 碰撞與物理表示烘焙
- 版本與擴充相容性記錄
- 內容雜湊與快取鍵

正式執行期不必解析完整 glTF JSON，也不必保留作者節點名稱。

## Schema 設計問題

- 語義放在 glTF extension、sidecar 檔，或兩者皆支援？
- 如何命名與治理擴充，避免宣稱尚未註冊的官方 `EXT_` 名稱？
- 哪些語義是跨遊戲通用，哪些應由遊戲或模組自訂？
- 能力的版本與相容性如何協商？
- 動畫重定向需要哪些基準姿勢、座標系與比例規則？
- 不可信資產的大小、拓樸、shader 與壓縮炸彈如何驗證？
- Blender add-on 如何顯示缺少語義、無效映射與執行期回退？

## 初步來源

- [Khronos glTF 2.0 規格](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)
- [glTF 擴充登錄表](https://github.com/KhronosGroup/glTF/tree/main/extensions)
- [Blender glTF 2.0 匯入／匯出手冊](https://docs.blender.org/manual/en/latest/addons/import_export/scene_gltf2.html)
- [VRM 規格與功能](https://vrm.dev/en/vrm/vrm_features/)
- [OpenUSD](https://openusd.org/)

這些連結來自原對話脈絡或其官方入口；真正選型前仍需建立需求矩陣與互通原型。

## 相關文件

- [實體、物理與表現層](entity-physics-presentation.md)
- [物理資產與局部形變創作](physical-authoring.md)
- [渲染與物理技術地圖](../research/renderer-physics-landscape.md)

