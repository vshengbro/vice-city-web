//! 渲染后端:WebGL2(优先)/ Canvas2D 软件渲染(回退)。
//!
//! 两个后端共享 [`SceneLighting`] 与 [`shade_face`] 这套光照 / 配色参数,
//! 因此同一份场景在两条路径上视觉一致(除 GPU 特有的泛光后期之外)。
//!
//! 数据组织方式是 **instancing 友好的**:同类资产只解析一次,顶点数据只上传一份,
//! 每个实例只提供 model matrix + tint(见 [`SceneBatch`] / [`MeshAssetGpu`])。

use euv::{
    wasm_bindgen::JsValue,
    web_sys::{
        HtmlCanvasElement, WebGl2RenderingContext, WebGlBuffer, WebGlFramebuffer, WebGlProgram,
        WebGlShader, WebGlTexture, WebGlUniformLocation, WebGlVertexArrayObject,
    },
};

use crate::{
    camera::{Mat4, is_back_facing},
    mesh::{GpuMesh, f32_slice_to_bytes},
    r#const::*,
    r#type::{Mat4Data, Rgb8, Vec3},
};

/// 每个顶点的 f32 数量,与 [`crate::mesh::FLOATS_PER_VERTEX`] 一致:
/// `position(3) | normal(3) | color(3) | emissive(3)`。
///
/// 资产 JSON 里 part 有 `emissive`,展开时追加第 4 个 vec3,这样霓虹招牌
/// 可以在着色阶段直接按自发光强度叠加,不需要再查一次表。
pub const STRIDE_FLOATS: usize = 12;

/// 单个 instance 在 buffer 里的字节跨度,必须等于
/// `FLOATS_PER_INSTANCE * 4`,并作为 instanced 属性的 `vertex_attrib_pointer`
/// stride 使用。
pub const INSTANCE_STRIDE_BYTES: i32 = (FLOATS_PER_INSTANCE * 4) as i32;

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
pub const FLOATS_PER_INSTANCE: usize = 20;

/// instance buffer 的预分配实例数。
///
/// 必须 ≥ 单批次最大的实例数(场景里最多的是 12 棵行道树),并且要在
/// `upload_mesh` 建 VAO **之前** 分配好 —— 否则 VAO 捕获的是一个 0 字节的
/// buffer,`vertex_attrib_pointer` 记下的偏移在后续 `bufferData` 扩容后
/// 不会重新绑定,部分驱动上会渲染出未初始化内存。
const INSTANCE_PREALLOC: usize = 64;

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
pub const NEAR_CULL_RADIUS: f32 = 26.0;

/// 第三人称模式的近处遮挡剔除半径(米)。
///
/// 必须远小于 `NEAR_CULL_RADIUS`,因为第三人称眼点本来就**故意**贴着
/// 一切走:角色 7 m、路缘 8 m、对面楼 12 m。这个半径只负责剔掉「真的
/// 糊在镜头上」的东西(半径内 = 半米量级),遮挡由相机的球体探针负责
/// 回避 —— 两者是互补的,不是重复的。
pub const NEAR_CULL_RADIUS_FOLLOW: f32 = 0.35;

/// 实例中心到眼点的距离(取模型矩阵的平移列)。
///
/// # Arguments
///
/// - `&Instance` - Instance 的只读引用。
/// - `Vec3` - 输入值。
///
/// # Returns
///
/// - `f32` - 计算结果。
///   实例中心到眼点的距离(取模型矩阵的平移列)。
///
/// # Arguments
///
/// - `&Instance` - Instance 的只读引用。
/// - `Vec3` - 输入值。
///
/// # Returns
///
/// - `f32` - 计算结果。
fn instance_distance(instance: &Instance, eye: Vec3) -> f32 {
    let m: &Mat4Data = instance.get_model_ref();
    let dx: f32 = m[12] - eye[0];
    let dy: f32 = m[13] - eye[1];
    let dz: f32 = m[14] - eye[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// 阴影 pass 的保守剔除半径(米):一个实例只要中心落在
/// `SHADOW_HALF_EXTENT` 的包围盒内、或者到视锥轴的距离小于这个半径,
/// 就一定有一部分落在阴影 frustum 里。
///
/// 取 [`SHADOW_CULL_MARGIN`] 而不是 0:实例的平移列是**模型原点**,
/// 而一辆车 / 一栋楼的模型原点通常在几何体中间偏下,几何体本身可以
/// 伸出好几米。留 20 m 余量意味着「被剔除的实例一定离视锥 20 m 以外」,
/// 地面上不会出现影子凭空消失的暗斑。
///
/// # Returns
///
/// - `f32` - 剔除余量(米),取 [`SHADOW_CULL_MARGIN`]。
pub fn shadow_frustum_cull_radius() -> f32 {
    SHADOW_CULL_MARGIN
}

/// 判断实例是否**可能**落进阴影 frustum。
///
/// 阴影 frustum 是**正交**盒:以 `focus` 为中心、XZ 方向半宽
/// [`SHADOW_HALF_EXTENT`],沿光线方向从 `SHADOW_NEAR` 到 `SHADOW_FAR`。
///
/// 这里刻意**不**用「到 focus 的水平距离」那种球形判据:黄昏光
/// (`light_dir = [0.86, 0.24, -0.44]`)相当斜,高楼顶的影子会被拉出
/// 几十米远,一个只比视锥中心远一点、高却很高的楼,影子其实落在
/// frustum 边缘之外 —— 球形判据会把它留下(无害),但反过来若用
/// 纯水平判据且余量给小了,就会把「影子伸进 frustum」的实例误剔掉,
/// 地面上凭空少一块影子。
///
/// 所以这里按**光空间**投影来判:把实例中心沿光线方向投影到 shadow
/// frustum 的中心平面上,得到它在 XZ 上真正落点,再和半宽比。
/// 落点 = `center + light_dir_horizontal * ((focus.y - center.y) / light_dir.y)`。
/// 竖直光线(`|light_dir.y| < 1e-3`)时退化成「只看水平距离」,不会除零。
///
/// # Arguments
///
/// - `&Instance` - Instance 的只读引用。
/// - `Vec3` - 阴影 frustum 的中心(世界坐标)。
/// - `Vec3` - 指向光源的单位方向向量。
///
/// # Returns
///
/// - `bool` - 实例可能影响阴影贴图时为 `true`。
pub fn instance_affects_shadow(instance: &Instance, focus: Vec3, light_dir: Vec3) -> bool {
    let m: &Mat4Data = instance.get_model_ref();
    let center: Vec3 = [m[12], m[13], m[14]];
    // 判据要同时看**原点**和**原点往下挪一截**这两个落点。
    //
    // 只看原点会误剔:斜光下(`dusk` 光 y 分量仅 0.24)一栋 40 m 高的楼,
    // 原点在 y = 40 时影子落点距 focus 119 m(在视锥外),而它的**底部**
    // (y = 20)落点只有 61 m,仍在半宽 + 余量之内 —— 只测原点就会把
    // 「影子还伸进视锥」的高楼整栋剔掉,地面上凭空少一块长影子。
    //
    // 往下挪 [`SHADOW_CULL_MARGIN`] 是最保守的写法:它覆盖「模型原点
    // 不在几何体底部」的一切情况,代价只是视锥附近多留一圈实例。
    let reach: f32 = SHADOW_HALF_EXTENT + shadow_frustum_cull_radius();
    // 贴地光:光线没有竖直分量,影子不随高度平移,只看水平距离。
    if light_dir[1].abs() <= SHADOW_GROUNDED_EPS {
        let dx: f32 = center[0] - focus[0];
        let dz: f32 = center[2] - focus[2];
        return dx * dx + dz * dz <= reach * reach;
    }
    for probe in [center[1], center[1] - SHADOW_CULL_MARGIN] {
        let along: f32 = (focus[1] - probe) / light_dir[1];
        let dx: f32 = center[0] + light_dir[0] * along - focus[0];
        let dz: f32 = center[2] + light_dir[2] * along - focus[2];
        if dx * dx + dz * dz <= reach * reach {
            return true;
        }
    }
    false
}

/// 一个三角面在 CPU 侧的表示,软件渲染与 WebGL 共用。
#[derive(Clone, Copy, Debug)]
pub struct Face {
    /// 三个顶点下标(指向 [`GpuMesh::vertices`],单位是顶点而不是 f32)。
    pub indices: [u32; 3],
    /// 平面法线(已归一化,单位向量)。
    pub normal: Vec3,
    /// 基础色,线性空间 0..1。
    pub color: Vec3,
    /// 自发光色,线性空间 0..1。
    pub emissive: Vec3,
}

/// 一份资产在 GPU / CPU 两种后端下共用的展开结果。
///
/// 解析一次(见 [`crate::mesh::parse_asset`]),顶点数据只保留一份,
/// 供「按资产类型分批」的 instancing 渲染复用。
#[derive(Debug, Default)]
pub struct MeshAssetGpu {
    /// 展平后的顶点数据,`STRIDE_FLOATS` 个 f32 / 顶点。
    pub vertices: Vec<f32>,
    /// 三角形索引(每个三角形 3 个)。
    pub indices: Vec<u32>,
    /// 每个三角形的面信息(法线 / 颜色 / 自发光)。
    pub faces: Vec<Face>,
    /// 三角形数量。
    pub triangle_count: usize,
}

impl MeshAssetGpu {
    /// 三角形数量的只读副本。
    ///
    /// # Returns
    ///
    /// - `usize` - 三角形数量。
    pub fn get_triangle_count(&self) -> usize {
        self.triangle_count
    }
}

/// 把 [`crate::mesh::parse_asset`] 的结果转换成渲染后端用的布局。
///
/// `mesh` 是 mesh.rs 展开出的 de-index 网格(每个三角形 3 个独立顶点、
/// 法线取面法线、颜色取 `face_colors`),这里额外把每个三角形的
/// 自发光强度写进第 4 个 vec3。
///
/// `part_emissive` 是与 `mesh.triangle_count` 等长的自发光数组,
/// 由 game.rs 在解析 JSON 时按 part 顺序汇总。
/// 把 [`crate::mesh::parse_asset`] 的结果转换成渲染后端用的布局。
///
/// `mesh` 是 mesh.rs 展开出的 de-index 网格(每个三角形 3 个独立顶点、
/// 法线取面法线、颜色取 `face_colors`),这里额外把每个三角形的
/// 自发光强度写进第 4 个 vec3。
///
/// `part_emissive` 是与 `mesh.triangle_count` 等长的自发光数组,
/// 由 game.rs 在解析 JSON 时按 part 顺序汇总。
///
/// # Arguments
///
/// - `&GpuMesh` - GpuMesh 的只读引用。
/// - `&[Vec3]` - [Vec3] 的只读引用。
///
/// # Returns
///
/// - `MeshAssetGpu` - 计算结果。
pub fn build_gpu_mesh(mesh: &GpuMesh, part_emissive: &[Vec3]) -> MeshAssetGpu {
    let mut out: MeshAssetGpu = MeshAssetGpu {
        triangle_count: mesh.triangle_count,
        ..Default::default()
    };
    out.vertices.reserve(mesh.vertex_count() * STRIDE_FLOATS);
    let vertex_count: usize = mesh.vertex_count();
    for vertex in 0..vertex_count {
        let base: usize = vertex * 9;
        let position: [f32; 3] = [
            mesh.vertices[base],
            mesh.vertices[base + 1],
            mesh.vertices[base + 2],
        ];
        let normal: [f32; 3] = [
            mesh.vertices[base + 3],
            mesh.vertices[base + 4],
            mesh.vertices[base + 5],
        ];
        let color: [f32; 3] = [
            mesh.vertices[base + 6],
            mesh.vertices[base + 7],
            mesh.vertices[base + 8],
        ];
        // 三角形 = 第 `vertex / 3` 个三角形;顶点 0/1/2 属于同一个三角形。
        let triangle: usize = vertex / 3;
        let emissive: [f32; 3] = part_emissive.get(triangle).copied().unwrap_or([0.0; 3]);
        out.vertices.extend_from_slice(&[
            position[0],
            position[1],
            position[2], //
            normal[0],
            normal[1],
            normal[2], //
            color[0],
            color[1],
            color[2], //
            emissive[0],
            emissive[1],
            emissive[2],
        ]);
    }
    for triangle in 0..mesh.triangle_count {
        let base: usize = (triangle * 3) * 9;
        let normal: [f32; 3] = [
            mesh.vertices[base + 3],
            mesh.vertices[base + 4],
            mesh.vertices[base + 5],
        ];
        let color: [f32; 3] = [
            mesh.vertices[base + 6],
            mesh.vertices[base + 7],
            mesh.vertices[base + 8],
        ];
        let emissive: [f32; 3] = part_emissive.get(triangle).copied().unwrap_or([0.0; 3]);
        out.indices.extend_from_slice(&[
            (triangle * 3) as u32,
            (triangle * 3 + 1) as u32,
            (triangle * 3 + 2) as u32,
        ]);
        out.faces.push(Face {
            indices: [
                (triangle * 3) as u32,
                (triangle * 3 + 1) as u32,
                (triangle * 3 + 2) as u32,
            ],
            normal,
            color,
            emissive,
        });
    }
    out
}

/// 昼夜预设。三档:正午 / 黄昏 / 夜晚。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DayPhase {
    /// 正午:高角度强白光。
    Noon,
    /// 黄昏:低角度暖橙光。
    Dusk,
    /// 夜晚:冷月光 + 高霓虹强度。
    Night,
}

impl DayPhase {
    /// 相位名(HUD 与调试快照用)。
    ///
    /// # Returns
    ///
    /// - `&'static str` - 相位名。
    pub fn as_str(&self) -> &'static str {
        match self {
            DayPhase::Noon => PHASE_NOON,
            DayPhase::Dusk => PHASE_DUSK,
            DayPhase::Night => PHASE_NIGHT,
        }
    }

    /// 下一个相位(循环)。
    pub fn next(self) -> Self {
        match self {
            DayPhase::Noon => DayPhase::Dusk,
            DayPhase::Dusk => DayPhase::Night,
            DayPhase::Night => DayPhase::Noon,
        }
    }

    /// 归一化到 `[0, 1)` 的滑块位置。
    ///
    /// # Returns
    ///
    /// - `f64` - 计算结果。
    pub fn slider_value(self) -> f64 {
        match self {
            DayPhase::Noon => 0.0,
            DayPhase::Dusk => 0.5,
            DayPhase::Night => 1.0,
        }
    }

    /// 由滑块位置反推相位(四舍五入到最近的档位)。
    ///
    /// # Arguments
    ///
    /// - `f64` - 输入值。
    pub fn from_slider(value: f64) -> Self {
        let clamped: f64 = value.clamp(0.0, 1.0);
        let index: usize = (clamped * 2.0).round() as usize;
        match index {
            0 => DayPhase::Noon,
            1 => DayPhase::Dusk,
            _ => DayPhase::Night,
        }
    }

    /// 显示名(HTML overlay 用)。
    ///
    /// # Returns
    ///
    /// - `&'static str` - 计算结果。
    pub fn label(self) -> &'static str {
        match self {
            DayPhase::Noon => NOON,
            DayPhase::Dusk => DUSK,
            DayPhase::Night => NIGHT,
        }
    }
}

/// 两个后端共享的光照参数。
///
/// WebGL 把它作为 uniform 上传,Canvas2D 后端在 CPU 上跑同一个公式,
/// 保证两个后端的画面在色调整体上一致。
#[derive(Clone, Copy, Debug)]
pub struct SceneLighting {
    /// 太阳 / 月亮方向光方向(指向光源)。
    pub light_dir: Vec3,
    /// 方向光颜色 × 强度。
    pub light_color: Vec3,
    /// 环境光颜色 × 强度(半球权重为 0 时的回退色)。
    pub ambient: Vec3,
    /// 天空 / 雾颜色。
    pub sky_color: Vec3,
    /// 半球环境光的「天光」色(朝上的面接收)。
    pub sky_ambient: Vec3,
    /// 半球环境光的「地面反弹」色(朝下的面接收)。
    pub ground_ambient: Vec3,
    /// 半球环境光权重(0 = 纯单色 ambient,1 = 纯半球)。
    pub ambient_hemi: f32,
    /// 阴影强度(0 = 关掉阴影,1 = 完全)。
    pub shadow_strength: f32,
    /// SSAO 强度(0 = 关掉 AO,1 = 完全)。
    pub ao_strength: f32,
    /// SSR 强度(0 = 关掉屏幕空间反射,1 = 完全)。
    pub ssr_strength: f32,
    /// 路面湿度(0 = 干,1 = 湿)。与 `ssr_strength` 相乘,前者决定
    /// 「要不要反光」,后者决定「反多狠」。
    ///
    /// 湿路面是 Miami / Vice City 的招牌画面,所以正午也保留一点点
    /// 湿气(而不是 0)—— 只在掠射角真正给得到强 Fresnel 的地方才看得见。
    pub wetness: f32,
    /// 色调分级:抬黑场(加法,线性空间)。
    pub grade_lift: Vec3,
    /// 色调分级:中间调 gamma(1.0 = 不变)。
    pub grade_gamma: Vec3,
    /// 色调分级:亮场增益(1.0 = 不变)。
    pub grade_gain: Vec3,
    /// 暗角强度(0 = 无暗角)。
    pub vignette: f32,
    /// 胶片颗粒强度(0 = 无颗粒)。
    pub grain: f32,
    /// 自发光全局增益(夜晚更大)。
    pub emissive_gain: f32,
    /// 色调映射的曝光系数(线性域乘子)。
    pub exposure: f32,
    /// 色调映射的白色点(亮度超过这个值就开始明显压缩高光)。
    pub tone_map_white: f32,
    /// 大气雾:起始距离(米)。比这更近的物体完全不受雾影响。
    pub fog_start: f32,
    /// 大气雾:完全饱和的距离(米)。
    pub fog_end: f32,
}

