//! WebGPU 后端的字符串与常量单一来源(rust-standards §1.3c)。
//!
//! 放在 [`crate::render::webgpu`] 自己的 `const.rs` 而不是全局
//! `crate::const`,因为 WebGPU 是**可选后端**:这些标识符只有走
//! `navigator.gpu` 的路径才会被读到,和 WebGL2 的 `u_view_proj` 之类
//! 是两套互不相干的命名空间,混在一起会让两边都难读。
/// `COPY_DST` —— 可被 `writeBuffer` / 复制指令**写入**。
///
/// ⚠️ 按 `GPUBufferUsage` 规范是 **0x0008**,不是 0x0004(那是
/// `COPY_SRC`)。写错一位的后果非常隐蔽:`UNIFORM | 0x0004` = 0x44
/// 在浏览器里正好解��成 `COPY_SRC | UNIFORM`,于是每帧的
/// `writeBuffer` 全部失败,而 **validation scope 要到 draw 之后才报**,
/// 且报的是「usage 不含 CopyDst」—— 画面全黑,矩阵恒为 0,
/// 顶点全部塌到裁剪空间之外。
pub const USAGE_COPY_DST: u32 = 0x0008;

/// `INDEX` —— 可作索引缓冲源。
pub const USAGE_INDEX: u32 = 0x0010;

/// `VERTEX` —— 可作顶点缓冲源。
pub const USAGE_VERTEX: u32 = 0x0020;

/// `UNIFORM` —— 可绑定到 uniform buffer binding。
pub const USAGE_UNIFORM: u32 = 0x0040;

// ---- GPUTextureUsage ----

/// `RENDER_ATTACHMENT` —— 可作渲染目标(深度纹理用)。
pub const USAGE_RENDER_ATTACHMENT: u32 = 0x0010;

// ---- 组合好的实际取值 ----

/// instance buffer 的初始容量(个实例)。
///
/// ⚠️ `draw_batch` 之前**必须**先把 instance buffer 建出来。
/// `setVertexBuffer` 绑 `null` 不会立刻报错,但随后的 `writeBuffer`
/// 写 `null` 会抛,而且 pipeline 布局声明的正是 slot 1 —— 少了这一步,
/// 第一帧会拿着一个 null buffer 去跑 instancing,画面全空,而
/// validation 依旧是 clean。
pub(crate) const INSTANCE_CAPACITY_FLOOR: usize = 4096;

// ===========================================================================
// JS 属性 / 方法名(全部走 `js_sys::Reflect`,不依赖 web-sys 的 GPU 绑定)
// ===========================================================================

/// `navigator` 上的 WebGPU 入口属性名。
pub(crate) const PROP_GPU: &str = "gpu";

/// `GPU` 对象上取画布首选格式的方法名。
pub(crate) const METHOD_GET_PREFERRED_CANVAS_FORMAT: &str = "getPreferredCanvasFormat";

/// `HTMLCanvasElement.getContext` 的 `webgpu` 上下文标识符。
pub(crate) const CONTEXT_WEBGPU: &str = "webgpu";

/// `GPUCanvasContext.configure` 的参数对象键:device。
pub(crate) const FIELD_DEVICE: &str = "device";

/// `GPUCanvasContext.configure` 的参数对象键:format。
pub(crate) const FIELD_CONFIG_FORMAT: &str = "format";

/// `GPUCanvasContext.configure` 的参数对象键:alphaMode。
pub(crate) const FIELD_ALPHA_MODE: &str = "alphaMode";

/// `configure` 使用的 alpha 模式。
pub(crate) const ALPHA_MODE_OPAQUE: &str = "opaque";

// ===========================================================================
// descriptor 字段名(全部是 WebGPU 规范里的字面键名)
// ===========================================================================

/// `GPUShaderModuleDescriptor` / `GPURenderPipelineDescriptor` 的键:layout。
pub(crate) const FIELD_LAYOUT: &str = "layout";

/// 键:module。
pub(crate) const FIELD_MODULE: &str = "module";

/// 键:entryPoint。
pub(crate) const FIELD_ENTRY_POINT: &str = "entryPoint";

/// 键:buffers。
pub(crate) const FIELD_BUFFERS: &str = "buffers";

/// 键:vertex。
pub(crate) const FIELD_VERTEX: &str = "vertex";

