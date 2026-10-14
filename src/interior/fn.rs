use super::*;

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
pub(crate) fn push_out_aabb(
    center: Vec2,
    half: Vec2,
    point: Vec2,
    radius: f32,
) -> Option<(Vec2, f32)> {
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
    use super::*;
    pub(crate) const STAIR_RISE: f32 = 0.305;
    pub(crate) const STAIR_RUN: f32 = 0.45;
    pub(crate) const STAIR_STEPS: usize = 10;
    pub(crate) const GROUND_TOP: f32 = 0.15;
    pub(crate) const UPPER_TOP: f32 = 3.20;
    pub(crate) const UPPER_BOT: f32 = 2.95;
    pub(crate) const DOOR_HALF: f32 = 0.80;
    pub(crate) const DOOR_TOP: f32 = 2.45;
    pub(crate) const WALL_T: f32 = 0.25;
    pub(crate) const BODY_TOP: f32 = 1.90;
    pub(crate) const RADIUS: f32 = 0.35;
    /// 60 fps 步行速度(米/秒),与 `crate::player::WALK_SPEED` 同值。
    pub(crate) const WALK_SPEED_60: f32 = 4.6;

    /// 把断言模板里的占位符换成实际数值。
    ///
    /// 占位符本身来自 `crate::r#config` 的 `T_INTERIOR_*_TOKEN` 常量,
    /// 与仓库既有做法一致(见 [`long_frame_height_message`])。
    pub(crate) fn fill(template: &str, args: &[(&str, &str)]) -> String {
        let mut out: String = template.to_string();
        for (key, value) in args {
            out = out.replace(key, value);
        }
        out
    }

    pub(crate) fn normal_frame_message(dt: f32, stride: f32, height: f32) -> String {
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

    pub(crate) fn baseline_agrees_message(new: f32, old: f32) -> String {
        fill(
            T_INTERIOR_BASELINE_AGREES,
            &[
                (T_INTERIOR_PEAK_TOKEN, &format!("{new:.3}")),
                (T_INTERIOR_BASELINE_TOKEN, &format!("{old:.3}")),
            ],
        )
    }

    pub(crate) fn descent_message(height: f32) -> String {
        fill(
            T_INTERIOR_NORMAL_FRAME_DESCENT,
            &[(T_INTERIOR_HEIGHT_TOKEN, &format!("{height:.3}"))],
        )
    }

    pub(crate) fn baseline_fails_message(height: f32) -> String {
        fill(
            T_INTERIOR_BASELINE_STILL_FAILS,
            &[(T_INTERIOR_HEIGHT_TOKEN, &format!("{height:.3}"))],
        )
    }

    pub(crate) fn sprint_frame_message(stride: f32, height: f32) -> String {
        let stride_token: &str = T_INTERIOR_STRIDE_TOKEN;
        fill(
            T_INTERIOR_SPRINT_FRAME_CLIMBS,
            &[
                (stride_token, &format!("{stride:.3}")),
                (T_INTERIOR_HEIGHT_TOKEN, &format!("{height:.3}")),
            ],
        )
    }

    pub(crate) fn sprint_baseline_message(stride: f32, height: f32) -> String {
        let stride_token: &str = T_INTERIOR_STRIDE_TOKEN;
        fill(
            T_INTERIOR_SPRINT_BASELINE_FAILS,
            &[
                (stride_token, &format!("{stride:.3}")),
                (T_INTERIOR_HEIGHT_TOKEN, &format!("{height:.3}")),
            ],
        )
    }

    pub(crate) fn no_op_message(stride: f32, new: f32, old: f32) -> String {
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
    pub(crate) fn cross_wall() -> FloorWorld {
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
    pub(crate) fn sliding_along_a_wall_keeps_the_tangential_step() {
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
    pub(crate) fn head_on_into_a_wall_never_crosses_it() {
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
    pub(crate) fn a_partition_wall_blocks_sight_only_across_itself() {
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
    pub(crate) fn an_upper_slab_does_not_block_eye_level_sight() {
        let mut world: FloorWorld = FloorWorld::new();
        world.push_slab([-5.0, UPPER_BOT, -5.0], [5.0, UPPER_TOP, 5.0]);
        // 视线高度 1.32 m,在二楼地板下面:通畅。
        assert!(!world.blocks_sight([-4.0, 0.0], [4.0, 0.0], 1.32));
        // 视线正好在板面高度:被挡。
        assert!(world.blocks_sight([-4.0, 0.0], [4.0, 0.0], UPPER_BOT));
    }

    /// 把长帧的实测高度填进断言消息(§1.3c:字面量只在 `const.rs` 定义)。
    pub(crate) fn long_frame_height_message(height: f32) -> String {
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
    pub(crate) fn slab_world() -> FloorWorld {
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
    pub(crate) const STAIR_TOP_Z: f32 = -0.05;

    pub(crate) fn height_at(w: &FloorWorld, x: f32, z: f32, from_y: f32) -> Option<f32> {
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
    pub(crate) fn overhanging_ground_world() -> FloorWorld {
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

    pub(crate) fn step_along_stair(w: &FloorWorld) -> Vec<f32> {
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
    pub(crate) fn stair_ramp_returns_increasing_heights() {
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
    pub(crate) fn stair_tolerance_exceeds_one_rise_and_one_storey() {
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
    pub(crate) fn a_long_frame_per_stride_still_reaches_the_top() {
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
    pub(crate) fn a_surface_below_the_feet_is_never_support() {
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
    pub(crate) fn single_point_baseline(w: &FloorWorld, to: Vec2, y: f32) -> f32 {
        w.support_height(to, y).unwrap_or(y)
    }

    /// 走完整个楼梯要多少帧 —— 60 fps 下每帧 0.077 m,整段楼梯 4.275 m。
    ///
    /// **必须刚好够走完、且不能走过头。** 楼梯在 z = -0.05 处到头,再往前
    /// 一格就没有支撑了,`support_height` 只会返回首层地板 —— 于是两种
    /// 实现都会从 3.20 掉回 0.15。所以断言必须盯**过程中的最高点**,
    /// 不能盯最后一帧:第一版盯了末帧,60 fps 走到第 55 帧时两种实现
    /// 都已走下楼梯,读数都是 0.150,测试红了但缺陷其实并不存在。
    pub(crate) const STAIRS_FRAME_BUDGET: usize = 52;

    /// 从 landing 走完整段梯段到地面要多少帧。
    ///
    /// 60 fps 每帧 0.077 m,梯段本身 4.5 m,加上起步那一格踏深,至少要
    /// 60 帧。取 100:走过头之后脚下是首层地板(y 恒 0.15),判据仍然成立,
    /// 而 52 帧只走到第 4 级就停了 —— 读数分不清「下到一半」和「下到地面」。
    pub(crate) const DESCENT_FRAME_BUDGET: usize = 100;

    /// 正常帧率(60 fps)下楼梯必须照旧逐级抬升 —— 长帧修复不得改坏它。
    ///
    /// **为什么必须单独测这一条:**长帧修复的杠杆是「把一帧切成
    /// `SUPPORT_SUBSTEP_DISTANCE` 的小步」。60 fps 下每帧只走
    /// `WALK_SPEED / 60 = 0.077 m`,远小于一个子步(0.225 m),于是
    /// `substeps == 1`,新路径**退化成**旧路径。若这一步悄悄变了(比如
    /// 子步取 0.05 m),60 fps 的楼梯手感就会被无声改掉,而软件渲染下
    /// 每帧 0.6 m,根本观察不到这个差异 —— 所以只能在单测里钉住。
    #[test]
    pub(crate) fn a_normal_frame_still_climbs_one_tread_at_a_time() {
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
    pub(crate) fn the_baseline_agrees_with_the_fix_at_sixty_fps() {
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
    pub(crate) fn a_normal_frame_descends_the_stairs_one_tread_at_a_time() {
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
    pub(crate) fn a_player_can_descend_from_the_upper_landing() {
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
    pub(crate) fn a_sprinting_frame_still_reaches_the_top() {
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
    pub(crate) fn the_step_down_pass_alone_kills_a_sprinting_frame() {
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
    pub(crate) fn the_sprint_fix_is_a_no_op_at_sixty_fps() {
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
    pub(crate) fn the_baseline_still_fails_on_a_long_frame() {
        let w: FloorWorld = slab_world();
        // 软件渲染实测:dt 被钳到 FIXED_DT * 4,步行每帧 0.307 m。
        let stride: f32 = 0.307;
        let mut z: f32 = -4.55 + 0.5 * STAIR_RUN;
        let mut y: f32 = GROUND_TOP;
        for _ in 0..24 {
            let _from: Vec2 = [5.1, z];
            z += stride;
            let to: Vec2 = [5.1, z];
            y = single_point_baseline(&w, to, y);
        }
        assert!(y < UPPER_TOP - 1.0, "{}", baseline_fails_message(y));
    }

    #[test]
    pub(crate) fn wall_between_two_rooms_blocks_movement() {
        let mut w: FloorWorld = FloorWorld::new();
        w.push_slab([-5.0, 0.0, -5.0], [5.0, GROUND_TOP, 5.0]);
        w.push_wall([-2.0, GROUND_TOP, -0.10], [2.0, 2.75, 0.10]);
        let pushed: Vec2 = w.resolve_interior([0.0, 0.0], GROUND_TOP, BODY_TOP, RADIUS);
        assert!(pushed[1].abs() > 0.1, "{}", T_INTERIOR_WALL_BLOCKS);
        let away: Vec2 = w.resolve_interior([0.0, 2.0], GROUND_TOP, BODY_TOP, RADIUS);
        assert!((away[1] - 2.0).abs() < 1e-4, "{}", T_INTERIOR_WALL_PASSES);
    }

    #[test]
    pub(crate) fn slab_above_head_does_not_block_player() {
        let w: FloorWorld = slab_world();
        let inside: Vec2 = w.resolve_interior([0.0, 0.0], GROUND_TOP, BODY_TOP, RADIUS);
        assert!(
            (inside[0] - 0.0).abs() < 1e-4 && (inside[1] - 0.0).abs() < 1e-4,
            "{}",
            T_INTERIOR_CEILING_INSIDE
        );
    }

    #[test]
    pub(crate) fn doorway_gap_is_the_only_way_through_the_front_wall() {
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
    pub(crate) fn support_height_reports_the_drop_when_walking_off_a_ledge() {
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
    pub(crate) fn probe_reports_containment_by_height() {
        let w: FloorWorld = slab_world();
        let on_ground: Vec2 = [0.0, 0.0];
        assert!(w.contains_interior_point(on_ground, GROUND_TOP * 0.5));
        assert!(!w.contains_interior_point(on_ground, 1.5));
        let under_stair: Vec2 = [5.1, -4.0];
        assert!(w.contains_interior_point(under_stair, 0.5));
    }

    #[test]
    pub(crate) fn outside_the_footprint_there_is_no_support() {
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
