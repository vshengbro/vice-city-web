//! 二维碰撞与分离:玩家圆 vs 建筑 AABB / 车辆 AABB / 道具圆柱 + 世界边界。
//!
//! 不引入任何物理引擎(rust-standards §13.1 优先不新增第三方依赖),碰撞体
//! 全部**从资产 JSON 的 `bounds` 自动推导** —— 摆放蓝图(BUILDINGS /
//! PROPS / PALM_POSITIONS / VEHICLES)只给位置 + yaw + scale,包围盒由
//! [`placement_box`] 按 yaw 旋转资产本地 XZ 足迹得到,因此资产换了模型
//! 碰撞体自动跟着变,不存在第二份手写的魔法坐标表。
//!
//! 分离算法是最朴素的「圆心 → 形状最近点」推出法:对每个形状求圆心到
//! 形状的最短分离向量,把圆心沿该向量推出 `penetration` 的距离。圆形站在
//! AABB 角上时单次迭代可能残留一点重叠,所以固定迭代若干轮直到稳定。

use crate::{
    r#const::PEDESTRIAN_PERSONAL_SPACE,
    r#type::{Vec2, Vec3},
};

/// 分离迭代轮数(圆心站在 AABB 角上时单轮残留一点,多轮收敛)。
const RESOLVE_ITERATIONS: usize = 4;

/// 判定「圆心在形状内部」的向量长度阈值(米)。
const INSIDE_EPSILON: f32 = 1e-5;

/// 判定「这次分离真的搬了人」的修正量阈值(米)。
///
/// 小于 1 mm 的修正只是浮点噪声,按「没有碰到墙」处理,免得每帧都
/// 多走一遍滑动。
const SLIDE_EPSILON: f32 = 1e-3;

/// 滑动分离的子步长(米)。
///
/// 一帧最多走 0.08 m(步行 4.6 m/s × 1/60 s),所以最多切成 2 步。
/// 取 0.05 m 保证单步位移远小于碰撞半径,不会跨过内表面落进
/// `push_out_aabb` 的「点在盒内」分支(那个分支对厚墙会选错面,把
/// 人送到墙的另一侧)。
const SLIDE_STEP: f32 = 0.05;

/// 一步里至少要走完这个比例才继续推进,否则判定为撞墙收手。
const SLIDE_MIN_PROGRESS: f32 = 0.25;

/// 车辆碰撞足迹:半长(沿车头方向)与半宽(垂直车头方向),单位米。
///
/// 数值由 `assets/car_sedan.json` 的 `bounds` 量得:`x` 跨度 4.566 m
/// (车长,沿资产本地 X = 车头方向)、`z` 跨度 1.851 m(车宽)。乘 0.5 得半长
/// 2.283 m 与半宽 0.926 m。`truck_pickup` 是 5.364 × 1.992,取轿车这一档
/// 作为整车族的代表:多出来的 0.07 m 半长对撞墙手感没有可察觉的影响,
/// 而按最长的车定尺会让轿车离墙更远,反而更容易在窄巷里剐蹭。
///
/// **为什么不能继续用 1.25 m 的圆:** 圆的最远点在 `1.25 m` 处,而车头在
/// `2.283 m` 处 —— 车身有整整 **1.033 m** 悬在碰撞边界之外。实测(把车
/// 满油门怼进最近的楼)车头最深扎进建筑 AABB **1.033 m**,也就是用户报的
/// 「车穿过建筑围墙」的可见深度。宽度方向圆反而比车宽(0.926 m)还大
/// 0.32 m,所以侧面不会漏 —— 漏的只有车头与车尾,也就是**迎着墙开进去
/// 之后看见车身插在墙里**的那一面。
///
/// 外部读者:`game::resolve_dynamic_bodies` 用它给每辆车建有向盒碰撞体,
/// 删掉或改成私有不符 §18。
pub const CAR_FOOTPRINT: Vec2 = [2.283, 0.926];

/// 车辆每步推进的子步长(米)。
///
/// 满油门(24 m/s)一帧走 `24 × FIXED_DT × 4 = 1.60 m`,而碰撞世界里
/// **最薄**的静态盒只有 0.133 m(交通锥,见 `prop_traffic_cone` ×
/// `CONE_COLLIDER_SCALE`)。1.60 / 0.133 = **12 倍** —— 单步推进足以跳过
/// 任何一堵薄墙,`push_out_aabb` 那一帧根本看不到重叠,于是既不挡也不推,
/// 车就过去了。取 0.05 m(与 [`SLIDE_STEP`] 同一档)后单步只有墙厚的
/// 38%,再也跨不过去。
pub(crate) const CAR_STEP: f32 = 0.05;

/// 相机当作球体扫描时的半径(米)。
///
/// 只用一条零半径射线会在墙角 / 屋檐这种掠射角上抖动 —— 命中距离随
/// yaw 的微小变化跳变。给探针一个实际尺寸(略大于一个肩宽)做球体推进,
/// 抖动幅度被压到可以忽略。它只被 [`ray_to_shapes`] 读到,所以定义在
/// 射线实现旁边而不是 `camera`。
pub const CAMERA_PROBE_RADIUS: f32 = 0.32;

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

/// 静态碰撞世界:一组形状 + 世界边界 + 玩家圆半径。
#[derive(Clone, Debug)]
pub struct CollisionWorld {
    /// 全部静态碰撞体。
    shapes: Vec<Shape>,
    /// 玩家圆半径(米)。
    player_radius: f32,
    /// 世界半边长(X / Z 各一半,米),玩家不许走出这个范围。
    half_extent: Vec2,
}

