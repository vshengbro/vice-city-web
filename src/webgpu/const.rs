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

// ---- 阴影贴图专用(compare sampler / sampler binding)----

/// `GPUSamplerBindingLayout.type` —— `comparison`(而非 `filtering`)。
///
/// ⚠️ 深度纹理**必须**用 `comparison`:它让采样硬件自己做
/// `refOp(depth) vs storedDepth` 并返回 0/1,而不是把深度当普通
/// 浮点纹理读回来。用 `filtering` 配 `texture_depth_2d` 会被 WebGPU
/// 判为 validation error(且是异步的:画面纯黑,console 一句话没有)。
pub(crate) const SAMPLER_TYPE_COMPARISON: &str = "comparison";

/// `GPUSamplerBindingLayout` / `GPUSamplerDescriptor` 的 `compare` 键。
pub(crate) const FIELD_COMPARE: &str = "compare";

/// `GPUSamplerDescriptor.compare` = `"less"`。
///
/// 语义与 WebGL2 那条手写比较 `compare_depth > stored ? 0 : 1` 逐字
/// 对应:参考值(当前片元深度)小于存下来的遮挡体深度 → 全亮。
pub(crate) const COMPARE_LESS: &str = "less";

/// `GPUSamplerDescriptor.magFilter` / `minFilter` = `"linear"`。
///
/// 比较采样也支持线性过滤,而且**必须**用线性:PCF 的半影就是靠
/// 硬件在 2×2 邻域内插值出来的。给 `"nearest"` 的话 3×3 核会退化成
/// 9 个方块状硬边,半影全丢。
pub(crate) const FILTER_LINEAR: &str = "linear";

/// `GPUSamplerDescriptor` 的 `magFilter` 键。
pub(crate) const FIELD_MAG_FILTER: &str = "magFilter";

/// `GPUSamplerDescriptor` 的 `minFilter` 键。
pub(crate) const FIELD_MIN_FILTER: &str = "minFilter";

/// `GPUBindGroupLayoutEntry.texture` —— 绑一张纹理(而非 buffer)。
pub(crate) const FIELD_TEXTURE: &str = "texture";

/// `GPUBindGroupLayoutEntry.sampler` —— 绑一个采样器。
pub(crate) const FIELD_SAMPLER: &str = "sampler";

/// `GPUTextureBindingLayout.sampleType` = `"depth"`。
///
/// ⚠️ 深度纹理必须声明 `"depth"`,采样器配套必须是
/// `sampler_comparison` + `compare`。这一对组合起来才是 WebGPU 的
/// 「阴影比较采样」;少了任何一半都是 validation error。
pub(crate) const SAMPLE_TYPE_DEPTH: &str = "depth";

// ===========================================================================
// descriptor 枚举值
// ===========================================================================

/// 顶点着色器入口点名。
pub(crate) const ENTRY_VERTEX: &str = "vs_main";

/// 片元着色器入口点名。
pub(crate) const ENTRY_FRAGMENT: &str = "fs_main";

/// 阴影 pass 的顶点着色器入口点名。
///
/// 与主 pass **刻意分开**:`SHADER_VERTEX` 里给光照用的那些 varying
/// 在阴影 pass 一概不需要,只留 `@builtin(position)`。但 WebGPU 的
/// `entryPoint` 是**按名字**查的,同名不等于同一段代码 —— 见
/// [`SHADER_SHADOW_VERTEX`]:阴影管线必须有一条自己那份、把实例变换
/// 乘进光源矩阵的顶点着色器。
pub(crate) const ENTRY_SHADOW_VERTEX: &str = "vs_shadow";

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

/// **不**剔除(`cullMode: "none"`)。
///
/// 后处理那三条 pass 用它:全屏三角形只有一面,没有「正反面」可言,
/// 而 WGSL 那边算出来的绕序不必依赖(也不该依赖)画布的上下方向。
pub(crate) const CULL_MODE_NONE: &str = "none";

/// `GPUError.message` —— 原型 getter,必须显式读。
pub(crate) const ERROR_FIELD_MESSAGE: &str = "message";

/// `depthCompare: "always"`。
/// 深度测试用 `<`(和 WebGL2 的 `depth_func(LESS)` 逐字对应)。
pub(crate) const DEPTH_COMPARE_LESS: &str = "less";

/// 深度纹理格式。
pub(crate) const DEPTH_FORMAT: &str = "depth24plus";

