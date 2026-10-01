//! 交通 AI + 拾取物:车道循环行驶的车队、可上下的车辆、地面拾取物。
//!
//! 车队**不是自由漫游**:每辆车被分配到一条固定车道(街道沿 Z 轴延伸,
//! 车道是一条沿 Z 的直线段),车辆只沿车道 Z 方向前进,到端点 wrap 回起点,
//! 因此永远不会拐进人行道、更不会穿楼。玩家上车后 WASD 直接驱动该车,
//! 车速上限提到街道限速(8–15 m/s)。
//!
//! 车灯:`car_*` 资产的 `frontlights` / `rearlights` part 带 `emissive`,
//! 展开时已经逐三角形写进顶点缓冲的第四个 vec3(见 `render::build_gpu_mesh`),
//! 夜间 `SceneLighting::emissive_gain` 拉到 1.85 + 泛光 pass,所以车灯
//! 是真的在发光,不是画一个假的亮点贴图。

use crate::{
    collision::CollisionWorld,
    r#const::*,
    player::{MAX_HEALTH, Player},
    r#type::{Vec2, Vec3},
};

/// 街道在 Z 轴上的可行驶半长(米)。
///
/// 由 [`crate::game::traffic_half`] 从城市半边长推导,车道两端到端点 wrap。
pub const LOOP_HALF_LENGTH: f32 = 120.0;
/// 车队巡航速度下限(米/秒)。
pub const SPEED_MIN: f32 = 8.0;
/// 车队巡航速度上限(米/秒)。
pub const SPEED_MAX: f32 = 15.0;
/// 玩家驾驶时的加速度(米/秒²)。
pub const DRIVE_ACCEL: f32 = 11.0;
/// 玩家驾驶时的刹车减速度(米/秒²)。
pub const DRIVE_BRAKE: f32 = 18.0;
// ---- 玩家驾驶的车辆模型:转向 + 漂移 ------------------------------
//
// 之前玩家驾驶只有油门:朝向是常量 `direction`��±1.0),位置被
// `set_position([lane_x, here[1], resolved[1]])` 钉死在车道 X 上。
// 后果是车**在物理上无法横向移动** —— 没有转向、没有漂移、也没有任何
// 横向速度可言,「开车」只是让摄像机跟着一个沿直线滑的盒子。
//
// 下面这一组常量支撑一个**最简 bicycle model**:车身有真实朝向角,
// 速度拆成「沿车身」和「垂直车身」两个分量,转向改变朝向并把一部分
// 速度甩向侧向;轮胎抓地力再把侧向分量按指数拉回 0。抓地力越小甩得
// 越多 = 漂移,漂移角随速度上升 —— 高速大转向才甩,低速几乎不甩,
// 与真实车辆一致。

/// 方向盘最大转速(弧度/秒)—— 满舵时的车身角速度上限。
pub const STEER_RATE: f32 = 1.9;
/// 车身朝向角速度随速度的下降系数(1/米):速度越高同样舵角转得越慢。
pub const STEER_SPEED_FALLOFF: f32 = 0.11;
/// 前轮最大偏角(弧度)—— 满舵时前轮相对车身偏多少。
pub const STEER_ANGLE_MAX: f32 = 0.52;
/// 侧向抓地衰减(1/秒):侧向速度每秒钟衰减到原来的 `1 - GRIP` 倍。
///
/// 抓地越小,转向甩出的侧向速度残留越久 = 滑得越远。这是「漂移」这一个
/// 旋钮:0 = 完全不滑(推头),大 = 甩尾。默认给一个普通路面抓地稍松的值,
/// 高速转向能明显滑出去,低速基本不滑。
pub const LATERAL_GRIP: f32 = 2.1;
/// 漂移时纵向速度的额外损失(1/秒):侧滑越厉害,加速越吃亏。
///
/// 真实车漂移会掉速。不加这一项的话,玩家可以侧滑着全速跑,漂移没有代价。
pub const DRIFT_SPEED_DRAG: f32 = 0.55;
/// 侧向速度超过这个比例(相对纵向)时开始明显掉速。
pub const DRIFT_SLIP_RATIO: f32 = 0.22;
/// 车轮在资产本地坐标里的四个安装位(XYZ 轮心),顺序为「左前 右前 左后 右后」。
///
/// `car_*` 资产的 `tyres` part 一次包含四个轮子,渲染时必须拆成四个
/// 实例各绕**自己的**轮心转,否则四个轮子会一起绕同一个点公转。
///
/// 数值由 `car_sedan.json` 的 `tyres` 顶点按 x/z 象限聚类后取质心量得
/// (左 x=-1.33 / 右 x=+1.32,前后各 z=±0.77,轮心 y=0.34);y 也正好
/// 等于 [`WHEEL_RADIUS`],与轮子 y∈[0, 0.68] 的实际跨度自洽。
pub const WHEEL_MOUNTS: [[f32; 3]; 4] = [
    [-1.32, 0.34, 0.77],
    [1.32, 0.34, 0.77],
    [-1.32, 0.34, -0.77],
    [1.32, 0.34, -0.77],
];
/// 车轮滚动半径(米)—— 车轮转角按 `行驶距离 / 半径` 积分。
///
/// 取 0.34 m 是 `car_*` 资产里 `tyres` 部件的实际半径量级(轮心 y≈0.38,
/// 轮胎外缘 y≈0.05),视觉上不会穿地也不会悬空。
pub const WHEEL_RADIUS: f32 = 0.34;
/// 车身朝向角速度的低速死区(米/秒):比这更慢时转向不生效。
///
/// 否则车在原地打方向也能转,看着像原地转圈。
pub const STEER_MIN_SPEED: f32 = 1.2;

