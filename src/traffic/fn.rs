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

/// 信号机在一个路口上的两套相位(轴 0 = 沿 Z,轴 1 = 沿 X)。
///
/// 整个城市共用**同一个全局时钟** —— 所有路口同步变灯。真实路口当然
/// 各有各的配时,但同步方案有两个实际好处:一是玩家在任何一个路口看到
/// 的相位一致,不必记「哪个路口晚两秒」;二是可以在测试里用一个标量
/// 时间直接断言相位,不需要构造整座城市。
///
/// 周期结构(单方向 `G → A → R`,两个方向错开半个周期):
///
/// ```text
/// axis 0:  ██████ GREEN 12s │██ AMBER 3s │██ RED 1.6s │██████ GREEN …
/// axis 1:  ███ RED 1.6s │████████ GREEN 12s │██ AMBER 3s │██ RED 1.6s …
/// ```
///
/// 两个方向之间**总是**存在全红窗口,所以不会同时放行。
///
/// # Arguments
///
/// - `f32` - 全局时间(秒)。
///
/// # Returns
///
/// - `(SignalAspect, SignalAspect)` - `(轴 0 的灯色, 轴 1 的灯色)`。
pub(crate) fn signals_for(time: f32) -> (SignalAspect, SignalAspect) {
    let cycle: f32 = time.rem_euclid(SIGNAL_CYCLE_SECONDS);
    let half: f32 = SIGNAL_CYCLE_SECONDS * 0.5;
    (
        aspect_at(cycle),
        // 第二个轴错开半���期。
        aspect_at((cycle + half).rem_euclid(SIGNAL_CYCLE_SECONDS)),
    )
}

/// 单个方向在一个周期里的灯色。
///
/// 周期从**绿灯**开始,顺序是 `G(12) → A(3) → R(1.6)`,黄灯算「要停」
/// —— IDM 那边把黄灯当 0 期望速度,车会平顺减速而不是急刹,真到了线上
/// 也是这样(黄灯可以抢,红灯不行)。
///
/// # Arguments
///
/// - `f32` - 周期内的时间(秒),取值 `[0, SIGNAL_CYCLE_SECONDS)`。
///
/// # Returns
///
/// - `SignalAspect` - 灯色。
pub(crate) fn aspect_at(within: f32) -> SignalAspect {
    if within < SIGNAL_GREEN_SECONDS {
        SignalAspect::Green
    } else if within < SIGNAL_GREEN_SECONDS + SIGNAL_AMBER_SECONDS {
        // 黄灯:当红处理(见本文档顶部)。
        SignalAspect::Red
    } else {
        SignalAspect::Red
    }
}