/// 阴影深度纹理格式:`depth32float`。
///
/// 画布深度用 `depth24plus`(WebGPU 唯一**被采样**保证的性质要靠
/// `depth24plus`,它带实现定义的不可压缩保证),但阴影贴图是
/// `depth32float` —— 理由和 WebGL2 端换成 `DEPTH_COMPONENT24` 一样:
/// 阴影比较需要**跨整个正交深度范围**都稳定可靠的数值,24 位整数深度
/// 在 near=1 / far=320 的长条区间上,近处的深度步进会大到足以让
/// 远处整片地面自阴影痤疮。
///
/// ⚠️ 换格式 = 偏置常数要重调,不是回归。见
/// [`SHADOW_DEPTH_BIAS_SCALE`] / [`SHADOW_NORMAL_OFFSET_SCALE`]:
/// 本切片的 depth bias 是在 WebGPU 的 `[0, 1]` NDC 深度域里算的
/// (与 WebGL2 的 `[-1, 1]` 不同),所以按 WebGPU 的深度跨度重新标定。
pub(crate) const SHADOW_DEPTH_FORMAT: &str = "depth32float";

/// 阴影 pass 剔**背面**(`cullMode: "front"`)。
///
/// 对应 WebGL2 端的 `cull_face(FRONT)`:阴影贴图存的是「从光看过去
/// 最靠后的表面」,拿背面当遮挡体能把自阴影痤疮与 Peter-Panning
/// 一起压掉一个量级,这是阴影贴图最经典的一招。
pub(crate) const CULL_MODE_FRONT: &str = "front";

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

/// 纹理 binding 槽位 0:阴影深度图(`texture_depth_2d`)。
///
/// 放在 **group 1** 而不是挤进 group 0:阴影管线是一条**独立的**
/// pipeline,它只有 binding 0(光源视投影矩阵),而主 pipeline 的
/// group 0 是 frame + shading。把阴影纹理塞进 group 0 就得让阴影管线
/// 也声明那两个 uniform(`layout` 不一致会被 WebGPU 判为 invalid),
/// 分离成两个 group 之后两条管线各自的布局都干净。
///
/// ⚠️ 本机 `maxBindGroups` 是规范下限 **4**,group 0 = 主 uniform、
/// group 1 = 阴影贴图,只用掉 2 个。
pub(crate) const BINDING_SHADOW_MAP: u32 = 0;

/// 采样器 binding 槽位 0:阴影图的**比较**采样器。
///
/// WebGPU 没有「普通 sampler 采深度纹理」这种东西:深度格式必须配
/// `sampler_comparison`,由硬件在采样时完成 PCF 比较。
pub(crate) const BINDING_SHADOW_SAMPLER: u32 = 1;

/// 光源视投影矩阵在 **group 1** 里的 binding 槽位。
///
/// ⚠️ 与 [`BINDING_FRAME`](上,group 0 的槽位 0) 是**不同的槽位**:
/// 两条管线各有自己的 bind group,同一个物理 buffer 被绑在两处 ——
/// 阴影管线(group 0 slot 0)用它做顶点变换,主管线(group 1 slot 2)
/// 用它在片元阶段把世界坐标投到光空间。
pub(crate) const BINDING_SHADOW_FRAME: u32 = 2;

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

/// `GPUDevice.createSampler`.
pub(crate) const METHOD_CREATE_SAMPLER: &str = "createSampler";

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

/// `GPURenderPassEncoder.draw(vertexCount)`。
///
/// 全屏三角形那条 pass 用它 —— 与几何 pass 的 `drawIndexed` 是两个
/// 方法名,拿错会得到 `drawIndexed is not a function`。
pub(crate) const METHOD_DRAW: &str = "draw";

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

/// `GPUTextureBindingLayout.sampleType` 键。
pub(crate) const FIELD_SAMPLE_TYPE: &str = "sampleType";

/// `GPUFragmentState.targets`。
pub(crate) const FIELD_TARGETS: &str = "targets";

/// `GPUMultisampleState.count`。
pub(crate) const FIELD_COUNT: &str = "count";

// ===========================================================================
// bloom(亮度提取 + 可分离高斯模糊 + 合成)
// ===========================================================================

/// `GPUTextureBindingLayout.sampleType` = `"float"`。
///
/// ⚠️ bloom 那两张颜色纹理必须声明成 `float`(而不是阴影那张的
/// `depth`):声明错采样类型时,绑 `rgba8unorm` 会被判 invalid,
/// 而且错误是**异步**报上来的 —— 画面直接黑,console 里一个字都没有。
pub(crate) const SAMPLE_TYPE_FLOAT: &str = "float";

/// `GPUSamplerBindingLayout.type` = `"filtering"`。
///
/// ⚠️ **与阴影那张 `comparison` 是两回事。** bloom 的 `rgba8unorm`
/// 颜色纹理只能配 `sampler`(filtering / non-filtering),配
/// `sampler_comparison` 会被 WebGPU 判 invalid。
pub(crate) const SAMPLER_TYPE_FILTERING: &str = "filtering";

/// bloom 全屏三角形顶点着色器的入口点名。
///
/// 与主管线的 [`ENTRY_VERTEX`]:**刻意分开** —— 后处理 pass 用的是
/// 「一个覆盖裁剪空间的大三角形,`vertex_index` 自己算位置」,
/// 不吃任何顶点缓冲,和几何那条管线没有一点共同之处。
pub(crate) const ENTRY_FULLSCREEN_VERTEX: &str = "vs_fullscreen";

