---
title: 凍結 R0／R1 Package、Nickel 契約與解析政策
status: accepted
type: decision
updated: 2026-08-20
---

# 決策 0021：凍結 R0／R1 Package、Nickel 契約與解析政策

## 背景

R0／R1 必須先把 Nickel authoring、typed `CompositionSpec`、local source、SemVer、
deterministic resolution 與 frozen lock 變成可建立 golden fixture 的具體契約。先前文件已經
確定 Nickel／Rust 的責任邊界與 package graph 的五個語義產物，但仍保留八項會讓 schema、
hash、diagnostic 或 resolver 實作分叉的問題。

本決策只凍結第一個 local／workspace vertical slice 所需的 contract。它不提前設計 public
registry、通用 SAT／PubGrub solver、遠端信任服務或 marketplace。

## 決策

### 1. Rust serde model 與 golden corpus 是 normative source

R0／R1 的 normative machine model 是版本化 Rust serde types。第一版至少包含：

- `PackageSpec`、`GameProfileSpec`、`RealizationSpec`；
- `CompositionSpec`、`LockedGameGraph`、`BuildPlan`；
- package／capability／domain／source／requirement／diagnostic newtypes；
- canonical encoding 與 semantic／provenance hash input types。

一組同版本的 positive／negative golden corpus 是 authoring contract 的 single source of truth。
embedded Nickel evaluator、CLI evaluator、Rust serde model、`latticeaxiom.lib` contracts 與
schema documentation 必須全部通過同一 corpus。第一版不做 Rust schema 與 Nickel contract 的
雙向 code generation；在 corpus 尚未通過時，任一側的修改都不得單獨合併。

model major、`latticeaxiom.lib` major 與 corpus major 明文記錄。增加 optional field 可以提升
minor；改變既有 field 意義、canonical encoding 或 acceptance set 必須提升 major。

### 2. `PackageSpec` 最小欄位

每個 fully evaluated package 必須產生以下欄位；沒有內容的集合仍要由 typed default 正規化，
不能因缺欄位與空集合產生不同 semantic hash：

| 欄位 | 契約 |
| --- | --- |
| `model_version` | `PackageSpec` schema version |
| `name` | root `name` 或 scoped `@scope/name` `PackageName` |
| `version` | strict SemVer 2.0.0 version |
| `domains` | 非空的 `authoritative`／`client`／`server`／`tool` 集合 |
| `features` | 具名、typed、無副作用的 feature declarations |
| `parameters` | graph-affecting typed parameter declarations 與 defaults |
| `dependencies` | package requirement、optional flag、feature 與 domain condition |
| `requires`／`provides` | versioned capability、range、cardinality 與 domain condition |
| `realizations` | 至少一個 `RealizationSpec`；pure-data package 也有 `data` realization |
| `registrations` | exact IDs、schemas、semantic、settings、observability、assets 與 code-bound fragment references |
| `namespace_requests` | package 要使用的 registration grants；authority 由 source policy 證明 |
| `trust` | Nickel/data/build/native 所需 trust class |
| `metadata` | display、license、documentation 等不影響玩法語義的 typed metadata |

`source`、discovery path、download URL、artifact hash 與 local absolute path 不由 package 自我
宣告為身分；它們由 source universe／kernel 加入 provenance 與 lock。

### 3. `GameProfileSpec` 最小欄位

每個 fully evaluated profile 必須產生：

| 欄位 | 契約 |
| --- | --- |
| `model_version` | profile schema version |
| `roots` | root package requirements；順序沒有 resolver 語義 |
| `source_universe` | 受控 sources、stable source ID 與顯式 integer priority |
| `projection` | `client-shell`、`client-world`、`dedicated-server`、`headless-test` 或 `tool` |
| `features`／`parameters` | package-qualified graph inputs |
| `realization_policy` | explicit choice 或按 kind 排列的 `auto` preference |
| `semantic_bindings` | profile Role binding／bundle policy intent |
| `overlays` | 有序、具 provenance 的 composition overlays；不是 source discovery order |
| `evaluation_policy` | 本決策定義的 versioned Nickel resource policy ID |
| `policy` | namespace、trust、force override 與 recovery 權限；普通 package 不可自授權 |

