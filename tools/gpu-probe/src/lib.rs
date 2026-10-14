//! gpu_probe
//!
//! ## 这个 crate 存在的唯一理由
//!
//! `docs/webgpu-migration.md` 里的方案声称「euv-engine 的 WebGPU 栈可
//! 从本项目调用」。但 `WebGpuRenderer::is_available()` 返回 `true` 只
//! 证明 `navigator.gpu` 是个非 null 对象 —— 它**不**证明能画出一张图。
//! 本 crate 回答的是后者:
//!
//! 1. 用 `euv_engine::WebGpuRenderer` 画一个三角形;
//! 2. 把 framebuffer 的像素**读回 CPU**,统计非清屏色像素 / 均值 / 唯一色数;
//! 3. 用 WebGL2 画同一个三角形,跑同一套统计;
//! 4. 两边都测帧率。
//!
//! 结论写进 `window.__probe`,由 `probe.py` 经 CDP 读走。
//!
//! ## 为什么是独立 crate
//!
//! 不能依赖 `vice_city_web` —— 那个 crate 拉进来等于在测旧 WebGL2 后端。
//! 独立性是本探针全部结论的前提。

mod config;
mod webgl;
mod wgsl;

use euv_engine::{
    BufferDescriptor, BufferUsage, Color, ColorTargetState, DrawIndexedArgs, FragmentState,
    GpuErrorFilter, GpuTextureFormat, IndexFormat, MultisampleState, PrimitiveState, RenderConfig,
    RenderPipelineDescriptor, TextureDescriptor, TextureUsage, VertexAttribute,
    VertexAttributeFormat, VertexBufferLayout, VertexState, VertexStepMode, WebGpuRenderer,
};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use web_sys::{
    HtmlCanvasElement, WebGl2RenderingContext, WebGlBuffer, WebGlProgram, WebGlShader,
    WebGlVertexArrayObject, Window,
};


use config::{
    BACKEND_WEBGPU, ERR_CALL_FAILED, FORMAT_BGRA8UNORM, FORMAT_RGBA8UNORM,
};
use wgsl::{FRAG_WGSL, VERT_WGSL};

/// 探针报告写到这个全局对象,`probe.py` 用 CDP 读走。
const REPORT_KEY: &str = "__probe";

/// 三角形顶点(裁剪空间坐标)。逆时针,与 `PrimitiveState` 的
/// `FrontFace::CounterClockwise` 默认值 + `CullMode::None` 都不冲突。
const TRI_XY: [f32; 6] = [-0.6, -0.5, 0.6, -0.5, 0.0, 0.6];

/// 帧率算不出来时的占位值(实测 0)。见 [`FPS_UNRESOLVED`]。
const FPS_UNRESOLVED: f64 = 0.0;

/// 计时循环的帧数。
///
/// 比直觉上大两个数量级,原因是:这是一个**同步**循环,测的是 CPU 端
/// 把帧编码 + 提交出去的速度,不是 GPU 画完的速度。Chromium 无头下
/// `performance.now()` 被量化到 100 µs 一档,120 帧跑完首尾落在同一档
/// 里(实测 `bench_end == bench_start`),算出来是 0 fps —— 那不是
/// 「后端慢」,是「时钟不够细」。2000 帧让总耗时跨过足够多的档位。
const BENCH_FRAMES: u32 = 2000;
/// 热身帧数,不计入。着色器编译与首帧管线预热都在这几帧里。
const WARMUP_FRAMES: u32 = 50;

/// Body of the `main` free function.
#[wasm_bindgen]
pub fn main() {
    wasm_bindgen_futures::spawn_local(async {
        if let Err(err) = run().await {
            fail(&err);
        }
    });
}

