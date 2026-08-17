# 插件架構與性能优化

#### You:
我比較好奇，如果插件架構設計的好的話，能不能做到插件載入的內容沒有性能損耗啊，遊戲

我舉一個例子，就是說我的世界吧，只有核心機制是核心，然後所有，包括所有生物，所有羣系，都是插件

#### ChatGPT:
可以，而且你描述的其實是一種很漂亮的遊戲架構：把「遊戲引擎／規則核心」縮到很小，然後把 Minecraft 裡的殭屍、苦力怕、牛、森林、生態域、方塊、物品、附魔、結構，甚至世界生成規則全部視為「內容插件」。

但要區分一句話：

**插件架構可以做到「幾乎沒有可測量的性能損耗」，但「動態插件抽象完全零成本」通常做不到。**

真正決定性能的，不是「它是不是插件」，而是插件邊界有沒有出現在 hot path 裡。

比如最糟糕的設計可能是每隻殭屍每一 tick 都這樣：

```text
entity.tick()
  -> pluginManager.find("minecraft:zombie")
  -> hashmap lookup
  -> dynamic dispatch
  -> event bus
  -> plugin callback
  -> scripting VM
```

如果世界裡有 50,000 個實體，每秒 20 tick，這些抽象成本就會被放大一百萬次。

但好的插件架構不是這樣。它會在載入階段完成插件解析：

```text
啟動
 ↓
載入 zombie plugin
 ↓
註冊 Zombie Components / Systems / AI / Assets
 ↓
分配 numeric type id
 ↓
編譯/建立 execution schedule
 ↓

遊戲迴圈：
ZombieSystem.run(zombies)
```

於是真正跑遊戲的時候，可能已經根本沒有「插件管理器」這個概念了。

例如：

```text
Plugin loading

minecraft:zombie
        ↓
EntityTypeId = 17
        ↓
components:
  Health
  Position
  Hostile
  ZombieAI
        ↓
system tables / archetypes
        ↓
continuous memory
```

runtime 就直接跑：

```cpp
for (ZombieAI& zombie : zombies) {
    updateZombie(zombie);
}
```

這和你把 Zombie 寫死在 core 裡面，在 CPU 執行層面可以幾乎一模一樣。

所以我甚至會把你說的架構再推進一步：

```text
Game
├── Kernel
│   ├── ECS
│   ├── scheduler
│   ├── rendering
│   ├── physics
│   ├── networking
│   ├── persistence
│   ├── registry
│   ├── asset system
│   └── plugin/module loader
│
├── minecraft-core-rules
│   ├── block mechanics
│   ├── entity mechanics
│   ├── inventory mechanics
│   ├── damage mechanics
│   └── world mechanics
│
├── minecraft-overworld
│   ├── plains
│   ├── forest
│   ├── desert
│   └── ...
│
├── minecraft-zombie
├── minecraft-creeper
├── minecraft-cow
├── minecraft-villager
├── minecraft-redstone
├── minecraft-enchantments
└── minecraft-nether
```

甚至 `minecraft-overworld` 都只是官方自帶插件。

這時候所謂 Vanilla Minecraft，其實就是：

> Engine + Mojang 官方插件集合

Mod 只是：

> 第三方插件集合

從架構上完全沒有「官方內容」和「Mod 內容」的特殊區別。

這種模型我其實很喜歡，因為它有一個非常強的特點：**你的官方內容本身就是插件系統最大的 integration test。**

如果官方自己寫 Zombie 都必須使用公開的：

```ts
registerEntity(...)
registerComponent(...)
registerSystem(...)
registerBiome(...)
registerFeature(...)
registerRecipe(...)
```

那 Mod 開發者得到的 API 就真的有能力做完整內容，而不是常見的：

> 「官方內部 API 什麼都能做，但是 Mod API 只能改一點皮毛。」

而且「插件」不一定意味著 DLL / `.so` 或 Java ClassLoader。

這一點非常重要。

我通常會把遊戲插件分成三層：

```text
Level 1
Data Plugin

JSON / binary / declarative DSL

方塊
物品
recipe
loot table
biome parameters
entity templates
animation graph
```

這種東西基本可以做到真正意義上的接近 **zero runtime overhead**，因為載入後就是普通遊戲資料。

第二層是：

```text
Level 2
Compiled Game Module

C++
Rust
Java
C#
native module
```

載入時：

```text
plugin.register(registry)
```

然後把 function pointer/system descriptor 放進 scheduler。

例如：

```rust
registry.add_system(ZombieAiSystem::new());
```

之後 scheduler 直接保存：

```text
[
    PhysicsSystem,
    ZombieAiSystem,
    CreeperAiSystem,
    VillagerSystem
]
```

每幀直接執行。

這個 overhead 基本就是一次 indirect call，甚至如果官方插件是 static linking，連這點都可以沒有。

第三層才是：

```text
Level 3
Sandboxed Script Plugin

Lua
JavaScript
WASM
etc.
```

這種就比較難做到 zero-cost，因為：

```text
host ↔ VM
```

本身就存在邊界。

尤其如果：

```text
every entity × every tick × WASM call
```

性能很容易出問題。

但是仍然可以透過 coarse-grained API 解決。

不要：

```js
onZombieTick(zombie)
```

50,000 次。

而是：

```js
updateZombies(zombieQuery)
```

一次把整批 zombie 處理掉。

---

所以如果你的目標真的是：

> 「所有生物、所有群系、所有內容都是插件。」

我認為完全可行，而且有可能比傳統架構還漂亮。

真正應該設計成 core 的東西其實非常少，大概是：

**機制，而不是內容。**

例如：

```text
Core:
Entity exists
Entity can have Components
Systems process Components

不是：
Zombie exists
Cow exists
Creeper exists
```

同樣：

```text
Core:
World consists of chunks
Biome can provide terrain parameters
Features can modify chunks
Generators produce terrain

不是：
Forest exists
Desert exists
Taiga exists
```

以及：

```text
Core:
Item exists
Item can have Components
Item can participate in recipes

不是：
Diamond Sword exists
Apple exists
Iron Ingot exists
```

最後你的 Minecraft 甚至可以變成：

```text
Minecraft Engine
        +
Vanilla Content Pack
```

把 Vanilla Content Pack 拿掉，啟動遊戲：

```text
空世界
沒有方塊
沒有 biome
沒有生物
沒有 item
```

然後安裝：

```text
@minecraft/stone
@minecraft/dirt
@minecraft/plains
@minecraft/zombie
...
```

遊戲逐漸變成 Minecraft。

這其實是一種非常徹底的 **microkernel / content-oriented architecture**。

而性能上最核心的一條規則就是：

> **Plugin boundary should exist at registration time, not execution time.**

插件在「載入時」是插件。

一旦載入完成，它就應該被降解成：

```text
data
component
system
function pointer
numeric ID
compiled schedule
```

而不是 runtime 還不停問：

```text
PluginManager，這個東西是哪個 plugin 的？
```

如果做到這一點，我會認為「所有 Minecraft 內容都是插件」不只是可行，甚至是我很推薦的一種大型遊戲／遊戲引擎架構。

而且這裡還有一個挺有意思的進一步結論：**第一方插件甚至可以靜態連結，第三方插件動態載入，但兩者使用完全相同的 API。** 這樣官方 build 可以享受 LTO、inlining 等最佳化，第三方仍然保有真正的 plugin 能力；架構統一，但 deployment model 不需要統一。這大概是最接近你所說「插件沒有性能損耗」的方案。


#### You:
第一方插件甚至可以靜態連結，第三方插件動態載入

那如果是官方 core 是開源的，那麼整合包，或者 類似 nixos 的語法，也能讓第三方也做到 靜態鏈接

#### ChatGPT:
對，而且如果官方 core 開源，這條路其實會非常自然。你可以把「整合包」理解成不是一組執行時插件，而是一份「遊戲組態／建置描述」，然後用類似 NixOS 的方式把 core、官方內容、第三方內容組合成一個新的完整遊戲映像。這樣第三方插件也完全可以靜態鏈接，甚至經過 LTO、dead-code elimination、跨模組 inline，最後在 runtime 幾乎看不出它原本是一個 plugin。

