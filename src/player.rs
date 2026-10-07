//! 玩家角色:第三人称移动、步态动画、跟随相机目标、生命值与拾取计数。
//!
//! 复用 `ped_suit` 资产(不新建模)。行人资产在 JSON 里是**分部件**的
//! (`torso` / `head` / `hair` / `upper_arm_L` … / `lower_leg_R` / `shoe_R`),
//! 所以渲染时把每个 part 拆成一个独立 mesh + 独立批次,每帧按行走相位
//! 给每个 part 算一条 model matrix —— 上臂 / 小臂 / 大腿 / 小腿绕各自
//! 关节的局部 X 轴摆动,鞋子随小腿一起摆,停下时相位衰减回 0、肢体归位。
//!
//! 关节枢轴点不是手写的魔法坐标,而是从**每个 part 自己的包围盒**推出来
//! 的:上臂枢轴 = 肩(盒顶面 Y + 靠近身体一侧的 X),大腿 = 胯,小腿 = 膝。
//! 换模型时只要 part 划分一致,骨架自动跟着长。

use crate::{
    camera::Mat4,
    collision::CollisionWorld,
    r#const::*,
    r#type::{Vec2, Vec3},
};

/// 步行速度(米/秒)。
pub const WALK_SPEED: f32 = 4.6;
/// 奔跑速度(米/秒,按住 Shift)。
pub const RUN_SPEED: f32 = 8.4;
/// 转身插值速度(1/秒):越小转身越「重」,不会瞬移式扭头。
pub const TURN_RATE: f32 = 11.0;
/// 行走相位推进速率(rad / 米):每走 1 米推进约 2.6 rad,约 2.4 步。
pub const GAIT_RATE_PER_METER: f32 = 2.6;
/// 肢体摆动幅度上限(弧度)。
pub const LIMB_SWING: f32 = 0.62;
/// 脚相对小腿的**自转**摆角上限(弧度)。
///
/// 取 0 而不是非零:脚是被小腿**带着走**的,踝关节自身在这套骨架里
/// 不额外发力。留成可调常量是为了把「不摆」这件事显式写进骨架表,而不是
/// 靠「表里没登记」来隐式实现 —— 后者正是本缺陷的成因(见 [`LIMB_PLAN`]
/// 里鞋那一段的注释)。要加踝部旋转时改这一个数即可。
pub const FOOT_SWING: f32 = 0.0;
/// 停止后相位回零的速率(1/秒)。
pub const GAIT_RELAX_RATE: f32 = 6.5;
/// 起步 / 停步时速度的指数趋近速率(1/秒)。
pub const SPEED_RAMP: f32 = 14.0;
/// 低于这个平面速度(米/秒)就认为「站住了」:不转身、不推进相位。
pub const PLANAR_EPSILON: f32 = 0.05;
/// 玩家初始生命值。
pub const MAX_HEALTH: f32 = 100.0;

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
pub const LIMB_PLAN: &[(&str, &str, f32, f32)] = &[
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

/// 查某个 part 的父 part 名。
///
/// # Arguments
///
/// - `&str` - part 名。
///
/// # Returns
///
/// - `&'static str` - 父 part 名;链根(或不在计划表里)返回空串。
pub fn limb_parent(part: &str) -> &'static str {
    LIMB_PLAN
        .iter()
        .find(|(name, _, _, _): &&(&str, &str, f32, f32)| *name == part)
        .map_or("", |(_, parent, _, _): &(&str, &str, f32, f32)| parent)
}

/// 某个 part 在当前步态相位下的**相对父关节**摆动角度(弧度)。
///
/// 上下臂同相位、上下腿同相位、左右反相,形成对角的步态;幅度随
/// `gait_amount` 缩放,所以停下时全部归零。返回的是**相对角** —— 上一
/// 节的绝对摆角由渲染端在父关节变换之上再乘进来。
///
/// # Arguments
///
/// - `&str` - part 名。
/// - `f32` - 当前行走相位(弧度)。
/// - `f32` - 当前步态幅度 `0.0..1.0`。
///
/// # Returns
///
/// - `f32` - 该 part 相对父关节的摆角(弧度)。
pub fn limb_swing(part: &str, phase: f32, amount: f32) -> f32 {
    let Some((_, _, offset, scale)) = LIMB_PLAN
        .iter()
        .find(|(name, _, _, _): &&(&str, &str, f32, f32)| *name == part)
    else {
        return 0.0;
    };
    scale * amount * (phase + offset).sin()
}

/// 左腿相位:0。
pub const GAIT_PHASE_L: f32 = 0.0;
/// 右腿相位:与左腿反相。
pub const GAIT_PHASE_R: f32 = std::f32::consts::PI;

/// 一个玩家的运行时状态。
#[derive(Clone, Debug)]
pub struct Player {
    /// 脚下世界坐标(Y 恒为 0,踩在人行道高度上由场景统一处理)。
    position: Vec3,
    /// 朝向(绕 Y 轴弧度,0 = 面向 +X)。
    yaw: f32,
    /// 当前水平速度(X / Z,米/秒)。
    velocity: Vec2,
    /// 行走相位(弧度)。
    gait_phase: f32,
    /// 当前步态幅度 0..1,由速度平滑驱动。
    gait_amount: f32,
    /// 生命值。
    health: f32,
    /// 护甲值:先于生命值承伤,按比例吸收伤害。
    armor: f32,
    /// 已拾取的现金。
    cash: f32,
    /// 当前驾驶的车辆索引(`None` = 在地面上)。
    vehicle: Option<usize>,
    /// 上一次收到的拾取提示(空串 = 无提示)。
    notice: String,
    /// 已被拾取掉的拾取物索引。
    collected: Vec<usize>,
    /// 身体高度(米):脚底到头顶。室内分离靠它判断「够不够得着」一块
    /// 二层楼板 —— 站在一楼时头顶 1.9 m 够不到 2.95 m 的板,所以不会被
    /// 上方的楼板推开。
    height: f32,
    /// 垂直速度(米/秒,向上为正)。踩在楼板上时被清零。
    vertical_velocity: f32,
    /// 脚底是否正踩在某块楼板 / 地面上。
    grounded: bool,
}

impl Player {
    /// 脚下世界坐标。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 当前世界坐标(Y 恒为 0)。
    pub fn get_position(&self) -> Vec3 {
        self.position
    }

    /// 写入脚下世界坐标。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 新世界坐标。
    pub fn set_position(&mut self, value: Vec3) {
        self.position = value;
    }

    /// 朝向(绕 Y 轴弧度)。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前朝向。
    pub fn get_yaw(&self) -> f32 {
        self.yaw
    }

    /// 写入朝向。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新朝向(弧度)。
    pub fn set_yaw(&mut self, value: f32) {
        self.yaw = value;
    }

    /// 当前水平速度。
    ///
    /// # Returns
    ///
    /// - `Vec2` - X / Z 速度分量(米/秒)。
    pub fn get_velocity(&self) -> Vec2 {
        self.velocity
    }

    /// 写入当前水平速度。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 新速度(米/秒)。
    pub fn set_velocity(&mut self, value: Vec2) {
        self.velocity = value;
    }

    /// 行走相位。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前相位(弧度)。
    pub fn get_gait_phase(&self) -> f32 {
        self.gait_phase
    }

    /// 写入行走相位。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新相位(弧度)。
    pub fn set_gait_phase(&mut self, value: f32) {
        self.gait_phase = value;
    }

    /// 当前步态幅度。
    ///
    /// # Returns
    ///
    /// - `f32` - `0.0` 站立,`1.0` 全速行走。
    pub fn get_gait_amount(&self) -> f32 {
        self.gait_amount
    }

    /// 写入当前步态幅度。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新幅度。
    pub fn set_gait_amount(&mut self, value: f32) {
        self.gait_amount = value;
    }

    /// 生命值。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前生命值。
    pub fn get_health(&self) -> f32 {
        self.health
    }

