# WebGPU 重构可行性报告(vice-city-web)

> 任务:**验证**可行性,**不**做重构。本报告不改动 `src/` 一行。
> 全部数字来自 `tools/gpu-probe/` 的实测,原始输出在
> `tools/gpu-probe/shots/run.log`,截图在 `tools/gpu-probe/shots/*.png`。
> 复现:`tools/gpu-probe/run.sh`(会自己起无头 Chrome + 静态服务器,跑完即弃)。

**一句话结论:技术上可行,但现在不能开工。**
WebGPU 在这台机器上完全可用(原生路径实测出像素),euv-engine 的
WebGPU API 面也**足够覆盖** render.rs 的全部能力;但 euv-engine
0.28.6 的 WebGPU 实现里有 **3 个会静默失败的缺陷**,其中 2 个正好落在
阴影/离屏渲染这条主干上。**先把引擎缺陷清掉,再谈重构** —— 否则第一阶段
就会卡住,而且失败方式是「画布全黑」而非报错,极难定位。

---

## 1. 最小 demo 的实测结果

同一个三角形(顶点 `[-0.6,-0.5] [0.6,-0.5] [0,0.6]`,片元返回品红
`(1.0, 0.25, 0.65)`),256×256,清屏纯黑,四条路径同页对照。

| 路径 | 三角形 | 非黑像素 | 覆盖率 | mean_rgb | max_ch | 唯一色数 |
|---|---|---|---|---|---|---|
| **原生 WebGPU**(不过 euv) | ✅ | 10818 / 65536 | **16.51%** | [42.09, 10.56, 27.40] | 255 | 2 |
| **原生 WebGPU 二分**(同上,离屏读回) | ✅ | 10818 / 65536 | 16.51% | — | 255 | — |
| **WebGL2** | ✅ | 10921 / 65536 | **16.66%** | [42.07, 10.56, 27.39] | 255 | 5 |
| **euv-engine WebGpuRenderer** | ❌ | **0** / 65536 | **0.00%** | [0, 0, 0] | **0** | 1 |

`mean_rgb` 的比值 42.09 : 10.56 : 27.40 ≈ 1 : 0.25 : 0.65,与片元
常量的比值逐位吻合 —— 读回的不是噪声,是**真的三角形**。

截图(`shots/`):`raw-canvas.png` 与 `bisect-canvas.png` 可见品红三角;
`gpu-canvas.png`(euv 侧)纯黑。

### 1.1 adapter / device

```
vendor=apple  architecture=metal-3
maxTextureDimension2D=8192  maxBindGroups=4
maxStorageBufferBindingSize=134217728  maxComputeWorkgroupSizeX=256
features=['core-features-and-limits']        ← 无 timestamp-query 等可选 feature
swapchain format = bgra8unorm
```

注意 `maxBindGroups=4`(`WebGPU` 规范下限就是 4)—— 迁移时 bind group
不能超过 4 层,现有后处理链(SSAO 要同时绑 gbuffer + sampler + 深度)
绰绰有余。**没有** `timestamp-query` feature,意味着 §1 阶段想用 GPU
时间戳做 pass 级 profile 是**做不到的**,只能靠 wall clock。

### 1.2 帧率 —— 这两个数字**不能用**

| 路径 | fps(2000 帧同步循环) | 冷启动 total_ms |
|---|---|---|
| euv WebGPU | 168017 | 24.2 |
| WebGL2 | 3248333 | 18.4 |

**这两个数字是 CPU 端提交速率,不是 GPU 吞吐量,不要引用。**
循环是同步的,没有 `await queue.onSubmittedWorkDone()`,所以它测的是
「把 2000 帧的 command 编码好扔进队列」要多久 —— GPU 还在后面排队。
(顺带:`fps=168017` 意味着 2000 帧只要 11.9 ms。)

**为什么仍要报它**:能说明**提交开销不是瓶颈**。WebGL2 侧 2000 帧
0.6 ms、WebGPU 侧 11.9 ms,说明 euv 的每次 `begin_render_pass` +
`Reflect` 调用比原生 WebGL2 贵一个数量级 —— 这在 9-pass 的帧里会累积,
是后面阶段需要盯的性能账。**GPU 侧真实帧率仍未测**(需要
`onSubmittedWorkDone` + 多帧同步,或真机有头环境)。

### 1.3 唯一可用的性能参照:上一轮的 +20.9%

