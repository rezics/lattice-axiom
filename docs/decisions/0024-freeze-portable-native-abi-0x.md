---
title: 冻结 Portable Native ABI 0.x 的线协议、执行政策与证据门禁
status: accepted
type: decision
updated: 2026-08-20
---

# 决策 0024：冻结 Portable Native ABI 0.x 的线协议、执行政策与证据门禁

## 背景

[决策 0017](0017-versioned-native-module-abi.md)已经固定动态 native 使用单一 C
bootstrap、独立版本化 capability tables、批次 ECS、command buffer、显式所有权与
per-`EngineInstance` 生命周期；[决策 0018](0018-package-kernel-from-first-vertical-slice.md)
又要求这条路径从第一个真实 vertical slice 开始，而不是在游戏完成后补做。

仍未冻结的细节会直接改变 generated C header、Rust SDK、old-binary fixture、loader
诊断与性能结论：header magic、request／response 布局、`InterfaceId`、ABI-POD
集合、handle 与 buffer 所有权、原地写入失败、thread／timeout flags、fault containment、
旧 major 支援窗口和 ABI 1.0 overhead gate。若这些规则继续只存在于概念图中，R3
实现会被迫以某次 Rust struct 或 loader 代码作为事实来源，之后也无法证明旧 artifact
究竟应加载还是拒绝。

本决策冻结首个可实现的 `0.x` contract。它仍是实验性 ABI，不承诺已经达到 1.0，
但每一项实验变更也必须有明确版本、不可重编 fixture 与精确诊断，不能用「尚未稳定」
掩盖无版本的 layout 漂移。

## 待决问题映射

下表是本决策与 `open-questions.md` 的机器可追踪映射：

| 问题 | 冻结章节 |
| --- | --- |
| `ABI-01` | 第 2、3 节：bootstrap header、request／response 与 target policy |
| `ABI-02` | 第 4 节：`InterfaceId` 与 collision／namespace 治理 |
| `ABI-03` | 第 5 节：首批与 deferred capability tables |
| `ABI-04` | 第 6 节：ABI-POD 集合、alignment 与 layout hash |
| `ABI-05` | 第 7 节：writable batch、staging 与 failure scope |
| `ABI-06` | 第 8 节：entity／asset／opaque handle layout 与 overflow |
| `ABI-07` | 第 9 节：scratch arena、owned buffer 与 release API |
| `ABI-08` | 第 10 节：thread、blocking、reentrancy 与 timeout defaults |
| `ABI-09` | 第 11 节：panic／fault containment 与 native trust policy |
| `ABI-10` | 第 12 节：old-major support 与 deprecation telemetry |
| `ABI-11` | 第 13 节：ABI 1.0 dynamic overhead gate |

## 决策

### 1. 单一 schema 是 ABI 的规范来源

Portable Native ABI 由一份 typed、versioned ABI schema 生成：

- C header 与 compile-time `sizeof`／`alignof`／`offsetof` assertions；
- Rust `#[repr(C)]` bindings、SDK-safe wrappers 与 callback shims；
- `RegistrationManifest` 的 interface、callback signature 与 layout descriptors；
- ABI inspector metadata、compatibility report 与文档中的表格；
- C reference module、Rust reference module与正／负 conformance corpus。

手写 C header、手写一份意义相近的 Rust FFI struct，或从编译器 debug layout 反推
contract 都不是规范来源。生成物必须可删除重建。实现 FFI pointer validation、dynamic
library entry 与 Bevy dynamic component descriptor 所需的 `unsafe` 只可在专门的 native
ABI／loader crate 局部放宽；每个 unsafe block 仍需 `// SAFETY:` 说明，其他 crates 继续
继承 workspace 的 `unsafe_code = "deny"`。

### 2. Bootstrap 0.1 的固定 C 基础型别

唯一 export 继续是：

```c
LaxStatus latticeaxiom_module_entry(
    const LaxEntryRequestV0* request,
    LaxEntryResponseV0* response);
```

`LaxStatus` 是 `uint32_t`；`0` 是成功，非零稳定 status code 的详细资料写入 diagnostic
sink。所有 function pointers 使用目标平台的 `extern "C"`／C calling convention，均返回
`LaxStatus`，包括 lifecycle 与 release callback。首版共同基础型别为：

