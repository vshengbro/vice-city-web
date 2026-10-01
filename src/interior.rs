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

/// `resolve_interior_slide` 里判定「墙没推我」/「没有切向可留」的阈值(米)。
///
/// 取 1e-3,与 `collision.rs` 的 `SLIDE_EPSILON` 同一个量级:比它再小,
/// 浮点噪声会被当成真实推动,玩家会在墙面上微微抽搐。
const INTERIOR_SLIDE_EPSILON: f32 = 1e-3;

/// `blocks_sight` 里判定「视线与某轴平行」的位移阈值(米)。
const INTERIOR_SIGHT_EPSILON: f32 = 1e-6;

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

    /// 全部室内碰撞体的可变视图。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<Floor>` - 楼板与隔墙表。
    pub fn get_floors_mut(&mut self) -> &mut Vec<Floor> {
        &mut self.floors
    }

    /// 追加一块水平楼板(玩家站在它的 `max[1]` 面上)。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 楼板的世界空间下角。
    /// - `Vec3` - 楼板的世界空间上角。
    pub fn push_slab(&mut self, min: Vec3, max: Vec3) {
        self.get_floors_mut().push(Floor::Slab { min, max });
    }

    /// 追加一段竖直隔墙。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 墙脚的世界空间下角。
    /// - `Vec3` - 墙顶的世界空间上角。
    pub fn push_wall(&mut self, min: Vec3, max: Vec3) {
        self.get_floors_mut().push(Floor::Wall { min, max });
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
        self.separate_interior(point, body_min_y, body_max_y, radius)
    }

    /// [`Self::resolve_interior`] 的**带位移**版本:沿墙保留切向分量。
    ///
    /// 纯分离(不传 `delta`)有个致命的结构缺陷:它只知道「现在在哪儿」,
    /// 不知道「这一帧想去哪儿」。于是玩家顶着隔墙走时,每帧的流程是
    /// 「静态层前进 → 室内层沿法线原路弹回」,**切向位移被整个丢掉**。
    /// 表现就是「有速度、无位移」:顶着墙几十帧推不动一毫米,而且
    /// 斜着走也拐不过弯 —— 因为连「往旁边挪一点」这个动作都传不进去。
    ///
    /// 更糟的是这两层每帧互相抵消,位置在极限环上抖动:实测静态层把
    /// 玩家送到 `z = 35.243`(已经陷进墙里 0.257 m),室内层再弹回
    /// `z = 35.550`,`step_vertical` 把这个结果 `set_position` 写回 ——
    /// 于是静态层下一帧又从墙里出发。静态层**从来不知道墙在这儿**,
    /// 室内层**从来不知道想去哪儿**。
    ///
    /// 传入 `delta` 之后走标准两步 `move_and_slide`(与
    /// [`crate::collision::CollisionWorld::resolve_slide`] 同一套思路):
    /// 先按法线推出穿透深度拿到修正向量,把 `delta` 的法向分量扣掉,
    /// **只保留切向**再走一遍。玩家于是顺着墙滑过去,而不是被钉在
    /// 接触点上。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待分离的世界 XZ 坐标。
    /// - `Vec2` - 本帧想要的水平位移(米);纯分离时传 `[0.0, 0.0]`。
    /// - `f32` - 身体底面高度(脚底,米)。
    /// - `f32` - 身体顶面高度(头顶,米)。
    /// - `f32` - 身体等效圆柱半径(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离(并保留切向位移)之后的世界 XZ 坐标。
    pub fn resolve_interior_slide(
        &self,
        point: Vec2,
        delta: Vec2,
        body_min_y: f32,
        body_max_y: f32,
        radius: f32,
    ) -> Vec2 {
        // 先做一次纯分离,拿到「墙把人推了多远、往哪个方向推」。
        let pushed: Vec2 = self.separate_interior(point, body_min_y, body_max_y, radius);
        let correction: Vec2 = [pushed[0] - point[0], pushed[1] - point[1]];
        let length: f32 = (correction[0] * correction[0] + correction[1] * correction[1]).sqrt();
        // 没有推动 → 人本来就在合法位置,切向位移原样放行。
        if length <= INTERIOR_SLIDE_EPSILON {
            let free: Vec2 = [point[0] + delta[0], point[1] + delta[1]];
            return self.separate_interior(free, body_min_y, body_max_y, radius);
        }
        // 推动方向指向墙的**外侧**,它就是法线。
        let normal: Vec2 = [correction[0] / length, correction[1] / length];
        let into: f32 = delta[0] * normal[0] + delta[1] * normal[1];
        // 没有往墙里钻(只是擦边),分离结果已经正确。
        if into >= 0.0 {
            return pushed;
        }
        // 纯正面顶墙:没有切向可留,分离结果就是最终位置。
        let tangent: Vec2 = [delta[0] - normal[0] * into, delta[1] - normal[1] * into];
        let tangent_len: f32 = (tangent[0] * tangent[0] + tangent[1] * tangent[1]).sqrt();
        if tangent_len <= INTERIOR_SLIDE_EPSILON {
            return pushed;
        }
        // 沿墙走完切向位移之后再分离一次:防止「贴着走时切向一步
        // 跨进了墙里」被下一次迭代当成正常位置。
        let slid: Vec2 = [pushed[0] + tangent[0], pushed[1] + tangent[1]];
        self.separate_interior(slid, body_min_y, body_max_y, radius)
    }

    /// 从 `from` 到 `to` 的水平线段是否被某段隔墙挡住(视线判定用)。
    ///
    /// **为什么必须有这个函数:**`CollisionWorld::has_line_of_sight` 只
    /// 射线检测静态碰撞世界,而**楼板与隔墙住在另一个世界**
    /// ([`FloorWorld`])。两套几何互不相交,所以一栋样板楼在静态世界里
    /// 根本不存在 —— 射线从它中间穿过去,报告「视线通畅」。实测把
    /// 玩家停进楼里(`interiors.inSolid == true`)、19 m 外的警察在
    /// `attack` 状态持续开火,90 帧掉 31.8 血,门控计数器全程为 0。
    ///
    /// 这里用**线段 vs AABB 的 2D  slab 法**做精确相交,而不是采样或
    /// 膨胀盒子:子弹是沿一条直线走的,采样会漏掉薄墙之间的缝隙。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 起点 XZ(敌人)。
    /// - `Vec2` - 终点 XZ(玩家)。
    /// - `f32` - 视线高度(米)—— 用它筛掉「脚下 / 头顶」的楼板。
    ///
    /// # Returns
    ///
    /// - `bool` - 被隔墙挡住为 `true`。
    pub fn blocks_sight(&self, from: Vec2, to: Vec2, sight_y: f32) -> bool {
        let dx: f32 = to[0] - from[0];
        let dz: f32 = to[1] - from[1];
        for floor in self.get_floors() {
            let (min, max): (Vec3, Vec3) = match floor {
                Floor::Slab { min, max } => (*min, *max),
                Floor::Wall { min, max } => (*min, *max),
            };
            // 楼板只在视线正好平躺在板面高度上时才算遮挡,否则二楼
            // 的地板会把一楼的人整片挡住(脚下 0.15 m 的地板同理)。
            let is_slab: bool = matches!(floor, Floor::Slab { .. });
            if is_slab
                && (sight_y < min[1] - INTERIOR_SIGHT_EPSILON
                    || sight_y > max[1] + INTERIOR_SIGHT_EPSILON)
            {
                continue;
            }
            // 线段 vs AABB:逐轴求进出参数区间,非空即相交。
            let (lo, hi): (f32, f32) = (0.0, 1.0);
            let mut enter: f32 = lo;
            let mut exit: f32 = hi;
            let mut clipped: bool = true;
            for axis in 0..2 {
                let (origin, delta, low, high): (f32, f32, f32, f32) = if axis == 0 {
                    (from[0], dx, min[0], max[0])
                } else {
                    (from[1], dz, min[2], max[2])
                };
                if delta.abs() <= INTERIOR_SIGHT_EPSILON {
                    // 与这一轴平行:起点不在板内就永远不相交。
                    if origin < low || origin > high {
                        clipped = false;
                        break;
                    }
                    continue;
                }
                let mut t0: f32 = (low - origin) / delta;
                let mut t1: f32 = (high - origin) / delta;
                if t0 > t1 {
                    std::mem::swap(&mut t0, &mut t1);
                }
                enter = enter.max(t0);
                exit = exit.min(t1);
                if enter > exit {
                    clipped = false;
                    break;
                }
            }
            if clipped && enter <= exit {
                return true;
            }
        }
        false
    }

    /// 纯分离:把圆心推出所有与身体区间相交的隔墙(楼板不参与)。
    ///
    /// [`Self::resolve_interior_slide`] 的底层实现,迭代若干轮直到一轮
    /// 没有任何墙推动圆心为止。**不动 Y** —— 垂直方向是重力的职责,
    /// 这里越权会把玩家吸到墙面上。
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
    fn separate_interior(
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

    /// 一面横墙:z ∈ [10.0, 10.2],x ∈ [-5, 5],高 0..6.4。
    fn cross_wall() -> FloorWorld {
        let mut w: FloorWorld = FloorWorld::new();
        w.push_wall([-5.0, 0.0, 10.0], [5.0, 6.4, 10.2]);
        w
    }

    /// 顶着横墙斜走时,切向位移必须被保留 —— 「有速度、无位移」的回归测试。
    ///
    /// **必须从「已经陷进墙里」的位置出发**,这才是线上的真实顺序:
    /// 静态层(`CollisionWorld`)看不见室内隔墙,它照常把玩家推进去,
    /// 室内层下一帧才分离。起点正好贴着墙面时(`d == radius`)推动量
    /// 为零,根本走不到滑动分支,那样的测试对任何实现都通过。
    ///
    /// 旧实现(纯分离,没有 `delta`)会把 x 一起弹回 0.000 —— 那正是
    /// 线上每帧净位移为零、玩家被钉在墙上的形态。
    #[test]
    fn sliding_along_a_wall_keeps_the_tangential_step() {
        let world: FloorWorld = cross_wall();
        // 墙占 z ∈ [10.0, 10.2],贴面站位 10.0 - 0.35 = 9.65。
        // 起点取 9.85 —— 陷进墙里 0.20 m,模拟静态层刚把人推进去。
        let here: Vec2 = [0.0, 10.0 - RADIUS + 0.20];
        let delta: Vec2 = [0.05, 0.05];
        let got: Vec2 = world.resolve_interior_slide(here, delta, 0.0, BODY_TOP, RADIUS);
        // 切向必须真的走掉。
        assert!(
            got[0] - here[0] > 0.03,
            "切向位移被吃掉了:x {} -> {}",
            here[0],
            got[0]
        );
        // 法向被墙推回接触面,不得越过(= 陷进墙里)。
        assert!(
            got[1] <= 10.0 - RADIUS + 1e-3,
            "穿墙了:{} > {}",
            got[1],
            10.0 - RADIUS
        );
        // 旧行为的对照:同样起点下纯分离会吃掉全部切向。
        let pure: Vec2 = world.resolve_interior(here, 0.0, BODY_TOP, RADIUS);
        assert!(
            got[0] - pure[0] > 0.03,
            "带位移版本必须比纯分离多走出切向:{} vs {}",
            got[0],
            pure[0]
        );
    }

    /// 正面顶墙时不得穿墙(切向为 0 的退化情形)。
    #[test]
    fn head_on_into_a_wall_never_crosses_it() {
        let world: FloorWorld = cross_wall();
        let here: Vec2 = [0.0, 10.0 - RADIUS];
        let got: Vec2 = world.resolve_interior_slide(here, [0.0, 0.05], 0.0, BODY_TOP, RADIUS);
        assert!(
            got[1] <= 10.0 - RADIUS + 1e-3,
            "穿墙了:{} > {}",
            got[1],
            10.0 - RADIUS
        );
    }

    /// 视线被横墙挡住时 `blocks_sight` 为真,绕过去之后为假。
    #[test]
    fn a_partition_wall_blocks_sight_only_across_itself() {
        let world: FloorWorld = cross_wall();
        // 正对着墙:南 -> 北,必然被挡。
        assert!(world.blocks_sight([0.0, 5.0], [0.0, 15.0], 1.32));
        // 沿墙同侧:不穿过墙,不该被挡。
        assert!(!world.blocks_sight([-4.0, 5.0], [4.0, 5.0], 1.32));
    }

    /// 楼板不挡「站在它上面的人的横向视线」,只挡真正压在视线高度上的。
    ///
    /// 二楼地板(2.95 ~ 3.20)绝不能把一楼的人整片挡死,否则站在街上
    /// 的玩家会被自家天花板保护起来。
    #[test]
    fn an_upper_slab_does_not_block_eye_level_sight() {
        let mut world: FloorWorld = FloorWorld::new();
        world.push_slab([-5.0, UPPER_BOT, -5.0], [5.0, UPPER_TOP, 5.0]);
        // 视线高度 1.32 m,在二楼地板下面:通畅。
        assert!(!world.blocks_sight([-4.0, 0.0], [4.0, 0.0], 1.32));
        // 视线正好在板面高度:被挡。
        assert!(world.blocks_sight([-4.0, 0.0], [4.0, 0.0], UPPER_BOT));
    }

    fn slab_world() -> FloorWorld {
        let mut w: FloorWorld = FloorWorld::new();
        w.push_slab([-5.75, 0.0, -4.75], [5.75, GROUND_TOP, 4.75]);
        w.push_slab([-5.75, UPPER_BOT, -4.75], [4.45, UPPER_TOP, 4.75]);
        for i in 0..STAIR_STEPS {
            let y0: f32 = -4.55 + i as f32 * STAIR_RUN;
            let top: f32 = GROUND_TOP + (i + 1) as f32 * STAIR_RISE;
            w.push_slab([4.45, GROUND_TOP, y0], [5.75, top, y0 + STAIR_RUN]);
        }
        w
    }

    fn height_at(w: &FloorWorld, x: f32, z: f32, from_y: f32) -> Option<f32> {
        w.support_height([x, z], from_y)
    }

    fn step_along_stair(w: &FloorWorld) -> Vec<f32> {
        let mut out: Vec<f32> = Vec::new();
        let mut y: f32 = GROUND_TOP;
        for i in 0..STAIR_STEPS {
            let z: f32 = -4.55 + (i as f32 + 0.5) * STAIR_RUN;
            y = height_at(w, 5.1, z, y).unwrap_or(y);
            out.push(y);
        }
        out
    }

    #[test]
    fn stair_ramp_returns_increasing_heights() {
        let w: FloorWorld = slab_world();
        let heights: Vec<f32> = step_along_stair(&w);
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
        // 走局部 `let` 而不是把常量直接写进 `assert!`:容差 / 台阶高 / 楼层高
        // 三个都是编译期常量,直接比较会被 clippy 判成 `assertions_on_constants`
        // —— 而这条测试的价值恰恰是「常量之间的大小关系一旦被改坏就报警」。
        let tolerance: f32 = STEP_UP_TOLERANCE;
        let rise: f32 = STAIR_RISE;
        let storey: f32 = UPPER_TOP - GROUND_TOP;
        assert!(tolerance > rise, "{}", T_INTERIOR_STAIR_CLIMBS);
        assert!(tolerance < storey, "{}", T_INTERIOR_UPPER_FLOOR_ABOVE);
    }

    #[test]
    fn wall_between_two_rooms_blocks_movement() {
        let mut w: FloorWorld = FloorWorld::new();
        w.push_slab([-5.0, 0.0, -5.0], [5.0, GROUND_TOP, 5.0]);
        w.push_wall([-2.0, GROUND_TOP, -0.10], [2.0, 2.75, 0.10]);
        let pushed: Vec2 = w.resolve_interior([0.0, 0.0], GROUND_TOP, BODY_TOP, RADIUS);
        assert!(pushed[1].abs() > 0.1, "{}", T_INTERIOR_WALL_BLOCKS);
        let away: Vec2 = w.resolve_interior([0.0, 2.0], GROUND_TOP, BODY_TOP, RADIUS);
        assert!((away[1] - 2.0).abs() < 1e-4, "{}", T_INTERIOR_WALL_PASSES);
    }

    #[test]
    fn slab_above_head_does_not_block_player() {
        let w: FloorWorld = slab_world();
        let inside: Vec2 = w.resolve_interior([0.0, 0.0], GROUND_TOP, BODY_TOP, RADIUS);
        assert!(
            (inside[0] - 0.0).abs() < 1e-4 && (inside[1] - 0.0).abs() < 1e-4,
            "{}",
            T_INTERIOR_CEILING_INSIDE
        );
    }

    #[test]
    fn doorway_gap_is_the_only_way_through_the_front_wall() {
        let mut w: FloorWorld = FloorWorld::new();
        w.push_slab([-5.0, 0.0, -5.0], [5.0, GROUND_TOP, 5.0]);
        let y_in: f32 = -5.0 + WALL_T;
        w.push_wall([-5.0, 0.0, y_in - WALL_T], [-DOOR_HALF, 6.4, y_in]);
        w.push_wall([DOOR_HALF, 0.0, y_in - WALL_T], [5.0, 6.4, y_in]);
        w.push_wall(
            [-DOOR_HALF, DOOR_TOP, y_in - WALL_T],
            [DOOR_HALF, 6.4, y_in],
        );
        let through: Vec2 = w.resolve_interior([0.0, y_in], GROUND_TOP, BODY_TOP, RADIUS);
        assert!(
            (through[1] - y_in).abs() < 1e-3,
            "{}",
            T_INTERIOR_DOORWAY_THROUGH
        );
        let blocked: Vec2 =
            w.resolve_interior([DOOR_HALF + 0.1, y_in], GROUND_TOP, BODY_TOP, RADIUS);
        assert!(blocked[1] > y_in, "{}", T_INTERIOR_DOORWAY_BLOCKS);
        let lintel: Vec2 = w.resolve_interior([0.0, y_in], GROUND_TOP, 3.0, RADIUS);
        assert!(lintel[1] > y_in, "{}", T_INTERIOR_DOORWAY_BLOCKS);
    }

    #[test]
    fn support_height_reports_the_drop_when_walking_off_a_ledge() {
        let w: FloorWorld = slab_world();
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
        let w: FloorWorld = slab_world();
        let on_ground: Vec2 = [0.0, 0.0];
        assert!(w.contains_interior_point(on_ground, GROUND_TOP * 0.5));
        assert!(!w.contains_interior_point(on_ground, 1.5));
        let under_stair: Vec2 = [5.1, -4.0];
        assert!(w.contains_interior_point(under_stair, 0.5));
    }

    #[test]
    fn outside_the_footprint_there_is_no_support() {
        let w: FloorWorld = slab_world();
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