/// bloom 亮度提取的片元入口点名。
pub(crate) const ENTRY_BLOOM_EXTRACT: &str = "fs_extract";

/// bloom **水平**模糊的片元入口点名。
pub(crate) const ENTRY_BLOOM_BLUR_H: &str = "fs_blur_h";

/// bloom **竖直**模糊的片元入口点名。
pub(crate) const ENTRY_BLOOM_BLUR_V: &str = "fs_blur_v";

/// bloom 合成的片元入口点名。
pub(crate) const ENTRY_BLOOM_COMPOSITE: &str = "fs_composite";

/// bloom uniform 的 binding 槽位 0:阈值 / 强度 / 模糊核 / texel 尺寸。
pub(crate) const BINDING_BLOOM_PARAMS: u32 = 0;

/// bloom 颜色输入的采样器 binding 槽位。
///
/// 提取 / 模糊 / 合成**共用**这一个槽位(形状都是
/// `GPUSamplerBindingLayout` 的 `filtering`),所以合成那条 5 槽
/// layout 里它出现两次(第 1 与第 3 槽)。
pub(crate) const BINDING_BLOOM_SAMPLER: u32 = 1;

/// bloom 颜色输入的纹理 binding 槽位。
pub(crate) const BINDING_BLOOM_SOURCE: u32 = 2;

/// bloom 模糊图在**合成** layout 里的第二个采样器槽位。
pub(crate) const BINDING_BLOOM_BLUR_SAMPLER: u32 = 3;

/// bloom 模糊图在**合成** layout 里的第二个纹理槽位。
pub(crate) const BINDING_BLOOM_BLUR_MAP: u32 = 4;

/// bloom 离屏颜色目标:`RENDER_ATTACHMENT | TEXTURE_BINDING`。
///
/// 四张(scene / bright / ping / pong)都要读写两遍:被上一个 pass 写
/// 进去,又被下一个 pass 当 `texture_2d<f32>` 采样回来。少
/// `TEXTURE_BINDING` 的症状是 validation error 指向 bind group 条目
/// 而不是纹理本身。
pub(crate) const USAGE_BLOOM_TARGET: u32 = USAGE_RENDER_ATTACHMENT | USAGE_TEXTURE_BINDING;

/// bloom 三张半分辨率目标的颜色格式。
///
/// ⚠️ **不等于**画布的 preferred format(`bgra8unorm`)。bloom 中间
/// 目标用 `rgba8unorm` 就够,而且它是**被 pipeline 声明**的格式 ——
/// 半分辨率目标上写 `bgra8unorm` 也能跑,但没有任何理由(那些数据
/// 从不上屏)。只有**合成**那条必须与画布一致,见
/// `build_bloom_resources_inner` 里那段说明。
pub(crate) const BLOOM_TARGET_FORMAT: &str = "rgba8unorm";

/// `build_bloom_layout` 的形状选择:提取 / 两条模糊(3 槽)。
pub(crate) const BLOOM_LAYOUT_SHAPE_PLAIN: bool = false;

/// `build_bloom_layout` 的形状选择:合成(5 槽)。
pub(crate) const BLOOM_LAYOUT_SHAPE_COMPOSITE: bool = true;

/// bloom uniform buffer 的字节数(3 × vec4 = 48 字节)。
///
/// ⚠️ 三块 vec4:`params`(阈值 / 强度 / 方向)、`kernel`(核权重前 4 个)、
/// `texel`(第 5 个权重 + 纹素尺寸)。这个数**必须**跟着
/// [`crate::webgpu::r#struct::BloomUniforms`] 与
/// [`crate::webgpu::r#struct::BLOOM_VEC4_COUNT`] 一起改:少分配的话
/// `writeBuffer` 会直接抛 `writeBuffer size exceeds buffer size`,
/// 而不是静默截断。
pub(crate) const UNIFORM_BLOOM_BYTES: u32 =
    crate::webgpu::r#struct::BLOOM_VEC4_COUNT as u32 * 16;

/// bloom 高斯核的 5 个权重(中心 + 4 个对称抽头)。
///
/// 与 WebGL2 端 `BLOOM_BLUR_FRAGMENT_SHADER` 里那个
/// `WEIGHTS[5] = float[5](0.227027, 0.1945946, 0.1216216, 0.054054,
/// 0.016216)` **逐项相同**,而且是 9 抽头线性采样优化的高斯核
/// (等价于 9 个独立抽头,只用 5 次纹理读取)。
///
/// ⚠️ 必须留在 Rust 侧再上传成 uniform,而不是抄进 WGSL 字符串:
/// 这样 `bloom_blur_kernel_weights_sum_to_one` 才能钉住「归一化」这个
/// 性质 —— 一旦有人把某个权重改成 0.2 而忘了重新归一化,整幅画面会
/// 莫名其妙变暗 10%,而没有任何编译错误。
pub(crate) const BLOOM_BLUR_WEIGHTS: [f32; 5] =
    [0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216];

