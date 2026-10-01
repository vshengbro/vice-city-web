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
];

/// 查某个 part 的父 part 名。
///
/// # Arguments
///
/// - `&str` - part 名。
///
/// # Returns
///
/// - `&str` - 父 part 名;链根(或不在计划表里)返回空串。
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
        // 目标速度 = 前向分量 + 侧向分量,两者各自受 `speed` 上限约束,
        // 所以斜向移动不会比直线快(和真实操作手感一致)。
        let forward: f32 = intent[1];
        let strafe: f32 = intent[0];
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
    let placement: Mat4 = Mat4::translation(origin).multiply(&Mat4::rotation_y(yaw));
    let root: Mat4 = Mat4::translation(root_pivot).multiply(&Mat4::rotation_x(root_swing));
    // 末端关节:先在「已经随根关节转过」的坐标系里定位,再绕自身枢轴摆。
    let child: Mat4 = root
        .multiply(&Mat4::translation(joint_pivot))
        .multiply(&Mat4::rotation_x(joint_swing))
        .multiply(&Mat4::translation([
            -joint_pivot[0],
            -joint_pivot[1],
            -joint_pivot[2],
        ]));
    placement.multiply(&child)
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
    use crate::r#const::{PART_LOWER_LEG_L, PART_UPPER_LEG_L};
    use crate::player::{
        GAIT_PHASE_L, GAIT_PHASE_R, LIMB_PLAN, PART_ARM, PART_LEG, facing_yaw, limb_parent,
        limb_swing,
    };
    use crate::r#type::{Mat4Data, Vec2, Vec3};

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
                "速度 {velocity:?} 应得到 yaw={want},实得 {yaw}"
            );
            // 再用渲染矩阵复核一次:本地 +Z 落到世界后必须与速度同向。
            let forward: Vec3 = apply(&Mat4::rotation_y(yaw), [0.0, 0.0, 1.0]);
            let len: f32 = (forward[0] * forward[0] + forward[2] * forward[2]).sqrt();
            assert!(
                (forward[0] / len - velocity[0]).abs() < 1.0e-4
                    && (forward[2] / len - velocity[1]).abs() < 1.0e-4,
                "yaw={yaw} 下资产正面朝 {forward:?},与速度 {velocity:?} 不符"
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

    /// 左右必须反相:两条腿同时朝前迈步就是「四肢不协调」。
    #[test]
    fn left_and_right_legs_swing_in_opposite_phase() {
        let phase: f32 = 1.1;
        let left: f32 = limb_swing(PART_UPPER_LEG_L, phase, 1.0);
        let right: f32 = limb_swing(crate::r#const::PART_UPPER_LEG_R, phase, 1.0);
        assert!(
            left * right < 0.0,
            "左右大腿必须反相(左={left} 右={right}),同相就是齐步走"
        );
        assert!(
            (left + right).abs() < 1.0e-5,
            "左右大腿摆角应互为相反数,实得 {left} / {right}"
        );
        let phase_gap: f32 = (GAIT_PHASE_R - GAIT_PHASE_L).abs();
        assert!(
            (phase_gap - std::f32::consts::PI).abs() < 1.0e-6,
            "左右相位必须差 π,实得 {phase_gap}"
        );
    }

    /// 同一侧的大腿 / 小腿必须同相位(否则就是抽搐而不是摆腿)。
    #[test]
    fn thigh_and_shin_on_one_side_share_a_phase() {
        let phase: f32 = 0.7;
        let thigh: f32 = limb_swing(PART_UPPER_LEG_L, phase, 1.0);
        let shin: f32 = limb_swing(PART_LOWER_LEG_L, phase, 1.0);
        assert!(
            thigh * shin > 0.0,
            "同侧大腿({thigh})与小腿({shin})必须同相"
        );
    }

    /// 停下(`gait_amount = 0`)时所有关节必须归零。
    #[test]
    fn every_limb_relaxes_to_zero_when_the_player_stops() {
        for (part, _, _, _) in LIMB_PLAN {
            assert!(
                limb_swing(part, 2.3, 0.0).abs() < 1.0e-9,
                "{part} 在停下时摆角必须为 0"
            );
        }
    }

    /// 小腿 / 小臂必须挂在父 part 上 —— 没有父关节的串联,腿会断成两截。
    #[test]
    fn distal_limbs_declare_their_parent_joint() {
        assert_eq!(limb_parent(PART_LOWER_LEG_L), PART_UPPER_LEG_L);
        assert!(
            limb_parent(PART_UPPER_LEG_L).is_empty(),
            "大腿是链根,没有父关节"
        );
        // 每一项要么是链根,要么其父 part 也在计划表里(不能指向表外)。
        for (part, parent, _, _) in LIMB_PLAN {
            if parent.is_empty() {
                continue;
            }
            let known: bool = LIMB_PLAN
                .iter()
                .any(|(name, _, _, _): &(&str, &str, f32, f32)| name == parent);
            assert!(known, "{part} 的父 part {parent} 不在 LIMB_PLAN 里");
        }
        // 手臂 / 腿两条链都必须是「上段有父、下段有父」的结构。
        let chained: usize = LIMB_PLAN
            .iter()
            .filter(|entry: &&(&str, &str, f32, f32)| {
                let (part, _, _, _): (&str, &str, f32, f32) = *(*entry);
                part.contains(PART_ARM) || part.contains(PART_LEG)
            })
            .count();
        assert_eq!(chained, 8, "四肢共 8 个 part 必须全部在计划表里");
    }
}
