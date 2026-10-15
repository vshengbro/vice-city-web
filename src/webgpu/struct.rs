use super::*;

/// 已经 `configure` 好的 WebGPU 上下文。
#[derive(Data, Debug, Clone)]
pub struct GpuContext {
    /// `GPUCanvasContext`,每帧 `getCurrentTexture()` 靠它。
    #[get(skip, pub(crate))]
    pub(crate) context: JsValue,
    /// `GPUDevice`,管线 / bind group 创建时取用。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) device: JsValue,
    /// 画布首选格式(`bgra8unorm` 或 `rgba8unorm`)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) format: String,
}

/// 一个已上传资产在 GPU 侧的句柄。
///
/// 与 `render::GlMesh` 一一对应且**下标语义完全相同** ——
/// [`crate::render::SceneBatch::mesh_index`] 在两个后端里都当这张表的
/// 下标用,所以两张表必须长度相等、顺序一致,否则批次会取到别人的网格。
#[derive(Data, Debug, Clone)]
pub struct GpuMesh {
    /// 顶点缓冲(`GPUBuffer`)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) vertex_buffer: JsValue,
    /// 索引缓冲(`GPUBuffer`)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) index_buffer: JsValue,
    /// `drawIndexed` 的索引数。
    #[get(type(copy), pub(crate))]
    pub(crate) index_count: u32,
    /// 顶点缓冲的**容量字节数**,不是本次写入的长度。
    ///
    /// ⚠️ 这个字段是「黑屏修复」的核心。`replace_mesh` 会把流式重建
    /// 出来的地面 / 水面数据重新写进**同一对**缓冲,而重建后的顶点数
    /// 会随生成中心变化 —— 可能变大。若没有容量记录,`writeBuffer` 就会
    /// 用「当初 `upload_mesh` 建的尺寸」去界,直接越界:Chrome 报
    /// `Write range (size: N) does not fit in buffer size (M)`,而写失败的
    /// 后果不是「这一帧没更新」,而是整条 `queue.submit()` 变成
    /// `[Invalid CommandBuffer]` **空提交** —— swapchain 那张纹理整个
    /// 没人画,于是屏幕**立刻变黑且永不恢复**。
    ///
    /// WebGL2 端没这个问题:`bufferData` 本身就是「按新数据重新分配」。
    /// WebGPU 的 `GPUBuffer` 一旦 `createBuffer` 就没有 `bufferData`,
    /// 必须自己记住容量并在需要时**换一个新的、更大的** `GPUBuffer`。
    #[get(type(copy), pub(crate))]
    pub(crate) vertex_capacity: usize,
    /// 索引缓冲的容量字节数(理由同 [`Self::vertex_capacity`])。
    #[get(type(copy), pub(crate))]
    pub(crate) index_capacity: usize,
}

/// 一帧渲染所需的全部参数。
///
/// 打包成结构体而不是 8 个位置参数:`render` 的参数个数已经越过
/// clippy `too_many_arguments` 的阈值,而这里**不能**加 `#[allow]`。
/// 更重要的是这八个参数天然成组(帧常量 / 着色参数 / 视口),
/// 将来补上 `time` / `quality` 时不必再改一次签名。
#[derive(Data, Clone, Copy, Debug)]
pub struct RenderParams<'a> {
    /// 场景(资产 + 批次)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) scene: &'a crate::render::Scene,
    /// 视投影矩阵。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) view_proj: &'a Mat4,
    /// 当前光照参数。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) lighting: &'a SceneLighting,
    /// 相机眼点。
    #[get(type(copy), pub(crate))]
    pub(crate) eye: Vec3,
    /// 画布宽(像素)。
    #[get(type(copy), pub(crate))]
    pub(crate) width: u32,
    /// 画布高(像素)。
    #[get(type(copy), pub(crate))]
    pub(crate) height: u32,
    /// 近处剔除半径。
    #[get(type(copy), pub(crate))]
    pub(crate) near_cull_radius: f32,
    /// 阴影 frustum 的中心(世界坐标)。
    ///
    /// ⚠️ 与 WebGL2 端**同一个参数、同一份语义**:跟的是**相机焦点**
    /// 而不是眼点(见 [`crate::render::shadow_view_projection`] 的说明
    /// —— 跟着眼点会让远处物体的影子落进 frustum 之外,边缘出现一条
    /// 整齐的「影子截止线」)。
    #[get(type(copy), pub(crate))]
    pub(crate) shadow_focus: Vec3,
}

