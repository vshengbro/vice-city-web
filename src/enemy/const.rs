use super::*;

/// 敌人开始「活」的距离(米)。
///
/// 比 [`ENEMY_SIGHT`] 远一截:这样玩家朝某条街冲过去时,警察是**先进入
/// 激活圈、再用视线确认**的,而不是突然从画面边缘冒出来。
pub(crate) const ENEMY_ACTIVATION: f32 = ENEMY_SIGHT * 1.9;

/// 敌人交火时与玩家的理想距离(米):退到近处反而更容易被玩家打死。
pub(crate) const ENEMY_STANDOFF: f32 = 9.0;

/// 敌人「放弃追击」时回头看的距离(米)。
pub(crate) const ENEMY_LEASH: f32 = 34.0;

/// 巡逻半径(米):在自己出生点附近转,不会走丢。
pub(crate) const ENEMY_WANDER_RADIUS: f32 = 7.0;