/// 键:fragment。
pub(crate) const FIELD_FRAGMENT: &str = "fragment";

/// 键:primitive。
pub(crate) const FIELD_PRIMITIVE: &str = "primitive";

/// 键:depthStencil。
pub(crate) const FIELD_DEPTH_STENCIL: &str = "depthStencil";

/// 键:multisample。
pub(crate) const FIELD_MULTISAMPLE: &str = "multisample";

/// 键:topology。
pub(crate) const FIELD_TOPOLOGY: &str = "topology";

/// 键:cullMode。
pub(crate) const FIELD_CULL_MODE: &str = "cullMode";

/// 键:frontFace。
pub(crate) const FIELD_FRONT_FACE: &str = "frontFace";

/// 键:depthWriteEnabled。
pub(crate) const FIELD_DEPTH_WRITE_ENABLED: &str = "depthWriteEnabled";

/// 键:depthCompare。
pub(crate) const FIELD_DEPTH_COMPARE: &str = "depthCompare";

/// 键:bindGroupLayouts。
pub(crate) const FIELD_BIND_GROUP_LAYOUTS: &str = "bindGroupLayouts";

/// 键:entries。
pub(crate) const FIELD_ENTRIES: &str = "entries";

/// 键:binding。
pub(crate) const FIELD_BINDING: &str = "binding";

/// 键:visibility。
pub(crate) const FIELD_VISIBILITY: &str = "visibility";

/// 键:buffer。
pub(crate) const FIELD_BUFFER: &str = "buffer";

/// 键:resource。
pub(crate) const FIELD_RESOURCE: &str = "resource";

/// 键:size。
pub(crate) const FIELD_SIZE: &str = "size";

/// 键:usage。
pub(crate) const FIELD_USAGE: &str = "usage";

/// 键:code。
pub(crate) const FIELD_CODE: &str = "code";

/// 键:format(深度附件、color target 共用)。
pub(crate) const FIELD_FORMAT: &str = "format";

/// 键:view。
pub(crate) const FIELD_VIEW: &str = "view";

/// 键:colorAttachments。
pub(crate) const FIELD_COLOR_ATTACHMENTS: &str = "colorAttachments";

/// 键:depthStencilAttachment。
pub(crate) const FIELD_DEPTH_STENCIL_ATTACHMENT: &str = "depthStencilAttachment";

/// 键:clearValue。
pub(crate) const FIELD_CLEAR_VALUE: &str = "clearValue";

/// 键:depthClearValue。
pub(crate) const FIELD_DEPTH_CLEAR_VALUE: &str = "depthClearValue";

/// 键:loadOp。
pub(crate) const FIELD_LOAD_OP: &str = "loadOp";

/// 键:depthLoadOp。
pub(crate) const FIELD_DEPTH_LOAD_OP: &str = "depthLoadOp";

/// 键:storeOp。
pub(crate) const FIELD_STORE_OP: &str = "storeOp";

/// 键:depthStoreOp。
pub(crate) const FIELD_DEPTH_STORE_OP: &str = "depthStoreOp";

/// 键:shaderLocation。
pub(crate) const FIELD_SHADER_LOCATION: &str = "shaderLocation";

/// 键:offset。
pub(crate) const FIELD_OFFSET: &str = "offset";

/// 键:arrayStride。
pub(crate) const FIELD_ARRAY_STRIDE: &str = "arrayStride";

/// 键:stepMode。
pub(crate) const FIELD_STEP_MODE: &str = "stepMode";

/// 键:attributes。
pub(crate) const FIELD_ATTRIBUTES: &str = "attributes";

// ===========================================================================
// descriptor 枚举值
// ===========================================================================

/// 顶点着色器入口点名。
pub(crate) const ENTRY_VERTEX: &str = "vs_main";

/// 片元着色器入口点名。
pub(crate) const ENTRY_FRAGMENT: &str = "fs_main";

/// 三角列表拓扑。
pub(crate) const TOPOLOGY_TRIANGLE_LIST: &str = "triangle-list";
/// `frontFace: "ccw"`。
///
/// WebGPU 的默认正面就是逆时针。游戏把 [`super::struct::frame_bytes`]
/// 里矩阵第 1 行取反来适配 WebGPU 的 **+Y 朝下** NDC,而取反 Y 同时会
/// 颠倒屏幕空间绕序,所以这里必须回到 `ccw` —— 与 Y 取反**配套**,不能
/// 只改其中一个。
pub(crate) const FRONT_FACE_CCW: &str = "ccw";