// ===========================================================================
// 其它规范字面量
// ===========================================================================

/// 索引缓冲格式:`uint32`(资产索引就是 `u32`)。
pub(crate) const INDEX_FORMAT_UINT32: &str = "uint32";

/// bind group 下标:主 uniform(frame + shading)。
pub(crate) const BINDING_GROUP: u32 = 0;

/// bind group 下标:阴影贴图 + 比较采样器。
///
/// 见 [`BINDING_SHADOW_MAP`] 的说明:两条管线共用 group 0 的**形状**
/// 是做不到的(阴影管线没有 frame / shading),所以阴影资源单开
/// group 1。`maxBindGroups` 规范下限 4,这里只用 0 和 1。
pub(crate) const BINDING_GROUP_SHADOW: u32 = 1;

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

// ---- GPUTextureUsage ----

/// `TEXTURE_BINDING` —— 可作 bind group 里的纹理资源。
///
/// ⚠️ **阴影深度纹理必须加上这一位**,不只是 `RENDER_ATTACHMENT`:
/// 它既要被阴影 pass 写进去(`RENDER_ATTACHMENT`),又要被主管线
/// 的片元着色器采样回来(`TEXTURE_BINDING`)。少一位的症状极隐蔽:
///
/// ```text
/// [TextureView ...] usage (TextureUsage::RenderAttachment) doesn't
/// include TextureUsage::TextureBinding.
///  - While validating entries[0] against { binding: 0, ... texture ... }
/// ```
///
/// 注意这是 `GPUTextureUsage` 的 **0x0004**,与 `GPUBufferUsage` 的
/// `COPY_SRC`(同样是 0x0004)是两套独立命名空间 —— 两个 usage
/// 掩码绝不能混用(见上面那段关于两组位掩码的说明)。
pub(crate) const USAGE_TEXTURE_BINDING: u32 = 0x0004;

/// 阴影深度纹理:`RENDER_ATTACHMENT | TEXTURE_BINDING`。
///
/// 见 [`USAGE_TEXTURE_BINDING`]:这张图既要写也要读。
pub(crate) const USAGE_SHADOW_TEXTURE: u32 = USAGE_DEPTH_TEXTURE | USAGE_TEXTURE_BINDING;

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

/// 光源视投影矩阵 uniform buffer 的字节数。
pub(crate) const UNIFORM_SHADOW_FRAME_BYTES: u32 = 64;

/// 着色参数 uniform buffer 的字节数(14 × vec4 = 224 字节)。
///
/// ⚠️ 阴影那两块 vec4 加进来之后从 192 涨到 224。这个数**必须**跟着
/// [`crate::webgpu::r#struct::ShadingUniforms`] 的字段数一起改:
/// 少分配的话 `writeBuffer` 会直接抛
/// `writeBuffer size exceeds buffer size`,而不是静默截断。
pub(crate) const UNIFORM_SHADING_BYTES: u32 = 14 * 16;

// ===========================================================================
// 阴影偏置的 WebGPU 标定
//
// WebGL2 端那两行偏置是在 **OpenGL 的 `[-1, 1]` NDC 深度域**里算的:
//
//     float bias_in_ndc = bias * 2.0 / 1024.0 * (u_shadow_params.z + 1.0);
//
// 那个 `2.0` 是 GL 的深度跨度(`z ∈ [-1,1]`),`1024.0` 是个魔数。
// WebGPU 的 NDC 深度是 `[0, 1]`,跨度只有一半,所以整套偏置必须
// 重新标定 —— **照抄 GLSL 的系数会直接导致影子整体偏移**(要么
// 全痤疮,要么整个影子浮起来)。
//
// 正确的标定方式是从**深度跨度**出发:正交投影下深度是线性的,所以
// 「NDC 深度 1.0」对应 `(SHADOW_FAR - SHADOW_NEAR)` 米。因此
// 「一个纹素在世界空间的高度」折算成深度,只要拿深度跨度去除。
//
// 这里把三个数拆开,各自对应一个可独立调的量:
//   - 纹素世界尺寸:由 [`crate::render::shadow_texel_world_size`] 给,
//     它是 `2 * SHADOW_HALF_EXTENT / SHADOW_MAP_SIZE`。
//   - 深度偏置倍数:[`SHADOW_DEPTH_BIAS_SCALE`],乘在纹素上。
//   - 法线偏移倍数:[`SHADOW_NORMAL_OFFSET_SCALE`],乘在纹素上。
// ===========================================================================

/// WebGPU NDC 深度 `[0, 1]` 对应的世界深度跨度(米)。
///
/// 正交投影下深度线性,所以 `1.0` 深度 = `SHADOW_FAR - SHADOW_NEAR`
/// 米 —— 这是偏置从「纹素(米)」换算到「深度」的换算系数来源。
pub(crate) const SHADOW_DEPTH_SPAN_M: f32 = 320.0 - 1.0;