/// 着色参数 uniform 的字节布局(12 × vec4 = 192 字节)。
///
/// 字段顺序必须与 [`SHADER_FRAGMENT`] 里的 `Shading` 结构逐项一致,
/// 否则读出来的是错位的颜色。统一用 `[f32; 4]` 是因为 WebGPU 的
/// uniform 布局按 16 字节对齐,而本块恰好是 vec4 的整数倍。
#[derive(Data, Clone, Copy, Debug, Default)]
pub struct ShadingUniforms {
    /// `light_dir` + 1 个 pad。
    #[get(type(copy), pub(crate))]
    pub(crate) light_dir: [f32; 4],
    /// `light_color` + 1 个 pad。
    #[get(type(copy), pub(crate))]
    pub(crate) light_color: [f32; 4],
    /// `ambient` + 1 个 pad。
    #[get(type(copy), pub(crate))]
    pub(crate) ambient: [f32; 4],
    /// `sky_color` + 1 个 pad。
    #[get(type(copy), pub(crate))]
    pub(crate) sky_color: [f32; 4],
    /// `sky_ambient` + 1 个 pad。
    #[get(type(copy), pub(crate))]
    pub(crate) sky_ambient: [f32; 4],
    /// `ground_ambient` + 1 个 pad。
    #[get(type(copy), pub(crate))]
    pub(crate) ground_ambient: [f32; 4],
    /// `x` = `ambient_hemi`,其余为 pad。
    #[get(type(copy), pub(crate))]
    pub(crate) ambient_hemi: [f32; 4],
    /// `x` = `emissive_gain`,其余为 pad。
    #[get(type(copy), pub(crate))]
    pub(crate) emissive_gain: [f32; 4],
    /// `eye` + 1 个 pad。
    #[get(type(copy), pub(crate))]
    pub(crate) eye: [f32; 4],
    /// `x` = `fog_start`,`y` = `fog_end`,其余为 pad。
    #[get(type(copy), pub(crate))]
    pub(crate) fog: [f32; 4],
    /// `x` = `ao_height`(垂直上界),`y` = `ao_floor`(最深压暗系数),
    /// `z` = `ao_reach`(水平作用半径),`w` = `ao_feather`(水平平滑宽度)。
    ///
    /// ⚠️ **不再有「按 `world.y` 压暗」那一项。** 旧布局只有 xy,
    /// WGSL 里写的是 `mix(y, 1.0, clamp(world.y / x, 0, 1))` —— 那是
    /// 世界高度代理量,地面整片在 y≈0 上于是全城统一吃最深压暗。
    /// 改判据见 [`SHADER_VERTEX`] 里的 `contact_ao()`。
    #[get(type(copy), pub(crate))]
    pub(crate) ao_params: [f32; 4],
    /// `x` = `exposure`,`y` = `tone_map_white`,其余为 pad。
    #[get(type(copy), pub(crate))]
    pub(crate) exposure_white: [f32; 4],
    /// `x` = `shadow_strength`,`y` = PCF 半径(纹素),
    /// `z` = 深度偏置(纹素),`w` = 法线偏移(纹素)。
    ///
    /// 与 WebGL2 端的 `u_shadow_params` **逐项同序**,便于一眼比对。
    #[get(type(copy), pub(crate))]
    pub(crate) shadow_params: [f32; 4],
    /// `x` = 一个纹素覆盖的世界尺寸(米),`y` = WebGPU 深度域跨度(米),
    /// `z` = 深度偏置的斜率增益,`w` = 法线偏移的斜率增益。
    ///
    /// 后者是偏置从「纹素(米)」换算到「`[0, 1]` 深度」的分母 ——
    /// WebGL2 那边的 `2.0 / 1024.0` 魔数在 WebGPU 里不成立,详见
    /// [`crate::webgpu::r#const::SHADOW_DEPTH_SPAN_M`]。
    ///
    /// 两个斜率增益走 uniform 而不是写死在 WGSL 里,是为了调偏置时
    /// 只改 [`crate::webgpu::r#const`] 一处,不必同时维护 GLSL 与
    /// WGSL 两份魔数。
    #[get(type(copy), pub(crate))]
    pub(crate) shadow_misc: [f32; 4],
    /// `x` = 接触 AO 的**高度下界**(米),其余为 pad。
    ///
    /// 放在**整块的最后**而不是插在 `ao_params` 后面,是为了让本块
    /// 里每一个既有字段的字节偏移都保持不变 —— WGSL 的 uniform 按
    /// 声明顺序排布,中间插一个 vec4 会把后面全部字段平移 16 字节,
    /// 而 `write_buffer` 那侧若忘了同步改,读到的是错位的值
    /// (阴影参数静默变成色调映射的曝光值:画面偏亮且无影)。
    ///
    /// 单开一块而不是塞进 `ao_params` 的 zw,是因为 zw 已经被水平
    /// 作用半径(6.0 m)与平滑宽度(2.0 m)占满了。
    #[get(type(copy), pub(crate))]
    pub(crate) ao_band: [f32; 4],
}

