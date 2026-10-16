use super::*;

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
pub(crate) fn instance_distance(instance: &Instance, eye: Vec3) -> f32 {
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
        triangle_count: mesh.get_triangle_count(),
        ..Default::default()
    };
    out.get_mut_vertices()
        .reserve(mesh.vertex_count() * STRIDE_FLOATS);
    let vertex_count: usize = mesh.vertex_count();
    // 包围球累积:AABB 中点 + 半对角线(保守球)。主 pass 视锥剔除用。
    let mut bmin: [f32; 3] = [f32::MAX; 3];
    let mut bmax: [f32; 3] = [f32::MIN; 3];
    for vertex in 0..vertex_count {
        let base: usize = vertex * 9;
        let pbr_base: usize = vertex * crate::mesh::FLOATS_PER_PBR_VERTEX;
        let position: [f32; 3] = [
            mesh.get_vertices()[base],
            mesh.get_vertices()[base + 1],
            mesh.get_vertices()[base + 2],
        ];
        for axis in 0..3 {
            bmin[axis] = bmin[axis].min(position[axis]);
            bmax[axis] = bmax[axis].max(position[axis]);
        }
        let normal: [f32; 3] = [
            mesh.get_vertices()[base + 3],
            mesh.get_vertices()[base + 4],
            mesh.get_vertices()[base + 5],
        ];
        let color: [f32; 3] = [
            mesh.get_vertices()[base + 6],
            mesh.get_vertices()[base + 7],
            mesh.get_vertices()[base + 8],
        ];
        // 三角形 = 第 `vertex / 3` 个三角形;顶点 0/1/2 属于同一个三角形。
        let triangle: usize = vertex / 3;
        let emissive: [f32; 3] = part_emissive.get(triangle).copied().unwrap_or([0.0; 3]);
        let pbr_uv: f32 = mesh
            .pbr
            .get(pbr_base)
            .copied()
            .unwrap_or(vertex as f32 * DEFAULT_UV_FALLBACK_STEP);
        let pbr_uv_next: f32 = mesh
            .pbr
            .get(pbr_base + 1)
            .copied()
            .unwrap_or(vertex as f32 * DEFAULT_UV_FALLBACK_STEP);
        // 粗糙度下限 0.04 与着色器一致:0 会让 GGX 分母除零。
        let pbr_rough: f32 = mesh
            .pbr
            .get(pbr_base + 6)
            .copied()
            .unwrap_or(0.6)
            .clamp(0.04, 1.0);
        let pbr_metal: f32 = mesh
            .pbr
            .get(pbr_base + 7)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let pbr_tangent: [f32; 4] = [
            mesh.get_pbr().get(pbr_base + 2).copied().unwrap_or(1.0),
            mesh.get_pbr().get(pbr_base + 3).copied().unwrap_or(0.0),
            mesh.get_pbr().get(pbr_base + 4).copied().unwrap_or(0.0),
            mesh.get_pbr().get(pbr_base + 5).copied().unwrap_or(1.0),
        ];
        out.get_mut_vertices().extend_from_slice(&[
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
            emissive[2], //
            pbr_uv,
            pbr_uv_next, //
            pbr_tangent[0],
            pbr_tangent[1],
            pbr_tangent[2],
            pbr_tangent[3],
            pbr_rough,
            pbr_metal,
        ]);
    }
    for triangle in 0..mesh.get_triangle_count() {
        let base: usize = (triangle * 3) * 9;
        let normal: [f32; 3] = [
            mesh.get_vertices()[base + 3],
            mesh.get_vertices()[base + 4],
            mesh.get_vertices()[base + 5],
        ];
        let color: [f32; 3] = [
            mesh.get_vertices()[base + 6],
            mesh.get_vertices()[base + 7],
            mesh.get_vertices()[base + 8],
        ];
        let emissive: [f32; 3] = part_emissive.get(triangle).copied().unwrap_or([0.0; 3]);
        out.get_mut_indices().extend_from_slice(&[
            (triangle * 3) as u32,
            (triangle * 3 + 1) as u32,
            (triangle * 3 + 2) as u32,
        ]);
        out.get_mut_faces().push(Face {
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
    // AABB 中点 + 半对角线 = 保守包围球(空 mesh 保持半径 0 = 未初始化)。
    if vertex_count > 0 {
        let center: [f32; 3] = [
            (bmin[0] + bmax[0]) * 0.5,
            (bmin[1] + bmax[1]) * 0.5,
            (bmin[2] + bmax[2]) * 0.5,
        ];
        let radius: f32 = ((bmax[0] - center[0]).powi(2)
            + (bmax[1] - center[1]).powi(2)
            + (bmax[2] - center[2]).powi(2))
        .sqrt();
        out.set_bounds_sphere([center[0], center[1], center[2], radius]);
    }
    out
}

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
/// - `[f32; 3]` - 计算结果。
pub fn shade_face(
    albedo: Vec3,
    emissive: Vec3,
    normal: Vec3,
    tint: Vec3,
    lighting: &SceneLighting,
    eye_distance: f32,
) -> [f32; 3] {
    let n_dot_l: f32 = (normal[0] * lighting.get_light_dir()[0]
        + normal[1] * lighting.get_light_dir()[1]
        + normal[2] * lighting.get_light_dir()[2])
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
        let hemi: f32 = lighting.get_ground_ambient()[channel]
            + (lighting.get_sky_ambient()[channel] - lighting.get_ground_ambient()[channel])
                * hemi_weight;
        let ambient: f32 = lighting.get_ambient()[channel]
            + (hemi - lighting.get_ambient()[channel]) * lighting.get_ambient_hemi();
        let diffuse: f32 = ambient + lighting.get_light_color()[channel] * n_dot_l;
        out[channel] = base[channel] * diffuse
            + base[channel] * emissive[channel] * lighting.get_emissive_gain();
    }
    // 色调映射(色相保持)+ 天空色晕染。顺序很重要:先在线性域压亮度,
    // 再加天空色,最后才是 sRGB 编码。
    let mut mapped: [f32; 3] = tonemap(out, lighting.get_exposure(), lighting.get_tone_map_white());
    for channel in 0..3 {
        mapped[channel] += lighting.get_sky_color()[channel] * SKY_TINT_GAIN;
    }
    // 大气雾:线性空间里向天空色插值,自发光通道不吃雾(夜里霓虹要穿透雾)。
    let span: f32 = (lighting.get_fog_end() - lighting.get_fog_start()).max(f32::EPSILON);
    let fog: f32 = ((eye_distance - lighting.get_fog_start()) / span).clamp(0.0, 1.0);
    let fog: f32 = fog * fog * (3.0 - 2.0 * fog);
    let self_lit: f32 = base[0] * emissive[0] + base[1] * emissive[1] + base[2] * emissive[2];
    if self_lit > 0.01 {
        let glow: f32 = (fog * 0.72).min(0.72);
        for (channel, sky) in mapped.iter_mut().zip(lighting.get_sky_color().iter()) {
            *channel += sky * glow;
        }
    }
    for (channel, sky) in mapped.iter_mut().zip(lighting.get_sky_color().iter()) {
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
pub(crate) fn vertex_position(mesh: &MeshAssetGpu, index: u32) -> Vec3 {
    let base: usize = (index as usize) * STRIDE_FLOATS;
    [
        mesh.get_vertices()[base],
        mesh.get_vertices()[base + 1],
        mesh.get_vertices()[base + 2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 回归:降档是**单向**的 —— 代码里刻意不执行升档。
    ///
    /// 这条以前只有一句注释守着(`单向降级:这里刻意不执行升档`),没有断言。
    /// 它很重要:SSR 只在 `High` 档跑(`wants_ssr`),所以一旦在慢机器上掉到
    /// `Medium`,整条湿地面反射管线会**静默消失**,而画面看起来仍然正常。
    /// 本测试把这个语义钉死,顺手确认 `Low` 不会自己爬回去。
    #[test]
    pub(crate) fn quality_only_ever_drops_and_never_climbs_back() {
        let mut quality: AdaptiveQuality = AdaptiveQuality::new();
        assert_eq!(
            quality.get_tier(),
            QualityTier::High,
            "初始档位必须是 High —— 否则第一帧就没有 SSR"
        );
        // 连续喂慢帧:High -> Medium -> Low,然后钉死在 Low。
        for _ in 0..(SLOW_FRAME_THRESHOLD * 4) {
            quality.sample(QUALITY_DOWN_FPS * 0.5);
        }
        let dropped: QualityTier = quality.get_tier();
        assert!(
            dropped == QualityTier::Medium || dropped == QualityTier::Low,
            "喂了 {} 个慢帧之后档位应该已经降过,实际是 {:?}",
            SLOW_FRAME_THRESHOLD * 4,
            dropped.tier_name()
        );
        // 再喂一堆**快**帧 —— 档位不许涨回去(这是刻意的设计)。
        let after_fast: QualityTier = {
            for _ in 0..(FAST_FRAME_THRESHOLD * 4) {
                quality.sample(240.0);
            }
            quality.get_tier()
        };
        assert_eq!(
            after_fast,
            dropped,
            "单向降档:快帧不能让档位自己涨回 {:?}(实际 {:?})",
            dropped.tier_name(),
            after_fast.tier_name()
        );
        assert!(
            !dropped.wants_ssr() || dropped == QualityTier::High,
            "只有 High 档才跑 SSR"
        );
    }

    /// `fill()` 断言文案里的字段名占位符(§1.3c:字面量集中在 const.rs)。
    pub(crate) const L_DIST: &str = "dist";
    pub(crate) const L_REACH: &str = "reach";
    pub(crate) const L_LY: &str = "ly";

    /// 把断言文案里的 `{名字}` 占位符替换成实际数值。
    pub(crate) fn fill(template: &str, args: &[(&str, &str)]) -> String {
        let mut out: String = template.to_string();
        let mut index: usize = 0;
        while index < args.len() {
            out = out.replace(&format!("{{{}}}", args[index].0), args[index].1);
            index += 1;
        }
        out
    }

    /// 在给定位置造一个单位缩放的实例。
    pub(crate) fn at(position: Vec3) -> Instance {
        Instance::new(position, 0.0, 1.0, [1.0, 1.0, 1.0])
    }

    /// 游戏正午的真实光照方向(`render.rs` 里 `NOON` 那一档)。
    pub(crate) fn noon_light() -> Vec3 {
        normalize3([0.35, 0.86, 0.36])
    }

    /// 判据的有效半径:半宽 + 余量。
    pub(crate) fn reach() -> f32 {
        SHADOW_HALF_EXTENT + SHADOW_CULL_MARGIN
    }

    /// frustum 正中心的实例必须保留 —— 否则玩家脚下会没有影子。
    #[test]
    pub(crate) fn shadow_cull_keeps_frustum_centre() {
        let focus: Vec3 = [12.0, 0.0, -34.0];
        let kept: bool = instance_affects_shadow(&at(focus), focus, noon_light());
        assert!(
            kept,
            "{}",
            fill(
                T_SHADOW_KEEPS_CENTRE,
                &[(L_DIST, "0.00"), (L_REACH, &format!("{:.2}", reach()))]
            )
        );
    }

    /// 远在 frustum 之外的实例必须被剔除 —— 这正是优化要的收益。
    #[test]
    pub(crate) fn shadow_cull_drops_far_instances() {
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
                    (L_DIST, &format!("{:.2}", reach() * 2.0)),
                    (L_REACH, &format!("{:.2}", reach()))
                ]
            )
        );
    }

    /// 余量之内(视锥之外一点点)必须仍然保留:余量就是防「影子凭空消失」。
    #[test]
    pub(crate) fn shadow_cull_keeps_inside_margin() {
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
                    (
                        L_DIST,
                        &format!("{:.2}", SHADOW_HALF_EXTENT + SHADOW_CULL_MARGIN * 0.5)
                    ),
                    (L_REACH, &format!("{:.2}", reach()))
                ]
            )
        );
    }

    /// 贴地光(`light_dir.y` ≈ 0)不得除零,也不得把视锥内的实例剔掉。
    #[test]
    pub(crate) fn shadow_cull_survives_grounded_light() {
        let focus: Vec3 = [0.0, 0.0, 0.0];
        let flat: Vec3 = normalize3([1.0, 0.0, 0.0]);
        let kept: bool = instance_affects_shadow(&at([1.0, 0.0, 0.0]), focus, flat);
        assert!(
            kept && flat[1].abs() <= SHADOW_GROUNDED_EPS,
            "{}",
            fill(
                T_SHADOW_GROUNDED_SAFE,
                &[
                    (L_LY, &format!("{:.6}", flat[1].abs())),
                    (L_DIST, "1.00"),
                    (L_REACH, &format!("{:.2}", reach()))
                ]
            )
        );
    }

    /// 斜光下的高楼:影子被拉得很远,但只要落点还在视锥内就必须画。
    ///
    /// 这是光空间投影存在的理由 —— 只比「到 focus 的水平距离」的话,
    /// 这栋楼会被误剔,地面上凭空少一块长影子。
    #[test]
    pub(crate) fn shadow_cull_keeps_tall_building_under_oblique_light() {
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
                    (L_DIST, &format!("{dist:.2}")),
                    (L_REACH, &format!("{:.2}", reach()))
                ]
            )
        );
    }

    /// `?phase=` 的三个取值必须分别落到三档相位。
    #[test]
    pub(crate) fn phase_query_maps_every_value() {
        let cases: [(String, DayPhase); 3] = [
            (String::from(PHASE_QUERY_NOON), DayPhase::Noon),
            (String::from(PHASE_QUERY_DUSK), DayPhase::Dusk),
            (String::from(PHASE_QUERY_NIGHT), DayPhase::Night),
        ];
        for (value, want) in cases {
            let got: Option<DayPhase> = DayPhase::from_query(&value);
            assert!(
                got == Some(want),
                "{}",
                fill(
                    T_PHASE_QUERY_MAPS,
                    &[
                        (PHASE_FIELD_VALUE, &value),
                        ("phase", want.as_str()),
                        ("got", &format!("{got:?}"))
                    ]
                )
            );
        }
    }

    /// 拼错的取值必须**回退**,不能悄悄落回某一档相位。
    ///
    /// 用 `duskish` 而不是空串:空串在「参数缺失」时也会出现,那样这条
    /// 测试证明不了「非法取值会被拒」,只证明了「空值会回退」。
    #[test]
    pub(crate) fn phase_query_rejects_unknown_value() {
        let got: Option<DayPhase> = DayPhase::from_query(T_PHASE_QUERY_PROBE);
        assert!(
            got.is_none(),
            "{}",
            fill(
                T_PHASE_QUERY_UNKNOWN,
                &[
                    (PHASE_FIELD_VALUE, T_PHASE_QUERY_PROBE),
                    ("got", &format!("{got:?}"))
                ]
            )
        );
    }

    /// 每个被 `get_uniform_location` 查过的 uniform 名都必须在某段 GLSL 里
    /// **真的声明过**。
    ///
    /// ⚠️ 这是两条**已经真实发生过**的静默 bug 的回归闸。查一个不存在的
    /// 名字不会报错:`get_uniform_location` 只是返回 `None`,而
    /// `uniform1f(None, …)` 是彻底的空操作 —— 没有警告、没有异常、画面
    /// 照常出图,只是那个 uniform 永远保持 GLSL 的默认值(0)。
    ///
    /// - `U_BLOOM_THRESHOLD` 曾经写成 `"u_bloom_threshold"`,而亮度提取
    ///   着色器里声明的是 `u_threshold`。阈值恒为 0.0,于是提取 pass
    ///   **把整幅画面原样透传**成「bloom 图」,合成时再乘 `0.85 * 1.85`。
    ///   实测夜景整幅被抬亮(均值 0.201 → 0.498,越过阈值的像素
    ///   0.20% → 30.2%),整座城市看上去像开了高光过曝 —— 而这正是
    ///   「WebGPU 的 bloom 弱 82 倍」这个结论的来源:WebGL2 的 0.296
    ///   根本不是 bloom,是这个 bug 抬起来的。
    /// - `U_BLOOM_DIR` 同样写成 `"u_bloom_dir"`,而模糊着色器声明的是
    ///   `u_direction`。方向恒为 `(0,0)`,可分离高斯退化成同点加权重采样,
    ///   完全没有模糊。
    ///
    /// 两条都不会让 `cargo test` 变红、不产生一条编译警告,只能靠这条测试。
    #[test]
    pub(crate) fn every_looked_up_uniform_is_declared_in_glsl() {
        // 所有 GLSL 源码常量(带 `#version 300 es` 的那些 raw string)。
        pub(crate) const SOURCES: [&str; 11] = [
            VERTEX_SHADER,
            FRAGMENT_SHADER,
            GLOW_FRAGMENT_SHADER,
            SHADOW_VERTEX_SHADER,
            SHADOW_FRAGMENT_SHADER,
            GBUFFER_FRAGMENT_SHADER,
            SSAO_FRAGMENT_SHADER,
            SSR_FRAGMENT_SHADER,
            BLOOM_EXTRACT_FRAGMENT_SHADER,
            BLOOM_BLUR_FRAGMENT_SHADER,
            COMPOSITE_FRAGMENT_SHADER,
        ];
        let mut declared: Vec<&str> = Vec::new();
        for source in SOURCES {
            for part in source.split("uniform ") {
                // `uniform vec2 u_direction;` → 末个标识符就是名字。
                let Some(head) = part.split(';').next() else {
                    continue;
                };
                if let Some(name) = head.split_whitespace().last() {
                    declared.push(name);
                }
            }
        }
        // 被 Rust 侧实际查过的 uniform 名 → 必须逐个在上面出现过。
        pub(crate) const LOOKED_UP: [(&str, &str); 7] = [
            ("U_BLOOM_THRESHOLD", U_BLOOM_THRESHOLD),
            ("U_BLOOM_DIR", U_BLOOM_DIR),
            ("U_AO_HEIGHT", U_AO_HEIGHT),
            ("U_AO_FLOOR", U_AO_FLOOR),
            ("U_AO_REACH", U_AO_REACH),
            ("U_AO_FEATHER", U_AO_FEATHER),
            ("U_AO_MIN_HEIGHT", U_AO_MIN_HEIGHT),
        ];
        for (constant, name) in LOOKED_UP {
            assert!(
                declared.contains(&name),
                "get_uniform_location({constant} = \"{name}\") 在任何 GLSL 里都查不到 —— \
                 它会返回 None,而 uniform1f(None, …) 是静默空操作,该 uniform 永远取默认值。"
            );
        }
    }

    /// GLSL `smoothstep(a, b, x)` 的 Rust 写法。
    ///
    /// # Arguments
    ///
    /// - `a` - 下界。
    /// - `b` - 上界(要求 `a < b`)。
    /// - `x` - 输入。
    ///
    /// # Returns
    ///
    /// 平滑后的 `0..=1`。
    pub(crate) fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
        let t: f32 = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// 顶点接触 AO 的 Rust 镜像,与 `VERTEX_SHADER` 里那一段逐字对应。
    ///
    /// 放在测试里而不是只测 GLSL 字符串,是因为缺陷本质是**数值**的:
    /// 字符串照样能通过「声明了 uniform」那类检查,只有跑一遍数值
    /// 才会发现开阔地是不是被无端压暗了。
    ///
    /// # Arguments
    ///
    /// - `world_y` - 顶点的世界高度(米)。
    /// - `horizontal` - 顶点到本模型世界原点的 XZ 水平距离(米)。
    ///
    /// # Returns
    ///
    /// 亮度系数 `1.0`(不压暗)..=`CONTACT_SHADOW_FLOOR`(贴基座最暗)。
    pub(crate) fn contact_ao_factor(world_y: f32, horizontal: f32) -> f32 {
        let reach: f32 = CONTACT_SHADOW_REACH;
        let feather: f32 = CONTACT_SHADOW_FEATHER;
        let low: f32 = CONTACT_SHADOW_MIN_HEIGHT;
        let high: f32 = BAKED_CONTACT_AO_HEIGHT;
        let proximity: f32 = 1.0 - smoothstep(reach - feather, reach, horizontal);
        let low_enough: f32 =
            smoothstep(low, low + 0.001, world_y) * (1.0 - smoothstep(high - 0.001, high, world_y));
        1.0 - (1.0 - CONTACT_SHADOW_FLOOR) * low_enough * proximity
    }

    /// 缺陷本体:接触 AO 拿**世界高度**当「离墙多近」的代理量。
    ///
    /// 旧表达式 `mix(u_ao_floor, 1.0, clamp(world.y / u_ao_height, 0, 1))`
    /// 对同一高度的两个顶点给出**完全相同**的压暗 —— 离基座 0.3 m
    /// 的和 40 m 外的都是 `CONTACT_SHADOW_FLOOR`。修好后应是前者更暗、
    /// 后者回到 1.0。
    ///
    /// 实测(2026-10-10,LOFT 样板楼南墙 z=35 外,沿墙法向 1.0 → 6.5 m
    /// 的地面 luma,采样带就是地面本身、干净无车辆):WebGPU
    /// `103.935..109.707`(range 5.77,斜率 -0.431 luma/m)、
    /// WebGL2 `106.750..108.750`(range 2.00,斜率 -0.111 luma/m)。
    /// 两条都是平线,墙根与 6.5 m 外看不出差别。
    #[test]
    pub(crate) fn contact_ao_darkens_by_horizontal_proximity_not_world_height() {
        // 楼身下段,同一高度,只改到基座的水平距离。
        let y: f32 = 0.5;
        let at_base: f32 = contact_ao_factor(y, 0.3);
        let nearby: f32 = contact_ao_factor(y, 3.0);
        let open: f32 = contact_ao_factor(y, 40.0);
        assert!(
            at_base < open - 0.05,
            "同高度 y={y}:离基座 0.3 m 的 {at_base:.4} 必须明显比 40 m 外的 {open:.4} 更暗 —— \
             旧公式按 world.y 算,两者会完全相等"
        );
        assert!(
            nearby < open,
            "离基座 3.0 m 的 {nearby:.4} 应比 40 m 外的 {open:.4} 暗"
        );
        assert!(
            (open - 1.0).abs() < 1e-5,
            "离基座 40 m 已在 CONTACT_SHADOW_REACH={CONTACT_SHADOW_REACH} m 之外,\
             接触 AO 必须完全回到 1.0,实测 {open:.5}"
        );
    }

    /// **地面网格整片不吃顶点接触 AO。**
    ///
    /// 这是用户报「地面的光追效果不对」时最直接的成因:旧公式按
    /// `world.y / u_ao_height` 压暗,而路面 `ROAD = 0.0`、人行道
    /// `SIDEWALK = 0.14`、地块 `LOT_GROUND = 0.16`(取自 `game.rs`
    /// 的 `build_ground_near`)全都远低于
    /// `BAKED_CONTACT_AO_HEIGHT = 1.2`,于是全城每一块地面都取到
    /// `clamp(0) = 0` 吃满最深压暗 —— 墙根与开阔街道一样暗。
    ///
    /// 修法是加高度下界 `CONTACT_SHADOW_MIN_HEIGHT`(0.30 m,高于
    /// 地面网格最高点),让这一项只负责「楼身下段 / 树干根部」的自遮蔽;
    /// 地面贴墙的暗部交给 SSAO —— 顶点着色器结构上不知道墙在哪。
    #[test]
    pub(crate) fn contact_ao_leaves_the_ground_mesh_untouched() {
        for (name, y) in [("road", 0.0_f32), ("sidewalk", 0.14), ("lot", 0.16)] {
            // 即便就站在一栋楼正下方(水平距离 0),也必须是 1.0。
            let value: f32 = contact_ao_factor(y, 0.0);
            assert!(
                (value - 1.0).abs() < 1e-5,
                "{name}(y={y})属于地面网格,不该吃顶点接触 AO —— 实测 {value:.5}; \
                 CONTACT_SHADOW_MIN_HEIGHT={CONTACT_SHADOW_MIN_HEIGHT} 本该把它整个排除"
            );
        }
        assert!(
            CONTACT_SHADOW_MIN_HEIGHT > 0.16,
            "高度下界 {CONTACT_SHADOW_MIN_HEIGHT} 必须高于地面网格最高点 0.16 \
             (LOT_GROUND),否则路面仍会被无端压暗"
        );
    }

    /// 高处顶点不吃「贴墙根」的压暗。
    ///
    /// 若垂直上界失效,一栋 40 m 高的楼会整条压暗,表现为
    /// 「每栋楼都自带一圈黑边」。上界由 `BAKED_CONTACT_AO_HEIGHT` 封顶。
    #[test]
    pub(crate) fn contact_ao_skips_the_proximity_term_above_the_height_band() {
        let y: f32 = BAKED_CONTACT_AO_HEIGHT * 4.0;
        let value: f32 = contact_ao_factor(y, 0.0);
        assert!(
            (value - 1.0).abs() < 1e-5,
            "y={y:.2}(远超 BAKED_CONTACT_AO_HEIGHT={BAKED_CONTACT_AO_HEIGHT})且贴着基座时\
             接触 AO 必须是 1.0,实测 {value:.5}"
        );
    }

    /// SSAO 必须真的采到东西 —— 修之前是 `ao == 1.0`(一帧遮蔽都没有)。
    ///
    /// 缺陷本体:`view_position()` 返回**视空间米**,而调用方传的是
    /// **归一化**深度(`depth / far`),于是原点小了两个数量级;同时
    /// 半径写的是 `u_radius / max(depth * u_far, 0.5)` —— 按米算的
    /// 半径除以了一个已经含 `far` 的量。两处一叠加,采样点被甩出
    /// `uv` 的 `0..=1`,8 个 kernel 方向**一个都活不下来**,`occlusion`
    /// 恒 0、SSAO 恒输出 1.0:整条管线静默空转,而且没有任何编译或
    /// 运行错误。
    ///
    /// 这里按修好的公式(`linear_depth = depth * u_far`,`radius = u_radius`)
    /// 在常见视距上复算一遍,断言样本点仍落在画面内。
    #[test]
    pub(crate) fn ssao_keeps_its_kernel_samples_inside_the_frame() {
        let far: f32 = 600.0; // FAR_PLANE
        let radius: f32 = SSAO_RADIUS;
        // u_proj_params = (tan(fov/2) * aspect, tan(fov/2));取 70° fov、16:9。
        let proj_x: f32 = 1.1547 * (16.0 / 9.0);
        let proj_y: f32 = 1.1547;
        // 中心像素的 ndc 是 0 —— 最容易出事的那个位置。
        for distance in [2.0_f32, 5.0, 10.0, 25.0, 50.0] {
            // 归一化深度 → 米。
            let linear: f32 = (distance / far) * far;
            let origin: [f32; 3] = [0.0, 0.0, -linear];
            let mut kept: usize = 0;
            for i in 0..8 {
                let fi: f32 = i as f32 + 0.5;
                let phi: f32 = fi * 2.399_963_2;
                let cos_theta: f32 = (1.0 - fi / 8.0).sqrt();
                let sin_theta: f32 = (fi / 8.0).sqrt();
                let dir: [f32; 3] = [phi.cos() * sin_theta, phi.sin() * sin_theta, cos_theta];
                let sv: [f32; 3] = [
                    origin[0] + dir[0] * radius,
                    origin[1] + dir[1] * radius,
                    origin[2] + dir[2] * radius,
                ];
                // ⚠️ 这一步必须与 `view_position()` 的映射**互逆**:
                // `view_position` 写的是 `view.xy = ndc.xy * u_proj_params * d`,
                // 所以反投影要**除以** `u_proj_params`。原着色器漏了这个除法 ——
                // 采样点被放大到 `u_proj_params`(≈ 2.05)倍,大量样本落到画面外
                // 被 `continue` 丢掉。这是第三个独立缺陷:即使前两处单位都改对,
                // 只活下来的样本依然不足以构成有意义的遮蔽。
                let uv: [f32; 2] = [
                    sv[0] / (-sv[2]).max(1e-3) / proj_x * 0.5 + 0.5,
                    sv[1] / (-sv[2]).max(1e-3) / proj_y * 0.5 + 0.5,
                ];
                if uv[0] >= 0.0 && uv[0] <= 1.0 && uv[1] >= 0.0 && uv[1] <= 1.0 {
                    kept += 1;
                }
            }
            assert!(
                kept >= 6,
                "D={distance:.0} m 处 8 个 kernel 方向只活下来 {kept} 个 —— \
                 SSAO 又会退化成输出 1.0 的空转(SSAO_RADIUS={radius})"
            );
        }
    }

    /// 顶点布局:`build_gpu_mesh` 每个顶点必须正好写满 `STRIDE_FLOATS`。
    ///
    /// 这是 PBR 改造最容易静默破坏的地方:少写两个 f32 不会编译报错,
    /// 只会在 VAO 读取 `roughness|metallic` 时越界取到邻居顶点的值 ——
    /// 表现是随机几个面变镜面,和「GPU 崩了」完全不像,极难定位。
    /// 最后一个 f32 必须是金属度,且缺省为介电质。
    ///
    /// # Arguments
    ///
    /// 无。
    ///
    /// # Returns
    ///
    /// - 无:断言失败即 panic。
    #[test]
    pub(crate) fn pbr_vertex_layout_is_exactly_stride_wide() {
        // 一个只含一个三角形的资产,uv/切线/粗糙度/金属度全部缺省,
        // 逼 expand_asset 走程序化投影与推导切线的兜底分支。
        let asset: crate::mesh::MeshAsset = crate::mesh::MeshAsset {
            id: "pbr_probe".to_string(),
            category: crate::r#config::TEST_CATEGORY.to_string(),
            y_up: true,
            bounds: None,
            parts: vec![crate::mesh::MeshPart {
                name: "probe".to_string(),
                base_color: Some([0.5, 0.5, 0.5]),
                emissive: None,
                positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
                normals: None,
                uv: None,
                tangents: None,
                roughness: None,
                metallic: None,
                texture_atlas: None,
                surface_kind: None,
                faces: vec![[0, 1, 2]],
                face_colors: None,
                flat: true,
            }],
        };
        let mesh: crate::mesh::GpuMesh = expand_asset(&asset).expect("probe asset expands");
        // 每个顶点 9 个紧凑 f32(位置/法线/颜色)。
        assert_eq!(
            mesh.get_vertices_ref().len(),
            9 * 3,
            "expand_asset 每顶点必须写 9 个 f32"
        );
        // 每个顶点 8 个 PBR f32(uv2|切线4|粗糙1|金属1)。
        assert_eq!(
            mesh.pbr.len(),
            crate::mesh::FLOATS_PER_PBR_VERTEX * 3,
            "expand_asset 每顶点必须写 8 个 PBR f32"
        );
        let gpu: super::MeshAssetGpu = super::build_gpu_mesh(&mesh, &[]);
        assert_eq!(
            gpu.vertices.len(),
            super::STRIDE_FLOATS * 3,
            "GPU 顶点缓冲必须是 STRIDE_FLOATS × 顶点,否则 VAO 会越界读"
        );
        // 最后两个 f32 必须是粗糙度 / 金属度,且落在缺省范围内。
        let rough: f32 = gpu.vertices[super::STRIDE_FLOATS - 2];
        let metal: f32 = gpu.vertices[super::STRIDE_FLOATS - 1];
        assert!(
            (crate::mesh::DEFAULT_ROUGHNESS..=1.0).contains(&rough),
            "缺省粗糙度应回落到 DEFAULT_ROUGHNESS,实际 {rough}"
        );
        assert_eq!(metal, crate::mesh::DEFAULT_METALLIC, "缺省金属度应是介电质");
    }

    /// 金属度必须真的改变着色结果,否则 PBR 只是多算了几个没用的数。
    ///
    /// 断言放在数据侧:`expand_asset` 要把 `roughness` / `metallic`
    /// 原样透传到 PBR 列。着色器分支本身要靠 GPU 采样验证,数据侧能
    /// 保证「值确实到位」,不会被一次着色器改动悄悄架空。
    #[test]
    pub(crate) fn pbr_scalars_survive_expansion() {
        let asset: crate::mesh::MeshAsset = crate::mesh::MeshAsset {
            id: "pbr_metal".to_string(),
            category: crate::r#config::TEST_CATEGORY.to_string(),
            y_up: true,
            bounds: None,
            parts: vec![crate::mesh::MeshPart {
                name: "chrome".to_string(),
                base_color: Some([0.8, 0.8, 0.9]),
                emissive: None,
                positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                normals: None,
                uv: None,
                tangents: None,
                // 车漆 / 铬件那一档:低粗糙度 + 高金属度。
                roughness: Some(0.08),
                metallic: Some(0.85),
                texture_atlas: None,
                surface_kind: None,
                faces: vec![[0, 1, 2]],
                face_colors: None,
                flat: true,
            }],
        };
        let mesh: crate::mesh::GpuMesh = expand_asset(&asset).expect("metal expands");
        let last: usize = mesh.pbr.len() - 1;
        assert_eq!(mesh.pbr[last - 1], 0.08, "粗糙度必须原样透传");
        assert_eq!(mesh.pbr[last], 0.85, "金属度必须原样透传");
    }
}