/// 深度偏置倍数(以纹素为单位)。
///
/// `1.0` 就是「把比较点沿光线推离表面一个纹素」,约 4.4 cm。
/// 斜率缩放(见 [`SHADOW_BIAS_SLOPE_GAIN`])在掠射面上还会再放大它。
///
/// 调这个值时看什么:地面出现平行条纹 = **偏小**(痤疮);影子整体
/// 从物体脚下脱开 = **偏大**(Peter-Panning)。
pub(crate) const SHADOW_DEPTH_BIAS_SCALE: f32 = 1.6;

/// 法线偏移倍数(以纹素为单位)。
///
/// 沿世界法线把比较点推离表面,专治自阴影痤疮;掠射面推得更远。
pub(crate) const SHADOW_NORMAL_OFFSET_SCALE: f32 = 1.4;

/// 斜率缩放的深度偏置增益:掠射面上的额外深度偏置
/// = `增益 × (1 - n·l)`。
///
/// 与 WebGL2 端 GLSL 里那个写死的 `3.0` 同一个形状,但这里**跟着
/// uniform 上传**(见 `ShadingUniforms::shadow_misc.z`),不写死在
/// WGSL 字符串里 —— 调偏置时只改 `const.rs` 这一处,不必重新编译
/// 着色器字符串再肉眼核对 GLSL 与 WGSL 两份魔数有没有同步。
pub(crate) const SHADOW_BIAS_SLOPE_GAIN: f32 = 3.0;

/// 斜率缩放的法线偏移增益(同 [`SHADOW_BIAS_SLOPE_GAIN`] 的理由,
/// 上传到 `ShadingUniforms::shadow_misc.w`)。
///
/// 与 WebGL2 端 GLSL 里那个写死的 `2.0` 同一个形状。
pub(crate) const SHADOW_NORMAL_OFFSET_SLOPE_GAIN: f32 = 2.0;

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

