//! 战斗系统:武器、敌人 AI、通缉等级、伤害与死亡。
//!
//! 与 `traffic.rs` 的车道循环同源:所有实体都是**索引可寻址**的 `Vec`,
//! 每帧 `step(dt, ctx)` 推进,渲染侧按索引把姿态写回 `SceneBatch`。
//! 但敌人多了三样车流不需要的东西:
//!
//! - **状态机** [`AiState`]:站桩 → 巡逻 → 追击 → 开火 → 逃跑。状态迁移
//!   只读「距离 + 视线是否被挡」两个量,所以行为是**可预测**的:躲在
//!   楼后面它就看不见你,不会隔墙锁血。
//! - **视线遮挡**:复用 `collision::ray_to_shapes` 的球体推进(相机遮挡
//!   回避用的同一套),而不是只比距离。没有这一步,隔一条街的警察会
//!   隔着楼开火,通缉系统立刻变成街垒射击游戏。
//! - **血量 / 死亡**:血量归零 → 倒地计时 → 从最近医院重生,扣钱。
//!
//! 武器是 **hitscan**(瞬时射线)而不是飞行的子弹:第三人称 + 60 FPS 下
//! 子弹的飞行时间短到无法察觉,却要多一份状态、多一次剔除。代价是
//! 距离衰减只能靠数值硬凑,换来的是零延迟与可预测的手感。
//!
//! **性能**:敌人与行人共用同一套 `Instance` instancing(同资产只上传一次
//! 顶点,每帧只传 model matrix + tint),并且走距离剔除 —— 超过
//! [`ENEMY_SIGHT`] 一倍多的敌人连 AI 都不更新,自然也不会被上传。

use crate::{
    camera::Mat4,
    collision::CollisionWorld,
    r#const::*,
    player::{Player, wrap_angle},
    r#type::{Vec2, Vec3},
};

// ===========================================================================
// 武器
// ===========================================================================

/// 三种可切换武器。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Weapon {
    /// 手枪:精度高、射速中等、弹匣小。
    Pistol,
    /// 冲锋枪:射速快、伤害低、弹匣大。
    Smg,
    /// 球棒:近战、无弹药、一击重创。
    Bat,
}

impl Weapon {
    /// 该武器的资产 id(用于在世界里绘制手持模型)。
    ///
    /// # Returns
    ///
    /// - `&'static str` - `wep_pistol` / `wep_smg` / `wep_bat`。
    pub fn asset(&self) -> &'static str {
        match self {
            Weapon::Pistol => WEP_PISTOL,
            Weapon::Smg => WEP_SMG,
            Weapon::Bat => WEP_BAT,
        }
    }

    /// 弹匣容量。
    ///
    /// # Returns
    ///
    /// - `u32` - 满弹匣的子弹数(球棒返回 0)。
    pub fn magazine(&self) -> u32 {
        match self {
            Weapon::Pistol => PISTOL_MAGAZINE,
            Weapon::Smg => SMG_MAGAZINE,
            Weapon::Bat => 0,
        }
    }

    /// 单发伤害。
    ///
    /// # Returns
    ///
    /// - `f32` - 命中瞬间的最大伤害值。
    pub fn damage(&self) -> f32 {
        match self {
            Weapon::Pistol => PISTOL_DAMAGE,
            Weapon::Smg => SMG_DAMAGE,
            Weapon::Bat => BAT_DAMAGE,
        }
    }

    /// 射速(次/秒)。
    ///
    /// # Returns
    ///
    /// - `f32` - 两次开火之间的最短间隔的倒数。
    pub fn fire_rate(&self) -> f32 {
        match self {
            Weapon::Pistol => PISTOL_FIRE_RATE,
            Weapon::Smg => SMG_FIRE_RATE,
            Weapon::Bat => 1.0 / BAT_COOLDOWN,
        }
    }

    /// 有效射程(米)。
    ///
    /// # Returns
    ///
    /// - `f32` - 超出这个距离不再判定命中。
    pub fn range(&self) -> f32 {
        match self {
            Weapon::Bat => BAT_RANGE,
            _ => GUN_RANGE,
        }
    }

    /// HUD 上的武器名。
    ///
    /// # Returns
    ///
    /// - `&'static str` - 展示用短名。
    pub fn label(&self) -> &'static str {
        match self {
            Weapon::Pistol => WEAPON_NAME_PISTOL,
            Weapon::Smg => WEAPON_NAME_SMG,
            Weapon::Bat => WEAPON_NAME_BAT,
        }
    }
}

/// 玩家当前的武器状态:选中哪把 + 各把剩多少子弹。
#[derive(Clone, Debug)]
pub struct Arsenal {
    /// 当前手持武器。
    current: Weapon,
    /// 手枪的弹匣内子弹。
    pistol_ammo: u32,
    /// 冲锋枪的弹匣内子弹。
    smg_ammo: u32,
    /// 手枪的弹匣之外备弹。
    pistol_reserve: u32,
    /// 冲锋枪的弹匣之外备弹。
    smg_reserve: u32,
    /// 距下次可以开火还有多久(秒)。
    cooldown: f32,
    /// 换弹剩余时间(秒)。
    reloading: f32,
    /// 瞄准方向(XZ,单位向量)。
    aim: Vec2,
}

impl Arsenal {
    /// 开局的武器状态:手枪满弹,冲锋枪 60 发。
    ///
    /// # Returns
    ///
    /// - `Self` - 就绪的武器架。
    pub fn new() -> Self {
        Self {
            current: Weapon::Pistol,
            pistol_ammo: PISTOL_MAGAZINE,
            smg_ammo: SMG_MAGAZINE,
            pistol_reserve: PISTOL_RESERVE,
            smg_reserve: SMG_RESERVE,
            cooldown: 0.0,
            reloading: 0.0,
            aim: [1.0, 0.0],
        }
    }

    /// 当前手持武器。
    ///
    /// # Returns
    ///
    /// - `Weapon` - 武器种类。
    pub fn get_weapon(&self) -> Weapon {
        self.current
    }

    /// 切换到指定武器(会打断换弹)。
    ///
    /// # Arguments
    ///
    /// - `Weapon` - 要切换到的武器。
    pub fn set_weapon(&mut self, value: Weapon) {
        if self.get_weapon() == value {
            return;
        }
        self.current = value;
        self.reloading = 0.0;
    }

    /// 当前武器的弹匣余量。
    ///
    /// # Returns
    ///
    /// - `u32` - 还能打多少发。
    pub fn get_magazine(&self) -> u32 {
        match self.get_weapon() {
            Weapon::Pistol => self.pistol_ammo,
            Weapon::Smg => self.smg_ammo,
            Weapon::Bat => 0,
        }
    }

