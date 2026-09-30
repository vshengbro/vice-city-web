//! 室内垂直碰撞:楼板 / 隔墙 / 楼梯的地面支撑与水平分离。
//!
//! [`crate::collision`] 的整个体系是**二维的**(XZ 平面,没有 Y),它服务
//! 于「玩家圆 vs 建筑 AABB」和「车辆 vs 车道」这两类需求。这里要解决的是
//! 另一个维度的问题 —— 玩家在多��层建筑里的**高度**:站在哪块楼板上、
//! 能不能踏上一级台阶、会不会从二楼掉下去。
//!
//! 所以这里刻意**不复用** `CollisionWorld`,而是并行定义 [`FloorWorld`]:
//! 把 `CollisionWorld::resolve_with_radius` 的接口原样保留,已有的几十个
//! 调用方一个都不用改;室内玩家额外查一次 `FloorWorld`,两者叠加出
//! 「能进楼、能上楼、不会穿墙」的行为。
//!
//! 三个要点:
//!
//! 1. [`Floor::Slab`] 与 [`Floor::Wall`] 都带完整的 Y 区间,于是「头顶的
//!    楼板」和「脚边的墙」可以被同一个圆形分离器区分开:身体区间够不到
//!    楼板就不推,够得到就推。
//! 2. [`FloorWorld::support_height`] 选「不超过 `from_y + 容差` 的最高一块
//!    板」,不是「脚下最近的一块板」。这一条差别就是楼梯能走上��的原因:
//!    一级 0.305 m 的台阶在容差内,于是走过去就自动抬升。
//! 3. 分��是纯水平(XZ)的,垂直方向交给重力 —— 不会把玩家「吸」到墙上。

use crate::r#type::{Vec2, Vec3};

/// 水平分离的迭代轮数(圆心站在墙角上时单轮会残留一点)。
const RESOLVE_ITERATIONS: usize = 4;

/// 判定「圆心落在盒内部」的向量长度阈值(米)。
const INSIDE_EPSILON: f32 = 1e-5;

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

/// 一栋可进入建筑的全部室内碰撞体。
#[derive(Clone, Debug)]
pub struct FloorWorld {
    /// 全部楼板与隔墙。
    floors: Vec<Floor>,
}

impl FloorWorld {
    /// 新建一个空的室内碰撞世界。
    ///
    /// # Returns
    ///
    /// - `Self` - 不含任何楼板与隔墙的室内碰撞世界。
    pub fn new() -> Self {
        Self { floors: Vec::new() }
    }

    /// 全部室内碰撞体的只读视图。
    ///
    /// # Returns
    ///
    /// - `&[Floor]` - 楼板与隔墙表。
    pub fn get_floors(&self) -> &[Floor] {
        &self.floors
    }

    /// 追加一块水平楼板(玩家站在它的 `max[1]` 面上)。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 楼板的世界空间下角。
    /// - `Vec3` - 楼板的世界空间上角。
    pub fn push_slab(&mut self, min: Vec3, max: Vec3) {
        self.floors.push(Floor::Slab { min, max });
    }

    /// 追加一段竖直隔墙。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 墙脚的世界空间下角。
    /// - `Vec3` - 墙顶的世界空间上角。
    pub fn push_wall(&mut self, min: Vec3, max: Vec3) {
        self.floors.push(Floor::Wall { min, max });
    }

    /// 玩家脚下应该踩的高度:在「不高于 `from_y + STEP_UP_TOLERANCE`」的
    /// 楼板里取**最高的一块**。
    ///
    /// 楼梯就是靠这条规则成立的:一级台阶比脚底高 0.305 m,在容差内,所以
    /// 走上去时被抬到台阶面;下一级再抬一级。反过来,从二楼往下看,低于
    /// 脚底的楼板一律不选 —— 于是走到楼板边缘会真的掉下去。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 玩家世界 XZ 坐标(用圆心,不用足迹)。
    /// - `f32` - 玩家当前脚底高度(米)。
    ///
    /// # Returns
    ///
    /// - `Option<f32>` - 该 XZ 点上的可站高度(米);下方没有任何楼板时为
    ///   `None`(调用方按「站在地面 y = 0」处理)。
    pub fn support_height(&self, point: Vec2, from_y: f32) -> Option<f32> {
        let ceiling: f32 = from_y + STEP_UP_TOLERANCE;
        let mut best: Option<f32> = None;
        for floor in self.get_floors() {
            let Floor::Slab { min, max } = floor else {
                continue;
            };
            if point[0] < min[0] || point[0] > max[0] {
                continue;
            }
            if point[1] < min[2] || point[1] > max[2] {
                continue;
            }
            let top: f32 = max[1];
            if top > ceiling {
                continue;
            }
            if best.is_none_or(|current: f32| top > current) {
                best = Some(top);
            }
        }
        best
    }

