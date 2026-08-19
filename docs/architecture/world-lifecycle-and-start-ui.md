---
title: World 目錄、開始頁與安全生命週期
status: proposed
type: architecture
updated: 2026-08-20
decision:
  - ../decisions/0009-rocksdb-authoritative-world-snapshots.md
  - ../decisions/0010-nickel-driven-package-system.md
  - ../decisions/0018-package-kernel-from-first-vertical-slice.md
---

# World 目錄、開始頁與安全生命週期

## 結論

client先进入一个已锁定的`ClientShellGraph`，由package驱动的开始页读取只读`WorldCatalog`；
玩家选择／创建world后，package kernel才验证该world的exact lock、schema、semantic bindings、
settings与checkpoint状态，并建立`LockedGameGraph`／EngineInstance。任何module code或world write
都发生在preflight通过之后。

开始页不是一张“Play按钮背景图”，而是package／world相容边界的主要用户界面。首阶段必须
支持健康的一键Continue、world list／create、明确兼容diff、checkpoint／clone、只读恢复与
可撤销删除。

## 兩個 Graph、兩種權限

```text
ClientShellGraph
  @latticeaxiom/front-end
  @latticeaxiom/settings
  @latticeaxiom/settings-ui
  @latticeaxiom/world-library
  @latticeaxiom/observability
          │ read-only headers / package catalog
          ▼
      WorldCatalog
          │ select / create intent
          ▼
      WorldPreflight ── package/schema/setting/migration diff
          │ accepted plan
          ▼
LockedGameGraph + RegistrationImage + RuntimeImage
          │ activation transaction
          ▼
       Playing world
```

`ClientShellGraph`本身也是SemVer／capability resolved的package closure，有自己的lock；它不是
host hidden plugin list。这个名称表示同一resolver／`LockedGameGraph` schema的一种shell角色，
不是第二套package model。它只能读取world header、checkpoint catalog与package metadata，不能
instantiate chunk或执行world-owned migration。

首个demo在同一process内使用**顺序、独立的EngineInstance／Bevy App**：shell选择world后正常
停止shell instance，再建立game instance；返回首页则停止game instance并重建shell。已经映射的
native library保持loaded，不声称支持hot unload。bootstrap只协调这些App的先后生命周期，不
推进game tick。若window／GPU／static realization隔离证明同process重建不可靠，可改为明确的
process restart而不改变world格式或UI contract。

## 基礎 Packages

| Package | Capability | 责任 |
| --- | --- | --- |
| `@latticeaxiom/front-end` | `client-shell@1` exactly-one | 开始页state／routing／loading／error surface |
| `@latticeaxiom/world-library` | `world-catalog@1` exactly-one | header scan、health、search／sort、checkpoint／trash operation |
| `@latticeaxiom/settings-ui` | `settings-surface@1` exactly-one client | shell与pause menu共用设置 |

world template可以由任意shell-visible package贡献；真正创建时仍解析它请求的root game package，
不能把template当成绕过resolver的目录复制脚本。

## World 身分與 Header

人类display name、directory slug与stable identity分开。`WorldId`是创建时生成的immutable ID；
rename不搬动或重写所有key。`WorldHeader`是可校验、大小有界、只读扫描的projection，至少包含：

- `WorldId`、display name、created／last played、play time；
- format／header schema、clean-shutdown marker、last durable revision／checkpoint；
- exact game lock／registration／semantic image fingerprints；
- required authoritative package／schema／used-content closure摘要；
- world／player-world setting fingerprint与migration state；
- dimensions、game mode／difficulty摘要、worldgen seed是否可显示的policy；
- screenshot／thumbnail stable asset reference；
- storage size、last verified time与health code；
- source path只用于diagnostic，不作为world identity。

header使用checksum并与RocksDB metadata交叉验证。header坏掉或未知version时，catalog保留一个
`NeedsRecovery` entry与路径／错误，不静默过滤整个world。

## WorldCatalog

catalog异步扫描明确的world roots与trash root：

