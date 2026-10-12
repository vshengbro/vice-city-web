//! 资产 JSON 解析模块。
//!
//! 输入是 `assets/*.json`,schema 如下(与资产生产方约定):
//!
//! ```json
//! {
//!   "id": "car_sedan",
//!   "category": "vehicle",
//!   "y_up": true,
//!   "bounds": { "min": [x,y,z], "max": [x,y,z] },
//!   "parts": [
//!     {
//!       "name": "body",
//!       "base_color": [r,g,b],
//!       "emissive": [r,g,b],
//!       "positions": [[x,y,z], ...],
//!       "normals": [[x,y,z], ...],
//!       "faces": [[i0,i1,i2], ...],
//!       "face_colors": [[r,g,b], ...],
//!       "flat": true
//!     }
//!   ]
//! }
//! ```
//!
//! 坐标单位是米、Y 轴向上、颜色 0..1,`faces` 是三角形顶点索引
//! (CCW,从外侧看逆时针)。
//!
//! 输出是 GPU 可直接使用的:
//! - **interleaved 顶点缓冲**:每个顶点 `position(3) | normal(3) | color(3)`,共 9 个 f32、36 字节
//! - **索引缓冲**:`u32` 三角形列表
//!
//! 由于要做「每面颜色 + 平面着色」,我们把索引数据**展开(de-index)**成
//! 每面 3 个独立顶点,法线取面法线、颜色取该面的 `face_colors[i]`。
//! 展开后索引缓冲退化为顺序索引,但仍然真实生成并上传,方便后续 worker
//! 在需要时改成共享顶点 / 索引优化。

