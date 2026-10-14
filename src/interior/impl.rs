use super::*;

/// Inherent implementation of [`FloorWorld`].
impl FloorWorld {
    /// 新建一个空的室内碰撞世界。
    ///
    /// # Returns
    ///
    /// - `Self` - 不含任何楼板与隔墙的室内碰撞世界。
    pub fn new() -> Self {
        Self { floors: Vec::new() }
    }

    /// 全部室内碰撞体的可变视图。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<Floor>` - 楼板与隔墙表。
    pub fn get_floors_mut(&mut self) -> &mut Vec<Floor> {
        &mut self.floors
    }

    /// 追加一块水平楼板(玩家站在它的 `max[1]` 面上)。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 楼板的世界空间下角。
    /// - `Vec3` - 楼板的世界空间上角。
    pub fn push_slab(&mut self, min: Vec3, max: Vec3) {
        self.get_floors_mut().push(Floor::Slab { min, max });
    }

    /// 追加一段竖直隔墙。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 墙脚的世界空间下角。
    /// - `Vec3` - 墙顶的世界空间上角。
    pub fn push_wall(&mut self, min: Vec3, max: Vec3) {
        self.get_floors_mut().push(Floor::Wall { min, max });
    }

    /// 玩家脚下应该踩的高度:在「不高于 `from_y + STEP_UP_TOLERANCE`」的
    /// 楼板里取**最高的一块**。
    ///
    /// 楼梯就是靠这条规则成立的:一级台阶比脚底高 0.305 m,在容差内,所以
    /// 走上去时被抬到台阶面;下一级再抬一级。反过来,从二楼往下看,低于
    /// 脚底的楼板一律不选 —— 于是走到楼板边缘会真的掉下去。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 玩家世界 XZ 坐标(用圆心,不用足迹)。
    /// - `f32` - 玩家当前脚底高度(米)。
    ///
    /// # Returns
    ///
    /// - `Option<f32>` - 该 XZ 点上的可站高度(米);下方没有任何楼板时为
    ///   `None`(调用方按「站在地面 y = 0」处理)。
    pub fn support_height(&self, point: Vec2, from_y: f32) -> Option<f32> {
        let ceiling: f32 = from_y + STEP_UP_TOLERANCE;
        let mut best: Option<f32> = None;
        for floor in self.get_floors() {
            let Floor::Slab { min, max } = floor else {
                continue;
            };
            if point[0] < min[0] || point[0] > max[0] {
                continue;
            }
            if point[1] < min[2] || point[1] > max[2] {
                continue;
            }
            let top: f32 = max[1];
            if top > ceiling {
                continue;
            }
            if best.is_none_or(|current: f32| top > current) {
                best = Some(top);
            }
        }
        best
    }

