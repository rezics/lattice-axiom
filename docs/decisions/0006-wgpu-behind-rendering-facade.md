---
title: demo 採 wgpu 並以渲染門面隔離
status: accepted
type: decision
updated: 2026-08-19
---

# 決策 0006：demo 採 wgpu 並以渲染門面隔離

## 背景

普通內容需要描述網格、材質與實例，但不應控制 GPU 裝置、同步或繪製順序。demo 又必須儘快得到一個可見、可測且能在無 GPU 環境驗證的垂直切片，因此不能把第二後端、自訂 Render Graph 或通用 shader IR 當成前置條件。

## 決策

1. demo 唯一 GPU 後端採 wgpu；第一階段直接使用 WGSL。
2. 建立 `lattice-render` 門面、`lattice-render-wgpu` GPU 實現與 `lattice-render-headless` 空提交實現。
3. 核心每幀依序執行模擬、渲染擷取、填入 `RenderWorld`，再呼叫 `Renderer::submit`。渲染 crate 不訂閱玩法事件。
4. 門面第一版只暴露相機、區塊網格實例、一般實體實例，以及 `MeshId`、`MaterialId` 等不透明控制代碼；wgpu 型別不得穿過門面。
5. demo 只開放第一級「渲染資料」擴充。自訂 shader、Render Feature、Render Graph 擴充、raw GPU 存取與第二後端延後。
6. 區塊使用 greedy meshing；第一版可採純色調色盤、CPU 視錐剔除與每區塊網格緩衝。實體先以立方體實例呈現。
7. headless 實現必須讓 CI 在不建立 GPU 的情況下驗證擷取資料與呼叫順序。

## 最小門面

概念介面保持窄小，實際 Rust 型別可在原型中調整：

```rust
pub struct MeshId(u32);
pub struct MaterialId(u32);

pub struct RenderWorld {
    pub camera: Camera,
    pub chunk_meshes: Vec<(ChunkPos, MeshId, MaterialId)>,
    pub entities: Vec<(Transform, MeshId, MaterialId)>,
}

pub trait Renderer: Send {
    fn upload_mesh(&mut self, mesh: MeshBytes) -> MeshId;
    fn upload_material(&mut self, material: MaterialDesc) -> MaterialId;
    fn submit(&mut self, world: &RenderWorld);
}
```

## 結果

- demo 有唯一且明確的 GPU 實作路徑，同時保留模擬與內容對具體後端的隔離。
- 門面會限制短期自由度；若真實需求超過它，必須以量測與新契約擴充，不能讓 wgpu 型別旁路滲透。
- 直接使用 WGSL 使第一版簡單，但跨後端 shader IR 與動態 shader 安全性仍是長期問題。

## 被否決的方案

### demo 同時比較兩個渲染後端

它會把原型時間消耗在抽象共同點，卻不能更快證明可玩性或模組熱路徑。

### 讓內容模組直接呼叫 wgpu

它會使批次、資源生命週期、headless 執行與未來後端替換失去集中控制。

### 先建立通用 Render Graph 與 shader IR

目前沒有第二個已驗證使用者；過早設計只會把未知需求固化成基礎設施。

## 驗證

- 以依賴檢查保證核心與內容 crate 無法引用 wgpu。
- 同一個 `RenderWorld` 能同時提交給 wgpu 與 headless 實現。
- 記錄區塊網格化、擷取、剔除與提交時間，確認門面沒有造成不可接受的複製。