`src/render.rs` 现有 WebGL2 阴影优化实测 **366.71 → 444.48 fps**
(commit `25f4a53`)。那是**真机有头**环境下的数字,是当前唯一可信的
性能基线。WebGPU 侧**尚无任何可信帧率**。

---

## 2. 能工作的 headless flag 组合

实测 8 组(`flag-sweep.sh` + `flag_probe.py`)。判据不是
`navigator.gpu` 存在,而是 `requestAdapter()` resolve 出非 null adapter
**且** `requestDevice()` 成功。

| # | 追加 flag | 结果 |
|---|---|---|
| 1 | **无**(只有无头/CDP/不节流) | ✅ `apple / metal-3` |
| 2 | `--enable-gpu` | ✅ |
| 3 | `--use-angle=metal` | ✅ |
| 4 | `--enable-gpu --use-angle=metal` | ✅ |
| 5 | `--enable-gpu --use-angle=metal --enable-features=Vulkan,WebGPU` | ✅ |
| 6 | `--use-angle=swiftshader --enable-unsafe-swiftshader` | ❌ `ADAPTER_NULL` |
| 7 | `--enable-gpu --use-angle=swiftshader --enable-unsafe-swiftshader` | ❌ `ADAPTER_NULL` |
| 8 | `--disable-gpu`(负向对照) | ❌ `ADAPTER_NULL` |

### 逐条说明

- **不要写任何 GPU 相关 flag**(第 1 组)。这是最省事、依赖最少的组合。
  `--enable-gpu` / `--use-angle=metal` 无害但不必要。
- **⚠️ `--use-angle=swiftshader` 会杀掉 WebGPU adapter。**
  这是本次**最反直觉的发现,也是 `docs/webgpu-migration.md` 漏掉的一条**。
  现有 `tools/verify-serial.sh` 给 6 个 WebGL 探针用的正是这组 flag
  (`--use-gl=angle --use-angle=swiftshader --enable-unsafe-swiftshader`),
  实测在该组下 `euv_probe()` 返回 `false`、
  `WebGpuRenderer::init` 报 `WEBGPU_ADAPTER_UNAVAILABLE`,而页面、
  `navigator.gpu`、wasm 全部正常 —— **只有 adapter 一个是空的**。
  → 迁移前必须先改 `verify-serial.sh`,否则 6 路探针会整组失败,
  且失败原因看起来像渲染层坏了。
- **`--disable-gpu` 同样致命**(第 8 组),与 docs 记录一致。
- **`--headless=new`** 必须。旧 headless 没有 GPU 栈。
- **`--disable-background-timer-throttling` / `--disable-renderer-backgrounding`**
  必须:无头下 rAF 被节流到 ~1 fps,不关掉的话任何基于帧的测量都是假的。
- **`navigator.gpu` 只在 secure context 暴露。** CDP `/json/new` 开的
  `about:blank` 是 **opaque origin**,拿不到接口 —— 实测 8 组 flag 全部
  报 `NO_NAVIGATOR_GPU`,是 origin 问题不是 flag 问题。必须导航到
  `http://127.0.0.1/...`(W3C 规定 localhost 是 potentially
  trustworthy,不需要 HTTPS)。这一条踩过,8 组假阴性。

---

## 3. euv-engine WebGPU 能力覆盖表

依据:euv-engine **0.28.6** 源码
(`~/.cargo/registry/src/*/euv-engine-0.28.6/src/renderer/webgpu/*`,
共 5692 行,`pub fn` 67 个),不是文档。