/// Body of the `run` free function.
///
/// # Returns
///
/// - `Result<(), String>` - Result of the operation; an `Err` variant on failure.
async fn run() -> Result<(), String> {
    // ---- 0. 环境记录 -------------------------------------------------
    // `is_available()` 只看 `navigator.gpu` 是不是非 null 对象;
    // `probe()` 才会真的 await `requestAdapter()`。两个都记下来,
    // 因为迁移方案把它们当成了同义词。
    let euv_is_available: bool = WebGpuRenderer::is_available();
    let euv_probe: bool = WebGpuRenderer::probe().await;

    // ---- 1. WebGPU:初始化 + 画 + 读回 -------------------------------
    let config: RenderConfig = RenderConfig::webgpu("#gpu-canvas", 256.0, 256.0);
    let webgpu: Result<ProbeReport, String> = match WebGpuRenderer::init(&config).await {
        Ok(mut renderer) => {
            let adapter: JsValue = read_adapter_info().await;
            run_webgpu(&mut renderer, adapter).await
        }
        Err(e) => Err(format!("WebGpuRenderer::init failed: {}", e.code())),
    };

    // ---- 2. WebGL2:同一个三角形,同一套统计 --------------------------
    let webgl: webgl::WebglReport = webgl::run_webgl2();

    // ---- 3. 汇总 -----------------------------------------------------
    let payload: JsValue = js_sys::Object::new().into();
    set(
        &payload,
        "euv_is_available",
        &JsValue::from_bool(euv_is_available),
    );
    set(&payload, "euv_probe", &JsValue::from_bool(euv_probe));
    match &webgpu {
        Ok(report) => set(&payload, BACKEND_WEBGPU, &to_js(report)),
        Err(message) => {
            let err: JsValue = js_sys::Object::new().into();
            set(&err, "error", &JsValue::from_str(message));
            set(&payload, "webgpu", &err);
        }
    }
    set(&payload, "webgl", &webgl.to_js_webgl2());
    write_report(&payload);
    Ok(())
}

/// Body of the `set` free function.
///
/// # Arguments
///
/// - `&JsValue` - Shared reference to a `JsValue`.
/// - `&str` - Shared reference to a `str`.
/// - `&JsValue` - Shared reference to a `JsValue`.
fn set(target: &JsValue, key: &str, value: &JsValue) {
    let _: Result<bool, JsValue> = js_sys::Reflect::set(target, &JsValue::from_str(key), value);
}

/// Body of the `write_report` free function.
///
/// # Arguments
///
/// - `&JsValue` - Shared reference to a `JsValue`.
fn write_report(payload: &JsValue) {
    if let Some(window) = web_sys::window() {
        set(window.as_ref(), REPORT_KEY, payload);
    }
}

/// Body of the `fail` free function.
///
/// # Arguments
///
/// - `&str` - Shared reference to a `str`.
fn fail(message: &str) {
    let _: () = web_sys::console::error_1(&JsValue::from_str(message));
    let payload: JsValue = js_sys::Object::new().into();
    set(&payload, "error", &JsValue::from_str(message));
    write_report(&payload);
}

/// 读 adapter 的 vendor / architecture / description。
///
/// `euv-engine` 没有暴露 adapter(它在 `init` 内部拿到 device 之后就
/// 丢掉了),所以这里独立发一次 `requestAdapter()` 只为拿 `info` ——
/// **不**调 `requestDevice()`,是纯只读探测。
///
/// # Returns
///
/// - `JsValue` - A `JsValue` value.
async fn read_adapter_info() -> JsValue {
    let Some(window) = web_sys::window() else {
        return JsValue::NULL;
    };
    let gpu: JsValue = js_sys::Reflect::get(&window.navigator(), &JsValue::from_str("gpu"))
        .unwrap_or(JsValue::UNDEFINED);
    if gpu.is_undefined() || gpu.is_null() {
        return JsValue::NULL;
    }
    let Ok(request) = js_sys::Reflect::get(&gpu, &JsValue::from_str("requestAdapter")) else {
        return JsValue::NULL;
    };
    let Ok(promise) = request
        .dyn_into::<js_sys::Function>()
        .map_err(|_| "not a function")
        .and_then(|f: js_sys::Function| f.call0(&gpu).map_err(|_| ERR_CALL_FAILED))
    else {
        return JsValue::NULL;
    };
    let Ok(promise) = js_sys::Promise::from(promise).await else {
        return JsValue::NULL;
    };
    if promise.is_null() || promise.is_undefined() {
        return JsValue::NULL;
    }
    // `adapter.info` 在新版 Chrome 上是同步属性;老版本要走
    // `requestAdapterInfo()`。两个都试,都失败就交白卷。
    let info: JsValue =
        js_sys::Reflect::get(&promise, &JsValue::from_str("info")).unwrap_or(JsValue::UNDEFINED);
    if !info.is_undefined() && !info.is_null() {
        return info;
    }
    let Ok(request_info) = js_sys::Reflect::get(&promise, &JsValue::from_str("requestAdapterInfo"))
    else {
        return JsValue::NULL;
    };
    let Ok(inner) = request_info
        .dyn_into::<js_sys::Function>()
        .map_err(|_| "not a function")
        .and_then(|f: js_sys::Function| f.call0(&promise).map_err(|_| ERR_CALL_FAILED))
    else {
        return JsValue::NULL;
    };
    js_sys::Promise::from(inner).await.unwrap_or(JsValue::NULL)
}

