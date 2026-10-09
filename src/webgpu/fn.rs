//! WebGPU 设备获取(异步部分)。
//!
//! `requestAdapter()` 与 `requestDevice()` 都返回 Promise,而 `boot()`
//! 是同步的 —— 所以设备获取必须整体跑在
//! [`wasm_bindgen_futures::spawn_local`] 的一条 `async` 链里。
//! 这是把 WebGPU 装进本项目**唯一**的架构侵入点:渲染器的**构造**
//! 变成异步的,`render()` 仍然是同步的(拿到 device 之后每帧不再需要
//! 任何 await)。

use super::*;

/// 依次 `requestAdapter()` → `requestDevice()` → `getContext` →
/// `configure`,返回一份可渲染的上下文。
///
/// 任何一步失败都返回 `Err(GpuUnavailable)`,调用方据此回退到
/// WebGL2 —— 绝不 panic:wasm 里没有 try/catch,一个抛异常的方法
/// 会毒化整个实例,游戏直接白屏。
///
/// # Arguments
///
/// - `&HtmlCanvasElement` - 游戏正在用的那块画布。
///
/// # Returns
///
/// - `Result<GpuContext, GpuUnavailable>` - 可渲染的上下文。
pub async fn acquire(canvas: &HtmlCanvasElement) -> Result<GpuContext, GpuUnavailable> {
    let window: euv::web_sys::Window =
        euv::web_sys::window().ok_or(GpuUnavailable::new(GPU_ERR_NO_NAVIGATOR))?;
    let navigator: euv::web_sys::Navigator = window.navigator();
    let gpu: JsValue = gpu_object(&navigator)?;
    let adapter: JsValue = match await_adapter(&gpu).await? {
        Some(adapter) => adapter,
        None => return Err(GpuUnavailable::new(GPU_ERR_NO_ADAPTER)),
    };
    let device: JsValue = match request_device(&adapter).await {
        Some(device) => device,
        None => return Err(GpuUnavailable::new(GPU_ERR_NO_DEVICE)),
    };
    let context: JsValue = canvas_context(canvas)?;

    // 首选格式必须在 configure 之前取:它是设备与画布之间的一次性约定。
    let format: String = preferred_format(&gpu)?;

    let config: Object = new_object();
    Reflect::set(&config, &JsValue::from_str(FIELD_DEVICE), &device)
        .map_err(|_: JsValue| GpuUnavailable::new(GPU_ERR_CONFIGURE_FAILED))?;
    Reflect::set(
        &config,
        &JsValue::from_str(FIELD_CONFIG_FORMAT),
        &JsValue::from_str(&format),
    )
    .map_err(|_: JsValue| GpuUnavailable::new(GPU_ERR_CONFIGURE_FAILED))?;
    Reflect::set(
        &config,
        &JsValue::from_str(FIELD_ALPHA_MODE),
        &JsValue::from_str(ALPHA_MODE_OPAQUE),
    )
    .map_err(|_: JsValue| GpuUnavailable::new(GPU_ERR_CONFIGURE_FAILED))?;
    let configure: euv::js_sys::Function = method(&context, "configure")
        .map_err(|_: String| GpuUnavailable::new(GPU_ERR_CONFIGURE_FAILED))?;
    configure
        .call1(&context, config.as_ref())
        .map_err(|_: JsValue| GpuUnavailable::new(GPU_ERR_CONFIGURE_FAILED))?;

    euv::web_sys::console::log_1(&JsValue::from_str(&format!(
        "[vcw-gpu-init] canvas={}x{} fmt={}",
        canvas.width(),
        canvas.height(),
        format
    )));

    Ok(GpuContext {
        context,
        device,
        format,
    })
}

/// `GPU.requestAdapter()` —— resolve 成 `null` 时返回 `None`。
///
/// **返回 `null` 的典型原因**是启动 flag:`--disable-gpu` 与
/// `--use-angle=swiftshader` 各自都会让适配器消失,而页面 / wasm /
/// `navigator.gpu` 全都正常。所以这个 `None` 必须区分于异常。
///
/// # Arguments
///
/// - `&JsValue` - `navigator.gpu`。
///
/// # Returns
///
/// - `Result<Option<JsValue>, GpuUnavailable>` - 适配器句柄。
async fn await_adapter(gpu: &JsValue) -> Result<Option<JsValue>, GpuUnavailable> {
    let request: euv::js_sys::Function = method(gpu, "requestAdapter")
        .map_err(|_: String| GpuUnavailable::new(GPU_ERR_NO_ADAPTER))?;
    let called: JsValue = request
        .call0(gpu)
        .map_err(|_: JsValue| GpuUnavailable::new(GPU_ERR_NO_ADAPTER))?;
    let promise: Promise =
        as_promise(&called).map_err(|_: String| GpuUnavailable::new(GPU_ERR_NO_ADAPTER))?;
    let resolved: JsValue = JsFuture::from(promise)
        .await
        .map_err(|_: JsValue| GpuUnavailable::new(GPU_ERR_NO_ADAPTER))?;
    if resolved.is_null() || resolved.is_undefined() {
        return Ok(None);
    }
    Ok(Some(resolved))
}

/// `GPUAdapter.requestDevice()` —— 返回 `null` / 抛异常时是 `None`。
///
/// # Arguments
///
/// - `&JsValue` - `GPUAdapter`。
///
/// # Returns
///
/// - `Option<JsValue>` - `GPUDevice` 句柄。
async fn request_device(adapter: &JsValue) -> Option<JsValue> {
    let request: euv::js_sys::Function = method(adapter, METHOD_REQUEST_DEVICE).ok()?;
    let promise: Promise = as_promise(&request.call0(adapter).ok()?).ok()?;
    let resolved: JsValue = JsFuture::from(promise).await.ok()?;
    if resolved.is_null() || resolved.is_undefined() {
        return None;
    }
    Some(resolved)
}