| render.rs 现有能力 | euv WebGPU 是否有等价物 | 依据 |
|---|---|---|
| **阴影贴图** | **部分支持** | `create_texture` + `GpuTextureFormat::Depth32Float` 可建深度纹理;`DepthStencilAttachment` 可作 attachment;`SamplerDescriptor.compare` 支持 `CompareFunction`,`BindGroupLayoutEntry::sampler_comparison` 可建比较采样器。**但 euv 没有任何阴影相关代码** —— `grep -ril shadow src/` 只命中 `lighting/fn.rs` 的 `soft_shadow_factor`(CPU 射线求交,与 GPU 无关)和一条注释。PCF 循环要自己写进 WGSL。 |
| **多 pass**(shadow → gbuffer → main → 后处理) | **支持** | `create_command_encoder` / `begin_render_pass_full(&encoder, &ColorAttachment, Option<&DepthStencilAttachment>)` / `begin_render_pass_to_texture` / `end_render_pass` / `finish_command_encoder` / `submit`。一个 encoder 内可开多个 pass。 |
| **instanced 批量绘制** | **支持** | `VertexBufferLayout { array_stride, step_mode: VertexStepMode::Instance, attributes }` 精确对应 `vertexAttribDivisor(loc, 1)`;`DrawIndexedArgs { index_count, instance_count, first_index, base_vertex, first_instance }` 覆盖 `drawElementsInstanced`。`FLOATS_PER_INSTANCE=20` / stride 80 字节的布局可直接搬,只改 `@location` 绑定。 |
| **G-buffer** | **支持** | `FragmentState.targets: Vec<ColorTargetState>` 是数组,多附件(MRT)天然可表达。注意:**现有 render.rs 的 gbuffer 其实只有单附件**(`GBUFFER_FRAGMENT` 只有一个 `out vec4`,深度存在 alpha 通道里,格式 RGBA8),不是 docs 里写的「normal + 独立 R32F 深度」。 |
| **雾 / ambient / emissive** | **支持** | 都是 uniform + varying,WGSL 逐行转写。`create_uniform_buffer` / `update_uniform_buffer` / `create_uniform_bind_group` 覆盖 per-frame uniform。 |
| **bloom** | **支持** | `create_texture` + `create_sampler` + 全屏 pass 即可。euv 无 mip-chain bloom,但现有实现也只是 3 个半分辨率 pass,不需要。 |
| **SSAO / SSR** | **支持(有坑)** | 都能用 fragment pass 做;`create_storage_texture` + `create_compute_pipeline` + `begin_compute_pass` + `dispatch_with_bind_group` 也支持改 compute。**坑**:现有实现里 AO/SSR 的输入是**上一帧**的纹理(WebGL 允许免费读上一帧),WebGPU 必须显式双缓冲,否则会读到正在写的 attachment。 |
| **自适应画质降级** | **部分支持** | `QualityTier` 是纯 CPU 枚举,跳过 pass 在 WebGPU 侧也可行(但仍需开一个 pass 清成中性色)。**注意现有实现根本没有分辨率缩放**,只有 skip pass,所以不需要 euv 支持 resize。`resize` / `sync_to_current_canvas` 提供画布尺寸变化。 |
| **`?hide=` 调试开关** | **不适用** | `?hide=` 在 `src/game.rs:2914` 解析,`render.rs:4368` **每帧**调 `hidden_batches()`(每帧一次 DOM 读 + `Vec` 分配),然后在 4 个批次循环里 `hidden.contains(&index)`。这与后端无关 —— 迁移时**不该照搬**:应该在 game 层缓存一次,或让调用方传已过滤的绘制列表。 |
| **像素读回(验证用)** | **有 bug** | `copy_texture_to_buffer` 静默 no-op(见 §5 缺陷 1),且 `create_offline_render_target` 返回 `undefined`(缺陷 2)。 |
| **device lost 处理** | **支持** | `on_device_lost(callback)` + `get_device_lost()`。 |

---

## 4. 发现的 euv-engine 0.28.6 缺陷(**全部实测,不是推断**)

这三条是本次验证**最实质的产出**。它们不是「API 缺失」,是「API 存在
但会静默失败」—— 后者比前者危险得多。

### 缺陷 1:`copy_texture_to_buffer` 是**静默 no-op**(影响所有离屏读回)

`impl.rs:2101` 走的是 `self.get_command_encoder()`,但
`create_command_encoder`(`impl.rs:579`)**只返回**新 encoder,
**从不把它写回** `self.command_encoder` 字段 —— 该字段只在 `init`
里被设成 `None`(`impl.rs:319`),之后**全 crate 无任何赋值**。

→ 不手动 `set_command_encoder(Some(enc))` 的话,`copyTextureToBuffer`
根本不会被调用。`read_buffer` 随后读到一个全 0 的 buffer,
`max_channel = 0`,看起来与「三角形没画出来」**完全一样**。
0.28.9 同样存在(已核对)。

### 缺陷 2:`create_offline_render_target` 返回 `undefined`

`impl.rs:2143` 把 `usage` 写成**字符串**
`"RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC"`,而 WebGPU 规范
要求 `GPUTextureDescriptor.usage` 是**数值位掩码**。`createTexture`
抛 TypeError,`Reflect::call1(...).unwrap_or(UNDEFINED)` 把它吞成
`undefined`。同文件的 `create_texture`(`impl.rs:2572`)用
`JsValue::from_f64(usage)` 是对的 —— **两条建纹理的路,一条通一条不通**。
探针里同时调两个并记录结果,报告 `errors` 里那条 note 就是它。

