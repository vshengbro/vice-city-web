use super::*;

/// 从 `origin` 沿 `dir` 推进一个球体探针,求撞上第一个室内碰撞体的距离。
///
/// 走的是「球体推进」而不是零半径射线:探针半径当成相机本体的尺寸,
/// 于是命中距离等于「球面贴上墙」的那一刻,掠射角下不会剧烈跳变 ——
/// 与 [`ray_to_shapes`] 处理外部世界的做法完全一致。
///
/// # Arguments
///
/// - `&FloorWorld` - 室内碰撞世界。
/// - `Vec3` - 射线起点(焦点)。
/// - `Vec3` - 射线的另一端参考点(眼点),只用来定探针的高度区间。
/// - `Vec3` - 单位方向。
/// - `f32` - 最多推进多远(米)。
///
/// # Returns
///
/// - `Option<f32>` - 命中距离(米);未命中为 `None`。
pub(crate) fn interior_ray_hit(
    interiors: &FloorWorld,
    origin: Vec3,
    eye: Vec3,
    dir: Vec3,
    max_distance: f32,
) -> Option<f32> {
    let mut travelled: f32 = 0.0;
    while travelled <= max_distance {
        let point: Vec3 = [
            origin[0] + dir[0] * travelled,
            origin[1] + dir[1] * travelled,
            origin[2] + dir[2] * travelled,
        ];
        let at: Vec2 = [point[0], point[2]];
        let blocked: bool = interiors
            .get_floors()
            .iter()
            .any(|floor: &crate::interior::Floor| {
                let (min, max): (Vec3, Vec3) = match floor {
                    crate::interior::Floor::Slab { min, max } => (*min, *max),
                    crate::interior::Floor::Wall { min, max } => (*min, *max),
                };
                // **脚下那块楼板不挡自己的视线。** 判据看**下表面**而不是
                // 顶面:楼梯每一级都是 `y = [地面, 级高]` 的实心盒,下表面
                // 全都贴着地面,所以用下表面判时楼梯永远不挡自己,只有真正
                // 越过头顶的二层楼板才计入。
                if let crate::interior::Floor::Slab { min, max } = floor {
                    if min[1] <= origin[1] + SLAB_HEADROOM {
                        return false;
                    }
                }
                // **高度必须逐采样点判,不能用整条射线的总区间。**
                // `low` / `high` 覆盖的是「焦点高度到眼点高度」的全程,
                // 拿它筛碰撞体等于假设「凡是落在这条高度带里的墙都挡视线」。
                // 但视线在途中会**钻到墙脚以下**:玩家站在楼梯上、相机在
                // 身后偏低时,采样点的高度已经低于外墙(外墙下表面 y = 0)
                // 之上但低于探针下沿,或者干脆从门洞下沿穿过去 —— 原来的
                // 写法把整面墙算成命中,实测玩家在楼梯第一级时 `cameraDist`
                // 被压到 0.71 m,而那条视线上一面墙都没有。
                if point[1] < min[1] - CAMERA_PROBE_RADIUS
                    || point[1] > max[1] + CAMERA_PROBE_RADIUS
                {
                    return false;
                }
                at[0] >= min[0] - CAMERA_PROBE_RADIUS
                    && at[0] <= max[0] + CAMERA_PROBE_RADIUS
                    && at[1] >= min[2] - CAMERA_PROBE_RADIUS
                    && at[1] <= max[2] + CAMERA_PROBE_RADIUS
            });
        if blocked {
            return Some(travelled);
        }
        travelled += INTERIOR_PROBE_STEP;
    }
    None
}

