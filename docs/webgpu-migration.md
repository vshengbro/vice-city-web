# WebGPU 迁移方案(WebGL2 → euv-engine WebGPU)

> 目标:把渲染后端从手写 WebGL2 整体迁到 euv-engine 的 WebGPU,
> **不留任何 WebGL 代码,不留回退路径**。无 WebGPU 的设备显示明确的不支持提示。

## 0. 已实测确认的前提(不是猜测,见 commit `7d423d6` / `4bfcde4`)

| 前提 | 实测结果 | 怎么验的 |
|---|---|---|
| euv-engine WebGPU 栈可从本项目调用 | ✅ `cargo check` 通过 | `use euv_engine::WebGpuRenderer` |
| **类型是扁平重导出在 crate 根** | ⚠️ 不是 `euv_engine::renderer::X` | E0603 private module;`lib.rs` 有 `pub use {...renderer::*...}` |
| `WebGpuRenderer::is_available()` 按设备返回 | ✅ 浏览器里返回 `true`,非恒 true | 挂 `window.__vcwWebgpu` 由 `d_webgpu.py` 读 |
| 无头 Chrome WebGPU 可用 | ✅ `vendor=apple / metal-3`,device=true | `tools/sweep/d_webgpu.py` |
| **探针不能带 `--disable-gpu`** | ❌ 带了就 `requestAdapter()` 返回 null | 4 组 flag 对照实测 |
| 顶点/实例二进制布局 | `FLOATS_PER_VERTEX` / `FLOATS_PER_INSTANCE` 不变 | 迁 WGSL 时只改 `@location` 绑定,不动字节布局 |

## 1. 工作量实测

```
src/render.rs                 4971 行
GLSL 着色器                     14 块 / 603 行
  FRAGMENT_SHADER             195   SSAO_FRAGMENT_SHADER   71
  SSR_FRAGMENT_SHADER         87   VERTEX_SHADER          55
  COMPOSITE_FRAGMENT_SHADER   36   AO_BLUR_FRAGMENT_SHADER 31
  SHADOW_VERTEX_SHADER        20   BLOOM_BLUR_FRAGMENT    21
  BLOOM_EXTRACT_FRAGMENT      19   GLOW_FRAGMENT_SHADER   19
  GBUFFER_FRAGMENT            17   WET_OVERLAY_FRAGMENT   14
  FULLSCREEN_VERTEX           10   SHADOW_FRAGMENT         8
手写 GL 封装 `pub fn`          115 个
WebGL 调用点                   192 处
后处理 pass                    8 段
```

**这是一次重写,不是局部调整。** 保守估计 5-7 个语义 commit。

## 2. 建议执行顺序

### 阶段 1:最小可跑通路(先拿到第一张 WebGPU 截图)
1. 新建 `src/render/webgpu/const.rs` —— 全部 WGSL 源码 + pass 顺序 + 纹理格式常量(§1.3c)
2. `WebGpuRenderer::is_available()` 同步探测 → false 则渲染「不支持 WebGPU」提示并 return
3. `Engine::webgpu_renderer(&RenderConfig).await` **异步**初始化;主循环改成拿到 renderer 再启动
4. 单 pass + 单着色器,只画地形 → 截图
5. 加角色 / 车辆批次

> 注意:渲染器初始化从同步 `boot()` 里挪出来,是本阶段唯一有架构影响的一步。

### 阶段 2:13 个着色器逐个迁 WGSL
按依赖顺序:VERTEX → GBUFFER → SHADOW(x2) → COMPOSITE → 其余后期。
每迁一个跑一次截图对比,**不能只跑 cargo check**。

WGSL 与 GLSL 的实质差异(每条都会改变画面,必须逐个验):
- `gl_FragColor` → `struct FragmentOutput { @location(0) color: vec4f }`
- 隐式 `attribute/varying` → `@location(n)` 显式绑定
- 纹理/采样器**必须**在 `@group(n) @binding(m)` 显式声明
- 深度比较 → `texture_depth_2d` + `sampler_comparison`
- instancing → `builtin(instance_index)` + `builtin(vertex_index)`
- 深度纹理格式 `DEPTH_COMPONENT24` → `depth24plus`

### 阶段 3:后处理链重建
```
shadow pass      → depth32float,  sampler_comparison
GBuffer pass     → 颜色 attachments + depth24plus
SSAO             → 改 compute:create_compute_pipeline + begin_compute_pass + dispatch
AO blur          → 可 compute 或 fragment pass
SSR              → 依赖 GBuffer depth + 上一帧 color
bloom extract/blur
composite        → → swap chain(bgra8unorm)
wet overlay      → 最后一段
```
GL 的 `bindBuffer`/`useProgram`/`activeTexture` 隐式全局状态,全部要显式建成
`BindGroupLayout` / `RenderPipelineDescriptor` / `RenderPassDescriptor`。

### 阶段 4:顺手根治 mesh 表错位(问题 2/7)
WebGPU 侧是**重新建表**,所以 `upload_mesh` 的 push 语义问题在这里根治:
批次写进 `mesh_index` 的必须是**上传时返回的 index**,不是枚举序号。
问题 10(武器捡到要重绑手上模型)依赖这个,一起验。

## 3. 硬约束自查表

- [ ] `grep -c "WebGl2RenderingContext\|webgl\|#version 300 es" src/render.rs` == 0(如实报告)
- [ ] 保留 `src/mesh.rs` 的资产展开与 `f32_slice_to_bytes` 复用
- [ ] 新增字符串/数值常量进 `src/const.rs`(§1.3c)
- [ ] 目录名语义化、文件用关键字名(§1.3d);`render.rs` 现在是 §1.4 违规之一
- [ ] 禁 push / 禁 PR / 禁 `--no-verify` / 禁 `#[ignore]` / 禁 `#[allow]`
- [ ] 禁正则批量编辑 `.rs`(14 块 raw string 会被毁掉)—— 逐个 patch
- [ ] audit 不新增 FAIL(基线 40/45)
- [ ] `cargo test` 不新增失败(基线 78 passed)
- [ ] clippy 0 新增 warning(基线 135)
- [ ] fmt 幂等
- [ ] 6 路探针 p1..p6 全过(现基线 15/15)

## 4. 验证

`tools/sweep/d_webgpu.py` 查环境;`tools/verify-serial.sh` 跑 6 路。
**正确性判据是截图对比,不是 cargo check。**
无头下拿的是 SwiftShader/Metal,画质和帧率不代表真机;有头 Chrome:
`open http://127.0.0.1:8765/index.html`。

## 5. 已知环境事实(实测,别重复踩)

`--disable-gpu` 会让 `requestAdapter()` 返回 null —— 现有两个探针脚本已修。
`--use-angle=swiftshader` 只影响 WebGL,对 WebGPU 无影响,留着无妨。
`euv_engine::renderer` 是私有模块,类型在 crate 根上。