例如概念上可以長這樣：

```nix
game = rezicsGame {
  core = minecraft-core;

  modules = [
    vanilla.blocks
    vanilla.entities
    vanilla.biomes

    mods.create
    mods.terralith
    mods.my-custom-mobs
  ];

  optimizations = {
    lto = true;
    strip = true;
  };
};
```

這份東西不是「啟動遊戲後載入 Create.jar」，而是描述：

```text
Minecraft Core
+ Vanilla
+ Create
+ Terralith
+ Custom Mobs
        ↓
resolve dependency graph
        ↓
compile / link
        ↓
minecraft-custom-build
```

最後生成的其實是一個新的 distribution。

如果做得徹底，它跟 NixOS 很像：

```text
NixOS:
Linux kernel
+ systemd
+ nginx
+ postgres
+ config
→ 一個 system closure

遊戲:
Game kernel
+ renderer
+ physics
+ vanilla modules
+ third-party modules
+ configuration
→ 一個 game closure
```

所以「modpack」可以不再只是：

```text
mods/
  a.jar
  b.jar
  c.jar
```

而是：

```text
game.lock
game.nix
```

或者：

```toml
[game]
core = "1.8"

[modules]
minecraft-vanilla = "1.8"
create = "6.2"
terralith = "3.1"
```

建置系統把整個 dependency graph 解出來。

這種模型對遊戲其實很強，因為你可以同時支持兩種 deployment。

開發階段：

```text
core
 + dynamic plugin A
 + dynamic plugin B
```

優點是熱重載、編譯快、方便調試。

發布整合包：

```text
core
 + static A
 + static B
 + static C
       ↓
whole-program optimization
```

所以插件作者根本不需要選「我是 static plugin 還是 dynamic plugin」。他的 source-level API 可以完全相同。

例如 Rust：

```rust
pub trait GameModule {
    fn register(&self, registry: &mut Registry);
}
```

官方模組：

```rust
register_module::<ZombieModule>();
```

第三方模組也是：

```rust
register_module::<CreateModule>();
```

dynamic 模式只是 loader 幫你找到 `CreateModule`。

static 模式則是 build system 在編譯時生成：

```rust
fn register_all(registry: &mut Registry) {
    ZombieModule.register(registry);
    CowModule.register(registry);
    CreateModule.register(registry);
    TerralithModule.register(registry);
}
```

然後這些函數甚至可以被 compiler inline 掉。

所以前面我說：

> Plugin boundary should exist at registration time, not execution time.

還可以再進一步：

> **Static build 裡甚至連 registration boundary 都可以被 compiler 消掉。**

例如：

```rust
registry.add_system(CreateMechanicalSystem::new());
```

經過編譯之後可能變成 scheduler 裡一個固定 slot。

runtime 根本不知道它是：

```text
官方程式碼？
第三方程式碼？
插件？
core？
```

對 CPU 來說都是 machine code。

這就是所謂的「插件是一種組織形式，而不一定是一種 runtime abstraction」。

這一點非常重要。

很多人看到 plugin architecture 會自然想到：

```text
dlopen()
reflection
event bus
virtual interface
IPC
Lua
WASM
```

但其實都不是必要條件。

你完全可以定義：

```text
Plugin = independently developed module
```

而不是：

```text
Plugin = dynamically loaded module
```

這兩件事本來就是正交的。

你的系統甚至可以有四種 mode：

```text
Source module
    ↓
┌───────────────────────┐
│ static native         │
│ dynamic native        │
│ sandboxed WASM        │
│ interpreted script    │
└───────────────────────┘
```

同一個插件 manifest 可以聲明：

```toml
[module]
name = "create"
version = "6.0"

[build]
supports = [
  "native-static",
  "native-dynamic"
]
```

而另一個不可信插件可能只允許：

```toml
[build]
supports = ["wasm"]
```

這樣「插件模型」和「執行隔離模型」也是解耦的。

我甚至會建議你不要直接叫 static plugin / dynamic plugin，而是區分：

```text
Module
```

和：

```text
Module realization
```

也就是模組在邏輯上永遠是模組，只是最終可以：

```text
module
 ├─ linked
 ├─ loaded
 ├─ sandboxed
 └─ interpreted
```

這樣 architecture 會乾淨很多。

而且 Nix 式設計還會帶來一個對 mod 生態很有價值的特性：**可重現 build**。

今天有人發布：

```text
Edge's Fantasy Modpack 1.4
```

實際上發布的是：

```text
core hash
module hashes
config
compiler version
assets hashes
```

那麼任何人都可以：

```text
game build edge-fantasy@1.4
```

得到完全相同的遊戲。

這比現在很多遊戲 Mod 的：

```text
請安裝 loader 0.15.8
請自己下載 137 個 mod
A 必須 2.4.1
B 不能用最新版
C 記得覆蓋 config
```

要乾淨非常多。

而且 server/client 也可以直接比較 closure hash：

```text
Server:
game closure =
sha256:abcdef...

Client:
game closure =
sha256:abcdef...
```

相同就知道：

```text
程式碼版本一致
資產一致
配置一致
插件一致
```

對 multiplayer compatibility 非常漂亮。

甚至可以做到：

```text
server:
required modules = [...]

client:
missing module Create@6.2

→ fetch
→ build/download binary cache
→ restart
```

這又很像 Nix binary cache。

所以如果真的做一個這種遊戲，我反而很推薦借鑑 Nix，而不只是借鑑 Minecraft Forge/Fabric。

架構可能是：

```text
Game Definition
       ↓
Dependency Solver
       ↓
Module Graph
       ↓
Build Planner
       ↓
┌────────────────────┐
│ Binary cache hit?  │
└─────────┬──────────┘
          ↓
Compile missing modules
          ↓
Static / dynamic realization
          ↓
Game closure
```

而不是傳統的：

```text
Game.exe
  ↓
scan ./mods
  ↓
load 100 DLLs
```

唯一需要注意的是：**靜態鏈接不是白送的。**

它最大的成本反而不是性能，而是 distribution。

第一，build 會變重。100 個 C++/Rust 插件全部 static，再加 LTO，整合包每次更新都可能要重新 link。

第二，binary cache 變得非常重要。否則每個玩家都本地 compile Minecraft 會很荒謬。所以比較合理的是：

```text
source recipe
      ↓
official/community builder
      ↓
signed binary closure
      ↓
CDN/cache
```

跟 Nix cache 幾乎一樣。

第三，是授權問題。GPL/LGPL/AGPL/proprietary plugin 在 static linking 下的法律關係可能不同，所以 package manager 最好把 license metadata 做成一等公民。

第四，是安全性。static native third-party code 等於完全 trusted code：

```text
插件 = 遊戲本身
```

它可以：

```text
讀 filesystem
開 socket
讀 token
memory corruption
```

所以不能因為「static 更快」就把所有未知 Mod 都 static。

比較好的策略可能是：

```text
Trusted mod
→ native static

Trusted + frequently updated
→ native dynamic

Untrusted/community mod
→ WASM sandbox
```

而官方整合包作者可以顯式聲明 trust。

所以你最開始那個想法其實可以發展成一個非常完整的模型：

> **遊戲不是一個 executable 加 plugins；遊戲本身就是 modules 的 immutable composition。**

核心只提供機制。

Vanilla 是一組 module。

Mod 是另一組 module。

Modpack 是一份 composition expression。

Release 是這份 expression 所產生的 closure。

至於 module 最終是 static、dynamic 還是 WASM，只是 build backend 的選擇。

如果真的按照這個方向設計，我覺得它比「Minecraft + Fabric/Forge」更接近「NixOS 作為遊戲平台」：**不是給一個完成的遊戲打補丁，而是用 declarative modules 定義出這個遊戲本身。**


#### You:
declarative modules 是機制的核心，nixos，or package.json or whatever

不過他動態加載的部分也支持 掃描文件夾加載插件，都是能支持的

然後就是 比較好奇，那渲染呢？因爲 遊戲邏輯是可以做，但是渲染很難很難啊

#### ChatGPT:
對，渲染確實是這種「萬物 module 化」架構裡最難的一塊，而且難點比遊戲邏輯高一個量級。

