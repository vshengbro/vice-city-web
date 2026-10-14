//! WebGL2 对照路径。
//!
//! 目的只有一个:**同口径**。同一个三角形顶点、同一块画布、同一套
//! 像素统计、同一段帧率测量区间,换成 WebGL2 再跑一遍。只有这样
//! 「WebGPU 更快 / 更慢」才有意义 —— 三角形小到两边都被
//! `submit` 的固定开销主导,没有对照就看不出任何东西。


use super::*;

/// 跑一遍 WebGL2 对照路径,把失败也折成一份报告。
///
/// 初始化失败不是「没有报告」而是「一份 errors 非空的报告」——
/// `probe.py` 只读 `window.__probe.webgl`,让它去区分两种情况等于
/// 逼它处理一个本不该存在的分支。
///
/// # Returns
///
/// - `WebglReport` - The control report, with the failure recorded in
///   `errors` when the context could not be created.
pub fn run_webgl2() -> WebglReport {
    let start: f64 = crate::now_ms();
    match run_inner_webgl2(start) {
        Ok(report) => report,
        Err(message) => WebglReport {
            backend: String::from(BACKEND_WEBGL2),
            adapter: None,
            triangle_drawn: false,
            non_clear_pixels: 0,
            total_pixels: 0,
            coverage: 0.0,
            mean_rgb: [0.0, 0.0, 0.0],
            distinct_colors: 0,
            max_channel: 0,
            fps: 0.0,
            width: 0,
            height: 0,
            total_ms: 0.0,
            errors: vec![message],
        },
    }
}

/// Body of the `run_inner_webgl2` free function.
///
/// # Arguments
///
/// - `f64` - A 64-bit float (`f64`).
///
/// # Returns
///
/// - `Result<WebglReport, String>` - Result of the operation; an `Err` variant on failure.