### 缺陷 3:任何 validation error 都被吞成 `undefined`,无处可查

`create_render_pipeline_full` 等全部走 `Reflect::call1(...).unwrap_or(JsValue::UNDEFINED)`。
validation error 只进浏览器 console,wasm 侧拿不到。
`push_error_scope` / `pop_error_sync` / `take_last_error` 存在,
但**必须由调用方主动配对**,且 `pop_error_scope` 走
`spawn_local` 微任务,同步流程里读不到 —— 探针实测
`take_last_error()` 返回 `None`,即使刚发生了 format 不匹配。

> 探针自己踩过一次这个坑:pipelin 颜色 target 写了 `Rgba8Unorm`,
> 而交换链是 `Bgra8Unorm`,`createRenderPipeline` validation 失败 →
> `pipeline` 是 `undefined` → 画布全黑 + 读回全 0。
> **症状与「WebGPU 在这台机器上不能用」一模一样。**
> 这也是为什么必须要有 §1 的原生对照组。

---

## 5. 分阶段方案

**前置(阶段 0,必须先做,不属于重构)**:清掉 §4 的三个引擎缺陷。
在 euv 侧改或先向上游报 issue;在此之前任何 WebGPU 阶段的验收都不可信。
同时把 `tools/verify-serial.sh` 的 `--use-angle=swiftshader` 拆成
「WebGL 探针用 / WebGPU 探针不用」两套 flag。

每阶段的验收标准都必须是**可测量的数字或布尔量**,不接受「看起来对」。

### 阶段 0:引擎缺陷 + flag 修正(不改渲染)
- **目标**:让 `verify-serial.sh` 全部 6 路仍通过,同时新增 WebGPU 环境探针通过。
- **文件**:`tools/verify-serial.sh`(改 flag,约 5 行);euv 侧
  `impl.rs` 3 处(或上游 issue)。**`src/` 零改动。**
- **验收**:
  - `tools/verify-serial.sh` 仍 6/6 通过(现有基线);
  - `tools/gpu-probe/run.sh` 的 `raw (control)` 与 `webgl` 两行
    `triangle_drawn=true` 且覆盖率落在 **16.0%~17.5%** 区间;
  - `webgpu` 一行也变成 `triangle_drawn=true`,覆盖率与 raw 的偏差 **< 1 个百分点**。
- **风险**:euv 是外部 crate,改不动 → 上游 PR 周期不可控。
  **回退**:阶段 0 不动 `src/`,零风险;若上游不修,在本项目
  `src/render/webgpu/` 里**自己实现那三个调用**(用裸 `web_sys` +
  `Reflect`,或给引擎提 patch)。这会让阶段 1 变复杂但不致命。

### 阶段 1:最小可玩(**不是**全量重写)
- **目标**:游戏在 WebGPU 下能跑起来,画面是**纯色 + 地形**,无阴影无后处理。
- **文件**:新增 `src/render/webgpu/{mod,const,pass}.rs`(约 600-900 行);
  `src/render.rs` 暂不动(保留 `SoftwareRenderer` 做对照)。**不删任何 WebGL 代码。**
- **内容**:单 pass、单个 WGSL 顶点/片元着色器、instanced 绘制地形网格、
  复用现有 `mesh.rs` 的顶点/索引数据。
- **验收**:
  - 玩家能走动(`p1.py` / `d_walk.py` 全过);
  - 截图非空:canvas 非黑像素占比 **> 90%**(纯色地面,不含天空);
  - `cargo test` **135 passed / 0 failed** 不变;
  - `FLOATS_PER_VERTEX=9`(`mesh.rs:340`)/ `FLOATS_PER_INSTANCE=20`(`render.rs:52`)
    字节布局**未变**(`git diff src/mesh.rs` 为空);
  - 帧率 ≥ 现有 WebGL2 基线 **444 fps** 的 80%(即 **≥ 356 fps**),
    在**有头真机**下测(无头测不出真实 GPU 帧率,见 §1.2)。
- **风险**:`WebGpuRenderer::init` 是 **async**(adapter/device 是 Promise),
  必须把渲染器初始化从同步 `boot()` 里挪出来 —— 这是本阶段唯一
  有架构影响的一步,会波及 `src/game.rs` 的启动流程。
  **回退**:阶段 1 保留 `WebGlRenderer` 不删,用 URL 参数
  `?backend=webgl2` 切回。**这是本阶段必须保留的逃生门。**

