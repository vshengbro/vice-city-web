use super::*;
/// 一个玩家的运行时状态。
#[derive(Data, Clone, Debug)]
pub struct Player {
    /// 脚下世界坐标(Y 恒为 0,踩在人行道高度上由场景统一处理)。
    #[get(type(copy), pub(crate))]
    pub(crate) position: Vec3,
    /// 朝向(绕 Y 轴弧度,0 = 面向 +X)。
    #[get(type(copy), pub(crate))]
    pub(crate) yaw: f32,
    /// 当前水平速度(X / Z,米/秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) velocity: Vec2,
    /// 行走相位(弧度)。
    #[get(type(copy), pub(crate))]
    pub(crate) gait_phase: f32,
    /// 当前步态幅度 0..1,由速度平滑驱动。
    #[get(type(copy), pub(crate))]
    pub(crate) gait_amount: f32,
    /// 生命值。
    #[get(type(copy), pub(crate))]
    pub(crate) health: f32,
    /// 护甲值:先于生命值承伤,按比例吸收伤害。
    #[get(type(copy), pub(crate))]
    pub(crate) armor: f32,
    /// 已拾取的现金。
    #[get(type(copy), pub(crate))]
    pub(crate) cash: f32,
    /// 当前驾驶的车辆索引(`None` = 在地面上)。
    #[get(type(copy), pub(crate))]
    pub(crate) vehicle: Option<usize>,
    /// 上一次收到的拾取提示(空串 = 无提示)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) notice: String,
    /// 已被拾取掉的拾取物索引。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) collected: Vec<usize>,
    /// 身体高度(米):脚底到头顶。室内分离靠它判断「够不够得着」一块
    /// 二层楼板 —— 站在一楼时头顶 1.9 m 够不到 2.95 m 的板,所以不会被
    /// 上方的楼板推开。
    #[get(type(copy), pub(crate))]
    pub(crate) height: f32,
    /// 垂直速度(米/秒,向上为正)。踩在楼板上时被清零。
    #[get(type(copy), pub(crate))]
    pub(crate) vertical_velocity: f32,
    /// 脚底是否正踩在某块楼板 / 地面上。
    #[get(type(copy), pub(crate))]
    pub(crate) grounded: bool,
}