profile root order、filesystem enumeration order 與 record key order不作 tie-break。真正有順序
語義的 overlay 與 realization preference 必須使用明文陣列，並進入 semantic hash。

### 4. `RealizationSpec` 最小欄位

每個 realization candidate 必須產生：

| 欄位 | 契約 |
| --- | --- |
| `id` | package-local、跨重建穩定的 realization ID |
| `kind` | `data`、`NativeStatic`、`PortableNative` 或 `EngineCoupledNative` |
| `domains` | package domains 的非空子集 |
| `targets` | OS／architecture／ABI target predicates；data 可使用 portable target |
| `required_features` | 必須已由 profile／dependency 啟用的 features |
| `artifact` | source-build、local prebuilt 或 data-root intent；精確 hash 由 plan／lock 加入 |
| `interfaces` | dynamic realization required／optional ABI capability ranges |
| `trust` | build、trusted-native 或 data-only trust requirement |
| `engine_build` | 只有 `EngineCoupledNative` 可要求 exact `EngineBuildId` |
| `registration_fragment` | 預期的 generated／data manifest fragment identity |

`PortableNative` 不得包含 `EngineBuildId` requirement；`NativeStatic` 不得把 C ABI table 當成
business call path。selected target、artifact／manifest hash、producer fingerprint 與實際
`EngineBuildId` 寫入 `BuildPlan`／lock，不回寫成 package authoring 身分。

### 5. Domain markers 與 profile projection

四個 marker 的語義固定為：

- `authoritative`：會影響 authoritative registration、world hash、save、worldgen 或 gameplay；
- `client`：只在 client／shell projection 執行的 presentation、input 或 local UI；
- `server`：只在 dedicated／server projection 執行的服務；`server` 本身不等於 authoritative；
- `tool`：只在明確 tool／test projection 執行，不得因被發現而進入 shipped gameplay closure。

package 可以有多個 marker；dependency、capability、realization 與 registration row 可將條件縮小
到 package 宣告 domains 的子集。`client-world` 投影 `authoritative + client`，
`dedicated-server` 投影 `authoritative + server`，`client-shell` 投影 shell roots 的 `client` rows，
`tool` 只投影明確 tool roots。`headless-test` 的 domain set 必須由 fixture 明文給出。

projection 後 required dependency／capability 若不存在就失敗；不得因另一 domain 有 provider 而
靜默接受。presentation rows 不進 authoritative hash；同一 world 的 client/server projection
必須對 authoritative rows、schemas、Role bindings 與 active bundles 產生相同 fingerprint。

### 6. `SourceSpan`、semantic hash 與 provenance hash 分離

typed conversion 為每個可診斷 contribution 保存：

```text
SourceSpan
├── source_id
├── logical_path
├── byte_start / byte_end
├── optional line / column cache
└── origin_chain (import / helper / overlay / generated fragment)
```

line／column 只是可重建的顯示 cache；byte range 與 origin chain 是 diagnostic provenance。
`SourceSpan`、absolute path、logical path、source URL、display metadata與filesystem mtime不進
semantic hash。相同求值結果即使 source file／directory 改名，semantic hash仍不變。

另計算 `provenance_hash`，其輸入包含 stable source ID、canonical logical paths、每檔 content
hash、origin chain、producer／toolchain 與 generated fragment identity。source path 重命名因此
通常改變 provenance hash，但不改變 semantic hash。lock 同時保存兩者，diagnostic 不得以
provenance hash 不同直接判定 graph 不相容。

### 7. 受控 import roots 與 Nickel resource policy

Nickel evaluator 只能讀取 package kernel 明文授予的 immutable roots：

1. 目前 package source root；
2. versioned `latticeaxiom.lib` virtual root；
3. profile 明文列出的 overlay roots；
4. fixture 明文列出的 test roots。

禁止 ambient current directory、home directory、network、clock、random、environment expansion與
任意 absolute import。logical import path 使用 `/`、UTF-8 NFC、消除 `.`，並在解析 `..` 後仍須
位於同一授權 root。

