# euv-engine WebGPU 缺陷核实清单

**核实对象**:`euv-engine` 0.28.6（本项目 `vice-city-web` 实际编译的版本）
**对照版本**:0.28.9
**核实方式**:逐条打开源码确认 + 在 headless Chrome(Chrome for Testing 154,Apple/Metal)实跑复现
**核实日期**:2026-10-07

---

## 0. 先说三个前置事实(它们改变了结论的性质)

### 0.1 0.28.6 与 0.28.9 的源码**逐字节相同**

```
$ diff -r --brief ~/.cargo/registry/src/index.crates.io-*/euv-engine-0.28.6/src \
          ~/Library/Caches/cargo-home/registry/src/rsproxy.cn-*/euv-engine-0.28.9/src
SRC IDENTICAL 0.28.6 == 0.28.9
```

`impl.rs` 的 md5 两边都是 `91896baeb05b3c73365b90f3c3d710c5`。两个版本号之间,
`src/` 下一个字节都没改。

**因此:本清单里所有缺陷都不存在「升到 0.28.9 就能解」这种可能。**
全部是上游未修,需要提 issue/PR。

### 0.1b 未发布的 master HEAD **同样没修**(更强的结论)

不止 0.28.9,`/Users/sqs/code/euv` 的工作树 HEAD(`27a12699`,workspace
version `0.28.5`)——也就是**下一个要发布的版本**——也原样带着这两个 bug:

```
$ grep -rn "command_encoder" /Users/sqs/code/euv/engine/src/renderer/webgpu/
struct.rs:134  pub(crate) command_encoder: Option<JsValue>,   ← 字段
impl.rs:319    command_encoder: None,                          ← 仍是唯一的赋值
impl.rs:579    pub(crate) fn create_command_encoder(&self)     ← 仍是 &self
impl.rs:2101   let encoder = match self.get_command_encoder()  ← 仍会拿到 None

$ grep -n -A2 "WEBGPU_PROPERTY_USAGE" .../impl.rs | grep from_str
2158:  &JsValue::from_str(WEBGPU_OFFSCREEN_TEXTURE_USAGE),       ← 仍是字符串
$ grep -n -A2 "WEBGPU_OFFSCREEN_TEXTURE_USAGE" .../const.rs
170-171: pub(crate) const WEBGPU_OFFSCREEN_TEXTURE_USAGE: &str =
             "RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC";  ← 仍是 &str
```

**结论:不是「选型失误」,也不是「等下个版本」——上游 master 此刻就是坏的。**
这两个 issue 提上去现在就能修。

> 顺带发现:HEAD 里存在 `engine/tests/webgpu_cache/` 目录(webgpu 缓存相关的
> 新测试),说明上游正在动这块。但那些测试不覆盖本清单的 D1/D2 —— 见 §7。

> 注:0.28.9 不在 `~/.cargo/registry` 里,而在 `~/Library/Caches/cargo-home/registry/src/rsproxy.cn-*`
> 下(不同 CARGO_HOME)。调研时如果只看默认 registry 会误判「0.28.9 不存在」。

### 0.2 本项目**根本没有走 WebGPU 渲染路径**

`vice-city-web` 的后端是 WebGL2,`WebGpuRenderer` 一次都没被调用:

```
$ grep -rn "WebGpu\|webgpu\|navigator.gpu\|requestAdapter" src/
(只有 src/camera.rs / src/const.rs / src/game.rs 里作为注释和常量出现的 "WebGL" 字样)
$ grep -rn "copy_texture_to_buffer\|create_offline_render_target\|take_last_error\|create_command_encoder" src/
(空)
```

`src/render.rs:5124` 的 `enum Renderer` 只有 `WebGl` 和 `Software` 两个变体。
WebGPU 只出现在 `tools/` 下的探针脚本里(`d_webgpu.py`、`flag_probe.py`)。

**因此:下面这些缺陷不可能是本项目「全黑画面」的成因。**
如果之前的报告把它们和「WebGPU 不支持」症状并列成同形现象,那是关联错误。

### 0.3 但它们是**真缺陷**,且 D1/D2 是上游真实 API 的静默失效

