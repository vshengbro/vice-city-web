use super::*;

/// 一点到单个静态形状表面的最短距离(米);点落在形状内部时为 0。
///
/// 公开它是为了让验收探针能回答「**我为什么走不动**」:玩家被挡住时
/// 圆心在碰撞体**外面**(贴着面),`contains_point` 返回 `false`,但本函数
/// 会给出 0.0x m 这样的贴面距离。没有这个量,「站在墙前」和「陷进墙里」
/// 在探针输出里长得一模一样。
///
/// # Arguments
///
/// - `&Shape` - 静态碰撞体.
/// - `Vec2` - 世界 XZ 坐标。
///
/// # Returns
///
/// - `f32` - 最短距离(米)。
pub fn shape_distance(shape: &Shape, point: Vec2) -> f32 {
    match shape {
        Shape::Aabb { center, half } => {
            let dx: f32 = ((point[0] - center[0]).abs() - half[0]).max(0.0);
            let dz: f32 = ((point[1] - center[1]).abs() - half[1]).max(0.0);
            (dx * dx + dz * dz).sqrt()
        }
        Shape::Circle { center, radius } => {
            let dx: f32 = point[0] - center[0];
            let dz: f32 = point[1] - center[1];
            ((dx * dx + dz * dz).sqrt() - *radius).max(0.0)
        }
    }
}

/// 一个点到所有静态形状表面的最短距离(米)。
///
/// # Arguments
///
/// - `&Shape` - 静态碰撞体。
/// - `Vec2` - 世界 XZ 坐标。
///
/// # Returns
///
/// - `f32` - 最短距离(米)。
pub(crate) fn distance_to_shape(shape: &Shape, point: Vec2) -> f32 {
    shape_distance(shape, point)
}

/// 同类**人形**实体之间额外的「个人空间」(米)。
///
/// 人并排走时不会贴着,也不应该被推进另一个人的身体里。车辆之间不给
/// 额外空间 —— 前后车就该紧贴着排队。
///
/// # Arguments
///
/// - `BodyKind` - 第一个实体的类型。
/// - `BodyKind` - 第二个实体的类型。
///
/// # Returns
///
/// - `f32` - 需要额外拉开的距离(米)。
pub fn same_kind_personal_space(a: BodyKind, b: BodyKind) -> f32 {
    if a == b && matches!(a, BodyKind::Pedestrian | BodyKind::Enemy) {
        PEDESTRIAN_PERSONAL_SPACE
    } else {
        0.0
    }
}

/// 从 `origin` 沿单位方向 `dir`(XZ 分量)投射,求撞上第一个碰撞体的距离。
///
/// 用**球体推进**而不是零半径射线:把探针半径当成「相机本体的尺寸」,
/// 于是命中距离等于「中心还能走多远才让球面贴上墙」。掠射角(视线几乎
/// 平行墙面扫过)下零半径射线会给出剧烈跳变的命中距离,球体则平滑得多。
///
/// 建筑 / 长椅这类方块用**膨胀 AABB** 求交(把半径加到半尺寸上),
/// 圆形道具用**圆-圆求交**。两者都是闭式解,没有迭代。
///
/// # Arguments
///
/// - `&CollisionWorld` - 静态碰撞世界的只读引用。
/// - `Vec3` - 射线起点(世界坐标;只用 XZ 分量)。
/// - `Vec3` - 单位方向(世界坐标;只用 XZ 分量)。
///
/// # Returns
///
/// - `Option<(f32, Vec2)>` - `(命中距离, 命中点世界 XZ)`;未命中为 `None`。
pub fn ray_to_shapes(world: &CollisionWorld, origin: Vec3, dir: Vec3) -> Option<(f32, Vec2)> {
    let flat: f32 = (dir[0] * dir[0] + dir[2] * dir[2]).sqrt();
    // 视线完全竖直(pitch 接近 ±90°)时 XZ 投影退化,没有遮挡可言。
    if flat < 1.0e-5 {
        return None;
    }
    let (ux, uz): (f32, f32) = (dir[0] / flat, dir[2] / flat);
    let mut best: Option<(f32, Vec2)> = None;
    for shape in world.get_shapes() {
        let candidate: Option<(f32, Vec2)> = match shape {
            Shape::Aabb { center, half } => ray_into_box(
                [origin[0], origin[2]],
                [ux, uz],
                *center,
                [half[0] + CAMERA_PROBE_RADIUS, half[1] + CAMERA_PROBE_RADIUS],
            ),
            Shape::Circle { center, radius } => ray_into_circle(
                [origin[0], origin[2]],
                [ux, uz],
                *center,
                *radius + CAMERA_PROBE_RADIUS,
            ),
        };
        let Some((distance, at)) = candidate else {
            continue;
        };
        if distance < 0.0 {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(current, _): &(f32, [f32; 2])| distance < *current)
        {
            best = Some((distance, at));
        }
    }
    best.map(|(distance, _): (f32, [f32; 2])| {
        (
            distance * flat,
            [
                origin[0] + ux * distance * flat,
                origin[2] + uz * distance * flat,
            ],
        )
    })
}

/// 单位方向 `dir` 的射线撞上轴对齐盒(半尺寸已含探针半径)时的参数 t。
///
/// 标准 slab 法:对 X / Z 两轴各求一次进出区间并取交集,交集的近端就是
/// 命中参数。起点在盒内时近端为 0(调用方按「已经贴住」处理)。
///
/// # Arguments
///
/// - `Vec2` - 射线起点 XZ。
/// - `Vec2` - XZ 平面上的单位方向。
/// - `Vec2` - 盒中心 XZ。
/// - `Vec2` - 盒半尺寸 XZ(已含探针半径)。
///
/// # Returns
///
/// - `Option<(f32, Vec2)>` - `(参数 t, 命中点 XZ)`;未命中为 `None`。
pub(crate) fn ray_into_box(
    origin: Vec2,
    dir: Vec2,
    center: Vec2,
    half: Vec2,
) -> Option<(f32, Vec2)> {
    let mut near: f32 = f32::NEG_INFINITY;
    let mut far: f32 = f32::INFINITY;
    for axis in 0..2 {
        // 方向分量接近 0 时该轴不构成约束:只有起点已经在板内才算命中。
        if dir[axis].abs() < 1.0e-6 {
            if (origin[axis] - center[axis]).abs() > half[axis] {
                return None;
            }
            continue;
        }
        let lo: f32 = (center[axis] - half[axis] - origin[axis]) / dir[axis];
        let hi: f32 = (center[axis] + half[axis] - origin[axis]) / dir[axis];
        let (enter, exit): (f32, f32) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        near = near.max(enter);
        far = far.min(exit);
        if near > far {
            return None;
        }
    }
    // 盒在射线**背后**时 `far` 为负:射线朝反方向走,永远不会撞上。
    // 少了这一句,任何背对建筑的相机都会拿到一个 `t = 0` 的假命中,
    // 回避逻辑随即把距离扣成负数(`res=-0.45`),相机被推到玩家背后。
    if far < 0.0 {
        return None;
    }
    let t: f32 = near.max(0.0);
    Some((t, [origin[0] + dir[0] * t, origin[1] + dir[1] * t]))
}

