use super::*;

impl Player {
    /// 累加护甲(拾取护甲背心时用)。
    ///
    /// # Arguments
    ///
    /// - `f32` - 增量(可以为负)。
    pub fn add_armor(&mut self, amount: f32) {
        self.set_armor((self.get_armor() + amount).clamp(0.0, MAX_ARMOR));
    }

    /// 累加现金。
    ///
    /// # Arguments
    ///
    /// - `f32` - 增量(可以为负)。
    pub fn set_cash_add(&mut self, amount: f32) {
        self.cash += amount;
    }

    /// 玩家是否坐在车里。
    ///
    /// # Returns
    ///
    /// - `bool` - 在车里时为 `true`。
    pub fn get_driving(&self) -> bool {
        self.vehicle.is_some()
    }

    /// 已被拾取掉的拾取物索引。
    ///
    /// # Returns
    ///
    /// - `&Vec<usize>` - 已拾取的索引列表。
    pub fn get_collected_ref(&self) -> &Vec<usize> {
        &self.collected
    }

    /// 追加一个已拾取的拾取物索引。
    ///
    /// # Arguments
    ///
    /// - `usize` - 拾取物索引。
    pub fn set_collected_push(&mut self, index: usize) {
        self.collected.push(index);
    }

    /// 已被拾取的数量。
    ///
    /// # Returns
    ///
    /// - `usize` - 已拾取的拾取物个数。
    pub fn collected_count(&self) -> usize {
        self.get_collected_ref().len()
    }

    /// 起跳(键位写入入口):给一个向上的初速度,并立刻离开「站着」状态。
    ///
    /// **必须同时把 `grounded` 打成 false。** 之前这个方法不存在,
    /// `vertical_velocity` 只由 `step_vertical` 在「离开支撑面」的
    /// 自由落体分支里写 —— 也就是说玩家站着时往上写一个正速度,
    /// 下一帧 `support - y <= STEP_UP_TOLERANCE` 依然成立,立刻被
    /// `set_vertical_velocity(0.0)` 抹掉。这正是「按了 Space 原地
    /// 弹一下就停」的那种假跳跃:速度被地板吃掉,人根本没离开地面。
    ///
    /// 初速度取 [`JUMP_VELOCITY`],顶高 1.245 m(见该常量的推导),
    /// 滞空 0.67 s。滞空途中的重力与落地判定都由 `step_vertical` 走
    /// **同一套**积分,这里不重复实现。
    ///
    /// # Arguments
    ///
    /// - `bool` - 上一帧是否踩在支撑面上;滞空时不得二次起跳。
    ///
    /// # Returns
    ///
    /// - `bool` - 本次是否真的起跳(滞空时为 `false`)。
    pub fn set_jump_requested(&mut self, was_grounded: bool) -> bool {
        if !was_grounded {
            return false;
        }
        // 经已有的 `set_*` 写入口,而不是 `self.field` 直写(§17.3/§17.12:
        // accessor 体内直写合法,业务方法里不合法)。
        self.set_vertical_velocity(JUMP_VELOCITY);
        self.set_grounded(false);
        true
    }

    /// 在给定位置创建一个满血玩家。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 初始世界坐标。
    /// - `f32` - 初始朝向(弧度)。
    ///
    /// # Returns
    ///
    /// - `Self` - 就绪的玩家状态。
    pub fn new(position: Vec3, yaw: f32) -> Self {
        Self {
            position,
            yaw,
            velocity: [0.0, 0.0],
            gait_phase: 0.0,
            gait_amount: 0.0,
            health: MAX_HEALTH,
            armor: 0.0,
            cash: 0.0,
            vehicle: None,
            notice: String::new(),
            collected: Vec::new(),
            height: PLAYER_BODY_HEIGHT,
            vertical_velocity: 0.0,
            grounded: true,
        }
    }

    /// 把两轴按键输入的模长钳到不超过 1(斜向不快于直线的关键)。
    ///
    /// **钳长度,不是无脑除以模长。** 无脑归一化(`intent / |intent|`)会把
    /// 半个身位的输入 `(0.5, 0.5)`(模长 0.707)放大成满速 —— 手柄半推和
    /// 键盘全按之间就失去了区别,微调会变成冲刺。正确做法是**只把超过 1 的
    /// 模长压下来**,模长不足 1 的原样保留,于是半速输入仍然是半速。
    ///
    /// 效果:W 给出 `1`,W+D 给出 `1 / √2 = 0.7071`,于是合速度恒为
    /// `speed`(`speed * |intent|` ≤ `speed * 1`)。单轴输入时模长本来就是
    /// 1,`scale` 恒为 1,直线档位**逐位不变**。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 原始的两轴输入(各分量 −1..1)。
    ///
    /// # Returns
    ///
    /// - `f32` - 缩放系数,模长超过 1 时为 `1 / |intent|`,否则为 1。
    fn diagonal_scale(intent: Vec2) -> f32 {
        let length: f32 = (intent[0] * intent[0] + intent[1] * intent[1]).sqrt();
        if length > 1.0 { 1.0 / length } else { 1.0 }
    }

