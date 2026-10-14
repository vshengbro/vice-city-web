use super::*;
/// 玩家当前的武器状态:选中哪把 + 各把剩多少子弹。
#[derive(Data, Clone, Debug)]
pub struct Arsenal {
    /// 当前手持武器。
    #[get(type(copy), pub(crate))]
    pub(crate) current: Weapon,
    /// 手枪的弹匣内子弹。
    #[get(type(copy), pub(crate))]
    pub(crate) pistol_ammo: u32,
    /// 冲锋枪的弹匣内子弹。
    #[get(type(copy), pub(crate))]
    pub(crate) smg_ammo: u32,
    /// 手枪的弹匣之外备弹。
    #[get(type(copy), pub(crate))]
    pub(crate) pistol_reserve: u32,
    /// 冲锋枪的弹匣之外备弹。
    #[get(type(copy), pub(crate))]
    pub(crate) smg_reserve: u32,
    /// 手上还剩几颗手雷(捡起 `wep_grenade` 时 +1,上限 `GRENADE_TUBES`)。
    #[get(type(copy), pub(crate))]
    pub(crate) grenade_tubes: u32,
    /// 距下次可以开火还有多久(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) cooldown: f32,
    /// 换弹剩余时间(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) reloading: f32,
    #[get(type(copy))]
    /// 瞄准方向(XZ,单位向量)。
    #[set(skip, pub(crate))]
    pub(crate) aim: Vec2,
}
/// 一个敌人 NPC。
#[derive(Data, Clone, Debug)]
pub struct Enemy {
    /// 所属阵营。
    #[get(type(copy), pub(crate))]
    pub(crate) faction: Faction,
    /// 世界坐标。
    #[get(type(copy), pub(crate))]
    pub(crate) position: Vec3,
    /// 朝向(弧度)。
    #[get(type(copy), pub(crate))]
    pub(crate) yaw: f32,
    /// 当前 AI 状态。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) state: AiState,
    /// 剩余血量。
    #[get(type(copy), pub(crate))]
    pub(crate) health: f32,
    /// 巡逻时的目标点(XZ)。
    #[get(type(copy), pub(crate))]
    pub(crate) patrol_target: Vec2,
    /// 出生点:巡逻绕着它转,不会走丢。
    #[get(type(copy), pub(crate))]
    pub(crate) home: Vec3,
    /// 巡逻游荡的计时(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) patrol_timer: f32,
    /// 距下次开火还有多久(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) fire_cooldown: f32,
    /// 受击硬直剩余时间(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) hurt_timer: f32,
    /// 逃跑剩余时间(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) flee_timer: f32,
    /// 死亡后尸体消失的倒计时(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) death_timer: f32,
    /// 受击闪光的剩余时间(秒),用于渲染时的红色闪烁。
    #[get(type(copy), pub(crate))]
    pub(crate) flash: f32,
    /// 巡逻用的相位,让每个敌人的游荡不同步。
    #[get(type(copy), pub(crate))]
    pub(crate) wander: f32,
    /// 当前水平速度(XZ)。
    #[get(type(copy), pub(crate))]
    pub(crate) velocity: Vec2,
    /// 已经开过的枪数(让每枪的散布不同)。
    #[get(type(copy), pub(crate))]
    pub(crate) shots: u32,
}
// ===========================================================================
// 通缉
// ===========================================================================
/// 通缉等级:累积的「热度」平滑映射到 0..5 星。
///
/// **为什么用连续热度而不是直接加减星。** GTA 的手感在于「打了三枪
/// 还在容忍范围内,第四枪直接两星」这种非线性:热度累积 + 阈值升星
/// 能自然表达它,而且让「一颗星掉下去」有明确的回落区间
/// ([`WANTED_STEP_DOWN`]),不会出现 1 星掉到 0 星又要重新攒的抖动。
#[derive(Data, Clone, Debug)]
pub struct Wanted {
    /// 当前热度。
    #[get(type(copy), pub(crate))]
    pub(crate) heat: f32,
    /// 当前星数(由热度算出,不单独存,避免两者不一致)。
    #[get(type(copy), pub(crate))]
    pub(crate) stars: u32,
    /// 距离上一次被警察看见过去了多久(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) unseen: f32,
    /// 上一帧是否有任何警察看得见玩家。
    #[get(type(copy), pub(crate))]
    pub(crate) spotted: bool,
}
// ===========================================================================
// 任务
// ===========================================================================
/// 一个任务的运行时状态。
#[derive(Data, Clone, Debug)]
pub struct Mission {
    /// 当前处于第几个阶段(见 `MISSION_*` 常量)。
    #[get(type(copy), pub(crate))]
    pub(crate) stage: u32,
    /// 任务名(展示用)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) title: &'static str,
    /// 当前目标的世界坐标。
    #[get(type(copy), pub(crate))]
    pub(crate) target: Vec3,
    /// 「干掉某人」阶段要杀的那个敌人的索引。
    #[get(type(copy), pub(crate))]
    pub(crate) target_enemy: Option<usize>,
    /// 完成奖励(美元)。
    #[get(type(copy), pub(crate))]
    pub(crate) reward: f32,
}
// ===========================================================================
// 行人
// ===========================================================================
/// 一个街上走的行人 NPC。
///
/// 行人只有两种状态 —— 走 / 跑,以及被撞飞后倒在地上。刻意**不给**行人
/// 战斗逻辑:他们的职责是让「开车撞人」有代价,以及给街道一点生气。
/// 碰撞用的还是玩家那套「实际位移 / dt」速度回算,所以贴着墙走会
/// 沿墙滑行而不是卡死。
#[derive(Data, Clone, Debug)]
pub struct Pedestrian {
    /// 世界坐标。
    #[get(type(copy), pub(crate))]
    pub(crate) position: Vec3,
    /// 朝向(弧度)。
    #[get(type(copy), pub(crate))]
    pub(crate) yaw: f32,
    /// 巡航目标点(XZ)。
    #[get(type(copy), pub(crate))]
    pub(crate) goal: Vec2,
    /// 当前水平速度。
    #[get(type(copy), pub(crate))]
    pub(crate) velocity: Vec2,
    /// 步态相位(弧度)。
    #[get(type(copy), pub(crate))]
    pub(crate) gait_phase: f32,
    /// 步态幅度 0..1。
    #[get(type(copy), pub(crate))]
    pub(crate) gait_amount: f32,
    /// 剩余逃跑时间(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) flee_timer: f32,
    /// 被撞飞后的倒地倒计时(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) down_timer: f32,
    /// 是否曾经被撞倒过:区分「没倒过」和「倒完已经过去」。
    #[get(type(copy), pub(crate))]
    pub(crate) has_fallen: bool,
    /// 击飞速度(米/秒),仅倒地时有意义。
    #[get(type(copy), pub(crate))]
    pub(crate) knock: Vec2,
    /// 使用的行人模型 id(4 选 1)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) model: &'static str,
    /// 正在扎堆聊天时,围成的那个圈子的圆心(XZ)。没在聊天时无意义。
    #[get(type(copy), pub(crate))]
    pub(crate) talk_center: Vec2,
    /// 还要聊多久(秒)。> 0 表示「正在聊天」。
    #[get(type(copy), pub(crate))]
    pub(crate) talk_timer: f32,
    /// 这次扎堆要聊多久(秒),进圈时定下来。
    #[get(type(copy), pub(crate))]
    pub(crate) talk_total: f32,
    /// 聊天时站在圈上的哪个角度(弧度),避免所有人挤在同一个点。
    #[get(type(copy), pub(crate))]
    pub(crate) talk_slot: f32,
    /// 距离下一次**主动**找伴扎堆还要等多久(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) gather_cooldown: f32,
}
// ===========================================================================
// 战斗总控
// ===========================================================================
/// 玩家受击后的短暂无敌,避免被一帧内多人集火打空。
#[derive(Data, Clone, Copy, Debug)]
pub struct HurtState {
    /// 无敌剩余时间(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) invuln: f32,
    /// 距上次受击多久(秒),用于护甲脱战回复。
    #[get(type(copy), pub(crate))]
    pub(crate) since_hit: f32,
    /// 死亡后的重生倒计时(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) respawn: f32,
    /// 是否已倒地。
    #[get(type(copy), pub(crate))]
    pub(crate) wasted: bool,
}
