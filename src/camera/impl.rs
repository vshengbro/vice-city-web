use super::*;

impl Mat4 {
    /// 由列主序 16 元数组构造。
    ///
    /// # Arguments
    ///
    /// - `Mat4Data` - 列主序排列的 16 个矩阵元素。
    ///
    /// # Returns
    ///
    /// - `Self` - 持有该元素数组的矩阵。
    pub const fn from_column_major(elements: Mat4Data) -> Self {
        Self { elements }
    }

    /// 矩阵乘法 `self * other`(数学意义上的乘法,注意顺序)。
    ///
    /// # Arguments
    ///
    /// - `&Mat4` - 右乘的矩阵。
    ///
    /// # Returns
    ///
    /// - `Mat4` - 乘积矩阵。
    pub fn multiply(&self, other: &Mat4) -> Mat4 {
        let a: &[f32; 16] = &self.get_elements();
        let b: &[f32; 16] = &other.get_elements();
        let mut out: [f32; 16] = [0.0; 16];
        for col in 0..4 {
            for row in 0..4 {
                let mut sum: f32 = 0.0;
                for k in 0..4 {
                    sum += a[k * 4 + row] * b[col * 4 + k];
                }
                out[col * 4 + row] = sum;
            }
        }
        Mat4::from_column_major(out)
    }

    /// 平移矩阵。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 平移向量(米)。
    ///
    /// # Returns
    ///
    /// - `Mat4` - 列主序平移矩阵。
    pub fn translation(offset: Vec3) -> Mat4 {
        let mut out: Mat4Data = [0.0; 16];
        out[0] = 1.0;
        out[5] = 1.0;
        out[10] = 1.0;
        out[15] = 1.0;
        out[12] = offset[0];
        out[13] = offset[1];
        out[14] = offset[2];
        Mat4::from_column_major(out)
    }

    /// 绕 X 轴旋转矩阵(用于步态的肩摆 / 髋摆)。
    ///
    /// # Arguments
    ///
    /// - `f32` - 旋转角(弧度)。
    ///
    /// # Returns
    ///
    /// - `Mat4` - 列主序旋转矩阵。
    pub fn rotation_x(radians: f32) -> Mat4 {
        let (sine, cosine) = radians.sin_cos();
        Mat4::from_column_major([
            1.0, 0.0, 0.0, 0.0, 0.0, cosine, sine, 0.0, 0.0, -sine, cosine, 0.0, 0.0, 0.0, 0.0, 1.0,
        ])
    }

    /// 绕 Y 轴旋转矩阵(与 `Instance::new` 的朝向约定一致)。
    ///
    /// # Arguments
    ///
    /// - `f32` - 旋转角(弧度)。
    ///
    /// # Returns
    ///
    /// - `Mat4` - 列主序旋转矩阵。
    pub fn rotation_y(radians: f32) -> Mat4 {
        let (sine, cosine) = radians.sin_cos();
        Mat4::from_column_major([
            cosine, 0.0, -sine, 0.0, 0.0, 1.0, 0.0, 0.0, sine, 0.0, cosine, 0.0, 0.0, 0.0, 0.0, 1.0,
        ])
    }

    /// 右手系透视投影矩阵,深度映射到 WebGL 的 `[-1, 1]`。
    ///
    /// # Arguments
    ///
    /// - `f32` - 垂直视场角(弧度)。
    /// - `f32` - 宽高比(width / height)。
    /// - `f32` - 近裁剪面距离(正数)。
    /// - `f32` - 远裁剪面距离(正数)。
    ///
    /// # Returns
    ///
    /// - `Mat4` - 投影矩阵。
    pub fn perspective(fov_y_rad: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
        let f: f32 = 1.0 / (fov_y_rad * 0.5).tan();
        let range: f32 = 1.0 / (near - far);
        let sx: f32 = f / aspect;
        let sy: f32 = f;
        let sz: f32 = (far + near) * range;
        let sz_translate: f32 = 2.0 * far * near * range;
        Mat4::from_column_major([
            sx,
            0.0,
            0.0,
            0.0, //
            0.0,
            sy,
            0.0,
            0.0, //
            0.0,
            0.0,
            sz,
            -1.0, //
            0.0,
            0.0,
            sz_translate,
            0.0,
        ])
    }