    /// 沿**这一帧走过的整段**线段找脚下该踩的高度(楼梯的正解)。
    ///
    /// [`Self::support_height`] 是**单点**查询,而 `step_vertical` 只在
    /// 一帧走完之后才调它一次。软件渲染下 rAF 只有约 1.1 fps,一帧的
    /// 水平位移是 `WALK_SPEED * dt ≈ 0.61 m`(CDP 实测 `0.613 m/frame`),
    /// 而**每一级踏面只有 `SHOWCASE_STAIR_RUN = 0.45 m` 深** —— 一帧就跨过
    /// 1.43 级。落点已经越过了容差内的所有踏面,单点查询只剩首层地板
    /// (顶面 0.15)可选,于是爬到第 4 级之后再也上不去。
    ///
    /// 这里改成沿线段**扫**一遍,取「不低于脚底、且在容差内」的最高一块
    /// 踏面。扫过的那几级正是玩家真实踩过的,逐级抬升因此恢复。
    ///
    /// **判据同时钉死了「不许往下吸」。** 低于脚底的板一律不算支撑 ——
    /// 首层地板铺满整个楼内,少了这条判据,站在第 8 级时脚底 2.59 会被
    /// 首层地板(0.15)当成合法支撑直接吸下去,那正是「y 一步掉回地面
    /// 0.15」的形态。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 这一帧起点 XZ(玩家的旧位置)。
    /// - `Vec2` - 这一帧落点 XZ(玩家的新位置)。
    /// - `f32` - 玩家当前脚底高度(米)。
    ///
    /// # Returns
    ///
    /// - `Option<f32>` - 该段上可站的高度(米);脚下没有任何合法板时为
    ///   `None`(调用方按自由落体处理)。
    pub fn support_height_along(&self, from: Vec2, to: Vec2, from_y: f32) -> Option<f32> {
        let ceiling: f32 = from_y + STEP_UP_TOLERANCE;
        let span: f32 = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
        // 步长取踏深的四分之一:保证每一级至少被采到三个点,又不会把采样
        // 数随帧率放大(1.84 m 的一帧也只有 17 次查询)。
        let steps: usize = ((span / (SHOWCASE_STAIR_RUN * 0.25)).ceil() as usize).max(1);
        let mut best: Option<f32> = None;
        for index in 0..=steps {
            let t: f32 = index as f32 / steps as f32;
            let point: Vec2 = [
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
            ];
            let Some(top) = self.support_height(point, from_y) else {
                continue;
            };
            // 只认「不高于脚底容差上限、也不低于脚底」的那块。
            if top > ceiling || top < from_y - SUPPORT_CATCH_EPSILON {
                continue;
            }
            if best.is_none_or(|current: f32| top > current) {
                best = Some(top);
            }
        }
        best
    }

