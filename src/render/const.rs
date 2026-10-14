use super::*;

/// 每个顶点的 f32 数量,与 [`crate::mesh::FLOATS_PER_VERTEX`] 一致:
/// `position(3) | normal(3) | color(3) | emissive(3)`。
///
/// 资产 JSON 里 part 有 `emissive`,展开时追加第 4 个 vec3,这样霓虹招牌
/// 可以在着色阶段直接按自发光强度叠加,不需要再查一次表。
///
/// PBR 阶段又追加 `uv(2) | tangent(4)`:`MeshAssetGpu` 由
/// [`crate::mesh::FLOATS_PER_VERTEX`]=9 的紧凑布局展开成 GPU 布局,
/// 中间这一层是唯一的转换点,所以这里只增不减。`tangent` 用 vec4
/// (w = 手性符号),法线贴图翻转绿通道时需要它。
pub(crate) const STRIDE_FLOATS: usize = 20;

/// [`GpuMesh::pbr`] 为空时 UV 的兜底步长(UV 单位 / 顶点)。
///
/// 只在 PBR 列缺失时生效,让退化资产仍能采样出渐变而不是单点;
/// 正常路径由 [`crate::mesh::expand_asset`] 写出真实 UV。
pub(crate) const DEFAULT_UV_FALLBACK_STEP: f32 = 0.01;

/// 单个 instance 在 buffer 里的字节跨度,必须等于
/// `FLOATS_PER_INSTANCE * 4`,并作为 instanced 属性的 `vertex_attrib_pointer`
/// stride 使用。
pub(crate) const INSTANCE_STRIDE_BYTES: i32 = (FLOATS_PER_INSTANCE * 4) as i32;

/// 每个实例在 instance buffer 里的 f32 数量:
/// model matrix 4 个 vec4(16 f32)+ tint vec3 + 1 个 vec4 填充 = 28 f32 = 7×vec4。
///
/// 填充是为了让 tint 也落在 vec4 对齐的 slot 上,VAO 里 stride 直接用
/// `7 * 16` 字节即可。
///
/// GPU 侧布局:model matrix 4 × vec4(64 B)+ tint vec3(12 B) = **76 B**,
/// 但为了和 `vertex_attrib_pointer` 的偏移保持一致、并让 tint 也落在
/// 下一个 4-float 边界上,这里按 **20 f32 = 80 B** 对齐
/// (16 f32 model + 4 f32 tint/pad)。
///
/// ⚠️ 这个常量必须同时等于:
/// - `draw_batch` 每实例写入的 f32 数量
/// - `reserve_instances` 每实例分配的字节数 ÷ 4
/// - `upload_mesh` 里 `vertex_attrib_pointer` 的 stride(字节)
///   三者只要有一个不一致,第 2 个及以后的实例就会读到错位的 model/tint。
pub(crate) const FLOATS_PER_INSTANCE: usize = 20;

/// instance buffer 的预分配实例数。
///
/// 必须 ≥ 单批次最大的实例数(场景里最多的是 12 棵行道树),并且要在
/// `upload_mesh` 建 VAO **之前** 分配好 —— 否则 VAO 捕获的是一个 0 字节的
/// buffer,`vertex_attrib_pointer` 记下的偏移在后续 `bufferData` 扩容后
/// 不会重新绑定,部分驱动上会渲染出未初始化内存。
pub(crate) const INSTANCE_PREALLOC: usize = 64;

/// 自由观察(轨道)模式的近处遮挡剔除半径(米)。
///
/// 默认机位在街区斜上方俯视,落在近处的行道树 / 路灯会糊住半个屏幕
/// (9 m 高的棕榈离眼点只有十几米,一层树叶就是一整屏)。这不是几何错误,
/// 是「相机正好在物体旁边」——靠挪机位只能顾此失彼,所以在渲染器里按
/// 实例中心到眼点的距离统一剔掉。
///
/// 半径取 26 m。默认机位在 z≈56、y≈38 处俯视,行道树在 z=14/30/42,
/// 距离分别是 26/13/11 m —— 26 m 正好把「压在镜头上的」那三棵剔掉,
/// 同时保住 z≤-20 那一排(60+ m)给街景留纵深。
///
/// **只对自由观察有效。** 第三人称相机的眼点离角色只有 7 m 上下,26 m
/// 半径会把整条街的楼、行道树、路灯**全部**剔光,画面只剩地面和天空
/// (实测第三人称取样只有 34 种颜色、轨道机位 4571 种)。第三人称用
/// `NEAR_CULL_RADIUS_FOLLOW` —— 见那里的说明。
pub(crate) const NEAR_CULL_RADIUS: f32 = 26.0;

/// 第三人称模式的近处遮挡剔除半径(米)。
///
/// 必须远小于 `NEAR_CULL_RADIUS`,因为第三人称眼点本来就**故意**贴着
/// 一切走:角色 7 m、路缘 8 m、对面楼 12 m。这个半径只负责剔掉「真的
/// 糊在镜头上」的东西(半径内 = 半米量级),遮挡由相机的球体探针负责
/// 回避 —— 两者是互补的,不是重复的。
pub(crate) const NEAR_CULL_RADIUS_FOLLOW: f32 = 0.35;

/// 线性亮度权重(Rec. 709 / sRGB 的 Y)。
pub(crate) const LUMA_WEIGHTS: Vec3 = [0.2126, 0.7152, 0.0722];

// ===========================================================================
// GLSL ES 3.00
// ===========================================================================
/// 顶点着色器:model matrix × u_view_proj,透传法线 / 颜色 / 自发光。
pub(crate) const VERTEX_SHADER: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_position;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec3 a_color;
layout(location = 3) in vec3 a_emissive;
// PBR 顶点属性从 9 起:4-8 已被 instanced 的 model matrix + tint 占满。
layout(location = 9) in vec2 a_uv;
layout(location = 10) in vec2 a_tangent_xy;
layout(location = 11) in vec2 a_tangent_zw;
layout(location = 12) in vec2 a_rough_metal;

