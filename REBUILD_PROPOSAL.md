# Lattice Axiom 合仓与 package 架构重建方案

日期：2026-09-05。状态：已获用户批准，按阶段实施并自主提交。

> Direction update, 2026-09-07: this remains the historical repository-rebuild
> proposal. The accepted [ecosystem direction](docs/ecosystem-direction.md) and
> [package interface design](packages/latticeaxiom/sdk/docs/package-interfaces.md)
> now govern product and interface planning: author-owned APIs, deep extension,
> manageable composition/upgrades and independent game distribution. This
> documentation update does not implement those pending capabilities or change
> the existing package, resource, saved-world or licensing boundaries.

## 已批准的补充约束

- 纹理、光影及其他呈现资源包独立于游戏规则 package，可独立选择和替换。
- 本轮只提交代码、目录约定与文本定义；不提交图片、模型、音频、字体等二进制资源。完整美术资源管理方案延后，Git LFS 不在本轮引入。
- 界面采用代码绘制的布局、文字、颜色、边框、渐变和交互状态；使用 Bevy 原生 UI、widgets 和 focus 能力，遵循已调研的游戏可访问性实践。
- 每个完成并验证的工作切片自主 Conventional Commit，无需逐次确认。

本轮交付是可评审方案。建议最终以 `lattice-axiom` 为唯一产品仓，重新建立当前工作树；`lattice-axiom-demo` 的实现经过筛选、拆分和验证后融入。旧文档不作为新树的基础整体搬入。本提案在决策完成后拆入少量 ADR、各 package README 和交付清单，不长期成为另一份总规范。

## 1. 要重建的核心模型

**Lattice package 是我们自己的组合、版本、资源、实现与分发单元；Rust crates 是 package 内部的实现组件。**

- 一个 Lattice package 有独立身份、manifest、依赖、可提供能力、代码入口和资源入口。
- 一个 package 可以拥有零个、一个或多个 Cargo packages；日常目录统一称内部 `crates/`。严格说，Cargo package 又可以包含 library、binary、test 等 crate targets。
- 每个自有实现 crate 只有一个 package owner。其他 package 可以依赖它的公开接口，但不能共同拥有同一个散落的实现目录。
- 纯数据 package 不创建空 crate。聚合 package 只声明组合，也不创建空实现。
- 基础平台也有 package owner；不再保留一个绕开 package 所有权的大型顶层 `crates/`。
- Bevy 继续承担 App、ECS、调度、窗口、渲染、资源运行时和任务池。Lattice 实现组合与体素游戏的专有语义。