use crate::{Display, r#type::Vec3};
use serde::Deserialize;

/// 一个三维向量,来自 JSON 数组 `[x, y, z]`。
pub type Vec3Json = [f32; 3];

/// 资产顶层结构。
#[derive(Clone, Debug, Deserialize)]
pub struct MeshAsset {
    /// 资产 id。
    pub id: String,
    /// 资产分类(vehicle / building / prop / character ...)。
    #[serde(default)]
    pub category: String,
    /// 是否 Y 轴向上。`false` 时解析阶段会把 Z 轴换到 Y 轴。
    #[serde(default = "default_y_up")]
    pub y_up: bool,
    /// 包围盒(可选,仅用于调试与自动取景)。
    #[serde(default)]
    pub bounds: Option<Bounds>,
    /// 模型零件。
    #[serde(default)]
    pub parts: Vec<MeshPart>,
}

/// `y_up` 缺省时按 true 处理。
///
/// # Returns
///
/// - `bool` - 恒为 `true`。
///   `y_up` 缺省时按 true 处理。
///
/// # Returns
///
/// - `bool` - 恒为 `true`。
fn default_y_up() -> bool {
    true
}

/// 包围盒。
#[derive(Clone, Debug, Deserialize)]
pub struct Bounds {
    /// 最小角。
    pub min: Vec3Json,
    /// 最大角。
    pub max: Vec3Json,
}

impl Bounds {
    /// 包围盒最小角。
    ///
    /// # Returns
    ///
    /// - `Vec3Json` - 包围盒最小角。
    pub fn get_min(&self) -> Vec3Json {
        self.min
    }

    /// 包围盒最大角。
    ///
    /// # Returns
    ///
    /// - `Vec3Json` - 包围盒最大角。
    pub fn get_max(&self) -> Vec3Json {
        self.max
    }
}

/// 单个零件。
#[derive(Clone, Debug, Deserialize)]
pub struct MeshPart {
    /// 零件名。
    #[serde(default)]
    pub name: String,
    /// 基础色(面未指定颜色时的回退值)。
    #[serde(default)]
    pub base_color: Option<Vec3Json>,
    /// 自发光色。
    #[serde(default)]
    pub emissive: Option<Vec3Json>,
    /// 顶点位置。
    pub positions: Vec<Vec3Json>,
    /// 顶点法线(可缺省,缺省时由面法线推导)。
    #[serde(default)]
    pub normals: Option<Vec<Vec3Json>>,
    /// 三角面索引,每项 3 个顶点下标。
    #[serde(default)]
    pub faces: Vec<[usize; 3]>,
    /// 每个面的颜色(可缺省,缺省时用 `base_color`)。
    #[serde(default)]
    pub face_colors: Option<Vec<Vec3Json>>,
    /// 是否平面着色。为 true 时忽略顶点法线,统一使用面法线。
    #[serde(default = "default_true")]
    pub flat: bool,
    /// 每个顶点的 UV 坐标(可缺省,缺省时按面法线做三平面投影)。
    ///
    /// 缺省不是「没有 UV」而是「用程序化投影兜底」:资产管线早期不产出 UV,
    /// 若直接置零,所有零件的贴图会塌到同一个 texel 上,表现为整块纯色。
    /// [`expand_asset`] 在缺省时按面法线选主轴生成投影坐标,保证新旧资产
    /// 在同一条渲染路径上都有可用的 UV。
    #[serde(default)]
    pub uv: Option<Vec<[f32; 2]>>,
    /// 每个顶点的切线,`w` 为手性符号(缺省时由面法线推导)。
    #[serde(default)]
    pub tangents: Option<Vec<[f32; 4]>>,
    /// 表面粗糙度 0..1(缺省 [`DEFAULT_ROUGHNESS`] = 0.6,偏哑光)。
    #[serde(default)]
    pub roughness: Option<f32>,
    /// 金属度 0..1(缺省 [`DEFAULT_METALLIC`] = 0.0,介电质)。
    #[serde(default)]
    pub metallic: Option<f32>,
    /// 运行时贴图图集路径(可缺省,缺省时该零件只有标量 PBR)。
    ///
    /// 只有写入了 `atlas/<id>.bin` 的资产才会有这个字段。运行时管线经
    /// HTTP 读取 `assets/*.json`,不读 GLB,所以贴图必须在这里有一个入口,
    /// 否则第三层(图像纹理)在游戏里完全不可见。
    #[serde(default, rename = "texture_atlas")]
    pub texture_atlas: Option<String>,
    /// 表面族(`paint` / `metal` / `glass` ...),决定该零件用图集里的哪套贴图。
    #[serde(default, rename = "surface_kind")]
    pub surface_kind: Option<String>,
}

/// `flat` 缺省时按 true(与资产 schema 一致)。
///
/// # Returns
///
/// - `bool` - 恒为 `true`。
///   `flat` 缺省时按 true(与资产 schema 一致)。
///
/// # Returns
///
/// - `bool` - 恒为 `true`。
fn default_true() -> bool {
    true
}

/// 资产解析 / 校验错误。
#[derive(Debug)]
pub enum MeshError {
    /// JSON 语法或结构错误。
    Json(serde_json::Error),
    /// schema 校验失败(缺字段、长度不匹配、索引越界 ...)。
    Schema(String),
}

impl Display for MeshError {
    /// 把错误格式化成人类可读的一行。
    ///
    /// # Arguments
    ///
    /// - `&mut std::fmt::Formatter<'_>` - 目标格式化器。
    ///
    /// # Returns
    ///
    /// - `std::fmt::Result` - `Ok(())` 表示写入成功。
    ///   把错误格式化成人类可读的一行。
    ///
    /// # Arguments
    ///
    /// - `&mut std::fmt::Formatter<'_>` - 目标格式化器。
    ///
    /// # Returns
    ///
    /// - `std::fmt::Result` - `Ok(())` 表示写入成功。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeshError::Json(err) => write!(f, "JSON parse error: {err}"),
            MeshError::Schema(msg) => write!(f, "schema error: {msg}"),
        }
    }
}