    /// 写入生命值。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新生命值。
    pub fn set_health(&mut self, value: f32) {
        self.health = value;
    }

    /// 护甲值。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前护甲。
    pub fn get_armor(&self) -> f32 {
        self.armor
    }

    /// 写入护甲值。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新护甲。
    pub fn set_armor(&mut self, value: f32) {
        self.armor = value;
    }

    /// 累加护甲(拾取护甲背心时用)。
    ///
    /// # Arguments
    ///
    /// - `f32` - 增量(可以为负)。
    pub fn add_armor(&mut self, amount: f32) {
        self.set_armor((self.get_armor() + amount).clamp(0.0, MAX_ARMOR));
    }

    /// 已拾取的现金。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前现金。
    pub fn get_cash(&self) -> f32 {
        self.cash
    }

    /// 累加现金。
    ///
    /// # Arguments
    ///
    /// - `f32` - 增量(可以为负)。
    pub fn set_cash_add(&mut self, amount: f32) {
        self.cash += amount;
    }

    /// 当前驾驶的车辆索引。
    ///
    /// # Returns
    ///
    /// - `Option<usize>` - 在驾驶时为 `Some(索引)`,否则为 `None`。
    pub fn get_vehicle(&self) -> Option<usize> {
        self.vehicle
    }

    /// 写入当前驾驶的车辆索引。
    ///
    /// # Arguments
    ///
    /// - `Option<usize>` - 新车辆索引。
    pub fn set_vehicle(&mut self, value: Option<usize>) {
        self.vehicle = value;
    }

    /// 玩家是否坐在车里。
    ///
    /// # Returns
    ///
    /// - `bool` - 在车里时为 `true`。
    pub fn get_driving(&self) -> bool {
        self.vehicle.is_some()
    }

    /// 上一次收到的拾取提示。
    ///
    /// # Returns
    ///
    /// - `&str` - 提示文本,空串表示无提示。
    pub fn get_notice(&self) -> &str {
        &self.notice
    }

    /// 写入拾取提示。
    ///
    /// # Arguments
    ///
    /// - `String` - 新提示文本。
    pub fn set_notice(&mut self, value: String) {
        self.notice = value;
    }

    /// 已被拾取掉的拾取物索引。
    ///
    /// # Returns
    ///
    /// - `&Vec<usize>` - 已拾取的索引列表。
    pub fn get_collected_ref(&self) -> &Vec<usize> {
        &self.collected
    }

    /// 追加一个已拾取的拾取物索引。
    ///
    /// # Arguments
    ///
    /// - `usize` - 拾取物索引。
    pub fn set_collected_push(&mut self, index: usize) {
        self.collected.push(index);
    }

    /// 已被拾取的数量。
    ///
    /// # Returns
    ///
    /// - `usize` - 已拾取的拾取物个数。
    pub fn collected_count(&self) -> usize {
        self.get_collected_ref().len()
    }

    /// 身体高度(米)。
    ///
    /// # Returns
    ///
    /// - `f32` - 脚底到头顶的距离(米)。
    pub fn get_height(&self) -> f32 {
        self.height
    }

    /// 垂直速度(米/秒,向上为正)。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前垂直速度(米/秒)。
    pub fn get_vertical_velocity(&self) -> f32 {
        self.vertical_velocity
    }

    /// 写入垂直速度。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新的垂直速度(米/秒,向上为正)。
    pub fn set_vertical_velocity(&mut self, value: f32) {
        self.vertical_velocity = value;
    }

    /// 起跳(键位写入入口):给一个向上的初速度,并立刻离开「站着」状态。
    ///
    /// **必须同时把 `grounded` 打成 false。** 之前这个方法不存在,
    /// `vertical_velocity` 只由 `step_vertical` 在「离开支撑面」的
    /// 自由落体分支里写 —— 也就是说玩家站着时往上写一个正速度,
    /// 下一帧 `support - y <= STEP_UP_TOLERANCE` 依然成立,立刻被
    /// `set_vertical_velocity(0.0)` 抹掉。这正是「按了 Space 原地
    /// 弹一下就停」的那种假跳跃:速度被地板吃掉,人根本没离开地面。
    ///
    /// 初速度取 [`JUMP_VELOCITY`],顶高 1.245 m(见该常量的推导),
    /// 滞空 0.67 s。滞空途中的重力与落地判定都由 `step_vertical` 走
    /// **同一套**积分,这里不重复实现。
    ///
    /// # Arguments
    ///
    /// - `bool` - 上一帧是否踩在支撑面上;滞空时不得二次起跳。
    ///
    /// # Returns
    ///
    /// - `bool` - 本次是否真的起跳(滞空时为 `false`)。
    pub fn set_jump_requested(&mut self, was_grounded: bool) -> bool {
        if !was_grounded {
            return false;
        }
        // 经已有的 `set_*` 写入口,而不是 `self.field` 直写(§17.3/§17.12:
        // accessor 体内直写合法,业务方法里不合法)。
        self.set_vertical_velocity(JUMP_VELOCITY);
        self.set_grounded(false);
        true
    }

    /// 脚底是否正踩在某块楼板 / 地面上。
    ///
    /// # Returns
    ///
    /// - `bool` - 踩在支撑面上时为 `true`。
    pub fn get_grounded(&self) -> bool {
        self.grounded
    }

    /// 写入「是否踩在地上」。
    ///
    /// # Arguments
    ///
    /// - `bool` - 新的踩地状态。
    pub fn set_grounded(&mut self, value: bool) {
        self.grounded = value;
    }

    /// 在给定位置创建一个满血玩家。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 初始世界坐标。
    /// - `f32` - 初始朝向(弧度)。
    ///
    /// # Returns
    ///
    /// - `Self` - 就绪的玩家状态。
    pub fn new(position: Vec3, yaw: f32) -> Self {
        Self {
            position,
            yaw,
            velocity: [0.0, 0.0],
            gait_phase: 0.0,
            gait_amount: 0.0,
            health: MAX_HEALTH,
            armor: 0.0,
            cash: 0.0,
            vehicle: None,
            notice: String::new(),
            collected: Vec::new(),
            height: PLAYER_BODY_HEIGHT,
            vertical_velocity: 0.0,
            grounded: true,
        }
    }

    /// 把两轴按键输入的模长钳到不超过 1(斜向不快于直线的关键)。
    ///
    /// **钳长度,不是无脑除以模长。** 无脑归一化(`intent / |intent|`)会把
    /// 半个身位的输入 `(0.5, 0.5)`(模长 0.707)放大成满速 —— 手柄半推和
    /// 键盘全按之间就失去了区别,微调会变成冲刺。正确做法是**只把超过 1 的
    /// 模长压下来**,模长不足 1 的原样保留,于是半速输入仍然是半速。
    ///
    /// 效果:W 给出 `1`,W+D 给出 `1 / √2 = 0.7071`,于是合速度恒为
    /// `speed`(`speed * |intent|` ≤ `speed * 1`)。单轴输入时模长本来就是
    /// 1,`scale` 恒为 1,直线档位**逐位不变**。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 原始的两轴输入(各分量 −1..1)。
    ///
    /// # Returns
    ///
    /// - `f32` - 缩放系数,模长超过 1 时为 `1 / |intent|`,否则为 1。
    fn diagonal_scale(intent: Vec2) -> f32 {
        let length: f32 = (intent[0] * intent[0] + intent[1] * intent[1]).sqrt();
        if length > 1.0 { 1.0 / length } else { 1.0 }
    }