/// `GPU.getPreferredCanvasFormat()` —— 必须是设备自己的格式,
/// 否则 `configure` 会报 validation error。
///
/// # Arguments
///
/// - `&JsValue` - `navigator.gpu`。
///
/// # Returns
///
/// - `Result<String, GpuUnavailable>` - `bgra8unorm` 之类。
pub fn preferred_format(gpu: &JsValue) -> Result<String, GpuUnavailable> {
    let get_format: euv::js_sys::Function = method(gpu, METHOD_GET_PREFERRED_CANVAS_FORMAT)
        .map_err(|_: String| GpuUnavailable::new(GPU_ERR_CONFIGURE_FAILED))?;
    let value: JsValue = get_format
        .call0(gpu)
        .map_err(|_: JsValue| GpuUnavailable::new(GPU_ERR_CONFIGURE_FAILED))?;
    value
        .as_string()
        .ok_or(GpuUnavailable::new(GPU_ERR_CONFIGURE_FAILED))
}

/// `device.pushErrorScope("validation")`。
///
/// WebGPU 的着色器编译错误与绑定不匹配都是**异步**报上来的:不开
/// error scope,失败的表现只是「画面纯黑」,而 console 里一个字都没有。
/// 管线创建的前后各包一层,错误文本才有地方可去。
///
/// # Arguments
///
/// - `&JsValue` - `GPUDevice`。
/// - `&str` - scope 名(`validation`)。
pub fn push_error_scope(device: &JsValue, scope: &str) {
    if let Ok(function) = method(device, METHOD_PUSH_ERROR_SCOPE) {
        let _pushed: Result<JsValue, JsValue> = function.call1(device, &JsValue::from_str(scope));
    }
}

/// `device.popErrorScope()` —— 把结果打进 console。
///
/// `popErrorScope()` 返回 **Promise**,管线创建本身是同步的,拿不到
/// 同步结果。所以这里 `spawn_local` 一条异步链把 resolve 出来的
/// `GPUError.message` 打到 console,再由 CDP 收集 ——
/// 这是「validation 失败时到底报了什么」唯一可靠的读法。
///
/// # Arguments
///
/// - `&JsValue` - `GPUDevice`。
/// - `&str` - scope 名(用于日志前缀)。
pub fn pop_error_scope(device: &JsValue, scope: &str) {
    let Ok(function) = method(device, METHOD_POP_ERROR_SCOPE) else {
        return;
    };
    let Ok(promise) = function.call0(device) else {
        return;
    };
    let Ok(promise): Result<Promise, String> = as_promise(&promise) else {
        return;
    };
    let prefix: String = format!("[vcw] WebGPU {scope} scope: ");
    euv::wasm_bindgen_futures::spawn_local(async move {
        match JsFuture::from(promise).await {
            Ok(value) if value.is_null() || value.is_undefined() => {
                euv::web_sys::console::log_1(&JsValue::from_str(&format!("{prefix}clean")));
            }
            Ok(value) => {
                // ⚠️ **必须**显式读 `message`。`GPUError` 的 `message`
                // 是原型上的 getter,不是自有属性 —— `JSON.stringify`
                // 对它只会吐出 `{}`(实测),而那正是「管线到底哪里错了」
                // 唯一的线索。
                let message: String = property(&value, ERROR_FIELD_MESSAGE)
                    .ok()
                    .and_then(|text: JsValue| text.as_string())
                    .unwrap_or_else(|| describe(&value));
                euv::web_sys::console::error_1(&JsValue::from_str(&format!("{prefix}{message}")));
            }
            Err(error) => {
                euv::web_sys::console::error_1(&JsValue::from_str(&format!(
                    "{prefix}pop failed: {}",
                    describe(&error)
                )));
            }
        }
    });
}