/// 背面剔除。
pub(crate) const CULL_MODE_BACK: &str = "back";

/// `GPUError.message` —— 原型 getter,必须显式读。
pub(crate) const ERROR_FIELD_MESSAGE: &str = "message";

/// `depthCompare: "always"`。
/// 深度测试用 `<`(和 WebGL2 的 `depth_func(LESS)` 逐字对应)。
pub(crate) const DEPTH_COMPARE_LESS: &str = "less";

/// 深度纹理格式。
pub(crate) const DEPTH_FORMAT: &str = "depth24plus";

/// 顶点属性格式:`float32x3`。
pub(crate) const VERTEX_FORMAT_F32X3: &str = "float32x3";

/// 顶点属性格式:`float32x4`(model matrix 的 4 行)。
pub(crate) const VERTEX_FORMAT_F32X4: &str = "float32x4";

/// 顶点缓冲的 stepMode。
pub(crate) const STEP_MODE_VERTEX: &str = "vertex";

/// instance 缓冲的 stepMode —— **instancing 的表达方式**。
pub(crate) const STEP_MODE_INSTANCE: &str = "instance";

/// bind group layout entry 的 buffer type:uniform。
pub(crate) const BUFFER_TYPE_UNIFORM: &str = "uniform";

/// 顶点着色阶段的可见性位。
pub(crate) const VISIBILITY_VERTEX: u32 = 0x0000_0001;

/// 片元着色阶段的可见性位。
pub(crate) const VISIBILITY_FRAGMENT: u32 = 0x0000_0002;

/// `pushErrorScope` 的 scope 名:validation。
pub(crate) const VALIDATION_SCOPE: &str = "validation";

/// 附件的 loadOp:先清。
pub(crate) const LOAD_OP_CLEAR: &str = "clear";

/// 附件的 storeOp:保留。
pub(crate) const STORE_OP_STORE: &str = "store";

/// uniform binding 槽位 0:视投影矩阵。
pub(crate) const BINDING_FRAME: u32 = 0;

/// uniform binding 槽位 1:着色参数。
pub(crate) const BINDING_SHADING: u32 = 1;

// ===========================================================================
// GPU 对象的方法名
// ===========================================================================

/// `GPU.createShaderModule`。
pub(crate) const METHOD_CREATE_SHADER_MODULE: &str = "createShaderModule";

/// `GPU.createBuffer`。
pub(crate) const METHOD_CREATE_BUFFER: &str = "createBuffer";

/// `GPU.createTexture`。
pub(crate) const METHOD_CREATE_TEXTURE: &str = "createTexture";

/// `GPU.createBindGroupLayout`。
pub(crate) const METHOD_CREATE_BIND_GROUP_LAYOUT: &str = "createBindGroupLayout";

/// `GPU.createPipelineLayout`。
pub(crate) const METHOD_CREATE_PIPELINE_LAYOUT: &str = "createPipelineLayout";

/// `GPU.createBindGroup`。
pub(crate) const METHOD_CREATE_BIND_GROUP: &str = "createBindGroup";

/// `GPUCommandEncoder.finish` —— **必须**在 `submit` 之前调用,否则
/// encoder 本身不是 GPUCommandBuffer,`submit` 会报
/// `Failed to convert value to 'GPUCommandBuffer'`。
pub(crate) const METHOD_FINISH: &str = "finish";

/// `GPUDevice.createRenderPipeline`。
pub(crate) const METHOD_CREATE_RENDER_PIPELINE: &str = "createRenderPipeline";

/// `GPU.createCommandEncoder`。
pub(crate) const METHOD_CREATE_COMMAND_ENCODER: &str = "createCommandEncoder";

/// `GPUQueue.writeBuffer`。
pub(crate) const METHOD_WRITE_BUFFER: &str = "writeBuffer";

/// `GPUQueue.submit`。
pub(crate) const METHOD_SUBMIT: &str = "submit";

/// `GPU.device.queue` 属性名。
pub(crate) const METHOD_QUEUE: &str = "queue";