impl SceneLighting {
    /// 取某个相位的默认光照参数。
    ///
    /// # Arguments
    ///
    /// - `DayPhase` - 输入值。
    pub fn for_phase(phase: DayPhase) -> Self {
        // 三相位的共同约束(数值都在 GPU / CPU 两端共用):
        //
        // 1. **朝光面总亮度可控。** 朝光面的线性亮度是
        //    `albedo * (ambient + light_color * n_dot_l)`。最亮的白色楼
        //    albedo 接近 0.86,如果 `ambient + light_color` 直接取到 2.0,
        //    线性值就是 1.7 —— 远超 1.0,`clamp` 之后所有浅色面一律糊成
        //    纯白,色相信息(粉 / 薄荷 / 珊瑚)在 sRGB 之前就已经被抹掉了。
        //    这里把三相位的 `ambient + light_color` 都压在 ~1.6 以内,
        //    剩下的高光交给 [`tonemap`] 的肩部压缩。
        // 2. **曝光固定为 1.0,靠白色点而不是靠压暗光强来「解决」过曝。**
        //    压暗方向光会让阴影面一起塌死;抬高白色点则保住中间调的
        //    色相,只把真正刺眼的顶端收进来。
        // 3. **夜晚不能死黑。** 环境光带一点蓝紫(城市天光 + 霓虹回弹),
        //    所以夜里路面和楼体仍然可辨,只是整体偏冷偏暗。
        match phase {
            DayPhase::Noon => Self {
                light_dir: normalize3([0.35, 0.86, 0.36]),
                // 方向光接近中性白(略偏暖),环境光偏天空蓝 —— 阴影面
                // 因此是冷蓝而不是灰,和阳光面的暖白拉开冷暖对比。
                light_color: [1.16, 1.10, 0.99],
                ambient: [0.34, 0.38, 0.46],
                sky_color: [0.44, 0.70, 0.92],
                // 半球:天光比 `ambient` 略亮偏冷,地面反弹是暖的沥青灰 ——
                // 沥青 albedo 只有 0.09,所以地面反弹**很暗**,这是对的。
                sky_ambient: [0.40, 0.46, 0.56],
                ground_ambient: [0.13, 0.12, 0.11],
                ambient_hemi: 1.0,
                // 正午是阴影最重的相位:高角度光让楼影落在街上,最显眼。
                shadow_strength: 1.0,
                ao_strength: 0.95,
                // 正午是掠射角最小的相位,湿路面反射几乎看不见 —— 强度压低,
                // 否则会在正午看到夜里才该有的反光。
                ssr_strength: 0.22,
                // 正午仍留一点湿气:柏油在雨后确实反光,而且它让路面
                // 远处的渐变不至于死板。权重低,只有掠射角才看得见。
                wetness: 0.35,
                // 冷亮:抬一点点蓝黑场,中间调轻微提蓝,亮场压一点暖。
                grade_lift: [0.002, 0.004, 0.010],
                grade_gamma: [0.99, 1.00, 1.02],
                grade_gain: [0.99, 1.00, 1.02],
                vignette: 0.26,
                grain: 0.012,
                emissive_gain: 0.18,
                exposure: 1.0,
                tone_map_white: TONE_MAP_WHITE_DAY,
                // 街区扩到 ±150 m,雾必须够远才看得到成片的街区;
                // fog_start 压到接近雾的起点,远景褪向天空色而不是突然消失。
                fog_start: 120.0,
                fog_end: 460.0,
            },
            DayPhase::Dusk => Self {
                light_dir: normalize3([0.86, 0.24, -0.44]),
                // 低角度暖橙直射 + 偏紫的天空环境光:朝光面是橙红,
                // 背光面落到冷紫,这是黄昏最主要的色相来源。
                light_color: [1.62, 0.84, 0.42],
                ambient: [0.26, 0.24, 0.40],
                sky_color: [0.95, 0.46, 0.36],
                sky_ambient: [0.34, 0.30, 0.48],
                ground_ambient: [0.20, 0.13, 0.11],
                ambient_hemi: 1.0,
                // 低角度光 = 长影子 = 黄昏的招牌画面,阴影权重甚至比正午更高。
                shadow_strength: 1.0,
                ao_strength: 0.85,
                // 掠射角大,Fresnel 强,湿路面在黄昏最出彩。
                ssr_strength: 0.85,
                wetness: 0.75,
                // 暖橙:黑场往洋红压,中间调提暖,亮场加金。
                grade_lift: [0.012, 0.004, 0.010],
                grade_gamma: [1.02, 0.99, 0.96],
                grade_gain: [1.04, 1.00, 0.95],
                vignette: 0.34,
                grain: 0.016,
                emissive_gain: 0.90,
                exposure: 1.0,
                tone_map_white: TONE_MAP_WHITE_DUSK,
                fog_start: 100.0,
                fog_end: 420.0,
            },
            DayPhase::Night => Self {
                light_dir: normalize3([-0.42, 0.72, -0.55]),
                // 月光很弱,但刻意保留蓝紫偏色;环境光是全画面「不死黑」
                // 的唯一来源 —— 路面 / 楼体靠它提亮,而不是把直射光调大。
                light_color: [0.24, 0.31, 0.58],
                ambient: [0.11, 0.14, 0.26],
                sky_color: [0.045, 0.055, 0.13],
                sky_ambient: [0.13, 0.17, 0.32],
                // 夜间地面反弹不是日光而是霓虹 —— 偏洋红,这是夜景的关键色偏。
                ground_ambient: [0.10, 0.06, 0.13],
                ambient_hemi: 1.0,
                // 月光很弱,阴影只是「比别处再暗一点点」,不是硬阴影。
                // 保留一点点(而不是 0)是为了让物体不失去体积感。
                shadow_strength: 0.34,
                ao_strength: 0.70,
                ssr_strength: 1.0,
                // 夜路最湿 —— 霓虹在积水上的倒影是夜景的全部意义。
                wetness: 1.0,
                // 冷蓝:黑场压蓝,中间调偏青,亮场压红。
                grade_lift: [0.004, 0.008, 0.024],
                grade_gamma: [1.03, 1.00, 0.95],
                grade_gain: [0.93, 0.98, 1.10],
                vignette: 0.46,
                grain: 0.022,
                emissive_gain: 1.85,
                exposure: 1.0,
                tone_map_white: TONE_MAP_WHITE_NIGHT,
                fog_start: 90.0,
                fog_end: 400.0,
            },
        }
    }
}

/// 线性亮度权重(Rec. 709 / sRGB 的 Y)。
const LUMA_WEIGHTS: Vec3 = [0.2126, 0.7152, 0.0722];

