/// 每个顶点的 f32 数量:position(3) + normal(3) + color(3)。
pub(crate) const FLOATS_PER_VERTEX: usize = 9;

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
pub(crate) const DEFAULT_UV_SCALE: f32 = 0.5;

/// 面积小于这个值(单位 m²)的三角形直接丢弃。
///
/// 取 1e-9:最小资产(交通锥)也有 0.01 m 量级的小面,离得足够远,
/// 只拦真正退化的情况。
pub(crate) const DEGENERATE_TRIANGLE_AREA: f32 = 1e-9;