    /// 右手系 look-at 视图矩阵(世界 → 相机)。
    ///
    /// 约定:相机看向 `-Z`,`+Y` 向上(与 WebGL 的 gl_Position 语义一致)。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 相机眼点世界坐标。
    /// - `Vec3` - 注视中心世界坐标。
    /// - `Vec3` - 上方向向量。
    ///
    /// # Returns
    ///
    /// - `Mat4` - 视图矩阵。
    pub fn look_at(eye: Vec3, center: Vec3, up: Vec3) -> Mat4 {
        let f: [f32; 3] = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let len_f: f32 = f[0] * f[0] + f[1] * f[1] + f[2] * f[2];
        let inv_len: f32 = if len_f <= f32::EPSILON {
            0.0
        } else {
            1.0 / len_f.sqrt()
        };
        let s: [f32; 3] = [
            f[1] * up[2] - f[2] * up[1],
            f[2] * up[0] - f[0] * up[2],
            f[0] * up[1] - f[1] * up[0],
        ];
        let len_s: f32 = s[0] * s[0] + s[1] * s[1] + s[2] * s[2];
        let inv_len_s: f32 = if len_s <= f32::EPSILON {
            0.0
        } else {
            1.0 / len_s.sqrt()
        };
        let s_norm: [f32; 3] = [s[0] * inv_len_s, s[1] * inv_len_s, s[2] * inv_len_s];
        let u: [f32; 3] = [
            s_norm[1] * f[2] * inv_len - s_norm[2] * f[1] * inv_len,
            s_norm[2] * f[0] * inv_len - s_norm[0] * f[2] * inv_len,
            s_norm[0] * f[1] * inv_len - s_norm[1] * f[0] * inv_len,
        ];
        Mat4::from_column_major([
            s_norm[0],
            u[0],
            -f[0] * inv_len,
            0.0, //
            s_norm[1],
            u[1],
            -f[1] * inv_len,
            0.0, //
            s_norm[2],
            u[2],
            -f[2] * inv_len,
            0.0, //
            -(s_norm[0] * eye[0] + s_norm[1] * eye[1] + s_norm[2] * eye[2]),
            -(u[0] * eye[0] + u[1] * eye[1] + u[2] * eye[2]),
            f[0] * inv_len * eye[0] + f[1] * inv_len * eye[1] + f[2] * inv_len * eye[2],
            1.0,
        ])
    }

    /// 用矩阵变换一个点(返回 `[x, y, z, w]`,未做透视除法)。
    ///
    /// # Arguments
    ///
    /// - `Vec4` - 待变换的四维齐次坐标。
    ///
    /// # Returns
    ///
    /// - `Vec4` - 变换后的齐次坐标。
    pub fn transform_vec4(&self, v: Vec4) -> Vec4 {
        let a: &[f32; 16] = &self.get_elements();
        let mut out: [f32; 4] = [0.0; 4];
        for row in 0..4 {
            out[row] = a[row] * v[0] + a[4 + row] * v[1] + a[8 + row] * v[2] + a[12 + row] * v[3];
        }
        out
    }
}

impl Camera {
    /// 创建一台默认相机:街区全景视角。
    ///
    /// 街区沿 Z 轴延伸约 ±48 m,街道半宽 7 m,因此默认镜头要退到
    /// 能同时看到两侧建筑立面、路面和几辆停车的位置,而不是贴脸视角。
    pub fn new() -> Self {
        Self {
            target: [0.0, 4.0, -14.0],
            distance: 250.0,
            yaw: 0.0,
            pitch: 0.42,
            fov_y: FRAC_PI_4,
            near: 0.1,
            far: 900.0,
            desired_distance: 250.0,
            eye_clearance: f32::MAX,
            occluded: false,
        }
    }

    /// 返回注视焦点的可变引用。
    ///
    /// # Returns
    ///
    /// - `&mut Vec3` - 注视焦点世界坐标的可变引用。
    pub fn get_target_mut(&mut self) -> &mut Vec3 {
        &mut self.target
    }

