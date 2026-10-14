use super::*;

/// Inherent implementation of [`CollisionWorld`].
impl CollisionWorld {
    /// 新建一个空的碰撞世界。
    ///
    /// # Returns
    ///
    /// - `Self` - 不含任何形状的碰撞世界。
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            player_radius: 0.35,
            half_extent: [48.0, 48.0],
        }
    }

    /// 静态碰撞体表的可变引用。
    ///
    /// # Returns
    ///
    /// - `&mut Vec<Shape>` - 碰撞体表。
    pub fn get_shapes_mut(&mut self) -> &mut Vec<Shape> {
        &mut self.shapes
    }

    /// 追加一个轴对齐包围盒。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 盒中心的世界 XZ 坐标。
    /// - `Vec2` - 盒的半尺寸(米)。
    pub fn push_aabb(&mut self, center: Vec2, half: Vec2) {
        self.get_shapes_mut().push(Shape::Aabb { center, half });
    }

    /// 追加一个竖直圆柱。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 圆柱中心的世界 XZ 坐标。
    /// - `f32` - 圆柱半径(米)。
    pub fn push_circle(&mut self, center: Vec2, radius: f32) {
        self.get_shapes_mut().push(Shape::Circle { center, radius });
    }

    /// 把一个圆形碰撞体推出所有重叠的静态形状,并钳进世界边界。
    ///
    /// 这是纯函数式的:输入一个世界 XZ 坐标,输出分离后的坐标。多次调用
    /// 幂等 —— 已经贴住的圆再调一次不会抖。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待分离的世界 XZ 坐标。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离并钳进边界后的世界 XZ 坐标。
    pub fn resolve(&self, point: Vec2) -> Vec2 {
        let radius: f32 = self.get_player_radius();
        let mut current: Vec2 = point;
        for _ in 0..RESOLVE_ITERATIONS {
            let mut moved: bool = false;
            for shape in self.get_shapes() {
                let hit: Option<(Vec2, f32)> = match shape {
                    Shape::Aabb { center, half } => push_out_aabb(*center, *half, current, radius),
                    Shape::Circle {
                        center,
                        radius: other,
                    } => push_out_circle(*center, *other, current, radius),
                };
                if let Some((direction, depth)) = hit {
                    current[0] += direction[0] * depth;
                    current[1] += direction[1] * depth;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        current
    }

    /// 圆 vs 静态形状的**滑动**分离:沿墙保留切向位移。
    ///
    /// [`Self::resolve`] 是「全有全无」的位置钳制 —— 它把圆心直接推到
    /// 形状的最近面,**本帧想要的切向位移也一起被改写**。玩家斜着
    /// 撞上墙面时位移被削到几乎为零,表现为「有速度、无位移」:
    /// `want (-1.00, -0.00)` 拿到 `got (+0.00, +0.00)`,几十帧推不动
    /// 一毫米。卡点不固定在某一面墙(实测 x = 28.3 / 28.4 / 33.1 / 35.0
    /// 都出现过),因为**每一面墙都这样** —— 弹到哪就贴住哪面墙。
    ///
    /// 这里是标准两步 `move_and_slide`:
    ///
    /// 1. `wanted = here + delta` 先做一次分离,拿到法线 `n` 与穿透
    ///    深度;
    /// 2. 把 `delta` 沿 `n` 的**法向分量扣掉**,只保留切向,再走一遍
    ///    分离。于是顶着墙斜走时玩家顺着墙滑过去,而不是被钉在接触点。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 上一帧结束时的世界 XZ 坐标。
    /// - `Vec2` - 本帧想要的位移(米)。
    /// - `f32` - 圆半径(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 本帧实际走到的世界 XZ 坐标(已钳进世界边界)。
    pub fn resolve_slide(&self, here: Vec2, delta: Vec2, radius: f32) -> Vec2 {
        // 逐段推进:把这一帧想走的位移切成若干小步,每步都从**上一步的
        // 合法位置**出发做分离。
        //
        // 不能只做一次「先 resolve(wanted) 再扣法向分量」:圆心一旦在一步
        // 里跨过内表面,`push_out_aabb` 的「点在盒内」分支只能猜一个面
        // 推 —— 对厚墙它会选**人正在走向的那一面**,于是人被直接送到墙
        // 的另一侧(实测 0.15 m 的一步穿过了整个墙:从 -0.15 跳到 +0.35)。
        // 切成小步之后,每一步的穿透都远小于步长,永远走不到「盒内」
        // 分支,墙也就不再是隐形的传送带。
        //
        // 步长取 0.05 m(不到半径的 1/6):一帧最多 0.08 m(4.6 m/s ×
        // 1/60 s),也就是 1~2 步,开销可以忽略。
        let want_len: f32 = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
        if want_len <= SLIDE_EPSILON {
            return here;
        }
        let steps: usize = (want_len / SLIDE_STEP).ceil().max(1.0) as usize;
        let step: Vec2 = [delta[0] / steps as f32, delta[1] / steps as f32];
        let mut current: Vec2 = here;
        for _ in 0..steps {
            // 已经陷进某个形状内部(不是「贴着」):停下,别再往里走。
            // 用严格小于 + 一层皮,于是「正好贴在墙面上」不算穿透,
            // 切向滑动才不会被自己的接触测试挡住。
            if self.penetrates(current, radius) {
                break;
            }
            let next: Vec2 = [current[0] + step[0], current[1] + step[1]];
            let resolved: Vec2 = self.resolve(next);
            let achieved: Vec2 = [resolved[0] - current[0], resolved[1] - current[1]];
            let want_dot: f32 = step[0] * step[0] + step[1] * step[1];
            let got_dot: f32 = achieved[0] * step[0] + achieved[1] * step[1];
            if got_dot >= want_dot * SLIDE_MIN_PROGRESS {
                // 这一步基本走完了,继续下一小步。
                current = resolved;
                continue;
            }
            // 被挡住了:只保留**切向**分量再走一次,玩家于是顺着墙滑
            // 过去,而不是被钉在接触点上(这正是「有速度、无位移」的
            // 修复点)。
            current = self.slide_step(current, step, resolved, radius);
            break;
        }
        current
    }

    /// 被挡住时的切向滑动:从 `before` 出发,沿 `want` 的切向分量走。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 这一小步开始时的位置。
    /// - `Vec2` - 这一小步想要的位移。
    /// - `Vec2` - 分离之后的位置(用来推法线)。
    /// - `f32` - 圆半径(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 沿墙滑过之后的位置。
    fn slide_step(&self, before: Vec2, want: Vec2, pushed: Vec2, _radius: f32) -> Vec2 {
        let correction: Vec2 = [
            pushed[0] - (before[0] + want[0]),
            pushed[1] - (before[1] + want[1]),
        ];
        let correction_len: f32 =
            (correction[0] * correction[0] + correction[1] * correction[1]).sqrt();
        if correction_len <= SLIDE_EPSILON {
            return pushed;
        }
        // 分离方向指向形状**外侧**,所以法向就是它本身。
        let normal: Vec2 = [
            correction[0] / correction_len,
            correction[1] / correction_len,
        ];
        let into: f32 = want[0] * normal[0] + want[1] * normal[1];
        if into >= 0.0 {
            // 没有往墙里钻(擦边),分离结果就是对的。
            return pushed;
        }
        let tangent: Vec2 = [want[0] - normal[0] * into, want[1] - normal[1] * into];
        if (tangent[0] * tangent[0] + tangent[1] * tangent[1]).sqrt() <= SLIDE_EPSILON {
            // 纯正面顶墙:没有切向可留。
            return pushed;
        }
        self.resolve([before[0] + tangent[0], before[1] + tangent[1]])
    }

    /// 圆心是否**陷进**了某个形状内部(贴着墙面不算)。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 圆心的世界 XZ 坐标。
    /// - `f32` - 圆半径(米)。
    ///
    /// # Returns
    ///
    /// - `bool` - 已经陷进某个形状时为 `true`。
    fn penetrates(&self, point: Vec2, radius: f32) -> bool {
        let reach: f32 = radius - SLIDE_EPSILON;
        self.get_shapes().iter().any(|shape: &Shape| match shape {
            // 圆 vs 盒 = 「圆心到盒的**距离** < 半径」。
            //
            // 两个坑都踩过:一开始写成「圆心落在盒的矩形足迹内」,对 20 m
            // 深的墙恒为真(墙在 z 方向覆盖 ±10 m),贴着墙面也会被判成陷
            // 进去,切向滑动被自己的接触测试冻住,于是又回到「有速度、
            // 无位移」。改用真距离后:贴着时距离恰为半径,减去一层皮后
            // 严格大于阈值,正常放行。
            Shape::Aabb { center: _, half: _ } => distance_to_shape(shape, point) < reach,
            Shape::Circle {
                center,
                radius: other,
            } => {
                let dx: f32 = point[0] - center[0];
                let dz: f32 = point[1] - center[1];
                (dx * dx + dz * dz).sqrt() < *other + reach
            }
        })
    }

    /// 把速度按**软边界**削一刀:越界越多,允许的速度越小。
    ///
    /// 旧实现是 `point.clamp(-half, half)` —— 一个**硬钳制**。在城市是
    /// 程序化无限生成的前提下,硬钳制就是用户报的「地图非无限大,触碰空气
    /// 墙无法前进」:走到 150 m 处坐标被死死钉住,几十帧推不动一毫米。
    ///
    /// 软边界的语义是「**你最多只能领先已生成世界这么远**」,不是「世界到
    /// 这儿为止」:
    ///
    /// - `radius + slack` 以内 —— 完全不干预(绝大多数时候都在这里);
    /// - 超出部分按 `excess / slack` 线性收缩速度上限,越界越深跑越慢;
    /// - 超出 `2 × slack` 时速度上限为 0 —— 玩家**到不了**那里,自然
    ///   被限制在「已生成世界 + 一圈缓冲」里,不会掉进虚空。
    ///
    /// 它不做位置钳制,所以永远不可能把玩家「传送」回某个点:推力只改变
    /// 速度,玩家的位置始终是他自己走出来的。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待削的速度(世界 XZ,米/秒)。
    /// - `Vec2` - 当前所在位置(世界 XZ,米)。
    /// - `Vec2` - 世界软边界半径(X / Z 各一半,米)。
    /// - `f32` - 允许越界的余量(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 收缩后的速度。
    pub fn soft_limit(&self, velocity: Vec2, at: Vec2, limits: Vec2, slack: f32) -> Vec2 {
        // 收缩从**软边界本身**(`limits`)开始,不是从 `limits + slack`:
        // 一旦越过 `limits`,速度就在一个余量的距离内从满速线性降到 0。
        // 两个轴各自收缩 —— 沿对角线越界时两个方向都要减速,只削一个轴
        // 会让玩家斜着「蹭」出边界。
        let over_x: f32 = ((at[0].abs() - limits[0]) / slack).clamp(0.0, 1.0);
        let over_z: f32 = ((at[1].abs() - limits[1]) / slack).clamp(0.0, 1.0);
        let scale_x: f32 = 1.0 - over_x;
        let scale_z: f32 = 1.0 - over_z;
        [velocity[0] * scale_x, velocity[1] * scale_z]
    }

    /// 软边界产生的回推速度(米/秒),方向指向世界中心。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 当前所在位置(世界 XZ,米)。
    /// - `Vec2` - 世界软边界半径(X / Z 各一半,米)。
    /// - `f32` - 允许越界的余量(米)。
    /// - `f32` - 回推增益(1/秒)。
    /// - `f32` - 回推速度上限(米/秒)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 回推速度;完全在界内时为零向量。
    pub fn soft_push(&self, at: Vec2, limits: Vec2, slack: f32, gain: f32, max_push: f32) -> Vec2 {
        let over_x: f32 = at[0].abs() - (limits[0] + slack);
        let over_z: f32 = at[1].abs() - (limits[1] + slack);
        if over_x <= 0.0 && over_z <= 0.0 {
            return [0.0, 0.0];
        }
        let mut push: Vec2 = [0.0, 0.0];
        if over_x > 0.0 {
            push[0] = -at[0].signum() * (over_x * gain).min(max_push);
        }
        if over_z > 0.0 {
            push[1] = -at[1].signum() * (over_z * gain).min(max_push);
        }
        push
    }

    /// 用车辆半径做分离(玩家开车时的碰撞体更大)。
    ///
    /// 与 [`Self::resolve`] 同样的迭代分离,只是把「玩家圆半径」换成
    /// 车辆半径。车辆因此也穿不过建筑 / 道具,只是被推挤得更早。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待分离的世界 XZ 坐标。
    /// - `f32` - 车辆的等效碰撞圆半径(米)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离并钳进边界后的世界 XZ 坐标。
    pub fn resolve_with_radius(&self, point: Vec2, radius: f32) -> Vec2 {
        let mut current: Vec2 = point;
        for _ in 0..RESOLVE_ITERATIONS {
            let mut moved: bool = false;
            for shape in self.get_shapes() {
                let hit: Option<(Vec2, f32)> = match shape {
                    Shape::Aabb { center, half } => push_out_aabb(*center, *half, current, radius),
                    Shape::Circle {
                        center,
                        radius: other,
                    } => push_out_circle(*center, *other, current, radius),
                };
                if let Some((direction, depth)) = hit {
                    current[0] += direction[0] * depth;
                    current[1] += direction[1] * depth;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        current
    }

    /// 用车辆的**真实有向足迹**做分离 —— 车与静态形状碰撞的唯一入口。
    ///
    /// 与 [`Self::resolve_with_radius`] 的区别有两条,两条都对应一个实测到的
    /// 缺陷:
    ///
    /// 1. **足迹是有向盒,不是圆。** 车的真实车身是 4.566 × 1.851 m
    ///    ([`CAR_FOOTPRINT`]),而 1.25 m 的圆最远只到 1.25 m —— 车头有
    ///    **1.033 m** 悬在碰撞边界外。满油门怼楼时实测车头扎进建筑 AABB
    ///    1.033 m,这就是「车穿过建筑围墙」的可见深度。
    /// 2. **位移按 [`CAR_STEP`] 切成子步,每步各做一次分离。** 满油门一帧
    ///    推进 1.60 m,而碰撞世界里最薄的静态盒只有 0.133 m,单步足以跨过
    ///    任何一堵薄墙 —— 那一帧分离看不到重叠,车就过去了。
    ///
    /// 子步还顺带解决了「圆被从薄盒的错误面弹出去」:每一步的穿透量都远
    /// 小于盒厚,落点永远走不到 `push_out_aabb` 的「点在盒内」分支,也就
    /// 永远不会被猜错面送到墙的另一侧。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 车心当前的世界 XZ 坐标(本帧起点)。
    /// - `Vec2` - 本帧想要的总位移(米)。
    /// - `f32` - 车身朝向(弧度,绕 Y 轴,与 `TrafficCar::get_yaw` 同一约定)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离后的世界 XZ 坐标。
    pub fn resolve_car_footprint(&self, here: Vec2, delta: Vec2, yaw: f32) -> Vec2 {
        let want: f32 = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
        if want <= SLIDE_EPSILON {
            return here;
        }
        let steps: usize = (want / CAR_STEP).ceil().max(1.0) as usize;
        let mut current: Vec2 = here;
        // 每一小步落在**整帧位移的一个精确分数**上,而不是「把 delta 平分
        // steps 份再累加」。后者在最后一步不会正好落回 `here + delta`:
        // 120 步的浮点累加误差实测约 1e-6 m,比 `f32::EPSILON`(1.19e-7)
        // 大一个数量级,于是调用点那一句「车有没有被挡」在**空旷地面上
        // 也会判成撞墙**,车每帧被砍掉 75% 的速度。插值写法让最后一步
        // 乘的是 `1.0`,结果与 `here + delta` 逐位相同。
        for step_index in 1..=steps {
            let fraction: f32 = step_index as f32 / steps as f32;
            let wanted: Vec2 = [here[0] + delta[0] * fraction, here[1] + delta[1] * fraction];
            let before: Vec2 = current;
            let step: Vec2 = [wanted[0] - before[0], wanted[1] - before[1]];
            current = self.resolve_car_box(wanted, yaw);
            // 这一小步基本被挡干净了就收手:车贴着墙不该还在原地反复
            // 推离(那会让它「立正」贴墙,失去蹭着墙走的能力)。
            let achieved: Vec2 = [current[0] - before[0], current[1] - before[1]];
            let want_dot: f32 = step[0] * step[0] + step[1] * step[1];
            let got_dot: f32 = achieved[0] * step[0] + achieved[1] * step[1];
            if got_dot < want_dot * SLIDE_MIN_PROGRESS {
                current = car_slide(current, step, wanted);
                break;
            }
        }
        current
    }

    /// 有向车辆足迹 vs 全部静态形状的分离(一次性,不切子步)。
    ///
    /// `resolve_car_footprint` 的单步内核,单独暴露是为了能直接单测「一步
    /// 里车能不能穿过去」。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 车心待分离的世界 XZ 坐标。
    /// - `f32` - 车身朝向(弧度,绕 Y 轴)。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离后的世界 XZ 坐标。
    pub fn resolve_car_box(&self, point: Vec2, yaw: f32) -> Vec2 {
        let (sin_yaw, cos_yaw): (f32, f32) = yaw.sin_cos();
        // 车身基向量,和 `TrafficCar::drive` 的第 3 步完全同一套:
        // `fwd = [cos yaw, -sin yaw]`、`right = [fwd[1], -fwd[0]]`。
        // 符号必须一模一样 —— 这里的 `right` 若取成 `[−fwd[1], fwd[0]]`,
        // 车身盒就成了左右翻转的镜像,斜着靠墙时会被判到墙的另一侧去。
        let forward: Vec2 = [cos_yaw, -sin_yaw];
        let side: Vec2 = [forward[1], -forward[0]];
        let mut current: Vec2 = point;
        for _ in 0..RESOLVE_ITERATIONS {
            let mut moved: bool = false;
            for shape in self.get_shapes() {
                let hit: Option<(Vec2, f32)> = match shape {
                    Shape::Aabb { center, half } => {
                        push_out_box_aabb(*center, *half, current, forward, side, CAR_FOOTPRINT)
                    }
                    Shape::Circle {
                        center,
                        radius: other,
                    } => push_out_circle(*center, *other, current, CAR_FOOTPRINT[1]),
                };
                if let Some((direction, depth)) = hit {
                    current[0] += direction[0] * depth;
                    current[1] += direction[1] * depth;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        current
    }

    /// 点是否落在某个形状内部(调试 / 自测用,不做分离)。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 世界 XZ 坐标。
    ///
    /// # Returns
    ///
    /// - `bool` - 圆心(零半径)与任一形状重叠时为 `true`。
    pub fn contains_point(&self, point: Vec2) -> bool {
        self.get_shapes().iter().any(|shape: &Shape| match shape {
            Shape::Aabb { center, half } => {
                (point[0] - center[0]).abs() <= half[0] && (point[1] - center[1]).abs() <= half[1]
            }
            Shape::Circle { center, radius } => {
                let dx: f32 = point[0] - center[0];
                let dy: f32 = point[1] - center[1];
                (dx * dx + dy * dy).sqrt() <= *radius
            }
        })
    }

    /// 两两分离一组动态实体,返回各自被推开后的新位置。
    ///
    /// **为什么要「按质量分」而不是「各推一半」:** 各推一半意味着撞上一辆
    /// 停着的车时,人会像撞上另一堵墙一样被弹开,而 1.5 t 的车却纹丝不动
    /// —— 这既不真实,手感也差(人被车「顶」住却推不动车)。按质量反比
    /// 分配,轻的那个几乎弹开、重的那个纹丝不动,才符合直觉。
    ///
    /// 动量守恒在这里是「穿透深度的分配」:总推开量 = 穿透深度,各自承担
    /// `m_other / (m_a + m_b)` 与 `m_a / (m_a + m_b)`。两车质量相同时各
    /// 承担一半(对撞后各退一半);人(1)撞车(30)时人承担 30/31、车承担
    /// 1/31 —— 人被弹飞、车几乎不动。
    ///
    /// 迭代若干轮直到稳定:三个人挤成一团时单轮只能解开一部分。
    ///
    /// # Arguments
    ///
    /// - `&[DynamicBody]` - 本帧所有动态实体。
    ///
    /// # Returns
    ///
    /// - `Vec<Vec2>` - 与输入同序的新位置(已互相分离)。
    pub fn resolve_dynamic(&self, bodies: &[DynamicBody]) -> Vec<Vec2> {
        let mut out: Vec<Vec2> = bodies
            .iter()
            .map(|body: &DynamicBody| body.get_position())
            .collect();
        for _ in 0..RESOLVE_ITERATIONS {
            let mut moved: bool = false;
            for i in 0..bodies.len() {
                for j in (i + 1)..bodies.len() {
                    let (Some(a), Some(b)) = (bodies.get(i), bodies.get(j)) else {
                        continue;
                    };
                    let reach: f32 = a.get_radius() + b.get_radius();
                    let delta: Vec2 = [out[i][0] - out[j][0], out[i][1] - out[j][1]];
                    let distance: f32 = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
                    if distance >= reach {
                        continue;
                    }
                    // 完全重合时给一个确定的分离方向,避免除零后
                    // 所有人往同一个方向抖。
                    // 行人之间额外留一点「个人空间」:两个人并排走时肩膀
                    // 不会贴着 —— 真实街景里人本来就是有间距的。
                    let reach: f32 = reach + same_kind_personal_space(a.get_kind(), b.get_kind());
                    let (normal, depth): (Vec2, f32) = if distance > INSIDE_EPSILON {
                        ([delta[0] / distance, delta[1] / distance], reach - distance)
                    } else {
                        ([1.0, 0.0], reach)
                    };
                    let mass_a: f32 = a.get_mass().max(0.01);
                    let mass_b: f32 = b.get_mass().max(0.01);
                    let total: f32 = mass_a + mass_b;
                    // 质量大的少让位 —— 推 `j` 的比例 = a 的质量占比。
                    // 质量加权:`i` 让位 `m_j/(m_i+m_j)`,`j` 让位
                    // `m_i/(m_i+m_j)`。轻的弹开、重的几乎不动。
                    let push_a: f32 = depth * (mass_b / total);
                    let push_b: f32 = depth * (mass_a / total);
                    // `normal` 指向 `i - j`(由 `out[i] - out[j]` 得到),
                    // 所以让 `i` 沿 `+normal` 退、`j` 沿 `-normal` 退。
                    out[i][0] += normal[0] * push_a;
                    out[i][1] += normal[1] * push_a;
                    out[j][0] -= normal[0] * push_b;
                    out[j][1] -= normal[1] * push_b;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        out
    }

    /// 把一组动态实体既与静态形状分离、又互相分离,返回最终位置。
    ///
    /// 这是动态层的**唯一入口**:车穿房子、树穿人、两个人穿在一起都在这里
    /// 一次解决。顺序是「先静态后动态」—— 先把每个体从墙里推出来,再让
    /// 它们互相让位;反过来的话,动态让位可能又把谁推进墙里。
    ///
    /// # Arguments
    ///
    /// - `&[DynamicBody]` - 本帧所有动态实体。
    ///
    /// # Returns
    ///
    /// - `Vec<Vec2>` - 与输入同序的最终世界 XZ 位置。
    pub fn resolve_all(&self, bodies: &[DynamicBody]) -> Vec<Vec2> {
        let mut placed: Vec<DynamicBody> = bodies.to_vec();
        // 静态分离:每个体用自己的**足迹**走一遍已有的迭代分离。
        //
        // 车的足迹是有向盒(`DynamicBody::new_box`),走的必须是
        // `push_out_box_aabb` —— 若这里退回圆的 `push_out_aabb`,它会把车
        // 按 1.25 m 推离墙面,而 `TrafficCar::drive` 刚把它停在 2.283 m,
        // 于是这一层又把车缩回 1.25 m,车头重新插进墙里 1.033 m。
        for body in placed.iter_mut() {
            body.set_position(self.separate_static(body.get_position(), body));
        }
        // 动态分离:质量加权的两两分离(玩家按玩家半径,车按车半径……)。
        let separated: Vec<Vec2> = self.resolve_dynamic(&placed);
        for (body, at) in placed.iter_mut().zip(separated.iter()) {
            body.set_position(*at);
        }
        // 动态让位可能又把谁推进了墙,最后再对静态收敛一次。
        let mut out: Vec<Vec2> = Vec::with_capacity(placed.len());
        for body in &placed {
            out.push(self.separate_static(body.get_position(), body));
        }
        out
    }

    /// 把一个动态体按它**自己的足迹**推出所有静态形状(迭代至稳定)。
    ///
    /// 车的足迹是有向盒,走 [`push_out_box_aabb`];其余实体是圆,走
    /// [`push_out_circle`]。这里必须按足迹分流而不是一律按圆 —— 一律按圆
    /// 会把 [`TrafficCar::drive`](crate::traffic::TrafficCar::drive) 刚停在
    /// 半车长处的车又缩回 1.25 m,车头重新插进墙里。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待分离的世界 XZ 坐标。
    /// - `&DynamicBody` - 提供足迹与朝向;**不读它的 `position`**。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 分离并迭代收敛后的世界 XZ 坐标。
    fn separate_static(&self, point: Vec2, body: &DynamicBody) -> Vec2 {
        let (sin_yaw, cos_yaw): (f32, f32) = body.get_yaw().sin_cos();
        let forward: Vec2 = [cos_yaw, -sin_yaw];
        let side: Vec2 = [forward[1], -forward[0]];
        let footprint: Vec2 = body.get_footprint();
        let mut current: Vec2 = point;
        for _ in 0..RESOLVE_ITERATIONS {
            let mut moved: bool = false;
            for shape in self.get_shapes() {
                let hit: Option<(Vec2, f32)> = match shape {
                    Shape::Aabb { center, half } => {
                        push_out_box_aabb(*center, *half, current, forward, side, footprint)
                    }
                    Shape::Circle {
                        center,
                        radius: other,
                    } => push_out_circle(*center, *other, current, footprint[1]),
                };
                if let Some((direction, depth)) = hit {
                    current[0] += direction[0] * depth;
                    current[1] += direction[1] * depth;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        current
    }

    /// 一点到所有静态形状表面的最短距离(米)。
    ///
    /// 落在某个形状**内部**时该形状贡献 0,整体取最小值 —— 所以返回值
    /// 为 0 就等价于「这个点在某个碰撞体里」。相机遮挡回避用它判断眼点
    /// 是否已经扎进楼里,验收调试通道用它把眼点的实际位置报出来。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 世界 XZ 坐标。
    ///
    /// # Returns
    ///
    /// - `f32` - 到最近形状表面的距离(米);空碰撞世界时为 `f32::MAX`。
    pub fn nearest_surface_distance(&self, point: Vec2) -> f32 {
        let mut best: f32 = f32::MAX;
        for shape in self.get_shapes() {
            let candidate: f32 = distance_to_shape(shape, point);
            if candidate < best {
                best = candidate;
            }
        }
        best
    }
}

/// Default construction for [`CollisionWorld`].
impl Default for CollisionWorld {
    /// 返回空碰撞世界,与 `CollisionWorld::new` 等价。
    fn default() -> Self {
        CollisionWorld::new()
    }
}

/// Inherent implementation of [`DynamicBody`].
impl DynamicBody {
    /// 构造一个**圆形**动态碰撞体。
    ///
    /// # Arguments
    ///
    /// - `BodyKind` - 实体类型。
    /// - `Vec2` - 圆心世界 XZ 坐标。
    /// - `f32` - 碰撞圆半径(米)。
    /// - `f32` - 质量(以「一个人」为单位)。
    ///
    /// # Returns
    ///
    /// - `Self` - 就绪的动态碰撞体。
    pub const fn new(kind: BodyKind, position: Vec2, radius: f32, mass: f32) -> Self {
        Self {
            kind,
            position,
            footprint: [radius, radius],
            yaw: 0.0,
            mass,
        }
    }

    /// 构造一个**有向盒**动态碰撞体(车用)。
    ///
    /// # Arguments
    ///
    /// - `BodyKind` - 实体类型。
    /// - `Vec2` - 足迹中心的世界 XZ 坐标。
    /// - `Vec2` - 足迹半长 / 半宽(米)。
    /// - `f32` - 足迹朝向(弧度,绕 Y 轴)。
    /// - `f32` - 质量(以「一个人」为单位)。
    ///
    /// # Returns
    ///
    /// - `Self` - 就绪的动态碰撞体。
    pub const fn new_box(
        kind: BodyKind,
        position: Vec2,
        footprint: Vec2,
        yaw: f32,
        mass: f32,
    ) -> Self {
        Self {
            kind,
            position,
            footprint,
            yaw,
            mass,
        }
    }

    /// 该实体的碰撞半径(米)。
    ///
    /// 取足迹两轴的**最大值**:圆形足迹下两者相等,就是原来的半径;车
    /// 走这条通道时拿到半车长 2.283 m,用于车与车的圆近似互相分离。
    ///
    /// # Returns
    ///
    /// - `f32` - 半径(米)。
    pub fn get_radius(&self) -> f32 {
        self.footprint[0].max(self.footprint[1])
    }
}