- 先读bounded header，thumbnail与size延迟加载；
- search按display name／WorldId／package／tag；
- sort支持last played、created、name、size、health；
- filter支持compatible、needs attention、trashed、package profile；
- scan error是每个entry状态，不阻塞其他world；
- filesystem watcher只更新projection，不在背景启动resolver或migration；
- 同名display name允许存在，UI总附加last played或short WorldId消歧。

## 開始頁資訊架構

### 首頁

顺序按高频与风险：

1. `Continue — <world name>`：只在最近world健康、exact-compatible且无需选择时出现；
2. `Worlds`；
3. `New World`；
4. `Packages / Profiles`；
5. `Settings`，且Accessibility在首次启动和任何错误前可达；
6. `Diagnostics / About / Quit`。

最近world若需要migration、缺package、上次crash或durability不明确，首页按钮改为
`Review <world name>`并显示原因，不把一键Continue变成隐藏的写入／升级动作。

### World card

默认显示thumbnail、name、last played、game／dimension摘要与一个明确health badge。选中后显示：

- `Play`／`Review and Play`；
- package lock状态：exact、compatible update available、missing、untrusted、conflict；
- durability与last checkpoint；
- `Duplicate / Checkpoint / Export / Rename / Move to Trash`；
- `Details`中的WorldId、size、schemas、packages、settings与diagnostics。

危险动作不混在primary Play旁边；keyboard／controller focus顺序与视觉顺序一致。

## New World

Quick Create只要求display name，使用当前profile的safe defaults并在按钮旁摘要template／game mode；
Advanced分步显示：

1. template／root game package；
2. worldgen seed与preset；
3. world-authoritative settings；
4. package／provider choices；
5. final lock／storage estimate／risk review。

创建交易：

```text
CreateIntent
  → resolve and lock package closure
  → validate setting/worldgen schema
  → create temp world + header + initial checkpoint
  → fsync/atomic publish into catalog
  → activate
```

任一步失败都不留下看似可玩的半个world；可诊断temp artifact可进入quarantine并明确清理。
seed preview只有真实异步generator consumer且有预算后才加入，不能阻塞首个demo。

## WorldPreflight

preflight只读取metadata／manifests，不执行module business code：

1. 验证header、storage、checkpoint与clean-shutdown状态；
2. 取得world frozen lock与本机available package sources／artifacts；
3. 验证artifact trust、ABI／EngineBuildId与authoritative schema；
4. 比较semantic image、Role bindings、active bundles与used-content closure；
5. 比较world setting schema／values与required migration；
6. 产生`WorldOpenPlan`、risk level、estimated work与可用恢复动作。

结果类型：

| 状态 | 默认动作 |
| --- | --- |
| `ReadyExact` | 一键Play |
| `ReadyCompatible` | 显示diff；允许使用compatible artifact，但不改lock除非明确确认 |
| `NeedsDownloadOrBuild` | 列出来源／信任／大小／build，完成后重新preflight |
| `NeedsMigration` | checkpoint／clone后staged migration |
| `RecoverableReadOnly` | 只读打开、export或restore checkpoint |
| `Blocked` | 不进入writable world；列缺失owner／schema与具体修复 |

`Use frozen lock`、`Resolve compatible graph`与`Clone and upgrade`都是屏幕上明确命名的动作，
不能藏在Ctrl／Shift点击中。

## Migration 與 Upgrade

- 默认策略是`checkpoint → stage copy／transaction → validate → atomic publish`；
- UI显示package／schema owner、from／to version、是否改变world bytes及不可逆范围；
- migration code只来自已经验证的target closure，并在限制的migration context运行；
- dry-run能给出范围／错误时先执行；不能保证dry-run的migration必须诚实标记；
- validation至少抽样／全量检查受影响record、required StableId与revision；
- failure保留原world与diagnostic bundle，不把半迁移store标为active；
- downgrade默认不允许写原world，可clone/export给显式转换工具。

## Checkpoint、Clone、Export 與 Trash

### Checkpoint

RocksDB checkpoint是同一world的恢复点，记录lock／header／checksum／size／reason。自动policy至少在
migration、graph change与显式玩家动作前创建；长期轮替按数量＋空间budget，并优先去重／只在
world改变后创建，避免无变化的每日重复backup。