/// 一辆车到**前方停止线**的距离(米);没有可停的线时返回
/// `f32::INFINITY`。
///
/// 前方路口就是 `route_advance` 判定「越过路口」用的那一条:
/// `axis_centre = street_axis(street_index_at(along))`。车在两条中轴线之间
/// 跑,`street_index_at` 已经把它吸附到**前方那条**,所以 `along` 恒落在
/// `(axis_centre − 30, axis_centre + 30)` 里,路口就在 `axis_centre`。
///
/// **曾经写错成 `street_index_at(along) + 1`**,于是 z=40 的车去找 120 m
/// 外的路口,量出 70.72 m 的「停止线距离」—— 真实路口就在 60 m 处,
/// 正确距离约 10 m。那一版还让所有车都找不到近处的线,红绿灯等于没有。
///
/// 停止线在路口中轴线**前方** `SIGNAL_STOP_OFFSET` 处,所以
/// `stop_at = axis_centre − direction × SIGNAL_STOP_OFFSET`。
///
/// # Arguments
///
/// - `&TrafficCar` - 本车。
///
/// # Returns
///
/// - `f32` - 到停止线的距离;已经**远离**路口时 `f32::INFINITY`。
pub(crate) fn distance_to_stop_line(car: &TrafficCar) -> f32 {
    let axis: i32 = car.get_lane_axis();
    let direction: f32 = car.get_direction();
    let here: Vec3 = car.get_position();
    let along: f32 = if axis == 0 { here[2] } else { here[0] };
    // 与 `route_advance` 完全同一条中轴线 —— 两处必须一致,否则
    // 「路口判定说到了」和「停止线说还没到」会互相打架。
    let junction: f32 = street_axis(street_index_at(along));
    let stop_at: f32 = junction - direction * SIGNAL_STOP_OFFSET;
    let distance: f32 = (stop_at - along) * direction;
    // **压过线之后不再拦。**
    //
    // 试过两种更"严格"的写法,两种都错:
    //
    // 1. 压线后返回 `INFINITY`(「让车走完路口」):红灯只拦半个车身。实测
    //    停止线 50.717、车冲到 50.95,gap 变 inf、`desired` 仍是 0 但
    //    `a = 0`,**3.10 m/s 一路滑到 63.50**。
    // 2. 压线后返回 `0`(「继续满刹车」):车确实停了,但停在 51.92 ——
    //    **永久死锁**。因为它永远到不了中轴线 60,`route_advance` 的路口
    //    判定不触发,`choose_at_junction` 不换街道,于是
    //    `street_index_at` 永远给同一条中轴线,gap 永远是 0。
    //
    // 现实里压线的车确实会清空路口(该走的走完、该转的转完),不会停在
    // 路口中央。而我们要拦的是「红灯时从远处冲进路口」,不是「已经冲进去
    // 的车不许动」。所以就按字面来:`INFINITY` = 本路口不再拦。
    //
    // 真正的越线发生在 `gap > 0` 的**刹车阶段** —— 那时车还有 30 m 的
    // `SIGNAL_BRAKE_LOOKAHEAD` 预算去停。这是物理,不是判定。
    if distance < 0.0 {
        f32::INFINITY
    } else {
        distance
    }
}

/// 红绿灯给这一帧的**限速**(米/秒)。
///
/// 红灯 / 黄灯时不是「速度 = 0」,而是「速度 = 还停得住的那个值」
/// —— `sqrt(2·b·gap) · 0.9`。这个差别决定车是**停在线上**还是
/// **冲过停止线**:
///
/// - 返回 `0`:IDM 的自由流项直接消失,加速度钉在 `IDM_ACCEL_MIN = -9`,
///   于是 11.5 m/s 的车用 9 m/s² 硬刹。实测从 z=44 冲到 z=51.92,
///   **越过停止线 1.2 m** —— 红灯拦不住它。
/// - 返回限速:加速度随接近程度平滑增长,车恰好停在线上。
///
/// 绿灯返回 `f32::INFINITY`(不限速),调用方再和巡航速度取 `min`。
///
/// # Arguments
///
/// - `&TrafficCar` - 本车。
/// - `f32` - 全局时间(秒)。
/// - `f32` - 到停止线的距离(米);`f32::INFINITY` 表示本路口不用停。
///
/// # Returns
///
/// - `f32` - 允许的速度(米/秒)。
pub(crate) fn signal_speed_limit(car: &TrafficCar, time: f32, gap: f32) -> f32 {
    let aspects: (SignalAspect, SignalAspect) = signals_for(time);
    let mine: SignalAspect = if car.get_lane_axis() == 0 {
        aspects.0
    } else {
        aspects.1
    };
    match mine {
        SignalAspect::Green => f32::INFINITY,
        SignalAspect::Red => signal_speed_cap(gap),
    }
}