/// `GPUAdapter.requestDevice`。
pub(crate) const METHOD_REQUEST_DEVICE: &str = "requestDevice";

/// `GPUCanvasContext.getCurrentTexture`。
pub(crate) const METHOD_GET_CURRENT_TEXTURE: &str = "getCurrentTexture";

/// `GPUTexture.createView`。
pub(crate) const METHOD_CREATE_VIEW: &str = "createView";

/// `GPUCommandEncoder.beginRenderPass`。
pub(crate) const METHOD_BEGIN_RENDER_PASS: &str = "beginRenderPass";

/// `GPURenderPassEncoder.setPipeline`。
pub(crate) const METHOD_SET_PIPELINE: &str = "setPipeline";

/// `GPURenderPassEncoder.setBindGroup`。
pub(crate) const METHOD_SET_BIND_GROUP: &str = "setBindGroup";

/// `GPURenderPassEncoder.setVertexBuffer`。
pub(crate) const METHOD_SET_VERTEX_BUFFER: &str = "setVertexBuffer";

/// `GPURenderPassEncoder.setIndexBuffer`。
pub(crate) const METHOD_SET_INDEX_BUFFER: &str = "setIndexBuffer";

/// `GPURenderPassEncoder.drawIndexed`(instanced 绘制)。
pub(crate) const METHOD_DRAW_INDEXED: &str = "drawIndexed";

/// `GPURenderPassEncoder.end`。
pub(crate) const METHOD_END: &str = "end";

/// `GPUDevice.pushErrorScope`。
pub(crate) const METHOD_PUSH_ERROR_SCOPE: &str = "pushErrorScope";

/// `GPUDevice.popErrorScope`。
pub(crate) const METHOD_POP_ERROR_SCOPE: &str = "popErrorScope";

/// `Array.prototype.push` —— WebGPU 的 sequence 参数必须是**可迭代对象**。
pub(crate) const METHOD_PUSH: &str = "push";

/// `length` 属性名。
pub(crate) const FIELD_LENGTH: &str = "length";

/// `GPUBufferBindingLayout.type` / `GPUTextureBindingLayout` 用的 `type` 键。
pub(crate) const FIELD_TYPE: &str = "type";

/// `GPUFragmentState.targets`。
pub(crate) const FIELD_TARGETS: &str = "targets";

/// `GPUMultisampleState.count`。
pub(crate) const FIELD_COUNT: &str = "count";

// ===========================================================================
// 其它规范字面量
// ===========================================================================

/// 索引缓冲格式:`uint32`(资产索引就是 `u32`)。
pub(crate) const INDEX_FORMAT_UINT32: &str = "uint32";

/// bind group 下标:本切片只有 group 0。
pub(crate) const BINDING_GROUP: u32 = 0;

/// 顶点缓冲槽位。
pub(crate) const SLOT_VERTEX: u32 = 0;

/// instance 缓冲槽位。
pub(crate) const SLOT_INSTANCE: u32 = 1;

/// 清屏色的 r 分量键。
pub(crate) const CHANNEL_R: &str = "r";

/// 清屏色的 g 分量键。
pub(crate) const CHANNEL_G: &str = "g";

/// 清屏色的 b 分量键。
pub(crate) const CHANNEL_B: &str = "b";

/// 清屏色的 a 分量键。
pub(crate) const CHANNEL_A: &str = "a";

/// 尺寸对象的宽键。
pub(crate) const CHANNEL_WIDTH: &str = "width";

/// 尺寸对象的高键。
pub(crate) const CHANNEL_HEIGHT: &str = "height";

/// pipeline layout 缺失时的错误文本。
pub(crate) const PIPELINE_LAYOUT_MISSING: &str = "pipeline layout missing";

/// 往 null buffer 上写时的错误文本。
pub(crate) const WRITE_BUFFER_NULL: &str = "writeBuffer: buffer is null";

// ===========================================================================
// 渲染管线用的 usage 位掩码
//
// web-sys 的 `GPUBufferUsage` / `GPUTextureUsage` 全部锁在
// `#[cfg(web_sys_unstable_apis)]` 后面,而打开那个开关要改
// `.github/workflows/pages.yml` 的 RUSTFLAGS。这里直接把规范里的
// 位值写成常量 —— 它们是 WebGPU 规范的一部分,不会随实现变。
//
// ⚠️ `GPUBufferUsage` 与 `GPUTextureUsage` 是**两套独立**的位:
// 同一个数值在两个命名空间里含义不同,所以这里分成两组,
// 绝不互相借用位。
// ===========================================================================