/// 玩家走到拾取物多少米内就算拾到。
pub const PICKUP_RADIUS: f32 = 2.2;
/// 玩家走到车边多少米内可以按 F 上车。
/// 车队巡航速度(米/秒)——城市道路 8–15 m/s,取区间中段。
pub const CRUISE_SPEED: f32 = 11.5;

pub const ENTER_VEHICLE_RADIUS: f32 = 4.2;
/// 车辆「靠边等人」的距离(米)。
///
/// 真实城市里路边车会为招手的人减速。这一条让**行人真的追得上车**:
/// 8–14 m/s 的车在 4.2 m 内根本没法靠走路追上,没有这个规则第三人称
/// 的上车功能在玩法上就是废的(测试跑了 14 次都上不去)。车发现有人类
/// 站在路边,就把巡航速度降到 0 当临时出租车站。
pub const HAUL_RANGE: f32 = 26.0;
/// 「靠边等人」的减速强度(1/秒)。
pub const HAUL_BRAKE: f32 = 3.4;
/// `pickup_health_pack` 的回血量(点)。
pub const HEALTH_PACK_HEAL: f32 = 35.0;
/// `pickup_cash_stack` 的金额。
pub const CASH_STACK_AMOUNT: f32 = 250.0;
/// 拾取物绕 Y 轴的自转角速度(弧度/秒)。
pub const PICKUP_SPIN_RATE: f32 = 1.1;

/// 一辆参与交通仿真的车。
#[derive(Clone, Debug)]
pub struct TrafficCar {
    /// 资产 id。
    pub asset: &'static str,
    /// 当前世界坐标。
    position: Vec3,
    /// 沿车道前进的当前速度(米/秒,恒为正,倒车 = 换方向)。
    speed: f32,
    /// 巡航速度(米/秒)。
    cruise: f32,
    /// 行车的固定 X 坐标(米)—— 车道中心,玩家驾驶时也会被强制拉回。
    lane_x: f32,
    /// 行驶方向:`1.0` = 沿 +Z,`-1.0` = 沿 -Z。
    ///
    /// 只描述 AI 巡航状态。玩家驾驶时以 [`Self::yaw`] 为准。
    direction: f32,
    /// 车身朝向(弧度,绕 Y 轴)。AI 巡航时由 `direction` 推出,玩家驾驶
    /// 时由转向输入连续改变 —— 这是「车能转向」的载体。
    yaw: f32,
    /// 垂直车身的侧向速度(米/秒)。
    ///
    /// 转向把一部分速度甩到这个轴上,它按 [`LATERAL_GRIP`] 指数衰减回 0。
    /// 非零即代表车正在滑,也就是漂移。
    lateral: f32,
    /// 车轮累计转角(弧度)—— 渲染用,`距离 / 轮半径` 积分出来。
    wheel_spin: f32,
    /// 玩家是否正在驾驶这辆车。
    driven: bool,
}

impl TrafficCar {
    /// 当前世界坐标。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 当前世界坐标。
    pub fn get_position(&self) -> Vec3 {
        self.position
    }

    /// 写入当前世界坐标。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 新世界坐标。
    pub fn set_position(&mut self, value: Vec3) {
        self.position = value;
    }

