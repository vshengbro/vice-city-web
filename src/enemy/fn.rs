use super::*;

/// 决定单个敌人下一帧的行为。
///
/// 判据顺序就是优先级:**死亡 > 逃跑 > 开火 > 追击 > 巡逻 > 站桩**。
/// 先判死亡再判其它,是为了让「刚被打死的那一帧」不会因为还在玩家视野里
/// 而继续开火。
///
/// # Arguments
///
/// - `&Enemy` - 当前状态(只读)。
/// - `Vec3` - 玩家坐标。
/// - `&CollisionWorld` - 静态碰撞世界,用于视线遮挡。
/// - `bool` - 玩家是否正在被通缉(警察据此决定是否敌对)。
/// - `f32` - 敌人自己的游荡相位(用于让巡逻点不同步)。
///
/// # Returns
///
/// - `EnemyDecision` - 本帧的决策。
pub fn decide(
    enemy: &Enemy,
    player_at: Vec3,
    world: &CollisionWorld,
    wanted: bool,
    wander: f32,
) -> EnemyDecision {
    let here: Vec3 = enemy.get_position();
    let distance: f32 = flat_distance(here, player_at);
    let sees: bool =
        distance <= ENEMY_SIGHT && has_line_of_sight(world, here, player_at, ENEMY_SIGHT);
    // 警察只在「有通缉」时敌对;混混一开始就是敌对的,不需要理由。
    let hostile: bool = match enemy.get_faction() {
        Faction::Police => wanted,
        Faction::Thug => true,
    };
    let idle: EnemyDecision = EnemyDecision {
        next: *enemy.get_state(),
        move_to: [0.0, 0.0],
        face: player_at,
        shoot: false,
    };
    // 死亡优先级最高:尸体不追人也不开枪。
    if !enemy.is_alive() {
        return EnemyDecision {
            next: AiState::Dead,
            ..idle
        };
    }
    // 逃跑:血量见底,朝远离玩家的方向跑,不反击。
    if enemy.get_health() <= ENEMY_FLEE_HEALTH {
        let away: Vec2 = [here[0] - player_at[0], here[2] - player_at[2]];
        let length: f32 = (away[0] * away[0] + away[1] * away[1])
            .sqrt()
            .max(f32::EPSILON);
        return EnemyDecision {
            next: AiState::Flee,
            move_to: [away[0] / length, away[1] / length],
            face: [here[0] + away[0], here[1], here[2] + away[1]],
            shoot: false,
        };
    }
    if !hostile {
        return patrol(enemy, wander);
    }
    // 看见 + 在射程内 = 开火,但保持一个交火距离:贴脸输出低,站远了瞄不准。
    //
    // **站得比交火距离远时必须继续靠近。** 原来这里只处理了「太近了后撤」
    // (`distance < ENEMY_STANDOFF` → -1.0),中间那段给的是 `sign = 0.0`,
    // 等于 `move_to = [0, 0]` —— 敌人只要落在 `ENEMY_STANDOFF`(9 m)到
    // `ENEMY_FIRE_RANGE`(26 m)之间就**彻底不动**:不进不退,站着挨打。
    // 实测 12/15 个敌人卡在 `attack` 状态、最近的一个冻结在 15.7 m,
    // `chase` 计数为 0。
    //
    // GTA 的开火 AI 是「逼近到交火距离 → 站定射击 → 玩家贴身则后撤」,
    // 三段都要有,少一段就少了逼近这一段。
    if sees && distance <= ENEMY_FIRE_RANGE {
        // 落在交火距离内就站定不动;在带外则朝目标方向逼近 / 后撤。
        let sign: f32 = if distance < ENEMY_STANDOFF {
            -1.0
        } else if distance > ENEMY_STANDOFF {
            1.0
        } else {
            0.0
        };
        let away: Vec2 = [here[0] - player_at[0], here[2] - player_at[2]];
        let length: f32 = (away[0] * away[0] + away[1] * away[1])
            .sqrt()
            .max(f32::EPSILON);
        return EnemyDecision {
            next: AiState::Attack,
            move_to: [away[0] / length * sign, away[1] / length * sign],
            face: player_at,
            shoot: true,
        };
    }
    // 看见但太远 = 追。
    if sees {
        return EnemyDecision {
            next: AiState::Chase,
            move_to: [player_at[0] - here[0], player_at[2] - here[2]],
            face: player_at,
            shoot: false,
        };
    }
    // 看不见,但已经打过照面(离得不远) = 朝玩家最后已知位置摸过去。
    if distance <= ENEMY_LEASH && *enemy.get_state() == AiState::Chase {
        return EnemyDecision {
            next: AiState::Chase,
            move_to: [player_at[0] - here[0], player_at[2] - here[2]],
            face: player_at,
            shoot: false,
        };
    }
    patrol(enemy, wander)
}