/// 单位方向 `dir` 的射线撞上圆柱(半径已含探针半径)时的参数 t。
///
/// 圆-圆求交:解 `|o + t·d - c|² = r²` 的二次方程取最小非负根。判别式
/// 小于 0 表示射线从旁边擦过,不相交。
///
/// # Arguments
///
/// - `Vec2` - 射线起点 XZ。
/// - `Vec2` - XZ 平面上的单位方向。
/// - `Vec2` - 圆柱中心 XZ。
/// - `f32` - 圆柱半径(已含探针半径)。
///
/// # Returns
///
/// - `Option<(f32, Vec2)>` - `(参数 t, 命中点 XZ)`;未命中为 `None`。
pub(crate) fn ray_into_circle(
    origin: Vec2,
    dir: Vec2,
    center: Vec2,
    radius: f32,
) -> Option<(f32, Vec2)> {
    let (ox, oz): (f32, f32) = (origin[0] - center[0], origin[1] - center[1]);
    let b: f32 = 2.0 * (ox * dir[0] + oz * dir[1]);
    let c: f32 = ox * ox + oz * oz - radius * radius;
    let discriminant: f32 = b * b - 4.0 * c;
    if discriminant < 0.0 {
        return None;
    }
    let root: f32 = discriminant.sqrt();
    let near: f32 = (-b - root) * 0.5;
    let far: f32 = (-b + root) * 0.5;
    // 起点在圆柱内:远端才是「穿出去」的那一侧,近端为负没有意义。
    let t: f32 = if near >= 0.0 { near } else { far };
    if t < 0.0 {
        return None;
    }
    Some((t, [origin[0] + dir[0] * t, origin[1] + dir[1] * t]))
}

/// 车身(有向盒)中心在一条任意方向轴上的投影半径。
///
/// # Arguments
///
/// - `Vec2` - 投影轴(无需归一化,结果按轴长线性缩放)。
/// - `Vec2` - 车头方向单位向量。
/// - `Vec2` - 车身侧向单位向量。
/// - `Vec2` - 车身半长 / 半宽(米)。
///
/// # Returns
///
/// - `f32` - 车心到该轴的投影半径(米)。
///
/// **必须取两段的绝对值之和。** 带符号直接相加会在 yaw 接近 45° 时互相
/// 抵消成 0,车就被当成一个点,什么墙都撞不上。
pub(crate) fn car_projection(axis: Vec2, forward: Vec2, side: Vec2, footprint: Vec2) -> f32 {
    let along: f32 = footprint[0] * (forward[0] * axis[0] + forward[1] * axis[1]).abs();
    let across: f32 = footprint[1] * (side[0] * axis[0] + side[1] * axis[1]).abs();
    along + across
}

/// 有向盒(车)vs 轴对齐盒(建筑 / 道具)的分离量。
///
/// **为什么要一个专门的函数,而不是把圆放大:** 圆对所有方向一视同仁,而车
/// 是长条形的 —— 同一辆车车头要离墙 `half_long`,车身侧面只要离墙
/// `half_wide`。用圆去近似,两个方向只能取同一个值,取小了车头扎进墙
/// (实测 1.033 m),取大了侧面凭空多出半米空隙、车在窄巷里剐蹭。两者在
/// 二维里等价于:车在盒的**局部坐标**里做一次「圆心到矩形」的距离测试。
///
/// 算法走标准 SAT(分离轴定理):两个凸多边形分离 ⟺ 存在一条轴使两个投影
/// 不相交。盒只有 3 条候选轴 —— 盒自身的两条 + 车的两条车身轴。取**穿透
/// 最浅**的那条轴把车推出去,所以车会被沿最省力的方向滑开,而不是被卡在
/// 墙角。这与玩家用的 `resolve_slide` 是同一套思路。
///
/// # Arguments
///
/// - `Vec2` - 静态盒中心的世界 XZ 坐标。
/// - `Vec2` - 静态盒的半尺寸(米)。
/// - `Vec2` - 车心的世界 XZ 坐标。
/// - `Vec2` - 车身前向单位向量(世界 XZ)。
/// - `Vec2` - 车身左向单位向量(世界 XZ)。
/// - `Vec2` - 车的半长 / 半宽(米)。
///
/// # Returns
///
/// - `Option<(Vec2, f32)>` - `(推出方向, 推出距离)`;不重叠时为 `None`。
pub(crate) fn push_out_box_aabb(
    center: Vec2,
    half: Vec2,
    point: Vec2,
    forward: Vec2,
    side: Vec2,
    footprint: Vec2,
) -> Option<(Vec2, f32)> {
    let offset: Vec2 = [point[0] - center[0], point[1] - center[1]];
    // 四条候选轴:车身两条 + 世界 X / Z 两条。`gap` > 0 表示该轴上两者
    // 已经分离 —— 只要有一条如此,按 SAT 这两个凸多边形就不相交。
    let axes: [Vec2; 4] = [forward, side, [1.0, 0.0], [0.0, 1.0]];
    // 车心在一条**任意方向**轴上的投影半径 = 车身矩形在轴上的半长投影。
    // 两端点各投一次,取两段**绝对值之和** —— 带符号相加会互相抵消,
    // yaw 接近 45° 时算出 0,车就被当成一个点。
    let mut separated: bool = false;
    let mut best: Option<(Vec2, f32)> = None;
    for (index, axis) in axes.into_iter().enumerate() {
        // 世界轴(后两条)的静态半径就是盒自己的半长;车身轴要投一次。
        let box_radius: f32 = match index {
            0 | 1 => half[0] * axis[0].abs() + half[1] * axis[1].abs(),
            2 => half[0],
            _ => half[1],
        };
        let gap: f32 = (offset[0] * axis[0] + offset[1] * axis[1]).abs()
            - box_radius
            - car_projection(axis, forward, side, footprint);
        if gap > 0.0 {
            separated = true;
            break;
        }
        // 选**最浅**的那条轴:最浅 = 沿它分离最省力,车于是被推去最
        // 容易脱身的方向,而不是被卡在墙角。
        let depth: f32 = -gap;
        if best
            .as_ref()
            .is_none_or(|(_axis, kept): &(Vec2, f32)| depth < *kept)
        {
            best = Some((axis, depth));
        }
    }
    if separated {
        return None;
    }
    let (mut axis, depth) = best?;
    // 推出方向必须**指向盒的外侧**。偏移量为 0(车心与盒心重合)时没有
    // 符号可言,取轴的第一非零分量的正负定一个确定的方向。
    let projection: f32 = offset[0] * axis[0] + offset[1] * axis[1];
    let sign: f32 = if projection > 0.0 {
        1.0
    } else if projection < 0.0 {
        -1.0
    } else if axis[0] != 0.0 {
        axis[0].signum()
    } else {
        axis[1].signum()
    };
    axis = [axis[0] * sign, axis[1] * sign];
    Some((axis, depth))
}