    /// 沿车道前进的当前速度。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前速度(米/秒)。
    pub fn get_speed(&self) -> f32 {
        self.speed
    }

    /// 写入当前速度。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新速度(米/秒)。
    pub fn set_speed(&mut self, value: f32) {
        self.speed = value;
    }

    /// 行驶方向。
    ///
    /// # Returns
    ///
    /// - `f32` - `1.0` = 沿 +Z,`-1.0` = 沿 -Z。
    pub fn get_direction(&self) -> f32 {
        self.direction
    }

    /// 巡航速度。
    ///
    /// # Returns
    ///
    /// - `f32` - 巡航速度(米/秒)。
    pub fn get_cruise(&self) -> f32 {
        self.cruise
    }

    /// 行车的固定车道 X 坐标(米)。
    ///
    /// # Returns
    ///
    /// - `f32` - 车道中心 X(米)。
    pub fn get_lane_x(&self) -> f32 {
        self.lane_x
    }

    /// 玩家是否正在驾驶这辆车。
    ///
    /// # Returns
    ///
    /// - `bool` - 正在驾驶时为 `true`。
    pub fn get_driven(&self) -> bool {
        self.driven
    }

    /// 写入「玩家是否正在驾驶」。
    ///
    /// # Arguments
    ///
    /// - `bool` - 是否正在驾驶。
    pub fn set_driven(&mut self, value: bool) {
        self.driven = value;
    }

    /// 在指定车道上放一辆车。
    ///
    /// # Arguments
    ///
    /// - `&'static str` - 资产 id。
    /// - `f32` - 行车的固定 X 坐标(米)。
    /// - `f32` - 起始 Z 坐标(米)。
    /// - `f32` - 巡航速度(米/秒)。
    /// - `f32` - 行驶方向(`1.0` 或 `-1.0`)。
    ///
    /// # Returns
    ///
    /// - `Self` - 就绪的车辆状态。
    pub fn new(
        asset: &'static str,
        lane_x: f32,
        start_z: f32,
        cruise: f32,
        direction: f32,
    ) -> Self {
        let yaw: f32 = -direction * std::f32::consts::FRAC_PI_2;
        Self {
            asset,
            position: [lane_x, 0.0, start_z],
            speed: cruise,
            cruise,
            lane_x,
            direction,
            yaw,
            lateral: 0.0,
            wheel_spin: 0.0,
            driven: false,
        }
    }

    /// 车辆的朝向(绕 Y 轴弧度)。
    ///
    /// 资产的本地 +X 是车头方向(车灯在本地 -X,所以车头朝 -X,见
    /// `car_sedan.json` 里 `frontlights` 的 x 全部为负),而
    /// `Instance::new` 把本地 +X 映到世界 `(cos yaw, 0, -sin yaw)`,所以
    /// 沿 +Z 行驶的车 `yaw = -PI/2`,沿 -Z 行驶的车 `yaw = +PI/2`。
    ///
    /// # Returns
    ///
    /// - `f32` - 绕 Y 轴的朝向(弧度)。
    pub fn get_yaw(&self) -> f32 {
        self.yaw
    }

    /// 垂直车身的侧向速度(米/秒)—— 漂移量。
    ///
    /// # Returns
    ///
    /// - `f32` - 侧向速度,正 = 往车身右侧甩。
    pub fn get_lateral(&self) -> f32 {
        self.lateral
    }

    /// 写入侧向速度。
    ///
    /// # Arguments
    ///
    /// - `f32` - 侧向速度(米/秒)。
    pub fn set_lateral(&mut self, value: f32) {
        self.lateral = value;
    }

    /// 累加车轮转角。
    ///
    /// # Arguments
    ///
    /// - `f32` - 本步新增的转角(弧度)。
    pub fn advance_wheel_spin(&mut self, delta: f32) {
        let current: f32 = self.get_wheel_spin();
        self.set_wheel_spin(current + delta);
    }

    /// 车轮累计转角(弧度)—— 渲染时按这个角转轮子。
    ///
    /// # Returns
    ///
    /// - `f32` - 累计转角(弧度)。
    pub fn get_wheel_spin(&self) -> f32 {
        self.wheel_spin
    }

    /// 写入车轮累计转角。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新转角(弧度)。
    pub fn set_wheel_spin(&mut self, value: f32) {
        self.wheel_spin = value;
    }