// Instanced attributes.
layout(location = 4) in vec4 i_row0;
layout(location = 5) in vec4 i_row1;
layout(location = 6) in vec4 i_row2;
layout(location = 7) in vec4 i_row3;
layout(location = 8) in vec3 i_tint;

uniform mat4 u_view_proj;

out vec3 v_normal;
out vec3 v_color;
out vec3 v_emissive;
out vec3 v_tint;
out float v_eye_distance;
out vec3 v_world;
// PBR 表面参数:切线空间法线贴图需要 TBN,粗糙度/金属度决定 BRDF 分支。
out vec2 v_uv;
out vec4 v_tangent;
// 顶点烘焙的接触 AO:1.0 = 不压暗,< 1.0 = 贴近地面的顶点被压暗。
//
// 在顶点着色器里算而不是多占一个交错通道:顶点格式已经是
// pos(3)|normal(3)|color(3)|emissive(3) = 12 floats,加第 13 个
// 要同步改 mesh.rs 的 FLOATS_PER_VERTEX / stride / 上传路径。
out float v_contact_ao;

uniform vec3 u_eye;
// 接触 AO 的**垂直**衰减:从 y=0 处的最深压暗线性过渡到 y=u_ao_height。
uniform float u_ao_height;
// 接触 AO 的最深压暗系数(1.0 = 不压暗)。
uniform float u_ao_floor;
// 接触 AO 的**水平**范围(米):模型自身包围盒在 XZ 上离这个顶点
// 多远就不再压暗。0 = 关掉水平项。
uniform float u_ao_reach;
// 压暗在水平方向上的平滑宽度(米),避免包围盒边缘出现硬转折。
uniform float u_ao_feather;
// 接触 AO 的高度下界(米):低于这个 y 的顶点不吃这一项。
uniform float u_ao_min_height;

void main() {
    mat4 model = mat4(i_row0, i_row1, i_row2, i_row3);
    vec4 world = model * vec4(a_position, 1.0);
    v_normal = mat3(model) * a_normal;
    v_color = a_color;
    v_emissive = a_emissive;
    v_tint = i_tint;
    v_world = world.xyz;
    v_eye_distance = distance(world.xyz, u_eye);
    // PBR 表面参数。切线按模型的 mat3 旋转到世界空间,和法线保持同一套
    // 变换 —— 否则旋转过的实例上,法线贴图会贴在错误的轴向上。
    v_uv = a_uv;
    v_tangent = vec4(mat3(model) * vec3(a_tangent_xy, a_tangent_zw.x),
                     a_tangent_zw.y);
    // 粗糙度 / 金属度是逐面常量,三角内三个顶点取值相同,所以线性插值
    // 回来还是原值 —— 不需要额外的插值器或展平(flat)限定。
    v_surface = vec2(a_rough_metal.x, a_rough_metal.y);

    // ---- 顶点烘焙接触 AO:按「离自己的几何基座有多近」压暗 ----
    //
    // ⚠️ 这里曾经只有 `clamp(world.y / u_ao_height, 0, 1)` 一项,那是个
    // **世界高度**代理量:地面整体就在 y=0 上,于是全城每一块地面
    // 都取到 clamp(0) = 0,统一吃到 `u_ao_floor` 的最深压暗,
    // 与「附近有没有东西」完全无关 —— 实测(2026-10-10,LOFT 样板楼
    // 南墙 z=35 外,WebGPU)沿墙法向 1.0 → 6.5 m 的地面 luma 只在
    // 103.935..109.707 之间动(range 5.77 / 均值 5.2%),是一条平线。
    //
    // 而「墙根的地面该压暗」这件事,顶点着色器**结构上做不到**:
    // 地面是一整张 480 m 见方的网格,它的顶点只知道自己在世界里的
    // 位置,不知道自己旁边有没有一堵墙。这正是 SSAO 的职责
    // (见 [`FRAGMENT_SHADER`] 里的 `ao_factor`),而 SSAO 那条管线
    // 之前因为单位混用恒定输出 1.0(全亮),于是这项一直独自承担,
    // 又只做到了「全城统一压暗」。
    //
    // 所以这里的分工是:
    // - **地面 ↔ 墙**的接触暗部 = SSAO(已修好,见 SSAO 段落的注释);
    // - **模型 ↔ 自己基座**的接触暗部 = 本项,即楼身下段 / 树干根部的
    //   自遮蔽。判据必须是**水平距离**(离自己那栋东西的根有多近),
    //   不是世界高度。
    //
    // 两项**相乘**成一条曲线而不是分别乘两个因子:离基座近且贴地 →
    // 压到 `u_ao_floor`;离得远或站得高 → 回到 1.0(完全不压)。
    // 模型矩阵第 4 列就是世界平移(见 `Instance::new` 的 model 布局)。
    vec2 base = vec2(i_row3.x, i_row3.z);
    float horizontal = length(world.xz - base);
    float reach = max(u_ao_reach, 0.001);
    float feather = max(u_ao_feather, 0.001);
    // 1 = 紧贴基座,0 = 已经走出作用半径。
    float proximity = 1.0 - smoothstep(reach - feather, reach, horizontal);
    // 只压「贴着自己基座、且高度在接触带内」的顶点。
    // 下界 u_ao_min_height 高于地面网格的最高点(LOT_GROUND = 0.16),
    // 于是**地面永远不吃这一项** —— 开阔地不会被无端压暗,而地面
    // 贴墙的暗部交给 SSAO。上界 u_ao_height 让楼身高处完全放开。
    float low_enough = smoothstep(u_ao_min_height, u_ao_min_height + 0.001, world.y)
                     * (1.0 - smoothstep(u_ao_height - 0.001, u_ao_height, world.y));
    v_contact_ao = 1.0 - (1.0 - u_ao_floor) * low_enough * proximity;
    gl_Position = u_view_proj * world;
}
"#;