R0 的預設 policy ID 為 `latticeaxiom:nickel-evaluation-policy/r0@1`：

| Limit | Default |
| --- | ---: |
| wall-clock deadline per complete composition | 10 seconds |
| evaluator memory | 256 MiB |
| function／contract recursion depth | 512 |
| nested import depth | 64 |
| distinct imported files | 4096 |
| cumulative imported source bytes | 16 MiB |
| normalized `CompositionSpec` canonical bytes | 16 MiB |
| retained diagnostics after deterministic sorting | 256 |

limit failure 是穩定 diagnostic，不輸出部分 `CompositionSpec`。trusted developer／tool profile
可選另一個明文 policy ID；policy ID 與所有 effective limits寫入 lock。放寬 limit 可新增 policy
minor；縮小既有 accepted input、改變計量單位或終止語義必須新增 policy major。package 不能自行
提高 limits。

### 8. Canonical path、SHA-256 與 symlink policy

每個 source universe entry 有 stable source ID、declared root 與 priority。kernel 先取得 real
absolute root 供 I/O 與安全檢查，再以 root-relative、`/` 分隔、UTF-8 NFC logical path 建立
canonical file table。Windows drive letter、absolute prefix 與 host-specific separator 不進
semantic model。

- file content hash 使用 raw file bytes 的 SHA-256；不做 newline、BOM 或文字正規化；
- source content hash 使用按 canonical logical path byte order排序的 `(path, file-type, byte-length,
  content-sha256)` table 再做 SHA-256；
- symlink／junction／reparse point 必須在讀取前完整解析；target 必須位於同一 declared root；
- hash 採 **follow-target**：file content hash 是最終 target bytes，不是 link text；canonical table
  仍使用 import 所見 logical path，並把 resolved target identity加入 provenance；
- escape、dangling link、cycle、case-fold collision、NFC collision與同一 logical path 指向不同
  target一律在 Nickel evaluation 前失敗。

source directory 或 file 改名會改 source content／provenance hash；只要 fully evaluated typed intent
不變，就不改 semantic hash。artifact cache 可以因 provenance 改變而 miss，不能因此改變
package／registration identity。

### 9. Strict SemVer 2.0 與 range

version parser 嚴格接受 SemVer 2.0.0。build metadata 不參與 precedence，但保留在 exact version
identity與lock。range grammar首版只接受：

- exact `=1.2.3`；
- comparator `<`、`<=`、`>`、`>=`；
- comparator intersection，例如 `>=1.2.0 <2.0.0`；
- tilde `~1.2.3`，展開為 `>=1.2.3 <1.3.0`；
- caret `^1.2.3`，展開為 `>=1.2.3 <2.0.0`。

caret 使用 declared SemVer **major**，不使用 Cargo 的 left-most-nonzero shorthand。因此
`^0.2.3` 展開為 `>=0.2.3 <1.0.0`，不是 `<0.3.0`；需要較窄 pre-1.0 範圍的 owner 必須寫
`~0.2.3` 或明確 bounded comparators。這個 range只表達 package owner選擇的接受集合，不宣稱
SemVer 2.0 為 `0.x` 提供穩定性保證。

pre-release candidate 只有在 range comparator 明文包含同一 `major.minor.patch` 的 pre-release
時才參與；release 永遠高於同 core version 的 pre-release。首版不接受裸版本、`*`、`x`、
hyphen range、`||` union或 Cargo-specific shorthand；需要時先新增 corpus並提升 range grammar
minor／major。

### 10. Deterministic resolution 與 frozen lock

resolver先套用 package/dependency/capability/domain/target/trust條件，再選擇 SemVer precedence
最高的 compatible version。相同 version 有多個 candidate 時使用以下 stable tuple，依序比較：

```text
(
  descending source priority,
  source kind rank: workspace < local-directory < local-prebuilt < future-remote,
  canonical source ID bytes,
  source content SHA-256 bytes
)
```