/// Inherent implementation of [`CollisionWorld`].
impl CollisionWorld {
    /// 新建一个空的碰撞世界。
    ///
    /// # Returns
    ///
    /// - `Self` - 不含任何形状的碰撞世界。
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            player_radius: 0.35,
            half_extent: [48.0, 48.0],
        }
    }

    /// 静态碰撞体表的只读视图。
    ///
    /// # Returns
    ///
    /// - `&[Shape]` - 碰撞体表。
    pub fn get_shapes(&self) -> &[Shape] {
        &self.shapes
    }

    /// 静态碰撞体表的可变引用。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<Shape>` - 碰撞体表。
    pub fn get_shapes_mut(&mut self) -> &mut Vec<Shape> {
        &mut self.shapes
    }

    /// 玩家圆半径。
    ///
    /// # Returns
    ///
    /// - `f32` - 半径(米)。
    pub fn get_player_radius(&self) -> f32 {
        self.player_radius
    }

    /// 覆盖玩家圆半径。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新的半径(米)。
    pub fn set_player_radius(&mut self, value: f32) {
        self.player_radius = value;
    }

    /// 世界半边长。
    ///
    /// # Returns
    ///
    /// - `Vec2` - X / Z 各一半的边界(米)。
    pub fn get_half_extent(&self) -> Vec2 {
        self.half_extent
    }

    /// 覆盖世界半边长。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 新的 X / Z 半边长(米)。
    pub fn set_half_extent(&mut self, value: Vec2) {
        self.half_extent = value;
    }

    /// 追加一个轴对齐包围盒。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 盒中心的世界 XZ 坐标。
    /// - `Vec2` - 盒的半尺寸(米)。
    pub fn push_aabb(&mut self, center: Vec2, half: Vec2) {
        self.get_shapes_mut().push(Shape::Aabb { center, half });
    }

    /// 追加一个竖直圆柱。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 圆柱中心的世界 XZ 坐标。
    /// - `f32` - 圆柱半径(米)。
    pub fn push_circle(&mut self, center: Vec2, radius: f32) {
        self.get_shapes_mut().push(Shape::Circle { center, radius });
    }

    /// 把一个圆形碰撞体推出所有重叠的静态形状,并钳进世界边界。
    ///
    /// 这是纯函数式的:输入一个世界 XZ 坐标,输出分离后的坐标。多次调用
    /// 幂等 —— 已经贴住的圆再调一次不会抖。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待分离的世界 XZ 坐标。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离并钳进边界后的世界 XZ 坐标。
    pub fn resolve(&self, point: Vec2) -> Vec2 {
        let radius: f32 = self.get_player_radius();
        let mut current: Vec2 = point;
        for _ in 0..RESOLVE_ITERATIONS {
            let mut moved: bool = false;
            for shape in self.get_shapes() {
                let hit: Option<(Vec2, f32)> = match shape {
                    Shape::Aabb { center, half } => push_out_aabb(*center, *half, current, radius),
                    Shape::Circle {
                        center,
                        radius: other,
                    } => push_out_circle(*center, *other, current, radius),
                };
                if let Some((direction, depth)) = hit {
                    current[0] += direction[0] * depth;
                    current[1] += direction[1] * depth;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        current
    }

    /// 圆 vs 静态形状的**滑动**分离:沿墙保留切向位移。
    ///
    /// [`Self::resolve`] 是「全有全无」的位置钳制 —— 它把圆心直接推到
    /// 形状的最近面,**本帧想要的切向位移也一起被改写**。玩家斜着
    /// 撞上墙面时位移被削到几乎为零,表现为「有速度、无位移」:
    /// `want (-1.00, -0.00)` 拿到 `got (+0.00, +0.00)`,几十帧推不动
    /// 一毫米。卡点不固定在某一面墙(实测 x = 28.3 / 28.4 / 33.1 / 35.0
    /// 都出现过),因为**每一面墙都这样** —— 弹到哪就贴住哪面墙。
    ///
    /// 这里是标准两步 `move_and_slide`:
    ///
    /// 1. `wanted = here + delta` 先做一次分离,拿到法线 `n` 与穿透
    ///    深度;
    /// 2. 把 `delta` 沿 `n` 的**法向分量扣掉**,只保留切向,再走一遍
    ///    分离。于是顶着墙斜走时玩家顺着墙滑过去,而不是被钉在接触点。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 上一帧结束时的世界 XZ 坐标。
    /// - `Vec2` - 本帧想要的位移(米)。
    /// - `f32` - 圆半径(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 本帧实际走到的世界 XZ 坐标(已钳进世界边界)。
    pub fn resolve_slide(&self, here: Vec2, delta: Vec2, radius: f32) -> Vec2 {
        // 逐段推进:把这一帧想走的位移切成若干小步,每步都从**上一步的
        // 合法位置**出发做分离。
        //
        // 不能只做一次「先 resolve(wanted) 再扣法向分量」:圆心一旦在一步
        // 里跨过内表面,`push_out_aabb` 的「点在盒内」分支只能猜一个面
        // 推 —— 对厚墙它会选**人正在走向的那一面**,于是人被直接送到墙
        // 的另一侧(实测 0.15 m 的一步穿过了整个墙:从 -0.15 跳到 +0.35)。
        // 切成小步之后,每一步的穿透都远小于步长,永远走不到「盒内」
        // 分支,墙也就不再是隐形的传送带。
        //
        // 步长取 0.05 m(不到半径的 1/6):一帧最多 0.08 m(4.6 m/s ×
        // 1/60 s),也就是 1~2 步,开销可以忽略。
        let want_len: f32 = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
        if want_len <= SLIDE_EPSILON {
            return here;
        }
        let steps: usize = (want_len / SLIDE_STEP).ceil().max(1.0) as usize;
        let step: Vec2 = [delta[0] / steps as f32, delta[1] / steps as f32];
        let mut current: Vec2 = here;
        for _ in 0..steps {
            // 已经陷进某个形状内部(不是「贴着」):停下,别再往里走。
            // 用严格小于 + 一层皮,于是「正好贴在墙面上」不算穿透,
            // 切向滑动才不会被自己的接触测试挡住。
            if self.penetrates(current, radius) {
                break;
            }
            let next: Vec2 = [current[0] + step[0], current[1] + step[1]];
            let resolved: Vec2 = self.resolve(next);
            let achieved: Vec2 = [resolved[0] - current[0], resolved[1] - current[1]];
            let want_dot: f32 = step[0] * step[0] + step[1] * step[1];
            let got_dot: f32 = achieved[0] * step[0] + achieved[1] * step[1];
            if got_dot >= want_dot * SLIDE_MIN_PROGRESS {
                // 这一步基本走完了,继续下一小步。
                current = resolved;
                continue;
            }
            // 被挡住了:只保留**切向**分量再走一次,玩家于是顺着墙滑
            // 过去,而不是被钉在接触点上(这正是「有速度、无位移」的
            // 修复点)。
            current = self.slide_step(current, step, resolved, radius);
            break;
        }
        current
    }

    /// 被挡住时的切向滑动:从 `before` 出发,沿 `want` 的切向分量走。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 这一小步开始时的位置。
    /// - `Vec2` - 这一小步想要的位移。
    /// - `Vec2` - 分离之后的位置(用来推法线)。
    /// - `f32` - 圆半径(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 沿墙滑过之后的位置。
    fn slide_step(&self, before: Vec2, want: Vec2, pushed: Vec2, _radius: f32) -> Vec2 {
        let correction: Vec2 = [
            pushed[0] - (before[0] + want[0]),
            pushed[1] - (before[1] + want[1]),
        ];
        let correction_len: f32 =
            (correction[0] * correction[0] + correction[1] * correction[1]).sqrt();
        if correction_len <= SLIDE_EPSILON {
            return pushed;
        }
        // 分离方向指向形状**外侧**,所以法向就是它本身。
        let normal: Vec2 = [
            correction[0] / correction_len,
            correction[1] / correction_len,
        ];
        let into: f32 = want[0] * normal[0] + want[1] * normal[1];
        if into >= 0.0 {
            // 没有往墙里钻(擦边),分离结果就是对的。
            return pushed;
        }
        let tangent: Vec2 = [want[0] - normal[0] * into, want[1] - normal[1] * into];
        if (tangent[0] * tangent[0] + tangent[1] * tangent[1]).sqrt() <= SLIDE_EPSILON {
            // 纯正面顶墙:没有切向可留。
            return pushed;
        }
        self.resolve([before[0] + tangent[0], before[1] + tangent[1]])
    }

    /// 圆心是否**陷进**了某个形状内部(贴着墙面不算)。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 圆心的世界 XZ 坐标。
    /// - `f32` - 圆半径(米)。
    ///
    /// # Returns
    ///
    /// - `bool` - 已经陷进某个形状时为 `true`。
    fn penetrates(&self, point: Vec2, radius: f32) -> bool {
        let reach: f32 = radius - SLIDE_EPSILON;
        self.get_shapes().iter().any(|shape: &Shape| match shape {
            // 圆 vs 盒 = 「圆心到盒的**距离** < 半径」。
            //
            // 两个坑都踩过:一开始写成「圆心落在盒的矩形足迹内」,对 20 m
            // 深的墙恒为真(墙在 z 方向覆盖 ±10 m),贴着墙面也会被判成陷
            // 进去,切向滑动被自己的接触测试冻住,于是又回到「有速度、
            // 无位移」。改用真距离后:贴着时距离恰为半径,减去一层皮后
            // 严格大于阈值,正常放行。
            Shape::Aabb { center: _, half: _ } => distance_to_shape(shape, point) < reach,
            Shape::Circle {
                center,
                radius: other,
            } => {
                let dx: f32 = point[0] - center[0];
                let dz: f32 = point[1] - center[1];
                (dx * dx + dz * dz).sqrt() < *other + reach
            }
        })
    }

    /// 把速度按**软边界**削一刀:越界越多,允许的速度越小。
    ///
    /// 旧实现是 `point.clamp(-half, half)` —— 一个**硬钳制**。在城市是
    /// 程序化无限生成的前提下,硬钳制就是用户报的「地图非无限大,触碰空气
    /// 墙无法前进」:走到 150 m 处坐标被死死钉住,几十帧推不动一毫米。
    ///
    /// 软边界的语义是「**你最多只能领先已生成世界这么远**」,不是「世界到
    /// 这儿为止」:
    ///
    /// - `radius + slack` 以内 —— 完全不干预(绝大多数时候都在这里);
    /// - 超出部分按 `excess / slack` 线性收缩速度上限,越界越深跑越慢;
    /// - 超出 `2 × slack` 时速度上限为 0 —— 玩家**到不了**那里,自然
    ///   被限制在「已生成世界 + 一圈缓冲」里,不会掉进虚空。
    ///
    /// 它不做位置钳制,所以永远不可能把玩家「传送」回某个点:推力只改变
    /// 速度,玩家的位置始终是他自己走出来的。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待削的速度(世界 XZ,米/秒)。
    /// - `Vec2` - 当前所在位置(世界 XZ,米)。
    /// - `Vec2` - 世界软边界半径(X / Z 各一半,米)。
    /// - `f32` - 允许越界的余量(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 收缩后的速度。
    pub fn soft_limit(&self, velocity: Vec2, at: Vec2, limits: Vec2, slack: f32) -> Vec2 {
        // 收缩从**软边界本身**(`limits`)开始,不是从 `limits + slack`:
        // 一旦越过 `limits`,速度就在一个余量的距离内从满速线性降到 0。
        // 两个轴各自收缩 —— 沿对角线越界时两个方向都要减速,只削一个轴
        // 会让玩家斜着「蹭」出边界。
        let over_x: f32 = ((at[0].abs() - limits[0]) / slack).clamp(0.0, 1.0);
        let over_z: f32 = ((at[1].abs() - limits[1]) / slack).clamp(0.0, 1.0);
        let scale_x: f32 = 1.0 - over_x;
        let scale_z: f32 = 1.0 - over_z;
        [velocity[0] * scale_x, velocity[1] * scale_z]
    }

    /// 软边界产生的回推速度(米/秒),方向指向世界中心。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 当前所在位置(世界 XZ,米)。
    /// - `Vec2` - 世界软边界半径(X / Z 各一半,米)。
    /// - `f32` - 允许越界的余量(米)。
    /// - `f32` - 回推增益(1/秒)。
    /// - `f32` - 回推速度上限(米/秒)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 回推速度;完全在界内时为零向量。
    pub fn soft_push(&self, at: Vec2, limits: Vec2, slack: f32, gain: f32, max_push: f32) -> Vec2 {
        let over_x: f32 = at[0].abs() - (limits[0] + slack);
        let over_z: f32 = at[1].abs() - (limits[1] + slack);
        if over_x <= 0.0 && over_z <= 0.0 {
            return [0.0, 0.0];
        }
        let mut push: Vec2 = [0.0, 0.0];
        if over_x > 0.0 {
            push[0] = -at[0].signum() * (over_x * gain).min(max_push);
        }
        if over_z > 0.0 {
            push[1] = -at[1].signum() * (over_z * gain).min(max_push);
        }
        push
    }

    /// 用车辆半径做分离(玩家开车时的碰撞体更大)。
    ///
    /// 与 [`Self::resolve`] 同样的迭代分离,只是把「玩家圆半径」换成
    /// 车辆半径。车辆因此也穿不过建筑 / 道具,只是被推挤得更早。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待分离的世界 XZ 坐标。
    /// - `f32` - 车辆的等效碰撞圆半径(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离并钳进边界后的世界 XZ 坐标。
    pub fn resolve_with_radius(&self, point: Vec2, radius: f32) -> Vec2 {
        let mut current: Vec2 = point;
        for _ in 0..RESOLVE_ITERATIONS {
            let mut moved: bool = false;
            for shape in self.get_shapes() {
                let hit: Option<(Vec2, f32)> = match shape {
                    Shape::Aabb { center, half } => push_out_aabb(*center, *half, current, radius),
                    Shape::Circle {
                        center,
                        radius: other,
                    } => push_out_circle(*center, *other, current, radius),
                };
                if let Some((direction, depth)) = hit {
                    current[0] += direction[0] * depth;
                    current[1] += direction[1] * depth;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        current
    }

    /// 用车辆的**真实有向足迹**做分离 —— 车与静态形状碰撞的唯一入口。
    ///
    /// 与 [`Self::resolve_with_radius`] 的区别有两条,两条都对应一个实测到的
    /// 缺陷:
    ///
    /// 1. **足迹是有向盒,不是圆。** 车的真实车身是 4.566 × 1.851 m
    ///    ([`CAR_FOOTPRINT`]),而 1.25 m 的圆最远只到 1.25 m —— 车头有
    ///    **1.033 m** 悬在碰撞边界外。满油门怼楼时实测车头扎进建筑 AABB
    ///    1.033 m,这就是「车穿过建筑围墙」的可见深度。
    /// 2. **位移按 [`CAR_STEP`] 切成子步,每步各做一次分离。** 满油门一帧
    ///    推进 1.60 m,而碰撞世界里最薄的静态盒只有 0.133 m,单步足以跨过
    ///    任何一堵薄墙 —— 那一帧分离看不到重叠,车就过去了。
    ///
    /// 子步还顺带解决了「圆被从薄盒的错误面弹出去」:每一步的穿透量都远
    /// 小于盒厚,落点永远走不到 `push_out_aabb` 的「点在盒内」分支,也就
    /// 永远不会被猜错面送到墙的另一侧。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 车心当前的世界 XZ 坐标(本帧起点)。
    /// - `Vec2` - 本帧想要的总位移(米)。
    /// - `f32` - 车身朝向(弧度,绕 Y 轴,与 `TrafficCar::get_yaw` 同一约定)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离后的世界 XZ 坐标。
    pub fn resolve_car_footprint(&self, here: Vec2, delta: Vec2, yaw: f32) -> Vec2 {
        let want: f32 = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
        if want <= SLIDE_EPSILON {
            return here;
        }
        let steps: usize = (want / CAR_STEP).ceil().max(1.0) as usize;
        let mut current: Vec2 = here;
        // 每一小步落在**整帧位移的一个精确分数**上,而不是「把 delta 平分
        // steps 份再累加」。后者在最后一步不会正好落回 `here + delta`:
        // 120 步的浮点累加误差实测约 1e-6 m,比 `f32::EPSILON`(1.19e-7)
        // 大一个数量级,于是调用点那一句「车有没有被挡」在**空旷地面上
        // 也会判成撞墙**,车每帧被砍掉 75% 的速度。插值写法让最后一步
        // 乘的是 `1.0`,结果与 `here + delta` 逐位相同。
        for step_index in 1..=steps {
            let fraction: f32 = step_index as f32 / steps as f32;
            let wanted: Vec2 = [here[0] + delta[0] * fraction, here[1] + delta[1] * fraction];
            let before: Vec2 = current;
            let step: Vec2 = [wanted[0] - before[0], wanted[1] - before[1]];
            current = self.resolve_car_box(wanted, yaw);
            // 这一小步基本被挡干净了就收手:车贴着墙不该还在原地反复
            // 推离(那会让它「立正」贴墙,失去蹭着墙走的能力)。
            let achieved: Vec2 = [current[0] - before[0], current[1] - before[1]];
            let want_dot: f32 = step[0] * step[0] + step[1] * step[1];
            let got_dot: f32 = achieved[0] * step[0] + achieved[1] * step[1];
            if got_dot < want_dot * SLIDE_MIN_PROGRESS {
                current = car_slide(current, step, wanted);
                break;
            }
        }
        current
    }

    /// 有向车辆足迹 vs 全部静态形状的分离(一次性,不切子步)。
    ///
    /// `resolve_car_footprint` 的单步内核,单独暴露是为了能直接单测「一步
    /// 里车能不能穿过去」。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 车心待分离的世界 XZ 坐标。
    /// - `f32` - 车身朝向(弧度,绕 Y 轴)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离后的世界 XZ 坐标。
    pub fn resolve_car_box(&self, point: Vec2, yaw: f32) -> Vec2 {
        let (sin_yaw, cos_yaw): (f32, f32) = yaw.sin_cos();
        // 车身基向量,和 `TrafficCar::drive` 的第 3 步完全同一套:
        // `fwd = [cos yaw, -sin yaw]`、`right = [fwd[1], -fwd[0]]`。
        // 符号必须一模一样 —— 这里的 `right` 若取成 `[−fwd[1], fwd[0]]`,
        // 车身盒就成了左右翻转的镜像,斜着靠墙时会被判到墙的另一侧去。
        let forward: Vec2 = [cos_yaw, -sin_yaw];
        let side: Vec2 = [forward[1], -forward[0]];
        let mut current: Vec2 = point;
        for _ in 0..RESOLVE_ITERATIONS {
            let mut moved: bool = false;
            for shape in self.get_shapes() {
                let hit: Option<(Vec2, f32)> = match shape {
                    Shape::Aabb { center, half } => {
                        push_out_box_aabb(*center, *half, current, forward, side, CAR_FOOTPRINT)
                    }
                    Shape::Circle {
                        center,
                        radius: other,
                    } => push_out_circle(*center, *other, current, CAR_FOOTPRINT[1]),
                };
                if let Some((direction, depth)) = hit {
                    current[0] += direction[0] * depth;
                    current[1] += direction[1] * depth;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        current
    }

    /// 点是否落在某个形状内部(调试 / 自测用,不做分离)。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 世界 XZ 坐标。
    ///
    /// # Returns
    ///
    /// - `bool` - 圆心(零半径)与任一形状重叠时为 `true`。
    pub fn contains_point(&self, point: Vec2) -> bool {
        self.get_shapes().iter().any(|shape: &Shape| match shape {
            Shape::Aabb { center, half } => {
                (point[0] - center[0]).abs() <= half[0] && (point[1] - center[1]).abs() <= half[1]
            }
            Shape::Circle { center, radius } => {
                let dx: f32 = point[0] - center[0];
                let dy: f32 = point[1] - center[1];
                (dx * dx + dy * dy).sqrt() <= *radius
            }
        })
    }

    /// 两两分离一组动态实体,返回各自被推开后的新位置。
    ///
    /// **为什么要「按质量分」而不是「各推一半」:** 各推一半意味着撞上一辆
    /// 停着的车时,人会像撞上另一堵墙一样被弹开,而 1.5 t 的车却纹丝不动
    /// —— 这既不真实,手感也差(人被车「顶」住却推不动车)。按质量反比
    /// 分配,轻的那个几乎弹开、重的那个纹丝不动,才符合直觉。
    ///
    /// 动量守恒在这里是「穿透深度的分配」:总推开量 = 穿透深度,各自承担
    /// `m_other / (m_a + m_b)` 与 `m_a / (m_a + m_b)`。两车质量相同时各
    /// 承担一半(对撞后各退一半);人(1)撞车(30)时人承担 30/31、车承担
    /// 1/31 —— 人被弹飞、车几乎不动。
    ///
    /// 迭代若干轮直到稳定:三个人挤成一团时单轮只能解开一部分。
    ///
    /// # Arguments
    ///
    /// - `&[DynamicBody]` - 本帧所有动态实体。
    ///
    /// # Returns
    ///
    /// - `Vec<Vec2>` - 与输入同序的新位置(已互相分离)。
    pub fn resolve_dynamic(&self, bodies: &[DynamicBody]) -> Vec<Vec2> {
        let mut out: Vec<Vec2> = bodies
            .iter()
            .map(|body: &DynamicBody| body.position)
            .collect();
        for _ in 0..RESOLVE_ITERATIONS {
            let mut moved: bool = false;
            for i in 0..bodies.len() {
                for j in (i + 1)..bodies.len() {
                    let (Some(a), Some(b)) = (bodies.get(i), bodies.get(j)) else {
                        continue;
                    };
                    let reach: f32 = a.get_radius() + b.get_radius();
                    let delta: Vec2 = [out[i][0] - out[j][0], out[i][1] - out[j][1]];
                    let distance: f32 = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
                    if distance >= reach {
                        continue;
                    }
                    // 完全重合时给一个确定的分离方向,避免除零后
                    // 所有人往同一个方向抖。
                    // 行人之间额外留一点「个人空间」:两个人并排走时肩膀
                    // 不会贴着 —— 真实街景里人本来就是有间距的。
                    let reach: f32 = reach + same_kind_personal_space(a.get_kind(), b.get_kind());
                    let (normal, depth): (Vec2, f32) = if distance > INSIDE_EPSILON {
                        ([delta[0] / distance, delta[1] / distance], reach - distance)
                    } else {
                        ([1.0, 0.0], reach)
                    };
                    let mass_a: f32 = a.get_mass().max(0.01);
                    let mass_b: f32 = b.get_mass().max(0.01);
                    let total: f32 = mass_a + mass_b;
                    // 质量大的少让位 —— 推 `j` 的比例 = a 的质量占比。
                    // 质量加权:`i` 让位 `m_j/(m_i+m_j)`,`j` 让位
                    // `m_i/(m_i+m_j)`。轻的弹开、重的几乎不动。
                    let push_a: f32 = depth * (mass_b / total);
                    let push_b: f32 = depth * (mass_a / total);
                    // `normal` 指向 `i - j`(由 `out[i] - out[j]` 得到),
                    // 所以让 `i` 沿 `+normal` 退、`j` 沿 `-normal` 退。
                    out[i][0] += normal[0] * push_a;
                    out[i][1] += normal[1] * push_a;
                    out[j][0] -= normal[0] * push_b;
                    out[j][1] -= normal[1] * push_b;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        out
    }

    /// 把一组动态实体既与静态形状分离、又互相分离,返回最终位置。
    ///
    /// 这是动态层的**唯一入口**:车穿房子、树穿人、两个人穿在一起都在这里
    /// 一次解决。顺序是「先静态后动态」—— 先把每个体从墙里推出来,再让
    /// 它们互相让位;反过来的话,动态让位可能又把谁推进墙里。
    ///
    /// # Arguments
    ///
    /// - `&[DynamicBody]` - 本帧所有动态实体。
    ///
    /// # Returns
    ///
    /// - `Vec<Vec2>` - 与输入同序的最终世界 XZ 位置。
    pub fn resolve_all(&self, bodies: &[DynamicBody]) -> Vec<Vec2> {
        let mut placed: Vec<DynamicBody> = bodies.to_vec();
        // 静态分离:每个体用自己的**足迹**走一遍已有的迭代分离。
        //
        // 车的足迹是有向盒(`DynamicBody::new_box`),走的必须是
        // `push_out_box_aabb` —— 若这里退回圆的 `push_out_aabb`,它会把车
        // 按 1.25 m 推离墙面,而 `TrafficCar::drive` 刚把它停在 2.283 m,
        // 于是这一层又把车缩回 1.25 m,车头重新插进墙里 1.033 m。
        for body in placed.iter_mut() {
            body.position = self.separate_static(body.position, body);
        }
        // 动态分离:质量加权的两两分离(玩家按玩家半径,车按车半径……)。
        let separated: Vec<Vec2> = self.resolve_dynamic(&placed);
        for (body, at) in placed.iter_mut().zip(separated.iter()) {
            body.position = *at;
        }
        // 动态让位可能又把谁推进了墙,最后再对静态收敛一次。
        let mut out: Vec<Vec2> = Vec::with_capacity(placed.len());
        for body in &placed {
            out.push(self.separate_static(body.position, body));
        }
        out
    }

    /// 把一个动态体按它**自己的足迹**推出所有静态形状(迭代至稳定)。
    ///
    /// 车的足迹是有向盒,走 [`push_out_box_aabb`];其余实体是圆,走
    /// [`push_out_circle`]。这里必须按足迹分流而不是一律按圆 —— 一律按圆
    /// 会把 [`TrafficCar::drive`](crate::traffic::TrafficCar::drive) 刚停在
    /// 半车长处的车又缩回 1.25 m,车头重新插进墙里。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待分离的世界 XZ 坐标。
    /// - `&DynamicBody` - 提供足迹与朝向;**不读它的 `position`**。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离并迭代收敛后的世界 XZ 坐标。
    fn separate_static(&self, point: Vec2, body: &DynamicBody) -> Vec2 {
        let (sin_yaw, cos_yaw): (f32, f32) = body.get_yaw().sin_cos();
        let forward: Vec2 = [cos_yaw, -sin_yaw];
        let side: Vec2 = [forward[1], -forward[0]];
        let footprint: Vec2 = body.get_footprint();
        let mut current: Vec2 = point;
        for _ in 0..RESOLVE_ITERATIONS {
            let mut moved: bool = false;
            for shape in self.get_shapes() {
                let hit: Option<(Vec2, f32)> = match shape {
                    Shape::Aabb { center, half } => {
                        push_out_box_aabb(*center, *half, current, forward, side, footprint)
                    }
                    Shape::Circle {
                        center,
                        radius: other,
                    } => push_out_circle(*center, *other, current, footprint[1]),
                };
                if let Some((direction, depth)) = hit {
                    current[0] += direction[0] * depth;
                    current[1] += direction[1] * depth;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        current
    }

    /// 一点到所有静态形状表面的最短距离(米)。
    ///
    /// 落在某个形状**内部**时该形状贡献 0,整体取最小值 —— 所以返回值
    /// 为 0 就等价于「这个点在某个碰撞体里」。相机遮挡回避用它判断眼点
    /// 是否已经扎进楼里,验收调试通道用它把眼点的实际位置报出来。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 世界 XZ 坐标。
    ///
    /// # Returns
    ///
    /// - `f32` - 到最近形状表面的距离(米);空碰撞世界时为 `f32::MAX`。
    pub fn nearest_surface_distance(&self, point: Vec2) -> f32 {
        let mut best: f32 = f32::MAX;
        for shape in self.get_shapes() {
            let candidate: f32 = distance_to_shape(shape, point);
            if candidate < best {
                best = candidate;
            }
        }
        best
    }
}

/// 一点到单个静态形状表面的最短距离(米);点落在形状内部时为 0。
///
/// 公开它是为了让验收探针能回答「**我为什么走不动**」:玩家被挡住时
/// 圆心在碰撞体**外面**(贴着面),`contains_point` 返回 `false`,但本函数
/// 会给出 0.0x m 这样的贴面距离。没有这个量,「站在墙前」和「陷进墙里」
/// 在探针输出里长得一模一样。
///
/// # Arguments
///
/// - `&Shape` - 静态碰撞体.
/// - `Vec2` - 世界 XZ 坐标。
///
/// # Returns
///
/// - `f32` - 最短距离(米)。
pub fn shape_distance(shape: &Shape, point: Vec2) -> f32 {
    match shape {
        Shape::Aabb { center, half } => {
            let dx: f32 = ((point[0] - center[0]).abs() - half[0]).max(0.0);
            let dz: f32 = ((point[1] - center[1]).abs() - half[1]).max(0.0);
            (dx * dx + dz * dz).sqrt()
        }
        Shape::Circle { center, radius } => {
            let dx: f32 = point[0] - center[0];
            let dz: f32 = point[1] - center[1];
            ((dx * dx + dz * dz).sqrt() - *radius).max(0.0)
        }
    }
}

/// 一个点到所有静态形状表面的最短距离(米)。
///
/// # Arguments
///
/// - `&Shape` - 静态碰撞体。
/// - `Vec2` - 世界 XZ 坐标。
///
/// # Returns
///
/// - `f32` - 最短距离(米)。
fn distance_to_shape(shape: &Shape, point: Vec2) -> f32 {
    shape_distance(shape, point)
}

/// 同类**人形**实体之间额外的「个人空间」(米)。
///
/// 人并排走时不会贴着,也不应该被推进另一个人的身体里。车辆之间不给
/// 额外空间 —— 前后车就该紧贴着排队。
///
/// # Arguments
///
/// - `BodyKind` - 第一个实体的类型。
/// - `BodyKind` - 第二个实体的类型。
///
/// # Returns
///
/// - `f32` - 需要额外拉开的距离(米)。
pub fn same_kind_personal_space(a: BodyKind, b: BodyKind) -> f32 {
    if a == b && matches!(a, BodyKind::Pedestrian | BodyKind::Enemy) {
        PEDESTRIAN_PERSONAL_SPACE
    } else {
        0.0
    }
}

/// 从 `origin` 沿单位方向 `dir`(XZ 分量)投射,求撞上第一个碰撞体的距离。
///
/// 用**球体推进**而不是零半径射线:把探针半径当成「相机本体的尺寸」,
/// 于是命中距离等于「中心还能走多远才让球面贴上墙」。掠射角(视线几乎
/// 平行墙面扫过)下零半径射线会给出剧烈跳变的命中距离,球体则平滑得多。
///
/// 建筑 / 长椅这类方块用**膨胀 AABB** 求交(把半径加到半尺寸上),
/// 圆形道具用**圆-圆求交**。两者都是闭式解,没有迭代。
///
/// # Arguments
///
/// - `&CollisionWorld` - 静态碰撞世界的只读引用。
/// - `Vec3` - 射线起点(世界坐标;只用 XZ 分量)。
/// - `Vec3` - 单位方向(世界坐标;只用 XZ 分量)。
///
/// # Returns
///
/// - `Option<(f32, Vec2)>` - `(命中距离, 命中点世界 XZ)`;未命中为 `None`。
pub fn ray_to_shapes(world: &CollisionWorld, origin: Vec3, dir: Vec3) -> Option<(f32, Vec2)> {
    let flat: f32 = (dir[0] * dir[0] + dir[2] * dir[2]).sqrt();
    // 视线完全竖直(pitch 接近 ±90°)时 XZ 投影退化,没有遮挡可言。
    if flat < 1.0e-5 {
        return None;
    }
    let (ux, uz): (f32, f32) = (dir[0] / flat, dir[2] / flat);
    let mut best: Option<(f32, Vec2)> = None;
    for shape in world.get_shapes() {
        let candidate: Option<(f32, Vec2)> = match shape {
            Shape::Aabb { center, half } => ray_into_box(
                [origin[0], origin[2]],
                [ux, uz],
                *center,
                [half[0] + CAMERA_PROBE_RADIUS, half[1] + CAMERA_PROBE_RADIUS],
            ),
            Shape::Circle { center, radius } => ray_into_circle(
                [origin[0], origin[2]],
                [ux, uz],
                *center,
                *radius + CAMERA_PROBE_RADIUS,
            ),
        };
        let Some((distance, at)) = candidate else {
            continue;
        };
        if distance < 0.0 {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(current, _): &(f32, [f32; 2])| distance < *current)
        {
            best = Some((distance, at));
        }
    }
    best.map(|(distance, _): (f32, [f32; 2])| {
        (
            distance * flat,
            [
                origin[0] + ux * distance * flat,
                origin[2] + uz * distance * flat,
            ],
        )
    })
}

/// 单位方向 `dir` 的射线撞上轴对齐盒(半尺寸已含探针半径)时的参数 t。
///
/// 标准 slab 法:对 X / Z 两轴各求一次进出区间并取交集,交集的近端就是
/// 命中参数。起点在盒内时近端为 0(调用方按「已经贴住」处理)。
///
/// # Arguments
///
/// - `Vec2` - 射线起点 XZ。
/// - `Vec2` - XZ 平面上的单位方向。
/// - `Vec2` - 盒中心 XZ。
/// - `Vec2` - 盒半尺寸 XZ(已含探针半径)。
///
/// # Returns
///
/// - `Option<(f32, Vec2)>` - `(参数 t, 命中点 XZ)`;未命中为 `None`。
fn ray_into_box(origin: Vec2, dir: Vec2, center: Vec2, half: Vec2) -> Option<(f32, Vec2)> {
    let mut near: f32 = f32::NEG_INFINITY;
    let mut far: f32 = f32::INFINITY;
    for axis in 0..2 {
        // 方向分量接近 0 时该轴不构成约束:只有起点已经在板内才算命中。
        if dir[axis].abs() < 1.0e-6 {
            if (origin[axis] - center[axis]).abs() > half[axis] {
                return None;
            }
            continue;
        }
        let lo: f32 = (center[axis] - half[axis] - origin[axis]) / dir[axis];
        let hi: f32 = (center[axis] + half[axis] - origin[axis]) / dir[axis];
        let (enter, exit): (f32, f32) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        near = near.max(enter);
        far = far.min(exit);
        if near > far {
            return None;
        }
    }
    // 盒在射线**背后**时 `far` 为负:射线朝反方向走,永远不会撞上。
    // 少了这一句,任何背对建筑的相机都会拿到一个 `t = 0` 的假命中,
    // 回避逻辑随即把距离扣成负数(`res=-0.45`),相机被推到玩家背后。
    if far < 0.0 {
        return None;
    }
    let t: f32 = near.max(0.0);
    Some((t, [origin[0] + dir[0] * t, origin[1] + dir[1] * t]))
}

/// 单位方向 `dir` 的射线撞上圆柱(半径已含探针半径)时的参数 t。
///
/// 圆-圆求交:解 `|o + t·d - c|² = r²` 的二次方程取最小非负根。判别式
/// 小于 0 表示射线从旁边擦过,不相交。
///
/// # Arguments
///
/// - `Vec2` - 射线起点 XZ。
/// - `Vec2` - XZ 平面上的单位方向。
/// - `Vec2` - 圆柱中心 XZ。
/// - `f32` - 圆柱半径(已含探针半径)。
///
/// # Returns
///
/// - `Option<(f32, Vec2)>` - `(参数 t, 命中点 XZ)`;未命中为 `None`。
fn ray_into_circle(origin: Vec2, dir: Vec2, center: Vec2, radius: f32) -> Option<(f32, Vec2)> {
    let (ox, oz): (f32, f32) = (origin[0] - center[0], origin[1] - center[1]);
    let b: f32 = 2.0 * (ox * dir[0] + oz * dir[1]);
    let c: f32 = ox * ox + oz * oz - radius * radius;
    let discriminant: f32 = b * b - 4.0 * c;
    if discriminant < 0.0 {
        return None;
    }
    let root: f32 = discriminant.sqrt();
    let near: f32 = (-b - root) * 0.5;
    let far: f32 = (-b + root) * 0.5;
    // 起点在圆柱内:远端才是「穿出去」的那一侧,近端为负没有意义。
    let t: f32 = if near >= 0.0 { near } else { far };
    if t < 0.0 {
        return None;
    }
    Some((t, [origin[0] + dir[0] * t, origin[1] + dir[1] * t]))
}

/// Default construction for [`CollisionWorld`].
impl Default for CollisionWorld {
    /// 返回空碰撞世界,与 `CollisionWorld::new` 等价。
    fn default() -> Self {
        CollisionWorld::new()
    }
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
#[derive(Clone, Copy, Debug)]
pub struct DynamicBody {
    /// 实体类型(决定同类之间是否也要分开)。
    pub kind: BodyKind,
    /// 足迹中心的世界 XZ 坐标(圆则是圆心)。
    pub position: Vec2,
    /// 足迹半长 / 半宽(米);圆填 `[radius, radius]`。
    pub footprint: Vec2,
    /// 足迹朝向(弧度,绕 Y 轴,与 `TrafficCar::get_yaw` 同一约定);
    /// 圆形忽略这一项。
    pub yaw: f32,
    /// 质量(以「一个人」为单位,1.0 = 一个人)。
    pub mass: f32,
}

/// Inherent implementation of [`DynamicBody`].
impl DynamicBody {
    /// 构造一个**圆形**动态碰撞体。
    ///
    /// # Arguments
    ///
    /// - `BodyKind` - 实体类型。
    /// - `Vec2` - 圆心世界 XZ 坐标。
    /// - `f32` - 碰撞圆半径(米)。
    /// - `f32` - 质量(以「一个人」为单位)。
    ///
    /// # Returns
    ///
    /// - `Self` - 就绪的动态碰撞体。
    pub const fn new(kind: BodyKind, position: Vec2, radius: f32, mass: f32) -> Self {
        Self {
            kind,
            position,
            footprint: [radius, radius],
            yaw: 0.0,
            mass,
        }
    }

    /// 构造一个**有向盒**动态碰撞体(车用)。
    ///
    /// # Arguments
    ///
    /// - `BodyKind` - 实体类型。
    /// - `Vec2` - 足迹中心的世界 XZ 坐标。
    /// - `Vec2` - 足迹半长 / 半宽(米)。
    /// - `f32` - 足迹朝向(弧度,绕 Y 轴)。
    /// - `f32` - 质量(以「一个人」为单位)。
    ///
    /// # Returns
    ///
    /// - `Self` - 就绪的动态碰撞体。
    pub const fn new_box(
        kind: BodyKind,
        position: Vec2,
        footprint: Vec2,
        yaw: f32,
        mass: f32,
    ) -> Self {
        Self {
            kind,
            position,
            footprint,
            yaw,
            mass,
        }
    }

    /// 该实体的质量。
    ///
    /// # Returns
    ///
    /// - `f32` - 质量(以「一个人」为单位)。
    pub fn get_mass(&self) -> f32 {
        self.mass
    }

    /// 该实体的类型。
    ///
    /// # Returns
    ///
    /// - `BodyKind` - 实体类型。
    pub fn get_kind(&self) -> BodyKind {
        self.kind
    }

    /// 该实体的碰撞半径(米)。
    ///
    /// 取足迹两轴的**最大值**:圆形足迹下两者相等,就是原来的半径;车
    /// 走这条通道时拿到半车长 2.283 m,用于车与车的圆近似互相分离。
    ///
    /// # Returns
    ///
    /// - `f32` - 半径(米)。
    pub fn get_radius(&self) -> f32 {
        self.footprint[0].max(self.footprint[1])
    }

    /// 该实体的足迹半长 / 半宽(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 足迹半尺寸。
    pub fn get_footprint(&self) -> Vec2 {
        self.footprint
    }

    /// 该实体足迹的朝向(弧度,绕 Y 轴)。
    ///
    /// # Returns
    ///
    /// - `f32` - 朝向(弧度)。
    pub fn get_yaw(&self) -> f32 {
        self.yaw
    }
}

/// 车身(有向盒)中心在一条任意方向轴上的投影半径。
///
/// # Arguments
///
/// - `Vec2` - 投影轴(无需归一化,结果按轴长线性缩放)。
/// - `Vec2` - 车头方向单位向量。
/// - `Vec2` - 车身侧向单位向量。
/// - `Vec2` - 车身半长 / 半宽(米)。
///
/// # Returns
///
/// - `f32` - 车心到该轴的投影半径(米)。
///
/// **必须取两段的绝对值之和。** 带符号直接相加会在 yaw 接近 45° 时互相
/// 抵消成 0,车就被当成一个点,什么墙都撞不上。
fn car_projection(axis: Vec2, forward: Vec2, side: Vec2, footprint: Vec2) -> f32 {
    let along: f32 = footprint[0] * (forward[0] * axis[0] + forward[1] * axis[1]).abs();
    let across: f32 = footprint[1] * (side[0] * axis[0] + side[1] * axis[1]).abs();
    along + across
}

/// 有向盒(车)vs 轴对齐盒(建筑 / 道具)的分离量。
///
/// **为什么要一个专门的函数,而不是把圆放大:** 圆对所有方向一视同仁,而车
/// 是长条形的 —— 同一辆车车头要离墙 `half_long`,车身侧面只要离墙
/// `half_wide`。用圆去近似,两个方向只能取同一个值,取小了车头扎进墙
/// (实测 1.033 m),取大了侧面凭空多出半米空隙、车在窄巷里剐蹭。两者在
/// 二维里等价于:车在盒的**局部坐标**里做一次「圆心到矩形」的距离测试。
///
/// 算法走标准 SAT(分离轴定理):两个凸多边形分离 ⟺ 存在一条轴使两个投影
/// 不相交。盒只有 3 条候选轴 —— 盒自身的两条 + 车的两条车身轴。取**穿透
/// 最浅**的那条轴把车推出去,所以车会被沿最省力的方向滑开,而不是被卡在
/// 墙角。这与玩家用的 `resolve_slide` 是同一套思路。
///
/// # Arguments
///
/// - `Vec2` - 静态盒中心的世界 XZ 坐标。
/// - `Vec2` - 静态盒的半尺寸(米)。
/// - `Vec2` - 车心的世界 XZ 坐标。
/// - `Vec2` - 车身前向单位向量(世界 XZ)。
/// - `Vec2` - 车身左向单位向量(世界 XZ)。
/// - `Vec2` - 车的半长 / 半宽(米)。
///
/// # Returns
///
/// - `Option<(Vec2, f32)>` - `(推出方向, 推出距离)`;不重叠时为 `None`。
fn push_out_box_aabb(
    center: Vec2,
    half: Vec2,
    point: Vec2,
    forward: Vec2,
    side: Vec2,
    footprint: Vec2,
) -> Option<(Vec2, f32)> {
    let offset: Vec2 = [point[0] - center[0], point[1] - center[1]];
    // 四条候选轴:车身两条 + 世界 X / Z 两条。`gap` > 0 表示该轴上两者
    // 已经分离 —— 只要有一条如此,按 SAT 这两个凸多边形就不相交。
    let axes: [Vec2; 4] = [forward, side, [1.0, 0.0], [0.0, 1.0]];
    // 车心在一条**任意方向**轴上的投影半径 = 车身矩形在轴上的半长投影。
    // 两端点各投一次,取两段**绝对值之和** —— 带符号相加会互相抵消,
    // yaw 接近 45° 时算出 0,车就被当成一个点。
    let mut separated: bool = false;
    let mut best: Option<(Vec2, f32)> = None;
    for (index, axis) in axes.into_iter().enumerate() {
        // 世界轴(后两条)的静态半径就是盒自己的半长;车身轴要投一次。
        let box_radius: f32 = match index {
            0 | 1 => half[0] * axis[0].abs() + half[1] * axis[1].abs(),
            2 => half[0],
            _ => half[1],
        };
        let gap: f32 = (offset[0] * axis[0] + offset[1] * axis[1]).abs()
            - box_radius
            - car_projection(axis, forward, side, footprint);
        if gap > 0.0 {
            separated = true;
            break;
        }
        // 选**最浅**的那条轴:最浅 = 沿它分离最省力,车于是被推去最
        // 容易脱身的方向,而不是被卡在墙角。
        let depth: f32 = -gap;
        if best
            .as_ref()
            .is_none_or(|(_axis, kept): &(Vec2, f32)| depth < *kept)
        {
            best = Some((axis, depth));
        }
    }
    if separated {
        return None;
    }
    let (mut axis, depth) = best?;
    // 推出方向必须**指向盒的外侧**。偏移量为 0(车心与盒心重合)时没有
    // 符号可言,取轴的第一非零分量的正负定一个确定的方向。
    let projection: f32 = offset[0] * axis[0] + offset[1] * axis[1];
    let sign: f32 = if projection > 0.0 {
        1.0
    } else if projection < 0.0 {
        -1.0
    } else if axis[0] != 0.0 {
        axis[0].signum()
    } else {
        axis[1].signum()
    };
    axis = [axis[0] * sign, axis[1] * sign];
    Some((axis, depth))
}

/// 车被墙挡住时的切向滑动:保留 `step` 的切向分量。
///
/// # Arguments
///
/// - `Vec2` - 分离之后的位置。
/// - `Vec2` - 这一小步想要的位移。
/// - `Vec2` - 分离之前想去的落点(用来算修正量)。
///
/// # Returns
///
/// - `Vec2` - 沿墙滑过之后的位置。
fn car_slide(pushed: Vec2, step: Vec2, wanted: Vec2) -> Vec2 {
    let correction: Vec2 = [pushed[0] - wanted[0], pushed[1] - wanted[1]];
    let length: f32 = (correction[0] * correction[0] + correction[1] * correction[1]).sqrt();
    if length <= SLIDE_EPSILON {
        return pushed;
    }
    let normal: Vec2 = [correction[0] / length, correction[1] / length];
    let into: f32 = step[0] * normal[0] + step[1] * normal[1];
    if into >= 0.0 {
        return pushed;
    }
    let tangent: Vec2 = [step[0] - normal[0] * into, step[1] - normal[1] * into];
    if (tangent[0] * tangent[0] + tangent[1] * tangent[1]).sqrt() <= SLIDE_EPSILON {
        return pushed;
    }
    [pushed[0] + tangent[0], pushed[1] + tangent[1]]
}

/// 圆形 vs AABB 的分离量。
///
/// # Arguments
///
/// - `Vec2` - AABB 中心的世界 XZ 坐标。
/// - `Vec2` - AABB 半尺寸(米)。
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
    // 圆心已经落在盒内:沿「离边界最近」的那个面推出去。
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

/// 圆形 vs 圆的分离量。
///
/// # Arguments
///
/// - `Vec2` - 圆心 A 的世界 XZ 坐标。
/// - `f32` - 圆 A 的半径(米)。
/// - `Vec2` - 圆心 B(玩家圆心)的世界 XZ 坐标。
/// - `f32` - 圆 B 的半径(米)。
///
/// # Returns
///
/// - `Option<(Vec2, f32)>` - `(推出方向, 推出距离)`;不重叠时为 `None`。
fn push_out_circle(center: Vec2, other: f32, point: Vec2, radius: f32) -> Option<(Vec2, f32)> {
    let delta: Vec2 = [point[0] - center[0], point[1] - center[1]];
    let distance: f32 = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
    let reach: f32 = other + radius;
    if distance > reach {
        return None;
    }
    if distance > INSIDE_EPSILON {
        return Some(([delta[0] / distance, delta[1] / distance], reach - distance));
    }
    Some(([1.0, 0.0], reach))
}

/// 由资产 `bounds` 推导一个摆放实例在世界 XZ 平面上的 AABB。
///
/// 资产的 `bounds` 是**本地** Y-up 包围盒:建筑 / 车辆这类资产的长度在
/// 本地 X 上、宽度在本地 Z 上。摆放时绕 Y 轴旋转 `yaw` 并统一缩放
/// `scale`,所以：
///
/// 1. 本地足迹中心 = `((min.x + max.x) / 2, (min.z + max.z) / 2)` —— 资产
///    不保证足迹相对原点居中(例如 `bldg_deco_pink` 的 z 从 -5.08 到
///    6.49),不补这一步墙就会整体偏移半个身位。
/// 2. 本地足迹半尺寸 = `((max.x - min.x) / 2, (max.z - min.z) / 2)`。
/// 3. 用与 `Instance::new` 相同的旋转约定(本地 +X → 世界
///    `(cos yaw, 0, -sin yaw)`,本地 +Z → `(sin yaw, 0, cos yaw)`)把
///    中心偏移旋到世界,半尺寸取旋转后外接盒(yaw 为 90° 整数倍时是精确值)。
///
/// # Arguments
///
/// - `Vec3` - 资产 `bounds.min`(本地坐标)。
/// - `Vec3` - 资产 `bounds.max`(本地坐标)。
/// - `f32` - 绕 Y 轴的摆放朝向(弧度)。
/// - `f32` - 统一缩放系数。
/// - `Vec3` - 摆放位置(世界坐标)。
///
/// # Returns
///
/// - `(Vec2, Vec2)` - `(世界 XZ 盒中心, 世界 XZ 盒半尺寸)`。
pub fn placement_box(min: Vec3, max: Vec3, yaw: f32, scale: f32, position: Vec3) -> (Vec2, Vec2) {
    let (sin_yaw, cos_yaw): (f32, f32) = yaw.sin_cos();
    let local_center: Vec2 = [(min[0] + max[0]) * 0.5, (min[2] + max[2]) * 0.5];
    let local_half: Vec2 = [(max[0] - min[0]) * 0.5, (max[2] - min[2]) * 0.5];
    let center: Vec2 = [
        position[0] + (cos_yaw * local_center[0] + sin_yaw * local_center[1]) * scale,
        position[2] + (-sin_yaw * local_center[0] + cos_yaw * local_center[1]) * scale,
    ];
    let half: Vec2 = [
        (cos_yaw.abs() * local_half[0] + sin_yaw.abs() * local_half[1]) * scale,
        (sin_yaw.abs() * local_half[0] + cos_yaw.abs() * local_half[1]) * scale,
    ];
    (center, half)
}

#[cfg(test)]
mod tests {
    use crate::collision::{BodyKind, CAR_FOOTPRINT, CollisionWorld, DynamicBody};
    use crate::r#const::{
        T_COLLISION_SLIDE_KEEPS_TANGENT, T_COLLISION_SLIDE_OPEN_GROUND_X,
        T_COLLISION_SLIDE_OPEN_GROUND_Z, T_COLLISION_SLIDE_STOPS_AT_WALL, T_DYNAMIC_MASS_WEIGHTED,
        T_DYNAMIC_NO_MOVE_WHEN_CLEAR, T_DYNAMIC_SAME_KIND_SEPARATES, T_DYNAMIC_STATIC_TOO,
        T_NO_POSITION_CLAMP, T_SOFT_LIMIT_RAMP, T_SOFT_LIMIT_SPARE_INSIDE,
        T_SOFT_LIMIT_STOPS_AT_VOID, T_SOFT_PUSH_ZERO_INSIDE,
    };
    use crate::r#type::Vec2;

    /// 回归测试:车的碰撞足迹必须是真实车身尺寸,而不是 1.25 m 的圆。
    ///
    /// 圆只到 1.25 m 而车头在 2.283 m,车头就有 1.033 m 悬在碰撞边界之外。
    const T_CAR_FOOTPRINT_NOT_A_DISC: &str = "车的碰撞足迹必须按真实车身算,而不是 1.25 m 的圆";

    /// 回归测试:车开到建筑 / 围墙里必须被挡住,车头不许插进去。
    const T_CAR_NEVER_INSIDE_A_WALL: &str = "车身不得插进建筑围墙";

    /// 回归测试:一帧的位移不得跨过最薄的墙(高速穿墙)。
    const T_CAR_NO_WALL_TUNNEL: &str = "满油门一帧不得穿过薄墙";

    /// 回归测试:斜着撞墙必须沿墙滑过去,而不是被弹到墙的另一侧。
    const T_CAR_SLIDES_ALONG_WALL: &str = "斜着撞墙必须沿墙滑过去";

    /// 测试里代表「车头正对 −X 墙面」的朝向(游戏约定 forward = [cos, −sin])。
    ///
    /// 写成 π 的字面值而不是引用 `std::f32::consts::PI`:§6.1 要求子模块里
    /// 不出现限定 std 路径,这里是 `#[cfg(test)]` 内的模块私有常量。
    /// 取 `f32::consts::PI` 的 f32 最近值(clippy 的 excessive_precision)。
    const YAW_FACING_NEG_X: f32 = 3.141_592_5;

    /// 测试里代表 45° 斜向位移的分解系数(1.6 m / √2 ≈ 1.1313708)。
    const STRIDE_DIAGONAL: f32 = 1.131_370_8;

    /// 泡测试的朝向步长:15°(π / 12 ≈ 0.2617994)。
    const YAW_STEP_RAD: f32 = 0.261_799_4;

    /// 墙占 x ∈ [-10, 0](盒心 -5、半尺寸 5),内表面在 x = 0,墙体在 -X 侧。
    ///
    /// 玩家从 **+X** 一侧靠近,圆心恰好贴在面上时 x = 半径 = `REST_X`。
    const REST_X: f32 = 0.35;
    const RADIUS: f32 = 0.35;

    fn wall_world() -> CollisionWorld {
        let mut world: CollisionWorld = CollisionWorld::new();
        world.set_player_radius(RADIUS);
        world.push_aabb([-5.0, 0.0], [5.0, 10.0]);
        world
    }

    /// 贴着墙斜着走:法向被墙挡住,**切向必须保住**。
    ///
    /// 这是「有速度、无位移」(`want (-1, 0) got (0, 0)`)的回归测试:
    /// 旧的 `resolve` 是全有全无钳制,0.30 m 的切向会被一起清零。
    #[test]
    fn sliding_keeps_the_tangential_component() {
        let world: CollisionWorld = wall_world();
        let here: Vec2 = [REST_X, 0.0];
        // 往 -X 顶进墙里,同时往 +Z 走。
        let delta: Vec2 = [-0.20, 0.30];
        let at: Vec2 = world.resolve_slide(here, delta, world.get_player_radius());
        assert!(
            (at[1] - 0.30).abs() < 1e-2,
            "{} 切向 z 只走了 {} m,应保留 0.30 m",
            T_COLLISION_SLIDE_KEEPS_TANGENT,
            at[1]
        );
        assert!(
            at[0] >= REST_X - 1e-2,
            "{} x={} 穿过了墙(应 >= {REST_X})",
            T_COLLISION_SLIDE_STOPS_AT_WALL,
            at[0]
        );
    }

    /// 正面顶墙仍然必须停住,不能穿墙 —— 滑动不能削弱阻挡。
    #[test]
    fn sliding_still_stops_head_on_at_a_wall() {
        let world: CollisionWorld = wall_world();
        let here: Vec2 = [REST_X, 0.0];
        let delta: Vec2 = [-0.50, 0.0];
        let at: Vec2 = world.resolve_slide(here, delta, world.get_player_radius());
        assert!(
            at[0] >= REST_X - 1e-2,
            "{} 正面顶墙后 x={},穿过了墙(应 >= {REST_X})",
            T_COLLISION_SLIDE_STOPS_AT_WALL,
            at[0]
        );
        assert!(
            (at[0] - REST_X).abs() < 1e-2,
            "{} 正面顶墙后 x={},应贴住 {REST_X}",
            T_COLLISION_SLIDE_STOPS_AT_WALL,
            at[0]
        );
    }

    /// 空地上滑动必须等于原位移(不得引入任何偏移)。
    #[test]
    fn sliding_on_open_ground_is_the_full_delta() {
        let mut world: CollisionWorld = CollisionWorld::new();
        world.set_player_radius(RADIUS);
        let here: Vec2 = [10.0, 20.0];
        let delta: Vec2 = [0.15, -0.25];
        let at: Vec2 = world.resolve_slide(here, delta, world.get_player_radius());
        assert!(
            (at[0] - (here[0] + delta[0])).abs() < 1e-5,
            "{}",
            T_COLLISION_SLIDE_OPEN_GROUND_X
        );
        assert!(
            (at[1] - (here[1] + delta[1])).abs() < 1e-5,
            "{}",
            T_COLLISION_SLIDE_OPEN_GROUND_Z
        );
    }

    /// 顶着墙连续走很多帧:位置必须稳定,绝不能被送到墙的另一侧。
    #[test]
    fn repeated_walking_into_a_wall_never_tunnels_through_it() {
        let world: CollisionWorld = wall_world();
        let mut at: Vec2 = [REST_X, 0.0];
        // 一帧 0.077 m(4.6 m/s × 1/60 s),共 300 帧 ≈ 5 s。
        for _ in 0..300 {
            at = world.resolve_slide(at, [-0.077, 0.0], world.get_player_radius());
            assert!(
                at[0] >= REST_X - 1e-2,
                "{} 顶着墙走了几帧后 x={},穿墙了(应 >= {REST_X})",
                T_COLLISION_SLIDE_STOPS_AT_WALL,
                at[0]
            );
        }
    }

    /// 软边界在界内时必须**完全**不干预 —— 否则玩家在正常街区里就
    /// 会被减速。
    #[test]
    fn soft_limit_does_not_touch_anyone_inside_the_boundary() {
        let world: CollisionWorld = CollisionWorld::new();
        let velocity: Vec2 = [4.6, -3.2];
        let limits: Vec2 = world.get_half_extent();
        for at in [[0.0, 0.0], [10.0, -20.0], [limits[0], limits[1]]] as [Vec2; 3] {
            let kept: Vec2 = world.soft_limit(velocity, at, limits, 24.0);
            assert!(
                (kept[0] - velocity[0]).abs() < 1.0e-6 && (kept[1] - velocity[1]).abs() < 1.0e-6,
                "{T_SOFT_LIMIT_SPARE_INSIDE}"
            );
        }
    }

    /// 越界时速度必须按比例收缩 —— 这是「软」的核心:不是墙,是减速带。
    #[test]
    fn soft_limit_shrinks_speed_more_the_further_out_you_go() {
        let world: CollisionWorld = CollisionWorld::new();
        let limits: Vec2 = world.get_half_extent();
        let slack: f32 = 24.0;
        // 探针只带 **Z** 分量:越界是沿 Z 发生的,X 轴不该被削。
        let probe: Vec2 = [0.0, 1.0];
        let inside: Vec2 = world.soft_limit(probe, [0.0, 0.0], limits, slack);
        let edge: f32 = limits[1];
        // 刚好越过软边界一点点 / 越过一半余量 / 越过一个完整余量。
        let middle: Vec2 = world.soft_limit(probe, [0.0, edge + 1.0], limits, slack);
        let deep: Vec2 = world.soft_limit(probe, [0.0, edge + slack * 0.5], limits, slack);
        let far: Vec2 = world.soft_limit(probe, [0.0, edge + slack], limits, slack);
        assert!((inside[1] - 1.0).abs() < 1.0e-6, "{T_SOFT_LIMIT_RAMP}");
        assert!((middle[0] - probe[0]).abs() < 1.0e-6, "{T_SOFT_LIMIT_RAMP}");
        assert!(middle[1] < inside[1], "{T_SOFT_LIMIT_RAMP}");
        assert!(deep[1] < middle[1], "{T_SOFT_LIMIT_RAMP}");
        assert!(far[1] <= 1.0e-6, "{T_SOFT_LIMIT_STOPS_AT_VOID}");
    }

    /// 越界一个余量以上时速度**归零** —— 玩家到不了那里,于是不会掉进
    /// 「还没生成出来的虚空」。这是「无限世界」能成立的前提。
    #[test]
    fn soft_limit_stops_the_player_before_the_unstreamed_void() {
        let world: CollisionWorld = CollisionWorld::new();
        let limits: Vec2 = world.get_half_extent();
        let push: Vec2 = world.soft_push([0.0, 400.0], limits, 24.0, 1.6, 9.0);
        assert!(push[1] < 0.0, "{T_SOFT_LIMIT_STOPS_AT_VOID}");
        assert!(push[1].abs() <= 9.0, "{T_SOFT_LIMIT_STOPS_AT_VOID}");
    }

    /// 界内不得有任何回推 —— 靠这个保证玩家不会被「温柔地拽走」。
    #[test]
    fn soft_push_is_zero_inside_the_boundary() {
        let world: CollisionWorld = CollisionWorld::new();
        let limits: Vec2 = world.get_half_extent();
        // 「界内」= 半径 + 余量之内,而不是 `limits` 之内 —— 后者还要再加
        // 一个余量的缓冲。
        let slack: f32 = 24.0;
        for at in [
            [0.0, 0.0],
            [limits[0], limits[1]],
            [limits[0] + slack - 1.0, limits[1] + slack - 1.0],
        ] as [Vec2; 3]
        {
            let push: Vec2 = world.soft_push(at, limits, slack, 1.6, 9.0);
            assert!(
                push[0].abs() < 1.0e-6 && push[1].abs() < 1.0e-6,
                "{T_SOFT_PUSH_ZERO_INSIDE}"
            );
        }
    }

    /// **这是「无限世界」的核心回归测试**:分离不得再做位置钳制。
    ///
    /// 旧实现最后一步是 `clamp(-150, 150)`,于是玩家走到边界处位移恒为
    /// 零 —— 有速度、无位移,几十帧推不动一毫米(用户报的空气墙)。
    #[test]
    fn separation_no_longer_clamps_the_player_inside_the_world() {
        let mut world: CollisionWorld = CollisionWorld::new();
        world.set_player_radius(RADIUS);
        // 空碰撞世界,让分离完全不介入。
        let far: Vec2 = [900.0, -900.0];
        let resolved: Vec2 = world.resolve(far);
        assert!(
            (resolved[0] - far[0]).abs() < 1.0e-6 && (resolved[1] - far[1]).abs() < 1.0e-6,
            "{T_NO_POSITION_CLAMP}"
        );
        let slid: Vec2 = world.resolve_slide(far, [3.0, 3.0], RADIUS);
        assert!(slid[0] > far[0] + 2.0, "{T_NO_POSITION_CLAMP}");
    }

    /// 人撞静止的车:人必须被弹开,车几乎不动 —— 这就是质量分离。
    #[test]
    fn a_person_bounces_off_a_car_but_the_car_barely_moves() {
        let world: CollisionWorld = CollisionWorld::new();
        // 人与车重叠 0.4 m。
        let overlap: f32 = 0.4;
        let person_radius: f32 = 0.35;
        let car_radius: f32 = 1.25;
        let gap: f32 = person_radius + car_radius - overlap;
        let bodies: [DynamicBody; 2] = [
            DynamicBody::new(BodyKind::Player, [0.0, 0.0], person_radius, 1.0),
            DynamicBody::new(BodyKind::Car, [gap, 0.0], car_radius, 30.0),
        ];
        let out: Vec<Vec2> = world.resolve_dynamic(&bodies);
        let person_moved: f32 = (out[0][0] - 0.0).abs();
        let car_moved: f32 = (out[1][0] - gap).abs();
        assert!(
            person_moved > 0.3,
            "{}: 人应被弹开,实得 {person_moved} m",
            T_DYNAMIC_MASS_WEIGHTED
        );
        assert!(
            car_moved < person_moved * 0.1,
            "{}: 车几乎不该动(人 {person_moved} / 车 {car_moved})",
            T_DYNAMIC_MASS_WEIGHTED
        );
    }

    /// 两车对撞:质量相同,各退一半。
    #[test]
    fn two_equal_cars_split_the_separation_evenly() {
        let world: CollisionWorld = CollisionWorld::new();
        let radius: f32 = 1.25;
        let gap: f32 = radius * 2.0 - 0.4;
        let bodies: [DynamicBody; 2] = [
            DynamicBody::new(BodyKind::Car, [0.0, 0.0], radius, 10.0),
            DynamicBody::new(BodyKind::Car, [gap, 0.0], radius, 10.0),
        ];
        let out: Vec<Vec2> = world.resolve_dynamic(&bodies);
        // 0 号在左、1 号在右,`normal` 指向 `+X`,所以 0 号退向 −X
        // (位移为负)、1 号退向 +X(位移为正)。用带符号的位移量。
        let left: f32 = -out[0][0];
        let right: f32 = out[1][0] - gap;
        assert!(
            (left - right).abs() < 1.0e-4,
            "{}: 等质量应各退一半,实得 {left} / {right}",
            T_DYNAMIC_MASS_WEIGHTED
        );
        assert!(
            (left + right - 0.4).abs() < 1.0e-3,
            "{}: 分离总量应等于穿透深度 0.4,实得 {}",
            T_DYNAMIC_MASS_WEIGHTED,
            left + right
        );
    }

    /// 完全不重叠时不得产生任何位移。
    #[test]
    fn separated_bodies_are_left_alone() {
        let world: CollisionWorld = CollisionWorld::new();
        let bodies: [DynamicBody; 2] = [
            DynamicBody::new(BodyKind::Player, [0.0, 0.0], 0.35, 1.0),
            DynamicBody::new(BodyKind::Player, [10.0, 0.0], 0.35, 1.0),
        ];
        let out: Vec<Vec2> = world.resolve_dynamic(&bodies);
        assert!(
            (out[0][0] - 0.0).abs() < 1.0e-6 && (out[1][0] - 10.0).abs() < 1.0e-6,
            "{}: 不重叠就不该动,得到 {out:?}",
            T_DYNAMIC_NO_MOVE_WHEN_CLEAR
        );
    }

    /// 同类实体之间**也要**分开 —— 两个人不能互相穿过。
    #[test]
    fn pedestrians_cannot_walk_through_each_other() {
        let world: CollisionWorld = CollisionWorld::new();
        let bodies: [DynamicBody; 2] = [
            DynamicBody::new(BodyKind::Pedestrian, [0.0, 0.0], 0.35, 1.0),
            DynamicBody::new(BodyKind::Pedestrian, [0.3, 0.0], 0.35, 1.0),
        ];
        let out: Vec<Vec2> = world.resolve_dynamic(&bodies);
        let gap: f32 = (out[0][0] - out[1][0]).abs();
        assert!(
            gap >= 0.7 - 1.0e-3,
            "{}: 两个行人应被分开到直径,实得 {gap} m",
            T_DYNAMIC_SAME_KIND_SEPARATES
        );
    }

    /// 动态层必须也能把人从**静态墙**里推出来(车穿房子)。
    #[test]
    fn dynamic_bodies_are_pushed_out_of_static_shapes_too() {
        let mut world: CollisionWorld = CollisionWorld::new();
        // 墙占 x ∈ [-10, 0],玩家圆心在 x = -0.2(陷进墙里 0.15 m)。
        world.push_aabb([-5.0, 0.0], [5.0, 10.0]);
        let bodies: [DynamicBody; 1] = [DynamicBody::new(BodyKind::Car, [-0.2, 0.0], 1.25, 10.0)];
        let out: Vec<Vec2> = world.resolve_all(&bodies);
        assert!(
            out[0][0] >= 1.25 - 1.0e-3,
            "{}: 车应被推出墙外,实得 x={}",
            T_DYNAMIC_STATIC_TOO,
            out[0][0]
        );
    }

    // ---- 车辆足迹回归(用户报:车穿过建筑围墙)--------------------
    //
    // 墙占 x ∈ [-10, 0]:盒心 (-5, 0)、半长 5,近面在 x = 0。
    /// 一面正对车头的建筑外墙。
    fn building_world() -> CollisionWorld {
        let mut world: CollisionWorld = CollisionWorld::new();
        world.push_aabb([-5.0, 0.0], [5.0, 10.0]);
        world
    }

    /// 车头怼墙:停在墙外的距离必须是**半车长**,不是 1.25 m。
    ///
    /// 这是用户报的那个缺陷本身。旧的 `resolve_car` 用 1.25 m 的圆,
    /// 于是车心停在 `x = 1.25`,而车头在 `1.25 + 2.283 = 3.533` ——
    /// **整整 1.033 m 的车身插在墙里**。
    #[test]
    fn a_car_stops_a_whole_car_length_short_of_a_wall() {
        let world: CollisionWorld = building_world();
        // 车头朝 −X(yaw = π),从 +X 侧一路开到墙前。
        let yaw: f32 = YAW_FACING_NEG_X;
        let resolved: Vec2 = world.resolve_car_footprint([20.0, 0.0], [-30.0, 0.0], yaw);
        // 车心离墙面至少要有半车长,车才整个儿在外面。
        assert!(
            resolved[0] >= CAR_FOOTPRINT[0] - 1.0e-2,
            "{}: 车心应停在离墙 {:.3} m 处(半车长),实得 x={}",
            T_CAR_FOOTPRINT_NOT_A_DISC,
            CAR_FOOTPRINT[0],
            resolved[0]
        );
        // 车身最远点(车头)也不许进墙。
        let nose: f32 = resolved[0] - CAR_FOOTPRINT[0];
        assert!(
            nose >= -1.0e-2,
            "{}: 车头扎进墙里 {:.3} m,落点 x={}",
            T_CAR_NEVER_INSIDE_A_WALL,
            -nose,
            resolved[0]
        );
    }

    /// 满油门怼楼:无论开多少帧,车身任何一角都不得进墙。
    ///
    /// 用**车身的四个角**判定而不是车心 —— 车心在墙外而车头插进去,
    /// 正是旧的圆足迹掩盖掉的那种情况(实测车头最深 1.033 m)。
    ///
    /// 容差取 `SKIN = 1e-4 m`(0.1 mm):车贴着墙面时 SAT 推离的浮点残差
    /// 实测在 3e-7 m 量级,那是 `f32` 精度而不是穿透。1.033 m 的缺陷与
    /// 3e-7 m 的噪声相差六个数量级,这条断言不可能被噪声蒙混过去。
    #[test]
    fn a_car_driven_at_full_speed_never_clips_into_a_building() {
        const SKIN: f32 = 1.0e-4;
        let world: CollisionWorld = building_world();
        for yaw_deg in [0.0f32, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0] {
            let yaw: f32 = yaw_deg.to_radians();
            let (sin_yaw, cos_yaw): (f32, f32) = yaw.sin_cos();
            let forward: Vec2 = [cos_yaw, -sin_yaw];
            let side: Vec2 = [forward[1], -forward[0]];
            let mut at: Vec2 = [20.0, 0.0];
            // 满油门一帧 1.60 m,共 200 帧,一路撞到底。
            for frame in 0..200 {
                at = world.resolve_car_footprint(at, [forward[0] * 1.6, forward[1] * 1.6], yaw);
                for sx in [-1.0f32, 1.0] {
                    for sz in [-1.0f32, 1.0] {
                        let corner: Vec2 = [
                            at[0]
                                + forward[0] * CAR_FOOTPRINT[0] * sx
                                + side[0] * CAR_FOOTPRINT[1] * sz,
                            at[1]
                                + forward[1] * CAR_FOOTPRINT[0] * sx
                                + side[1] * CAR_FOOTPRINT[1] * sz,
                        ];
                        let inside: bool = corner[0] < -SKIN && corner[1].abs() < 10.0;
                        assert!(
                            !inside,
                            "{}: yaw {yaw_deg}° 第 {frame} 帧车身角 {corner:?} 已在墙内(墙占 x<=0)",
                            T_CAR_NEVER_INSIDE_A_WALL
                        );
                    }
                }
            }
        }
    }

    /// 满油门一帧不得跨过**一堵薄墙**(被实测出来的真实场景)。
    ///
    /// 碰撞世界里最薄的静态盒是 0.133 m(交通锥),一帧 1.60 m = 12 倍。
    /// 旧实现一次 `resolve_car(proposed)` 只看落点:车从墙这侧一步跨到
    /// 墙那侧时落点已经离墙比 1.25 m 还远,`push_out_aabb` 返回 `None`,
    /// 于是既不挡也不推 —— 车就过去了。
    ///
    /// 起点必须让**车头**恰好停在墙的这一侧(还差一丁点没到),这样这一帧
    /// 才会真的跨过整堵墙;若按车心算起点,车头一开始就伸到墙那边去了。
    #[test]
    fn a_full_speed_frame_cannot_tunnel_through_a_thin_wall() {
        const THIN: f32 = 0.133;
        const FRAME: f32 = 1.6;
        let mut world: CollisionWorld = CollisionWorld::new();
        // 0.133 m 厚的薄墙,两面在 x = ∓0.0665;车从 +X 一侧冲 −X。
        world.push_aabb([0.0, 0.0], [THIN * 0.5, 10.0]);
        // 车头正好贴在 +X 面上:车心 = 面 + 半车长。
        let near_face: f32 = THIN * 0.5;
        let start: f32 = near_face + CAR_FOOTPRINT[0];
        // 不切子步的话这一帧落点在 start - 1.6,早已越到墙的另一侧。
        let resolved: Vec2 =
            world.resolve_car_footprint([start, 0.0], [-FRAME, 0.0], YAW_FACING_NEG_X);
        assert!(
            resolved[0] >= -CAR_FOOTPRINT[0] - 1.0e-3,
            "{}: 起点 x={start:.3}、一帧想走 {FRAME} m(墙只有 {THIN} m 厚),车却到了 x={:.3} —— 已在墙的另一侧",
            T_CAR_NO_WALL_TUNNEL,
            resolved[0]
        );
        // 车头不许越过墙的远面(x = −THIN/2)。
        let nose: f32 = resolved[0] - CAR_FOOTPRINT[0];
        assert!(
            nose >= -near_face - 1.0e-3,
            "{}: 车头 x={nose:.4} 已越过墙的远面 {near_face:.4}",
            T_CAR_NO_WALL_TUNNEL
        );
    }

    /// 斜着撞**薄墙**:必须**沿墙滑过去**,而不是穿墙。
    ///
    /// 墙厚 0.20 m —— 窄巷隔断的量级。这一条测的是**薄墙**上的穿透:
    /// 0.20 m 的墙配上一帧 1.60 m 的位移,旧实现一次
    /// `resolve_car(proposed)` 根本看不到重叠(落点已经在墙那边 1.4 m
    /// 处,比 1.25 m 的圆还远),于是既不挡也不推。判据取「沿墙滑动时
    /// 不得越过墙的远面」,而不是滑行距离 —— 距离多少取决于手感和墙长,
    /// 不是一个能证伪的性质。
    #[test]
    fn a_car_hitting_a_thin_wall_at_an_angle_slides_along_it() {
        const THIN: f32 = 0.20;
        let mut world: CollisionWorld = CollisionWorld::new();
        // 墙占 x ∈ [-THIN, 0]。Z 方向必须**足够长**:车沿墙滑行时一直往
        // +Z 走,墙若在 Z 上也有限长,车滑过端头之后就是真的开到墙外面
        // 去了,再往 -X 走当然畅通无阻 —— 那是正确物理,不是缺陷。
        world.push_aabb([-THIN * 0.5, 0.0], [THIN * 0.5, 400.0]);
        // 一路往 −X 带 +Z 斜着撞(45°),跑满 200 帧(每帧 0.283 m = 1.60/√2,
        // 正好是 `TrafficCar::drive` 满油门斜向走一帧的距离)。
        let stride: f32 = STRIDE_DIAGONAL;
        let mut at: Vec2 = [20.0, 14.0];
        for frame in 0..200 {
            at = world.resolve_car_footprint(at, [-stride, stride], YAW_FACING_NEG_X);
            assert!(
                at[0] >= -THIN - 1.0e-3,
                "{}: 第 {frame} 帧车被弹到了墙的另一侧 x={}(墙只占 x ∈ [{}, 0])",
                T_CAR_SLIDES_ALONG_WALL,
                at[0],
                -THIN
            );
        }
        // 切向必须保住:沿墙滑了一段距离,而不是钉死在接触点上。
        assert!(
            at[1] > 5.0,
            "{}: 切向位移被吃光,实得 z={}(应沿墙滑到 z 明显增大)",
            T_CAR_SLIDES_ALONG_WALL,
            at[1]
        );
        // 车身离墙的距离必须按**半车长**算(yaw = π,车头正对墙面)。
        // 旧的圆足迹停在 x = 1.25 —— 车身侧面离墙 1.033 m,看上去就是
        // 「车飘在墙外一米」;足迹修正后停在 2.283 m。
        assert!(
            (at[0] - CAR_FOOTPRINT[0]).abs() < 1.0e-2,
            "{}: 贴墙时车心离墙面 {:.3} m,应等于半车长 {:.3} m",
            T_CAR_FOOTPRINT_NOT_A_DISC,
            at[0],
            CAR_FOOTPRINT[0]
        );
    }

    /// 把上面几条合起来**泡很久**:各朝向的车满油门怼同一栋楼,跑满 600 帧,
    /// 车身的任何一个角都不许进墙。
    ///
    /// 单帧的测试各自只覆盖一个几何,真正在游戏里出问题的是**连续**状态:
    /// 车贴着墙蹭、车头慢慢压进去又弹出来。这一条按 15° 的朝向步长把整圈
    /// 都过一遍,每一帧都重新解一次,断言取**四个角**的最小间隙。
    #[test]
    fn a_car_soaked_at_every_yaw_never_enters_a_building() {
        const FRAME: f32 = 1.6;
        let mut world: CollisionWorld = CollisionWorld::new();
        // 一面长墙:x ∈ [-12, 0],Z 上足够长,车任何朝向都撞得到它。
        world.push_aabb([-6.0, 0.0], [6.0, 600.0]);
        let mut worst: f32 = f32::MAX;
        for step in 0..24 {
            // yaw = π 让车头大致对着墙(游戏里 forward = [cos, −sin])。
            let yaw: f32 = YAW_FACING_NEG_X + step as f32 * YAW_STEP_RAD;
            let (sin_yaw, cos_yaw): (f32, f32) = yaw.sin_cos();
            let forward: Vec2 = [cos_yaw, -sin_yaw];
            let side: Vec2 = [forward[1], -forward[0]];
            let (dir_x, dir_z): (f32, f32) = (forward[0], forward[1]);
            // 从墙外 30 m 处朝墙开,不给它停在接触点上的机会。
            let mut at: Vec2 = [30.0 + 30.0 * forward[0], 100.0 + 30.0 * forward[1]];
            for frame in 0..600 {
                let step_len: f32 = FRAME;
                at = world.resolve_car_footprint(at, [dir_x * step_len, dir_z * step_len], yaw);
                // 车身四角相对墙的内表面(x = 0)的最小间隙。
                for sign_l in [-1.0f32, 1.0] {
                    for sign_w in [-1.0f32, 1.0] {
                        let corner_x: f32 = at[0]
                            + forward[0] * sign_l * CAR_FOOTPRINT[0]
                            + side[0] * sign_w * CAR_FOOTPRINT[1];
                        let gap: f32 = corner_x;
                        if gap < worst {
                            worst = gap;
                        }
                        assert!(
                            gap >= -1.0e-3,
                            "{}: yaw={:.2} 第 {frame} 帧,车身某角 x={:.4} 已进墙 {:.4} m",
                            T_CAR_NEVER_INSIDE_A_WALL,
                            yaw,
                            gap,
                            -gap
                        );
                    }
                }
            }
        }
        // 泡完之后车确实贴着墙待过(证明断言不是「因为车根本没靠近才过」)。
        assert!(
            worst < 0.5,
            "{}: 600 帧里车身离墙最近 {:.3} m,却从未接近 —— 这条测试没有真的撞到墙",
            T_CAR_NEVER_INSIDE_A_WALL,
            worst
        );
    }
}