    /// 覆盖期望跟随距离,并让 `distance` 跟上(未被遮挡时即刻生效)。
    ///
    /// 滚轮 / TAB 复位走这条路径。遮挡回避每帧写的 `distance` 不会经过
    /// 这里,所以「玩家想拉近一点」不会被回避逻辑覆盖掉。
    ///
    /// # Arguments
    ///
    /// - `f32` - 新的期望距离(米)。
    pub fn set_desired_distance(&mut self, value: f32) {
        self.desired_distance = value;
        if !self.get_occluded() {
            self.set_distance(value);
        }
    }

    /// 第三人称俯仰的合法区间(弧度)。
    ///
    /// 下限保证**眼点永远在角色上方**:pitch 为负时相机在焦点下方,
    /// 而地面是整张 `y = 0` 的网格,从地下看出去整屏只有地面色 / 天空
    /// 色,角色和街景全被地面挡住。上限避免接近垂直时的万向节翻转。
    pub const FOLLOW_PITCH_MIN: f32 = -0.05;
    /// 第三人称俯仰的合法上限(弧度)。
    ///
    /// GTA III / VC 的越肩相机俯角上限约 34°(0.60 rad);1.05 rad
    /// 等于 60°,是 GTA 的近两倍,俯视下去只剩楼顶和马路,失去街道感。
    pub const FOLLOW_PITCH_MAX: f32 = 0.60;

    /// 把第三人称的 pitch 收进「相机在角色上方」的区间。
    pub fn clamp_follow_pitch(&mut self) {
        let clamped: f32 = self
            .get_pitch()
            .clamp(Self::FOLLOW_PITCH_MIN, Self::FOLLOW_PITCH_MAX);
        self.set_pitch(clamped);
    }

    /// 钳制 pitch,避免接近垂直时的万向节翻转。
    ///
    /// **只夹 pitch,不动距离。** 之前这里顺带调了 `confine_to_city()`,
    /// 而 `confine_to_city` 收尾会 `clamp_distance()` 把 distance 顶到轨道
    /// 相机的下限 24 m。游戏主循环每帧都调 `clamp_pitch`,于是第三人称
    /// 跟随的 5.6 m 每帧被顶回 24 m:角色缩成几个像素,相机越拉越高,
    /// 连地面都被挤出取景框(截图里只剩一片蓝天)。距离的钳制是
    /// `clamp_distance` / `clamp_follow_distance` 的职责,谁改距离谁夹。
    pub fn clamp_pitch(&mut self) {
        let limit: f32 = FRAC_PI_2 - 0.02;
        let clamped: f32 = self.get_pitch().clamp(-limit, limit);
        self.set_pitch(clamped);
    }

    /// 钳制距离,防止穿模或跑到无穷远。
    pub fn clamp_distance(&mut self) {
        // **自由观察的最小距离是 24 m** —— 这是为「绕着整座城市看」的
        // 轨道相机定的,第三人称跟随完全用不上。之前 `confine_to_city`
        // 无条件收尾调 `clamp_distance`,把第三人称的 5.6 m 硬生生顶到
        // 24 m:角色小到只剩几个像素,而且镜头越拉越远,连地面都被挤出
        // 取景框(截图里只剩一片蓝天)。
        let clamped: f32 = self
            .get_distance()
            .clamp(ORBIT_MIN_DISTANCE, ORBIT_MAX_DISTANCE);
        self.set_distance(clamped);
    }

    /// 第三人称跟随专用的距离钳制(允许贴近角色)。
    ///
    /// # Arguments
    ///
    /// - `f32` - 允许的最近距离(米)。
    /// - `f32` - 允许的最远距离(米)。
    pub fn clamp_follow_distance(&mut self, min: f32, max: f32) {
        let clamped: f32 = self.get_distance().clamp(min, max);
        self.set_distance(clamped);
    }