// ---- GPUBufferUsage ----

/// 顶点缓冲:`VERTEX | COPY_DST`。
pub(crate) const USAGE_VERTEX_BUFFER: u32 = USAGE_VERTEX | USAGE_COPY_DST;

/// 索引缓冲:`INDEX | COPY_DST`。
pub(crate) const USAGE_INDEX_BUFFER: u32 = USAGE_INDEX | USAGE_COPY_DST;

/// instance 缓冲:与顶点缓冲同 usage(它同样是顶点源)。
pub(crate) const USAGE_INSTANCE_BUFFER: u32 = USAGE_VERTEX | USAGE_COPY_DST;

/// uniform 缓冲:`UNIFORM | COPY_DST`。
pub(crate) const USAGE_UNIFORM_BUFFER: u32 = USAGE_UNIFORM | USAGE_COPY_DST;

/// 深度纹理:`RENDER_ATTACHMENT`。
pub(crate) const USAGE_DEPTH_TEXTURE: u32 = USAGE_RENDER_ATTACHMENT;

// ===========================================================================
// WGSL 着色器
//
// WGSL 与 GLSL ES 3.00 的四处关键差异决定了下面的写法:
//
// 1. **没有 `gl_Position` / `gl_FragColor`。** 入口分别返回
//    `@builtin(position) vec4<f32>` 与 `@location(0) vec4<f32>`,
//    顶点裁剪坐标与深度都从返回值里取。
// 2. **`layout(location = N) in` 要写成 `@location(N)` + 显式类型。**
//    属性偏移与步长由 `GPUVertexBufferLayout` 描述,而不是着色器。
//    WebGPU 只要求 `offset` 和 `arrayStride` 是 4 的倍数(不是
//    Vulkan 的 16),所以 48 B 的顶点步长 + 四个 `float32x3` 直接可用。
// 3. **instancing 来自 `@builtin(instance_index)`**,没有 VAO 概念。
// 4. **uniform buffer 按 16 字节对齐**,所以每个标量都包在 `vec4`
//    里,偏移可以手算(`index * 16`),不必猜 std140 的填充规则。
//
// instance 数据的**字节布局与 WebGL2 端逐字节相同**(20 f32 = 80 B
// 步长:model matrix 4×vec4 + tint vec3 + 1 个 pad),因此
// [`crate::render::Instance`] 一个字节都不用改。
// ===========================================================================

/// 视投影矩阵 uniform buffer 的字节数。
pub(crate) const UNIFORM_FRAME_BYTES: u32 = 64;

/// 着色参数 uniform buffer 的字节数(12 × vec4)。
pub(crate) const UNIFORM_SHADING_BYTES: u32 = 192;

/// WebGPU 顶点着色器。
pub(crate) const SHADER_VERTEX: &str = r#"
struct Frame {
    view_proj : mat4x4<f32>,
};

struct Varyings {
    @builtin(position) clip : vec4<f32>,
    @location(0) normal : vec3<f32>,
    @location(1) color : vec3<f32>,
    @location(2) emissive : vec3<f32>,
    @location(3) tint : vec3<f32>,
    @location(4) world : vec3<f32>,
    @location(5) eye_distance : f32,
    @location(6) contact_ao : f32,
};

