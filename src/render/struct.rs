use super::*;
/// 一个三角面在 CPU 侧的表示,软件渲染与 WebGL 共用。
#[derive(Clone, Copy, Data, Debug)]
pub struct Face {
    /// 三个顶点下标(指向 [`GpuMesh::vertices`],单位是顶点而不是 f32)。
    #[get(type(copy), pub(crate))]
    pub(crate) indices: [u32; 3],
    /// 平面法线(已归一化,单位向量)。
    #[get(type(copy), pub(crate))]
    pub(crate) normal: Vec3,
    /// 基础色,线性空间 0..1。
    #[get(type(copy), pub(crate))]
    pub(crate) color: Vec3,
    /// 自发光色,线性空间 0..1。
    #[get(type(copy), pub(crate))]
    pub(crate) emissive: Vec3,
}
/// 一份资产在 GPU / CPU 两种后端下共用的展开结果。
///
/// 解析一次(见 [`crate::mesh::parse_asset`]),顶点数据只保留一份,
/// 供「按资产类型分批」的 instancing 渲染复用。
#[derive(Data, Debug, Default)]
pub struct MeshAssetGpu {
    /// 展平后的顶点数据,`STRIDE_FLOATS` 个 f32 / 顶点。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) vertices: Vec<f32>,
    /// 三角形索引(每个三角形 3 个)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) indices: Vec<u32>,
    /// 每个三角形的面信息(法线 / 颜色 / 自发光)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) faces: Vec<Face>,
    /// 三角形数量。
    #[get(type(copy), pub(crate))]
    pub(crate) triangle_count: usize,
    /// 包围球(局部空间,`[cx, cy, cz, radius]`)。
    ///
    /// 主 pass 的视锥剔除按「实例模型矩阵 × 包围球」做:球在视锥外
    /// 整实例跳过。`radius <= 0.0` 表示未初始化(测试手工构造的
    /// mesh),渲染端按**永远可见**处理,保证剔除只会少画、不会漏画。
    #[get(type(copy), pub(crate))]
    #[set(pub(crate))]
    pub(crate) bounds_sphere: [f32; 4],
}
/// 两个后端共享的光照参数。
///
/// WebGL 把它作为 uniform 上传,Canvas2D 后端在 CPU 上跑同一个公式,
/// 保证两个后端的画面在色调整体上一致。
#[derive(Clone, Copy, Data, Debug)]
pub struct SceneLighting {
    /// 太阳 / 月亮方向光方向(指向光源)。
    #[get(type(copy), pub(crate))]
    pub(crate) light_dir: Vec3,
    /// 方向光颜色 × 强度。
    #[get(type(copy), pub(crate))]
    pub(crate) light_color: Vec3,
    /// 环境光颜色 × 强度(半球权重为 0 时的回退色)。
    #[get(type(copy), pub(crate))]
    pub(crate) ambient: Vec3,
    /// 天空 / 雾颜色。
    #[get(type(copy), pub(crate))]
    pub(crate) sky_color: Vec3,
    /// 半球环境光的「天光」色(朝上的面接收)。
    #[get(type(copy), pub(crate))]
    pub(crate) sky_ambient: Vec3,
    /// 半球环境光的「地面反弹」色(朝下的面接收)。
    #[get(type(copy), pub(crate))]
    pub(crate) ground_ambient: Vec3,
    /// 半球环境光权重(0 = 纯单色 ambient,1 = 纯半球)。
    #[get(type(copy), pub(crate))]
    pub(crate) ambient_hemi: f32,
    /// 阴影强度(0 = 关掉阴影,1 = 完全)。
    #[get(type(copy), pub(crate))]
    pub(crate) shadow_strength: f32,
    /// SSAO 强度(0 = 关掉 AO,1 = 完全)。
    #[get(type(copy), pub(crate))]
    pub(crate) ao_strength: f32,
    /// SSR 强度(0 = 关掉屏幕空间反射,1 = 完全)。
    #[get(type(copy), pub(crate))]
    pub(crate) ssr_strength: f32,
    /// 路面湿度(0 = 干,1 = 湿)。与 `ssr_strength` 相乘,前者决定
    /// 「要不要反光」,后者决定「反多狠」。
    ///
    /// 湿路面是 Miami / Vice City 的招牌画面,所以正午也保留一点点
    /// 湿气(而不是 0)—— 只在掠射角真正给得到强 Fresnel 的地方才看得见。
    #[get(type(copy), pub(crate))]
    pub(crate) wetness: f32,
    /// 色调分级:抬黑场(加法,线性空间)。
    #[get(type(copy), pub(crate))]
    pub(crate) grade_lift: Vec3,
    /// 色调分级:中间调 gamma(1.0 = 不变)。
    #[get(type(copy), pub(crate))]
    pub(crate) grade_gamma: Vec3,
    /// 色调分级:亮场增益(1.0 = 不变)。
    #[get(type(copy), pub(crate))]
    pub(crate) grade_gain: Vec3,
    /// 暗角强度(0 = 无暗角)。
    #[get(type(copy), pub(crate))]
    pub(crate) vignette: f32,
    /// 胶片颗粒强度(0 = 无颗粒)。
    #[get(type(copy), pub(crate))]
    pub(crate) grain: f32,
    /// 自发光全局增益(夜晚更大)。
    #[get(type(copy), pub(crate))]
    pub(crate) emissive_gain: f32,
    /// 色调映射的曝光系数(线性域乘子)。
    #[get(type(copy), pub(crate))]
    pub(crate) exposure: f32,
    /// 色调映射的白色点(亮度超过这个值就开始明显压缩高光)。
    #[get(type(copy), pub(crate))]
    pub(crate) tone_map_white: f32,
    /// 大气雾:起始距离(米)。比这更近的物体完全不受雾影响。
    #[get(type(copy), pub(crate))]
    pub(crate) fog_start: f32,
    /// 大气雾:完全饱和的距离(米)。
    #[get(type(copy), pub(crate))]
    pub(crate) fog_end: f32,
}
// ===========================================================================
// 场景批次(instancing 友好的组织方式)
// ===========================================================================
/// 一个实例:model matrix(列主序 16 f32)+ tint。
#[derive(Clone, Copy, Data, Debug)]
pub struct Instance {
    /// model matrix,列主序,直接喂 `uniformMatrix4fv` / 自己算。
    #[get(type(copy), pub(crate))]
    pub(crate) model: Mat4Data,
    /// 逐实例色调乘子。
    #[get(type(copy), pub(crate))]
    pub(crate) tint: Vec3,
}
/// 同一资产的一批实例 —— 这正是 instancing 的粒度:
/// 顶点数据只上传一份,每帧只更新 `[Instance; count]` 组成的 instance buffer。
#[derive(Clone, Data, Debug, Default)]
pub struct SceneBatch {
    /// 对应的资产索引(指向 `Scene::meshes`)。
    #[get(type(copy), pub(crate))]
    pub(crate) mesh_index: usize,
    /// 实例列表。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) instances: Vec<Instance>,
    /// 是否参与深度测试 / 背面剔除(地面与透明片为 false)。
    #[get(type(copy), pub(crate))]
    pub(crate) opaque: bool,
    /// 是否参与「近处遮挡剔除」(见 [`NEAR_CULL_RADIUS`])。
    ///
    /// 第三人称模式下相机离角色只有 4–7 m,而近处剔除半径是 26 m ——
    /// 如果不豁免,玩家身上的每一块 part 都会被剔掉,画面里根本没有角色。
    /// 玩家骨架批次因此固定为 `false`,车辆批次也关掉(车灯要在近处
    /// 看清楚)。地面 / 建筑 / 树保持 `true`。
    #[get(type(copy), pub(crate))]
    pub(crate) near_cull: bool,
}
/// 整个场景:资产表 + 批次表。
///
/// 资产只解析一次(`meshes`),实例只传矩阵(`batches`),
/// 因此「12 栋楼 + 20 棵棕榈 + 40 个道具」不会产生 72 份顶点数据。
#[derive(Data, Debug, Default)]
pub struct Scene {
    /// 资产表,每个资产解析一次。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) meshes: Vec<MeshAssetGpu>,
    /// 批次表,每个批次 = 一次 instanced draw call。
    #[get(type(clone), pub(crate))]
    pub(crate) batches: Vec<SceneBatch>,
    /// 三角形总数(统计用)。
    #[get(type(copy), pub(crate))]
    pub(crate) total_triangles: usize,
}
/// 按实测帧率自动降档的状态机。
///
/// 目标很简单:**宁可掉画质,不要掉帧率**。这在软渲染(SwiftShader)上尤其
/// 重要 —— 基准线已经是 25 fps,再叠上海面 / 草地 / 山体,不动的话会掉到
/// 个位数,截图能截但画面已经没法看了。
#[derive(Clone, Copy, Data, Debug)]
pub struct AdaptiveQuality {
    #[get(type(copy), pub(crate))]
    pub(crate) tier: QualityTier,
    #[get(type(copy), pub(crate))]
    pub(crate) smoothed_fps: f32,
    #[get(type(copy), pub(crate))]
    pub(crate) slow_frames: u32,
    #[get(type(copy), pub(crate))]
    pub(crate) fast_frames: u32,
    #[get(type(copy), pub(crate))]
    pub(crate) samples: u32,
}
// ===========================================================================
// 离屏渲染目标
// ===========================================================================
/// 一个离屏渲染目标(FBO + 它的颜色 / 深度纹理)。
///
/// 深度用**纹理**而不是 renderbuffer:阴影贴图与 G-buffer 的深度都要
/// 在后续 pass 里被采样,renderbuffer 不可采样。
#[derive(Data, Debug)]
pub(crate) struct RenderTarget {
    /// framebuffer 句柄。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) fbo: WebGlFramebuffer,
    /// 颜色纹理句柄(纯深度目标为 `None`)。
    #[get(type(clone), pub(crate))]
    pub(crate) color: Option<WebGlTexture>,
    /// 深度纹理句柄。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) depth: WebGlTexture,
    /// 颜色缓冲的像素宽度。
    #[get(type(copy), pub(crate))]
    pub(crate) width: i32,
    /// 颜色缓冲的像素高度。
    #[get(type(copy), pub(crate))]
    pub(crate) height: i32,
}
// ===========================================================================
// WebGL2 后端
// ===========================================================================
/// 一个 GPU 侧资产(顶点 / 索引 / VAO 全部只上传一次)。
#[derive(Data, Debug)]
pub(crate) struct GlMesh {
    /// 顶点属性数组,里面绑定了 instance buffer 的除数设置。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) vertex_array: WebGlVertexArrayObject,
    /// 顶点缓冲。`replace_mesh` 就地重传时要它(`upload_mesh` 建 VAO 时
    /// 创建,但那时只把 VAO 存了下来)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) vertex_buffer: WebGlBuffer,
    /// 元素索引缓冲。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) index_buffer: WebGlBuffer,
    /// 索引个数(= `triangle_count * 3`)。
    #[get(type(copy), pub(crate))]
    pub(crate) index_count: i32,
}
/// WebGL2 渲染后端。
///
/// - 顶点:交错 `pos(3) | normal(3) | color(3) | emissive(3)`,见 [`STRIDE_FLOATS`]。
/// - 实例:`mat4 model`(4 个 vec4 属性)+ `vec3 tint`,`vertex_attrib_divisor = 1`。
/// - 每批次一次 `drawElementsInstanced`,即「同类资产只传矩阵」。
/// - 深度测试 + 背面剔除 + 深度写入。
#[derive(Data)]
pub struct WebGlRenderer {
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) context: WebGl2RenderingContext,
    #[get(type(clone), pub(crate))]
    pub(crate) program: WebGlProgram,
    #[get(type(clone), pub(crate))]
    pub(crate) glow_program: WebGlProgram,
    #[get(type(clone), pub(crate))]
    pub(crate) shadow_program: WebGlProgram,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) gbuffer_program: WebGlProgram,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) ssao_program: WebGlProgram,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) ao_blur_program: WebGlProgram,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) ssr_program: WebGlProgram,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) bright_program: WebGlProgram,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) blur_program: WebGlProgram,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) composite_program: WebGlProgram,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) meshes: Vec<GlMesh>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_view_proj: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_light_dir: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_light_color: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ambient: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_sky_color: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_emissive_gain: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_fog: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_eye: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_exposure: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_tone_map_white: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_glow_strength: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_shadow_map: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_shadow_matrix: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_shadow_params: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_shadow_texel: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_shadow_view_proj: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_sky_ambient: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ground_ambient: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ambient_hemi: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ao_map: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ao_strength: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssr_map: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssr_strength: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_wetness: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_wet_height: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ao_height: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ao_floor: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ao_reach: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ao_feather: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ao_min_height: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_gbuffer_view: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_gbuffer_far: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssao_gbuffer: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssao_texel: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssao_radius: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssao_power: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssao_proj: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssao_far: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssao_samples: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_blur_ao: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_blur_texel: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_blur_radius: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssr_gbuffer: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssr_scene: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssr_texel: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssr_proj: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssr_far: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssr_max_dist: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_ssr_steps: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_bright_scene: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_bright_threshold: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_blur_source: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_blur_direction: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_composite_scene: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_composite_bloom: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_composite_lift: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_composite_gamma: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_composite_gain: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_composite_bloom_strength: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_composite_vignette: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_composite_grain: Option<WebGlUniformLocation>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) uniform_composite_time: Option<WebGlUniformLocation>,
    /// 所有批次共享的 instance buffer(按最大实例数预分配)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) instance_buffer: WebGlBuffer,
    #[get(type(copy), pub(crate))]
    #[set(pub(crate))]
    pub(crate) instance_capacity: usize,
    /// 复用缓冲:剔除近处实例时避免每次分配。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) scratch: Vec<Instance>,
    /// 验收探针:GPU 侧资产表长度(与 `Scene::meshes` 对照用)。
    ///
    /// 两者必须**完全相等**。`draw_batch` 把 `SceneBatch::mesh_index`
    /// 直接当本表下标取值,少传或多传一次都会让后面所有下标整体错位。
    #[get(type(copy), pub(crate))]
    pub(crate) gpu_mesh_count: usize,
    /// 验收探针:本批次实际准备画的索引数(0 = 被静默跳过)。
    #[get(type(copy), pub(crate))]
    pub(crate) gpu_index_count: i32,
    /// 验收探针:最近一次 `draw_batch` 的 `mesh_index` 是否越界。
    #[get(type(copy), pub(crate))]
    pub(crate) gpu_mesh_index_oob: usize,
    /// 增强管线的全部离屏目标(按画布尺寸惰性分配 / 重建)。
    #[set(skip, pub(crate))]
    pub(crate) targets: PipelineTargets,
}
/// 增强管线的离屏目标集合。
///
/// 全部惰性分配:画布尺寸变化时才重建,稳定分辨率下每帧零分配。
#[derive(Data, Debug, Default)]
pub(crate) struct PipelineTargets {
    /// 阴影贴图(只有深度)。
    #[get(skip, pub(crate))]
    pub(crate) shadow: Option<RenderTarget>,
    /// 主场景颜色(阴影 / AO / SSR 之后的最终颜色)。
    #[get(skip, pub(crate))]
    pub(crate) scene: Option<RenderTarget>,
    /// 法线 + 线性深度 G-buffer。
    #[get(skip, pub(crate))]
    pub(crate) gbuffer: Option<RenderTarget>,
    /// SSAO 原始输出(半分辨率)。
    #[get(skip, pub(crate))]
    pub(crate) ssao: Option<RenderTarget>,
    /// AO 双边模糊后的结果。
    #[get(skip, pub(crate))]
    pub(crate) ao: Option<RenderTarget>,
    /// SSR 结果(半分辨率)。
    #[get(skip, pub(crate))]
    pub(crate) ssr: Option<RenderTarget>,
    /// bloom 亮度提取(半分辨率)。
    #[get(skip, pub(crate))]
    pub(crate) bright: Option<RenderTarget>,
    /// bloom 模糊的 ping-pong 中转。
    #[get(skip, pub(crate))]
    pub(crate) blur_ping: Option<RenderTarget>,
    /// bloom 模糊的最终结果。
    #[get(skip, pub(crate))]
    pub(crate) blur_pong: Option<RenderTarget>,
    /// 目标分配时的画布宽高,用来判断是否需要重建。
    #[get(type(copy), pub(crate))]
    pub(crate) allocated: (u32, u32),
}
// ===========================================================================
// Canvas2D 软件渲染后端
// ===========================================================================
/// 一个待绘制的屏幕空间三角形。
#[derive(Clone, Data, Debug)]
pub(crate) struct ScreenTriangle {
    /// 三个屏幕顶点 `(x, y)`。
    #[get(type(copy), pub(crate))]
    pub(crate) points: [(f32, f32); 3],
    /// 归一化深度(越小越近)。
    #[get(type(copy), pub(crate))]
    pub(crate) depth: f32,
    /// 填充色(sRGB 量化后)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) fill: String,
}
/// Canvas2D 软件渲染后端(WebGL 不可用时的回退)。
///
/// 管线:变换顶点 → 世界空间背面剔除 → 投影到屏幕 → 画家算法深度排序
/// → 逐三角形填充。与 WebGL 后端共用 [`shade_face`],因此画面配色一致。
#[derive(Data)]
pub struct SoftwareRenderer {
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) context: euv::web_sys::CanvasRenderingContext2d,
}