/// 构造一个正交投影矩阵(列主序,深度映射到 WebGL 的 `[-1, 1]`)。
///
/// 阴影贴图必须用正交投影而不是透视:平行光的「视锥」在几何上是
/// 一个无限棱柱,只有正交投影才能让光空间里的深度线性对应世界深度,
/// PCF 的比较才有意义。
///
/// # Arguments
///
/// - `f32` - 左裁剪面(世界坐标,沿右向量)。
/// - `f32` - 右裁剪面。
/// - `f32` - 下裁剪面(沿上向量)。
/// - `f32` - 上裁剪面。
/// - `f32` - 近裁剪面距离(正数)。
/// - `f32` - 远裁剪面距离(正数)。
///
/// # Returns
///
/// - `Mat4` - 列主序正交投影矩阵。
pub fn mat4_ortho(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Mat4 {
    let range: f32 = 1.0 / (near - far);
    let mut out: Mat4Data = [0.0; 16];
    out[0] = 2.0 / (right - left);
    out[5] = 2.0 / (top - bottom);
    out[10] = 2.0 * range;
    out[12] = -((right + left) * range);
    out[13] = -((top + bottom) * range);
    out[14] = (far + near) * range;
    out[15] = 1.0;
    Mat4::from_column_major(out)
}

/// 构造阴影贴图的光源视投影矩阵。
///
/// 视锥跟着**相机焦点**平移(而不是跟着眼点):阴影 frustum 只需要
/// 覆盖玩家周围 `SHADOW_HALF_EXTENT` 米,跟着眼点会让远处物体的影子
/// 落进 frustum 之外、边缘出现一条整齐的「影子截止线」。
///
/// 光源眼点沿 `light_dir` 后撤 [`SHADOW_LIGHT_DISTANCE`] 米,
/// 视锥的近平面因此落在城市最高楼之上,高楼顶不会被切掉。
///
/// **正午的光几乎垂直向上**,此时视线与 `up = (0, 1, 0)` 平行,
/// `look_at` 的叉积退化成 0 → 整张矩阵是 NaN。所以 `light_dir.y`
/// 超过阈值时改用 `up = (0, 0, 1)`。这正是「正午画面整块变黑 /
/// 阴影全丢」这类 bug 的经典来源。
///
/// # Arguments
///
/// - `Vec3` - 阴影 frustum 的中心(世界坐标)。
/// - `Vec3` - 指向光源的单位方向向量。
///
/// # Returns
///
/// - `Mat4` - 世界坐标 → 光空间裁剪坐标的矩阵。
pub fn shadow_view_projection(focus: Vec3, light_dir: Vec3) -> Mat4 {
    let eye: Vec3 = [
        focus[0] + light_dir[0] * SHADOW_LIGHT_DISTANCE,
        focus[1] + light_dir[1] * SHADOW_LIGHT_DISTANCE,
        focus[2] + light_dir[2] * SHADOW_LIGHT_DISTANCE,
    ];
    let up: Vec3 = if light_dir[1].abs() > SHADOW_DEGENERATE_UP_Y {
        [0.0, 0.0, 1.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let view: Mat4 = Mat4::look_at(eye, focus, up);
    let projection: Mat4 = mat4_ortho(
        -SHADOW_HALF_EXTENT,
        SHADOW_HALF_EXTENT,
        -SHADOW_HALF_EXTENT,
        SHADOW_HALF_EXTENT,
        SHADOW_NEAR,
        SHADOW_FAR,
    );
    projection.multiply(&view)
}

/// 一个阴影纹素覆盖的世界尺寸(米)。
///
/// # Returns
///
/// - `f32` - `2 * SHADOW_HALF_EXTENT / SHADOW_MAP_SIZE`。
pub fn shadow_texel_world_size() -> f32 {
    2.0 * SHADOW_HALF_EXTENT / SHADOW_MAP_SIZE as f32
}

/// 阴影深度偏置(以纹素为单位)。
///
/// # Returns
///
/// - `f32` - 深度偏置的纹素数。
pub fn shadow_depth_bias_texels() -> f32 {
    SHADOW_DEPTH_BIAS_TEXELS
}

/// 阴影法线偏移系数。
///
/// # Returns
///
/// - `f32` - 法线偏移的纹素数。
pub fn shadow_normal_offset_texels() -> f32 {
    SHADOW_NORMAL_OFFSET_TEXELS
}

/// 单个线性亮度的 Reinhard 扩展色调映射(有肩部的高光滚降)。
///
/// 标准 Reinhard 是 `L / (1 + L)`,它把 L=1 映射到 0.5 —— 也就是说
/// 一个「正常曝光」的白色面在 sRGB 编码后只有 ~188/255,整个画面永远
/// 偏暗发灰。扩展版(Jim Hejl / Richard Burgess-Dawson 的 W 参数化)
/// 满足 `f(W) = 1`,即**白色点恰好映射到 1.0**,中间调保持线性感,
/// 只有超过 `white` 的部分才进肩部压缩:
///
/// ```text
/// f(L) = L * (1 + L / W²) / (1 + L)
/// ```
///
/// 这正是「过曝」的正确解法:压的是高光顶端,不是把光强整体调暗。
///
/// # Arguments
///
/// - `f32` - 线性亮度(非负)。
/// - `f32` - 曝光系数(线性域乘子)。
/// - `f32` - 白色点(必须 > 0)。
///
/// # Returns
///
/// - `f32` - 映射后的线性亮度,落在 `[0, 1)`。
pub fn tonemap_luma(luma: f32, exposure: f32, white: f32) -> f32 {
    let w: f32 = if white > f32::EPSILON { white } else { 1.0 };
    let x: f32 = if luma > 0.0 { luma } else { 0.0 } * exposure;
    let w2: f32 = w * w;
    x * (1.0 + x / w2) / (1.0 + x)
}

/// 按亮度做色相保持(color-preserving)的色调映射。
///
/// 逐通道独立映射会把过曝的亮面直接去色 —— 一个亮度 1.7 的粉红面
/// 逐通道 clamp 后是 (1.0, 1.0, 1.0),纯白;而同一个面如果按亮度整体
/// 缩放,R:G:B 的比例被保留,仍然读得出是粉红。所以这里只对**亮度**
/// 做色调映射,再按 `f(L') / (L * exposure)` 的比例把颜色整体缩放回去,
/// 即保留色相与饱和度关系,只压亮度。
///
/// 纯黑(`luma <= 0`)直接原样返回,避免除零。
///
/// # Arguments
///
/// - `Vec3` - 线性 RGB(未做曝光与色调映射)。
/// - `f32` - 曝光系数(线性域乘子)。
/// - `f32` - 白色点(必须 > 0)。
///
/// # Returns
///
/// - `Vec3` - 色调映射后的线性 RGB。
pub fn tonemap(value: Vec3, exposure: f32, white: f32) -> Vec3 {
    let luma: f32 =
        value[0] * LUMA_WEIGHTS[0] + value[1] * LUMA_WEIGHTS[1] + value[2] * LUMA_WEIGHTS[2];
    if luma <= 1e-6 {
        return [0.0, 0.0, 0.0];
    }
    let mapped: f32 = tonemap_luma(luma, exposure, white);
    let divisor: f32 = luma * exposure;
    let scale: f32 = if divisor > f32::EPSILON {
        mapped / divisor
    } else {
        1.0
    };
    [value[0] * scale, value[1] * scale, value[2] * scale]
}

/// 两个后端共用的平面着色公式。
///
/// ```text
/// base    = albedo * tint
/// hemi    = mix(ground_ambient, sky_ambient, n.y * 0.5 + 0.5)
/// ambient = mix(ambient, hemi, ambient_hemi)
/// lit     = base * (ambient + light_color * max(dot(normal, light_dir), 0))
///         + base * emissive * emissive_gain
/// color   = tonemap(lit, exposure, tone_map_white)   // 线性域,色相保持
///         + sky_color * SKY_TINT_GAIN                  // 天空色晕染
/// color   = mix(color, sky_color, fog)                // 大气雾
/// out     = linear_to_srgb(color)
/// ```
///
/// **色相为什么必须保住(这正是「一片惨白」的历史根因):**
/// 资产颜色按 `assets/SCHEMA.md` 是**线性** albedo,浅色面的亮度
/// 可以接近 0.86。经过 `ambient + light_color * n_dot_l` 之后,朝光面
/// 的线性亮度会到 1.5~1.9 —— 远超 1.0。如果这里逐通道 `clamp` 或逐通道
/// 映射,(1.5, 1.2, 1.4) 会被压成 (1, 1, 1),粉红面和薄荷面全部变成
/// 纯白,「有颜色」在 sRGB 编码之前就已经丢失了。色相保持的映射把
/// 亮度压到 1.0 以下,R:G:B 的比例不变,浅粉仍然读得出是浅粉。
///
/// **半球环境光**在两个后端跑的是同一条公式(`hemi_weight` 与
/// `ambient_hemi` 的插值),所以 Canvas2D 回退不会因为缺半球而「变平」。
///
/// `eye_distance` 是面中心到相机眼点的距离(米),用于大气雾。
///
/// # Arguments
///
/// - `Vec3` - 输入值。
/// - `Vec3` - 输入值。
/// - `Vec3` - 输入值。
/// - `Vec3` - 输入值。
/// - `&SceneLighting` - SceneLighting 的只读引用。
/// - `f32` - 输入值。
///
/// # Returns
///
/// - `[f32` - 计算结果。
pub fn shade_face(
    albedo: Vec3,
    emissive: Vec3,
    normal: Vec3,
    tint: Vec3,
    lighting: &SceneLighting,
    eye_distance: f32,
) -> [f32; 3] {
    let n_dot_l: f32 = (normal[0] * lighting.light_dir[0]
        + normal[1] * lighting.light_dir[1]
        + normal[2] * lighting.light_dir[2])
        .max(0.0);
    // 半球环境光:与 GLSL 端 `mix(ground, sky, n.y * 0.5 + 0.5)` 逐项对应。
    let hemi_weight: f32 = normal[1] * 0.5 + 0.5;
    let base: [f32; 3] = [
        albedo[0] * tint[0],
        albedo[1] * tint[1],
        albedo[2] * tint[2],
    ];
    let mut out: [f32; 3] = [0.0; 3];
    for channel in 0..3 {
        let hemi: f32 = lighting.ground_ambient[channel]
            + (lighting.sky_ambient[channel] - lighting.ground_ambient[channel]) * hemi_weight;
        let ambient: f32 =
            lighting.ambient[channel] + (hemi - lighting.ambient[channel]) * lighting.ambient_hemi;
        let diffuse: f32 = ambient + lighting.light_color[channel] * n_dot_l;
        out[channel] =
            base[channel] * diffuse + base[channel] * emissive[channel] * lighting.emissive_gain;
    }
    // 色调映射(色相保持)+ 天空色晕染。顺序很重要:先在线性域压亮度,
    // 再加天空色,最后才是 sRGB 编码。
    let mut mapped: [f32; 3] = tonemap(out, lighting.exposure, lighting.tone_map_white);
    for channel in 0..3 {
        mapped[channel] += lighting.sky_color[channel] * SKY_TINT_GAIN;
    }
    // 大气雾:线性空间里向天空色插值,自发光通道不吃雾(夜里霓虹要穿透雾)。
    let span: f32 = (lighting.fog_end - lighting.fog_start).max(f32::EPSILON);
    let fog: f32 = ((eye_distance - lighting.fog_start) / span).clamp(0.0, 1.0);
    let fog: f32 = fog * fog * (3.0 - 2.0 * fog);
    let self_lit: f32 = base[0] * emissive[0] + base[1] * emissive[1] + base[2] * emissive[2];
    if self_lit > 0.01 {
        let glow: f32 = (fog * 0.72).min(0.72);
        for (channel, sky) in mapped.iter_mut().zip(lighting.sky_color.iter()) {
            *channel += sky * glow;
        }
    }
    for (channel, sky) in mapped.iter_mut().zip(lighting.sky_color.iter()) {
        *channel = *channel * (1.0 - fog) + sky * fog;
    }
    linear_to_srgb(mapped)
}

/// 线性 → sRGB 传输函数(与 `assets/SCHEMA.md` §2「linear RGB, apply the
/// usual linear→sRGB transfer at display time」一致)。
/// 线性 → sRGB 传输函数(与 `assets/SCHEMA.md` §2「linear RGB, apply the
/// usual linear→sRGB transfer at display time」一致)。
///
/// # Arguments
///
/// - `Vec3` - 输入值。
///
/// # Returns
///
/// - `Vec3` - 计算结果。
pub fn linear_to_srgb(value: Vec3) -> Vec3 {
    let mut out: [f32; 3] = [0.0; 3];
    for channel in 0..3 {
        let c: f32 = value[channel].clamp(0.0, 1.0);
        out[channel] = if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
    }
    out
}

/// 把归一化的 sRGB 分量量化成 `0..255`。
///
/// # Arguments
///
/// - `Vec3` - 输入值。
///
/// # Returns
///
/// - `Rgb8` - 计算结果。
pub fn srgb_to_u8(value: Vec3) -> Rgb8 {
    [
        (value[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (value[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (value[2].clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

/// 归一化向量(零向量原样返回)。
///
/// # Arguments
///
/// - `Vec3` - 输入值。
///
/// # Returns
///
/// - `Vec3` - 计算结果。
pub fn normalize3(value: Vec3) -> Vec3 {
    let len: f32 = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    if len <= f32::EPSILON {
        [0.0, 1.0, 0.0]
    } else {
        [value[0] / len, value[1] / len, value[2] / len]
    }
}

// ===========================================================================
// 场景批次(instancing 友好的组织方式)
// ===========================================================================

/// 一个实例:model matrix(列主序 16 f32)+ tint。
#[derive(Clone, Copy, Debug)]
pub struct Instance {
    /// model matrix,列主序,直接喂 `uniformMatrix4fv` / 自己算。
    pub(crate) model: Mat4Data,
    /// 逐实例色调乘子。
    pub(crate) tint: Vec3,
}

impl Instance {
    /// model matrix 的只读引用。
    ///
    /// # Returns
    ///
    /// - `&Mat4Data` - 列主序 model matrix。
    pub fn get_model_ref(&self) -> &Mat4Data {
        &self.model
    }

    /// 用**显式 model matrix** 构造实例。
    ///
    /// 车轮要同时做两件事:跟着车身绕 Y 转,再绕自己的轮心自转。
    /// [`Self::new`] 只能表达单轴 Y 旋转,表达不了这个复合变换,所以这里
    /// 直接接受调用方算好的列主序矩阵。
    ///
    /// # Arguments
    ///
    /// - `Mat4Data` - 列主序 model matrix(长度 16)。
    /// - `Vec3` - 逐实例色调乘子。
    ///
    /// # Returns
    ///
    /// - `Self` - 构造好的实例。
    pub fn from_model(model: Mat4Data, tint: Vec3) -> Self {
        Self { model, tint }
    }

    /// 构造一个实例。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 世界坐标位置。
    /// - `f32` - 绕 Y 轴朝向(弧度)。
    /// - `f32` - 均匀缩放系数。
    /// - `Vec3` - 逐实例色调乘子。
    ///
    /// # Returns
    ///
    /// - `Self` - 构造好的实例。
    pub fn new(position: Vec3, yaw: f32, uniform_scale: f32, tint: Vec3) -> Self {
        let (sin_yaw, cos_yaw): (f32, f32) = yaw.sin_cos();
        // 绕 Y 轴旋转 + 非均匀缩放(xz 可拉伸,用于拉长车身 / 招牌)。
        let (scale_x, scale_y, scale_z): (f32, f32, f32) =
            (uniform_scale, uniform_scale, uniform_scale);
        Self {
            model: [
                cos_yaw * scale_x,
                0.0,
                -sin_yaw * scale_x,
                0.0, //
                0.0,
                scale_y,
                0.0,
                0.0, //
                sin_yaw * scale_z,
                0.0,
                cos_yaw * scale_z,
                0.0, //
                position[0],
                position[1],
                position[2],
                1.0,
            ],
            tint,
        }
    }

    /// 用一条显式的 model matrix 构造实例。
    ///
    /// 玩家的步态骨架需要「局部肢体摆动矩阵 × 整体位置 / 朝向」,不再是
    /// 单纯的 TRS,所以给渲染器开一个直接吃矩阵的入口。
    ///
    /// # Arguments
    ///
    /// - `Mat4` - 列主序的 model matrix。
    /// - `Vec3` - 逐实例色调乘子。
    ///
    /// # Returns
    ///
    /// - `Self` - 构造好的实例。
    pub fn from_matrix(model: Mat4, tint: Vec3) -> Self {
        Self {
            model: *model.get_elements(),
            tint,
        }
    }

    /// 用 model matrix 变换一个点。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 输入值。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 计算结果。
    pub fn transform_point(&self, point: Vec3) -> Vec3 {
        let m: &Mat4Data = self.get_model_ref();
        let x: f32 = m[0] * point[0] + m[4] * point[1] + m[8] * point[2] + m[12];
        let y: f32 = m[1] * point[0] + m[5] * point[1] + m[9] * point[2] + m[13];
        let z: f32 = m[2] * point[0] + m[6] * point[1] + m[10] * point[2] + m[14];
        [x, y, z]
    }

    /// 法线变换:因为只用「绕 Y 旋转 + 各向同性均匀缩放」,法线的旋转部分
    /// 与顶点相同,缩放对单位法线无影响,因此直接复用同一线性变换即可。
    /// 法线变换:因为只用「绕 Y 旋转 + 各向同性均匀缩放」,法线的旋转部分
    /// 与顶点相同,缩放对单位法线无影响,因此直接复用同一线性变换即可。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 输入值。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 计算结果。
    pub fn transform_normal(&self, normal: Vec3) -> Vec3 {
        normalize3(self.transform_point(normal))
    }
}

/// 同一资产的一批实例 —— 这正是 instancing 的粒度:
/// 顶点数据只上传一份,每帧只更新 `[Instance; count]` 组成的 instance buffer。
#[derive(Debug, Default)]
pub struct SceneBatch {
    /// 对应的资产索引(指向 `Scene::meshes`)。
    pub mesh_index: usize,
    /// 实例列表。
    pub instances: Vec<Instance>,
    /// 是否参与深度测试 / 背面剔除(地面与透明片为 false)。
    pub opaque: bool,
    /// 是否参与「近处遮挡剔除」(见 [`NEAR_CULL_RADIUS`])。
    ///
    /// 第三人称模式下相机离角色只有 4–7 m,而近处剔除半径是 26 m ——
    /// 如果不豁免,玩家身上的每一块 part 都会被剔掉,画面里根本没有角色。
    /// 玩家骨架批次因此固定为 `false`,车辆批次也关掉(车灯要在近处
    /// 看清楚)。地面 / 建筑 / 树保持 `true`。
    pub near_cull: bool,
}

/// 整个场景:资产表 + 批次表。
///
/// 资产只解析一次(`meshes`),实例只传矩阵(`batches`),
/// 因此「12 栋楼 + 20 棵棕榈 + 40 个道具」不会产生 72 份顶点数据。
#[derive(Debug, Default)]
pub struct Scene {
    /// 资产表,每个资产解析一次。
    pub meshes: Vec<MeshAssetGpu>,
    /// 批次表,每个批次 = 一次 instanced draw call。
    pub batches: Vec<SceneBatch>,
    /// 三角形总数(统计用)。
    pub total_triangles: usize,
}

impl Scene {
    /// 追加一个已解析的资产,返回其索引。
    ///
    /// # Arguments
    ///
    /// - `MeshAssetGpu` - 输入值。
    ///
    /// # Returns
    ///
    /// - `usize` - 计数结果。
    pub fn push_mesh(&mut self, mesh: MeshAssetGpu) -> usize {
        self.set_total_triangles(self.get_total_triangles() + mesh.get_triangle_count());
        let index: usize = self.get_meshes_mut().len();
        self.get_meshes_mut().push(mesh);
        index
    }

    /// 三角形总数的只读副本。
    ///
    /// # Returns
    ///
    /// - `usize` - 场景累计的三角形数。
    pub fn get_total_triangles(&self) -> usize {
        self.total_triangles
    }

    /// 设置三角形总数。
    ///
    /// # Arguments
    ///
    /// - `usize` - 新的三角形总数。
    pub fn set_total_triangles(&mut self, value: usize) {
        self.total_triangles = value;
    }

    /// 资产表的可变引用(仅供同模块内的 accessor 使用)。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<MeshAssetGpu>` - 资产表。
    pub fn get_meshes_mut(&mut self) -> &mut Vec<MeshAssetGpu> {
        &mut self.meshes
    }

    /// 新建一个批次并立即返回其索引。
    ///
    /// # Arguments
    ///
    /// - `usize` - 输入值。
    /// - `bool` - 输入值。
    ///
    /// # Returns
    ///
    /// - `usize` - 计数结果。
    pub fn push_batch(&mut self, mesh_index: usize, opaque: bool) -> usize {
        self.push_batch_with_cull(mesh_index, opaque, true)
    }

    /// 新建一个批次,并显式指定它是否参与近处遮挡剔除。
    ///
    /// # Arguments
    ///
    /// - `usize` - 资产索引。
    /// - `bool` - 是否参与深度测试 / 背面剔除。
    /// - `bool` - 是否参与近处遮挡剔除。
    ///
    /// # Returns
    ///
    /// - `usize` - 新批次的索引。
    pub fn push_batch_with_cull(
        &mut self,
        mesh_index: usize,
        opaque: bool,
        near_cull: bool,
    ) -> usize {
        let index: usize = self.get_batches_mut().len();
        self.get_batches_mut().push(SceneBatch {
            mesh_index,
            instances: Vec::new(),
            opaque,
            near_cull,
        });
        index
    }

    /// 批次表的可变引用(仅供同模块内的 accessor 使用)。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<SceneBatch>` - 批次表。
    pub fn get_batches_mut(&mut self) -> &mut Vec<SceneBatch> {
        &mut self.batches
    }

    /// 批次表的只读引用。
    ///
    /// # Returns
    ///
    /// - `&[SceneBatch]` - 批次表。
    pub fn get_batches(&self) -> &[SceneBatch] {
        &self.batches
    }

    /// 向批次追加一个实例。
    ///
    /// # Arguments
    ///
    /// - `usize` - 输入值。
    /// - `Instance` - 输入值。
    pub fn push_instance(&mut self, batch_index: usize, instance: Instance) {
        self.get_batches_mut()[batch_index].instances.push(instance);
    }

    /// 三角形总数减去被背面剔除的(粗估,用于 HUD)。
    ///
    /// # Returns
    ///
    /// - `usize` - 计数结果。
    pub fn instance_count(&self) -> usize {
        self.get_batches()
            .iter()
            .map(|batch: &SceneBatch| batch.instances.len())
            .sum()
    }
}

// ===========================================================================
// GLSL ES 3.00
// ===========================================================================

/// 顶点着色器:model matrix × u_view_proj,透传法线 / 颜色 / 自发光。
const VERTEX_SHADER: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_position;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec3 a_color;
layout(location = 3) in vec3 a_emissive;

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
// 顶点烘焙的接触 AO:1.0 = 不压暗,< 1.0 = 贴近地面的顶点被压暗。
//
// 在顶点着色器里算而不是多占一个交错通道:顶点格式已经是
// pos(3)|normal(3)|color(3)|emissive(3) = 12 floats,加第 13 个
// 要同步改 mesh.rs 的 FLOATS_PER_VERTEX / stride / 上传路径。
out float v_contact_ao;

uniform vec3 u_eye;
// 接触 AO 的高度衰减:从 y=0 处的最深压暗线性过渡到 y=u_ao_height。
uniform float u_ao_height;
// 接触 AO 的最深压暗系数(1.0 = 不压暗)。
uniform float u_ao_floor;

void main() {
    mat4 model = mat4(i_row0, i_row1, i_row2, i_row3);
    vec4 world = model * vec4(a_position, 1.0);
    v_normal = mat3(model) * a_normal;
    v_color = a_color;
    v_emissive = a_emissive;
    v_tint = i_tint;
    v_world = world.xyz;
    v_eye_distance = distance(world.xyz, u_eye);
    // 贴地越近压得越暗,到 u_ao_height 之上完全不压。
    // 0.001 的下限保证除法不炸,同时也让 y<0 的面(不该有,但资产
    // 里可能有)走到最深压暗而不是 NaN。
    v_contact_ao = mix(
        u_ao_floor,
        1.0,
        clamp(world.y / max(u_ao_height, 0.001), 0.0, 1.0)
    );
    gl_Position = u_view_proj * world;
}
"#;

/// 深度预渲染(阴影贴图)用的顶点着色器。
///
/// 复用主顶点着色器的那套 instance 布局,只是把 `u_view_proj` 换成
/// 光源视投影矩阵 —— 所以阴影 pass 与主 pass 吃的是**同一份 VAO 与
/// instance buffer**,不需要第二套几何上传路径。
const SHADOW_VERTEX_SHADER: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_position;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec3 a_color;
layout(location = 3) in vec3 a_emissive;

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
const SHADOW_FRAGMENT_SHADER: &str = r#"#version 300 es
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
const FULLSCREEN_VERTEX_SHADER: &str = r#"#version 300 es
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
const GBUFFER_FRAGMENT_SHADER: &str = r#"#version 300 es
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
const SSAO_FRAGMENT_SHADER: &str = r#"#version 300 es
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

const int KERNEL = 8;

// 黄金角螺旋 —— 8 个方向均匀铺满半球面,比正方形网格的簇拥程度低。
vec3 kernel_direction(int index) {
    float fi = float(index) + 0.5;
    float phi = fi * 2.39996323;
    float cos_theta = sqrt(1.0 - fi / float(KERNEL));
    float sin_theta = sqrt(fi / float(KERNEL));
    return vec3(cos(phi) * sin_theta, sin(phi) * sin_theta, cos_theta);
}

// 从线性深度 + uvscreen 反推视空间坐标(与 SSAO 采样端共用)。
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
    vec3 origin = view_position(v_uv, depth);

    // 用一个固定的世界尺度把屏幕空间偏移换算成视空间偏移。
    float radius = u_radius / max(depth * u_far_plane, 0.5);
    float occlusion = 0.0;
    for (int i = 0; i < KERNEL; ++i) {
        vec3 dir = kernel_direction(i);
        // 只取面向观察者的一半(SSAO 的经典构造:背面的样本贡献很小)。
        vec3 sample_view = origin + dir * radius;
        vec2 sample_uv = sample_view.xy / max(-sample_view.z, 1e-3);
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
            // 越贴近表面、差值越大,遮蔽越强。
            float falloff = 1.0 - clamp(diff / max(radius / u_far_plane, 1e-4), 0.0, 1.0);
            occlusion += falloff * falloff;
        }
    }
    float ao = 1.0 - clamp(occlusion / float(KERNEL), 0.0, 1.0);
    ao = pow(ao, u_power);
    out_color = vec4(ao, ao, ao, 1.0);
}
"#;

/// AO 的双边模糊片元着色器。
///
/// 之所以要双边(bilateral)而不是普通高斯:普通模糊会把「暗的墙角」
/// 糊到「亮的天空」上,画面上出现一条假的灰边。双边模糊只在内部分
/// 深度差小的邻域里加权,把模糊限制在深度不连续处的同一侧。
const AO_BLUR_FRAGMENT_SHADER: &str = r#"#version 300 es
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
const SSR_FRAGMENT_SHADER: &str = r#"#version 300 es
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
const BLOOM_EXTRACT_FRAGMENT_SHADER: &str = r#"#version 300 es
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
const BLOOM_BLUR_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;

in vec2 v_uv;

uniform sampler2D u_scene_color;
uniform vec2 u_direction;

out vec4 out_color;

const float WEIGHTS[5] = float[5](0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216);

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
const COMPOSITE_FRAGMENT_SHADER: &str = r#"#version 300 es
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
const WET_OVERLAY_FRAGMENT_SHADER: &str = r#"#version 300 es
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
const FRAGMENT_SHADER: &str = r#"#version 300 es
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

const vec3 LUMA_WEIGHTS = vec3(0.2126, 0.7152, 0.0722);

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

    vec3 lit = base * (ambient * ao_factor + u_light_color * n_dot_l * shadow)
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
const GLOW_FRAGMENT_SHADER: &str = r#"#version 300 es
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

// ===========================================================================
// 自适应画质
// ===========================================================================

/// 渲染画质档位。
///
/// 降级是**单向**的:只往下走,不自动升回去。否则玩家开着车穿过一片
/// 楼群(掉到 Low)又开回空旷海面,画质会来回抖,比一直低更难受。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualityTier {
    /// 全开:阴影 + SSAO + SSR + bloom。
    High,
    /// 关掉 SSR(最贵的一项),保留阴影 / SSAO / bloom。
    Medium,
    /// 只留阴影 + bloom —— SSR 与 SSAO 全关。
    Low,
}

impl QualityTier {
    /// 该档位下是否跑 SSR。
    ///
    /// # Returns
    ///
    /// - `bool` - `true` 表示开启。
    pub fn wants_ssr(&self) -> bool {
        *self == QualityTier::High
    }

    /// 该档位下是否跑 SSAO。
    ///
    /// # Returns
    ///
    /// - `bool` - `true` 表示开启。
    pub fn wants_ssao(&self) -> bool {
        *self != QualityTier::Low
    }
}

/// 按实测帧率自动降档的状态机。
///
/// 目标很简单:**宁可掉画质,不要掉帧率**。这在软渲染(SwiftShader)上尤其
/// 重要 —— 基准线已经是 25 fps,再叠上海面 / 草地 / 山体,不动的话会掉到
/// 个位数,截图能截但画面已经没法看了。
#[derive(Clone, Copy, Debug)]
pub struct AdaptiveQuality {
    tier: QualityTier,
    smoothed_fps: f32,
    slow_frames: u32,
    fast_frames: u32,
    samples: u32,
}

impl AdaptiveQuality {
    /// 初始为 [`QualityTier::High`],尚未采样。
    ///
    /// # Returns
    ///
    /// - `Self` - 全新状态机。
    pub fn new() -> Self {
        Self {
            tier: QualityTier::High,
            smoothed_fps: 0.0,
            slow_frames: 0,
            fast_frames: 0,
            samples: 0,
        }
    }

    /// 当前的画质档位。
    ///
    /// # Returns
    ///
    /// - `QualityTier` - 档位。
    pub fn tier(&self) -> QualityTier {
        self.get_tier()
    }

    /// 读 `tier` 字段。
    ///
    /// # Returns
    ///
    /// - `QualityTier` - 档位。
    pub fn get_tier(&self) -> QualityTier {
        self.tier
    }

    /// 写 `tier` 字段。
    ///
    /// # Arguments
    ///
    /// - `QualityTier` - 新档位。
    pub fn set_tier(&mut self, value: QualityTier) {
        self.tier = value;
    }

    /// 读 `smoothed_fps` 字段(指数平滑后的实测帧率)。
    ///
    /// # Returns
    ///
    /// - `f32` - 平滑帧率。
    pub fn get_smoothed_fps(&self) -> f32 {
        self.smoothed_fps
    }

    /// 写 `smoothed_fps` 字段。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新平滑帧率。
    pub fn set_smoothed_fps(&mut self, value: f32) {
        self.smoothed_fps = value;
    }

    /// 读 `slow_frames` 字段(连续慢帧计数)。
    ///
    /// # Returns
    ///
    /// - `u32` - 慢帧计数。
    pub fn get_slow_frames(&self) -> u32 {
        self.slow_frames
    }

    /// 写 `slow_frames` 字段。
    ///
    /// # Arguments
    ///
    /// - `u32` - 新慢帧计数。
    pub fn set_slow_frames(&mut self, value: u32) {
        self.slow_frames = value;
    }

    /// 读 `fast_frames` 字段(连续快帧计数)。
    ///
    /// # Returns
    ///
    /// - `u32` - 快帧计数。
    pub fn get_fast_frames(&self) -> u32 {
        self.fast_frames
    }

    /// 写 `fast_frames` 字段。
    ///
    /// # Arguments
    ///
    /// - `u32` - 新快帧计数。
    pub fn set_fast_frames(&mut self, value: u32) {
        self.fast_frames = value;
    }

    /// 读 `samples` 字段(已采样帧数)。
    ///
    /// # Returns
    ///
    /// - `u32` - 采样帧数。
    pub fn get_samples(&self) -> u32 {
        self.samples
    }

    /// 写 `samples` 字段。
    ///
    /// # Arguments
    ///
    /// - `u32` - 新采样帧数。
    pub fn set_samples(&mut self, value: u32) {
        self.samples = value;
    }

    /// 喂入一帧的实测帧率,必要时降档。
    ///
    /// **要连续若干帧都慢才降档。** 单帧的尖峰(资产加载、GC、标签页
    /// 切回)不应该触发降级,所以用 `SLOW_FRAME_THRESHOLD` 帧的滑动
    /// 计数;帧率用指数平滑,单帧抖动不会让档位来回跳。
    ///
    /// # Arguments
    ///
    /// - `f32` - 本帧的帧率(fps)。小于等于 0 的值会被忽略(分母为零)。
    pub fn sample(&mut self, fps: f32) {
        if fps <= 0.0 || !fps.is_finite() {
            return;
        }
        if self.get_samples() == 0 {
            self.set_smoothed_fps(fps);
        } else {
            self.set_smoothed_fps(
                self.get_smoothed_fps() * FPS_SMOOTHING + fps * (1.0 - FPS_SMOOTHING),
            );
        }
        self.set_samples(self.get_samples() + 1);
        // 前几秒不判 —— 管线刚建立起来的第一帧总是最慢的。
        if self.get_samples() < QUALITY_WARMUP_FRAMES {
            return;
        }
        if self.get_smoothed_fps() < QUALITY_DOWN_FPS {
            self.set_slow_frames(self.get_slow_frames() + 1);
            self.set_fast_frames(0);
        } else if self.get_smoothed_fps() > QUALITY_UP_FPS {
            self.set_fast_frames(self.get_fast_frames() + 1);
            self.set_slow_frames(0);
        } else {
            self.set_slow_frames(0);
            self.set_fast_frames(0);
        }
        if self.get_slow_frames() >= SLOW_FRAME_THRESHOLD {
            self.set_slow_frames(0);
            self.set_tier(match self.get_tier() {
                QualityTier::High => QualityTier::Medium,
                QualityTier::Medium => QualityTier::Low,
                QualityTier::Low => QualityTier::Low,
            });
        }
        // 升档只在明确很快、且已经稳定很久时发生,避免抖动。
        if self.get_fast_frames() >= FAST_FRAME_THRESHOLD && self.get_tier() != QualityTier::High {
            // 单向降级:这里刻意**不**执行升档。
            self.set_fast_frames(0);
        }
    }
}

// ===========================================================================
// 离屏渲染目标
// ===========================================================================

/// 一个离屏渲染目标(FBO + 它的颜色 / 深度纹理)。
///
/// 深度用**纹理**而不是 renderbuffer:阴影贴图与 G-buffer 的深度都要
/// 在后续 pass 里被采样,renderbuffer 不可采样。
#[derive(Debug)]
struct RenderTarget {
    /// framebuffer 句柄。
    fbo: WebGlFramebuffer,
    /// 颜色纹理句柄(纯深度目标为 `None`)。
    color: Option<WebGlTexture>,
    /// 深度纹理句柄。
    depth: WebGlTexture,
    /// 颜色缓冲的像素宽度。
    width: i32,
    /// 颜色缓冲的像素高度。
    height: i32,
}

impl RenderTarget {
    /// 分配一张颜色纹理(RGBA8,线性过滤,边缘钳制)。
    ///
    /// 颜色附件的过滤**必须**是 `LINEAR`:bloom 的模糊与 SSAO 的上采样
    /// 都依赖硬件双线性,`NEAREST` 会让半分辨率的 pass 出现方块。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `i32` - 纹理宽度(像素)。
    /// - `i32` - 纹理高度(像素)。
    ///
    /// # Returns
    ///
    /// - `Result<WebGlTexture, String>` - 分配好的纹理。
    fn create_color_texture(
        context: &WebGl2RenderingContext,
        width: i32,
        height: i32,
    ) -> Result<WebGlTexture, String> {
        let texture: WebGlTexture = context
            .create_texture()
            .ok_or_else(|| CREATE_TEXTURE_FAILED.to_string())?;
        context.bind_texture(WebGl2RenderingContext::TEXTURE_2D, Some(&texture));
        context
            .tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
                WebGl2RenderingContext::TEXTURE_2D,
                0,
                WebGl2RenderingContext::RGBA8 as i32,
                width,
                height,
                0,
                WebGl2RenderingContext::RGBA,
                WebGl2RenderingContext::UNSIGNED_BYTE,
                None,
            )
            .map_err(|_| CREATE_TEXTURE_FAILED.to_string())?;
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_MIN_FILTER,
            WebGl2RenderingContext::LINEAR as i32,
        );
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_MAG_FILTER,
            WebGl2RenderingContext::LINEAR as i32,
        );
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_WRAP_S,
            WebGl2RenderingContext::CLAMP_TO_EDGE as i32,
        );
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_WRAP_T,
            WebGl2RenderingContext::CLAMP_TO_EDGE as i32,
        );
        context.bind_texture(WebGl2RenderingContext::TEXTURE_2D, None);
        Ok(texture)
    }

    /// 分配一张深度纹理(DEPTH_COMPONENT24)。
    ///
    /// **必须是 `DEPTH_COMPONENT24`,不能是 `R32F` 颜色纹理。**
    /// `R32F` 只有通过 `EXT_color_buffer_float` 才是**可渲染**格式,
    /// 而即便扩展存在,把它挂到 `DEPTH_ATTACHMENT` 上也只在部分实现里
    /// 被接受 —— 实测这台机器的 WebGL2(SwiftShader)直接判
    /// `INCOMPLETE_ATTACHMENT`(36054),于是 `checkFramebufferStatus`
    /// 说「不完整」,而更糟的是清除与绘制都静默失效:
    /// `glClear` 不写入、每个 draw call 被丢弃,画面全黑。
    ///
    /// 阴影 pass 用的是「手动比较」(`texture(u_shadow_map, uv).r` 再和
    /// 片元深度比),所以换成 `DEPTH_COMPONENT24` 不需要改采样方式 ——
    /// 采样回来的仍然是单个 `.r` 分量。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `i32` - 纹理宽度(像素)。
    /// - `i32` - 纹理高度(像素)。
    ///
    /// # Returns
    ///
    /// - `Result<WebGlTexture, String>` - 分配好的深度纹理。
    ///
    fn create_depth_texture(
        context: &WebGl2RenderingContext,
        width: i32,
        height: i32,
    ) -> Result<WebGlTexture, String> {
        let texture: WebGlTexture = context
            .create_texture()
            .ok_or_else(|| CREATE_TEXTURE_FAILED.to_string())?;
        context.bind_texture(WebGl2RenderingContext::TEXTURE_2D, Some(&texture));
        context
            .tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
                WebGl2RenderingContext::TEXTURE_2D,
                0,
                WebGl2RenderingContext::DEPTH_COMPONENT24 as i32,
                width,
                height,
                0,
                WebGl2RenderingContext::DEPTH_COMPONENT,
                WebGl2RenderingContext::UNSIGNED_INT,
                None,
            )
            .map_err(|_| CREATE_TEXTURE_FAILED.to_string())?;
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_MIN_FILTER,
            WebGl2RenderingContext::NEAREST as i32,
        );
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_MAG_FILTER,
            WebGl2RenderingContext::NEAREST as i32,
        );
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_WRAP_S,
            WebGl2RenderingContext::CLAMP_TO_EDGE as i32,
        );
        context.tex_parameteri(
            WebGl2RenderingContext::TEXTURE_2D,
            WebGl2RenderingContext::TEXTURE_WRAP_T,
            WebGl2RenderingContext::CLAMP_TO_EDGE as i32,
        );
        context.bind_texture(WebGl2RenderingContext::TEXTURE_2D, None);
        Ok(texture)
    }

    /// 分配一个「颜色 + 深度」渲染目标(主场景、G-buffer、SSR / bloom 中转)。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `i32` - 目标宽度(像素)。
    /// - `i32` - 目标高度(像素)。
    ///
    /// # Returns
    ///
    /// - `Result<Self, String>` - 分配好的渲染目标。
    fn new_color(
        context: &WebGl2RenderingContext,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        let color: WebGlTexture = Self::create_color_texture(context, width, height)?;
        let depth: WebGlTexture = Self::create_depth_texture(context, width, height)?;
        let fbo: WebGlFramebuffer = context
            .create_framebuffer()
            .ok_or_else(|| CREATE_FRAMEBUFFER_FAILED.to_string())?;
        context.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&fbo));
        context.framebuffer_texture_2d(
            WebGl2RenderingContext::FRAMEBUFFER,
            WebGl2RenderingContext::COLOR_ATTACHMENT0,
            WebGl2RenderingContext::TEXTURE_2D,
            Some(&color),
            0,
        );
        context.framebuffer_texture_2d(
            WebGl2RenderingContext::FRAMEBUFFER,
            WebGl2RenderingContext::DEPTH_ATTACHMENT,
            WebGl2RenderingContext::TEXTURE_2D,
            Some(&depth),
            0,
        );
        let target: Self = Self {
            fbo,
            color: Some(color),
            depth,
            width,
            height,
        };
        if !target.is_complete(context) {
            return Err(FRAMEBUFFER_INCOMPLETE.to_string());
        }
        context.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, None);
        Ok(target)
    }

    /// 分配一个「只有深度」的渲染目标(阴影贴图)。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `i32` - 目标边长(像素)。
    ///
    /// # Returns
    ///
    /// - `Result<Self, String>` - 分配好的渲染目标。
    fn new_depth(context: &WebGl2RenderingContext, size: i32) -> Result<Self, String> {
        let depth: WebGlTexture = Self::create_depth_texture(context, size, size)?;
        let fbo: WebGlFramebuffer = context
            .create_framebuffer()
            .ok_or_else(|| CREATE_FRAMEBUFFER_FAILED.to_string())?;
        context.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&fbo));
        context.framebuffer_texture_2d(
            WebGl2RenderingContext::FRAMEBUFFER,
            WebGl2RenderingContext::DEPTH_ATTACHMENT,
            WebGl2RenderingContext::TEXTURE_2D,
            Some(&depth),
            0,
        );
        // 没有颜色附件时必须显式声明「不画颜色」,否则 FBO 不完整。
        context.draw_buffers(&js_sys::Array::of1(&JsValue::from_f64(
            WebGl2RenderingContext::NONE as f64,
        )));
        context.read_buffer(WebGl2RenderingContext::NONE);
        let target: Self = Self {
            fbo,
            color: None,
            depth,
            width: size,
            height: size,
        };
        if !target.is_complete(context) {
            return Err(FRAMEBUFFER_INCOMPLETE.to_string());
        }
        context.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, None);
        Ok(target)
    }

    /// 绑为当前渲染目标(`fbo` 为 `None` 时绑回屏幕)。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `Option<&Self>` - 要绑定的目标;`None` 表示屏幕。
    fn bind(context: &WebGl2RenderingContext, target: Option<&Self>) {
        match target {
            Some(target) => {
                context.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&target.fbo))
            }
            None => context.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, None),
        }
    }

    /// 当前的 FBO 是否完整。
    ///
    /// 少了这一步的后果不是「没有画面」而是**画错**:不完整的 FBO 上
    /// 任何 draw call 都会被驱动丢弃,而我们仍然会继续跑后面所有 pass,
    /// 最终把一张空纹理当成 bloom 贴进主画面 —— 整屏发白。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    ///
    /// # Returns
    ///
    /// - `bool` - 完整时为 `true`。
    fn is_complete(&self, context: &WebGl2RenderingContext) -> bool {
        // **不要**拿返回值去和 `WebGl2RenderingContext::FRAMEBUFFER_COMPLETE`
        // 比。web-sys 把这个常量按 WebGL 1 的值(36053)写死,但 WebGL2 里
        // `checkFramebufferStatus` 返回的是 WebGL2 语义的 36054;两者是
        // 同一个「完整」状态,只是编号差一。于是这个比较恒为假 —— 每个
        // FBO 明明是好的,却被判成不完整,每帧都回退软件渲染,画面全黑。
        //
        // 规范里 `FRAMEBUFFER_COMPLETE` 是**最小**的状态码(36053..36061
        // 全是各种 INCOMPLETE),所以「返回值落在完整区间」这个判据在两套
        // 编号下都成立,也不依赖任何驱动私有编号。
        let status: u32 = context.check_framebuffer_status(WebGl2RenderingContext::FRAMEBUFFER);
        status == WebGl2RenderingContext::FRAMEBUFFER_COMPLETE
            || status == FRAMEBUFFER_COMPLETE_WEBGL2_OFFSET
    }
}