    /// 沿**这一帧走过的整段**线段找脚下该踩的高度(楼梯的正解)。
    ///
    /// [`Self::support_height`] 是**单点**查询,而 `step_vertical` 只在
    /// 一帧走完之后才调它一次。软件渲染下 rAF 只有约 1.1 fps,一帧的
    /// 水平位移是 0.613 m(CDP 实测),而**每一级踏面只有
    /// `SHOWCASE_STAIR_RUN = 0.45 m` 深** —— 一帧跨过 1.36 级。
    ///
    /// 关键在于**抬升速率**:楼梯坡度是 `0.305 / 0.45 = 0.678 m/m`,走
    /// 0.613 m 本该升 0.415 m;而单次 `support_height` 每次只返回「容差内
    /// 最高的一块」,**一次最多抬一级**(0.305 m,第二级就超出容差)。只查
    /// 一次的话,抬升速率追不上前进速率,亏空逐帧累积,最终脚下每一级都
    /// 高出容差,支撑彻底丢失,`y` 塌回首层地板 —— 实测正是「第 4 级
    /// y=1.370 之后一步掉回 0.150,之后 33 帧不动」。
    ///
    /// 所以这里把一帧切成一串 [`SUPPORT_SUBSTEP_DISTANCE`] 的小步,
    /// **每小步用当步的脚底重新取一次支撑并抬高**,抬升这才跟得上坡度。
    ///
    /// **判据同时钉死了「不许往下吸」。** 低于脚底的板一律不算支撑 ——
    /// 首层地板铺满整个楼内,少了这条判据,站在楼梯中段时脚底会被地板
    /// 重新拽回地面 0.15。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 这一帧起点 XZ(玩家的旧位置)。
    /// - `Vec2` - 这一帧落点 XZ(玩家的新位置)。
    /// - `f32` - 玩家当前脚底高度(米)。
    ///
    /// # Returns
    ///
    /// - `(Option<f32>, f32)` - `(最终支撑高度, 逐级抬升后的脚底高度)`。
    ///   支撑为 `None` 表示这一路都没有合法板,调用方按自由落体处理。
    ///
    /// # 下楼怎么处理的
    ///
    /// 每一小步做**扫掠抬升**(上楼时把脚底一级一级顶上去);全部小步走完后,
    /// **再补一次「允许往下落一��」的整体查询**,这才是下楼能一级一级
    /// 往下走的原因。
    ///
    /// **为什么必须补这一次:**扫掠的判据是「不接受低于脚底的面」,那是
    /// 为了挡住首层地板把楼梯中段的人吸回地面(楼内地板铺满全场)。但同
    /// 一条判据也会把**下一级踏面**(低一个踏高 0.305 m)一起拒掉 ——
    /// 于是往回走时每一小步都只认得脚下当前那一级,脚底一格都不降,人
    /// 就**悬在楼梯上横着往下挪**,直到走出梯段才掉进楼梯井。单测
    /// `a_normal_frame_descends_the_stairs_one_tread_at_a_time` 钉的就是
    /// 这一条(修复前它停在 2.895 不动)。
    ///
    /// **为什么补查必须带下界、不能直接用 [`Self::support_height`]。**
    /// `support_height` 只有上界没有下界,长帧(0.613 m/帧)下落点早已越过
    /// 好几级踏面,查出来的是**首层地板 0.15**,于是一路辛苦抬到 2.895
    /// 又被这一脚拉回地面 —— 正是组 1 最初那个缺陷的翻版(实测
    /// `a_long_frame_per_stride_still_reaches_the_top` 从 3.200 掉到 0.150)。
    /// 所以补查的下界取**一个踏高** [`STEP_DOWN_TOLERANCE`]:只放行
    /// 「往下一级」,不放行「掉到楼下」。首层地板离楼梯中段足有 2.4 m,
    /// 两个目标因此不会互相打架。
    pub fn support_along_frame(&self, from: Vec2, to: Vec2, from_y: f32) -> (Option<f32>, f32) {
        let span: f32 = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
        let substeps: usize = ((span / SUPPORT_SUBSTEP_DISTANCE).ceil() as usize).max(1);
        let mut y: f32 = from_y;
        let mut best: Option<f32> = None;
        let mut cursor: Vec2 = from;
        for index in 0..substeps {
            let t: f32 = (index + 1) as f32 / substeps as f32;
            let probe: Vec2 = [
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
            ];
            if let Some(top) = self.support_height_along(cursor, probe, y) {
                y = top;
                best = Some(top);
            }
            cursor = probe;
        }
        // 往下走一级:再扫一遍整段,但允许落点比**这一帧起点**的脚底低
        // 至多一个踏高。基准必须是起点而不是抬升后的高度 —— 后者会让
        // 补查把同一帧里刚抬上去的那一级又退回来(实测长帧上楼卡在 2.895)。
        //
        // **但这一帧只要抬升过,补查就必须让位。** 一个冲刺帧长 1.1 m,
        // 抬升阶段会顺着 6 个子步一级一级爬到 1.370;这时补查的下界是
        // 「帧起点 0.455 往下 0.355 = 0.100」,而**首层地板 0.150 正好落在
        // 这个窗口里** —— 楼梯底端那一段扫掠必然会采到它。于是补查把刚爬
        // 上去的 1.370 覆盖回 0.150,人钉死在首层,冲出梯顶再从楼梯井掉
        // 回地面(实测 peak_y=0.455、end_x 一路飞到 -6)。
        //
        // 关键在于**扫掠窗口和实际走过的段落不同**:抬升只看「不低于脚底」的
        // 那一格,补查却扫**整帧**并允许比帧起点低一个踏高。帧越长,补查
        // 越可能扫到低处那块铺满全场的首层地板;帧短的时候(步行 0.31 m)
        // 扫不到,于是缺陷只在使用冲刺时暴露。
        //
        // 语义上也说得通:「这一帧踩着更高的一级」与「这一帧往下一级」是
        // 互斥的,一帧之内两者不可能同时成立。因此抬升生效时直接返回,
        // 下楼的那一帧(抬升不生效)照旧补查。
        if y <= from_y + SUPPORT_CATCH_EPSILON
            && let Some(top) = self.step_down_along(from, to, from_y)
        {
            y = top;
            best = Some(top);
        }
        (best, y)
    }