不管本项目用不用,`copy_texture_to_buffer` 和 `create_offline_render_target`
是 `euv-engine` 公开 API,任何 WebGPU 用户一调就中。这两条要提 PR。

---

## 1. 摘要表

| # | 缺陷 | 严重度 | 影响 | 影响形态 | 0.28.6 | 0.28.9 | 一句话 |
|---|------|--------|------|----------|--------|--------|--------|
| **D1** | `copy_texture_to_buffer` 因 `command_encoder` 字段恒为 `None` 而永不执行 | **高** | 任何 GPU readback(截图/拾取/后处理)拿到空数据 | 🔴 **静默失败** | 存在 | **未修**(master HEAD 也未修) | 函数在 `impl.rs:2103` 直接 `return`,调用方以为拷贝成功了 |
| **D2** | `create_offline_render_target` 把 `usage` 写成字符串 | **高** | 离屏渲染目标创建失败,恒返回 `(UNDEFINED, UNDEFINED)` | 🔴 **静默失败** | 存在 | **未修**(master HEAD 也未修) | 规范要求 `unsigned long` 位掩码,代码传的是 `"RENDER_ATTACHMENT \| TEXTURE_BINDING \| COPY_SRC"` |
| **D3** | ~~validation error 被 `unwrap_or(UNDEFINED)` 吞掉~~ | — | — | — | ❌ **报告有误** | — | 源码是 `cell.take()`,**没有** `unwrap_or`。通道本身实测可用(见 §3) |
| **D4** | D2 的异常被 `unwrap_or` 吞成 `UNDEFINED`,连 console 都不打 | **中** | 上面那两条的报错彻底静默 | 🔴 静默放大 | 存在 | **未修** | `impl.rs:2168` / `2109` 等处 `call*.unwrap_or(UNDEFINED)` 吞掉 `TypeError` |
| **D5** | error scope 只覆盖 `create_bind_group`,其余 GPU 调用无验证 | **中** | 大部分 validation error 直接消失 | 🔴 静默 | 存在 | **未修** | `impl.rs:2822`/`3047` 是仅有的两处 `push_error_scope` |
| **D6** | 20 个 webgpu 测试**零覆盖**上述 API | **中** | 缺陷能一路发到 crates.io | — | 存在 | **未修** | `tests/webgpu/fn.rs` 无一处引用这三个 API |

---

## 2. D1 —— `copy_texture_to_buffer` 静默失效

**严重度:高 · 静默失败 · 上游未修(0.28.9 同样存在)**

### 2.1 源码位置

字段声明,`src/renderer/webgpu/struct.rs:115-125`:

```rust
/// The currently-open `GpuCommandEncoder`, if any.
///
/// WebGPU expects the application to encode all work for a
/// frame (clear, render passes, compute passes, copy ops) into
/// a single command encoder, then call `encoder.finish()` to
/// produce a `GpuCommandBuffer` and submit it to the queue.
/// The encoder is `None` after `submit()` finishes and must
/// be re-acquired via `device.createCommandEncoder()` before
/// the next frame.
#[get(type(clone))]
pub(crate) command_encoder: Option<JsValue>,
```

唯一的赋值,`src/renderer/webgpu/impl.rs:319`(在 `init` 里):

```rust
command_encoder: None,
```

消费点,`src/renderer/webgpu/impl.rs:2101-2104`(在 `copy_texture_to_buffer` 内):

```rust
let encoder: JsValue = match self.get_command_encoder() {
    Some(enc) => enc,
    None => return,        // ← 永远走这里
};
```

产生 encoder 的方法,`src/renderer/webgpu/impl.rs:579-592`:

```rust
pub fn create_command_encoder(&self) -> JsValue {   // ← &self,无法写回字段
    let create_fn: Function = cached_method(
        GpuReceiverClass::Device,
        self.get_device(),
        WEBGPU_METHOD_CREATE_COMMAND_ENCODER,
    )
    .unwrap_or_else(|_| JsValue::UNDEFINED.unchecked_into());
    create_fn
        .call0(self.get_device())
        .unwrap_or(JsValue::UNDEFINED)
}
```

### 2.2 核实过程:字段是否真的永远不变

全 crate grep(只有 8 处 `command_encoder`,**没有一处是写**):

