use super::*;

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
pub(crate) fn route_rand(seed: u32) -> u32 {
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
pub(crate) fn next_unit(seed: u32) -> f32 {
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
pub(crate) fn street_axis(index: i32) -> f32 {
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
pub(crate) fn street_index_at(along: f32) -> i32 {
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
pub(crate) fn lane_axis(street: i32, side: f32) -> f32 {
    street_axis(street) + side * LANE_OFFSET
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
    let notice: String = match *pickup.get_asset() {
        PICKUP_HEALTH_PACK => {
            let healed: f32 = (player.get_health() + HEALTH_PACK_HEAL).min(MAX_HEALTH);
            player.set_health(healed);
            String::from(NOTICE_HEALTH)
        }
        PICKUP_CASH_STACK => {
            player.set_cash_add(CASH_STACK_AMOUNT);
            String::from(NOTICE_CASH)
        }
        _ => format!("{NOTICE_WEAPON}{}", pickup.get_label()),
    };
    player.set_notice(notice);
    pickup.set_taken(true);
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(crate) fn empty_world() -> crate::collision::CollisionWorld {
        crate::collision::CollisionWorld::new()
    }

    pub(crate) fn car() -> TrafficCar {
        TrafficCar::new(CAR_SEDAN, 0.0, 0.0, 8.0, 1.0)
    }

    /// 路网积分的不变量:无论走直还是在路口转弯,车都必须**压在自己这条
    /// 车道的中轴线上**,而且速度不能掉下来。
    ///
    /// 转向分支换到横向街道时,`set_lane` 会重算 `lane_x`;如果落位那一行
    /// 用错了坐标(比如写成路口中轴线而不是新车道中心),车就会偏离整整
    /// 一个车道宽,下一帧被碰撞推回 —— 每帧推回、每帧判定「被挡住」。
    #[test]
    pub(crate) fn a_car_stays_on_its_lane_through_junctions() {
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

    /// 路网的抽签流**不能坍缩到常数 0**。
    ///
    /// 原实现 `get_route_seed()` 读出旧值之后写 0,而 `route_rand(0) == 0`
    /// —— 全零是这个哈希的不动点。于是流状态从第一次抽签之后就永远停在
    /// 0,`next_unit(0) == 0.0`,而路口判定是 `draw < ROUTE_PICK`(0.62),
    /// 于是**每一个路口都必然转向**,文档里写的「38% 直行」从来没发生过。
    ///
    /// 这条测试断言的是**行为**(直行与转向都真的会出现),而不是
    /// 「某个数字等于某个数字」—— 前者能在实现被重构时继续成立。
    #[test]
    pub(crate) fn junction_draws_vary_instead_of_collapsing_to_zero() {
        let mut c: TrafficCar = TrafficCar::new_on_lane(CAR_SEDAN, 1, 1.0, 0, 45.0, 11.5, 1.0);
        let mut turns: usize = 0;
        let mut straights: usize = 0;
        for _ in 0..256 {
            let draw: f32 = c.next_route_draw();
            assert!(
                draw > 0.0 && draw < 1.0,
                "抽签结果必须落在 [0,1),实得 {draw}:坍缩成 0 之后路口决策就是常量的"
            );
            if draw < ROUTE_PICK {
                turns += 1;
            } else {
                straights += 1;
            }
        }
        assert!(
            turns > 0 && straights > 0,
            "256 次抽签必须同时出现转向({turns})与直行({straights}):\
             只有一个分支命中就说明随机流又退化成常量了"
        );
    }

    /// 同一条车流轨迹必须**可重放**:同样的初始状态跑两遍,抽签序列逐项
    /// 相同。这是 `route_seed` 存在的唯一理由,也是它 doc comment 的承诺。
    #[test]
    pub(crate) fn junction_draws_are_replayable() {
        let mut a: TrafficCar = TrafficCar::new_on_lane(CAR_SEDAN, 1, 1.0, 0, 45.0, 11.5, 1.0);
        let mut b: TrafficCar = a.clone();
        for _ in 0..64 {
            assert_eq!(
                a.next_route_draw(),
                b.next_route_draw(),
                "同初始状态的两辆车抽签序列发散了:同一条轨迹无法重放"
            );
        }
    }

    /// 车必须真的在路口**转弯**,而不是 10 条永不交叉的直线轨道。
    ///
    /// 原判定是 `along >= street_axis(street_index_at(along) + 1)`,而
    /// `street_index_at` 是 `(along / 60).round()`,`along` 恒落在
    /// `[60k - 30, 60k + 30)` —— 比要求整整差一个 60 m 档。穷举
    /// 802,000 个坐标验证过:满足条件的坐标 **0 个**,所以
    /// `choose_at_junction()` 是死代码。
    ///
    /// 判定「真的转过」不看内部状态,只看**世界坐标里的行进轴**:
    /// 一辆沿 Z 走的车转过之后必须改沿 X 走。
    #[test]
    pub(crate) fn a_car_actually_changes_street_at_a_junction() {
        let world: crate::collision::CollisionWorld = empty_world();
        let mut c: TrafficCar = TrafficCar::new_on_lane(CAR_SEDAN, 1, 1.0, 0, 45.0, 11.5, 1.0);
        let start_axis: i32 = c.get_lane_axis();
        let mut turns_seen: usize = 0;
        let mut previous_axis: i32 = start_axis;
        for _ in 0..6000 {
            c.route_advance(1.0 / 60.0, &world);
            let axis: i32 = c.get_lane_axis();
            if axis != previous_axis {
                turns_seen += 1;
                previous_axis = axis;
            }
        }
        let p: Vec3 = c.get_position();
        assert!(
            turns_seen > 0,
            "6000 帧({:.0} m 行程)一次都没转弯,最终在 ({:.1},{:.1}) 轴向 {} —— \
             路口判定又是死代码了",
            p[0].abs() + p[2].abs(),
            p[0],
            p[2],
            c.get_lane_axis()
        );
    }

    /// 满舵直线行驶:车必须真的转出角度,而不是继续沿原方向直行。
    #[test]
    pub(crate) fn steering_turns_the_car() {
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
    pub(crate) fn steering_produces_lateral_slip() {
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
    pub(crate) fn steering_is_disabled_below_the_dead_zone() {
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
    pub(crate) fn wheel_spin_follows_travelled_distance() {
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
    pub(crate) fn drift_costs_forward_speed() {
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
    pub(crate) fn ai_cruise_still_holds_its_lane() {
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
    pub(crate) fn grip_constant_is_a_valid_decay() {
        assert!(LATERAL_GRIP > 0.0 && LATERAL_GRIP.is_finite());
        assert!(STEER_RATE > 0.0 && STEER_RATE.is_finite());
    }

    // ---- IDM 跟车:前车停住时必须真的刹车 --------------------------

    /// 前车停住 30 m 外,后车**必须减速**。
    ///
    /// 原来的模型是 `speed += (cruise - speed) * approach` —— 一个朝
    /// 巡航速度的指数逼近,**完全没有前车感知**。数值实测它在
    /// t = 2.60 s 直接撞上前车,一次都不减速。
    #[test]
    pub(crate) fn a_car_behind_a_stopped_car_brakes_instead_of_ramming_it() {
        let world: crate::collision::CollisionWorld = empty_world();
        let mut back: TrafficCar =
            TrafficCar::new_on_lane(CAR_SEDAN, 1, 1.0, 0, 60.0, CRUISE_SPEED, 1.0);
        // 前车:同一根车道轴、同向、停在正前方 30 m,速度 0。
        let mut front: TrafficCar = TrafficCar::new_on_lane(CAR_SEDAN, 1, 1.0, 0, 90.0, 0.0, 1.0);
        front.set_speed(0.0);
        let cars: Vec<TrafficCar> = vec![back.clone(), front.clone()];
        let (gap, delta_v): (f32, f32) = TrafficCar::leader_gap(&cars, 0);
        assert!(gap < f32::INFINITY, "后车必须看得见前车,实得间距 {gap}");
        assert!(
            delta_v > 0.0,
            "前车停着时速度差必须是正的(自己在动),实得 {delta_v}"
        );
        // 自由流(没有前车)时应当**加速**去追巡航速度,证明下面测到的
        // 减速确实来自 IDM 的前车项,不是别的东西。
        let free: f32 = TrafficCar::idm_accel(CRUISE_SPEED, CRUISE_SPEED, f32::INFINITY, 0.0);
        let blocked: f32 = TrafficCar::idm_accel(CRUISE_SPEED, CRUISE_SPEED, gap, delta_v);
        assert!(free >= 0.0, "无前车时不该减速,实得 {free}");
        assert!(
            blocked < -1.0,
            "前车停在 30 m 外,后车必须给出明显减速,实得 {blocked}"
        );
        // 真的跑一遍:带上前车推 5 秒,后车必须掉速。
        for _ in 0..300 {
            back.step(1.0 / 60.0, false, &world);
            let (g, dv): (f32, f32) = {
                let snapshot: Vec<TrafficCar> = vec![back.clone(), front.clone()];
                TrafficCar::leader_gap(&snapshot, 0)
            };
            back.set_speed(
                (back.get_speed()
                    + TrafficCar::idm_accel(back.get_speed(), CRUISE_SPEED, g, dv) / 60.0)
                    .max(0.0),
            );
        }
        let travelled: f32 = back.get_position()[2] - 60.0;
        assert!(
            back.get_speed() < CRUISE_SPEED * 0.6,
            "5 秒后后车必须明显掉速,实得 {} m/s(巡航 {CRUISE_SPEED})",
            back.get_speed()
        );
        assert!(
            travelled < 50.0,
            "5 秒行程 {travelled:.1} m 超过它以巡航速度能跑的距离 —— \
             说明它压根没减速"
        );
    }

    /// `Traffic::step` 必须真的调 IDM —— 上面几条测的是 `idm_accel` 和
    /// `leader_gap` **本身**,把车队层的调用换成旧 lerp 它们照样全绿
    /// (我第一版就是这么写的,发现方式是把这行改成 `0.0` 重跑)。
    /// 这条盯住**调用点**:车队每帧都必须对每辆没被开的车算一次跟车。
    #[test]
    pub(crate) fn traffic_step_applies_idm_to_every_undriven_car() {
        let source: String =
            crate::fs::read_to_string("src/traffic/impl.rs").expect("必须能读回自己的源文件");
        let start: usize = source
            .find("pub fn step(&mut self, dt: f32, hailer: Option<[f32; 2]>")
            .expect("Traffic::step 必须还在");
        let body: &str = source[start..]
            .split("\n    pub fn ")
            .next()
            .expect("能切出函数体");
        assert!(
            body.contains("idm_accel") && body.contains("leader_gap"),
            "Traffic::step 没有调用 IDM —— 跟车不会生效(P1)"
        );
        // 还要确保玩家开的那辆车不参与 AI 跟车(否则玩家自己会被减速)。
        assert!(
            body.contains("get_driven"),
            "Traffic::step 必须排除玩家开的车"
        );
    }

    /// 跟车稳态:队列里每辆车都保持正的净间距,不重叠。
    ///
    /// 队列必须留在**一条直道的一格内** —— 超过一格就会跨过路口,而
    /// 跨路口会真的转弯(`choose_at_junction`),于是沿街坐标不再单调,
    /// 「后车在前面」这个前提本身就不成立,测的就不是跟车了。
    #[test]
    pub(crate) fn a_car_following_keeps_a_positive_gap() {
        let world: crate::collision::CollisionWorld = empty_world();
        // 三辆车、一条直道、一格之内(街道间距 60 m)。
        //
        // 间距取 **9 m** 而不是稳态的 `s* ≈ 19.9 m`:`s*` 附近 IDM 会
        // 判成「间距刚好」,车继续跑巡航 —— 那是**正确**行为。所以要测
        // 「会减速」就必须排在**比 s* 更近**的位置上。
        //
        // 沿 +Z 行驶,所以**队首是 z 最大的那辆**:数组按 z 降序摆,
        // `cars[0]` 是队首(没有前车),`cars[2]` 是队尾。整队落在
        // (0, 60) 这一格里,离两侧的路口中轴线都有余量。
        let mut cars: Vec<TrafficCar> = Vec::new();
        for i in 0..3 {
            let z: f32 = 26.0 - i as f32 * 9.0;
            let mut c: TrafficCar =
                TrafficCar::new_on_lane(CAR_SEDAN, 1, 1.0, 0, z, CRUISE_SPEED, 1.0);
            c.set_speed(CRUISE_SPEED);
            cars.push(c);
        }
        // 队首(索引 0)没有前车,同样要走一遍 IDM,否则它会一直保持巡航
        // 速度而后车全在减速,量出来的「重叠」其实是队列被拉散。
        //
        // 只跑 **0.75 s**。路口中轴线在 z = 0 / 60 / 120;队首从 z=34
        // 起跑,0.75 s 后到 ~42,离 60 还有 18 m。这段窗口里 IDM 已经有
        // 足够帧数给出可观的减速,又不会跨过中轴线触发
        // `choose_at_junction` 的 `set_position` —— 那会把车搬到路口中心,
        // 沿街坐标不再单调,「谁在前谁在后」的前提就失效了。
        for _ in 0..45 {
            let order: Vec<usize> = (0..cars.len()).collect();
            for i in order {
                let snapshot: Vec<TrafficCar> = cars.clone();
                let (gap, dv): (f32, f32) = TrafficCar::leader_gap(&snapshot, i);
                let a: f32 = TrafficCar::idm_accel(cars[i].get_speed(), CRUISE_SPEED, gap, dv);
                let next: f32 = (cars[i].get_speed() + a / 60.0).max(0.0);
                cars[i].set_speed(next);
            }
            for i in (0..cars.len()).rev() {
                cars[i].route_advance(1.0 / 60.0, &world);
            }
        }
        // 判据用 `leader_gap` 自己报出来的净间距 —— 队列按 z 降序摆,
        // 所以「前面那辆」不是 `cars[i-1]`(它在后面)。手写 `cars[i-1]`
        // 的比较等于拿后车当前车,净间距必然为负,和 IDM 无关。
        for i in 0..cars.len() {
            let (gap, _dv): (f32, f32) = TrafficCar::leader_gap(&cars, i);
            if gap == f32::INFINITY {
                continue;
            }
            assert!(
                gap > 0.0,
                "第 {i} 辆车已经压到前车身上(净间距 {gap:.2} m)—— \
                 IDM 没有守住跟车距离"
            );
        }
        // 队尾必须真的**慢下来**:跟车的意义就在这里。
        let tail: f32 = cars[cars.len() - 1].get_speed();
        assert!(
            tail < CRUISE_SPEED * 0.9,
            "队尾必须因为前面有车而减速,实得 {tail:.2} m/s(巡航 {CRUISE_SPEED})"
        );
    }

    /// 无前车时 IDM 必须**收敛到巡航速度**,不能越冲越快。
    #[test]
    pub(crate) fn idm_converges_to_cruise_with_no_leader() {
        let mut speed: f32 = 0.0;
        for _ in 0..600 {
            let a: f32 = TrafficCar::idm_accel(speed, CRUISE_SPEED, f32::INFINITY, 0.0);
            speed = (speed + a / 60.0).max(0.0);
        }
        assert!(
            (speed - CRUISE_SPEED).abs() < 0.05,
            "无前车时应收敛到巡航 {CRUISE_SPEED},实得 {speed:.3}"
        );
    }

    /// `TrafficCar::step` 单独跑时不会看别人 —— 跟车必须由车队层做。
    /// 这条测试把两者的职责钉死,免得有人把 IDM 又塞回单车模型里。
    #[test]
    pub(crate) fn single_car_step_has_no_leader_awareness() {
        let world: crate::collision::CollisionWorld = empty_world();
        let mut solo: TrafficCar =
            TrafficCar::new_on_lane(CAR_SEDAN, 1, 1.0, 0, 60.0, CRUISE_SPEED, 1.0);
        for _ in 0..300 {
            solo.step(1.0 / 60.0, false, &world);
        }
        // 单车模型只朝巡航逼近,不该出现任何「因为前车而减速」的痕迹 ——
        // 这正是它当初漏掉跟车的原因。
        assert!(
            solo.get_speed() > CRUISE_SPEED * 0.9,
            "单车 step 不该做跟车减速,实得 {} m/s",
            solo.get_speed()
        );
    }

    // ---- 倒车:速度必须能变负 --------------------------------------

    /// 静止时按住 S 必须真的倒出去(速度变负),而不是停在 0。
    ///
    /// 这条钉的是原来那个 `clamp(0.0, ..)`:下限 0 把倒车整段从物理上
    /// 删掉了,车「只能减速到 0」,所以这个断言在修复前必然失败。
    #[test]
    pub(crate) fn standstill_reverses_instead_of_staying_at_zero() {
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
    pub(crate) fn reverse_after_braking_passes_through_exactly_zero() {
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
    pub(crate) fn reverse_steering_keeps_working_and_is_distinguishable() {
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