/// 车被墙挡住时的切向滑动:保留 `step` 的切向分量。
///
/// # Arguments
///
/// - `Vec2` - 分离之后的位置。
/// - `Vec2` - 这一小步想要的位移。
/// - `Vec2` - 分离之前想去的落点(用来算修正量)。
///
/// # Returns
///
/// - `Vec2` - 沿墙滑过之后的位置。
pub(crate) fn car_slide(pushed: Vec2, step: Vec2, wanted: Vec2) -> Vec2 {
    let correction: Vec2 = [pushed[0] - wanted[0], pushed[1] - wanted[1]];
    let length: f32 = (correction[0] * correction[0] + correction[1] * correction[1]).sqrt();
    if length <= SLIDE_EPSILON {
        return pushed;
    }
    let normal: Vec2 = [correction[0] / length, correction[1] / length];
    let into: f32 = step[0] * normal[0] + step[1] * normal[1];
    if into >= 0.0 {
        return pushed;
    }
    let tangent: Vec2 = [step[0] - normal[0] * into, step[1] - normal[1] * into];
    if (tangent[0] * tangent[0] + tangent[1] * tangent[1]).sqrt() <= SLIDE_EPSILON {
        return pushed;
    }
    [pushed[0] + tangent[0], pushed[1] + tangent[1]]
}

/// 圆形 vs AABB 的分离量。
///
/// # Arguments
///
/// - `Vec2` - AABB 中心的世界 XZ 坐标。
/// - `Vec2` - AABB 半尺寸(米)。
/// - `Vec2` - 圆心的世界 XZ 坐标。
/// - `f32` - 圆半径(米)。
///
/// # Returns
///
/// - `Option<(Vec2, f32)>` - `(推出方向, 推出距离)`;不重叠时为 `None`。
pub(crate) fn push_out_aabb(
    center: Vec2,
    half: Vec2,
    point: Vec2,
    radius: f32,
) -> Option<(Vec2, f32)> {
    let closest: Vec2 = [
        point[0].clamp(center[0] - half[0], center[0] + half[0]),
        point[1].clamp(center[1] - half[1], center[1] + half[1]),
    ];
    let delta: Vec2 = [point[0] - closest[0], point[1] - closest[1]];
    let distance: f32 = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
    if distance > radius {
        return None;
    }
    if distance > INSIDE_EPSILON {
        return Some((
            [delta[0] / distance, delta[1] / distance],
            radius - distance,
        ));
    }
    // 圆心已经落在盒内:沿「离边界最近」的那个面推出去。
    let offset: Vec2 = [point[0] - center[0], point[1] - center[1]];
    let slack_x: f32 = half[0] - offset[0].abs();
    let slack_z: f32 = half[1] - offset[1].abs();
    if slack_x <= slack_z {
        let sign: f32 = if offset[0] >= 0.0 { 1.0 } else { -1.0 };
        Some(([sign, 0.0], radius + slack_x))
    } else {
        let sign: f32 = if offset[1] >= 0.0 { 1.0 } else { -1.0 };
        Some(([0.0, sign], radius + slack_z))
    }
}

/// 圆形 vs 圆的分离量。
///
/// # Arguments
///
/// - `Vec2` - 圆心 A 的世界 XZ 坐标。
/// - `f32` - 圆 A 的半径(米)。
/// - `Vec2` - 圆心 B(玩家圆心)的世界 XZ 坐标。
/// - `f32` - 圆 B 的半径(米)。
///
/// # Returns
///
/// - `Option<(Vec2, f32)>` - `(推出方向, 推出距离)`;不重叠时为 `None`。
pub(crate) fn push_out_circle(
    center: Vec2,
    other: f32,
    point: Vec2,
    radius: f32,
) -> Option<(Vec2, f32)> {
    let delta: Vec2 = [point[0] - center[0], point[1] - center[1]];
    let distance: f32 = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
    let reach: f32 = other + radius;
    if distance > reach {
        return None;
    }
    if distance > INSIDE_EPSILON {
        return Some(([delta[0] / distance, delta[1] / distance], reach - distance));
    }
    Some(([1.0, 0.0], reach))
}

/// 由资产 `bounds` 推导一个摆放实例在世界 XZ 平面上的 AABB。
///
/// 资产的 `bounds` 是**本地** Y-up 包围盒:建筑 / 车辆这类资产的长度在
/// 本地 X 上、宽度在本地 Z 上。摆放时绕 Y 轴旋转 `yaw` 并统一缩放
/// `scale`,所以：
///
/// 1. 本地足迹中心 = `((min.x + max.x) / 2, (min.z + max.z) / 2)` —— 资产
///    不保证足迹相对原点居中(例如 `bldg_deco_pink` 的 z 从 -5.08 到
///    6.49),不补这一步墙就会整体偏移半个身位。
/// 2. 本地足迹半尺寸 = `((max.x - min.x) / 2, (max.z - min.z) / 2)`。
/// 3. 用与 `Instance::new` 相同的旋转约定(本地 +X → 世界
///    `(cos yaw, 0, -sin yaw)`,本地 +Z → `(sin yaw, 0, cos yaw)`)把
///    中心偏移旋到世界,半尺寸取旋转后外接盒(yaw 为 90° 整数倍时是精确值)。
///
/// # Arguments
///
/// - `Vec3` - 资产 `bounds.min`(本地坐标)。
/// - `Vec3` - 资产 `bounds.max`(本地坐标)。
/// - `f32` - 绕 Y 轴的摆放朝向(弧度)。
/// - `f32` - 统一缩放系数。
/// - `Vec3` - 摆放位置(世界坐标)。
///
/// # Returns
///
/// - `(Vec2, Vec2)` - `(世界 XZ 盒中心, 世界 XZ 盒半尺寸)`。
pub fn placement_box(min: Vec3, max: Vec3, yaw: f32, scale: f32, position: Vec3) -> (Vec2, Vec2) {
    let (sin_yaw, cos_yaw): (f32, f32) = yaw.sin_cos();
    let local_center: Vec2 = [(min[0] + max[0]) * 0.5, (min[2] + max[2]) * 0.5];
    let local_half: Vec2 = [(max[0] - min[0]) * 0.5, (max[2] - min[2]) * 0.5];
    let center: Vec2 = [
        position[0] + (cos_yaw * local_center[0] + sin_yaw * local_center[1]) * scale,
        position[2] + (-sin_yaw * local_center[0] + cos_yaw * local_center[1]) * scale,
    ];
    let half: Vec2 = [
        (cos_yaw.abs() * local_half[0] + sin_yaw.abs() * local_half[1]) * scale,
        (sin_yaw.abs() * local_half[0] + cos_yaw.abs() * local_half[1]) * scale,
    ];
    (center, half)
}

