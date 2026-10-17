use super::*;

/// Inherent implementation of [`TrafficCar`].
impl TrafficCar {
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

    /// 取当前流状态。
    ///
    /// **不要在这里把 `route_seed` 清零。** 原来的实现读出旧值之后写 0,
    /// 而 `route_rand(0) == 0`(全零是这个哈希的**不动点**:每一轮
    /// `wrapping_mul` / `xor-shift` 都保持 0),于是 `set_route_seed()`
    /// 把 0 洗回 0,流状态从第一次抽签之后就永远停在 0,
    /// `next_unit(0) == 0.0`。
    ///
    /// 后果实测:`next_route_draw()` 的序列是
    /// `0.581102, 0.000000, 0.000000, 0.000000, ...`,
    /// 而路口判定是 `draw < ROUTE_PICK`(0.62),于是**每一个路口都必然
    /// 转向** —— 文档里写的「38% 直行」从来没有发生过。
    ///
    /// 现在流状态是纯粹的 `route_rand` 递推:
    /// 读旧值 → 洗牌 → 写回,同一条轨迹仍然可重放(这正是它文档承诺的),
    /// 但不再坍缩。
    ///
    /// # Returns
    ///
    /// - `u32` - 本轮抽签使用的流状态。
    fn get_route_seed(&mut self) -> u32 {
        self.route_seed
    }