```
$ grep -rn "command_encoder" euv-engine-0.28.6/src/
struct.rs:125:    pub(crate) command_encoder: Option<JsValue>,   ← 声明
impl.rs:319:            command_encoder: None,                    ← 唯一的赋值(init)
impl.rs:579:    pub fn create_command_encoder(&self) -> JsValue   ← 方法,不写字段
impl.rs:1653:   pub fn finish_command_encoder(&self, ...)        ← 方法
impl.rs:2101:        let encoder = match self.get_command_encoder()  ← 读
impl.rs:2895/2900: render_frame 内部自建自用
impl.rs:2925/2931: render_frame_with_bind_group 内部自建自用
```

`create_command_encoder` 的签名是 `&self`,**类型系统上就无法**写回字段。
`get_command_encoder()` 在源码里搜不到定义 —— 它是 `#[derive(Data)]`
(lombok-macros 10.2.3,`Cargo.toml:64`)根据 `#[get(type(clone))]`
生成的 getter,恒返回 init 时那个 `None`。

**结论:`copy_texture_to_buffer` 是一个恒等 no-op。**

### 2.3 为什么用户可见失败(推理链)

1. 用户调 `renderer.copy_texture_to_buffer(&tex, &buf, ...)` 做 readback。
2. 函数体走到 `impl.rs:2101`,`get_command_encoder()` 返回 `None`。
3. `impl.rs:2103` 直接 `return`,**返回值类型是 `()`,不是 `Result`** ——
   调用方在类型层面就无法得知失败。
4. 用户随后 `map_async` 那个 buffer,拿到的是未写入的旧数据(通常是全 0)。
5. 症状:**截图全黑 / 拾取坐标全 0 / 后处理读回纯黑**,但没有任何报错。

注意 `render_frame`(impl.rs:2894)和 `render_frame_with_bind_group`(impl.rs:2918)
**不受影响** —— 它们在本地变量里持有 encoder,只把 encoder 当参数传下去,不碰这个字段。
所以基本渲染是好的,坏的只有 readback 路径 —— 这正是「静默」的来源:
大多数测试跑不出问题。

### 2.4 最小复现

已实测,见 `tools/gpu-probe/defect-repro.html`。核心:

```js
// 模拟 euv 的状态机:renderer.command_encoder 从 init 起就是 null
const renderer = { command_encoder: null };          // impl.rs:125 + :319
function copyTextureToBuffer_euv(...args) {         // impl.rs:2051
  const encoder = renderer.command_encoder;          // impl.rs:2101
  if (encoder === null) return { executed: false };  // impl.rs:2103
  encoder.copyTextureToBuffer(...args);
  return { executed: true };
}
// euv 路径
D1_euv_path = copyTextureToBuffer_euv(...copyArgs);   // → executed: false
// 对照组:同样参数,只是自己持有 encoder
const enc = device.createCommandEncoder();            // impl.rs:579 的返回值
enc.copyTextureToBuffer(...copyArgs);
device.queue.submit([enc.finish()]);                  // → 成功
```

跑法(见 §5)。

**实测输出**(headless Chrome 154,`vendor: "apple"`):

```json
"D1_euv_path":  { "executed": false, "why": "encoder==None → early return (impl.rs:2103)" },
"D1_control":  { "executed": true,  "why": "same call with a live encoder works" }
```

对照组成功,证明纹理/缓冲区本身合法,**是 euv 的状态机坏了**。

### 2.5 建议修法

最小改动:让 `copy_texture_to_buffer` 自建 encoder 并提交,和 `render_frame` 的形状一致。
`impl.rs:2101-2111` 改为:

```rust
let encoder: JsValue = self.create_command_encoder();   // 已是 pub,impl.rs:579
let cmd_fn: Function = Reflect::get(&encoder, &JsValue::from_str(WEBGPU_METHOD_COPY_TEXTURE_TO_BUFFER))
    .unwrap_or(JsValue::UNDEFINED)
    .unchecked_into();
if let Ok(_) = cmd_fn.call1(&encoder, &info) {
    let buffer = self.finish_command_encoder(&encoder);  // impl.rs:1653
    self.submit(&[buffer]);
}
```