    /// 写入车身朝向。
    ///
    /// # Arguments
    ///
    /// - `f32` - 绕 Y 轴的朝向(弧度)。
    pub fn set_yaw(&mut self, value: f32) {
        self.yaw = value;
    }

    /// 车队模式下推进一个固定步长:沿车道前进 + 到端点 wrap。
    ///
    /// 速度被平滑逼近巡航速度,玩家驾驶模式由 [`Self::drive`] 接管,
    /// 这里只做 wrap。
    ///
    /// # Arguments
    ///
    /// - `f32` - 固定步长(秒)。
    /// - `bool` - `true` 表示路边有玩家招手,这辆车减速停靠等客。
    pub fn step(&mut self, dt: f32, hailer: bool) {
        // 有行人在路边招手 → 当临时出租车站:松油门,直到停下等人。
        let target: f32 = if hailer { 0.0 } else { self.get_cruise() };
        let approach: f32 = if hailer {
            (HAUL_BRAKE * dt).min(1.0)
        } else {
            (DRIVE_ACCEL * dt / self.get_cruise().max(1.0)).min(1.0)
        };
        let cruise: f32 = target;
        let speed: f32 = self.get_speed();
        self.set_speed(speed + (cruise - speed) * approach);
        let step_z: f32 = self.get_position()[2] + self.get_direction() * self.get_speed() * dt;
        // wrap:出了 [−LOOP_HALF_LENGTH, +LOOP_HALF_LENGTH] 就折回另一头,
        // 车道是一条闭合的环形轨道,车永远看不到尽头。停着的车不 wrap ——
        // 停在原地等人,不然「招手停车」会把人甩到另一条街去。
        let wrapped: f32 = if self.get_speed() < 0.05 {
            step_z
        } else if step_z > LOOP_HALF_LENGTH {
            step_z - 2.0 * LOOP_HALF_LENGTH
        } else if step_z < -LOOP_HALF_LENGTH {
            step_z + 2.0 * LOOP_HALF_LENGTH
        } else {
            step_z
        };
        let here: Vec3 = self.get_position();
        self.set_position([here[0], here[1], wrapped]);
    }