/// 着色参数 uniform 的字节数(15 × vec4 = 240 字节)。
///
/// ⚠️ 必须与 [`ShadingUniforms`] 的字段数**逐项一致**:少写一个
/// `vec4` 不会编译报错,只会在 GPU 上读到**后面那个块的数据**当成本
/// 字段 —— 阴影参数会静默变成色调映射的曝光值,画面偏亮且无影。
pub(crate) const SHADING_VEC4_COUNT: usize = 15;

/// bloom 参数 uniform 的字节布局(3 × vec4 = 48 字节)。
///
/// 字段顺序必须与 WGSL 那个 `BloomParams` 结构体**逐项一致**,
/// 少一个 `vec4` 不会编译报错,只会让模糊核读成阈值。
///
/// 每条 pass 有**自己**那份 buffer(见
/// [`WebGpuRenderer::bloom_buffers`]),所以这个结构体里同时带着
/// 「提取要的阈值」「两条模糊各要的方向」「合成要的强度」——
/// 每个 pass 只读自己那两个分量,其余是陪坐。
#[derive(Data, Clone, Copy, Debug, Default)]
pub struct BloomUniforms {
    /// `x` = 亮度阈值(线性空间,提取用),
    /// `y` = 合成强度,`zw` = 模糊方向(uv 单位,H 为 `(dx, 0)`、
    /// V 为 `(0, dy)`)。
    ///
    /// 方向**跟着每次上传走**而不是写死在 WGSL 里,是与 WebGL2 端
    /// `u_direction` 同一个做法 —— 那边的水平 / 垂直模糊共用一个
    /// program,方向就是 uniform。
    #[get(type(copy), pub(crate))]
    pub(crate) params: [f32; 4],
    /// 高斯核的前 4 个权重(w0..w3)。
    #[get(type(copy), pub(crate))]
    pub(crate) kernel: [f32; 4],
    /// `x` = 第 5 个权重(w4),`yz` = 一个纹素覆盖的 uv 尺寸,
    /// `w` = 保留。
    #[get(type(copy), pub(crate))]
    pub(crate) texel: [f32; 4],
}

/// bloom 参数 uniform 的字节数(3 × vec4 = 48 字节)。
///
/// ⚠️ 必须与 [`BloomUniforms`] 的字段数**逐项一致**:少分配的话
/// `writeBuffer` 会抛 `writeBuffer size exceeds buffer size`,
/// 而不是静默截断。
pub(crate) const BLOOM_VEC4_COUNT: usize = 3;

/// 视投影矩阵的字节序(列主序,直接 `writeBuffer`)。
///
/// # Arguments
///
/// - `&Mat4` - 列主序视投影矩阵。
///
/// # Returns
///
/// WebGPU 不可用的原因。
///
/// 原本是一个 `enum` + `impl`,但 §1.3 规定 `struct.rs` 里只允许
/// `struct`(不允许 `enum` / `type`),而它的 `impl` 也不能跟着待在
/// `struct.rs`。改成**只包一个字符串的 struct**,`Display` 由
/// `impl.rs` 里的 `impl std::fmt::Display` 提供 —— 这样
/// `format!("[vcw] WebGPU unavailable: {}", reason)` 照样能写。
#[derive(Data, Debug, Clone, Copy, Eq, PartialEq)]
pub struct GpuUnavailable {
    /// 人可读的失败原因,例如 [`super::r#const::GPU_ERR_NO_ADAPTER`]。
    ///
    /// 全部取值见 `super::r#const` 里的 `GPU_ERR_*` 系列(§1.3c:
    /// 硬编码字符串的单一来源)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) reason: &'static str,
}