    /// 推进一个固定步长:移动、碰撞分离、转身、步态相位。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 期望的前 / 侧向输入(各分量 −1..1)。
    /// - `Vec2` - 相机「前」方向的世界 XZ 分量。
    /// - `f32` - 固定步长(秒)。
    /// - `f32` - 目标速度上限(米/秒)。
    /// - `&CollisionWorld` - 静态碰撞世界。
    pub fn step(
        &mut self,
        intent: Vec2,
        direction: Vec2,
        dt: f32,
        speed: f32,
        world: &CollisionWorld,
    ) {
        // 目标速度 = 前向分量 + 侧向分量,两者**合起来**受 `speed` 上限约束,
        // 所以斜向移动不比直线快(和 GTA V 的手感一致:那边按 W+D 也不会比
        // 单按 W 快)。
        //
        // **「各自受上限」并不够。** 这里原来直接把
        // `direction * forward + perp * strafe` 乘 `speed`,于是两轴分别顶到
        // `speed` 时合速度是 `speed * √(forward² + strafe²)` —— W+D 时
        // `√2 = 1.4142`。实测步行 6.5052(`4.6 * √2`,直线是 4.5998)、冲刺
        // 11.8787(`8.4 * √2`,直线是 8.3997),斜向比直线快 41.4%。紧挨着这
        // 段的旧注释写的却是「斜向移动不会比直线快」—— 注释描述的是**意图**,
        // 代码做的是另一回事,两者对不上,于是这个偏差一直没人发现。
        //
        // **归一化的是 `intent`(按键轴),不是世界方向。** `direction` 本身
        // 已经是单位向量(相机的水平朝向),旋过去不改变模长,所以「`intent`
        // 归一 + `speed` 乘在旋后的方向上」与「合速度钳到 `speed`」完全等价,
        // 取前者更省一次钳制。
        let scale: f32 = Self::diagonal_scale(intent);
        let forward: f32 = intent[1] * scale;
        let strafe: f32 = intent[0] * scale;
        let target: Vec2 = [
            (direction[0] * forward + -direction[1] * strafe) * speed,
            (direction[1] * forward + direction[0] * strafe) * speed,
        ];
        let ramp: f32 = (1.0 - (-SPEED_RAMP * dt).exp()).clamp(0.0, 1.0);
        let current: Vec2 = self.get_velocity();
        self.set_velocity([
            current[0] + (target[0] - current[0]) * ramp,
            current[1] + (target[1] - current[1]) * ramp,
        ]);

        // 碰撞:标准 **move_and_slide**。先把想要的位移交给
        // `resolve_slide`,它只沿碰撞法线推出穿透深度,**切向分量原样
        // 保留**。
        //
        // 之前这里是 `world.resolve(wanted)` —— 那是**全有全无**的位置
        // 钳制:圆心被直接推到形状的最近面,沿墙方向的位移也一起没了。
        // 于是顶着墙走时位移恰好为零,`want (-1.00, 0.00)` 拿到
        // `got (+0.00, +0.00)`,几十帧推不动一毫米。卡点不固定在某一
        // 面墙(实测 x = 28.3 / 28.4 / 33.1 / 35.0 各出现过一次),因为
        // **每一面墙都这样**:被弹到哪就贴住哪面墙。
        let velocity: Vec2 = self.get_velocity();
        let here: Vec3 = self.get_position();
        let delta: Vec2 = [velocity[0] * dt, velocity[1] * dt];
        let radius: f32 = world.get_player_radius();
        let resolved: Vec2 = world.resolve_slide([here[0], here[2]], delta, radius);
        self.set_position([resolved[0], here[1], resolved[1]]);

        // 速度取「**实际走出来的位移** / dt」,而不是把分离修正量加回去。
        // 后者会被 1/dt 放大几十倍,一次 1 cm 的分离就会注入 0.6 m/s 的
        // 侧向速度,角色会被「弹」着走。现在撞墙时**朝墙**的速度分量
        // 自然归零,不会一直顶着墙搓,也不会被弹飞。
        //
        // 关键在于 `resolved - here` 现在**包含切向位移**了(滑动之前是
        // 零),所以反算出来的速度正确地保留了沿墙分量,不会把玩家
        // 钉死在接触点上。
        let inverse_dt: f32 = 1.0 / dt.max(f32::EPSILON);
        let achieved: Vec2 = [resolved[0] - here[0], resolved[1] - here[2]];
        self.set_velocity([
            (achieved[0] * inverse_dt).clamp(-speed, speed),
            (achieved[1] * inverse_dt).clamp(-speed, speed),
        ]);

        // 转身 / 步态用**碰撞前的目标速度** `velocity`,而不是被墙削过的
        // 实际速度:蹭着墙走时手脚照样摆,只是真的走不动 —— 这才符合直觉。
        let planar: f32 = (velocity[0] * velocity[0] + velocity[1] * velocity[1]).sqrt();
        if planar > PLANAR_EPSILON {
            let desired: f32 = facing_yaw(velocity);
            let turning: f32 = wrap_angle(desired - self.get_yaw()) * TURN_RATE * dt;
            self.set_yaw(self.get_yaw() + turning);
        }

        // 步态:幅度跟随实际速度,相位按走过的距离推进。
        let ratio: f32 = (planar / speed).clamp(0.0, 1.0);
        let relaxed: f32 = (1.0 - (-GAIT_RELAX_RATE * dt).exp()).clamp(0.0, 1.0);
        if planar > PLANAR_EPSILON {
            let scaled: f32 = self.get_gait_amount() + (ratio - self.get_gait_amount()) * relaxed;
            self.set_gait_amount(scaled);
            self.set_gait_phase(
                (self.get_gait_phase() + planar * GAIT_RATE_PER_METER * dt)
                    % (2.0 * std::f32::consts::PI),
            );
        } else {
            let shrunk: f32 = self.get_gait_amount() - self.get_gait_amount() * relaxed;
            self.set_gait_amount(shrunk);
            self.set_gait_phase(self.get_gait_phase() * (1.0 - relaxed));
        }
        self.set_gait_amount(self.get_gait_amount().clamp(0.0, 1.0));
    }
}

/// 把角速度折成**朝向角**,使角色正面朝着它走的方向。
///
/// **朝向约定(全项目唯一的真源):`yaw = 0` 面向 `+Z`,且朝向向量为
/// `(sin yaw, cos yaw)`。** 这与渲染侧完全一致:
///
/// - `Mat4::rotation_y` 是列主序,第 3 列是 `(sin yaw, 0, cos yaw)`,
///   也就是资产本地 `+Z` 轴在世界里的落点;
/// - 行人资产的正面确实是本地 `+Z`:`ped_suit` 的鞋尖在
///   `z = +0.126 .. +0.378`(脚跟 z < 脚尖 z),`body` 零件同理。
///
/// 之前的实现写的是 `-vz.atan2(vx)`,那个约定是 **`yaw = 0` 面向 `+X`**,
/// 与渲染差**正好 90°**。后果:玩家朝 `+X` 走时 `yaw` 算成 0,渲染出来正面
/// 朝着 `+Z` —— 也就是「前进时侧身面向正前方」。这也解释了为什么转向动画
/// 本身看着是对的(`wrap_angle` 在两套约定下都自洽),但人在世界里永远斜着走。
///
/// # Arguments
///
/// - `Vec2` - 世界 XZ 速度(米/秒);长度任意,只取方向。
///
/// # Returns
///
/// - `f32` - 让资产正面朝向该速度的 `yaw`(弧度)。
pub fn facing_yaw(velocity: Vec2) -> f32 {
    velocity[0].atan2(velocity[1])
}

/// 把角度折到 (−π, π],避免转身走远路。
///
/// # Arguments
///
/// - `f32` - 原始角度差(弧度)。
///
/// # Returns
///
/// - `f32` - 折回后的角度差(弧度)。
pub fn wrap_angle(angle: f32) -> f32 {
    let two_pi: f32 = 2.0 * std::f32::consts::PI;
    let mut value: f32 = angle % two_pi;
    if value > std::f32::consts::PI {
        value -= two_pi;
    } else if value < -std::f32::consts::PI {
        value += two_pi;
    }
    value
}