    /// 玩家驾驶输入:油门 / 转向 / 刹车,并把车挡在碰撞世界之外。
    ///
    /// 这里**不再把车钉死在车道 X 上**(那是 AI 巡航的约束,玩家驾驶时
    /// 反而让人开不了车)。取而代之的是真实车身朝向 + 侧向速度,见本文件
    /// 顶部的车辆模型常量组。
    ///
    /// # Arguments
    ///
    /// - `f32` - 油门输入(−1..1,正 = 加速)。
    /// - `f32` - 转向输入(−1..1,正 = 右转)。
    /// - `f32` - 固定步长(秒)。
    /// - `&CollisionWorld` - 静态碰撞世界。
    pub fn drive(&mut self, throttle: f32, steer: f32, dt: f32, world: &CollisionWorld) {
        // ---- 1. 纵向:油门 / 刹车 --------------------------------
        let speed: f32 = self.get_speed();
        let next: f32 = if throttle > 0.0 {
            speed + DRIVE_ACCEL * throttle * dt
        } else if throttle < 0.0 {
            speed + DRIVE_BRAKE * throttle * dt
        } else {
            speed
        };
        self.set_speed(next.clamp(0.0, SPEED_MAX * 1.6));

        // ---- 2. 转向:车身角速度 = 舵角 × 纵向速度 / 轴距 -----------
        //
        // bicycle model 的核心:偏航角速度与**速度成正比**。所以低速几乎
        // 转不动(方向盘打满也只是慢慢挪),高速才敢打舵 —— 这就是为什么
        // 真车倒库要来回打好几把。
        let steer: f32 = steer.clamp(-1.0, 1.0);
        let speed: f32 = self.get_speed();
        let grip_scale: f32 = 1.0 / (1.0 + speed.abs() * STEER_SPEED_FALLOFF);
        let yaw_rate: f32 = if speed > STEER_MIN_SPEED {
            steer * STEER_RATE * grip_scale
        } else {
            0.0
        };
        self.set_yaw(self.get_yaw() + yaw_rate * dt);

        // ---- 3. 速度分解 + 侧向甩出 ------------------------------
        //
        // 世界速度 = 沿车身分量 + 侧向分量。转向使车身转过 `yaw_rate*dt`,
        // 原先沿车身的那个速度向量**没有跟着转**,于是它在新的车身坐标系
        // 里多出一个侧向分量 —— 这就是「转向把车甩出去」的物理来源,
        // 不是额外加的人工外推力。
        let yaw: f32 = self.get_yaw();
        let fwd: Vec2 = [yaw.cos(), -yaw.sin()];
        let right: Vec2 = [fwd[1], -fwd[0]];
        let rotated: f32 = yaw_rate * dt;
        // 旧车身系的速度投影 → 新车身系:前向多出 `speed * sin(rotated)`,
        // 侧向多出 `-speed * sin(rotated) * steer_sign`。
        let swung: f32 = speed * rotated.sin();
        let lateral: f32 = self.get_lateral() - swung;

        // ---- 4. 轮胎抓地:侧向速度按指数衰减回 0 -------------------
        let decay: f32 = (-LATERAL_GRIP * dt).exp();
        let lateral: f32 = lateral * decay;

        // ---- 5. 漂移代价:侧滑越狠,纵向掉速越快 --------------------
        let slip: f32 = lateral.abs() / speed.max(1.0);
        let drag: f32 = if slip > DRIFT_SLIP_RATIO {
            DRIFT_SPEED_DRAG * (slip - DRIFT_SLIP_RATIO) * dt
        } else {
            0.0
        };
        self.set_speed((speed - drag).max(0.0));
        self.set_lateral(lateral);

        // ---- 6. 积分位置:沿「纵向 + 侧向」两个分量一起走 ---------
        let forward_speed: f32 = speed * rotated.cos();
        let velocity: Vec2 = [
            fwd[0] * forward_speed + right[0] * lateral,
            fwd[1] * forward_speed + right[1] * lateral,
        ];
        let here: Vec3 = self.get_position();
        let proposed: Vec2 = [here[0] + velocity[0] * dt, here[2] + velocity[1] * dt];
        let resolved: Vec2 = world.resolve_car(proposed);
        let blocked: bool = (resolved[0] - proposed[0]).abs() > f32::EPSILON
            || (resolved[1] - proposed[1]).abs() > f32::EPSILON;
        self.set_position([resolved[0], here[1], resolved[1]]);
        if blocked {
            // 撞墙:速度砍掉,但**不清侧向** —— 蹭着墙甩尾是真实车该有的
            // 反应,清零会让车一碰墙就「立正」,完全失去漂移感。
            self.set_speed(self.get_speed() * 0.25);
        }

        // ---- 7. 车轮转角:走过的距离 / 轮半径 ----------------------
        let travelled: f32 = (velocity[0] * velocity[0] + velocity[1] * velocity[1]).sqrt() * dt;
        self.advance_wheel_spin(travelled / WHEEL_RADIUS);
    }
}

/// 地面拾取物。
#[derive(Clone, Debug)]
pub struct Pickup {
    /// 资产 id。
    pub asset: &'static str,
    /// HUD 上显示的名字。
    pub label: &'static str,
    /// 世界坐标。
    position: Vec3,
    /// 是否已被拾走。
    taken: bool,
    /// 自转相位(弧度),让拾取物在原地缓缓转。
    spin: f32,
}

impl Pickup {
    /// 在指定位置放一个拾取物。
    ///
    /// # Arguments
    ///
    /// - `&'static str` - 资产 id。
    /// - `&'static str` - HUD 显示名。
    /// - `Vec3` - 世界坐标。
    ///
    /// # Returns
    ///
    /// - `Self` - 就绪的拾取物。
    pub fn new(asset: &'static str, label: &'static str, position: Vec3) -> Self {
        Self {
            asset,
            label,
            position,
            taken: false,
            spin: 0.0,
        }
    }

    /// 世界坐标。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 当前世界坐标。
    pub fn get_position(&self) -> Vec3 {
        self.position
    }

    /// 该拾取物的资产 id。
    ///
    /// # Returns
    ///
    /// - `&'static str` - 资产 id。
    pub fn get_asset(&self) -> &'static str {
        self.asset
    }

    /// 是否已被拾走。
    ///
    /// # Returns
    ///
    /// - `bool` - 已被拾走时为 `true`。
    pub fn get_taken(&self) -> bool {
        self.taken
    }

    /// 写入「是否已被拾走」。
    ///
    /// # Arguments
    ///
    /// - `bool` - 是否已被拾走。
    pub fn set_taken(&mut self, value: bool) {
        self.taken = value;
    }

    /// 自转相位。
    ///
    /// # Returns
    ///
    /// - `f32` - 自转相位(弧度)。
    pub fn get_spin(&self) -> f32 {
        self.spin
    }

    /// 推进自转相位。
    ///
    /// # Arguments
    ///
    /// - `f32` - 固定步长(秒)。
    pub fn set_spin_advance(&mut self, dt: f32) {
        let two_pi: f32 = 2.0 * std::f32::consts::PI;
        self.spin = (self.get_spin() + PICKUP_SPIN_RATE * dt) % two_pi;
    }
}