    /// 沿「焦点 → 眼点」方向扫描碰撞世界,求出相机不被穿模的最远距离。
    ///
    /// 这是**纯查询**,不写 `distance` —— 分成两步是因为回避的「命中多近」
    /// 和「实际走到多近」必须解耦:
    ///
    /// - 命中距离在镜头快速扫过一栋楼时会一帧一变(掠射角),直接用
    ///   距离赋值相机会疯狂抽搐;
    /// - 所以这里只返回**这一帧允许的目标距离**,由调用方做阻尼逼近。
    ///
    /// **返回 `None` 表示这一帧没有遮挡** —— 调试通道据此报告
    /// `camOccluded`,验收脚本读它才能证明「回避真的触发了」而不是
    /// 「每帧都被贴墙余量削掉 0.45 m」。
    ///
    /// 命中距离**超过**期望距离时同样返回 `None`:那说明挡在中间的东西
    /// 本来就在相机该待的位置之外,扣贴墙余量只会让相机无缘无故短一截
    /// (这正是「第三人称永远停在 6.95 m 而不是 7.4 m」那个 bug 的成因)。
    ///
    /// 焦点本身可能落在碰撞体里(玩家被挤进墙角的极端情况):此时任何
    /// 朝外的射线都立刻命中,回避无从判断方向,直接放弃,交给距离硬下限
    /// 兜底。
    ///
    /// # Arguments
    ///
    /// - `&CollisionWorld` - 静态碰撞世界的只读引用。
    /// - `f32` - 期望的最大距离(米)。
    ///
    /// # Returns
    ///
    /// - `Option<f32>` - 真正遮挡时为「本帧允许的最远距离(米)」;否则为 `None`。
    pub fn resolve_occlusion(&self, world: &CollisionWorld, max_distance: f32) -> Option<f32> {
        let target: Vec3 = self.get_target();
        if world.contains_point([target[0], target[2]]) {
            return None;
        }
        let forward: Vec3 = self.eye_direction();
        // 侧向 = 视线方向在 XZ 上的左法线,三条射线共用同一个起点。
        let flat: f32 = (forward[0] * forward[0] + forward[2] * forward[2]).sqrt();
        if flat < 1.0e-5 {
            // 视线完全竖直:地面之上没有东西能挡,交给 `lift_above_ground`。
            return None;
        }
        let side: Vec3 = [-forward[2] / flat, 0.0, forward[0] / flat];
        // 侧线在距离 `t` 处的横向偏移 = `t · tan(FOV/2) · 宽度系数`。
        let spread: f32 = (self.get_fov_y() * 0.5).tan() * OCCLUSION_COVERAGE_WIDTH;
        let mut tightest: Option<f32> = None;
        for sign in [-1.0_f32, 0.0, 1.0] {
            let dir: Vec3 = [
                forward[0] + side[0] * spread * sign,
                forward[1],
                forward[2] + side[2] * spread * sign,
            ];
            let Some((hit, _point)) = ray_to_shapes(world, target, dir) else {
                continue;
            };
            if hit >= max_distance {
                continue;
            }
            // `sign != 0` 的侧线更长(斜着走),要按投影折回焦点轴上,
            // 否则会高估遮挡范围、把相机缩得比需要更近。
            let along: f32 = if sign == 0.0 {
                hit
            } else {
                hit / (1.0 + spread * spread).sqrt()
            };
            tightest = Some(match tightest {
                Some(current) => current.min(along),
                None => along,
            });
        }
        tightest.map(|limit: f32| (limit - OCCLUSION_SKIN).max(0.0))
    }

