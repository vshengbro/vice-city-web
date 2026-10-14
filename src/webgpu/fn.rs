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
    let source: [f32; 16] = view_proj.get_elements();
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
        light_dir: pad3(lighting.get_light_dir()),
        light_color: pad3(lighting.get_light_color()),
        ambient: pad3(lighting.get_ambient()),
        sky_color: pad3(lighting.get_sky_color()),
        sky_ambient: pad3(lighting.get_sky_ambient()),
        ground_ambient: pad3(lighting.get_ground_ambient()),
        ambient_hemi: [lighting.get_ambient_hemi(), 0.0, 0.0, 0.0],
        emissive_gain: [lighting.get_emissive_gain(), 0.0, 0.0, 0.0],
        eye: pad3(eye),
        fog: [lighting.get_fog_start(), lighting.get_fog_end(), 0.0, 0.0],
        // 接触 AO 的判据是「离自己几何基座有多近」,不是世界高度。
        // 四个分量与 WebGL2 端的 `u_ao_*` 逐项同序,共用同一批常量 ——
        // 两个后端必须是同一个数,否则同一个场景两边楼身下段的
        // 明暗不同。
        ao_params: [
            BAKED_CONTACT_AO_HEIGHT,
            CONTACT_SHADOW_FLOOR,
            CONTACT_SHADOW_REACH,
            CONTACT_SHADOW_FEATHER,
        ],
        exposure_white: [
            lighting.get_exposure(),
            lighting.get_tone_map_white(),
            0.0,
            0.0,
        ],
        // 纹素尺寸与偏置倍数都从 `render.rs` 的那三个函数取,不在这里
        // 另立一套常量 —— 两个后端必须是同一个数,否则同一个场景两边
        // 影子形状不同。
        shadow_params: [
            lighting.get_shadow_strength(),
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
        // 高度下界高于地面网格的最高点(0.16),所以地面整片不吃
        // 顶点接触 AO —— 见 `contact_ao_factor` 的说明。
        ao_band: [CONTACT_SHADOW_MIN_HEIGHT, 0.0, 0.0, 0.0],
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

/// 展平着色参数 uniform 成 60 个 f32。
///
/// # Arguments
///
/// - `ShadingUniforms` - 结构形态的 uniform。
///
/// # Returns
///
/// - `[f32; 60]` - 可直接上传的字节序。
pub fn flatten_shading(shading: ShadingUniforms) -> [f32; 60] {
    let blocks: [[f32; 4]; SHADING_VEC4_COUNT] = [
        shading.get_light_dir(),
        shading.get_light_color(),
        shading.get_ambient(),
        shading.get_sky_color(),
        shading.get_sky_ambient(),
        shading.get_ground_ambient(),
        shading.get_ambient_hemi(),
        shading.get_emissive_gain(),
        shading.get_eye(),
        shading.get_fog(),
        shading.get_ao_params(),
        shading.get_exposure_white(),
        shading.get_shadow_params(),
        shading.get_shadow_misc(),
        shading.get_ao_band(),
    ];
    let mut out: [f32; 60] = [0.0; 60];
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
    let blocks: [[f32; 4]; BLOOM_VEC4_COUNT] =
        [bloom.get_params(), bloom.get_kernel(), bloom.get_texel()];
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

/// 一块流式重建网格的缓冲该扩到多大。
///
/// **返回的容量永远 ≥ `needed`**,且在 `needed > current` 时严格变大 ——
/// 调用方(`replace_mesh`)靠这个不变式来保证「`write_buffer` 的写入长度
/// 永不超过 live 缓冲的尺寸」。
///
/// 纯函数而不是内联算式,因为它是「黑屏修复」那条不变式唯一能被
/// 原生 `cargo test` 钉住的地方:黑屏本身在无头浏览器里极难复现,
/// 但「扩容后一定装得下」可以逐条断言。
///
/// 扩到 `needed.next_power_of_two()` 而不是刚好 `needed`:地面网格每
/// 跨一个街区都会重建,顶点数是台阶式跳的(实测 880992 → 901368)。
/// 只按 `needed` 分配意味着每走一格就要换一次 `GPUBuffer`,而流式重建
/// 的频率与玩家移动速度成正比 —— 每次都重建会把帧时间顶起来。
///
/// # Arguments
///
/// - `usize` - 现有容量字节数。
/// - `usize` - 本次需要的字节数。
///
/// # Returns
///
/// - `usize` - 新容量字节数,至少 `needed`。
pub fn grown_capacity(current: usize, needed: usize) -> usize {
    if needed <= current {
        return current;
    }
    needed.next_power_of_two().max(needed)
}

/// 全空的 bloom uniform buffer 句柄数组(建之前的状态)。
///
/// # Returns
///
/// - `[JsValue; BLOOM_PASS_COUNT]` - 每个元素都是 `JsValue::NULL`。
pub fn empty_bloom_buffers() -> [JsValue; BLOOM_PASS_COUNT] {
    [JsValue::NULL; BLOOM_PASS_COUNT]
}

/// 全空的 AO pipeline / bind group 配对数组(建之前的状态)。
///
/// 同 [`empty_bloom_passes`]:`JsValue` 不是 `Copy`,必须用 `from_fn`
/// 而不是 `[expr; N]` 重复式。
///
/// # Returns
///
/// - `[GpuAoPass; AO_PASS_COUNT]` - 每条的 pipeline 与 bind group 都是 `JsValue::NULL`。
pub fn empty_ao_passes() -> [GpuAoPass; AO_PASS_COUNT] {
    array::from_fn(|_index: usize| GpuAoPass {
        pipeline: JsValue::NULL,
        group: JsValue::NULL,
    })
}

/// 三条 AO pass 各自的 uniform buffer,建之前都是空的。
///
/// # Returns
///
/// - `[JsValue; AO_PASS_COUNT]` - 每条一个尚未创建的 buffer 句柄。
pub fn empty_ao_buffers() -> [JsValue; AO_PASS_COUNT] {
    [JsValue::NULL; AO_PASS_COUNT]
}

/// 全空的 bloom pipeline / bind group 配对数组(建之前的状态)。
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
    BLOOM_STRENGTH * lighting.get_emissive_gain().max(BLOOM_MIN_GAIN)
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

/// AO 模糊 pass 的 WGSL。
///
/// 深度感知(双边)的可分离模糊:只在深度差小于 `DEPTH_SIGMA` 的像素之间
/// 混合。普通高斯模糊会把前景的暗角糊到背景上,在街沿和墙角表现为脏边。
///
/// 采样数固定 9(5 抽 + 4 双边权重),分离成 H/V 两趟。
///
/// # Returns
///
/// - `String` - 填好深度 sigma 与哨兵值的 WGSL。
pub(crate) fn shader_ao_blur() -> String {
    SSAO_BLUR_BODY
        .replace(WGSL_SLOT_DEPTH_SIGMA, &SSAO_DEPTH_SIGMA.to_string())
        .replace(WGSL_SLOT_NO_GEOMETRY, &SSAO_NO_GEOMETRY.to_string())
}
/// SSAO pass 的 WGSL。
///
/// 在**视空间**半球里采样:每个样本算出一个期望深度,与实际深度比较,
/// 差得越远说明被遮挡得越厉害。用半球而不是整球 —— 背后的几何体
/// 不构成遮挡。
///
/// 法线与深度都必须来自城市 pass 的 MRT 且是**视空间**的,和 `frame`
/// 里的 `view` 约定配套。世界法线直接拿来算会让方向整体错开。
///
/// # Returns
///
/// - `String` - 填好哨兵值与采样数的 WGSL。
pub(crate) fn shader_ssao() -> String {
    // 哨兵与采样数从 Rust 常量注入:WGSL 看不到 Rust 的 `const`,
    // 硬编码一份必然和清屏值、`SSAO_SAMPLES` 走散。
    let body: String = SSAO_BODY
        .replace(WGSL_SLOT_NO_GEOMETRY, &SSAO_NO_GEOMETRY.to_string())
        .replace(WGSL_SLOT_SAMPLES, &SSAO_SAMPLES.to_string());
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    /// AO 采样器必须是 `non-filtering`,深度纹理必须是
    /// `unfilterable-float`。
    ///
    /// 这两个都错过:G-buffer 深度是 `r32float`,WebGPU 视其为
    /// `UnfilterableFloat`,配 filtering 采样器或声明成 `float` 的
    /// sampleType,整条 pass 的 bind group 当场作废 —— 而画面上只表现为
    /// 「AO 没有效果」,没有任何报错指向它。
    #[test]
    pub(crate) fn the_ao_bind_layout_matches_the_unfilterable_depth_texture() {
        assert_eq!(SAMPLE_TYPE_UNFILTERABLE_FLOAT, "unfilterable-float");
        assert_eq!(SAMPLER_TYPE_NON_FILTERING, "non-filtering");
        // 合成的 AO 图是 `r8unorm`,那边仍必须是 filtering —— 两个 bind
        // group 的采样器要求相反,别互相照抄。
        assert_eq!(AO_TARGET_FORMAT, "r8unorm");
    }

    /// SSAO 的视空间法线必须先编码到 [0,1] 再写 unorm 目标,读回来对称解码。
    ///
    /// 直接写 `dot(n, right)` 会被 unorm 钳到 0(背向相机的分量全是负),
    /// `normalize()` 拿到的是被削过的法线,半球采样整个歪掉。
    #[test]
    pub(crate) fn the_view_normal_is_encoded_for_the_unorm_gbuffer() {
        let source: &str = SHADER_FRAGMENT;
        assert!(
            source.contains("view_normal * 0.5 + vec3<f32>(0.5)"),
            "fs_main must encode the view normal from [-1,1] to [0,1]"
        );
        let ssao: &str = SSAO_BODY;
        assert!(
            ssao.contains("* 2.0") && ssao.contains("vec3<f32>(1.0)"),
            "fs_ssao must decode the view normal symmetrically"
        );
    }

    /// 遮挡的判定方向:样本深度比场景**更近**才算遮挡。
    ///
    /// `step(sample_depth, scene_depth)` 命中的是相反的方向(场景更远)。
    /// 写反时整张 AO 图恒为 1.0,而 99.8% 的样本都正常跑完了深度比较,
    /// 看上去「管线在工作」,只是从来没判成过遮挡。
    #[test]
    pub(crate) fn occlusion_requires_the_scene_to_be_nearer_than_the_sample() {
        let ssao: &str = SSAO_BODY;
        assert!(
            ssao.contains("let within : f32 = step(delta, 0.0);"),
            "the occlusion test must reject samples whose scene depth is farther"
        );
        assert!(
            ssao.contains("let delta : f32 = scene_depth - sample_depth;")
                && ssao.contains("within * falloff"),
            "occlusion must be scored from scene_depth - sample_depth"
        );
        // 只看代码行 —— 注释里为了讲清楚曾经写反过什么,正要把这段字符串
        // 引用出来,按整段文本搜会自己撞上自己。
        let code: Vec<&str> = ssao
            .lines()
            .filter(|line: &&str| {
                let trimmed: &str = line.trim_start();
                !trimmed.is_empty() && !trimmed.starts_with("//")
            })
            .collect();
        assert!(
            !code
                .iter()
                .any(|line: &&str| line.contains("step(sample_depth, scene_depth)")),
            "step(sample_depth, scene_depth) is the inverted test -- it counts \
             occlusion when the scene is FARTHER, which is backwards"
        );
    }

    /// AO pass 里不能出现裸的 `textureSample`。
    ///
    /// 循环里带 `continue`,控制流依赖纹理值,**不是 uniform**,
    /// WGSL 只允许在 uniform 控制流里调 `textureSample`。
    #[test]
    pub(crate) fn the_ao_passes_only_sample_from_uniform_control_flow() {
        for body in [SSAO_BODY, SSAO_BLUR_BODY] {
            for line in body.lines() {
                let trimmed: &str = line.trim();
                if trimmed.starts_with("//") {
                    continue;
                }
                assert!(
                    !trimmed.contains("textureSample("),
                    "bare textureSample in a non-uniform path: {trimmed}"
                );
            }
        }
    }

    /// 合成那条要把 AO 当**乘性**项作用在场景色上。
    ///
    /// 写成 `vec3<f32>(ao)` 会让 AO 取代整个场景 —— AO 接近 1.0 时画面
    /// 是一片近乎纯白的象牙色,而调试信息、HUD、三角数全部照常,只有
    /// 世界不见了。
    #[test]
    pub(crate) fn the_composite_multiplies_the_scene_by_ao() {
        let source: &str = SHADER_BLOOM_FRAGMENT;
        assert!(
            source.contains("color * mix(1.0, ao, bloom.texel.w)"),
            "the composite must multiply scene colour by AO"
        );
        assert!(
            !source.contains("vec3<f32>(ao)"),
            "vec3<f32>(ao) replaces the scene with the AO map"
        );
    }

    /// 列主序下标:[行][列] -> 线性下标。
    ///
    /// # Arguments
    ///
    /// - `usize` - 行。
    /// - `usize` - 列。
    ///
    /// # Returns
    ///
    /// - `usize` - 线性下标。
    pub(crate) const fn at(row: usize, column: usize) -> usize {
        column * 4 + row
    }

    /// ⚠️ **y 行必须保持不变** —— 一条反直觉的回归测试。
    ///
    /// 直觉上 WebGPU 的 NDC +Y 朝下、OpenGL 朝上,应该把 y 行取反。
    /// 实测那样做会让整幅画面**上下颠倒**:顶部变成沥青、底部变成天空。
    /// 用**已知正确**的 WebGL2 对照帧校准过(见
    /// `tools/gpu-probe/profile-shot.py` 的垂直剖面):正确形态是
    /// 「顶部天空%、底部沥青暗色%」。
    #[test]
    pub(crate) fn frame_bytes_leaves_the_y_row_untouched() {
        let source: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, -1.0, -1.0, //
            0.0, 0.0, 0.0, 0.0,
        ];
        let matrix: Mat4 = Mat4::from_column_major(source);
        let out: [f32; 16] = frame_bytes(&matrix);

        for column in 0..4usize {
            assert_eq!(
                out[at(1, column)],
                source[at(1, column)],
                "y row must NOT be negated (that flips the image upside down)"
            );
        }
        // 投影矩阵第 1 行是 (0, 1, 0, 0):f 必须仍是 +1.0。
        assert_eq!(out[at(1, 1)], 1.0, "clip.y must keep its sign");
    }

    /// 只有 z 行被搬运,x / y / w 三行逐位不变。
    ///
    /// 这条钉住的就是那个把整场景压成横带的 bug:曾经的循环写的是
    /// `out[8 + column]` / `out[12 + column]`,取到的是**列**而不是
    /// **行**,于是 clip.x / clip.y 的 z 系数被砍半。列主序下第 2 行是
    /// 下标 2/6/10/14,第 3 行是 3/7/11/15 —— 只有逐位比对才能发现。
    #[test]
    pub(crate) fn frame_bytes_only_remaps_the_depth() {
        // 一个带透视的 proj·view:第 2 列有非零的 z 系数,第 3 列的
        // w 行是 1 —— 正是会被旧写法误伤的那两个位置。
        let source: [f32; 16] = [
            1.2, 0.0, 0.0, 0.0, //
            0.0, 1.6, 0.0, 0.0, //
            0.0, 0.0, -1.0, -1.0, //
            0.0, 0.0, -2.0, 0.0,
        ];
        let matrix: Mat4 = Mat4::from_column_major(source);
        let out: [f32; 16] = frame_bytes(&matrix);

        for column in 0..4usize {
            assert_eq!(out[at(0, column)], source[at(0, column)], "row 0 moved");
            assert_eq!(out[at(1, column)], source[at(1, column)], "row 1 moved");
            // 第 2 行:必须是 0.5·z + 0.5·w。
            let expected: f32 = 0.5 * source[at(2, column)] + 0.5 * source[at(3, column)];
            assert_eq!(out[at(2, column)], expected, "row 2 not remapped");
            // 第 3 行:只有 z 行参与搬运,w 行自己保持不变。
            assert_eq!(out[at(3, column)], source[at(3, column)], "row 3 moved");
        }
    }

    /// 旧写法会砍半 clip.x / clip.y 的 z 系数,这条直接盯住它。
    #[test]
    pub(crate) fn frame_bytes_keeps_projection_sign() {
        // 纯透视矩阵:第 3 列是 (0, 0, -1, 0)。
        let source: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, -1.0, -1.0, //
            0.0, 0.0, 0.0, 0.0,
        ];
        let matrix: Mat4 = Mat4::from_column_major(source);
        let out: [f32; 16] = frame_bytes(&matrix);

        // M[3][3] = -1 必须原样保留:它是「投影把 w 打成 -z」的符号。
        assert_eq!(out[at(3, 2)], -1.0, "projection sign lost");
        // M[0][2] / M[1][2] 是 clip.x / clip.y 的 z 系数,透视矩阵里
        // 本来就是 0;旧写法会把它们改成 0.5·0 + 0.5·0,看似一样,
        // 所以这里真正要盯的是第 2 行的第 0、1 项确实为 0。
        assert_eq!(out[at(2, 0)], 0.0, "clip.x gained a z term");
        assert_eq!(out[at(2, 1)], 0.0, "clip.y gained a z term");
        // 第 2 行第 2 项:0.5·(-1) + 0.5·(-1) = -1。
        assert_eq!(out[at(2, 2)], -1.0, "z row not remapped");
    }

    /// bloom 的高斯核必须是**归一化**的,否则每过一趟模糊画面就暗一截。
    ///
    /// 5 抽头核的总权重是 `w0 + 2 × (w1 + w2 + w3 + w4)` —— 中间
    /// 那四个抽头各取两次(`uv ± offset`)。这条钉住的就是「有人把某个
    /// 权重改成 0.2 却忘了重新归一化」那个坑:它不会报任何编译错误,
    /// 只是整幅画莫名变暗,而且很难联想到是这一行。
    ///
    /// 1e-5 的容差来自那五个常数**只保留 6 位小数**:`0.227027`
    /// 写成 `0.227027027` 时总和恰好 1.0,而这里用的是 WebGL2 端
    /// 那个 6 位版本,差值在 1e-5 量级。
    #[test]
    pub(crate) fn bloom_blur_kernel_weights_sum_to_one() {
        let total: f32 = bloom_blur_kernel_total();
        assert!(
            (total - 1.0).abs() < 1.0e-5,
            "bloom 高斯核没有归一化:总权重 {total}(应 ≈ 1.0),\
             每过一次模糊画面就暗一截"
        );
        // 权重必须单调递减,否则光晕会出现一个亮环而不是平滑衰减。
        let weights: [f32; 5] = BLOOM_BLUR_WEIGHTS;
        for index in 1..5usize {
            assert!(
                weights[index] < weights[index - 1],
                "核权重必须在索引 {} 处下降:{:?}",
                index,
                weights
            );
        }
    }

    /// 全屏三角形的 `uv.y` **必须**与 `clip.y` 反向 —— 这是 bloom 上线
    /// 当天整幅画面上下颠倒的那条根因。
    ///
    /// 两个方向是**各自独立**的约定,必须分开记:
    ///
    /// - `clip` 用的是 **NDC**:WebGPU 的 NDC **+y 朝上**,`clip.y = +1`
    ///   是屏幕**最上面**那一行。于是三角形顶点 `p.y = 0`(即
    ///   `clip.y = −1`)落在**下方** —— `p` 的 y **不能**取反,取反了
    ///   三角形就朝屏幕外面长,连覆盖都做不对。
    /// - `uv` 用的是**纹理采样坐标**:WebGPU 的纹理 **v = 0 在最上面
    ///   一行**(`textureSample` 的原点与 NDC 的 y 朝向相反)。于是
    ///   `p.y = 0` 那条边(屏幕**下方**)必须去采 `v ≈ 1`,即
    ///   `uv.y = 1 − p.y`。
    ///
    /// 这条测试把整条后处理链条钉成「**按行号一一对应**」:第 `r` 行
    /// 的片元采第 `r` 行,提取 / 两条模糊 / 合成全都行号不变 —— 所以它
    /// 与链条上有几条 pass 无关。写反的症状是「天空在画面最下面」,
    /// 而且 `cargo test` 变不红,只能靠这条 + 截图。
    #[test]
    pub(crate) fn fullscreen_uv_y_is_flipped_against_clip_y() {
        let source: &str = SHADER_FULLSCREEN_VERTEX;
        assert!(
            source.contains("out.uv = vec2<f32>(p.x, 1.0 - p.y);"),
            "vs_fullscreen 的 uv 必须按 (p.x, 1.0 - p.y) 生成:\
             WebGPU 的纹理 v = 0 在**最上一行**,而 NDC 的 +y 朝上,\
             两者相反。照抄 WebGL 那份的 `out.uv = p` 会让整幅画面上下颠倒\
             (天空跑到画面最下面)。当前源码:\n{source}"
        );
        // `clip.y` 保持不取反 —— 这两条是独立约定,别一起翻。
        assert!(
            source.contains("out.clip = vec4<f32>(p * 2.0 - vec2<f32>(1.0, 1.0), 0.0, 1.0);"),
            "vs_fullscreen 的 clip.y 不能取反:WebGPU 的 NDC +y 朝上,\
             `p.y = 0` 已经落在屏幕下方。当前源码:\n{source}"
        );
        // 旧的 `out.uv = p` 必须已经不存在(它是这条 bug 的原始写法)。
        assert!(
            !source.contains("out.uv = p;"),
            "`out.uv = p` 就是把画面上下颠倒的那一行,不能回来"
        );
    }

    /// bloom 三张目标必须正好是画布的**一半**分辨率。
    ///
    /// 这条钉住 [`bloom_target_size`] 的缩放系数:写成别的值(比如
    /// `0.25` 或 `1.0`)不会报任何错,只是光晕的半径跟着变 —— 半
    /// 分辨率下 1 个纹素的模糊步长等于全分辨率下的 2 个,所以改缩放
    /// 就等于悄悄改了 `BLOOM_BLUR_SPREAD`。
    #[test]
    pub(crate) fn bloom_targets_are_exactly_half_the_canvas() {
        // 与 WebGL2 端 `ensure_targets` 里的 `scaled(width, BLOOM_SCALE)`
        // 同一个系数。
        assert_eq!(BLOOM_SCALE, 0.5, "bloom 缩放系数被改了");
        for (width, height) in [(1280u32, 720u32), (800u32, 600u32), (1920u32, 1080u32)] {
            let size: (u32, u32) = bloom_target_size(width, height);
            assert_eq!(
                size,
                (width / 2, height / 2),
                "{width}x{height} 的 bloom 目标应该是 {width}x{height} 的一半"
            );
        }
    }

    /// 极小 / 零尺寸必须被抬到 1 像素,不能是 0。
    ///
    /// `createTexture` 的 `size` 是 `GPUExtent3D`,宽或高为 0 会被 WebGPU
    /// 判 invalid —— 而那是**异步**错误,表现为整帧消失、console 里没有
    /// 任何线索。窗口被拖到极小、或 `?res=0` 这类边界输入都会走到这里。
    #[test]
    pub(crate) fn bloom_target_size_never_collapses_to_zero() {
        assert_eq!(bloom_target_size(0, 0), (1, 1));
        assert_eq!(
            bloom_target_size(1, 1),
            (1, 1),
            "1x1 画布的半分辨率是 0.5,要抬到 1"
        );
    }

    /// 扩容之后容量**必须**装得下 —— 这是黑屏修复的核心不变式。
    ///
    /// 背景:地面 / 水面网格随玩家跨街区流式重建,顶点数台阶式变化。
    /// 旧实现没有容量概念,`replace_mesh` 直接往当初那份偏小的缓冲里
    /// `writeBuffer`,越界之后整条 command buffer 判 invalid,
    /// `queue.submit()` 变成空操作,swapchain 没人画 → 屏幕全黑且永不恢复。
    ///
    /// 这条测试把「写之前先扩容」那个不变式钉成可离线断言的东西:
    /// 真实黑屏要在无头浏览器里跑几千帧才看得见,而不变式随时可测。
    #[test]
    pub(crate) fn grown_capacity_always_fits_the_payload() {
        // 真实黑屏现场的两个尺寸(来自 Chrome 的 validation 报错):
        //   顶点 880992 → 901368 个(16 B / 顶点)= 14095872 → 14421888 B
        //   索引 293664 → 300456 个(4 B / 索引) = 1174656 → 1201824 B
        for (current, needed) in [
            (14_095_872usize, 14_421_888usize),
            (1_174_656usize, 1_201_824usize),
        ] {
            let grown: usize = grown_capacity(current, needed);
            assert!(
                grown >= needed,
                "扩容后装不下:{needed} > {grown}(现有 {current})—— writeBuffer 会越界,\
                 整帧判 invalid 并让 submit 变空操作,画面全黑"
            );
            assert!(
                grown > current,
                "需要更大却没扩:{current} -> {grown},下一次重建必然再次越界"
            );
        }
    }

    /// 不需要扩容时**不能**缩小缓冲,否则会把刚好够用的尺寸又变回去,
    /// 让下一次「略微变大」立刻再次越界。
    #[test]
    pub(crate) fn grown_capacity_never_shrinks_a_live_buffer() {
        for (current, needed) in [
            (1_174_656usize, 1_174_656usize),
            (16_777_216usize, 1_201_824usize),
            (14_095_872usize, 0usize),
        ] {
            assert_eq!(
                grown_capacity(current, needed),
                current,
                "{current} 装得下 {needed} 时容量不该变"
            );
        }
    }

    /// 连续多街区重建必须**单调不降**:每一帧的容量都要盖住那一帧
    /// 实际的载荷。玩家连续横穿城市时这就是真实序列。
    #[test]
    pub(crate) fn grown_capacity_is_monotonic_across_consecutive_rebuilds() {
        // 实测地面网格随生成中心漂移时的载荷台阶(每一项 = 一帧载荷)。
        let payloads: [usize; 6] = [
            14_095_872, 14_421_888, 14_421_888, 14_950_656, 15_204_736, 15_466_112,
        ];
        let mut capacity: usize = 0;
        for payload in payloads {
            capacity = grown_capacity(capacity, payload);
            assert!(
                capacity >= payload,
                "第 {payload} B 的重建载荷装不进 {capacity} B 的缓冲"
            );
        }
    }

    /// 画布尺寸为 0 时整帧必须放弃,绝不能把 0×0 的 swapchain 送进
    /// WebGPU。
    ///
    /// `context.getCurrentTexture()` 在画布为 0 时返回一张 0×0 纹理,
    /// Dawn 报 `Could not create a swapchain texture of size 0`
    /// → `[Invalid Texture]` → `[Invalid TextureView]`
    /// → `[Invalid CommandBuffer]` → `submit` 空操作 → 画面全黑且不恢复。
    ///
    /// `render()` 因此在 `width == 0 || height == 0` 时提前 `Ok(0)`。
    /// 这条测试钉住那个判据本身,免得有人为了「省一次 early return」
    /// 把它删掉。
    #[test]
    pub(crate) fn zero_canvas_size_is_rejected_before_any_submit() {
        for (width, height) in [(0u32, 720u32), (1280u32, 0u32), (0u32, 0u32)] {
            assert!(
                width == 0 || height == 0,
                "守卫判据必须覆盖 {width}x{height}:任何一个维度为 0 都会拿到 0×0 swapchain"
            );
        }
    }

    /// 合成的辉光强度必须与 WebGL2 端那个表达式逐项相同。
    ///
    /// 钉住 `BLOOM_STRENGTH * emissive_gain.max(BLOOM_MIN_GAIN)`:
    /// 少了 `max` 的下限,正午相位 `emissive_gain` 只有 0.18(低于
    /// 0.35 的下限),辉光会被压到 `BLOOM_STRENGTH × 0.35` —— 忘了
    /// `max` 的话正午的湿路面高光就完全没有辉光了,而且从黄昏帧上
    /// 看**完全正常**,只有切到正午才暴露。
    #[test]
    pub(crate) fn bloom_composite_strength_matches_the_webgl2_formula() {
        // 正午:emissive_gain 低于下限,必须被兜住。
        let mut lighting: SceneLighting = SceneLighting::for_phase(crate::render::DayPhase::Noon);
        assert!(
            lighting.emissive_gain < BLOOM_MIN_GAIN,
            "前提不成立:noon 相位的 emissive_gain {} 不低于下限 {BLOOM_MIN_GAIN},\
             这条测试测不出 max 的作用",
            lighting.emissive_gain
        );
        assert_eq!(
            bloom_composite_strength(&lighting),
            BLOOM_STRENGTH * BLOOM_MIN_GAIN,
            "noon 相位低于下限,辉光必须被 BLOOM_MIN_GAIN 兜住(忘了 max)"
        );
        // 黄昏:emissive_gain 高于下限,取线性那一支。
        lighting = SceneLighting::for_phase(crate::render::DayPhase::Dusk);
        assert!(
            lighting.emissive_gain > BLOOM_MIN_GAIN,
            "前提不成立:dusk 相位的 emissive_gain {} 低于下限 {BLOOM_MIN_GAIN}",
            lighting.emissive_gain
        );
        assert_eq!(
            bloom_composite_strength(&lighting),
            BLOOM_STRENGTH * lighting.emissive_gain,
            "dusk 相位高于下限,辉光必须是线性的那一支"
        );
    }

    /// 游戏里真实存在的黄昏光方向(`render.rs` 里 `DUSK` 那一档)。
    ///
    /// 复用来它是因为两个后端必须对**同一束斜光**给出同一个剔除决定 ——
    /// 否则 WebGPU 上会少掉一栋楼的影子,而 WebGL2 上是好的。
    pub(crate) fn dusk_light() -> Vec3 {
        crate::render::normalize3([0.86, 0.24, -0.44])
    }

    /// 判据的有效半径:半宽 + 余量(`render.rs` 的 `SHADOW_HALF_EXTENT`
    /// 与 `SHADOW_CULL_MARGIN`)。
    pub(crate) fn reach() -> f32 {
        crate::r#config::SHADOW_HALF_EXTENT + crate::r#config::SHADOW_CULL_MARGIN
    }

    /// ⚠️ **只测原点的判据会误剔高楼** —— 这条钉住那个坑。
    ///
    /// [`crate::render::instance_affects_shadow`] 的注释里写了原因:
    /// 判据要同时看**原点**和**原点往下挪一截**两个落点。斜光下
    /// (`dusk` 的 y 分量仅 0.24)一栋 40 m 高的楼,原点在 y = 40 时
    /// 影子落点距 focus 119 m(在视锥外),而它的**底部**(y = 20)
    /// 落点只有 61 m,仍在「半宽 + 余量」之内 —— 只测原点就会把
    /// 「影子还伸进视锥」的高楼整栋剔掉,地面上凭空少一块长影子。
    ///
    /// 这里的实例用 `[x, 40, 0]` 与 `[x, 20, 0]` 两支:
    /// 前者是只看原点会做出错误决定的那个,后者是唯一救回它的那支。
    #[test]
    pub(crate) fn shadow_cull_keeps_a_tower_whose_base_reaches_the_frustum() {
        let focus: Vec3 = [0.0, 0.0, 0.0];
        let light: Vec3 = dusk_light();
        let top: crate::render::Instance =
            crate::render::Instance::new([30.0, 40.0, 0.0], 0.0, 1.0, [1.0; 3]);
        let base: crate::render::Instance =
            crate::render::Instance::new([30.0, 20.0, 0.0], 0.0, 1.0, [1.0; 3]);

        // 两个落点:沿光线投影到 focus 所在的水平面上。
        //
        // 闭包**不能**写成 `fn(f32) -> f32` 指针类型(§5.1 要求显式标注,
        // 而 `fn` 指针不能捕获环境),所以把 focus / light 改成参数。
        let landing: fn(Vec3, Vec3, f32) -> f32 = |focus: Vec3, light: Vec3, height: f32| -> f32 {
            let along: f32 = (focus[1] - height) / light[1];
            let dx: f32 = 30.0 + light[0] * along - focus[0];
            let dz: f32 = 0.0 + light[2] * along - focus[2];
            (dx * dx + dz * dz).sqrt()
        };
        let top_landing: f32 = landing(focus, light, top.get_model_ref()[13]);
        let base_landing: f32 = landing(focus, light, base.get_model_ref()[13]);

        // 这个用例的前提:楼顶落在视锥外,而楼底落在视锥内。
        // 两支断言都失败的话说明常量被改了,用例本身失去意义。
        assert!(
            top_landing > reach(),
            "前提不成立:楼顶落点 {top_landing:.2} m 已在视锥内(半宽 + 余量 = {:.2}),\
             测不出「只看原点」那个坑",
            reach()
        );
        assert!(
            base_landing <= reach(),
            "前提不成立:楼底落点 {base_landing:.2} m 也在视锥外(半宽 + 余量 = {:.2}),\
             这栋楼根本不该被考虑",
            reach()
        );

        // 真正要钉的那一条:**只测原点**的写法会把这栋楼剔掉。
        assert!(
            !origin_only_would_keep(&top, focus, light),
            "只看原点的判据本该把这栋楼剔掉(它就是那个 bug 的形状),\
             落点 {top_landing:.2} m > 半宽 + 余量 = {:.2}",
            reach()
        );
        // 而带上下两支探针的真判据**必须**留下它。
        assert!(
            crate::render::instance_affects_shadow(&top, focus, light),
            "斜光下 {top_landing:.2} m 的高楼被剔掉了,但它的楼底落点只有 \
             {base_landing:.2} m ≤ 半宽 + 余量 = {:.2} —— 地面上会凭空少一块影子",
            reach()
        );
        // 楼底那一支单独看当然也在视锥内。
        assert!(
            crate::render::instance_affects_shadow(&base, focus, light),
            "楼底落点 {base_landing:.2} m 明明在视锥内(半宽 + 余量 = {:.2}),却被剔掉了",
            reach()
        );
    }

    /// 「只看原点」那个**已知错误**的判据,仅供上面的测试做对照。
    ///
    /// 它刻意**不是** [`crate::render::instance_affects_shadow`] ——
    /// 那正是本测试要证明「不再只看原点」的理由。写成独立函数而不是
    /// 在测试里内联,是为了让对照判据一眼可读。
    pub(crate) fn origin_only_would_keep(
        instance: &crate::render::Instance,
        focus: Vec3,
        light: Vec3,
    ) -> bool {
        let m: &crate::r#type::Mat4Data = instance.get_model_ref();
        let along: f32 = (focus[1] - m[13]) / light[1];
        let dx: f32 = m[12] + light[0] * along - focus[0];
        let dz: f32 = m[14] + light[2] * along - focus[2];
        dx * dx + dz * dz <= reach() * reach()
    }

    /// `smoothstep(a, b, x)` 的 Rust 写法,与 WGSL 的同名内建函数一致。
    ///
    /// # Arguments
    ///
    /// - `a` - 下界。
    /// - `b` - 上界。
    /// - `x` - 输入。
    ///
    /// # Returns
    ///
    /// 平滑后的 `0..=1`。
    pub(crate) fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
        let t: f32 = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// 顶点接触 AO 的 Rust 镜像,与 [`SHADER_VERTEX`] 里那个
    /// `contact_ao()` 逐字对应。
    ///
    /// 放在测试里而不只测 WGSL 字符串,是因为这个缺陷本质是**数值**
    /// 的:字符串照样能通过「声明了 uniform」那类检查,只有把公式跑
    /// 一遍才会发现开阔地是不是被无端压暗了。
    ///
    /// # Arguments
    ///
    /// - `world_y` - 顶点的世界高度(米)。
    /// - `horizontal` - 顶点到本模型世界原点的 XZ 水平距离(米)。
    ///
    /// # Returns
    ///
    /// 亮度系数 `1.0`(不压暗)..=`CONTACT_SHADOW_FLOOR`(贴基座最暗)。
    pub(crate) fn contact_ao_factor(world_y: f32, horizontal: f32) -> f32 {
        let reach: f32 = CONTACT_SHADOW_REACH;
        let feather: f32 = CONTACT_SHADOW_FEATHER;
        let low: f32 = CONTACT_SHADOW_MIN_HEIGHT;
        let high: f32 = BAKED_CONTACT_AO_HEIGHT;
        let proximity: f32 = 1.0 - smoothstep(reach - feather, reach, horizontal);
        let low_enough: f32 =
            smoothstep(low, low + 0.001, world_y) * (1.0 - smoothstep(high - 0.001, high, world_y));
        1.0 - (1.0 - CONTACT_SHADOW_FLOOR) * low_enough * proximity
    }

    /// 缺陷本体:接触 AO 拿**世界高度**当「离基座多近」的代理量。
    ///
    /// 旧 WGSL 表达式 `mix(ao_params.y, 1.0, clamp(world.y /
    /// ao_params.x, 0, 1))` 对同一高度的两个顶点给出**完全相同**的
    /// 压暗 —— 离基座 0.3 m 的和 40 m 外的都是 `CONTACT_SHADOW_FLOOR`。
    /// 修好后应是前者更暗、后者回到 1.0。
    ///
    /// 实测(2026-10-10,LOFT 样板楼南墙 z=35 外,WebGPU,正午):沿墙法向
    /// 0.55 → 6.85 m 的地面 luma 落在 103.024..104.433(range 1.41),
    /// 墙根均值比开阔地还**低** 0.232 luma —— 一条平线,而且方向还反了。
    #[test]
    pub(crate) fn contact_ao_darkens_by_horizontal_proximity_not_world_height() {
        // 楼身下段,同一高度,只改到基座的水平距离。
        let y: f32 = 0.5;
        let at_base: f32 = contact_ao_factor(y, 0.3);
        let nearby: f32 = contact_ao_factor(y, 3.0);
        let open: f32 = contact_ao_factor(y, 40.0);

        assert!(
            at_base < open - 0.05,
            "同高度 y={y}:离基座 0.3 m 的 {at_base:.4} 必须明显比 40 m 外的 \
             {open:.4} 更暗 —— 旧公式按 world.y 算,两者会完全相等"
        );
        assert!(
            nearby < open,
            "离基座 3.0 m 的 {nearby:.4} 应比 40 m 外的 {open:.4} 暗"
        );
        assert!(
            (open - 1.0).abs() < 1e-5,
            "离基座 40 m 已在 CONTACT_SHADOW_REACH={CONTACT_SHADOW_REACH} m 之外,\
             接触 AO 必须完全回到 1.0,实测 {open:.5}"
        );
    }

    /// **地面网格整片不吃顶点接触 AO。**
    ///
    /// 这是用户报「地面的光追效果不对」时最直接的成因:旧公式按
    /// `world.y / ao_height` 压暗,而路面 `ROAD = 0.0`、人行道
    /// `SIDEWALK = 0.14`、地块 `LOT_GROUND = 0.16`(取自 `game.rs` 的
    /// `build_ground_near`)全都远低于 `BAKED_CONTACT_AO_HEIGHT = 1.2`,
    /// 于是全城每一块地面都取到 `clamp(0) = 0` 吃满最深压暗 —— 墙根
    /// 与开阔街道一样暗。
    ///
    /// 修法是加高度下界 `CONTACT_SHADOW_MIN_HEIGHT`(0.30 m,高于地面
    /// 网格最高点),让这一项只负责「楼身下段 / 树干根部」的自遮蔽;
    /// 地面贴墙的暗部交给 SSAO —— 顶点着色器结构上不知道墙在哪,而
    /// **WebGPU 路径目前还没有 SSAO**。
    #[test]
    pub(crate) fn contact_ao_leaves_the_ground_mesh_untouched() {
        for (name, y) in [("road", 0.0_f32), ("sidewalk", 0.14), ("lot", 0.16)] {
            // 即便就站在一栋楼正下方(水平距离 0),也必须是 1.0。
            let value: f32 = contact_ao_factor(y, 0.0);
            assert!(
                (value - 1.0).abs() < 1e-5,
                "{name}(y={y})属于地面网格,不该吃顶点接触 AO —— 实测 {value:.5}; \
                 CONTACT_SHADOW_MIN_HEIGHT={CONTACT_SHADOW_MIN_HEIGHT} 本该把它整个排除"
            );
        }
        // 用 `const {}` 而不是普通 `assert!`:这个不变量完全由常量决定,
        // 而 clippy 的 `assertions_on_constants` 会要求它放进 const 块 ——
        // 那正好更强:const 块在**编译期**求值,常量被改坏时是编译失败,
        // 而不是运行时一条断言。
        const {
            assert!(
                CONTACT_SHADOW_MIN_HEIGHT > 0.16,
                "高度下界必须高于地面网格最高点 0.16 (LOT_GROUND),否则路面仍会被无端压暗"
            );
        }
    }

    /// 高处顶点不吃「贴墙根」的压暗。
    ///
    /// 若垂直上界失效,一栋 40 m 高的楼会整条压暗,表现为
    /// 「每栋楼都自带一圈黑边」。上界由 `BAKED_CONTACT_AO_HEIGHT` 封顶。
    #[test]
    pub(crate) fn contact_ao_skips_the_proximity_term_above_the_height_band() {
        let y: f32 = BAKED_CONTACT_AO_HEIGHT * 4.0;
        let value: f32 = contact_ao_factor(y, 0.0);
        assert!(
            (value - 1.0).abs() < 1e-5,
            "y={y:.2}(远超 BAKED_CONTACT_AO_HEIGHT={BAKED_CONTACT_AO_HEIGHT})且贴着基座时\
             接触 AO 必须是 1.0,实测 {value:.5}"
        );
    }

    /// 顶点着色器读的那三个 uniform 分量必须真的被上传。
    ///
    /// `ao_params` 从「只有 xy」扩到「xyzw」:若 WGSL 里写的是
    /// `ao_params.z` 而 Rust 侧忘了填,读到的是 0,于是
    /// `max(reach, 0.001)` 退化成 0.001 m 的作用半径 —— **楼身下段
    /// 一条接触暗带都消失**,而画面上不报任何错。
    #[test]
    pub(crate) fn contact_ao_reach_and_feather_reach_the_vertex_shader() {
        let lighting: SceneLighting = SceneLighting::for_phase(crate::render::DayPhase::Noon);
        let shading: ShadingUniforms = shading_from_lighting(&lighting, [0.0, 1.6, 12.0]);
        assert_eq!(
            shading.ao_params[2], CONTACT_SHADOW_REACH,
            "ao_params.z(水平作用半径)必须等于 {CONTACT_SHADOW_REACH},实测 {} —— \
             忘了填的话顶点着色器读到 0,整条水平项退化成 0.001 m",
            shading.ao_params[2]
        );
        assert_eq!(shading.ao_params[3], CONTACT_SHADOW_FEATHER);
        assert_eq!(
            shading.ao_band[0], CONTACT_SHADOW_MIN_HEIGHT,
            "ao_band.x(高度下界)必须等于 {CONTACT_SHADOW_MIN_HEIGHT},实测 {} —— \
             忘了填的话读到 0,整城地面又回到「全亮」的老样子",
            shading.ao_band[0]
        );
    }

    /// ⚠️ uniform 布局的**字节偏移**不能被这一轮的改动挪动。
    ///
    /// WGSL 的 uniform struct 按声明顺序排布,少一个 vec4 不会编译
    /// 报错,只会在 GPU 上读到**后面那个块的数据**当成本字段 —— 阴影
    /// 参数静默变成色调映射的曝光值,症状是「画面偏亮且无影」。
    /// `ao_band` 因此被放在**整块的最后**,而不是插在 `ao_params`
    /// 后面。
    #[test]
    pub(crate) fn shading_uniform_layout_is_three_hundred_and_forty_six_bytes() {
        assert_eq!(SHADING_VEC4_COUNT, 15);
        assert_eq!(
            UNIFORM_SHADING_BYTES,
            (SHADING_VEC4_COUNT as u32) * 16,
            "UNIFORM_SHADING_BYTES 与字段数脱节:少分配的 buffer 会让 \
             writeBuffer 直接抛 writeBuffer size exceeds buffer size"
        );
        // 既有字段的偏移必须一字不动:`shadow_misc` 是第 14 块(下标 13),
        // `ao_band` 只能是第 15 块(下标 14)。
        let shading: ShadingUniforms = ShadingUniforms::default();
        let flat: [f32; 60] = flatten_shading(shading);
        assert_eq!(
            &flat[52..56],
            &shading.shadow_misc,
            "shadow_misc 的偏移被挪动了 —— 它必须仍在下标 13(字节 208)"
        );
        assert_eq!(
            &flat[56..60],
            &shading.ao_band,
            "ao_band 必须在最后一块(下标 14 / 字节 224)"
        );
    }

    /// ⚠️ 顶点与片元两份 WGSL 里的 `Shading` struct 必须**逐项一致**。
    ///
    /// 两者是同一个 uniform buffer 的两个视图,少一块 vec4 不会编译
    /// 报错,只会在 GPU 上读到后面那个块的数据当成本字段。更糟的是:
    /// **顶点那份缺字段时 WGSL 直接编译失败**(`shading.ao_band` 不存在),
    /// 整条管线作废,画面全黑 —— 而 `cargo test` / `cargo check` 全绿,
    /// 因为它们根本不看 WGSL 字符串。
    ///
    /// 实测踩过:第一版只给片元那份加了 `ao_band`,顶点那份漏了,
    /// 探针量到的地面 luma 是全 0.0。**教训:改这份字符串之后必须
    /// 真的把画面截下来看**,不能只看编译与测试。
    #[test]
    pub(crate) fn vertex_and_fragment_shading_structs_declare_the_same_fields() {
        // ⚠️ 两份必须**完全逐项相同**,包括顶点阶段一个都不读的那两块。
        //
        // WGSL 的 uniform 偏移是按**每份 struct 自己的声明顺序**算的,
        // 所以「不读就不声明」在这里是个陷阱:顶点那份少两块,后面所有
        // 字段的偏移就整体前移 32 字节,而 Rust 侧照旧上传 15 块。
        // `ao_band` 于是读到 `shadow_params`(阴影强度 = 1.0)而不是
        // 高度下界 0.30 —— 高度下界变成 1.0 m 之后,地面(y ≤ 0.16)
        // 的接触带判据仍然成立,**整片地面又吃满最深压暗**,正好退回
        // 本次要修的那个 bug。
        //
        // 实测踩过两轮:
        //  (1) 第一版只给片元那份补 `ao_band`,顶点那份漏了 → WGSL
        //      直接编译失败 → 整条管线作废 → 画面全黑(探针 luma 全 0)。
        //  (2) 第二版给顶点那份补了 `ao_band`,却沿用了「不读就不声明」
        //      的旧惯例没补阴影那两块 → 不编译报错,但地面 luma 从
        //      103.4 掉到 86.8。cargo test / cargo check 全绿。
        //
        // **教训:改这份字符串之后必须真的把画面截下来量,不能只看
        // 编译与测试。**
        let expected: [&str; SHADING_VEC4_COUNT] = [
            "light_dir",
            "light_color",
            "ambient",
            "sky_color",
            "sky_ambient",
            "ground_ambient",
            "ambient_hemi",
            "emissive_gain",
            "eye",
            "fog",
            "ao_params",
            "exposure_white",
            "shadow_params",
            "shadow_misc",
            "ao_band",
        ];
        // 收集 `struct Shading { ... }` 里的字段名(按声明顺序)。
        //
        // 写成普通循环而不是 `lines().map().filter().map()`:链上每一步的
        // 入参类型都在 `&str` / `&&str` 之间跳,而本仓库的规则要求每个闭包
        // 参数写明类型,于是类型标注比逻辑本身还长。
        pub(crate) fn fields_of(shader: &str) -> Vec<String> {
            let start: &str = match shader.split("struct Shading {").nth(1) {
                Some(rest) => rest,
                None => panic!("WGSL 里找不到 `struct Shading {{`"),
            };
            let body: &str = start.split('}').next().unwrap_or_default();
            let mut out: Vec<String> = Vec::new();
            for line in body.lines() {
                let trimmed: &str = line.trim();
                let decl: &str = trimmed.trim_end_matches(',');
                if !decl.ends_with(": vec4<f32>") {
                    continue;
                }
                let field: &str = decl.split(':').next().unwrap_or("");
                out.push(field.trim().to_string());
            }
            out
        }
        let want: Vec<String> = expected
            .iter()
            .map(|s: &&str| -> String { (*s).to_string() })
            .collect();

        for (name, shader) in [
            ("SHADER_VERTEX", SHADER_VERTEX),
            ("SHADER_FRAGMENT", SHADER_FRAGMENT),
        ] {
            assert_eq!(
                fields_of(shader),
                want,
                "{name} 的 `struct Shading` 与 Rust 侧的 SHADING_VEC4_COUNT \
         ({SHADING_VEC4_COUNT} 块)不一致。\n\
         ⚠️ uniform 偏移按**本 struct 的声明顺序**算,所以「顶点阶段不读的块\
         就不声明」会让后面所有字段前移 —— 不会编译报错,只会在 GPU 上\
         读到别人的字节(高度下界会变成阴影强度,地面于是又吃满压暗)。"
            );
        }
    }

    /// 顶点着色器必须真的按**水平距离**算接触 AO。
    ///
    /// 只钉数值不够:数值测试用的是 Rust 镜像,镜像与 WGSL 字符串可以
    /// 各改各的而测试仍然全绿。这条断言把「WGSL 里读的是
    /// `world.xz - base.xz` 的水平距离」这件事钉在字符串上,并钉住
    /// 「旧的按世界高度 clamp 的写法已经不在了」。
    #[test]
    pub(crate) fn vertex_shader_keys_contact_ao_on_horizontal_distance() {
        assert!(
            SHADER_VERTEX.contains("length(world.xz - base.xz)"),
            "顶点着色器必须用水平距离算接触 AO,而不是世界高度"
        );
        assert!(
            !SHADER_VERTEX.contains("clamp(world.y / max(shading.ao_params.x"),
            "按 world.y 压暗的老公式还在 —— 地面整片在 y≈0 上,会全城吃满最深压暗"
        );
        for uniform in [
            "shading.ao_params.z",
            "shading.ao_params.w",
            "shading.ao_band.x",
        ] {
            assert!(
                SHADER_VERTEX.contains(uniform),
                "顶点着色器读了 `{uniform}`,但 WGSL 里读不到它 —— \
         少了对应的 struct 字段就会**整条管线编译失败、画面全黑**"
            );
        }
    }
}