/// 巡逻决策:在自己的活动半径内绕圈。
///
/// # Arguments
///
/// - `&Enemy` - 当前状态。
/// - `f32` - 游荡相位。
///
/// # Returns
///
/// - `EnemyDecision` - 巡逻决策。
pub(crate) fn patrol(enemy: &Enemy, wander: f32) -> EnemyDecision {
    let here: Vec3 = enemy.get_position();
    let home: Vec3 = enemy.get_home();
    // 巡逻点是**围绕出生点扫出来的一个点**。之前这里写的是
    // `wander + TAU`,一个恒定的角,于是所有敌人永远朝着同一个方向,
    // 表现得像钉在原地。现在用 `wander` 当相位:它在 `step_timers` 里
    // 按 `ENEMY_WANDER_RATE` 持续累加,所以巡逻点真的沿着出生点绕圈。
    // 每个敌人的 `wander` 初值不同(见 `spawn_thugs` 的黄金角散布),
    // 因此不会有一堆人挤在同一个点。
    let phase: f32 = wander * 3.0;
    let target: Vec3 = [
        home[0] + phase.cos() * ENEMY_WANDER_RADIUS,
        home[1],
        home[2] + phase.sin() * ENEMY_WANDER_RADIUS,
    ];
    EnemyDecision {
        next: AiState::Patrol,
        move_to: [target[0] - here[0], target[2] - here[2]],
        face: target,
        shoot: false,
    }
}