更彻底的做法:把 `create_command_encoder` 改成 `&mut self` 并写回字段,
`render_frame` 也改用它,以兑现 `struct.rs:117-123` 那段注释描述的生命周期。
前者是 6 行改动,后者更符合原设计意图。

---

## 3. D3 —— 报告称「validation error 被 `unwrap_or(UNDEFINED)` 吞掉」:❌ **推翻**

**核实结论:报告的机制描述是错的。**

### 3.1 报告称 vs 实际源码

报告称 `take_last_error()` 拿不到错误、被 `unwrap_or(UNDEFINED)` 吞掉。
实际 `src/renderer/webgpu/impl.rs:1957-1966`:

```rust
pub fn take_last_error(&self) -> Option<JsValue> {
    // SAFETY: ...
    let cell: &mut Option<JsValue> = unsafe { &mut *self.get_pending_error().as_ptr() };
    cell.take()          // ← Option 的 take(),没有 unwrap_or
}
```

**没有 `unwrap_or(UNDEFINED)`。** 逻辑上完全正确:
`push_error_scope` → `pop_error_sync` 用 `spawn_local` 把 `popErrorScope()`
的 resolve 值写进共享 cell → `take_last_error` 用 `cell.take()` 排空。
返回值是 `Option<JsValue>`,类型层面就能表达「有/无错误」。

### 3.2 那这条被删掉,是不是就算没问题了?—— 不是

`take_last_error` 本身没问题,但**它只能看到极小一部分错误**。这构成 D4/D5。

### 3.3 error 通道实测:能用,但覆盖面窄

`tools/gpu-probe/errscope-probe.html`,5 个已知会报错的调用各 push/pop 一次:

```json
{
  "vendor": "apple",
  "textureUsageZero":            { "verdict": "CAUGHT",     "scopeCaptured": "The texture usage must not be 0. - While validating [TextureDescriptor]..." },
  "writeToBufferWithoutCopyDst": { "verdict": "CAUGHT",     "scopeCaptured": "[Buffer (unlabeled)] usage (BufferUsage::CopySrc) doesn't include BufferUsage::CopyDst..." },
  "pipelineLayoutMismatch":      { "verdict": "NOTHING" },
  "bufferSizeNotMultipleOf4":    { "verdict": "NOTHING" },
  "descriptorLayerUsageTypeError": { "verdict": "THREW_ONLY", "threwSynchronously": "TypeError: Failed to execute 'createTexture'... 'usage' property ... not of type 'unsigned long'." }
}
```

**关键结论:validation error 通道是通的**(2 个 CAUGHT),不是报告说的「拿不到」。
但 `THREW_ONLY` 那一栏揭穿了 D2 的严重性:

> WebGPU 的 **descriptor 层**错误(类型不对、缺 required 成员)是**同步抛异常**的,
> error scope **抓不到**,只有 `try/catch` 能看见。
> 而 euv 恰恰把这些调用包在 `.unwrap_or(JsValue::UNDEFINED)` 里
> —— 异常连 JS 的 `TypeError` 都不会冒出来,直接被 Rust 侧吃掉。

这就是 D4。

---

## 4. D2 —— `create_offline_render_target` 的 `usage` 写成字符串

**严重度:高 · 静默失败 · 上游未修(0.28.9 同样存在)**

### 4.1 源码位置

`src/renderer/webgpu/impl.rs:2155-2159`:

```rust
let _: Result<bool, JsValue> = Reflect::set(
    &descriptor,
    &JsValue::from_str(WEBGPU_PROPERTY_USAGE),
    &JsValue::from_str(WEBGPU_OFFSCREEN_TEXTURE_USAGE),   // ← 字符串!
);
```

常量定义,`src/renderer/webgpu/const.rs:596-603`:

```rust
/// The `usage` bitmask label used for offscreen render targets created by
/// `create_offline_render_target`.
///
/// It names the three spec-defined `GpuTextureUsage` bits the offscreen
/// baseline needs: render into the texture, sample it in a later pass, and
/// copy out of it for readback.
pub(crate) const WEBGPU_OFFSCREEN_TEXTURE_USAGE: &str =
    "RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC";
```

然后 `impl.rs:2166-2171`:

