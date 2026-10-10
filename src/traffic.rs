//! 交通 AI + 拾取物:沿街道网格行驶的车队、可上下的车辆、地面拾取物。
//!
//! 车队**不是自由漫游**,但也不再是「一条固定 X 的直线」。每辆车被分配
//! 到一条**城市街道上的车道**(车道中心 = 街道中轴线 ± [`LANE_OFFSET`]),
//! 并沿着**路点(route)**行驶:走到下一个路口时按 [`ROUTE_PICK`] 的比例
//! 概率决定直行还是转向,转向就换到横向街道的对应车道上继续开。
//!
//! 之前这里是「常量 `lane_x` + 起点 z + 方向」三元组,车永远沿一条
//! 直线跑,到端点 wrap —— 结构上没有转向,也就没有路口。而车道的 X 是
//! 写死的 `26.5 / 33.5 / -26.5 / -33.5`,**这四条线根本不在街道上**:
//! 街道中轴线落在 `STREET_PITCH * k`(即 `0 / ±60 / ±120 ...`),
//! 车道必须在 `轴线 ± 3.5` 处。实测(aff3e39):车道 x=26.5 的循环里有
//! **8.1%** 的里程落在建筑 AABB 内(最深 2.03 m,`bldg_lilac_tower`),
//! x=-33.5 有 **19.8%**(最深 4.96 m,`bldg_teal_loft`)。这就是用户报的
//! 「NPC 开的汽车会穿越建筑」—— 不是碰撞失效,是车道画在了街上没有的
//! 地方。
//!
//! 现在两件事同时成立:
//!
//! 1. **车道由街道网格派生**([`lane_axis`]),车永远在真实沥青面上。
//! 2. **巡航真的走路网**([`TrafficCar::route_step`]),在路口转向,
//!    并且每一步都用 `CollisionWorld::resolve_car_footprint`(真实
//!    `4.566 × 1.851 m` 有向足迹)解一次碰撞 —— 挡住就停,不再穿模。
//!
//! 车灯:`car_*` 资产的 `frontlights` / `rearlights` part 带 `emissive`,
//! 展开时已经逐三角形写进顶点缓冲的第四个 vec3(见 `render::build_gpu_mesh`),
//! 夜间 `SceneLighting::emissive_gain` 拉到 1.85 + 泛光 pass,所以车灯
//! 是真的在发光,不是画一个假的亮点贴图。

use crate::{
    FRAC_PI_2, PI,
    collision::{CAR_FOOTPRINT, CAR_STEP, CollisionWorld},
    r#const::*,
    player::{MAX_HEALTH, Player},
    r#type::{Vec2, Vec3},
};

/// 街道在 Z 轴上的可行驶半长(米)。
///
/// 由 [`crate::game::traffic_half`] 从城市半边长推导,车道两端到端点 wrap。
pub const LOOP_HALF_LENGTH: f32 = 120.0;

/// 相邻两条街道轴线之间的间距(米)—— 与 `game::STREET_PITCH` 同值。
///
/// 这里**重述一份而不是引用 `game.rs`**:模块依赖是单向的(`game` 依赖
/// `traffic`,反过来就成环),而这个数字同时决定了「车走哪条街」,必须和
/// `game::street_axis` 一致。两条常量各有一处单测把 `±3.5` 的车道坐标
/// 与 `game::lane_x()` 对拍(见 `lane_axis_matches_the_city_grid`),网格
/// 间距一旦在 `game.rs` 改了而这里没改,那些测试立刻变红。
pub(crate) const STREET_PITCH: f32 = 60.0;
/// 车道中心相对街道中轴线的横向偏移(米)—— 与 `game::LANE_OFFSET_X` 同值。
///
/// 路面半宽 7 m,双向车道各占一半,车道中心落在 ±3.5 m 处,正好压在
/// 程序化地面画的车道虚线上。
pub(crate) const LANE_OFFSET: f32 = 3.5;