    /// 沿整段找「比脚底低、但不超过一个踏高」的最高一块板(下楼用)。
    ///
    /// **只在确实比脚底更低时才返回。** 抬升阶段已经处理过「比脚底高」
    /// 的面,所以这里返回非 `None` 就等价于「这一帧在往下走」。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 这一帧起点 XZ。
    /// - `Vec2` - 这一帧落点 XZ。
    /// - `f32` - **这一帧开始时**的脚底高度(米),不是抬升后的高度。
    ///
    /// # Returns
    ///
    /// - `Option<f32>` - 可站的下一级高度;这一路上没有「往下一级」那么
    ///   深的面时为 `None`(调用方保持原高度,由重力处理)。
    pub(crate) fn step_down_along(&self, from: Vec2, to: Vec2, from_y: f32) -> Option<f32> {
        let ceiling: f32 = from_y + STEP_UP_TOLERANCE;
        let floor: f32 = from_y - STEP_DOWN_TOLERANCE;
        let span: f32 = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
        let steps: usize = ((span / (SHOWCASE_STAIR_RUN * 0.25)).ceil() as usize).max(1);
        let mut best: Option<f32> = None;
        for index in 0..=steps {
            let t: f32 = index as f32 / steps as f32;
            let point: Vec2 = [
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
            ];
            let Some(top) = self.support_height(point, from_y) else {
                continue;
            };
            if top > ceiling || top < floor {
                continue;
            }
            // 只接受**确实更低**的那一块,否则它与抬升阶段的结果重复。
            if top >= from_y - SUPPORT_CATCH_EPSILON {
                continue;
            }
            if best.is_none_or(|current: f32| top > current) {
                best = Some(top);
            }
        }
        best
    }

    /// 把一个圆形身体推出所有与它身体区间相交的隔墙与楼板。
    ///
    /// 只有**竖直方向**与身体区间 `[body_min_y, body_max_y]` 有交集的碰撞
    /// 体才参与:站在一楼时头顶的二楼楼板(下表面 2.95 m)与身体(0.15 ~
    /// 1.90 m)不相交,不会被推开;走上楼梯、脑袋快要顶到楼板时才会生效。
    ///
    /// 分离方式是纯 XZ 的「圆心推到最近面」,和
    /// [`crate::collision::CollisionWorld::resolve_with_radius`] 同一套:
    /// 迭代若干轮直到一轮没有任何形状推动圆心为止。**不动 Y** —— 垂直
    /// 方向是重力的职责,这里越权会把玩家吸到墙面上。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待分离的世界 XZ 坐标。
    /// - `f32` - 身体底面高度(脚底,米)。
    /// - `f32` - 身体顶面高度(头顶,米)。
    /// - `f32` - 身体等效圆柱半径(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离后的世界 XZ 坐标。
    pub fn resolve_interior(
        &self,
        point: Vec2,
        body_min_y: f32,
        body_max_y: f32,
        radius: f32,
    ) -> Vec2 {
        self.separate_interior(point, body_min_y, body_max_y, radius)
    }