```rust
let texture: JsValue = create_fn
    .call1(self.get_device(), &descriptor)
    .unwrap_or(JsValue::UNDEFINED);      // ← 吞掉 TypeError(D4)
if texture.is_undefined() {
    return (JsValue::UNDEFINED, JsValue::UNDEFINED);
}
```

### 4.2 同文件里正确的写法(证明这是笔误,不是有意为之)

同一个文件、同一个 `usage` 属性,其它 4 处都传数值:

```
impl.rs:380-382   usage ← texture_usage_bit(TextureUsage::RenderAttachment)     ✅
impl.rs:2268-2270 usage ← f64::from(descriptor.get_usage())                     ✅
impl.rs:2457-2459 usage ← f64::from(usage as f64)                               ✅
impl.rs:2578-2580 usage ← f64::from(descriptor.get_usage())                     ✅
impl.rs:3273-3277 usage ← texture_usage_mask(&[...])                            ✅
impl.rs:2155-2159 usage ← from_str("RENDER_ATTACHMENT | ...")                   ❌ 唯一一处
```

甚至 `fn.rs:99-119` 就有一个现成的位掩码 helper `texture_usage(render_target, copy_src, copy_dst, sampled, storage) -> u32`,
`const.rs:122/304/478` 里也有对应的 `f64` 常量(`16.0 / 1.0 / 4.0`)。**只有这一个调用点没走。**

正确值应为 `16 | 1 | 4 = 21`。

### 4.3 为什么用户可见失败(推理链)

1. 用户调 `create_offline_render_target(w, h, "rgba8unorm")`。
2. `usage` 是字符串,浏览器按 WebIDL 转换 `GPUTextureDescriptor.usage`(类型 `unsigned long`)失败。
3. **同步抛 `TypeError`**(不是 validation error,error scope 抓不到,见 §3.3)。
4. `impl.rs:2168` 的 `.unwrap_or(JsValue::UNDEFINED)` 把异常吃掉 → `texture = UNDEFINED`。
5. `impl.rs:2169` 命中 `is_undefined()` → 返回 `(UNDEFINED, UNDEFINED)`。
6. 文档(`impl.rs:2130-2133`)明确承诺「Either may be `UNDEFINED` on failure」,
   所以调用方**无法区分**「离屏目标创建失败」和「一切正常」。
7. 症状:离屏渲染/后处理拿到 undefined view,渲染 pass 静默不出图 → 全黑。

而且这一步是 D1 的**前置依赖**:readback 流程通常是
`create_offline_render_target` → 渲染进去 → `copy_texture_to_buffer` 读回。
两步都坏,所以 readback 这条路**完全不可用**。

### 4.4 最小复现(已实测)

```js
const WEBGPU_OFFSCREEN_TEXTURE_USAGE = "RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC"; // const.rs:602
device.createTexture({ size: [64,64,1], format: "rgba8unorm",
                       usage: WEBGPU_OFFSCREEN_TEXTURE_USAGE });   // impl.rs:2140-2168 的形状
```

**实测输出**:

```json
"D2_euv_path": { "ok": false,
  "err": "TypeError: Failed to execute 'createTexture' on 'GPUDevice': Failed to read the 'usage' property from 'GPUTextureDescriptor': Value is not of type 'unsigned long'.",
  "errName": "TypeError" },
"D2_control":  { "ok": true, "usage": 21,
  "note": "同样的 size/format,只有 usage 换成数值就成功" }
```

对照组 `usage: 21` 成功,坐实「只有 usage 这一处错」。

### 4.5 建议修法

`impl.rs:2155-2159` 改为数值:

```rust
let _: Result<bool, JsValue> = Reflect::set(
    &descriptor,
    &JsValue::from_str(WEBGPU_PROPERTY_USAGE),
    &JsValue::from_f64(texture_usage(true, true, false, true, false) as f64), // fn.rs:99 → 21.0
);
```

同时把 `const.rs:602` 的 `WEBGPU_OFFSCREEN_TEXTURE_USAGE` 从 `&str` 改成 `f64`,
避免下一个人再写错。顺带把 `impl.rs:2130-2133` 的文档从
「Either may be UNDEFINED on failure」改成「**失败时返回 `(UNDEFINED, UNDEFINED)`,
且不会通过 error scope 报告**」——现在这个措辞让失败不可观测。