struct Shading {
    light_dir : vec4<f32>,
    light_color : vec4<f32>,
    ambient : vec4<f32>,
    sky_color : vec4<f32>,
    sky_ambient : vec4<f32>,
    ground_ambient : vec4<f32>,
    ambient_hemi : vec4<f32>,
    emissive_gain : vec4<f32>,
    eye : vec4<f32>,
    fog : vec4<f32>,
    ao_params : vec4<f32>,
    exposure_white : vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame : Frame;
@group(0) @binding(1) var<uniform> shading : Shading;


@vertex
/// Body of the `vs_main` free function.
///
/// # Arguments
///
/// - `vec3<f32>` - A `vec3<f32>` parameter.
/// - `vec3<f32>` - A `vec3<f32>` parameter.
/// - `vec3<f32>` - A `vec3<f32>` parameter.
/// - `vec3<f32>` - A `vec3<f32>` parameter.
/// - `vec4<f32>` - A `vec4<f32>` parameter.
/// - `vec4<f32>` - A `vec4<f32>` parameter.
/// - `vec4<f32>` - A `vec4<f32>` parameter.
/// - `vec4<f32>` - A `vec4<f32>` parameter.
/// - `vec3<f32>` - A `vec3<f32>` parameter.
/// - `u32` - A 32-bit unsigned integer (`u32`).
///
/// # Returns
///
/// - `Varyings` - A `Varyings` value.
fn vs_main(
    @location(0) position : vec3<f32>,
    @location(1) normal : vec3<f32>,
    @location(2) color : vec3<f32>,
    @location(3) emissive : vec3<f32>,
    @location(4) row0 : vec4<f32>,
    @location(5) row1 : vec4<f32>,
    @location(6) row2 : vec4<f32>,
    @location(7) row3 : vec4<f32>,
    @location(8) tint : vec3<f32>,
    @builtin(instance_index) instance : u32,
) -> Varyings {
    // `instance` 由 WebGPU 自己递增给出,这里断言性地把它绑到整数上:
    // 这条管线里 instancing 的真实数据源是 4..8 号顶点属性,builtin
    // 只用来证明实例编号确实是硬件提供的而不是自己累加的。
    let index : u32 = instance;
    let model : mat4x4<f32> = mat4x4<f32>(row0, row1, row2, row3);
    let world : vec4<f32> = model * vec4<f32>(position, 1.0);
    var out : Varyings;
    out.clip = frame.view_proj * world;

    out.normal = (model * vec4<f32>(normal, 0.0)).xyz;
    out.color = color;
    out.emissive = emissive;
    out.tint = tint;
    out.world = world.xyz;
    out.eye_distance = length(world.xyz - shading.eye.xyz);
    out.contact_ao = mix(
        shading.ao_params.y,
        1.0,
        clamp(world.y / max(shading.ao_params.x, 0.001), 0.0, 1.0),
    );
    // 让优化器保留 builtin 读取:写入一个恒等于 0 的分量不会影响画面。
    out.clip = out.clip + vec4<f32>(0.0, 0.0, 0.0, f32(index) * 0.0);
    return out;
}
"#;

/// WebGPU 片元着色器。
///
/// 光照公式刻意与 [`crate::render::shade_face`] 逐项对应(半球环境光 →
/// 色相保持色调映射 → 接触 AO → 天空色晕染 → smoothstep 雾 → sRGB 编码),
/// 这样两个后端画出来的是同一座城市,而不是「一个亮一个暗」的两种美术。
pub(crate) const SHADER_FRAGMENT: &str = r#"
struct Shading {
    light_dir : vec4<f32>,
    light_color : vec4<f32>,
    ambient : vec4<f32>,
    sky_color : vec4<f32>,
    sky_ambient : vec4<f32>,
    ground_ambient : vec4<f32>,
    ambient_hemi : vec4<f32>,
    emissive_gain : vec4<f32>,
    eye : vec4<f32>,
    fog : vec4<f32>,
    ao_params : vec4<f32>,
    exposure_white : vec4<f32>,
};

@group(0) @binding(1) var<uniform> shading : Shading;

/// Body of the `tonemap_luma` free function.
///
/// # Arguments
///
/// - `f32` - A 32-bit float (`f32`).
/// - `f32` - A 32-bit float (`f32`).
/// - `f32` - A 32-bit float (`f32`).
///
/// # Returns
///
/// - `f32` - A 32-bit float.
fn tonemap_luma(luma : f32, exposure : f32, white : f32) -> f32 {
    let w : f32 = select(1.0, white, white > 0.0000001);
    let x : f32 = max(luma, 0.0) * exposure;
    return (x * (1.0 + x / (w * w))) / (1.0 + x);
}

// 色相保持:只压亮度再按比例缩回 RGB,保住粉/薄荷这类浅色的色相。
// 逐通道 clamp 会把 1.5 亮度的粉红面去色成纯白 —— 那正是「一片惨白」。
/// Body of the `tonemap` free function.
///
/// # Arguments
///
/// - `vec3<f32>` - A `vec3<f32>` parameter.
/// - `f32` - A 32-bit float (`f32`).
/// - `f32` - A 32-bit float (`f32`).
///
/// # Returns
///
/// - `vec3<f32>` - A `vec3<f32>` value.
fn tonemap(value : vec3<f32>, exposure : f32, white : f32) -> vec3<f32> {
    let luma : f32 = dot(value, vec3<f32>(0.2126, 0.7152, 0.0722));
    if (luma <= 0.000001) {
        return vec3<f32>(0.0, 0.0, 0.0);
    }
    let mapped : f32 = tonemap_luma(luma, exposure, white);
    let divisor : f32 = luma * exposure;
    let scale : f32 = select(1.0, mapped / divisor, divisor > 0.0000001);
    return value * scale;
}

/// Body of the `linear_to_srgb` free function.
///
/// # Arguments
///
/// - `vec3<f32>` - A `vec3<f32>` parameter.
///
/// # Returns
///
/// - `vec3<f32>` - A `vec3<f32>` value.
fn linear_to_srgb(value : vec3<f32>) -> vec3<f32> {
    let c : vec3<f32> = clamp(value, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo : vec3<f32> = c * 12.92;
    let hi : vec3<f32> = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
/// Body of the `fs_main` free function.
///
/// # Arguments
///
/// - `vec3<f32>` - A `vec3<f32>` parameter.
/// - `vec3<f32>` - A `vec3<f32>` parameter.
/// - `vec3<f32>` - A `vec3<f32>` parameter.
/// - `vec3<f32>` - A `vec3<f32>` parameter.
/// - `vec3<f32>` - A `vec3<f32>` parameter.
/// - `f32` - A 32-bit float (`f32`).
/// - `f32` - A 32-bit float (`f32`).
///
/// # Returns
///
/// - `@location(0) vec4<f32>` - A `@location(0) vec4<f32>` value.
fn fs_main(
    @location(0) normal : vec3<f32>,
    @location(1) color : vec3<f32>,
    @location(2) emissive : vec3<f32>,
    @location(3) tint : vec3<f32>,
    @location(4) world : vec3<f32>,
    @location(5) eye_distance : f32,
    @location(6) contact_ao : f32,
) -> @location(0) vec4<f32> {
    let _unused_world : vec3<f32> = world;
    let n : vec3<f32> = normalize(normal);
    let n_dot_l : f32 = max(dot(n, shading.light_dir.xyz), 0.0);
    let hemi_weight : f32 = n.y * 0.5 + 0.5;
    let hemi : vec3<f32> = mix(shading.ground_ambient.xyz, shading.sky_ambient.xyz, hemi_weight);
    let ambient : vec3<f32> = mix(shading.ambient.xyz, hemi, shading.ambient_hemi.x);
    let base : vec3<f32> = color * tint;
    let lit : vec3<f32> = base * (ambient + shading.light_color.xyz * n_dot_l)
        + base * emissive * shading.emissive_gain.x;
    var mapped : vec3<f32> = tonemap(lit, shading.exposure_white.x, shading.exposure_white.y);
    mapped = mapped * contact_ao;
    mapped = mapped + shading.sky_color.xyz * 0.012;
    let span : f32 = max(shading.fog.y - shading.fog.x, 0.0001);
    var fog : f32 = clamp((eye_distance - shading.fog.x) / span, 0.0, 1.0);
    fog = fog * fog * (3.0 - 2.0 * fog);
    let energy : f32 = dot(base * emissive, vec3<f32>(1.0));
    if (energy > 0.01) {
        mapped = mapped + shading.sky_color.xyz * min(fog * 0.72, 0.72);
    }
    mapped = mix(mapped, shading.sky_color.xyz, fog);
    return vec4<f32>(linear_to_srgb(mapped), 1.0);
}
"#;

pub(crate) const GPU_ERR_NO_NAVIGATOR: &str = "window.navigator unavailable";

pub(crate) const GPU_ERR_NO_GPU_OBJECT: &str = "navigator.gpu undefined";

pub(crate) const GPU_ERR_NO_ADAPTER: &str = "requestAdapter() resolved null";

pub(crate) const GPU_ERR_NO_DEVICE: &str = "requestDevice() failed";

pub(crate) const GPU_ERR_NO_CONTEXT: &str = "getContext(webgpu) returned null";

pub(crate) const GPU_ERR_CONFIGURE_FAILED: &str = "context.configure failed";