/// 视投影矩阵的字节序(列主序,直接 `writeBuffer`)。
///
/// # Arguments
///
/// - `&Mat4` - 列主序视投影矩阵。
///
/// # Returns
///
/// - `[f32; 16]` - 可直接上传的字节序。
pub fn frame_bytes(view_proj: &Mat4) -> [f32; 16] {
    // ================================================================
    // GL → WebGPU 的 NDC 差异里,**只有深度这一处**需要在这里搬运:
    //
    //   **深度**: OpenGL 的 NDC z ∈ [-1, +1],WebGPU 是 [0, 1]。
    //   → 对每一列 c 做 `z' = 0.5·z + 0.5·w`(第 2、3 行)。
    //
    // ⚠️ **y 方向不要动。** 直觉上「WebGPU 的 NDC +Y 朝下、OpenGL 朝上,
    // 该翻 y」,但实测**翻了就是上下颠倒**(见下面循环体里的实测记录)。
    // 这套相机矩阵送进 WebGPU 时方向本来已经是对的,翻了等于翻两次。
    //
    // 验收手段:`tools/gpu-probe/profile-shot.py` 用**同一段代码、同样的
    // 阈值**去量已知正确的 WebGL2 对照帧和 WebGPU 帧的垂直剖面 ——
    // 正确的样子是「顶部天空%、底部沥青暗色%」,倒过来就是翻了。
    // ================================================================
    //
    // ⚠️ **必须**把 z 从 OpenGL 的 [-1, 1] 搬到 WebGPU 的 [0, 1]:
    // 对每一列 c 做 `z' = 0.5·z + 0.5·w`。
    //
    // 游戏的 `view_proj` 是给 WebGL2 用的,深度范围是 GL 的 [-w, +w];
    // WebGPU 的 NDC 深度是 [0, 1]。不搬的话 `clip.z < 0` 的顶点会被
    // **近平面直接裁掉**,而 `clip.z > clip.w` 的又会被远平面裁掉 ——
    // 整个场景落在可视深度区间之外,画布只剩清屏色(天空),一个三角形
    // 都没有,而 validation 依然是 clean。
    //
    // ⚠️⚠️ **索引必须是「第 2 行」「第 3 行」,不是「第 2 列」「第 3 列」。**
    //
    // 本仓库的 `Mat4` 是**列主序**([`crate::render`] 里所有
    // `Mat4::from_column_major` 都是证据),所以
    //
    //     M[row][col] == elements[col * 4 + row]
    //
    // 也就是说:
    //   - 第 `c` 列 = 下标 `c*4 .. c*4+3`
    //   - 第 2 行  = 下标 **2, 6, 10, 14**
    //   - 第 3 行  = 下标 **3, 7, 11, 15**
    //
    // ⚠️⚠️⚠️ **Y 也必须翻。**
    //
    // WebGL 的 NDC 是 **+Y 朝上**,WebGPU 的 NDC 是 **+Y 朝下**。投影矩阵
    // 是给 WebGL 用的,直接送进 WebGPU 会让整幅画面**上下颠倒**。
    // 做法是把第 1 行整体取反(`y' = -y`)。
    //
    // ⚠️ 翻转 Y 会**同时**把三角形绕序反过来:原来在 WebGL 里逆时针
    // (正面)的三角形,取反后在 WebGPU 里变成顺时针。所以 pipeline 的
    // `frontFace` 必须**跟着一起**改回 `"ccw"`(WebGPU 的默认值)——
    // 只改其中一个,画面就会变成「从建筑内部往外看」的反面几何。
    //
    // 曾经写 `out[8 + column]` / `out[12 + column]`,那取到的是
    // **第 2 列和第 3 列**(`8,9,10,11` 与 `12,13,14,15`)。后果是
    // 每个**行**被改了**一个元素**,而真正要改的两行一个都没碰到:
    // 对 proj·view 来说 `M[0][3] = M[1][3] = 0`、`M[3][3] = 1`,
    // 于是
    //
    //     out[8] = 0.5·M[0][2] + 0.5·M[0][3] = 0.5·M[0][2]
    //     out[9] = 0.5·M[1][2] + 0.5·M[1][3] = 0.5·M[1][2]
    //
    // 即 **clip.x 与 clip.y 的 z 系数被砍半**,而投影矩阵里那个 `-1`
    // (下标 15)原封不动 —— 结果整个世界朝屏幕中线压扁成一条横带,
    // 画面上只剩一条挤扁的地平线色带。`#[test]` 现在把这个钉住。
    let source: [f32; 16] = *view_proj.get_elements();
    let mut out: [f32; 16] = source;
    for column in 0..4usize {
        let z_row: usize = 2 + column * 4;
        let w_row: usize = 3 + column * 4;
        out[z_row] = 0.5 * source[z_row] + 0.5 * source[w_row];
        // ⚠️⚠️⚠️ **第 1 行(y)**:**不能**取反。
        //
        // 直觉上「WebGPU 的 NDC +Y 朝下、OpenGL 朝上,所以该翻 y」,但
        // 实测是反的。证据(`tools/gpu-probe/profile-shot.py` 的垂直剖面,
        // 与**已知正确**的 WebGL2 对照帧用同一段代码、同样的阈值量):
        //
        //   翻了 y + frontFace=ccw  ->  顶部暗、底部天空  = **上下颠倒**
        //   不翻 y + frontFace=ccw  ->  顶部天空、底部沥青 = **正确**
        //
        // 也就是说这套相机矩阵送进 WebGPU 时,画面方向本来就对。原因
        // 是 `Camera::perspective` 用的已经是适合 WebGPU 的约定,GL/WebGPU
        // 的 y 差异在这一层已经被抵消掉了 —— 再翻一次等于翻两次。
        //
        // ⚠️ 所以**只有 z 这一处**需要搬运,不要动 y。
    }
    out
}

/// 把 [`SceneLighting`] 摊进 WGSL 期望的 vec4 布局。
///
/// 阴影那几项来自 WebGL2 端的 `u_shadow_params` / `u_shadow_texel`
/// 两个 uniform,这里合成两个 vec4 上传,顺序与 GLSL 完全一致 ——
/// **不要**调换,换了偏置与法线偏移会互换,症状是「影子整体浮起来」
/// 或「满屏痤疮」。
///
/// # Arguments
///
/// - `&SceneLighting` - 当前相位的光照参数。
/// - `Vec3` - 相机眼点(世界坐标)。
///
/// # Returns
///
/// - `ShadingUniforms` - 填充好的 uniform 块。
pub fn shading_from_lighting(lighting: &SceneLighting, eye: Vec3) -> ShadingUniforms {
    ShadingUniforms {
        light_dir: pad3(lighting.light_dir),
        light_color: pad3(lighting.light_color),
        ambient: pad3(lighting.ambient),
        sky_color: pad3(lighting.sky_color),
        sky_ambient: pad3(lighting.sky_ambient),
        ground_ambient: pad3(lighting.ground_ambient),
        ambient_hemi: [lighting.ambient_hemi, 0.0, 0.0, 0.0],
        emissive_gain: [lighting.emissive_gain, 0.0, 0.0, 0.0],
        eye: pad3(eye),
        fog: [lighting.fog_start, lighting.fog_end, 0.0, 0.0],
        ao_params: [BAKED_CONTACT_AO_HEIGHT, CONTACT_SHADOW_FLOOR, 0.0, 0.0],
        exposure_white: [lighting.exposure, lighting.tone_map_white, 0.0, 0.0],
        // 纹素尺寸与偏置倍数都从 `render.rs` 的那三个函数取,不在这里
        // 另立一套常量 —— 两个后端必须是同一个数,否则同一个场景两边
        // 影子形状不同。
        shadow_params: [
            lighting.shadow_strength,
            SHADOW_PCF_RADIUS,
            SHADOW_DEPTH_BIAS_SCALE,
            SHADOW_NORMAL_OFFSET_SCALE,
        ],
        shadow_misc: [
            crate::render::shadow_texel_world_size(),
            SHADOW_DEPTH_SPAN_M,
            SHADOW_BIAS_SLOPE_GAIN,
            SHADOW_NORMAL_OFFSET_SLOPE_GAIN,
        ],
    }
}