realization 先遵守 profile explicit choice；`auto` 按 profile `realization_policy` 的明文 kind order，
再以 `(realization id bytes, target triple bytes, expected manifest fragment bytes)` 升序打破完全等價
候選。capability provider 同样先比较 compatible provider package version，再使用上述source tuple
与完整 provider `PackageName`；resolver tie-break 不得用于解决 semantic Role ambiguity。

source discovery、directory enumeration、root request order、manifest merge、Cargo link與dynamic load
order完全不参与选择。所有 discarded candidates與逐层理由写入 resolution explanation。

有效 frozen lock 永远优先于重新解析。`--frozen` 或 world exact-open 只接受 lock 中的精确
package／source／content hash／realization／artifact；缺失或不匹配就失败，不退回 compatible选择。
非 frozen workflow 也先尝试完整复用lock，只有用户明确执行 resolve／upgrade action 才产生新选择
与diff。source universe 新增版本不能隐式改变既有 lock。

## Hash 邊界摘要

| Hash | 包含 | 不包含 | 用途 |
| --- | --- | --- | --- |
| semantic hash | normalized typed intent、explicit overlay order、policy semantic inputs | source path/span、mtime、URL、display metadata | 相同组合语义与 canonical fixture |
| provenance hash | source IDs/paths/content、origin chain、producer/toolchain | runtime handles | 诊断、重建、审计 |
| lock hash | exact resolution、realization、artifacts、semantic/provenance receipts、policy ID | process-local Bevy/ABI handles | exact closure |

hash 不互相代理。尤其 provenance 或 lock hash 不同不能单独证明 persisted world 不相容。

## 結果

- R0 可立即建立不会因source rename或discovery order漂移的semantic golden。
- Nickel/Rust 暂不承担双向codegen成本，但共享 corpus 防止 contract 漂移。
- local source 的安全边界、symlink与hash行为可在任何compiler/module code执行前测试。
- `0.x` range不再暗中继承Cargo caret特例；package owner必须显式选择范围宽度。
- resolver结果可重现且可解释，frozen world不会因新增source版本改变。
- client/server/tool projection进入typed graph，不再依靠文件夹或手写plugin list推断。

## 被否決的方案

### 以 Nickel contract 或 JSON Schema 單獨作 single source

它们无法独自表达 Rust runtime invariants，也会让 typed conversion与diagnostic acceptance set漂移。
首阶段用 Rust serde model加共同golden corpus更直接；真实重复成本出现后再评估codegen。

### 讓 source path 進 semantic hash

这会让移动目录成为玩法／存档语义变化。path与span属于provenance，不属于evaluated intent。

### 沿用 Cargo 的 pre-1.0 caret

Cargo的left-most-nonzero规则是Cargo依赖体验，不是SemVer 2.0本身。Lattice package range必须有
自己的公开、可跨语言重建的grammar。

### 用 filesystem／registry discovery order 打破平手

发现顺序会随平台、cache与安装操作改变，无法作为lock或world重现规则。

## 驗證

- CLI与embedded evaluator对positive corpus产生逐byte相同canonical `CompositionSpec`。
- negative corpus对field、contract、import、limit、symlink、SemVer与domain错误产生相同stable code
  与source provenance。
- file／directory rename不改变semantic hash，但改变provenance/source content hash。
- record key reorder、root request reorder与source discovery randomization不改变semantic hash或lock。
- `^0.2.3`接受`0.9.0`、拒绝`1.0.0`；`~0.2.3`拒绝`0.3.0`；pre-release规则有golden。
- 同version多source、同package多realization与同capability多provider均按stable tuple选择并输出解释。
- frozen lock在source universe新增更高version后保持逐byte不变；locked artifact缺失时精确失败。
- client-world与dedicated-server projection产生相同authoritative fingerprint，presentation rows不同。
- evaluator在每项limit边界、limit+1、import escape、symlink escape／cycle上有fault fixture。

## 相關文件

- [決策 0010：Nickel 驅動套件系統](0010-nickel-driven-package-system.md)
- [決策 0018：首個垂直切片交付套件內核](0018-package-kernel-from-first-vertical-slice.md)
- [套件內核](../architecture/package-management.md)
- [執行期整合路線](../planning/roadmap-game-engine.md)
- [待決問題](../planning/open-questions.md)