/// WebGPU 渲染后端。
///
/// 构造是**异步**的(见 [`acquire`]),但 [`WebGpuRenderer::render`]
/// 是同步的 —— 拿到 device 之后,每帧不再需要任何 await。
#[derive(Clone, Data, Debug)]
pub struct WebGpuRenderer {
    /// 已 `configure` 的画布上下文。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) context: JsValue,
    /// `GPUDevice`。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) device: JsValue,
    /// 画布格式。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) format: String,
    /// 主 render pipeline。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) pipeline: JsValue,
    /// pipeline layout(`createBindGroup` 需要它)。
    /// `GPUBindGroupLayout` —— `createBindGroup` 要的是**它**,不是
    /// `GPUPipelineLayout`(后者只是它的容器)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) group_layout: JsValue,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) pipeline_layout: JsValue,
    /// 深度纹理(尺寸随画布变化重建)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) depth_texture: JsValue,
    /// 深度纹理当前尺寸,(0, 0) = 还没建。
    #[get(type(copy), pub(crate))]
    pub(crate) depth_size: (u32, u32),
    /// uniform buffer 0:视投影矩阵。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) frame_buffer: JsValue,
    /// uniform buffer 1:着色参数。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) shading_buffer: JsValue,
    /// 与上面两个 buffer 对应的 bind group。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) bind_group: JsValue,
    /// 阴影 pass 的 pipeline(纯深度、无颜色输出)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) shadow_pipeline: JsValue,
    /// 阴影 pass 的 pipeline layout(只有 group 0 的光源矩阵)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) shadow_pipeline_layout: JsValue,
    /// 阴影 pass 的 bind group layout。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) shadow_group_layout: JsValue,
    /// 阴影 pass 的 bind group(group 0 = 光源矩阵)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) shadow_bind_group: JsValue,
    /// 光源视投影矩阵 uniform buffer(主 pass 与阴影 pass 各绑一份
    /// 同一个 buffer 到不同的 binding 上)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) shadow_frame_buffer: JsValue,
    /// 阴影深度纹理(`depth32float`,边长 [`crate::r#config::SHADOW_MAP_SIZE`] 的正方形)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) shadow_texture: JsValue,
    /// 阴影 pass 专用的 instance buffer。
    ///
    /// ⚠️ **不能与主管线共用**:`queue.writeBuffer` 在队列时间线上先于
    /// 整条 command buffer 执行,同帧两次上传会互相覆盖。详见
    /// [`WebGpuRenderer::draw_shadow_pass`] 里的说明。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) shadow_instance_buffer: JsValue,
    /// 阴影 instance buffer 的容量(实例数)。
    #[get(type(copy), pub(crate))]
    pub(crate) shadow_instance_capacity: usize,
    /// 主 pass 用的 group 1:阴影图 + 比较采样器 + 光源矩阵。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) shadow_sample_bind_group: JsValue,
    /// group 1 的 `GPUBindGroupLayout`。
    ///
    /// 单独存一份是因为它被**两个阶段**按顺序用到:主 pipeline 的
    /// layout 里要列它(`build_pipeline_inner`),而 bind group 要拿它
    /// 当描述符的 `layout`(`create_shadow_sample_bind_group`)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) shadow_sample_group_layout: JsValue,
    /// 顶点 / 索引缓冲,下标与 `Scene::meshes` 一一对应。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) meshes: Vec<GpuMesh>,
    /// 所有批次共享的 instance buffer。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) instance_buffer: JsValue,
    /// instance buffer 当前容量(实例数)。
    #[get(type(copy), pub(crate))]
    pub(crate) instance_capacity: usize,
    /// 验收探针:GPU 侧资产表长度。
    #[get(type(copy), pub(crate))]
    pub(crate) gpu_mesh_count: usize,
    /// 主场景颜色目标(全分辨率,画布 preferred format)。
    ///
    /// ⚠️ **有 bloom 就不能直接把主 pass 画进 swapchain。** 亮度提取
    /// 要采样主场景的颜色,而 `getCurrentTexture()` 给的那张纹理
    /// 只有 `RENDER_ATTACHMENT | COPY_SRC`,**不能**被采样(除非
    /// `configure` 时显式要 `TEXTURE_BINDING`,而本项目没有)。
    ///
    /// 所以主 pass 画进这张全分辨率离屏目标,合成那一条 pass 再把它
    /// 读回来、加完辉光画进 swapchain —— 与 WebGL2 端
    /// `RenderTarget::bind(context, Some(scene))` 完全同构。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) bloom_scene: JsValue,
    /// 亮度提取的目标纹理(半分辨率)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) bloom_bright: JsValue,
    /// 水平模糊的输出纹理(ping)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) bloom_ping: JsValue,
    /// 竖直模糊的输出纹理(pong,也是合成读的「模糊结果」)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) bloom_pong: JsValue,
    /// 四张后处理目标当前的尺寸,(0, 0) = 还没建。
    ///
    /// 记的是**画布**尺寸而不是 bloom 那三张的尺寸:四张一起重建,
    /// 一个尺寸就够(见 [`WebGpuRenderer::ensure_bloom_targets`]）。
    #[get(type(copy), pub(crate))]
    pub(crate) bloom_size: (u32, u32),
    /// 四条 bloom pass **各一个**的 uniform buffer。
    ///
    /// ⚠️ **不能共用一个。** `queue.writeBuffer` 排在队列时间线上,
    /// 整条 command buffer 之前就执行完了 —— 同帧往**同一个** buffer
    /// 写三次,只有最后一次的内容会生效。而提取 / 模糊 H / 模糊 V /
    /// 合成四份数据**互不相同**(各自的阈值、方向、强度),于是共用
    /// 的话两条模糊 pass 会读到同一个方向,竖直模糊退化成第二次水平
    /// 模糊,光晕变成横向条纹。
    ///
    /// 这与阴影 pass 那个「实例 buffer 必须另开一份」是同一条队列
    /// 时间线陷阱。
    #[get(skip, pub(crate))]
    pub(crate) bloom_buffers: [JsValue; BLOOM_PASS_COUNT],
    /// bloom 颜色输入用的**普通**(非比较)采样器。
    ///
    /// ⚠️ 与阴影那张 `comparison` 采样器是**两个不同的对象**:WebGPU
    /// 的深度纹理只配 `sampler_comparison`,而 `rgba8unorm` 颜色纹理
    /// 只配普通 `sampler` —— 拿错会得到 validation error。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) bloom_sampler: JsValue,
    /// 提取 / 模糊三条 pipeline 的 bind group layout(3 槽:uniform +
    /// sampler + texture,形状完全一致)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) bloom_group_layout: JsValue,
    /// 合成那条 5 槽 bind group layout(多一对 sampler + 模糊图)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) bloom_composite_group_layout: JsValue,
    /// 四条 bloom pipeline 各自的 `(pipeline, bind group)` 配对。
    ///
    /// 收在一个数组里而不是摊成八个字段:它们一一对应,拆开就会出现
    /// 「某条 pipeline 用了别人的 bind group」这种静默不兼容 ——
    /// WebGPU 只在真正 `setBindGroup` 那一刻才报 validation error,
    /// 症状是那一帧消失而前面几帧都正常。
    #[get(skip, pub(crate))]
    pub(crate) bloom_passes: [GpuBloomPass; BLOOM_PASS_COUNT],