    /// 把流状态洗牌后写回。
    pub(crate) fn set_route_seed(&mut self) {
        self.route_seed = route_rand(self.route_seed);
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
    fn route_from_xz(x: f32, _z: f32, direction: f32) -> (i32, f32, i32, f32) {
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
            hailing_seconds: 0.0,
            blocked: false,
            route_street: street,
            route_side: side,
            route_axis: axis,
            route_seed: 0x7A11_u32 ^ street as u32 ^ (direction > 0.0) as u32,
            // 起始沿街坐标必须和 `position` 对齐,否则第一帧会拿 `last_along`
            // = 0 去比「有没有越过中轴线」,车一出生就抽一次签。
            last_along: start_z,
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
            hailing_seconds: 0.0,
            blocked: false,
            route_street: street,
            route_side: side,
            route_axis: axis,
            route_seed: 0x7A11_u32 ^ (street as u32).wrapping_mul(31) ^ side.to_bits(),
            last_along: start_along,
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
            if to_x_street < to_z_street { 0 } else { 1 }
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
        // 本文件的 yaw 约定是 `fwd = yaw_forward(yaw) = [sin yaw, cos yaw]`
        // (见 [`Self::drive`] 第 3 步),所以**朝向就是 `fwd` 要指的方向**:
        //
        // - 沿 **+Z** 行进 → `fwd = (0, 1)` → `atan2(0, 1) = 0`。
        // - 沿 **-Z** 行进 → `fwd = (0, -1)` → `atan2(0, -1) = PI`。
        // - 沿 **+X** 行进 → `fwd = (1, 0)` → `atan2(1, 0) = PI/2`。
        // - 沿 **-X** 行进 → `fwd = (-1, 0)` → `atan2(-1, 0) = -PI/2`。
        //
        // 旧写法 `-direction * FRAC_PI_2 / +direction * FRAC_PI_2` 是按
        // `fwd = [cos yaw, -sin yaw]` 那套(90°错的)约定推导的,在新的前向
        // 下会把沿 Z 行驶的车摆成沿 X —— 也就是**顺着车道横着开**。
        // 逆向的分支同样重要:`yaw = 0` 与 `yaw = PI` 指向相反,写成 0 会让
        // 一半的车倒着走(见 §负零号坑:`-0.0` 与 `0.0` 差的是 180°)。
        match (axis, direction >= 0.0) {
            (0, true) => 0.0,
            (0, false) => PI,
            (_, true) => FRAC_PI_2,
            (_, false) => -FRAC_PI_2,
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

    /// 累加车轮转角。
    ///
    /// # Arguments
    ///
    /// - `f32` - 本步新增的转角(弧度)。
    pub fn advance_wheel_spin(&mut self, delta: f32) {
        let current: f32 = self.get_wheel_spin();
        self.set_wheel_spin(current + delta);
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

    /// IDM(Intelligent Driver Model)的纵向加速度。
    ///
    /// `a = a_max · [1 − (v/v₀)^δ − (s*/s)²]`
    ///
    /// - `v₀` 是期望速度(巡航,或红灯前的 0)。
    /// - `s* = s0 + max(0, v·T + v·Δv / (2·√(a·b)))`,`Δv = v − v_lead`。
    /// - `s` 是与**同向同一根车道轴上的前车**的净间距;没有前车时取
    ///   `f32::INFINITY`,于是 `(s*/s)² = 0`,退化成自由流。
    ///
    /// 指数取 `IDM_DELTA = 4` 时 `s*` 在 `Δv < 0`(前车更快)时自动变大 ——
    /// 所以被加塞时会**主动减速让出空间**,而不是顶上去。
    ///
    /// # Arguments
    ///
    /// - `f32` - 本车当前速度(米/秒)。
    /// - `f32` - 期望速度(米/秒)。
    /// - `f32` - 与前车的净间距(米);`f32::INFINITY` 表示无前车。
    /// - `f32` - 与前车的速度差 `v − v_lead`(米/秒)。
    ///
    /// # Returns
    ///
    /// - `f32` - 加速度(米/秒²),已钳在 `[IDM_ACCEL_MIN, IDM_ACCEL_MAX]`。
    pub fn idm_accel(speed: f32, desired: f32, gap: f32, delta_v: f32) -> f32 {
        // 期望间距。`sqrt(a·b)` 只算一次。
        let dynamic: f32 = speed * delta_v / (2.0 * (IDM_ACCEL_MAX * IDM_BRAKE_COMFORT).sqrt());
        let wanted: f32 = IDM_MIN_GAP + (IDM_HEADWAY * speed + dynamic).max(0.0);
        // 自由流项。`v0 <= 0`(等红灯)时这一项归零,于是 `a = a_max·[1 − (s*/s)²]`
        // —— 前车停住就直接全力刹车,而不是慢慢逼近。
        let free: f32 = if desired > f32::EPSILON {
            (speed / desired).powf(IDM_DELTA)
        } else {
            1.0
        };
        // `gap <= 0` 表示已经贴上/重叠:比例项发散,直接给满刹车。
        //
        // 注意这里**不能**写 `gap.is_finite()`:没有前车时 `gap` 是
        // `f32::INFINITY`,而 `INFINITY.is_finite()` 是 **false**,于是会掉进
        // else 分支拿到 `interaction = 1.0` —— 也就是「全力刹车」。
        // 结果是空旷路段上的车反而全在刹车。这条判据必须是「间距够大就
        // 不产生作用」:`INFINITY > 1e-3` 成立,比值自然是 0。
        let interaction: f32 = if gap > 1e-3 {
            (wanted / gap).powi(2)
        } else {
            1.0
        };
        (IDM_ACCEL_MAX * (1.0 - free - interaction)).clamp(IDM_ACCEL_MIN, IDM_ACCEL_MAX)
    }

    /// 找出本车**正前方同车道**的最近一辆车,返回 (净间距, 速度差)。
    ///
    /// 只在**同一根车道轴**上看,而且要求前车和本车朝同一个方向走 ——
    /// 对向车不构成跟车对象,路口横向车也不算(那是路权问题,交给
    /// 红绿灯相位处理,不是 IDM)。
    ///
    /// 间距按**车头到车尾**算(减两个半车长),不是中心距:中心距在
    /// 贴住时会是 0,`s*` 的比例项发散,车会被顶成负加速度乱抖。
    ///
    /// # Arguments
    ///
    /// - `&[TrafficCar]` - 全车队(当前车也在里面,靠索引排除自己)。
    /// - `usize` - 本车在车队里的索引。
    ///
    /// # Returns
    ///
    /// - `(f32, f32)` - `(净间距, 速度差)`;前方没有车时净间距是
    ///   `f32::INFINITY`。
    pub fn leader_gap(cars: &[TrafficCar], me: usize) -> (f32, f32) {
        let Some(front) = cars.get(me) else {
            return (f32::INFINITY, 0.0);
        };
        let axis: i32 = front.get_lane_axis();
        let direction: f32 = front.get_direction();
        let here: Vec3 = front.get_position();
        let mine: f32 = if axis == 0 { here[2] } else { here[0] };
        let mut best: f32 = f32::INFINITY;
        let mut lead_speed: f32 = 0.0;
        for (index, other) in cars.iter().enumerate() {
            if index == me
                || other.get_lane_axis() != axis
                // 对向车道不跟。
                || other.get_direction() != direction
            {
                continue;
            }
            let at: Vec3 = other.get_position();
            let along: f32 = if axis == 0 { at[2] } else { at[0] };
            // 只看**前方**:同向同轴但落后于本车的不构成威胁。
            let ahead: f32 = (along - mine) * direction;
            if ahead <= 0.0 {
                continue;
            }
            // 净间距 = 中心距 − 半车长(本车)− 半车长(前车)。
            let clear: f32 = ahead - CAR_FOOTPRINT[0];
            if clear < best {
                best = clear;
                lead_speed = other.get_speed();
            }
        }
        (best, front.get_speed() - lead_speed)
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
        self.set_blocked(false);
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
        let resolved: Vec2 = world.resolve_car_footprint([here[0], here[2]], delta, self.get_yaw());
        self.set_position([resolved[0], here[1], resolved[1]]);
        if self.get_speed() > 0.0 {
            let moved: f32 = (resolved[0] - here[0]).abs() + (resolved[1] - here[2]).abs();
            let wanted: f32 = travel.abs();
            if moved + CAR_ROUTE_EPSILON < wanted {
                // 被完全挡住(或者一步都没走成):不推进路网状态,停在
                // 障碍前。速度砍到零让它重新加速而不是顶着墙磨。
                //
                // `blocked` 必须置位:车队层的 IDM 在**之后**才回写速度,
                // 它看不到这里发生过什么,只会看到「速度 0」,于是算出正
                // 加速度把速度拉回 0.067,下一帧又被挡回去 —— 永远 0 位移。
                self.set_speed(0.0);
                self.set_blocked(true);
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
        // 走过路口了吗?
        //
        // **判定必须比较本帧与上一帧的沿街坐标**,而不是「是否到达下一条
        // 街道的索引」。原实现是:
        //
        // ```text
        // junction = street_axis(street_index_at(along) + 1)
        // crossed  = along >= junction - EPS
        // ```
        //
        // 而 `street_index_at` 是 `(along / 60).round()`,所以 `along` 恒落在
        // `[60k - 30, 60k + 30)` 里,而判定要求 `along >= 60(k + 1)` —— 差整整
        // 一个 60 m 档。穷举 802,000 个坐标验证过:正向分支满足的坐标有
        // **0 个**,反向分支同样是 **0 个**。也就是说
        // `choose_at_junction()` 从来没在生产里被调用过,10 辆车是 10 条
        // 永不转弯的直线轨道。
        //
        // 现在改成真正的「越过中轴线」:本帧和上一帧分处中轴线两侧。
        // 用上一帧的位置而不是「距离中轴线够近」,是因为停在中轴线上
        // 的车不该反复触发路口决策。
        let axis_centre: f32 = street_axis(street_index_at(along));
        let previous: f32 = self.get_last_along();
        let crossed: bool = if direction >= 0.0 {
            previous < axis_centre && along >= axis_centre
        } else {
            previous > axis_centre && along <= axis_centre
        };
        self.set_last_along(along);
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
        let steer: f32 = steer.clamp(-1.0, 1.0);
        let speed: f32 = self.get_speed();
        let grip_scale: f32 = 1.0 / (1.0 + speed.abs() * STEER_SPEED_FALLOFF);
        // 死区按**速度的绝对值**判:倒车时 `speed` 是负的,但倒车一样要能
        // 打方向(倒库全靠它)。写 `speed > STEER_MIN_SPEED` 会让整段倒车
        // 永远转不动 —— 速度为负时那个判断恒假。
        //
        // **这里必须带负号 —— 「右」是屏幕上的右,不是叉乘写反的那个。**
        //
        // `steer` 的契约是「正 = 右转」(`steer_sign` 对 KEYD / ARROWRIGHT
        // 返回 +1),「右」的屏幕真值由第三人称相机锚定:相机跟在车尾沿
        // `fwd` 看时,画面右侧 = `cross(fwd, up)`(右手系)。前向
        // [`yaw_forward`](crate::orient::yaw_forward) = `[sin yaw, cos yaw]`
        // 对 yaw 求导:
        //
        //     d(fwd)/d(yaw) = [cos yaw, -sin yaw] = **−** yaw_right(yaw)
        //
        // 即 **yaw 增大 = 车头向自己的左方转**,所以前进右转要
        // `yaw_rate = −steer * K`。上一版把 `cross(up, fwd)`(左)当成了
        // 「右」,于是按 D 车往屏幕左拐 —— 用户实测「左右反了」。
        //
        // 再乘 `speed.signum()`:bicycle model 的偏航率正比于**带符号**
        // 速度(`v·tanδ/L`)。倒车打同一把舵,车身旋转方向与前进相反 ——
        // 真车如此,GTA 也如此:倒车按 D,车尾甩向屏幕右、车头摆左。
        let yaw_rate: f32 = if speed.abs() > STEER_MIN_SPEED {
            -steer * STEER_RATE * grip_scale * speed.signum()
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
        // **前向用 [`yaw_forward`](crate::orient::yaw_forward)。**
        //
        // 本文件过去在这里写 `fwd = [cos yaw, -sin yaw]`,而那正是**渲染
        // 出来的资产本地 +X**:实测车辆资产的引擎盖 `z=+1.380`、前保险杠
        // `z=+2.330`、前大灯 `z=+2.310`,车头在本地 **+Z**;而
        // `Instance::new` 把本地 +Z 映射到世界 `(sin yaw, cos yaw)`。
        // 两者点积恒为 0(`sin·cos + cos·(-sin) = 0`),也就是**物理前进
        // 方向和画出来的车头差了整整 90°** —— 车横着开。
        //
        // `yaw_forward` 与渲染矩阵的 `model[8], model[10]` 是同一对分量,
        // 与玩家的 `facing_yaw(atan2(vx, vz))` 也是同一个,所以物理、
        // 渲染、角色三方终于指向同一个车头。
        let fwd: Vec2 = crate::orient::yaw_forward(yaw);
        let right: Vec2 = crate::orient::yaw_right(yaw);
        let rotated: f32 = yaw_rate * dt;
        // 旧车身系的速度投影 → 新车身系:前向多出 `speed * cos(rotated)`,
        // 侧向多出 `speed * sin(rotated)`。符号推导:屏幕右约定下
        // `d(fwd)/d(yaw) = −yaw_right`,车体转过 `rotated` 后,原来沿
        // `fwd_old` 的速度在新车身系的 right 轴上分量为
        // `speed * sin(rotated)` —— 右转(`rotated < 0`)时该分量为负,
        // 即车尾向弯**外**侧滑,物理正确;符号取反车会往弯心里吸。
        let swung: f32 = speed * rotated.sin();
        let lateral: f32 = self.get_lateral() + swung;
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

    /// **按玩家位置重铺车队**(GTA 的生成环 + 回收重生)。
    ///
    /// [`Self::populate`] 只在开局跑一次,而 [`crate::game::dense_traffic_lanes`]
    /// 现在依赖玩家位置(生成环)、朝向(方向门)与车速(半径倍率)—— 这三者
    /// 全程在变,所以车道表必须跟着重算,否则玩家开出一段距离之后,车流
    /// 就永远停在他出生时的那个环上。
    ///
    /// GTA 的做法不是每帧重建:`CPopulation::ManagePopulation` 扫一遍对象
    /// 池,超出 [`crate::traffic::PED_REMOVE_DIST`] 的回收,可见范围内的
    /// 不足才重新生成。这里同样**只在玩家跨过一个街格时才重铺** ——
    /// 街格是 60 m,11.5 m/s 下每 5 秒一次,远小于「每帧重建 500 辆车」的成本。
    ///
    /// 重铺会**保留现有车辆的位置**,只增删:现有车辆已经在路上跑了,
    /// 推倒重来会让整支车队的相位被重置(全城车突然齐刷刷变道)。
    /// 判据是「不在新环带内且够远」的车被移除,「在环带内但缺失」的槽位
    /// 补新车 —— 于是车队是**渐变**的,不是闪烁的。
    ///
    /// # Arguments
    ///
    /// - `[f32; 2]` - 玩家 `[x, z]`。
    /// - `f32` - 玩家面朝方向的偏航角(弧度)。
    /// - `f32` - 玩家水平速度(米/秒)。
    /// - `&[(&'static str, f32, f32, f32)]` - 车道蓝图,由
    ///   [`crate::game::traffic_lanes`] 按上面三个量算出。
    ///
    /// # Returns
    ///
    /// - `bool` - 车队是否真的变了(没变时不触发任何重活)。
    pub fn respawn_around(
        &mut self,
        player: [f32; 2],
        player_yaw: f32,
        speed: f32,
        lanes: &[(&'static str, f32, f32, f32)],
    ) -> bool {
        // 现有车辆按 (车道 X, 沿街 Z, 方向) 建一个占用表,避免同槽位
        // 反复补出两辆并排的车。
        let mut taken: Vec<(f32, f32, f32)> = self
            .get_cars_ref()
            .iter()
            .map(|car: &TrafficCar| {
                let here: Vec3 = car.get_position();
                let along: f32 = if car.get_lane_axis() == 0 {
                    here[2]
                } else {
                    here[0]
                };
                (car.get_lane_x(), along, car.get_direction())
            })
            .collect();
        let mut added: Vec<TrafficCar> = Vec::new();
        for (asset, lane_x, start_z, direction) in lanes {
            if taken.iter().any(|(x, z, d): &(f32, f32, f32)| {
                (x - lane_x).abs() < 6.0
                    && (z - start_z).abs() < 12.0
                    && (d - direction).abs() < 0.1
            }) {
                continue;
            }
            added.push(TrafficCar::new(
                asset,
                *lane_x,
                *start_z,
                CRUISE_SPEED.clamp(SPEED_MIN, SPEED_MAX),
                *direction,
            ));
        }
        if added.is_empty() {
            return false;
        }
        let cars: &mut Vec<TrafficCar> = self.get_cars_mut();
        cars.extend(added);
        // 硬上限:补进来之前先扔掉最远的,免得分片与渲染的每帧开销随
        // 玩家漫游时长无限增长。
        if cars.len() > MAX_CARS {
            let here: Vec3 = cars[0].get_position();
            cars.sort_by(|a: &TrafficCar, b: &TrafficCar| {
                let pa: Vec3 = a.get_position();
                let pb: Vec3 = b.get_position();
                let da: f32 = (pa[0] - here[0]).powi(2) + (pa[2] - here[2]).powi(2);
                let db: f32 = (pb[0] - here[0]).powi(2) + (pb[2] - here[2]).powi(2);
                da.partial_cmp(&db).unwrap_or(Ordering::Equal)
            });
            cars.truncate(MAX_CARS);
        }
        true
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
    /// - `f32` - 全局时间(秒),信号机相位用它当时钟。
    /// - `u32` - 帧计数,只用于 [`manage_shard`] 的 32 帧分片;物理与
    ///   跟车的结果不依赖它,只影响「这一帧哪些车参与跟车」。
    pub fn step(
        &mut self,
        dt: f32,
        hailer: Option<[f32; 2]>,
        world: &CollisionWorld,
        time: f32,
        frame: u32,
    ) {
        // 只有**离玩家最近的那一辆**会靠边等人,不是半径内所有车 ——
        // 否则 26 m 内整条街的车一起停,交通直接堵死。
        let hailer_car: Option<usize> = hailer.and_then(|who: [f32; 2]| {
            let mut best: Option<(usize, f32)> = None;
            for (index, car) in self.get_cars_ref().iter().enumerate() {
                if car.get_driven() {
                    continue;
                }
                // **停靠计时超时的车不再参与竞争。**
                //
                // 原来每帧都重选「26 m 内最近的一辆」,唯一排除是
                // `get_driven()`。于是它停下之后下一帧仍然最近,继续被
                // 选中,`desired = 0` 一直施加 —— 它**永远不会**开走,整条
                // 车道被一辆停着的车永久占住。
                //
                // 实测(生产出生点 + 玩家固定站在出生点招手,跑 300 s):
                // car7 在第 4 个 30 s 窗口之后**连续六个窗口位移精确
                // 0.00 m**,卡死在 `(33.84, 56.50)`、速度 `0.000`,玩家就在
                // 11.51 m 外 —— 它把整条街堵了 180 s。
                //
                // 判据用 `hailing_seconds` 而不是速度:按速度判会在**刹车
                // 途中**掉出选择(速度降过阈值),于是 `desired` 又变回巡航,
                // 车重新加速、再被选中、再刹车 —— 永远停在玩家旁边抖动。
                // 计时只在真的**停稳等客**时累加,一旦开走就清零。
                if car.get_hailing_seconds() >= HAUL_PARKED_SECONDS {
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
                let is_hailer: bool = Some(index) == hailer_car;
                car.step(dt, is_hailer, world);
                // 停靠计时:只有**真的停住等客**才累加,开走就清零。
                //
                // 计时用「速度已经接近零」而不是「本帧是 hailer」—— 刹车
                // 途中也可能是 hailer,但那时候车还在动,不该算在等客时间里。
                // 清零用 `min`,于是中途取消(玩家走远 / 上了车)立刻归零。
                let stopped: bool = car.get_speed() <= HAIL_PARKED_SPEED;
                if is_hailer && stopped {
                    let waited: f32 = car.get_hailing_seconds() + dt;
                    car.set_hailing_seconds(waited);
                } else {
                    car.set_hailing_seconds(car.get_hailing_seconds().min(dt));
                }
            }
        }
        // **跟车**在纵向积分**之后**统一做一遍。
        //
        // 必须分两步:`TrafficCar::step` 里的速度是各自按巡航逼近的,
        // 谁也没看别人。IDM 需要**同一条线上前车已经算好的速度** ——
        // 一起推进去,队列里越靠后的车看到的越是上一帧的旧速度。
        //
        // 顺序也有讲究:从队首(沿街坐标最靠前)往后算,于是每辆车看到的
        // 前车速度都是**本帧**的,不会出现整条队列一起抖。
        //
        // `sort_key` 单独具名而不是内联闭包:闭包捕获 `self` 就不能写成
        // `fn(usize) -> f32`,而 §5.1 要求 `let` 必须带显式类型标注。
        //
        // 切片借用必须**在循环外就结束**:`get_car_mut` 要 `&mut self`,
        // 而持有 `&[TrafficCar]` 会一直占着不可变借用。所以这里先在同一个
        // 快照上把每辆车的目标加速度全部算完,再单独回写速度。
        //
        // 这是 IDM 的**同步更新**(论文里的标准写法):所有车都基于同一帧的
        // 前车状态决策,于是结果与数组顺序无关,也就不会有「同一支车队
        // 换个下标就走出不同轨迹」的不确定性。代价是后车看到的前车速度
        // 晚一帧 —— 1/60 s,在 `T = 1.5 s` 的时间间隔面前可以忽略。
        let planned: Vec<(usize, f32)> = {
            let cars: &[TrafficCar] = self.get_cars_ref();
            let sort_key: &dyn Fn(usize) -> f32 = &|index: usize| -> f32 {
                let car: &TrafficCar = &cars[index];
                let at: f32 = if car.get_lane_axis() == 0 {
                    car.get_position()[2]
                } else {
                    car.get_position()[0]
                };
                at * car.get_direction()
            };
            // **32 帧分片**(GTA `CPopulation::ManagePopulation` 的
            // `frameMod32 = GetFrameCounter() & 31`)。
            //
            // 跟车是 O(n²):500 辆车时每帧要做 25 万次「谁在我前面」的比较。
            // GTA 把这一层摊到 32 帧上,每帧只扫 1/32 的对象池。这里照做,
            // 但分片键是 `(index + frame) % 32` 而不是 `frame % 32` ——
            // 后者会让同一帧里**所有车都不更新跟车**,车队整体「抖一下、
            // 再抖一下」。前者让每辆车落在不同相位,任何一帧都只有均匀的
            // 1/32 在跟车,画面上看不出有卡顿的车。
            //
            // 代价:每辆车每 32 帧(60 fps 下 0.53 s)才跟一次前车。
            // IDM 的车头时间间隔 `IDM_HEADWAY` 是 1.2 s,分片延迟只占
            // 44%,远小于一次正常加减速。
            let mut order: Vec<usize> = (0..cars.len())
                .filter(|index: &usize| !cars[*index].get_driven())
                .filter(|index: &usize| manage_shard(*index, frame))
                .collect();
            order.sort_by(|a: &usize, b: &usize| {
                sort_key(*b)
                    .partial_cmp(&sort_key(*a))
                    .unwrap_or(Ordering::Equal)
            });
            order
                .into_iter()
                .map(|index: usize| {
                    let (mut gap, mut delta_v): (f32, f32) = TrafficCar::leader_gap(cars, index);
                    let car: &TrafficCar = &cars[index];
                    let mut desired: f32 = if Some(index) == hailer_car {
                        0.0
                    } else {
                        // **弯道减速**(GTA `CCarAI::ModifySpeed`)。
                        //
                        // GTA 按当前路段与下一路段的**夹角**缩放通过时间:
                        // 直路全速,进弯前收油。本项目没有节点图,但路口转向
                        // 同样是 90° 折,所以用转向瞬间甩出的**横向速度**当
                        // 夹角代理 —— 它已经是本项目车身上现成的量,不必再加
                        // 一个「下一次是否转向」的预判字段。
                        car.get_cruise() * corner_speed_scale(car.get_lateral())
                    };
                    // **红绿灯当作「一辆停在停止线上的前车」。**
                    //
                    // 不新增一套减速逻辑,而是把停止线塞进 IDM 已有的
                    // `gap` / `delta_v`:`delta_v = 自己速度 − 0` 就是
                    // 「前面有东西」的完整语义,`s*` 会随距离变大而迅速
                    // 收紧,车于是平顺减速停在线前。绿灯亮起后停止线消失,
                    // 同一个 `gap` 变回 `INFINITY`,车自动起步。
                    //
                    // **期望速度不是一步压到 0,而是按距离反解的限速。**
                    // 一步压到 0 会让 IDM 自由流项直接消失、加速度钉在
                    // `IDM_ACCEL_MIN = -9`,于是 11.5 m/s 的车用 9 m/s²
                    // 硬刹 —— 实测从 z=44 冲到 z=51.92,**越过停止线 1.2 m**。
                    // 限速 `sqrt(2·b·gap)` 让加速度随接近程度平滑增长,
                    // 车会恰好停在线上。
                    let to_line: f32 = distance_to_stop_line(car);
                    // 绿灯不限速;红灯时限速到「还停得住」的速度。
                    let cap: f32 = signal_speed_limit(car, time, to_line);
                    // **停止线只在红灯时才是障碍。**
                    //
                    // 无条件把它塞进 `gap` 会让绿灯也起不来:车停下后
                    // gap = 2.67、`s*` = 2.675,`interaction` ≈ 1.00,
                    // 于是 `a = 4·(1 − 0 − 1.00) = −0.016` —— 绿灯下
                    // 车以 0.05 m/s **原地蹭**,永远越不过停止线。
                    // 绿灯时这根线根本不存在。
                    if cap < desired {
                        desired = cap;
                        if to_line < gap {
                            gap = to_line;
                            delta_v = car.get_speed();
                        }
                    }
                    (
                        index,
                        TrafficCar::idm_accel(car.get_speed(), desired, gap, delta_v),
                    )
                })
                .collect()
        };
        // 回写按算出来的顺序来,保证同一帧的决策顺序可复现。
        for (index, accel) in planned {
            if let Some(car) = self.get_car_mut(index) {
                // **被静态几何挡下的车,IDM 不许把它重新加速。**
                //
                // `route_advance` 已经把这类车的速度砍到 0 并 `return`,那是
                // 「前面有墙,先停下」的意思。可 IDM 看到的是「速度 0、
                // 同向前车 49 m 外、不在红灯前」——`gap = INFINITY`、
                // `interaction = 0`、`free = (0 / 11.5)⁴ = 0`,于是
                // `a = A·(1 − 0 − 0) = +4 m/s²`,把速度拉回 0.067。
                // 下一帧车又顶着同一个障碍、再被砍到 0,于是永远循环。
                //
                // 线上症状:探针读出 `carSpeed = 0.067`,而位置连续 80 s
                // **精确 0.00 m** —— 位置和速度互相矛盾,速度是 IDM 刚写下的
                // 那个还没来得及被碰撞消耗掉的中间值。
                if car.get_blocked() {
                    car.set_speed(0.0);
                    continue;
                }
                let next: f32 = (car.get_speed() + accel * dt).max(0.0);
                car.set_speed(next.min(SPEED_MAX));
            }
        }
        for index in 0..self.get_pickups_ref().len() {
            if let Some(pickup) = self.get_pickup_mut(index) {
                pickup.set_spin_advance(dt);
            }
        }
    }
}