因為遊戲邏輯天然可以寫成：

```text
Module
  → 註冊 component
  → 註冊 system
  → scheduler
```

但 GPU 不喜歡這種高度動態的抽象。GPU 喜歡的是：少量 pipeline、大批量資料、連續 buffer、預先編譯 shader、極少 state change。所以如果真的讓每個插件自己 `draw()`，架構很快就會炸掉。

最重要的原則其實和我們剛才說的遊戲邏輯完全一致：

> 插件負責「描述如何渲染」，Renderer 負責「真正執行渲染」。

而不是插件直接控制 GPU。

例如千萬不要設計成：

```cpp
plugin.onRender(entity) {
    gpu.bindShader(...);
    gpu.bindTexture(...);
    gpu.draw(...);
}
```

如果 100 個 Mod 都這樣做，你會得到：

```text
Zombie plugin → draw
Sword plugin  → draw
Hat plugin    → draw
Particle mod  → draw
Create mod    → draw
...
```

Renderer 根本沒辦法全局排序、batch、instance、做 occlusion culling，也不知道哪些 draw call 可以合併。

更合理的是讓插件註冊「Render Description」：

```text
Zombie Module

renderable:
    mesh = zombie.mesh
    material = minecraft:entity
    skeleton = humanoid
    animation = zombie.animation
    shadow = true
```

載入之後 Renderer 把它編譯成自己的內部表示：

```text
Plugin description
        ↓
Render Archetype
        ↓
Pipeline ID = 14
Mesh ID = 271
Material ID = 38
        ↓
instance buffer
```

遊戲 runtime 真正看到的可能只是：

```cpp
struct RenderInstance {
    Transform transform;
    MeshId mesh;
    MaterialId material;
};
```

然後 Renderer 可以把整個世界重新整理成：

```text
Pipeline 14
 ├─ zombie × 482
 ├─ skeleton × 233
 └─ player × 37

Pipeline 15
 ├─ cow × 150
 └─ sheep × 183
```

接著：

```text
culling
→ sorting
→ batching
→ indirect draw
→ GPU
```

插件的存在已經消失了。

這和前面的 static module 思想其實是一樣的。

我會把整個遊戲拆成兩個世界：

```text
Simulation World
      ↓
Render Extraction
      ↓
Render World
      ↓
Renderer
```

Simulation World 是：

```text
Zombie
Health
AI
Inventory
Position
Biome
Block
```

而 Render World 根本不應該知道什麼是 Zombie。

它只知道：

```text
MeshInstance
SkinnedMesh
Light
ParticleEmitter
Decal
TerrainChunk
Camera
```

例如：

```text
Zombie
↓ extraction
SkinnedMeshInstance {
    mesh: 15
    material: 7
    skeleton: 3
    transform: ...
}
```

所以：

> `Zombie` 是插件概念。

但：

> `SkinnedMeshInstance` 是 renderer core 概念。

這個邊界非常關鍵。

---

Minecraft 式方塊反而是非常適合 plugin 化的。

插件可以定義：

```text
grass_block:
    geometry = cube
    textures:
        top = grass_top
        side = grass_side
        bottom = dirt
    opaque = true
```

世界生成完成以後：

```text
Block data

[grass][grass][stone][stone]...
           ↓
       Chunk Mesher
           ↓
    optimized mesh
           ↓
    vertex/index buffer
```

Renderer 最終根本不知道：

```text
這個 quad 是 minecraft:grass_block
還是某個 Mod 的 magic_crystal_block
```

它看到的只是：

```text
material=17
vertices=...
indices=...
```

甚至可以：

```text
greedy meshing
meshlets
GPU culling
indirect rendering
```

全部照常做。

所以方塊插件本身幾乎沒有 runtime rendering overhead。

---

Biome 更簡單。

Biome 插件可以只是提供：

```text
grass_color
fog
sky
water
ambient lighting
weather profile
terrain material
```

例如：

```text
mymod:hell_forest
{
    sky: #441122
    fog: #220011
    grassTint: ...
    atmosphere: ...
}
```

renderer 收到的是一小組 parameters：

```text
BiomeRenderParameters
```

照樣沒有什麼插件 overhead。

---

真正麻煩的是「插件要添加新的渲染機制」。

比如某個 Mod 說：

> 我的 Portal 需要特殊的 recursive rendering。

或者：

> 我的水晶需要自己的 volumetric ray marching。

或者：

> 我要增加 SSAO 之後、Bloom 之前的一個 pass。

這時普通的：

```text
Mesh
Material
Texture
```

已經不夠。

這就是為什麼現代 renderer 通常需要另一層：

```text
Render Graph
```

例如：

```text
DepthPrepass
     ↓
GBuffer
     ↓
Lighting
     ↓
Transparent
     ↓
Bloom
     ↓
Tonemap
```

插件可以聲明：

```text
MyMagicPass
inputs:
    sceneColor
    depth

outputs:
    magicColor

after:
    lighting

before:
    transparent
```

系統最後組成：

```text
Depth
  ↓
GBuffer
  ↓
Lighting
  ↓
MyMagicPass
  ↓
Transparent
  ↓
Bloom
```

但還是 Renderer 統一：

```text
dependency analysis
resource lifetime
barriers
render target allocation
pass scheduling
```

插件不能自己隨便：

```text
vkCmdPipelineBarrier(...)
vkCreateImage(...)
```

否則抽象就崩了。

因此我會把 renderer plugin API 分大概三個等級。

第一級是幾乎所有內容：

```text
Assets / Render Data

Mesh
Texture
Material
Animation
Particles
Block model
Terrain surface
Light parameters
```

這層最自由，而且性能幾乎完全可以做到與 core content 一樣。

第二級是：

```text
Render Feature

Custom material
Custom shader
Custom geometry generator
Custom particle simulation
Custom post process
```

這時插件仍然不碰底層 GPU，只提供 renderer 可以編譯的描述。

第三級才是：

```text
Render Graph Extension

Custom passes
Custom buffers
Custom compute pipeline
```

這是一個 power-user API。

再下面的：

```text
Raw Vulkan
Raw DX12
Raw Metal
```

我反而不會當成普通插件 API，而可以作為：

```text
unsafe native extension
```

因為一旦給 Mod raw GPU access，Renderer 幾乎不可能再保證：

```text
性能
正確同步
跨平台
資源生命週期
device loss recovery
```

---

shader 也是這個架構的一個核心問題。

你不能要求 plugin 作者分別寫：

```text
Vulkan GLSL
DirectX HLSL
Metal MSL
```

比較合理的是定義一種統一 shader source，比如：

```text
WGSL
Slang
自己的 Shader IR
```

然後：

```text
mod shader
    ↓
shader compiler
    ↓
intermediate representation
    ↓
Vulkan / DX12 / Metal
```

而 declarative build 正好又能幫忙。

比如：

```nix
game {
  modules = [
    vanilla
    create
    magic-rendering
  ];
}
```

build 時就能：

```text
收集所有 shader
       ↓
分析 shader variants
       ↓
compile pipelines
       ↓
pipeline cache
       ↓
package
```

所以 static modpack 甚至有一個很大的優勢：

**你在發布遊戲之前就知道整個 render feature set。**

那麼就可以做：

```text
shader permutation pruning
pipeline precompilation
resource packing
texture atlas construction
mesh optimization
LTO
```

這可能比普通動態 Mod 架構反而更快。

而你說的另一條路：

```text
./plugins
  create/
  magic/
  biome-x/
```

啟動時掃描動態插件，也完全可以同時存在。

只是這時：

```text
scan
↓
load manifest
↓
register render features
↓
compile missing shaders
↓
build pipelines
↓
cache
```

第一次啟動可能會慢一些，之後直接讀 cache。

所以 static declarative build 和 folder scanning 不是兩套架構。

它們應該只是：

```text
Module Discovery
├─ declarative closure
├─ static linker
├─ package manager
└─ directory scanner
```

最後全部得到同一個：

```text
ResolvedModuleGraph
```

後面的 Renderer 根本不在乎這個 module 是怎麼進來的。

---

其實可以把整個架構濃縮成這樣：

