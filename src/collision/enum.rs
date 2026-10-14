use super::*;

/// 一个静态碰撞体(投影到世界 XZ 平面)。
#[derive(Clone, Copy, Debug)]
pub enum Shape {
    /// 轴对齐包围盒 —— 建筑 / 车辆 / 长椅这类方块状道具。
    Aabb {
        /// 盒中心的世界 XZ 坐标。
        center: Vec2,
        /// 盒的半尺寸(世界 XZ,米)。
        half: Vec2,
    },
    /// 竖直圆柱 —— 棕榈 / 垃圾桶 / 消防栓这类近似圆形的道具。
    Circle {
        /// 圆柱中心的世界 XZ 坐标。
        center: Vec2,
        /// 圆柱半径(米)。
        radius: f32,
    },
}

/// 一个动态碰撞体的**身份**。
///
/// 分离力的大小按质量比分配(见 [`CollisionWorld::resolve_dynamic`]),
/// 所以「谁在撞谁」必须可判定 —— 同一类实体之间同样要分开,否则两个人
/// 走在一起会互相穿过去。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BodyKind {
    /// 玩家角色。
    Player,
    /// 交通车辆。
    Car,
    /// 行人。
    Pedestrian,
    /// 敌对 / 警察 NPC。
    Enemy,
}