    /// 相机回避的**室内**版本:额外考虑 [`crate::interior::FloorWorld`]。
    ///
    /// 玩家站进样板楼之后,眼点到焦点之间横着的往往是**隔墙 / 门垛**,
    /// 它们根本不在 `CollisionWorld` 里(那里面装的是整车城,样板楼为了
    /// 能走进去被特意排除了)。只查 `CollisionWorld` 的相机会直接穿墙:
    /// 隔墙在镜头上糊成一片平面,玩家在另一侧完全看不见。
    ///
    /// 这里用与 [`Self::resolve_occlusion`] 相同的三射线 + 球体探针规则,
    /// 只是把求交换成「沿射线按步长采样,被室内碰撞体包住就记下距离」。
    /// 室内形状总数是常数级(两栋楼 ~30 件),按步长采样的开销可以忽略,
    /// 而换来的是**和现有回避手感完全一致**的参数与阻尼。
    ///
    /// # Arguments
    ///
    /// - `&CollisionWorld` - 静态碰撞世界的只读引用。
    /// - `&FloorWorld` - 室内碰撞世界的只读引用。
    /// - `f32` - 期望的最大距离(米)。
    ///
    /// # Returns
    ///
    /// - `Option<f32>` - 真正遮挡时为「本帧允许的最远距离(米)」;否则为 `None`。
    pub fn resolve_occlusion_interior(
        &self,
        world: &CollisionWorld,
        interiors: &FloorWorld,
        max_distance: f32,
    ) -> Option<f32> {
        let outside: Option<f32> = self.resolve_occlusion(world, max_distance);
        let target: Vec3 = self.get_target();
        // 视线垂直:地面上没东西可挡,沿用外部世界的结果。
        let forward: Vec3 = self.eye_direction();
        let flat: f32 = (forward[0] * forward[0] + forward[2] * forward[2]).sqrt();
        if flat < 1.0e-5 {
            return outside;
        }
        let side: Vec3 = [-forward[2] / flat, 0.0, forward[0] / flat];
        let spread: f32 = (self.get_fov_y() * 0.5).tan() * OCCLUSION_COVERAGE_WIDTH;
        // 眼点高度就是焦点高度加上俯角带来的那一段:回避时相机就在
        // 这条线上,所以用它决定「墙够不够得着」。
        let eye: Vec3 = self.eye();
        let mut tightest: Option<f32> = outside;
        for sign in [-1.0_f32, 0.0, 1.0] {
            let dir: Vec3 = [
                forward[0] + side[0] * spread * sign,
                forward[1],
                forward[2] + side[2] * spread * sign,
            ];
            let Some(hit) = interior_ray_hit(interiors, target, eye, dir, max_distance) else {
                continue;
            };
            let along: f32 = if sign == 0.0 {
                hit
            } else {
                hit / (1.0 + spread * spread).sqrt()
            };
            tightest = Some(match tightest {
                Some(current) => current.min(along),
                None => along,
            });
        }
        tightest.map(|limit: f32| (limit - OCCLUSION_SKIN).max(0.0))
    }

    /// 眼点相对焦点的单位方向向量(焦点 → 眼点)。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 单位方向。
    pub fn eye_direction(&self) -> Vec3 {
        let (sp, cp): (f32, f32) = self.get_pitch().sin_cos();
        let (sy, cy): (f32, f32) = self.get_yaw().sin_cos();
        // 与 [`Self::eye`] 同一套 yaw 约定:焦点 → 眼点 = 前向的反方向。
        [-cy * cp, sp, sy * cp]
    }

    /// 阻尼逼近一个目标距离,并把结果夹在 `[OCCLUSION_MIN_DISTANCE, max]`。
    ///
    /// **拉近比拉远快**:贴墙时相机必须立刻缩进来(否则仍有一两帧糊脸),
    /// 离开墙时慢慢弹回(否则镜头会像弹簧一样抖)。两个速率都是
    /// 指数阻尼 `1 - exp(-rate * dt)`,与帧率无关。
    ///
    /// # Arguments
    ///
    /// - `f32` - 本帧允许的目标距离(米)。
    /// - `f32` - 距离硬上限(米)。
    /// - `f32` - 本帧秒数增量。
    /// - `f32` - 拉近速率(1/秒)。
    /// - `f32` - 拉远速率(1/秒)。
    pub fn approach_distance(
        &mut self,
        target: f32,
        max: f32,
        dt: f32,
        in_rate: f32,
        out_rate: f32,
    ) {
        self.approach_distance_within(target, max, dt, in_rate, out_rate, OCCLUSION_MIN_DISTANCE);
    }

    /// [`Self::approach_distance`] 的可调下限版本:把「最近能贴到多近」
    /// 交给调用方按场景决定。
    ///
    /// 分成两个函数而不是加一个可选参数,是因为下限不是「有没有」的问题
    /// 而是「哪里」的问题:室外 2.2 m 是防穿楼的安全网,室内 0.45 m 才是
    /// 「贴墙时不穿墙」的正解。调用方必须显式说出自己处在哪一种场景里。
    ///
    /// # Arguments
    ///
    /// - `f32` - 本帧允许的目标距离(米)。
    /// - `f32` - 距离硬上限(米)。
    /// - `f32` - 本帧秒数增量。
    /// - `f32` - 拉近速率(1/秒)。
    /// - `f32` - 拉远速率(1/秒)。
    /// - `f32` - 距离下限(米)。
    pub fn approach_distance_within(
        &mut self,
        target: f32,
        max: f32,
        dt: f32,
        in_rate: f32,
        out_rate: f32,
        floor: f32,
    ) {
        let current: f32 = self.get_distance();
        let rate: f32 = if target < current { in_rate } else { out_rate };
        let blend: f32 = (1.0 - (-rate * dt).exp()).clamp(0.0, 1.0);
        let stepped: f32 = current + (target - current) * blend;
        self.set_distance(stepped.clamp(floor.min(max), max));
    }