/// 从 part 的本地包围盒推出关节枢轴。
///
/// 手臂 / 腿:枢轴 = 盒的**顶面** Y(肩 / 胯),绕 X 轴摆动就是标准的
/// 肩摆 / 髋摆;横向取靠近身体中线的一侧。鞋:枢轴 = 踝(盒顶面 Y)。
///
/// # Arguments
///
/// - `&str` - part 名。
/// - `(Vec3, Vec3)` - 该 part 的本地包围盒 `(min, max)`。
///
/// # Returns
///
/// - `Vec3` - 关节枢轴的资产本地坐标。
pub fn joint_pivot(part: &str, bounds: (Vec3, Vec3)) -> Vec3 {
    let (min, max) = bounds;
    if part.contains(PART_ARM) || part.contains(PART_LEG) {
        return [min[0], max[1], (min[2] + max[2]) * 0.5];
    }
    if part == PART_SHOE_L || part == PART_SHOE_R {
        return [(min[0] + max[0]) * 0.5, max[1], (min[2] + max[2]) * 0.5];
    }
    [0.0, 0.0, 0.0]
}

/// 一个玩家肢体的 model matrix:整体位姿 × **父子串联**的关节链摆动。
///
/// **串联是必须的,不是可选的。** 资产里 `upper_leg_L` 与 `lower_leg_L`
/// 是两个**独立**的 part,各自在资产空间里有绝对坐标(大腿
/// `y = 0.485 .. 0.946`,小腿 `y = 0.098 .. 0.486`)。如果两者各自绕
/// **同一个局部枢轴**摆动,大腿转 30° 时小腿不会跟着转 —— 膝盖以下整体
/// 原地不动,表现就是「两条腿像被切断又拼回去」。真实的人腿是**串联**的:
/// 髋转带动整条腿,膝再在髁的基础上多转一点。
///
/// 两条链各有两节,深度固定,所以用两个标量参数表达:
///
/// - `hip` / `knee` —— 髋关节摆角(整条腿绕胯)与膝关节摆角(小腿在
///   **已经转过 `hip` 的躯干**上再绕膝);
/// - `shoulder` / `elbow` —— 肩关节摆角与肘关节摆角。
///
/// 顺序固定为 `M = T(origin) · Ry(yaw) · T(hip) · Rx(hip_a) · T(knee)
/// · Rx(knee_a) · T(-knee)` —— 注意 `T(-knee)` 之前**必须**已经乘过
/// `Rx(hip_a)`,这样膝盖枢轴才是「跟随大腿转过之后」的位置。
///
/// # Arguments
///
/// - `Vec3` - 玩家脚下世界坐标。
/// - `f32` - 玩家朝向(弧度,`0` 面向 `+Z`)。
/// - `Vec3` - 根关节(肩 / 胯)枢轴的资产本地坐标。
/// - `Vec3` - 末端关节(肘 / 膝)枢轴的资产本地坐标。
/// - `f32` - 根关节摆角(弧度)。
/// - `f32` - 末端关节相对父关节的摆角(弧度)。
///
/// # Returns
///
/// - `Mat4` - 列主序 model matrix。
pub fn limb_chain_matrix(
    origin: Vec3,
    yaw: f32,
    root_pivot: Vec3,
    joint_pivot: Vec3,
    root_swing: f32,
    joint_swing: f32,
) -> Mat4 {
    // **每个 part 的顶点本来就在角色空间的绝对高度上,不能再平移一次。**
    //
    // 旧实现是
    //   `child = T(root) · Rx(root_swing) · T(joint) · Rx(joint_swing) · T(-joint)`
    // 链根(上臂 / 大腿,`root_pivot == joint_pivot`)展开后多出一个
    // `T(root) · T(-root)` 不成对,净效果是**把整节 part 抬高了
    // root_pivot.y**;末端节(小臂 / 小腿)则额外吃了整个肩 / 髋的平移。
    // 实测:上臂的局部 Y 范围是 1.095..1.431,加上多出来的 +1.431 之后
    // 世界 Y 变成 2.52..2.88 —— 手臂被举到头顶**上方** 0.8 m,和躯干
    // (0.84..1.56)、头(1.52..1.75)之间隔着一整段空隙。
    // 这就是「13 摞碎片、彼此之间大片背景空隙」的直接成因。
    //
    // 正确写法是**每个关节各自绕自己的枢轴原地转**,不再移动顶点:
    //   `M = T(origin) · Ry(yaw) · [T(root)·Rx(root_swing)·T(-root)] · [T(joint)·Rx(joint_swing)·T(-joint)]`
    // 父子关系体现在「小臂的旋转作用在上臂已经转过的坐标系上」,
    // 而这个串联效果由父节**自己的** `root_swing` 提供,不需要再平移。
    let placement: Mat4 = Mat4::translation(origin).multiply(&Mat4::rotation_y(yaw));
    let root: Mat4 = Mat4::translation(root_pivot)
        .multiply(&Mat4::rotation_x(root_swing))
        .multiply(&Mat4::translation([
            -root_pivot[0],
            -root_pivot[1],
            -root_pivot[2],
        ]));
    let child: Mat4 = Mat4::translation(joint_pivot)
        .multiply(&Mat4::rotation_x(joint_swing))
        .multiply(&Mat4::translation([
            -joint_pivot[0],
            -joint_pivot[1],
            -joint_pivot[2],
        ]));
    placement.multiply(&root).multiply(&child)
}

/// 一个玩家肢体的 model matrix:整体位姿 × 绕枢轴的局部 X 轴摆动。
///
/// 单关节版本(躯干 / 头 / 鞋这类没有父子关系的 part 用它):
/// `M = T(origin) · Ry(yaw) · T(pivot) · Rx(swing) · T(-pivot)`。
///
/// # Arguments
///
/// - `Vec3` - 玩家脚下世界坐标。
/// - `f32` - 玩家朝向(弧度,`0` 面向 `+Z`)。
/// - `Vec3` - 关节枢轴的资产本地坐标。
/// - `f32` - 该关节的摆角(弧度)。
///
/// # Returns
///
/// - `Mat4` - 列主序 model matrix。
pub fn limb_matrix(origin: Vec3, yaw: f32, pivot: Vec3, swing: f32) -> Mat4 {
    limb_chain_matrix(origin, yaw, pivot, pivot, swing, 0.0)
}

