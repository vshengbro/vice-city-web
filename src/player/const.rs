use super::*;

/// 步行速度(米/秒)。
pub(crate) const WALK_SPEED: f32 = 4.6;

/// 奔跑速度(米/秒,按住 Shift)。
pub(crate) const RUN_SPEED: f32 = 8.4;

/// 转身插值速度(1/秒):越小转身越「重」,不会瞬移式扭头。
pub(crate) const TURN_RATE: f32 = 11.0;

/// 行走相位推进速率(rad / 米):每走 1 米推进约 2.6 rad,约 2.4 步。
pub(crate) const GAIT_RATE_PER_METER: f32 = 2.6;

/// 肢体摆动幅度上限(弧度)。
pub(crate) const LIMB_SWING: f32 = 0.62;

/// 脚相对小腿的**自转**摆角上限(弧度)。
///
/// 取 0 而不是非零:脚是被小腿**带着走**的,踝关节自身在这套骨架里
/// 不额外发力。留成可调常量是为了把「不摆」这件事显式写进骨架表,而不是
/// 靠「表里没登记」来隐式实现 —— 后者正是本缺陷的成因(见 [`LIMB_PLAN`]
/// 里鞋那一段的注释)。要加踝部旋转时改这一个数即可。
pub(crate) const FOOT_SWING: f32 = 0.0;

/// 停止后相位回零的速率(1/秒)。
pub(crate) const GAIT_RELAX_RATE: f32 = 6.5;

/// 起步 / 停步时速度的指数趋近速率(1/秒)。
pub(crate) const SPEED_RAMP: f32 = 14.0;

/// 低于这个平面速度(米/秒)就认为「站住了」:不转身、不推进相位。
pub(crate) const PLANAR_EPSILON: f32 = 0.05;

/// 玩家初始生命值。
pub(crate) const MAX_HEALTH: f32 = 100.0;

/// 玩家的全部**可动**骨架 part(与 `ped_suit` 的 part 划分一一对应)。
///
/// 每一项是 `(part 名, 父 part 名, 相位偏移, 摆动幅度)`。
///
/// **`parent` 不能省。** 资产把 `upper_leg_L` / `lower_leg_L` 切成两个
/// 独立 part,各自带**绝对**局部坐标。渲染一节 limb 时必须知道它的父
/// part 叫什么,才能把父关节的摆角叠加进来(见 [`limb_chain_matrix`])。
/// 躯干 / 头 / 鞋这类没有父关节的 part 用空串表示「就是链根」。
///
/// 相位:左右反相(`GAIT_PHASE_R = π`),同一侧上下同相位,形成对角步态。
pub(crate) const LIMB_PLAN: &[(&str, &str, f32, f32)] = &[
    (PART_UPPER_ARM_L, "", GAIT_PHASE_L, LIMB_SWING),
    (
        PART_LOWER_ARM_L,
        PART_UPPER_ARM_L,
        GAIT_PHASE_L,
        LIMB_SWING * 0.8,
    ),
    (PART_UPPER_ARM_R, "", GAIT_PHASE_R, LIMB_SWING),
    (
        PART_LOWER_ARM_R,
        PART_UPPER_ARM_R,
        GAIT_PHASE_R,
        LIMB_SWING * 0.8,
    ),
    (PART_UPPER_LEG_L, "", GAIT_PHASE_L, LIMB_SWING),
    (
        PART_LOWER_LEG_L,
        PART_UPPER_LEG_L,
        GAIT_PHASE_L,
        LIMB_SWING * 0.9,
    ),
    (PART_UPPER_LEG_R, "", GAIT_PHASE_R, LIMB_SWING),
    (
        PART_LOWER_LEG_R,
        PART_UPPER_LEG_R,
        GAIT_PHASE_R,
        LIMB_SWING * 0.9,
    ),
    // 鞋挂在**小腿**上,不是躯干上。
    //
    // 漏掉这两项时 `limb_swing` 对鞋返回 `0.0`,于是鞋的矩阵退化成纯位姿
    // (只有 `T(origin)·Ry(yaw)`),整条腿绕髋转起来时脚留在原地 —— 表现
    // 就是「鞋子不跟随腿部」。实测(HEAD `25f4a53`):走路时 `shoe_L` 的平移列
    // 恒等于 `origin`,而 `lower_leg_L` 在 ±0.39 m 之间摆动。
    //
    // 摆角取 `0`:脚本身相对小腿**不摆**,它是被小腿带着走的。GTA V 里
    // 落脚那一瞬有轻微的踝部旋转,那属于摆动相位而不是幅度问题,靠
    // `SWING` 之外的相位项表达;在这里给一个与小腿同相位的常量会把脚
    // 变成第二条腿。
    (PART_SHOE_L, PART_LOWER_LEG_L, GAIT_PHASE_L, FOOT_SWING),
    (PART_SHOE_R, PART_LOWER_LEG_R, GAIT_PHASE_R, FOOT_SWING),
];

/// 左腿相位:0。
pub(crate) const GAIT_PHASE_L: f32 = 0.0;

/// 右腿相位:与左腿反相。
pub(crate) const GAIT_PHASE_R: f32 = PI;