/// WebGPU 侧的完整流程:建管线 → 画 N 帧测速 → 渲一帧到离屏纹理读回像素。
///
/// 帧率与像素读回**刻意分开**。读回要 `await mapAsync`,把它算进
/// 帧时间会把 fps 数字彻底污染(那不是渲染耗时,是 IO 等待)。
///
/// # Arguments
///
/// - `&mut WebGpuRenderer` - Mutable reference to a `WebGpuRenderer` (mutated in place).
/// - `JsValue` - A `JsValue` parameter.
///
/// # Returns
///
/// - `Result<ProbeReport, String>` - Result of the operation; an `Err` variant on failure.
async fn run_webgpu(
    renderer: &mut WebGpuRenderer,
    adapter: JsValue,
) -> Result<ProbeReport, String> {
    let mut errors: Vec<String> = Vec::new();
    renderer.push_error_scope(GpuErrorFilter::Validation);

    // ---- 顶点/索引缓冲 ---------------------------------------------
    let mut vertex_bytes: Vec<u8> = Vec::with_capacity(TRI_XY.len() * 4);
    for value in TRI_XY {
        vertex_bytes.extend_from_slice(&value.to_le_bytes());
    }
    let vertex_buffer: JsValue = renderer.create_vertex_buffer(&vertex_bytes);
    if vertex_buffer.is_undefined() {
        return Err("create_vertex_buffer returned undefined".into());
    }

    let mut index_bytes: Vec<u8> = Vec::with_capacity(12);
    for i in [0u32, 1, 2] {
        index_bytes.extend_from_slice(&i.to_le_bytes());
    }
    let index_buffer: JsValue = renderer.create_index_buffer(&index_bytes);
    if index_buffer.is_undefined() {
        return Err("create_index_buffer returned undefined".into());
    }

    // ---- 管线 ------------------------------------------------------
    let module: JsValue = renderer
        .create_shader_module_with_label(&format!("{VERT_WGSL}\n{FRAG_WGSL}"), "probe-triangle");
    if module.is_undefined() {
        return Err("create_shader_module returned undefined (WGSL rejected?)".into());
    }

    // 顶点布局(1 个 float32x2 @location(0),stride 8 字节,与 `TRI_XY`
    // 的字节布局严格一一对应)在 `build_pipeline` 里,两条 pipeline 共用。
    //
    // ⚠️ 颜色 target 的格式**必须**跟交换链一致,也就是
    // `renderer.get_format()`(= macOS 上的 `"bgra8unorm"`)，
    // 不能想当然写 `Rgba8Unorm`。不一致时 `createRenderPipeline`
    // 会因为「pipeline 的 target 格式与 render pass 的 attachment
    // 格式不匹配」而抛 validation error,`Reflect::call1` 把它吞成
    // `undefined` —— 于是 `pipeline.is_undefined()` 为真。
    //
    // 但这里有个真实的取舍:探针要用**两条**渲染路径,它们的
    // attachment 格式不同:
    //
    //   - 帧率那条画在交换链上,格式 = `get_format()`(bgra8unorm);
    //   - 读回那条画在离屏纹理上,格式 = `rgba8unorm`。
    //
    // WebGPU 的 pipeline 把 target 格式**编译进**了状态机,所以一条
    // pipeline 不能同时喂给两者。必须建**两条** pipeline。踩这个坑
    // 时的症状极具误导性:画布全黑,读回全 0,看起来像「WebGPU
    // 在这台机器上根本不能画」,而实际上硬件好得很。
    let swap_format: GpuTextureFormat = swapchain_format(renderer.get_format().as_str());
    let swap_pipeline: JsValue =
        build_pipeline(renderer, &module, swap_format, "probe-triangle-swapchain");
    if swap_pipeline.is_undefined() {
        return Err(format!(
            "create_render_pipeline_full(swapchain {swap_format:?}) returned undefined",
        ));
    }
    let offscreen_pipeline: JsValue = build_pipeline(
        renderer,
        &module,
        GpuTextureFormat::Rgba8Unorm,
        "probe-triangle-offscreen",
    );
    if offscreen_pipeline.is_undefined() {
        return Err(String::from(
            "create_render_pipeline_full(offscreen Rgba8Unorm) returned undefined",
        ));
    }
    let pipeline: JsValue = swap_pipeline.clone();

    // ---- 帧率:交换链上连画 BENCH_FRAMES 帧 -------------------------
    let width: u32 = renderer.get_width();
    let height: u32 = renderer.get_height();
    let clear: Color = Color::new(0.0, 0.0, 0.0, 1.0);

    let start: f64 = now_ms();
    let mut bench_start: f64 = 0.0;
    let mut bench_end: f64 = 0.0;
    for frame in 0..BENCH_FRAMES {
        draw_frame(renderer, &pipeline, &vertex_buffer, &index_buffer, clear);
        if frame == WARMUP_FRAMES {
            bench_start = now_ms();
        }
        if frame == BENCH_FRAMES - 1 {
            bench_end = now_ms();
        }
    }
    let measured: u32 = BENCH_FRAMES - WARMUP_FRAMES - 1;
    let fps: f64 = if bench_end > bench_start && measured > 0 {
        f64::from(measured) * 1000.0 / (bench_end - bench_start)
    } else {
        // 计时窗口没跨过一个时钟档位。报 0 等于谎报「慢到无法渲染」。
        FPS_UNRESOLVED
    };
    if fps == FPS_UNRESOLVED {
        errors.push(format!(
            "fps unresolved: {measured} frames took < one performance.now() tick ({} ms) -- \
             this is a CPU-side submit-rate number, NOT GPU throughput",
            bench_end - bench_start,
        ));
    }

    // ---- 像素读回:渲一帧到离屏 rgba8unorm,再 copyTextureToBuffer ----
    //
    // ⚠️ 这里**不用** `create_offline_render_target`。它看起来正是干这
    // 件事的,但在 euv-engine 0.28.6 / 0.28.9 上实测返回 `undefined`。
    // 根因在 impl.rs:2143 —— 它把 `usage` 写成**字符串**
    // `"RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC"`,而 WebGPU
    // 规范要求 `GPUTextureDescriptor.usage` 是一个 GPUShaderStage 风格的
    // **数值位掩码**。`createTexture` 直接抛 TypeError,`Reflect::call1`
    // 把它吞成 `undefined`。同文件里 `create_texture`(impl.rs:2572)
    // 用的是 `JsValue::from_f64(usage as f64)`,那条路是好的。
    //
    // 所以这里改用 `TextureDescriptor` + `create_texture`,显式 OR 出
    // 三个 use 位。这同时是一次对照:同一份 euv-engine,两条建纹理
    // 的路,一条通一条不通。
    // `TextureDescriptor::new` 的签名是
    // `(width, height, dimension, format, usage)` —— `depth_or_layers` /
    // `mip_level_count` / `sample_count` / `label` 都是 `#[new(skip)]`,
    // 且 lombok 的 setter 在这些字段上没有 `#[set(pub)]`,所以外部
    // 零值正好等价于「一张普通的 2d 单采样纹理」——
    // `create_texture` 内部对 `depth_or_layers` 与 `sample_count`
    // 都做了 `.max(1)`(impl.rs:2530 / 2568)。
    let usage: u32 = TextureUsage::RenderAttachment.usage_bit()
        | TextureUsage::TextureBinding.usage_bit()
        | TextureUsage::CopySource.usage_bit();
    let descriptor: TextureDescriptor =
        TextureDescriptor::new(width, height, "2d", GpuTextureFormat::Rgba8Unorm, usage);
    let texture: JsValue = renderer.create_texture(&descriptor);
    if texture.is_undefined() {
        return Err("create_texture(readback target) returned undefined".into());
    }
    let view: JsValue = renderer.create_texture_view(&texture);
    if view.is_undefined() {
        return Err("create_texture_view returned undefined".into());
    }

    // 对照记录:`create_offline_render_target` 在同一台机器上的结果,
    // 让「为什么不用它」这条结论可复核,而不是只剩一句注释。
    let (offline_texture, offline_view): (JsValue, JsValue) =
        renderer.create_offline_render_target(width, height, FORMAT_RGBA8UNORM);
    let offline_usable: bool = !offline_texture.is_undefined() && !offline_view.is_undefined();
    if !offline_usable {
        errors.push(String::from(
            "create_offline_render_target returned undefined (usage passed as string, not bitmask) \
             -- using create_texture instead",
        ));
    }

    // `copyTextureToBuffer` 要求 `bytesPerRow` 是 256 的倍数。
    let bytes_per_row: u32 = width.div_ceil(64) * 256;
    let readback_size: u64 = u64::from(bytes_per_row) * u64::from(height);

    let readback_buffer: JsValue = renderer.create_buffer(&BufferDescriptor::new(
        readback_size,
        BufferUsage::CopyDestination.usage_bit() | BufferUsage::MapRead.usage_bit(),
    ));
    if readback_buffer.is_undefined() {
        return Err("create_buffer(readback) returned undefined".into());
    }

    let encoder: JsValue = renderer.create_command_encoder();
    let pass: JsValue = renderer.begin_render_pass_to_texture(
        &encoder,
        &view,
        Some(Color::new(0.0, 0.0, 0.0, 1.0)),
        None,
        None,
    );
    renderer.set_pipeline(&pass, &offscreen_pipeline);
    renderer.set_vertex_buffer(&pass, 0, &vertex_buffer);
    renderer.set_index_buffer(&pass, &index_buffer, IndexFormat::Uint32);
    renderer.draw_indexed(&pass, &DrawIndexedArgs::whole_buffer(3, 1));
    renderer.end_render_pass(&pass);

    // ⚠️ euv-engine 0.28.6 的 `copy_texture_to_buffer` 内部走的是
    // `self.get_command_encoder()`(impl.rs:2101),而
    // `create_command_encoder`(impl.rs:579)**只返回**新 encoder、
    // 从不把它写回 `self.command_encoder` 字段(该字段只在 `init`
    // 里被设成 `None`,之后无任何赋值)。所以不手动 `set_command_encoder`
    // 的话,这里的 copy 是**静默 no-op**,后面 `read_buffer` 读到的
    // 全是 0 —— 看起来像「三角形没画出来」,实际是引擎 bug。
    renderer.set_command_encoder(Some(encoder.clone()));

    renderer.copy_texture_to_buffer(&texture, &readback_buffer, bytes_per_row, width, height);
    let command: JsValue = renderer.finish_command_encoder(&encoder);
    renderer.submit(&[command]);

    let bytes: Vec<u8> = renderer
        .read_buffer(&readback_buffer, 0, readback_size)
        .await
        .ok_or("read_buffer returned None (mapAsync failed?)")?;

    let stats: PixelStats = PixelStats::from_rgba(&bytes, width, height, bytes_per_row);
    if let Some(err) = renderer.take_last_error() {
        errors.push(format!("{err:?}"));
    }

    let adapter_text: Option<String> = if adapter.is_null() || adapter.is_undefined() {
        None
    } else {
        Some(adapter_summary(&adapter))
    };

    Ok(ProbeReport {
        backend: String::from(BACKEND_WEBGPU),
        adapter: adapter_text,
        triangle_drawn: stats.non_clear > 0,
        non_clear_pixels: stats.non_clear,
        total_pixels: stats.total,
        coverage: stats.non_clear as f64 / f64::from(stats.total.max(1)) * 100.0,
        mean_rgb: stats.mean_rgb,
        distinct_colors: stats.distinct,
        max_channel: stats.max_channel,
        fps,
        width,
        height,
        // 从「拿到 renderer」到「读完像素」的总耗时,含一次性建管线
        // 开销。跟 `fps` 分开报:后者是稳态,前者是冷启动。
        total_ms: now_ms() - start,
        errors,
    })
}