/// 把一次决策应用到一个敌人上:移动、转身、开火计时。
///
/// 碰撞用的是玩家那套「实际位移 / dt」回算,所以贴着墙走会沿墙滑行
/// 而不是卡死 —— 这一点对交火时的走位手感影响很大。
///
/// # Arguments
///
/// - `&mut Enemy` - 要更新的敌人。
/// - `&EnemyDecision` - 本帧决策。
/// - `f32` - 本帧秒数。
/// - `&CollisionWorld` - 静态碰撞世界。
/// - `bool` - 决策是否要求开火。
///
/// # Returns
///
/// - `bool` - 本帧是否真的开了一枪(冷却结束时)。
pub fn apply(
    enemy: &mut Enemy,
    decision: &EnemyDecision,
    dt: f32,
    world: &CollisionWorld,
    wants_fire: bool,
) -> bool {
    let mut shot: bool = false;
    if !enemy.is_alive() {
        enemy.step_death(dt);
        return false;
    }
    // 计时器。
    enemy.step_timers(dt);
    // 速度:逃跑更快,其余按基准速度。
    let speed: f32 = if decision.get_next() == AiState::Flee {
        ENEMY_SPEED * ENEMY_FLEE_SPEED_GAIN
    } else if decision.get_next() == AiState::Chase || decision.get_next() == AiState::Attack {
        ENEMY_SPEED
    } else {
        ENEMY_SPEED * 0.45
    };
    // 朝向:即使不动也要转,不然站桩的敌人会一直背对玩家。
    let here: Vec3 = enemy.get_position();
    enemy.set_yaw(face_towards(enemy.get_yaw(), here, decision.get_face()));
    // 水平速度按决策方向积分,受碰撞分离约束。
    let length: f32 = (decision.get_move_to()[0] * decision.get_move_to()[0])
        + (decision.get_move_to()[1] * decision.get_move_to()[1]);
    let magnitude: f32 = length.sqrt();
    let velocity: Vec2 = if magnitude > f32::EPSILON {
        [
            decision.get_move_to()[0] / magnitude * speed,
            decision.get_move_to()[1] / magnitude * speed,
        ]
    } else {
        [0.0, 0.0]
    };
    let wanted: Vec2 = [here[0] + velocity[0] * dt, here[2] + velocity[1] * dt];
    let resolved: Vec2 = world.resolve_with_radius(wanted, ENEMY_RADIUS);
    // 速度 = 实际位移 / dt:被墙挡住的分量自然消失,剩下的分量继续滑动。
    let inverse_dt: f32 = 1.0 / dt.max(f32::EPSILON);
    let achieved: Vec2 = [resolved[0] - here[0], resolved[1] - here[2]];
    enemy.set_velocity([achieved[0] * inverse_dt, achieved[1] * inverse_dt]);
    enemy.set_position([resolved[0], here[1], resolved[1]]);
    enemy.set_state(decision.get_next());
    // 开火:只有「要求开火 + 冷却结束 + 没在硬直」才真的打。
    if wants_fire && enemy.get_fire_cooldown() <= 0.0 && enemy.get_hurt_timer() <= 0.0 {
        enemy.set_fire_cooldown(1.0 / ENEMY_FIRE_RATE);
        shot = true;
    }
    shot
}

/// 带散布的瞄准方向:警察不该百发百中,否则玩家一露头就死。
///
/// # Arguments
///
/// - `Vec3` - 敌人坐标。
/// - `Vec3` - 目标坐标。
/// - `f32` - 游荡相位(保证同一敌人每枪的散布不同)。
/// - `u32` - 第几枪。
///
/// # Returns
///
/// - `Vec2` - 带散布的 XZ 方向。
pub fn aim_with_spread(from: Vec3, to: Vec3, wander: f32, shot_index: u32) -> Vec2 {
    // 角度用项目统一的 `yaw` 约定(见 [`crate::combat::face_towards`]):
    // `desired = dx.atan2(dz)`,朝向向量 `(sin, cos)`。以前这里写的是
    // `−dz.atan2(dx)` 配 `[cos, −sin]` —— 那套自洽,但整体偏 90°,
    // 于是「朝玩家散布」实际朝的是玩家左侧 90°,命中率恒定偏低。
    let desired: f32 = (to[0] - from[0]).atan2(to[2] - from[2]);
    // 用敌人数 + 枪序错开相位,避免所有人朝同一方向偏。
    let jitter: f32 = ((shot_index as f32) * 2.399_963 + wander * 3.0).sin() * ENEMY_SPREAD;
    let angle: f32 = desired + jitter;
    [angle.sin(), angle.cos()]
}

/// 敌人是否还在这一帧的模拟范围内。
///
/// 判据是**距离**,不是视野:远处连 AI 决策都不跑。这一层是这一整套
/// 战斗系统能塞进 60 fps 的关键 —— 场上可以有几十个警察,但每帧真正
/// 需要「想一下」的只有靠近玩家的那几个。
///
/// # Arguments
///
/// - `Vec3` - 敌人坐标。
/// - `Vec3` - 玩家坐标。
///
/// # Returns
///
/// - `bool` - 在激活圈内为 `true`。
pub fn is_active(at: Vec3, player_at: Vec3) -> bool {
    flat_distance(at, player_at) <= ENEMY_ACTIVATION
}