/// 车队与拾取物的集合。
#[derive(Clone, Debug)]
pub struct Traffic {
    /// 车队车辆。
    cars: Vec<TrafficCar>,
    /// 地面拾取物。
    pickups: Vec<Pickup>,
}

impl Traffic {
    /// 新建一个空的交通世界。
    ///
    /// # Returns
    ///
    /// - `Self` - 空的车队与拾取物集合。
    pub fn new() -> Self {
        Self {
            cars: Vec::new(),
            pickups: Vec::new(),
        }
    }

    /// 按蓝图铺出车队与拾取物。
    ///
    /// # Arguments
    ///
    /// - `&[(&'static str, f32, f32, f32)]` - 车道蓝图。
    /// - `&[(&'static str, &'static str, Vec3)]` - 拾取物蓝图。
    pub fn populate(
        &mut self,
        lanes: &[(&'static str, f32, f32, f32)],
        pickups: &[(&'static str, &'static str, Vec3)],
    ) {
        self.get_cars_mut().clear();
        for (asset, lane_x, start_z, direction) in lanes {
            self.get_cars_mut().push(TrafficCar::new(
                asset,
                *lane_x,
                *start_z,
                CRUISE_SPEED.clamp(SPEED_MIN, SPEED_MAX),
                *direction,
            ));
        }
        self.get_pickups_mut().clear();
        for (asset, label, position) in pickups {
            self.get_pickups_mut()
                .push(Pickup::new(asset, label, *position));
        }
    }

    /// 车队车辆列表。
    ///
    /// # Returns
    ///
    /// - `&Vec<TrafficCar>` - 车队。
    pub fn get_cars_ref(&self) -> &Vec<TrafficCar> {
        &self.cars
    }

    /// 地面拾取物列表。
    ///
    /// # Returns
    ///
    /// - `&Vec<Pickup>` - 拾取物集合。
    pub fn get_pickups_ref(&self) -> &Vec<Pickup> {
        &self.pickups
    }

    /// 可变地借用车队。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<TrafficCar>` - 车队的可变引用。
    pub fn get_cars_mut(&mut self) -> &mut Vec<TrafficCar> {
        &mut self.cars
    }

    /// 可变地借用拾取物集合。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<Pickup>` - 拾取物集合的可变引用。
    pub fn get_pickups_mut(&mut self) -> &mut Vec<Pickup> {
        &mut self.pickups
    }

    /// 可变地取一辆车。
    ///
    /// # Arguments
    ///
    /// - `usize` - 车辆索引。
    ///
    /// # Returns
    ///
    /// - `Option<&mut TrafficCar>` - 越界时为 `None`。
    pub fn get_car_mut(&mut self, index: usize) -> Option<&mut TrafficCar> {
        self.cars.get_mut(index)
    }

    /// 可变地取一个拾取物。
    ///
    /// # Arguments
    ///
    /// - `usize` - 拾取物索引。
    ///
    /// # Returns
    ///
    /// - `Option<&mut Pickup>` - 越界时为 `None`。
    pub fn get_pickup_mut(&mut self, index: usize) -> Option<&mut Pickup> {
        self.pickups.get_mut(index)
    }

    /// 推进整个交通世界:车队行驶 + 拾取物自转。
    ///
    /// # Arguments
    ///
    /// - `f32` - 固定步长(秒)。
    /// - `Option<[f32; 2]>` - 正在路边招手的玩家 `[x, z]`;`None` 表示没人。
    pub fn step(&mut self, dt: f32, hailer: Option<[f32; 2]>) {
        // 只有**离玩家最近的那一辆**会靠边等人,不是半径内所有车 ——
        // 否则 26 m 内整条街的车一起停,交通直接堵死。
        let hailer_car: Option<usize> = hailer.and_then(|who: [f32; 2]| {
            let mut best: Option<(usize, f32)> = None;
            for (index, car) in self.get_cars_ref().iter().enumerate() {
                if car.get_driven() {
                    continue;
                }
                let here: Vec3 = car.get_position();
                let dx: f32 = here[0] - who[0];
                let dz: f32 = here[2] - who[1];
                let distance: f32 = (dx * dx + dz * dz).sqrt();
                if distance <= HAUL_RANGE
                    && best
                        .map(|(_held, held): (usize, f32)| distance < held)
                        .unwrap_or(true)
                {
                    best = Some((index, distance));
                }
            }
            best.map(|(index, _distance): (usize, f32)| index)
        });
        for index in 0..self.get_cars_ref().len() {
            if let Some(car) = self.get_car_mut(index) {
                if car.get_driven() {
                    continue;
                }
                car.step(dt, Some(index) == hailer_car);
            }
        }
        for index in 0..self.get_pickups_ref().len() {
            if let Some(pickup) = self.get_pickup_mut(index) {
                pickup.set_spin_advance(dt);
            }
        }
    }
}

/// 找到离玩家最近、且近到可以上车的车。
///
/// # Arguments
///
/// - `&Traffic` - 交通世界。
/// - `Vec3` - 玩家世界坐标。
///
/// # Returns
///
/// - `Option<usize>` - 可上车车辆的索引;附近没有车时为 `None`。
pub fn nearest_car(traffic: &Traffic, position: Vec3) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (index, car) in traffic.get_cars_ref().iter().enumerate() {
        let here: Vec3 = car.get_position();
        let dx: f32 = here[0] - position[0];
        let dz: f32 = here[2] - position[2];
        let distance: f32 = (dx * dx + dz * dz).sqrt();
        if distance <= ENTER_VEHICLE_RADIUS
            && best
                .map(|(_held, held): (usize, f32)| distance < held)
                .unwrap_or(true)
        {
            best = Some((index, distance));
        }
    }
    best.map(|(index, _distance): (usize, f32)| index)
}

/// 结算一次拾取:回血 / 加钱 / 记录,并标记拾取物已被拿走。
///
/// # Arguments
///
/// - `&mut Player` - 玩家状态。
/// - `&mut Pickup` - 命中的拾取物。
pub fn apply_pickup(player: &mut Player, pickup: &mut Pickup) {
    let notice: String = match pickup.asset {
        PICKUP_HEALTH_PACK => {
            let healed: f32 = (player.get_health() + HEALTH_PACK_HEAL).min(MAX_HEALTH);
            player.set_health(healed);
            String::from(NOTICE_HEALTH)
        }
        PICKUP_CASH_STACK => {
            player.set_cash_add(CASH_STACK_AMOUNT);
            String::from(NOTICE_CASH)
        }
        _ => format!("{NOTICE_WEAPON}{}", pickup.label),
    };
    player.set_notice(notice);
    pickup.set_taken(true);
}

#[cfg(test)]
mod tests {
    use crate::r#const::*;
    use crate::traffic::{
        DRIFT_SLIP_RATIO, LATERAL_GRIP, STEER_MIN_SPEED, STEER_RATE, TrafficCar, WHEEL_RADIUS,
    };
    use crate::r#type::Vec3;