---

## 5. D4 —— `unwrap_or(JsValue::UNDEFINED)` 吞掉同步异常(放大 D1/D2)

**严重度:中 · 静默放大 · 上游未修**

这是 D1/D2 之所以难查的直接原因。WebGPU 的 descriptor 层错误是**同步异常**,
而 euv 到处用 `.unwrap_or(JsValue::UNDEFINED)`:

```
impl.rs:567/571   create_shader_module
impl.rs:588/591   create_command_encoder
impl.rs:2109      copy_texture_to_buffer
impl.rs:2164/2168 create_offline_render_target
impl.rs:2190/2192 create_texture_view
```

`call1(...)` 抛的 `TypeError` 被 `unwrap_or` 转成 `UNDEFINED`,
**连 console 都不打**。用户看到的是「函数返回了 undefined」,
而不是「usage 类型写错了」。

`impl.rs:3047-3059` 是唯一的例外 —— 那里老老实实 pop 出来打 console:

```rust
self.push_error_scope(GpuErrorFilter::Validation);
let result: JsValue = create_fn.call1(self.get_device(), &descriptor).unwrap_or(JsValue::UNDEFINED);
if let Some(error) = self.pop_error_sync() {
    web_sys::console::error_1(&error);
}
```

**建议**:至少给 `create_*` 系列加一个 debug-only 的
`if let Err(e) = ... { web_sys::console::error_1(&e) }`,或加 `#[cfg(debug_assertions)]` 的日志。
静默吞掉一个明确的 `TypeError` 是最难查的一类问题。

---

## 6. D5 —— error scope 覆盖面

**严重度:中 · 静默 · 上游未修**

全 crate 只有两处 `push_error_scope`,都在 `create_bind_group`:

```
impl.rs:2822   create_bind_group
impl.rs:3047   另一个 create_bind_group 变体
```

`create_render_pipeline`、`create_shader_module`、`create_texture_2d`、
`begin_render_pass`、所有 `copy_*` —— **全部没有验证包裹**。
结合 §3.3 的实测(pipelineLayoutMismatch 也是 `NOTHING`),
大量 validation error 直接消失。

**建议**:在 `create_render_pipeline` 和 `begin_render_pass_full` 外层
各加一对 push/pop,与 `create_bind_group` 对齐。

---

## 7. D6 —— 测试零覆盖

`euv-engine-0.28.6/tests/webgpu/` 共 20 个测试函数:

```
$ grep -n "copy_texture_to_buffer\|offline_render_target\|take_last_error\|command_encoder" tests/webgpu/*.rs
(空)
```

**三个有问题的 API 一个测试都没有。** 这解释了为什么 D1/D2 能一路发到 crates.io。

---

## 8. 复现怎么跑

```bash
cd /Users/sqs/code/vice-city-web/tools/gpu-probe

# 1) 起静态服务器 —— localhost 才是 secure context,navigator.gpu 才存在
#    (踩过的坑:用 about:blank 测会全部报 NO_NAVIGATOR_GPU,是 origin 问题不是 flag 问题)
python3 -m http.server 8879 --bind 127.0.0.1 &

# 2) 起独立的 Chrome for Testing。⚠️ 绝对不能带 --use-angle=swiftshader 或
#    --disable-gpu —— 两者各自都让 requestAdapter() 返回 null,那样测出来的
#    是 flag 问题而不是 euv 的缺陷。不写任何 GPU flag 反而最好(实测 apple/metal-3)。
#    用独立二进制而非用户的 /Applications/Google Chrome.app,别抢用户的窗口。
"chrome-for-testing" --headless=new --remote-debugging-port=9251 \
  --user-data-dir=/tmp/vcw-defects-profile about:blank &

# 3) 跑
VCW_CDP=http://127.0.0.1:9251 \
VCW_DEFECT_PAGE=http://127.0.0.1:8879/defect-repro.html \
  python3 defect_probe.py          # → D1/D2/D3 的实测 JSON

# D3 的通道深挖(5 个错误样本,判断哪些能被抓到):
VCW_DEFECT_PAGE=http://127.0.0.1:8879/errscope-probe.html python3 defect_probe.py
```