impl From<serde_json::Error> for MeshError {
    /// 把 serde_json 的解析错误包装成 `MeshError::Json`。
    ///
    /// # Arguments
    ///
    /// - `serde_json::Error` - 原始解析错误。
    ///
    /// # Returns
    ///
    /// - `Self` - 对应的 `MeshError`。
    ///   把 serde_json 的解析错误包装成 `MeshError::Json`。
    ///
    /// # Arguments
    ///
    /// - `serde_json::Error` - 原始解析错误。
    ///
    /// # Returns
    ///
    /// - `Self` - 对应的 `MeshError`。
    fn from(value: serde_json::Error) -> Self {
        MeshError::Json(value)
    }
}

/// 一个已经展开成 GPU 布局的三角网格。
///
/// 顶点缓冲是 interleaved 的:`[px, py, pz, nx, ny, nz, r, g, b]`,共 9 个 f32。
#[derive(Clone, Debug, Default)]
pub struct GpuMesh {
    /// interleaved 顶点数据(9 f32 / 顶点)。
    pub vertices: Vec<f32>,
    /// 三角形索引(u32)。
    pub indices: Vec<u32>,
    /// 三角形数量。
    pub triangle_count: usize,
    /// 展开后的世界包围盒最小角。
    pub min: Vec3,
    /// 展开后的世界包围盒最大角。
    pub max: Vec3,
    /// 每顶点的 PBR 数据:`uv(2) | tangent(4) | rough(1) | metal(1)` = 8 f32。
    ///
    /// 单列而不是并进 `vertices`,是为了不动 [`FLOATS_PER_VERTEX`]=9 的
    /// 紧凑布局 —— 那个常量被 `vertex_count`、包围盒推导和全部既有测试
    /// 共用。GPU 侧由 [`crate::render::build_gpu_mesh`] 把两列合成
    /// 20 f32 的交错缓冲。
    pub pbr: Vec<f32>,
}

impl MeshAsset {
    /// 资产声明的包围盒(导出管线写入,见 `assets/SCHEMA.md` §4)。
    ///
    /// # Returns
    ///
    /// - `Option<&Bounds>` - 资产包围盒;资产未声明时为 `None`。
    pub fn get_bounds(&self) -> Option<&Bounds> {
        self.bounds.as_ref()
    }

    /// 包围盒是否自洽(每个轴上 min <= max,且不是一个退化的点)。
    ///
    /// 资产由离线管线导出,`bounds` 与 `positions` 是同一次计算的产物
    /// (见 `assets/SCHEMA.md` §4)。加载时校验一次,可以在几何被写坏时
    /// 立刻发现,而不是等到画面上出现一个 NaN 三角形才去猜。
    ///
    /// # Returns
    ///
    /// - `bool` - 包围盒各轴方向正确且至少有一个轴非零时为 `true`。
    pub fn bounds_envelope_is_sane(&self) -> bool {
        let Some(bounds) = self.get_bounds() else {
            return true;
        };
        let min: Vec3Json = bounds.get_min();
        let max: Vec3Json = bounds.get_max();
        let ordered: bool = (0..3).all(|axis: usize| min[axis] <= max[axis]);
        let non_degenerate: bool = (0..3).any(|axis: usize| max[axis] > min[axis]);
        ordered && non_degenerate
    }
}

impl GpuMesh {
    /// 返回 interleaved 顶点数据的只读视图。
    ///
    /// # Returns
    ///
    /// - `&Vec<f32>` - 顶点数据。
    pub fn get_vertices_ref(&self) -> &Vec<f32> {
        &self.vertices
    }

    /// 返回 interleaved 顶点数据的可变引用。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<f32>` - 顶点数据。
    pub fn get_vertices_mut(&mut self) -> &mut Vec<f32> {
        &mut self.vertices
    }

    /// 返回三角形索引的可变引用。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<u32>` - 三角形索引。
    pub fn get_indices_mut(&mut self) -> &mut Vec<u32> {
        &mut self.indices
    }

    /// 返回三角形数量。
    ///
    /// # Returns
    ///
    /// - `usize` - 三角形数量。
    pub fn get_triangle_count(&self) -> usize {
        self.triangle_count
    }

