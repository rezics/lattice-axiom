---
title: 凍結受控 Nickel 求值計量與 Worker 協議
status: accepted
type: decision
updated: 2026-08-20
---

# 決策 0022：凍結受控 Nickel 求值計量與 Worker 協議

## 背景

[決策 0021](0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md)
已凍結 R0 Nickel policy ID、八項資源上限、受控 roots 與 CLI／embedded corpus，
但沒有定義 entry 是否計數、deadline 起止、重複 import、recursion frame、memory backend、
diagnostic 排序與 worker 非正常終止。這些差異會改變同一 policy 接受的程式集合；依 0021，
改變計量單位或終止語義需要 policy major，因此不能留給實作自行選擇。

Nickel `0.18` 的公開高階 API 也不能注入 fail-closed source loader，且沒有執行期
function／contract depth 或 evaluator memory hard limit。R0 實作必須誠實表達 enforcement
capability；process isolation 限制故障範圍，但不是 hostile-code sandbox。

## 決策

### 1. 求值單位與原子結果

一次 **complete composition evaluation** 從 controller 接受已授權 source grants 與 profile
entry 開始，到下列全部工作完成為止：

1. immutable source snapshot 與 canonical table 驗證；
2. 靜態 import closure、resource preflight 與 worker 啟動；
3. Nickel parse／typecheck／contract／fully evaluate；
4. Rust normative serde model validation 與 `GameProfileSpec` → `CompositionSpec` 正規化；
5. semantic／provenance receipt、diagnostic 排序截斷與 canonical response encoding。

任何階段失敗只回傳 versioned diagnostics 與 failure receipt，不得回傳部分
`CompositionSpec`、部分 hash 或可供 resolver 使用的 package candidate。CLI 與 embedded API
必須呼叫同一 controller 路徑；直接呼叫 worker 只供 conformance fixture。

### 2. Source root 與跨 root 尋址

controller request 明文列出 root grant；每項包含 stable `SourceId`、root kind、immutable
source-table hash、root entry logical path 與可選 Nickel package alias。R0 只接受 0021 定義的
current-package、versioned-library、profile-overlay 與 test-fixture 四種 root kind。

- main entry 使用 typed `(SourceId, logical_path)`，不靠 current directory；
- quoted relative import 只在 importing file 的同一 root 內解析；
- `.` segment 先消除，`..` 必須仍留在同一 root；absolute、drive、UNC、device、反斜線、
  非 UTF-8 NFC 與未授權 path 一律拒絕；
- 跨 root 只使用 request 明文給定的 Nickel package alias；alias 是 Nickel identifier，
  `latticeaxiom_lib_v1` 保留給 `latticeaxiom.lib` contract major 1；
- alias 在一個 request 內唯一；同一 root／entry 不得由多個 alias 指向，package map 不做
  filesystem、environment 或 discovery-order fallback；
- overlay／fixture alias 由 profile／fixture 明文記錄，不能由目錄列舉推導。

實作用 deterministic synthetic absolute path 配合 upstream Nickel 時，該 path 只存在於 worker，
不得進入 diagnostic、hash、lock 或 protocol response。真正 R0 exit 需要 source-table-only loader；
只靠 preflight 後的 filesystem staging 是 hardening 過渡，不得宣稱受控 closure 已完成。

### 3. Import closure 計量

closure node identity 是 `(SourceId, canonical logical path)`。entry depth 為 `0`，其直接 import
depth 為 `1`；`import_depth` 是任一路徑上的最大 edge 數。R0 對 import cycle 在求值前穩定失敗，
不以 traversal visited order 截斷 cycle。

`imported_files` 沿用既有欄位名，但計量 **包含 main entry** 的 unique closure nodes。
diamond import 與相同 canonical target 的重複 import 只計一次；不同 path／alias 指向同一 target
在 source preflight 視為 alias collision，不合併。`source_bytes` 是同一批 unique closure nodes 的
raw bytes 總和，也包含 main entry；不做 UTF-8、newline 或 BOM 正規化。兩項都在 parse／typecheck
前以 checked integer arithmetic 計量，boundary 值接受，`limit + 1` 拒絕。

