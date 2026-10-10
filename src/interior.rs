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

use crate::{
    r#const::SHOWCASE_STAIR_RUN,
    r#type::{Vec2, Vec3},
};

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

/// 水平滑动把一帧切分成多小的子步(米)。
///
/// **必须小于最薄的那堵墙。** [`FloorWorld::resolve_interior_slide`] 靠
/// 子步把长帧切开:每一小步各自做一次完整 `move_and_slide`,只要一小步
/// 走不完一整堵墙,法向钳制就总能生效。最薄的是首层隔墙
/// (2 × `SHOWCASE_PARTITION_THICKNESS` = 0.15 m)。
///
/// **取 0.10 m**:约为最薄墙的 2/3,留出浮点余量;同时远小于冲刺一帧
/// 1.1269 m(拆成 12 小步)与步行一帧 0.31 m(拆成 4 小步)。
///
/// **60 fps 下不参与。** 步行一帧 `WALK_SPEED / 60 = 0.077 m` 已经
/// 小于它,`substeps == 1`,整帧仍走一次 —— 正常帧率的手感逐帧不变。
/// 取值一旦掉到 0.077 以下,那条 no-op 前提就没了,单测
/// `the_interior_sweep_is_a_no_op_at_sixty_fps` 会立刻红。
pub const INTERIOR_SLIDE_SUBSTEP: f32 = 0.10;

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