/// 把 3 分量向量补成 4 分量(WGSL 的 `vec4` 对齐要求)。
///
/// # Arguments
///
/// - `Vec3` - 输入值。
///
/// # Returns
///
/// - `[f32; 4]` - 末位补 0 的四分量。
pub fn pad3(value: Vec3) -> [f32; 4] {
    [value[0], value[1], value[2], 0.0]
}

/// WebGPU 不可用的各种原因,用作 `Result` 的错误文本。
/// 从 JS 对象读一个**属性**(不是方法)。
///
/// `device.queue` 这类成员必须走这里:拿 `call0` 去调它会得到
/// `queue is not a function`。
///
/// # Arguments
///
/// - `&JsValue` - 承载属性的 JS 对象。
/// - `&str` - 属性名。
///
/// # Returns
///
/// - `Result<JsValue, String>` - 属性值。
pub fn property(target: &JsValue, name: &str) -> Result<JsValue, String> {
    Reflect::get(target, &JsValue::from_str(name))
        .map_err(|error: JsValue| format!("{}: {}", name, describe(&error)))
}

/// 从 JS 值里取出一个可调用的成员函数。
///
/// WebGPU 的方法走 JS 的抛异常模型,而 wasm 里没有 try/catch ——
/// 一个方法抛异常会直接毒化整个 wasm 实例。所以每次调用都先确认
/// 目标真的是函数,取不到就把错误交回给调用方决定回退。
///
/// # Arguments
///
/// - `&JsValue` - 承载方法的 JS 对象。
/// - `&'static str` - 方法名.
///
/// # Returns
///
/// - `Result<Function, String>` - 方法句柄。
pub fn method(target: &JsValue, name: &'static str) -> Result<Function, String> {
    let value: JsValue = Reflect::get(target, &JsValue::from_str(name))
        .map_err(|error: JsValue| format!("{}: {}", name, describe(&error)))?;
    value
        .dyn_into::<Function>()
        .map_err(|_| format!("{name} is not a function"))
}

/// 把任意 `JsValue` 变成一行可读文本(用于把 JS 异常写进日志)。
///
/// # Arguments
///
/// - `&JsValue` - 待描述的值。
///
/// # Returns
///
/// - `String` - 可读文本。
pub fn describe(value: &JsValue) -> String {
    if let Some(text) = value.as_string() {
        return text;
    }
    match JSON::stringify(value) {
        Ok(text) => text.as_string().unwrap_or_default(),
        Err(_) => String::from("<unprintable>"),
    }
}

/// 建一个空的 JS 对象(字典形式的 `configure` 参数 / descriptor)。
///
/// # Returns
///
/// - `Object` - 空对象。
pub fn new_object() -> Object {
    Object::new()
}

/// 把任意 `JsValue` 当 `Promise` 引用取用。
///
/// `JsFuture::from` 只接受 `Promise`,而 WebGPU 的
/// `requestAdapter` / `requestDevice` 在 Rust 侧先是 `JsValue`。
///
/// # Arguments
///
/// - `&JsValue` - 待转换的值。
///
/// # Returns
///
/// - `Result<euv::js_sys::Promise, String>` - Promise 句柄。
pub fn as_promise(value: &JsValue) -> Result<euv::js_sys::Promise, String> {
    value
        .clone()
        .dyn_into::<euv::js_sys::Promise>()
        .map_err(|_| String::from("value is not a Promise"))
}

/// `navigator.gpu` 的句柄 —— 没有它就没有 WebGPU。
///
/// # Arguments
///
/// - `&Navigator` - 浏览器 navigator。
///
/// # Returns
///
/// - `Result<JsValue, GpuUnavailable>` - `GPU` 对象。
pub fn gpu_object(navigator: &Navigator) -> Result<JsValue, GpuUnavailable> {
    Reflect::get(navigator, &JsValue::from_str(PROP_GPU))
        .map_err(|_: JsValue| GpuUnavailable::new(GPU_ERR_NO_GPU_OBJECT))
}

/// 画布的 GPU 上下文句柄。
///
/// # Arguments
///
/// - `&HtmlCanvasElement` - 目标画布。
///
/// # Returns
///
/// - `Result<JsValue, GpuUnavailable>` - `GPUCanvasContext`。
pub fn canvas_context(canvas: &HtmlCanvasElement) -> Result<JsValue, GpuUnavailable> {
    let result: Result<Option<Object>, JsValue> = canvas.get_context(CONTEXT_WEBGPU);
    let value: Option<Object> =
        result.map_err(|_: JsValue| GpuUnavailable::new(GPU_ERR_NO_CONTEXT))?;
    match value {
        Some(context) => Ok(context.into()),
        None => Err(GpuUnavailable::new(GPU_ERR_NO_CONTEXT)),
    }
}