这一区分有成熟先例：Unreal 的 plugin 可以拥有代码、内容和多个内部 modules，模块与 plugin 的依赖关系也分层管理。我们借鉴所有权模型，不采用 Unreal 引擎或其构建系统。[Epic：Plugins](https://dev.epicgames.com/documentation/en-us/unreal-engine/plugins-in-unreal-engine)

## 2. 已核实的现状

调查对象是本地 checkout，未启动客户端、未进行新的性能跑分或视觉验收：

- 文档仓：`57c37e2334ea019521fcd85237cb8dba47d51d24`。
- 实现仓：`3fa521ecb663ca076f460f86b746e5b016e58660`。
- 调查开始时两个仓库的 tracked/untracked 普通 Git status 均干净；ignored 的 `run/`、缓存、参考 checkout 等不在此结论内。

| 已核实事实 | 对重建的含义 |
| --- | --- |
| 实现仓有 29 个 workspace members：24 个位于顶层 `crates/`，5 个直接以 package 根目录作为 Cargo package | 当前结构不是统一的 package → crates；旧文档的 28 个计数也已落后 |
| `source_build_payload` 固定读取 package 根部 `Cargo.toml`，并检查 `src/` | 单纯移动到内部 `crates/` 会破坏源码 realization；构建入口必须改造 |
| `profiles/dev.toml` 使用 `realization_policy = ["data"]`，当前根 lock 的 12 个被选 package 均为 data realization | 已有原生构建基础不能等同于默认产品已通过 package graph 选择 Rust 实现 |
| engine 直接依赖 input、settings-ui、front-end、Terrenia worldgen；host/worldgen 直接导入 Terrenia 策略 | 通用 host 与产品实现仍耦合，必须将静态装配移到生成的 product root |
| `host/spine.rs` 为 6,939 行，混合流送、生成、派生任务与发布；行数包含测试 | 需要按状态和职责拆分；行数本身不是性能根因 |
| 当前已经有 AsyncComputeTaskPool、每 tick 派生 apply 预算、近远景拆分和 LOD 调度 | 应保留其可验证行为，不能再把“加多线程/加 LOD”当作全新方案 |
| 最新远景计划写明 Slices 0–5 已实现，仍等待 Slice 6 的 visual/soak acceptance | 当前证据不能证明区块加载和真实画面已经满足体验目标 |
| tracked 媒体扩展名扫描找到 201 个 PNG，包括 fixtures；未找到扫描范围内的音频、字体或模型源格式 | 有图片基础，但不等于完整的设计资源生产体系 |
| presentation catalog 声明 `visual_qa: not-claimed`；地形 `from_layer_table` 仍调用 `solid_tile(color)` 生成色块 | 需要打通 authored texture 到实际地形渲染的路径，再验收风格、细节和远景效果 |

关键实现证据：

- [workspace members 与依赖](https://github.com/rezics/lattice-axiom-demo/blob/3fa521ecb663ca076f460f86b746e5b016e58660/Cargo.toml)
- [源码构建入口](https://github.com/rezics/lattice-axiom-demo/blob/3fa521ecb663ca076f460f86b746e5b016e58660/crates/latticeaxiom-compose/src/cli.rs#L912)
- [默认开发 profile](https://github.com/rezics/lattice-axiom-demo/blob/3fa521ecb663ca076f460f86b746e5b016e58660/profiles/dev.toml)
- [host 的 Terrenia 依赖](https://github.com/rezics/lattice-axiom-demo/blob/3fa521ecb663ca076f460f86b746e5b016e58660/crates/latticeaxiom-engine/src/host/worldgen.rs#L29)
- [地形色块生成](https://github.com/rezics/lattice-axiom-demo/blob/3fa521ecb663ca076f460f86b746e5b016e58660/crates/latticeaxiom-engine/src/host/chunk_mesh.rs#L191)
- [现有远景计划与未完成验收](https://github.com/rezics/lattice-axiom-demo/blob/3fa521ecb663ca076f460f86b746e5b016e58660/docs/plan/render-simulation-and-far-terrain.md)

旧文档中存在过时诊断。例如当前派生任务回收先检查 `is_finished()`，不能仅看到 `block_on` 就断言正在逐帧等待后台工作。本提案不把历史问题直接当作当前性能根因。

## 3. 目标目录和 package 所有权

```text
lattice-axiom/
├── Cargo.toml                      # 唯一开发用 virtual workspace
├── Cargo.lock                      # 开发 workspace 的 Rust 依赖锁
├── latticeaxiom.toml                # 本地 source universe / bootstrap 配置
├── rust-toolchain.toml
├── packages/
│   ├── latticeaxiom/
│   │   ├── kernel/                  # Lattice identity / resolver / registration
│   │   │   ├── latticeaxiom-package.toml
│   │   │   ├── package.ncl
│   │   │   ├── crates/
│   │   │   │   ├── latticeaxiom-core/
│   │   │   │   │   ├── Cargo.toml
│   │   │   │   │   └── src/
│   │   │   │   ├── latticeaxiom-packages/
│   │   │   │   └── latticeaxiom-registration/
│   │   │   ├── schemas/
│   │   │   ├── tests/
│   │   │   └── README.md
│   │   ├── sdk/                     # SDK / macros / ABI / 公开运行时契约
│   │   ├── host/                    # 通用启动和 Bevy 运行时适配
│   │   ├── tooling/                 # composer / build / pack / 开发 CLI
│   │   ├── world/                   # authority / wire / storage / worldgen 机制
│   │   ├── voxel/                   # streaming / mesh / collision / far terrain
│   │   ├── input/
│   │   ├── front-end/
│   │   ├── world-library/
│   │   ├── settings/
│   │   ├── settings-ui/
│   │   └── ...                      # observability / progress 等有实际职责的包
│   ├── terrenia/
│   │   ├── main/                    # terrenia 聚合身份，选择能力和内容
│   │   ├── blocks/
│   │   ├── worldgen/
│   │   │   ├── latticeaxiom-package.toml
│   │   │   ├── package.ncl
│   │   │   ├── crates/
│   │   │   │   └── latticeaxiom-terrenia-worldgen/
│   │   │   │       ├── Cargo.toml
│   │   │   │       └── src/
│   │   │   ├── data/
│   │   │   ├── tests/
│   │   │   └── README.md
│   │   ├── gameplay/
│   │   ├── tools/
│   │   ├── presentation/
│   │   │   ├── latticeaxiom-package.toml
│   │   │   ├── assets/
│   │   │   │   ├── source/          # 可编辑图像、模型、音频等源资源
│   │   │   │   └── import/          # 导入参数和构建规则
│   │   │   ├── data/
│   │   │   ├── docs/art-direction.md
│   │   │   ├── tests/
│   │   │   └── README.md
│   │   └── journey/
│   └── example/
│       └── dual-gameplay/           # 同一业务代码的 static/dynamic 实例
├── products/
│   ├── terrenia-desktop/            # profile、精确 lock、发行配置
│   ├── client-shell/
│   └── headless-test/
├── tests/acceptance/                # 跨 package 的实际用户旅程
├── benchmarks/                     # 场景、输入、预算；Rust runner 归 tooling
├── docs/                           # 少量跨包解释、教程、ADR
├── tools/                          # 脚本与入口；自有 Rust 工具仍归 package
├── .github/
└── .gitignore                      # target / CAS / 生成产品 / 本地运行数据
```

目录中的新基础 package 名称是所有权提案，不要求一次性创建全部空壳。保留现有公开 package/content ID；新增 owner 与拆分内部 crate 不应顺带改写世界内的 stable IDs。

初始迁移映射：

| 现有职责 | 新归属 | 需要的边界修正 |
| --- | --- | --- |
| core、packages、registration | kernel 内部 crates | 保持无产品依赖的底层身份与图编译 |
| sdk、sdk-macros、abi、runtime/render contracts | sdk 内部 crates | 只有公开契约可被跨 package 消费；稳定接口与 Bevy 内部实现分开 |
| compose、开发/发布工具 | tooling 内部 crates | 去掉 package 根部单 crate 假设；生成产品构建图 |
| engine、launcher | host 内部 crates | engine 缩为通用适配；移出 Terrenia、UI、区块调度等策略 |
| storage、world-wire、world-db、通用 worldgen/territory 机制 | world 内部 crates | authoritative 数据与生成策略分离 |
| voxel-runtime、voxel-mesh、近远景 presenter | voxel 内部 crates | 流送、网格、碰撞和主线程提交职责分开 |
| world-catalog、shell/HUD/settings 等 | 对应 world-library/front-end/settings-ui 等包 | 通用 UI 能力可共享，但具体体验由所属 package 拥有 |
| gameplay/player/content 的规则与目录 | 按通用机制和 Terrenia 策略拆分 | 通用交互不认识具体矿物或配方 ID；Terrenia 规则回到内容包 |

`voxel-playground` 等实验适配仅在仍有有效对照价值时进入 example/test package，不作为第二条产品实现路径。

## 4. 构建与运行：用 package 选择代码，而不只选择数据

### 4.1 两张图，各自负责自己的事实

- Lattice graph：package 身份、版本、依赖、capability、产品投影、原生/数据 realization、资源闭包。
- Cargo graph：Rust 编译单元、编译依赖、target、features、工具链。
- Lattice build plan 将前者编译成后者的产品子图；Cargo workspace membership 不产生游戏选择语义。
- 跨 package 的本地 Cargo dependency 必须映射到显式 Lattice edge 和确切 instance；开发方便的 path dependency 不能成为隐藏后门。
- Cargo features 只用于 Rust 编译配置；可替换 provider 和游戏模组选择由 Lattice graph 处理。Cargo features 会合并，默认应当是 additive，不能承担完整产品求解。[Cargo：Features](https://doc.rust-lang.org/cargo/reference/features.html)

### 4.2 开发 workspace 与发行构建分开

日常只保留一个 root virtual workspace，members 显式指向 `packages/**/crates/*`。package 根部不放 `[package]` 或嵌套 `[workspace]`。纯数据包不列入 Cargo members。Cargo 官方支持这种成员布局和共享 lock、lints、依赖配置。[Cargo：Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)

但“开发时能 cargo build”不等于“package 能独立分发”。必须解决现有 `workspace = true`、跨目录 path、宏、`include_str!`、build.rs 与原生依赖对 checkout 的隐含依赖：

1. manifest 显式列出内部 crate manifest、公开入口和 realization target；字段设计在首个实现切片中完成，不预先宣称现有 schema 支持。
2. `pack` 计算源闭包，收集内部源码、构建输入、schema、必要资源及有效 Cargo 元数据；默认不包含文档、benchmark 输出和缓存。
3. composer 在生成目录建立独立 product workspace，将继承字段展开或写入该 workspace 的锁定配置，并重写受控的内部路径映射。
4. 跨包依赖只来自选中 source instance；外部 crates 和 target/feature 选择得到单独的产品 Cargo lock/build receipt。
5. 成品绑定 Lattice lock、Rust 构建记录、资源构建记录与最终 artifact hashes；二者不能用一个 `Cargo.lock` 混充。
6. 在干净目录、去掉两个原 checkout、依赖缓存预先准备好的离线环境中运行 conformance。这里的隔离首先证明闭包完整，不把 Cargo build script 执行误称为安全沙箱。

复制一个 package 目录到仓库外后仍需显式获取其依赖；“自包含源包”不表示把所有第三方依赖重复塞进每个包。

### 4.3 通用 host 和生成产品入口

```text
package manifests / Nickel / sources / asset rules
                     ↓
              resolve + validate + lock
                     ↓
              product build plan
                ↙            ↘
       selected Rust graph    processed asset graph
                ↘            ↙
          generated product root + receipts
                     ↓
            generic Bevy host + packages
```

通用 host 可以依赖基础契约和必需运行能力，但不能静态依赖 Terrenia 或某个 shell UI。生成的产品入口链接所选实现，并安装相应 Bevy plugins。替换 worldgen package 不修改 host 源码。

bootstrap 工具必须有固定的可信启动边界：先用已锁工具链构建/获得 composer，再解析产品图。基础 package 可以是必选项，不意味着可以随意卸载；composer 无需在运行前递归解析自己。

保留 NativeStatic 与 PortableNative 的同一语义模型。首轮以真实 NativeStatic 产品和一个多 crate static/dynamic conformance package 证明结构，随后扩展更多动态能力。跨动态边界保持稳定 DTO/C ABI；进程内部 static 代码直接使用 Bevy。不要把全套 native hot reload 混入本次合仓。

最小架构验收：

- 一个 package 内含两个有实际分工的 crates 和一项资源，可打包、锁定、构建、启动。
- 移除或替换示例 package 后，产物代码、资源和注册项同时改变。
- 声明 data-only 的包不偷偷通过 host 提供自己的原生实现。
- 新建 headless 产品不被迫链接 UI/图形后端；与客户端共享的权威玩法结果一致。
- source closure 缺失、未声明跨包依赖、过期资源或 artifact 不匹配均有真实失败案例。

## 5. 区块性能：重整整条流送路径

### 5.1 首先建立当前基线

在旧 demo 的固定 commit、release 构建上保存可重复输入与 trace，然后对新树使用相同场景。记录真实 CPU/GPU/驱动、分辨率、世界 seed、产品 lock、距离、速度和内容密度。已有 `desktop-reference-v1.toml` 仍标记参考机冻结前的 provisional 生命周期，不能当作已获实机认证。

至少覆盖冷启动、重进存档、持续直行、180 度转向、快速跨区块、洞穴边界、批量编辑、近远景切换、提高视距、回到已访问区域和保存退出。

每个 chunk 关联完整时间线：

```text
请求 → 排队 → 读盘/生成 → 权威数据就绪
                        ├→ mesh → 主世界提交 → GPU 可见
                        └→ collider → 可安全通行
编辑/退出 → 排队保存 → written → durable
```

指标必须区分计算耗时和排队耗时，区分“收到 chunk”“mesh 就绪”“真正呈现”“可以走过去”。首屏准备只等待出生点的最小安全集合，远景继续异步收敛。

### 5.2 保留正确概念，重写混合职责

建议 voxel package 内部分为 interest、scheduler、meshing、collision、presentation；world package 拥有权威 chunk、存储及生成接口，Terrenia worldgen 拥有地形与生态策略。模块先拆职责，只有独立依赖/编译/测试需求成立时再拆 crate。

- 分离渲染距离、全细节距离、模拟距离；远景 LOD 不加载整列权威 chunk，也不参加碰撞或玩法。
- 对读盘、worldgen、mesh、collider、资源提交、保存分别设置数量和字节上限，建立总内存账本，避免每个队列单独合规却合计爆内存。
- 按出生/脚下安全、当前交互、移动前方、附近缺口、远景排序；加入任务老化或保底配额，避免远景与保存永久饥饿。
- 用 world/session epoch、chunk revision、邻域版本标识作业；重入世界、反向移动和连续编辑时能丢弃过期结果并及时回收其预算。
- 生成/网格等 CPU 工作使用 Bevy AsyncComputeTaskPool；磁盘适配按其阻塞行为有界执行。把同步 RocksDB 调用装进 async block 并不会自动成为异步 I/O。
- 限定每个实际渲染帧的提交成本，包括多个 fixed ticks 的累计影响，以及 mesh 上传、collider 插入和资源销毁；不只限制后台 job 数。
- 每个 work item 也要可控；一个超大 mesh/collider 即使被允许作为“首个任务”执行，也可能超过整帧预算。
- 合并同区块重复编辑，只重建 dirty chunk 和必要 halo；共享不可变输入，避免在派生链复制整个世界集合。
- 近景碰撞未就绪时只约束受影响的移动/区域，不让玩家穿透地面，也不把全局 queue drain 放进正常帧循环。
- 稳定场景采用经测量合适的 meshing；高频编辑可优先低延迟 mesh，随后替换优化 mesh。是否采用双阶段取决于实际重建成本。

参考依据：Bevy 对跨帧 CPU 任务的分类；Voxel Tools 对主线程任务预算、mesh 销毁尖峰和 block 大小的分析；block-mesh 明确区分 visible faces 与 greedy meshing 的生成速度和三角形数量取舍。这些指导诊断路径，不证明它们就是当前 demo 的瓶颈。[Bevy tasks](https://docs.rs/bevy/latest/bevy/tasks/) · [Voxel Tools performance](https://voxel-tools.readthedocs.io/en/latest/performance/) · [block-mesh](https://docs.rs/block-mesh/latest/block_mesh/)

Minecraft 的官方 1.18 说明已将 simulation distance 与渲染负担分离，也描述了同步/异步 chunk 更新的取舍；这里引用的是该版本明确记录的设计，不假定其内部调度算法代表所有当前版本。[Mojang 1.18 release notes](https://feedback.minecraft.net/hc/en-us/articles/4415128577293-Minecraft-Java-Edition-1-18)

### 5.3 第一组性能验收预算

以下沿用现有配置的主要预算，作为重建后的测量起点，**不是本轮测得结果，也不是对所有机器的承诺**。重建不通过悄悄调低画质、缩小选中视距来过关。

| 场景/指标 | 初始验收线 |
| --- | --- |
| 1080p medium，指定参考机 | presented frame p95 ≤ 16.67 ms，p99 ≤ 25 ms |
| 编辑到可见 | p95 ≤ 100 ms，p99 ≤ 200 ms |
| 热 chunk load / 冷 chunk load | 现有 p95 50 / 250 ms；明确旧端点后继承，另测 request-to-visible 与 collision-ready |
| 世界激活 | 现有 p95 ≤ 2 s；分别记录打开存档、最小安全可玩与完整远景收敛 |
| 主线程派生 apply | 现有 2 ms 预算；补足帧累计、上传/销毁与单任务超限的诊断 |
| 进程 RAM / GPU memory | 现有高水位 4 / 3 GiB；所有队列、缓存、资源纳入测量 |
| 压力轨迹 | 确认队列有界；持续移动后回到原点，内存回落到稳定平台 |

先用现有协议的 120 秒预热、600 秒采样、3 次运行复现主要预算，再做较长遍历。GPU 呈现验收必须有真实设备；headless 测试不能代替帧时间和画面检查。远景还要验证选中距离实际形成连续可见覆盖、地表轮廓和接缝，而不只是相机 far plane 数值变化。

## 6. 可玩性：先完成一段有目标的游戏

保留既有“体素探索、长期世界、科学与自然魔法”的方向。新的首个可玩目标建议是 **20–30 分钟的完整体验**；这只是首个验收切片，不代替原有完整 v1 世界和长期内容目标。

建议玩家旅程：

1. 从完整开始页创建世界，看到清楚的加载状态，安全出生。
2. 从地貌和环境线索辨认资源，采集木石，体验工具效率差异。
3. 制作工作台和第一把工具，知道接下来要找什么、为什么值得去找。
4. 沿线索找到洞穴/资源点，采集铜等材料并加工。
5. 制作一个具有新用途的成果，例如帮助寻找矿物的简易探测工具；效果要能改变下一次探索。
6. 建立小据点，保存退出并重进；世界改动、玩家状态和进度完整恢复。

上述探测工具是本提案的产品建议，不冒充已确认功能。首个主题可以在玩法灰盒中验证，再扩展科学/魔法两条长期进展。不要在这一步同时做完整工业树、法术体系、同伴 AI 和联机。

`@terrenia/journey` 贡献引导内容，通用 progress package 负责机制；游戏目标不是 host 硬编码教程。技术面板按需打开，普通 HUD 只表达玩家当前行动需要的信息。

可玩性完成条件不是方块/配方数量：

- 首次游玩的测试者能在不阅读技术文档、不使用控制台命令的情况下完成基本循环。
- 采集、合成、加工和探索之间有明确因果与反馈；升级工具产生可感知变化。
- 操作、UI、命中、音效、挖掘/放置反馈和存档恢复一起验收。
- 记录试玩时的卡点、误操作、下一步不明确的时刻及重复劳动；据此调数值和引导。
- 自动旅程验证状态正确，真人试玩验证理解与乐趣；两者缺一不可。

## 7. 设计资源：从风格、源文件到游戏内效果

建议首轮视觉方向为“材质清楚、轮廓易辨认的体素自然世界，科学器具与自然魔法共享材料语言”。这是候选美术方向，需要概念与实际场景共同验证。

### 7.1 首轮明确交付物

| 交付物 | 内容 | 验收场景 |
| --- | --- | --- |
| 风格页 | 色板、明暗层级、纹素密度、物件比例、材质语言、UI 字体和图标规则 | 同一屏地形、工具、HUD 是否协调 |
| 地表和洞穴资源 | 土石、木材、矿物、植被、水，以及必要的变体和六面映射 | 近距离挖掘、远距离地表、洞穴入口 |
| 玩法物件 | 基础工具、工作台、炉、容器与首个阶段成果 | 手持、放置、背包图标之间能互相辨认 |
| UI 资源 | 开始页、世界卡片、热键栏、背包、合成/加工、暂停/设置、加载/失败反馈 | 完整 20–30 分钟流程 |
| 声音与动态反馈 | 行走、不同材质挖掘/放置、合成、交互、环境和简易 VFX | 玩家不用读诊断文字也能理解动作结果 |

不在所有循环方块、物件与界面完成前追求更多 catalog 条目。现有 201 个 PNG 逐项检查其用途、内容、来源及实际 consumer；可用的保留，未达标的替换。

### 7.2 资源生产链

```text
package/assets/source + import rules
                  ↓
     可重现的导入/转换/校验
                  ↓
     hash-addressed processed assets
                  ↓
     product-selected asset sources
                  ↓
      Bevy AssetServer / renderer
```

- 可编辑源文件留在 owner package；Bevy 原有 AssetProcessor/AssetSource 作为处理和加载基础，不重建第二个通用资源系统。
- 明确色彩空间、采样、mipmap、透明/裁剪、动画、碰撞和声音格式。地形 atlas 若继续使用，处理 mip bleeding；是否换 texture array 由目标 GPU、材质需求与测量决定。
- 实现 authored texture 到真正地形 mesh/material 的消费路径。fallback 仅用于缺失/开发诊断，发布所需资源缺失应在构建检查中失败。
- cache key 包含源文件、导入设置、工具版本、目标平台和相关 package instance；游戏不从开发者绝对路径找资源。
- 开发热更新与发行冻结分开；发行只消费 lock 绑定的处理结果，headless 可以移除纯呈现资产。
- 小图片/文本走 Git；大型可编辑二进制源按需要使用 Git LFS。离线包必须包含实际字节，不能误打包 LFS pointer。
- 每项交付资源有来源、作者/生成方式、许可和必要署名记录；模型、字体、声音同样纳入。

Bevy 官方将资产处理定义为从艺术源文件生成运行格式的构建流程，并强调保留源文件、可配置和可重建。LFS 的 pointer 与真实对象分离也意味着备份及发行必须验证对象完整性。[Bevy asset processor](https://docs.rs/bevy/latest/bevy/asset/processor/index.html) · [Bevy AssetSource](https://docs.rs/bevy/latest/bevy/asset/io/struct.AssetSource.html) · [GitHub LFS archives](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/managing-repository-settings/managing-git-lfs-objects-in-archives-of-your-repository)

## 8. 文档融入实现：每类事实只有一个维护入口

**取消“独立文档仓先定义全部事实、实现仓再逐条追赶”的组织方式。** 文档继续解释产品意图和兼容性承诺；当前实现事实由对应的代码、schema、manifest、资源源文件与运行证据共同表达。

| 事实类型 | 维护入口 | 文档形式 |
| --- | --- | --- |
| package 身份、依赖、入口、realization | package manifest + schema/校验器 | 自动生成 package reference |
| Rust API | 公共类型、接口、rustdoc | cargo doc 与可运行例子 |
| 存储/线协议 | 版本化 schema/codec + golden/conformance | 兼容性说明与迁移指南 |
| 默认配方、参数和资源绑定 | package data / import rules | 生成目录或示例，不手抄一张表 |
| 性能预算 | benchmark profile | 测量解释；报告保留机器、输入和 commit |
| 当前是否交付 | 对应产品的验收产物 | CI/release report，不能只填文档 checkbox |
| 为什么采用某个边界 | 简短 ADR | 背景、决策、代价、替代方案和替代关系 |
| 玩家/作者如何使用 | 可执行教程、操作指南 | 与所属 package、产品入口相邻 |

新 `docs/` 只保留项目入口、跨包架构图、开发/作者教程、少量 ADR 和跨包验收说明。package 的专属文档回到自己的 `README.md`/`docs/`；不重建一套与源码平行的 `docs/packages/` 镜像树。

采用 Diátaxis 对教程、操作指南、reference、explanation 的区分，但只在有内容时创建文件。ADR 用于记录重大选择及理由，不承担所有运行细节的百科全书职责。[Diátaxis](https://diataxis.fr/start-here/) · [ADR 定义](https://adr.github.io/)

旧文档按文件和具体条款判定：转成代码/schema/测试、浓缩为解释、保留为兼容承诺，或退休。旧 ADR 不自动继续支配新树；将保留与替代关系写入一次迁移记录。代码若违反仍有效的兼容承诺，是待修缺陷，不能仅以“代码是真相”免除。

同一个变更同时更新实现、必要测试、manifest/schema 和相邻使用说明。CI 校验已存在的生成文档与实例，不再建设一个需要大量人工回填的独立事实数据库。

## 9. 合仓与完整清理的方法

“完整清理”执行为：**当前工作树重新建基线，两个历史仓库保留恢复与追溯能力。** 不把删除 `.git` 或永久销毁历史混入目录重整。

1. 重新核对两仓 HEAD、refs、stash、worktrees、未提交与 ignored 数据。建立仓外恢复包，并实际验证恢复；`git bundle` 备份 refs 可达历史，但不能代替工作树、配置、未跟踪文件和运行数据备份。[Git bundle](https://git-scm.com/docs/git-bundle)
2. 建立 `codex/package-first-rebuild` 隔离工作树；记录固定的两仓来源。先把当前文档树从新分支的 active tree 退役，再建立新目录。
3. 逐项建立迁移清单：旧路径/commit → 新 owner/path → 复用、重构或删除 → 对应验证。源代码按功能迁入，避免把 demo 整体改名后宣布完成。
4. 两仓历史用明确的 merge 记录连接；推荐先完成可审查的重建提交，再以单独的历史归并提交连接固定 demo tip。若使用 `ours` 策略，仅用于已审查的历史收录，明确它本身不会导入文件；代码导入已由迁移提交完成。用 ancestor 检查证明两仓 tip 可追溯。[Git merge strategies](https://git-scm.com/docs/git-merge)
5. 旧 `docs/packages`/`platform`/多套 plan/status/meta 管理树不整体进入新 active tree。保留选中的语义和证据，旧原文可通过历史恢复。
6. demo 的 `target`、CAS 缓存、`.temp`、本地参考仓、开发日志、机器相关配置不导入；世界存档另行保存并只对副本测试迁移。可重建缓存与不可重建用户数据明确区分。
7. 保留适用的 LICENSE、第三方许可/署名、工具链锁和必要 supply-chain 记录；“重建”不删除这些义务。重写 README、AGENTS、CI 和开发入口，使它们直接描述新仓当前用法。
8. 新仓达到首个可玩/可构建基线后切换唯一开发入口；完成产品验收后，demo 停止双写并转为历史入口。远端仓库不需要删除来完成合仓。

迁移期间不承诺旧存档自动兼容。先保存副本，复测 palette、schema、generation epoch、玩家/容器状态；必要时实现显式离线迁移工具。既有 stable ID 不因目录变化重命名，禁止静默重生成已修改 chunk。

## 10. 实施顺序与每阶段的结束条件

| 阶段 | 工作 | 完成后必须能展示 |
| --- | --- | --- |
| M0 基线与恢复 | 两仓盘点、备份验证、旧运行轨迹、首个体验目标、关键美术场景清单 | 可以恢复；知道当前慢在哪里或明确尚无数据；资源与玩法有具体范围 |
| M1 新树与真实 package | 重建根目录、所有权检查、改 pack/source-build、多 crate 示例、生成产品入口 | 仓外构建的 package 含两 crates + 一资源；旧目录假设不再必要 |
| M2 最小产品迁入 | host/权威世界/持久化/输入/UI 的必要部分迁入 owner packages；首次资源消费 | 从新仓唯一入口启动、挖放、保存重进，使用真实 package 代码与资源；最小场景可玩 |
| M3 流送重整与验收 | 对照旧 trace 修 bottleneck，拆职责、覆盖预算、取消与近远景接缝 | 连续遍历及编辑达到既定预算，真实 GPU 画面和长时内存通过 |
| M4 完整玩法与设计资源 | 20–30 分钟循环、引导、道具/配方节奏、整套必要美术/声音/UI | 新玩家试玩可完成目标，游戏内画面、手感、音效与重进状态一起通过 |
| M5 收敛与正式合仓 | 残余职责迁移或退休、文档生成/教程验证、发行打包、历史归并、唯一入口切换 | 干净 checkout 可构建，产品可离线启动；无旧仓路径依赖或双写事实源 |

M0 的风格与场景定义、M2 的真实资源消费、M4 的完整资源验收是一条连续工作线；不能等性能和平台“全部写完”才开始设计。M2 之后每个切片保持新产品可启动。

优先级最高的风险是多 crate package 的隔离构建、默认原生产品选择、近远景编辑一致性和实际美术资源对性能的影响。先用最小真实切片验证这些风险，再估计后续工期；目前没有数据支持精确天数承诺。

## 11. 最终验收清单

- 所有自有 Rust 实现都有唯一 Lattice package owner，代码位于其内部 `crates/`。
- product 的 lock、构建和运行选择一致；替换 package 可改变真实行为和资源，不改通用 host。
- 代码、schema、数据、资源、兼容性测试和必要说明同仓维护；旧独立文档事实树已经退休。
- 区块加载的排队、生成、mesh、碰撞、GPU 呈现和销毁成本都有测量；目标场景通过真实设备预算。
- 完整玩家流程无需开发者介入，具有目标、反馈、进展、保存和恢复。
- 所有循环内资源都在真实场景中验收，拥有可编辑源或可再生成输入及合法来源；色块 fallback 不能冒充美术完成。
- 从干净环境构建并离线启动成品，不读取任何 `lattice-axiom-demo`、旧参考 checkout 或开发者绝对路径。
- 两仓历史可追溯、旧存档可恢复；只有 `lattice-axiom` 作为后续实现和文档的工作入口。
