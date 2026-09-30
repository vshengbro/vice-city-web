//! 战斗玩法的世界侧逻辑:藏身区、通缉增派、任务蓝图、行人生成。
//!
//! 与 `enemy.rs` 的分工:那边管「一个敌人这一帧干什么」,这里管
//! 「场上应该有谁」—— 谁该被增派出来、藏身区在哪、任务目标是谁。
//! 拆开的原因是**生成策略**和**行为策略**的变更频率完全不同:
//! 通缉 5 星会频繁增派,而 AI 判据很少动。

use crate::{
    combat::{Enemy, Faction, Mission, Pedestrian},
    r#const::*,
    r#type::{Vec2, Vec3},
};

/// 藏身区圆心:后巷与警局门口。
///
/// 规则很简单 —— 站进任何一个圆里,通缉就会**强制降温**且不被视为
/// 「被看见」。选点标准是「街边够窄、车进不来、人能贴墙」,所以全部
/// 落在街区的内圈空地上(见 `block_layouts` 的 `BLOCK_INNER`)。
///
/// 街区对角方向的一半(米):`STREET_LINES` 是 ±30 / ±90,取最外侧那条的
/// 中点,得到 4 个街区中心。
const BLOCK_HALF: f32 = 60.0;
/// 街区中心在 X / Z 上从原点偏移的距离(米)。
const BLOCK_OFFSET: f32 = 60.0;
/// 警局门口:街区边缘、贴着一栋楼的位置(与后巷同构,只是取外圈)。
const STATION_OFFSET: f32 = 78.0;

/// 藏身区圆心:每个街区中心 + 若干个贴楼的「门口」。
///
/// 选点标准是「街边够窄、车进不来、四周有遮挡」,因此全部落在街区
/// 内部的空地上,而不是马路中间。
///
/// # Returns
///
/// - `Vec<Vec2>` - 全部藏身区圆心的 XZ 列表。
pub fn build_hideouts() -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::new();
    for sx in [-1.0, 1.0] {
        for sz in [-1.0, 1.0] {
            // 街区中心(后巷)。
            out.push([
                sx * (BLOCK_HALF + HIDE_OFFSET),
                sz * (BLOCK_HALF + HIDE_OFFSET),
            ]);
            // 贴楼的门口(警局门口),同样在街区里但更靠外。
            out.push([sx * STATION_OFFSET, sz * (BLOCK_HALF + HIDE_OFFSET)]);
            out.push([sx * (BLOCK_HALF + HIDE_OFFSET), sz * STATION_OFFSET]);
        }
    }
    // 出生点所在的街区(0,0) 附近也给一个,保证玩家一出门就够得到。
    out.push([BLOCK_OFFSET, BLOCK_OFFSET]);
    out
}

/// 藏身区圆心在「相邻两条街道中点」上的额外偏移(米)。
///
/// 街区的几何中心是两组街道的交点,那里通常是一片空地(四周是楼),
/// 玩家跑进去之后四周的楼会挡住所有警察的视线 —— 这就是「后巷甩警」的
/// 真实原理:靠几何遮挡,不靠无敌帧。偏移把这个点往街区中心推一点,
/// 避免玩家停在正中间(四周都是开阔地,反而没遮挡)。
const HIDE_OFFSET: f32 = 2.0;

/// 警察增派点的散布半径(米):新警察在玩家周围的这个半径里冒出来。
const DEPLOY_RING_MIN: f32 = 16.0;
/// 警察增派点的散布半径上限(米)。
const DEPLOY_RING_MAX: f32 = 27.0;