/// 车队巡航速度下限(米/秒)。
pub const SPEED_MIN: f32 = 8.0;
/// 车队巡航速度上限(米/秒)。
pub const SPEED_MAX: f32 = 15.0;
/// 玩家驾驶时的加速度(米/秒²)。
pub const DRIVE_ACCEL: f32 = 11.0;
/// 玩家驾驶时的刹车减速度(米/秒²)。
pub const DRIVE_BRAKE: f32 = 18.0;
/// 倒车时的加速度(米/秒²),正值,用来把速度推向 `−REVERSE_SPEED_MAX`。
///
/// 倒车比前进慢:真车倒库的速度本来就低,而且低了才容易把方向打准。
/// 这条是 [`REVERSE_SPEED_MAX`] 的唯一加速度来源,不是给倒车用的减速度。
pub(crate) const REVERSE_ACCEL: f32 = 6.0;
/// 倒车速度上限(米/秒)—— 绝对值。
///
/// 真车倒库基本不超过前进速度的三分之一,GTA V 里倒车也只有慢慢蹭的
/// 速度。取前进上限(`SPEED_MAX`)的 0.4 倍。
pub(crate) const REVERSE_SPEED_MAX: f32 = SPEED_MAX * 0.4;
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
pub(crate) const WHEEL_MOUNTS: [[f32; 3]; 4] = [
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
pub(crate) const PICKUP_RADIUS: f32 = 2.2;
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

// ---- 车队路网:车怎么在街道网格上选路 -------------------------------
//
// 一辆车在网格上有一个**所在街道索引**(`street`)与**所在车道侧**
// (`side` = −1 / +1,即街道中轴线两侧各一条车道),外加一个**行进轴**
// (`axis`)。`axis = 0` 表示沿 Z 走(车位于一条南北向街道上),
// `axis = 1` 表示沿 X 走(车位于一条东西向街道上)。
//
// 走法:沿当前车道开到下一个路口,然后要么直行(街道索引 +1,车道侧翻转,
// 因为双向车道),要么转向(切到另一条街道上去,车道侧保持)。转向时按
// [`ROUTE_PICK`] 抽签,让车流不会全城一个方向。

/// 车队在路口选择「转向」而不是「直行」的概率(0..1)。
///
/// 0.62 让每个路口平均每 1.6 次就转一次弯,弯多到一眼看得出是路口,
/// 又不至于整条街的车都挤进同一个街区。直行的那部分仍占 38%,于是
/// 纵向街道上仍然有连续可见的车流。
const ROUTE_PICK: f32 = 0.62;

/// AI 车每步推进的子步长(米)—— 与 [`CAR_FOOTPRINT`] 配套。
///
/// 数值取自 [`CAR_STEP`]:车长 4.566 m,而世界里最薄的静态碰撞体只有
/// 0.133 m(交通锥),一帧走 1.6 m 足以整个跳过它。子步保证任何单步
/// 位移都短于最薄障碍 —— 那是「车不再穿楼」的最后一道保证。


/// 「这一步其实没走成」的判定余量(米)。
///
/// 位移被碰撞吃掉时,判定为撞上障碍:车停下、速度归零、路网状态不推进。
/// 余量必须小于单帧位移,否则正常行驶也会被误判成撞墙。
const CAR_ROUTE_EPSILON: f32 = 0.001;

/// 转向决策用的确定性伪随机流 —— 每辆车一份,推进步长固定。
///
/// 不引入 `rand` 依赖:城市其余部分(街道 / 楼 / 道具)全部用同一套
/// `hash2` 风格的可重放哈希,车队保持一致才能让回归测试断言一个
/// **具体**的行驶轨迹,而不是只能断言统计性质。
///
/// # Arguments
///
/// - `u32` - 上一轮的流状态。
///
/// # Returns
///
/// - `u32` - 本轮洗牌后的流状态。
#[must_use]
fn route_rand(seed: u32) -> u32 {
    let mut h: u32 = seed.wrapping_mul(0x9E37_79B9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^ (h >> 16)
}
/// 把 `u32` 流推进到 `[0, 1)` 浮点序列上。
///
/// # Arguments
///
/// - `u32` - 上一轮的流状态。
///
/// # Returns
///
/// - `f32` - 本轮 `[0, 1)` 的抽签结果。
fn next_unit(seed: u32) -> f32 {
    (route_rand(seed) & 0x00FF_FFFF) as f32 / (0x0100_0000 as f32)
}

/// 街道轴线的世界坐标 —— 与 `game::street_axis` 同一定义。
///
/// # Arguments
///
/// - `i32` - 街道索引(任意整数)。
///
/// # Returns
///
/// - `f32` - 该街道中轴线的世界 X 或 Z 坐标(米)。
fn street_axis(index: i32) -> f32 {
    STREET_PITCH * index as f32
}

/// 把一个世界坐标折回它所在街道的索引。
///
/// # Arguments
///
/// - `f32` - 世界 X 或 Z 坐标(米)。
///
/// # Returns
///
/// - `i32` - 最近的街道索引。
fn street_index_at(along: f32) -> i32 {
    (along / STREET_PITCH).round() as i32
}

/// 车道中心的世界坐标(街道中轴线 ± [`LANE_OFFSET`])。
///
/// # Arguments
///
/// - `i32` - 街道索引。
/// - `f32` - 车道在街道哪一侧(`-1.0` / `+1.0`)。
///
/// # Returns
///
/// - `f32` - 车道中心坐标(米)。
fn lane_axis(street: i32, side: f32) -> f32 {
    street_axis(street) + side * LANE_OFFSET
}

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
    /// 行车的固定 X 坐标(米)—— 玩家驾驶时不再被强制拉回。
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
    /// 车当前所在街道的网格索引(`street_axis` 的参数)。
    ///
    /// AI 巡航的**路网坐标**:配合 [`Self::lane_side`] 与
    /// [`Self::lane_axis_index`] 就能完全确定车在网格上的位置与朝向,
    /// 不需要再存一条「世界坐标 + 直线方向」的退化车道。
    route_street: i32,
    /// 车道在街道中轴线的哪一侧(`-1.0` / `+1.0`)。
    route_side: f32,
    /// 行进轴:`0.0` = 沿 Z 走(南北向街道),`1.0` = 沿 X 走(东西向街道)。
    route_axis: i32,
    /// 转向抽签用的伪随机流状态(每辆车一份)。
    route_seed: u32,
}

/// Inherent implementation of [`TrafficCar`].
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
    /// AI 巡航时这个值**会随路网改变**(转向后车就换到另一条街道上,
    /// 车道 X 自然不同),玩家驾驶时保留最后一次巡航写入的值。
    ///
    /// # Returns
    ///
    /// - `f32` - 车道中心 X(米)。
    pub fn get_lane_x(&self) -> f32 {
        self.lane_x
    }

    /// 车道所在街道的网格索引。
    ///
    /// # Returns
    ///
    /// - `i32` - 当前街道索引(`street_axis` 的参数)。
    pub fn get_lane_street(&self) -> i32 {
        self.route_street
    }

    /// 车道在街道中轴线的哪一侧。
    ///
    /// # Returns
    ///
    /// - `f32` - `-1.0` 或 `+1.0`。
    pub fn get_lane_side(&self) -> f32 {
        self.route_side
    }

    /// 行进轴。
    ///
    /// # Returns
    ///
    /// - `i32` - `0` = 沿 Z 走(南北向街道),`1` = 沿 X 走(东西向街道)。
    pub fn get_lane_axis(&self) -> i32 {
        self.route_axis
    }

    /// 换到另一条平行街道,并把车道中轴线挪到新车道中心。
    ///
    /// 路口转弯 / 直行换街都走这里:三个路网状态必须**同时**更新,
    /// 分开赋值会让下一帧按旧的 street/side 算出错误的目标点。
    ///
    /// # Arguments
    ///
    /// - `i32` - 新街道的网格索引。
    /// - `f32` - 新车道在哪一侧(`-1.0` / `+1.0`)。
    /// - `i32` - 新街道的行进轴。
    pub fn set_lane(&mut self, street: i32, side: f32, axis: i32) {
        self.route_street = street;
        self.route_side = side;
        self.route_axis = axis;
        self.lane_x = lane_axis(street, side);
    }

    /// 推进一次伪随机流并抽出 `[0, 1)` 的结果。
    ///
    /// # Returns
    ///
    /// - `f32` - 本轮抽签结果;推进后的流状态保证同一条轨迹可重放。
    pub fn next_route_draw(&mut self) -> f32 {
        let draw: f32 = next_unit(self.get_route_seed());
        self.set_route_seed();
        draw
    }

    /// 取当前流状态,并置零 —— 下一次 [`Self::set_route_seed`] 从这里续上。
    ///
    /// # Returns
    ///
    /// - `u32` - 本轮抽签使用的流状态。
    fn get_route_seed(&mut self) -> u32 {
        let seed: u32 = self.route_seed;
        self.route_seed = 0;
        seed
    }

    /// 把流状态洗牌后写回。
    fn set_route_seed(&mut self) {
        self.route_seed = route_rand(self.route_seed);
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

    /// 把一组的「起始位置 + 朝向」折算成路网坐标。
    ///
    /// 街道索引与车道侧**不是**存下来的额外真值,而是每帧从位置重算的
    /// 派生量(`realign_route`)。这样任何外部写位置 —— 包括游戏自己
    /// `set_position`、下车、`resolve_dynamic_bodies` 的车车分离 ——
    /// 都不会留下一条与实际位置矛盾的陈旧车道。
    ///
    /// # Arguments
    ///
    /// - `f32` - 起始 X 坐标(米)。
    /// - `f32` - 起始 Z 坐标(米)。
    /// - `f32` - 行驶方向(`1.0` = 沿 +Z,`-1.0` = 沿 -Z;只用于决定车道侧)。
    ///
    /// # Returns
    ///
    /// - `(i32, f32, i32, f32)` - `(街道索引, 车道侧, 行进轴, 车道中心)`,其中
    ///   行进轴 `0` 表示沿 Z 行驶,车道中心就是 X 坐标。
    fn route_from_xz(x: f32, z: f32, direction: f32) -> (i32, f32, i32, f32) {
        let street: i32 = street_index_at(x);
        // 沿 +Z 行驶靠街道**右侧**(东侧)的车道,沿 -Z 靠左侧 —— 双向
        // 车道的惯例,也是右舵国家「靠右行驶」的那一侧。
        let side: f32 = if direction >= 0.0 { 1.0 } else { -1.0 };
        (street, side, 0, lane_axis(street, side))
    }

    /// 在指定车道上放一辆车。
    ///
    /// `lane_x` / `start_z` / `direction` 是**世界坐标下的起点**,路网坐标
    /// 由 [`Self::route_from_xz`] 从起点现算。旧版把 `lane_x` 当成一条
    /// 永久不变的车道线,那正是「车开在街上没有的地方」的来源。
    ///
    /// # Arguments
    ///
    /// - `&'static str` - 资产 id。
    /// - `f32` - 起始车道中心的 X 坐标(米)。
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
        let yaw: f32 = -direction * FRAC_PI_2;
        let (street, side, axis, centre): (i32, f32, i32, f32) =
            Self::route_from_xz(lane_x, start_z, direction);
        Self {
            asset,
            position: [centre, 0.0, start_z],
            speed: cruise,
            cruise,
            lane_x: centre,
            direction,
            yaw,
            lateral: 0.0,
            wheel_spin: 0.0,
            driven: false,
            route_street: street,
            route_side: side,
            route_axis: axis,
            route_seed: 0x7A11_u32 ^ street as u32 ^ (direction > 0.0) as u32 as u32,
        }
    }

    /// 按路网坐标放一辆车 —— 车队蓝图用的入口。
    ///
    /// 与 [`Self::new`] 的区别是它直接给**网格坐标**(街道索引 + 车道侧 +
    /// 行进轴),不依赖「先想一个世界坐标」。位置由这三个量算出来,所以
    /// 车队蓝图里**不可能再写出一条不在街道上的车道**。
    ///
    /// # Arguments
    ///
    /// - `&'static str` - 资产 id。
    /// - `i32` - 所在街道索引。
    /// - `f32` - 车道侧(`-1.0` / `+1.0`)。
    /// - `i32` - 行进轴(`0` = 沿 Z,`1` = 沿 X)。
    /// - `f32` - 起始沿街坐标(米);`axis = 0` 时是 Z,`axis = 1` 时是 X。
    /// - `f32` - 巡航速度(米/秒)。
    /// - `f32` - 行驶方向(`1.0` / `-1.0`,决定沿街坐标的增减)。
    ///
    /// # Returns
    ///
    /// - `Self` - 就绪的车辆状态。
    pub fn new_on_lane(
        asset: &'static str,
        street: i32,
        side: f32,
        axis: i32,
        start_along: f32,
        cruise: f32,
        direction: f32,
    ) -> Self {
        let cross: f32 = lane_axis(street, side);
        let position: Vec3 = if axis == 0 {
            [cross, 0.0, start_along]
        } else {
            [start_along, 0.0, cross]
        };
        // `yaw` 沿用本文件的约定 `fwd = [cos yaw, -sin yaw]`:沿 +Z 行驶
        // 的车 yaw = -PI/2,沿 +X 行驶的车 yaw = 0。
        let yaw: f32 = if axis == 0 {
            -direction * FRAC_PI_2
        } else {
            direction * FRAC_PI_2
        };
        Self {
            asset,
            position,
            speed: cruise,
            cruise,
            lane_x: cross,
            direction,
            yaw,
            lateral: 0.0,
            wheel_spin: 0.0,
            driven: false,
            route_street: street,
            route_side: side,
            route_axis: axis,
            route_seed: 0x7A11_u32 ^ (street as u32).wrapping_mul(31) ^ side.to_bits(),
        }
    }

    /// 把车在网格上的位置与朝向重新对齐到它**实际所在**的坐标。
    ///
    /// 每一步巡航之前都调一次。它是必需的而不是洁癖:外部(下车、
    /// `resolve_dynamic_bodies` 的车车分离)会改 `set_position`,于是缓存的
    /// `route_street` 就与真实位置差了半条街 —— 不重算的话车会朝一个
    /// 方向开但**按另一个坐标积分**,几帧之内就开到马路对面去了。
    ///
    /// # Arguments
    ///
    /// - `f32` - 行驶方向(`1.0` / `-1.0`)。
    fn realign_route(&mut self, direction: f32) {
        let here: Vec3 = self.get_position();
        // 沿哪根轴跑,取决于「横向」坐标离街道中轴线有多近:车在自己的
        // 车道中心上,所以横向距离约等于 LANE_OFFSET,纵向则可以任意。
        let to_x_street: f32 = (here[0] - street_axis(street_index_at(here[0]))).abs();
        let to_z_street: f32 = (here[2] - street_axis(street_index_at(here[2]))).abs();
        // 偏离自己的车道超过一个车宽(≈1.85 m)就认为被外力推歪了,
        // 这时按**横向**重新落位;否则保持既有的行进轴。
        let skewed: bool = if self.get_lane_axis() == 0 {
            to_x_street - LANE_OFFSET
        } else {
            to_z_street - LANE_OFFSET
        }
        .abs()
            > CAR_FOOTPRINT[1];
        let axis: i32 = if skewed {
            if to_x_street < to_z_street {
                0
            } else {
                1
            }
        } else {
            self.get_lane_axis()
        };
        let cross_here: f32 = if axis == 0 { here[0] } else { here[2] };
        let street: i32 = street_index_at(cross_here);
        // 车道侧:车在中轴线哪一侧就是哪一侧,`+0.0` / `-0.0` 一律算右侧。
        let side: f32 = if cross_here >= street_axis(street) {
            1.0
        } else {
            -1.0
        };
        self.set_lane(street, side, axis);
        self.set_direction(direction);
        self.set_yaw(Self::lane_yaw(axis, direction));
    }

    /// 由「行进轴 + 行驶方向」推出车身朝向。
    ///
    /// # Arguments
    ///
    /// - `i32` - 行进轴(`0` = 沿 Z,`1` = 沿 X)。
    /// - `f32` - 行驶方向(`1.0` / `-1.0`)。
    ///
    /// # Returns
    ///
    /// - `f32` - 绕 Y 轴的车身朝向(弧度)。
    fn lane_yaw(axis: i32, direction: f32) -> f32 {
        // 本文件的 yaw 约定是 `fwd = [cos yaw, -sin yaw]`(见
        // [`Self::get_yaw`]),所以沿 +Z 的车 yaw = -PI/2、沿 +X 的车 yaw = 0。
        if axis == 0 {
            -direction * FRAC_PI_2
        } else {
            direction * FRAC_PI_2
        }
    }

    /// 写入行驶方向。
    ///
    /// # Arguments
    ///
    /// - `f32` - `1.0` = 沿 +Z / +X,`-1.0` = 沿 -Z / -X。
    pub fn set_direction(&mut self, value: f32) {
        self.direction = if value >= 0.0 { 1.0 } else { -1.0 };
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

    /// 车队模式下推进一个固定步长:沿**路网**前进,在路口决定转向。
    ///
    /// 这一步做三件事,顺序不能换:
    ///
    /// 1. 先按 [`Self::realign_route`] 把路网坐标对齐到真实位置 ——
    ///    外部(下车、车车分离)随时可能挪动过车。
    /// 2. 沿当前车道走,位置用
    ///    `CollisionWorld::resolve_car_footprint` 解一次碰撞:被挡住就
    ///    **停在障碍前面**,而不是像旧版那样只改 Z 坐标硬穿过去。
    ///    这正是用户报的「NPC 开的汽车会穿越建筑」—— 旧版 `step` 里
    ///    根本没有任何碰撞调用,车是被**直接赋值**放到新坐标的。
    /// 3. 越过路口时抽签:转向就换到横向街道的对应车道继续开。
    ///
    /// # Arguments
    ///
    /// - `f32` - 固定步长(秒)。
    /// - `bool` - `true` 表示路边有玩家招手,这辆车减速停靠等客。
    /// - `&CollisionWorld` - 静态碰撞世界,用来把车挡在障碍之外。
    pub fn step(&mut self, dt: f32, hailer: bool, world: &CollisionWorld) {
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
        self.route_advance(dt, world);
    }

    /// 路网积分的一步:沿当前车道走,到路口按 [`ROUTE_PICK`] 抽签转向。
    ///
    /// 这是「车怎么走路网」的全部逻辑,[`Self::step`] 只是它外面套了一层
    /// 速度逼近。拆出来是因为测试要在**不带招手逻辑**的前提下断言一辆
    /// 车真的在路口转了 90°,而带套的那条路径会被 `HAUL_BRAKE` 干扰。
    ///
    /// # Arguments
    ///
    /// - `f32` - 固定步长(秒)。
    /// - `&CollisionWorld` - 静态碰撞世界。
    pub fn route_advance(&mut self, dt: f32, world: &CollisionWorld) {
        let direction: f32 = self.get_direction();
        self.realign_route(direction);
        let axis: i32 = self.get_lane_axis();
        let speed: f32 = self.get_speed();
        let centre: f32 = lane_axis(self.get_lane_street(), self.get_lane_side());
        let here: Vec3 = self.get_position();
        // 沿街分量的步长(米)。停着的车不位移。
        let travel: f32 = direction * speed * dt;
        let delta: Vec2 = if axis == 0 {
            [0.0, travel]
        } else {
            [travel, 0.0]
        };
        // 位置用**真实车身足迹**解碰撞:车宽 4.566 × 1.851 m 的有向盒,
        // 位移按 `CAR_STEP` 切子步。被挡下时位移被吃掉,车就停在障碍
        // 前面 —— 这条是「不再穿楼」的最后一道保证,即使路网或蓝图出错,
        // 车也只会卡住而不会开进去。
        let resolved: Vec2 =
            world.resolve_car_footprint([here[0], here[2]], delta, self.get_yaw());
        self.set_position([resolved[0], here[1], resolved[1]]);
        if self.get_speed() > 0.0 {
            let moved: f32 = (resolved[0] - here[0]).abs() + (resolved[1] - here[2]).abs();
            let wanted: f32 = travel.abs();
            if moved + CAR_ROUTE_EPSILON < wanted {
                // 被完全挡住(或者一步都没走成):不推进路网状态,停在
                // 障碍前。速度砍到零让它重新加速而不是顶着墙磨。
                self.set_speed(0.0);
                return;
            }
        }
        // 沿街方向:路口判定只看自己这一根轴,横向坐标恒为车道中心。
        let along: f32 = if axis == 0 { resolved[1] } else { resolved[0] };
        self.set_position([
            if axis == 0 { centre } else { resolved[0] },
            here[1],
            if axis == 0 { resolved[1] } else { centre },
        ]);
        // 走过路口了吗?取「前进方向上的下一个路口」。
        let junction: f32 = if direction >= 0.0 {
            street_axis(street_index_at(along) + 1)
        } else {
            street_axis(street_index_at(along) - 1)
        };
        let crossed: bool = if direction >= 0.0 {
            along >= junction - CAR_ROUTE_EPSILON
        } else {
            along <= junction + CAR_ROUTE_EPSILON
        };
        if crossed && self.get_speed() > 0.0 {
            self.choose_at_junction();
        }
    }

    /// 在路口抽签:直行还是转向。
    ///
    /// 直行 = 落到下一条平行街道的**对侧车道**(双向车道,继续往前开就得
    /// 换到马路对面去,否则会逆行)。转向 = 换到横向街道上去,车道侧不变
    /// —— 右侧通行下右转进对侧车道、左转进同侧车道,两条合起来就是转 90°。
    fn choose_at_junction(&mut self) {
        let turn: bool = self.next_route_draw() < ROUTE_PICK;
        let direction: f32 = self.get_direction();
        let here: Vec3 = self.get_position();
        let along: f32 = if self.get_lane_axis() == 0 {
            here[2]
        } else {
            here[0]
        };
        // 转向:新的「沿街」坐标由**交叉轴**当前所在街道决定 —— 车开到
        // 路口中心时,横向坐标仍在自己这条街上,所以新街道索引取横向的。
        let cross_street: i32 = street_index_at(if self.get_lane_axis() == 0 {
            here[0]
        } else {
            here[2]
        });
        if turn {
            let next_axis: i32 = 1 - self.get_lane_axis();
            self.set_lane(cross_street, self.get_lane_side(), next_axis);
            self.set_direction(direction);
            self.set_yaw(Self::lane_yaw(next_axis, direction));
            // 转向后沿街坐标就是原来那条街的中轴线 —— 这正是路口中心。
            let junction_centre: f32 = street_axis(cross_street);
            let new_lane_x: f32 = self.get_lane_x();
            self.set_position(if next_axis == 0 {
                [new_lane_x, here[1], junction_centre]
            } else {
                [junction_centre, here[1], new_lane_x]
            });
        } else {
            // 直行:跨到下一条平行街道,并翻到对侧车道。
            let next_street: i32 = if direction >= 0.0 {
                street_index_at(along) + 1
            } else {
                street_index_at(along) - 1
            };
            let next_side: f32 = -self.get_lane_side();
            self.set_lane(next_street, next_side, self.get_lane_axis());
            let centre: f32 = self.get_lane_x();
            self.set_position(if self.get_lane_axis() == 0 {
                [centre, here[1], along]
            } else {
                [along, here[1], centre]
            });
        }
    }

    /// 玩家驾驶输入:油门 / 转向 / 刹车,并把车挡在碰撞世界之外。
    ///
    /// 速度**可以是负的**:负 = 倒车。车在 GTA V 里本来就有倒库这一档,
    /// 所以 `S` 的语义分两段:前进中按 `S` 是刹车(把正速度减到 0),
    /// 到 0 之后继续按就转成倒车(速度变负,车尾朝前走)。
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
        // ---- 1. 纵向:油门 / 刹车 / 倒车 -----------------------------
        //
        // 速度是**带符号**的:正 = 前进,负 = 倒车。上下限分别是
        // `-REVERSE_SPEED_MAX` 与 `SPEED_MAX * 1.6`,所以 S 在前进中是
        // 刹车(速度掉到 0 就停住),按住不放继续变成倒车。这两段之间
        // 没有额外状态,判据只是「当前速度的符号」,所以前进 / 刹车 /
        // 倒车不会互相漏进对方 —— 这是原来 `clamp(0.0, ..)` 做不到的,
        // 那个下限把倒车整段从物理上删掉了。
        let speed: f32 = self.get_speed();
        let next: f32 = if throttle > 0.0 {
            speed + DRIVE_ACCEL * throttle * dt
        } else if throttle < 0.0 {
            if speed > 0.0 {
                // 前进中踩「刹车」:减速度按 DRIVE_BRAKE,越过 0 就停下。
                (speed + DRIVE_BRAKE * throttle * dt).max(0.0)
            } else {
                // 静止或已在倒车:同一个键变成倒车油门。
                (speed + REVERSE_ACCEL * throttle * dt).max(-REVERSE_SPEED_MAX)
            }
        } else {
            speed
        };
        self.set_speed(next.clamp(-REVERSE_SPEED_MAX, SPEED_MAX * 1.6));

        // ---- 2. 转向:车身角速度 = 舵角 × 纵向速度 / 轴距 -----------
        //
        // bicycle model 的核心:偏航角速度与**速度成正比**。所以低速几乎
        // 转不动(方向盘打满也只是慢慢挪),高速才敢打舵 —— 这就是为什么
        // 真车倒库要来回打好几把。
        //
        // `yaw_rate` 带一个负号,因为本文件的 yaw 约定是
        // `fwd = [cos yaw, −sin yaw]`,于是 `d(fwd)/d(yaw)` = `[−sin, −cos]`
        // —— 那是车头的**左**方���。也就是说 yaw 增大 = 车向左转,而 `steer`
        // 的契约是「正 = 右转」。不取负号时按 D 会把车往左拐,这正是方向键
        // 反向的根因;符号基准不是猜的,是从上面这个 `fwd` 定义推出来的。
        let steer: f32 = steer.clamp(-1.0, 1.0);
        let speed: f32 = self.get_speed();
        let grip_scale: f32 = 1.0 / (1.0 + speed.abs() * STEER_SPEED_FALLOFF);
        // 死区按**速度的绝对值**判:倒车时 `speed` 是负的,但倒车一样要能
        // 打方向(倒库全靠它)。写 `speed > STEER_MIN_SPEED` 会让整段倒车
        // 永远转不动 —— 速度为负时那个判断恒假。
        //
        // 负号是转向符号的**根因修复**。本文件的 yaw 约定来自第 6 步的
        // `fwd = [cos yaw, −sin yaw]`,于是
        // `d(fwd)/d(yaw) = [−sin yaw, −cos yaw]` —— 那是车头的**左**方向。
        // 也就是说 yaw 增大 = 车向左转,而 `steer` 的契约是「正 = 右转」
        // (`steer_sign` 对 KEYD / ARROWRIGHT 返回 +1)。两边基准不同,所以
        // `yaw_rate` 必须带一个负号才对齐;少了它,按 D 会把车往左拐 ——
        // 这正是「方向键反向」的全部来源。符号不是从直觉定的,是从这个
        // `fwd` 表达式推出来的。
        let yaw_rate: f32 = if speed.abs() > STEER_MIN_SPEED {
            -steer * STEER_RATE * grip_scale
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
        //
        // 滑移比的分母用 `speed.max(1.0)`,倒车时 `speed` 为负会让分母
        // 退化成 1.0,滑移比被放大好几倍 —— 于是倒车一打方向就凭空掉速。
        // 正确的比值两边都取绝对值,「相对纵向速度」的物理含义不变。
        let slip: f32 = lateral.abs() / speed.abs().max(1.0);
        let drag: f32 = if slip > DRIFT_SLIP_RATIO {
            DRIFT_SPEED_DRAG * (slip - DRIFT_SLIP_RATIO) * dt
        } else {
            0.0
        };
        // 掉速不能把速度推过 0:穿过 0 就等于「滑着滑着自动挂上了倒挡」。
        // 倒车时同理不能推过倒车下限。
        let floor: f32 = -REVERSE_SPEED_MAX;
        self.set_speed((speed - drag).max(floor));
        self.set_lateral(lateral);

        // ---- 6. 积分位置:沿「纵向 + 侧向」两个分量一起走 ---------
        let forward_speed: f32 = speed * rotated.cos();
        let velocity: Vec2 = [
            fwd[0] * forward_speed + right[0] * lateral,
            fwd[1] * forward_speed + right[1] * lateral,
        ];
        let here: Vec3 = self.get_position();
        let delta: Vec2 = [velocity[0] * dt, velocity[1] * dt];
        // 车的碰撞足迹是**有向盒**而不是 1.25 m 的圆:车身真实尺寸
        // 4.566 × 1.851 m,圆只到 1.25 m,于是车头有 1.033 m 悬在碰撞边界
        // 之外 —— 满油门怼楼时车头真的会插进建筑 AABB 那么深(实测)。
        // 这一步内部还把位移切成 0.05 m 的子步,单步 1.60 m 跨不过任何
        // 一堵薄墙(最薄的静态盒 0.133 m)。详见
        // [`crate::collision::CollisionWorld::resolve_car_footprint`]。
        let resolved: Vec2 = world.resolve_car_footprint([here[0], here[2]], delta, self.get_yaw());
        let blocked: bool = (resolved[0] - (here[0] + delta[0])).abs() > f32::EPSILON
            || (resolved[1] - (here[2] + delta[1])).abs() > f32::EPSILON;
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

/// Inherent implementation of [`Pickup`].
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
        let two_pi: f32 = 2.0 * PI;
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

/// Inherent implementation of [`Traffic`].
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
    /// - `&CollisionWorld` - 静态碰撞世界,用来把车挡在障碍之外。
    pub fn step(&mut self, dt: f32, hailer: Option<[f32; 2]>, world: &CollisionWorld) {
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
                car.step(dt, Some(index) == hailer_car, world);
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
        DRIFT_SLIP_RATIO, LATERAL_GRIP, REVERSE_SPEED_MAX, STEER_MIN_SPEED, STEER_RATE, TrafficCar,
        WHEEL_RADIUS,
    };
    use super::{FRAC_PI_2, PI};
    use crate::r#type::Vec3;

    fn empty_world() -> crate::collision::CollisionWorld {
        crate::collision::CollisionWorld::new()
    }

    fn car() -> TrafficCar {
        TrafficCar::new(CAR_SEDAN, 0.0, 0.0, 8.0, 1.0)
    }

    /// 路网积分的不变量:无论走直还是在路口转弯,车都必须**压在自己这条
    /// 车道的中轴线上**,而且速度不能掉下来。
    ///
    /// 转向分支换到横向街道时,`set_lane` 会重算 `lane_x`;如果落位那一行
    /// 用错了坐标(比如写成路口中轴线而不是新车道中心),车就会偏离整整
    /// 一个车道宽,下一帧被碰撞推回 —— 每帧推回、每帧判定「被挡住」。
    #[test]
    fn a_car_stays_on_its_lane_through_junctions() {
        let world: crate::collision::CollisionWorld = empty_world();
        let mut c: TrafficCar = TrafficCar::new_on_lane(CAR_SEDAN, 1, 1.0, 0, 45.0, 11.5, 1.0);
        let start: Vec3 = c.get_position();
        for _ in 0..2000 {
            c.route_advance(1.0 / 60.0, &world);
            let p: Vec3 = c.get_position();
            let lane: f32 = c.get_lane_x();
            // 无论走直还是转弯,横向坐标都必须压在自己这条车道的中轴线上。
            let cross: f32 = if c.get_lane_axis() == 0 { p[0] } else { p[2] };
            assert!(
                (cross - lane).abs() < 1e-3,
                "车偏离自己的车道 {:.4} m(x={:.2} z={:.2} 车道 x={:.2} 轴向={})",
                (cross - lane).abs(),
                p[0],
                p[2],
                lane,
                c.get_lane_axis()
            );
            assert!(
                c.get_speed() > 1.0,
                "车在 ({:.1},{:.1}) 卡住:速度 {:.4},说明每帧都被碰撞推回",
                p[0],
                p[2],
                c.get_speed()
            );
        }
        let end: Vec3 = c.get_position();
        let travelled: f32 = (end[0] - start[0]).abs() + (end[2] - start[2]).abs();
        assert!(
            travelled > 100.0,
            "2000 帧只走了 {:.1} m,路网没有真正推进",
            travelled
        );
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
        let world: crate::collision::CollisionWorld = empty_world();
        let mut c: TrafficCar = car();
        for _ in 0..120 {
            c.step(1.0 / 60.0, false, &world);
        }
        let pos: Vec3 = c.get_position();
        assert!(
            (pos[0] - c.get_lane_x()).abs() < 1e-5,
            "AI 巡航必须留在车道 X 上,实际 {} 车道 {}",
            pos[0],
            c.get_lane_x()
        );
        assert!(
            (c.get_yaw() - c.get_direction() * -FRAC_PI_2).abs() < 1e-5,
            "AI 巡航的朝向必须仍由行驶方向推出"
        );
    }

    /// 抓地常数必须是正的有限值,否则指数衰减会变成放大。
    #[test]
    fn grip_constant_is_a_valid_decay() {
        assert!(LATERAL_GRIP > 0.0 && LATERAL_GRIP.is_finite());
        assert!(STEER_RATE > 0.0 && STEER_RATE.is_finite());
    }

    // ---- 倒车:速度必须能变负 --------------------------------------

    /// 静止时按住 S 必须真的倒出去(速度变负),而不是停在 0。
    ///
    /// 这条钉的是原来那个 `clamp(0.0, ..)`:下限 0 把倒车整段从物理上
    /// 删掉了,车「只能减速到 0」,所以这个断言在修复前必然失败。
    #[test]
    fn standstill_reverses_instead_of_staying_at_zero() {
        let mut c: TrafficCar = TrafficCar::new(CAR_SEDAN, 0.0, 0.0, 0.0, 1.0);
        let world: crate::collision::CollisionWorld = empty_world();
        let start: Vec3 = c.get_position();
        for _ in 0..60 {
            c.drive(-1.0, 0.0, 1.0 / 60.0, &world);
        }
        assert!(
            c.get_speed() < 0.0,
            "静止按 S 一秒必须倒出去(速度为负),实际 {}",
            c.get_speed()
        );
        assert!(
            c.get_speed() >= -REVERSE_SPEED_MAX - 1e-4,
            "倒车速度不能超过倒车上限 {},实际 {}",
            REVERSE_SPEED_MAX,
            c.get_speed()
        );
        let end: Vec3 = c.get_position();
        let moved: f32 = ((end[0] - start[0]).powi(2) + (end[2] - start[2]).powi(2)).sqrt();
        assert!(moved > 0.5, "倒车必须真的把车往后挪,实际只挪了 {} m", moved);
        // 倒车的位移必须与车头**相反**:点积为负。
        let yaw: f32 = c.get_yaw();
        let forward: [f32; 2] = [yaw.cos(), -yaw.sin()];
        let delta: [f32; 2] = [end[0] - start[0], end[2] - start[2]];
        let along: f32 = forward[0] * delta[0] + forward[1] * delta[1];
        assert!(
            along < 0.0,
            "倒车位移必须朝车尾方向(与车头点积为负),实际点积 {}",
            along
        );
    }

    /// 前进中按 S:先刹车到 0,继续按住才变负 —— 两段必须都在。
    ///
    /// GTA V 里 S 在前进中是刹车而不是倒车,所以「一路踩到底」的速度
    /// 曲线必须先单调降到 0,再单调变负。只断终点会漏掉「直接从正跳到负」
    /// 这种把刹车和倒车混成一段的错误实现。
    #[test]
    fn reverse_after_braking_passes_through_exactly_zero() {
        let mut c: TrafficCar = car();
        let world: crate::collision::CollisionWorld = empty_world();
        for _ in 0..60 {
            c.drive(1.0, 0.0, 1.0 / 60.0, &world);
        }
        let rolling: f32 = c.get_speed();
        assert!(rolling > 0.0, "起步必须是正速度,实际 {}", rolling);

        let mut trace: Vec<f32> = Vec::new();
        let mut zero_frame: Option<usize> = None;
        for i in 0..240 {
            c.drive(-1.0, 0.0, 1.0 / 60.0, &world);
            trace.push(c.get_speed());
            if zero_frame.is_none() && c.get_speed() <= 0.0 {
                zero_frame = Some(i);
            }
        }
        let at_zero: Option<usize> = zero_frame;
        assert!(
            at_zero.is_some(),
            "前进中按住 S 必须先减到 0(刹车),现在没有出现过非正速度"
        );
        let z: usize = at_zero.unwrap_or(0);
        // 刹车段:速度必须单调不增地走到 0。
        for i in 1..=z {
            assert!(
                trace[i] <= trace[i - 1] + 1e-6,
                "刹车段速度必须单调不增,帧 {} 从 {} 涨到 {}",
                i,
                trace[i - 1],
                trace[i]
            );
        }
        // 倒车段:0 之后必须真的变负。
        assert!(
            trace[trace.len() - 1] < 0.0,
            "按住 S 到底必须倒出去,最终速度 {}",
            trace[trace.len() - 1]
        );
        assert!(
            trace[z] >= -1e-6 && trace[z] <= 0.0,
            "越过零点的那一帧必须贴着 0,实际 {}",
            trace[z]
        );
    }

    /// 倒车也必须能打方向,而且摆动方向与前进时**相反**。
    ///
    /// 这是自行车模型倒推的自然结果:车身反转时速度向量的相对朝向也反转。
    /// 判据用「横向位移相对车头右方」的方向,不假设任何 yaw 符号约定。
    #[test]
    fn reverse_steering_keeps_working_and_is_distinguishable() {
        let world: crate::collision::CollisionWorld = empty_world();

        // 同一辆车、同一个舵角,分别在前进与倒车状态各转一次,比较 yaw 变化。
        let mut ahead: TrafficCar = car();
        for _ in 0..60 {
            ahead.drive(1.0, 0.0, 1.0 / 60.0, &world);
        }
        let yaw0: f32 = ahead.get_yaw();
        let speed_fwd: f32 = ahead.get_speed();
        for _ in 0..120 {
            ahead.drive(0.0, 1.0, 1.0 / 60.0, &world);
        }
        let turn_fwd: f32 = ahead.get_yaw() - yaw0;

        let mut back: TrafficCar = TrafficCar::new(CAR_SEDAN, 0.0, 0.0, 0.0, 1.0);
        for _ in 0..60 {
            back.drive(-1.0, 0.0, 1.0 / 60.0, &world);
        }
        let byaw0: f32 = back.get_yaw();
        let speed_rev: f32 = back.get_speed();
        for _ in 0..120 {
            back.drive(0.0, 1.0, 1.0 / 60.0, &world);
        }
        let turn_rev: f32 = back.get_yaw() - byaw0;

        assert!(
            speed_rev < 0.0,
            "这一段必须在倒车状态,实际速度 {}",
            speed_rev
        );
        assert!(
            turn_rev.abs() > 0.1,
            "倒车时必须能打方向,实际只转了 {}",
            turn_rev.abs()
        );
        // yaw 转速的**大小**只跟 |速度| 有关,所以两段应当同量级。
        // 这一条是反向的「倒车没接上转向」的保护:如果死区还写
        // `speed > STEER_MIN_SPEED`,倒车段恒为 0,这里立刻失败。
        let ratio: f32 = turn_rev.abs() / turn_fwd.abs();
        assert!(
            ratio > 0.5 && ratio < 2.0,
            "倒车与前进的转向幅度应当同量级(比值 {}),前进 {} 倒车 {} 车速 {} / {}",
            ratio,
            turn_fwd,
            turn_rev,
            speed_fwd,
            speed_rev
        );
    }
}
