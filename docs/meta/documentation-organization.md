---
title: 文件組織方式
status: active
type: meta
locale: zh-Hant
updated: 2026-08-19
---

# 文件組織方式

## 採用方式

目前採用「讀者需求導向、主題式、文件即程式碼、決策另行留痕」的輕量結構：

1. 以一頁一個主要問題的方式整理概念，避免照對話時間線切成難以重用的片段。
2. 使用 Markdown 與 Git，讓文件和未來實作共享版本、審查與變更歷史。
3. 每頁標記成熟度、文件類型、語系與更新日期。
4. 討論與草稿是暫時輸入；主題頁與決策紀錄才是倉庫內維護的知識，同一項規格只維護一份。
5. 只有真正形成選擇時才建立決策紀錄，保留背景、選項、結果與後果。
6. 先建立有內容的分類，不為了看起來完整而建立空目錄。
7. 不保存對話索引或聊天副本；完成主題化與決策記錄後直接刪除暫時材料，由 Git 歷史保留文件演進。

## 為何適合現在的 Lattice Axiom

現有材料是架構探索，不是操作流程或 API。依 Diátaxis 的分類，它主要屬於「解釋」，另有少量「調查」與「規劃」。因此現在按領域建立解釋頁；等有可執行原型後，再增加教學、操作指南與參考文件。

Diátaxis 本身也提醒：四種類型是判斷文件目的的地圖，不應在一開始硬建四個空區。這與「文件仍要演進」的需求一致。

## 目錄契約

```text
docs/
├── foundations/   專案願景、原則與共同詞彙
├── architecture/  可驗證的架構假說與邊界
├── decisions/     已明確採納的重大選擇、理由與後果
├── research/      外部調查與候選方案
├── planning/      待決問題與下一步
└── meta/          文件維護方式
```

`decisions/` 已在第一項明確採納的世界生成方向出現後建立；每份紀錄只處理一項決策。主題依領域分目錄，不另建 `docs/zh-hant/` 或 `docs/en/` 等語系平行樹。一頁只使用一種語言，語系寫在 frontmatter 的 `locale`。現有頁保留現有檔名與 `locale: zh-Hant`；同一主題若要第二種語言，在同一目錄新增 sibling，檔名加語系後綴（例如 `world-generation.en.md`，`locale: en`）。

## 文件生命週期

```text
討論或調查
    ↓
主題頁（exploration）
    ↓ 原型、量測、評審
具體提案（proposed）
    ↓ 明確採納
決策紀錄（accepted）
    ↓ 日後改變
新決策取代舊決策（superseded）
```

對既有決策做重大改變時，不覆寫歷史理由；建立新紀錄並互相連結。普通文字修正與補充則直接更新主題頁。

## 每頁最低要求

- 標題能直接說明頁面回答的問題。
- 開頭說明範圍與成熟度。
- 區分「目前假說」「已知限制」「待驗證項目」。
- 相關內容用連結引用，不複製整段規格。
- 外部事實附原始或官方來源；未驗證的主張明確標記。
- 不完整頁面在開頭直接說明缺口，避免讀者誤以為完整。

## 決策紀錄最低欄位

真正需要建立決策紀錄時，至少包含：

- 狀態
- 背景與問題
- 決策驅動因素
- 考慮過的選項
- 決策結果與理由
- 正面與負面後果
- 如何驗證決策有效

檔名採 `NNNN-short-title.md`；狀態至少支援 `proposed`、`accepted`、`rejected`、`deprecated` 與 `superseded`。

## 目前不做的事

- 不把所有對話逐句複製到主題頁。
- 不因技術偏好出現在對話中就提升為已接受決策；需要維護者確認與 ADR。
- 沒有選擇靜態網站產生器；目前 Markdown 已足夠。
- 沒有建立尚無讀者任務可支撐的空教學或 API 目錄。

## 調查依據

- [Diátaxis：依讀者需求區分教學、操作指南、參考與解釋](https://diataxis.fr/)
- [Diátaxis：把框架當指南，並以小步迭代改善文件](https://www.diataxis.fr/how-to-use-diataxis/)
- [Write the Docs：Docs as Code](https://www.writethedocs.org/guide/docs-as-code/)
- [Write the Docs：文件應可掃讀、就近、單一來源且可定位](https://www.writethedocs.org/guide/writing/docs-principles/)
- [The Good Docs Project：先從讀者、任務與維護能力設計資訊架構](https://www.thegooddocsproject.dev/tactic/ia-guide)
- [MADR：以精簡 Markdown 紀錄單一重要決策及其理由](https://adr.github.io/madr/)