/// Body of the `adapter_summary` free function.
///
/// # Arguments
///
/// - `&JsValue` - Shared reference to a `JsValue`.
///
/// # Returns
///
/// - `String` - A `String` value.
fn adapter_summary(adapter: &JsValue) -> String {
    let field: fn(&JsValue, &str) -> String = |adapter: &JsValue, key: &str| -> String {
        js_sys::Reflect::get(adapter, &JsValue::from_str(key))
            .ok()
            .and_then(|v: JsValue| v.as_string())
            .unwrap_or_else(|| String::from("?"))
    };
    format!(
        "vendor={} architecture={} description={} device={}",
        field(adapter, "vendor"),
        field(adapter, "architecture"),
        field(adapter, "description"),
        field(adapter, "device"),
    )
}

/// 交换链格式字符串 → euv 的 `GpuTextureFormat`。
///
/// 探针不能把颜色 target 格式写死:`getPreferredCanvasFormat()` 在
/// macOS 上返回 `bgra8unorm`,在别的平台可能是 `rgba8unorm`。格式与
/// render pass 的 attachment 对不上时 `createRenderPipeline` 直接
/// validation 失败,而 euv 的 `Reflect::call1(...).unwrap_or(UNDEFINED)`
/// 会把这个错误吞成 `undefined` —— 症状是「画布全黑、读回全 0」,
/// 看起来像 WebGPU 不能用,实际只是一处字符串不匹配。
///
/// # Arguments
///
/// - `&str` - Shared reference to a `str`.
///
/// # Returns
///
/// - `GpuTextureFormat` - A `GpuTextureFormat` value.
fn swapchain_format(name: &str) -> GpuTextureFormat {
    match name {
        FORMAT_BGRA8UNORM => GpuTextureFormat::Bgra8Unorm,
        FORMAT_RGBA8UNORM => GpuTextureFormat::Rgba8Unorm,
        // 未见过的格式不猜。退回 `Rgba8Unorm` 并让调用方的
        // `is_undefined()` 检查去暴露它 —— 一个错的枚举值会让
        // `gpu_texture_format_name` 产出一个驱动不认的字符串,那
        // 是更难查的失败。
        _ => GpuTextureFormat::Rgba8Unorm,
    }
}