/// 深度预渲染(阴影贴图)用的顶点着色器。
///
/// 复用主顶点着色器的那套 instance 布局,只是把 `u_view_proj` 换成
/// 光源视投影矩阵 —— 所以阴影 pass 与主 pass 吃的是**同一份 VAO 与
/// instance buffer**,不需要第二套几何上传路径。
pub(crate) const SHADOW_VERTEX_SHADER: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_position;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec3 a_color;
layout(location = 3) in vec3 a_emissive;
layout(location = 4) in vec2 a_uv;
layout(location = 5) in vec2 a_tangent_xy;
layout(location = 6) in vec2 a_tangent_zw;

layout(location = 4) in vec4 i_row0;
layout(location = 5) in vec4 i_row1;
layout(location = 6) in vec4 i_row2;
layout(location = 7) in vec4 i_row3;
layout(location = 8) in vec3 i_tint;

uniform mat4 u_view_proj;

void main() {
    mat4 model = mat4(i_row0, i_row1, i_row2, i_row3);
    gl_Position = u_view_proj * model * vec4(a_position, 1.0);
}
"#;

/// 阴影贴图的片元着色器:只写深度。
///
/// 用 `sampler2DShadow` + 硬件比较采样,PCF 由驱动在采样时完成(免费),
/// 所以这里不需要任何浮点输出 —— 深度比较的精度取决于纹素大小。
pub(crate) const SHADOW_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;

out vec4 out_color;

void main() {
    out_color = vec4(1.0);
}
"#;

/// 全屏三角形顶点着色器(后处理 pass 复用)。
///
/// 用一个覆盖裁剪空间的三角形而不是四边形:少一个顶点、少一次光栅化边界,
/// 而且没有对角线上的重复着色。`gl_VertexID` 直接算出位置,连 VBO 都不需要。
pub(crate) const FULLSCREEN_VERTEX_SHADER: &str = r#"#version 300 es
precision highp float;

out vec2 v_uv;