// ===========================================================================
// WebGL2 后端
// ===========================================================================

/// 一个 GPU 侧资产(顶点 / 索引 / VAO 全部只上传一次)。
#[derive(Debug)]
struct GlMesh {
    /// 顶点属性数组,里面绑定了 instance buffer 的除数设置。
    vertex_array: WebGlVertexArrayObject,
    /// 顶点缓冲。`replace_mesh` 就地重传时要它(`upload_mesh` 建 VAO 时
    /// 创建,但那时只把 VAO 存了下来)。
    vertex_buffer: WebGlBuffer,
    /// 元素索引缓冲。
    index_buffer: WebGlBuffer,
    /// 索引个数(= `triangle_count * 3`)。
    index_count: i32,
}

/// WebGL2 渲染后端。
///
/// - 顶点:交错 `pos(3) | normal(3) | color(3) | emissive(3)`,见 [`STRIDE_FLOATS`]。
/// - 实例:`mat4 model`(4 个 vec4 属性)+ `vec3 tint`,`vertex_attrib_divisor = 1`。
/// - 每批次一次 `drawElementsInstanced`,即「同类资产只传矩阵」。
/// - 深度测试 + 背面剔除 + 深度写入。
pub struct WebGlRenderer {
    context: WebGl2RenderingContext,
    program: WebGlProgram,
    glow_program: WebGlProgram,
    shadow_program: WebGlProgram,
    gbuffer_program: WebGlProgram,
    ssao_program: WebGlProgram,
    ao_blur_program: WebGlProgram,
    ssr_program: WebGlProgram,
    bright_program: WebGlProgram,
    blur_program: WebGlProgram,
    composite_program: WebGlProgram,
    meshes: Vec<GlMesh>,
    uniform_view_proj: Option<WebGlUniformLocation>,
    uniform_light_dir: Option<WebGlUniformLocation>,
    uniform_light_color: Option<WebGlUniformLocation>,
    uniform_ambient: Option<WebGlUniformLocation>,
    uniform_sky_color: Option<WebGlUniformLocation>,
    uniform_emissive_gain: Option<WebGlUniformLocation>,
    uniform_fog: Option<WebGlUniformLocation>,
    uniform_eye: Option<WebGlUniformLocation>,
    uniform_exposure: Option<WebGlUniformLocation>,
    uniform_tone_map_white: Option<WebGlUniformLocation>,
    uniform_glow_strength: Option<WebGlUniformLocation>,
    uniform_shadow_map: Option<WebGlUniformLocation>,
    uniform_shadow_matrix: Option<WebGlUniformLocation>,
    uniform_shadow_params: Option<WebGlUniformLocation>,
    uniform_shadow_texel: Option<WebGlUniformLocation>,
    uniform_shadow_view_proj: Option<WebGlUniformLocation>,
    uniform_sky_ambient: Option<WebGlUniformLocation>,
    uniform_ground_ambient: Option<WebGlUniformLocation>,
    uniform_ambient_hemi: Option<WebGlUniformLocation>,
    uniform_ao_map: Option<WebGlUniformLocation>,
    uniform_ao_strength: Option<WebGlUniformLocation>,
    uniform_ssr_map: Option<WebGlUniformLocation>,
    uniform_ssr_strength: Option<WebGlUniformLocation>,
    uniform_wetness: Option<WebGlUniformLocation>,
    uniform_wet_height: Option<WebGlUniformLocation>,
    uniform_ao_height: Option<WebGlUniformLocation>,
    uniform_ao_floor: Option<WebGlUniformLocation>,
    uniform_gbuffer_view: Option<WebGlUniformLocation>,
    uniform_gbuffer_far: Option<WebGlUniformLocation>,
    uniform_ssao_gbuffer: Option<WebGlUniformLocation>,
    uniform_ssao_texel: Option<WebGlUniformLocation>,
    uniform_ssao_radius: Option<WebGlUniformLocation>,
    uniform_ssao_power: Option<WebGlUniformLocation>,
    uniform_ssao_proj: Option<WebGlUniformLocation>,
    uniform_ssao_far: Option<WebGlUniformLocation>,
    uniform_ssao_samples: Option<WebGlUniformLocation>,
    uniform_blur_ao: Option<WebGlUniformLocation>,
    uniform_blur_texel: Option<WebGlUniformLocation>,
    uniform_blur_radius: Option<WebGlUniformLocation>,
    uniform_ssr_gbuffer: Option<WebGlUniformLocation>,
    uniform_ssr_scene: Option<WebGlUniformLocation>,
    uniform_ssr_texel: Option<WebGlUniformLocation>,
    uniform_ssr_proj: Option<WebGlUniformLocation>,
    uniform_ssr_far: Option<WebGlUniformLocation>,
    uniform_ssr_max_dist: Option<WebGlUniformLocation>,
    uniform_ssr_steps: Option<WebGlUniformLocation>,
    uniform_bright_scene: Option<WebGlUniformLocation>,
    uniform_bright_threshold: Option<WebGlUniformLocation>,
    uniform_blur_source: Option<WebGlUniformLocation>,
    uniform_blur_direction: Option<WebGlUniformLocation>,
    uniform_composite_scene: Option<WebGlUniformLocation>,
    uniform_composite_bloom: Option<WebGlUniformLocation>,
    uniform_composite_lift: Option<WebGlUniformLocation>,
    uniform_composite_gamma: Option<WebGlUniformLocation>,
    uniform_composite_gain: Option<WebGlUniformLocation>,
    uniform_composite_bloom_strength: Option<WebGlUniformLocation>,
    uniform_composite_vignette: Option<WebGlUniformLocation>,
    uniform_composite_grain: Option<WebGlUniformLocation>,
    uniform_composite_time: Option<WebGlUniformLocation>,
    /// 所有批次共享的 instance buffer(按最大实例数预分配)。
    instance_buffer: WebGlBuffer,
    instance_capacity: usize,
    /// 复用缓冲:剔除近处实例时避免每次分配。
    scratch: Vec<Instance>,
    /// 验收探针:GPU 侧资产表长度(与 `Scene::meshes` 对照用)。
    ///
    /// 两者必须**完全相等**。`draw_batch` 把 `SceneBatch::mesh_index`
    /// 直接当本表下标取值,少传或多传一次都会让后面所有下标整体错位。
    gpu_mesh_count: usize,
    /// 验收探针:本批次实际准备画的索引数(0 = 被静默跳过)。
    gpu_index_count: i32,
    /// 验收探针:最近一次 `draw_batch` 的 `mesh_index` 是否越界。
    gpu_mesh_index_oob: usize,
    /// 增强管线的全部离屏目标(按画布尺寸惰性分配 / 重建)。
    targets: PipelineTargets,
}

/// 增强管线的离屏目标集合。
///
/// 全部惰性分配:画布尺寸变化时才重建,稳定分辨率下每帧零分配。
#[derive(Debug, Default)]
struct PipelineTargets {
    /// 阴影贴图(只有深度)。
    shadow: Option<RenderTarget>,
    /// 主场景颜色(阴影 / AO / SSR 之后的最终颜色)。
    scene: Option<RenderTarget>,
    /// 法线 + 线性深度 G-buffer。
    gbuffer: Option<RenderTarget>,
    /// SSAO 原始输出(半分辨率)。
    ssao: Option<RenderTarget>,
    /// AO 双边模糊后的结果。
    ao: Option<RenderTarget>,
    /// SSR 结果(半分辨率)。
    ssr: Option<RenderTarget>,
    /// bloom 亮度提取(半分辨率)。
    bright: Option<RenderTarget>,
    /// bloom 模糊的 ping-pong 中转。
    blur_ping: Option<RenderTarget>,
    /// bloom 模糊的最终结果。
    blur_pong: Option<RenderTarget>,
    /// 目标分配时的画布宽高,用来判断是否需要重建。
    allocated: (u32, u32),
}

impl WebGlRenderer {
    /// GL 上下文的克隆句柄。
    ///
    /// # Returns
    ///
    /// - `WebGl2RenderingContext` - 上下文句柄的独立副本。
    pub fn get_context(&self) -> WebGl2RenderingContext {
        self.context.clone()
    }

    /// 主 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// - `WebGlProgram` - 主 program。
    pub fn get_program(&self) -> WebGlProgram {
        self.program.clone()
    }

    /// 泛光 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// - `WebGlProgram` - 泛光 program。
    pub fn get_glow_program(&self) -> WebGlProgram {
        self.glow_program.clone()
    }

    /// 阴影 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// - `WebGlProgram` - 阴影 program。
    pub fn get_shadow_program(&self) -> WebGlProgram {
        self.shadow_program.clone()
    }

