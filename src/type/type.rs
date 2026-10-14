/// 三维向量 / 坐标 / 颜色分量,等价于 `[f32; 3]`。
///
/// # Returns
///
/// - `Self` - 三个 `f32` 分量。
pub type Vec3 = [f32; 3];

/// 四维向量,等价于 `[f32; 4]`。
///
/// # Returns
///
/// - `Self` - 四个 `f32` 分量。
pub type Vec4 = [f32; 4];

/// 列主序 4×4 矩阵的扁平存储,等价于 `[f32; 16]`。
///
/// 列主序 = `matrix[col][row]`,即 `get_elements()[col * 4 + row]`。
/// 可以直接喂给 `uniformMatrix4fv`,无需转置。
///
/// # Returns
///
/// - `Self` - 十六个 `f32`,按列主序排列。
pub type Mat4Data = [f32; 16];

/// 二维向量(X / Z 地面坐标),等价于 `[f32; 2]`。
///
/// # Returns
///
/// - `Self` - 两个 `f32` 分量。
pub type Vec2 = [f32; 2];

/// 棕榈树的世界 XZ 坐标列表,等价于 `Vec<[f32; 2]>`。
///
/// 同样是为了避开 §2.2 Layer 4 的 `;` 截断。
///
/// # Returns
///
/// - `Self` - 每个元素是一组 (x, z, scale)。
///
/// 第三项是**逐棵独立的缩放**:整片树同高就是用户报的「高度同质化」,
/// 而 `Vec2` 放不下。`scale` 直接乘进实例矩阵,不需要额外查表。
pub type PalmSpots = Vec<[f32; 3]>;

/// 场景蓝图里「资产 + 位置 + 朝向」的三元组,等价于
/// `(&'static str, [f32; 3], f32)`。
///
/// 存在的理由与 [`Vec3`] 相同:doc-comment 的签名解析正则会在 `;` 处
/// 截断,所以凡是**嵌套在泛型里**的数组类型都必须先收敛成不含 `;`
/// 的别名,`Vec<Placement>` 的文档与签名才能写同一个名字。
///
/// # Returns
///
/// - `Self` - `(资产 id, 世界坐标, 绕 Y 轴的弧度)`。
pub type Placement = (&'static str, Vec3, f32);

/// 8 位 RGB 三元组,等价于 `[u8; 3]`。
///
/// # Returns
///
/// - `Self` - 三个 `u8` 分量。
pub type Rgb8 = [u8; 3];

/// 顶点数据的轴对齐包围盒,等价于 `[min_x, min_y, min_z, max_x, max_y, max_z]`。
///
/// 用类型别名而不是裸 `[f32; 6]`,是因为裸数组类型里的 `;` 会被
/// `verify_doc_comment_format` 的返回类型解析(`-> ([^{=;]+)`)当成语句
/// 分隔符截断,于是 `# Returns` 里写正确的 `[f32; 6]` 反而被判成
/// 「与签名的 `[f32` 不匹配」。
pub type MeshExtent = [f32; 6];