/// 背面剔除测试:三角形是否朝向相机。
///
/// 约定:资产里的 `faces` 顶点顺序为「从外侧看逆时针(CCW)」。
/// 在右手系 + 屏幕 Y 向下的像素坐标下,CCW 三角形在屏幕上变成顺时针,
/// 因此可见面的有符号面积(按屏幕坐标)为 **负**。
///
/// 这里采用更稳健的等价判据:计算面法线与「面 → 眼睛」向量。
/// `dot > 0` 表示法线背离相机,判定为背面,应被剔除。
///
/// 退化三角形(零面积)同样剔除,避免画出一个点。
///
/// # Arguments
///
/// - `Vec3` - 三角形第一个顶点。
/// - `Vec3` - 三角形第二个顶点。
/// - `Vec3` - 三角形第三个顶点。
/// - `Vec3` - 相机眼点世界坐标。
///
/// # Returns
///
/// - `bool` - `true` 表示该面应被背面剔除。
pub fn is_back_facing(a: Vec3, b: Vec3, c: Vec3, eye: Vec3) -> bool {
    let edge1: [f32; 3] = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let edge2: [f32; 3] = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let normal: [f32; 3] = [
        edge1[1] * edge2[2] - edge1[2] * edge2[1],
        edge1[2] * edge2[0] - edge1[0] * edge2[2],
        edge1[0] * edge2[1] - edge1[1] * edge2[0],
    ];
    let to_eye: [f32; 3] = [eye[0] - a[0], eye[1] - a[1], eye[2] - a[2]];
    let dot: f32 = normal[0] * to_eye[0] + normal[1] * to_eye[1] + normal[2] * to_eye[2];
    let area2: f32 = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if area2 <= 1e-12 {
        return true;
    }
    dot <= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 把断言文案里的 `{名字}` 占位符替换成实际数值。
    ///
    /// 断言文案按 §1.3c 全部住在 `const.rs`,而 `assert!` 的格式参数必须是
    /// 字面量,所以这里先把文案填好再整体塞进 `"{}"`。
    pub(crate) fn fill(template: &str, args: &[(&str, &str)]) -> String {
        let mut out: String = template.to_string();
        let mut index: usize = 0;
        while index < args.len() {
            let key: &str = args[index].0;
            let value: &str = args[index].1;
            let placeholder: String =
                format!("{open}{open}{key}{close}{close}", open = "{", close = "}");
            out = out.replace(&placeholder, value);
            index += 1;
        }
        out
    }

    /// 游戏第三人称相机真实的垂直视场(弧度);`Camera::new()` 默认是轨道相机的 45°。
    pub(crate) const FOLLOW_FOV: f32 = 1.309;

    /// 造一个「玩家站在一堵墙正前方」的最小场景。
    ///
    /// 焦点在原点;`yaw = 0` 时眼点落在 -X 方向,所以墙立在 x = -4 m 处。
    pub(crate) fn world_with_wall() -> CollisionWorld {
        let mut world: CollisionWorld = CollisionWorld::new();
        world.push_aabb([-4.0, 0.0], [1.0, 6.0]);
        world
    }

    pub(crate) fn empty_interiors() -> FloorWorld {
        FloorWorld::new()
    }

    pub(crate) fn follow_camera() -> Camera {
        let mut camera: Camera = Camera::new();
        camera.set_target([0.0, 1.45, 0.0]);
        camera.set_pitch(0.0);
        camera.set_yaw(0.0);
        camera.set_fov_y(FOLLOW_FOV);
        camera.set_distance(9.5);
        camera.set_desired_distance(9.5);
        camera
    }

    pub(crate) fn interiors_with_wall() -> FloorWorld {
        let mut world: FloorWorld = FloorWorld::new();
        world.push_wall([-4.25, 0.0, -6.0], [-3.75, 6.4, 6.0]);
        world
    }

    #[test]
    pub(crate) fn interior_wall_pulls_the_camera_in() {
        // 眼点会落在 -X 一侧(见 `world_with_wall` 的说明),所以墙立在
        // x = -4 m。外部碰撞世界里**没有**这堵墙,只有室内碰撞世界有。
        let camera: Camera = follow_camera();
        let empty: FloorWorld = empty_interiors();
        let outside: Option<f32> =
            camera.resolve_occlusion_interior(&CollisionWorld::new(), &empty, 9.5);
        assert!(outside.is_none(), "{}", T_NO_OCCLUSION);
        let hit: Option<f32> =
            camera.resolve_occlusion_interior(&CollisionWorld::new(), &interiors_with_wall(), 7.4);
        let allowed: f32 = hit.unwrap_or(7.4);
        assert!(
            allowed < 7.4,
            "{}",
            fill(T_OCCLUSION_SHORTER, &[("allowed", &format!("{allowed}"))])
        );
        assert!(
            allowed > 0.0,
            "{}",
            fill(T_OCCLUSION_POSITIVE, &[("allowed", &format!("{allowed}"))])
        );
    }

    #[test]
    pub(crate) fn empty_interiors_never_pull_the_camera_in() {
        let camera: Camera = follow_camera();
        let hit: Option<f32> =
            camera.resolve_occlusion_interior(&CollisionWorld::new(), &empty_interiors(), 7.4);
        assert!(hit.is_none(), "{}", fill(T_NO_OCCLUSION, &[]));
    }

    #[test]
    pub(crate) fn ray_hits_wall_between_focus_and_eye() {
        let world: CollisionWorld = world_with_wall();
        let origin: Vec3 = [0.0, 1.45, 0.0];
        let hit: Option<(f32, Vec2)> = ray_to_shapes(&world, origin, [-1.0, 0.0, 0.0]);
        let (distance, _point): (f32, Vec2) = hit.expect(T_RAY_MUST_HIT);
        // 墙前表面在 x = -3 m,射线从原点出发 -> 约 3 m 再减探针半径。
        assert!(
            (distance - 2.68).abs() < 0.35,
            "{}",
            fill(T_RAY_DISTANCE, &[("distance", &format!("{distance}"))])
        );
    }

    #[test]
    pub(crate) fn occlusion_shortens_camera_distance() {
        let world: CollisionWorld = world_with_wall();
        let mut camera: Camera = Camera::new();
        camera.set_target([0.0, 1.45, 0.0]);
        camera.set_pitch(0.0);
        camera.set_yaw(0.0);
        camera.set_fov_y(FOLLOW_FOV);
        camera.set_distance(9.5);
        camera.set_desired_distance(9.5);

        let allowed: f32 = camera
            .resolve_occlusion(&world, camera.get_desired_distance())
            .expect(T_OCCLUSION_REPORTED);
        assert!(
            allowed < camera.get_desired_distance(),
            "{}",
            fill(T_OCCLUSION_SHORTER, &[("allowed", &format!("{allowed}"))])
        );
        assert!(
            allowed > 0.5,
            "{}",
            fill(T_OCCLUSION_POSITIVE, &[("allowed", &format!("{allowed}"))])
        );
    }

    #[test]
    pub(crate) fn clear_line_of_sight_reports_no_occlusion() {
        let world: CollisionWorld = world_with_wall();
        let mut camera: Camera = Camera::new();
        camera.set_target([0.0, 1.45, 0.0]);
        camera.set_pitch(0.0);
        // 眼点朝 +X(墙的另一侧),中间没有东西。
        camera.set_yaw(PI);
        camera.set_fov_y(FOLLOW_FOV);
        camera.set_distance(9.5);
        camera.set_desired_distance(9.5);

        assert!(
            camera
                .resolve_occlusion(&world, camera.get_desired_distance())
                .is_none(),
            "{}",
            T_NO_OCCLUSION
        );
    }

    #[test]
    pub(crate) fn skin_keeps_camera_off_the_wall() {
        assert!(OCCLUSION_SKIN > 0.0, "{}", T_SKIN_POSITIVE);
        assert!(OCCLUSION_SKIN <= 0.6, "{}", T_SKIN_BOUNDED);
    }

    #[test]
    pub(crate) fn approach_distance_is_smooth_and_respects_floor() {
        let mut camera: Camera = Camera::new();
        camera.set_distance(7.4);
        // 一步 16 ms 拉向 1.0 m,不允许一帧跳完。
        camera.approach_distance(1.0, 22.0, 0.016, 9.0, 2.5);
        let after: f32 = camera.get_distance();
        assert!(
            after < 7.4,
            "{}",
            fill(T_PULL_IN, &[("after", &format!("{after}"))])
        );
        assert!(
            after > 1.0,
            "{}",
            fill(T_PULL_IN_SMOOTH, &[("after", &format!("{after}"))])
        );
    }

    #[test]
    pub(crate) fn approach_distance_never_goes_below_floor() {
        let mut camera: Camera = Camera::new();
        camera.set_distance(1.2);
        for _ in 0..400 {
            camera.approach_distance(0.05, 22.0, 0.016, 30.0, 6.0);
        }
        let distance: f32 = camera.get_distance();
        let floor: f32 = OCCLUSION_MIN_DISTANCE;
        assert!(
            distance >= floor - 1e-3,
            "{}",
            fill(
                T_FLOOR_HELD,
                &[
                    ("distance", &format!("{distance}")),
                    ("floor", &format!("{floor}"))
                ]
            )
        );
    }

    #[test]
    pub(crate) fn approach_distance_recovers_to_desired() {
        let mut camera: Camera = Camera::new();
        camera.set_distance(2.5);
        // 遮挡消失,慢慢回弹到 7.4 m,400 步(约 6.4 s)足够。
        let mut steps: usize = 0;
        while steps < 400 && camera.get_distance() < 7.3 {
            camera.approach_distance(7.4, 22.0, 0.016, 9.0, 2.5);
            steps += 1;
        }
        let distance: f32 = camera.get_distance();
        assert!(
            distance > 7.0,
            "{}",
            fill(T_RECOVERS, &[("distance", &format!("{distance}"))])
        );
    }

    #[test]
    pub(crate) fn side_ray_catches_off_axis_wall() {
        // `yaw = 0` 时中心线沿 -X 走;墙摆在 (-6, -3) —— 侧线在 t ≈ 6 m
        // 处正好经过 (-6, -2.85),中心线却离它 3 m 远。这正是「大片模型
        // 不展示」的真实成因:楼在画面边缘,不在视线正中。
        let mut world: CollisionWorld = CollisionWorld::new();
        world.push_aabb([-6.0, -3.0], [0.6, 1.6]);
        let mut camera: Camera = Camera::new();
        camera.set_target([0.0, 1.45, 0.0]);
        camera.set_pitch(0.0);
        camera.set_yaw(0.0);
        camera.set_fov_y(FOLLOW_FOV);
        camera.set_distance(9.5);
        camera.set_desired_distance(9.5);
        // 中心射线单独打:应当无命中(墙在正侧方)。
        let centre: Option<(f32, Vec2)> =
            ray_to_shapes(&world, camera.get_target(), camera.eye_direction());
        assert!(
            centre.is_none(),
            "{}",
            fill(T_CENTRE_MISSES, &[("centre", &format!("{centre:?}"))])
        );
        // 但整体判定必须认为视线被挡。
        let allowed: Option<f32> = camera.resolve_occlusion(&world, camera.get_desired_distance());
        assert!(allowed.is_some(), "{}", T_SIDE_HITS);
    }
}