/// 展平着色参数 uniform 成 56 个 f32。
///
/// # Arguments
///
/// - `ShadingUniforms` - 结构形态的 uniform。
///
/// # Returns
///
/// - `[f32; 56]` - 可直接上传的字节序。
pub fn flatten_shading(shading: ShadingUniforms) -> [f32; 56] {
    let blocks: [[f32; 4]; SHADING_VEC4_COUNT] = [
        shading.light_dir,
        shading.light_color,
        shading.ambient,
        shading.sky_color,
        shading.sky_ambient,
        shading.ground_ambient,
        shading.ambient_hemi,
        shading.emissive_gain,
        shading.eye,
        shading.fog,
        shading.ao_params,
        shading.exposure_white,
        shading.shadow_params,
        shading.shadow_misc,
    ];
    let mut out: [f32; 56] = [0.0; 56];
    for (index, block) in blocks.iter().enumerate() {
        let base: usize = index * 4;
        out[base] = block[0];
        out[base + 1] = block[1];
        out[base + 2] = block[2];
        out[base + 3] = block[3];
    }
    out
}

/// 展平 bloom 参数 uniform 成 `BLOOM_VEC4_COUNT` × 4 个 f32。
///
/// 与 [`flatten_shading`] 同一种做法:WebGPU 的 uniform 按 16 字节
/// 对齐,所以每块都刚好是 4 个 f32,下标可以手算。
///
/// # Arguments
///
/// - `BloomUniforms` - 结构形态的 uniform。
///
/// # Returns
///
/// - `[f32; 12]` - 可直接上传的字节序。
pub fn flatten_bloom(bloom: BloomUniforms) -> [f32; 12] {
    let blocks: [[f32; 4]; BLOOM_VEC4_COUNT] = [bloom.params, bloom.kernel, bloom.texel];
    let mut out: [f32; 12] = [0.0; 12];
    for (index, block) in blocks.iter().enumerate() {
        let base: usize = index * 4;
        out[base] = block[0];
        out[base + 1] = block[1];
        out[base + 2] = block[2];
        out[base + 3] = block[3];
    }
    out
}

/// bloom 三张半分辨率目标的尺寸(相对画布)。
///
/// # Arguments
///
/// - `u32` - 画布宽(像素)。
/// - `u32` - 画布高(像素)。
///
/// # Returns
///
/// - `(u32, u32)` - bloom 目标宽高,至少 1 像素见方。
///
/// 与 WebGL2 端 `ensure_targets` 那个 `scaled(width, BLOOM_SCALE)`
/// **同一个缩放系数**(`BLOOM_SCALE = 0.5`),而且同样走
/// `.max(1.0)`:0 像素高的目标是 `createTexture` 会被拒。
pub fn bloom_target_size(width: u32, height: u32) -> (u32, u32) {
    let scaled: fn(u32, f32) -> u32 =
        |base: u32, factor: f32| (((base as f32) * factor).max(1.0)) as u32;
    (scaled(width, BLOOM_SCALE), scaled(height, BLOOM_SCALE))
}

/// 全空的 bloom uniform buffer 句柄数组(建之前的状态)。
///
/// # Returns
///
/// - `[JsValue; BLOOM_PASS_COUNT]` - 每个元素都是 `JsValue::NULL`。
pub fn empty_bloom_buffers() -> [JsValue; BLOOM_PASS_COUNT] {
    [JsValue::NULL; BLOOM_PASS_COUNT]
}

/// 全空的 bloom pipeline / bind group 配对数组(建之前的状态)。
///
/// ⚠️ `array::from_fn` 从 [`crate::webgpu`] 的 `mod.rs` 统一 import
/// (§6.4 要求 import 集中在那里,§1.3 又不写 `std::` 限定路径,所以走
/// crate 根的 `::core`)。这里能直接写 `array::from_fn` 是因为
/// [`super::mod`] 已经把它 re-export 出来了。
///
/// # Returns
///
/// - `[GpuBloomPass; BLOOM_PASS_COUNT]` - 每条的 pipeline 与 bind group 都是 `JsValue::NULL`。
pub fn empty_bloom_passes() -> [GpuBloomPass; BLOOM_PASS_COUNT] {
    // ⚠️ 用 `from_fn` 而不是 `[expr; N]` 重复式:后者要求元素是 `Copy`,
    // 而 `GpuBloomPass` 只 `Clone` 不 `Copy`(字段是 `JsValue`)。
    // 这正是那条编译期类型标注约束在起作用的地方。
    array::from_fn(|_index: usize| GpuBloomPass {
        pipeline: JsValue::NULL,
        bind_group: JsValue::NULL,
    })
}

/// bloom 高斯核的**总权重**。
///
/// 这个 5 抽头核在模糊时每个 `i ∈ 1..4` 取**两次**(`uv ± offset`),
/// 只有中心抽头取一次,所以总权重是 `w0 + 2 × (w1+w2+w3+w4)` 而不是
/// 五个数直接相加。
///
/// # Returns
///
/// - `f32` - 总权重,归一化时恰好 1.0。
///
/// ⚠️ 这是**纯 CPU 端**的核验,GPU 上跑的那份权重是从
/// [`BLOOM_BLUR_WEIGHTS`] 上传过去的同一组数 —— 所以两者不可能漂移。
pub fn bloom_blur_kernel_total() -> f32 {
    let weights: [f32; 5] = BLOOM_BLUR_WEIGHTS;
    // 中心抽头只取一次,其余四个各取两次(`uv ± offset`) —— 所以
    // `split_at(1)` 正好把「取一次的那一个」与「取两次的那四个」分开。
    let (center, taps): (&[f32], &[f32]) = weights.split_at(1);
    let mut total: f32 = center[0];
    for weight in taps.iter() {
        total += 2.0 * weight;
    }
    total
}