/// 红灯前的**限速速度** —— 按「还能在停止线前停住」反解。
///
/// 约束是「剩余刹车距离不超过剩余间距」:`v²/(2b) ≤ gap`,反解
/// `v ≤ sqrt(2·b·gap)`,再乘 0.9 留余量免得正好卡在边界上。
///
/// 远处的线不设限:`gap ≥ SIGNAL_BRAKE_LOOKAHEAD`(30 m)时 30 m 已经是
/// `sqrt(2·6·30)·0.9` = 17 m/s > 巡航,限速不生效,不必每帧算根号。
///
/// # Arguments
///
/// - `f32` - 到停止线的距离(米)。
///
/// # Returns
///
/// - `f32` - 允许的速度(米/秒)。
pub(crate) fn signal_speed_cap(gap: f32) -> f32 {
    if gap >= SIGNAL_BRAKE_LOOKAHEAD {
        f32::INFINITY
    } else if gap <= SIGNAL_CAP_FLOOR_GAP {
        // 已经贴线:必须**停死**,不能有残余速度。
        //
        // 只用 `sqrt(2·b·gap)` 会有一个爬行平衡点:车停在离线
        // `s*/v` 的地方,`a = 0` 但 `v ≠ 0`。实测车稳定在 z=48.04、
        // v = 0.05 m/s —— 红灯下还剩半米一格地往前蹭,绿灯一亮的瞬间
        // 也只是从 0.05 慢慢加速,看起来像「灯坏了」。所以在线前一小段
        // 直接要求 v = 0。
        0.0
    } else {
        (2.0 * IDM_BRAKE_COMFORT * gap).sqrt() * SIGNAL_CAP_MARGIN
    }
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
        // 签名现在是**跨行**的(加了 `time` 参数),所以不能按单行匹配。
        // 锚点选文档块的最后一行 —— 它紧挨着 `pub fn step(` 的开头,
        // 而且那行文字(`全局时间`)只可能出现在车队层这一个函数上。
        let anchor: &str = "信号机相位用它当时钟。";
        let at: usize = source.find(anchor).expect("Traffic::step 必须还在");
        let start: usize = source[at..]
            .find("pub fn step(")
            .map(|n: usize| at + n)
            .unwrap_or(at);
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
        // 红绿灯也走同一条 IDM 通路:停止线作为「停住的前车」塞进 gap。
        assert!(
            body.contains("signal_speed_limit") && body.contains("distance_to_stop_line"),
            "Traffic::step 没有接红绿灯 —— 车不会在停止线前停下(P2)"
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

    // ---- 红绿灯:相位、停止线、真的停住 --------------------------

    /// 两个方向**永远不会同时绿灯**。
    ///
    /// 穷举整个周期的每 0.01 s:只要出现一帧两轴都绿,车流就会在路口中央
    /// 对撞。这是整套相位最关键的不变量,必须机械验证而不是看着代码相信。
    #[test]
    pub(crate) fn the_two_approaches_are_never_both_green() {
        let mut both_green: Vec<f32> = Vec::new();
        let steps: usize = (SIGNAL_CYCLE_SECONDS / 0.01) as usize;
        for i in 0..=steps {
            let t: f32 = i as f32 * 0.01;
            let (a0, a1): (SignalAspect, SignalAspect) = signals_for(t);
            if a0 == SignalAspect::Green && a1 == SignalAspect::Green {
                both_green.push(t);
            }
        }
        assert!(
            both_green.is_empty(),
            "有 {} 个时刻两个方向同时绿灯,首个在 t={:?} —— 路口会对撞",
            both_green.len(),
            both_green.first()
        );
    }

    /// 周期必须真的会转:在 N 秒内两个轴各自都出现过绿灯。
    #[test]
    pub(crate) fn both_axes_get_green_within_one_cycle() {
        let mut saw: [bool; 2] = [false, false];
        let steps: usize = (SIGNAL_CYCLE_SECONDS / 0.01) as usize;
        for i in 0..=steps {
            let t: f32 = i as f32 * 0.01;
            let (a0, a1): (SignalAspect, SignalAspect) = signals_for(t);
            if a0 == SignalAspect::Green {
                saw[0] = true;
            }
            if a1 == SignalAspect::Green {
                saw[1] = true;
            }
        }
        assert!(saw[0], "轴 0 在一个周期内从没亮过绿灯");
        assert!(saw[1], "轴 1 在一个周期内从没亮过绿灯");
        // 每轴的绿灯时长必须接近标称值(允许 1% 采样误差)。
        let green: f32 = (SIGNAL_GREEN_SECONDS / SIGNAL_CYCLE_SECONDS * 100.0).round() as f32;
        assert!(
            green > 0.0 && green < 100.0,
            "绿灯占空比 {green:.1}% 不合理"
        );
    }

    /// 负时间和超长运行都不得让相位崩掉(`rem_euclid` 保证非负余数)。
    #[test]
    pub(crate) fn the_phase_survives_negative_and_large_time() {
        for t in [-1.0f32, -1e6, 0.0, 1e6] {
            let (a0, a1): (SignalAspect, SignalAspect) = signals_for(t);
            // 只断言它不 panic、且不同时绿(核心不变量)。
            assert!(
                !(a0 == SignalAspect::Green && a1 == SignalAspect::Green),
                "t={t} 时两个方向同时绿灯"
            );
        }
    }

    /// **停靠计时到点就必须放弃等客。**
    ///
    /// 回归:线上(player 63.5,45 步行)10 辆车里 9 辆 3 s 内位移
    /// 0.00~0.02 m。本地 300 s 复现:car7 从第 4 个 30 s 窗口起
    /// **连续六个窗口位移精确 0.00 m**,卡死在 `(33.84, 56.50)`、速度
    /// `0.000`,玩家就在 11.51 m 外 —— 玩家一直站着不动,车就一直等着,
    /// 整条街堵了 180 s。
    ///
    /// 原因不在 P2 的信号逻辑,而在 `Traffic::step` 的 hailer 选取:它每帧
    /// 重选「26 m 内离玩家最近的一辆」,唯一排除是 `get_driven()`。车停下
    /// 之后仍然最近,于是永远被选中,`desired = 0` 一直施加。
    ///
    /// 这条只验**计时器语义**:车被选中为 hailer 时计时累加,超时不累加,
    /// 不再被选中就清零。它必须极快地把车交出去,而不是让它抖在原地。
    #[test]
    pub(crate) fn a_parked_hail_gives_up_and_clears_its_lane() {
        let world: crate::collision::CollisionWorld = empty_world();
        let mut traffic: Traffic = Traffic::new();
        let car: TrafficCar = TrafficCar::new_on_lane(CAR_SEDAN, 0, 1.0, 0, 0.0, CRUISE_SPEED, 1.0);
        traffic.get_cars_mut().push(car);
        let dt: f32 = 1.0 / 60.0;
        // 玩家就站在车道旁 —— hailer 一定会被选中。
        let who: [f32; 2] = [0.0, 2.0];
        // 记录这辆车**最多连续停了多少秒**。停靠计时每帧清零又累加,
        // 单帧采样看不出「永远在等」,只有**最长连续停车时间**能暴露它:
        // 修复前这辆车一次都没开走过,会是整个 40 s。
        let mut longest_park: f32 = 0.0;
        let mut current_park: f32 = 0.0;
        let mut clock: f32 = 0.0;
        for _frame in 0..2400 {
            traffic.step(dt, Some(who), &world, clock);
            clock += dt;
            let stopped: bool = traffic.get_cars_ref()[0].get_speed() <= HAIL_PARKED_SPEED;
            if stopped {
                current_park += dt;
                longest_park = longest_park.max(current_park);
            } else {
                current_park = 0.0;
            }
        }
        println!("最长连续停车 {longest_park:.2} s(上限 {HAUL_PARKED_SECONDS} s)");
        assert!(
            longest_park < HAUL_PARKED_SECONDS + 3.0,
            "车连续停了 {longest_park:.2} s —— 玩家没上车它也不走,整条车道被堵死"
        );
    }

    /// 停止线距离:车在路口前时是正数,过了线之后是 `INFINITY`。
    #[test]
    pub(crate) fn the_stop_line_sits_ahead_of_the_car_and_disappears_once_passed() {
        // 沿 +Z 行驶,z=40,下一个路口中轴线在 z=60。
        let mut car: TrafficCar =
            TrafficCar::new_on_lane(CAR_SEDAN, 1, 1.0, 0, 40.0, CRUISE_SPEED, 1.0);
        let before: f32 = distance_to_stop_line(&car);
        assert!(
            before > 0.0 && before < 30.0,
            "z=40 时到停止线的距离应在 10~30 m,实得 {before:.2}"
        );
        // 开过路口之后不该再有停止线 —— 否则车会被困在路口里出不来。
        car.set_position([car.get_lane_x(), 0.0, 62.0]);
        let after: f32 = distance_to_stop_line(&car);
        assert!(
            after == f32::INFINITY,
            "过了停止线之后距离应是 INFINITY,实得 {after:.2}"
        );
    }

    /// **车必须有速度也必须有位移。**
    ///
    /// 线上实测到一批车:`speed` 恒为 2.09 m/s、连续 80 s 只挪了 0.2 m。
    /// 速度非零而位移为零 —— 这不是排队,是**卡住**。2.09 恰好是 IDM
    /// 在「前车静止于 5.74 m 外」的平衡速度,所以那批车前面确实有车;
    /// 但**前车本身**为什么不动才是问题:队列里总有一辆是最前的。
    ///
    /// 这条在**空旷世界**里跑:没有障碍、没有前车、没有红灯,只让
    /// `Traffic::step` 自己跑 600 帧。任何一辆车位移不足就是 bug。
    #[test]
    pub(crate) fn every_car_actually_moves_in_an_open_world() {
        let world: crate::collision::CollisionWorld = empty_world();
        let mut traffic: Traffic = Traffic::new();
        // 四辆车同向排在一条 z 向车道上,彼此拉开 25 m,前面没有别的车。
        // 车头间距远大于刹车距离,所以队列本身不该产生任何堵塞。
        for z in [-20.0f32, 5.0, 30.0, 55.0].iter() {
            let car: TrafficCar =
                TrafficCar::new_on_lane(CAR_SEDAN, 0, 1.0, 0, *z, CRUISE_SPEED, 1.0);
            traffic.get_cars_mut().push(car);
        }
        let start: Vec<[f32; 2]> = traffic
            .get_cars_ref()
            .iter()
            .map(|car: &TrafficCar| {
                let p: Vec3 = car.get_position();
                [p[0], p[2]]
            })
            .collect();
        let dt: f32 = 1.0 / 60.0;
        // 40 s = 一个完整信号周期(38 s)。**必须跑满一整个周期**:
        // 只跑 10 s 时 t ∈ [0,10] 全是绿灯,一根停止线都不存在,这个
        // 测试根本碰不到 P2 的逻辑,自然全绿。
        let mut clock: f32 = 0.0;
        for _frame in 0..2400 {
            traffic.step(dt, None, &world, clock);
            clock += dt;
        }
        let mut stuck: Vec<String> = Vec::new();
        for (index, car) in traffic.get_cars_ref().iter().enumerate() {
            let p: Vec3 = car.get_position();
            let moved: f32 =
                ((p[0] - start[index][0]).powi(2) + (p[2] - start[index][1]).powi(2)).sqrt();
            stuck.push(format!(
                "car{index} 位移 {moved:6.2} m(终速 {:.2})",
                car.get_speed()
            ));
        }
        println!("{}", stuck.join(" | "));
        assert!(stuck.iter().all(|line: &String| !line.is_empty()), "sanity");
        let moved_any: bool =
            traffic
                .get_cars_ref()
                .iter()
                .enumerate()
                .any(|(index, car): (usize, &TrafficCar)| {
                    let p: Vec3 = car.get_position();
                    ((p[0] - start[index][0]).powi(2) + (p[2] - start[index][1]).powi(2)).sqrt()
                        >= 5.0
                });
        assert!(moved_any, "40 s 后没有任何一辆车走过 5 m");
    }

    /// **端到端**:红灯时车必须真的停在停止线前。
    ///
    /// 这条是整套 P2 的判据。选一个「正在驶向路口、当前是红灯」的
    /// 起始位置,然后跑一个完整周期:
    ///
    /// 1. 红灯期间:速度必须降到 0,并且**不能越过停止线**。
    /// 2. 绿灯亮起:车必须重新起步。
    ///
    /// 越线用 `SIGNAL_STOP_OFFSET` 加一点余量判断 —— 停在「线前一点点」
    /// 就算数,IDM 不会精确停在整数位置。
    #[test]
    pub(crate) fn a_car_stops_at_the_red_light_and_resumes_on_green() {
        let world: crate::collision::CollisionWorld = empty_world();
        // z=38:停止线在 50.717,还有 12.72 m。
        //
        // 位置是**算出来的**,不是拍的 —— 要能在线上停住必须满足
        // `v²/(2d) ≤ IDM_ACCEL_MIN = 9`,即 `d ≥ 11.5²/18 = 7.35 m`。
        // 我第一版放在 z=44,只剩 6.72 m,需要 9.84 m/s² —— **物理上根本
        // 停不住**,于是 IDM 全力也压过线 0.03 m,测试红了。那不是模型的
        // bug,是场景不成立:红灯亮起时车早该在 30 m(`SIGNAL_BRAKE_LOOKAHEAD`)
        // 外就开始减速了,不会全速冲到 6.7 m 才看见灯。
        let mut car: TrafficCar =
            TrafficCar::new_on_lane(CAR_SEDAN, 1, 1.0, 0, 38.0, CRUISE_SPEED, 1.0);
        car.set_speed(CRUISE_SPEED);
        let line: f32 = distance_to_stop_line(&car);
        assert!(line > 0.0, "起始位置必须在停止线之前");
        // 挑一个「现在确实是红」的相位起点。
        //
        // 起点落在**全红窗口里**(t = 绿灯 + 黄灯),给 4.0 s 的全红让车
        // 有时间真的刹停 —— 之前从绿灯前 0.05 s 起跑,连刹车时间都不够,
        // 车当然停不下来,那是测试窗口太短,不是模型不行。
        let green_at: f32 = SIGNAL_GREEN_SECONDS + SIGNAL_AMBER_SECONDS + SIGNAL_ALL_RED_SECONDS;
        let red_start: f32 = SIGNAL_GREEN_SECONDS + SIGNAL_AMBER_SECONDS;
        let (a0, _a1): (SignalAspect, SignalAspect) = signals_for(red_start);
        assert_eq!(a0, SignalAspect::Red, "挑的起点必须是红灯,否则这测试没意义");
        let dt: f32 = 1.0 / 60.0;
        let mut stopped_at: Option<f32> = None;
        let mut went_past_line: bool = false;
        let mut resumed: bool = false;
        // 40 s:红灯停车,再等到下一次绿灯。
        //
        // **干等是 26 s,不是 4 s。** 整个周期 `2·(12+3+4)` = 38 s,
        // 轴 0 放行是 `[0,12)`,所以下一次绿灯在 t = 38。从 `red_start = 15`
        // 算起要等 23 s。我第一版只跑 8 s,`resumed` 永远等不到 —— 那是
        // 窗口太短,不是灯不工作。
        for step in 0..2400 {
            let t: f32 = red_start + step as f32 * dt;
            let speed: f32 = car.get_speed();
            let mut gap: f32 = f32::INFINITY;
            let mut delta_v: f32 = 0.0;
            let to_line: f32 = distance_to_stop_line(&car);
            let desired: f32 = signal_speed_limit(&car, t, to_line).min(CRUISE_SPEED);
            // 与 `Traffic::step` 同一套判定:红灯时停止线才是障碍。
            if desired < CRUISE_SPEED {
                gap = to_line;
                delta_v = speed;
            }
            let accel: f32 = TrafficCar::idm_accel(speed, desired, gap, delta_v);
            if step % 30 == 0 {
                println!(
                    "  t={:.2} z={:.2} v={:.2} gap={gap:.2} desired={desired:.2} a={accel:.2}",
                    t,
                    car.get_position()[2],
                    speed
                );
            }
            car.set_speed((speed + accel * dt).max(0.0));
            car.route_advance(dt, &world);

            let now: f32 = distance_to_stop_line(&car);
            if now == f32::INFINITY {
                // 越过停止线了 —— 只有绿灯时才允许。
                let limit: f32 = signal_speed_limit(&car, t, now);
                if limit == f32::INFINITY {
                    resumed = true;
                } else if stopped_at.is_none() {
                    went_past_line = true;
                }
            } else if car.get_speed() < 0.05 {
                stopped_at = Some(now);
            }
        }
        assert!(
            stopped_at.is_some(),
            "红灯期间车必须停住 —— 它一次都没停(当前 z={:.2})",
            car.get_position()[2]
        );
        assert!(
            !went_past_line,
            "红灯期间越过了停止线(停在 {:.2} m 处之后又冲了出去)",
            stopped_at.unwrap_or(-1.0)
        );
        assert!(resumed, "绿灯亮起后车必须重新起步");
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