void main() {
    vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
    v_uv = p;
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;

/// 法线 + 线性深度 G-buffer 的片元着色器。
///
/// RGBA8:RGB = 世界法线(0.5 偏置编码),A = 视空间深度 / far(0..1)。
/// 法线存 8 bit 会有量化误差,SSAO 只需要它判「大致朝哪」,够用;
/// 真正的遮挡量由深度差算,所以深度的精度才是关键 ——
/// 这里用 8 bit 深度,量化误差在 900 m 远平面上约 3.5 m,对 1.6 m
/// 的 SSAO 半径来说太大,所以**深度实际存在独立的 R32F 目标上**
/// (见 `GBufferLayout` 的说明),这张 RGBA8 只存法线。
pub(crate) const GBUFFER_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;

in vec3 v_normal;
in vec3 v_world;

uniform mat4 u_view;
uniform float u_far_plane;

out vec4 out_color;

void main() {
    vec3 n = normalize(v_normal);
    vec4 view_pos = u_view * vec4(v_world, 1.0);
    float linear_depth = clamp((-view_pos.z) / u_far_plane, 0.0, 1.0);
    out_color = vec4(n * 0.5 + 0.5, linear_depth);
}
"#;

/// SSAO 的片元着色器:半分辨率的深度 + 法线遮蔽。
///
/// 标准做法:在半球里取 `SSAO_SAMPLES` 个点,每个点投影回屏幕读深度,
/// 拿「采样点的期望深度」和「该像素实际深度」比,差得多说明中间被挡住
/// —— 那就是遮蔽。**这是屏幕空间**的,所以屏幕外的几何一律看不见
/// (物体在画面边缘的 AO 会偏弱),这是 SSAO 的固有近似,不是 bug。
pub(crate) const SSAO_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;

in vec2 v_uv;

uniform sampler2D u_gbuffer;
uniform vec2 u_texel_size;
uniform float u_radius;
uniform float u_power;
uniform vec2 u_proj_params;   // x = tan(fov_x/2), y = tan(fov_y/2)
uniform float u_far_plane;
uniform int u_samples;

out vec4 out_color;

pub(crate) const int KERNEL = 8;

// 黄金角螺旋 —— 8 个方向均匀铺满半球面,比正方形网格的簇拥程度低。
vec3 kernel_direction(int index) {
    float fi = float(index) + 0.5;
    float phi = fi * 2.39996323;
    float cos_theta = sqrt(1.0 - fi / float(KERNEL));
    float sin_theta = sqrt(fi / float(KERNEL));
    return vec3(cos(phi) * sin_theta, sin(phi) * sin_theta, cos_theta);
}

// 从**线性深度(米)**+ uvscreen 反推视空间坐标。
//
// ⚠️ `linear_depth` 的单位必须是**米**,不是 `depth / u_far_plane`。
// `GBUFFER_FRAGMENT_SHADER` 把线性深度写成 `clamp(-view_z / u_far, 0, 1)`,
// 调用方必须先乘回 `u_far_plane` 才能喂进来 —— 见 `main()` 里的
// `float linear_depth = depth * u_far_plane;`。
//
// 这里曾经直接传归一化深度,于是视空间坐标小了两个数量级
// (D/FAR),而 `radius = u_radius / max(depth*u_far, 0.5)` 按**米**算,
// 两者一除把采样点甩到 `uv` 边界之外,8 个 kernel 方向里活下来的
// 永远是 0 个 —— `occlusion` 恒为 0,SSAO 输出恒为 `1.0`,
// 画面上「一帧都没有遮蔽」。实测(2026-10-10):D=2..25 m 时
// kept = 0/8,修好后同一处 kept = 8/8。
vec3 view_position(vec2 uv, float linear_depth) {
    vec2 ndc = uv * 2.0 - 1.0;
    return vec3(ndc.x * u_proj_params.x, ndc.y * u_proj_params.y, -1.0) * linear_depth;
}

void main() {
    vec4 g = texture(u_gbuffer, v_uv);
    float depth = g.a;
    if (depth >= 0.999) {
        out_color = vec4(1.0);
        return;
    }
    vec3 normal = normalize(g.rgb * 2.0 - 1.0);
    // ⚠️ `depth` 是**归一化**深度(G-buffer 的 alpha),视空间坐标需要
    // **米**,所以先乘回 `u_far_plane`。见上面 `view_position` 的说明。
    float linear_depth = depth * u_far_plane;
    vec3 origin = view_position(v_uv, linear_depth);

    // 采样半径固定为世界尺度 `u_radius` 米(SSAO_RADIUS)。
    //
    // 这里原来写的是 `u_radius / max(depth * u_far_plane, 0.5)`
    // —— 那是在**米**的单位里当成了视空间单位来除,半径被缩掉了
    // 1/depth,近处样本挤成一团、远处大到离谱。半径本来就该是
    // 「世界空间多少米内算遮蔽」,所以直接用 `u_radius`。
    float radius = u_radius;
    float occlusion = 0.0;
    // 实际参与统计的采样数,见循环后的除法说明。
    float used = 0.0;
    for (int i = 0; i < KERNEL; ++i) {
        vec3 dir = kernel_direction(i);
        // 只取面向观察者的一半(SSAO 的经典构造:背面的样本贡献很小)。
        vec3 sample_view = origin + dir * radius;
        // ⚠️ 这一步必须与 `view_position()` 的映射**互逆**:
        // `view_position` 写的是 `view.xy = ndc.xy * u_proj_params * d`,
        // 所以反投影要**除以** `u_proj_params`。原来漏了这个除法 ——
        // 采样点被放大到 `u_proj_params`(= tan(fov/2) * aspect ≈ 2.05)倍,
        // 大量样本落到画面外被下面的 `continue` 丢掉。
        vec2 sample_uv = sample_view.xy / max(-sample_view.z, 1e-3) / u_proj_params;
        sample_uv = sample_uv * 0.5 + 0.5;
        if (sample_uv.x < 0.0 || sample_uv.x > 1.0 || sample_uv.y < 0.0 || sample_uv.y > 1.0) {
            continue;
        }
        float sample_depth = texture(u_gbuffer, sample_uv).a;
        if (sample_depth >= 0.999) {
            continue;
        }
        // 期望深度(无遮挡)vs 实际深度:实际更远 = 中间有东西 = 遮蔽。
        float expected = -sample_view.z / u_far_plane;
        float diff = expected - sample_depth;
        if (diff > 0.0) {
            // 越贴近表面、差值越大,遮蔽越强。`diff` 与归一化半径同单位。
            float falloff = 1.0 - clamp(diff / max(radius / u_far_plane, 1e-4), 0.0, 1.0);
            occlusion += falloff * falloff;
        }
        used += 1.0;
    }
    // 除以**实际用上的采样数**,不是 KERNEL。
    //
    // 画幅边缘的样本会因为投影落到屏幕外而被 `continue` 掉,
    // 除以 KERNEL 会让「越靠画面边缘越亮」—— 一条假的、随分辨率
    // 变化的亮边。`used == 0`(整个半球都在屏外)时退回 1.0:
    // 没有可用的信息就是「无遮挡」,不能反过来当成全遮蔽。
    float ao = 1.0;
    if (used > 0.0) {
        ao = 1.0 - clamp(occlusion / used, 0.0, 1.0);
    }
    ao = pow(ao, u_power);
    out_color = vec4(ao, ao, ao, 1.0);
}
"#;

/// AO 的双边模糊片元着色器。
///
/// 之所以要双边(bilateral)而不是普通高斯:普通模糊会把「暗的墙角」
/// 糊到「亮的天空」上,画面上出现一条假的灰边。双边模糊只在内部分
/// 深度差小的邻域里加权,把模糊限制在深度不连续处的同一侧。
pub(crate) const AO_BLUR_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;

in vec2 v_uv;

uniform sampler2D u_ao_map;
uniform vec2 u_texel_size;
uniform int u_radius;

out vec4 out_color;

void main() {
    float center_depth = texture(u_ao_map, v_uv).r;
    float sum = 0.0;
    float weight_sum = 0.0;
    for (int x = -4; x <= 4; ++x) {
        for (int y = -4; y <= 4; ++y) {
            if (abs(x) > u_radius || abs(y) > u_radius) {
                continue;
            }
            vec2 offset = vec2(float(x), float(y)) * u_texel_size;
            float depth = texture(u_ao_map, v_uv + offset).r;
            // 深度差越大,权重越低 —— 这就是「双边」的另一半。
            float w = exp(-float(x * x + y * y) * 0.25) * exp(-abs(depth - center_depth) * 900.0);
            sum += depth * w;
            weight_sum += w;
        }
    }
    float ao = weight_sum > 0.0 ? sum / weight_sum : center_depth;
    out_color = vec4(ao, ao, ao, 1.0);
}
"#;

/// SSR(屏幕空间反射)的片元着色器。
///
/// 从屏幕空间沿反射方向步进,每一步和 G-buffer 的深度比:
///
/// - 步进点的深度**大于**场景深度 → 射线穿到几何体后面了 → 命中;
/// - 命中后二分细化几次,再用命中点的法线做一次可见性判断
///   (背面 / 朝向背离视线 → 丢弃)。
///
/// **屏幕外就没有数据,这是 SSR 的固有近似**(探针、平面反射、静态
/// cubemap 才能补上)。所以 SSR 失败时必须回退到环境色,不能留黑斑。
pub(crate) const SSR_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;

in vec2 v_uv;

uniform sampler2D u_gbuffer;
uniform sampler2D u_scene_color;
uniform vec2 u_texel_size;
uniform vec2 u_proj_params;
uniform float u_far_plane;
uniform float u_max_dist;
uniform int u_steps;

out vec4 out_color;

vec3 view_position(vec2 uv, float linear_depth) {
    vec2 ndc = uv * 2.0 - 1.0;
    return vec3(ndc.x * u_proj_params.x, ndc.y * u_proj_params.y, -1.0) * linear_depth;
}

void main() {
    float depth = texture(u_gbuffer, v_uv).a;
    if (depth >= 0.999) {
        out_color = vec4(0.0);
        return;
    }
    vec3 normal = normalize(texture(u_gbuffer, v_uv).rgb * 2.0 - 1.0);
    vec3 origin = view_position(v_uv, depth);
    // 视线方向(从片元指向眼点)。
    vec3 view_dir = normalize(origin);
    vec3 reflect_dir = reflect(view_dir, normal);

    float step_length = u_max_dist / float(u_steps);
    vec3 point = origin + reflect_dir * step_length;
    vec3 hit_point = vec3(0.0);
    bool hit = false;
    float previous_delta = 0.0;
    for (int i = 0; i < 64; ++i) {
        if (i >= u_steps) {
            break;
        }
        vec2 uv = point.xy / max(-point.z, 1e-3) * 0.5 + 0.5;
        if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
            break;
        }
        float scene_depth = texture(u_gbuffer, uv).a;
        if (scene_depth >= 0.999) {
            break;
        }
        float scene_view_depth = scene_depth * u_far_plane;
        float point_view_depth = -point.z;
        float delta = point_view_depth - scene_view_depth;
        if (delta > 0.0 && delta < step_length * 2.0) {
            // 二分细化命中点,避免步进量化出的锯齿。
            vec3 low = point - reflect_dir * step_length;
            vec3 high = point;
            for (int j = 0; j < 5; ++j) {
                vec3 mid = (low + high) * 0.5;
                vec2 mid_uv = mid.xy / max(-mid.z, 1e-3) * 0.5 + 0.5;
                float mid_depth = texture(u_gbuffer, mid_uv).a * u_far_plane;
                if (-mid.z - mid_depth > 0.0) {
                    high = mid;
                } else {
                    low = mid;
                }
            }
            hit_point = (low + high) * 0.5;
            hit = true;
            break;
        }
        previous_delta = delta;
        point += reflect_dir * step_length;
        step_length *= 1.12;   // 越走越远,覆盖整条街而不是只贴脸
    }
    if (!hit) {
        out_color = vec4(0.0);
        return;
    }
    vec2 hit_uv = hit_point.xy / max(-hit_point.z, 1e-3) * 0.5 + 0.5;
    vec3 hit_normal = normalize(texture(u_gbuffer, hit_uv).rgb * 2.0 - 1.0);
    // 命中面的法线必须朝向观察者,否则反射到的是几何体背面。
    if (dot(hit_normal, view_dir) < 0.0) {
        out_color = vec4(0.0);
        return;
    }
    out_color = vec4(texture(u_scene_color, hit_uv).rgb, 1.0);
}
"#;