    fn empty_world() -> crate::collision::CollisionWorld {
        crate::collision::CollisionWorld::new()
    }

    fn car() -> TrafficCar {
        TrafficCar::new(CAR_SEDAN, 0.0, 0.0, 8.0, 1.0)
    }

    /// 满舵直线行驶:车必须真的转出角度,而不是继续沿原方向直行。
    #[test]
    fn steering_turns_the_car() {
        let mut c: TrafficCar = car();
        let world: crate::collision::CollisionWorld = empty_world();
        let yaw0: f32 = c.get_yaw();
        for _ in 0..120 {
            c.drive(0.6, 1.0, 1.0 / 60.0, &world);
        }
        assert!(
            (c.get_yaw() - yaw0).abs() > 0.3,
            "满舵 2 秒后车必须转过明显角度,实际只转了 {}",
            (c.get_yaw() - yaw0).abs()
        );
    }

    /// 转向必须产生侧向速度(漂移),且不给舵时侧向速度衰减回 0。
    #[test]
    fn steering_produces_lateral_slip() {
        let mut c: TrafficCar = car();
        let world: crate::collision::CollisionWorld = empty_world();
        for _ in 0..30 {
            c.drive(0.8, 1.0, 1.0 / 60.0, &world);
        }
        let slip: f32 = c.get_lateral().abs();
        assert!(slip > 0.5, "转向 0.5 秒必须甩出侧向速度,实际 {}", slip);
        let before: f32 = slip;
        for _ in 0..180 {
            c.drive(0.0, 0.0, 1.0 / 60.0, &world);
        }
        assert!(
            c.get_lateral().abs() < before * 0.1,
            "不打舵时侧向速度必须按抓地衰减,前 {} 现在 {}",
            before,
            c.get_lateral().abs()
        );
    }