    /// 当前武器的备弹(弹匣之外的)。
    ///
    /// # Returns
    ///
    /// - `u32` - 备弹数量。
    pub fn get_reserve(&self) -> u32 {
        match self.get_weapon() {
            Weapon::Pistol => self.pistol_reserve,
            Weapon::Smg => self.smg_reserve,
            Weapon::Bat => 0,
        }
    }

    /// 瞄准方向。
    ///
    /// # Returns
    ///
    /// - `Vec2` - XZ 平面上的单位向量。
    pub fn get_aim(&self) -> Vec2 {
        self.aim
    }

    /// 写入瞄准方向并归一化。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 期望的瞄准方向(XZ)。
    pub fn set_aim(&mut self, value: Vec2) {
        let length: f32 = (value[0] * value[0] + value[1] * value[1]).sqrt();
        if length > f32::EPSILON {
            self.aim = [value[0] / length, value[1] / length];
        }
    }

    /// 推进冷却与换弹计时。
    ///
    /// # Arguments
    ///
    /// - `f32` - 本帧的秒数增量。
    pub fn tick(&mut self, dt: f32) {
        if self.get_cooldown() > 0.0 {
            self.set_cooldown((self.get_cooldown() - dt).max(0.0));
        }
        if self.get_reloading() <= 0.0 {
            return;
        }
        self.set_reloading(self.get_reloading() - dt);
        if self.get_reloading() > 0.0 {
            return;
        }
        // 换弹计时归零的那一刻才真的把弹药填进弹匣 —— 之前这里只把
        // 计时清零,弹匣永远是空的,于是「按 R 换弹」看起来毫无反应。
        self.set_reloading(0.0);
        let capacity: u32 = self.get_weapon().magazine();
        let missing: u32 = capacity.saturating_sub(self.get_magazine());
        let from_reserve: u32 = missing.min(self.get_reserve());
        self.finish_reload(from_reserve);
    }

    /// 当前能否开火。
    ///
    /// # Returns
    ///
    /// - `bool` - 冷却结束且弹匣非空时为 `true`。
    pub fn can_fire(&self) -> bool {
        if self.get_cooldown() > 0.0 || self.get_reloading() > 0.0 {
            return false;
        }
        match self.get_weapon() {
            Weapon::Bat => true,
            _ => self.get_magazine() > 0,
        }
    }

    /// 是否正在换弹。
    ///
    /// # Returns
    ///
    /// - `bool` - 换弹计时未结束时为 `true`。
    pub fn is_reloading(&self) -> bool {
        self.get_reloading() > 0.0
    }

    /// 开一枪:消耗弹药并进入冷却。
    ///
    /// 弹匣打空时**不会**自动换弹 —— 换弹必须是玩家的显式动作(按 C),
    /// 否则「打空 → 自动满上」会让 R 键失去意义。
    ///
    /// # Arguments
    ///
    /// - `u32` - 补给的弹药数量。
    ///
    /// # Returns
    ///
    /// - `bool` - 本次开火是否真的发生了。
    pub fn fire(&mut self, ammo_grant: u32) -> bool {
        if !self.can_fire() {
            return false;
        }
        match self.get_weapon() {
            Weapon::Bat => {
                self.set_cooldown(1.0 / self.get_weapon().fire_rate());
                true
            }
            _ => {
                let left: u32 = self.get_magazine().saturating_sub(1);
                match self.get_weapon() {
                    Weapon::Pistol => self.set_pistol_ammo(left),
                    _ => self.set_smg_ammo(left),
                }
                self.set_cooldown(1.0 / self.get_weapon().fire_rate());
                let _: u32 = ammo_grant;
                true
            }
        }
    }

    /// 开始换弹。
    ///
    /// # Returns
    ///
    /// - `bool` - 换弹是否被启动(已经在换 / 满弹匣 / 球棒时为 `false`)。
    pub fn reload(&mut self) -> bool {
        if self.get_weapon() == Weapon::Bat || self.get_reloading() > 0.0 {
            return false;
        }
        if self.get_magazine() >= self.get_weapon().magazine() {
            return false;
        }
        self.set_reloading(RELOAD_TIME);
        true
    }

    /// 换弹完成时由外部调用:把弹匣补满,并从备弹里扣。
    ///
    /// # Arguments
    ///
    /// - `u32` - 本次补给进弹匣的弹药。
    pub fn finish_reload(&mut self, rounds: u32) {
        let capacity: u32 = self.get_weapon().magazine();
        match self.get_weapon() {
            Weapon::Pistol => {
                self.set_pistol_ammo((self.get_pistol_ammo() + rounds).min(capacity));
                self.set_pistol_reserve(self.get_pistol_reserve() - rounds);
            }
            Weapon::Smg => {
                self.set_smg_ammo((self.get_smg_ammo() + rounds).min(capacity));
                self.set_smg_reserve(self.get_smg_reserve() - rounds);
            }
            Weapon::Bat => {}
        }
        self.set_reloading(0.0);
    }

