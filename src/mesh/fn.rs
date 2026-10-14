use super::*;

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
pub(crate) fn default_y_up() -> bool {
    true
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
pub(crate) fn default_true() -> bool {
    true
}

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
pub(crate) fn triplanar_uv(normal: [f32; 3], point: [f32; 3], scale: f32) -> [f32; 2] {
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
pub(crate) fn tangent_for_normal(normal: [f32; 3]) -> [f32; 3] {
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
    if asset.get_parts().is_empty() {
        return Err(MeshError::Schema(format!(
            "asset {:?} has no parts",
            asset.get_id()
        )));
    }
    let mut mesh: GpuMesh = GpuMesh {
        ..Default::default()
    };
    let mut min: [f32; 3] = [f32::INFINITY; 3];
    let mut max: [f32; 3] = [f32::NEG_INFINITY; 3];
    let mut saw_vertex: bool = false;
    for part in asset.get_parts() {
        validate_part(part)?;
        let base_color: [f32; 3] = part.get_base_color().unwrap_or([0.8, 0.8, 0.8]);
        for (face_index, face) in part.get_faces().iter().enumerate() {
            let color: [f32; 3] = match &part.get_face_colors() {
                Some(colors) if colors.len() == part.get_faces().len() => {
                    clamp_color(colors[face_index])
                }
                _ => base_color,
            };
            let v0: [f32; 3] = fix_up(part.get_positions()[face[0]], asset.get_y_up());
            let v1: [f32; 3] = fix_up(part.get_positions()[face[1]], asset.get_y_up());
            let v2: [f32; 3] = fix_up(part.get_positions()[face[2]], asset.get_y_up());
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
            let mut normal: [f32; 3] = if part.get_flat() {
                face_normal(v0, v1, v2)
            } else {
                match &part.get_normals() {
                    Some(normals) => normalize3([
                        normals[face[0]][0],
                        normals[face[0]][1],
                        normals[face[0]][2],
                    ]),
                    None => face_normal(v0, v1, v2),
                }
            };
            // Z-up 资产转换后,法线同样要交换 Y/Z 分量。
            if !asset.get_y_up() {
                normal = [normal[0], normal[2], normal[1]];
            }
            // `uv` / `tangents` 缺省或长度对不上时整体退回程序化兜底 —— 逐顶点数组
            // 长度一旦和 `positions` 不符,按角取值就会越界 panic。
            let uv_matches: bool = part
                .uv
                .as_ref()
                .is_some_and(|values: &Vec<[f32; 2]>| values.len() == part.get_positions().len());
            let tangent_matches: bool = part
                .tangents
                .as_ref()
                .is_some_and(|values: &Vec<[f32; 4]>| values.len() == part.get_positions().len());
            // 粗糙度下限 0.04:0 会让 GGX 的分母除零并炸出 NaN 白斑。
            let rough: f32 = part
                .get_roughness()
                .unwrap_or(DEFAULT_ROUGHNESS)
                .clamp(0.04, 1.0);
            let metal: f32 = part
                .get_metallic()
                .unwrap_or(DEFAULT_METALLIC)
                .clamp(0.0, 1.0);
            let derived_tangent: [f32; 3] = tangent_for_normal(normal);
            for (slot, vertex) in [v0, v1, v2].into_iter().enumerate() {
                mesh.get_vertices_mut().extend_from_slice(&[
                    vertex[0], vertex[1], vertex[2], //
                    normal[0], normal[1], normal[2], //
                    color[0], color[1], color[2],
                ]);
                let corner: usize = face[slot];
                let corner_uv: [f32; 2] = match (uv_matches, part.get_uv().as_ref()) {
                    (true, Some(values)) => values[corner],
                    _ => triplanar_uv(normal, vertex, DEFAULT_UV_SCALE),
                };
                let corner_tangent: [f32; 4] = match (tangent_matches, part.get_tangents()) {
                    (true, Some(values)) => values[corner],
                    _ => [
                        derived_tangent[0],
                        derived_tangent[1],
                        derived_tangent[2],
                        1.0,
                    ],
                };
                mesh.get_mut_pbr().extend_from_slice(&[
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
            asset.get_id()
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
pub(crate) fn validate_part(part: &MeshPart) -> Result<(), MeshError> {
    if part.get_positions().is_empty() {
        return Err(MeshError::Schema(format!(
            "part {:?} has empty positions",
            part.get_name()
        )));
    }
    if let Some(normals) = &part.get_normals()
        && normals.len() != part.get_positions().len()
    {
        return Err(MeshError::Schema(format!(
            "part {:?} normals len {} != positions len {}",
            part.get_name(),
            normals.len(),
            part.get_positions().len()
        )));
    }
    if let Some(colors) = &part.get_face_colors()
        && !colors.is_empty()
        && colors.len() != part.get_faces().len()
    {
        return Err(MeshError::Schema(format!(
            "part {:?} face_colors len {} != faces len {}",
            part.get_name(),
            colors.len(),
            part.get_faces().len()
        )));
    }
    let vertex_count: usize = part.get_positions().len();
    for (face_index, face) in part.get_faces().iter().enumerate() {
        for corner in face.iter() {
            if *corner >= vertex_count {
                return Err(MeshError::Schema(format!(
                    "part {:?} face {} index {} out of range (positions len {})",
                    part.get_name(),
                    face_index,
                    corner,
                    vertex_count
                )));
            }
        }
    }
    for (index, position) in part.get_positions().iter().enumerate() {
        if position.iter().any(|value: &f32| !value.is_finite()) {
            return Err(MeshError::Schema(format!(
                "part {:?} position {} is not finite: {:?}",
                part.get_name(),
                index,
                position
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
pub(crate) fn fix_up(point: Vec3Json, y_up: bool) -> Vec3 {
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
pub(crate) fn face_normal(a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
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
pub(crate) fn clamp_color(color: Vec3Json) -> Vec3 {
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