    /// 泛光强度 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_glow_strength(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_glow_strength.as_ref()
    }

    /// 阴影贴图 sampler 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_shadow_map(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_shadow_map.as_ref()
    }

    /// 阴影矩阵 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_shadow_matrix(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_shadow_matrix.as_ref()
    }

    /// 阴影参数 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_shadow_params(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_shadow_params.as_ref()
    }

    /// 阴影纹素尺寸 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_shadow_texel(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_shadow_texel.as_ref()
    }

    /// 阴影 pass 的光源视投影矩阵 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_shadow_view_proj(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_shadow_view_proj.as_ref()
    }

    /// 天光环境色 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_sky_ambient(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_sky_ambient.as_ref()
    }

    /// 地面反弹色 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ground_ambient(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ground_ambient.as_ref()
    }

    /// 半球权重 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ambient_hemi(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ambient_hemi.as_ref()
    }

    /// AO 贴图 sampler 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ao_map(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ao_map.as_ref()
    }

    /// AO 强度 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ao_strength(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ao_strength.as_ref()
    }

    /// SSR 贴图 sampler 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssr_map(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssr_map.as_ref()
    }

    /// SSR 强度 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssr_strength(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssr_strength.as_ref()
    }

    /// 湿度 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_wetness(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_wetness.as_ref()
    }

    /// 湿度衰减高度 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_wet_height(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_wet_height.as_ref()
    }

    /// 接触 AO 高度衰减 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ao_height(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ao_height.as_ref()
    }

    /// 接触 AO 最深压暗系数 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ao_floor(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ao_floor.as_ref()
    }

    /// G-buffer 视图矩阵 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_gbuffer_view(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_gbuffer_view.as_ref()
    }

    /// G-buffer 远裁剪面 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_gbuffer_far(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_gbuffer_far.as_ref()
    }

    /// SSAO 的 G-buffer sampler 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssao_gbuffer(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssao_gbuffer.as_ref()
    }

    /// SSAO 的 texel 尺寸 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssao_texel(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssao_texel.as_ref()
    }

    /// SSAO 半径 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssao_radius(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssao_radius.as_ref()
    }

    /// SSAO 幂次 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssao_power(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssao_power.as_ref()
    }

    /// SSAO 投影参数 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssao_proj(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssao_proj.as_ref()
    }

    /// SSAO 远裁剪面 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssao_far(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssao_far.as_ref()
    }

    /// SSAO 采样数 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssao_samples(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssao_samples.as_ref()
    }

    /// AO 模糊输入贴图的 sampler 位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_blur_ao(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_blur_ao.as_ref()
    }

    /// AO 模糊 texel 尺寸 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_blur_texel(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_blur_texel.as_ref()
    }

    /// AO 模糊半径 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_blur_radius(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_blur_radius.as_ref()
    }

    /// SSR 的 G-buffer sampler 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssr_gbuffer(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssr_gbuffer.as_ref()
    }

    /// SSR 的场景颜色 sampler 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssr_scene(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssr_scene.as_ref()
    }

    /// SSR 的 texel 尺寸 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssr_texel(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssr_texel.as_ref()
    }

    /// SSR 的投影参数 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssr_proj(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssr_proj.as_ref()
    }

    /// SSR 的远裁剪面 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssr_far(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssr_far.as_ref()
    }

    /// SSR 的最大追踪距离 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssr_max_dist(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssr_max_dist.as_ref()
    }

    /// SSR 的步数 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ssr_steps(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ssr_steps.as_ref()
    }

    /// bloom 亮度提取的输入 sampler 位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_bright_scene(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_bright_scene.as_ref()
    }

    /// bloom 亮度阈值 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_bright_threshold(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_bright_threshold.as_ref()
    }

    /// bloom 模糊的输入 sampler 位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_blur_source(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_blur_source.as_ref()
    }

    /// bloom 模糊方向 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_blur_direction(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_blur_direction.as_ref()
    }

    /// 合成 pass 的场景颜色 sampler 位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_composite_scene(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_composite_scene.as_ref()
    }

    /// 合成 pass 的 bloom sampler 位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_composite_bloom(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_composite_bloom.as_ref()
    }

    /// 合成 pass 的 lift uniform 位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_composite_lift(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_composite_lift.as_ref()
    }

    /// 合成 pass 的 gamma uniform 位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_composite_gamma(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_composite_gamma.as_ref()
    }

    /// 合成 pass 的 gain uniform 位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_composite_gain(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_composite_gain.as_ref()
    }

    /// 合成 pass 的 bloom 强度 uniform 位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_composite_bloom_strength(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_composite_bloom_strength.as_ref()
    }

    /// 合成 pass 的暗角 uniform 位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_composite_vignette(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_composite_vignette.as_ref()
    }

    /// 合成 pass 的颗粒 uniform 位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_composite_grain(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_composite_grain.as_ref()
    }

    /// 合成 pass 的时间种子 uniform 位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_composite_time(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_composite_time.as_ref()
    }

    /// 视图投影矩阵 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_view_proj(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_view_proj.as_ref()
    }

    /// 方向光 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_light_dir(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_light_dir.as_ref()
    }

    /// 方向光颜色 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_light_color(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_light_color.as_ref()
    }

    /// 环境光 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_ambient(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_ambient.as_ref()
    }

    /// 天空色 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_sky_color(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_sky_color.as_ref()
    }

    /// 自发光增益 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_emissive_gain(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_emissive_gain.as_ref()
    }

    /// 雾参数 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_fog(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_fog.as_ref()
    }

    /// 眼点 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_eye(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_eye.as_ref()
    }

    /// 曝光系数 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_exposure(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_exposure.as_ref()
    }

    /// 色调映射白色点 uniform 的位置。
    ///
    /// # Returns
    ///
    /// - `Option<&WebGlUniformLocation>` - uniform 位置。
    pub fn get_uniform_tone_map_white(&self) -> Option<&WebGlUniformLocation> {
        self.uniform_tone_map_white.as_ref()
    }

    /// instance buffer 的克隆句柄。
    ///
    /// # Returns
    ///
    /// - `WebGlBuffer` - instance buffer。
    pub fn get_instance_buffer(&self) -> WebGlBuffer {
        self.instance_buffer.clone()
    }

    /// instance buffer 的当前容量(以实例计)。
    ///
    /// # Returns
    ///
    /// - `usize` - 当前容量。
    pub fn get_instance_capacity(&self) -> usize {
        self.instance_capacity
    }

    /// 读 `targets` 字段(增强管线的全部离屏目标)。
    ///
    /// # Returns
    ///
    /// - `&PipelineTargets` - 离屏目标集合。
    fn get_targets(&self) -> &PipelineTargets {
        &self.targets
    }

    /// 写 `targets` 字段(增强管线的全部离屏目标)。
    ///
    /// # Arguments
    ///
    /// - `PipelineTargets` - 新的离屏目标集合。
    fn set_targets(&mut self, value: PipelineTargets) {
        self.targets = value;
    }

    /// SSAO program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// - `WebGlProgram` - SSAO program。
    fn get_ssao_program(&self) -> WebGlProgram {
        self.ssao_program.clone()
    }

    /// AO 模糊 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// - `WebGlProgram` - AO 模糊 program。
    fn get_ao_blur_program(&self) -> WebGlProgram {
        self.ao_blur_program.clone()
    }

    /// SSR program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// - `WebGlProgram` - SSR program。
    fn get_ssr_program(&self) -> WebGlProgram {
        self.ssr_program.clone()
    }

    /// bloom 亮度提取 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// - `WebGlProgram` - 亮度提取 program。
    fn get_bright_program(&self) -> WebGlProgram {
        self.bright_program.clone()
    }

    /// bloom 模糊 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// - `WebGlProgram` - 模糊 program。
    fn get_blur_program(&self) -> WebGlProgram {
        self.blur_program.clone()
    }

    /// 合成 program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// - `WebGlProgram` - 合成 program。
    fn get_composite_program(&self) -> WebGlProgram {
        self.composite_program.clone()
    }

    /// G-buffer program 的克隆句柄。
    ///
    /// # Returns
    ///
    /// - `WebGlProgram` - G-buffer program。
    fn get_gbuffer_program(&self) -> WebGlProgram {
        self.gbuffer_program.clone()
    }

    /// 设置 instance buffer 的当前容量。
    ///
    /// # Arguments
    ///
    /// - `usize` - 新的容量。
    pub fn set_instance_capacity(&mut self, value: usize) {
        self.instance_capacity = value;
    }

    /// 取出复用缓冲的可变引用。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<Instance>` - 复用缓冲。
    pub fn get_scratch_mut(&mut self) -> &mut Vec<Instance> {
        &mut self.scratch
    }

    /// 写回复用缓冲。
    ///
    /// # Arguments
    ///
    /// - `Vec<Instance>` - 复用缓冲的新内容。
    pub fn set_scratch(&mut self, value: Vec<Instance>) {
        self.scratch = value;
    }

    /// GPU 侧资产表长度(验收探针)。
    ///
    /// 必须和 `Scene::meshes.len()` **完全相等**:`SceneBatch::mesh_index`
    /// 是直接拿当本表下标用的,少上传一次就让后面全部下标错位。
    ///
    /// # Returns
    ///
    /// - `usize` - 已上传的 GPU mesh 数量。
    pub fn get_gpu_mesh_count(&self) -> usize {
        self.gpu_mesh_count
    }

    /// 写入 GPU 侧资产表长度(验收探针,见 [`Self::get_gpu_mesh_count`])。
    ///
    /// # Arguments
    ///
    /// - `usize` - 本次 `draw_batch` 看到的 GPU 表长度。
    pub fn set_gpu_mesh_count(&mut self, value: usize) {
        self.gpu_mesh_count = value;
    }

    /// 越界批次累计数自增(验收探针,见 [`Self::get_gpu_mesh_index_oob`])。
    ///
    /// # Returns
    ///
    /// - `usize` - 自增后的累计越界次数。
    pub fn set_gpu_mesh_index_oob_bumped(&mut self) -> usize {
        self.gpu_mesh_index_oob += 1;
        self.gpu_mesh_index_oob
    }

    /// 写入最近一次 `draw_batch` 的索引数(探针,见 [`Self::get_gpu_index_count`])。
    ///
    /// # Arguments
    ///
    /// - `i32` - 本批次实际要画的索引数。
    pub fn set_gpu_index_count(&mut self, value: i32) {
        self.gpu_index_count = value;
    }

    /// 最近一次 `draw_batch` 实际画的索引数(验收探针)。
    ///
    /// # Returns
    ///
    /// - `i32` - 索引数;0 表示该批次被静默跳过(没画任何三角形)。
    pub fn get_gpu_index_count(&self) -> i32 {
        self.gpu_index_count
    }

    /// `mesh_index` 越界的批次累计数(验收探针)。
    ///
    /// # Returns
    ///
    /// - `usize` - 越界次数;非 0 即表示 GPU 表与批次表已错位。
    pub fn get_gpu_index_oob(&self) -> usize {
        self.gpu_mesh_index_oob
    }

    /// GPU 侧资产表的只读视图。
    ///
    /// # Returns
    ///
    /// - `&[GlMesh]` - 已上传的 GPU 资产表。
    fn get_meshes(&self) -> &[GlMesh] {
        &self.meshes
    }

    /// GPU 侧资产表的可变引用(仅供 accessor 使用)。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<GlMesh>` - GPU 侧资产表。
    fn get_meshes_mut(&mut self) -> &mut Vec<GlMesh> {
        &mut self.meshes
    }

    /// 编译着色器;失败时返回 info log。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `u32` - 输入值。
    /// - `&str` - str 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Result<WebGlShader, String>` - 计算结果。
    ///   编译着色器;失败时返回 info log。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `u32` - 输入值。
    /// - `&str` - str 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Result<WebGlShader, String>` - 计算结果。
    fn compile_shader(
        context: &WebGl2RenderingContext,
        type_: u32,
        source: &str,
    ) -> Result<WebGlShader, String> {
        let shader: WebGlShader = context
            .create_shader(type_)
            .ok_or_else(|| CREATE_SHADER_RETURNED_NULL.to_string())?;
        context.shader_source(&shader, source);
        context.compile_shader(&shader);
        let status: bool = context
            .get_shader_parameter(&shader, WebGl2RenderingContext::COMPILE_STATUS)
            .as_bool()
            .unwrap_or(false);
        if !status {
            let log: String = context
                .get_shader_info_log(&shader)
                .unwrap_or_else(|| NO_INFO_LOG.to_string());
            context.delete_shader(Some(&shader));
            return Err(format!("shader compile failed: {log}"));
        }
        Ok(shader)
    }

    /// 链接一个 program。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `&str` - str 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Result<WebGlProgram, String>` - 计算结果。
    ///   链接一个 program。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `&str` - 顶点着色器源码。
    /// - `&str` - 片元着色器源码。
    /// - `&str` - program 的名字,只用于让链接错误信息能指出是哪个 pass。
    ///
    /// # Returns
    ///
    /// - `Result<WebGlProgram, String>` - 链接好的 program。
    fn link_program(
        context: &WebGl2RenderingContext,
        vertex_source: &str,
        fragment_source: &str,
        label: &str,
    ) -> Result<WebGlProgram, String> {
        let vertex: WebGlShader = Self::compile_shader(
            context,
            WebGl2RenderingContext::VERTEX_SHADER,
            vertex_source,
        )?;
        let fragment: WebGlShader = Self::compile_shader(
            context,
            WebGl2RenderingContext::FRAGMENT_SHADER,
            fragment_source,
        )?;
        let program: WebGlProgram = context
            .create_program()
            .ok_or_else(|| CREATE_PROGRAM_RETURNED_NULL.to_string())?;
        context.attach_shader(&program, &vertex);
        context.attach_shader(&program, &fragment);
        context.link_program(&program);
        context.delete_shader(Some(&vertex));
        context.delete_shader(Some(&fragment));
        let status: bool = context
            .get_program_parameter(&program, WebGl2RenderingContext::LINK_STATUS)
            .as_bool()
            .unwrap_or(false);
        if !status {
            let log: String = context
                .get_program_info_log(&program)
                .unwrap_or_else(|| NO_INFO_LOG.to_string());
            return Err(format!("program link failed [{label}]: {log}"));
        }
        Ok(program)
    }

    /// 在已有 canvas 上创建 WebGL2 后端;任何一步失败都返回 `Err`,
    /// 由调用方回退到 Canvas2D 软件渲染。
    /// 在已有 canvas 上创建 WebGL2 后端;任何一步失败都返回 `Err`,
    /// 由调用方回退到 Canvas2D 软件渲染。
    ///
    /// # Arguments
    ///
    /// - `&HtmlCanvasElement` - HtmlCanvasElement 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Result<Self, String>` - 计算结果。
    pub fn new(canvas: &HtmlCanvasElement) -> Result<Self, String> {
        // `getContext` 的第二个参数是 `ContextAttributes`:
        // - `depth: true`  —— 要深度缓冲;
        // - `antialias: false` —— 城市是硬边低多边形,MSAA 在软件光栅上
        //   代价太高,换来的是肉眼几乎看不出的边缘;
        // - **`depth` 不指定 `stencil: false`** 会让浏览器连 16 位模板一起
        //   分配,在 swiftshader 上深度缓冲可能因此被降到 16 位,近距离
        //   几何全部 z-fighting(第三人称贴着地面看只剩一片单色)。
        let attributes: JsValue = JsValue::from(js_sys::Object::new());
        for (name, value) in [
            (crate::r#const::GL_ATTR_ALPHA, false),
            (crate::r#const::GL_ATTR_DEPTH, true),
            (crate::r#const::GL_ATTR_STENCIL, false),
            (crate::r#const::GL_ATTR_ANTIALIAS, false),
            (crate::r#const::GL_ATTR_PRESERVE_DRAWING_BUFFER, true),
        ] {
            let _: Result<bool, JsValue> = js_sys::Reflect::set(
                &attributes,
                &JsValue::from_str(name),
                &JsValue::from_bool(value),
            );
        }
        let context: WebGl2RenderingContext = canvas
            .get_context_with_context_options(WEBGL2_2, &attributes)
            .map_err(|err: JsValue| format!("get_context threw: {err:?}"))?
            .ok_or_else(|| WEBGL2_UNAVAILABLE.to_string())?
            .dyn_into_webgl();
        let program: WebGlProgram =
            Self::link_program(&context, VERTEX_SHADER, FRAGMENT_SHADER, PROGRAM_MAIN)?;
        let glow_program: WebGlProgram =
            Self::link_program(&context, VERTEX_SHADER, GLOW_FRAGMENT_SHADER, PROGRAM_GLOW)?;
        let shadow_program: WebGlProgram = Self::link_program(
            &context,
            SHADOW_VERTEX_SHADER,
            SHADOW_FRAGMENT_SHADER,
            PROGRAM_SHADOW,
        )?;
        let gbuffer_program: WebGlProgram = Self::link_program(
            &context,
            VERTEX_SHADER,
            GBUFFER_FRAGMENT_SHADER,
            PROGRAM_GBUFFER,
        )?;
        let ssao_program: WebGlProgram = Self::link_program(
            &context,
            FULLSCREEN_VERTEX_SHADER,
            SSAO_FRAGMENT_SHADER,
            PROGRAM_SSAO,
        )?;
        let ao_blur_program: WebGlProgram = Self::link_program(
            &context,
            FULLSCREEN_VERTEX_SHADER,
            AO_BLUR_FRAGMENT_SHADER,
            PROGRAM_AO_BLUR,
        )?;
        let ssr_program: WebGlProgram = Self::link_program(
            &context,
            FULLSCREEN_VERTEX_SHADER,
            SSR_FRAGMENT_SHADER,
            PROGRAM_SSR,
        )?;
        let bright_program: WebGlProgram = Self::link_program(
            &context,
            FULLSCREEN_VERTEX_SHADER,
            BLOOM_EXTRACT_FRAGMENT_SHADER,
            PROGRAM_BRIGHT,
        )?;
        let blur_program: WebGlProgram = Self::link_program(
            &context,
            FULLSCREEN_VERTEX_SHADER,
            BLOOM_BLUR_FRAGMENT_SHADER,
            PROGRAM_BLUR,
        )?;
        let composite_program: WebGlProgram = Self::link_program(
            &context,
            FULLSCREEN_VERTEX_SHADER,
            COMPOSITE_FRAGMENT_SHADER,
            PROGRAM_COMPOSITE,
        )?;
        let instance_buffer: WebGlBuffer = context
            .create_buffer()
            .ok_or_else(|| CREATE_BUFFER_FAILED.to_string())?;
        // **必须在任何 VAO 引用它之前就分配存储。**
        //
        // `upload_mesh` 会给每个资产的 VAO 调 `vertex_attrib_pointer`,
        // 那时会把 instance buffer 记录进 VAO。如果此刻它还是 0 字节,
        // 后面 `reserve_instances` 里的 `bufferData` 只是扩容,已经建好的
        // VAO 不会重新绑定 —— 在部分驱动(SwiftShader / ANGLE)上
        // attribute 会一直读到未初始化内容,表现为整屏的彩色大三角面。
        // 一次性按 MAX 批大小预分配,之后只做 `bufferSubData` 覆写。
        context.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(&instance_buffer));
        context.buffer_data_with_i32(
            WebGl2RenderingContext::ARRAY_BUFFER,
            (INSTANCE_PREALLOC * FLOATS_PER_INSTANCE * 4) as i32,
            WebGl2RenderingContext::DYNAMIC_DRAW,
        );
        context.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, None);

        let u_view_proj: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_VIEW_PROJ);
        let u_light_dir: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_LIGHT_DIR);
        let u_light_color: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_LIGHT_COLOR);
        let u_ambient: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_AMBIENT);
        let u_sky_color: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_SKY_COLOR);
        let u_emissive_gain: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_EMISSIVE_GAIN);
        let u_fog: Option<WebGlUniformLocation> = context.get_uniform_location(&program, U_FOG);
        let u_eye: Option<WebGlUniformLocation> = context.get_uniform_location(&program, U_EYE);
        let u_exposure: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_EXPOSURE);
        let u_tone_map_white: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_TONE_MAP_WHITE);
        let u_glow_strength: Option<WebGlUniformLocation> =
            context.get_uniform_location(&glow_program, U_GLOW_STRENGTH);
        // 阴影 pass 用同一个 `u_view_proj` 名字,但绑在 shadow_program 上。
        let u_shadow_view_proj: Option<WebGlUniformLocation> =
            context.get_uniform_location(&shadow_program, U_VIEW_PROJ);
        let u_shadow_texel: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_SHADOW_TEXEL);
        let u_composite_bloom_strength: Option<WebGlUniformLocation> =
            context.get_uniform_location(&composite_program, U_BLOOM_STRENGTH);
        let u_wet_height: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_WET_HEIGHT);
        let u_ao_height: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_AO_HEIGHT);
        let u_ao_floor: Option<WebGlUniformLocation> =
            context.get_uniform_location(&program, U_AO_FLOOR);
        let gl_context: WebGl2RenderingContext = context.clone();
        let uniform_shadow_map: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_SHADOW_MAP);
        let uniform_shadow_matrix: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_SHADOW_MATRIX);
        let uniform_shadow_params: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_SHADOW_PARAMS);
        let uniform_sky_ambient: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_SKY_AMBIENT);
        let uniform_ground_ambient: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_GROUND_AMBIENT);
        let uniform_ambient_hemi: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_AMBIENT_HEMI);
        let uniform_ao_map: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_AO_MAP);
        let uniform_ao_strength: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_AO_STRENGTH);
        let uniform_ssr_map: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_SSR_MAP);
        let uniform_ssr_strength: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_SSR_STRENGTH);
        let uniform_wetness: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&program, U_WETNESS);
        let uniform_gbuffer_view: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&gbuffer_program, U_VIEW);
        let uniform_gbuffer_far: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&gbuffer_program, U_FAR_PLANE);
        let uniform_ssao_gbuffer: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_GBUFFER);
        let uniform_ssao_texel: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_TEXEL_SIZE);
        let uniform_ssao_radius: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_AO_RADIUS);
        let uniform_ssao_power: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_AO_POWER);
        let uniform_ssao_proj: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_PROJ_PARAMS);
        let uniform_ssao_far: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_FAR_PLANE);
        let uniform_ssao_samples: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssao_program, U_SAMPLES);
        let uniform_blur_ao: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ao_blur_program, U_AO_BLUR_MAP);
        let uniform_blur_texel: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ao_blur_program, U_TEXEL_SIZE);
        let uniform_blur_radius: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ao_blur_program, U_BLUR_RADIUS);
        let uniform_ssr_gbuffer: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_GBUFFER);
        let uniform_ssr_scene: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_SCENE_COLOR);
        let uniform_ssr_texel: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_TEXEL_SIZE);
        let uniform_ssr_proj: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_PROJ_PARAMS);
        let uniform_ssr_far: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_FAR_PLANE);
        let uniform_ssr_max_dist: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_SSR_MAX_DIST);
        let uniform_ssr_steps: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&ssr_program, U_SSR_STEPS);
        let uniform_bright_scene: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&bright_program, U_SCENE_COLOR);
        let uniform_bright_threshold: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&bright_program, U_BLOOM_THRESHOLD);
        let uniform_blur_source: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&blur_program, U_SCENE_COLOR);
        let uniform_blur_direction: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&blur_program, U_BLOOM_DIR);
        let uniform_composite_scene: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_SCENE_COLOR);
        let uniform_composite_bloom: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_BLOOM_MAP);
        let uniform_composite_lift: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_GRADE_LIFT);
        let uniform_composite_gamma: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_GRADE_GAMMA);
        let uniform_composite_gain: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_GRADE_GAIN);
        let uniform_composite_vignette: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_VIGNETTE);
        let uniform_composite_grain: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_GRAIN);
        let uniform_composite_time: Option<WebGlUniformLocation> =
            gl_context.get_uniform_location(&composite_program, U_TIME);
        let renderer: Self = Self {
            context,
            program,
            glow_program,
            shadow_program,
            gbuffer_program,
            ssao_program,
            ao_blur_program,
            ssr_program,
            bright_program,
            blur_program,
            composite_program,
            meshes: Vec::new(),
            uniform_view_proj: u_view_proj,
            uniform_light_dir: u_light_dir,
            uniform_light_color: u_light_color,
            uniform_ambient: u_ambient,
            uniform_sky_color: u_sky_color,
            uniform_emissive_gain: u_emissive_gain,
            uniform_fog: u_fog,
            uniform_eye: u_eye,
            uniform_exposure: u_exposure,
            uniform_tone_map_white: u_tone_map_white,
            uniform_glow_strength: u_glow_strength,
            uniform_shadow_map,
            uniform_shadow_matrix,
            uniform_shadow_params,
            uniform_shadow_texel: u_shadow_texel,
            uniform_shadow_view_proj: u_shadow_view_proj,
            uniform_sky_ambient,
            uniform_ground_ambient,
            uniform_ambient_hemi,
            uniform_ao_map,
            uniform_ao_strength,
            uniform_ssr_map,
            uniform_ssr_strength,
            uniform_wetness,
            uniform_wet_height: u_wet_height,
            uniform_ao_height: u_ao_height,
            uniform_ao_floor: u_ao_floor,
            uniform_gbuffer_view,
            uniform_gbuffer_far,
            uniform_ssao_gbuffer,
            uniform_ssao_texel,
            uniform_ssao_radius,
            uniform_ssao_power,
            uniform_ssao_proj,
            uniform_ssao_far,
            uniform_ssao_samples,
            uniform_blur_ao,
            uniform_blur_texel,
            uniform_blur_radius,
            uniform_ssr_gbuffer,
            uniform_ssr_scene,
            uniform_ssr_texel,
            uniform_ssr_proj,
            uniform_ssr_far,
            uniform_ssr_max_dist,
            uniform_ssr_steps,
            uniform_bright_scene,
            uniform_bright_threshold,
            uniform_blur_source,
            uniform_blur_direction,
            uniform_composite_scene,
            uniform_composite_bloom,
            uniform_composite_lift,
            uniform_composite_gamma,
            uniform_composite_gain,
            uniform_composite_bloom_strength: u_composite_bloom_strength,
            uniform_composite_vignette,
            uniform_composite_grain,
            uniform_composite_time,
            instance_buffer,
            instance_capacity: INSTANCE_PREALLOC,
            scratch: Vec::new(),
            gpu_mesh_count: 0,
            gpu_index_count: 0,
            gpu_mesh_index_oob: 0,
            targets: PipelineTargets::default(),
        };
        let _: &WebGl2RenderingContext = gl_context.as_ref();

        gl_context.enable(WebGl2RenderingContext::DEPTH_TEST);
        gl_context.depth_func(WebGl2RenderingContext::LESS);
        gl_context.enable(WebGl2RenderingContext::CULL_FACE);
        gl_context.cull_face(WebGl2RenderingContext::BACK);
        gl_context.enable(WebGl2RenderingContext::BLEND);
        gl_context.blend_func(
            WebGl2RenderingContext::SRC_ALPHA,
            WebGl2RenderingContext::ONE,
        );
        Ok(renderer)
    }

    /// 上传一个资产的顶点 / 索引,并创建带实例布局的 VAO。
    ///
    /// **每个 mesh 只能调用一次。** 本函数是 `push` 语义:在
    /// `self.meshes` 尾部追加并返回 `len - 1`。而 `SceneBatch::mesh_index`
    /// 是 `Scene::meshes` 的下标,`draw_batch` 把它**直接**当作
    /// `self.meshes` 的下标来取 VAO —— 两者只有在「每个 `Scene::meshes`
    /// 元素恰好上传一次」时才成立。
    ///
    /// 同一个 mesh 上传两次会让 GPU 表多出一整轮,之后所有下标整体错位:
    /// 尾部新加的批次(玩家 13 个骨架 part)取到的是**重传那一轮的旧
    /// 资产**,于是几栋几十米高的楼被按 1.75 米小人的 model matrix 摆到
    /// 玩家脚下糊满屏幕,而角色真正的 mesh 一次都没画过。
    ///
    /// # Arguments
    ///
    /// - `&MeshAssetGpu` - MeshAssetGpu 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Result<usize, String>` - 新追加的 GPU mesh 索引。
    pub fn upload_mesh(&mut self, mesh: &MeshAssetGpu) -> Result<usize, String> {
        let context: WebGl2RenderingContext = self.get_context();
        let vertex_array: WebGlVertexArrayObject = context
            .create_vertex_array()
            .ok_or_else(|| CREATE_VERTEX_ARRAY_FAILED.to_string())?;
        context.bind_vertex_array(Some(&vertex_array));

        let vertex_buffer: WebGlBuffer = context
            .create_buffer()
            .ok_or_else(|| CREATE_BUFFER_FAILED.to_string())?;
        context.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(&vertex_buffer));
        context.buffer_data_with_u8_array(
            WebGl2RenderingContext::ARRAY_BUFFER,
            f32_slice_to_bytes(&mesh.vertices),
            WebGl2RenderingContext::STATIC_DRAW,
        );
        let stride: i32 = (STRIDE_FLOATS * 4) as i32;
        for attribute in 0..4u32 {
            context.enable_vertex_attrib_array(attribute);
            context.vertex_attrib_pointer_with_i32(
                attribute,
                3,
                WebGl2RenderingContext::FLOAT,
                false,
                stride,
                (attribute as i32) * 12,
            );
        }

        let index_buffer: WebGlBuffer = context
            .create_buffer()
            .ok_or_else(|| CREATE_BUFFER_FAILED.to_string())?;
        context.bind_buffer(
            WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER,
            Some(&index_buffer),
        );
        context.buffer_data_with_u8_array(
            WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER,
            crate::mesh::u32_slice_to_bytes(&mesh.indices),
            WebGl2RenderingContext::STATIC_DRAW,
        );

        // 实例属性:model matrix 的 4 个 vec4 + tint。
        context.bind_buffer(
            WebGl2RenderingContext::ARRAY_BUFFER,
            Some(&self.get_instance_buffer()),
        );
        for attribute in 4..8u32 {
            context.enable_vertex_attrib_array(attribute);
            context.vertex_attrib_pointer_with_i32(
                attribute,
                4,
                WebGl2RenderingContext::FLOAT,
                false,
                INSTANCE_STRIDE_BYTES,
                (attribute as i32 - 4) * 16,
            );
            context.vertex_attrib_divisor(attribute, 1);
        }
        context.enable_vertex_attrib_array(8);
        context.vertex_attrib_pointer_with_i32(
            8,
            3,
            WebGl2RenderingContext::FLOAT,
            false,
            INSTANCE_STRIDE_BYTES,
            64,
        );
        context.vertex_attrib_divisor(8, 1);

        context.bind_vertex_array(None);
        let index_count: i32 = mesh.indices.len() as i32;
        self.get_meshes_mut().push(GlMesh {
            vertex_array,
            vertex_buffer,
            index_buffer,
            index_count,
        });
        Ok(self.get_meshes().len() - 1)
    }

    /// 就地替换某个已上传 mesh 的顶点 / 索引数据。
    ///
    /// **这是程序化无限世界的前提。** `upload_mesh` 是 `push` 语义,只能
    /// 在启动时把 `Scene::meshes` 一一对应地传上 GPU 一次。但地面 / 水面
    /// 跟着玩家流式重建(`rebuild_streamed_surface` 原地改写
    /// `scene.meshes[i]`),CPU 侧的网格换了而 GPU 缓冲还指着旧数据 ——
    /// 走出街区边界后画出来的仍是出生点那块地。
    ///
    /// 这里**不能**改用 `upload_mesh` 重传:那是 `push` 语义,GPU 表会
    /// 多出一整条,而 `SceneBatch::mesh_index` 是直接当 GPU 表下标用的
    ///(`draw_batch`),之后所有批次整体错位一格 —— 角色 / 车会取到别人
    /// 的网格,表现就是「移动时丢失建模、只剩轮子」。原地重传保持 GPU
    /// 表长度不变,下标**永久有效**。
    ///
    /// # Arguments
    ///
    /// - `usize` - `Scene::meshes` 的下标(即批次里的 `mesh_index`)。
    /// - `&MeshAssetGpu` - 新的顶点 / 索引数据。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 下标越界时的错误信息。
    pub fn replace_mesh(&mut self, mesh_index: usize, mesh: &MeshAssetGpu) -> Result<(), String> {
        let context: WebGl2RenderingContext = self.get_context();
        let Some(slot) = self.get_meshes_mut().get_mut(mesh_index) else {
            return Err(format!(
                "{}: {mesh_index} of {}",
                REPLACE_MESH_OUT_OF_RANGE,
                self.get_meshes().len()
            ));
        };
        // 顶点:先绑 VAO(索引缓冲是它的状态),再重传并重设属性指针。
        context.bind_vertex_array(Some(&slot.vertex_array));
        context.bind_buffer(
            WebGl2RenderingContext::ARRAY_BUFFER,
            Some(&slot.vertex_buffer),
        );
        context.buffer_data_with_u8_array(
            WebGl2RenderingContext::ARRAY_BUFFER,
            f32_slice_to_bytes(&mesh.vertices),
            WebGl2RenderingContext::DYNAMIC_DRAW,
        );
        let stride: i32 = (STRIDE_FLOATS * 4) as i32;
        for attribute in 0..4u32 {
            context.vertex_attrib_pointer_with_i32(
                attribute,
                3,
                WebGl2RenderingContext::FLOAT,
                false,
                stride,
                (attribute as i32) * 12,
            );
        }
        context.bind_buffer(
            WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER,
            Some(&slot.index_buffer),
        );
        context.buffer_data_with_u8_array(
            WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER,
            crate::mesh::u32_slice_to_bytes(&mesh.indices),
            WebGl2RenderingContext::DYNAMIC_DRAW,
        );
        context.bind_vertex_array(None);
        slot.index_count = mesh.indices.len() as i32;
        Ok(())
    }

    /// 保证 instance buffer 至少能装下 `capacity` 个实例。
    ///
    /// 每实例 28 个 f32(model matrix 16 + tint 3,加上对齐填充共 28 = 7×vec4)。
    /// 保证 instance buffer 至少能装下 `capacity` 个实例。
    ///
    /// 每实例 28 个 f32(model matrix 16 + tint 3,加上对齐填充共 28 = 7×vec4)。
    ///
    /// # Arguments
    ///
    /// - `usize` - 输入值。
    pub fn reserve_instances(&mut self, capacity: usize) {
        if capacity <= self.get_instance_capacity() {
            return;
        }
        let context: WebGl2RenderingContext = self.get_context();
        let next: usize = capacity.next_power_of_two();
        let bytes: Vec<u8> = vec![0u8; next * FLOATS_PER_INSTANCE * 4];
        context.bind_buffer(
            WebGl2RenderingContext::ARRAY_BUFFER,
            Some(&self.get_instance_buffer()),
        );
        context.buffer_data_with_u8_array(
            WebGl2RenderingContext::ARRAY_BUFFER,
            &bytes,
            WebGl2RenderingContext::DYNAMIC_DRAW,
        );
        self.set_instance_capacity(next);
    }

    /// 按需分配 / 重建整条增强管线的离屏目标。
    ///
    /// 画布尺寸不变时**直接返回**:2048² 的阴影贴图 + 5 张全分辨率
    /// 纹理每帧重建一次会直接吃掉整个帧预算。
    ///
    /// # Arguments
    ///
    /// - `u32` - 画布宽度(像素)。
    /// - `u32` - 画布高度(像素)。
    ///
    /// # Returns
    ///
    /// - `Result<(), String>` - 分配失败时的错误信息。
    fn ensure_targets(&mut self, width: u32, height: u32) -> Result<(), String> {
        if self.get_targets().allocated == (width, height) && self.get_targets().scene.is_some() {
            return Ok(());
        }
        let context: WebGl2RenderingContext = self.get_context();
        // 每张目标各按自己的缩放系数算尺寸。G-buffer 走全分辨率
        // (SSAO 的边缘质量完全由它决定),AO / SSR / bloom 走半分辨率
        // —— 后三者都要在后面做一次模糊,半分辨率几乎无损。
        let scaled: fn(u32, f32) -> i32 =
            |base: u32, factor: f32| (((base as f32) * factor).max(1.0)) as i32;
        let w: i32 = width as i32;
        let h: i32 = height as i32;
        let g_w: i32 = scaled(width, GBUFFER_SCALE);
        let g_h: i32 = scaled(height, GBUFFER_SCALE);
        let ao_w: i32 = scaled(g_w as u32, SSAO_SCALE);
        let ao_h: i32 = scaled(g_h as u32, SSAO_SCALE);
        let ssr_w: i32 = scaled(width, SSR_SCALE);
        let ssr_h: i32 = scaled(height, SSR_SCALE);
        let bloom_w: i32 = scaled(width, BLOOM_SCALE);
        let bloom_h: i32 = scaled(height, BLOOM_SCALE);
        self.set_targets(PipelineTargets {
            shadow: Some(RenderTarget::new_depth(&context, SHADOW_MAP_SIZE as i32)?),
            scene: Some(RenderTarget::new_color(&context, w, h)?),
            gbuffer: Some(RenderTarget::new_color(&context, g_w, g_h)?),
            ssao: Some(RenderTarget::new_color(&context, ao_w, ao_h)?),
            ao: Some(RenderTarget::new_color(&context, ao_w, ao_h)?),
            ssr: Some(RenderTarget::new_color(&context, ssr_w, ssr_h)?),
            bright: Some(RenderTarget::new_color(&context, bloom_w, bloom_h)?),
            blur_ping: Some(RenderTarget::new_color(&context, bloom_w, bloom_h)?),
            blur_pong: Some(RenderTarget::new_color(&context, bloom_w, bloom_h)?),
            allocated: (width, height),
        });
        Ok(())
    }

    /// 把一张纹理绑到指定的纹理单元并设给一个 sampler uniform。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `&WebGlTexture` - 要绑定的纹理。
    /// - `u32` - 纹理单元编号。
    /// - `Option<&WebGlUniformLocation>` - 对应的 sampler uniform。
    fn bind_sampler(
        context: &WebGl2RenderingContext,
        texture: &WebGlTexture,
        unit: u32,
        uniform: Option<&WebGlUniformLocation>,
    ) {
        context.active_texture(WebGl2RenderingContext::TEXTURE0 + unit);
        context.bind_texture(WebGl2RenderingContext::TEXTURE_2D, Some(texture));
        context.uniform1i(uniform, unit as i32);
    }

    /// 画一个覆盖当前视口的全屏三角形。
    ///
    /// `draw_arrays` 而不是带 VBO 的 `draw_arrays_instanced`:
    /// [`FULLSCREEN_VERTEX_SHADER`] 用 `gl_VertexID` 算位置,不需要任何缓冲。
    /// 后处理 pass 用 `draw_arrays` 时**不能**留着一个 VAO 绑着 ——
    /// 那是几何的 VAO,它的 attribute 会让绘制结果完全错乱。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    fn draw_fullscreen(context: &WebGl2RenderingContext) {
        context.bind_vertex_array(None);
        context.draw_arrays(WebGl2RenderingContext::TRIANGLES, 0, 3);
    }

    /// 把一个离屏目标清成常量 —— 降级时替代被跳过的 pass。
    ///
    /// 没有这一步的话,关掉 SSAO 的那一帧会继续采样上一档留下的 AO 贴图,
    /// 表现为「画质降了但画面突然脏了一块」。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `&Option<RenderTarget>` - 要清的目标。
    /// - `f32` - 写入 R 通道的常量(G / B 通道自动取 0)。
    fn clear_to_neutral(
        &self,
        context: &WebGl2RenderingContext,
        target: &Option<RenderTarget>,
        value: f32,
    ) {
        let Some(target) = target.as_ref() else {
            return;
        };
        RenderTarget::bind(context, Some(target));
        context.disable(WebGl2RenderingContext::DEPTH_TEST);
        context.disable(WebGl2RenderingContext::BLEND);
        context.disable(WebGl2RenderingContext::CULL_FACE);
        context.viewport(0, 0, target.width, target.height);
        context.clear_color(value, 0.0, 0.0, 1.0);
        context.clear(WebGl2RenderingContext::COLOR_BUFFER_BIT);
    }

    /// 跑 SSAO + 双边模糊,结果写进 AO 目标。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `f32` - 垂直视场角(弧度)。
    /// - `f32` - 宽高比。
    /// - `f32` - 远裁剪面距离(米)。
    fn render_ao(&self, context: &WebGl2RenderingContext, fov_y: f32, aspect: f32, far: f32) {
        let (Some(ssao), Some(ao), Some(gbuffer)) = (
            self.get_targets().ssao.as_ref(),
            self.get_targets().ao.as_ref(),
            self.get_targets().gbuffer.as_ref(),
        ) else {
            return;
        };
        let proj_params: [f32; 2] = [(fov_y * 0.5).tan() * aspect, (fov_y * 0.5).tan()];
        // ---- SSAO 原始输出 ----
        RenderTarget::bind(context, Some(ssao));
        context.disable(WebGl2RenderingContext::DEPTH_TEST);
        context.disable(WebGl2RenderingContext::BLEND);
        context.disable(WebGl2RenderingContext::CULL_FACE);
        context.viewport(0, 0, ssao.width, ssao.height);
        context.use_program(Some(&self.get_ssao_program()));
        // G-buffer 的「法线 + 线性深度」打包在同一张 RGBA8 上。
        Self::bind_sampler(
            context,
            gbuffer.color.as_ref().unwrap_or(&gbuffer.depth),
            0,
            self.get_uniform_ssao_gbuffer(),
        );
        context.uniform2f(
            self.get_uniform_ssao_texel(),
            1.0 / gbuffer.width as f32,
            1.0 / gbuffer.height as f32,
        );
        context.uniform1f(self.get_uniform_ssao_radius(), SSAO_RADIUS);
        context.uniform1f(self.get_uniform_ssao_power(), SSAO_POWER);
        context.uniform2f(self.get_uniform_ssao_proj(), proj_params[0], proj_params[1]);
        context.uniform1f(self.get_uniform_ssao_far(), far);
        context.uniform1i(self.get_uniform_ssao_samples(), SSAO_SAMPLES);
        Self::draw_fullscreen(context);
        // ---- 双边模糊 ----
        RenderTarget::bind(context, Some(ao));
        context.viewport(0, 0, ao.width, ao.height);
        context.use_program(Some(&self.get_ao_blur_program()));
        Self::bind_sampler(
            context,
            ssao.color.as_ref().unwrap_or(&ssao.depth),
            0,
            self.get_uniform_blur_ao(),
        );
        context.uniform2f(
            self.get_uniform_blur_texel(),
            1.0 / ssao.width as f32,
            1.0 / ssao.height as f32,
        );
        context.uniform1i(self.get_uniform_blur_radius(), AO_BLUR_RADIUS);
        Self::draw_fullscreen(context);
    }

    /// 跑 SSR(屏幕空间反射)。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `f32` - 垂直视场角(弧度)。
    /// - `f32` - 宽高比。
    /// - `f32` - 远裁剪面距离(米)。
    fn render_ssr(&self, context: &WebGl2RenderingContext, fov_y: f32, aspect: f32, far: f32) {
        let (Some(ssr), Some(gbuffer), Some(scene)) = (
            self.get_targets().ssr.as_ref(),
            self.get_targets().gbuffer.as_ref(),
            self.get_targets().scene.as_ref(),
        ) else {
            return;
        };
        RenderTarget::bind(context, Some(ssr));
        context.disable(WebGl2RenderingContext::DEPTH_TEST);
        context.disable(WebGl2RenderingContext::BLEND);
        context.disable(WebGl2RenderingContext::CULL_FACE);
        context.viewport(0, 0, ssr.width, ssr.height);
        context.use_program(Some(&self.get_ssr_program()));
        Self::bind_sampler(
            context,
            gbuffer.color.as_ref().unwrap_or(&gbuffer.depth),
            0,
            self.get_uniform_ssr_gbuffer(),
        );
        Self::bind_sampler(
            context,
            scene.color.as_ref().unwrap_or(&scene.depth),
            1,
            self.get_uniform_ssr_scene(),
        );
        context.uniform2f(
            self.get_uniform_ssr_texel(),
            1.0 / gbuffer.width as f32,
            1.0 / gbuffer.height as f32,
        );
        context.uniform2f(
            self.get_uniform_ssr_proj(),
            (fov_y * 0.5).tan() * aspect,
            (fov_y * 0.5).tan(),
        );
        context.uniform1f(self.get_uniform_ssr_far(), far);
        context.uniform1f(self.get_uniform_ssr_max_dist(), SSR_MAX_DIST);
        context.uniform1i(self.get_uniform_ssr_steps(), SSR_STEPS);
        Self::draw_fullscreen(context);
    }

    /// 跑 bloom:亮度提取 → 水平模糊 → 垂直模糊。
    ///
    /// **两 pass 高斯模糊**是「软光晕」与「硬边色块」的分界:
    /// 现在的老实现只做加法叠加,没有模糊,所以霓虹周围是硬边。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    fn render_bloom(&self, context: &WebGl2RenderingContext) {
        let (Some(scene), Some(bright), Some(ping), Some(pong)) = (
            self.get_targets().scene.as_ref(),
            self.get_targets().bright.as_ref(),
            self.get_targets().blur_ping.as_ref(),
            self.get_targets().blur_pong.as_ref(),
        ) else {
            return;
        };
        context.disable(WebGl2RenderingContext::DEPTH_TEST);
        context.disable(WebGl2RenderingContext::BLEND);
        context.disable(WebGl2RenderingContext::CULL_FACE);
        // ---- 亮度提取 ----
        RenderTarget::bind(context, Some(bright));
        context.viewport(0, 0, bright.width, bright.height);
        context.use_program(Some(&self.get_bright_program()));
        Self::bind_sampler(
            context,
            scene.color.as_ref().unwrap_or(&scene.depth),
            0,
            self.get_uniform_bright_scene(),
        );
        context.uniform1f(self.get_uniform_bright_threshold(), BLOOM_THRESHOLD);
        Self::draw_fullscreen(context);
        // ---- 水平模糊 ----
        RenderTarget::bind(context, Some(ping));
        context.viewport(0, 0, ping.width, ping.height);
        context.use_program(Some(&self.get_blur_program()));
        Self::bind_sampler(
            context,
            bright.color.as_ref().unwrap_or(&bright.depth),
            0,
            self.get_uniform_blur_source(),
        );
        let texel: [f32; 2] = [1.0 / ping.width as f32, 1.0 / ping.height as f32];
        context.uniform2f(
            self.get_uniform_blur_direction(),
            texel[0] * BLOOM_BLUR_SPREAD,
            0.0,
        );
        Self::draw_fullscreen(context);
        // ---- 垂直模糊 ----
        RenderTarget::bind(context, Some(pong));
        context.viewport(0, 0, pong.width, pong.height);
        Self::bind_sampler(
            context,
            ping.color.as_ref().unwrap_or(&ping.depth),
            0,
            self.get_uniform_blur_source(),
        );
        context.uniform2f(
            self.get_uniform_blur_direction(),
            0.0,
            texel[1] * BLOOM_BLUR_SPREAD,
        );
        Self::draw_fullscreen(context);
    }

    /// 把主场景颜色 + bloom 合成到屏幕,顺带做色调分级 / 暗角 / 颗粒。
    ///
    /// # Arguments
    ///
    /// - `&WebGl2RenderingContext` - WebGl2RenderingContext 的只读引用。
    /// - `&SceneLighting` - SceneLighting 的只读引用。
    /// - `u32` - 画布宽度(像素)。
    /// - `u32` - 画布高度(像素)。
    /// - `f32` - 本帧的时间(秒),用作胶片颗粒的种子。
    fn render_composite(
        &self,
        context: &WebGl2RenderingContext,
        lighting: &SceneLighting,
        width: u32,
        height: u32,
        time: f32,
    ) {
        let Some(scene) = self.get_targets().scene.as_ref() else {
            return;
        };
        let bloom: Option<&RenderTarget> = self.get_targets().blur_pong.as_ref();
        RenderTarget::bind(context, None);
        context.viewport(0, 0, width as i32, height as i32);
        context.disable(WebGl2RenderingContext::DEPTH_TEST);
        context.disable(WebGl2RenderingContext::BLEND);
        context.disable(WebGl2RenderingContext::CULL_FACE);
        context.use_program(Some(&self.get_composite_program()));
        Self::bind_sampler(
            context,
            scene.color.as_ref().unwrap_or(&scene.depth),
            0,
            self.get_uniform_composite_scene(),
        );
        if let Some(bloom) = bloom {
            Self::bind_sampler(
                context,
                bloom.color.as_ref().unwrap_or(&bloom.depth),
                1,
                self.get_uniform_composite_bloom(),
            );
        }
        context.uniform3f(
            self.get_uniform_composite_lift(),
            lighting.grade_lift[0],
            lighting.grade_lift[1],
            lighting.grade_lift[2],
        );
        context.uniform3f(
            self.get_uniform_composite_gamma(),
            lighting.grade_gamma[0],
            lighting.grade_gamma[1],
            lighting.grade_gamma[2],
        );
        context.uniform3f(
            self.get_uniform_composite_gain(),
            lighting.grade_gain[0],
            lighting.grade_gain[1],
            lighting.grade_gain[2],
        );
        context.uniform1f(
            self.get_uniform_composite_bloom_strength(),
            BLOOM_STRENGTH * lighting.emissive_gain.max(BLOOM_MIN_GAIN),
        );
        context.uniform1f(
            self.get_uniform_composite_vignette(),
            lighting.vignette * VIGNETTE_BASE,
        );
        context.uniform1f(
            self.get_uniform_composite_grain(),
            lighting.grain * GRAIN_BASE,
        );
        context.uniform1f(self.get_uniform_composite_time(), time);
        Self::draw_fullscreen(context);
    }

    /// 渲染一帧。
    ///
    /// **管线顺序**(每一趟都写进不同的离屏目标):
    /// 1. 阴影 pass —— 从光源看,写 2048² 的深度贴图;
    /// 2. G-buffer —— 写世界法线 + 视空间线性深度;
    /// 3. 主 pass —— 用阴影 + AO 引用把颜色画进 `scene` 目标
    ///    (此时 bloom 还没做,所以霓虹是硬边的原始色);
    /// 4. SSAO + 双边模糊(读 G-buffer);
    /// 5. SSR(读 G-buffer + `scene`);
    /// 6. bloom(亮度提取 + 水平 / 垂直模糊);
    /// 7. 合成(主颜色 + bloom + 色调分级 + 暗角 + 颗粒)→ 屏幕。
    ///
    /// 阴影 / AO / SSR 的贴图在第 3 步就要被**引用**,所以第 4/5 步
    /// 严格来说是「这一帧的 SSAO 会晚一步生效」。这是所有基于延迟
    /// 缓冲的管线的固有滞后(需要一份上一帧的结果),在这套单 pass
    /// 结构里用「同帧内先算、主 pass 引用上一帧的贴图」来规避:
    /// 目标纹理 ping-pong 一次,代价是多一张全分辨率纹理。
    ///
    /// # Arguments
    ///
    /// - `&Scene` - Scene 的只读引用。
    /// - `&Mat4` - 视图投影矩阵。
    /// - `&Mat4` - 视图矩阵(G-buffer 深度归一化用)。
    /// - `&SceneLighting` - SceneLighting 的只读引用。
    /// - `Vec3` - 相机眼点世界坐标。
    /// - `u32` - 画布宽度(像素)。
    /// - `u32` - 画布高度(像素)。
    /// - `f32` - 本帧的近处遮挡剔除半径(米):第三人称与自由观察不同。
    /// - `f32` - 垂直视场角(弧度)。
    /// - `f32` - 远裁剪面距离(米)。
    /// - `f32` - 本帧时间(秒,胶片颗粒种子)。
    /// - `Vec3` - 阴影 frustum 的中心(世界坐标)。
    /// - `QualityTier` - 当前画质档位,决定是否跑 SSAO / SSR。
    ///
    /// # Returns
    ///
    /// - `Result<u32, String>` - 绘制的三角形数。
    pub fn render(
        &mut self,
        scene: &Scene,
        view_proj: &Mat4,
        view: &Mat4,
        lighting: &SceneLighting,
        eye: Vec3,
        width: u32,
        height: u32,
        near_cull_radius: f32,
        fov_y: f32,
        far: f32,
        time: f32,
        shadow_focus: Vec3,
        quality: QualityTier,
    ) -> Result<u32, String> {
        // `WebGl2RenderingContext` 是 Clone 的 JS handle:克隆一份让
        // `context` 独立于 `&mut self`,这样 `draw_batch(&mut self, ..)`
        // 不会和 context 的不可变借用冲突。
        let context: WebGl2RenderingContext = self.get_context();
        self.ensure_targets(width, height)?;
        let aspect: f32 = if height == 0 {
            1.0
        } else {
            width as f32 / height as f32
        };
        let hidden: Vec<usize> = crate::game::hidden_batches();
        let light_matrix: Mat4 = shadow_view_projection(shadow_focus, lighting.light_dir);

        // ---- 1) 阴影 pass ----
        if let Some(shadow) = self.get_targets().shadow.as_ref() {
            RenderTarget::bind(&context, Some(shadow));
            context.viewport(0, 0, shadow.width, shadow.height);
            // 阴影贴图要从 1.0 清到「最远」:正交投影下深度是线性的,
            // 清成 1.0 意味着「这里什么都没有」,PCF 才会判全亮。
            context.clear_color(1.0, 1.0, 1.0, 1.0);
            context.clear_depth(1.0);
            context.clear(
                WebGl2RenderingContext::COLOR_BUFFER_BIT | WebGl2RenderingContext::DEPTH_BUFFER_BIT,
            );
            context.enable(WebGl2RenderingContext::DEPTH_TEST);
            context.depth_func(WebGl2RenderingContext::LESS);
            context.disable(WebGl2RenderingContext::BLEND);
            // 阴影 pass 要画**背面**:正面被挡住时用背面的深度当遮挡体,
            // peter-panning / 痤疮都少一个量级(front-face culling 是
            // 阴影贴图最经典的一招,对薄墙场景收益尤其大)。
            context.enable(WebGl2RenderingContext::CULL_FACE);
            context.cull_face(WebGl2RenderingContext::FRONT);
            context.use_program(Some(&self.get_shadow_program()));
            context.uniform_matrix4fv_with_f32_array(
                self.get_uniform_shadow_view_proj(),
                false,
                &light_matrix.elements,
            );
            for (index, batch) in scene.batches.iter().enumerate() {
                if !batch.opaque || batch.instances.is_empty() || hidden.contains(&index) {
                    continue;
                }
                // 阴影 pass 只画 frustum 内的实例:整座城市每帧都往
                // 2048² 的阴影贴图上提交顶点,而阴影 frustum 只覆盖
                // 玩家周围 `SHADOW_HALF_EXTENT` 米 —— 视锥外那些实例
                // 画上去的深度**永远不会被采样到**,是纯粹的浪费。
                //
                // 逐实例剔除而不是整批跳过的原因:一排行道树 / 一排路灯
                // 往往跨在视锥边界上,整批丢会把还在范围内的影子也弄没。
                let visible: Vec<Instance> = batch
                    .instances
                    .iter()
                    .copied()
                    .filter(|inst: &Instance| {
                        instance_affects_shadow(inst, shadow_focus, lighting.light_dir)
                    })
                    .collect();
                if visible.is_empty() {
                    continue;
                }
                // 阴影 pass 不做近处剔除:被剔掉的实例如果还留着影子,
                // 地面上会出现一块「无中生有」的暗斑。
                self.draw_batch(batch.mesh_index, &visible);
            }
        }

        // ---- 2) G-buffer pass ----
        if let Some(gbuffer) = self.get_targets().gbuffer.as_ref() {
            RenderTarget::bind(&context, Some(gbuffer));
            context.viewport(0, 0, gbuffer.width, gbuffer.height);
            context.clear_color(0.5, 0.5, 1.0, 1.0);
            context.clear(
                WebGl2RenderingContext::COLOR_BUFFER_BIT | WebGl2RenderingContext::DEPTH_BUFFER_BIT,
            );
            context.enable(WebGl2RenderingContext::DEPTH_TEST);
            context.depth_func(WebGl2RenderingContext::LESS);
            context.disable(WebGl2RenderingContext::BLEND);
            context.enable(WebGl2RenderingContext::CULL_FACE);
            context.cull_face(WebGl2RenderingContext::BACK);
            context.use_program(Some(&self.get_gbuffer_program()));
            context.uniform_matrix4fv_with_f32_array(
                self.get_uniform_gbuffer_view(),
                false,
                &view.elements,
            );
            context.uniform1f(self.get_uniform_gbuffer_far(), far);
            for (index, batch) in scene.batches.iter().enumerate() {
                if !batch.opaque || batch.instances.is_empty() || hidden.contains(&index) {
                    continue;
                }
                self.draw_batch(batch.mesh_index, &batch.instances);
            }
        }

        // ---- 3) 主 pass(阴影 + AO + SSR 全部在这一个 program 里)----
        let scene_target: &RenderTarget = self
            .targets
            .scene
            .as_ref()
            .ok_or_else(|| SCENE_TARGET_MISSING.to_string())?;
        RenderTarget::bind(&context, Some(scene_target));
        context.viewport(0, 0, width as i32, height as i32);
        context.clear_color(
            lighting.sky_color[0],
            lighting.sky_color[1],
            lighting.sky_color[2],
            1.0,
        );
        context.clear(
            WebGl2RenderingContext::COLOR_BUFFER_BIT | WebGl2RenderingContext::DEPTH_BUFFER_BIT,
        );

        // 不透明物体:关混合,正常深度测试。
        //
        // **`depth_mask(true)` 必须每帧显式打开。** 泛光 pass 结束时用
        // `depth_mask(false)` 关掉了深度写入却没有恢复:从第二帧起整幅
        // 深度缓冲就再也写不进去,深度测试对每一对重叠面都判成
        // `LEQUAL` 通过,于是「最后画的那个批次」覆盖掉整屏 —— 第三人称
        // 贴着地面看过去,整帧只剩一两种颜色,整座城市(和角色)全被抹掉。
        // 自由观察机位在 200 m 外,批次之间很少重叠,所以看不出来。
        context.depth_mask(true);
        context.enable(WebGl2RenderingContext::DEPTH_TEST);
        context.disable(WebGl2RenderingContext::BLEND);
        context.enable(WebGl2RenderingContext::CULL_FACE);
        context.cull_face(WebGl2RenderingContext::BACK);
        context.use_program(Some(&self.get_program()));
        context.uniform_matrix4fv_with_f32_array(
            self.get_uniform_view_proj(),
            false,
            &view_proj.elements,
        );
        context.uniform3f(
            self.get_uniform_light_dir(),
            lighting.light_dir[0],
            lighting.light_dir[1],
            lighting.light_dir[2],
        );
        context.uniform3f(
            self.get_uniform_light_color(),
            lighting.light_color[0],
            lighting.light_color[1],
            lighting.light_color[2],
        );
        context.uniform3f(
            self.get_uniform_ambient(),
            lighting.ambient[0],
            lighting.ambient[1],
            lighting.ambient[2],
        );
        context.uniform3f(
            self.get_uniform_sky_color(),
            lighting.sky_color[0],
            lighting.sky_color[1],
            lighting.sky_color[2],
        );
        context.uniform1f(self.get_uniform_emissive_gain(), lighting.emissive_gain);
        context.uniform2f(self.get_uniform_fog(), lighting.fog_start, lighting.fog_end);
        context.uniform3f(self.get_uniform_eye(), eye[0], eye[1], eye[2]);
        context.uniform1f(self.get_uniform_exposure(), lighting.exposure);
        context.uniform1f(self.get_uniform_tone_map_white(), lighting.tone_map_white);
        // 阴影 / 半球 / AO / SSR 的 uniform。
        context.uniform_matrix4fv_with_f32_array(
            self.get_uniform_shadow_matrix(),
            false,
            &light_matrix.elements,
        );
        context.uniform4f(
            self.get_uniform_shadow_params(),
            lighting.shadow_strength,
            SHADOW_PCF_RADIUS,
            shadow_depth_bias_texels(),
            shadow_normal_offset_texels(),
        );
        context.uniform1f(self.get_uniform_shadow_texel(), shadow_texel_world_size());
        context.uniform3f(
            self.get_uniform_sky_ambient(),
            lighting.sky_ambient[0],
            lighting.sky_ambient[1],
            lighting.sky_ambient[2],
        );
        context.uniform3f(
            self.get_uniform_ground_ambient(),
            lighting.ground_ambient[0],
            lighting.ground_ambient[1],
            lighting.ground_ambient[2],
        );
        context.uniform1f(self.get_uniform_ambient_hemi(), lighting.ambient_hemi);
        context.uniform1f(self.get_uniform_ao_strength(), lighting.ao_strength);
        context.uniform1f(self.get_uniform_ssr_strength(), lighting.ssr_strength);
        if let Some(shadow) = self.get_targets().shadow.as_ref() {
            Self::bind_sampler(&context, &shadow.depth, 0, self.get_uniform_shadow_map());
        }
        if let Some(ao) = self.get_targets().ao.as_ref() {
            Self::bind_sampler(
                &context,
                ao.color.as_ref().unwrap_or(&ao.depth),
                1,
                self.get_uniform_ao_map(),
            );
        }
        if let Some(ssr) = self.get_targets().ssr.as_ref() {
            Self::bind_sampler(
                &context,
                ssr.color.as_ref().unwrap_or(&ssr.depth),
                2,
                self.get_uniform_ssr_map(),
            );
        }
        // 湿度:地面越低越湿(路面 y=0 湿、人行道以上干)。
        context.uniform1f(self.get_uniform_wetness(), lighting.wetness);
        context.uniform1f(self.get_uniform_wet_height(), WET_SURFACE_MAX_HEIGHT);
        // 顶点烘焙接触 AO 的形状参数。
        context.uniform1f(self.get_uniform_ao_height(), BAKED_CONTACT_AO_HEIGHT);
        context.uniform1f(self.get_uniform_ao_floor(), CONTACT_SHADOW_FLOOR);

        let mut drawn_triangles: u32 = 0;
        for (index, batch) in scene.batches.iter().enumerate() {
            if !batch.opaque || batch.instances.is_empty() || hidden.contains(&index) {
                continue;
            }
            // 近处遮挡剔除(见 `NEAR_CULL_RADIUS` 的说明)。
            // `near_cull == false` 的批次(玩家骨架、车辆)完全豁免。
            if batch.near_cull {
                let all_near: bool = batch
                    .instances
                    .iter()
                    .all(|inst: &Instance| instance_distance(inst, eye) < near_cull_radius);
                if all_near {
                    continue;
                }
                if batch
                    .instances
                    .iter()
                    .any(|inst: &Instance| instance_distance(inst, eye) < near_cull_radius)
                {
                    // 整批都在近平面之外(常见情况:一排路灯 / 一排行道树),
                    // 就地过滤一次,避免为了剔一两个实例而重新分配。
                    let mut kept: Vec<Instance> = std::mem::take(self.get_scratch_mut());
                    kept.clear();
                    kept.extend(
                        batch
                            .instances
                            .iter()
                            .filter(|inst: &&Instance| {
                                instance_distance(inst, eye) >= near_cull_radius
                            })
                            .copied(),
                    );
                    let tris: u32 = self.draw_batch(batch.mesh_index, &kept);
                    self.set_scratch(kept);
                    drawn_triangles += tris;
                    continue;
                }
            }
            drawn_triangles += self.draw_batch(batch.mesh_index, &batch.instances);
        }

        // ---- 4) 泛光:带 emissive 的面再叠一遍,加法混合 + 半透明 ----
        // 这一 pass 画进**主场景目标**,让 bloom 的亮度提取能看到霓虹。
        if lighting.emissive_gain > 0.25 {
            context.enable(WebGl2RenderingContext::BLEND);
            context.blend_func(
                WebGl2RenderingContext::SRC_ALPHA,
                WebGl2RenderingContext::ONE,
            );
            context.depth_mask(false);
            context.use_program(Some(&self.get_glow_program()));
            context.uniform_matrix4fv_with_f32_array(
                self.get_uniform_view_proj(),
                false,
                &view_proj.elements,
            );
            context.uniform1f(self.get_uniform_glow_strength(), 0.42);
            // 这里必须复用主循环的可见性判定:否则上一轮被剔除掉的
            // 近处实例(以及 `?hide=` 掉的批次)会在泛光 pass 里复活,
            // 表现为一层盖住半屏的加法混合亮片。
            for (index, batch) in scene.batches.iter().enumerate() {
                if !batch.opaque || batch.instances.is_empty() || hidden.contains(&index) {
                    continue;
                }
                let all_near: bool = !batch.near_cull
                    || batch
                        .instances
                        .iter()
                        .all(|inst: &Instance| instance_distance(inst, eye) < near_cull_radius);
                if all_near {
                    continue;
                }
                let any_near: bool = batch
                    .instances
                    .iter()
                    .any(|inst: &Instance| instance_distance(inst, eye) < near_cull_radius);
                if any_near && batch.near_cull {
                    let mut kept: Vec<Instance> = std::mem::take(self.get_scratch_mut());
                    kept.clear();
                    kept.extend(
                        batch
                            .instances
                            .iter()
                            .filter(|inst: &&Instance| {
                                instance_distance(inst, eye) >= near_cull_radius
                            })
                            .copied(),
                    );
                    self.draw_batch(batch.mesh_index, &kept);
                    self.set_scratch(kept);
                    continue;
                }
                self.draw_batch(batch.mesh_index, &batch.instances);
            }
            context.depth_mask(true);
            context.disable(WebGl2RenderingContext::BLEND);
        }

        // ---- 5) SSAO / SSR / bloom / 合成 ----
        // 按画质档位跳过:SSAO / SSR 读两张全屏纹理再各写一张,
        // 在软渲染上它们合起来能吃掉一半的帧预算。跳过后必须把对应
        // 目标清成「无 AO / 无反射」的中性值,否则会采样到上一档残留的
        // 贴图 —— 降档的那一帧会突然出现一片脏污。
        if quality.wants_ssao() {
            self.render_ao(&context, fov_y, aspect, far);
        } else {
            self.clear_to_neutral(&context, &self.get_targets().ao, NEUTRAL_AO);
        }
        if quality.wants_ssr() {
            self.render_ssr(&context, fov_y, aspect, far);
        } else {
            self.clear_to_neutral(&context, &self.get_targets().ssr, NEUTRAL_SSR);
        }
        self.render_bloom(&context);
        self.render_composite(&context, lighting, width, height, time);
        Ok(drawn_triangles)
    }

    /// 一次 instanced draw call。
    ///
    /// 只接受 `mesh_index`,mesh handle 在函数内部查表取,这样调用方
    /// 不必持有 `&self.meshes` 的不可变借用(否则与 `&mut self` 冲突)。
    /// 一次 instanced draw call。
    ///
    /// 只接受 `mesh_index`,mesh handle 在函数内部查表取,这样调用方
    /// 不必持有 `&self.meshes` 的不可变借用(否则与 `&mut self` 冲突)。
    ///
    /// # Arguments
    ///
    /// - `usize` - 输入值。
    /// - `&[Instance]` - [Instance] 的只读引用。
    ///
    /// # Returns
    ///
    /// - `u32` - 计数结果。
    ///   一次 instanced draw call。
    ///
    /// 只接受 `mesh_index`,mesh handle 在函数内部查表取,这样调用方
    /// 不必持有 `&self.meshes` 的不可变借用(否则与 `&mut self` 冲突)。
    /// 一次 instanced draw call。
    ///
    /// 只接受 `mesh_index`,mesh handle 在函数内部查表取,这样调用方
    /// 不必持有 `&self.meshes` 的不可变借用(否则与 `&mut self` 冲突)。
    ///
    /// # Arguments
    ///
    /// - `usize` - 输入值。
    /// - `&[Instance]` - [Instance] 的只读引用。
    ///
    /// # Returns
    ///
    /// - `u32` - 计数结果。
    fn draw_batch(&mut self, mesh_index: usize, instances: &[Instance]) -> u32 {
        let context: WebGl2RenderingContext = self.get_context();
        let count: usize = instances.len();
        if count == 0 {
            return 0;
        }
        self.reserve_instances(count);
        let index_count: i32 = match self.get_meshes().get(mesh_index) {
            Some(mesh) => mesh.index_count,
            None => return 0,
        };
        // **验收探针:记下 GPU 表长度与本批次的索引数。**
        //
        // 「批次有实例、模型矩阵正确、画面上却没有角色」只剩一种解释:
        // `mesh_index` 越界或错位,`draw_batch` 在 `get_meshes().get()`
        // 处静默 `return 0` —— 那条路径**一个三角形都不画,也不报错**。
        // 验收脚本据此核对 GPU 表与 `Scene::meshes` 是否一一对应。
        let table_len: usize = self.get_meshes().len();
        self.set_gpu_mesh_count(table_len);
        self.set_gpu_index_count(index_count);
        if mesh_index >= table_len {
            let _: usize = self.set_gpu_mesh_index_oob_bumped();
        }
        let (vertex_array, index_buffer): (WebGlVertexArrayObject, WebGlBuffer) =
            match self.get_meshes().get(mesh_index) {
                Some(mesh) => (mesh.vertex_array.clone(), mesh.index_buffer.clone()),
                None => return 0,
            };
        // 打包 instance 数据:每实例 20 f32 = model(16) + tint(3) + pad(1)。
        //
        // ⚠️ pad 是**必需的**,不能省:VAO 里实例属性的 stride 是
        // `INSTANCE_STRIDE_BYTES`(= FLOATS_PER_INSTANCE * 4),GPU 按 stride 切分。
        // model(16) + tint(3) 一共 19 个 f32,少写 1 个会让第 2 个及以后的实例
        // 整体前移一个 f32 读到错位的 model matrix —— 表现为一堆从原点放射
        // 出来的巨大三角形楔形糊满屏幕(单实例批次不会触发,所以更难发现)。
        let mut data: Vec<f32> = Vec::with_capacity(count * FLOATS_PER_INSTANCE);
        for instance in instances {
            debug_assert_eq!(FLOATS_PER_INSTANCE, 20);
            let before: usize = data.len();
            data.extend_from_slice(&instance.model);
            data.extend_from_slice(&instance.tint);
            // 补齐到 FLOATS_PER_INSTANCE:model 16 + tint 3 + 1 个 pad。
            data.resize(before + FLOATS_PER_INSTANCE, 0.0);
        }
        debug_assert_eq!(data.len(), count * FLOATS_PER_INSTANCE);
        // **`bind_vertex_array` 必须排在 `bind_buffer` 前面。**
        //
        // VAO 里存着实例属性的 `vertexAttribPointer` —— 也就是「这个
        // attribute 从 instance buffer 的第几字节读」。那个 pointer 是
        // 在 `upload_mesh` 时、对着**当时绑定的 buffer** 记下来的,而且
        // `ELEMENT_ARRAY_BUFFER` 的绑定也是 VAO 状态的一部分。
        // 先 `bind_buffer(ARRAY_BUFFER)` 再 `bind_vertex_array`,等于
        // 先把全局 ARRAY_BUFFER 指针挪走、再让 VAO 接管:这一次
        // `buffer_sub_data` 写进去的 instance 数据没有任何 VAO 的
        // attribute pointer 指向它,GPU 读到的是上一次残留的内容。
        // 对多实例批次只是偶尔错位,对每帧只写 1 个实例的**玩家骨架**
        // 尤其致命:13 个 limb 批次共用一个 instance buffer,残留的
        // model matrix 会被当成角色的矩阵用,结果是一堆从原点放射的
        // 巨大楔形三角形糊满整屏(实测整屏被涂成单色 `135,179,190`,
        // 隐藏任意一个 limb 批次画面就恢复)。
        context.bind_vertex_array(Some(&vertex_array));
        context.bind_buffer(
            WebGl2RenderingContext::ARRAY_BUFFER,
            Some(&self.get_instance_buffer()),
        );
        context.buffer_sub_data_with_i32_and_u8_array(
            WebGl2RenderingContext::ARRAY_BUFFER,
            0,
            f32_slice_to_bytes(&data),
        );
        context.bind_buffer(
            WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER,
            Some(&index_buffer),
        );
        context.draw_elements_instanced_with_i32(
            WebGl2RenderingContext::TRIANGLES,
            index_count,
            WebGl2RenderingContext::UNSIGNED_INT,
            0,
            count as i32,
        );
        context.bind_vertex_array(None);
        (index_count / 3) as u32 * count as u32
    }
}