/// 合成 pass 的辉光强度。
///
/// 与 WebGL2 端 `render_composite` 里那个 uniform **逐项相同**:
/// `BLOOM_STRENGTH * emissive_gain.max(BLOOM_MIN_GAIN)`。
///
/// ⚠️ `max` 那个下限不能省:正午相位的 `emissive_gain` 只有 0.18,
/// 低于 0.35 的下限 —— 少了 `max` 的话正午的辉光强度会掉到
/// `0.85 × 0.18`,而黄昏帧**完全正常**,只有切到正午才暴露。
///
/// # Arguments
///
/// - `&SceneLighting` - 当前光照。
///
/// # Returns
///
/// - `f32` - 传给 `bloom.params.y` 的辉光强度。
pub fn bloom_composite_strength(lighting: &SceneLighting) -> f32 {
    BLOOM_STRENGTH * lighting.emissive_gain.max(BLOOM_MIN_GAIN)
}

/// 组装一条 bloom pass 的 uniform。
///
/// 四条 pass **共用这一个形状**(WGSL 那个 `BloomParams` 的布局是固定的),
/// 区别只在填进去的数值:提取填阈值、两条模糊各填一个方向、合成填强度。
/// 与 WebGL2 端同一个 program 靠 `u_direction` 切换水平 / 垂直的做法一致。
///
/// # Arguments
///
/// - `f32` - `params.x`:亮度阈值(提取用)。
/// - `f32` - `params.y`:合成强度(合成用)。
/// - `(f32, f32)` - `params.zw`:模糊方向(uv 单位)。
/// - `(u32, u32)` - 这条 pass **读写的那张目标**的尺寸,用来算纹素大小。
///
/// # Returns
///
/// - `BloomUniforms` - 可直接 [`flatten_bloom`] 的 uniform。
///
/// ⚠️ `size` 必须是**目标本身**的尺寸而不是画布尺寸:两条模糊跑在
/// 半分辨率目标上,一个纹素覆盖 2 个全分辨率像素。传错的话模糊步长
/// 会大一倍,光晕看起来虚了一圈。
pub fn bloom_uniforms(
    threshold: f32,
    strength: f32,
    direction: (f32, f32),
    size: (u32, u32),
) -> BloomUniforms {
    let weights: [f32; 5] = BLOOM_BLUR_WEIGHTS;
    // ⚠️ **上传前按实际总权重归一化。** 模糊是「采样 × 加权求和」,
    // 总权重小于 1 就等于每过一趟把画面压暗 1 − total,两次之后光晕
    // 整体塌掉一截。归一化让「有人改了某个权重却忘了重新配平」这个
    // 改动在**亮度上完全无害**(只会改变光晕的形状)。当前这组权重
    // 本来就归一化(total ≈ 0.9999994),所以这一步对现值是恒等变换 ——
    // 它防的是下一次改动。
    let total: f32 = bloom_blur_kernel_total();
    let scale: f32 = if total > 0.0 { 1.0 / total } else { 1.0 };
    let normalized: [f32; 5] = weights.map(|weight: f32| weight * scale);
    let packed: (&[f32], &[f32]) = normalized.split_at(4);
    BloomUniforms {
        params: [threshold, strength, direction.0, direction.1],
        kernel: [packed.0[0], packed.0[1], packed.0[2], packed.0[3]],
        // `x` = 第 5 个权重(WGSL 那边 `kernel_weight(4)` 从这里取),
        // `yz` = 一个纹素覆盖的 uv 尺寸。
        texel: [
            packed.1[0],
            1.0 / (size.0.max(1) as f32),
            1.0 / (size.1.max(1) as f32),
            0.0,
        ],
    }
}