    /// 把眼点抬到地面之上,避免相机沉进 `y = 0` 的地面网格。
    ///
    /// 碰撞体里只有建筑 / 车辆 / 道具,没有地面,所以这一条是纯几何
    /// 修正。只在俯角为负(视线朝下)且焦点离地够低时才会真正缩短距离;
    /// 俯角为正(常规第三人称)时相机本来就在高处,直接返回。
    pub fn lift_above_ground(&mut self) {
        let target: Vec3 = self.get_target();
        let direction: Vec3 = self.eye_direction();
        // 方向朝上(y 分量 > 0)时相机只会更高,永远碰不到地面。
        if direction[1] > 0.0 {
            return;
        }
        // 眼点高度 = target.y + direction.y * distance,要 >= CAMERA_MIN_HEIGHT。
        let headroom: f32 = CAMERA_MIN_HEIGHT - target[1];
        // 焦点本来就比眼点下限还高(`FOLLOW_HEIGHT` 1.45 > `CAMERA_MIN_HEIGHT`
        // 0.45),任何朝下的距离都满足约束。此时 `headroom` 为负,除出来
        // 的 `needed` 也是负的,`.max()` 会把距离钳到下限 —— 第三人称
        // 相机在任何俯角 <= 0 的地方都被粘在 2.2 m 上。焦点已经够高就
        // 直接返回,地面修正不该存在。
        if headroom <= 0.0 {
            return;
        }
        let needed: f32 = if direction[1] < -1.0e-4 {
            headroom / -direction[1]
        } else {
            0.0
        };
        let allowed: f32 = self.get_distance().min(needed);
        self.set_distance(allowed.max(OCCLUSION_MIN_DISTANCE));
    }

    /// 把眼点约束回城市范围内。
    ///
    /// 城市是 300 m × 300 m 的开放网格,不像原来的单条街道那样有
    /// 「走廊」可钻。因此这里约束的是**焦点与眼点都落在城市边界内**:
    /// 焦点被钳在 `[-CITY_..., CITY_...]`,眼点单独钳一次,保证平移
    /// (WASD)、缩放(滚轮 / 捏合)、拖拽转视角之后视角始终成立 ——
    /// 既不会飞到城市外面看到虚空,也不会钻进楼群里被近处墙面糊死。
    pub fn confine_to_city(&mut self) {
        self.confine_follow(true)
    }

    /// 第三人称跟随版的边界约束:只夹焦点,不动距离下限。
    ///
    /// # Arguments
    ///
    /// - `bool` - `true` 表示自由观察(应用 24 m 的轨道距离下限)。
    fn confine_follow(&mut self, orbit: bool) {
        {
            let target: &mut [f32; 3] = self.get_target_mut();
            target[0] = target[0].clamp(-CITY_BOUNDS, CITY_BOUNDS);
            target[2] = target[2].clamp(-CITY_BOUNDS, CITY_BOUNDS);
            target[1] = target[1].clamp(0.5, 90.0);
        }
        // 眼点被 distance 拉到城外时,缩短 distance 直到眼点回到边界内。
        let eye: [f32; 3] = self.eye();
        if eye[0].abs() > CITY_BOUNDS || eye[2].abs() > CITY_BOUNDS {
            let mut distance: f32 = self.get_distance();
            for _ in 0..24 {
                let (_, cp): (f32, f32) = self.get_pitch().sin_cos();
                let (sy, cy): (f32, f32) = self.get_yaw().sin_cos();
                let target: [f32; 3] = self.get_target();
                let x: f32 = target[0] + sy * cp * distance;
                let z: f32 = target[2] + cy * cp * distance;
                if x.abs() <= CITY_BOUNDS && z.abs() <= CITY_BOUNDS {
                    break;
                }
                distance -= distance * 0.12;
            }
            self.set_distance(distance.max(ORBIT_MIN_DISTANCE));
        }
        if orbit {
            self.clamp_distance();
        }
    }

