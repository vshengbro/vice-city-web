use super::*;

/// 室内垂直碰撞体:一块楼板或一段隔墙。
#[derive(Clone, Copy, Debug)]
pub enum Floor {
    /// 一块水平可站立的楼板 —— 玩家**站在它上面**。
    Slab {
        /// 楼板的世界空间下角(X, Y, Z)。
        min: Vec3,
        /// 楼板的世界空间上角(X, Y, Z)。
        max: Vec3,
    },
    /// 一段竖直的墙,Y 方向有区间,用来分隔房间。
    Wall {
        /// 墙脚的世界空间下角(X, Y, Z)。
        min: Vec3,
        /// 墙顶的世界空间上角(X, Y, Z)。
        max: Vec3,
    },
}