/// 建一条渲染管线。顶点布局与调用方一致(1 个 `float32x2`
/// @location(0),stride 8),唯一变量是颜色 target 格式 —— 所以
/// swapchain / offscreen 两条 pipeline 的差别只在这一项。
///
/// # Arguments
///
/// - `&WebGpuRenderer` - Shared reference to a `WebGpuRenderer`.
/// - `&JsValue` - Shared reference to a `JsValue`.
/// - `GpuTextureFormat` - A `GpuTextureFormat` parameter.
/// - `&str` - Shared reference to a `str`.
///
/// # Returns
///
/// - `JsValue` - A `JsValue` value.
fn build_pipeline(
    renderer: &WebGpuRenderer,
    module: &JsValue,
    format: GpuTextureFormat,
    label: &str,
) -> JsValue {
    let vertex_state: VertexState = VertexState::new(
        module.clone(),
        String::from("vs_main"),
        vec![VertexBufferLayout::new(
            8,
            VertexStepMode::Vertex,
            vec![VertexAttribute::new(0, 0, VertexAttributeFormat::Float32x2)],
        )],
    );
    let fragment_state: FragmentState = FragmentState::new(
        module.clone(),
        String::from("fs_main"),
        vec![ColorTargetState::for_format(format)],
    );
    let descriptor: RenderPipelineDescriptor = RenderPipelineDescriptor::new(
        vertex_state,
        PrimitiveState::default(),
        None,
        MultisampleState::with_sample_count(1),
        Some(fragment_state),
    );
    let _: &str = label;
    renderer.create_render_pipeline_full(&descriptor)
}