    /// 低速不转向:bicycle model 的角速度与速度成正比,静止时不能原地转圈。
    #[test]
    fn steering_is_disabled_below_the_dead_zone() {
        let mut c: TrafficCar = TrafficCar::new(CAR_SEDAN, 0.0, 0.0, 0.0, 1.0);
        let world: crate::collision::CollisionWorld = empty_world();
        let yaw0: f32 = c.get_yaw();
        for _ in 0..120 {
            c.drive(0.0, 1.0, 1.0 / 60.0, &world);
        }
        assert!(
            c.get_speed() <= STEER_MIN_SPEED,
            "不给油时车速必须留在死区内,实际 {}",
            c.get_speed()
        );
        assert!(
            (c.get_yaw() - yaw0).abs() < 1e-4,
            "静止时打满舵也不能转,实际转了 {}",
            (c.get_yaw() - yaw0).abs()
        );
    }

    /// 车轮转角 = 行驶距离 / 轮半径,与速度无关。
    #[test]
    fn wheel_spin_follows_travelled_distance() {
        let mut c: TrafficCar = car();
        let world: crate::collision::CollisionWorld = empty_world();
        let start: Vec3 = c.get_position();
        for _ in 0..120 {
            c.drive(0.5, 0.0, 1.0 / 60.0, &world);
        }
        let end: Vec3 = c.get_position();
        let distance: f32 = ((end[0] - start[0]).powi(2) + (end[2] - start[2]).powi(2)).sqrt();
        let expected: f32 = distance / WHEEL_RADIUS;
        assert!(
            (c.get_wheel_spin() - expected).abs() < 0.05,
            "车轮转角 {} 应等于距离/轮半径 {}",
            c.get_wheel_spin(),
            expected
        );
        assert!(
            c.get_wheel_spin() > 0.1,
            "开起来之后车轮必须真的在转,实际 {}",
            c.get_wheel_spin()
        );
    }

    /// 抓地力旋钮方向正确:侧滑会吃掉纵向速度。
    #[test]
    fn drift_costs_forward_speed() {
        let mut straight: TrafficCar = car();
        let mut drifting: TrafficCar = car();
        let world: crate::collision::CollisionWorld = empty_world();
        for _ in 0..60 {
            straight.drive(0.8, 0.0, 1.0 / 60.0, &world);
            drifting.drive(0.8, 1.0, 1.0 / 60.0, &world);
        }
        assert!(
            drifting.get_lateral().abs() / drifting.get_speed().max(1.0) > DRIFT_SLIP_RATIO,
            "满舵必须进入侧滑区间,实际滑移比 {}",
            drifting.get_lateral().abs() / drifting.get_speed().max(1.0)
        );
        assert!(
            drifting.get_speed() < straight.get_speed(),
            "漂移必须比直行慢,漂移 {} 直行 {}",
            drifting.get_speed(),
            straight.get_speed()
        );
    }

    /// AI 巡航不受影响:不打舵的 AI 车仍然走直线、方向常量。
    #[test]
    fn ai_cruise_still_holds_its_lane() {
        let mut c: TrafficCar = car();
        for _ in 0..120 {
            c.step(1.0 / 60.0, false);
        }
        let pos: Vec3 = c.get_position();
        assert!(
            (pos[0] - c.get_lane_x()).abs() < 1e-5,
            "AI 巡航必须留在车道 X 上,实际 {} 车道 {}",
            pos[0],
            c.get_lane_x()
        );
        assert!(
            (c.get_yaw() - c.get_direction() * -std::f32::consts::FRAC_PI_2).abs() < 1e-5,
            "AI 巡航的朝向必须仍由行驶方向推出"
        );
    }

    /// 抓地常数必须是正的有限值,否则指数衰减会变成放大。
    #[test]
    fn grip_constant_is_a_valid_decay() {
        assert!(LATERAL_GRIP > 0.0 && LATERAL_GRIP.is_finite());
        assert!(STEER_RATE > 0.0 && STEER_RATE.is_finite());
    }
}