/// 通缉增派:按星数补足警察人数。
///
/// 「补足」而不是「每个星数额外加」—— 后者会让 4 星时场上堆到几十个人,
/// 帧率直接崩。这里把场上警察总数钉在 `WANTED_COP_PER_STAR * stars`,
/// 并且**只有警察**参与这个计数(混混不算)。
///
/// # Arguments
///
/// - `&mut Vec<Enemy>` - 敌人列表(可能被追加)。
/// - `u32` - 当前星数。
/// - `Vec3` - 玩家坐标(警察在这个周围出现)。
/// - `u32` - 期望的警察总数。
/// - `f32` - 生成用的伪随机相位(避免同帧全部用同一随机数)。
///
/// # Returns
///
/// - `usize` - 实际新生成的警察数。
pub fn deploy_police(
    enemies: &mut Vec<Enemy>,
    stars: u32,
    player_at: Vec3,
    want_total: u32,
    phase: f32,
) -> usize {
    if stars == 0 {
        return 0;
    }
    let live: u32 = enemies
        .iter()
        .filter(|enemy: &&Enemy| enemy.get_faction() == Faction::Police && enemy.is_alive())
        .count() as u32;
    let target: u32 = want_total.min(WANTED_COP_PER_STAR * stars);
    if live >= target {
        return 0;
    }
    let mut made: usize = 0;
    for slot in live..target {
        // 环形散布:每个增派点用不同相位,避免所有警察叠在同一个点。
        let angle: f32 = phase * std::f32::consts::TAU + slot as f32 * 2.399_963;
        let radius: f32 = DEPLOY_RING_MIN
            + (DEPLOY_RING_MAX - DEPLOY_RING_MIN) * (0.5 + 0.5 * (angle * 1.7).sin());
        let at: Vec3 = [
            player_at[0] + angle.cos() * radius,
            0.0,
            player_at[2] + angle.sin() * radius,
        ];
        // 朝向玩家 —— 警察一出现就该是「在处理事情」的姿态。
        let yaw: f32 = -(player_at[2] - at[2]).atan2(player_at[0] - at[0]);
        enemies.push(Enemy::new(Faction::Police, at, yaw, angle * 0.7));
        made += 1;
    }
    made
}

/// 街头的混混:城市一开始就有的敌对 NPC,不需要通缉理由。
///
/// 分布刻意避开出生点 —— 玩家第一次落地时不该立刻被两个人夹击。
///
/// # Arguments
///
/// - `Vec3` - 玩家坐标。
/// - `usize` - 要生成的数量。
/// - `f32` - 伪随机相位。
///
/// # Returns
///
/// - `Vec<Enemy>` - 新生成的混混。
pub fn spawn_thugs(player_at: Vec3, count: usize, phase: f32) -> Vec<Enemy> {
    let mut out: Vec<Enemy> = Vec::new();
    for slot in 0..count {
        let angle: f32 = phase * std::f32::consts::TAU + slot as f32 * 1.618_034;
        let radius: f32 =
            THUG_SPAWN_MIN + (THUG_SPAWN_MAX - THUG_SPAWN_MIN) * (0.5 + 0.5 * (angle * 2.3).cos());
        let at: Vec3 = [
            player_at[0] + angle.cos() * radius,
            0.0,
            player_at[2] + angle.sin() * radius,
        ];
        out.push(Enemy::new(Faction::Thug, at, angle, angle * 1.3));
    }
    out
}

/// 混混的生成距离下限(米)。
const THUG_SPAWN_MIN: f32 = 34.0;
/// 混混的生成距离上限(米)。
const THUG_SPAWN_MAX: f32 = 62.0;

/// 街上的行人。
///
/// 行人不参与任何战斗,只提供两件事:街道的生活感,以及「开车撞人会
/// 付出代价」的物理后果。数量由 [`PED_COUNT`] 钉死 —— 每个行人是一个
/// 独立批次,数量直接乘在 draw call 上。
///
/// # Arguments
///
/// - `Vec3` - 玩家坐标(行人围着玩家铺开,保证看得到)。
/// - `usize` - 数量。
/// - `f32` - 伪随机相位。
///
/// # Returns
///
/// - `Vec<Pedestrian>` - 新生成的行人。
pub fn spawn_peds(player_at: Vec3, count: usize, phase: f32) -> Vec<Pedestrian> {
    let mut out: Vec<Pedestrian> = Vec::new();
    for slot in 0..count {
        let angle: f32 = phase * std::f32::consts::TAU + slot as f32 * 0.785_398;
        let radius: f32 = 7.0 + (slot % 6) as f32 * 4.4;
        let at: Vec3 = [
            player_at[0] + angle.cos() * radius,
            0.0,
            player_at[2] + angle.sin() * radius,
        ];
        // 行人在街上左右横穿,不是绕圈。
        let goal: Vec2 = [
            at[0] + (angle + 1.9).cos() * 9.0,
            at[2] + (angle + 1.9).sin() * 9.0,
        ];
        let model: &'static str = PED_POOL[slot % PED_POOL.len()];
        out.push(Pedestrian::new(at, angle, goal, model, angle * 2.0));
    }
    out
}

/// 行人在街上行走的四种模型。
const PED_POOL: &[&str] = &[PED_SUIT, PED_STREETWEAR, PED_DRESS, PED_OVERALLS];