#[cfg(test)]
mod tests {
    use crate::camera::Mat4;
    use crate::collision::CollisionWorld;
    use crate::r#const::{
        DIAGONAL_SPEED_TOLERANCE, KEY_DIAGONAL, KEY_MODE, KEY_PART, KEY_STRAIGHT, MODE_SPRINT,
        MODE_WALK, PART_LOWER_ARM_L, PART_LOWER_LEG_L, PART_SHOE_L, PART_SHOE_R, PART_UPPER_ARM_L,
        PART_UPPER_LEG_L, PED_BOB_HEIGHT, T_DIAGONAL_MATCHES_STRAIGHT,
        T_DISTAL_INHERITS_PARENT_SWING, T_DISTAL_LIMB_HAS_PARENT, T_FACING_MATCHES_VELOCITY,
        T_HALF_INPUT_STAYS_HALF_SPEED, T_LEGS_ANTIPHASE, T_LIMB_STAYS_AT_ASSET_HEIGHT,
        T_LIMBS_RELAX_TO_ZERO, T_PARENT_IN_PLAN, T_PED_BOB_DOUBLE_FREQUENCY,
        T_PED_BOB_OUT_OF_PHASE, T_SAME_SIDE_IN_PHASE, T_SHOE_FOLLOWS_SHIN,
        T_STRAIGHT_KEEPS_FULL_SPEED,
    };
    use crate::player::{
        GAIT_PHASE_L, GAIT_PHASE_R, LIMB_PLAN, PART_ARM, PART_LEG, Player, RUN_SPEED, WALK_SPEED,
        facing_yaw, joint_pivot, limb_chain_matrix, limb_parent, limb_swing,
    };
    use crate::r#type::{Mat4Data, Vec2, Vec3};

    /// 把断言文案里的 `{名字}` 占位符替换成实际数值(与 `camera.rs` 同形)。
    fn fill(template: &str, args: &[(&str, &str)]) -> String {
        let mut out: String = template.to_string();
        let mut index: usize = 0;
        while index < args.len() {
            out = out.replace(&format!("{{{}}}", args[index].0), args[index].1);
            index += 1;
        }
        out
    }

    /// 把列主序 `Mat4` 作用在一个点上。
    fn apply(m: &Mat4, v: Vec3) -> Vec3 {
        let e: &Mat4Data = m.get_elements();
        [0, 1, 2].map(|r: usize| e[r] * v[0] + e[4 + r] * v[1] + e[8 + r] * v[2] + e[12 + r])
    }

    /// 朝向角必须让资产的正面(`+Z`)朝着速度方向 —— 这是玩家「前进时侧身」
    /// 的回归测试。`yaw = 0` 面向 `+Z`,所以朝 `+Z` 走必须是 0、朝 `+X`
    /// 走必须是 `+π/2`。旧的 `-vz.atan2(vx)` 恰好把这两条对调了 90°。
    #[test]
    fn facing_yaw_points_the_model_along_the_velocity() {
        for (velocity, want) in [
            (Vec2::from([0.0, 1.0]), 0.0),
            (Vec2::from([1.0, 0.0]), std::f32::consts::FRAC_PI_2),
            (Vec2::from([0.0, -1.0]), std::f32::consts::PI),
            (Vec2::from([-1.0, 0.0]), -std::f32::consts::FRAC_PI_2),
        ] {
            let yaw: f32 = facing_yaw(velocity);
            assert!(
                wrap(yaw - want).abs() < 1.0e-4,
                "{T_FACING_MATCHES_VELOCITY}: 速度 {velocity:?} 应得 yaw={want},实得 {yaw}"
            );
            // 再用渲染矩阵复核一次:本地 +Z 落到世界后必须与速度同向。
            let forward: Vec3 = apply(&Mat4::rotation_y(yaw), [0.0, 0.0, 1.0]);
            let len: f32 = (forward[0] * forward[0] + forward[2] * forward[2]).sqrt();
            assert!(
                (forward[0] / len - velocity[0]).abs() < 1.0e-4
                    && (forward[2] / len - velocity[1]).abs() < 1.0e-4,
                "{}",
                T_FACING_MATCHES_VELOCITY
            );
        }
    }

    /// 把角度折到 (−π, π],避免断言时被 ±2π 干扰。
    fn wrap(angle: f32) -> f32 {
        let two_pi: f32 = 2.0 * std::f32::consts::PI;
        let mut value: f32 = angle % two_pi;
        if value > std::f32::consts::PI {
            value -= two_pi;
        } else if value < -std::f32::consts::PI {
            value += two_pi;
        }
        value
    }

    /// 每一节 limb 的顶点都必须停在资产给出的**绝对高度**上。
    ///
    /// 这是「角色渲染成 13 摞碎片」的直接回归测试。资产把
    /// `upper_arm_L` 的顶点放在角色空间 y = 1.095..1.431(肩到肘),
    /// `joint_pivot` 从包围盒推出 `[0.156, 1.431, 0.0]` —— 枢轴的 y
    /// 就是肩高,顶点本来已经在那个高度上。矩阵若再乘一次
    /// `T(joint_pivot)`,整节手臂会平移到 y = 2.53..2.87,举到头顶
    /// 上方 0.8 m,和躯干之间留下一整段空隙。
    ///
    /// 断言用「摆角为 0」的最简情形:此时矩阵退化成纯位姿,变换后的
    /// 包围盒必须和资产包围盒**逐轴相等**(平移 origin / 旋转 yaw 除外),
    /// 即 `Ry(yaw)·(I)` 不改变局部高度。
    #[test]
    fn limb_vertices_stay_at_their_asset_height() {
        // 实测自 `ped_suit` 资产的 `__VCW_DEBUG__.limbExtents`。
        let cases: [(&str, Vec3, Vec3, Vec3); 4] = [
            (
                PART_UPPER_ARM_L,
                [0.156, 1.095, -0.062],
                [0.302, 1.431, 0.062],
                [0.156, 1.431, 0.0],
            ),
            (
                PART_LOWER_ARM_L,
                [0.202, 0.769, -0.050],
                [0.312, 1.103, 0.050],
                [0.202, 1.103, 0.0],
            ),
            (
                PART_UPPER_LEG_L,
                [0.02, 0.946, -0.06],
                [0.21, 1.43, 0.06],
                [0.02, 1.43, 0.0],
            ),
            (
                PART_SHOE_L,
                [0.05, 0.098, -0.05],
                [0.15, 0.25, 0.05],
                [0.10, 0.25, 0.0],
            ),
        ];
        for (part, lo, hi, pivot) in cases {
            // 摆角为 0:矩阵应当只剩「绕原点平移 + 朝向」,不动局部高度。
            let model: Mat4 = limb_chain_matrix([0.0, 0.0, 0.0], 0.0, pivot, pivot, 0.0, 0.0);
            let want_lo: Vec3 = apply(&model, lo);
            let want_hi: Vec3 = apply(&model, hi);
            assert!(
                (want_lo[1] - lo[1]).abs() < 1.0e-4 && (want_hi[1] - hi[1]).abs() < 1.0e-4,
                "{}",
                fill(
                    T_LIMB_STAYS_AT_ASSET_HEIGHT,
                    &[
                        (KEY_PART, part),
                        ("off", &format!("{:.3}", (want_lo[1] - lo[1]).abs())),
                        ("got", &format!("{:.2}", want_lo[1])),
                        ("got_hi", &format!("{:.2}", want_hi[1])),
                        ("want", &format!("{:.2}", lo[1])),
                        ("want_hi", &format!("{:.2}", hi[1])),
                    ]
                )
            );
        }
    }

    /// 父子串联的闭合性:小臂的旋转必须作用在上臂**已经转过**的坐标系上。
    ///
    /// 这条是 ab0610d 引入的原始意图,保留下来防止修高度时把串联也一起
    /// 抹掉(那样又回到「大腿摆动时小腿原地不动」)。
    #[test]
    fn distal_limb_inherits_the_parents_swing() {
        let origin: Vec3 = [0.0, 0.0, 0.0];
        let shoulder: Vec3 = [0.156, 1.431, 0.0];
        let elbow: Vec3 = [0.202, 1.103, 0.0];
        let root_swing: f32 = 0.5;
        let joint_swing: f32 = 0.0;
        // 肘部世界位置:应当只受肩关节影响(小臂自身零摆角)。
        let direct: Vec3 = apply(
            &limb_chain_matrix(origin, 0.0, shoulder, elbow, root_swing, joint_swing),
            elbow,
        );
        // 手算的串联值:先绕肩转肩到肘的向量。
        let offset: Vec3 = [
            elbow[0] - shoulder[0],
            elbow[1] - shoulder[1],
            elbow[2] - shoulder[2],
        ];
        let (sine, cosine): (f32, f32) = root_swing.sin_cos();
        // `Mat4::rotation_x` 是列主序,其第 2、3 行给出
        // `y' = y·cos - z·sin`、`z' = y·sin + z·cos`(手算要照这个约定)。
        let hand: Vec3 = [
            shoulder[0] + offset[0],
            shoulder[1] + offset[1] * cosine - offset[2] * sine,
            shoulder[2] + offset[1] * sine + offset[2] * cosine,
        ];
        assert!(
            (direct[0] - hand[0]).abs() < 1.0e-4
                && (direct[1] - hand[1]).abs() < 1.0e-4
                && (direct[2] - hand[2]).abs() < 1.0e-4,
            "{}",
            fill(
                T_DISTAL_INHERITS_PARENT_SWING,
                &[
                    ("got", &format!("{direct:?}")),
                    ("want", &format!("{hand:?}"))
                ]
            )
        );
    }

    /// 左右必须反相:两条腿同时朝前迈步就是「四肢不协调」。
    #[test]
    fn left_and_right_legs_swing_in_opposite_phase() {
        let phase: f32 = 1.1;
        let left: f32 = limb_swing(PART_UPPER_LEG_L, phase, 1.0);
        let right: f32 = limb_swing(crate::r#const::PART_UPPER_LEG_R, phase, 1.0);
        assert!(
            left * right < 0.0,
            "{T_LEGS_ANTIPHASE}: 左={left} 右={right}"
        );
        assert!(
            (left + right).abs() < 1.0e-5,
            "{T_LEGS_ANTIPHASE}: 摆角应互为相反数,实得 {left} / {right}"
        );
        let phase_gap: f32 = (GAIT_PHASE_R - GAIT_PHASE_L).abs();
        assert!(
            (phase_gap - std::f32::consts::PI).abs() < 1.0e-6,
            "{T_LEGS_ANTIPHASE}: 左右相位必须差 π,实得 {phase_gap}"
        );
    }

    /// 行人起伏必须与落脚**同相**:每一步落地身体都抬到最高点。
    ///
    /// 落脚时刻 = 腿摆角为 0 = `phase ∈ {0, π}`(左右脚交替,每步一次)。
    /// 身体在那两个时刻都应该最高,所以起伏的峰值必须**两个**都落在那里。
    /// 原来的 `sin(phase * 2.0)` 把波峰推到 `pi/4` 与 `5pi/4` —— 脚还
    /// 悬在摆动中段 —— 身体反而最高;脚一落地身体过零。每个高峰都错开
    /// 四分之一个步周期,叠起来就是弹簧式弹跳(用户报的「NPC 蹦蹦跳跳」)。
    #[test]
    fn ped_bob_peaks_when_the_foot_plants() {
        let footfalls: [f32; 2] = [GAIT_PHASE_L, GAIT_PHASE_R];
        for phase in footfalls {
            // 此刻(脚正落地)身体必须是最高点。
            let at_plant: f32 = (phase * 2.0).cos() * PED_BOB_HEIGHT;
            assert!(
                (at_plant - PED_BOB_HEIGHT).abs() < 1.0e-5,
                "{}",
                fill(
                    T_PED_BOB_OUT_OF_PHASE,
                    &[
                        ("deg:.1", &format!("{:.1}", phase.to_degrees())),
                        ("bob:.5", &format!("{at_plant:.5}")),
                        ("cos:.5", &format!("{:.5}", (phase * 2.0).cos())),
                    ]
                )
            );
            // 摆动中段(脚悬空)身体必须压到最低 —— 不能反过来。
            let mid: f32 = phase + GAIT_PHASE_R / 2.0;
            let at_mid: f32 = (mid * 2.0).cos() * PED_BOB_HEIGHT;
            assert!(
                (at_mid + PED_BOB_HEIGHT).abs() < 1.0e-5,
                "{}",
                fill(
                    T_PED_BOB_OUT_OF_PHASE,
                    &[
                        ("deg:.1", &format!("{:.1}", mid.to_degrees())),
                        ("bob:.5", &format!("{at_mid:.5}")),
                        ("cos:.5", &format!("{:.5}", (mid * 2.0).cos())),
                    ]
                )
            );
        }
    }

    /// 起伏必须是**每步一次**两个高峰,且两个高峰等高。
    ///
    /// 扫一个完整步周期数局部极大值:落脚在 `0` 与 `pi`,所以恰好 2 个。
    /// 这条同时挡住 `cos(phi)`(只有 1 个峰)和 `sin(2·phi)`(2 个峰但
    /// 位置错开四分之一个周期)两种退化。
    #[test]
    fn ped_bob_has_one_peak_per_footfall() {
        // 只扫 `[0, TAU)` —— 闭区间会把末端 `TAU` 也算成一个峰,而那个
        // 峰其实就是 `t = 0` 的周期性重复,数出来就多一个。
        //
        // 数波峰用**斜率过零**(由正转负)而不是「值超过阈值」:阈值法在
        // 峰顶附近有一整片样本都达标,会数出十几个「峰」。过零法每个
        // 真峰只触发一次。
        let steps: usize = 1440;
        let mut peaks: Vec<f32> = Vec::new();
        let bob_at: fn(f32) -> f32 = |t: f32| (t * 2.0).cos() * PED_BOB_HEIGHT;
        let mut rising: bool = true;
        let indices: Vec<usize> = (0..steps).collect();
        for i in indices {
            let t: f32 = i as f32 / steps as f32 * 2.0 * GAIT_PHASE_R;
            let slope: f32 = bob_at(t + 0.001) - bob_at(t);
            if rising && slope < 0.0 {
                peaks.push(t);
                rising = false;
            } else if !rising && slope > 0.0 {
                rising = true;
            }
        }
        assert_eq!(
            peaks.len(),
            2,
            "{}",
            fill(
                T_PED_BOB_DOUBLE_FREQUENCY,
                &[("peaks", &format!("{}", peaks.len()))]
            )
        );
        // 两个峰必须落在两次落脚相位上(0 与 π),且到顶。
        for (index, peak) in peaks.iter().enumerate() {
            let want: f32 = index as f32 * GAIT_PHASE_R;
            let gap: f32 = (peak - want).abs();
            assert!(
                gap < 0.01,
                "{}",
                fill(
                    T_PED_BOB_OUT_OF_PHASE,
                    &[
                        ("deg:.1", &format!("{:.1}", peak.to_degrees())),
                        ("bob:.5", &format!("{:.5}", bob_at(*peak))),
                        ("cos:.5", &format!("{:.5}", (peak * 2.0).cos())),
                    ]
                )
            );
            assert!(
                (bob_at(*peak) - PED_BOB_HEIGHT).abs() < 1.0e-3,
                "{}",
                fill(
                    T_PED_BOB_OUT_OF_PHASE,
                    &[
                        ("deg:.1", &format!("{:.1}", peak.to_degrees())),
                        ("bob:.5", &format!("{:.5}", bob_at(*peak))),
                        ("cos:.5", &format!("{:.5}", (peak * 2.0).cos())),
                    ]
                )
            );
        }
    }

    /// 同一侧的大腿 / 小腿必须同相位(否则就是抽搐而不是摆腿)。
    #[test]
    fn thigh_and_shin_on_one_side_share_a_phase() {
        let phase: f32 = 0.7;
        let thigh: f32 = limb_swing(PART_UPPER_LEG_L, phase, 1.0);
        let shin: f32 = limb_swing(PART_LOWER_LEG_L, phase, 1.0);
        assert!(
            thigh * shin > 0.0,
            "{T_SAME_SIDE_IN_PHASE}: 大腿 {thigh} 小腿 {shin}"
        );
    }

    /// 停下(`gait_amount = 0`)时所有关节必须归零。
    #[test]
    fn every_limb_relaxes_to_zero_when_the_player_stops() {
        for (part, _, _, _) in LIMB_PLAN {
            assert!(
                limb_swing(part, 2.3, 0.0).abs() < 1.0e-9,
                "{T_LIMBS_RELAX_TO_ZERO}: {part}"
            );
        }
    }

    /// 鞋必须挂在**小腿**上,并且真的跟着腿走。
    ///
    /// 这是「鞋子不跟随腿部」的回归测试。缺陷成因是 `LIMB_PLAN` 只登记了
    /// 8 节肢体(上臂 / 小臂 ×2、大腿 / 小腿 ×2),`shoe_L` / `shoe_R` 根本
    /// 不在表里 —— `limb_swing` 对它们返回 `0.0`,矩阵退化成纯位姿,
    /// 于是腿绕髋转起来时脚留在原地。
    ///
    /// 断言分两层,少任何一层都会漏掉一种坏法:
    ///
    /// 1. **骨架表**:鞋有父 part,且父 part 是**小腿**(不是躯干、不是大腿)。
    ///    父节点指错的话鞋会跟着躯干走,同样表现为「脚跟不上腿」。
    /// 2. **变换结果**:把整条腿的链矩阵跑一遍,取鞋盒里一个离踝关节最远
    ///    的角点(脚尖),断言它**离开**了静止姿态的位置。只断言 `swing`
    ///    的符号是不够的 —— 摆角为 0 但链串对了的情形同样会让脚跟随,
    ///    所以这里判的是世界坐标,不是角度。
    ///
    /// 静止姿态用摆角全 0 复现(和渲染端 `gait_amount == 0` 一致)。
    #[test]
    fn shoes_follow_the_shin_through_the_limb_chain() {
        // --- 1. 骨架表层 ---
        assert_eq!(
            limb_parent(PART_SHOE_L),
            PART_LOWER_LEG_L,
            "{T_SHOE_FOLLOWS_SHIN}: 左鞋必须挂在左小腿上,实得父 {}",
            limb_parent(PART_SHOE_L)
        );
        assert_eq!(
            limb_parent(PART_SHOE_R),
            crate::r#const::PART_LOWER_LEG_R,
            "{T_SHOE_FOLLOWS_SHIN}: 右鞋必须挂在右小腿上,实得父 {}",
            limb_parent(PART_SHOE_R)
        );

        // --- 2. 变换层:脚尖必须随小腿摆动 ---
        // 枢轴不写死:和渲染端一样从**资产包围盒**推出(`joint_pivot`),
        // 所以换模型时这条断言跟着变,不会钉死一份过期魔法坐标。
        // 实测包围盒(`www/assets/ped_suit.json` 的 positions):
        //   lower_leg_L  x 0.0304..0.1916, y 0.0976..0.4856, z -0.0806..0.0806
        //   shoe_L       x 0.0630..0.1650, y 0.0000..0.1120, z  0.1260..0.3780
        let shin_bounds: (Vec3, Vec3) = ([0.0304, 0.0976, -0.0806], [0.1916, 0.4856, 0.0806]);
        let shoe_bounds: (Vec3, Vec3) = ([0.0630, 0.0000, 0.1260], [0.1650, 0.1120, 0.3780]);
        // 渲染端(`game.rs:7962-7967`):父 part 有值时 `root_pivot` 取**父**
        // 的枢轴,`pivot` 取自己的;`root_swing` 是**父**的摆角。这里逐字
        // 复刻鞋的那一次调用。
        let root_pivot: Vec3 = joint_pivot(PART_LOWER_LEG_L, shin_bounds);
        let own_pivot: Vec3 = joint_pivot(PART_SHOE_L, shoe_bounds);
        let toe: Vec3 = [0.063, 0.0, 0.378];

        let shin_swing: f32 = limb_swing(PART_LOWER_LEG_L, 0.9, 1.0);
        let shoe_swing: f32 = limb_swing(PART_SHOE_L, 0.9, 1.0);
        assert!(
            shin_swing.abs() > 1.0e-3,
            "{T_SHOE_FOLLOWS_SHIN}: 选的相位上小腿必须在摆(膝 {shin_swing}),否则这条断言是空的"
        );

        // 摆角为 0 = 站立姿态(与渲染端 `gait_amount == 0` 一致)。
        let rest: Mat4 = limb_chain_matrix([0.0, 0.0, 0.0], 0.0, root_pivot, own_pivot, 0.0, 0.0);
        let posed: Mat4 = limb_chain_matrix(
            [0.0, 0.0, 0.0],
            0.0,
            root_pivot,
            own_pivot,
            shin_swing,
            shoe_swing,
        );

        let rest_toe: Vec3 = apply(&rest, toe);
        let posed_toe: Vec3 = apply(&posed, toe);
        let moved: f32 = ((posed_toe[0] - rest_toe[0]).powi(2)
            + (posed_toe[1] - rest_toe[1]).powi(2)
            + (posed_toe[2] - rest_toe[2]).powi(2))
        .sqrt();
        assert!(
            moved > 0.02,
            "{}{}",
            fill(
                T_SHOE_FOLLOWS_SHIN,
                &[
                    (KEY_PART, PART_SHOE_L),
                    ("moved", &format!("{moved:.4}")),
                    ("hip", &format!("{shin_swing:.3}")),
                    ("knee", &format!("{shoe_swing:.3}")),
                ]
            ),
            " —— 脚尖必须离开静止位置(它挂在小腿上)"
        );

        // **独立第二条路径**:手算 `T(knee)·Rx(knee_a)·T(-knee)` 作用在脚尖上
        // 的结果,必须与 `limb_chain_matrix` 逐分量相等。
        //
        // 只有「动了」不够:一个把鞋挂到**躯干**上的实现同样会让脚尖移动。
        // 手算值把枢轴写死在测试里,所以父节点挂错时两条路径必然分叉。
        // `FOOT_SWING = 0` ⇒ 鞋自身不转,于是整条链退化成绕膝的一次旋转。
        let offset: Vec3 = [
            toe[0] - root_pivot[0],
            toe[1] - root_pivot[1],
            toe[2] - root_pivot[2],
        ];
        let (sine, cosine): (f32, f32) = shin_swing.sin_cos();
        // `Mat4::rotation_x` 列主序 ⇒ `y' = y·cos - z·sin`、`z' = y·sin + z·cos`
        let hand: Vec3 = [
            root_pivot[0] + offset[0],
            root_pivot[1] + offset[1] * cosine - offset[2] * sine,
            root_pivot[2] + offset[1] * sine + offset[2] * cosine,
        ];
        assert!(
            (posed_toe[0] - hand[0]).abs() < 1.0e-4
                && (posed_toe[1] - hand[1]).abs() < 1.0e-4
                && (posed_toe[2] - hand[2]).abs() < 1.0e-4,
            "{T_SHOE_FOLLOWS_SHIN}: 脚尖必须绕**膝**摆(父 = 小腿),实得 {posed_toe:?} 应为 {hand:?}"
        );

        // 左右两条腿的鞋必须反相 —— 脚同相就是「两条腿一起蹦」。
        //
        // 判**相位偏移**而不是判摆角乘积:`FOOT_SWING = 0` 时两个摆角都
        // 恒为 0,乘积永远是 0,拿它当断言等于断言 `0 < 0`(恒假)。
        // 左右反相是骨架表的**相位列**属性,与幅度无关。
        //
        // 每只鞋的相位必须**等于它那条腿**的相位 —— 挂错腿时两者会分叉。
        for (shoe, leg) in [
            (PART_SHOE_L, PART_UPPER_LEG_L),
            (PART_SHOE_R, crate::r#const::PART_UPPER_LEG_R),
        ] {
            let mut shoe_phase: Option<f32> = None;
            let mut leg_phase: Option<f32> = None;
            for (part, parent, phase, _) in LIMB_PLAN {
                if *part == shoe {
                    shoe_phase = Some(*phase);
                    assert_eq!(
                        *parent,
                        if shoe == PART_SHOE_L {
                            PART_LOWER_LEG_L
                        } else {
                            crate::r#const::PART_LOWER_LEG_R
                        },
                        "{T_SHOE_FOLLOWS_SHIN}: {shoe} 必须挂在对应的小腿上"
                    );
                }
                if *part == leg {
                    leg_phase = Some(*phase);
                }
            }
            assert!(
                shoe_phase.is_some(),
                "{T_SHOE_FOLLOWS_SHIN}: {shoe} 没有登记进 LIMB_PLAN(它将永远不随腿动)"
            );
            assert_eq!(
                shoe_phase, leg_phase,
                "{T_SHOE_FOLLOWS_SHIN}: {shoe} 的相位必须与 {leg} 一致,否则脚会跟错腿"
            );
        }

        // 左右相位差必须正好是 π。上面那轮循环已经取到了左右鞋的相位,
        // 这里直接比较二者的差,不再重新查表。
        let left_phase: f32 = LIMB_PLAN
            .iter()
            .find_map(|(part, _, phase, _): &(&str, &str, f32, f32)| {
                (*part == PART_SHOE_L).then_some(*phase)
            })
            .unwrap_or(0.0);
        let right_phase: f32 = LIMB_PLAN
            .iter()
            .find_map(|(part, _, phase, _): &(&str, &str, f32, f32)| {
                (*part == PART_SHOE_R).then_some(*phase)
            })
            .unwrap_or(0.0);
        let gap: f32 = (right_phase - left_phase).abs();
        assert!(
            (gap - GAIT_PHASE_R).abs() < 1.0e-4,
            "{T_SHOE_FOLLOWS_SHIN}: 左右脚相位差必须等于 π(反相),实得 {gap}"
        );
    }

    /// 小腿 / 小臂必须挂在父 part 上 —— 没有父关节的串联,腿会断成两截。
    #[test]
    fn distal_limbs_declare_their_parent_joint() {
        assert_eq!(limb_parent(PART_LOWER_LEG_L), PART_UPPER_LEG_L);
        assert!(
            limb_parent(PART_UPPER_LEG_L).is_empty(),
            "{T_DISTAL_LIMB_HAS_PARENT}: 大腿是链根,没有父关节"
        );
        // 每一项要么是链根,要么其父 part 也在计划表里(不能指向表外)。
        for (part, parent, _, _) in LIMB_PLAN {
            if parent.is_empty() {
                continue;
            }
            let known: bool = LIMB_PLAN
                .iter()
                .any(|(name, _, _, _): &(&str, &str, f32, f32)| name == parent);
            assert!(known, "{T_PARENT_IN_PLAN}: {part} 的父 {parent}");
        }
        // 手臂 / 腿两条链都必须是「上段有父、下段有父」的结构。
        let chained: usize = LIMB_PLAN
            .iter()
            .filter(|entry: &&(&str, &str, f32, f32)| {
                let (part, _, _, _): (&str, &str, f32, f32) = *(*entry);
                part.contains(PART_ARM) || part.contains(PART_LEG)
            })
            .count();
        assert_eq!(chained, 8, "{T_DISTAL_LIMB_HAS_PARENT}");
    }

    /// 让一个 `Player` 在空地上按某组键走够时间,返回稳定后的速度模长。
    ///
    /// 走的是真实的 [`Player::step`] —— `CollisionWorld::new()` 是空世界,
    /// 所以 `|v|` 就是目标速度,碰撞分支不参与。**跑够 300 帧**是必须的:
    /// `SPEED_RAMP` 的指数逼近需要时间,少跑几帧读到的是半路上的值,
    /// 那样任何实现都测不出斜向差别。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - `intent`(侧向, 前向)。
    /// - `f32` - 速度上限(米/秒)。
    ///
    /// # Returns
    ///
    /// - `f32` - 稳定后的速度模长(米/秒)。
    fn settle_speed(intent: Vec2, speed: f32) -> f32 {
        let world: CollisionWorld = CollisionWorld::new();
        let mut player: Player = Player::new([0.0, 0.0, 0.0], 0.0);
        let dt: f32 = 1.0 / 60.0;
        for _ in 0..300 {
            player.step(intent, [1.0, 0.0], dt, speed, &world);
        }
        let v: Vec2 = player.get_velocity();
        (v[0] * v[0] + v[1] * v[1]).sqrt()
    }

    /// 斜向移动不得比直线快 —— 参照物是 GTA V(那边按 W+D 不会更快)。
    ///
    /// 回归测试(修复前实测失败):`Player::step` 把 `direction * forward +
    /// perp * strafe` 直接乘 `speed`,两轴各顶到 `speed` 时合速度是
    /// `speed * sqrt(2)`。实测步行 `W+D` = 6.5052(`4.6 * √2`),`Shift+W+D`
    /// = 11.8787(`8.4 * √2`),与直线 W 的 4.5998 / 8.3997 相比分别快
    /// 41.4% —— 而紧挨着这段代码的注释写的是「斜向移动不会比直线快」。
    ///
    /// **判据用比值 + 区间容差,不用浮点等值。** 归一化后 `(1, 1)` 缩到
    /// `(0.7071, 0.7071)`,余量约 1e-6;容差取
    /// [`DIAGONAL_SPEED_TOLERANCE`] = 1e-3,又远低于修复前的 1.4142。
    #[test]
    fn diagonal_movement_is_not_faster_than_straight() {
        for (mode, speed) in [(MODE_WALK, WALK_SPEED), (MODE_SPRINT, RUN_SPEED)] {
            let straight: f32 = settle_speed([0.0, 1.0], speed);
            for diagonal in [[1.0f32, 1.0f32], [-1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]] {
                let got: f32 = settle_speed(diagonal, speed);
                let ratio: f32 = got / straight;
                assert!(
                    (ratio - 1.0).abs() <= DIAGONAL_SPEED_TOLERANCE,
                    "{}",
                    fill(
                        T_DIAGONAL_MATCHES_STRAIGHT,
                        &[
                            (KEY_MODE, mode),
                            (
                                "diagonal",
                                &format!("W{:+}", if diagonal[0] > 0.0 { "D" } else { "A" })
                            ),
                            ("got:.4", &format!("{got:.4}")),
                            (KEY_STRAIGHT, "W"),
                            ("want:.4", &format!("{straight:.4}")),
                            ("ratio:.4", &format!("{ratio:.4}")),
                        ]
                    )
                );
            }
        }
    }

    /// 归一化只该压斜向,不该动直线 —— `W` 仍然必须是满速。
    ///
    /// 上一条只钉住「斜向不更快」,一个把速度**整体**砍到 `speed / √2` 的
    /// 修法也能让它变绿,而那是把走路和冲刺都拖慢了。所以这里独立地钉住
    /// 直线档位:`|W|` 落在 `speed` 的容差内,`Shift+W` 同理。
    #[test]
    fn straight_movement_keeps_the_full_speed() {
        for (mode, speed) in [(MODE_WALK, WALK_SPEED), (MODE_SPRINT, RUN_SPEED)] {
            let straight: f32 = settle_speed([0.0, 1.0], speed);
            let error: f32 = (straight - speed).abs() / speed;
            assert!(
                error <= DIAGONAL_SPEED_TOLERANCE,
                "{}",
                fill(
                    T_STRAIGHT_KEEPS_FULL_SPEED,
                    &[
                        (KEY_MODE, mode),
                        ("got:.4", &format!("{straight:.4}")),
                        ("want:.4", &format!("{speed:.4}")),
                    ]
                )
            );
        }
    }

    /// 半个身位(模拟摇杆推一半)必须仍然是半速,而不是被归一化成满速。
    ///
    /// 归一化最常见的实现错误就是**无脑除以模长**:`(0.5, 0.5)` 的模长是
    /// 0.707,除完变成 `(0.707, 0.707)`,半速输入被放大成满速。正确写法是
    /// **只把超过 1 的模长钳下来**。这条守着那个区别 —— `Player::step` 的
    /// `intent` 由 `axis()` 产生,每分量在 −1..1,0.5 正是真实会出现的值。
    #[test]
    fn a_half_strength_input_stays_at_half_speed() {
        for (mode, speed) in [(MODE_WALK, WALK_SPEED), (MODE_SPRINT, RUN_SPEED)] {
            let half: f32 = settle_speed([0.0, 0.5], speed);
            let full: f32 = settle_speed([0.0, 1.0], speed);
            let ratio: f32 = half / full;
            assert!(
                (ratio - 0.5).abs() <= DIAGONAL_SPEED_TOLERANCE,
                "{}",
                fill(
                    T_HALF_INPUT_STAYS_HALF_SPEED,
                    &[
                        (KEY_MODE, mode),
                        ("got:.4", &format!("{half:.4}")),
                        ("want:.4", &format!("{:.4}", full * 0.5)),
                        ("ratio:.4", &format!("{ratio:.4}")),
                    ]
                )
            );
        }
    }
}