/// 小 helper:把 `get_context` 返回的 `js_sys::Object` 转成 `WebGl2RenderingContext`。
trait DynIntoWebGl {
    /// dyn into webgl。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `WebGl2RenderingContext` - 计算结果。
    ///   dyn into webgl。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `WebGl2RenderingContext` - 计算结果。
    fn dyn_into_webgl(self) -> WebGl2RenderingContext;
}

impl DynIntoWebGl for euv::js_sys::Object {
    /// dyn into webgl。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `WebGl2RenderingContext` - 计算结果。
    ///   dyn into webgl。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `WebGl2RenderingContext` - 计算结果。
    fn dyn_into_webgl(self) -> WebGl2RenderingContext {
        use euv::wasm_bindgen::JsCast;
        // 先绑定到局部变量再转型:`unchecked_into::<T>()` 的 turbofish
        // 写法会让「self.field」静态检查把 `unchecked_into` 误判成字段名,
        // 局部绑定让接收者不再是 `self`,语义不变但能通过校验。
        let source: euv::js_sys::Object = self;
        source.unchecked_into::<WebGl2RenderingContext>()
    }
}

// ===========================================================================
// Canvas2D 软件渲染后端
// ===========================================================================

/// 一个待绘制的屏幕空间三角形。
#[derive(Clone, Debug)]
struct ScreenTriangle {
    /// 三个屏幕顶点 `(x, y)`。
    points: [(f32, f32); 3],
    /// 归一化深度(越小越近)。
    depth: f32,
    /// 填充色(sRGB 量化后)。
    fill: String,
}

