# Lattice Axiom

Lattice Axiom（晶格公理）是建在 [Bevy](https://github.com/bevyengine/bevy) 上、以 Nickel／SemVer package graph 與靜態／動態雙 realization 為核心的體素世界遊戲。這個倉庫採取「文件先行」：保存產品架構、決策、技術基線與第一個可玩 demo 的驗收路線。

目前尚無程式實作。專案預設完整採用 Bevy 與成熟 upstream；只有真實可玩原型證明其無法滿足不可妥協需求時，才允許最小自研替代。Nickel 套件內核、統一 package graph、版本化 semantic registration 與原生 ABI 是產品組合語義，從第一個垂直切片即開始；它們不取代 Bevy 的 ECS、排程器或 renderer。已接受的 ADR 是實作基線，其餘 proposed／exploration 文件會依原型與量測修正。

從[文件入口](docs/README.md)開始閱讀。

## License

Except where otherwise noted, REZICS is licensed under the
[GNU Affero General Public License v3.0 only](./LICENSE) (`AGPL-3.0-only`).
Copyright © 2026 Rezics Inc.

Third-party components remain under their respective terms; exact notices
will be added when implementation dependencies are locked. The AGPL grants
copyright permissions only and does not grant trademark rights in the REZICS
name, logos, or other brand identifiers.