/// WebGPU 阴影 pass 的顶点着色器。
///
/// 对应 WebGL2 端的 `SHADOW_VERTEX_SHADER`:复用**同一套 instance
/// 布局**,只把 `u_view_proj` 换成光源视投影矩阵 —— 所以阴影 pass
/// 与主 pass 吃的是同一份顶点 / 索引 / instance buffer,不需要第二条
/// 几何上传路径。
///
/// ⚠️ 只需要 `@location(0)`(position)与 `@location(4..8)`
/// (model matrix);`normal` / `color` / `emissive` / `tint` 一概不读。
/// 顶点 buffer layout 里它们**仍然声明着**(见
/// [`crate::webgpu::WebGpuRenderer`] 建的 `build_vertex_layout`),而
/// WebGPU 允许管线声明的 location 集合**多于**着色器实际读取的集合 ——
/// 少声明才会触发「着色器读了未声明的 location」validation error。
///
/// @group(0) 的 binding 0 是**光源**视投影矩阵,不是相机那一份:两条
/// 管线各有各的 pipeline layout,同一个 binding 号在它们里面指的是
/// 不同的 buffer。
pub(crate) const SHADER_SHADOW_VERTEX: &str = r#"
struct ShadowFrame {
    view_proj : mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> shadow_frame : ShadowFrame;

@vertex
fn vs_shadow(
    @location(0) position : vec3<f32>,
    @location(4) row0 : vec4<f32>,
    @location(5) row1 : vec4<f32>,
    @location(6) row2 : vec4<f32>,
    @location(7) row3 : vec4<f32>,
) -> @builtin(position) vec4<f32> {
    let model : mat4x4<f32> = mat4x4<f32>(row0, row1, row2, row3);
    let world : vec4<f32> = model * vec4<f32>(position, 1.0);
    let clip : vec4<f32> = shadow_frame.view_proj * world;
    return clip;
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
    // x = 阴影强度, y = PCF 半径(纹素), z = 深度偏置(纹素),
    // w = 法线偏移(纹素), 与 WebGL2 端 `u_shadow_params` 逐项一致。
    shadow_params : vec4<f32>,
    // x = 一个纹素覆盖的世界尺寸(米), y = WebGPU 深度域跨度(米)。
    shadow_misc : vec4<f32>,
};

// ---- 阴影(见 WebGL2 端 FRAGMENT_SHADER 的「---- 阴影 ----」一段)----
//
// 阴影资源放在 **group 1**:group 0 是主 pass 的 frame + shading,
// 阴影管线的 group 0 只有光源矩阵(见 SHADER_SHADOW_VERTEX),
// 两者的 group 0 形状不同,不能共用。
//
// 光源矩阵**不塞进** `Shading` —— 它要按每帧上传的独立 uniform buffer
// 走(binding 2),和 shading 那 192 字节的静态块分开。压成 vec4 对再
// 拼回去,既多 16 字节又让矩阵的下标变成手算偏移,不值。
@group(0) @binding(1) var<uniform> shading : Shading;

@group(1) @binding(0) var shadow_map : texture_depth_2d;
@group(1) @binding(1) var shadow_sampler : sampler_comparison;

struct ShadowFrame {
    view_proj : mat4x4<f32>,
};

@group(1) @binding(2) var<uniform> shadow_frame : ShadowFrame;

fn tonemap_luma(luma : f32, exposure : f32, white : f32) -> f32 {
    let w : f32 = select(1.0, white, white > 0.0000001);
    let x : f32 = max(luma, 0.0) * exposure;
    return (x * (1.0 + x / (w * w))) / (1.0 + x);
}

// 色相保持:只压亮度再按比例缩回 RGB,保住粉/薄荷这类浅色的色相。
// 逐通道 clamp 会把 1.5 亮度的粉红面去色成纯白 —— 那正是「一片惨白」。
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

fn linear_to_srgb(value : vec3<f32>) -> vec3<f32> {
    let c : vec3<f32> = clamp(value, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo : vec3<f32> = c * 12.92;
    let hi : vec3<f32> = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

/// 3×3 PCF 阴影。返回 1 = 全亮,0 = 全暗。
///
/// 与 WebGL2 端 `FRAGMENT_SHADER` 的 `sample_shadow` 逐项对应,但有
/// **一处必须重算**:NDC y 与纹理 v 的对应关系。
///
/// ⚠️ 这里实测过(见 `tools/gpu-probe` 侧的 ndc-y 探针):WebGPU 的
/// NDC **+y 在屏幕上方**,与 OpenGL 一致。所以纹理坐标是
/// `v = 0.5 - ndc.y * 0.5`,**不是** `0.5 + ndc.y * 0.5`。
/// 写反了影子会整体上下颠倒 —— 太阳在东,影子却落在西。
///
/// 另外 WebGPU 的 NDC 深度是 `[0, 1]`(GL 是 `[-1, 1]`),所以
/// `current` **不需要**再乘 `0.5 + 0.5`;而纹理里存下来的深度已经
/// 是 `[0, 1]` 的设备深度,可以直接喂给 `textureSampleCompare`。
fn sample_shadow(world : vec3<f32>, n_dot_l : f32) -> f32 {
    let light_clip : vec4<f32> = shadow_frame.view_proj * vec4<f32>(world, 1.0);
    let ndc : vec3<f32> = light_clip.xyz / light_clip.w;
    // 走出 shadow frustum 的地方没有数据,判全亮而不是判全黑 ——
    // 判全黑会让视锥边界出现一圈整齐的黑框。
    let inside : bool = ndc.x >= -1.0 && ndc.x <= 1.0 && ndc.y >= -1.0
        && ndc.y <= 1.0 && ndc.z >= 0.0 && ndc.z <= 1.0;
    // ⚠️ y 取反:WebGPU 的 NDC +y 在上方(实测),而纹理 v=0 在**上方**
    // 那一行,所以 v 要跟着翻。见本函数 doc 的说明。
    let uv : vec2<f32> = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    // WebGPU 深度域已经是 [0, 1],直接就是纹理里存的那个值。
    var current : f32 = ndc.z;

    // 斜率缩放偏置:掠射面(light_dir 与法线夹角大)的深度梯度最陡,
    // 固定偏置在这种面上必然要么痤疮要么 Peter-Panning。
    let slope : f32 = clamp(1.0 - n_dot_l, 0.0, 1.0);
    let bias_texels : f32 = shading.shadow_params.z
        * (1.0 + shading.shadow_misc.z * slope);
    // 一个纹素覆盖多少世界距离(米)→ 折算成 WebGPU `[0, 1]` 深度域。
    // 正交投影下深度线性,所以换算就是「世界高度 / 深度跨度」。
    let texel_world : f32 = shading.shadow_misc.x;
    let depth_span : f32 = shading.shadow_misc.y;
    let bias_in_depth : f32 = bias_texels * texel_world / max(depth_span, 0.001);
    current = current - bias_in_depth;

    let radius : f32 = max(shading.shadow_params.y, 0.0);
    // 纹素尺寸换算成 uv 单位:1 / 阴影贴图边长。
    // WebGPU 里有 `textureDimensions()`,比再传一个 uniform 可靠 ——
    // 它永远等于实际分配的贴图边长,不会和 resize 后的贴图脱节。
    let dims : vec2<f32> = vec2<f32>(textureDimensions(shadow_map));
    let texel_uv : f32 = 1.0 / max(dims.x, 1.0);
    // ⚠️ **循环与采样都不许放进 `if` 里。**
    //
    // `textureSampleCompare` 属于「隐式求导」的纹理采样(它自带 mip
    // 选择与过滤),WGSL 规定它只能在**一致控制流**里调用 —— 写在
    // `if (outside) { return 1.0; }` 之后就是非一致控制流,着色器
    // **编译失败**(validation error,画面直接黑)。
    //
    // 所以判据只用来 `select` 一个结果:采样无条件跑完,再用
    // `inside` 与「强度是否为零」把结果调回 1.0。
    var visibility : f32 = 0.0;
    for (var x : i32 = -1; x <= 1; x = x + 1) {
        for (var y : i32 = -1; y <= 1; y = y + 1) {
            let offset : vec2<f32> = vec2<f32>(f32(x), f32(y)) * radius * texel_uv;
            visibility = visibility + textureSampleCompare(
                shadow_map, shadow_sampler, uv + offset, current);
        }
    }
    let pcf : f32 = visibility / 9.0;
    let lit : f32 = select(1.0, pcf, inside);
    // 强度为 0 时直接返回 1 —— 夜晚关掉阴影时连一次采样都不做。
    return select(lit, 1.0, shading.shadow_params.x <= 0.0);
}

@fragment
fn fs_main(
    @location(0) normal : vec3<f32>,
    @location(1) color : vec3<f32>,
    @location(2) emissive : vec3<f32>,
    @location(3) tint : vec3<f32>,
    @location(4) world : vec3<f32>,
    @location(5) eye_distance : f32,
    @location(6) contact_ao : f32,
) -> @location(0) vec4<f32> {
    let n : vec3<f32> = normalize(normal);
    let n_dot_l : f32 = max(dot(n, shading.light_dir.xyz), 0.0);
    let hemi_weight : f32 = n.y * 0.5 + 0.5;
    let hemi : vec3<f32> = mix(shading.ground_ambient.xyz, shading.sky_ambient.xyz, hemi_weight);
    let ambient : vec3<f32> = mix(shading.ambient.xyz, hemi, shading.ambient_hemi.x);
    let base : vec3<f32> = color * tint;

    // ---- 阴影 ----
    // 法线偏移:沿世界法线把比较点推离表面,专治自阴影痤疮。
    // 掠射面(n_dot_l 小)推得更远,因为那里的深度梯度最陡。
    let shadow_slope : f32 = clamp(1.0 - n_dot_l, 0.0, 1.0);
    let normal_offset : f32 = shading.shadow_misc.x * shading.shadow_params.w
        * (1.0 + shading.shadow_misc.w * shadow_slope);
    let shadow_world : vec3<f32> = world + n * normal_offset;
    var shadow : f32 = sample_shadow(shadow_world, n_dot_l);
    // 强度低的相位(夜)把阴影调淡,不是完全关掉 —— 路灯下仍有一层
    // 淡淡的接触暗部,物体才不至于「漂起来」。
    shadow = mix(1.0, shadow, shading.shadow_params.x);

    let lit : vec3<f32> = base * (ambient + shading.light_color.xyz * n_dot_l * shadow)
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

/// 全屏三角形顶点着色器(bloom 四条 pass 共用)。
///
/// 与 WebGL2 端 [`crate::render`] 里那份 `FULLSCREEN_VERTEX_SHADER`
/// **同一个形状**:`vertex_index` 自己算出裁剪空间位置,所以整条
/// pipeline 的 `buffers` 是空的 —— 不绑任何顶点缓冲,也不需要
/// instance buffer。
///
/// ⚠️ **`p` 的 y 不取反。** WebGL 那份算完是
/// `vec4(p * 2.0 - 1.0, 0.0, 1.0)`,而 WebGPU 的 NDC **+y 在屏幕
/// 上方**(与 OpenGL 一致,见 [`SHADER_FRAGMENT`] 里 `sample_shadow`
/// 那段实测记录),所以两条后端的 `v_uv` 语义相同、可以直接照抄
/// 公式 —— 取反反而会把合成 pass 上下颠倒。
///
/// ⚠️ 必须 `return clip`(真实裁剪坐标),不能塞
/// `vec4(clip.xy, 0.5, 1.0)` —— 那是几何 pass 踩过的坑。
pub(crate) const SHADER_FULLSCREEN_VERTEX: &str = r#"
struct FullscreenOut {
    @builtin(position) clip : vec4<f32>,
    @location(0) uv : vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) index : u32) -> FullscreenOut {
    // 一个覆盖裁剪空间的三角形:(0,0) (2,0) (0,2)。
    let p : vec2<f32> = vec2<f32>(
        f32((index << 1u) & 2u),
        f32(index & 2u),
    );
    var out : FullscreenOut;
    out.clip = vec4<f32>(p * 2.0 - vec2<f32>(1.0, 1.0), 0.0, 1.0);
    out.uv = p;
    return out;
}
"#;

/// bloom 的片元着色器:亮度提取 + 可分离高斯模糊(方向由 uniform 给)
/// + 合成。
///
/// 三条 pass 合成一个模块、四个入口点,原因有两个:
///
/// 1. **布局完全一致** —— 都只绑 group 0 的三个 binding
///    (uniform / sampler / texture),所以四条 pipeline 能共用同一个
///    `GPUPipelineLayout`,不必建四份。
/// 2. **高斯核来自 uniform 而不是 WGSL 常量** —— 与 WebGL2 端那份
///    `const float WEIGHTS[5]` 同一个值,但走 uniform 后
///    `bloom_blur_kernel_weights_sum_to_one` 才钉得住「归一化」。
///
/// ⚠️ **合成那个入口多绑一对 sampler + 纹理**(槽位 3 / 4),所以它
/// 用的是**另一条** bind group layout。
pub(crate) const SHADER_BLOOM_FRAGMENT: &str = r#"
struct BloomParams {
    // x = 亮度阈值, y = 合成强度, zw = 模糊方向(uv 单位)。
    params : vec4<f32>,
    // 高斯核权重 w0..w3。
    kernel : vec4<f32>,
    // x = 核权重 w4, yz = 一个纹素的 uv 尺寸。
    texel : vec4<f32>,
};

@group(0) @binding(0) var<uniform> bloom : BloomParams;
@group(0) @binding(1) var source_sampler : sampler;
@group(0) @binding(2) var source_color : texture_2d<f32>;

// 只有合成那一个入口用得到这一对。
@group(0) @binding(3) var blur_sampler : sampler;
@group(0) @binding(4) var blur_map : texture_2d<f32>;

// 取第 index 个核权重(index ∈ 0..4)。第 5 个(w4)放在 texel.x,
// 所以这里单独分流而不是直接索引 kernel。
fn kernel_weight(index : i32) -> f32 {
    if (index == 4) {
        return bloom.texel.x;
    }
    return bloom.kernel[index];
}

// 亮度提取:软阈值,只留下比阈值亮得多的部分。
// 与 WebGL2 端 BLOOM_EXTRACT_FRAGMENT_SHADER 逐项对应 —— 软阈值
// (threshold 与 threshold-knee 之间的 smoothstep)比硬阈值好:硬阈值会让
// 亮度刚好越线的像素「突然」出现光晕,看起来像描边。
@fragment
fn fs_extract(
    @location(0) uv : vec2<f32>,
) -> @location(0) vec4<f32> {
    let color : vec3<f32> = textureSample(source_color, source_sampler, uv).rgb;
    let luma : f32 = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    let threshold : f32 = bloom.params.x;
    let knee : f32 = max(threshold * 0.6, 0.001);
    var soft : f32 = clamp(luma - threshold + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee);
    let contribution : f32 = max(soft, luma - threshold) / max(luma, 0.0001);
    return vec4<f32>(color * contribution, 1.0);
}

// 5 抽头线性采样优化的高斯核(等价于 9 抽头,只用 5 次纹理读取)。
// 方向来自 uniform:水平传 (texel.x * spread, 0),竖直传 (0, texel.y * spread)。
fn blur(uv : vec2<f32>) -> vec4<f32> {
    let direction : vec2<f32> = bloom.params.zw;
    var color : vec3<f32> = textureSample(source_color, source_sampler, uv).rgb
        * kernel_weight(0);
    for (var i : i32 = 1; i < 5; i = i + 1) {
        let offset : vec2<f32> = direction * f32(i);
        let weight : f32 = kernel_weight(i);
        color = color + textureSample(source_color, source_sampler, uv + offset).rgb * weight;
        color = color + textureSample(source_color, source_sampler, uv - offset).rgb * weight;
    }
    return vec4<f32>(color, 1.0);
}

// 水平模糊:方向 (texel.x * spread, 0)。
@fragment
fn fs_blur_h(@location(0) uv : vec2<f32>) -> @location(0) vec4<f32> {
    return blur(uv);
}

// 竖直模糊:方向 (0, texel.y * spread)。两条模糊共用同一个 blur 函数体
// (与 WebGL2 端 u_direction 共用一个 program 同理),区别只在 uniform。
// 写死成两个函数体的话,核权重就得分叉两份,改一处忘另一处的症状是
// 「光晕一边糊一边不糊」。
@fragment
fn fs_blur_v(@location(0) uv : vec2<f32>) -> @location(0) vec4<f32> {
    return blur(uv);
}

// 合成:主场景颜色 + 模糊后的 bloom,加法混合。
// 强度 bloom.params.y 由 Rust 侧算好再上传。
@fragment
fn fs_composite(@location(0) uv : vec2<f32>) -> @location(0) vec4<f32> {
    let color : vec3<f32> = textureSample(source_color, source_sampler, uv).rgb;
    let glow : vec3<f32> = textureSample(blur_map, blur_sampler, uv).rgb;
    return vec4<f32>(color + glow * bloom.params.y, 1.0);
}
"#;

pub(crate) const GPU_ERR_NO_NAVIGATOR: &str = "window.navigator unavailable";

pub(crate) const GPU_ERR_NO_GPU_OBJECT: &str = "navigator.gpu undefined";

pub(crate) const GPU_ERR_NO_ADAPTER: &str = "requestAdapter() resolved null";

pub(crate) const GPU_ERR_NO_DEVICE: &str = "requestDevice() failed";

pub(crate) const GPU_ERR_NO_CONTEXT: &str = "getContext(webgpu) returned null";

pub(crate) const GPU_ERR_CONFIGURE_FAILED: &str = "context.configure failed";