/// 实例模型矩阵原点到眼点的距离(供近处剔除用)。
///
/// # Arguments
///
/// - `&crate::render::Instance` - 实例。
/// - `Vec3` - 眼点。
///
/// # Returns
///
/// - `f32` - 距离(米)。
pub fn distance_to(instance: &crate::render::Instance, eye: Vec3) -> f32 {
    let m: &crate::r#type::Mat4Data = instance.get_model_ref();
    let dx: f32 = m[12] - eye[0];
    let dy: f32 = m[13] - eye[1];
    let dz: f32 = m[14] - eye[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

// ===========================================================================
// 以下是纯 JS 分发的薄封装(全部经 js_sys::Reflect,无 unstable cfg)
// ===========================================================================

/// 在 JS 对象上设置一个字段。
///
/// # Arguments
///
/// - `&Object` - 目标对象。
/// - `&str` - 字段名。
/// - `&JsValue` - 字段值。
///
/// # Returns
///
/// - `Result<(), String>` - 设置失败时的错误文本。
pub fn set(target: &Object, name: &str, value: &JsValue) -> Result<(), String> {
    Reflect::set(target, &JsValue::from_str(name), value)
        .map_err(|_| format!("Reflect::set({name}) failed"))
        .map(|_: bool| ())
}

/// 调一个不取参数的 JS 方法。
///
/// # Arguments
///
/// - `&JsValue` - 承载方法的对象(同时是 `this`)。
/// - `&'static str` - 方法名。
///
/// # Returns
///
/// - `Result<JsValue, String>` - 返回值。
pub fn call0(target: &JsValue, name: &'static str) -> Result<JsValue, String> {
    let function: Function = method(target, name)?;
    function
        .call0(target)
        .map_err(|error: JsValue| format!("{name}() threw: {}", describe(&error)))
}

/// 调一个取一个参数的 JS 方法。
///
/// # Arguments
///
/// - `&JsValue` - 承载方法的对象(同时是 `this`)。
/// - `&'static str` - 方法名。
/// - `&JsValue` - 唯一参数。
///
/// # Returns
///
/// - `Result<JsValue, String>` - 返回值。
pub fn call1(target: &JsValue, name: &'static str, arg: &JsValue) -> Result<JsValue, String> {
    let function: Function = method(target, name)?;
    function
        .call1(target, arg)
        .map_err(|error: JsValue| format!("{name}() threw: {}", describe(&error)))
}

/// 建一个空 JS 数组。
///
/// # Returns
///
/// - `JsValue` - 空数组。
pub fn make_array() -> JsValue {
    Array::new().into()
}

/// 把一个值按下标塞进 JS 数组。
///
/// # Arguments
///
/// - `&JsValue` - 数组。
/// - `usize` - 下标。
/// - `&JsValue` - 值。
///
/// # Returns
///
/// - `Result<(), String>` - 写入失败时的错误文本。
pub fn push_into(array: &JsValue, index: usize, value: &JsValue) -> Result<(), String> {
    // ⚠️ **必须**用 `Array.prototype.push`,不能只 `Reflect::set` 下标。
    // `array[i] = x` 会建出一个「类数组」的普通对象:它有下标、有
    // `length`,但**没有可调用的 `@@iterator`**。而 WebGPU 把所有
    // sequence 参数都声明成可迭代对象(`createBindGroupLayout` 的
    // `entries`、`createRenderPipeline` 的 `targets`、render pass 的
    // `colorAttachments`…),实测直接报:
    //
    //   Failed to read the 'entries' property from
    //   'GPUBindGroupLayoutDescriptor': The object must have a callable
    //   @@iterator property.
    //
    // 只有真正的 JS Array 才带迭代器。这里显式 push(下标仍然断言等于
    // 期望值,防止调用点自己排错了顺序)。
    let Ok(push) = method(array, METHOD_PUSH) else {
        return Err(String::from("array.push is not callable"));
    };
    push.call1(array, value)
        .map_err(|error: JsValue| format!("array.push({index}) failed: {}", describe(&error)))?;
    let length: JsValue = Reflect::get(array, &JsValue::from_str(FIELD_LENGTH))
        .map_err(|_| String::from("array.length unreadable"))?;
    let pushed: usize = length.as_f64().unwrap_or(-1.0) as usize;
    if pushed != index + 1 {
        return Err(format!(
            "array.push order wrong: expected len {}, got {pushed}",
            index + 1
        ));
    }
    Ok(())
}

/// 建一个 `{r, g, b, a}` 的清屏色。
///
/// # Arguments
///
/// - `f32` - 红。
/// - `f32` - 绿。
/// - `f32` - 蓝。
///
/// # Returns
///
/// - `JsValue` - 清屏色对象。
pub fn make_rgba(red: f32, green: f32, blue: f32) -> JsValue {
    let color: Object = new_object();
    let channels: [(f32, &str); 4] = [
        (red, CHANNEL_R),
        (green, CHANNEL_G),
        (blue, CHANNEL_B),
        (1.0, CHANNEL_A),
    ];
    for (value, key) in channels {
        let _written: Result<(), String> = set(&color, key, &JsValue::from_f64(value as f64));
    }
    color.into()
}

/// 建一个 `{width, height}` 的尺寸对象。
///
/// # Arguments
///
/// - `u32` - 宽。
/// - `u32` - 高。
///
/// # Returns
///
/// - `JsValue` - 尺寸对象。
pub fn make_extent(width: u32, height: u32) -> JsValue {
    let extent: Object = new_object();
    let _w: Result<(), String> = set(&extent, CHANNEL_WIDTH, &JsValue::from_f64(width as f64));
    let _h: Result<(), String> = set(&extent, CHANNEL_HEIGHT, &JsValue::from_f64(height as f64));
    extent.into()
}

/// 顶点 + 片元两个阶段的可见性位。
///
/// # Returns
///
/// - `u32` - `GPUShaderStage` 位掩码。
pub fn visibility_vertex_fragment() -> u32 {
    VISIBILITY_VERTEX | VISIBILITY_FRAGMENT
}

/// `device.createShaderModule({code})`。
///
/// # Arguments
///
/// - `&JsValue` - `GPUDevice`。
/// - `&str` - WGSL 源码。
///
/// # Returns
///
/// - `Result<JsValue, String>` - `GPUShaderModule`。
pub fn create_shader_module(device: &JsValue, code: &str) -> Result<JsValue, String> {
    let descriptor: Object = new_object();
    set(&descriptor, FIELD_CODE, &JsValue::from_str(code))?;
    call1(device, METHOD_CREATE_SHADER_MODULE, descriptor.as_ref())
}

/// `device.createBuffer({size, usage})`。
///
/// # Arguments
///
/// - `&JsValue` - `GPUDevice`。
/// - `usize` - 字节数。
/// - `u32` - usage 位掩码。
///
/// # Returns
///
/// - `Result<JsValue, String>` - `GPUBuffer`。
pub fn create_buffer(device: &JsValue, size: usize, usage: u32) -> Result<JsValue, String> {
    let descriptor: Object = new_object();
    set(&descriptor, FIELD_SIZE, &JsValue::from_f64(size as f64))?;
    set(
        &descriptor,
        FIELD_USAGE,
        &JsValue::from_f64(f64::from(usage)),
    )?;
    call1(device, METHOD_CREATE_BUFFER, descriptor.as_ref())
}

/// `device.queue.writeBuffer(buffer, offset, bytes)`。
///
/// # Arguments
///
/// - `&JsValue` - `GPUDevice`。
/// - `&JsValue` - 目标 buffer。
/// - `f64` - 字节偏移。
/// - `&[u8]` - 要写入的字节。
///
/// # Returns
///
/// - `Result<(), String>` - 写入失败时的错误文本。
pub fn write_buffer(
    device: &JsValue,
    buffer: &JsValue,
    offset: f64,
    bytes: &[u8],
) -> Result<(), String> {
    if buffer.is_null() {
        return Err(String::from(WRITE_BUFFER_NULL));
    }
    // `GPUDevice.queue` 是属性,不是方法(见上面的 `submit`)。
    let queue: JsValue = property(device, METHOD_QUEUE)?;
    let writer: Function = method(&queue, METHOD_WRITE_BUFFER)?;
    let view: Uint8Array = Uint8Array::from(bytes);
    writer
        .call3(&queue, buffer, &JsValue::from_f64(offset), view.as_ref())
        .map(|_: JsValue| ())
        .map_err(|error: JsValue| format!("writeBuffer threw: {}", describe(&error)))
}

/// `texture.createView()`。
///
/// # Arguments
///
/// - `&JsValue` - `GPUTexture`。
///
/// # Returns
///
/// - `Result<JsValue, String>` - `GPUTextureView`。
pub fn create_view(texture: &JsValue) -> Result<JsValue, String> {
    call0(texture, METHOD_CREATE_VIEW)
}

/// `pass.setBindGroup(index, group)`。
///
/// # Arguments
///
/// - `&JsValue` - render pass。
/// - `u32` - group 下标。
/// - `&JsValue` - bind group。
///
/// # Returns
///
/// - `Result<(), String>` - 失败时的错误文本。
pub fn set_bind_group(pass: &JsValue, index: u32, group: &JsValue) -> Result<(), String> {
    let function: Function = method(pass, METHOD_SET_BIND_GROUP)?;
    function
        .call2(pass, &JsValue::from_f64(index as f64), group)
        .map(|_: JsValue| ())
        .map_err(|error: JsValue| format!("setBindGroup threw: {}", describe(&error)))
}

/// `pass.setVertexBuffer(slot, buffer)`。
///
/// # Arguments
///
/// - `&JsValue` - render pass。
/// - `u32` - 槽位号。
/// - `&JsValue` - 顶点缓冲。
///
/// # Returns
///
/// - `Result<(), String>` - 失败时的错误文本。
pub fn set_vertex_buffer(pass: &JsValue, slot: u32, buffer: &JsValue) -> Result<(), String> {
    let function: Function = method(pass, METHOD_SET_VERTEX_BUFFER)?;
    function
        .call2(pass, &JsValue::from_f64(slot as f64), buffer)
        .map(|_: JsValue| ())
        .map_err(|error: JsValue| format!("setVertexBuffer threw: {}", describe(&error)))
}

/// `pass.setIndexBuffer(buffer, "uint32")`。
///
/// # Arguments
///
/// - `&JsValue` - render pass。
/// - `&JsValue` - 索引缓冲。
///
/// # Returns
///
/// - `Result<(), String>` - 失败时的错误文本。
pub fn set_index_buffer(pass: &JsValue, buffer: &JsValue) -> Result<(), String> {
    let function: Function = method(pass, METHOD_SET_INDEX_BUFFER)?;
    function
        .call2(pass, buffer, &JsValue::from_str(INDEX_FORMAT_UINT32))
        .map(|_: JsValue| ())
        .map_err(|error: JsValue| format!("setIndexBuffer threw: {}", describe(&error)))
}

/// `pass.drawIndexed(...)`。
///
/// # Arguments
///
/// - `&JsValue` - render pass。
/// - `u32` - 索引数。
/// - `u32` - 实例数。
/// - `u32` - 首个索引。
/// - `u32` - 顶点偏移。
///
/// # Returns
///
/// - `Result<(), String>` - 失败时的错误文本。
pub fn draw_indexed(
    pass: &JsValue,
    index_count: u32,
    instances: u32,
    first_index: u32,
    base_vertex: u32,
    first_instance: u32,
) -> Result<(), String> {
    let function: Function = method(pass, METHOD_DRAW_INDEXED)?;
    // ⚠️ `drawIndexed` 有 **6** 个参数(indexCount / instanceCount /
    // firstIndex / baseVertex / firstInstance),但 wasm-bindgen 的
    // `callN` 把 `this` 算作第一个参数,所以只能到 `call6`(总共 7 个
    // 位置)。最后一个参数 `firstInstance` 用 `Reflect::apply` 传。
    let arguments: euv::js_sys::Array = euv::js_sys::Array::new();
    for value in [
        index_count,
        instances,
        first_index,
        base_vertex,
        first_instance,
    ] {
        arguments.push(&JsValue::from_f64(f64::from(value)));
    }
    function
        .apply(pass, &arguments)
        .map(|_: JsValue| ())
        .map_err(|error: JsValue| format!("drawIndexed threw: {}", describe(&error)))
}