### 阶段 2:阴影管线(**最高风险,建议单独一期**)
- **目标**:shadow pass + PCF 采样,画面与 WebGL2 版本对齐。
- **文件**:`src/render/webgpu/shadow.rs` + 2 个 WGSL 着色器(约 400-600 行)。
- **关键设计**:
  - 深度纹理用 `GpuTextureFormat::Depth32Float`(现有 WebGL2 用的是
    `DEPTH_COMPONENT24` —— 见 §6 的更正),采样用
    `BindGroupLayoutEntry::sampler_comparison` + `CompareFunction`;
  - **手写 9 tap 3×3 PCF,带 slope-scaled bias**,逐行对照
    `render.rs:1765-1830`;euv 无现成实现可抄;
  - 移植 `25f4a53` 的 shadow frustum 剔除(`instance_affects_shadow`,
    `render.rs:157-185`)—— **这段纯 CPU 数学,与后端无关,直接复用,
    不要重写**。上一轮 +20.9% 帧率优化就在这里,重写等于白做。
- **验收**:
  - 同一相机位置、同一帧序号下,WebGPU 与 WebGL2 截图的
    **阴影区域逐像素差异**:平均色差 **< 3/255**,最大色差 **< 12/255**
    (抗锯齿边缘容差);
  - 阴影 pass 的 GPU 时间(需有头 + `performance.now` 前后包夹)
    ≤ 现有 WebGL2 阴影 pass 的 **1.2×**;
  - `?hide=<批次号>` 在两个后端下**隐藏同一批批次**(批次索引语义一致)。
- **风险**:深度格式从 `DEPTH_COMPONENT24` 迁到 `depth32float` 会改变
  深度精度,bias 需要重新调 —— 可能出现 shadow acne 或 peter-panning。
  **回退**:bias 常量单独放 `src/const.rs`,可即时调;阶段 1 的
  `?backend=webgl2` 逃生门继续有效。

### 阶段 3:后处理链重建
- **目标**:gbuffer → SSAO/AO blur → SSR → bloom → composite。
- **文件**:`src/render/webgpu/post.rs`(约 800-1200 行)+ 7 个 WGSL 着色器。
- **关键设计**:
  - AO/SSR **必须显式双缓冲**:现有 WebGL2 实现靠「读上一帧的纹理」
    拿到一帧延迟,WebGPU 里读正在写的 attachment 是 validation error。
  - 半分辨率中间纹理按 `BLOOM_SCALE=0.5` / `SSAO_SCALE=0.5` 建;
  - 8 个后处理 pass 全部走 `begin_render_pass_to_texture`。
- **验收**:
  - 合成后截图与 WebGL2 版本逐区域对比:gbuffer 区域色差 **< 3/255**,
    bloom 区域 **< 8/255**(bloom 是模糊,边缘容差更大);
  - SSAO `KERNEL=8`、AO blur `9×9=81` tap 的采样次数与 WebGL2 一致
    (可在 WGSL 里加一个 debug 输出通道计数验证);
  - `QualityTier::Medium/Low` 下 SSAO/SSR 被跳过时,screenshots
    **与 WebGL2 同档位**一致(这是唯一能验证降级逻辑的客观判据)。
- **风险**:最高 —— 8 个 pass + 7 个着色器,任何一处格式/对齐错位都是
  全黑或全白,无错误信息。**回退**:每个 pass 单独开关
  (`?fx=ao,ssr,bloom`),逐个打开;任何一个不 work 就只关那一个。

### 阶段 4:收尾(删 WebGL、删回退)
- **目标**:`grep -c "WebGl2RenderingContext\|webgl" src/render.rs` == 0,
  删除 `?backend=` 逃生门。
- **验收**:上述 grep == 0;`cargo test` 135 全过;clippy 0 新增 warning
  (基线 135);6 路探针全过;截图与阶段 3 完全一致(证明删代码没改行为)。
- **风险**:此时已无回退。**建议**:这一步单独一个 commit,
  且**放在全部验收通过之后**,不在同一次改动里做。

---

## 6. 对既有文档 / 认知的更正

以下是本次实测发现的、`docs/webgpu-migration.md` 与任务背景里
**不准确或缺失**的地方:

1. **`--use-angle=swiftshader` 同样会让 WebGPU adapter 为 null**,
   不只是 `--disable-gpu`。而现有 `verify-serial.sh` 正在用它。