    // ---- SSAO -------------------------------------------------------------
    /// G-buffer 的**视空间法线**图(城市 pass 的 MRT 附件 1)。
    ///
    /// `rgba8unorm`:法线分量会在 ±1,直接存 `rgba8unorm` 会在着色器
    /// 里做钳位/编码往返,采样回来再解一次。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) ao_normal: JsValue,
    /// G-buffer 的**线性深度**图(城市 pass 的 MRT 附件 2)。
    ///
    /// `r32float`:深度要线性且不量化,8bit 会把整条马赛克化成
    /// 台阶,双边模糊的权重全错。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) ao_depth: JsValue,
    /// 半分辨率的 AO 结果(ping)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) ao_ping: JsValue,
    /// 半分辨率的 AO 结果(pong,也是模糊的最终读出)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) ao_pong: JsValue,
    /// 四个 AO 目标当前的尺寸,(0, 0) = 还没建。
    #[get(type(copy), pub(crate))]
    pub(crate) ao_size: (u32, u32),
    /// 三条 AO pass(采样 / 模糊 H / 模糊 V)**各一个**的 uniform buffer。
    ///
    /// ⚠️ 和 bloom 同样受队列时间线约束:同帧写三次同一个 buffer,
    /// 只有最后一次生效。方向参数因此必须分开。
    /// 本帧序号,供 SSAO 的逐帧抖动种子用。
    #[get(type(copy), pub(crate))]
    pub(crate) frame_index: u64,
    #[get(skip, pub(crate))]
    pub(crate) ao_buffers: [JsValue; AO_PASS_COUNT],
    /// AO 用的普通采样器(G-buffer 是颜色类纹理,不是比较采样器)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) ao_sampler: JsValue,
    /// AO 采样 pass 的 bind group layout(4 槽:sampler + 法线 + 深度 +
    /// uniform)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) ao_group_layout: JsValue,
    /// 三条 AO pipeline 各自的 `(pipeline, bind group)` 配对。
    #[get(skip, pub(crate))]
    pub(crate) ao_passes: [GpuAoPass; AO_PASS_COUNT],
    /// 城市 pass 现在的 MRT 附件数(1 = 只写场景色,SSR/SSAO 关)。
    #[get(type(copy), pub(crate))]
    pub(crate) gbuffer_slots: u32,
}