```text
                      Modules
                 /       |       \
             Vanilla    Mod A    Mod B
                 \       |       /
                  Module Registry
                         │
        ┌────────────────┼────────────────┐
        ↓                ↓                ↓
   Simulation       Assets/Content   Render Features
        ↓                                ↓
   ECS World                        Render Registry
        ↓                                ↓
        └────── Render Extraction ───────┘
                         ↓
                    Render World
                         ↓
                     Culling
                         ↓
                     Batching
                         ↓
                   Render Graph
                         ↓
                  GPU submission
```

這樣你會得到一個很有意思的結果：

**插件架構一直存在於 authoring / composition 層，但在 hot path 中基本消失。**

尤其 entity renderer，可以做到：

```text
10,000 vanilla zombies
10,000 mod zombies
```

對 GPU renderer 根本沒有本質區別。

最後最難的其實不是「插件能不能畫東西」，而是另外一個問題：

> **你究竟允許插件創造多少種全新的 rendering primitive？**

如果 API 只提供：

```text
mesh
material
animation
particle
terrain
```

那非常容易做到極高性能。

如果你允許：

```text
我要自己改 shadow algorithm
我要自己改 deferred lighting
我要自己增加 geometry pass
我要替換 atmosphere renderer
```

那就必須進入 Render Graph / Render Feature 系統，架構複雜度會陡增。

但這仍然可以做好。

所以我覺得一個理想的這類遊戲引擎，核心甚至不應該有：

```text
ZombieRenderer
CowRenderer
GrassBlockRenderer
```

而應該只有大約：

```text
MeshRenderer
SkinnedMeshRenderer
TerrainRenderer
ParticleRenderer
LightingRenderer
UIRenderer
RenderGraph
MaterialSystem
```

然後所有「Minecraft 長什麼樣」的東西都來自 modules。

這樣 Vanilla 真正就只是內容包，而 renderer 仍然保持高度集中、全局可優化。這大概就是你想要的那種「極端插件化，但性能並沒有因為插件化而崩掉」最重要的一步。


#### You:
我比較好奇，那有沒有專門的 rendering 開源 引擎？

然後 其實主要就是 自定義 實體，自定義物理 這種 mc 都支持的，比如說 你冒險模組肯定要有支持這個 的怪物對吧

#### ChatGPT:
有，而且如果你的目標是「自己做遊戲邏輯／ECS／模組系統，只把 GPU 那一坨交給成熟項目」，其實有幾個很合適的開源 renderer。你後面說的「冒險模組加自定義怪物」也正好說明：大部分插件根本不需要自定義整套 renderer，只需要自定義 **renderable + animation + movement/physics behavior**。

有，而且你現在描述的需求，其實讓我覺得「專門的 Rendering Engine + 專門的 Physics Engine + 自己的 Game/Module Core」比直接採用 Godot/Unreal 這種完整 Game Engine 更契合。

目前比較值得看的幾個開源 rendering 項目，可以大致分成這樣：

| 項目                  | 它實際提供什麼                                         | 和你這個架構的匹配度                     |
| ------------------- | ----------------------------------------------- | ------------------------------ |
| **OGRE-Next**       | 真正意義上的 3D Rendering Engine                      | **非常高**                        |
| **Filament**        | 完整、偏 PBR 的 real-time renderer                   | 高，但比較 opinionated              |
| **The Forge**       | 現代跨平台 rendering framework                       | 很高，但更底層                        |
| **bgfx**            | 跨 API graphics abstraction / mid-level renderer | 高，但你還得自己造很多 renderer           |
| **Diligent Engine** | graphics API abstraction + rendering framework  | 類似 bgfx，但更 modern API-oriented |