```c
typedef uint32_t LaxStatus;

typedef struct LaxHash256 {
    uint8_t bytes[32];
} LaxHash256;

typedef struct LaxBytes {
    const uint8_t* data;
    uint64_t len;
} LaxBytes;

typedef struct LaxMutBytes {
    uint8_t* data;
    uint64_t len;
} LaxMutBytes;

typedef struct LaxInterfaceId {
    uint64_t hi;
    uint64_t lo;
} LaxInterfaceId;

typedef struct LaxAbiHeader {
    uint32_t magic;
    uint16_t abi_major;
    uint16_t abi_minor;
    uint32_t struct_size;
    uint32_t flags;
} LaxAbiHeader;
```

`LaxAbiHeader` 的 size 固定为 16、alignment 固定为 4，字段 offset 依次为
`0／4／6／8／12`。magic 的内存 bytes 固定为 ASCII `LAXB`；在首版 little-endian
target 上数值为 `0x4258414c`。magic 不编码 ABI 版本。

`flags` 的高 16 bits 是 must-understand flags，遇到未知 bit 必须拒绝；低 16 bits 是
advisory flags，未知 bit 可忽略但必须保留在 inspector output。所有 reserved field 写端
必须写零；读端发现非零 required reserved bit 时拒绝。

`LaxBytes`／`LaxMutBytes` 在 `len == 0` 时允许 null pointer；`len > 0` 时 pointer 必须非
null。所有 `len`／`count`／`stride`／`capacity` 乘加使用 checked arithmetic，任何 overflow
在 dereference 前拒绝。

### 3. Target 与 entry request／response

Portable ABI 0.x 只支援 little-endian、64-bit pointer target。ABI 不使用 `long`、
`size_t`、C／Rust `bool`、compiler enum layout 或 target-native SIMD type。artifact descriptor
先按 exact target triple、endianness、pointer width 与 C calling ABI 过滤；OS loader 载入
library 后，entry 再重复验证。新增 big-endian 或 32-bit target 需要新的 conformance corpus
与 accepted decision，不能只放宽一个 flag。

`LaxEntryRequestV0` 的生成字段顺序固定为：

```text
header
host_min_minor / host_max_minor
endianness / pointer_width_bits / reserved
c_call_abi / reserved
EngineBuildId
locked graph hash
expected RegistrationManifest hash
target triple UTF-8 span
locked package ID UTF-8 span
locked realization ID UTF-8 span
host context + host query_interface
diagnostic context + emit_diagnostic
```

`EngineBuildId` 对 `PortableNative` 为全零，对 `EngineCoupledNative` 必须精确匹配。package、
realization 与 target spans 只在 entry callback dynamic extent 内有效。

`LaxEntryResponseV0` 的生成字段顺序固定为：

```text
header
module flags / reserved
actual RegistrationManifest hash
module ID UTF-8 span
realization ID UTF-8 span
module context
create_instance / start_instance / stop_instance / destroy_instance
module query_interface
```

artifact hash 由 host 对 descriptor 指定的完整 library bytes计算并在 `dlopen`／`LoadLibrary`
以前验证；module 不回报嵌入自身的 artifact hash，避免自引用 hash contract。response 由 host
分配、清零并在调用前以 `struct_size` 表明容量；module 只写容量覆盖的
字段，返回时把 `struct_size` 写成其已知 layout size。response 中的 ID spans 至少在 library
保持 mapped 时有效，但 host 必须在 negotiation 完成时立即复制；function pointers 在 v1
no-unload policy 下保持有效，仍不可在 `stop`／`destroy_instance` 后调用。

host 先验证 descriptor、target 与 artifact hash，才载入 library。entry 阶段的验证顺序固定为
pointer／minimum size、magic、ABI range、`struct_size`／alignment、flags／reserved、target、
package／realization、manifest hash、callback map，全部通过后才可 `create_instance`。create／
start 失败时，host 以逆序 stop／destroy 已建立 instance，且 world writer 仍未打开。