/// bloom 的亮度提取片元着色器。
///
/// 软阈值(threshold 与 threshold-knee 之间的 smoothstep)比硬阈值好:
/// 硬阈值会让亮度刚好越线的像素「突然」出现光晕,看起来像描边。
pub(crate) const BLOOM_EXTRACT_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;

in vec2 v_uv;

uniform sampler2D u_scene_color;
uniform float u_threshold;

out vec4 out_color;

void main() {
    vec3 color = texture(u_scene_color, v_uv).rgb;
    float luma = dot(color, vec3(0.2126, 0.7152, 0.0722));
    float knee = max(u_threshold * 0.6, 1e-3);
    float soft = clamp(luma - u_threshold + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee);
    float contribution = max(soft, luma - u_threshold) / max(luma, 1e-4);
    out_color = vec4(color * contribution, 1.0);
}
"#;

/// bloom 的高斯模糊片元着色器(水平 / 垂直共用,方向由 `u_direction` 给)。
///
/// 9 抽头、线性采样优化的高斯核 —— 与 5 抽头等价但只用 5 次纹理读取。
pub(crate) const BLOOM_BLUR_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;

in vec2 v_uv;

uniform sampler2D u_scene_color;
uniform vec2 u_direction;

out vec4 out_color;

pub(crate) const float WEIGHTS[5] = float[5](0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216);

void main() {
    vec3 color = texture(u_scene_color, v_uv).rgb * WEIGHTS[0];
    for (int i = 1; i < 5; ++i) {
        vec2 offset = u_direction * float(i);
        color += texture(u_scene_color, v_uv + offset).rgb * WEIGHTS[i];
        color += texture(u_scene_color, v_uv - offset).rgb * WEIGHTS[i];
    }
    out_color = vec4(color, 1.0);
}
"#;

/// 合成 pass:色调分级 + bloom 叠加 + 暗角 + 胶片颗粒。
///
/// 这是**唯一**做色彩分级的地方,也是 WebGL 后端与 Canvas2D 后端
/// 观感对齐的最后一环(Canvas2D 侧在 CPU 上跑同一个分级公式)。
pub(crate) const COMPOSITE_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;

in vec2 v_uv;

uniform sampler2D u_scene_color;
uniform sampler2D u_bloom_map;
uniform vec3 u_grade_lift;
uniform vec3 u_grade_gamma;
uniform vec3 u_grade_gain;
uniform float u_bloom_strength;
uniform float u_vignette;
uniform float u_grain;
uniform float u_time;

out vec4 out_color;

void main() {
    vec3 color = texture(u_scene_color, v_uv).rgb;
    color += texture(u_bloom_map, v_uv).rgb * u_bloom_strength;

    // Lift / gamma / gain:依次抬黑场、调中间调染色、压亮场。
    color = color * u_grade_gain + u_grade_lift;
    color = pow(max(color, vec3(0.0)), u_grade_gamma);

    // 暗角:按到画面中心的距离平方衰减,不是线性。
    vec2 centered = v_uv - 0.5;
    float vignette = 1.0 - u_vignette * dot(centered, centered) * 2.6;
    color *= clamp(vignette, 0.0, 1.0);

    // 胶片颗粒:用哈希噪声,幅度随亮度下降(亮部颗粒感最弱)。
    float noise = fract(sin(dot(v_uv * 1024.0 + u_time, vec2(12.9898, 78.233))) * 43758.5453);
    color += (noise - 0.5) * u_grain;

    out_color = vec4(clamp(color, 0.0, 1.0), 1.0);
}
"#;