    /// [`Self::resolve_interior`] 的**带位移**版本:沿墙保留切向分量。
    ///
    /// 纯分离(不传 `delta`)有个致命的结构缺陷:它只知道「现在在哪儿」,
    /// 不知道「这一帧想去哪儿」。于是玩家顶着隔墙走时,每帧的流程是
    /// 「静态层前进 → 室内层沿法线原路弹回」,**切向位移被整个丢掉**。
    /// 表现就是「有速度、无位移」:顶着墙几十帧推不动一毫米,而且
    /// 斜着走也拐不过弯 —— 因为连「往旁边挪一点」这个动作都传不进去。
    ///
    /// 更糟的是这两层每帧互相抵消,位置在极限环上抖动:实测静态层把
    /// 玩家送到 `z = 35.243`(已经陷进墙里 0.257 m),室内层再弹回
    /// `z = 35.550`,`step_vertical` 把这个结果 `set_position` 写回 ——
    /// 于是静态层下一帧又从墙里出发。静态层**从来不知道墙在这儿**,
    /// 室内层**从来不知道想去哪儿**。
    ///
    /// 传入 `delta` 之后走标准两步 `move_and_slide`(与
    /// [`crate::collision::CollisionWorld::resolve_slide`] 同一套思路):
    /// 先按法线推出穿透深度拿到修正向量,把 `delta` 的法向分量扣掉,
    /// **只保留切向**再走一遍。玩家于是顺着墙滑过去,而不是被钉在
    /// 接触点上。
    ///
    /// **但单步做这件事会穿墙,所以整帧必须先切子步。** 上面那套
    /// 分离是**位置推离**:它只看「现在在哪儿」,于是只要一帧的位移
    /// 超过墙的厚度,帧末人就落在墙的另一侧、且离墙面比半径还远 ——
    /// 推离向量恰好为 0,`length <= INTERIOR_SLIDE_EPSILON` 分支把
    /// **整帧**原样放行,墙被一步跨过去。
    ///
    /// 实测(CDP,HUD 逐帧,软件渲染 rAF ≈ 1.1 fps,`__vcw.vel` 中位数
    /// 8.3999 = `RUN_SPEED`):冲刺一帧推进 **1.1269 m**,而首层隔墙只有
    /// 0.15 m 厚、前墙门垛 0.20 m 厚。从 LOFT 梯脚(世界 27.3, 24.9)冲
    /// 向前墙,步行正确地停在 `x = 27.40`(门垛内表面 27.75 减半径
    /// 0.35),冲刺却在 15 帧里推进 15.25 m。同一面墙、同一时刻 ——
    /// 差别只有一帧多长。
    ///
    /// 所以这里把整帧切成一串 [`INTERIOR_SLIDE_SUBSTEP`] 的小步(和
    /// [`Self::support_along_frame` 对踏面做的事同形),**每小步各做一次
    /// 完整的 `move_and_slide`**。小步长度压到比最薄的墙还短,每一小步的
    /// 落点就不可能跳过整堵墙,法向钳制于是总能生效。
    ///
    /// **60 fps 下必须是 no-op**:步行一帧 `4.6 / 60 = 0.077 m`,远小于
    /// 一个小步,`substeps == 1`,整帧仍走一次 —— 正常帧率的手感逐帧
    /// 不变。这条由单测 `the_interior_sweep_is_a_no_op_at_sixty_fps` 守着。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待分离的世界 XZ 坐标。
    /// - `Vec2` - 本帧想要的水平位移(米);纯分离时传 `[0.0, 0.0]`。
    /// - `f32` - 身体底面高度(脚底,米)。
    /// - `f32` - 身体顶面高度(头顶,米)。
    /// - `f32` - 身体等效圆柱半径(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离(并保留切向位移)之后的世界 XZ 坐标。
    pub fn resolve_interior_slide(
        &self,
        point: Vec2,
        delta: Vec2,
        body_min_y: f32,
        body_max_y: f32,
        radius: f32,
    ) -> Vec2 {
        let span: f32 = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
        let substeps: usize = ((span / INTERIOR_SLIDE_SUBSTEP).ceil() as usize).max(1);
        if substeps <= 1 {
            return self.slide_one_step(point, delta, body_min_y, body_max_y, radius);
        }
        // 每小步走**等分**的一段(`delta / substeps`),不是从头累积的
        // `delta * t`:后者会让第 k 步再走一遍前 k-1 步的路,整帧位移被
        // 放大成 `delta * (1 + 2 + ... + n) / n`。
        let step: Vec2 = [delta[0] / substeps as f32, delta[1] / substeps as f32];
        let mut cursor: Vec2 = point;
        for _ in 0..substeps {
            cursor = self.slide_one_step(cursor, step, body_min_y, body_max_y, radius);
        }
        cursor
    }