OGRE-Next 尤其符合你說的「我不要一個 Game Engine，我只要 Rendering Engine」。它自己甚至明確說：它是 **3D graphics rendering engine，不是提供 networking、sound、physics 等功能的 game engine**。([GitHub](https://github.com/OGRECave/ogre-next?utm_source=chatgpt.com), [Ogre Cave](https://ogrecave.github.io/ogre-next/api/3.0/index.html?utm_source=chatgpt.com))

所以你完全可以做：

```text
Your Game Core
│
├── Module System
├── ECS / World
├── Networking
├── Game Rules
├── AI
├── Asset System
│
├── Physics
│    └── Jolt
│
└── Rendering
     └── Ogre-Next
```

Jolt 剛好又是另一個很乾淨的開源 C++ physics/collision library，本身就是面向遊戲與 VR 的多核心 rigid-body physics library。([GitHub](https://github.com/jrouwe/JoltPhysics?utm_source=chatgpt.com), [GitHub](https://github.com/jrouwe/JoltPhysics/blob/master/Docs/Architecture.md?utm_source=chatgpt.com))

如果你偏 Rust，那 Physics 可以看 Rapier，它就是獨立的 Rust 2D/3D physics engine。([Rapier](https://rapier.rs/docs/?utm_source=chatgpt.com), [Rapier](https://rapier.rs/docs/user_guides/rust/getting_started/?utm_source=chatgpt.com))

而 bgfx 則更底層。它官方直接把自己定位成：

> Bring Your Own Engine/Framework

也就是它幫你抽象 Vulkan、D3D、Metal、OpenGL、WebGPU 等 backend，但 PBR、shadow、scene、material 等更高層內容基本還是你的事情。([GitHub](https://github.com/bkaradzic/bgfx?utm_source=chatgpt.com), [GitHub](https://github.com/bkaradzic/bgfx/issues/660?utm_source=chatgpt.com))

所以如果你的目的只是「我不想碰 Vulkan/DX12」，bgfx 很舒服；但如果你的目的其實是「我連完整的 Mesh/Material/Shadow/Scene rendering 都不想重新造」，那我反而會優先研究 **OGRE-Next**。

Filament 又是另一個方向。它是一個完整的跨平台 real-time PBR rendering engine，支援 Android、iOS、Linux、macOS、Windows 和 WASM，material system 也相當完整。([Google GitHub](https://google.github.io/filament/?utm_source=chatgpt.com), [Google GitHub](https://google.github.io/filament/Materials.md.html?utm_source=chatgpt.com))

所以可以粗略理解：

```text
Vulkan / DX12 / Metal
        ↑
   bgfx / Diligent
        ↑
你自己寫 renderer
```

而：

```text
Vulkan / DX12 / Metal
        ↑
   OGRE / Filament
        ↑
   你的 Game Engine
```

The Forge 則介於兩者之間，更像給真正想自己造引擎的人用的 rendering framework，而且覆蓋 PC、Steam Deck、Android、Apple 平台以及多種主機平台。([GitHub](https://github.com/confettifx/the-forge?utm_source=chatgpt.com))

---

但你後半句其實正好擊中了最重要的一點：

> 「主要就是自定義實體、自定義物理，比如冒險模組自己的怪物。」

**這種需求其實完全不需要讓插件擁有 renderer。**

甚至可以說，99% 的冒險 Mod 都不應該碰 `RenderGraph`。

比如一個插件加入：

```text
mod:ancient_dragon
```

它需要的其實可能是：

```text
Entity
├── Transform
├── Health
├── DragonAI
├── FlyingMovement
├── BossCombat
│
├── Physics
│   ├── Body / CharacterController
│   ├── CollisionShape
│   └── Hitboxes
│
└── Visual
    ├── Mesh
    ├── Skeleton
    ├── Material
    ├── AnimationGraph
    └── ParticleEmitters
```

注意這裡：

**Dragon 是插件定義的。**

但是：

```text
Mesh
Skeleton
RigidBody
Collider
Animation
ParticleEmitter
```

都是 engine core 提供的 primitive。

這就跟 Minecraft 的 custom entity 很接近。現在 Fabric 的文檔本身就是把自定義 entity 拆成 entity 邏輯，以及客戶端的 renderer/model/animation；服務端管理 entity 行為，客戶端負責模型、材質與動畫的呈現。([Fabric Documentation](https://docs.fabricmc.net/zh_cn/develop/entities/first-entity?utm_source=chatgpt.com), [Fabric Documentation](https://docs.fabricmc.net/develop/entities/first-entity?utm_source=chatgpt.com))

NeoForge 目前甚至已經採用非常接近我們剛才討論的模式：Entity Renderer 先產生/更新 `EntityRenderState`，再透過 submission 把需要的 rendering feature 提交出去，而不是 entity 自己立即到 GPU 上畫。([NeoForged Documentation](https://docs.neoforged.net/docs/1.21.10/entities/renderer/?utm_source=chatgpt.com), [NeoForged Documentation](https://docs.neoforged.net/docs/rendering/feature/?utm_source=chatgpt.com))

這其實很漂亮：

```text
Dragon Entity

Simulation
──────────────
position
velocity
AI
attack state
animation state
health
        │
        │ extraction
        ▼
Render State
──────────────
transform
mesh
skeleton pose
material
effects
        │
        ▼
Renderer
```

Renderer 不需要知道：

> 「這是一隻 Ancient Dragon。」

它只知道：

> 「這裡有一個 skinned mesh，需要這個 skeleton pose、這幾個 materials 和兩個 particle emitters。」

---

而「自定義物理」我會再區分一下。

冒險模組裡絕大部分所謂 custom physics，其實不是：

> 插件實作了一套自己的牛頓力學 solver。

而是：

> 插件定義了一個特殊的 movement/controller。

例如 Dragon：

```text
FlyingMovement
```

Spider：

```text
WallClimbingMovement
```

Slime：

```text
BounceMovement
```

Ghost：

```text
NoClipMovement
```

Giant：

```text
HeavyCharacterMovement
```

這些完全可以是 plugin system：

```rust
register_system::<DragonFlightSystem>();
register_system::<SpiderClimbSystem>();
```

但是底下碰撞檢測：

```text
shape cast
ray cast
contact
broad phase
collision query
```

仍然交給 Jolt/Rapier。

所以你可能有：

```text
DragonFlightSystem
        │
        ├─ query terrain collision
        ├─ query entities
        ├─ calculate desired velocity
        └─ update physics body
                 │
                 ▼
              Jolt
```

這樣插件自由度其實已經非常高。

如果真的有某個 Mod 說：

> 「我的星球有球形重力。」

也不需要換掉整套 Physics Engine。

它可以註冊：

```text
SphericalGravitySystem
```

計算：

```text
gravity direction =
entity → planet center
```

然後把 force/acceleration 交給 physics engine。

甚至：

```text
ZeroGravitySystem
WaterBuoyancySystem
WindSystem
MagneticFieldSystem
PortalTransformSystem
```

都可以作為 module system。

只有非常少數情況，比如：

```text
布料
流體
破壞模擬
特殊軟體
完全不同的物理世界
```

才需要真正加入新的 physics backend。

所以我會把 API 做成兩級：

```text
普通 Mod
────────────
Collider
RigidBody
CharacterController
MovementSystem
Force
Constraint
RayCast
ShapeCast
Trigger
```

已經足以做：

```text
怪物
Boss
飛龍
飛船
載具
魔法彈
鉤爪
平台
陷阱
```

而高級插件才有：

```text
PhysicsFeature
──────────────
Custom Solver
Custom Constraint
Custom Broadphase interaction
Custom Simulation World
```

---

這樣你之前的「萬物都是 module」架構就非常完整了：

```text
                   Game Core
                       │
        ┌──────────────┼──────────────┐
        │              │              │
       ECS          Physics        Renderer
        │              │              │
        │            Jolt        Ogre-Next
        │              │              │
        └──────────────┼──────────────┘
                       │
                 Module API
                       │
        ┌──────────────┼───────────────┐
        │              │               │
     Vanilla        Adventure Mod   Tech Mod
        │              │               │
      Zombie          Dragon         Machine
      Cow             Boss           Robot
      Boat            Spell          Vehicle
```

而一個 `DragonModule` 可以同時註冊：

```text
DragonEntity
DragonAISystem
DragonFlightSystem

DragonMesh
DragonSkeleton
DragonAnimationGraph

DragonCollider
DragonHitboxes

DragonFireParticle
DragonFireSound
```

但是它**沒有自己的 renderer，也沒有自己的 physics engine**。

這一點我覺得特別重要：你不是把 engine 的能力限制死，而是在定義一套非常廣泛的「mechanism primitives」。

因此 Vanilla 的 Zombie 和第三方的超大型八頭龍，在架構上根本沒有特權差異：

```text
minecraft:zombie
adventure:ancient_dragon
```

都只是一個 module 註冊出來的 Entity Type。

而且如果前面那套 Nix 式 declarative composition 繼續存在，那 `DragonFlightSystem` 最後還可以被 static link + LTO；`Dragon` 的 mesh/material/shader 又可以在 build 階段 bake。**最終遊戲性能完全沒有理由因為「這條龍來自第三方插件」而天然比官方 Zombie 差。**

如果讓我現在選一套最貼近這個思想、又避免自己從 Vulkan 開始造輪子的 C++ 技術底座，我會首先深入研究：

**OGRE-Next + Jolt Physics + 自己的 ECS/module/composition layer。**

這三層的責任邊界恰好非常乾淨。([GitHub](https://github.com/OGRECave/ogre-next?utm_source=chatgpt.com), [GitHub](https://github.com/jrouwe/JoltPhysics?utm_source=chatgpt.com))

**Sources:**

- [GitHub](https://github.com/OGRECave/ogre-next?utm_source=chatgpt.com)
- [Rapier](https://rapier.rs/docs/?utm_source=chatgpt.com)
- [Google GitHub](https://google.github.io/filament/?utm_source=chatgpt.com)
- [Fabric Documentation](https://docs.fabricmc.net/zh_cn/develop/entities/first-entity?utm_source=chatgpt.com)
- [NeoForged Documentation](https://docs.neoforged.net/docs/1.21.10/entities/renderer/?utm_source=chatgpt.com)


#### You:
是的，就比如模型定義好之後，受到攻擊的晃動效果，移動的抖動效果，這些都不再需要插件去單獨的設計

#### ChatGPT:
對，這樣才是最舒服的抽象：插件定義「這是什麼」，引擎定義「它在遊戲世界裡應該如何自然地表現」。

比如插件只需要定義一個怪物的基本視覺與行為資產：

```text
AncientDragon
├── model
├── skeleton
├── animations
│   ├── idle
│   ├── walk
│   ├── fly
│   └── attack
├── collider
├── movement
└── combat
```

至於「受到攻擊時閃紅、模型後仰/晃動」「走路時細微 bobbing」「落地震動」「死亡時淡出」「被擊退時姿態變化」這些，本質上應該屬於一套通用的 presentation/effect system，而不是每個怪物重新實作。

例如 simulation 只產生：

```text
DamageEvent {
    entity: dragon,
    amount: 20,
    direction: ...
}
```

然後 presentation layer 自動響應：

```text
DamageEvent
    ├─ HitReactionSystem
    │    └─ skeleton / model recoil
    │
    ├─ DamageTintSystem
    │    └─ material flash
    │
    ├─ CameraFeedbackSystem
    │
    └─ Particle / Sound System
```

插件甚至不需要知道「閃紅是怎麼實現的」。

它最多聲明：

```yaml
damage_reaction:
  profile: heavy_creature
```

或者根本不寫，直接繼承：

```text
Entity
  ↓
LivingEntity
  ↓
HostileCreature
```

的預設表現。

移動也一樣。插件提供的可能只是：

```text
movement:
  type: quadruped
```

或者：

```text
animation:
  locomotion: dragon_locomotion
```

引擎根據實際速度：

```text
velocity
   ↓
Locomotion Presentation
   ├─ animation playback speed
   ├─ stride
   ├─ body bob
   ├─ leaning
   └─ footstep events
```

這樣就有一個非常好的分層：

```text
Game Logic
「怪物正在以 4.2 m/s 移動」
            ↓
Animation
「walk animation 播放到這裡」
            ↓
Presentation
「身體上下 2cm、略微前傾」
            ↓
Renderer
「這是最終 skeleton pose」
            ↓
GPU
```

renderer 本身甚至不應該知道什麼叫「受到攻擊」。

Renderer 最好只收到：

```text
transform
skeleton pose
material parameters
mesh
```

「受到攻擊 → 晃一下」其實屬於 renderer 上面一層的 **presentation system**。

這個區分挺重要，否則最後 Renderer 很容易慢慢長成：

```cpp
if (entity.isDamaged()) ...
if (entity.isWalking()) ...
if (entity.isBurning()) ...
if (entity.isFrozen()) ...
```

那就又把遊戲內容塞回 renderer core 了。

比較乾淨的是把最終 pose/material 看成多個 effect 疊加的結果：

```text
Base Animation
    walk
      +
Hit Reaction
      +
Movement Bob
      +
Look Direction
      +
Recoil
      +
Status Effect
      ↓
Final Pose
```

甚至可以有一個通用的 modifier stack：

```text
Pose
├── LocomotionModifier
├── LookAtModifier
├── HitReactionModifier
├── KnockbackModifier
└── ProceduralMotionModifier

Material
├── DamageFlash
├── Burning
├── Frozen
└── Poisoned
```

於是第三方作者做一個新 Boss，大部分情況只需要給你：

```text
模型
骨骼
動畫
碰撞體
AI
攻擊方式
移動機制
```

玩家打它一下，它自然就：

```text
受擊
→ Hit event
→ 通用受擊 presentation
→ 模型 recoil
→ damage material effect
```

插件作者不寫一行渲染程式碼。

更進一步，我甚至會讓「受擊效果」本身也是 module，但是是遊戲級別的 module，而不是怪物級別的 module：

```text
vanilla.presentation.hit-reaction
vanilla.presentation.locomotion
vanilla.presentation.damage-flash
```

這樣整合包作者可以全局替換，例如裝：

```text
soulslike-hit-reaction
```

之後所有符合條件的實體--包括 Vanilla 和第三方怪物--自動換成更重的受擊停頓、後仰與 stagger 效果。

這就非常符合前面那個 declarative-module 思路：

> 插件不是每個東西都自己完成全部功能，而是把自己的 entity 接入一組可組合的通用機制。

結果會是很有意思的：**你新增一種怪物，只需要定義它與通用機制不同的部分。**

這也是這種架構真正能讓 Mod 生態變強的地方，而不只是「把原本寫在 core 裡的程式碼搬進 plugin」。


#### You:
「受擊效果」本身也是 module yes, 所以核心在於 模型 的規範是什麼對不對，我們 需要一個開源的，大家都認可，可以轉換 的規範，比如 blender 的模型？一個例子啊

#### ChatGPT:
對，這裡其實就碰到整個架構最關鍵的一層了：**不是「插件怎麼畫模型」，而是大家對「一個可被遊戲理解的模型」達成什麼共同規範。**

如果你問我現在最適合當這個底層交換格式的是什麼，我會首先看 **glTF 2.0 / GLB**，而不是直接把 Blender 的 `.blend` 當遊戲規範。glTF 本來就是 Khronos 為 runtime 3D asset delivery 設計的開放標準，能描述 mesh、PBR material、texture、node hierarchy、skin/joints、skeletal animation、morph targets 等；Blender 本身也直接內建 glTF 2.0 import/export。([The Khronos Group](https://www.khronos.org/news/press/khronos-releases-gltf-2.0-specification?utm_source=chatgpt.com), [GitHub](https://github.com/KhronosGroup/glTF/blob/main/specification/2.0/Specification.adoc?utm_source=chatgpt.com), [GitHub](https://github.com/khronosgroup/gltf-blender-io?utm_source=chatgpt.com))

所以我會把關係理解成：

```text
Blender / Maya / 其他 DCC
       │
       │ authoring
       ↓
     glTF / GLB
       │
       │ interchange
       ↓
Game Asset Compiler
       │
       ↓
Engine Runtime Asset
```

而不是：

```text
Game Engine
   ↓
直接讀 .blend
```

`.blend` 比較像「Blender project/source file」；glTF 才比較像 PNG/JPEG 那種「大家約定好的交換語言」。

但你真正要解決的問題，其實還比 glTF 多一層。

因為 glTF 可以告訴你：

```text
這裡有 37 根骨骼
bone_0
bone_1
bone_2
...
```

卻不一定知道：

```text
哪一根是頭？
哪一根是左手？
哪一根是右腿？
哪裡是眼睛？
哪裡可以握武器？
哪裡可以產生受擊粒子？
```

而你剛才說的：

> 模型定義好後，受擊晃動、移動抖動等效果就可以由通用 module 自動處理

真正依賴的就是這一層 **semantic model schema**。

所以我反而會把整套規範拆成兩層。

第一層直接採用 glTF：

```text
Geometry Standard
─────────────────
glTF 2.0

Mesh
Material
Texture
Node
Skin
Joint
Animation
Morph Target
```

第二層才是你們自己的 Game Model Schema：

```text
Game Model Semantics
────────────────────
Skeleton semantics
Sockets
Hit zones
Look target
Ground contact
Locomotion semantics
Physics hints
Presentation capabilities
```

例如一隻龍仍然是一個普通：

```text
dragon.glb
```

但旁邊或者 glTF extension 裡存在：

```yaml
model:
  skeleton:
    root: Root
    head: Head
    chest: Chest
    left_wing: Wing.L
    right_wing: Wing.R

  sockets:
    mouth: MouthSocket
    weapon: Hand.R
    effect_origin: Chest

  contacts:
    - Foot.FL
    - Foot.FR
    - Foot.BL
    - Foot.BR

  hit_zones:
    head:
      node: Head
      multiplier: 2.0

  capabilities:
    locomotion: quadruped
    look_at: true
    hit_reaction: true
```

於是受擊插件根本不用問：

> 「Dragon plugin，你受到攻擊的動畫怎麼做？」

它直接看：

```text
capabilities.hit_reaction = true
chest = Chest
head = Head
```

然後：

```text
DamageEvent
     ↓
HitReactionModule
     ↓
apply procedural rotation / translation
     ↓
Chest / Head / Root
```

這才是你真正想要的。

而且這個 schema 最好不要建立在「骨骼名字必須叫 Head」這種脆弱規則上。

不應該是：

```text
bone.name == "Head"
```

因為 Blender 作者可能叫：

```text
head
Head
頭
mixamorig:Head
Bip01_Head
DragonSkull
```

應該是語義 mapping：

```text
semantic.head → node 17
semantic.root → node 0
semantic.left_hand → node 34
```

也就是：

> **名字是作者的；semantic role 是引擎的。**

這點其實已經有現成的標準可以參考：**VRM**。

VRM 是建立在 glTF 上的 humanoid model 規範，它做的正是這件事：規定 humanoid 的骨骼角色與基準姿勢，使不同模型之間可以共享動作。VRM Animation 甚至可以讓同一份 humanoid animation 被不同 VRM 模型 retarget 使用。([VRM](https://vrm.dev/en/vrm/vrm_features/?utm_source=chatgpt.com), [VRM](https://vrm.dev/en/vrma/?utm_source=chatgpt.com))

換句話說：

```text
glTF
= 這裡有骨骼

VRM
= 這根骨骼是 hips
  這根是 head
  這根是 leftUpperArm
```

這個思想跟你要做的東西高度一致。

只是 VRM 的 domain 是：

```text
Humanoid Avatar
```

而你要的是：

```text
General Game Entity
```

所以你完全可以想像一個：

```text
GameModel 1.0
based on glTF 2.0
```

其中存在：

```text
humanoid
quadruped
flying
serpentine
vehicle
static
custom
```

但我甚至不建議讓這些類型過於死板。

最好採用 capability-based schema。

例如：

```yaml
capabilities:
  locomotion:
    ground_contact:
      - foot_front_left
      - foot_front_right
      - foot_back_left
      - foot_back_right

  gaze:
    origin: head
    forward_axis: +Z

  hit_reaction:
    root: chest

  equipment:
    right_hand: hand_right

  projectile_origin:
    mouth: mouth_socket
```

那麼：

```text
人
四足獸
八足蜘蛛
六翼龍
機械人
```

都可以使用同一套機制。

---

更有意思的是，動畫也不一定要隨模型綁死。

例如你的模型包：

```text
dragon.glb

animations:
  idle
  walk
  fly
  attack
```

這是最簡單的情況。

但如果 skeleton semantics 足夠完善，就可以逐漸做到：

```text
Animation Module
       ↓
semantic skeleton
       ↓
retarget
       ↓
Dragon A

同一動畫
       ↓
Dragon B
```

VRM 已經在 humanoid 領域證明了這個方向可行。([VRM](https://vrm.dev/en/vrm/vrm_features/?utm_source=chatgpt.com), [VRM](https://vrm.dev/en/vrm/vrm_development/?utm_source=chatgpt.com))

所以整個 ecosystem 就會變成：

```text
Model Pack
Animation Pack
Physics Pack
Presentation Pack
AI Pack
```

彼此組合。

例如：

```text
my-dragon-model
+
generic-quadruped-locomotion
+
heavy-hit-reaction
+
boss-health-system
+
dragon-ai
```

就得到一隻怪物。

這比：

```text
DragonPlugin.cpp

render()
animate()
physics()
damage()
AI()
```

高一個抽象層次。

---

至於 OpenUSD，它也很值得看，但我不會把它和 glTF 放在同一個角色。

OpenUSD 的強項是大型 DCC pipeline、scene composition、layering、references、協作式場景組裝，而且它本身也有 geometry、skeleton animation、physics 等 schema。([Pixar Animation Studios](https://www.pixar.com/openusd?utm_source=chatgpt.com), [OpenUSD](https://openusd.org/dev/api/_usd_skel__schemas.html?utm_source=chatgpt.com), [OpenUSD](https://openusd.org/release/api/usd_physics_page_front.html?utm_source=chatgpt.com))

所以更像：

```text
Authoring / Production
       ↓
OpenUSD
       ↓
Asset Build
       ↓
glTF / engine binary
```

尤其大型遊戲：

```text
世界
├── terrain
├── castle
├── NPCs
├── lighting
├── variants
└── references
```

USD 非常強。

但如果只是：

> 「Mod 作者給我一個怪物模型，遊戲直接載入。」

我會明確偏向：

**GLB/glTF。**

它本身就以 runtime delivery 為設計目標。([The Khronos Group](https://www.khronos.org/developers/linkto/lets-get-moving-adding-physics-to-gltf?utm_source=chatgpt.com))

所以我目前會把你這個 architecture 定成：

```text
Authoring format
──────────────
.blend
.usd
.ma
whatever

        ↓ exporter

Interchange format
──────────────────
glTF / GLB
+ Game Model Extension

        ↓ asset compiler

Runtime format
──────────────
engine-specific binary
mesh buffers
compressed textures
compiled animations
physics shapes
semantic tables
```

這裡還有一個很重要的點：

**Runtime 不一定真的直接使用 glTF。**

glTF 是插件作者與 engine 之間的 ABI/交換契約。

例如：

```text
dragon.glb
       ↓
game build
       ↓
validate
       ↓
optimize
       ↓
meshlet generation
       ↓
texture compression
       ↓
animation compression
       ↓
collision cooking
       ↓
dragon.assetbin
```

正式跑遊戲時載入的是 `dragon.assetbin`。

這樣「開放格式」和「極致 runtime performance」完全不衝突。

所以如果讓我為我們前面討論的這種 modular game architecture 選一個模型基礎，我會選：

> **glTF 2.0 作為通用 3D 資產 ABI，再定義一套很薄的、開放的 Game Entity Semantic Schema。**

甚至這個 schema 本身最好做成 glTF extension：

```text
EXT_game_entity
```

概念上：

```json
{
  "extensions": {
    "EXT_game_entity": {
      "semantics": {
        "head": 17,
        "root": 0,
        "mouth": 26
      }
    }
  }
}
```

這樣一個 `.glb` 本身就是完整可攜的「遊戲模型」。

Blender 只需要裝 exporter/add-on，就可以直接：

```text
Blender
→ Export Game Entity (.glb)
→ 放到 mods/my-dragon/assets/
→ 引擎識別
```

這時你的插件生態就真的開始有一種很有吸引力的性質了：

**Blender 是創作工具，glTF 是公共語言，Game Model Schema 是遊戲語義，而具體 renderer/physics engine 都只是這份語義的消費者。**

**Sources:**

- [The Khronos Group](https://www.khronos.org/news/press/khronos-releases-gltf-2.0-specification?utm_source=chatgpt.com)
- [VRM](https://vrm.dev/en/vrm/vrm_features/?utm_source=chatgpt.com)
- [Pixar Animation Studios](https://www.pixar.com/openusd?utm_source=chatgpt.com)


#### You:
模型定義好後，受擊晃動、移動抖動等效果就可以由通用 module 自動處理 

我這個其實就是想說 遊戲編輯器，和 blender 的那種功能，就是比如說 我這個模型，龍的肚子是柔軟的，我那樣建模，那麼劍攻擊上去，他就是一個柔軟的局部的受擊反映

#### ChatGPT:
對，我明白你真正想表達的東西了。你說的其實已經不只是「模型格式」，而是：

> **模型在製作階段就帶有物理語義。遊戲運行時，通用物理／受擊 module 根據這些語義，自動產生合理的局部形變。**

也就是 Blender 裡你把龍做好的時候，不只是畫出：

```text
vertices
bones
textures
```

而是順便「塗」出：

```text
這裡是硬鱗片
這裡是柔軟腹部
這裡是厚脂肪
這裡幾乎不能變形
這裡會晃動
這裡連著骨骼
```

然後劍砍到哪裡，遊戲自己算。

這個方向是完全成立的，而且 Blender 現有工作流其實已經非常接近你描述的 authoring model。Blender 的 Soft Body 本身就允許用 vertex group 給不同頂點不同的權重，並控制 stiffness、damping 等效果；也就是「同一張 mesh 不同位置有不同物理性質」這件事本來就是成熟的 DCC 思路。([Blender Documentation](https://docs.blender.org/manual/zh-hans/dev/physics/soft_body/settings/goal.html?utm_source=chatgpt.com), [Blender Documentation](https://docs.blender.org/manual/nb/4.5/modeling/meshes/properties/vertex_groups/introduction.html?utm_source=chatgpt.com))

假設一隻龍長這樣：

```text
            Head
             │
     ┌───────┴───────┐
   Wing              Wing
     \                /
      \    Torso     /
       █████████████
       ░░░ Belly ░░░
       ░░░░░░░░░░░░
          Legs
```

建模者可以直接在 Blender 裡用 weight painting / attributes 把物理參數畫上去：

```text
Dragon mesh

hardness
████████████  back scales = 0.95
██████████    skull       = 0.90
███           belly       = 0.20

stiffness
████████████  back
██            belly

damping
█████         belly

deformation
█             scales
██████████    belly
```

這不是「把 Belly 做成一個特殊插件」。

它仍然是一張普通 mesh，只是 mesh vertex / surface 帶著 metadata。

例如概念上：

```text
vertex 4821:
    stiffness     = 0.18
    damping       = 0.45
    compliance    = 0.80
    thickness     = 0.12
    deformation   = 1.00

vertex 1922:
    stiffness     = 0.96
    damping       = 0.20
    compliance    = 0.05
    deformation   = 0.08
```

於是 runtime 發生：

```text
Sword
  ↓
collision
  ↓
contact point = dragon belly
  ↓
impulse = ...
  ↓
query physical surface properties
  ↓
soft / deformable
  ↓
local deformation
  ↓
spring back + damping
```

插件作者不需要寫：

```cpp
if (swordHitBelly) {
    belly.wobble();
}
```

這正是你想避免的東西。

更理想的效果應該是：

```text
同樣一劍

砍頭骨
→ 幾乎不形變
→ 整個頭受到 impulse

砍背部鱗片
→ 非常小的局部形變

砍腹部
→ 明顯凹陷
→ 周圍組織產生波動
→ 慢慢恢復

砍翅膀薄膜
→ 大範圍柔性形變
```

而這些差異全部來自**模型資產本身的 physical field**。

### 這樣的「模型規範」其實應該有三部分

之前我們談的 glTF 只解決了第一部分：

```text
Geometry
────────
mesh
vertices
normals
UV
skin
bones
animation
materials
```

第二部分則應該是：

```text
Physical Surface
────────────────
mass / density
stiffness
compliance
damping
thickness
friction
restitution
deformability
tear resistance
```

第三部分是 interaction semantics：

```text
Semantic regions
────────────────
armor
skin
flesh
fat
muscle
bone
cloth
wing membrane
...
```

注意第二和第三還應該分開。

比如：

```text
semantic = flesh
```

不應該強行意味著某一組硬度。

某條龍可以是：

```text
belly:
    semantic = flesh
    stiffness = 0.25
```

另一隻岩石龍可能：

```text
belly:
    semantic = flesh
    stiffness = 0.75
```

這樣模型真正描述的是它自己。

---

而 Blender 非常適合做這種 Editor。

例如作者選擇：

```text
Physics Paint → Stiffness
```

然後像 Weight Paint 一樣直接畫：

```text
紅色 = 硬
藍色 = 軟
```

再切換：

```text
Physics Paint
├── Stiffness
├── Damping
├── Compliance
├── Density
├── Thickness
└── Damage Resistance
```

這其實沒有必要自己重新造完整 3D 編輯器。

完全可以提供一個：

```text
Game Engine Blender Add-on
```

於是 Blender 直接成為你這套引擎的 model/physics authoring tool。

Blender 現在的 vertex group 就能給每個 vertex 儲存 0～1 的 weight，而且這些權重本來就被用於 armature deformation 和各類物理效果。([Blender Documentation](https://docs.blender.org/manual/nb/4.5/modeling/meshes/properties/vertex_groups/introduction.html?utm_source=chatgpt.com), [Blender Documentation](https://docs.blender.org/manual/en/3.6/physics/soft_body/forces/exterior.html?utm_source=chatgpt.com))

---

### 但 runtime 不應該直接模擬 Blender Soft Body

這點又很重要。

Blender 是 offline DCC：

```text
漂亮
準確
可以花很多計算
```

遊戲需要：

```text
60 / 120 FPS
幾十隻怪
多人同步
```

所以 Blender 裡的：

```text
Soft Body configuration
```

應該理解為 **authoring semantics**，而不是 runtime implementation。

比如：

```text
Blender

dragon.glb
+
physical fields
       ↓
Asset Compiler
       ↓
runtime representation
```

compiler 可以決定：

```text
腹部
→ 建立 low-resolution simulation cage

翅膀
→ cloth/soft-body mesh

背部
→ rigid/skinned mesh
```

最終真正渲染的是高模：

```text
High-resolution render mesh
     ↑ deformation transfer
Low-resolution physics cage
     ↑
physics solver
```

這是一個非常重要的性能技巧。

你不會拿：

```text
Dragon = 200,000 vertices
```

全部做 soft-body simulation。

而是：

```text
Render mesh
200,000 vertices

         ↑ skin/deform

Simulation mesh
2,000 vertices
```

甚至腹部只：

```text
300 simulation vertices
```

---

而且你描述的「劍砍肚子產生局部柔性受擊」不一定要上完整 FEM。

對遊戲而言，通常可以有分級：

```text
Level 0
Rigid / skeletal response

Level 1
Procedural local deformation

Level 2
Spring / PBD soft-body region

Level 3
Full deformable simulation
```

一個普通小怪受到攻擊可能只是 Level 1：

```text
impact position
     ↓
radial deformation field
     ↓
spring recovery
```

看起來已經會：

```text
凹下去
彈回來
抖兩下
```

Boss 龍腹部則可以是 Level 2，真正維護 low-poly soft-body simulation。

而玩家貼臉觀察時：

```text
LOD0 → soft body
```

遠處：

```text
LOD2 → procedural vertex displacement
```

再遠：

```text
LOD3 → 不模擬
```

這樣才適合大型遊戲。

Jolt 現在本身就已經存在 Soft Body，而且支援將 soft body 部分 skin 到 joints，因此「骨骼動畫 + 柔體局部區域」這種混合模型不是理論上的架構。([Jrouwe](https://jrouwe.github.io/JoltPhysicsDocs/5.5.0/index.html?utm_source=chatgpt.com), [Jrouwe](https://jrouwe.github.io/JoltPhysicsDocs/5.2.0/md__docs__release_notes.html?utm_source=chatgpt.com))

這點對你的龍尤其關鍵。

因為龍不是：

```text
Rigid body
```

也不應該是：

```text
整隻 Soft Body
```

而應該是：

```text
Dragon
│
├── Skeleton animation
│    ├── bones
│    ├── walking
│    ├── attacks
│    └── flight
│
├── Rigid-ish surfaces
│    ├── skull
│    ├── claws
│    └── scales
│
└── Secondary deformation
     ├── belly
     ├── muscles
     ├── neck flesh
     └── wing membrane
```

最後：

```text
Animation pose
      ↓
Soft-body constraints
      ↓
External collision impulses
      ↓
Secondary motion
      ↓
Final mesh
```

這樣龍跑起來，肚子本身也可以有重量感：

```text
腳落地
  ↓
身體加速度
  ↓
腹部因 inertia 滯後
  ↓
晃動
```

玩家一劍：

```text
Sword Collision
      ↓
contact triangle
      ↓
physical field lookup
      ↓
belly stiffness = low
      ↓
inject localized impulse
      ↓
soft body deformation
      ↓
spring + damping
      ↓
自然回彈
```

**根本不需要「受擊動畫」。**

這其實比我們前面說的 `HitReactionModule` 又進了一步。

前面是：

```text
DamageEvent
→ 播放通用 HitReaction
```

現在變成：

```text
Physical interaction
→ 世界自己產生 reaction
```

`HitReactionModule` 只需要負責那些物理模型沒有涵蓋的高階表現，例如 AI stagger、音效、camera shake、表情等。

---

我現在反而會修正前面「glTF + semantic schema」那個結論：

**你真正需要的標準核心不是只有 Game Entity Semantic Schema，而是一套 Game Physical Asset Schema。**

例如：

```text
Game Asset
├── glTF Geometry
│
├── Skeleton Semantics
│
├── Physical Fields
│   ├── stiffness
│   ├── compliance
│   ├── damping
│   ├── density
│   └── thickness
│
├── Simulation Regions
│   ├── rigid
│   ├── deformable
│   ├── cloth
│   └── disabled
│
├── Collision
│
└── Interaction Semantics
```

而目前我沒有看到一個像 glTF 本身那樣、已被整個遊戲業界接受的「通用 deformable game asset schema」。glTF 官方有成熟的 extension registry，但這種完整的 soft-body/局部材料物理並不是 glTF 2.0 core 的通用標準；OpenUSD 的標準 `UsdPhysics` 目前定位也仍然是 common rigid-body physics 的 baseline，而不是你想像中的通用遊戲柔體語義。([Khronos Registry](https://registry.khronos.org/glTF/?utm_source=chatgpt.com), [OpenUSD](https://openusd.org/release/api/usd_physics_page_front.html?utm_source=chatgpt.com))

所以這一塊反而很適合自己定義一個非常薄的開放 extension。

例如：

```text
EXT_game_physical_surface
EXT_game_deformable
```

Blender addon 負責畫，glTF 負責攜帶，引擎 Asset Compiler 負責轉成 Jolt/PBD/其他 solver 的 runtime representation。

這樣最後對 Mod 作者來說，體驗真的可以做到你說的那樣：

> **我只是在 Blender 裡把「龍的肚子比較軟」建模／paint 出來。至於走路時它怎麼晃、被劍砍時怎麼凹、撞牆時怎麼變形，是遊戲引擎的事情。**

我覺得這其實比「每個 Mod 自己做受擊動畫」漂亮非常多，因為它開始接近真正的 **physical authoring**：作者描述物體的性質，而不是預先窮舉物體遇到每一種情況時應該播放哪個動畫。

**Sources:**

- [Blender Documentation](https://docs.blender.org/manual/zh-hans/dev/physics/soft_body/settings/goal.html?utm_source=chatgpt.com)
- [Jrouwe](https://jrouwe.github.io/JoltPhysicsDocs/5.5.0/index.html?utm_source=chatgpt.com)
- [Khronos Registry](https://registry.khronos.org/glTF/?utm_source=chatgpt.com)
