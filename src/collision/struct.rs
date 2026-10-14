use super::*;
/// 静态碰撞世界:一组形状 + 世界边界 + 玩家圆半径。
#[derive(Data, Clone, Debug)]
pub struct CollisionWorld {
    /// 全部静态碰撞体。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) shapes: Vec<Shape>,
    /// 玩家圆半径(米)。
    #[get(type(copy), pub(crate))]
    pub(crate) player_radius: f32,
    /// 世界半边长(X / Z 各一半,米),玩家不许走出这个范围。
    #[get(type(copy), pub(crate))]
    pub(crate) half_extent: Vec2,
}
/// 一个参与动态分离的实体(**有向足迹** + 质量)。
///
/// 质量是物理分离的唯一输入:撞静止的车时人会被弹开而车几乎不动,
/// 两辆车相撞则按质量比分配速度。质量单位取「人」的整数倍,便于读数。
///
/// 足迹默认是圆(`[radius, radius]`,yaw = 0);车例外 —— 车把这个字段填成
/// [`CAR_FOOTPRINT`] 并带上朝向。**这一步不是可选的润色:** `resolve_all`
/// 在 `TrafficCar::drive` **之后**再跑一遍静态分离,若它还用 1.25 m 的圆
/// 把车推离墙,`drive` 里刚算好的 2.283 m 会被缩回 1.25 m,车头重新插进
/// 墙里 —— 也就是这个缺陷在同一条调用链上被复活一次。
#[derive(Data, Clone, Copy, Debug)]
pub struct DynamicBody {
    /// 实体类型(决定同类之间是否也要分开)。
    #[get(type(copy), pub(crate))]
    pub(crate) kind: BodyKind,
    /// 足迹中心的世界 XZ 坐标(圆则是圆心)。
    #[get(type(copy), pub(crate))]
    pub(crate) position: Vec2,
    /// 足迹半长 / 半宽(米);圆填 `[radius, radius]`。
    #[get(type(copy), pub(crate))]
    pub(crate) footprint: Vec2,
    /// 足迹朝向(弧度,绕 Y 轴,与 `TrafficCar::get_yaw` 同一约定);
    /// 圆形忽略这一项。
    #[get(type(copy), pub(crate))]
    pub(crate) yaw: f32,
    /// 质量(以「一个人」为单位,1.0 = 一个人)。
    #[get(type(copy), pub(crate))]
    pub(crate) mass: f32,
}