/// 世界坐标 → 网格单元下标(floor 闭区间映射)。
///
/// # Arguments
///
/// - `f32` - 单轴世界坐标(米)。
///
/// # Returns
///
/// - `i32` - 该坐标所在的 [`GRID_CELL`] 单元下标。
pub(crate) fn grid_cell(v: f32) -> i32 {
    (v / GRID_CELL).floor() as i32
}

/// 形状的轴对齐包围盒(圆取外接方)。
///
/// # Arguments
///
/// - `&Shape` - 静态碰撞形状(盒或圆)。
///
/// # Returns
///
/// - `(Vec2, Vec2)` - (包围盒下角, 包围盒上角)。
pub(crate) fn shape_aabb(shape: &Shape) -> (Vec2, Vec2) {
    match shape {
        Shape::Aabb { center, half } => (
            [center[0] - half[0], center[1] - half[1]],
            [center[0] + half[0], center[1] + half[1]],
        ),
        Shape::Circle { center, radius } => (
            [center[0] - radius, center[1] - radius],
            [center[0] + radius, center[1] + radius],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 回归测试:车的碰撞足迹必须是真实车身尺寸,而不是 1.25 m 的圆。
    ///
    /// 圆只到 1.25 m 而车头在 2.283 m,车头就有 1.033 m 悬在碰撞边界之外。
    pub(crate) const T_CAR_FOOTPRINT_NOT_A_DISC: &str =
        "车的碰撞足迹必须按真实车身算,而不是 1.25 m 的圆";

    /// 回归测试:车开到建筑 / 围墙里必须被挡住,车头不许插进去。
    pub(crate) const T_CAR_NEVER_INSIDE_A_WALL: &str = "车身不得插进建筑围墙";

    /// 回归测试:一帧的位移不得跨过最薄的墙(高速穿墙)。
    pub(crate) const T_CAR_NO_WALL_TUNNEL: &str = "满油门一帧不得穿过薄墙";

    /// 回归测试:斜着撞墙必须沿墙滑过去,而不是被弹到墙的另一侧。
    pub(crate) const T_CAR_SLIDES_ALONG_WALL: &str = "斜着撞墙必须沿墙滑过去";

    /// 测试里代表「车头正对 −X 墙面」的朝向。
    ///
    /// **朝向由共享约定 [`crate::orient::yaw_forward`] 决定**:车头在资产
    /// 本地 +Z、渲染成 `(sin yaw, cos yaw)`,所以「车头朝 −X」是
    /// `atan2(-1, 0) = -π/2`,不是 π。旧值 π 在旧约定(`[cos yaw, -sin yaw]`)
    /// 下确实朝 −X,但那个前向与渲染出来的车头差 90° —— 这几个测试正是
    /// 在那个错误约定下写的,所以换约定后必须跟着换成同一个「车头」定义。
    ///
    /// 直接取模块里的 `FRAC_PI_2`(§6.1 要求子模块里不出现限定 std 路径),
    /// 不要写 `-1.5707963` 这种字面值:那是 `clippy::approx_constant`
    /// 认得的近似值,会直接报错。
    pub(crate) const YAW_FACING_NEG_X: f32 = -FRAC_PI_2;

    /// 测试里代表 45° 斜向位移的分解系数(1.6 m / √2 ≈ 1.1313708)。
    pub(crate) const STRIDE_DIAGONAL: f32 = 1.131_370_8;

    /// 泡测试的朝向步长:15°(π / 12 ≈ 0.2617994)。
    pub(crate) const YAW_STEP_RAD: f32 = 0.261_799_4;

    /// 墙占 x ∈ [-10, 0](盒心 -5、半尺寸 5),内表面在 x = 0,墙体在 -X 侧。
    ///
    /// 玩家从 **+X** 一侧靠近,圆心恰好贴在面上时 x = 半径 = `REST_X`。
    pub(crate) const REST_X: f32 = 0.35;
    pub(crate) const RADIUS: f32 = 0.35;

    pub(crate) fn wall_world() -> CollisionWorld {
        let mut world: CollisionWorld = CollisionWorld::new();
        world.set_player_radius(RADIUS);
        world.push_aabb([-5.0, 0.0], [5.0, 10.0]);
        world
    }

    /// 贴着墙斜着走:法向被墙挡住,**切向必须保住**。
    ///
    /// 这是「有速度、无位移」(`want (-1, 0) got (0, 0)`)的回归测试:
    /// 旧的 `resolve` 是全有全无钳制,0.30 m 的切向会被一起清零。
    #[test]
    pub(crate) fn sliding_keeps_the_tangential_component() {
        let world: CollisionWorld = wall_world();
        let here: Vec2 = [REST_X, 0.0];
        // 往 -X 顶进墙里,同时往 +Z 走。
        let delta: Vec2 = [-0.20, 0.30];
        let at: Vec2 = world.resolve_slide(here, delta, world.get_player_radius());
        assert!(
            (at[1] - 0.30).abs() < 1e-2,
            "{} 切向 z 只走了 {} m,应保留 0.30 m",
            T_COLLISION_SLIDE_KEEPS_TANGENT,
            at[1]
        );
        assert!(
            at[0] >= REST_X - 1e-2,
            "{} x={} 穿过了墙(应 >= {REST_X})",
            T_COLLISION_SLIDE_STOPS_AT_WALL,
            at[0]
        );
    }

    /// 正面顶墙仍然必须停住,不能穿墙 —— 滑动不能削弱阻挡。
    #[test]
    pub(crate) fn sliding_still_stops_head_on_at_a_wall() {
        let world: CollisionWorld = wall_world();
        let here: Vec2 = [REST_X, 0.0];
        let delta: Vec2 = [-0.50, 0.0];
        let at: Vec2 = world.resolve_slide(here, delta, world.get_player_radius());
        assert!(
            at[0] >= REST_X - 1e-2,
            "{} 正面顶墙后 x={},穿过了墙(应 >= {REST_X})",
            T_COLLISION_SLIDE_STOPS_AT_WALL,
            at[0]
        );
        assert!(
            (at[0] - REST_X).abs() < 1e-2,
            "{} 正面顶墙后 x={},应贴住 {REST_X}",
            T_COLLISION_SLIDE_STOPS_AT_WALL,
            at[0]
        );
    }

    /// 空地上滑动必须等于原位移(不得引入任何偏移)。
    #[test]
    pub(crate) fn sliding_on_open_ground_is_the_full_delta() {
        let mut world: CollisionWorld = CollisionWorld::new();
        world.set_player_radius(RADIUS);
        let here: Vec2 = [10.0, 20.0];
        let delta: Vec2 = [0.15, -0.25];
        let at: Vec2 = world.resolve_slide(here, delta, world.get_player_radius());
        assert!(
            (at[0] - (here[0] + delta[0])).abs() < 1e-5,
            "{}",
            T_COLLISION_SLIDE_OPEN_GROUND_X
        );
        assert!(
            (at[1] - (here[1] + delta[1])).abs() < 1e-5,
            "{}",
            T_COLLISION_SLIDE_OPEN_GROUND_Z
        );
    }

    /// 顶着墙连续走很多帧:位置必须稳定,绝不能被送到墙的另一侧。
    #[test]
    pub(crate) fn repeated_walking_into_a_wall_never_tunnels_through_it() {
        let world: CollisionWorld = wall_world();
        let mut at: Vec2 = [REST_X, 0.0];
        // 一帧 0.077 m(4.6 m/s × 1/60 s),共 300 帧 ≈ 5 s。
        for _ in 0..300 {
            at = world.resolve_slide(at, [-0.077, 0.0], world.get_player_radius());
            assert!(
                at[0] >= REST_X - 1e-2,
                "{} 顶着墙走了几帧后 x={},穿墙了(应 >= {REST_X})",
                T_COLLISION_SLIDE_STOPS_AT_WALL,
                at[0]
            );
        }
    }

    /// 软边界在界内时必须**完全**不干预 —— 否则玩家在正常街区里就
    /// 会被减速。
    #[test]
    pub(crate) fn soft_limit_does_not_touch_anyone_inside_the_boundary() {
        let world: CollisionWorld = CollisionWorld::new();
        let velocity: Vec2 = [4.6, -3.2];
        let limits: Vec2 = world.get_half_extent();
        for at in [[0.0, 0.0], [10.0, -20.0], [limits[0], limits[1]]] as [Vec2; 3] {
            let kept: Vec2 = world.soft_limit(velocity, at, limits, 24.0);
            assert!(
                (kept[0] - velocity[0]).abs() < 1.0e-6 && (kept[1] - velocity[1]).abs() < 1.0e-6,
                "{T_SOFT_LIMIT_SPARE_INSIDE}"
            );
        }
    }

    /// 越界时速度必须按比例收缩 —— 这是「软」的核心:不是墙,是减速带。
    #[test]
    pub(crate) fn soft_limit_shrinks_speed_more_the_further_out_you_go() {
        let world: CollisionWorld = CollisionWorld::new();
        let limits: Vec2 = world.get_half_extent();
        let slack: f32 = 24.0;
        // 探针只带 **Z** 分量:越界是沿 Z 发生的,X 轴不该被削。
        let probe: Vec2 = [0.0, 1.0];
        let inside: Vec2 = world.soft_limit(probe, [0.0, 0.0], limits, slack);
        let edge: f32 = limits[1];
        // 刚好越过软边界一点点 / 越过一半余量 / 越过一个完整余量。
        let middle: Vec2 = world.soft_limit(probe, [0.0, edge + 1.0], limits, slack);
        let deep: Vec2 = world.soft_limit(probe, [0.0, edge + slack * 0.5], limits, slack);
        let far: Vec2 = world.soft_limit(probe, [0.0, edge + slack], limits, slack);
        assert!((inside[1] - 1.0).abs() < 1.0e-6, "{T_SOFT_LIMIT_RAMP}");
        assert!((middle[0] - probe[0]).abs() < 1.0e-6, "{T_SOFT_LIMIT_RAMP}");
        assert!(middle[1] < inside[1], "{T_SOFT_LIMIT_RAMP}");
        assert!(deep[1] < middle[1], "{T_SOFT_LIMIT_RAMP}");
        assert!(far[1] <= 1.0e-6, "{T_SOFT_LIMIT_STOPS_AT_VOID}");
    }

    /// 越界一个余量以上时速度**归零** —— 玩家到不了那里,于是不会掉进
    /// 「还没生成出来的虚空」。这是「无限世界」能成立的前提。
    #[test]
    pub(crate) fn soft_limit_stops_the_player_before_the_unstreamed_void() {
        let world: CollisionWorld = CollisionWorld::new();
        let limits: Vec2 = world.get_half_extent();
        let push: Vec2 = world.soft_push([0.0, 400.0], limits, 24.0, 1.6, 9.0);
        assert!(push[1] < 0.0, "{T_SOFT_LIMIT_STOPS_AT_VOID}");
        assert!(push[1].abs() <= 9.0, "{T_SOFT_LIMIT_STOPS_AT_VOID}");
    }

    /// 界内不得有任何回推 —— 靠这个保证玩家不会被「温柔地拽走」。
    #[test]
    pub(crate) fn soft_push_is_zero_inside_the_boundary() {
        let world: CollisionWorld = CollisionWorld::new();
        let limits: Vec2 = world.get_half_extent();
        // 「界内」= 半径 + 余量之内,而不是 `limits` 之内 —— 后者还要再加
        // 一个余量的缓冲。
        let slack: f32 = 24.0;
        for at in [
            [0.0, 0.0],
            [limits[0], limits[1]],
            [limits[0] + slack - 1.0, limits[1] + slack - 1.0],
        ] as [Vec2; 3]
        {
            let push: Vec2 = world.soft_push(at, limits, slack, 1.6, 9.0);
            assert!(
                push[0].abs() < 1.0e-6 && push[1].abs() < 1.0e-6,
                "{T_SOFT_PUSH_ZERO_INSIDE}"
            );
        }
    }

    /// **这是「无限世界」的核心回归测试**:分离不得再做位置钳制。
    ///
    /// 旧实现最后一步是 `clamp(-150, 150)`,于是玩家走到边界处位移恒为
    /// 零 —— 有速度、无位移,几十帧推不动一毫米(用户报的空气墙)。
    #[test]
    pub(crate) fn separation_no_longer_clamps_the_player_inside_the_world() {
        let mut world: CollisionWorld = CollisionWorld::new();
        world.set_player_radius(RADIUS);
        // 空碰撞世界,让分离完全不介入。
        let far: Vec2 = [900.0, -900.0];
        let resolved: Vec2 = world.resolve(far);
        assert!(
            (resolved[0] - far[0]).abs() < 1.0e-6 && (resolved[1] - far[1]).abs() < 1.0e-6,
            "{T_NO_POSITION_CLAMP}"
        );
        let slid: Vec2 = world.resolve_slide(far, [3.0, 3.0], RADIUS);
        assert!(slid[0] > far[0] + 2.0, "{T_NO_POSITION_CLAMP}");
    }

    /// 人撞静止的车:人必须被弹开,车几乎不动 —— 这就是质量分离。
    #[test]
    pub(crate) fn a_person_bounces_off_a_car_but_the_car_barely_moves() {
        let world: CollisionWorld = CollisionWorld::new();
        // 人与车重叠 0.4 m。
        let overlap: f32 = 0.4;
        let person_radius: f32 = 0.35;
        let car_radius: f32 = 1.25;
        let gap: f32 = person_radius + car_radius - overlap;
        let bodies: [DynamicBody; 2] = [
            DynamicBody::new(BodyKind::Player, [0.0, 0.0], person_radius, 1.0),
            DynamicBody::new(BodyKind::Car, [gap, 0.0], car_radius, 30.0),
        ];
        let out: Vec<Vec2> = world.resolve_dynamic(&bodies);
        let person_moved: f32 = (out[0][0] - 0.0).abs();
        let car_moved: f32 = (out[1][0] - gap).abs();
        assert!(
            person_moved > 0.3,
            "{}: 人应被弹开,实得 {person_moved} m",
            T_DYNAMIC_MASS_WEIGHTED
        );
        assert!(
            car_moved < person_moved * 0.1,
            "{}: 车几乎不该动(人 {person_moved} / 车 {car_moved})",
            T_DYNAMIC_MASS_WEIGHTED
        );
    }

    /// 两车对撞:质量相同,各退一半。
    #[test]
    pub(crate) fn two_equal_cars_split_the_separation_evenly() {
        let world: CollisionWorld = CollisionWorld::new();
        let radius: f32 = 1.25;
        let gap: f32 = radius * 2.0 - 0.4;
        let bodies: [DynamicBody; 2] = [
            DynamicBody::new(BodyKind::Car, [0.0, 0.0], radius, 10.0),
            DynamicBody::new(BodyKind::Car, [gap, 0.0], radius, 10.0),
        ];
        let out: Vec<Vec2> = world.resolve_dynamic(&bodies);
        // 0 号在左、1 号在右,`normal` 指向 `+X`,所以 0 号退向 −X
        // (位移为负)、1 号退向 +X(位移为正)。用带符号的位移量。
        let left: f32 = -out[0][0];
        let right: f32 = out[1][0] - gap;
        assert!(
            (left - right).abs() < 1.0e-4,
            "{}: 等质量应各退一半,实得 {left} / {right}",
            T_DYNAMIC_MASS_WEIGHTED
        );
        assert!(
            (left + right - 0.4).abs() < 1.0e-3,
            "{}: 分离总量应等于穿透深度 0.4,实得 {}",
            T_DYNAMIC_MASS_WEIGHTED,
            left + right
        );
    }

    /// 完全不重叠时不得产生任何位移。
    #[test]
    pub(crate) fn separated_bodies_are_left_alone() {
        let world: CollisionWorld = CollisionWorld::new();
        let bodies: [DynamicBody; 2] = [
            DynamicBody::new(BodyKind::Player, [0.0, 0.0], 0.35, 1.0),
            DynamicBody::new(BodyKind::Player, [10.0, 0.0], 0.35, 1.0),
        ];
        let out: Vec<Vec2> = world.resolve_dynamic(&bodies);
        assert!(
            (out[0][0] - 0.0).abs() < 1.0e-6 && (out[1][0] - 10.0).abs() < 1.0e-6,
            "{}: 不重叠就不该动,得到 {out:?}",
            T_DYNAMIC_NO_MOVE_WHEN_CLEAR
        );
    }

    /// 同类实体之间**也要**分开 —— 两个人不能互相穿过。
    #[test]
    pub(crate) fn pedestrians_cannot_walk_through_each_other() {
        let world: CollisionWorld = CollisionWorld::new();
        let bodies: [DynamicBody; 2] = [
            DynamicBody::new(BodyKind::Pedestrian, [0.0, 0.0], 0.35, 1.0),
            DynamicBody::new(BodyKind::Pedestrian, [0.3, 0.0], 0.35, 1.0),
        ];
        let out: Vec<Vec2> = world.resolve_dynamic(&bodies);
        let gap: f32 = (out[0][0] - out[1][0]).abs();
        assert!(
            gap >= 0.7 - 1.0e-3,
            "{}: 两个行人应被分开到直径,实得 {gap} m",
            T_DYNAMIC_SAME_KIND_SEPARATES
        );
    }

    /// 动态层必须也能把人从**静态墙**里推出来(车穿房子)。
    #[test]
    pub(crate) fn dynamic_bodies_are_pushed_out_of_static_shapes_too() {
        let mut world: CollisionWorld = CollisionWorld::new();
        // 墙占 x ∈ [-10, 0],玩家圆心在 x = -0.2(陷进墙里 0.15 m)。
        world.push_aabb([-5.0, 0.0], [5.0, 10.0]);
        let bodies: [DynamicBody; 1] = [DynamicBody::new(BodyKind::Car, [-0.2, 0.0], 1.25, 10.0)];
        let out: Vec<Vec2> = world.resolve_all(&bodies);
        assert!(
            out[0][0] >= 1.25 - 1.0e-3,
            "{}: 车应被推出墙外,实得 x={}",
            T_DYNAMIC_STATIC_TOO,
            out[0][0]
        );
    }

    // ---- 车辆足迹回归(用户报:车穿过建筑围墙)--------------------
    //
    // 墙占 x ∈ [-10, 0]:盒心 (-5, 0)、半长 5,近面在 x = 0。
    /// 一面正对车头的建筑外墙。
    pub(crate) fn building_world() -> CollisionWorld {
        let mut world: CollisionWorld = CollisionWorld::new();
        world.push_aabb([-5.0, 0.0], [5.0, 10.0]);
        world
    }

    /// 车头怼墙:停在墙外的距离必须是**半车长**,不是 1.25 m。
    ///
    /// 这是用户报的那个缺陷本身。旧的 `resolve_car` 用 1.25 m 的圆,
    /// 于是车心停在 `x = 1.25`,而车头在 `1.25 + 2.283 = 3.533` ——
    /// **整整 1.033 m 的车身插在墙里**。
    #[test]
    pub(crate) fn a_car_stops_a_whole_car_length_short_of_a_wall() {
        let world: CollisionWorld = building_world();
        // 车头朝 −X(yaw = π),从 +X 侧一路开到墙前。
        let yaw: f32 = YAW_FACING_NEG_X;
        let resolved: Vec2 = world.resolve_car_footprint([20.0, 0.0], [-30.0, 0.0], yaw);
        // 车心离墙面至少要有半车长,车才整个儿在外面。
        assert!(
            resolved[0] >= CAR_FOOTPRINT[0] - 1.0e-2,
            "{}: 车心应停在离墙 {:.3} m 处(半车长),实得 x={}",
            T_CAR_FOOTPRINT_NOT_A_DISC,
            CAR_FOOTPRINT[0],
            resolved[0]
        );
        // 车身最远点(车头)也不许进墙。
        let nose: f32 = resolved[0] - CAR_FOOTPRINT[0];
        assert!(
            nose >= -1.0e-2,
            "{}: 车头扎进墙里 {:.3} m,落点 x={}",
            T_CAR_NEVER_INSIDE_A_WALL,
            -nose,
            resolved[0]
        );
    }

    /// 满油门怼楼:无论开多少帧,车身任何一角都不得进墙。
    ///
    /// 用**车身的四个角**判定而不是车心 —— 车心在墙外而车头插进去,
    /// 正是旧的圆足迹掩盖掉的那种情况(实测车头最深 1.033 m)。
    ///
    /// 容差取 `SKIN = 1e-4 m`(0.1 mm):车贴着墙面时 SAT 推离的浮点残差
    /// 实测在 3e-7 m 量级,那是 `f32` 精度而不是穿透。1.033 m 的缺陷与
    /// 3e-7 m 的噪声相差六个数量级,这条断言不可能被噪声蒙混过去。
    #[test]
    pub(crate) fn a_car_driven_at_full_speed_never_clips_into_a_building() {
        pub(crate) const SKIN: f32 = 1.0e-4;
        let world: CollisionWorld = building_world();
        for yaw_deg in [0.0f32, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0] {
            let yaw: f32 = yaw_deg.to_radians();
            // 用共享约定取前 / 侧基向量,不自己再写一份 —— 测试里重算
            // 公式只能验证自己那份副本,换约定时就会和真实实现错开。
            let forward: Vec2 = crate::orient::yaw_forward(yaw);
            let side: Vec2 = crate::orient::yaw_right(yaw);
            let mut at: Vec2 = [20.0, 0.0];
            // 满油门一帧 1.60 m,共 200 帧,一路撞到底。
            for frame in 0..200 {
                at = world.resolve_car_footprint(at, [forward[0] * 1.6, forward[1] * 1.6], yaw);
                for sx in [-1.0f32, 1.0] {
                    for sz in [-1.0f32, 1.0] {
                        let corner: Vec2 = [
                            at[0]
                                + forward[0] * CAR_FOOTPRINT[0] * sx
                                + side[0] * CAR_FOOTPRINT[1] * sz,
                            at[1]
                                + forward[1] * CAR_FOOTPRINT[0] * sx
                                + side[1] * CAR_FOOTPRINT[1] * sz,
                        ];
                        let inside: bool = corner[0] < -SKIN && corner[1].abs() < 10.0;
                        assert!(
                            !inside,
                            "{}: yaw {yaw_deg}° 第 {frame} 帧车身角 {corner:?} 已在墙内(墙占 x<=0)",
                            T_CAR_NEVER_INSIDE_A_WALL
                        );
                    }
                }
            }
        }
    }

    /// 满油门一帧不得跨过**一堵薄墙**(被实测出来的真实场景)。
    ///
    /// 碰撞世界里最薄的静态盒是 0.133 m(交通锥),一帧 1.60 m = 12 倍。
    /// 旧实现一次 `resolve_car(proposed)` 只看落点:车从墙这侧一步跨到
    /// 墙那侧时落点已经离墙比 1.25 m 还远,`push_out_aabb` 返回 `None`,
    /// 于是既不挡也不推 —— 车就过去了。
    ///
    /// 起点必须让**车头**恰好停在墙的这一侧(还差一丁点没到),这样这一帧
    /// 才会真的跨过整堵墙;若按车心算起点,车头一开始就伸到墙那边去了。
    #[test]
    pub(crate) fn a_full_speed_frame_cannot_tunnel_through_a_thin_wall() {
        pub(crate) const THIN: f32 = 0.133;
        pub(crate) const FRAME: f32 = 1.6;
        let mut world: CollisionWorld = CollisionWorld::new();
        // 0.133 m 厚的薄墙,两面在 x = ∓0.0665;车从 +X 一侧冲 −X。
        world.push_aabb([0.0, 0.0], [THIN * 0.5, 10.0]);
        // 车头正好贴在 +X 面上:车心 = 面 + 半车长。
        let near_face: f32 = THIN * 0.5;
        let start: f32 = near_face + CAR_FOOTPRINT[0];
        // 不切子步的话这一帧落点在 start - 1.6,早已越到墙的另一侧。
        let resolved: Vec2 =
            world.resolve_car_footprint([start, 0.0], [-FRAME, 0.0], YAW_FACING_NEG_X);
        assert!(
            resolved[0] >= -CAR_FOOTPRINT[0] - 1.0e-3,
            "{}: 起点 x={start:.3}、一帧想走 {FRAME} m(墙只有 {THIN} m 厚),车却到了 x={:.3} —— 已在墙的另一侧",
            T_CAR_NO_WALL_TUNNEL,
            resolved[0]
        );
        // 车头不许越过墙的远面(x = −THIN/2)。
        let nose: f32 = resolved[0] - CAR_FOOTPRINT[0];
        assert!(
            nose >= -near_face - 1.0e-3,
            "{}: 车头 x={nose:.4} 已越过墙的远面 {near_face:.4}",
            T_CAR_NO_WALL_TUNNEL
        );
    }

    /// 斜着撞**薄墙**:必须**沿墙滑过去**,而不是穿墙。
    ///
    /// 墙厚 0.20 m —— 窄巷隔断的量级。这一条测的是**薄墙**上的穿透:
    /// 0.20 m 的墙配上一帧 1.60 m 的位移,旧实现一次
    /// `resolve_car(proposed)` 根本看不到重叠(落点已经在墙那边 1.4 m
    /// 处,比 1.25 m 的圆还远),于是既不挡也不推。判据取「沿墙滑动时
    /// 不得越过墙的远面」,而不是滑行距离 —— 距离多少取决于手感和墙长,
    /// 不是一个能证伪的性质。
    #[test]
    pub(crate) fn a_car_hitting_a_thin_wall_at_an_angle_slides_along_it() {
        pub(crate) const THIN: f32 = 0.20;
        let mut world: CollisionWorld = CollisionWorld::new();
        // 墙占 x ∈ [-THIN, 0]。Z 方向必须**足够长**:车沿墙滑行时一直往
        // +Z 走,墙若在 Z 上也有限长,车滑过端头之后就是真的开到墙外面
        // 去了,再往 -X 走当然畅通无阻 —— 那是正确物理,不是缺陷。
        world.push_aabb([-THIN * 0.5, 0.0], [THIN * 0.5, 400.0]);
        // 一路往 −X 带 +Z 斜着撞(45°),跑满 200 帧(每帧 0.283 m = 1.60/√2,
        // 正好是 `TrafficCar::drive` 满油门斜向走一帧的距离)。
        let stride: f32 = STRIDE_DIAGONAL;
        let mut at: Vec2 = [20.0, 14.0];
        for frame in 0..200 {
            at = world.resolve_car_footprint(at, [-stride, stride], YAW_FACING_NEG_X);
            assert!(
                at[0] >= -THIN - 1.0e-3,
                "{}: 第 {frame} 帧车被弹到了墙的另一侧 x={}(墙只占 x ∈ [{}, 0])",
                T_CAR_SLIDES_ALONG_WALL,
                at[0],
                -THIN
            );
        }
        // 切向必须保住:沿墙滑了一段距离,而不是钉死在接触点上。
        assert!(
            at[1] > 5.0,
            "{}: 切向位移被吃光,实得 z={}(应沿墙滑到 z 明显增大)",
            T_CAR_SLIDES_ALONG_WALL,
            at[1]
        );
        // 车身离墙的距离必须按**半车长**算(yaw = π,车头正对墙面)。
        // 旧的圆足迹停在 x = 1.25 —— 车身侧面离墙 1.033 m,看上去就是
        // 「车飘在墙外一米」;足迹修正后停在 2.283 m。
        assert!(
            (at[0] - CAR_FOOTPRINT[0]).abs() < 1.0e-2,
            "{}: 贴墙时车心离墙面 {:.3} m,应等于半车长 {:.3} m",
            T_CAR_FOOTPRINT_NOT_A_DISC,
            at[0],
            CAR_FOOTPRINT[0]
        );
    }

    /// 把上面几条合起来**泡很久**:各朝向的车满油门怼同一栋楼,跑满 600 帧,
    /// 车身的任何一个角都不许进墙。
    ///
    /// 单帧的测试各自只覆盖一个几何,真正在游戏里出问题的是**连续**状态:
    /// 车贴着墙蹭、车头慢慢压进去又弹出来。这一条按 15° 的朝向步长把整圈
    /// 都过一遍,每一帧都重新解一次,断言取**四个角**的最小间隙。
    #[test]
    pub(crate) fn a_car_soaked_at_every_yaw_never_enters_a_building() {
        pub(crate) const FRAME: f32 = 1.6;
        let mut world: CollisionWorld = CollisionWorld::new();
        // 一面长墙:x ∈ [-12, 0],Z 上足够长,车任何朝向都撞得到它。
        world.push_aabb([-6.0, 0.0], [6.0, 600.0]);
        let mut worst: f32 = f32::MAX;
        for step in 0..24 {
            // `YAW_FACING_NEG_X` 让车头大致对着墙(朝向由
            // `crate::orient::yaw_forward` 定义,不是旧的那套 `[cos, -sin]`)。
            let yaw: f32 = YAW_FACING_NEG_X + step as f32 * YAW_STEP_RAD;
            let forward: Vec2 = crate::orient::yaw_forward(yaw);
            let side: Vec2 = crate::orient::yaw_right(yaw);
            let (dir_x, dir_z): (f32, f32) = (forward[0], forward[1]);
            // 从墙外 30 m 处朝墙开,不给它停在接触点上的机会。
            let mut at: Vec2 = [30.0 + 30.0 * forward[0], 100.0 + 30.0 * forward[1]];
            for frame in 0..600 {
                let step_len: f32 = FRAME;
                at = world.resolve_car_footprint(at, [dir_x * step_len, dir_z * step_len], yaw);
                // 车身四角相对墙的内表面(x = 0)的最小间隙。
                for sign_l in [-1.0f32, 1.0] {
                    for sign_w in [-1.0f32, 1.0] {
                        let corner_x: f32 = at[0]
                            + forward[0] * sign_l * CAR_FOOTPRINT[0]
                            + side[0] * sign_w * CAR_FOOTPRINT[1];
                        let gap: f32 = corner_x;
                        if gap < worst {
                            worst = gap;
                        }
                        assert!(
                            gap >= -1.0e-3,
                            "{}: yaw={:.2} 第 {frame} 帧,车身某角 x={:.4} 已进墙 {:.4} m",
                            T_CAR_NEVER_INSIDE_A_WALL,
                            yaw,
                            gap,
                            -gap
                        );
                    }
                }
            }
        }
        // 泡完之后车确实贴着墙待过(证明断言不是「因为车根本没靠近才过」)。
        assert!(
            worst < 0.5,
            "{}: 600 帧里车身离墙最近 {:.3} m,却从未接近 —— 这条测试没有真的撞到墙",
            T_CAR_NEVER_INSIDE_A_WALL,
            worst
        );
    }

    /// broadphase 网格 ≡ 全表扫描:候选必须是「AABB 与查询盒重叠的形状」
    /// 的**升序全集**,一个不多、一个不少。
    ///
    /// 分离结果是顺序敏感的(每个形状立即推一次),所以网格不只是「不能
    /// 漏」,返回顺序也必须是全表扫描的子序列 —— 这条测试用确定性伪随机
    /// 的形状表 + 查询盒把两条性质一起钉死。
    #[test]
    pub(crate) fn grid_candidates_match_linear_scan() {
        // 确定性伪随机(xorshift),不引入 rand 依赖。
        let mut seed: u64 = 0x9E3779B97F4A7C15;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 10000) as f32 / 10000.0
        };
        let mut shapes: Vec<Shape> = Vec::new();
        // 超过 GRID_MIN_SHAPES,网格路径才真正启用。
        for i in 0..200 {
            let at: Vec2 = [next() * 200.0 - 100.0, next() * 200.0 - 100.0];
            if i % 3 == 0 {
                shapes.push(Shape::Circle {
                    center: at,
                    radius: 0.3 + next() * 3.0,
                });
            } else {
                shapes.push(Shape::Aabb {
                    center: at,
                    half: [0.3 + next() * 15.0, 0.3 + next() * 15.0],
                });
            }
        }
        let grid: ShapeGrid = ShapeGrid::build(&shapes);
        for _ in 0..500 {
            let lo: Vec2 = [next() * 220.0 - 110.0, next() * 220.0 - 110.0];
            let hi: Vec2 = [lo[0] + next() * 8.0, lo[1] + next() * 8.0];
            let mut from_grid: Vec<usize> = Vec::new();
            grid.query(lo, hi, |index: usize| from_grid.push(index));
            // broadphase 的不变量(比「逐位等于重叠集」弱,但对分离等价已足够):
            //
            // 1. **不漏**:AABB 与查询盒真重叠的形状必须全部在候选里;
            // 2. **有序**:候选升序(全表扫描的子序列顺序);
            // 3. 允许**多报**(单元粒度保守)—— 多报的形状在分离循环里
            //    走 `push_out_*` 返回 `None`,与全表扫描同样不产生位移,
            //    所以分离结果逐位一致。
            let reference: Vec<usize> = shapes
                .iter()
                .enumerate()
                .filter(|(_, shape): &(usize, &Shape)| {
                    let (slo, shi): (Vec2, Vec2) = shape_aabb(shape);
                    slo[0] <= hi[0] && shi[0] >= lo[0] && slo[1] <= hi[1] && shi[1] >= lo[1]
                })
                .map(|(index, _): (usize, &Shape)| index)
                .collect();
            assert!(
                from_grid.windows(2).all(|w: &[usize]| w[0] < w[1]),
                "候选必须严格升序: {from_grid:?}"
            );
            if let Some(missing) = reference
                .iter()
                .find(|index: &&usize| !from_grid.contains(index))
            {
                panic!("网格漏了真重叠的形状 {missing}: lo={lo:?} hi={hi:?}");
            }
        }
    }
}
