use super::*;
/// 一个列主序(mat4 GLSL 约定)的 4x4 矩阵。
///
/// 存储顺序为 `m[col * 4 + row]`,与 GLSL `mat4` 的 uniform 布局一致,
/// 因此可以直接把元素原样喂给 `uniformMatrix4fv`。
#[derive(Data, Clone, Copy, Debug, PartialEq)]
pub struct Mat4 {
    /// 16 个元素,列主序。
    #[get(type(copy), pub(crate))]
    pub(crate) elements: Mat4Data,
}
/// 一台第三人称轨道/跟随相机。
#[derive(Data, Clone, Debug)]
pub struct Camera {
    /// 注视焦点(世界坐标)。
    #[get(type(copy), pub(crate))]
    pub(crate) target: Vec3,
    /// 相机与焦点的距离(米)。
    #[get(type(copy), pub(crate))]
    pub(crate) distance: f32,
    /// 绕 Y 轴方位角(弧度)。0 表示相机在 +Z 侧看向 -Z。
    #[get(type(copy), pub(crate))]
    pub(crate) yaw: f32,
    /// 仰角(弧度),正值表示相机在焦点上方。
    #[get(type(copy), pub(crate))]
    pub(crate) pitch: f32,
    /// 垂直视场角(弧度)。
    #[get(type(copy), pub(crate))]
    pub(crate) fov_y: f32,
    /// 近裁剪面。
    #[get(type(copy), pub(crate))]
    pub(crate) near: f32,
    /// 远裁剪面。
    #[get(type(copy), pub(crate))]
    pub(crate) far: f32,
    /// 玩家自定义的跟随距离(滚轮缩放结果),不受遮挡回避影响。
    ///
    /// 这是「想要多远」;`distance` 是「实际能走多远」。遮挡时
    /// `distance` 被压到 `distance` 射线命中点之前,遮挡消失后再阻尼
    /// 回到 `desired_distance`。分成两个字段是为了让回避**可逆**:
    /// 如果只有一个 `distance`,被拉近之后就没有基准可以回弹了。
    #[set(skip, pub(crate))]
    #[get(type(copy), pub(crate))]
    pub(crate) desired_distance: f32,
    /// 眼点(相机世界位置)到最近静态碰撞体表面的距离(米,0 = 扎在楼里)。
    #[get(type(copy), pub(crate))]
    pub(crate) eye_clearance: f32,
    /// 本帧相机是否被几何体遮挡(射线在到达 `desired_distance` 之前命中)。
    #[get(type(copy), pub(crate))]
    pub(crate) occluded: bool,
}
