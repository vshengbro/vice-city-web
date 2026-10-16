//! 朝向(yaw)约定的**唯一来源**。
//!
//! 资产约定是从几何实测出来的,不是抄 schema:车辆资产的引擎盖
//! `z = +1.380`、尾箱 `z = -1.426`、前保险杠 `z = +2.330`、后保险杠
//! `z = -2.330`(x 全为 0),所以**车头在资产本地 +Z**。`assets/` 下
//! 38 个 +Z 车头的资产、0 个 +X 车头的资产,+Z 是全仓约定。
//!
//! 渲染边界 [`crate::render::Instance::new`] 建的列主序矩阵把资产本地
//! `+Z` 映射到世界 `(sin yaw, 0, cos yaw)`(第 3 列
//! `model[8], model[10]`),本地 `+X` 映射到 `(cos yaw, 0, -sin yaw)`
//! (第 1 列 `model[0], model[2]`)。于是:
//!
//! - **车头 / 模型正面** = [`yaw_forward`] = `[sin yaw, cos yaw]`。
//! - **手性为正的「右」** = [`yaw_right`] = `[cos yaw, -sin yaw]`,
//!   即 `cross(up, fwd)`。
//!
//! 历史上物理侧用的是 `[cos yaw, -sin yaw]` 当「前」,而那正是渲染出来的
//! **本地 +X**,与车头差了 90°:车「横着」开。修法不是改渲染 —— 相机、
//! WASD、瞄准、以及玩家的 [`crate::player::facing_yaw`] 都以
//! `[cos yaw, -sin yaw]` 为移动基准 —— 而是让物理侧改用 [`yaw_forward`],
//! 于是整条链路两端终于一致。
//!
//! `yaw_right` 写成 `cross(up, fwd)` 的形式而不是写死分量,是让它**与
//! `fwd` 的定义解耦**:哪天资产约定或渲染矩阵改了,只有 [`yaw_forward`]
//! 需要动,`yaw_right` 自动跟着对。
use super::*;

/// 资产本地 `+Z`(车头 / 模型正面)在世界 XZ 上的方向。
///
/// 与 [`crate::render::Instance::new`] 的 `model[8], model[10]` 逐位一致,
/// 也与玩家的 [`crate::player::facing_yaw`](`atan2(vx, vz)`)一致 ——
/// 三个都指向同一个车头。
///
/// # Arguments
///
/// - `f32` - 绕 Y 轴的朝向(弧度)。
///
/// # Returns
///
/// - `Vec2` - `(sin yaw, cos yaw)`,模长恒为 1。
pub(crate) fn yaw_forward(yaw: f32) -> Vec2 {
    let (sin_yaw, cos_yaw): (f32, f32) = yaw.sin_cos();
    [sin_yaw, cos_yaw]
}

/// 绕 Y 轴朝 `yaw` 时的世界「右」方向(XZ 分量)。
///
/// 定义为 `cross(up, fwd)`,其中 `up = (0, 1, 0)`、`fwd = yaw_forward(yaw)`。
/// 于是它恒为 `yaw_forward` 顺时针转 90°:`[fwd[1], -fwd[0]]`。
///
/// 写成由 `fwd` 推出而不是直接给分量,是为了让「右」的定义**不可能**
/// 与「前」脱节 —— 历史上正是两处各写一份分量,差了一个符号。
///
/// # Arguments
///
/// - `f32` - 绕 Y 轴的朝向(弧度)。
///
/// # Returns
///
/// - `Vec2` - `(cos yaw, -sin yaw)`,模长恒为 1。
pub(crate) fn yaw_right(yaw: f32) -> Vec2 {
    let fwd: Vec2 = yaw_forward(yaw);
    [fwd[1], -fwd[0]]
}