/// Canvas2D 软件渲染后端(WebGL 不可用时的回退)。
///
/// 管线:变换顶点 → 世界空间背面剔除 → 投影到屏幕 → 画家算法深度排序
/// → 逐三角形填充。与 WebGL 后端共用 [`shade_face`],因此画面配色一致。
pub struct SoftwareRenderer {
    context: euv::web_sys::CanvasRenderingContext2d,
}

impl SoftwareRenderer {
    /// 2D 上下文的克隆句柄。
    ///
    /// # Returns
    ///
    /// - `euv::web_sys::CanvasRenderingContext2d` - 2D 上下文句柄。
    pub fn get_context(&self) -> euv::web_sys::CanvasRenderingContext2d {
        self.context.clone()
    }

    /// 在 canvas 上创建 2D 上下文。
    ///
    /// # Arguments
    ///
    /// - `&HtmlCanvasElement` - HtmlCanvasElement 的只读引用。
    ///
    /// # Returns
    ///
    /// - `Result<Self, String>` - 计算结果。
    pub fn new(canvas: &HtmlCanvasElement) -> Result<Self, String> {
        let context: euv::web_sys::CanvasRenderingContext2d = canvas
            .get_context("2d")
            .map_err(|err: JsValue| format!("get_context threw: {err:?}"))?
            .ok_or_else(|| CANVAS2D_UNAVAILABLE.to_string())?
            .dyn_into_2d();
        Ok(Self { context })
    }