    /// 把一个圆形身体推出所有与它身体区间相交的隔墙与楼板。
    ///
    /// 只有**竖直方向**与身体区间 `[body_min_y, body_max_y]` 有交集的碰撞
    /// 体才参与:站在一楼时头顶的二楼楼板(下表面 2.95 m)与身体(0.15 ~
    /// 1.90 m)不相交,不会被推开;走上楼梯、脑袋快要顶到楼板时才会生效。
    ///
    /// 分离方式是纯 XZ 的「圆心推到最近面」,和
    /// [`crate::collision::CollisionWorld::resolve_with_radius`] 同一套:
    /// 迭代若干轮直到一轮没有任何形状推动圆心为止。**不动 Y** —— 垂直
    /// 方向是重力的职责,这里越权会把玩家吸到墙面上。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待分离的世界 XZ 坐标。
    /// - `f32` - 身体底面高度(脚底,米)。
    /// - `f32` - 身体顶面高度(头顶,米)。
    /// - `f32` - 身体等效圆柱半径(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离后的世界 XZ 坐标。
    pub fn resolve_interior(
        &self,
        point: Vec2,
        body_min_y: f32,
        body_max_y: f32,
        radius: f32,
    ) -> Vec2 {
        let mut current: Vec2 = point;
        for _ in 0..RESOLVE_ITERATIONS {
            let mut moved: bool = false;
            for floor in self.get_floors() {
                let (min, max): (Vec3, Vec3) = match floor {
                    Floor::Slab { min, max } => (*min, *max),
                    Floor::Wall { min, max } => (*min, *max),
                };
                // 竖直区间不相交就整块跳过:头顶的楼板不该推人,脚底的
                // 楼板也不该(人在楼上时它在身体下方)。
                if max[1] <= body_min_y || min[1] >= body_max_y {
                    continue;
                }
                // 楼板还多一条规则:脚底已经站到板面上(或者差一点点就站
                // 上去)时不许再横向推。否则首层地板会变成一圈看不见的
                // 围墙 —— 玩家站在街上 y = 0,身体区间与 0.15 m 高的地板
                // 相交,会被推着绕楼一圈,门洞永远走不进去。正确的做法是
                // 让 `support_height` 把人**抬上去**。
                // 楼板只在「脚已经站上去了」时跳过横向分离。这里必须是
                // `>=`,不能是 `==` 附近的比较:玩家踩在第 N 级踏面上时
                // 脚底 y 恰好等于该级的 max[1],而 `support_height` 每次都
                // 把人放到**恰好**这个高度,于是条件成立、正常跳过。
                //
                // 但一旦人还在往上跳的半空(脚底比踏面**低**几毫米),旧写法
                // 就会把这个楼板当成实体墙,`push_out_aabb` 取最小松弛方向
                // 把人从**侧面**弹出去 —— 上楼梯时正好卡在两级之间触发,
                // 人被弹到楼梯侧面外的中厅(y 从 1.065 一路掉回 0.15),
                // 于是「楼梯有台阶但永远上不去」。所以这里只认「脚底
                // 已经在板面之上」这一种情况。
                // 楼板永远不该把人**从侧面**推走。
                //
                // 判据不是「脚底有没有站到板面上」,而是「脚底是否已经
                // 高过这块板」——只要人还在板面以下或齐平,这块板就是他
                // 脚下要踩的东西(或者下一级台阶),横向分离只应该由
                // **墙**来做。早先只看 `body_min_y >= max[1] - TOLERANCE`,
                // 于是人站在第 N 级踏面、还没跨上第 N+1 级的那一瞬间
                // (脚底比 N+1 级踏面低一个踏高)会被当成撞上实体,
                // `push_out_aabb` 取最小松弛方向把人从楼梯**侧边**弹出去 ——
                // 表现就是上到一半 y 突然从 1.065 掉回 0.15,楼梯永远上不去。
                //
                // 代价:一层那圈 0.15 m 高的地板也不再横向推人。这本来是
                // 好事(否则首层地板会变成看不见的围墙,门洞走不进去),
                // 由 `support_height` 负责把人**抬**上去。
                //
                // **楼板(踏面)永远不做横向分离。** 玩家踩在第 N 级踏面上时,
                // 身体的竖直区间会与第 1..N 级的每一级相交,而人又正好站在
                // 那一级的 XZ 盒子里 —— `push_out_aabb` 于是走「点在盒内」
                // 分支,按 `slack_x <= slack_z` 取**最小松弛轴**,把人从
                // 楼梯的**侧面**弹出去(踏面宽 1.30 m,人这一弹直接掉到
                // 中厅,实测本地 x 5.03 → 4.10、y 从 1.065 掉回 0.15)。
                // 表现就是:楼梯有台阶、能踩两级,然后永远上不去。
                //
                // 所以踏面只提供**竖直支撑**(`support_height` 负责把人
                // 抬上去),横向只由 `Wall` 挡。一层那块 0.15 m 的地板因此
                // 也不再横向推人 —— 这本来就想要,否则首层地板会变成一圈
                // 看不见的围墙,门洞永远走不进去。
                if let Floor::Slab { .. } = floor {
                    continue;
                }

                let center: Vec2 = [(min[0] + max[0]) * 0.5, (min[2] + max[2]) * 0.5];
                let half: Vec2 = [(max[0] - min[0]) * 0.5, (max[2] - min[2]) * 0.5];
                let Some((direction, depth)) = push_out_aabb(center, half, current, radius) else {
                    continue;
                };
                current[0] += direction[0] * depth;
                current[1] += direction[1] * depth;
                moved = true;
            }
            if !moved {
                break;
            }
        }
        current
    }