### 4. InterfaceId、version negotiation 与 collision 治理

Interface 的规范身份是现有 stable ID grammar 下的 canonical ASCII name，例如：

```text
latticeaxiom:interface/core.diagnostics
latticeaxiom:interface/ecs.batch
latticeaxiom:interface/ecs.command-buffer
latticeaxiom:interface/messages
```

版本不写进 canonical name。wire `LaxInterfaceId` 由以下算法产生：

```text
digest = SHA-256("latticeaxiom.interface-id\0" || canonical_name_utf8)
hi = big_endian_u64(digest[0..8])
lo = big_endian_u64(digest[8..16])
```

完整 canonical name 与完整 SHA-256 仍保存在 manifest、schema registry 与 inspector；128-bit
token 只用于 table lookup。schema generation 或 runtime 发现不同 canonical names 得到相同
token 时必须 fail closed，禁止静默 alias。`latticeaxiom` interface namespace 由 core ABI
schema 管理；第三方或 engine-coupled interface 使用其已经取得 grant 的 registration namespace。
首版不建立 network registry。

`query_interface` request／response 同样以 `LaxAbiHeader` 开头。request 指定
`InterfaceId`、minimum／maximum major 与 minimum／maximum minor；provider 选择范围内最高的
已实现 compatible version。每个 table 以共同前缀开始：

```text
LaxAbiHeader
InterfaceId
interface_major / interface_minor
interface_flags / reserved
context pointer
```

`LaxAbiHeader.struct_size` 是整个 table size。stable major 的 minor 只可在 table 尾部追加
optional function／field；改变既有 offset、ownership、thread、error 或 callback 语义必须提升
interface major。table pointer 默认借用至 instance stop；需要不同 ownership 的 interface 必须
在 response 带 allocator-side release，而不能由 caller 猜测。

### 5. 首批 tables 与明确延后项目

R3 必须实现并由同一个 `@example/dual-gameplay` consumer 实际使用：

| Interface | 首版责任 |
| --- | --- |
| `core.diagnostics@0.1` | stable code、package／realization／callback context、structured fields 与 remediation |
| `ecs.batch@0.1` | system call、entity／column batches、read／write views 与 scratch |
| `ecs.command-buffer@0.1` | spawn／despawn、component add／remove、asset／world command 的 staged validation |
| `messages@0.1` | typed batched publish／consume 与 versioned payload |

实现 table 不表示每个 module 都必须 require 它；manifest 分别声明 required／optional range，
缺 required interface 时在 instance create 前失败。structured logging 首版适配到 diagnostics／
host tracing，不另冻 `core.log`。

以下 tables 延后到同时存在真实 consumer、provider、budget 与 conformance fixture 时：

- `tasks`：D4 worldgen 或另一个真实异步 package；
- `assets`：portable package 的 stable asset／readiness consumer；
- settings read／change、diagnostic sample 与 inspect sample 的独立 tables：由对应 settings／
  information-surface contract 冻结；R3 prototype 不构成永久 table 承诺；
- `render.feature`／`render.command-list`：R6 的 feature pair、exclusive provider 与 command budget；
- 任何 engine-coupled renderer／profiler internal table：必须先有 exact-build consumer，不能以
  `EngineBuildId` fixture 本身假造通用产品需求。

新增独立服务使用新 `InterfaceId`，不得扩大 bootstrap 或把所有服务合成一个 Host API table。

### 6. ABI-POD 0.1 集合与 layout hash

ABI-POD 0.1 允许：

- `i8／u8／i16／u16／i32／u32／i64／u64`；
- IEEE-754 `f32／f64`；
- 固定长度 byte array 或 ABI-POD array；
- 由 schema generator 显式排列的 nested ABI-POD struct；
- 以 `u8` 表示且在输入边界验证为 `0／1` 的 semantic boolean。

ABI-POD 0.1 禁止：

- `usize／isize`、`i128／u128`、Rust／C `bool` 与 Rust `char`；
- pointer、reference、slice、string、function pointer、allocator ownership 或 drop glue；
- implicit enum discriminant、union、bitfield、zero-sized type、flexible array、packed struct；
- niche-dependent value、target-native SIMD 或任何 Bevy／Rust process type。