    /// 累加三角形数量(展开时每写入一个面调用一次)。
    ///
    /// # Arguments
    ///
    /// - `usize` - 要累加的三角形数量。
    pub fn add_triangle_count(&mut self, delta: usize) {
        self.set_triangle_count(self.get_triangle_count() + delta);
    }

    /// 覆盖三角形数量。
    ///
    /// # Arguments
    ///
    /// - `usize` - 新的三角形数量。
    pub fn set_triangle_count(&mut self, value: usize) {
        self.triangle_count = value;
    }

    /// 覆盖包围盒最小角。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 新的包围盒最小角。
    pub fn set_min(&mut self, value: Vec3) {
        self.min = value;
    }

    /// 覆盖包围盒最大角。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 新的包围盒最大角。
    pub fn set_max(&mut self, value: Vec3) {
        self.max = value;
    }

    /// 顶点数。
    ///
    /// # Returns
    ///
    /// - `usize` - 顶点数量。
    pub fn vertex_count(&self) -> usize {
        self.get_vertices_ref().len() / FLOATS_PER_VERTEX
    }
}

/// 每个顶点的 f32 数量:position(3) + normal(3) + color(3)。
pub const FLOATS_PER_VERTEX: usize = 9;

/// 金属度缺省值:0.0 = 介电质(非金属)。
pub(crate) const DEFAULT_METALLIC: f32 = 0.0;

/// 粗糙度缺省值:0.6 = 偏哑光。涂装车漆实测约 0.35,哑光塑料约 0.8,
/// 取中间值使缺省零件不至于看起来像塑料或镜子。
pub(crate) const DEFAULT_ROUGHNESS: f32 = 0.6;

/// 每个顶点的 PBR 数据 f32 数量:`uv(2) | tangent(4) | rough(1) | metal(1)`。
pub(crate) const FLOATS_PER_PBR_VERTEX: usize = 8;

/// 缺省 UV 的投影比例(每米多少个 UV 循环)。
///
/// 取 0.5:一块 2 m 宽的车门映出 1 个循环,颗粒度接近贴图的常见 TILE 值;
/// 太密会让 2048² 的贴图在远处摩尔纹爆掉,太疏则细节拉不开。
const DEFAULT_UV_SCALE: f32 = 0.5;

/// 按面法线选主轴做三平面投影,给没有 UV 的面生成一组坐标。
///
/// 三平面而不是单轴:单轴投影会让与投影轴垂直的面(例如朝上的引擎盖被
/// 按 X 投影)得到一条常量 UV,贴图整个塌成一条线。取法线绝对值最大的
/// 轴作为投影面,保证任何朝向的面都有两个变化的坐标。
///
/// # Arguments
///
/// - `[f32; 3]` - 面法线。
/// - `[f32; 3]` - 面上一个顶点的位置。
/// - `f32` - 投影比例(UV 单位 / 米)。
///
/// # Returns
///
/// - `[f32; 2]` - 该面主投影平面上的 UV。
fn triplanar_uv(normal: [f32; 3], point: [f32; 3], scale: f32) -> [f32; 2] {
    let ax: f32 = normal[0].abs();
    let ay: f32 = normal[1].abs();
    let az: f32 = normal[2].abs();
    if az >= ax && az >= ay {
        [point[0] * scale, point[1] * scale]
    } else if ax >= ay {
        [point[1] * scale, point[2] * scale]
    } else {
        [point[0] * scale, point[2] * scale]
    }
}