/// 画一帧到交换链:清屏 → 画三角形 → submit。
///
/// # Arguments
///
/// - `&mut WebGpuRenderer` - Mutable reference to a `WebGpuRenderer` (mutated in place).
/// - `&JsValue` - Shared reference to a `JsValue`.
/// - `&JsValue` - Shared reference to a `JsValue`.
/// - `&JsValue` - Shared reference to a `JsValue`.
/// - `Color` - A `Color` parameter.
fn draw_frame(
    renderer: &mut WebGpuRenderer,
    pipeline: &JsValue,
    vertex_buffer: &JsValue,
    index_buffer: &JsValue,
    clear: Color,
) {
    let encoder: JsValue = renderer.create_command_encoder();
    let pass: JsValue = renderer.begin_render_pass(&encoder, clear);
    renderer.set_pipeline(&pass, pipeline);
    renderer.set_vertex_buffer(&pass, 0, vertex_buffer);
    renderer.set_index_buffer(&pass, index_buffer, IndexFormat::Uint32);
    renderer.draw_indexed(&pass, &DrawIndexedArgs::whole_buffer(3, 1));
    renderer.end_render_pass(&pass);
    let command: JsValue = renderer.finish_command_encoder(&encoder);
    renderer.submit(&[command]);
}

/// WebGPU 单后端的测量结果。
#[derive(serde::Serialize)]
pub struct ProbeReport {
    pub backend: String,
    /// adapter 描述;WebGL2 恒为 `None`(WebGL2 没这个概念)。
    pub adapter: Option<String>,
    /// 判据:非清屏色像素数 > 0。
    pub triangle_drawn: bool,
    pub non_clear_pixels: u32,
    pub total_pixels: u32,
    /// 非清屏像素占全图百分比 —— 用来对照 WebGL2 的同口径数字。
    pub coverage: f64,
    pub mean_rgb: [f64; 3],
    pub distinct_colors: u32,
    pub max_channel: u8,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    /// 冷启动总耗时(含建管线),与稳态 `fps` 分开看。
    pub total_ms: f64,
    pub errors: Vec<String>,
}

/// Inherent implementation of [`ProbeReport`].
impl ProbeReport {
    /// Backend identifier this report was produced by.
    ///
    /// # Returns
    ///
    /// - `&str` - Always `webgpu`; the WebGL2 control reports carry their
    ///   own value.
    fn get_backend(&self) -> &str {
        &self.backend
    }

    /// adapter 描述;WebGL2 恒为 `None`(WebGL2 没这个概念)。
    ///
    /// 返回引用而非克隆:`to_js` 只借用一次,克隆会白付一次分配。
    ///
    /// # Returns
    ///
    /// - `&Option<String>` - `Some(text)` when the adapter answered,
    ///   `None` when the probe failed or WebGL2 has no adapter.
    fn get_adapter(&self) -> &Option<String> {
        &self.adapter
    }