### Clone

clone产生新`WorldId`、新display name与独立writer；保留source world／checkpoint provenance，
但后续revision分叉。用于试验新package或migration，不把copy folder当正式clone。

### Export

export是可移动、只读bundle，含world checkpoint、header、exact lock、license／package清单与
checksum；是否携带native artifact由trust／license policy决定。export不等于backup catalog。

### Trash

Move to Trash关闭writer、原子移动／标记catalog entry并允许Restore。永久Purge显示WorldId、
name、size、last checkpoint与不可恢复后果；不以长按作为唯一确认。trash retention／磁盘上限
可设置，自动purge前再次通知。

## Loading、Cancel 與 Durability

loading surface显示可理解阶段：`Checking world → Resolving packages → Building/Loading →
Validating content → Loading spawn → Playing`。每阶段报告进度是否determinate、当前package／
chunk与错误修复动作。

- 在world writer建立前可安全cancel；
- activation后cancel走正常shutdown barrier，不能kill I/O后返回菜单；
- `Saving…`只代表queued，UI分别表达written／durable／checkpointed；
- 低磁盘、水位超限或WAL／checkpoint失败立即升级为persistent警告，必要时暂停新world mutation；
- crash后首页显示`Recovered to durable revision N`或`Needs review`，不只写日志。

## Package 注入邊界

shell package可以贡献：

- world template与创建表单中的结构化`SettingSpec`；
- world card的namespaced read-only detail fragment；
- preflight diagnostic与明确repair action capability；
- migration progress stage与localized explanation。

它不能：

- 改写Play／Delete等core action语义或把广告放进primary surface；
- 在catalog scan执行native code或读取所有chunk；
- 隐藏另一个package的compatibility error；
- 未经checkpoint／review直接迁移或删除；
- 从display name／folder猜world identity。

## 首個 Demo 範圍

- package-driven shell首页、Worlds与New World；
- WorldHeader／catalog scan、search、last-played sort与health状态；
- exact lock preflight、missing package／schema／unclean shutdown三类错误；
- Quick Create、rename、checkpoint、clone、move-to-trash／restore；
- `RecoverableReadOnly`最小只读open与export；只读open不建立writer，export为含checkpoint、header、
  exact lock、package／license清单与checksum的可移动bundle，不携带未经policy允许的native artifact；
- Continue只对`ReadyExact`world启用；
- loading阶段、cancel boundary与durability indicator；
- migration先实现一个old schema fixture的clone／checkpoint路径；
- keyboard／controller navigation与基础screen narration语义。

public registry下载、cloud sync、多人server browser与任意旧world修复器不进入首个demo。

## 驗收

- world目录有一个损坏header时其他world仍列出，损坏项不会静默消失。
- 同名world可消歧；rename不改变WorldId或RocksDB keys。
- Continue不会对missing package、migration、unclean／non-durable world直接写入。
- preflight不执行module business code且不产生world write；错误列出owner、required／available与动作。
- migration各故障点终止后原world与checkpoint可载入，半迁移copy不发布。
- Move to Trash可restore；Purge前可由narration读出对象与不可逆后果。
- 低磁盘fixture在新mutation前产生persistent UI并保护durable snapshot。
- `RecoverableReadOnly`open期间writer与authoritative command计数为零；export可在独立目录验证checksum，
  并由headless catalog／preflight读取而不修改source world。
- world list扫描／thumbnail／size计算不阻塞60 FPS shell输入。
- mouse、keyboard、controller都能完成create／play／checkpoint／restore；focus在异步刷新后不跳到
  另一个world。
- headless使用同一preflight／open plan与错误码，不依赖client UI。

## 相關文件

- [世界持久化](world-persistence.md)
- [套件內核](package-management.md)
- [套件驅動的 Bevy runtime](game-engine-runtime.md)
- [Package 設定與配置](settings-and-configuration.md)
- [外部調查](../research/debug-settings-and-world-ux-lessons.md)