允许的 field／struct alignment 只有 `1／2／4／8／16`。generator 按自然 alignment 计算
offset，产生显式 padding、C `_Alignas`、Rust `repr(C, align(N))` 与 static assertions；不使用
`#pragma pack`。host storage alignment 或 layout 不符时使用 generated accessor／staging copy，
绝不直接 cast。

layout hash 是完整 256-bit SHA-256：

```text
SHA-256(
  "latticeaxiom.abi-layout.v1\0" ||
  canonical_json(layout_policy_version, endianness, size, alignment,
                 ordered(stable_field_key, scalar_encoding, array_length,
                         nested_layout_hash, offset))
)
```

producer、source path 与 provenance 不进入 layout hash。component stable ID、schema ID／version
与 layout hash 必须分别匹配；layout hash 相同永远不构成 Rust type identity，也不能代理
persistence compatibility。`RuntimeDynamic` 只可注册通过该 descriptor 验证且无 drop 的 component。

### 7. Batch、writable column 与 command atomicity

一个 dynamic system callback 处理一个 system 的一个或多个有上限 batch。`LaxSystemCall`
至少携带 EngineInstance／tick／stage、system numeric ID、batch array、command sink、message interface、
scratch arena 与 host monotonic deadline。`LaxColumnView` 至少携带 component numeric ID、layout
hash、data、count、stride、element size、alignment 与 read／write／optional flags。pointer 只在
callback dynamic extent 内有效；不得逐 entity／voxel／slot 往返，也不得缓存 batch pointer 或
Bevy entity。

首版不实现通用 rollback／undo log。system manifest 分别声明：

```text
write_commit_mode = staged | direct
failure_scope = callback-recoverable | instance-fatal
```

未声明时使用 `staged + instance-fatal`。`callback-recoverable` 只允许 staged writable columns
或完全没有 authoritative write；成功返回并通过 command validation 后才 copy back。`direct`
原地写入只允许 `instance-fatal`，用于已有 conformance／benchmark 证据的热路径。

direct callback 返回非成功、panic 或产生 invalid output 时，host 丢弃该 callback 的 command／
message staging，立即把 EngineInstance 标为 failed，停止后续 callbacks，禁止 persistence／
checkpoint 把该 tick 的部分状态写入 durable world，并安全退出到 shell／crash recovery。只结束
当前 tick 然后继续运行是不合法的。

structural command buffer 在 schedule barrier 验证 numeric ID、schema、permission、handle generation、
payload size 与 deterministic ordering；任一 command 失败时整批不 apply，不留下半套 archetype
mutation。

### 8. Entity／asset／opaque handles

首版 handle 不使用 bitfield packing，也不暴露 Bevy `Entity`／`Handle<T>` layout：

```c
typedef struct LaxOpaqueHandle {
    uint64_t table_id;
    uint32_t slot;
    uint32_t generation;
} LaxOpaqueHandle;
```

size 固定为 16、alignment 固定为 8。entity、asset、task 与其他服务在 C signatures 和 Rust
bindings 中使用不同 typed wrappers；底层 layout 相同不允许跨 kind 使用。

全零 handle 是 invalid。`table_id` 由 host 按 process 内 EngineInstance + service table 分配且不
复用；slot generation 从 1 开始，每次释放递增。generation 达到 `UINT32_MAX` 时永久 retire 该
slot，不得 wrap；slot 或 table ID 耗尽时拒绝新 allocation／instance 并要求 process restart。
despawn、asset unload 或 instance stop 立即使旧 generation／table 失效。host 每次使用都验证
expected table、kind、instance、generation 与 permission。

handle 只可在声明的 EngineInstance／service lifetime 内使用。它可由 callback 写入 command buffer
并在 barrier 再验证，但永远不进入 lock、save、network payload、authoritative hash 或 diagnostic
持久资料。

### 9. Borrowed spans、scratch arena 与 owned buffer

三种 memory contract 必须是不同 generated types：

1. `LaxBytes`／`LaxMutBytes` 是明确 callback lifetime 的 borrowed spans；
2. `LaxScratchArena` 是 host-owned、per-callback、有 hard byte budget 的 bump allocator；
3. `LaxOwnedBuffer` 是显式 ownership transfer。