    /// 点是否落在某个室内碰撞体的投影里(调试探针用,不做分离)。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 世界 XZ 坐标。
    /// - `f32` - 该点的高度(米)。
    ///
    /// # Returns
    ///
    /// - `bool` - 该 XZ 点落在某个碰撞体的竖直区间内时为 `true`。
    pub fn contains_interior_point(&self, point: Vec2, y: f32) -> bool {
        self.get_floors().iter().any(|floor: &Floor| {
            let (min, max): (Vec3, Vec3) = match floor {
                Floor::Slab { min, max } => (*min, *max),
                Floor::Wall { min, max } => (*min, *max),
            };
            if y < min[1] || y > max[1] {
                return false;
            }
            point[0] >= min[0] && point[0] <= max[0] && point[1] >= min[2] && point[1] <= max[2]
        })
    }
}

impl Default for FloorWorld {
    /// 返回空室内碰撞世界,与 `FloorWorld::new` 等价。
    fn default() -> Self {
        FloorWorld::new()
    }
}

/// 踏上一级台阶的最大高度(米)。
///
/// 室内楼梯每级 0.305 m,这里留到 0.45 m:既高于单级台阶(走上去会被抬),
/// 又远低于门洞 2.30 m 与层高 3.05 m(所以不会一步跨上二层楼板)。
pub const STEP_UP_TOLERANCE: f32 = 0.45;

/// 圆形 vs 轴对齐盒的分离量(只读 XZ)。
///
/// # Arguments
///
/// - `Vec2` - 盒中心的世界 XZ 坐标。
/// - `Vec2` - 盒的半尺寸(米)。
/// - `Vec2` - 圆心的世界 XZ 坐标。
/// - `f32` - 圆半径(米)。
///
/// # Returns
///
/// - `Option<(Vec2, f32)>` - `(推出方向, 推出距离)`;不重叠时为 `None`。
fn push_out_aabb(center: Vec2, half: Vec2, point: Vec2, radius: f32) -> Option<(Vec2, f32)> {
    let closest: Vec2 = [
        point[0].clamp(center[0] - half[0], center[0] + half[0]),
        point[1].clamp(center[1] - half[1], center[1] + half[1]),
    ];
    let delta: Vec2 = [point[0] - closest[0], point[1] - closest[1]];
    let distance: f32 = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
    if distance > radius {
        return None;
    }
    if distance > INSIDE_EPSILON {
        return Some((
            [delta[0] / distance, delta[1] / distance],
            radius - distance,
        ));
    }
    let offset: Vec2 = [point[0] - center[0], point[1] - center[1]];
    let slack_x: f32 = half[0] - offset[0].abs();
    let slack_z: f32 = half[1] - offset[1].abs();
    if slack_x <= slack_z {
        let sign: f32 = if offset[0] >= 0.0 { 1.0 } else { -1.0 };
        Some(([sign, 0.0], radius + slack_x))
    } else {
        let sign: f32 = if offset[1] >= 0.0 { 1.0 } else { -1.0 };
        Some(([0.0, sign], radius + slack_z))
    }
}