/// 推出一条与该面共面的单位切线 —— 法线贴图 TBN 里的 T 分量。
///
/// 缺省资产没有切线,但没有切线就做不了切线空间法线贴图,只能退回
/// 屏幕空间扰动,那会让金属高光在相机移动时「游动」。取离法线最远的
/// 坐标轴叉乘,保证 T 与 N 垂直;退化(法线正好是坐标轴)时换一个次选轴。
///
/// # Arguments
///
/// - `[f32; 3]` - 面法线。
///
/// # Returns
///
/// - `[f32; 3]` - 与该面共面的单位切线。
fn tangent_for_normal(normal: [f32; 3]) -> [f32; 3] {
    let axis: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut best: [f32; 3] = axis[0];
    let mut best_dot: f32 = -1.0;
    for candidate in axis {
        let dot: f32 =
            (candidate[0] * normal[0] + candidate[1] * normal[1] + candidate[2] * normal[2]).abs();
        if dot > best_dot {
            best_dot = dot;
            best = candidate;
        }
    }
    let cross: [f32; 3] = [
        normal[1] * best[2] - normal[2] * best[1],
        normal[2] * best[0] - normal[0] * best[2],
        normal[0] * best[1] - normal[1] * best[0],
    ];
    let length: f32 = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    if length < 1e-6 {
        return [1.0, 0.0, 0.0];
    }
    [cross[0] / length, cross[1] / length, cross[2] / length]
}

/// 面积小于这个值(单位 m²)的三角形直接丢弃。
///
/// 取 1e-9:最小资产(交通锥)也有 0.01 m 量级的小面,离得足够远,
/// 只拦真正退化的情况。
pub const DEGENERATE_TRIANGLE_AREA: f32 = 1e-9;