Scratch API 只有 `alloc(context, size: u64, alignment: u32, out)`，没有 free／realloc；callback
返回即整体 reset。scratch memory 不可存入 message、command 或 owned result，也不可在 callback
后读取。

Owned buffer 的固定字段为：

```text
data pointer / len / capacity
alignment / flags
release context / release function
```

非空 owned buffer 必须有 release function；接受方 exactly once 调用分配方的 release，并传回
原 pointer、capacity 与 alignment。`len <= capacity`，alignment 必须是非零 power of two；所有
条件在读取前验证。host 跟踪 outstanding owned buffers，全部 release 前不得 destroy instance。
release callback 也遵守该 instance 的非重入政策。

高频输出优先使用 caller-provided sink 或 scratch；owned buffer 只用于确实跨 callback／低频的
payload。UTF-8 仍是 pointer + byte length，不要求 NUL terminator；receiver 在需要长期保存时复制。

### 10. Thread、blocking、reentrancy 与 timeout

零 callback／table flags 的规范默认值为：

- main-thread only；
- per-instance serialized；
- no concurrent batches；
- nonblocking；
- host 不会同步 re-enter module callback。

`host-worker-safe`、`concurrent-batches`、`may-block` 与 `reentrant` 分别 opt in，不能以一个
“thread safe”总 flag 代理。`host-worker-safe` 与 concurrent batches 必须通过 race、alias、
deterministic command merge 与 repeated multi-instance tests；规模路径由 Bevy scheduler／task
pools 调度，不建立 project-owned thread runtime。blocking 工作使用未来的 host `tasks` table，
background foreign code不得直接修改 Bevy World。

in-process trusted native callback 无法被安全强制取消。`LaxSystemCall` 携带 host monotonic
deadline／budget，host 量测 elapsed 并在 callback 返回后的 barrier 产生 overrun diagnostic 或
依政策 fail instance；不得声称能在 deadline 到达时恢复仍在执行的 C function。hard timeout
只能终止整个 process，并由 durable world crash recovery 处理。具体每 callback 数值来自 fixed
tick／system budget，不写死在 wire layout。stop／quiesce 有独立有界 deadline；超时同样选择
process termination，而不是 unload library。

### 11. Status、panic 与 fault containment

故障分为四级：

| 类别 | 允许的处理 |
| --- | --- |
| declared recoverable domain status | 仅 staged／无 authoritative write callback 可拒绝本次结果并继续 |
| Rust panic／C++ exception／invalid callback output | SDK boundary 截获后 instance failed，停止 callbacks／commands／messages |
| ABI contract／permission／ownership violation | instance fatal，产生 owner-aware diagnostic并安全退出 world |
| access violation／segfault／illegal instruction／无限 blocking | 不承诺 in-process recovery；process crash／terminate |

authoritative system 不可在 panic 后静默 disable 并继续 writable world。只有 manifest 明确标记、
没有 authoritative write、使用 staged output 且有独立 conformance 的 presentation system／render
feature 可 disable 当前 system。

`PortableNative` 与 `EngineCoupledNative` 都是 trusted full-process code；二者只在兼容承诺与
`EngineBuildId` 上不同，不存在更安全的 native trust profile。未来 `WasmComponent`／process
isolation 另立 fault／timeout policy，不能改变本表对 native 的描述。

instance 生命周期固定为：

```text
create -> start -> callbacks -> quiesce -> stop -> destroy
```

stop 后 host 不发新 callback；module 不再产生 command／message。outstanding callback、task、GPU
work 或 owned buffer 未完成时不得 destroy。v1 停止后 library 仍保持 mapped，不支援 native hot
unload。

### 12. 0.x 相容、support window 与 deprecation telemetry

首个 bootstrap 与首批 interfaces 都是 `0.1`。在 `major == 0` 时，minor 是 experimental epoch：

- artifact／manifest 必须声明确切可接受 range；没有声明 compatibility 时按 exact minor；
- append-only tail extension可以声明与旧 minor compatible，但必须由不可重编 old-binary fixture
  证明；
