# AI做游戏可行性

#### You:
我比较好奇，我到底可不可以尝试 去做游戏对吧，就是假如说我不在乎 任何 模型精细程度，就对着 开源项目抄 整个 渲染层的事情，我能不能做到 用 ai 去做 游戏呢？

#### ChatGPT:
可以，而且如果你把目标定义成「先做出一个能玩的、视觉可以很粗糙的系统型游戏」，这件事现在已经比你想象中现实很多。真正要避免的，是一开始把自己当成“要独立造一个商业级游戏引擎的人”；你其实更适合把现成渲染、物理、资源格式当基础设施，然后把精力集中到世界规则、实体系统和玩法上。

可以。更准确地说：**以你现在设想的游戏类型，如果你愿意大幅降低美术目标，并且不坚持自己造渲染器，我认为“一个主要程序员 + AI + 大量成熟开源基础设施”已经是可以认真尝试的路线。**

而且你刚才说的「对着开源项目抄整个渲染层」其实还可以再激进一点--**连抄都不要抄，尽可能直接把渲染层当第三方基础设施。**

你可以把自己的所谓“游戏引擎”想象成：

```text
你的游戏
│
├── World
│   ├── Chunk / 无限世界
│   ├── World Generation
│   ├── Biome
│   ├── Structure
│   ├── Save / Persistence
│   └── Simulation
│
├── Gameplay
│   ├── Entity
│   ├── Item
│   ├── Block
│   ├── Combat
│   ├── Machine / Automation
│   └── Module / Mod
│
├── Game Runtime
│   ├── ECS
│   ├── Scheduling
│   ├── Events
│   ├── Networking
│   └── Asset management
│
├── 第三方物理
│       Rapier
│
├── 第三方渲染
│       Bevy Render / Godot RenderingServer / ...
│
├── GPU abstraction
│       wgpu / Vulkan / DX12 / Metal
│
└── OS
```

真正属于你的东西，是上面那一半。

下面那一半，你原则上都可以不写。

