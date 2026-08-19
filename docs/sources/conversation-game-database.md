---
title: 遊戲資料庫架構對話來源索引
status: active
type: source-index
locale: zh-Hant
updated: 2026-08-19
---

# 遊戲資料庫架構對話來源索引

## 來源

- 歷史原始檔名：`ChatGPT-遊戲資料庫架構.md`
- 主要問題：PostgreSQL、World Store、空間實體、區塊快照、生成器更新與 regenerate 的責任邊界
- 處理方式：按主題整理為正式文件後，於 2026-08-19 依維護要求刪除原始 `chat-history` 檔

原始對話是設計來源，不是持續維護的規格；外部事實以正式文件中的官方連結為準。

## 主題對照

| 對話主題 | 整理後文件 |
| --- | --- |
| RAM／ECS、World Store、PostgreSQL 與物件儲存分層 | [世界持久化與 RocksDB](../architecture/world-persistence.md) |
| 一般空間實體與社會性／關聯式物件的差異 | [世界持久化與 RocksDB](../architecture/world-persistence.md) |
| 保存目前狀態與模擬續行，而非永久事件流 | [世界持久化與 RocksDB](../architecture/world-persistence.md) |
| 生成後區塊快照成為權威 | [決策 0009](../decisions/0009-rocksdb-authoritative-world-snapshots.md) |
| provenance、generation epoch 與套件雜湊 | [版本與相容性](../architecture/versioning-and-compatibility.md)、[世界持久化](../architecture/world-persistence.md) |
| `/regenerate` 的保留政策、交易與邊界 | [世界持久化與 RocksDB](../architecture/world-persistence.md) |
| Persistent World Object 與未來 PostgreSQL 查詢 | [世界持久化與 RocksDB](../architecture/world-persistence.md) |