    /// [`Self::resolve_interior_slide`] 切完子步之后,真正做**一步**
    /// `move_and_slide` 的那部分(单步版即修复前的实现)。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待分离的世界 XZ 坐标。
    /// - `Vec2` - 本步想要的水平位移(米)。
    /// - `f32` - 身体底面高度(脚底,米)。
    /// - `f32` - 身体顶面高度(头顶,米)。
    /// - `f32` - 身体等效圆柱半径(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 这一步走完(并保留切向位移)之后的世界 XZ 坐标。
    fn slide_one_step(
        &self,
        point: Vec2,
        delta: Vec2,
        body_min_y: f32,
        body_max_y: f32,
        radius: f32,
    ) -> Vec2 {
        // 先做一次纯分离,拿到「墙把人推了多远、往哪个方向推」。
        let pushed: Vec2 = self.separate_interior(point, body_min_y, body_max_y, radius);
        let correction: Vec2 = [pushed[0] - point[0], pushed[1] - point[1]];
        let length: f32 = (correction[0] * correction[0] + correction[1] * correction[1]).sqrt();
        // 没有推动 → 人本来就在合法位置,切向位移原样放行。
        if length <= INTERIOR_SLIDE_EPSILON {
            let free: Vec2 = [point[0] + delta[0], point[1] + delta[1]];
            return self.separate_interior(free, body_min_y, body_max_y, radius);
        }
        // 推动方向指向墙的**外侧**,它就是法线。
        let normal: Vec2 = [correction[0] / length, correction[1] / length];
        let into: f32 = delta[0] * normal[0] + delta[1] * normal[1];
        // 没有往墙里钻(只是擦边),分离结果已经正确。
        if into >= 0.0 {
            return pushed;
        }
        // 纯正面顶墙:没有切向可留,分离结果就是最终位置。
        let tangent: Vec2 = [delta[0] - normal[0] * into, delta[1] - normal[1] * into];
        let tangent_len: f32 = (tangent[0] * tangent[0] + tangent[1] * tangent[1]).sqrt();
        if tangent_len <= INTERIOR_SLIDE_EPSILON {
            return pushed;
        }
        // 沿墙走完切向位移之后再分离一次:防止「贴着走时切向一步
        // 跨进了墙里」被下一次迭代当成正常位置。
        let slid: Vec2 = [pushed[0] + tangent[0], pushed[1] + tangent[1]];
        self.separate_interior(slid, body_min_y, body_max_y, radius)
    }

    /// 从 `from` 到 `to` 的水平线段是否被某段隔墙挡住(视线判定用)。
    ///
    /// **为什么必须有这个函数:**`CollisionWorld::has_line_of_sight` 只
    /// 射线检测静态碰撞世界,而**楼板与隔墙住在另一个世界**
    /// ([`FloorWorld`])。两套几何互不相交,所以一栋样板楼在静态世界里
    /// 根本不存在 —— 射线从它中间穿过去,报告「视线通畅」。实测把
    /// 玩家停进楼里(`interiors.inSolid == true`)、19 m 外的警察在
    /// `attack` 状态持续开火,90 帧掉 31.8 血,门控计数器全程为 0。
    ///
    /// 这里用**线段 vs AABB 的 2D  slab 法**做精确相交,而不是采样或
    /// 膨胀盒子:子弹是沿一条直线走的,采样会漏掉薄墙之间的缝隙。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 起点 XZ(敌人)。
    /// - `Vec2` - 终点 XZ(玩家)。
    /// - `f32` - 视线高度(米)—— 用它筛掉「脚下 / 头顶」的楼板。
    ///
    /// # Returns
    ///
    /// - `bool` - 被隔墙挡住为 `true`。
    pub fn blocks_sight(&self, from: Vec2, to: Vec2, sight_y: f32) -> bool {
        let dx: f32 = to[0] - from[0];
        let dz: f32 = to[1] - from[1];
        for floor in self.get_floors() {
            let (min, max): (Vec3, Vec3) = match floor {
                Floor::Slab { min, max } => (*min, *max),
                Floor::Wall { min, max } => (*min, *max),
            };
            // 楼板只在视线正好平躺在板面高度上时才算遮挡,否则二楼
            // 的地板会把一楼的人整片挡住(脚下 0.15 m 的地板同理)。
            let is_slab: bool = matches!(floor, Floor::Slab { .. });
            if is_slab
                && (sight_y < min[1] - INTERIOR_SIGHT_EPSILON
                    || sight_y > max[1] + INTERIOR_SIGHT_EPSILON)
            {
                continue;
            }
            // 线段 vs AABB:逐轴求进出参数区间,非空即相交。
            let (lo, hi): (f32, f32) = (0.0, 1.0);
            let mut enter: f32 = lo;
            let mut exit: f32 = hi;
            let mut clipped: bool = true;
            for axis in 0..2 {
                let (origin, delta, low, high): (f32, f32, f32, f32) = if axis == 0 {
                    (from[0], dx, min[0], max[0])
                } else {
                    (from[1], dz, min[2], max[2])
                };
                if delta.abs() <= INTERIOR_SIGHT_EPSILON {
                    // 与这一轴平行:起点不在板内就永远不相交。
                    if origin < low || origin > high {
                        clipped = false;
                        break;
                    }
                    continue;
                }
                let mut t0: f32 = (low - origin) / delta;
                let mut t1: f32 = (high - origin) / delta;
                if t0 > t1 {
                    swap(&mut t0, &mut t1);
                }
                enter = enter.max(t0);
                exit = exit.min(t1);
                if enter > exit {
                    clipped = false;
                    break;
                }
            }
            if clipped && enter <= exit {
                return true;
            }
        }
        false
    }

