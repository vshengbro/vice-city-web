use super::*;

/// Default construction for [`InputState`].
impl Default for InputState {
    /// default。
    fn default() -> Self {
        Self {
            keys: HashMap::new(),
            phase: DEFAULT_PHASE,
            dragging: false,
            last_pointer: [0.0, 0.0],
            last_pinch: 0.0,
            pointer_locked: false,
            fire_held: false,
            fire_pressed: false,
            fire_released: false,
            aim_point: [0.0, 0.0],
            viewport: [1280.0, 720.0],
            steer_key: String::new(),
        }
    }
}

/// Inherent implementation of [`InputState`].
impl InputState {
    /// 某个键是否按住(按 `KeyboardEvent.code` 判定,兼容所有键盘布局)。
    ///
    /// # Arguments
    ///
    /// - `&str` - str 的只读引用。
    ///
    /// # Returns
    ///
    /// - `bool` - 判定结果。
    pub(crate) fn get_held(&self, code: &str) -> bool {
        self.get_keys().get(code).copied().unwrap_or(false)
    }

    /// 当前拥有方向盘的按键(空串 = 没人按)。
    ///
    /// # Returns
    ///
    /// 写回「当前谁拥有方向盘」。传空串即交还控制权。
    ///
    /// # Arguments
    ///
    /// - `String` - 新的拥有者按键。
    pub(crate) fn set_steer_key(&mut self, value: String) {
        self.steer_key = value;
    }

    /// 驾驶时的舵角输入(−1..1,**左负右正**,与 `drive` 的约定一致)。
    ///
    /// 两组键都能转方向盘:A/D 与 ←/→。方向键**只**在这里被读,
    /// 而这个函数**只有驾驶时**才被 `simulate` 调用,所以步行时方向键
    /// 既不会开车,也不会进移动轴 —— GTA V 的移动只认 WASD。
    ///
    /// 符号直接由「拥有方向盘的那个键」决定,而不是把两组的键塞进
    /// 一个 `axis` 调用 —— 后者会把 `ArrowRight` 当成 `KeyA` 的反向键,
    /// 算出 `0 - 1 = -1`,右箭头反而左转。
    ///
    /// **为什么是「后按的赢」而不是相加:**A/D 与方向键各自都是满舵 ±1,
    /// 相加在两组反向时正好得 0 —— 玩家明明压着两个相反方向,车却
    /// 笔直走,体感就是「方向盘突然失灵」。成熟开车游戏(Forza、GTA V
    /// 的辅助转向)统一用后按优先:每次按下一个转向键就把控制权交给它,
    /// 直到它松开为止。`steer_key` 记的就是这个「当前谁拥有方向盘」。
    ///
    /// # Returns
    ///
    /// - `f32` - 舵角:左转 −1.0,右转 +1.0,无人按键 `0.0`。
    pub fn steer_axis(&self) -> f32 {
        // 拥有者一旦松手,控制权作废,回到「谁在按就听谁」。
        let owner: &str = self.get_steer_key();
        if !owner.is_empty() && self.get_held(owner) {
            return steer_sign(owner);
        }
        // 没人拥有方向盘:按「右优先,其次左」兜底,同样是满舵二选一。
        if self.get_held(KEYD) || self.get_held(ARROWRIGHT) {
            return 1.0;
        }
        if self.get_held(KEYA) || self.get_held(ARROWLEFT) {
            return -1.0;
        }
        0.0
    }

    /// 记下「这个键此刻接管了方向盘」。keyup 时对应清除。
    ///
    /// # Arguments
    ///
    /// - `&str` - 被按下的 `KeyboardEvent.code`。
    pub fn claim_steer(&mut self, code: &str) {
        if is_steer_key(code) {
            self.set_steer_key(String::from(code));
        }
    }
}

impl Game {
    /// 拾取批次的只读引用。
    ///
    /// # Returns
    ///
    /// - `&Vec<usize>` - 与 `&self` 同生命周期,不复制。
    pub(crate) fn get_pickup_batches_ref(&self) -> &Vec<usize> {
        &self.pickup_batches
    }

    /// 碰撞世界的只读引用。
    ///
    /// # Returns
    ///
    /// - `&CollisionWorld` - 与 `&self` 同生命周期,不复制;可在持有 `&mut Game` 时先取出再用。
    pub(crate) fn get_world_ref(&self) -> &CollisionWorld {
        &self.world
    }

    /// 当前渲染后端的只读引用。
    ///
    /// # Returns
    ///
    /// - `Option<&Renderer>` - 后端已创建时为 `Some`,未创建时为 `None`;不会 panic。
    pub(crate) fn get_renderer(&self) -> Option<&Renderer> {
        self.renderer.as_ref()
    }
}