whole-root canonical source-table scan 可有獨立的 acquisition safety budget；它不是
`imported_files`／`source_bytes`，不得把未進 closure 的 asset 或 authoring file 計入 R0 evaluator
policy。worker 只接收 snapshot bytes 與 receipts，不得在 snapshot 後重讀原 source root；receipt
長度或 SHA-256 不符即失敗。

### 4. Deadline 與 worker 終止

`wall_clock_ms` 使用 controller 的 monotonic clock，從接受完整 request、開始 preflight 前計時，
到完整 response 已驗證為止。process spawn、IPC、source closure、typed normalization 與 diagnostic
處理都在同一 deadline 內；source acquisition／download 在 request 形成前完成，不計入 evaluator
deadline。

到達 boundary 的工作可成功；觀察到 elapsed `> wall_clock_ms` 或下一 blocking operation 無剩餘
budget 時，controller 必須 kill 並 reap worker，再回 `compose.worker_timeout`。controller cancellation、
EOF、crash、signal／exception、nonzero exit、truncated frame、trailing bytes、unknown protocol field 與
wrong version 各自轉成穩定 controller diagnostic；不得接受 worker 在終止前寫出的半包或 partial
model。

### 5. Recursion 計量

`recursion_depth` 是同一 worker 中 active Nickel function application 與 contract-check frame 的
共享最大深度。entry expression 為 `0`，進入第一個 function／contract frame 為 `1`；進入會使
active depth 成為 `limit + 1` 的 frame 前失敗。tail call 若由 evaluator 明確以 frame replacement
執行，不增加 active depth；無窮 tail call仍由 deadline 終止。

AST nesting、import depth、Rust stack depth、`eval_permissive` 的 export traversal limit 與 contract
equality gas 都不是這個 meter。沒有 VM call-frame hook 的 backend 必須報
`compose.worker_capability` 並拒絕 production R0 policy，不能靜默停用此限制。

### 6. Memory 計量與 backend receipt

`memory_bytes` 是 worker containment domain 在求值期間的 hard byte ceiling，不是週期性 RSS sample。
R0 支援的 backend 必須在 untrusted Nickel instruction 執行前安裝 hard limit，並在 response receipt
明文記錄 versioned backend ID：

- Windows 使用 Job Object 的 process committed-memory limit；
- Linux 使用 dedicated cgroup v2 的 `memory.max`，且該 cgroup 只包含本次 worker；
- 其他平台或無權建立上述 containment 時，production R0 policy fail closed 為
  `compose.worker_capability`。

backend 的 kernel accounting 可有平台差異；target、backend ID 與 effective byte limit共同進入
evaluation receipt／lock，CLI 與 embedded parity 必須使用同一 backend。新增 backend可提升 policy
minor；改變既有 backend 的計量來源、是否包含 page cache／shared pages 或 OOM 映射需 policy major。
memory hard limit命中回 `compose.worker_memory`；controller不能把不明 crash猜成 memory failure。

開發／tool profile可明文選擇另一個 policy與 soft／unsupported capability，但 receipt必須如實記錄，
且結果不能冒充 `r0@1` production evaluation。

### 7. Typed output 與 output bytes

Nickel value先精確轉成 JSON data model，再由 Rust normative DTO反序列化、套 typed defaults、驗證並
正規化為 `CompositionSpec`。`output_bytes` 只計該 normalized typed `CompositionSpec` 的 canonical
JSON bytes；raw Nickel export、IPC frame與人類 render 不使用此 limit。boundary接受，`limit + 1`
回 `compose.output_limit`。

無法精確表示的 Nickel number、function、contract、foreign value或其他非 JSON data value穩定失敗；
不得先以 `f64` 或 display text改寫 author intent。semantic hash只使用 typed canonical intent；
SourceSpan、synthetic path、worker/backend operational metadata只進 provenance／evaluation receipt。

### 8. Diagnostic 排序、去重與截斷

worker與controller diagnostic先轉成 Lattice DTO，再以以下 bytewise tuple排序：

