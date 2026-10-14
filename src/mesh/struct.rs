use super::*;
/// 资产顶层结构。
#[derive(Data, Clone, Debug, Deserialize)]
pub struct MeshAsset {
    /// 资产 id。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) id: String,
    /// 资产分类(vehicle / building / prop / character ...)。
    #[serde(default)]
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) category: String,
    /// 是否 Y 轴向上。`false` 时解析阶段会把 Z 轴换到 Y 轴。
    #[serde(default = "default_y_up")]
    #[get(type(copy), pub(crate))]
    pub(crate) y_up: bool,
    /// 包围盒(可选,仅用于调试与自动取景)。
    #[serde(default)]
    #[get(type(clone), pub(crate))]
    pub(crate) bounds: Option<Bounds>,
    /// 模型零件。
    #[serde(default)]
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) parts: Vec<MeshPart>,
}
/// 包围盒。
#[derive(Data, Clone, Debug, Deserialize)]
pub struct Bounds {
    /// 最小角。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) min: Vec3Json,
    /// 最大角。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) max: Vec3Json,
}
/// 单个零件。
#[derive(Data, Clone, Debug, Deserialize)]
pub struct MeshPart {
    /// 零件名。
    #[serde(default)]
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) name: String,
    /// 基础色(面未指定颜色时的回退值)。
    #[serde(default)]
    #[get(type(clone), pub(crate))]
    pub(crate) base_color: Option<Vec3Json>,
    /// 自发光色。
    #[serde(default)]
    #[get(type(clone), pub(crate))]
    pub(crate) emissive: Option<Vec3Json>,
    /// 顶点位置。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) positions: Vec<Vec3Json>,
    /// 顶点法线(可缺省,缺省时由面法线推导)。
    #[serde(default)]
    #[get(type(clone), pub(crate))]
    pub(crate) normals: Option<Vec<Vec3Json>>,
    /// 三角面索引,每项 3 个顶点下标。
    #[serde(default)]
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) faces: Vec<[usize; 3]>,
    /// 每个面的颜色(可缺省,缺省时用 `base_color`)。
    #[serde(default)]
    #[get(type(clone), pub(crate))]
    pub(crate) face_colors: Option<Vec<Vec3Json>>,
    /// 是否平面着色。为 true 时忽略顶点法线,统一使用面法线。
    #[serde(default = "default_true")]
    #[get(type(copy), pub(crate))]
    pub(crate) flat: bool,
    /// 每个顶点的 UV 坐标(可缺省,缺省时按面法线做三平面投影)。
    ///
    /// 缺省不是「没有 UV」而是「用程序化投影兜底」:资产管线早期不产出 UV,
    /// 若直接置零,所有零件的贴图会塌到同一个 texel 上,表现为整块纯色。
    /// [`expand_asset`] 在缺省时按面法线选主轴生成投影坐标,保证新旧资产
    /// 在同一条渲染路径上都有可用的 UV。
    #[serde(default)]
    #[get(type(clone), pub(crate))]
    pub(crate) uv: Option<Vec<[f32; 2]>>,
    /// 每个顶点的切线,`w` 为手性符号(缺省时由面法线推导)。
    #[serde(default)]
    #[get(type(clone), pub(crate))]
    pub(crate) tangents: Option<Vec<[f32; 4]>>,
    /// 表面粗糙度 0..1(缺省 [`DEFAULT_ROUGHNESS`] = 0.6,偏哑光)。
    #[serde(default)]
    #[get(type(copy), pub(crate))]
    pub(crate) roughness: Option<f32>,
    /// 金属度 0..1(缺省 [`DEFAULT_METALLIC`] = 0.0,介电质)。
    #[serde(default)]
    #[get(type(copy), pub(crate))]
    pub(crate) metallic: Option<f32>,
    /// 运行时贴图图集路径(可缺省,缺省时该零件只有标量 PBR)。
    ///
    /// 只有写入了 `atlas/<id>.bin` 的资产才会有这个字段。运行时管线经
    /// HTTP 读取 `assets/*.json`,不读 GLB,所以贴图必须在这里有一个入口,
    /// 否则第三层(图像纹理)在游戏里完全不可见。
    #[serde(default, rename = "texture_atlas")]
    #[get(type(clone), pub(crate))]
    pub(crate) texture_atlas: Option<String>,
    /// 表面族(`paint` / `metal` / `glass` ...),决定该零件用图集里的哪套贴图。
    #[serde(default, rename = "surface_kind")]
    #[get(type(clone), pub(crate))]
    pub(crate) surface_kind: Option<String>,
}
/// 一个已经展开成 GPU 布局的三角网格。
///
/// 顶点缓冲是 interleaved 的:`[px, py, pz, nx, ny, nz, r, g, b]`,共 9 个 f32。
#[derive(Data, Clone, Debug, Default)]
pub struct GpuMesh {
    /// interleaved 顶点数据(9 f32 / 顶点)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) vertices: Vec<f32>,
    /// 三角形索引(u32)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) indices: Vec<u32>,
    /// 三角形数量。
    #[get(type(copy), pub(crate))]
    pub(crate) triangle_count: usize,
    /// 展开后的世界包围盒最小角。
    #[get(type(copy), pub(crate))]
    pub(crate) min: Vec3,
    /// 展开后的世界包围盒最大角。
    #[get(type(copy), pub(crate))]
    pub(crate) max: Vec3,
    /// 每顶点的 PBR 数据:`uv(2) | tangent(4) | rough(1) | metal(1)` = 8 f32。
    ///
    /// 单列而不是并进 `vertices`,是为了不动 [`FLOATS_PER_VERTEX`]=9 的
    /// 紧凑布局 —— 那个常量被 `vertex_count`、包围盒推导和全部既有测试
    /// 共用。GPU 侧由 [`crate::render::build_gpu_mesh`] 把两列合成
    /// 20 f32 的交错缓冲。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) pbr: Vec<f32>,
}