/// SSAO 这条链上有几条 pass:采样 → 模糊 H → 模糊 V。
pub(crate) const AO_PASS_COUNT: usize = 3;

/// 一条 AO pass 的 `(pipeline, bind group)`。
///
/// 收成数组而不是拆成六个字段,理由和 [`GpuBloomPass`] 一样:一一对应
/// 的东西拆开就会出现「某条 pipeline 绑了别人的 group」这种静默错误。
#[derive(Data, Debug, Clone)]
pub struct GpuAoPass {
    /// 渲染 pipeline。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) pipeline: JsValue,
    /// 与之配套的 bind group。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) group: JsValue,
}

/// 一条 bloom pass 要上传的那份 uniform 的**参数**
/// (不含核权重 —— 那是所有 pass 共用的)。
///
/// # Arguments
///
/// - `threshold` - `params.x`:亮度阈值,只对提取那条有意义。
/// - `strength` - `params.y`:辉光强度,只对合成那条有意义。
/// - `direction` - `params.zw`:模糊方向(uv 单位)。
/// - `size` - 这条 pass **读写的那张目标**的尺寸。
///
/// 具名成 struct 而不是四元组:四元组那四个位置在下游只能靠
/// `uploads[i].2` 这种下标读,而提取与合成两条的形状一样
/// (方向恒为 `(0.0, 0.0)`),读错了编译器不会报错。字段名让
/// 「这条 pass 的方向是什么」变成一句能读的话。
#[derive(Data, Clone, Copy, Debug, Default)]
pub(crate) struct BloomPassUpload {
    /// `params.x`:亮度阈值(线性空间)。
    #[get(type(copy), pub(crate))]
    pub(crate) threshold: f32,
    /// `params.y`:合成强度。
    #[get(type(copy), pub(crate))]
    pub(crate) strength: f32,
    /// `params.zw`:模糊方向(uv 单位)。
    #[get(type(copy), pub(crate))]
    pub(crate) direction: (f32, f32),
    /// 这条 pass 读写的那张目标的尺寸。
    #[get(type(copy), pub(crate))]
    pub(crate) size: (u32, u32),
}

/// bloom 一共要跑的四条 pass(顺序即下标)。
///
/// 提取 → 水平模糊 → 竖直模糊 → 合成,下标与
/// [`WebGpuRenderer::bloom_passes`] 一一对应。
pub(crate) const BLOOM_PASS_COUNT: usize = 4;

/// 提取那条 pass 在 [`WebGpuRenderer::bloom_passes`] 里的下标。
pub(crate) const BLOOM_PASS_EXTRACT: usize = 0;

/// 水平模糊那条 pass 在 [`WebGpuRenderer::bloom_passes`] 里的下标。
pub(crate) const BLOOM_PASS_BLUR_H: usize = 1;

/// 竖直模糊那条 pass 在 [`WebGpuRenderer::bloom_passes`] 里的下标。
pub(crate) const BLOOM_PASS_BLUR_V: usize = 2;

/// 合成那条 pass 在 [`WebGpuRenderer::bloom_passes`] 里的下标。
pub(crate) const BLOOM_PASS_COMPOSITE: usize = 3;

/// 一条 bloom pipeline 与它的 bind group 的配对句柄。
///
/// ⚠️ **刻意不带 `Copy`**:两个字段都是 `JsValue`,而 `JsValue` 只是
/// `Clone` 不是 `Copy`(wasm-bindgen 把它当成栈上的临时句柄)。所以
/// 传递整张表时用 [`WebGpuRenderer::get_bloom_passes`] 按值克隆 ——
/// 每帧一次、四个句柄,代价可以忽略,换来的是调用点不必操心
/// 借用与 `&mut self` 的冲突。
#[derive(Data, Debug, Clone)]
pub struct GpuBloomPass {
    /// `GPURenderPipeline`。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) pipeline: JsValue,
    /// 与 `pipeline` 的 pipeline layout 严格对应的 `GPUBindGroup`。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) bind_group: JsValue,
}