/// 校验并展开一个已反序列化的资产。
///
/// 丢掉退化三角形(面积 ≈ 0)。
///
/// 资产里有一批 Blender 导出的「未焊接」顶点 —— 坐标正好是 (0, 0, 0),
/// 而且真的被某些面引用。单个看每个三角形都极小,但它们一旦被 instancing
/// 的 model matrix 搬到街上,就会变成从模型原点射向整条街的长条,把画面撕成
/// 放射状的碎片。
///
/// 判据用面积而不是「坐标是否为 0」:真正的零面积三角形无论成因是什么
/// (顶点重合、共线、导出错误)都该被丢掉,而且不会误伤任何正常资产 ——
/// 正规三角形的面积都有 1e-3 m² 量级以上。
///
/// # Arguments
///
/// - `&MeshAsset` - 已反序列化的资产。
///
/// # Returns
///
/// - `Result<GpuMesh, MeshError>` - 展开后的 GPU 网格;无零件或无有效三角形时为 `Err`。
pub fn expand_asset(asset: &MeshAsset) -> Result<GpuMesh, MeshError> {
    if asset.parts.is_empty() {
        return Err(MeshError::Schema(format!(
            "asset {:?} has no parts",
            asset.id
        )));
    }
    let mut mesh: GpuMesh = GpuMesh {
        ..Default::default()
    };
    let mut min: [f32; 3] = [f32::INFINITY; 3];
    let mut max: [f32; 3] = [f32::NEG_INFINITY; 3];
    let mut saw_vertex: bool = false;

    for part in &asset.parts {
        validate_part(part)?;
        let base_color: [f32; 3] = part.base_color.unwrap_or([0.8, 0.8, 0.8]);
        for (face_index, face) in part.faces.iter().enumerate() {
            let color: [f32; 3] = match &part.face_colors {
                Some(colors) if colors.len() == part.faces.len() => clamp_color(colors[face_index]),
                _ => base_color,
            };
            let v0: [f32; 3] = fix_up(part.positions[face[0]], asset.y_up);
            let v1: [f32; 3] = fix_up(part.positions[face[1]], asset.y_up);
            let v2: [f32; 3] = fix_up(part.positions[face[2]], asset.y_up);
            let cross: [f32; 3] = [
                (v1[1] - v0[1]) * (v2[2] - v0[2]) - (v1[2] - v0[2]) * (v2[1] - v0[1]),
                (v1[2] - v0[2]) * (v2[0] - v0[0]) - (v1[0] - v0[0]) * (v2[2] - v0[2]),
                (v1[0] - v0[0]) * (v2[1] - v0[1]) - (v1[1] - v0[1]) * (v2[0] - v0[0]),
            ];
            // 面积 = |cross| / 2
            let area: f32 =
                (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt() * 0.5;
            if area < DEGENERATE_TRIANGLE_AREA {
                continue;
            }
            let mut normal: [f32; 3] = if part.flat {
                face_normal(v0, v1, v2)
            } else {
                match &part.normals {
                    Some(normals) => normalize3([
                        normals[face[0]][0],
                        normals[face[0]][1],
                        normals[face[0]][2],
                    ]),
                    None => face_normal(v0, v1, v2),
                }
            };
            // Z-up 资产转换后,法线同样要交换 Y/Z 分量。
            if !asset.y_up {
                normal = [normal[0], normal[2], normal[1]];
            }
            // `uv` / `tangents` 缺省或长度对不上时整体退回程序化兜底 —— 逐顶点数组
            // 长度一旦和 `positions` 不符,按角取值就会越界 panic。
            let uv_matches: bool = part
                .uv
                .as_ref()
                .is_some_and(|values: &Vec<[f32; 2]>| values.len() == part.positions.len());
            let tangent_matches: bool = part
                .tangents
                .as_ref()
                .is_some_and(|values: &Vec<[f32; 4]>| values.len() == part.positions.len());
            // 粗糙度下限 0.04:0 会让 GGX 的分母除零并炸出 NaN 白斑。
            let rough: f32 = part.roughness.unwrap_or(DEFAULT_ROUGHNESS).clamp(0.04, 1.0);
            let metal: f32 = part.metallic.unwrap_or(DEFAULT_METALLIC).clamp(0.0, 1.0);
            let derived_tangent: [f32; 3] = tangent_for_normal(normal);
            for (slot, vertex) in [v0, v1, v2].into_iter().enumerate() {
                mesh.get_vertices_mut().extend_from_slice(&[
                    vertex[0], vertex[1], vertex[2], //
                    normal[0], normal[1], normal[2], //
                    color[0], color[1], color[2],
                ]);
                let corner: usize = face[slot];
                let corner_uv: [f32; 2] = match (uv_matches, part.uv.as_ref()) {
                    (true, Some(values)) => values[corner],
                    _ => triplanar_uv(normal, vertex, DEFAULT_UV_SCALE),
                };
                let corner_tangent: [f32; 4] = match (tangent_matches, part.tangents.as_ref()) {
                    (true, Some(values)) => values[corner],
                    _ => [
                        derived_tangent[0],
                        derived_tangent[1],
                        derived_tangent[2],
                        1.0,
                    ],
                };
                mesh.pbr.extend_from_slice(&[
                    corner_uv[0],
                    corner_uv[1],
                    corner_tangent[0],
                    corner_tangent[1],
                    corner_tangent[2],
                    corner_tangent[3],
                    rough,
                    metal,
                ]);
                for axis in 0..3 {
                    if vertex[axis] < min[axis] {
                        min[axis] = vertex[axis];
                    }
                    if vertex[axis] > max[axis] {
                        max[axis] = vertex[axis];
                    }
                }
                saw_vertex = true;
            }
            // 顺序索引:每 3 个新顶点构成一个三角形。
            let base: u32 = mesh.get_triangle_count() as u32 * 3;
            mesh.get_indices_mut()
                .extend_from_slice(&[base, base + 1, base + 2]);
            mesh.add_triangle_count(1);
        }
    }

    if !saw_vertex {
        return Err(MeshError::Schema(format!(
            "asset {:?} produced zero triangles",
            asset.id
        )));
    }
    mesh.set_min(min);
    mesh.set_max(max);
    Ok(mesh)
}

/// 校验单个 part 的内部一致性。
///
/// 越界断言:`faces` 里的每个下标都必须落在 `positions` 范围内;
/// `normals` 若存在则长度必须与 `positions` 一致。
///
/// # Arguments
///
/// - `&MeshPart` - 待校验的零件。
///
/// # Returns
///
/// - `Result<(), MeshError>` - `Ok(())` 表示一致性通过。
///   校验单个 part 的内部一致性。
///
/// 越界断言:`faces` 里的每个下标都必须落在 `positions` 范围内;
/// `normals` 若存在则长度必须与 `positions` 一致。
///
/// # Arguments
///
/// - `&MeshPart` - 待校验的零件。
///
/// # Returns
///
/// - `Result<(), MeshError>` - `Ok(())` 表示一致性通过。
fn validate_part(part: &MeshPart) -> Result<(), MeshError> {
    if part.positions.is_empty() {
        return Err(MeshError::Schema(format!(
            "part {:?} has empty positions",
            part.name
        )));
    }
    if let Some(normals) = &part.normals
        && normals.len() != part.positions.len()
    {
        return Err(MeshError::Schema(format!(
            "part {:?} normals len {} != positions len {}",
            part.name,
            normals.len(),
            part.positions.len()
        )));
    }
    if let Some(colors) = &part.face_colors
        && !colors.is_empty()
        && colors.len() != part.faces.len()
    {
        return Err(MeshError::Schema(format!(
            "part {:?} face_colors len {} != faces len {}",
            part.name,
            colors.len(),
            part.faces.len()
        )));
    }
    let vertex_count: usize = part.positions.len();
    for (face_index, face) in part.faces.iter().enumerate() {
        for corner in face.iter() {
            if *corner >= vertex_count {
                return Err(MeshError::Schema(format!(
                    "part {:?} face {} index {} out of range (positions len {})",
                    part.name, face_index, corner, vertex_count
                )));
            }
        }
    }
    for (index, position) in part.positions.iter().enumerate() {
        if position.iter().any(|value: &f32| !value.is_finite()) {
            return Err(MeshError::Schema(format!(
                "part {:?} position {} is not finite: {:?}",
                part.name, index, position
            )));
        }
    }
    Ok(())
}

/// `y_up == false` 时把 Z-up 转为 Y-up(交换 Y / Z,并对法线做同样的处理)。
///
/// # Arguments
///
/// - `Vec3Json` - 待转换的顶点坐标。
/// - `bool` - 资产是否已经是 Y 轴向上。
///
/// # Returns
///
/// - `Vec3` - Y-up 坐标系下的顶点坐标。
///   `y_up == false` 时把 Z-up 转为 Y-up(交换 Y / Z,并对法线做同样的处理)。
///
/// # Arguments
///
/// - `Vec3Json` - 待转换的顶点坐标。
/// - `bool` - 资产是否已经是 Y 轴向上。
///
/// # Returns
///
/// - `Vec3` - Y-up 坐标系下的顶点坐标。
fn fix_up(point: Vec3Json, y_up: bool) -> Vec3 {
    if y_up {
        [point[0], point[1], point[2]]
    } else {
        [point[0], point[2], point[1]]
    }
}

/// 计算三角形的面法线(未归一化方向,按 CCW 绕序用叉积)。
///
/// # Arguments
///
/// - `Vec3` - 三角形第一个顶点。
/// - `Vec3` - 三角形第二个顶点。
/// - `Vec3` - 三角形第三个顶点。
///
/// # Returns
///
/// - `Vec3` - 归一化后的面法线。
///   计算三角形的面法线(未归一化方向,按 CCW 绕序用叉积)。
///
/// # Arguments
///
/// - `Vec3` - 三角形第一个顶点。
/// - `Vec3` - 三角形第二个顶点。
/// - `Vec3` - 三角形第三个顶点。
///
/// # Returns
///
/// - `Vec3` - 归一化后的面法线。
fn face_normal(a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let edge1: [f32; 3] = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let edge2: [f32; 3] = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    normalize3([
        edge1[1] * edge2[2] - edge1[2] * edge2[1],
        edge1[2] * edge2[0] - edge1[0] * edge2[2],
        edge1[0] * edge2[1] - edge1[1] * edge2[0],
    ])
}

/// 归一化一个向量;零向量原样返回。
///
/// # Arguments
///
/// - `Vec3` - 待归一化的向量。
///
/// # Returns
///
/// - `Vec3` - 单位长度向量。
pub fn normalize3(v: Vec3) -> Vec3 {
    let len: f32 = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= f32::EPSILON {
        [0.0, 1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

/// 把颜色分量钳制到 `[0, 1]`。
///
/// # Arguments
///
/// - `Vec3Json` - 原始线性 RGB。
///
/// # Returns
///
/// - `Vec3` - 钳制到 `[0, 1]` 的 RGB。
///   把颜色分量钳制到 `[0, 1]`。
///
/// # Arguments
///
/// - `Vec3Json` - 原始线性 RGB。
///
/// # Returns
///
/// - `Vec3` - 钳制到 `[0, 1]` 的 RGB。
fn clamp_color(color: Vec3Json) -> Vec3 {
    [
        color[0].clamp(0.0, 1.0),
        color[1].clamp(0.0, 1.0),
        color[2].clamp(0.0, 1.0),
    ]
}

/// 把 `&[f32]` 重解释为字节切片(wasm32 上二者布局一致)。
///
/// # Arguments
///
/// - `&[f32]` - 待重解释的浮点切片。
///
/// # Returns
///
/// - `&[u8]` - 同一段内存的字节视图。
pub fn f32_slice_to_bytes(data: &[f32]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 4) }
}

/// 把 `&[u32]` 重解释为字节切片(wasm32 上二者布局一致)。
///
/// # Arguments
///
/// - `&[u32]` - 待重解释的整数切片。
///
/// # Returns
///
/// - `&[u8]` - 同一段内存的字节视图。
pub fn u32_slice_to_bytes(data: &[u32]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 4) }
}

