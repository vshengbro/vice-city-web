use super::*;

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
            Weapon::Grenade => WEP_GRENADE,
            // 徒手没有模型资产。返回空串而不是编一个 id:`rebind_weapon_batch`
            // 查不到就是 `usize::MAX`,手持批次自然被跳过(见
            // `sync_combat_instances` 的 `batch != usize::MAX` 分支)。
            Weapon::Unarmed => "",
        }
    }

    /// 由资产 id 反查武器。
    ///
    /// 拾取流程需要「地上的模型 id → 手里的武器」这一步反向映射:玩家
    /// 捡起的是 `wep_smg` 这个资产,而战斗系统操作的是 `Weapon::Smg`
    /// 这个枚举。之前没有这个函数,所以地上的武器模型被拾起来之后
    /// **只弹一行「捡到武器」的提示**,枪里既没换武器、手上也没模型
    /// —— 用户报的「武器捡到之后无法展示和射击」。
    ///
    /// # Arguments
    ///
    /// - `&str` - 武器资产 id。
    ///
    /// # Returns
    ///
    /// - `PickupWeapon` - 对应的武器;不是武器资产时为 `None`。
    pub fn from_asset(asset: &str) -> PickupWeapon {
        match asset {
            WEP_PISTOL => Some(Weapon::Pistol),
            WEP_SMG => Some(Weapon::Smg),
            WEP_BAT => Some(Weapon::Bat),
            WEP_GRENADE => Some(Weapon::Grenade),
            // 徒手不是地上的模型,不能被 `from_asset` 反查出来 ——
            // 否则空串会反过来认成徒手。
            _ => None,
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
            Weapon::Grenade => GRENADE_TUBES,
            Weapon::Unarmed => 0,
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
            Weapon::Grenade => GRENADE_DAMAGE,
            // 徒手打不出伤害:按 H 收起之后开火键不该结算任何东西。
            Weapon::Unarmed => 0.0,
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
            Weapon::Grenade => 1.0 / GRENADE_COOLDOWN,
            Weapon::Unarmed => 0.0,
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
            Weapon::Grenade => GRENADE_RANGE,
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
            Weapon::Grenade => WEAPON_NAME_GRENADE,
            Weapon::Unarmed => WEAPON_NAME_UNARMED,
        }
    }
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
            grenade_tubes: 0,
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
            Weapon::Pistol => self.get_pistol_ammo(),
            Weapon::Smg => self.get_smg_ammo(),
            Weapon::Bat => 0,
            Weapon::Grenade => self.get_grenade_tubes(),
            Weapon::Unarmed => 0,
        }
    }

    /// 当前武器的备弹(弹匣之外的)。
    ///
    /// # Returns
    ///
    /// - `u32` - 备弹数量。
    pub fn get_reserve(&self) -> u32 {
        match self.get_weapon() {
            Weapon::Pistol => self.get_pistol_reserve(),
            Weapon::Smg => self.get_smg_reserve(),
            // 球棒 / 手雷 / 徒手都没有「备弹」概念:要补充得在地上再捡。
            Weapon::Bat | Weapon::Grenade | Weapon::Unarmed => 0,
        }
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
            Weapon::Bat | Weapon::Grenade => true,
            // 徒手永远不会「能开火」——这是 `fire_weapon` 唯一的早退闸门。
            Weapon::Unarmed => false,
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
    /// 弹匣打空时**不会**自动换弹 —— 换弹必须是玩家的显式动作(按 R),
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
                    Weapon::Pistol => {
                        self.set_pistol_ammo(left);
                    }
                    _ => {
                        self.set_smg_ammo(left);
                    }
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
        if matches!(self.get_weapon(), Weapon::Bat | Weapon::Unarmed) || self.get_reloading() > 0.0
        {
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
            Weapon::Bat | Weapon::Unarmed => {}
            Weapon::Grenade => {
                self.set_grenade_tubes((self.get_grenade_tubes() + rounds).min(capacity));
            }
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
            Weapon::Bat | Weapon::Grenade | Weapon::Unarmed => {}
        }
    }

    /// 投掉一颗手雷:数量 -1,手上没有就不给投。
    ///
    /// 计数和「有没有投出去」必须放在**一处**判:`throw_grenade` 先看
    /// `get_grenade_tubes() == 0` 再调本函数,两次判之间没有别的写者,
    /// 所以这里仍要自己再判一次 —— 它是这条路径上的最后一道闸门。
    ///
    /// # Returns
    ///
    /// - `bool` - 是否真的投出了一颗(没有则 `false`)。
    pub fn take_grenade(&mut self) -> bool {
        let now: u32 = self.get_grenade_tubes();
        if now == 0 {
            return false;
        }
        self.set_grenade_tubes(now - 1);
        true
    }

    /// 捡起一颗手雷:数量 +1,不超过 `GRENADE_TUBES`。
    ///
    /// 这是「地上的 `wep_grenade` 捡起来要有用」的那一步 —— 之前
    /// `apply_combat_pickup` 只认护甲和弹药箱,武器模型被拾起来之后
    /// 只弹一行提示,手雷数量永远是 0。
    ///
    /// # Returns
    ///
    /// - `bool` - 是否真的收下了(已经满管时为 `false`)。
    pub fn add_grenade(&mut self) -> bool {
        let now: u32 = self.get_grenade_tubes();
        if now >= GRENADE_TUBES {
            return false;
        }
        self.set_grenade_tubes(now + 1);
        true
    }
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

    /// 是否还活着。
    ///
    /// # Returns
    ///
    /// - `bool` - 存活为 `true`。
    pub fn is_alive(&self) -> bool {
        *self.get_state() != AiState::Dead
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

    /// 记一次开火:累加枪数。
    pub fn mark_shot(&mut self) {
        self.set_shots(self.get_shots() + 1);
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
        self.set_wander((self.get_wander() + ENEMY_WANDER_RATE * dt) % TAU);
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
            talk_center: goal,
            talk_timer: 0.0,
            talk_total: 0.0,
            // 用相位错开每个人的站位,否则同一堆人会全部挤到圆心一点上。
            talk_slot: gait_phase,
            gather_cooldown: 0.0,
        }
    }

    /// 还在聊天吗。
    ///
    /// # Returns
    ///
    /// - `bool` - `talk_timer > 0` 时为真。
    pub fn is_talking(&self) -> bool {
        self.get_talk_timer() > 0.0
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
        //
        // 开车时的放大系数必须**小于** 1:放大警觉半径等于让行人更早开始跑,
        // 而跑得越早,车越追不上 —— 1.8 那个值让行人从 16.2 m 外就逃窜,
        // 24 m/s 的车永远进不了 `CAR_HIT_RADIUS` 的判定圈(实测一次都没撞到过)。
        // 真正该有的手感是「车逼近才躲」,所以车只给一点点提前量。
        let distance: f32 = crate::combat::flat_distance(self.get_position(), player_at);
        let alert: f32 = if driving {
            PED_ALERT_RADIUS * PED_ALERT_CAR_SCALE
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
        // 目标:逃跑时背对玩家;正在聊天就走向圈上的站位;否则走向
        // 巡航点,到点换一个。
        let mut target: Vec2 = self.get_goal();
        if self.is_talking() {
            // 站进圈子:目标是「圈心 + 自己那个方位」。走到位之后
            // `set_goal` 每次都把它压回同一个点,于是行人自然停住 ——
            // 不需要额外的「站定」分支,行为由目标点唯一决定。
            let center: Vec2 = self.get_talk_center();
            let slot: f32 = self.get_talk_slot();
            target = [
                center[0] + slot.cos() * PED_TALK_RING_RADIUS,
                center[1] + slot.sin() * PED_TALK_RING_RADIUS,
            ];
        }
        if fleeing {
            let dx: f32 = self.get_position()[0] - player_at[0];
            let dz: f32 = self.get_position()[2] - player_at[2];
            let length: f32 = (dx * dx + dz * dz).sqrt().max(f32::EPSILON);
            target = [
                self.get_position()[0] + dx / length * 8.0,
                self.get_position()[2] + dz / length * 8.0,
            ];
        } else if !self.is_talking() {
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
            // 聊天时面朝**圈心**(同伴),而不是面朝走路方向 —— 一堆人
            // 背对背「聊天」是最明显的假信号。
            let desired: f32 = if self.is_talking() {
                let center: Vec2 = self.get_talk_center();
                let facing: Vec2 = [center[0] - here[0], center[1] - here[2]];
                -facing[1].atan2(facing[0])
            } else {
                -to[1].atan2(to[0])
            };
            self.set_yaw(
                self.get_yaw() + wrap_angle(desired - self.get_yaw()) * PED_TURN_RATE * dt,
            );
            let ratio: f32 = (planar / speed).clamp(0.0, 1.0);
            let relaxed: f32 = (1.0 - (-PED_RELAX_RATE * dt).exp()).clamp(0.0, 1.0);
            self.set_gait_amount(
                self.get_gait_amount() + (ratio - self.get_gait_amount()) * relaxed,
            );
            self.set_gait_phase((self.get_gait_phase() + planar * PED_GAIT_RATE * dt) % (2.0 * PI));
        } else {
            let relaxed: f32 = (1.0 - (-PED_RELAX_RATE * dt).exp()).clamp(0.0, 1.0);
            self.set_gait_amount(self.get_gait_amount() - self.get_gait_amount() * relaxed);
            self.set_gait_phase(0.0);
        }
        self.set_gait_amount(self.get_gait_amount().clamp(0.0, 1.0));
    }
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
}