```text
(
  severity rank: error < warning < info,
  diagnostic code,
  primary source_id or empty,
  primary logical_path or empty,
  primary byte_start or 0,
  primary byte_end or 0,
  summary,
  canonical labels,
  canonical notes
)
```

只有完整 DTO canonical bytes相同的相鄰項才去重。若數量不超過 `retained_diagnostics` 全部保留；
若超過，保留排序後前 `limit - 1` 項，最後加入 `compose.diagnostics_truncated`，其 note記錄總數、
保留數與省略數。因 limits非零，summary永遠有位置。截斷後不得再追加非 controller-fatal診斷；
controller-fatal failure以單一 fatal diagnostic取代 worker列表，避免超限與partial result歧義。

### 9. Worker protocol 與 enforcement capability

worker protocol是 Lattice-owned、versioned、`deny_unknown_fields` 的單 request／單 response framed DTO。
request至少包含 protocol／catalog／model／library／corpus major、target、entry、sorted immutable source
snapshots、package aliases、policy ID與全部effective limits。response是 success typed canonical
`CompositionSpec`與receipts，或 failure diagnostics與receipts的tagged union；兩者不能同時存在。

worker啟動後 stdout只承載一個有明文長度上限的protocol frame；log只到有界stderr。controller重驗
protocol version、frame length、canonical source receipts、policy receipt、typed payload、canonical bytes
與hash。worker宣告的 enforcement capability至少分 `hard`、`soft`、`unsupported`；R0 production policy
對 deadline、memory、function／contract recursion三項只接受 `hard`。

worker process的目的，是讓controller可強制終止、限制資源並原子拒絕故障；它不是native code或
Nickel的通用security sandbox。compiler、download、native library load、network、clock、random與
environment expansion仍完全留在求值路徑之外。

### 10. Stable diagnostic codes

R0 compose catalog新增並凍結：

- `compose.import_cycle`、`compose.import_depth`、`compose.import_files`；
- `compose.worker_timeout`、`compose.worker_memory`、`compose.worker_recursion`；
- `compose.worker_capability`、`compose.worker_protocol`、`compose.worker_unavailable`；
- `compose.diagnostics_truncated`。

source/path/hash/schema/output既有code沿用 0021 corpus。Nickel upstream英文訊息只能是note；stable code、
severity、labels、logical source provenance與byte range不得靠解析rendered text產生。

## 取捨

- 把entry納入file／byte meter消除「巨大main不計量」漏洞，但也明確固定了既有欄位名的特殊語義。
- Windows Job與Linux cgroup accounting不完全相同；versioned backend receipt讓差異可稽核，且避免把
  sampled RSS誤稱hard limit。
- stock Nickel staging可先建立fault corpus，但source-table-only loader與VM frame hook仍是R0 exit
  blocker；fail-closed capability使過渡實作不會被誤用為production conformance。
- reject import cycle比實作SCC lazy semantics更窄；未來若要接受cycle，需新增policy major與corpus。

## 驗證

- 每一limit都有`boundary`與`boundary + 1` fixture；file／byte fixture包含main、diamond與reimport。
- absolute／drive／UNC／backslash／escape、missing alias、duplicate alias、cycle、NFC／case-fold／target
  alias collision均在Nickel求值前失敗。
- timeout fixture證明worker已kill且reap；memory與recursion fixture只在hard capability backend上標為
  production pass，unsupported backend穩定fail closed。
- CLI／embedded對positive corpus產生逐byte相同canonical `CompositionSpec`、hash與receipts；negative
  corpus產生相同stable code、source ID、logical path與byte range。
- source rename只改provenance／source receipt；record key reorder與等值typed defaults不改semantic hash。
- crash、EOF、truncated／oversized／trailing frame、wrong version與unknown field均不洩漏partial output。

## 相關文件

- [決策 0010：Nickel 驅動套件系統](0010-nickel-driven-package-system.md)
- [決策 0018：第一個垂直切片即採用 package kernel](0018-package-kernel-from-first-vertical-slice.md)
- [決策 0021：凍結 R0／R1 Package、Nickel 契約與解析政策](0021-freeze-r0-r1-package-nickel-contract-and-resolution-policy.md)
- [執行期路線圖](../planning/roadmap-game-engine.md)