fn run_inner_webgl2(start: f64) -> Result<WebglReport, String> {
    let element: web_sys::HtmlCanvasElement = canvas("gl-canvas")?;
    // `getContext("webgl2")` 失败时返回 `null`,`Ok(None)`。
    let context: WebGl2RenderingContext = element
        .get_context(BACKEND_WEBGL2)
        .map_err(|_| "getContext(webgl2) threw".to_string())?
        .ok_or("getContext(webgl2) returned null (no WebGL2?)")?
        .dyn_into::<WebGl2RenderingContext>()
        .map_err(|_| "getContext(webgl2) not a WebGl2RenderingContext".to_string())?;

    // `start` 只在 `run_inner_webgl2` 内部用不到 —— 帧率用 `bench_start/end`
    // 单独算,免得把「建上下文 + 编译着色器」的一次性开销算进稳态帧率。
    let _: &f64 = &start;
    let width: u32 = element.width();
    let height: u32 = element.height();
    if width == 0 || height == 0 {
        return Err(format!("canvas is {width}x{height}"));
    }

    let program: WebGlProgram = build_program(&context)?;
    let vao: WebGlVertexArrayObject = context
        .create_vertex_array()
        .ok_or("createVertexArray returned null")?;
    let vbo: WebGlBuffer = context
        .create_buffer()
        .ok_or("createBuffer(vbo) returned null")?;
    let ibo: WebGlBuffer = context
        .create_buffer()
        .ok_or("createBuffer(ibo) returned null")?;

    context.bind_vertex_array(Some(&vao));

    context.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(&vbo));
    // `Float32Array::view` 直接零拷贝映射 wasm 线性内存 —— 不需要
    // 先把 f32 打包成字节再解包回来。
    unsafe {
        let view: js_sys::Float32Array = js_sys::Float32Array::view(&TRI_XY);
        context.buffer_data_with_array_buffer_view(
            WebGl2RenderingContext::ARRAY_BUFFER,
            &view,
            WebGl2RenderingContext::STATIC_DRAW,
        );
    }
    context.enable_vertex_attrib_array(0);
    context.vertex_attrib_pointer_with_i32(0, 2, WebGl2RenderingContext::FLOAT, false, 8, 0);

    context.bind_buffer(WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER, Some(&ibo));
    let indices: [u32; 3] = [0, 1, 2];
    unsafe {
        let view: js_sys::Uint32Array = js_sys::Uint32Array::view(&indices);
        context.buffer_data_with_array_buffer_view(
            WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER,
            &view,
            WebGl2RenderingContext::STATIC_DRAW,
        );
    }

    // ---- 帧率 --------------------------------------------------------
    context.use_program(Some(&program));
    context.viewport(0, 0, width as i32, height as i32);
    context.disable(WebGl2RenderingContext::DEPTH_TEST);
    context.disable(WebGl2RenderingContext::CULL_FACE);

    let mut bench_start: f64 = 0.0;
    let mut bench_end: f64 = 0.0;
    for frame in 0..BENCH_FRAMES {
        draw_frame_webgl2(&context);
        if frame == WARMUP_FRAMES {
            bench_start = crate::now_ms();
        }
        if frame == BENCH_FRAMES - 1 {
            bench_end = crate::now_ms();
        }
    }
    let measured: u32 = BENCH_FRAMES - WARMUP_FRAMES - 1;
    let fps: f64 = if bench_end > bench_start && measured > 0 {
        f64::from(measured) * 1000.0 / (bench_end - bench_start)
    } else {
        crate::FPS_UNRESOLVED
    };

    // ---- 像素读回 ----------------------------------------------------
    // WebGL2 可以直接 `readPixels`,不需要像 WebGPU 那样绕
    // copyTextureToBuffer。但**必须**在同一帧里读:默认
    // `preserveDrawingBuffer = false` 时,交换链在合成后就被丢弃了,
    // 所以先画一帧、紧接着同步读,中间不能有别的 GL 调用。
    draw_frame_webgl2(&context);
    let mut pixels: Vec<u8> = vec![0u8; (width * height * 4) as usize];
    // 用 `read_pixels_with_u8_array_and_dst_offset` 而不是
    // `..._with_opt_u8_array` —— 后者按 `gl.RGBA` 的**分量**取参数
    // (`format`, `type`, `offset`),很容易把 `RGBA` 写成 `RGBA8`
    // 之类的字面量而被驱动拒绝。
    // `read_pixels` 的 `Result` **必须**检查。WebGL 规范允许它失败
    // (例如 `preserveDrawingBuffer=false` 时读了已合成的帧),而
    // web-sys 是直接 `Result` 返回、不抛 —— 忽略它会得到一片全黑,
    // 看起来和「三角形没画出来」一模一样。
    context
        .read_pixels_with_u8_array_and_dst_offset(
            0,
            0,
            width as i32,
            height as i32,
            WebGl2RenderingContext::RGBA,
            WebGl2RenderingContext::UNSIGNED_BYTE,
            &mut pixels,
            0,
        )
        .map_err(|_| "readPixels threw (swap chain already presented?)".to_string())?;

    // WebGL 的 `readPixels` 行序是**下往上**。统计只关心「有多少
    // 非黑像素」和均值,行序不影响结果,所以不翻转 —— 只要 WebGPU
    // 侧也用同口径统计即可(那边是 `copyTextureToBuffer`,上往下)。
    // 这里用 `width * 4` 当行距,因为 `readPixels` 的输出是紧密排列的。
    let stats: crate::PixelStats = crate::PixelStats::from_rgba(&pixels, width, height, width * 4);

    let mut errors: Vec<String> = Vec::new();
    loop {
        match context.get_error() {
            WebGl2RenderingContext::NO_ERROR => break,
            code => {
                errors.push(format!("glGetError = 0x{code:04x}"));
                if errors.len() > 8 {
                    break;
                }
            }
        }
    }
    let renderer: String = context
        .get_parameter(WebGl2RenderingContext::RENDERER)
        .ok()
        .and_then(|v: JsValue| v.as_string())
        .unwrap_or_else(|| String::from("?"));
    errors.push(format!("GL_RENDERER = {renderer}"));

    Ok(WebglReport {
        backend: String::from(BACKEND_WEBGL2),
        adapter: None,
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
        total_ms: crate::now_ms() - start,
        errors,
    })
}