这并不是什么奇怪的做法。Bevy 本身甚至就是按照这种思想设计的：包括 renderer 在内的引擎功能都是 plugin，甚至可以完全不加载 `RenderPlugin` 来构建 headless server。([Bevy Engine](https://bevyengine.org/learn/quick-start/getting-started/plugins/?utm_source=chatgpt.com), [Docs.rs](https://docs.rs/bevy?utm_source=chatgpt.com)) Bevy 的 renderer 内部已经负责 render phase、GPU asset preparation、pipelined rendering 等东西。([Docs.rs](https://docs.rs/bevy/latest/bevy/render/render_phase/index.html?utm_source=chatgpt.com), [Docs.rs](https://docs.rs/bevy/latest/bevy/render/render_asset/struct.RenderAssetPlugin.html?utm_source=chatgpt.com), [Docs.rs](https://docs.rs/bevy/latest/bevy/render/pipelined_rendering/index.html?utm_source=chatgpt.com))

所以我反而很不建议你让 AI 从：

```text
wgpu
 ↓
render graph
 ↓
PBR
 ↓
shadow
 ↓
lighting
 ↓
skinning
 ↓
animation
```

一路给你造。

这条路会把你拖死。

你要的是：

```rust
commands.spawn((
    Dragon,
    Transform::from_xyz(...),
    Mesh3d(dragon_mesh),
    ...
));
```

然后：

> “龙为什么显示出来？”

最好永远不要成为你主要需要解决的问题。

---

你真正需要理解的一件事情是：

**“做游戏”其实不是一个任务。它是大约二三十种工程问题叠起来。**

过去一个人很难做大型游戏，很大程度并不是因为其中某一个问题绝对无法解决，而是因为：

```text
rendering
animation
physics
UI
audio
input
world generation
AI
save
network
editor
asset pipeline
gameplay
debugging
profiling
build
deployment
...
```

每一个都要有人做。

AI 改变最大的地方就在这里。

它不意味着：

> Prompt：帮我做一个 Minecraft。

然后过几分钟得到游戏。

真正有价值的是，你可以把工程切成非常明确的小问题：

```text
实现 ChunkCoord
↓
写 property test

实现 Chunk lifecycle
↓
写 state transition test

实现 terrain generator interface
↓
写 deterministic test

实现 biome provider
↓
写 test

实现 structure placement
↓
写 conflict resolution test

实现 chunk serialization
↓
写 round-trip test

实现 player modified block overlay
↓
写 test
```

这种工作，AI 非常适合参与。

于是你的角色慢慢从：

> 写每一行代码的人

变成：

> **定义系统的人。**

这其实非常符合你前面一直在设计的那个游戏。你关心的本来就不是 shader 怎么写，而是：

> Chunk 是什么？\
> Package 怎样改变 world generation？\
> 已经生成的 chunk 为什么不能因为 generator 更新而变化？\
> Entity 应该怎么持久化？\
> 模型与 physics module 怎么分离？\
> biome / structure / block / machine 怎么插件化？

这些恰恰是**架构问题，而不是人海内容生产问题**。

---

而且，如果你的美术要求真的是：

> “我暂时根本不在乎模型精细程度。”

难度会骤降很多。

你完全可以第一版：

```text
树 = 几个 box
石头 = box
玩家 = capsule
怪物 = cube
房子 = cubes
龙 = 一个极其难看的 low-poly mesh
```

甚至：

```text
所有材质
= 单色 + 简单贴图
```

这都没有关系。

因为 Blender → glTF → engine 这条资产链已经非常成熟。glTF 本身就能携带 mesh、material、texture、camera、light、skeletal animation、shape key animation 等数据，Blender 官方直接支持导入导出。([Blender Documentation](https://docs.blender.org/manual/en/3.3/addons/import_export/scene_gltf2.html?utm_source=chatgpt.com), [Blender Documentation](https://docs.blender.org/manual/zh-hans/5.0/addons/import_export/scene_gltf2.html?utm_source=chatgpt.com))

也就是说，你根本不需要自己设计：

```text
RezicsGameModelFormat_v1
```

第一版直接：

```text
.blend
   ↓
.glb
   ↓
engine
```

就完事了。

等哪一天你的游戏真的活下来了，再讨论自己的 asset metadata。

---

物理也是一样。

你之前设想过：

> 模型说“龙肚子这里是软的”，攻击的时候由通用 module 做局部反应。

这个最终确实很复杂。

但第一版根本不用实现。

第一版：

```text
RigidBody
Collider
CharacterController
Raycast
Trigger
```

Rapier 已经给你提供 2D/3D rigid body、collider、scene query 和通用 character controller。([Rapier](https://rapier.rs/docs/?utm_source=chatgpt.com), [Rapier](https://rapier.rs/docs/user_guides/templates/scene_queries/?utm_source=chatgpt.com), [Rapier](https://rapier.rs/docs/user_guides/templates/character_controller/?utm_source=chatgpt.com))

所以：

```text
复杂软体龙
```

可以先变成：

```text
Dragon
 ├─ body collider
 ├─ head collider
 ├─ wing collider
 └─ tail collider
```

攻击：

```text
Sword hit
→ collider = DragonHead
→ damage × 1.5
→ spawn hit effect
```

已经是游戏了。

以后再把：

```text
HitReaction
```

替换成：

```text
HitReaction
 ├── RigidReaction
 ├── BoneReaction
 ├── SoftBodyReaction
 └── MaterialReaction
```

就好。

这也是我非常建议你从一开始就**把“能力接口”设计好，但不要把能力全部实现**。

---

至于你说的“抄开源项目”，这里我要稍微改一下措辞。

不是：

> 找到一个 renderer，把代码复制过来。

而最好是：

> **选择一个许可证适合的 renderer/engine，把它当 upstream。**

例如 Bevy 的仓库目前绝大部分代码是 MIT / Apache-2.0 双许可。([GitHub](https://github.com/bevyengine/bevy?utm_source=chatgpt.com)) Godot 则是 MIT，官方明确允许修改、分发，甚至商业和 proprietary derivative，只需要遵守对应的 license notice 要求。([Godot Engine documentation](https://docs.godotengine.org/en/stable/about/complying_with_licenses.html?utm_source=chatgpt.com), [Godot Engine](https://godotengine.org/license/?utm_source=chatgpt.com))

所以完全没有必要制造：

```text
my_renderer/
    复制了 80 万行代码
```

这种维护灾难。

最好：

```text
game/
engine/
    world/
    entity/
    gameplay/
    mod/
    network/

dependencies/
    bevy
    rapier
```

只有当 Bevy 某部分真的限制你：

```text
bevy_render
       ↓
你的 adapter
       ↓
override / fork
```

再 fork。

这会把 AI 的工作量也降低一个数量级。

---

如果让我按你的情况选，我实际上比较倾向：

```text
Rust
+
Bevy
+
Rapier
+
glTF
+
你自己的 World Runtime
```

而不是 Unity，也不是 Unreal，甚至不是一开始就 Godot。

不是因为 Bevy “最强”，而是因为你的脑子里那个游戏明显越来越像：

```text
Game
=
一个 simulation platform
+
plugin ecosystem
+
renderer
```

而不是：

```text
Game
=
一堆 Scene
+
一堆 GameObject
+
Scripts
```

Bevy 的 ECS + plugin 结构天然比较适合你这种思维，而且其 renderer 本身也是可模块化的；headless server 可以完全不加载 renderer。([Bevy Engine](https://bevyengine.org/learn/quick-start/getting-started/plugins/?utm_source=chatgpt.com), [Docs.rs](https://docs.rs/bevy/latest/bevy/app/index.html?utm_source=chatgpt.com))

反过来，如果你的目标只是：

> **我要最快看到一个人在世界里走起来。**

那我会选 Godot。

Godot 甚至允许你直接操作 `RenderingServer`，绕过 Scene/Node 系统；官方把它定义为所有可见内容的 API backend。([Godot Engine documentation](https://docs.godotengine.org/zh-cn/4.x/classes/class_renderingserver.html?utm_source=chatgpt.com), [Godot Engine documentation](https://docs.godotengine.org/en/stable/tutorials/performance/using_servers.html?utm_source=chatgpt.com))

所以大概是：

```text
最快做游戏
      Godot
        ↑
        │
        │
        ↓
      Bevy
最适合你自己定义 Runtime
```

---

不过这里有一个非常重要的现实边界。

AI 能大幅解决：

```text
代码量
boilerplate
API 使用
算法实现
测试
重构
shader 初稿
工具代码
serialization
editor utility
debugging
```

但是它目前仍然无法替你解决最核心的一件事：

> **“这个系统到底应该是什么？”**

例如你前面问：

> 岩浆造成的 block change 要不要保存？

AI 可以给你十种实现。

但究竟：

```text
lava simulation
=
world state

还是

lava simulation
=
deterministic simulation + event

还是

chunk unload 时 bake

还是

全部 delta persist
```

这是游戏架构。

如果这个决策错了，AI 会非常勤奋地给你写出 **50,000 行结构漂亮的错误代码**。

所以你真正需要掌握的技能，反而越来越不是：

> 我会不会写 Vulkan。

而是：

```text
Specification
Architecture
Invariant
Data model
Ownership
Lifecycle
State machine
Boundary
Test
Profiling
```

这几个东西。

而这些恰好已经是你在 Rezics 里长期做的工作方式。

---

因此我认为，你现在最危险的想法不是：

> “我是不是能力不够做游戏？”

而是：

> “我要先把我理想中的 engine 做出来，然后才能做游戏。”

**千万别这么走。**

正确顺序应该反过来。

第一版甚至只做：

```text
Infinite World
      ↓
可以走

Block
      ↓
可以挖 / 放

Entity
      ↓
可以生成 / 移动

Item
      ↓
可以捡

Combat
      ↓
可以打死一个怪

World Save
      ↓
退出重进东西还在

World Package
      ↓
加入一个 package
      ↓
出现一个新 biome + block + creature
```

如果这个东西成立了，你其实已经证明了你最重要的假设：

> **这个“游戏作为可组合世界平台”的架构是真的能玩的。**

模型全部是方块都没关系。

然后再：

```text
方块
↓
low-poly
↓
animation
↓
particles
↓
lighting
↓
漂亮 UI
↓
真正的 art direction
```

逐步替换。

所以如果问题非常直接地问：

> **“我一个人，现在靠 AI，能不能开始做这种游戏？”**

我的答案是：

**能。**

不是“一个人 + AI 现在可以轻松做 Minecraft 2”。

而是：**一个具备软件架构能力的人，现在已经可以把成熟的 renderer、physics、asset pipeline、audio 等系统拼起来，让 AI 承担巨量实现工作，然后把自己的精力集中到真正具有原创价值的 world simulation 和 gameplay architecture 上。**

而你甚至不需要先赌几十万美元。

你真正需要验证的第一件事，其实只是一件很小的东西：

> **让一个极丑的角色，走进一个程序生成的无限世界，挖掉一个方块；退出游戏，再进去，那个洞还在那里。**

如果这个 prototype 跑起来了，后面我们讨论的 Chunk、generation package、entity persistence、biome plugin、structure、machine，其实就都从“梦想中的游戏”变成了一个可以一层层扩展的软件工程项目。

**Sources:**

- [Bevy Engine](https://bevyengine.org/learn/quick-start/getting-started/plugins/?utm_source=chatgpt.com)
- [Docs.rs](https://docs.rs/bevy/latest/bevy/render/render_phase/index.html?utm_source=chatgpt.com)
- [Blender Documentation](https://docs.blender.org/manual/en/3.3/addons/import_export/scene_gltf2.html?utm_source=chatgpt.com)
- [Rapier](https://rapier.rs/docs/?utm_source=chatgpt.com)
- [GitHub](https://github.com/bevyengine/bevy?utm_source=chatgpt.com)
- [Godot Engine documentation](https://docs.godotengine.org/en/stable/about/complying_with_licenses.html?utm_source=chatgpt.com)