    /// 判据:非清屏色像素数 > 0。
    ///
    /// # Returns
    ///
    /// - `bool` - `true` when at least one pixel differs from the clear
    ///   colour, i.e. the triangle actually reached the framebuffer.
    fn get_triangle_drawn(&self) -> bool {
        self.triangle_drawn
    }

/// Count of pixels that differ from the clear colour.
///
    /// # Returns
    ///
    /// - `u32` - Count of pixels that are not the clear colour.
    fn get_non_clear_pixels(&self) -> u32 {
        self.non_clear_pixels
    }

/// Total pixels in the readback, clear colour included.
///
    /// # Returns
    ///
    /// - `u32` - Total pixels in the readback, clear colour included.
    fn get_total_pixels(&self) -> u32 {
        self.total_pixels
    }

    /// 非清屏像素占全图百分比 —— 用来对照 WebGL2 的同口径数字。
    ///
    /// # Returns
    ///
    /// - `f64` - Percentage of the image covered by the triangle.
    fn get_coverage(&self) -> f64 {
        self.coverage
    }

/// Per-channel mean computed over the whole readback.
///
    /// # Returns
    ///
    /// - `[f64; 3]` - Per-channel mean over the whole readback.
    fn get_mean_rgb(&self) -> [f64; 3] {
        self.mean_rgb
    }

/// Number of distinct RGBA values seen in the readback.
///
    /// # Returns
    ///
    /// - `u32` - Number of distinct RGBA values seen in the readback.
    fn get_distinct_colors(&self) -> u32 {
        self.distinct_colors
    }

/// Largest channel value found anywhere in the readback.
///
    /// # Returns
    ///
    /// - `u8` - Largest channel value anywhere in the readback.
    fn get_max_channel(&self) -> u8 {
        self.max_channel
    }

/// Submit-rate FPS measured over the benchmark window.
///
    /// # Returns
    ///
    /// - `f64` - Submit-rate FPS over the measured window, or
    ///   `FPS_UNRESOLVED` when the clock could not resolve it.
    fn get_fps(&self) -> f64 {
        self.fps
    }

/// Canvas width in pixels.
///
    /// # Returns
    ///
    /// - `u32` - Canvas width in pixels.
    fn get_width(&self) -> u32 {
        self.width
    }

/// Canvas height in pixels.
///
    /// # Returns
    ///
    /// - `u32` - Canvas height in pixels.
    fn get_height(&self) -> u32 {
        self.height
    }

    /// 冷启动总耗时(含建管线),与稳态 `fps` 分开看。
    ///
    /// # Returns
    ///
    /// - `f64` - Milliseconds from renderer acquisition to pixel readback.
    fn get_total_ms(&self) -> f64 {
        self.total_ms
    }

/// Diagnostics collected during the run.
///
    /// # Returns
    ///
    /// - `&[String]` - Collected diagnostics, empty when the run was clean.
    fn get_errors(&self) -> &[String] {
        &self.errors
    }

    /// 转成 JS 对象。`serde` 那边是 derive,这里手工搭对象,
    /// 免得为一个报告再拖一个 `serde_wasm_bindgen` 依赖。
    ///
    /// 所有字段读取都走上面的 accessor(§17.12)—— 报告结构与 JS 键名
    /// 一一对应,`probe.py` 经 CDP 按这些键取值,改键名等于改协议。
    ///
    /// # Returns
    ///
    /// - `JsValue` - A `JsValue` value.
    fn to_js(&self) -> JsValue {
        let out: JsValue = js_sys::Object::new().into();
        set(&out, "backend", &JsValue::from_str(self.get_backend()));
        set(
            &out,
            "adapter",
            &match self.get_adapter() {
                Some(text) => JsValue::from_str(text),
                None => JsValue::NULL,
            },
        );
        set(
            &out,
            "triangle_drawn",
            &JsValue::from_bool(self.get_triangle_drawn()),
        );
        set(
            &out,
            "non_clear_pixels",
            &JsValue::from_f64(f64::from(self.get_non_clear_pixels())),
        );
        set(
            &out,
            "total_pixels",
            &JsValue::from_f64(f64::from(self.get_total_pixels())),
        );
        set(&out, "coverage_pct", &JsValue::from_f64(self.get_coverage()));
        let mean: JsValue = js_sys::Array::of3(
            &JsValue::from_f64(self.get_mean_rgb()[0]),
            &JsValue::from_f64(self.get_mean_rgb()[1]),
            &JsValue::from_f64(self.get_mean_rgb()[2]),
        )
        .into();
        set(&out, "mean_rgb", &mean);
        set(
            &out,
            "distinct_colors",
            &JsValue::from_f64(f64::from(self.get_distinct_colors())),
        );
        set(
            &out,
            "max_channel",
            &JsValue::from_f64(f64::from(self.get_max_channel())),
        );
        set(&out, "fps", &JsValue::from_f64(self.get_fps()));
        set(&out, "width", &JsValue::from_f64(f64::from(self.get_width())));
        set(&out, "height", &JsValue::from_f64(f64::from(self.get_height())));
        set(&out, "total_ms", &JsValue::from_f64(self.get_total_ms()));
        let errors: JsValue = self
            .get_errors()
            .iter()
            .map(|message: &String| JsValue::from_str(message))
            .collect::<js_sys::Array>()
            .into();
        set(&out, "errors", &errors);
        out
    }
}