/// 三个任务的蓝图:`(标题, 阶段, 目标坐标)`。
///
/// 三个任务覆盖了 GTA 任务的三种基本句式 —— 走到某地、开到某地、
/// 干掉某人。目标坐标都落在真实可走的位置(街心 / 人行道),不是空气。
///
/// # Returns
///
/// - `Vec<(&'static str, u32, Vec3)>` - 任务蓝图列表。
pub fn mission_blueprints() -> Vec<(&'static str, u32, Vec3)> {
    let mut out: Vec<(&'static str, u32, Vec3)> = Vec::new();
    for (title, stage, at) in MISSION_TABLE {
        out.push((title, *stage, *at));
    }
    out
}

/// 任务目标点所在的街道轴线(米):取自 `STREET_LINES` 的 ±30 / ±90,
/// 目标落在街边的人行道上 —— 走路能到、开车也能到。
const MISSION_ROAD_X: f32 = 30.0;
/// 任务目标点的 Z 坐标(米)。
const MISSION_ROAD_Z: f32 = 30.0;
/// 目标点往人行道方向的偏移(米):`STREET_HALF_WIDTH + 1.2`。
const MISSION_SIDEWALK: f32 = 8.2;
/// 任务蓝图表:`(标题, 阶段, 目标坐标)`。
///
/// 阶段见 `MISSION_GOTO` / `MISSION_DRIVE` / `MISSION_KILL`。三个目标
/// 分别落在三条不同的街上,强制玩家真的横穿城市。
const MISSION_TABLE: &[(&str, u32, Vec3)] = &[
    (
        MISSION_TITLE_A,
        MISSION_GOTO,
        [
            MISSION_ROAD_X + MISSION_SIDEWALK,
            0.0,
            MISSION_ROAD_Z + MISSION_SIDEWALK,
        ],
    ),
    (
        MISSION_TITLE_B,
        MISSION_DRIVE,
        [-MISSION_ROAD_X - MISSION_SIDEWALK, 0.0, -MISSION_ROAD_Z],
    ),
    (
        MISSION_TITLE_C,
        MISSION_KILL,
        [
            MISSION_ROAD_X - MISSION_SIDEWALK,
            0.0,
            -MISSION_ROAD_Z - MISSION_SIDEWALK,
        ],
    ),
];

/// 推进任务状态机。
///
/// 阶段推进的判据全部是「玩家离目标多近」或「目标死了没」,没有计时器 ——
/// 玩家自己决定节奏。
///
/// # Arguments
///
/// - `&mut Mission` - 当前任务。
/// - `Vec3` - 玩家坐标。
/// - `usize` - 目标敌人的索引(阶段推进时用来确认它已被击杀)。
/// - `bool` - 目标敌人是否已死亡。
/// - `u32` - 已完成的任务数(用于给下一个任务换目标点)。
///
/// # Returns
///
/// - `f32` - 任务完成时返回报酬,否则返回 0。
pub fn advance_mission(
    mission: &mut Mission,
    player_at: Vec3,
    target_index: usize,
    target_dead: bool,
    done: u32,
) -> f32 {
    if !mission.is_active() {
        return 0.0;
    }
    match mission.get_stage() {
        MISSION_KILL => {
            if target_dead {
                let reward: f32 = mission.get_reward();
                mission.complete();
                return reward;
            }
            0.0
        }
        MISSION_GOTO | MISSION_DRIVE => {
            let target: Vec3 = mission.get_target();
            let dx: f32 = target[0] - player_at[0];
            let dz: f32 = target[2] - player_at[2];
            if dx * dx + dz * dz > MISSION_RADIUS * MISSION_RADIUS {
                return 0.0;
            }
            // 第一段走完就转成「去干掉某人」,让同一个任务有后续。
            let next_done: u32 = done + 1;
            if next_done >= MISSION_CHAIN_LENGTH {
                let reward: f32 = mission.get_reward();
                mission.complete();
                return reward;
            }
            mission.set_target_enemy(target_index);
            mission.advance(MISSION_KILL, target);
            0.0
        }
        _ => 0.0,
    }
}

/// 一个任务串起来的段数。
const MISSION_CHAIN_LENGTH: u32 = 2;

/// 玩家是否站在藏身区里。
///
/// # Arguments
///
/// - `&[Vec2]` - 藏身区圆心。
/// - `Vec3` - 玩家坐标。
///
/// # Returns
///
/// - `bool` - 在任一藏身区内为 `true`。
pub fn is_hidden(hideouts: &[Vec2], player_at: Vec3) -> bool {
    for spot in hideouts {
        let dx: f32 = spot[0] - player_at[0];
        let dz: f32 = spot[1] - player_at[2];
        if dx * dx + dz * dz <= HIDE_RADIUS * HIDE_RADIUS {
            return true;
        }
    }
    false
}