    /// 弹药箱:给当前武器补弹。
    ///
    /// # Arguments
    ///
    /// - `u32` - 弹药箱提供的子弹数。
    pub fn add_ammo(&mut self, rounds: u32) {
        match self.get_weapon() {
            Weapon::Pistol => {
                self.set_pistol_reserve(self.get_pistol_reserve() + rounds);
            }
            Weapon::Smg => {
                self.set_smg_reserve(self.get_smg_reserve() + rounds);
            }
            Weapon::Bat => {}
        }
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `u32` - 当前值。
    pub fn get_pistol_ammo(&self) -> u32 {
        self.pistol_ammo
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `u32` - 新值。
    pub fn set_pistol_ammo(&mut self, value: u32) {
        self.pistol_ammo = value;
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `u32` - 当前值。
    pub fn get_smg_ammo(&self) -> u32 {
        self.smg_ammo
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `u32` - 新值。
    pub fn set_smg_ammo(&mut self, value: u32) {
        self.smg_ammo = value;
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `u32` - 当前值。
    pub fn get_pistol_reserve(&self) -> u32 {
        self.pistol_reserve
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `u32` - 新值。
    pub fn set_pistol_reserve(&mut self, value: u32) {
        self.pistol_reserve = value;
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `u32` - 当前值。
    pub fn get_smg_reserve(&self) -> u32 {
        self.smg_reserve
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `u32` - 新值。
    pub fn set_smg_reserve(&mut self, value: u32) {
        self.smg_reserve = value;
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前值。
    pub fn get_cooldown(&self) -> f32 {
        self.cooldown
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_cooldown(&mut self, value: f32) {
        self.cooldown = value;
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前值。
    pub fn get_reloading(&self) -> f32 {
        self.reloading
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_reloading(&mut self, value: f32) {
        self.reloading = value;
    }
}

// ===========================================================================
// 敌人 AI
// ===========================================================================

/// 敌人的行为状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AiState {
    /// 站桩不动(出生后的第一帧)。
    Idle,
    /// 沿出生点附近小范围游荡。
    Patrol,
    /// 看见玩家,正在靠近。
    Chase,
    /// 已在射程内,正在开火。
    Attack,
    /// 血量见底,转身逃跑。
    Flee,
    /// 已死亡,尸体下沉中。
    Dead,
}

impl AiState {
    /// 该状态在 HUD / 调试快照里的名字。
    ///
    /// # Returns
    ///
    /// - `&'static str` - 状态名。
    pub fn label(&self) -> &'static str {
        match self {
            AiState::Idle => AI_STATE_IDLE,
            AiState::Patrol => AI_STATE_PATROL,
            AiState::Chase => AI_STATE_CHASE,
            AiState::Attack => AI_STATE_ATTACK,
            AiState::Flee => AI_STATE_FLEE,
            AiState::Dead => AI_STATE_DEAD,
        }
    }
}

/// 敌人是警察还是敌对混混。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Faction {
    /// 警察:被通缉触发,攻击玩家,不掉通缉热度。
    Police,
    /// 敌对混混:自由巡逻,被攻击后反击。
    Thug,
}

/// 一个敌人 NPC。
#[derive(Clone, Debug)]
pub struct Enemy {
    /// 所属阵营。
    faction: Faction,
    /// 世界坐标。
    position: Vec3,
    /// 朝向(弧度)。
    yaw: f32,
    /// 当前 AI 状态。
    state: AiState,
    /// 剩余血量。
    health: f32,
    /// 巡逻时的目标点(XZ)。
    patrol_target: Vec2,
    /// 出生点:巡逻绕着它转,不会走丢。
    home: Vec3,
    /// 巡逻游荡的计时(秒)。
    patrol_timer: f32,
    /// 距下次开火还有多久(秒)。
    fire_cooldown: f32,
    /// 受击硬直剩余时间(秒)。
    hurt_timer: f32,
    /// 逃跑剩余时间(秒)。
    flee_timer: f32,
    /// 死亡后尸体消失的倒计时(秒)。
    death_timer: f32,
    /// 受击闪光的剩余时间(秒),用于渲染时的红色闪烁。
    flash: f32,
    /// 巡逻用的相位,让每个敌人的游荡不同步。
    wander: f32,
    /// 当前水平速度(XZ)。
    velocity: Vec2,
    /// 已经开过的枪数(让每枪的散布不同)。
    shots: u32,
}

impl Enemy {
    /// 在给定位置生成一个敌人。
    ///
    /// # Arguments
    ///
    /// - `Faction` - 阵营。
    /// - `Vec3` - 世界坐标。
    /// - `f32` - 初始朝向(弧度)。
    /// - `f32` - 随机游荡相位。
    ///
    /// # Returns
    ///
    /// - `Self` - 就绪的敌人。
    pub fn new(faction: Faction, position: Vec3, yaw: f32, wander: f32) -> Self {
        let health: f32 = match faction {
            Faction::Police => ENEMY_HEALTH,
            Faction::Thug => THUG_HEALTH,
        };
        Self {
            faction,
            position,
            yaw,
            state: AiState::Idle,
            health,
            patrol_target: [position[0], position[2]],
            home: position,
            patrol_timer: 0.0,
            fire_cooldown: 0.0,
            hurt_timer: 0.0,
            flee_timer: 0.0,
            death_timer: 0.0,
            flash: 0.0,
            wander,
            velocity: [0.0, 0.0],
            shots: 0,
        }
    }

    /// 阵营。
    ///
    /// # Returns
    ///
    /// - `Faction` - 警察或混混。
    pub fn get_faction(&self) -> Faction {
        self.faction
    }

    /// 世界坐标。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 当前坐标。
    pub fn get_position(&self) -> Vec3 {
        self.position
    }

    /// 朝向。
    ///
    /// # Returns
    ///
    /// - `f32` - 绕 Y 轴弧度。
    pub fn get_yaw(&self) -> f32 {
        self.yaw
    }

    /// 当前状态。
    ///
    /// # Returns
    ///
    /// - `AiState` - 行为状态。
    pub fn get_state(&self) -> AiState {
        self.state
    }

    /// 剩余血量。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前血量。
    pub fn get_health(&self) -> f32 {
        self.health
    }

    /// 是否还活着。
    ///
    /// # Returns
    ///
    /// - `bool` - 存活为 `true`。
    pub fn is_alive(&self) -> bool {
        self.get_state() != AiState::Dead
    }

    /// 巡逻的出生点。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 出生坐标。
    pub fn get_home(&self) -> Vec3 {
        self.home
    }

    /// 当前水平速度(XZ)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 速度。
    pub fn get_velocity(&self) -> Vec2 {
        self.velocity
    }

    /// 速度的模长。
    ///
    /// # Returns
    ///
    /// - `f32` - 速度大小(米/秒)。
    pub fn length(&self) -> f32 {
        let v: Vec2 = self.get_velocity();
        (v[0] * v[0] + v[1] * v[1]).sqrt()
    }

    /// 被 hitscan 命中时的后退位移。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 击退速度(米/秒)。
    pub fn knock_back(&mut self, impulse: Vec2) {
        if self.is_alive() {
            self.set_velocity([impulse[0], impulse[1]]);
        }
    }

    /// 受击闪光的剩余时间。
    ///
    /// # Returns
    ///
    /// - `f32` - 秒;大于 0 时渲染层把它调红。
    pub fn get_flash(&self) -> f32 {
        self.flash
    }

    /// 扣血。
    ///
    /// # Arguments
    ///
    /// - `f32` - 伤害量。
    ///
    /// # Returns
    ///
    /// - `bool` - 这一击是否把它打死(用于播报击杀)。
    pub fn damage(&mut self, amount: f32) -> bool {
        if !self.is_alive() {
            return false;
        }
        self.set_health(self.get_health() - amount);
        self.set_flash(ENEMY_FLASH_TIME);
        if self.get_health() <= 0.0 {
            self.set_health(0.0);
            self.set_state(AiState::Dead);
            self.set_death_timer(DEATH_FADE_TIME);
            return true;
        }
        self.set_hurt_timer(ENEMY_HIT_STUN);
        // 血量见底就掉头跑,而不是站着被打死。
        if self.get_health() <= ENEMY_FLEE_HEALTH {
            self.set_state(AiState::Flee);
            self.set_flee_timer(ENEMY_FLEE_TIME);
        }
        false
    }

    /// 写入水平速度。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 新速度。
    pub fn set_velocity(&mut self, value: Vec2) {
        self.velocity = value;
    }

    /// 写入行为状态。
    ///
    /// # Arguments
    ///
    /// - `AiState` - 新状态。
    pub fn set_state(&mut self, value: AiState) {
        self.state = value;
    }

    /// 写入朝向。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新朝向(弧度)。
    pub fn set_yaw(&mut self, value: f32) {
        self.yaw = value;
    }

    /// 写入世界坐标。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 新坐标。
    pub fn set_position(&mut self, value: Vec3) {
        self.position = value;
    }

    /// 距下次开火还有多久(秒)。
    ///
    /// # Returns
    ///
    /// - `f32` - 剩余秒数。
    pub fn get_fire_cooldown(&self) -> f32 {
        self.fire_cooldown
    }

    /// 写入开火冷却。
    ///
    /// # Arguments
    ///
    /// - `f32` - 剩余秒数。
    pub fn set_fire_cooldown(&mut self, value: f32) {
        self.fire_cooldown = value;
    }

    /// 记一次开火:累加枪数。
    pub fn mark_shot(&mut self) {
        self.set_shots(self.get_shots() + 1);
    }

    /// 已开过的枪数。
    ///
    /// # Returns
    ///
    /// - `u32` - 枪数。
    pub fn get_shots(&self) -> u32 {
        self.shots
    }

    /// 受击硬直剩余时间(秒)。
    ///
    /// # Returns
    ///
    /// - `f32` - 剩余秒数。
    pub fn get_hurt_timer(&self) -> f32 {
        self.hurt_timer
    }

    /// 游荡相位。
    ///
    /// # Returns
    ///
    /// - `f32` - 相位(弧度)。
    pub fn get_wander(&self) -> f32 {
        self.wander
    }

    /// 推进全部存活计时器(冷却 / 硬直 / 逃跑 / 闪烁 / 游荡相位 / 巡逻选点)。
    ///
    /// # Arguments
    ///
    /// - `f32` - 本帧秒数。
    pub fn step_timers(&mut self, dt: f32) {
        self.set_fire_cooldown((self.get_fire_cooldown() - dt).max(0.0));
        self.set_hurt_timer((self.get_hurt_timer() - dt).max(0.0));
        self.set_flee_timer((self.get_flee_timer() - dt).max(0.0));
        self.set_flash((self.get_flash() - dt).max(0.0));
        self.set_wander((self.get_wander() + ENEMY_WANDER_RATE * dt) % std::f32::consts::TAU);
        if self.get_patrol_timer() >= ENEMY_PATROL_RESELECT {
            self.set_patrol_timer(0.0);
        } else {
            self.set_patrol_timer(self.get_patrol_timer() + dt);
        }
    }

    /// 死亡后的尸体计时推进。
    ///
    /// # Arguments
    ///
    /// - `f32` - 本帧秒数。
    pub fn step_death(&mut self, dt: f32) {
        self.set_death_timer((self.get_death_timer() - dt).max(0.0));
        self.set_flash((self.get_flash() - dt).max(0.0));
    }

    /// 尸体是否该被回收。
    ///
    /// **必须同时判「已死」**:活着���敌人的 `death_timer` 本来就是 0,
    /// 只判计时的话,一个活人也会被判成「尸体到期」,于是回收循环第一帧
    /// 就把刚生成的敌人全 `swap_remove` 掉了 —— 场上永远一个敌人都不剩。
    ///
    /// # Returns
    ///
    /// - `bool` - 已死且倒计时归零时为 `true`。
    pub fn death_expired(&self) -> bool {
        !self.is_alive() && self.get_death_timer() <= 0.0
    }

    /// 死亡剩余时间(秒)。
    ///
    /// # Returns
    ///
    /// - `f32` - 剩余秒数。
    pub fn get_death_timer(&self) -> f32 {
        self.death_timer
    }

    /// 就地复活为满血(空位补人时复用同一个索引,不做增删)。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 新的世界坐标。
    /// - `f32` - 新的朝向(弧度)。
    pub fn respawn(&mut self, position: Vec3, yaw: f32) {
        self.set_home(position);
        self.set_patrol_target([position[0], position[2]]);
        self.set_position(position);
        self.set_yaw(yaw);
        self.set_state(AiState::Idle);
        self.set_health(match self.get_faction() {
            Faction::Police => ENEMY_HEALTH,
            Faction::Thug => THUG_HEALTH,
        });
        self.set_death_timer(0.0);
        self.set_hurt_timer(0.0);
        self.set_flash(0.0);
        self.set_flee_timer(0.0);
        self.set_velocity([0.0, 0.0]);
    }

    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 新值。
    pub fn set_patrol_target(&mut self, value: Vec2) {
        self.patrol_target = value;
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前值。
    pub fn get_patrol_timer(&self) -> f32 {
        self.patrol_timer
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_patrol_timer(&mut self, value: f32) {
        self.patrol_timer = value;
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前值。
    pub fn get_flee_timer(&self) -> f32 {
        self.flee_timer
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_flee_timer(&mut self, value: f32) {
        self.flee_timer = value;
    }
    /// 写 `health`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_health(&mut self, value: f32) {
        self.health = value;
    }
    /// 写 `home`。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 新值。
    pub fn set_home(&mut self, value: Vec3) {
        self.home = value;
    }
    /// 写 `hurt_timer`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_hurt_timer(&mut self, value: f32) {
        self.hurt_timer = value;
    }
    /// 写 `death_timer`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_death_timer(&mut self, value: f32) {
        self.death_timer = value;
    }
    /// 写 `flash`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_flash(&mut self, value: f32) {
        self.flash = value;
    }
    /// 写 `wander`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_wander(&mut self, value: f32) {
        self.wander = value;
    }
    /// 写 `shots`。
    ///
    /// # Arguments
    ///
    /// - `u32` - 新值。
    pub fn set_shots(&mut self, value: u32) {
        self.shots = value;
    }
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
#[derive(Clone, Debug)]
pub struct Wanted {
    /// 当前热度。
    heat: f32,
    /// 当前星数(由热度算出,不单独存,避免两者不一致)。
    stars: u32,
    /// 距离上一次被警察看见过去了多久(秒)。
    unseen: f32,
    /// 上一帧是否有任何警察看得见玩家。
    spotted: bool,
}

impl Wanted {
    /// 干净的通缉状态。
    ///
    /// # Returns
    ///
    /// - `Self` - 零星无热度。
    pub fn new() -> Self {
        Self {
            heat: 0.0,
            stars: 0,
            unseen: WANTED_COOLDOWN,
            spotted: false,
        }
    }

    /// 当前星数。
    ///
    /// # Returns
    ///
    /// - `u32` - `0..=5`。
    pub fn get_stars(&self) -> u32 {
        self.stars
    }

    /// 当前热度。
    ///
    /// # Returns
    ///
    /// - `f32` - 连续热度值。
    pub fn get_heat(&self) -> f32 {
        self.heat
    }

    /// 连续「没被警察看见」的时长(秒)。
    ///
    /// # Returns
    ///
    /// - `f32` - 秒。
    pub fn get_unseen(&self) -> f32 {
        self.unseen
    }

    /// 记一次犯罪。
    ///
    /// # Arguments
    ///
    /// - `f32` - 本次行为累积的热度。
    pub fn add_heat(&mut self, amount: f32) {
        self.set_heat((self.get_heat() + amount).min(WANTED_PER_STAR * WANTED_MAX as f32));
        self.recompute_stars();
    }

    /// 重新按热度算星数,并返回是否发生了变化。
    ///
    /// # Returns
    ///
    /// - `u32` - 新的星数。
    pub fn recompute_stars(&mut self) -> u32 {
        let target: u32 = (self.get_heat() / WANTED_PER_STAR).floor().max(0.0) as u32;
        self.set_stars(target.min(WANTED_MAX));
        self.get_stars()
    }

    /// 每帧推进降温与升星。
    ///
    /// # Arguments
    ///
    /// - `f32` - 本帧秒数。
    /// - `bool` - 本帧是否被警察看见。
    /// - `bool` - 玩家是否站在藏身区(后巷 / 警局门口)。
    ///
    /// # Returns
    ///
    /// - `u32` - 推进后的星数。
    pub fn update(&mut self, dt: f32, spotted: bool, hidden: bool) -> u32 {
        self.set_spotted(spotted);
        if spotted {
            self.set_unseen(0.0);
        } else {
            self.set_unseen(self.get_unseen() + dt);
        }
        // 藏身区:强制降温且不重置「未被看见」的计时 —— GTA 里躲进警局
        // 门口之所以有效,是因为警员**看不见**你,而不是因为你跑得快。
        if hidden {
            self.set_unseen(self.get_unseen().max(WANTED_COOLDOWN));
            self.set_heat((self.get_heat() - HIDE_COOL_RATE * dt).max(0.0));
        } else if self.get_unseen() >= WANTED_COOLDOWN {
            self.set_heat(
                (self.get_heat() - WANTED_PER_STAR * WANTED_STEP_DOWN / WANTED_COOLDOWN * dt)
                    .max(0.0),
            );
        }
        self.recompute_stars()
    }

    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `bool` - 新值。
    pub fn set_spotted(&mut self, value: bool) {
        self.spotted = value;
    }
    /// 写 `heat`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_heat(&mut self, value: f32) {
        self.heat = value;
    }
    /// 写 `stars`。
    ///
    /// # Arguments
    ///
    /// - `u32` - 新值。
    pub fn set_stars(&mut self, value: u32) {
        self.stars = value;
    }
    /// 写 `unseen`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_unseen(&mut self, value: f32) {
        self.unseen = value;
    }
}

// ===========================================================================
// 任务
// ===========================================================================

/// 一个任务的运行时状态。
#[derive(Clone, Debug)]
pub struct Mission {
    /// 当前处于第几个阶段(见 `MISSION_*` 常量)。
    stage: u32,
    /// 任务名(展示用)。
    title: &'static str,
    /// 当前目标的世界坐标。
    target: Vec3,
    /// 「干掉某人」阶段要杀的那个敌人的索引。
    target_enemy: Option<usize>,
    /// 完成奖励(美元)。
    reward: f32,
}

impl Mission {
    /// 还没有接任务。
    ///
    /// # Returns
    ///
    /// - `Self` - 空闲状态。
    pub fn new() -> Self {
        Self {
            stage: MISSION_NONE,
            title: "",
            target: [0.0, 0.0, 0.0],
            target_enemy: None,
            reward: 0.0,
        }
    }

    /// 当前阶段。
    ///
    /// # Returns
    ///
    /// - `u32` - `MISSION_*` 之一。
    pub fn get_stage(&self) -> u32 {
        self.stage
    }

    /// 任务名。
    ///
    /// # Returns
    ///
    /// - `&'static str` - 任务标题。
    pub fn get_title(&self) -> &'static str {
        self.title
    }

    /// 目标坐标。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 目标点世界坐标。
    pub fn get_target(&self) -> Vec3 {
        self.target
    }

    /// 目标敌人索引。
    ///
    /// # Returns
    ///
    /// - `Option<usize>` - 「干掉某人」阶段的目标。
    pub fn get_target_enemy(&self) -> Option<usize> {
        self.target_enemy
    }

    /// 报酬。
    ///
    /// # Returns
    ///
    /// - `f32` - 完成后加的钱。
    pub fn get_reward(&self) -> f32 {
        self.reward
    }

    /// 是否有任务在进行。
    ///
    /// # Returns
    ///
    /// - `bool` - 进行中为 `true`。
    pub fn is_active(&self) -> bool {
        self.get_stage() != MISSION_NONE
    }

    /// 接受一个任务。
    ///
    /// # Arguments
    ///
    /// - `&'static str` - 任务名。
    /// - `u32` - 初始阶段。
    /// - `Vec3` - 目标坐标。
    pub fn accept(&mut self, title: &'static str, stage: u32, target: Vec3) {
        self.set_stage(stage);
        self.set_title(title);
        self.set_target(target);
        self.set_reward(MISSION_REWARD);
        self.set_target_enemy_opt(None);
    }

    /// 设定「干掉某人」的目标。
    ///
    /// # Arguments
    ///
    /// - `usize` - 目标敌人的索引。
    pub fn set_target_enemy(&mut self, index: usize) {
        self.target_enemy = Some(index);
    }
    /// 写 `target_enemy`(可为 `None`)。
    ///
    /// # Arguments
    ///
    /// - `Option<usize>` - 新值。
    pub fn set_target_enemy_opt(&mut self, value: Option<usize>) {
        self.target_enemy = value;
    }

    /// 推进到下一个阶段。
    ///
    /// # Arguments
    ///
    /// - `u32` - 新阶段。
    /// - `Vec3` - 新的目标坐标。
    pub fn advance(&mut self, stage: u32, target: Vec3) {
        self.set_stage(stage);
        self.set_target(target);
        self.set_target_enemy_opt(None);
    }

    /// 结算并清空任务。
    pub fn complete(&mut self) {
        self.set_stage(MISSION_NONE);
        self.set_title("");
        self.set_target([0.0, 0.0, 0.0]);
        self.set_target_enemy_opt(None);
        self.set_reward(0.0);
    }
    /// 写 `stage`。
    ///
    /// # Arguments
    ///
    /// - `u32` - 新值。
    pub fn set_stage(&mut self, value: u32) {
        self.stage = value;
    }
    /// 写 `title`。
    ///
    /// # Arguments
    ///
    /// - `&'static str` - 新值。
    pub fn set_title(&mut self, value: &'static str) {
        self.title = value;
    }
    /// 写 `target`。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 新值。
    pub fn set_target(&mut self, value: Vec3) {
        self.target = value;
    }
    /// 写 `reward`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_reward(&mut self, value: f32) {
        self.reward = value;
    }
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
#[derive(Clone, Debug)]
pub struct Pedestrian {
    /// 世界坐标。
    position: Vec3,
    /// 朝向(弧度)。
    yaw: f32,
    /// 巡航目标点(XZ)。
    goal: Vec2,
    /// 当前水平速度。
    velocity: Vec2,
    /// 步态相位(弧度)。
    gait_phase: f32,
    /// 步态幅度 0..1。
    gait_amount: f32,
    /// 剩余逃跑时间(秒)。
    flee_timer: f32,
    /// 被撞飞后的倒地倒计时(秒)。
    down_timer: f32,
    /// 是否曾经被撞倒过:区分「没倒过」和「倒完已经过去」。
    has_fallen: bool,
    /// 击飞速度(米/秒),仅倒地时有意义。
    knock: Vec2,
    /// 使用的行人模型 id(4 选 1)。
    model: &'static str,
}

impl Pedestrian {
    /// 生成一个行人。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 世界坐标。
    /// - `f32` - 初始朝向。
    /// - `Vec2` - 巡航目标点。
    /// - `&'static str` - 模型 id。
    /// - `f32` - 初始步态相位。
    ///
    /// # Returns
    ///
    /// - `Self` - 就绪的行人。
    pub fn new(position: Vec3, yaw: f32, goal: Vec2, model: &'static str, gait_phase: f32) -> Self {
        Self {
            position,
            yaw,
            goal,
            velocity: [0.0, 0.0],
            gait_phase,
            gait_amount: 0.0,
            flee_timer: 0.0,
            down_timer: 0.0,
            has_fallen: false,
            knock: [0.0, 0.0],
            model,
        }
    }

    /// 世界坐标。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 当前坐标。
    pub fn get_position(&self) -> Vec3 {
        self.position
    }

    /// 朝向。
    ///
    /// # Returns
    ///
    /// - `f32` - 绕 Y 轴弧度。
    pub fn get_yaw(&self) -> f32 {
        self.yaw
    }

    /// 步态相位。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前相位(弧度)。
    pub fn get_gait_phase(&self) -> f32 {
        self.gait_phase
    }

    /// 步态幅度。
    ///
    /// # Returns
    ///
    /// - `f32` - `0.0` 站立,`1.0` 全速。
    pub fn get_gait_amount(&self) -> f32 {
        self.gait_amount
    }

    /// 使用的模型。
    ///
    /// # Returns
    ///
    /// - `&'static str` - 行人资产 id。
    pub fn get_model(&self) -> &'static str {
        self.model
    }

    /// 是否被撞倒在地。
    ///
    /// # Returns
    ///
    /// - `bool` - 倒地未消失时为 `true`。
    pub fn is_down(&self) -> bool {
        self.get_down_timer() > 0.0
    }

    /// 被车撞飞。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 击飞方向 × 速度(米/秒)。
    pub fn knock_down(&mut self, impulse: Vec2) {
        self.set_has_fallen(true);
        self.set_knock(impulse);
        self.set_down_timer(PED_DOWN_TIME);
        self.set_flee_timer(0.0);
    }

    /// 是否已经被处理掉(可以从列表里移除)。
    ///
    /// 判据是**倒计时归零过**:刚生成的行人 `down_timer` 本来就是 0,
    /// 只判 `down_timer <= 0` 的话,站着走路的行人会被当��「已经处理完」,
    /// 第一帧就被回收光 —— 街上一个人都不剩。所以必须再要求
    /// 「曾经倒过地」(见 `has_fallen`)。
    ///
    /// # Returns
    ///
    /// - `bool` - 该回收时为 `true`。
    pub fn is_gone(&self) -> bool {
        self.get_has_fallen()
            && self.get_down_timer() <= 0.0
            && self.get_knock()[0].abs() < f32::EPSILON
    }

    /// 推进一个固定步长:走向目标 / 逃跑 / 倒地弹飞。
    ///
    /// 移动复用玩家那套「实际位移 / dt」回算,所以贴墙会滑行;
    /// 步态相位按走过的距离推进,停下就归位。
    ///
    /// # Arguments
    ///
    /// - `f32` - 固定步长(秒)。
    /// - `&CollisionWorld` - 静态碰撞世界。
    /// - `Vec3` - 玩家坐标(用来判断要不要躲)。
    /// - `bool` - 玩家是否在开车(开车更危险)。
    pub fn step(&mut self, dt: f32, world: &CollisionWorld, player_at: Vec3, driving: bool) {
        if self.get_down_timer() > 0.0 {
            self.set_down_timer(self.get_down_timer() - dt);
            // 击飞:按当前击飞速度匀速飞出去,直到停下来。
            let here: Vec3 = self.get_position();
            let wanted: Vec2 = [
                here[0] + self.get_knock()[0] * dt,
                here[2] + self.get_knock()[1] * dt,
            ];
            let resolved: Vec2 = world.resolve_with_radius(wanted, PED_RADIUS);
            self.set_knock([
                (resolved[0] - here[0]) / dt.max(f32::EPSILON),
                (resolved[1] - here[2]) / dt.max(f32::EPSILON),
            ]);
            self.set_position([resolved[0], here[1], resolved[1]]);
            if self.get_knock()[0].abs() < 0.2 && self.get_knock()[1].abs() < 0.2 {
                self.set_knock([0.0, 0.0]);
                self.set_gait_amount(0.0);
            }
            return;
        }
        // 危险感知:玩家在附近 / 开车靠近 → 跑开。
        let distance: f32 = crate::combat::flat_distance(self.get_position(), player_at);
        let alert: f32 = if driving {
            PED_ALERT_RADIUS * 1.8
        } else {
            PED_ALERT_RADIUS
        };
        if distance < alert {
            self.set_flee_timer(PED_FLEE_TIME);
        }
        let fleeing: bool = self.get_flee_timer() > 0.0;
        if fleeing {
            self.set_flee_timer(self.get_flee_timer() - dt);
        }
        // 目标:逃跑时背对玩家,否则走向巡航点;到点就换一个。
        let mut target: Vec2 = self.get_goal();
        if fleeing {
            let dx: f32 = self.get_position()[0] - player_at[0];
            let dz: f32 = self.get_position()[2] - player_at[2];
            let length: f32 = (dx * dx + dz * dz).sqrt().max(f32::EPSILON);
            target = [
                self.get_position()[0] + dx / length * 8.0,
                self.get_position()[2] + dz / length * 8.0,
            ];
        } else {
            let reached: f32 =
                crate::combat::flat_distance(self.get_position(), [target[0], 0.0, target[1]]);
            if reached < 1.4 {
                // 换一个巡航点:绕当前点随机转个角度。
                let angle: f32 = self.get_gait_phase() * 0.37 + self.get_yaw();
                target = [
                    self.get_position()[0] + angle.cos() * 7.0,
                    self.get_position()[2] + angle.sin() * 7.0,
                ];
            }
        }
        self.set_goal(target);
        let here: Vec3 = self.get_position();
        let to: Vec2 = [target[0] - here[0], target[1] - here[2]];
        let length: f32 = (to[0] * to[0] + to[1] * to[1]).sqrt();
        let speed: f32 = if fleeing { PED_FLEE_SPEED } else { PED_SPEED };
        let velocity: Vec2 = if length > f32::EPSILON {
            [to[0] / length * speed, to[1] / length * speed]
        } else {
            [0.0, 0.0]
        };
        let wanted_at: Vec2 = [here[0] + velocity[0] * dt, here[2] + velocity[1] * dt];
        let resolved: Vec2 = world.resolve_with_radius(wanted_at, PED_RADIUS);
        let achieved: Vec2 = [resolved[0] - here[0], resolved[1] - here[2]];
        self.set_velocity([
            achieved[0] / dt.max(f32::EPSILON),
            achieved[1] / dt.max(f32::EPSILON),
        ]);
        self.set_position([resolved[0], here[1], resolved[1]]);
        // 朝向 + 步态:用「打算走的方向」,所以贴着墙走时手脚照样摆。
        let planar: f32 = (to[0] * to[0] + to[1] * to[1]).sqrt();
        if planar > 0.05 {
            let desired: f32 = -to[1].atan2(to[0]);
            self.set_yaw(
                self.get_yaw() + wrap_angle(desired - self.get_yaw()) * PED_TURN_RATE * dt,
            );
            let ratio: f32 = (planar / speed).clamp(0.0, 1.0);
            let relaxed: f32 = (1.0 - (-PED_RELAX_RATE * dt).exp()).clamp(0.0, 1.0);
            self.set_gait_amount(
                self.get_gait_amount() + (ratio - self.get_gait_amount()) * relaxed,
            );
            self.set_gait_phase(
                (self.get_gait_phase() + planar * PED_GAIT_RATE * dt)
                    % (2.0 * std::f32::consts::PI),
            );
        } else {
            let relaxed: f32 = (1.0 - (-PED_RELAX_RATE * dt).exp()).clamp(0.0, 1.0);
            self.set_gait_amount(self.get_gait_amount() - self.get_gait_amount() * relaxed);
            self.set_gait_phase(0.0);
        }
        self.set_gait_amount(self.get_gait_amount().clamp(0.0, 1.0));
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 当前值。
    pub fn get_goal(&self) -> Vec2 {
        self.goal
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 新值。
    pub fn set_goal(&mut self, value: Vec2) {
        self.goal = value;
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前值。
    pub fn get_flee_timer(&self) -> f32 {
        self.flee_timer
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_flee_timer(&mut self, value: f32) {
        self.flee_timer = value;
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前值。
    pub fn get_down_timer(&self) -> f32 {
        self.down_timer
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_down_timer(&mut self, value: f32) {
        self.down_timer = value;
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `bool` - 当前值。
    pub fn get_has_fallen(&self) -> bool {
        self.has_fallen
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `bool` - 新值。
    pub fn set_has_fallen(&mut self, value: bool) {
        self.has_fallen = value;
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 当前值。
    pub fn get_knock(&self) -> Vec2 {
        self.knock
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 新值。
    pub fn set_knock(&mut self, value: Vec2) {
        self.knock = value;
    }
    /// 写 `position`。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 新值。
    pub fn set_position(&mut self, value: Vec3) {
        self.position = value;
    }
    /// 写 `yaw`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_yaw(&mut self, value: f32) {
        self.yaw = value;
    }

    /// 写 `velocity`。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 新值。
    pub fn set_velocity(&mut self, value: Vec2) {
        self.velocity = value;
    }
    /// 写 `gait_phase`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_gait_phase(&mut self, value: f32) {
        self.gait_phase = value;
    }
    /// 写 `gait_amount`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_gait_amount(&mut self, value: f32) {
        self.gait_amount = value;
    }
}

// ===========================================================================
// 战斗总控
// ===========================================================================

/// 玩家受击后的短暂无敌,避免被一帧内多人集火打空。
#[derive(Clone, Copy, Debug)]
pub struct HurtState {
    /// 无敌剩余时间(秒)。
    invuln: f32,
    /// 距上次受击多久(秒),用于护甲脱战回复。
    since_hit: f32,
    /// 死亡后的重生倒计时(秒)。
    respawn: f32,
    /// 是否已倒地。
    wasted: bool,
}

impl HurtState {
    /// 初始状态:不无敌,满血站着。
    ///
    /// # Returns
    ///
    /// - `Self` - 干净状态。
    pub fn new() -> Self {
        Self {
            invuln: 0.0,
            since_hit: 999.0,
            respawn: 0.0,
            wasted: false,
        }
    }

    /// 是否正处于无敌帧。
    ///
    /// # Returns
    ///
    /// - `bool` - 无敌中为 `true`。
    pub fn is_invulnerable(&self) -> bool {
        self.get_invuln() > 0.0
    }

    /// 是否倒地。
    ///
    /// # Returns
    ///
    /// - `bool` - 血量归零后为 `true`。
    pub fn is_wasted(&self) -> bool {
        self.get_wasted()
    }

    /// 重生倒计时。
    ///
    /// # Returns
    ///
    /// - `f32` - 剩余秒数。
    pub fn get_respawn(&self) -> f32 {
        self.respawn
    }

    /// 距上次受击过去了多久(秒)。
    ///
    /// # Returns
    ///
    /// - `f32` - 秒;刚开局就是 999(视为「早就脱战」)。
    pub fn get_since_hit(&self) -> f32 {
        self.since_hit
    }

    /// 推进无敌 / 受击计时 / 重生倒计时。
    ///
    /// # Arguments
    ///
    /// - `f32` - 本帧秒数。
    pub fn advance(&mut self, dt: f32) {
        if self.get_invuln() > 0.0 {
            self.set_invuln((self.get_invuln() - dt).max(0.0));
        }
        if self.get_since_hit() < 999.0 {
            self.set_since_hit(self.get_since_hit() + dt);
        }
        if self.get_respawn() > 0.0 {
            self.set_respawn((self.get_respawn() - dt).max(0.0));
        }
    }

    /// 记一次被击中:进入无敌帧并重置脱战计时。
    ///
    /// # Arguments
    ///
    /// - `f32` - 无敌时间(秒)。
    pub fn on_hit(&mut self, invuln: f32) {
        self.set_invuln(invuln);
        self.set_since_hit(0.0);
    }

    /// 标记倒地并开始重生倒计时。
    ///
    /// # Arguments
    ///
    /// - `f32` - 重生延迟(秒)。
    pub fn waste(&mut self, delay: f32) {
        self.set_wasted(true);
        self.set_respawn(delay);
        self.set_since_hit(0.0);
    }

    /// 复位到「没死」的状态。
    pub fn revive(&mut self) {
        self.set_wasted(false);
        self.set_respawn(0.0);
        self.set_invuln(PLAYER_HIT_INVULN);
        self.set_since_hit(999.0);
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `f32` - 当前值。
    pub fn get_invuln(&self) -> f32 {
        self.invuln
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_invuln(&mut self, value: f32) {
        self.invuln = value;
    }

    /// 读 `field`。
    ///
    /// # Returns
    ///
    /// - `bool` - 当前值。
    pub fn get_wasted(&self) -> bool {
        self.wasted
    }
    /// 写 `field`。
    ///
    /// # Arguments
    ///
    /// - `bool` - 新值。
    pub fn set_wasted(&mut self, value: bool) {
        self.wasted = value;
    }
    /// 写 `since_hit`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_since_hit(&mut self, value: f32) {
        self.since_hit = value;
    }
    /// 写 `respawn`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新值。
    pub fn set_respawn(&mut self, value: f32) {
        self.respawn = value;
    }
}

/// 距离衰减:近处满伤,超过 `FALLOFF_START` 后线性掉到 `FALLOFF_MIN`。
///
/// 球棒不衰减(近战本来就是贴脸)。
///
/// # Arguments
///
/// - `Weapon` - 开火的武器。
/// - `f32` - 命中点到射手的距离(米)。
///
/// # Returns
///
/// - `f32` - 实际造成的伤害。
pub fn falloff(weapon: Weapon, distance: f32) -> f32 {
    let base: f32 = weapon.damage();
    if weapon == Weapon::Bat {
        return base;
    }
    if distance <= FALLOFF_START {
        return base;
    }
    let span: f32 = (GUN_RANGE - FALLOFF_START).max(f32::EPSILON);
    let t: f32 = ((distance - FALLOFF_START) / span).clamp(0.0, 1.0);
    let scale: f32 = 1.0 + (FALLOFF_MIN - 1.0) * t;
    base * scale
}

/// 两个 XZ 点之间的平面距离。
///
/// # Arguments
///
/// - `Vec3` - 起点。
/// - `Vec3` - 终点。
///
/// # Returns
///
/// - `f32` - 米。
pub fn flat_distance(a: Vec3, b: Vec3) -> f32 {
    let dx: f32 = a[0] - b[0];
    let dz: f32 = a[2] - b[2];
    (dx * dx + dz * dz).sqrt()
}

/// 视线是否通畅:从 `from` 到 `to` 不会被静态碰撞体挡住。
///
/// 复用相机遮挡回避的球体推进(`ray_to_shapes` 返回 `None` = 没被挡)。
/// 敌人有视线判定之后,躲到楼后就真的能甩掉警察,而不是隔墙挨打。
///
/// # Arguments
///
/// - `&CollisionWorld` - 静态碰撞世界。
/// - `Vec3` - 观察者(取 XZ)。
/// - `Vec3` - 目标(取 XZ)。
/// - `f32` - 最大探测距离(米)。
///
/// # Returns
///
/// - `bool` - 通畅为 `true`。
pub fn has_line_of_sight(world: &CollisionWorld, from: Vec3, to: Vec3, max_range: f32) -> bool {
    let dx: f32 = to[0] - from[0];
    let dz: f32 = to[2] - from[2];
    let length: f32 = (dx * dx + dz * dz).sqrt();
    if length <= f32::EPSILON {
        return true;
    }
    let direction: Vec3 = [dx / length, 0.0, dz / length];
    let reached: f32 = length.min(max_range);
    match crate::collision::ray_to_shapes(world, from, direction) {
        None => true,
        Some(hit) => hit.0 > reached,
    }
}

/// 把一个位置转向另一个位置(取最短角,避免转 359°)。
///
/// # Arguments
///
/// - `f32` - 当前朝向。
/// - `Vec3` - 当前坐标。
/// - `Vec3` - 目标坐标。
///
/// # Returns
///
/// - `f32` - 期望的新朝向(弧度)。
pub fn face_towards(yaw: f32, from: Vec3, to: Vec3) -> f32 {
    let desired: f32 = -(to[2] - from[2]).atan2(to[0] - from[0]);
    yaw + wrap_angle(desired - yaw)
}

/// 玩家的受伤结算入口:护甲优先吃伤害,护甲空后掉血。
///
/// # Arguments
///
/// - `&mut Player` - 玩家。
/// - `f32` - 原始伤害。
///
/// # Returns
///
/// - `f32` - 真正扣掉的血量。
pub fn apply_damage(player: &mut Player, amount: f32) -> f32 {
    let armor: f32 = player.get_armor();
    if armor > 0.0 {
        let absorbed: f32 = amount * ARMOR_ABSORB;
        let to_armor: f32 = absorbed.min(armor);
        player.set_armor(armor - to_armor);
        let through: f32 = amount - to_armor;
        if through > 0.0 {
            player.set_health((player.get_health() - through).max(0.0));
        }
        return through.min(amount);
    }
    player.set_health((player.get_health() - amount).max(0.0));
    amount
}

/// 护甲的脱战回复推进。
///
/// # Arguments
///
/// - `&mut Player` - 玩家。
/// - `&mut HurtState` - 受击状态。
/// - `f32` - 本帧秒数。
pub fn regen_armor(player: &mut Player, hurt: &mut HurtState, dt: f32) {
    hurt.advance(dt);
    if hurt.get_since_hit() < ARMOR_REGEN_DELAY {
        return;
    }
    let healed: f32 = (player.get_armor() + ARMOR_REGEN * dt).min(MAX_ARMOR);
    player.set_armor(healed);
}

/// 把一个方向转成模型矩阵(带 Y 偏移与缩放)。
///
/// # Arguments
///
/// - `Vec3` - 世界坐标。
/// - `f32` - 朝向(弧度)。
/// - `f32` - 均匀缩放。
///
/// # Returns
///
/// - `Mat4` - 可直接喂给 `Instance::from_matrix` 的矩阵。
pub fn body_matrix(at: Vec3, yaw: f32, scale: f32) -> Mat4 {
    let (sin_yaw, cos_yaw): (f32, f32) = yaw.sin_cos();
    Mat4::from_column_major([
        cos_yaw * scale,
        0.0,
        -sin_yaw * scale,
        0.0,
        0.0,
        scale,
        0.0,
        0.0,
        sin_yaw * scale,
        0.0,
        cos_yaw * scale,
        0.0,
        at[0],
        at[1],
        at[2],
        1.0,
    ])
}