    /// 计算相机在世界空间中的眼位置。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 眼点世界坐标。
    pub fn eye(&self) -> Vec3 {
        let (sp, cp): (f32, f32) = self.get_pitch().sin_cos();
        let (sy, cy): (f32, f32) = self.get_yaw().sin_cos();
        let target: [f32; 3] = self.get_target();
        let distance: f32 = self.get_distance();
        // 球坐标:从焦点沿 yaw/pitch 反方向退后 `distance`。
        //
        // yaw 的约定必须和「玩家朝向」完全一致:玩家的前向是
        // `(cos yaw, -sin yaw)`(见 `player::Player::step` 用的 `direction`),
        // 所以相机要退到**前向的反方向**,即
        // `(-cos yaw·cos pitch, sin pitch, +sin yaw·cos pitch)`。
        // 早先这里用的是 `(sin yaw, cos yaw)`,和玩家前向差了 90° ——
        // 角色明明朝西走,镜头却站在正南,于是出生时镜头正好怼在
        // 车道对面的行道树上,画面里根本看不到角色。
        [
            target[0] - cy * cp * distance,
            target[1] + sp * distance,
            target[2] + sy * cp * distance,
        ]
    }

    /// 相机的前向单位向量(眼 → 焦点)。
    ///
    /// # Returns
    ///
    /// - `Vec3` - 前向单位向量。
    pub fn forward(&self) -> Vec3 {
        let eye: [f32; 3] = self.eye();
        let target: [f32; 3] = self.get_target();
        let d: [f32; 3] = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
        let len: f32 = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len <= f32::EPSILON {
            [0.0, 0.0, -1.0]
        } else {
            [d[0] / len, d[1] / len, d[2] / len]
        }
    }

    /// 视图矩阵。
    ///
    /// # Returns
    ///
    /// - `Mat4` - 世界坐标到相机坐标的视图矩阵。
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at(self.eye(), self.get_target(), [0.0, 1.0, 0.0])
    }

    /// 投影矩阵,`aspect` = 宽 / 高。
    ///
    /// # Arguments
    ///
    /// - `f32` - 宽高比(width / height)。
    ///
    /// # Returns
    ///
    /// - `Mat4` - 透视投影矩阵。
    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        Mat4::perspective(self.get_fov_y(), aspect, self.get_near(), self.get_far())
    }

    /// 视图投影矩阵。
    ///
    /// # Arguments
    ///
    /// - `f32` - 宽高比(width / height)。
    ///
    /// # Returns
    ///
    /// - `Mat4` - 视图与投影的乘积矩阵。
    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection_matrix(aspect).multiply(&self.view_matrix())
    }

    /// 把世界坐标点投影到屏幕像素坐标。
    ///
    /// 返回 `Some((x, y, depth))`;若点在相机后方(w <= 0)返回 `None`。
    /// `depth` 为归一化深度 `[0, 1]`(近平面 0、远平面 1),供软件渲染排序用。
    ///
    /// # Arguments
    ///
    /// - `Vec3` - 世界坐标点。
    /// - `f32` - 画布宽度(像素)。
    /// - `f32` - 画布高度(像素)。
    ///
    /// # Returns
    ///
    /// - `Option<(f32, f32, f32)>` - 屏幕 `(x, y)` 与归一化深度;点在相机后方时为 `None`。
    pub fn world_to_screen(&self, point: Vec3, width: f32, height: f32) -> Option<(f32, f32, f32)> {
        let vp: Mat4 = self.view_projection(width / height.max(f32::EPSILON));
        let clip: [f32; 4] = vp.transform_vec4([point[0], point[1], point[2], 1.0]);
        if clip[3] <= f32::EPSILON {
            return None;
        }
        let inv_w: f32 = 1.0 / clip[3];
        let ndc_x: f32 = clip[0] * inv_w;
        let ndc_y: f32 = clip[1] * inv_w;
        let ndc_z: f32 = clip[2] * inv_w;
        let x: f32 = (ndc_x * 0.5 + 0.5) * width;
        let y: f32 = (1.0 - (ndc_y * 0.5 + 0.5)) * height;
        let depth: f32 = (ndc_z * 0.5 + 0.5).clamp(0.0, 1.0);
        Some((x, y, depth))
    }
}

impl Default for Camera {
    /// 返回默认相机,与 `Camera::new` 等价。
    fn default() -> Self {
        Camera::new()
    }
}