/// Body of the `draw_frame_webgl2` free function.
///
/// # Arguments
///
/// - `&WebGl2RenderingContext` - Shared reference to a `WebGl2RenderingContext`.

fn draw_frame_webgl2(context: &WebGl2RenderingContext) {
    context.clear_color(0.0, 0.0, 0.0, 1.0);
    context.clear(WebGl2RenderingContext::COLOR_BUFFER_BIT);
    context.draw_elements_with_i32(
        WebGl2RenderingContext::TRIANGLES,
        3,
        WebGl2RenderingContext::UNSIGNED_INT,
        0,
    );
}

/// Body of the `build_program` free function.
///
/// # Arguments
///
/// - `&WebGl2RenderingContext` - Shared reference to a `WebGl2RenderingContext`.
///
/// # Returns
///
/// - `Result<WebGlProgram, String>` - Result of the operation; an `Err` variant on failure.

fn build_program(context: &WebGl2RenderingContext) -> Result<WebGlProgram, String> {
    let vertex: WebGlShader = compile(context, WebGl2RenderingContext::VERTEX_SHADER, WEBGL_VERT_GLSL)?;
    let fragment: WebGlShader =
        compile(context, WebGl2RenderingContext::FRAGMENT_SHADER, WEBGL_FRAG_GLSL)?;
    let program: WebGlProgram = context
        .create_program()
        .ok_or("createProgram returned null")?;
    context.attach_shader(&program, &vertex);
    context.attach_shader(&program, &fragment);
    context.link_program(&program);
    let linked: bool = context
        .get_program_parameter(&program, WebGl2RenderingContext::LINK_STATUS)
        .as_bool()
        .unwrap_or(false);
    if !linked {
        let log: String = info_log(context.get_program_info_log(&program));
        return Err(format!("linkProgram failed: {log}"));
    }
    Ok(program)
}

/// Body of the `compile` free function.
///
/// # Arguments
///
/// - `&WebGl2RenderingContext` - Shared reference to a `WebGl2RenderingContext`.
/// - `u32` - A 32-bit unsigned integer (`u32`).
/// - `&str` - Shared reference to a `str`.
///
/// # Returns
///
/// - `Result<WebGlShader, String>` - Result of the operation; an `Err` variant on failure.

fn compile(
    context: &WebGl2RenderingContext,
    kind: u32,
    source: &str,
) -> Result<WebGlShader, String> {
    let shader: WebGlShader = context
        .create_shader(kind)
        .ok_or_else(|| format!("createShader(0x{kind:04x}) returned null"))?;
    context.shader_source(&shader, source);
    context.compile_shader(&shader);
    let ok: bool = context
        .get_shader_parameter(&shader, WebGl2RenderingContext::COMPILE_STATUS)
        .as_bool()
        .unwrap_or(false);
    if !ok {
        let log: String = info_log(context.get_shader_info_log(&shader));
        return Err(format!("compileShader(0x{kind:04x}) failed: {log}"));
    }
    Ok(shader)
}

/// `getShaderInfoLog` / `getProgramInfoLog` 在成功时返回 `Some("")`。
/// 空串在错误信息里毫无用处,统一换成占位符,免得报告里出现
/// `compileShader failed: ` 这种断掉的话。
///
/// # Arguments
///
/// - `Option<String>` - A `Option<String>` parameter.
///
/// # Returns
///
/// - `String` - A `String` value.

fn info_log(text: Option<String>) -> String {
    match text {
        Some(value) if !value.trim().is_empty() => value,
        _ => String::from("(empty log)"),
    }
}
