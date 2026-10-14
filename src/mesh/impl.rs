use super::*;

impl Bounds {}

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

impl MeshAsset {
    /// 包围盒是否自洽(每个轴上 min <= max,且不是一个退化的点)。
    ///
    /// 资产由离线管线导出,`bounds` 与 `positions` 是同一次计算的产物
    /// (见 `assets/SCHEMA.md` §4)。加载时校验一次,可以在几何被写坏时
    /// 立刻发现,而不是等到画面上出现一个 NaN 三角形才去猜。
    ///
    /// # Returns
    ///
    /// - `bool` - 包围盒各轴方向正确且至少有一个轴非零时为 `true`。
    pub fn get_bounds_envelope_is_sane(&self) -> bool {
        let Some(bounds) = self.get_bounds() else {
            return true;
        };
        let min: Vec3Json = *bounds.get_min();
        let max: Vec3Json = *bounds.get_max();
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

    /// 累加三角形数量(展开时每写入一个面调用一次)。
    ///
    /// # Arguments
    ///
    /// - `usize` - 要累加的三角形数量。
    pub fn add_triangle_count(&mut self, delta: usize) {
        self.set_triangle_count(self.get_triangle_count() + delta);
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