- breaking ownership／offset／语义变化可以提升 0.x minor，但不得声称旧 minor compatible；
- 从 stable major `1` 开始，minor 一律只允许 append-only optional extension，breaking change
  提升 major。

这条规则细化决策 0017 的 minor policy：append-only 是所有**被声明为 compatible**的 minor 与
所有 stable-major minor 的硬条件；experimental 0.x breaking epoch 必须精确拒绝旧 artifact，
不能靠 `struct_size` 猜测 shim。

0.x 不作永久 support 承诺，但 CI 至少保留当前与前两代不可重编 binary fixtures。每个 host 对
每份 fixture 必须产出「成功协商」或包含 exact bootstrap／interface／field／owner 原因的拒绝报告，
测试不得重新编译 fixture。

ABI 1.0 后，每个 bootstrap／interface 至少支援 current major 与 immediately previous major；旧
major 从 successor 发布起的支援窗口不得短于 **18 个月或两个 stable product release trains**，
取較長者。移除时只提供明确拒绝与 rebuild／replacement action，不在 loader 写无 schema 的猜测转换。

deprecation 产生本地、machine-readable diagnostic／metric，至少包含 canonical interface、required／
selected version、package／realization、first-deprecated host、remove-not-before、replacement 与
rebuild action。ABI inspector／CLI 可汇总；默认不上传远端 telemetry。

### 13. ABI 1.0 性能 Gate

首个 provisional reference profile 固定为：

```text
desktop-reference-v1
CPU: 6 cores / 12 threads, AVX2
RAM: 16 GiB
GPU: 6 GiB VRAM
presentation: 1920x1080 medium
fixed tick: 60 Hz
FixedUpdate CPU: P95 <= 8 ms, P99 <= 12 ms
```

所有 ABI benchmark 使用 optimized distributable build、真实 `cdylib`、相同输入与 state hash；
static baseline 保留 direct Bevy system、Thin LTO 与相同业务算法。对至少 10,000 entities 的真实
batch workload：

```text
dynamic_batch_P95 <= max(1.25 * static_P95, static_P95 + 0.50 ms)
```

同时仍须满足 FixedUpdate budget。十分钟 D10 representative journey 中 dynamic bridge CPU 不得
超过 FixedUpdate CPU 的 10%；steady-state shim／callback 必须是 0 heap allocations。报告必须
分别量测 pack／staging、FFI dispatch、business body、command validation 与 apply，不能只给总时长。
FFI callback count 随 system／batch，不随 entity 线性增长；per-entity fixture 只作为明确反例，
不算可接受 realization。

D1 的 `@example/dual-gameplay` 先提供 gameplay-shaped POD read／write + command + message 早期基线，
但 ABI 1.0 不能只靠测试 package。最终 gate 至少包括：

- D8 shipped mining／drop／pickup／inventory／furnace authoritative batch；
- D9 bounded fluid update 的规模压力；
- 若 `tasks` interface 已冻结，D4／D7 worldgen 的大 payload／async consumer；
- R6 render command 的独立 interface budget，不用 gameplay 数字掩盖 GPU／upload cost。

这些数字在 D10 representative journey 后以 machine-readable baseline 复核；若真实 consumer 超过
预算，则 ABI 保持 0.x、修改 batch contract，或把该 system 明确标为 static-only，不能为时间表
降低 gate 后宣告 1.0。

## 自动证据与出场条件

R3／D1 自动证据至少包括：

1. generated C header、Rust bindings、manifest、inspector 与 docs 的 schema regeneration diff 为零；
2. C reference module 与 Rust SDK module 都通过同一 host conformance；
3. static／dynamic registration hash、numeric IDs、system order、commands 与 N-tick state hash 相同；
4. wrong magic／minor／size／flag／reserved／target／manifest／artifact／callback 在业务 code 前失败；
5. unknown tail、short struct、missing optional／required interface 与 collision corpus；
6. POD size／alignment／offset／endianness／layout hash、bad count／stride 与 typed-identity negative corpus；
7. stale／wrong-kind／cross-instance handle与generation overflow corpus；
8. scratch escape、allocator mismatch、double release、missing release 与 callback-after-stop corpus；
9. staged failure、direct failure-after-write、panic、domain error、timeout overrun 与 process-crash recovery；
10. repeated create／start／quiesce／stop／destroy 与 multi-instance global-state test；
11. static direct、dynamic batch、per-entity反例 benchmark，且 profiler 可分离 bridge stages；
12. native library 保持 mapped，任何测试都不调用 `FreeLibrary`／`dlclose`。