/// 程序化生成一个立方体的三角网格(`part` 表示一个立方体)。
///
/// 用于 JSON 缺失时的回退,也可用于程序化搭建关卡道具。
///
/// # Arguments
///
/// - `Vec3` - 立方体中心。
/// - `f32` - 立方体半边长。
/// - `Vec3` - 基础色(线性 RGB)。
/// - `&str` - 零件名。
///
/// # Returns
///
/// - `MeshPart` - 展开后的立方体零件。
pub fn cube_part(center: Vec3, half_size: f32, color: Vec3, name: &str) -> MeshPart {
    let (cx, cy, cz): (f32, f32, f32) = (center[0], center[1], center[2]);
    let (hx, hy, hz): (f32, f32, f32) = (half_size, half_size, half_size);
    let positions: Vec<Vec3Json> = vec![
        [cx - hx, cy - hy, cz - hz],
        [cx + hx, cy - hy, cz - hz],
        [cx + hx, cy + hy, cz - hz],
        [cx - hx, cy + hy, cz - hz],
        [cx - hx, cy - hy, cz + hz],
        [cx + hx, cy - hy, cz + hz],
        [cx + hx, cy + hy, cz + hz],
        [cx - hx, cy + hy, cz + hz],
    ];
    // 每个面 CCW(从外侧看逆时针)。
    let faces: Vec<[usize; 3]> = vec![
        [4, 5, 6],
        [4, 6, 7], // +Z
        [1, 0, 3],
        [1, 3, 2], // -Z
        [5, 1, 2],
        [5, 2, 6], // +X
        [0, 4, 7],
        [0, 7, 3], // -X
        [3, 7, 6],
        [3, 6, 2], // +Y
        [0, 1, 5],
        [0, 5, 4], // -Y
    ];
    MeshPart {
        name: name.to_string(),
        base_color: Some(color),
        emissive: None,
        positions,
        normals: None,
        uv: None,
        tangents: None,
        roughness: None,
        metallic: None,
        texture_atlas: None,
        surface_kind: None,
        faces,
        face_colors: None,
        flat: true,
    }
}