新增文件:

- `tools/gpu-probe/defect-repro.html` —— 三个 claim 的最小复现,纯 JS 不需要编译 WASM。
  每段 JS 都标了对应的 `impl.rs` 行号。
- `tools/gpu-probe/defect_probe.py` —— CDP 驱动器。复用 `flag_probe.py` 的约定
  (复用已有实例开新 tab,不新开窗口;采集 console 与未捕获异常)。
- `tools/gpu-probe/errscope-probe.html` —— error scope 通道覆盖面实测。

---

## 9. 我推翻/修正的线索汇总

| 报告的说法 | 实际源码 | 判定 |
|-----------|---------|------|
| `create_command_encoder`(impl.rs:579 附近)从不把 encoder 写回字段 | **正确**,且比报告更彻底:字段(`struct.rs:125`)全 crate 只有 `impl.rs:319` 一处赋值且恒为 `None`;`&self` 签名使写回在类型上不可能 | ✅ **成立**(机制需修正表述) |
| `copy_texture_to_buffer` 是 no-op | **正确** | ✅ 成立 |
| 0.28.9 同样存在该问题 | **正确**,而且更强:0.28.6 与 0.28.9 的 `src/` **逐字节相同** | ✅ 成立 |
| `create_offline_render_target` 返回 `undefined` | **正确** | ✅ 成立 |
| `usage` 参数被写成字符串而非数值位掩码(impl.rs:2143 附近) | **正确**,精确行号是 **2155-2159**(常量在 `const.rs:602`)。2143 是 `size` 那一行,行号偏了 12 行 | ✅ 成立(行号需修正) |
| `take_last_error()` 拿不到错误,被 `unwrap_or(UNDEFINED)` 吞掉 | **推翻**。`impl.rs:1964` 是 `cell.take()`,没有 `unwrap_or`。实测 validation error 通道可用(2/5 CAUGHT) | ❌ **不成立** |
| 这三条都导致「全黑画面」,与「WebGPU 不支持」同形 | **推翻**。本项目后端是 WebGL2(`enum Renderer` 只有 `WebGl`/`Software`),`WebGpuRenderer` 一次都没被调用 | ❌ **不成立** |

**真正需要提 issue 的是 D1、D2、D4、D5、D6,全部上游未修。**

---

## 10. 给本项目的结论

`vice-city-web` 目前的「WebGPU 不支持」症状,与本清单里的 euv-engine WebGPU
缺陷**没有因果关系**:

1. 本项目走的是 WebGL2,没碰过 `WebGpuRenderer`。
2. 那三个 API 一个都没被调用。

若未来要迁 WebGPU(`docs/webgpu-migration.md` 的方向),
**D1 + D2 会直接堵死 readback 路线**(离屏目标建不出来 + 拷贝不执行),
迁移前必须先修上游或本地 patch。

另外提醒:`tools/verify-parallel.sh` / `tools/gpu-probe/run.sh` 用的是
`--use-angle=swiftshader`,那组 flag 实测 `requestAdapter()` 返回 **null**。
若验收探针要测 WebGPU,flag 必须换掉 —— 否则测出来的「WebGPU 不支持」
是 flag 问题,不是浏览器或框架问题。

---

## 附:核实用到的命令

```bash
V6=~/.cargo/registry/src/index.crates.io-*/euv-engine-0.28.6
V9=~/Library/Caches/cargo-home/registry/src/rsproxy.cn-*/euv-engine-0.28.9

diff -r --brief $V6/src $V9/src                       # 逐字节对比
md5 -q $V6/src/renderer/webgpu/impl.rs $V9/.../impl.rs
grep -rn "command_encoder" $V6/src/                 # D1:只有一处赋值
grep -n "unwrap_or(JsValue::UNDEFINED)" $V6/src/renderer/webgpu/impl.rs
grep -n -A4 "WEBGPU_PROPERTY_USAGE" $V6/src/renderer/webgpu/impl.rs   # D2:唯一写字符串处
cd vice-city-web && grep -rn "WebGpu\|requestAdapter" src/            # 证明没走 WebGPU
```