#[cfg(test)]
mod tests {
    use crate::r#const::{
        T_INTERIOR_CEILING_INSIDE, T_INTERIOR_DOORWAY_BLOCKS, T_INTERIOR_DOORWAY_THROUGH,
        T_INTERIOR_NO_SLAB_UNDER, T_INTERIOR_STAIR_CLIMBS, T_INTERIOR_STAIR_MONOTONIC,
        T_INTERIOR_STAIR_PAIR, T_INTERIOR_UPPER_FLOOR_ABOVE, T_INTERIOR_WALL_BLOCKS,
        T_INTERIOR_WALL_PASSES,
    };
    use crate::interior::{FloorWorld, STEP_UP_TOLERANCE};
    use crate::r#type::Vec2;

    const STAIR_RISE: f32 = 0.305;
    const STAIR_RUN: f32 = 0.45;
    const STAIR_STEPS: usize = 10;
    const GROUND_TOP: f32 = 0.15;
    const UPPER_TOP: f32 = 3.20;
    const UPPER_BOT: f32 = 2.95;
    const DOOR_HALF: f32 = 0.80;
    const DOOR_TOP: f32 = 2.45;
    const WALL_T: f32 = 0.25;
    const BODY_TOP: f32 = 1.90;
    const RADIUS: f32 = 0.35;

    fn slab_world() -> FloorWorld {
        let mut w = FloorWorld::new();
        w.push_slab([-5.75, 0.0, -4.75], [5.75, GROUND_TOP, 4.75]);
        w.push_slab([-5.75, UPPER_BOT, -4.75], [4.45, UPPER_TOP, 4.75]);
        for i in 0..STAIR_STEPS {
            let y0 = -4.55 + i as f32 * STAIR_RUN;
            let top = GROUND_TOP + (i + 1) as f32 * STAIR_RISE;
            w.push_slab([4.45, GROUND_TOP, y0], [5.75, top, y0 + STAIR_RUN]);
        }
        w
    }

    fn height_at(w: &FloorWorld, x: f32, z: f32, from_y: f32) -> Option<f32> {
        w.support_height([x, z], from_y)
    }

    fn step_along_stair(w: &FloorWorld) -> Vec<f32> {
        let mut out: Vec<f32> = Vec::new();
        let mut y = GROUND_TOP;
        for i in 0..STAIR_STEPS {
            let z = -4.55 + (i as f32 + 0.5) * STAIR_RUN;
            y = height_at(w, 5.1, z, y).unwrap_or(y);
            out.push(y);
        }
        out
    }

    #[test]
    fn stair_ramp_returns_increasing_heights() {
        let w = slab_world();
        let heights = step_along_stair(&w);
        assert_eq!(heights.len(), STAIR_STEPS, "{}", T_INTERIOR_STAIR_MONOTONIC);
        for pair in heights.windows(2) {
            assert!(pair[1] > pair[0], "{}", T_INTERIOR_STAIR_PAIR);
        }
        assert!(
            (heights[STAIR_STEPS - 1] - UPPER_TOP).abs() < 1e-4,
            "{}",
            T_INTERIOR_STAIR_CLIMBS
        );
    }

    #[test]
    fn stair_tolerance_exceeds_one_rise_and_one_storey() {
        assert!(
            STEP_UP_TOLERANCE > STAIR_RISE,
            "{}",
            T_INTERIOR_STAIR_CLIMBS
        );
        assert!(
            STEP_UP_TOLERANCE < UPPER_TOP - GROUND_TOP,
            "{}",
            T_INTERIOR_UPPER_FLOOR_ABOVE
        );
    }

    #[test]
    fn wall_between_two_rooms_blocks_movement() {
        let mut w = FloorWorld::new();
        w.push_slab([-5.0, 0.0, -5.0], [5.0, GROUND_TOP, 5.0]);
        w.push_wall([-2.0, GROUND_TOP, -0.10], [2.0, 2.75, 0.10]);
        let pushed = w.resolve_interior([0.0, 0.0], GROUND_TOP, BODY_TOP, RADIUS);
        assert!(pushed[1].abs() > 0.1, "{}", T_INTERIOR_WALL_BLOCKS);
        let away = w.resolve_interior([0.0, 2.0], GROUND_TOP, BODY_TOP, RADIUS);
        assert!((away[1] - 2.0).abs() < 1e-4, "{}", T_INTERIOR_WALL_PASSES);
    }

    #[test]
    fn slab_above_head_does_not_block_player() {
        let w = slab_world();
        let inside = w.resolve_interior([0.0, 0.0], GROUND_TOP, BODY_TOP, RADIUS);
        assert!(
            (inside[0] - 0.0).abs() < 1e-4 && (inside[1] - 0.0).abs() < 1e-4,
            "{}",
            T_INTERIOR_CEILING_INSIDE
        );
    }

    #[test]
    fn doorway_gap_is_the_only_way_through_the_front_wall() {
        let mut w = FloorWorld::new();
        w.push_slab([-5.0, 0.0, -5.0], [5.0, GROUND_TOP, 5.0]);
        let y_in = -5.0 + WALL_T;
        w.push_wall([-5.0, 0.0, y_in - WALL_T], [-DOOR_HALF, 6.4, y_in]);
        w.push_wall([DOOR_HALF, 0.0, y_in - WALL_T], [5.0, 6.4, y_in]);
        w.push_wall(
            [-DOOR_HALF, DOOR_TOP, y_in - WALL_T],
            [DOOR_HALF, 6.4, y_in],
        );
        let through = w.resolve_interior([0.0, y_in], GROUND_TOP, BODY_TOP, RADIUS);
        assert!(
            (through[1] - y_in).abs() < 1e-3,
            "{}",
            T_INTERIOR_DOORWAY_THROUGH
        );
        let blocked = w.resolve_interior([DOOR_HALF + 0.1, y_in], GROUND_TOP, BODY_TOP, RADIUS);
        assert!(blocked[1] > y_in, "{}", T_INTERIOR_DOORWAY_BLOCKS);
        let lintel = w.resolve_interior([0.0, y_in], GROUND_TOP, 3.0, RADIUS);
        assert!(lintel[1] > y_in, "{}", T_INTERIOR_DOORWAY_BLOCKS);
    }

    #[test]
    fn support_height_reports_the_drop_when_walking_off_a_ledge() {
        let w = slab_world();
        // 站在二楼高度、脚下已经没有二层板的位置(楼梯井):仍然只有
        // 0.15 m 的首层板够得着,返回值就是那 3.05 m 的落差。调用方据此
        // 判断「该走上去」还是「该掉下去」。
        let well: Vec2 = [5.1, 2.0];
        let upper: Option<f32> = height_at(&w, well[0], well[1], UPPER_TOP);
        assert_eq!(upper, Some(GROUND_TOP), "{}", T_INTERIOR_NO_SLAB_UNDER);
        // 二层板之下再看:只有首层板够得着,这就是「该掉下去」的落差。
        assert_eq!(
            height_at(&w, 0.0, 0.0, GROUND_TOP + STEP_UP_TOLERANCE),
            Some(GROUND_TOP),
            "{}",
            T_INTERIOR_NO_SLAB_UNDER
        );
        // 二层板之上:容差内仍然踩得到二楼板面,不再往回首层。
        assert_eq!(
            height_at(&w, -5.0, -3.0, UPPER_TOP),
            Some(UPPER_TOP),
            "{}",
            T_INTERIOR_NO_SLAB_UNDER
        );
        // 楼板外侧什么都没有。
        assert!(
            height_at(&w, 40.0, 40.0, GROUND_TOP).is_none(),
            "{}",
            T_INTERIOR_NO_SLAB_UNDER
        );
    }

    #[test]
    fn probe_reports_containment_by_height() {
        let w = slab_world();
        let on_ground: Vec2 = [0.0, 0.0];
        assert!(w.contains_interior_point(on_ground, GROUND_TOP * 0.5));
        assert!(!w.contains_interior_point(on_ground, 1.5));
        let under_stair: Vec2 = [5.1, -4.0];
        assert!(w.contains_interior_point(under_stair, 0.5));
    }

    #[test]
    fn outside_the_footprint_there_is_no_support() {
        let w = slab_world();
        let far: Vec2 = [40.0, 40.0];
        assert!(
            w.support_height(far, GROUND_TOP).is_none(),
            "{}",
            T_INTERIOR_NO_SLAB_UNDER
        );
        assert!(
            w.resolve_interior(far, GROUND_TOP, BODY_TOP, RADIUS) == far,
            "{}",
            T_INTERIOR_WALL_PASSES
        );
    }
}