    /// 推进一个固定步长:移动、碰撞分离、转身、步态相位。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 期望的前 / 侧向输入(各分量 −1..1)。
    /// - `Vec2` - 相机「前」方向的世界 XZ 分量。
    /// - `f32` - 固定步长(秒)。
    /// - `f32` - 目标速度上限(米/秒)。
    /// - `&CollisionWorld` - 静态碰撞世界。
    pub fn step(
        &mut self,
        intent: Vec2,
        direction: Vec2,
        dt: f32,
        speed: f32,
        world: &CollisionWorld,
    ) {
        // 目标速度 = 前向分量 + 侧向分量,两者**合起来**受 `speed` 上限约束,
        // 所以斜向移动不比直线快(和 GTA V 的手感一致:那边按 W+D 也不会比
        // 单按 W 快)。
        //
        // **「各自受上限」并不够。** 这里原来直接把
        // `direction * forward + perp * strafe` 乘 `speed`,于是两轴分别顶到
        // `speed` 时合速度是 `speed * √(forward² + strafe²)` —— W+D 时
        // `√2 = 1.4142`。实测步行 6.5052(`4.6 * √2`,直线是 4.5998)、冲刺
        // 11.8787(`8.4 * √2`,直线是 8.3997),斜向比直线快 41.4%。紧挨着这
        // 段的旧注释写的却是「斜向移动不会比直线快」—— 注释描述的是**意图**,
        // 代码做的是另一回事,两者对不上,于是这个偏差一直没人发现。
        //
        // **归一化的是 `intent`(按键轴),不是世界方向。** `direction` 本身
        // 已经是单位向量(相机的水平朝向),旋过去不改变模长,所以「`intent`
        // 归一 + `speed` 乘在旋后的方向上」与「合速度钳到 `speed`」完全等价,
        // 取前者更省一次钳制。
        let scale: f32 = Self::diagonal_scale(intent);
        let forward: f32 = intent[1] * scale;
        let strafe: f32 = intent[0] * scale;
        let target: Vec2 = [
            (direction[0] * forward + -direction[1] * strafe) * speed,
            (direction[1] * forward + direction[0] * strafe) * speed,
        ];
        let ramp: f32 = (1.0 - (-SPEED_RAMP * dt).exp()).clamp(0.0, 1.0);
        let current: Vec2 = self.get_velocity();
        self.set_velocity([
            current[0] + (target[0] - current[0]) * ramp,
            current[1] + (target[1] - current[1]) * ramp,
        ]);

        // 碰撞:标准 **move_and_slide**。先把想要的位移交给
        // `resolve_slide`,它只沿碰撞法线推出穿透深度,**切向分量原样
        // 保留**。
        //
        // 之前这里是 `world.resolve(wanted)` —— 那是**全有全无**的位置
        // 钳制:圆心被直接推到形状的最近面,沿墙方向的位移也一起没了。
        // 于是顶着墙走时位移恰好为零,`want (-1.00, 0.00)` 拿到
        // `got (+0.00, +0.00)`,几十帧推不动一毫米。卡点不固定在某一
        // 面墙(实测 x = 28.3 / 28.4 / 33.1 / 35.0 各出现过一次),因为
        // **每一面墙都这样**:被弹到哪就贴住哪面墙。
        let velocity: Vec2 = self.get_velocity();
        let here: Vec3 = self.get_position();
        let delta: Vec2 = [velocity[0] * dt, velocity[1] * dt];
        let radius: f32 = world.get_player_radius();
        let resolved: Vec2 = world.resolve_slide([here[0], here[2]], delta, radius);
        self.set_position([resolved[0], here[1], resolved[1]]);

        // 速度取「**实际走出来的位移** / dt」,而不是把分离修正量加回去。
        // 后者会被 1/dt 放大几十倍,一次 1 cm 的分离就会注入 0.6 m/s 的
        // 侧向速度,角色会被「弹」着走。现在撞墙时**朝墙**的速度分量
        // 自然归零,不会一直顶着墙搓,也不会被弹飞。
        //
        // 关键在于 `resolved - here` 现在**包含切向位移**了(滑动之前是
        // 零),所以反算出来的速度正确地保留了沿墙分量,不会把玩家
        // 钉死在接触点上。
        let inverse_dt: f32 = 1.0 / dt.max(f32::EPSILON);
        let achieved: Vec2 = [resolved[0] - here[0], resolved[1] - here[2]];
        self.set_velocity([
            (achieved[0] * inverse_dt).clamp(-speed, speed),
            (achieved[1] * inverse_dt).clamp(-speed, speed),
        ]);

        // 转身 / 步态用**碰撞前的目标速度** `velocity`,而不是被墙削过的
        // 实际速度:蹭着墙走时手脚照样摆,只是真的走不动 —— 这才符合直觉。
        let planar: f32 = (velocity[0] * velocity[0] + velocity[1] * velocity[1]).sqrt();
        if planar > PLANAR_EPSILON {
            let desired: f32 = facing_yaw(velocity);
            let turning: f32 = wrap_angle(desired - self.get_yaw()) * TURN_RATE * dt;
            self.set_yaw(self.get_yaw() + turning);
        }

        // 步态:幅度跟随实际速度,相位按走过的距离推进。
        let ratio: f32 = (planar / speed).clamp(0.0, 1.0);
        let relaxed: f32 = (1.0 - (-GAIT_RELAX_RATE * dt).exp()).clamp(0.0, 1.0);
        if planar > PLANAR_EPSILON {
            let scaled: f32 = self.get_gait_amount() + (ratio - self.get_gait_amount()) * relaxed;
            self.set_gait_amount(scaled);
            self.set_gait_phase(
                (self.get_gait_phase() + planar * GAIT_RATE_PER_METER * dt) % (2.0 * PI),
            );
        } else {
            let shrunk: f32 = self.get_gait_amount() - self.get_gait_amount() * relaxed;
            self.set_gait_amount(shrunk);
            self.set_gait_phase(self.get_gait_phase() * (1.0 - relaxed));
        }
        self.set_gait_amount(self.get_gait_amount().clamp(0.0, 1.0));
    }
}