/// 把 `ProbeReport` 转成 JS 值(模块内共用)。
///
/// # Arguments
///
/// - `&ProbeReport` - Shared reference to a `ProbeReport`.
///
/// # Returns
///
/// - `JsValue` - A `JsValue` value.
pub fn to_js(report: &ProbeReport) -> JsValue {
    report.to_js()
}

/// 像素统计。
///
/// `bytes_per_row` 有 256 字节对齐填充,统计时必须按行跳过填充 ——
/// 否则行尾的 0 会被算成「像素」,coverage 直接虚高。
pub struct PixelStats {
    pub non_clear: u32,
    pub total: u32,
    pub mean_rgb: [f64; 3],
    pub distinct: u32,
    pub max_channel: u8,
}

/// Inherent implementation of [`PixelStats`].
impl PixelStats {
/// Implements the `from_rgba` operation on [`PixelStats`].
///
/// # Arguments
///
/// - `&[u8]` - Shared reference to a `[u8]`.
/// - `u32` - A 32-bit unsigned integer (`u32`).
/// - `u32` - A 32-bit unsigned integer (`u32`).
/// - `u32` - A 32-bit unsigned integer (`u32`).
    pub fn from_rgba(bytes: &[u8], width: u32, height: u32, bytes_per_row: u32) -> Self {
        let mut non_clear: u32 = 0;
        let mut sum: [u64; 3] = [0, 0, 0];
        let mut max_channel: u8 = 0;
        let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let total: u32 = width * height;
        for y in 0..height {
            let row_start: usize = (y * bytes_per_row) as usize;
            for x in 0..width {
                let i: usize = row_start + (x * 4) as usize;
                if i + 3 >= bytes.len() {
                    continue;
                }
                let (r, g, b, a) = (bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]);
                max_channel = max_channel.max(r).max(g).max(b);
                sum[0] += u64::from(r);
                sum[1] += u64::from(g);
                sum[2] += u64::from(b);
                // 清屏色是纯黑,三角形是品红偏红,「非黑」即可判定。
                if r != 0 || g != 0 || b != 0 {
                    non_clear += 1;
                }
                seen.insert(
                    (u32::from(r) << 24)
                        | (u32::from(g) << 16)
                        | (u32::from(b) << 8)
                        | u32::from(a),
                );
            }
        }
        let n: f64 = f64::from(total).max(1.0);
        Self {
            non_clear,
            total,
            mean_rgb: [sum[0] as f64 / n, sum[1] as f64 / n, sum[2] as f64 / n],
            distinct: seen.len() as u32,
            max_channel,
        }
    }
}

/// Implements the `now_ms` operation on [`PixelStats`].
///
/// # Returns
///
/// - `f64` - A 64-bit float.
pub fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w: Window| w.performance())
        .map(|p: web_sys::Performance| p.now())
        .unwrap_or(0.0)
}

/// 页面上的 canvas,按 id 取。WebGPU 用 `#gpu-canvas`,WebGL2 用
/// `#gl-canvas` —— 两张分开的画布,因为 `getContext("webgpu")` 和
/// `getContext("webgl2")` 一旦绑在同一张上就再也换不掉了。
///
/// # Arguments
///
/// - `&str` - Shared reference to a `str`.
///
/// # Returns
///
/// - `Result<HtmlCanvasElement, String>` - Result of the operation; an `Err` variant on failure.
pub fn canvas(id: &str) -> Result<HtmlCanvasElement, String> {
    let element: Option<web_sys::Element> = web_sys::window()
        .and_then(|w: Window| w.document())
        .and_then(|d: web_sys::Document| d.get_element_by_id(id));
    let element: web_sys::Element =
        element.ok_or_else(|| format!("canvas #{id} not found in the DOM"))?;
    element
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| format!("#{id} is not an HtmlCanvasElement"))
}