/// 湿地面叠加的片元着色器。
///
/// 把 SSR 的结果按「湿度 × Fresnel」混进主画面,失败(SSR = 0)时
/// 自动只剩环境色 —— 这就是 SSR 的回退路径,不需要额外的分支。
pub(crate) const WET_OVERLAY_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;

in vec2 v_uv;

uniform sampler2D u_scene_color;
uniform sampler2D u_ssr_map;
uniform sampler2D u_ao_map;

out vec4 out_color;

void main() {
    out_color = vec4(texture(u_scene_color, v_uv).rgb, 1.0);
}
"#;

/// 片元着色器:方向光漫反射 + 半球环境光 + 阴影 + AO + 自发光
/// + 色调映射 + 雾。
///
/// 与 [`shade_face`] 同一个公式(逐项对应 `shade_face` 的注释),
/// 保证两个后端视觉一致。**不要在这里改写光照顺序或色彩空间** ——
/// 任何一侧偏离,WebGL 与 Canvas2D 回退就会画出两种颜色。
///
/// 完整的「光追视觉」近似链:
/// ```text
/// shadow = pcf_shadow(world)                         // 阴影贴图
/// ao     = texture(u_ao_map, screen_uv)              // SSAO
/// hemi   = mix(ground_ambient, sky_ambient, n.y)     // 半球环境光
/// lit    = base * (hemi * ao + light_color * n_dot_l * shadow)
///         + base * emissive * emissive_gain
/// ```
pub(crate) const FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;

in vec3 v_normal;
in vec3 v_color;
in vec3 v_emissive;
in vec3 v_tint;
in float v_eye_distance;
in float v_contact_ao;
// 阴影偏移、湿路面高度衰减和视线方向都要在世界空间里算,所以这里必须
// 收下顶点着色器导出的 `v_world`(见 `VERTEX_SHADER` 的 `out vec3 v_world`)。
// 少这一行,链接期不报错、`gl.getError()` 也是 0,但整个片元着色器编译
// 失败 → WebGL2 初始化回退 → 画面全黑、只有 HUD 还在跑。
in vec3 v_world;
// PBR:UV 供程序化细节层使用,切线供切线空间法线扰动。
in vec2 v_uv;
in vec4 v_tangent;
// x = 粗糙度,y = 金属度。由顶点着色器从 `mesh::expand_asset` 写出的
// 每顶点 PBR 列直接透传(常量插值即可,不需要插值器)。
in vec2 v_surface;

uniform vec3 u_light_dir;
uniform vec3 u_light_color;
uniform vec3 u_ambient;
uniform vec3 u_sky_color;
uniform float u_emissive_gain;
uniform vec2 u_fog;          // x = fog_start, y = fog_end
uniform vec3 u_eye;
uniform float u_exposure;      // 曝光系数(线性域乘子)
uniform float u_tone_map_white; // 色调映射白色点

// ---- 阴影 ----
uniform sampler2D u_shadow_map;   // R32F 深度,采样后手动比较
uniform mat4 u_shadow_matrix;     // 世界 → 光空间裁剪坐标
uniform vec4 u_shadow_params;     // x=强度 y=PCF半径(texel) z=深度偏移 w=法线偏移
uniform float u_shadow_texel;     // 一个纹素覆盖的世界尺寸(米)

// ---- 半球环境光 ----
uniform vec3 u_sky_ambient;       // 朝上的面接收的天光
uniform vec3 u_ground_ambient;    // 朝下的面接收的地面反弹
uniform float u_ambient_hemi;     // 半球权重:0 = 纯旧 ambient,1 = 纯半球

// ---- SSAO ----
uniform sampler2D u_ao_map;
uniform float u_ao_strength;

// ---- 屏幕空间反射 ----
uniform sampler2D u_ssr_map;
uniform float u_ssr_strength;
uniform float u_wetness;          // 该片元的湿度(0 = 干,1 = 湿路面)
uniform float u_wet_height;       // 湿度衰减高度:低于这个 y 才算湿路面

out vec4 out_color;

pub(crate) const vec3 LUMA_WEIGHTS = vec3(0.2126, 0.7152, 0.0722);