    /// 渲染一帧。
    ///
    /// # Arguments
    ///
    /// - `&Scene` - Scene 的只读引用。
    /// - `&crate::camera::Camera` - crate::camera::Camera 的只读引用。
    /// - `&SceneLighting` - SceneLighting 的只读引用。
    /// - `u32` - 输入值。
    /// - `usize` - 输入值。
    ///
    /// # Returns
    ///
    /// - `u32` - 计数结果。
    pub fn render(
        &self,
        scene: &Scene,
        camera: &crate::camera::Camera,
        lighting: &SceneLighting,
        width: u32,
        height: u32,
        max_triangles: usize,
    ) -> u32 {
        // 克隆一份上下文句柄,让后续绘制代码不必反复访问 `self`。
        let context: euv::web_sys::CanvasRenderingContext2d = self.get_context();
        let width_f: f32 = width as f32;
        let height_f: f32 = height as f32;
        let background: Rgb8 = srgb_to_u8(linear_to_srgb(lighting.sky_color));
        context.clear_rect(0.0, 0.0, width_f as f64, height_f as f64);
        context.set_fill_style_str(&format!(
            "rgb({}, {}, {})",
            background[0], background[1], background[2]
        ));
        context.fill_rect(0.0, 0.0, width_f as f64, height_f as f64);
        context.set_global_alpha(1.0);

        let eye: [f32; 3] = camera.eye();
        let mut queue: Vec<ScreenTriangle> = Vec::with_capacity(max_triangles.min(65_536));

        for batch in &scene.batches {
            let Some(mesh) = scene.meshes.get(batch.mesh_index) else {
                continue;
            };
            for instance in &batch.instances {
                for face in &mesh.faces {
                    if queue.len() >= max_triangles {
                        break;
                    }
                    let world: [[f32; 3]; 3] = [
                        instance.transform_point(vertex_position(mesh, face.indices[0])),
                        instance.transform_point(vertex_position(mesh, face.indices[1])),
                        instance.transform_point(vertex_position(mesh, face.indices[2])),
                    ];
                    // 世界空间背面剔除。
                    if is_back_facing(world[0], world[1], world[2], eye) {
                        continue;
                    }
                    let mut projected: [(f32, f32, f32); 3] = [(0.0, 0.0, 0.0); 3];
                    let mut all_in_front: bool = true;
                    for corner in 0..3 {
                        match camera.world_to_screen(world[corner], width_f, height_f) {
                            Some((x, y, depth)) => {
                                projected[corner] = (x, y, depth);
                            }
                            None => {
                                all_in_front = false;
                                break;
                            }
                        }
                    }
                    if !all_in_front {
                        continue;
                    }
                    let normal: [f32; 3] = instance.transform_normal(face.normal);
                    // 面中心到眼点的距离,喂给与 GPU 端同款的雾。
                    let centroid: [f32; 3] = [
                        (world[0][0] + world[1][0] + world[2][0]) / 3.0,
                        (world[0][1] + world[1][1] + world[2][1]) / 3.0,
                        (world[0][2] + world[1][2] + world[2][2]) / 3.0,
                    ];
                    let eye_distance: f32 = {
                        let dx: f32 = centroid[0] - eye[0];
                        let dy: f32 = centroid[1] - eye[1];
                        let dz: f32 = centroid[2] - eye[2];
                        (dx * dx + dy * dy + dz * dz).sqrt()
                    };
                    let color: [f32; 3] = shade_face(
                        face.color,
                        face.emissive,
                        normal,
                        instance.tint,
                        lighting,
                        eye_distance,
                    );
                    let rgb: [u8; 3] = srgb_to_u8(color);
                    let depth: f32 = (projected[0].2 + projected[1].2 + projected[2].2) / 3.0;
                    queue.push(ScreenTriangle {
                        points: [
                            (projected[0].0, projected[0].1),
                            (projected[1].0, projected[1].1),
                            (projected[2].0, projected[2].1),
                        ],
                        depth,
                        fill: format!("rgb({}, {}, {})", rgb[0], rgb[1], rgb[2]),
                    });
                }
            }
        }

        // 画家算法:远 → 近。
        queue.sort_by(|a: &ScreenTriangle, b: &ScreenTriangle| {
            b.depth
                .partial_cmp(&a.depth)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let count: u32 = queue.len() as u32;
        let mut previous: &str = "";
        for triangle in &queue {
            if triangle.fill != previous {
                context.set_fill_style_str(&triangle.fill);
                previous = triangle.fill.as_str();
            }
            context.begin_path();
            context.move_to(triangle.points[0].0 as f64, triangle.points[0].1 as f64);
            context.line_to(triangle.points[1].0 as f64, triangle.points[1].1 as f64);
            context.line_to(triangle.points[2].0 as f64, triangle.points[2].1 as f64);
            context.close_path();
            context.fill();
        }
        count
    }
}

/// 小 helper:把 `get_context` 返回的 `js_sys::Object` 转成 `CanvasRenderingContext2d`。
trait DynInto2d {
    /// dyn into 2d。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `euv::web_sys::CanvasRenderingContext2d` - 计算结果。
    ///   dyn into 2d。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `euv::web_sys::CanvasRenderingContext2d` - 计算结果。
    fn dyn_into_2d(self) -> euv::web_sys::CanvasRenderingContext2d;
}

impl DynInto2d for euv::js_sys::Object {
    /// dyn into 2d。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `euv::web_sys::CanvasRenderingContext2d` - 计算结果。
    ///   dyn into 2d。
    ///
    /// # Arguments
    ///
    ///
    /// # Returns
    ///
    /// - `euv::web_sys::CanvasRenderingContext2d` - 计算结果。
    fn dyn_into_2d(self) -> euv::web_sys::CanvasRenderingContext2d {
        use euv::wasm_bindgen::JsCast;
        // 同 `dyn_into_webgl`:局部绑定接收者,避免 turbofish 写法被
        // 「self.field」静态检查误判成字段访问。
        let source: euv::js_sys::Object = self;
        source.unchecked_into::<euv::web_sys::CanvasRenderingContext2d>()
    }
}

/// 读取一个顶点的世界坐标(本地坐标)。
///
/// # Arguments
///
/// - `&MeshAssetGpu` - MeshAssetGpu 的只读引用。
/// - `u32` - 输入值。
///
/// # Returns
///
/// - `Vec3` - 计算结果。
///   读取一个顶点的世界坐标(本地坐标)。
///
/// # Arguments
///
/// - `&MeshAssetGpu` - MeshAssetGpu 的只读引用。
/// - `u32` - 输入值。
///
/// # Returns
///
/// - `Vec3` - 计算结果。
fn vertex_position(mesh: &MeshAssetGpu, index: u32) -> Vec3 {
    let base: usize = (index as usize) * STRIDE_FLOATS;
    [
        mesh.vertices[base],
        mesh.vertices[base + 1],
        mesh.vertices[base + 2],
    ]
}

// ===========================================================================
// 后端枚举
// ===========================================================================

/// 实际使用的渲染后端。
pub enum Renderer {
    /// WebGL2(GLSL ES 3.00,instancing)。
    WebGl(Box<WebGlRenderer>),
    /// Canvas2D 软件渲染(背面剔除 + 画家算法)。
    Software(SoftwareRenderer),
}

impl Renderer {
    /// 后端名字(显示在 HUD 上)。
    ///
    /// # Returns
    ///
    /// - `&'static str` - 计算结果。
    pub fn backend_name(&self) -> &'static str {
        match self {
            Renderer::WebGl(_) => WEBGL2,
            Renderer::Software(_) => CANVAS2D,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::r#const::{
        SHADOW_CULL_MARGIN, SHADOW_GROUNDED_EPS, SHADOW_HALF_EXTENT,
        T_SHADOW_CULLS_OUTSIDE, T_SHADOW_GROUNDED_SAFE, T_SHADOW_KEEPS_CENTRE,
        T_SHADOW_KEEPS_MARGIN, T_SHADOW_TALL_KEPT,
    };
    use crate::render::{Instance, instance_affects_shadow, normalize3};
    use crate::r#type::Vec3;

    /// 把断言文案里的 `{名字}` 占位符替换成实际数值。
    fn fill(template: &str, args: &[(&str, &str)]) -> String {
        let mut out: String = template.to_string();
        let mut index: usize = 0;
        while index < args.len() {
            out = out.replace(&format!("{{{}}}", args[index].0), args[index].1);
            index += 1;
        }
        out
    }

    /// 在给定位置造一个单位缩放的实例。
    fn at(position: Vec3) -> Instance {
        Instance::new(position, 0.0, 1.0, [1.0, 1.0, 1.0])
    }

    /// 游戏正午的真实光照方向(`render.rs` 里 `NOON` 那一档)。
    fn noon_light() -> Vec3 {
        normalize3([0.35, 0.86, 0.36])
    }

    /// 判据的有效半径:半宽 + 余量。
    fn reach() -> f32 {
        SHADOW_HALF_EXTENT + SHADOW_CULL_MARGIN
    }

    /// frustum 正中心的实例必须保留 —— 否则玩家脚下会没有影子。
    #[test]
    fn shadow_cull_keeps_frustum_centre() {
        let focus: Vec3 = [12.0, 0.0, -34.0];
        let kept: bool = instance_affects_shadow(&at(focus), focus, noon_light());
        assert!(
            kept,
            "{}",
            fill(
                T_SHADOW_KEEPS_CENTRE,
                &[("dist", "0.00"), ("reach", &format!("{:.2}", reach()))]
            )
        );
    }

    /// 远在 frustum 之外的实例必须被剔除 —— 这正是优化要的收益。
    #[test]
    fn shadow_cull_drops_far_instances() {
        let focus: Vec3 = [0.0, 0.0, 0.0];
        // 2 倍有效半径之外,肯定不可能有任何影子落进视锥。
        let far: Vec3 = [reach() * 2.0, 0.0, 0.0];
        let kept: bool = instance_affects_shadow(&at(far), focus, noon_light());
        assert!(
            !kept,
            "{}",
            fill(
                T_SHADOW_CULLS_OUTSIDE,
                &[
                    ("dist", &format!("{:.2}", reach() * 2.0)),
                    ("reach", &format!("{:.2}", reach()))
                ]
            )
        );
    }

    /// 余量之内(视锥之外一点点)必须仍然保留:余量就是防「影子凭空消失」。
    #[test]
    fn shadow_cull_keeps_inside_margin() {
        let focus: Vec3 = [0.0, 0.0, 0.0];
        // 落在半宽与「半宽 + 余量」之间。
        let edge: Vec3 = [SHADOW_HALF_EXTENT + SHADOW_CULL_MARGIN * 0.5, 0.0, 0.0];
        let kept: bool = instance_affects_shadow(&at(edge), focus, noon_light());
        assert!(
            kept,
            "{}",
            fill(
                T_SHADOW_KEEPS_MARGIN,
                &[
                    ("dist", &format!("{:.2}", SHADOW_HALF_EXTENT + SHADOW_CULL_MARGIN * 0.5)),
                    ("reach", &format!("{:.2}", reach()))
                ]
            )
        );
    }

    /// 贴地光(`light_dir.y` ≈ 0)不得除零,也不得把视锥内的实例剔掉。
    #[test]
    fn shadow_cull_survives_grounded_light() {
        let focus: Vec3 = [0.0, 0.0, 0.0];
        let flat: Vec3 = normalize3([1.0, 0.0, 0.0]);
        let kept: bool = instance_affects_shadow(&at([1.0, 0.0, 0.0]), focus, flat);
        assert!(
            kept && flat[1].abs() <= SHADOW_GROUNDED_EPS,
            "{}",
            fill(
                T_SHADOW_GROUNDED_SAFE,
                &[
                    ("ly", &format!("{:.6}", flat[1].abs())),
                    ("dist", "1.00"),
                    ("reach", &format!("{:.2}", reach()))
                ]
            )
        );
    }

    /// 斜光下的高楼:影子被拉得很远,但只要落点还在视锥内就必须画。
    ///
    /// 这是光空间投影存在的理由 —— 只比「到 focus 的水平距离」的话,
    /// 这栋楼会被误剔,地面上凭空少一块长影子。
    #[test]
    fn shadow_cull_keeps_tall_building_under_oblique_light() {
        let focus: Vec3 = [0.0, 0.0, 0.0];
        // 游戏里真实存在的黄昏光方向(`DUSK` 那一档)。
        let dusk: Vec3 = normalize3([0.86, 0.24, -0.44]);
        let height: f32 = 40.0;
        // 楼刚好立在 frustum 边缘外一点,但斜光会把影子拉回视锥内。
        let building: Vec3 = [SHADOW_HALF_EXTENT + 4.0, height, 0.0];
        let kept: bool = instance_affects_shadow(&at(building), focus, dusk);
        // 落点 = center + light_dir_xz * ((focus.y - center.y) / light_dir.y)
        let along: f32 = (focus[1] - building[1]) / dusk[1];
        let dist: f32 = ((building[0] + dusk[0] * along - focus[0]).powi(2)
            + (building[2] + dusk[2] * along - focus[2]).powi(2))
        .sqrt();
        assert!(
            kept,
            "{}",
            fill(
                T_SHADOW_TALL_KEPT,
                &[
                    ("height", &format!("{height:.0}")),
                    ("dist", &format!("{dist:.2}")),
                    ("reach", &format!("{:.2}", reach()))
                ]
            )
        );
    }
}