    /// 纯分离:把圆心推出所有与身体区间相交的隔墙(楼板不参与)。
    ///
    /// [`Self::resolve_interior_slide`] 的底层实现,迭代若干轮直到一轮
    /// 没有任何墙推动圆心为止。**不动 Y** —— 垂直方向是重力的职责,
    /// 这里越权会把玩家吸到墙面上。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待分离的世界 XZ 坐标。
    /// - `f32` - 身体底面高度(脚底,米)。
    /// - `f32` - 身体顶面高度(头顶,米)。
    /// - `f32` - 身体等效圆柱半径(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离后的世界 XZ 坐标。
    fn separate_interior(
        &self,
        point: Vec2,
        body_min_y: f32,
        body_max_y: f32,
        radius: f32,
    ) -> Vec2 {
        let mut current: Vec2 = point;
        for _ in 0..RESOLVE_ITERATIONS {
            let mut moved: bool = false;
            for floor in self.get_floors() {
                let (min, max): (Vec3, Vec3) = match floor {
                    Floor::Slab { min, max } => (*min, *max),
                    Floor::Wall { min, max } => (*min, *max),
                };
                // 竖直区间不相交就整块跳过:头顶的楼板不该推人,脚底的
                // 楼板也不该(人在楼上时它在身体下方)。
                if max[1] <= body_min_y || min[1] >= body_max_y {
                    continue;
                }
                // 楼板还多一条规则:脚底已经站到板面上(或者差一点点就站
                // 上去)时不许再横向推。否则首层地板会变成一圈看不见的
                // 围墙 —— 玩家站在街上 y = 0,身体区间与 0.15 m 高的地板
                // 相交,会被推着绕楼一圈,门洞永远走不进去。正确的做法是
                // 让 `support_height` 把人**抬上去**。
                // 楼板只在「脚已经站上去了」时跳过横向分离。这里必须是
                // `>=`,不能是 `==` 附近的比较:玩家踩在第 N 级踏面上时
                // 脚底 y 恰好等于该级的 max[1],而 `support_height` 每次都
                // 把人放到**恰好**这个高度,于是条件成立、正常跳过。
                //
                // 但一旦人还在往上跳的半空(脚底比踏面**低**几毫米),旧写法
                // 就会把这个楼板当成实体墙,`push_out_aabb` 取最小松弛方向
                // 把人从**侧面**弹出去 —— 上楼梯时正好卡在两级之间触发,
                // 人被弹到楼梯侧面外的中厅(y 从 1.065 一路掉回 0.15),
                // 于是「楼梯有台阶但永远上不去」。所以这里只认「脚底
                // 已经在板面之上」这一种情况。
                // 楼板永远不该把人**从侧面**推走。
                //
                // 判据不是「脚底有没有站到板面上」,而是「脚底是否已经
                // 高过这块板」——只要人还在板面以下或齐平,这块板就是他
                // 脚下要踩的东西(或者下一级台阶),横向分离只应该由
                // **墙**来做。早先只看 `body_min_y >= max[1] - TOLERANCE`,
                // 于是人站在第 N 级踏面、还没跨上第 N+1 级的那一瞬间
                // (脚底比 N+1 级踏面低一个踏高)会被当成撞上实体,
                // `push_out_aabb` 取最小松弛方向把人从楼梯**侧边**弹出去 ——
                // 表现就是上到一半 y 突然从 1.065 掉回 0.15,楼梯永远上不去。
                //
                // 代价:一层那圈 0.15 m 高的地板也不再横向推人。这本来是
                // 好事(否则首层地板会变成看不见的围墙,门洞走不进去),
                // 由 `support_height` 负责把人**抬**上去。
                //
                // **楼板(踏面)永远不做横向分离。** 玩家踩在第 N 级踏面上时,
                // 身体的竖直区间会与第 1..N 级的每一级相交,而人又正好站在
                // 那一级的 XZ 盒子里 —— `push_out_aabb` 于是走「点在盒内」
                // 分支,按 `slack_x <= slack_z` 取**最小松弛轴**,把人从
                // 楼梯的**侧面**弹出去(踏面宽 1.30 m,人这一弹直接掉到
                // 中厅,实测本地 x 5.03 → 4.10、y 从 1.065 掉回 0.15)。
                // 表现就是:楼梯有台阶、能踩两级,然后永远上不去。
                //
                // 所以踏面只提供**竖直支撑**(`support_height` 负责把人
                // 抬上去),横向只由 `Wall` 挡。一层那块 0.15 m 的地板因此
                // 也不再横向推人 —— 这本来就想要,否则首层地板会变成一圈
                // 看不见的围墙,门洞永远走不进去。
                if let Floor::Slab { .. } = floor {
                    continue;
                }

                let center: Vec2 = [(min[0] + max[0]) * 0.5, (min[2] + max[2]) * 0.5];
                let half: Vec2 = [(max[0] - min[0]) * 0.5, (max[2] - min[2]) * 0.5];
                let Some((direction, depth)) = push_out_aabb(center, half, current, radius) else {
                    continue;
                };
                current[0] += direction[0] * depth;
                current[1] += direction[1] * depth;
                moved = true;
            }
            if !moved {
                break;
            }
        }
        current
    }

    /// 点是否落在某个室内碰撞体的投影里(调试探针用,不做分离)。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 世界 XZ 坐标。
    /// - `f32` - 该点的高度(米)。
    ///
    /// # Returns
    ///
    /// - `bool` - 该 XZ 点落在某个碰撞体的竖直区间内时为 `true`。
    pub fn contains_interior_point(&self, point: Vec2, y: f32) -> bool {
        self.get_floors().iter().any(|floor: &Floor| {
            let (min, max): (Vec3, Vec3) = match floor {
                Floor::Slab { min, max } => (*min, *max),
                Floor::Wall { min, max } => (*min, *max),
            };
            if y < min[1] || y > max[1] {
                return false;
            }
            point[0] >= min[0] && point[0] <= max[0] && point[1] >= min[2] && point[1] <= max[2]
        })
    }
}

/// Default construction for [`FloorWorld`].
impl Default for FloorWorld {
    /// 返回空室内碰撞世界,与 `FloorWorld::new` 等价。
    fn default() -> Self {
        FloorWorld::new()
    }
}
