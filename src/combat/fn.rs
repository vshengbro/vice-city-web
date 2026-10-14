use super::*;

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

#[cfg(test)]
mod tests {
    use super::*;
    /// 回归测试(第 10 条):地上的每一种武器模型都必须能被认出来。
    ///
    /// 之前 `Weapon` 没有 `from_asset`,拾取流程根本没法把 `wep_smg`
    /// 这个资产 id 映射到手里的枪 —— 于是武器被拾起来只弹一行提示。
    #[test]
    pub(crate) fn every_pickup_weapon_asset_maps_to_a_weapon() {
        assert_eq!(Weapon::from_asset(WEP_PISTOL), Some(Weapon::Pistol));
        assert_eq!(Weapon::from_asset(WEP_SMG), Some(Weapon::Smg));
        assert_eq!(Weapon::from_asset(WEP_BAT), Some(Weapon::Bat));
        assert_eq!(Weapon::from_asset(WEP_GRENADE), Some(Weapon::Grenade));
    }

    /// 非武器资产(护甲 / 弹药 / 现金)不该被误认成武器。
    #[test]
    pub(crate) fn non_weapon_assets_are_not_weapons() {
        assert_eq!(Weapon::from_asset(PICKUP_ARMOR_VEST), None);
        assert_eq!(Weapon::from_asset(PICKUP_AMMO_BOX), None);
        assert_eq!(Weapon::from_asset(PICKUP_HEALTH_PACK), None);
    }

    /// 每种武器的 `asset()` 必须是 `from_asset` 的逆映射 —— 手持模型
    /// 和地上的模型必须指向同一个资产,否则捡起来手上会变出另一把枪。
    #[test]
    pub(crate) fn weapon_asset_round_trips() {
        for weapon in [Weapon::Pistol, Weapon::Smg, Weapon::Bat, Weapon::Grenade] {
            assert_eq!(Weapon::from_asset(weapon.asset()), Some(weapon));
        }
    }

    /// 徒手不是地上的模型:空串不能被 `from_asset` 反查成徒手。
    ///
    /// 之前 `Weapon` 里根本没有「手上没东西」这一档,按 H 无处可去。
    /// 加了 `Unarmed` 之后,它的 `asset()` 返回空串 —— 如果 `from_asset`
    /// 顺手把空串也认成徒手,任何一次资产查表落空(比如 `index_map`
    /// 少一项)都会把人莫名其妙切成徒手。
    #[test]
    pub(crate) fn unarmed_is_not_a_pickup_asset() {
        assert_eq!(
            Weapon::from_asset(Weapon::Unarmed.asset()),
            None,
            "{}",
            T_UNARMED_NOT_A_PICKUP
        );
        // 空串也不能反查成徒手:任何一次资产查表落空都会把人切成徒手。
        assert_eq!(Weapon::from_asset(""), None, "{}", T_UNARMED_NOT_A_PICKUP);
    }

    /// 徒手没有模型资产:手持批次必须落空成 `usize::MAX`。
    #[test]
    pub(crate) fn unarmed_has_no_held_mesh() {
        assert!(
            Weapon::Unarmed.asset().is_empty(),
            "{}",
            T_UNARMED_HAS_NO_MESH
        );
    }

    /// 徒手永远不能开火 —— 这是 `fire_weapon` 唯一的早退闸门。
    ///
    /// 少了这一条,按 H 收起武器之后点左键会走进 `fire_weapon` 的
    /// hitscan 分支(手上没模型但照样结算),看起来就是「收枪了还在开枪」。
    #[test]
    pub(crate) fn unarmed_can_never_fire() {
        let mut arsenal: Arsenal = Arsenal::new();
        arsenal.set_weapon(Weapon::Unarmed);
        arsenal.set_cooldown(0.0);
        assert_eq!(arsenal.get_magazine(), 0);
        assert!(!arsenal.can_fire(), "{}", T_UNARMED_CANNOT_FIRE);
        assert!(!arsenal.fire(0));
    }

    /// 徒手换不了弹(没有弹匣可换)。
    #[test]
    pub(crate) fn unarmed_cannot_reload() {
        let mut arsenal: Arsenal = Arsenal::new();
        arsenal.set_weapon(Weapon::Unarmed);
        assert!(!arsenal.reload(), "{}", T_UNARMED_NO_RELOAD);
    }

    /// 投掉一颗手雷:计数 -1,投完就不再给投。
    ///
    /// 这条是 G 键的**计数**判据:实测必须能看到 `grenadeTubes` 真的
    /// 往下掉,否则「按 G」只是切了个武器名,玩家手上并没有少东西。
    #[test]
    pub(crate) fn throwing_a_grenade_takes_one_tube() {
        let mut arsenal: Arsenal = Arsenal::new();
        assert!(arsenal.add_grenade());
        assert!(arsenal.add_grenade());
        let before: u32 = arsenal.get_grenade_tubes();
        assert!(arsenal.take_grenade());
        let after: u32 = arsenal.get_grenade_tubes();
        assert_eq!(after, before - 1, "{}", T_GRENADE_COUNT_DOWN);
        // 投到最后一颗为止,之后必须投不出。
        while arsenal.get_grenade_tubes() > 0 {
            assert!(arsenal.take_grenade());
        }
        assert_eq!(arsenal.get_grenade_tubes(), 0);
        assert!(!arsenal.take_grenade(), "{}", T_GRENADE_NONE_LEFT);
    }

    /// 捡手雷要真的 +1,并在 `GRENADE_TUBES` 处封顶。
    #[test]
    pub(crate) fn picking_up_a_grenade_adds_one_tube() {
        let mut arsenal: Arsenal = Arsenal::new();
        assert_eq!(arsenal.get_grenade_tubes(), 0);
        assert!(arsenal.add_grenade());
        assert_eq!(arsenal.get_grenade_tubes(), 1);
        assert!(arsenal.add_grenade());
        assert_eq!(arsenal.get_grenade_tubes(), GRENADE_TUBES);
        // 满了就不再收,但也不能报错。
        assert!(!arsenal.add_grenade());
        assert_eq!(arsenal.get_grenade_tubes(), GRENADE_TUBES);
    }

    /// 切到枪械后它必须**能开火**:`can_fire` 取决于弹匣里有没有子弹,
    /// 所以切枪时必须顺手把弹匣填满,否则玩家拿着一把空枪。
    #[test]
    pub(crate) fn switching_weapons_leaves_it_ready_to_fire() {
        let mut arsenal: Arsenal = Arsenal::new();
        arsenal.set_grenade_tubes(0);
        arsenal.set_weapon(Weapon::Smg);
        arsenal.finish_reload(arsenal.get_weapon().magazine());
        assert_eq!(arsenal.get_magazine(), SMG_MAGAZINE);
        arsenal.set_cooldown(0.0);
        assert!(arsenal.can_fire());
    }
}