2. **`WET_OVERLAY_FRAGMENT_SHADER` 是死代码** —— 定义了从未编译或引用。
3. **阴影贴图不是 R32F**,是 `DEPTH_COMPONENT24`(`render.rs:2239`)。
   注释里「R32F」的说法是历史遗留(`:1282` / `:1316` / `:1707`)。
4. **G-buffer 不是 MRT**:只有一个 `out vec4`,深度塞在 alpha 通道,
   格式 RGBA8。全仓 `draw_buffers` 只有一处命中,还是阴影 FBO 上的
   `NONE` 调用。
- **PCF 是手写 9 tap,没有用硬件比较采样器** —— `render.rs:1791-1798`
  是双层 `for (x=-1..1) / for (y=-1..1)` 循环,读普通 `sampler2D`
  的 `.r` 再手动 `compare_depth > stored`。
  `render.rs:1282` 的注释声称「用 `sampler2DShadow` + 硬件比较采样,
  PCF 由驱动在采样时完成(免费)」—— **这条注释是错的**,
  全仓 `TEXTURE_COMPARE_MODE` / `sampler2DShadow` 只在注释里出现,
  没有任何实际调用。
  迁移含义:WebGPU 侧**可以**改用 `sampler_comparison` 让硬件做 PCF
  (9 tap → 1 tap),但那会改变阴影外观,需重调 `u_shadow_params.y`;
  或保持手写 3×3 以保证与 WebGL2 逐像素对齐。
6. **`?hide=` 在 `render.rs:4368` 每帧重读 `location.search`**,
   每帧一次 DOM 读 + `Vec` 分配,还要 4 处 `hidden.contains()` 线性扫。
   这是**与后端无关的性能债**,迁移时顺手根治,别照搬。
7. **`euv_engine::renderer` 仍是 private module**,类型在 crate 根 ——
   这条 docs 记对了,本次复核无误。

---

## 7. 明确结论

**WebGPU 重构可行吗?**
**可行。** 但「可行」的含义是:*技术路径成立、API 面足够、硬件就绪*,
**不是**「可以马上开工」。

**最大风险是什么?**
不是 render.rs 的 5287 行,不是阴影贴图,也不是测试要重跑。
**是 euv-engine 0.28.6 的 WebGPU 实现质量** —— 具体是 §4 的三个缺陷:
**validation error 被静默吞成 `undefined`,且错误信息无法从 wasm 侧取回**。
在一个「错了不报错」的底层上重建 9 个 pass、13 个着色器的渲染管线,
意味着**每次故障的排查成本都是「开 DevTools 猜」**,而这类故障的
表现形式(全黑画面)与「WebGPU 不支持」「flag 配错」完全同形 ——
本次探针自己就误判了两次。

**建议的投入决策:**
1. **先做阶段 0**(引擎缺陷 + flag),不碰 `src/`,零风险。
2. 阶段 0 通过后再决定是否投入阶段 1。**如果阶段 0 拿不到 euv 上游
   的修复**,要在本项目里自己补那三处调用 —— 这会让阶段 1 的工作量
   从「重写渲染层」变成「重写渲染层 + 绕过引擎缺陷」,风险显著上升,
   此时应当重新评估。
3. **不要**按 docs 里原计划的 4 阶段直接开工。原计划假设
   `euv-engine WebGPU 栈可从本项目调用` = `可依赖`。本次实测表明
   两者不是一回事。

**尚无结论、必须在有头真机补测的:**
- WebGPU 在真实场景下的帧率(无头测不出,见 §1.2);
- 8 pass + 13 着色器在 Metal 上的实际耗时分布;
- 内存占用对比(9 个离屏纹理在 WebGPU 下的对齐/驻留行为)。

---

## 附:探针复现

```bash
# 主探针(euv vs WebGL2 vs 原生 WebGPU,四路同页对照)
tools/gpu-probe/run.sh

# flag 组合扫描(8 组,约 3 分钟)
tools/gpu-probe/flag-sweep.sh

# 重新构建 wasm
cd tools/gpu-probe
CARGO_TARGET_DIR=/Users/sqs/Library/Caches/cargo-target/vcweb-probe \
  cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir pkg --out-name gpu_probe \
  /Users/sqs/Library/Caches/cargo-target/vcweb-probe/wasm32-unknown-unknown/release/gpu_probe.wasm
```

探针 crate 用 `euv-engine = "=0.28.6"`(**必须带 `=`**):cargo 的 caret
语义会把 `"0.28.6"` 解析到 0.28.9,那就成在测另一份引擎代码了。
本次已核对 `Cargo.lock` 与主仓库一致。