ABI 1.0 还必须满足：

- 一个 shipped gameplay package 的 static／portable state 与 normative save-byte equivalence；
- 至少两代不可重编 old-binary fixtures；
- 本决策的真实场景 overhead 与 allocation gate；
- memory／bad pointer／bad length／panic／lifecycle fault injection；
- 一次 Bevy upgrade rehearsal，portable old artifact不重编，engine-coupled artifact精确拒绝并重建；
- 两个 composable render features 与两个 exclusive providers 的 conflict／fallback；
- settings／metric／inspect 的 static／dynamic 与两代 schema fixture；
- shell preflight、checkpoint／clone与world recovery证明失败不会损坏原 world；
- 每个错误都包含 package、realization、expected／actual contract 与可执行修复动作。

所有自动证据必须可在 headless CI 运行；不要求 window、GPU、视觉检查或人工输入的 gate 不得以
“未执行人工测试”代替。需要 GPU 的 render performance 证据在明确的 release benchmark job 运行，
headless CI 仍验证 graph、bounds、fallback 与 command encoding。

## 结果

- R3 可以从同一 schema 实作 loader、SDK 与 fixture，而不是让某个 Rust FFI struct成为意外契约。
- 0.x 仍可实验，但 compatibility claim、breaking epoch 与拒绝原因均可机械验证。
- dynamic 热路径保持 batch 与 bounded allocation；static path不被迫经过 C table。
- writable failure、panic、timeout 与 native process fault 的恢复范围不再含糊。
- tasks、assets、settings／inspect、render 与 engine-coupled internals 不会在没有真实 consumer 时膨胀
  首版 ABI。
- ABI 1.0 由 shipped gameplay、old binaries、upgrade、fault 与性能证据共同决定，不由 demo 日期决定。

## 被否决的方案

### 以 UUID 随机分配 InterfaceId

它需要额外中央登记且无法从 canonical contract重建；canonical name + domain-separated hash同时提供
稳定生成、collision检测与可读诊断。

### 允许任意 Rust `repr(C)` 型别自动成为 ABI-POD

`repr(C)` 不解决 drop、allocator、niche、semantic schema、target SIMD 或跨 realization type identity。
只有 schema允许集合与 generated descriptor 可以进入 zero-copy column。

### 为 recoverable error 对所有 writable batch 建通用 undo log

它把昂贵 transaction成本强加给每个 hot system，也无法回滚任意 foreign side effect。staged commit
与 direct-instance-fatal 的显式选择更可验证。

### 在另一个 thread 强制取消超时 native callback

任意停止 C／Rust code会留下锁、allocator、TLS 与 host pointer的不一致状态。in-process native只能
soft deadline或终止process；需要可抢占隔离时使用未来process／Wasm realization。

### 从第一版一次实现所有 capability tables

没有 consumer 的 task、asset、settings、render 与 internal tables只会冻结猜测性 API。独立
`InterfaceId` 允许它们按真实 vertical slice加入，不阻塞最小 gameplay ABI。

## 相关文件

- [决策 0008：静态与动态共用一图](0008-static-and-dynamic-realizations-share-one-graph.md)
- [决策 0017：版本化原生模组 ABI](0017-versioned-native-module-abi.md)
- [决策 0018：从第一个垂直切片交付套件内核与双实现](0018-package-kernel-from-first-vertical-slice.md)
- [原生模组 ABI 架构](../architecture/native-module-abi.md)
- [模组组合](../architecture/module-composition.md)
- [版本与相容性](../architecture/versioning-and-compatibility.md)
- [执行期路线图](../planning/roadmap-game-engine.md)
- [第一个可玩 demo](../planning/roadmap-first-demo.md)