/// Inherent implementation of [`FloorWorld`].
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

    /// 沿**这一帧走过的整段**线段找脚下该踩的高度(楼梯的正解)。
    ///
    /// [`Self::support_height`] 是**单点**查询,而 `step_vertical` 只在
    /// 一帧走完之后才调它一次。软件渲染下 rAF 只有约 1.1 fps,一帧的
    /// 水平位移是 `WALK_SPEED * dt ≈ 0.61 m`(CDP 实测 `0.613 m/frame`),
    /// 而**每一级踏面只有 `SHOWCASE_STAIR_RUN = 0.45 m` 深** —— 一帧就跨过
    /// 1.43 级。落点已经越过了容差内的所有踏面,单点查询只剩首层地板
    /// (顶面 0.15)可选,于是爬到第 4 级之后再也上不去。
    ///
    /// 这里改成沿线段**扫**一遍,取「不低于脚底、且在容差内」的最高一块
    /// 踏面。扫过的那几级正是玩家真实踩过的,逐级抬升因此恢复。
    ///
    /// **判据同时钉死了「不许往下吸」。** 低于脚底的板一律不算支撑 ——
    /// 首层地板铺满整个楼内,少了这条判据,站在第 8 级时脚底 2.59 会被
    /// 首层地板(0.15)当成合法支撑直接吸下去,那正是「y 一步掉回地面
    /// 0.15」的形态。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 这一帧起点 XZ(玩家的旧位置)。
    /// - `Vec2` - 这一帧落点 XZ(玩家的新位置)。
    /// - `f32` - 玩家当前脚底高度(米)。
    ///
    /// # Returns
    ///
    /// - `Option<f32>` - 该段上可站的高度(米);脚下没有任何合法板时为
    ///   `None`(调用方按自由落体处理)。
    pub fn support_height_along(&self, from: Vec2, to: Vec2, from_y: f32) -> Option<f32> {
        let ceiling: f32 = from_y + STEP_UP_TOLERANCE;
        let span: f32 = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
        // 步长取踏深的四分之一:保证每一级至少被采到三个点,又不会把采样
        // 数随帧率放大(1.84 m 的一帧也只有 17 次查询)。
        let steps: usize = ((span / (SHOWCASE_STAIR_RUN * 0.25)).ceil() as usize).max(1);
        let mut best: Option<f32> = None;
        for index in 0..=steps {
            let t: f32 = index as f32 / steps as f32;
            let point: Vec2 = [
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
            ];
            let Some(top) = self.support_height(point, from_y) else {
                continue;
            };
            // 只认「不高于脚底容差上限、也不低于脚底」的那块。
            if top > ceiling || top < from_y - SUPPORT_CATCH_EPSILON {
                continue;
            }
            if best.is_none_or(|current: f32| top > current) {
                best = Some(top);
            }
        }
        best
    }

    /// 沿**这一帧走过的整段**线段找脚下该踩的高度(楼梯的正解)。
    ///
    /// [`Self::support_height`] 是**单点**查询,而 `step_vertical` 只在
    /// 一帧走完之后才调它一次。软件渲染下 rAF 只有约 1.1 fps,一帧的
    /// 水平位移是 0.613 m(CDP 实测),而**每一级踏面只有
    /// `SHOWCASE_STAIR_RUN = 0.45 m` 深** —— 一帧跨过 1.36 级。
    ///
    /// 关键在于**抬升速率**:楼梯坡度是 `0.305 / 0.45 = 0.678 m/m`,走
    /// 0.613 m 本该升 0.415 m;而单次 `support_height` 每次只返回「容差内
    /// 最高的一块」,**一次最多抬一级**(0.305 m,第二级就超出容差)。只查
    /// 一次的话,抬升速率追不上前进速率,亏空逐帧累积,最终脚下每一级都
    /// 高出容差,支撑彻底丢失,`y` 塌回首层地板 —— 实测正是「第 4 级
    /// y=1.370 之后一步掉回 0.150,之后 33 帧不动」。
    ///
    /// 所以这里把一帧切成一串 [`SUPPORT_SUBSTEP_DISTANCE`] 的小步,
    /// **每小步用当步的脚底重新取一次支撑并抬高**,抬升这才跟得上坡度。
    ///
    /// **判据同时钉死了「不许往下吸」。** 低于脚底的板一律不算支撑 ——
    /// 首层地板铺满整个楼内,少了这条判据,站在楼梯中段时脚底会被地板
    /// 重新拽回地面 0.15。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 这一帧起点 XZ(玩家的旧位置)。
    /// - `Vec2` - 这一帧落点 XZ(玩家的新位置)。
    /// - `f32` - 玩家当前脚底高度(米)。
    ///
    /// # Returns
    ///
    /// - `(Option<f32>, f32)` - `(最终支撑高度, 逐级抬升后的脚底高度)`。
    ///   支撑为 `None` 表示这一路都没有合法板,调用方按自由落体处理。
    ///
    /// # 下楼怎么处理的
    ///
    /// 每一小步做**扫掠抬升**(上楼时把脚底一级一级顶上去);全部小步走完后,
    /// **再补一次「允许往下落一��」的整体查询**,这才是下楼能一级一级
    /// 往下走的原因。
    ///
    /// **为什么必须补这一次:**扫掠的判据是「不接受低于脚底的面」,那是
    /// 为了挡住首层地板把楼梯中段的人吸回地面(楼内地板铺满全场)。但同
    /// 一条判据也会把**下一级踏面**(低一个踏高 0.305 m)一起拒掉 ——
    /// 于是往回走时每一小步都只认得脚下当前那一级,脚底一格都不降,人
    /// 就**悬在楼梯上横着往下挪**,直到走出梯段才掉进楼梯井。单测
    /// `a_normal_frame_descends_the_stairs_one_tread_at_a_time` 钉的就是
    /// 这一条(修复前它停在 2.895 不动)。
    ///
    /// **为什么补查必须带下界、不能直接用 [`Self::support_height`]。**
    /// `support_height` 只有上界没有下界,长帧(0.613 m/帧)下落点早已越过
    /// 好几级踏面,查出来的是**首层地板 0.15**,于是一路辛苦抬到 2.895
    /// 又被这一脚拉回地面 —— 正是组 1 最初那个缺陷的翻版(实测
    /// `a_long_frame_per_stride_still_reaches_the_top` 从 3.200 掉到 0.150)。
    /// 所以补查的下界取**一个踏高** [`STEP_DOWN_TOLERANCE`]:只放行
    /// 「往下一级」,不放行「掉到楼下」。首层地板离楼梯中段足有 2.4 m,
    /// 两个目标因此不会互相打架。
    pub fn support_along_frame(&self, from: Vec2, to: Vec2, from_y: f32) -> (Option<f32>, f32) {
        let span: f32 = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
        let substeps: usize = ((span / SUPPORT_SUBSTEP_DISTANCE).ceil() as usize).max(1);
        let mut y: f32 = from_y;
        let mut best: Option<f32> = None;
        let mut cursor: Vec2 = from;
        for index in 0..substeps {
            let t: f32 = (index + 1) as f32 / substeps as f32;
            let probe: Vec2 = [
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
            ];
            if let Some(top) = self.support_height_along(cursor, probe, y) {
                y = top;
                best = Some(top);
            }
            cursor = probe;
        }
        // 往下走一级:再扫一遍整段,但允许落点比**这一帧起点**的脚底低
        // 至多一个踏高。基准必须是起点而不是抬升后的高度 —— 后者会让
        // 补查把同一帧里刚抬上去的那一级又退回来(实测长帧上楼卡在 2.895)。
        //
        // **但这一帧只要抬升过,补查就必须让位。** 一个冲刺帧长 1.1 m,
        // 抬升阶段会顺着 6 个子步一级一级爬到 1.370;这时补查的下界是
        // 「帧起点 0.455 往下 0.355 = 0.100」,而**首层地板 0.150 正好落在
        // 这个窗口里** —— 楼梯底端那一段扫掠必然会采到它。于是补查把刚爬
        // 上去的 1.370 覆盖回 0.150,人钉死在首层,冲出梯顶再从楼梯井掉
        // 回地面(实测 peak_y=0.455、end_x 一路飞到 -6)。
        //
        // 关键在于**扫掠窗口和实际走过的段落不同**:抬升只看「不低于脚底」的
        // 那一格,补查却扫**整帧**并允许比帧起点低一个踏高。帧越长,补查
        // 越可能扫到低处那块铺满全场的首层地板;帧短的时候(步行 0.31 m)
        // 扫不到,于是缺陷只在使用冲刺时暴露。
        //
        // 语义上也说得通:「这一帧踩着更高的一级」与「这一帧往下一级」是
        // 互斥的,一帧之内两者不可能同时成立。因此抬升生效时直接返回,
        // 下楼的那一帧(抬升不生效)照旧补查。
        if y <= from_y + SUPPORT_CATCH_EPSILON
            && let Some(top) = self.step_down_along(from, to, from_y)
        {
            y = top;
            best = Some(top);
        }
        (best, y)
    }

    /// 沿整段找「比脚底低、但不超过一个踏高」的最高一块板(下楼用)。
    ///
    /// **只在确实比脚底更低时才返回。** 抬升阶段已经处理过「比脚底高」
    /// 的面,所以这里返回非 `None` 就等价于「这一帧在往下走」。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 这一帧起点 XZ。
    /// - `Vec2` - 这一帧落点 XZ。
    /// - `f32` - **这一帧开始时**的脚底高度(米),不是抬升后的高度。
    ///
    /// # Returns
    ///
    /// - `Option<f32>` - 可站的下一级高度;这一路上没有「往下一级」那么
    ///   深的面时为 `None`(调用方保持原高度,由重力处理)。
    fn step_down_along(&self, from: Vec2, to: Vec2, from_y: f32) -> Option<f32> {
        let ceiling: f32 = from_y + STEP_UP_TOLERANCE;
        let floor: f32 = from_y - STEP_DOWN_TOLERANCE;
        let span: f32 = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
        let steps: usize = ((span / (SHOWCASE_STAIR_RUN * 0.25)).ceil() as usize).max(1);
        let mut best: Option<f32> = None;
        for index in 0..=steps {
            let t: f32 = index as f32 / steps as f32;
            let point: Vec2 = [
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
            ];
            let Some(top) = self.support_height(point, from_y) else {
                continue;
            };
            if top > ceiling || top < floor {
                continue;
            }
            // 只接受**确实更低**的那一块,否则它与抬升阶段的结果重复。
            if top >= from_y - SUPPORT_CATCH_EPSILON {
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
    /// **但单步做这件事会穿墙,所以整帧必须先切子步。** 上面那套
    /// 分离是**位置推离**:它只看「现在在哪儿」,于是只要一帧的位移
    /// 超过墙的厚度,帧末人就落在墙的另一侧、且离墙面比半径还远 ——
    /// 推离向量恰好为 0,`length <= INTERIOR_SLIDE_EPSILON` 分支把
    /// **整帧**原样放行,墙被一步跨过去。
    ///
    /// 实测(CDP,HUD 逐帧,软件渲染 rAF ≈ 1.1 fps,`__vcw.vel` 中位数
    /// 8.3999 = `RUN_SPEED`):冲刺一帧推进 **1.1269 m**,而首层隔墙只有
    /// 0.15 m 厚、前墙门垛 0.20 m 厚。从 LOFT 梯脚(世界 27.3, 24.9)冲
    /// 向前墙,步行正确地停在 `x = 27.40`(门垛内表面 27.75 减半径
    /// 0.35),冲刺却在 15 帧里推进 15.25 m。同一面墙、同一时刻 ——
    /// 差别只有一帧多长。
    ///
    /// 所以这里把整帧切成一串 [`INTERIOR_SLIDE_SUBSTEP`] 的小步(和
    /// [`Self::support_along_frame` 对踏面做的事同形),**每小步各做一次
    /// 完整的 `move_and_slide`**。小步长度压到比最薄的墙还短,每一小步的
    /// 落点就不可能跳过整堵墙,法向钳制于是总能生效。
    ///
    /// **60 fps 下必须是 no-op**:步行一帧 `4.6 / 60 = 0.077 m`,远小于
    /// 一个小步,`substeps == 1`,整帧仍走一次 —— 正常帧率的手感逐帧
    /// 不变。这条由单测 `the_interior_sweep_is_a_no_op_at_sixty_fps` 守着。
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
        let span: f32 = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
        let substeps: usize = ((span / INTERIOR_SLIDE_SUBSTEP).ceil() as usize).max(1);
        if substeps <= 1 {
            return self.slide_one_step(point, delta, body_min_y, body_max_y, radius);
        }
        // 每小步走**等分**的一段(`delta / substeps`),不是从头累积的
        // `delta * t`:后者会让第 k 步再走一遍前 k-1 步的路,整帧位移被
        // 放大成 `delta * (1 + 2 + ... + n) / n`。
        let step: Vec2 = [delta[0] / substeps as f32, delta[1] / substeps as f32];
        let mut cursor: Vec2 = point;
        for _ in 0..substeps {
            cursor = self.slide_one_step(cursor, step, body_min_y, body_max_y, radius);
        }
        cursor
    }

    /// [`Self::resolve_interior_slide`] 切完子步之后,真正做**一步**
    /// `move_and_slide` 的那部分(单步版即修复前的实现)。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待分离的世界 XZ 坐标。
    /// - `Vec2` - 本步想要的水平位移(米)。
    /// - `f32` - 身体底面高度(脚底,米)。
    /// - `f32` - 身体顶面高度(头顶,米)。
    /// - `f32` - 身体等效圆柱半径(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 这一步走完(并保留切向位移)之后的世界 XZ 坐标。
    fn slide_one_step(
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

/// Default construction for [`FloorWorld`].
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

/// 「脚下的板」允许比脚底低多少仍然算支撑(米)。
///
/// 逐级抬升会把脚底**精确**放到踏面顶面(`support_height` 返回的就是
/// `max[1]`),所以稳定态下这个差值是 0。但浮点与「人正好站在两级交界」
/// 都会产生零点几毫米的负差,留 1 mm 余量,免得站在踏面正中间时被判成
/// 「脚下没有板」而开始自由落体。
///
/// **它必须远小于 `STEP_UP_TOLERANCE`**:首层地板铺满整个楼内,容差一旦
/// 放到踏高(0.305)以上,站在楼梯中段就会被首层地板重新吸回地面。
pub const SUPPORT_CATCH_EPSILON: f32 = 0.001;

/// 往下走一级台阶的最大高度(米)。
///
/// 抬升扫掠([`FloorWorld::support_height_along`])只接受「不低于脚底」的
/// 面,否则首层地板会把楼梯中段的人吸回地面。但那一条判据同时也把
/// **下一级踏面**拒掉了 —— 走下楼梯时脚底一格都不降,人悬在楼梯上横着
/// 挪。这个常量就是给「往下一级」单独开的口子。
///
/// **取 `SHOWCASE_STAIR_RISE`(0.305 m)加 5 cm 余量**,理由:
///
/// - **必须 ≥ 一个踏高**,否则下不了楼(下一级恰好低 0.305 m)。
/// - **必须远小于到首层地板的落差**。站在第 8 级时首层地板在脚下 2.44 m
///   处,取 0.355 与它相去甚远,所以「往下一级」和「掉到楼下」不会混淆 ——
///   这正是长帧修复要避免的第二个失效模式。
///
/// 上界仍然是 `STEP_UP_TOLERANCE`,两者一起构成一个以脚底为心的窗口
/// `[y - 0.355, y + 0.45]`。
pub const STEP_DOWN_TOLERANCE: f32 = crate::r#const::SHOWCASE_STAIR_RISE + 0.05;

/// 竖直解算把一帧切分成多小的水平小步(米)。
///
/// **取半个踏面**(`SHOWCASE_STAIR_RUN / 2`):保证每小步至少踩到一级踏面,
/// 同时抬升粒度仍比单级踏高细,不会一次跨两级(两级 0.61 m 已经超出
/// `STEP_UP_TOLERANCE = 0.45`)。
///
/// 换算成帧率:0.225 m 的小步在 `WALK_SPEED = 4.6` 下是 49 ms 一级,远
/// 快于 60 fps 的一帧(77 ms),所以真机上**大部分帧只有一个小步**,行为与
/// 修复前完全一致 —— 这条只影响长帧(低帧率 / 冲刺),不会引入回归。
pub const SUPPORT_SUBSTEP_DISTANCE: f32 = 0.225;

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
        T_INTERIOR_BASELINE_AGREES, T_INTERIOR_BASELINE_STILL_FAILS, T_INTERIOR_BASELINE_TOKEN,
        T_INTERIOR_CEILING_INSIDE, T_INTERIOR_DESCENT_NO_CLIMB, T_INTERIOR_DESCENT_REACHES_GROUND,
        T_INTERIOR_DOORWAY_BLOCKS, T_INTERIOR_DOORWAY_THROUGH, T_INTERIOR_DT_TOKEN,
        T_INTERIOR_HEIGHT_TOKEN, T_INTERIOR_LONG_FRAME_HEIGHT, T_INTERIOR_LONG_FRAME_LADDER,
        T_INTERIOR_NO_DOWNWARD_SNAP, T_INTERIOR_NO_SLAB_UNDER, T_INTERIOR_NORMAL_FRAME_DESCENT,
        T_INTERIOR_NORMAL_FRAME_UNCHANGED, T_INTERIOR_PEAK_TOKEN, T_INTERIOR_SPRINT_BASELINE_FAILS,
        T_INTERIOR_SPRINT_FRAME_CLIMBS, T_INTERIOR_SPRINT_NO_OP, T_INTERIOR_STAIR_CLIMBS,
        T_INTERIOR_STAIR_MONOTONIC, T_INTERIOR_STAIR_PAIR, T_INTERIOR_STAIRWELL_IS_OPEN,
        T_INTERIOR_STRIDE_TOKEN, T_INTERIOR_UPPER_FLOOR_ABOVE, T_INTERIOR_WALL_BLOCKS,
        T_INTERIOR_WALL_PASSES,
    };
    use crate::interior::{FloorWorld, STEP_UP_TOLERANCE, SUPPORT_SUBSTEP_DISTANCE};
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
    /// 60 fps 步行速度(米/秒),与 `crate::player::WALK_SPEED` 同值。
    const WALK_SPEED_60: f32 = 4.6;

    /// 把断言模板里的占位符换成实际数值。
    ///
    /// 占位符本身来自 `crate::r#const` 的 `T_INTERIOR_*_TOKEN` 常量,
    /// 与仓库既有做法一致(见 [`long_frame_height_message`])。
    fn fill(template: &str, args: &[(&str, &str)]) -> String {
        let mut out: String = template.to_string();
        for (key, value) in args {
            out = out.replace(key, value);
        }
        out
    }

    fn normal_frame_message(dt: f32, stride: f32, height: f32) -> String {
        let dt_token: &str = T_INTERIOR_DT_TOKEN;
        let stride_token: &str = T_INTERIOR_STRIDE_TOKEN;
        fill(
            T_INTERIOR_NORMAL_FRAME_UNCHANGED,
            &[
                (dt_token, &format!("{dt:.4}")),
                (stride_token, &format!("{stride:.3}")),
                (T_INTERIOR_HEIGHT_TOKEN, &format!("{height:.3}")),
            ],
        )
    }

    fn baseline_agrees_message(new: f32, old: f32) -> String {
        fill(
            T_INTERIOR_BASELINE_AGREES,
            &[
                (T_INTERIOR_PEAK_TOKEN, &format!("{new:.3}")),
                (T_INTERIOR_BASELINE_TOKEN, &format!("{old:.3}")),
            ],
        )
    }

    fn descent_message(height: f32) -> String {
        fill(
            T_INTERIOR_NORMAL_FRAME_DESCENT,
            &[(T_INTERIOR_HEIGHT_TOKEN, &format!("{height:.3}"))],
        )
    }

    fn baseline_fails_message(height: f32) -> String {
        fill(
            T_INTERIOR_BASELINE_STILL_FAILS,
            &[(T_INTERIOR_HEIGHT_TOKEN, &format!("{height:.3}"))],
        )
    }

    fn sprint_frame_message(stride: f32, height: f32) -> String {
        let stride_token: &str = T_INTERIOR_STRIDE_TOKEN;
        fill(
            T_INTERIOR_SPRINT_FRAME_CLIMBS,
            &[
                (stride_token, &format!("{stride:.3}")),
                (T_INTERIOR_HEIGHT_TOKEN, &format!("{height:.3}")),
            ],
        )
    }

    fn sprint_baseline_message(stride: f32, height: f32) -> String {
        let stride_token: &str = T_INTERIOR_STRIDE_TOKEN;
        fill(
            T_INTERIOR_SPRINT_BASELINE_FAILS,
            &[
                (stride_token, &format!("{stride:.3}")),
                (T_INTERIOR_HEIGHT_TOKEN, &format!("{height:.3}")),
            ],
        )
    }

    fn no_op_message(stride: f32, new: f32, old: f32) -> String {
        let stride_token: &str = T_INTERIOR_STRIDE_TOKEN;
        fill(
            T_INTERIOR_SPRINT_NO_OP,
            &[
                (stride_token, &format!("{stride:.3}")),
                (T_INTERIOR_PEAK_TOKEN, &format!("{new:.3}")),
                (T_INTERIOR_BASELINE_TOKEN, &format!("{old:.3}")),
            ],
        )
    }

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

    /// 把长帧的实测高度填进断言消息(§1.3c:字面量只在 `const.rs` 定义)。
    fn long_frame_height_message(height: f32) -> String {
        T_INTERIOR_LONG_FRAME_HEIGHT.replace(T_INTERIOR_HEIGHT_TOKEN, &format!("{height}"))
    }

    /// 楼梯世界的夹具,**几何与真实样板楼逐值一致**。
    ///
    /// **为什么必须用真实布局:**早先这个夹具的二层楼板只铺到
    /// `z = 4.45`,而踏面从 `z = -4.55` 排到 `0.05` —— 两者在 z 上**完全
    /// 不重叠**,于是「二层楼板盖住梯段」这件事在夹具上根本不成立。
    /// 真实的楼(`push_showcase_interior`)里二层楼板第二片是
    /// `x[sx0, hx] z[stair_end, hz]`,而梯段正好占 `z[stair_end, sz_last]`
    /// 且 `stair_end < sz_last` —— **整条梯段都在 landing 底下**。缺陷
    /// 是 09-29 引入的几何错误,夹具却因为摆错了形状而一直测不到它。
    ///
    /// 修好之后的真实布局(与 `assets/bldg_*_showcase.json` 的
    /// `floor_upper` 顶面逐块对齐;夹具把 z 轴镜像成「沿 +z 上行」,
    /// 两边 landing 与梯段的**相对关系**一致):
    ///
    /// ```text
    /// 首层地板  x[-5.75, 5.75]  z[-4.75, 4.75]  顶面 0.150
    /// 二层主片  x[-5.75, 4.45]  z[-4.75, 4.75]  顶面 3.200  (楼梯井左侧)
    /// 二层第二片 x[4.45, 5.75]  z[-0.05, 4.75]  顶面 3.200  (梯顶那一侧)
    /// 第 1..10 级 x[4.45, 5.75]  z[-4.55 .. 0.05]  0.455..3.200
    /// ```
    ///
    /// **关键不变量:梯段占的 z 带与第二片 landing 的 z 带只在端点
    /// `STAIR_TOP_Z = -0.05` 相接,不重叠。** 这正是修复要钉住的那一条 ——
    /// 真实楼里 landing 在梯段的**后方**(更小的 z),这里镜像成**前方**,
    /// 但两条要求在两个方向上是同一条。
    fn slab_world() -> FloorWorld {
        let mut w: FloorWorld = FloorWorld::new();
        w.push_slab([-5.75, 0.0, -4.75], [5.75, GROUND_TOP, 4.75]);
        // 二层主片:楼梯井左侧的整片楼板。
        w.push_slab([-5.75, UPPER_BOT, -4.75], [4.45, UPPER_TOP, 4.75]);
        // 梯段:沿 +z 上行,最低一级顶面 0.455,最高一级顶面 3.200。
        for i in 0..STAIR_STEPS {
            let z1: f32 = -4.55 + i as f32 * STAIR_RUN;
            let top: f32 = GROUND_TOP + (i + 1) as f32 * STAIR_RISE;
            w.push_slab([4.45, GROUND_TOP, z1], [5.75, top, z1 + STAIR_RUN]);
        }
        // 二层第二片:**从 `stair_end`(=-0.05)起往 +z 铺**,也就是梯顶的
        // 另一侧。楼梯井留在 `z[-4.55, -0.05]` 那一段,整条梯段头顶是空的。
        // 真实楼里这一片是 `z[-hz, stair_end]`,这里为了沿 +z 上行而镜像
        // 成 `z[stair_end, hz]` —— 两边的**相对关系**一致:landing 永远
        // 在梯段的「上坡方向之外」,不压住任何一级踏面。
        w.push_slab([4.45, UPPER_BOT, STAIR_TOP_Z], [5.75, UPPER_TOP, 4.75]);
        w
    }

    /// 梯顶那一级的**前沿** z(顶面最高处的边)——`stair_end`。
    ///
    /// 真实的 `push_showcase_interior` 里 `stair_end = sz_last - STEPS * RUN`,
    /// 二层第二片 landing 就接在它**外侧**(更小的 z)。夹具用同一个值,
    /// 于是「landing 会不会压住梯段」在夹具和真楼里是同一个问题。
    const STAIR_TOP_Z: f32 = -0.05;

    fn height_at(w: &FloorWorld, x: f32, z: f32, from_y: f32) -> Option<f32> {
        w.support_height([x, z], from_y)
    }

    /// **首层地板铺到梯脚之外**的楼梯世界(沿 -x 上行,与真实样板楼一致)。
    ///
    /// 之所以要另建一个夹具,是因为 [`slab_world`] 的地板和踏面**在 X 上
    /// 完全重叠**(地板到 5.75,踏面 4.45..5.75),于是「首层地板」永远不
    /// 可能是任何一点的最高支撑,补查扫不到它 —— 缺陷在那个夹具上根本不
    /// 成立。真实的楼(从 wasm 里 dump 出来的实测值)是:
    ///
    /// ```text
    /// 首层地板  x[18.250, 27.750]  y 顶面 0.150
    /// 第 1 级   x[27.100, 27.550]  y 顶面 0.455
    /// 第 2 级   x[26.650, 27.100]  y 顶面 0.760
    /// ...                              0.45 一级,往 -x 一路升到 3.200
    /// 二层楼板  x[18.250, 27.750]  y 顶面 3.200
    /// ```
    ///
    /// 关键差异:**地板比第一级踏面还往 +x 延伸 0.20 m**,而第一级踏面
    /// 只在 `x ≥ 27.10` 才开始。所以楼梯底端那一小段(27.55 → 27.10)同时
    /// 压在首层地板和第一级踏面上 —— 冲刺帧的整段补查必然采到地板 0.150,
    /// 把刚抬升上去的高度覆盖掉。这条重叠是缺陷成立的前提。
    fn overhanging_ground_world() -> FloorWorld {
        let mut w: FloorWorld = FloorWorld::new();
        // 地板往 +x 一直铺到 27.75,比第一级踏面(27.55)多 0.20。
        w.push_slab([-5.75, 0.0, -0.65], [27.75, GROUND_TOP, 0.65]);
        for i in 0..STAIR_STEPS {
            let x1: f32 = 27.55 - i as f32 * STAIR_RUN;
            let top: f32 = GROUND_TOP + (i + 1) as f32 * STAIR_RISE;
            w.push_slab([x1 - STAIR_RUN, GROUND_TOP, -0.65], [x1, top, 0.65]);
        }
        // 梯顶一侧的二层楼板:与最后一级齐平。
        w.push_slab([-5.75, UPPER_BOT, -0.65], [27.75, UPPER_TOP, 0.65]);
        w
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

    /// 一帧跨过多级踏面时,楼梯仍然爬得上去(线上「上到第 4 级就卡住」的回归)。
    ///
    /// 线上实测(CDP,软件渲染):rAF ≈ 1.1 fps,一帧水平位移 **0.613 m**,
    /// 而每级踏面只有 `STAIR_RUN = 0.45 m` 深 —— 一帧跨过 1.43 级。旧的
    /// 单点查询只看**落点**,那里早已越过了容差内的所有踏面,只剩首层地板
    /// (顶面 `GROUND_TOP = 0.15`)可选,于是 `y` 被压回地面后再也上不去。
    ///
    /// **注意判据是「跨多帧累积升到顶」,不是「一帧跨多级」。** 单帧仍然
    /// 只抬一级(踏高 0.305 m 在容差 0.45 m 内,第二级就超了),这是容差
    /// 的定义决定的正确行为;被测的是逐帧抬升能不能累积上去。
    #[test]
    fn a_long_frame_per_stride_still_reaches_the_top() {
        let w: FloorWorld = slab_world();
        // 线上实测的一帧水平位移:跨 0.613 / 0.45 = 1.43 级。
        let stride: f32 = 0.613;
        // 本测试世界的踏面随 i 向 **+z** 升高(`slab_world` 里
        // `y0 = -4.55 + i * STAIR_RUN`),所以上行方向是 +z。
        let mut z: f32 = -4.55 + 0.5 * STAIR_RUN;
        let mut y: f32 = GROUND_TOP;
        let mut heights: Vec<f32> = vec![y];
        for _ in 0..24 {
            let from: Vec2 = [5.1, z];
            z += stride;
            let to: Vec2 = [5.1, z];
            // 走线上真实的那条路径:整帧切分 + 逐级抬升。
            let (_, stepped): (Option<f32>, f32) = w.support_along_frame(from, to, y);
            y = stepped;
            heights.push(y);
        }
        // 一帧之内最多抬一级(容差的定义),所以是单调不降而不是单调升。
        for pair in heights.windows(2) {
            assert!(
                pair[1] >= pair[0] - 1e-4,
                "{}",
                T_INTERIOR_LONG_FRAME_LADDER
            );
        }
        // 但必须真的爬到顶,而不是停在首层楼板上。
        assert!(
            (y - UPPER_TOP).abs() < 1e-3,
            "{}",
            long_frame_height_message(y)
        );
    }

    /// 支撑面**绝不能低于脚底** —— 首层地板铺满整个楼内,一旦允许往下吸,
    /// 站在楼梯中段就会被地板拽回地面(线上「y 一步掉回 0.15」的形态)。
    #[test]
    fn a_surface_below_the_feet_is_never_support() {
        let w: FloorWorld = slab_world();
        // 站在第 8 级(顶面 2.590)上,脚下就是首层地板。
        let on_step_eight: f32 = GROUND_TOP + 8.0 * STAIR_RISE;
        let here: Vec2 = [5.1, -4.55 + 7.5 * STAIR_RUN];
        let got: Option<f32> = w.support_height_along(here, here, on_step_eight);
        assert_eq!(got, Some(on_step_eight), "{}", T_INTERIOR_NO_DOWNWARD_SNAP);
    }

    /// 修复前的基线:单点采样,**只看落点**。
    ///
    /// **这个函数只存在于测试里**,因为它就是被换掉的那版实现
    /// (`step_vertical` 原来每帧只调一次 `support_height(落点, y)`)。留着
    /// 它是为了让断言有对照:只有当旧实现在大 dt 下确实爬不上去时,新的
    /// 爬得上才说明缺陷被修了,而不是两个实现本来都一样。
    ///
    /// **只查落点,不查起点** —— 这是缺陷的关键。早先的版本把起点也算
    /// 进去,等于给旧实现多送一级台阶,于是它在大 dt 下也能爬上去,这条
    /// 对照测试就失去了意义(实测:多送起点后基线也能到 3.20)。
    fn single_point_baseline(w: &FloorWorld, to: Vec2, y: f32) -> f32 {
        w.support_height(to, y).unwrap_or(y)
    }

    /// 走完整个楼梯要多少帧 —— 60 fps 下每帧 0.077 m,整段楼梯 4.275 m。
    ///
    /// **必须刚好够走完、且不能走过头。** 楼梯在 z = -0.05 处到头,再往前
    /// 一格就没有支撑了,`support_height` 只会返回首层地板 —— 于是两种
    /// 实现都会从 3.20 掉回 0.15。所以断言必须盯**过程中的最高点**,
    /// 不能盯最后一帧:第一版盯了末帧,60 fps 走到第 55 帧时两种实现
    /// 都已走下楼梯,读数都是 0.150,测试红了但缺陷其实并不存在。
    const STAIRS_FRAME_BUDGET: usize = 52;

    /// 从 landing 走完整段梯段到地面要多少帧。
    ///
    /// 60 fps 每帧 0.077 m,梯段本身 4.5 m,加上起步那一格踏深,至少要
    /// 60 帧。取 100:走过头之后脚下是首层地板(y 恒 0.15),判据仍然成立,
    /// 而 52 帧只走到第 4 级就停了 —— 读数分不清「下到一半」和「下到地面」。
    const DESCENT_FRAME_BUDGET: usize = 100;

    /// 正常帧率(60 fps)下楼梯必须照旧逐级抬升 —— 长帧修复不得改坏它。
    ///
    /// **为什么必须单独测这一条:**长帧修复的杠杆是「把一帧切成
    /// `SUPPORT_SUBSTEP_DISTANCE` 的小步」。60 fps 下每帧只走
    /// `WALK_SPEED / 60 = 0.077 m`,远小于一个子步(0.225 m),于是
    /// `substeps == 1`,新路径**退化成**旧路径。若这一步悄悄变了(比如
    /// 子步取 0.05 m),60 fps 的楼梯手感就会被无声改掉,而软件渲染下
    /// 每帧 0.6 m,根本观察不到这个差异 —— 所以只能在单测里钉住。
    #[test]
    fn a_normal_frame_still_climbs_one_tread_at_a_time() {
        let w: FloorWorld = slab_world();
        // 60 fps 步行:4.6 m/s ÷ 60 = 0.0767 m/帧。
        let dt: f32 = 1.0 / 60.0;
        let stride: f32 = WALK_SPEED_60 * dt;
        let mut z: f32 = -4.55 + 0.5 * STAIR_RUN;
        let mut y: f32 = GROUND_TOP;
        let mut heights: Vec<f32> = vec![y];
        for _ in 0..STAIRS_FRAME_BUDGET {
            let from: Vec2 = [5.1, z];
            z += stride;
            let to: Vec2 = [5.1, z];
            let (_, stepped): (Option<f32>, f32) = w.support_along_frame(from, to, y);
            y = stepped;
            heights.push(y);
        }
        // 走到梯顶之前单调不降,且最高点正好是二层楼板面。
        //
        // 盯**峰值**而不是末帧:预算 52 帧刚好走完 4.0 m 楼梯,再多一格
        // 就会走出梯顶掉回首层,末帧读数将失去意义。
        let peak: f32 = heights.iter().copied().fold(f32::MIN, f32::max);
        let climbed: Vec<f32> = heights
            .iter()
            .copied()
            .take_while(|value: &f32| *value < UPPER_TOP - 1e-4)
            .collect();
        for pair in climbed.windows(2) {
            assert!(
                pair[1] >= pair[0] - 1e-4,
                "{}",
                T_INTERIOR_LONG_FRAME_LADDER
            );
        }
        assert!(
            (peak - UPPER_TOP).abs() < 1e-3,
            "{}",
            normal_frame_message(dt, stride, peak)
        );
    }

    /// 同一个 60 fps 步长下,**旧的单点采样也爬得上去** —— 因为每帧只走
    /// 0.077 m,远小于一个踏深 0.45 m,根本不会跨级。
    ///
    /// 这条把两种 dt 的分工钉死:缺陷只存在于「每帧位移 > 单级踏高」的
    /// 长帧;正常帧率下两种实现等价,所以长帧修复对 60 fps 必须是
    /// no-op(和上一条一起证明)。
    #[test]
    fn the_baseline_agrees_with_the_fix_at_sixty_fps() {
        let w: FloorWorld = slab_world();
        let dt: f32 = 1.0 / 60.0;
        let stride: f32 = WALK_SPEED_60 * dt;
        let mut z: f32 = -4.55 + 0.5 * STAIR_RUN;
        let mut y_new: f32 = GROUND_TOP;
        let mut y_old: f32 = GROUND_TOP;
        // 逐帧比较**峰值**,不是末帧:两种实现走过头都会从梯顶掉回首层,
        // 末帧读数于是都是 0.15,分不出高下。
        let mut peak_new: f32 = y_new;
        let mut peak_old: f32 = y_old;
        for _ in 0..STAIRS_FRAME_BUDGET {
            let from: Vec2 = [5.1, z];
            z += stride;
            let to: Vec2 = [5.1, z];
            let (_, stepped): (Option<f32>, f32) = w.support_along_frame(from, to, y_new);
            y_new = stepped;
            y_old = single_point_baseline(&w, to, y_old);
            peak_new = peak_new.max(y_new);
            peak_old = peak_old.max(y_old);
        }
        assert!(
            (peak_new - peak_old).abs() < 1e-3,
            "{}",
            baseline_agrees_message(peak_new, peak_old)
        );
    }

    /// 正常帧率下走下楼梯必须逐级下降,不得被「只接受不低于脚底的面」
    /// 卡成悬空,也不得自由落体。
    ///
    /// 起步点取**第 9 级**而不是第 10 级:第 10 级那一段被二层楼板压着
    ///(`slab_world` 的二层板顶面同为 `UPPER_TOP`),从它起步时脚下有两块
    ///同高的板,测不到「下一级比脚底低」这件事 —— 那是本条真正要验的
    ///判据(低于脚底的面在 `support_height_along` 里会被拒绝)。
    #[test]
    fn a_normal_frame_descends_the_stairs_one_tread_at_a_time() {
        let w: FloorWorld = slab_world();
        let dt: f32 = 1.0 / 60.0;
        let stride: f32 = WALK_SPEED_60 * dt;
        // 站在第 9 级(顶面 2.895),朝 -z 下行。下一级 2.590 比脚底低。
        let mut z: f32 = -4.55 + 8.0 * STAIR_RUN;
        let mut y: f32 = GROUND_TOP + 9.0 * STAIR_RISE;
        let mut heights: Vec<f32> = vec![y];
        for _ in 0..STAIRS_FRAME_BUDGET {
            let from: Vec2 = [5.1, z];
            z -= stride;
            let to: Vec2 = [5.1, z];
            let (_, stepped): (Option<f32>, f32) = w.support_along_frame(from, to, y);
            y = stepped;
            heights.push(y);
        }
        // 必须真的降到首层,而不是被按在原高度。
        assert!(y <= GROUND_TOP + STAIR_RISE, "{}", descent_message(y));
    }

    /// **从二层 landing 往下走必须逐级降回地面** —— 「楼内走不下楼梯」这条
    /// 真缺陷的回归。
    ///
    /// **实测缺陷(CDP,软件渲染,HUD 逐帧):** 二层楼板第二片 landing 写成了
    /// `z[stair_end, hz]`,而梯段正好占 `z[stair_end, sz_last]` 且
    /// `stair_end < sz_last` —— **整条梯段都在 landing 底下**。landing 顶面
    /// 恒为 3.20,`support_height` 取「容差内最高的一块」时它每帧都中选,
    /// 于是人站在梯段上永远是 y = 3.20:下楼沿 +X 走到 x = 27.4(墙)仍
    /// y = 3.2,`descended_to_ground` 恒 false。
    ///
    /// **这条必须用 [`slab_world`] 而不是自造夹具:** 缺陷成立的**前提**是
    /// 「二层 landing 的 z 带与梯段的 z 带重叠」。早先那个夹具的二层板止于
    /// `z = 4.45`、踏面占 `z[-4.55, 0.05]`,两者不重叠 —— 缺陷在那个夹具
    /// 上**永远测不到**,于是一条本该红的测试一直是绿的。现在夹具按真实
    /// 布局重建,`STAIR_TOP_Z = -0.05` 就是 landing 的起始边:改之前
    /// landing 从 `-4.75` 铺(压住整段),改之后从 `-0.05` 铺(让开梯段)。
    ///
    /// 三条判据一起钉:
    /// 1. **梯段头顶是空的** —— 站在第 5 级上,脚下只能有那一级,不能是 3.20。
    /// 2. **逐级降到地面** —— 走完整段楼梯后 y 必须真的落到首层。
    /// 3. **全程不出现抬升** —— 下楼过程中 dy 一旦 > 0,那是踩着 landing
    ///    往回爬,那条路径不算下楼。
    #[test]
    fn a_player_can_descend_from_the_upper_landing() {
        let w: FloorWorld = slab_world();
        // ---- 判据 1:楼梯井真的空着。
        // 站在第 5 级(顶面 1.980)正上方查支撑:只有那一级够得着。
        let mid_tread: f32 = GROUND_TOP + 5.0 * STAIR_RISE;
        let over_run: Vec2 = [5.1, -4.55 + 4.5 * STAIR_RUN];
        let got: Option<f32> = w.support_height(over_run, mid_tread);
        assert_eq!(
            got,
            Some(mid_tread),
            "{}",
            fill(
                T_INTERIOR_STAIRWELL_IS_OPEN,
                &[("{support:?}", &format!("{got:?}"))]
            )
        );
        // 顶住 landing 那条边界:landing 自己必须仍然站得住人(修复不能把
        // 二层楼板整块删掉,那会让上楼也上不去)。
        let on_landing: Vec2 = [5.1, STAIR_TOP_Z + 0.5 * STAIR_RUN];
        assert_eq!(
            w.support_height(on_landing, UPPER_TOP),
            Some(UPPER_TOP),
            "{}",
            fill(
                T_INTERIOR_STAIRWELL_IS_OPEN,
                &[("{support:?}", "landing 顶面不是 3.20")]
            )
        );

        // ---- 判据 2 + 3:从 landing 沿 -z 下行,逐级降到地面且不回头爬。
        let dt: f32 = 1.0 / 60.0;
        let stride: f32 = WALK_SPEED_60 * dt;
        // 起步点在 landing 上(顶面 3.20),朝 -z 就是下坡。
        let mut z: f32 = STAIR_TOP_Z + 0.5 * STAIR_RUN;
        let mut y: f32 = UPPER_TOP;
        let mut heights: Vec<f32> = vec![y];
        // 下楼要走完整段梯段(4.5 m)再加 landing 那一格,60 fps 每帧
        // 0.077 m,`STAIRS_FRAME_BUDGET`(=52)只够 4.0 m —— 会停在半路,
        // 读数分不清「下到一半」和「下到地面」。下楼给足 100 帧。
        for _ in 0..DESCENT_FRAME_BUDGET {
            let from: Vec2 = [5.1, z];
            z -= stride;
            let to: Vec2 = [5.1, z];
            let (_, stepped): (Option<f32>, f32) = w.support_along_frame(from, to, y);
            y = stepped;
            heights.push(y);
        }
        // 3. 不得出现抬升:任何一帧比上一帧高都算「往回上楼」。
        let mut climbed: Vec<usize> = Vec::new();
        for (index, pair) in heights.windows(2).enumerate() {
            if pair[1] > pair[0] + 1e-3 {
                climbed.push(index);
            }
        }
        assert!(
            climbed.is_empty(),
            "{}",
            fill(
                T_INTERIOR_DESCENT_NO_CLIMB,
                &[("{heights:?}", &format!("{heights:?}"))]
            )
        );
        // 2. 必须真的降到首层,而不是悬在梯段上。
        assert!(
            y <= GROUND_TOP + 1e-2,
            "{}",
            fill(
                T_INTERIOR_DESCENT_REACHES_GROUND,
                &[("{heights:?}", &format!("{heights:?}"))]
            )
        );
    }

    /// 冲刺帧必须同样爬得上 —— 「下楼补查把刚抬上去的高度覆盖掉」的回归。
    ///
    /// **实测缺陷(CDP,软件渲染,冲刺 8.4 m/s + dt 钳到 `FIXED_DT * 4`):**
    /// 步行每帧 0.307 m,冲刺每帧 **1.13 m**。长帧下抬升阶段顺着子步爬到
    /// 1.370 是对的,坏就坏在**下楼补查**:它的下界取「帧起点脚底低一个踏高」
    /// (`0.455 - 0.355 = 0.100`),而首层地板顶面 0.150 正好落在窗口内,楼梯
    /// 底端那一段扫掠必然采到它。于是 1.370 被覆盖回 0.150,人钉死在首层,
    /// 冲出梯顶再掉回地面 —— 实测 `peak_y = 0.455`、`end_x` 一路飞到 -6。
    ///
    /// **为什么步行不受影响:** 0.307 m 的帧短到扫不到首层地板那块 XZ,
    /// 补查返回 `None`,缺陷只在帧长跨过整段梯脚时才暴露。所以这条必须用
    /// 冲刺步长测,拿行走的步长测永远是绿的。
    ///
    /// 用 [`overhanging_ground_world`] 而不是 [`slab_world`]:缺陷成立的
    /// 前提是「首层地板比第一级踏面还往梯脚方向多铺一截」,`slab_world`
    /// 的地板与踏面在 X 上完全重叠,复现不出来。
    #[test]
    fn a_sprinting_frame_still_reaches_the_top() {
        let w: FloorWorld = overhanging_ground_world();
        // 冲刺实测步长:8.4 m/s × dt 0.0667 s(软件渲染 + dt 钳位)。
        let stride: f32 = 1.1269;
        let mut x: f32 = 27.3;
        let mut y: f32 = GROUND_TOP + STAIR_RISE;
        let mut heights: Vec<f32> = vec![y];
        for _ in 0..24 {
            let from: Vec2 = [x, 0.0];
            x -= stride;
            let to: Vec2 = [x, 0.0];
            let (_, stepped): (Option<f32>, f32) = w.support_along_frame(from, to, y);
            y = stepped;
            heights.push(y);
        }
        let peak: f32 = heights.iter().copied().fold(f32::MIN, f32::max);
        assert!(
            (peak - UPPER_TOP).abs() < 1e-3,
            "{}",
            sprint_frame_message(stride, peak)
        );
    }

    /// 修复前那一版(下楼补查无条件覆盖)在这个步长下确实爬不上去 ——
    /// 没有这一条,「新实现爬上去了」就分不清是修好了还是本来就能爬。
    ///
    /// **必须用「首层地板铺到梯脚之外」的世界,不能用 [`slab_world`]。**
    /// 后者的地板只到 `x = 5.75`,而踏面正好占 `x ∈ [4.45, 5.75]`,两者
    /// 重叠 —— 补查在踏面下面永远采不到首层地板,缺陷在那个夹具上根本
    /// 复现不出来。真实的楼里首层地板铺满全场(到 `x = 27.75`),第一级踏面
    /// 才开始(27.10),楼梯底端那一段**同时压在两块板上**,这才是缺陷成立
    /// 的前提。
    #[test]
    fn the_step_down_pass_alone_kills_a_sprinting_frame() {
        let w: FloorWorld = overhanging_ground_world();
        // 冲刺实测步长:8.4 m/s × dt 0.0667 s(软件渲染 + dt 钳位)。
        let stride: f32 = 1.1269;
        let mut x: f32 = 27.3;
        let mut y: f32 = GROUND_TOP + STAIR_RISE;
        for _ in 0..24 {
            let from: Vec2 = [x, 0.0];
            x -= stride;
            let to: Vec2 = [x, 0.0];
            // 旧行为:先逐级抬升,再让整帧下楼补查无条件覆盖。
            let mut raised: f32 = y;
            let substeps: usize = ((stride / SUPPORT_SUBSTEP_DISTANCE).ceil() as usize).max(1);
            let mut cursor: Vec2 = from;
            for index in 0..substeps {
                let t: f32 = (index + 1) as f32 / substeps as f32;
                let probe: Vec2 = [from[0] + (to[0] - from[0]) * t, 0.0];
                if let Some(top) = w.support_height_along(cursor, probe, raised) {
                    raised = top;
                }
                cursor = probe;
            }
            y = w.step_down_along(from, to, y).unwrap_or(raised);
        }
        assert!(
            y < UPPER_TOP - 1.0,
            "{}",
            sprint_baseline_message(stride, y)
        );
    }

    /// 60 fps 下新实现与「补查照旧」的实现必须逐帧一致。
    ///
    /// 修复只在「这一帧抬升过」时跳过补查,而 60 fps 步行每帧只走
    /// 0.077 m(远小于一个子步 0.225 m),**抬升永远不生效**,所以补查照旧
    /// 执行 —— 新路径必须与修复前逐帧相同。软件渲染下每帧 0.6 m,
    /// 观察不到这个差异,只能靠单测钉住。
    #[test]
    fn the_sprint_fix_is_a_no_op_at_sixty_fps() {
        let w: FloorWorld = slab_world();
        let stride: f32 = WALK_SPEED_60 / 60.0;
        for (from, to, y0) in [
            ([5.1f32, -4.55f32 + 0.5 * STAIR_RUN], 0.0f32, GROUND_TOP),
            (
                [5.1, -4.55 + 8.0 * STAIR_RUN],
                0.0,
                GROUND_TOP + 9.0 * STAIR_RISE,
            ),
        ] {
            let from: Vec2 = [from[0], from[1] + to];
            let mut new_y: f32 = y0;
            let mut old_y: f32 = y0;
            for _ in 0..STAIRS_FRAME_BUDGET {
                let a: Vec2 = from;
                let b: Vec2 = [from[0], from[1] + stride];
                let (_, stepped) = w.support_along_frame(a, b, new_y);
                new_y = stepped;
                // 旧实现:同样的抬升,补查无条件执行。
                let mut raised: f32 = old_y;
                let substeps: usize = ((stride / SUPPORT_SUBSTEP_DISTANCE).ceil() as usize).max(1);
                let mut cursor: Vec2 = a;
                for index in 0..substeps {
                    let t: f32 = (index + 1) as f32 / substeps as f32;
                    let probe: Vec2 = [a[0], a[1] + (b[1] - a[1]) * t];
                    if let Some(top) = w.support_height_along(cursor, probe, raised) {
                        raised = top;
                    }
                    cursor = probe;
                }
                old_y = w.step_down_along(a, b, old_y).unwrap_or(raised);
            }
            assert!(
                (new_y - old_y).abs() < 1e-6,
                "{}",
                no_op_message(stride, new_y, old_y)
            );
        }
    }

    /// 旧实现在**大 dt** 下确实爬不上去 —— 没有这一条,上面三条的「新实现
    /// 爬上去了」就分不清是修好了还是本来就能爬。
    #[test]
    fn the_baseline_still_fails_on_a_long_frame() {
        let w: FloorWorld = slab_world();
        // 软件渲染实测:dt 被钳到 FIXED_DT * 4,步行每帧 0.307 m。
        let stride: f32 = 0.307;
        let mut z: f32 = -4.55 + 0.5 * STAIR_RUN;
        let mut y: f32 = GROUND_TOP;
        for _ in 0..24 {
            let from: Vec2 = [5.1, z];
            z += stride;
            let to: Vec2 = [5.1, z];
            y = single_point_baseline(&w, to, y);
        }
        assert!(y < UPPER_TOP - 1.0, "{}", baseline_fails_message(y));
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
        // 站在二楼高度、脚下**没有**二层板的位置:楼梯井(梯段本身在
        // 这里,但它的踏面顶面远低于二楼,`support_height` 取「容差内最高
        // 的一块」时够不着,于是返回的是脚下唯一够得着的首层板 0.15)——
        // 那 3.05 m 的落差就是调用方判断「该走上去」还是「该掉下去」的依据。
        // **探针点必须在楼梯井里(二层板不存在的那一侧)。** `slab_world`
        // 的二层主片只盖到 `x = 4.45`,探针的 `x = 5.1` 落在楼梯井这一侧,
        // 头顶**没有** 3.20 的楼板 —— 这就是判据要验的:站在二楼高度,
        // 脚下这块没有楼板接着,于是 `support_height` 返回的是**远低于**
        // 二楼的最高一块(那一级踏面,或梯段外的首层地板),落差 3.05 m。
        //
        // `support_height` 只有上界(`from_y + STEP_UP_TOLERANCE`)没有下界,
        // 所以「够不着二楼板」时它会给出脚下最高的那一块。早先的夹具在
        // 这里**没有踏面**(二层板止于 4.45 而这条带是空的),读到的是
        // 首层地板 0.15;现在夹具按真实布局铺满踏面,读到的是那一级踏面
        // —— 两者都对,关键是**都不是 3.20**。
        let well: Vec2 = [5.1, -3.0];
        let upper: Option<f32> = height_at(&w, well[0], well[1], UPPER_TOP);
        assert!(
            upper.is_some_and(|height: f32| height < UPPER_TOP - STEP_UP_TOLERANCE),
            "{}",
            fill(
                T_INTERIOR_STAIRWELL_IS_OPEN,
                &[("{support:?}", &format!("{upper:?}"))]
            )
        );
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