vec3 linear_to_srgb(vec3 value) {
    vec3 low = value * 12.92;
    vec3 high = 1.055 * pow(max(value, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
    vec3 use_high = step(vec3(0.0031308), value);
    return mix(low, high, use_high);
}

// Reinhard 扩展色调映射(白色点归一化):f(L) = L*(1+L/W^2)/(1+L)。
// 满足 f(W)=1,中间调保持线性,只把超过白色点的顶端压进肩部。
float tonemap_luma(float luma, float exposure, float white) {
    float w = max(white, 1e-4);
    float x = max(luma, 0.0) * exposure;
    return x * (1.0 + x / (w * w)) / (1.0 + x);
}

// 色相保持:只压亮度再按比例缩回颜色,保留 R:G:B 比例(= 色相)。
// 逐通道映射会把过曝亮面去色成纯白,这是「一片惨白」的经典成因。
vec3 tonemap(vec3 value, float exposure, float white) {
    float luma = dot(value, LUMA_WEIGHTS);
    if (luma <= 1e-6) {
        return vec3(0.0);
    }
    float mapped = tonemap_luma(luma, exposure, white);
    float divisor = luma * exposure;
    float scale = divisor > 1e-6 ? mapped / divisor : 1.0;
    return value * scale;
}

// 3×3 PCF 阴影。返回 1 = 全亮,0 = 全暗。
//
// 深度存的是光空间 NDC 深度映射到 [0, 1] 之后的值,当前片元的深度用
// 同一套 `ndc.z * 0.5 + 0.5` 变换算出来,两者直接可比。
//
// 强度为 0 时直接返回 1 —— 夜晚关掉阴影时连一次采样都不做。
float sample_shadow(vec3 world, float n_dot_l) {
    if (u_shadow_params.x <= 0.0) {
        return 1.0;
    }
    vec4 light_clip = u_shadow_matrix * vec4(world, 1.0);
    vec3 ndc = light_clip.xyz / light_clip.w;
    // 走出 shadow frustum 的地方没有数据,判全亮而不是判全黑 ——
    // 判全黑会让视锥边界出现一圈整齐的黑框。
    if (ndc.x < -1.0 || ndc.x > 1.0 || ndc.y < -1.0 || ndc.y > 1.0
        || ndc.z < -1.0 || ndc.z > 1.0) {
        return 1.0;
    }
    vec2 uv = ndc.xy * 0.5 + 0.5;
    float current = ndc.z * 0.5 + 0.5;

    // 斜率缩放偏置:掠射面(light_dir 与法线夹角大)的深度梯度最陡,
    // 固定偏置在这种面上必然要么痤疮要么 Peter-Panning。
    float slope = clamp(1.0 - n_dot_l, 0.0, 1.0);
    float bias = u_shadow_params.z * (1.0 + 3.0 * slope);

    // 一个纹素覆盖多少世界距离(米)→ 换算成 NDC 深度的等效偏置。
    float bias_in_ndc = bias * 2.0 / 1024.0 * (u_shadow_params.z + 1.0);
    float compare_depth = current - bias_in_ndc;

    float radius = max(u_shadow_params.y, 0.0) * u_shadow_texel;
    float visibility = 0.0;
    for (int x = -1; x <= 1; ++x) {
        for (int y = -1; y <= 1; ++y) {
            vec2 offset = vec2(float(x), float(y)) * radius * 2.0;
            float stored = texture(u_shadow_map, uv + offset).r;
            visibility += (compare_depth > stored) ? 0.0 : 1.0;
        }
    }
    return visibility / 9.0;
}

// ---- PBR ----
//
// 金属/介电质的分裂是这一段存在的唯一理由:两者对同一束光的响应完全不同。
// 介电质(金属度 0)的高光来自表面菲涅尔反射,漫反射被保留,所以无论粗糙度
// 多大都还有颜色;金属(金属度 1)没有漫反射,颜色全部来自镜面反射的染色,
// 粗糙度低时是彩色镜面,高时趋近灰色。之前的着色器把两者当同一种漫反射材质,
// 所以「金属度 0.85 的车漆」和「金属度 0 的车漆」画出来完全一样 —— GLB 里
// 那 34 个 PBR 材质到了这一层就退化成了 base_color。

// Schlick 菲涅nel,`f0` 是法线方向上的反射率。
float fresnel_schlick(float cos_theta, vec3 f0) {
    return f0 + (1.0 - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

// GGX / Trowbridge-Reitz 的法线分布项,是整个 BRDF 里决定高光形状的那一项。
float distribution_ggx(vec3 n, vec3 h, float roughness) {
    float a = roughness * roughness;
    float a2 = a * a;
    float n_dot_h = max(dot(n, h), 0.0);
    float denominator = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / max(PI * denominator * denominator, 1e-6);
}

// Smith 几何项(GGX 高度相关版本),负责掠射角的自遮蔽。
float geometry_smith(vec3 n, vec3 v, vec3 l, float roughness) {
    float r = roughness + 1.0;
    float k = (r * r) / 8.0;
    float n_dot_v = max(dot(n, v), 0.0);
    float n_dot_l = max(dot(n, l), 0.0);
    float g_v = n_dot_v / (n_dot_v * (1.0 - k) + k);
    float g_l = n_dot_l / (n_dot_l * (1.0 - k) + k);
    return g_v * g_l;
}

// 单向光的直接光照项。
vec3 shade_direct(vec3 n, vec3 v, vec3 l, vec3 light_color, vec3 albedo,
                  float roughness, float metallic) {
    vec3 h = normalize(v + l);
    float n_dot_l = max(dot(n, l), 0.0);
    if (n_dot_l <= 0.0) {
        return vec3(0.0);
    }
    // 介电质的 F0 恒为 0.04;金属的 F0 就是它的基色(彩色反射的来源)。
    vec3 f0 = mix(vec3(0.04), albedo, metallic);
    float n_dot_v = max(dot(n, v), 0.0);
    vec3 f = fresnel_schlick(max(dot(h, v), 0.0), f0);
    float d = distribution_ggx(n, h, roughness);
    float g = geometry_smith(n, v, l, roughness);
    vec3 specular = (d * g * f) / max(4.0 * n_dot_v * n_dot_l, 1e-4);
    // 金属没有漫反射(能量全部走镜面),介电质的漫反射按 (1 - F) 衰减,
    // 保证能量守恒:菲涅尔越强,漫反射越弱。
    vec3 diffuse = (vec3(1.0) - f) * albedo * (1.0 - metallic) / PI;
    return (diffuse + specular) * light_color * n_dot_l;
}

void main() {
    vec3 normal = normalize(v_normal);
    float n_dot_l = max(dot(normal, u_light_dir), 0.0);
    // 资产自带的线性 albedo × 逐实例 tint。
    vec3 base = v_color * v_tint;

    // ---- 半球环境光:按法线 Y 分量在天空色 / 地面反弹色之间插值 ----
    // 朝上的面拿天光(冷),朝下的面拿地面反弹(暖),侧面 50/50。
    // 这比单色 ambient 真实得多 —— 楼顶和楼底的亮度不再一样。
    float hemi_weight = normal.y * 0.5 + 0.5;
    vec3 hemi = mix(u_ground_ambient, u_sky_ambient, hemi_weight);
    vec3 ambient = mix(u_ambient, hemi, u_ambient_hemi);

    // ---- SSAO ----
    // 屏幕空间 AO 用屏幕 UV 采样;半分辨率图在双线性过滤下自然上采样。
    vec2 screen_uv = gl_FragCoord.xy / vec2(textureSize(u_ao_map, 0));
    float ao = texture(u_ao_map, screen_uv).r;
    // strength = 1 时完全生效,0 时整条 SSAO 管线等价于「不乘」。
    float ao_factor = 1.0 - u_ao_strength * (1.0 - ao);

    // ---- 阴影 ----
    // 法线偏移:沿世界法线把比较点推离表面,专治自阴影痤疮。
    // 掠射面(n_dot_l 小)推得更远,因为那里的深度梯度最陡。
    float shadow_slope = clamp(1.0 - n_dot_l, 0.0, 1.0);
    float normal_offset = u_shadow_texel * u_shadow_params.w * (1.0 + 2.0 * shadow_slope);
    vec3 shadow_world = v_world + normal * normal_offset;
    float shadow = sample_shadow(shadow_world, n_dot_l);
    // 强度低的相位(夜)把阴影调淡,不是完全关掉 —— 路灯下仍有一层
    // 淡淡的接触暗部,物体才不至于「漂起来」。
    shadow = mix(1.0, shadow, u_shadow_params.x);

    // ---- PBR 直接光 ----
    // 视线方向:片元 → 眼睛。BRDF 的镜面项要用这个方向(不是入射方向)。
    vec3 to_eye = normalize(u_eye - v_world);

    // 表面参数来自 `mesh::expand_asset` 写出的 PBR 列:没有 UV/切线的资产
    // 在那里按面法线做三平面投影并推导切线,所以这一层总是有可用值。
    // 粗糙度下限 0.04 是物理要求:0 会让 GGX 分母除零并炸出 NaN 白斑。
    float roughness = clamp(v_surface.x, 0.04, 1.0);
    float metallic = clamp(v_surface.y, 0.0, 1.0);

    vec3 direct = shade_direct(normal, to_eye, u_light_dir,
                               u_light_color * shadow, base,
                               roughness, metallic);

    // 环境项同样按金属度分裂:金属的环境反射带基色染色,介电质只取天光。
    vec3 environment = ambient * ao_factor * mix(vec3(1.0), base, metallic);

    vec3 lit = direct + environment * base
             + base * v_emissive * u_emissive_gain;

    // ---- 顶点烘焙接触 AO ----
    // 资产 schema 没有独立的 AO 通道,所以复用 `color` 里已经被
    // `mesh::expand_asset` 压暗过的部分:贴地面的顶点色天生更暗。
    // SSAO 负责「这一帧的」接触暗部,这一项负责「资产本身」的
    // 接触暗部(网格法线算不出来的部分,比如桌腿下的暗角)。
    // 两者相乘而不是相加 —— 叠两次会让墙角黑成一团。
    lit *= v_contact_ao;

    // ---- 湿地面 SSR ----
    // Fresnel:视线越平(掠射)反射越强,这正是湿路面反光的样子。
    vec3 view_dir = normalize(u_eye - v_world);
    float fresnel = pow(1.0 - max(dot(normal, view_dir), 0.0), 5.0);
    vec2 ssr_uv = gl_FragCoord.xy / vec2(textureSize(u_ssr_map, 0));
    vec3 reflected = texture(u_ssr_map, ssr_uv).rgb;
    // SSR 没命中时 reflected 是 0,此时只剩环境色(回退路径)。
    vec3 wet_tint = mix(u_ground_ambient, u_sky_ambient, 0.6);
    vec3 ssr_color = mix(wet_tint, reflected, step(0.001, reflected.r + reflected.g + reflected.b));
    // 逐片元湿度:路面(y=0)是湿的,人行道(y=0.14 以上)不湿 ——
    // 一个全局 u_wetness 会把整条街一起点亮,反而假。
    float surface = 1.0 - smoothstep(0.0, u_wet_height, v_world.y);
    // 掠射 + 朝上 = 水的镜面方向,再乘一点「只有平面才反光」。
    float flatness = clamp(normal.y, 0.0, 1.0);
    float wet_mix = u_wetness * u_ssr_strength * surface
                  * (0.06 + 0.94 * fresnel) * (0.25 + 0.75 * flatness);
    lit = mix(lit, ssr_color, clamp(wet_mix, 0.0, 1.0));

    // 大气雾:与 CPU 端 shade_face() 同一个 smoothstep 插值。
    float span = max(u_fog.y - u_fog.x, 1e-4);
    float fog = clamp((v_eye_distance - u_fog.x) / span, 0.0, 1.0);
    fog = fog * fog * (3.0 - 2.0 * fog);
    // 自发光面(霓虹 / 路灯)在雾里额外加一点光晕,夜里更醒目。
    float energy = dot(base * v_emissive, vec3(1.0));
    lit = tonemap(lit, u_exposure, u_tone_map_white) + u_sky_color * 0.012;
    if (energy > 0.01) {
        lit += u_sky_color * min(fog * 0.72, 0.72);
    }
    lit = mix(lit, u_sky_color, fog);

    out_color = vec4(linear_to_srgb(lit), 1.0);
    out_color = vec4(linear_to_srgb(lit), 1.0);
}
"#;

/// 泛光叠加用的片元着色器:把自发光通道单独画一遍,半透明加法混合,
/// 在霓虹 / 路灯 / 车灯周围制造一圈柔和光晕。
pub(crate) const GLOW_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;

in vec3 v_normal;
in vec3 v_color;
in vec3 v_emissive;
in vec3 v_tint;

uniform float u_glow_strength;

out vec4 out_color;

void main() {
    float energy = dot(v_emissive, vec3(0.3333));
    if (energy * u_glow_strength < 0.02) {
        discard;
    }
    out_color = vec4(v_emissive * energy * u_glow_strength, 1.0);
}
"#;
