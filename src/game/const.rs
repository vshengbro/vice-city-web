use super::*;

// ===========================================================================
// 常量
//
// 字符串常量统一放在 `crate::r#config`(上面已 glob 引入),
// 这里只保留数值常量。
// ===========================================================================
/// 固定步长:1/60 秒。
pub(crate) const FIXED_DT: f32 = 1.0 / 60.0;

/// 单帧最大累积时间(秒),超过就丢弃(标签页切回来时不追赶)。
pub(crate) const MAX_FRAME_TIME: f32 = 0.25;

/// 远裁剪面距离(米)。
///
/// G-buffer 的线性深度、SSAO 的视空间位置重建、SSR 的射线步进
/// 都以这个值做归一化,所以必须和相机真正使用的远裁剪面一致。
pub(crate) const FAR_PLANE: f32 = 600.0;

/// 滚轮每单位缩放。
pub(crate) const ZOOM_STEP: f32 = 0.0016;

/// 鼠标水平位移转相机偏航的灵敏度(弧度/像素)。
///
/// **符号由实测确定,不是试出来的。** 判据:在真实矩阵下把一对关于
/// 玩家对称的世界点投影到屏幕,量 `ndc_x` 的变化。
///
/// 实测(玩家固定不动,鼠标右移 18 px = +0.108 rad):
/// 对称点 `(-0.0968, +0.0968)` 的 `ndc_x` 同时 **增大** 到
/// `(-0.0369, +0.1571)`(Δ = +0.0599 / +0.0602,两者相等 = 纯旋转)。
/// `ndc_x` 增大 = 场景内容整体**右移**;内容右移 = 相机向左摇
/// (与「相机左摇时,原本在画面中央右侧的物体会被推向更右」一致)。
/// 所以 `dx > 0`(鼠标右移)对应的是视角**左转**,必须取负。
///
/// 取负之后:鼠标右移 → `yaw` 减小 → 视角右转,与玩家直觉一致。
pub(crate) const LOOK_YAW_SENSITIVITY: f32 = -0.006;

/// 鼠标纵向位移转相机俯角的灵敏度(弧度/像素)。
///
/// 屏幕 Y 向下为正,`dy > 0` 是鼠标下移。第三人称下 `pitch` 是相机
/// **俯角**(`camera.rs` 的 `eye()` 用 `+ sin(pitch)·distance` 抬高眼点),
/// 俯角增大 = 相机抬高 = 视线往下压,与玩家「往下看」一致,故取正。
///
/// 实测:向下拖 240 px → `camPitch` 0.16 → 1.05(Δ=+0.89,被
/// `FOLLOW_PITCH_MAX` 截断),方向正确。
pub(crate) const LOOK_PITCH_SENSITIVITY: f32 = 0.005;

/// 下车后玩家与车身保持的距离(米)。
pub(crate) const EXIT_CAR_OFFSET: f32 = 2.4;

/// 所有会被游戏接管的按键。
///
/// 之所以用数组扫描而不是 `matches!`:这些键名都是 `&'static str` 常量,
/// 放进 `matches!` 会被解释成 pattern 绑定(编译报 E0408),不是常量比较。
pub(crate) const TRACKED_KEYS: &[&str] = &[
    KEYW,
    KEYS,
    KEYA,
    KEYD,
    KEYT,
    KEYF,
    KEYTAB,
    // 换弹就是 GTA V 的 R。曾经的 `KEYR` 常量已删 —— 它和 `KEY_RELOAD`
    // 指向同一个 `KeyboardEvent.code`,两个常量只能留一个。
    KEY_RELOAD,
    DIGIT1,
    DIGIT2,
    DIGIT3,
    KEY_MISSION,
    ARROWUP,
    ARROWDOWN,
    ARROWLEFT,
    ARROWRIGHT,
    KEY_SHIFT_LEFT,
    KEY_SHIFT_RIGHT,
    // GTA V 键位(这一组补上的五个)。`KEYTAB` 同时在列:GTA V 的 Tab
    // 是地图,`KEYTAB` 与 `KEY_MAP` 是同一个 `KeyboardEvent.code`。
    KEY_SPACE,
    KEY_GRENADE,
    KEY_UNARMED,
    KEY_CAMERA,
    KEY_MAP,
];

/// 玩家骨架切 part 时的占位错误信息。
/// 第三人称默认俯角(弧度)。
///
/// **必须很小。** 眼点高度 = `FOLLOW_HEIGHT + sin(pitch) · distance`。
/// 旧的 0.22 rad 配 7.4 m 距离把相机顶到 2.96 m,而焦点只有 1.35 m ——
/// 视线俯角约 12.6°,再叠上 30° 的垂直视场,画面下缘几乎全是地面。
/// 站在街上时地面是一整张 `y = 0` 的网格,于是整屏被那片单色地面糊死,
/// 角色和街景都看不见(实测:0.22 时整帧只有一种颜色,0.05 才露出楼房)。
/// 0.05 rad(≈2.9°)让相机只比角色胸口高 0.37 m,取景是标准的
/// 「越过肩膀看前方街道」。
pub(crate) const FOLLOW_PITCH: f32 = 0.16;

/// 第三人称垂直视场角(弧度)。
///
/// 取 75° 而不是 60°:玩家站在 14 m 宽的街道上,两侧的楼离镜头中心
/// 轴约 20 m。60° 垂直视场配 16:9 画幅只有约 34° 的水平张角,半角
/// 17° 在 7.4 m 处只覆盖 ±2.3 m —— 街道两侧的楼、行道树、路灯全部
/// 落在视锥之外,画面里只剩脚下的一小块路面。75° 把水平张角拉到约
/// 50°,半角 25° 覆盖 ±3.5 m,再配合稍近的跟随距离,整条街的立面
/// 才进得了取景框。
pub(crate) const FOLLOW_FOV: f32 = 1.309;

/// 第三人称近裁剪面(米)。
///
/// **不能太小。** 0.05 m 配 420 m 的远平面是 8400:1 的深度比,在没有
/// GPU 的软件光栅(swiftshader)上深度缓冲精度极低,近处几何的深度值
/// 全被压到 0 附近,互相 `LEQUAL` 判等 —— 整帧只剩最后画的那个批次,
/// 画面糊成一片单色(实测:贴地第三人称只有 2~3 种颜色,把 near 提到
/// 12 m 反而"正常"了,因为近处几何直接被切掉、只剩远景不打架)。
/// 相机最近的落点是 2.2 m(遮挡回避硬下限),0.35 m 的近平面已经
/// 足够避开角色自身,又给深度缓冲留出 1200:1 的余量。
pub(crate) const FOLLOW_NEAR: f32 = 0.35;

/// 第三人称远裁剪面(米)。
///
/// 城市是 ±150 m 的网格,站在角落能看到的最远楼角约 420 m,但雾在
/// 120 m 就开始起效,460 m 处完全融进天色 —— 所以 300 m 已经「看不出
/// 被裁掉」,却把深度比从 8400:1 压到 857:1,配合 `FOLLOW_NEAR` 给出
/// 软件光栅也能用的精度。
pub(crate) const FOLLOW_FAR: f32 = 300.0;

/// 第三人称跟随相机的默认距离(米)。
///
/// **取景比例的依据**:垂直视场 `FOLLOW_FOV = 60°`,焦点高度
/// `FOLLOW_HEIGHT = 1.35 m`。角色约 1.8 m 高,在 7.4 m 处的垂直张角
/// 是 `2·atan(0.9 / 7.4) ≈ 13.8°`,占 60° 视场约 **23%** 屏高 —— 角色
/// 清晰可辨,同时周围两条街的立面 / 路缘 / 行道树都还在画面里。
///
/// 之前的 5.6 m 只有 ~17% 屏高,再叠加 `NEAR_CULL_RADIUS = 26 m` 的
/// 近处剔除(相机离角色才 5.6 m,半径 26 m 内的**所有**楼都被剔掉)之后,
/// 画面里只剩地面和一面近处的楼,角色几乎看不见。
pub(crate) const FOLLOW_DISTANCE: f32 = 8.2;

/// 第三人称跟随相机的最小距离(滚轮拉近下限)。
pub(crate) const FOLLOW_DISTANCE_MIN: f32 = 2.2;

/// 第三人称跟随相机的最大距离(滚轮拉远上限)。
pub(crate) const FOLLOW_DISTANCE_MAX: f32 = 22.0;

/// 遮挡回避「拉近」的阻尼速率(1/秒):越大越快缩进来。
///
/// 必须比拉远快一个量级 —— 撞墙时镜头的第一反应就该是「躲」,而不是
/// 用 0.3 s 慢慢推进去展示墙面。
pub(crate) const OCCLUSION_IN_RATE: f32 = 22.0;

/// 遮挡回避「拉远」的阻尼速率(1/秒):离开墙后平滑弹回。
pub(crate) const OCCLUSION_OUT_RATE: f32 = 3.4;

/// 跟随焦点的高度(米)——看胸口而不是看脚。
///
/// 取 1.45 而不是 1.35:角色约 1.8 m 高,焦点略高一点能让头顶留在
/// 取景框内,而不是被画面上缘切掉。
pub(crate) const FOLLOW_HEIGHT: f32 = 1.45;

/// 跟随焦点跟随角色的阻尼速率(1/秒):越大越硬,越小越飘。
pub(crate) const FOLLOW_DAMPING: f32 = 9.0;

/// 玩家碰撞圆半径(米)。
pub(crate) const PLAYER_RADIUS: f32 = 0.35;

/// 世界的**软边界**半径(米):玩家不许走出这个圆。
///
/// 这不是硬墙。城市是**程序化无限生成**的(见
/// `step_streamed_surface` / `rebuild_streamed_surface`),任何一条硬边界都
/// 等于在无限世界里砌一堵看不见的墙 —— 走到那儿就再也推不动,用户报的
/// 「地图非无限大,触碰空气墙无法前进」正是它。
///
pub(crate) const WORLD_HALF: f32 = 150.0;

// ===========================================================================
// 动态实体的碰撞体尺寸与质量
// ===========================================================================
//
// 质量以「一个人 = 1.0」为单位,方便直观比较。轿车取 30(1.5 t 量级对
// 75 kg 的人),所以人被车顶开时人承担 30/31 的分离量、车只承担 1/31 ——
// 手感上是「人被撞飞、车几乎不动」。
/// 行人 / 玩家的等效碰撞圆半径(米)。
pub(crate) const PED_RADIUS: f32 = 0.35;

/// 敌人的等效碰撞圆半径(米)。
pub(crate) const ENEMY_RADIUS: f32 = 0.35;

/// 玩家质量(单位:一个「人」)。
pub(crate) const PLAYER_MASS: f32 = 1.0;

/// 动态层把玩家推开所需的最低自身速度(米/秒)。
///
/// 低于这个速度就认为玩家「站着不动」:人群的互相让位是在解算**扰动**,
/// 不是解算墙 —— 静止时接受它会让每一帧都重复推出同一份分离向量
/// (实测零输入时恒定 +0.094 m/帧,合 5.66 m/s,人站着不动一路漂走)。
/// 静态层的推离不受此限制,那是真的解算结果。
pub(crate) const PLAYER_PUSH_MIN_SPEED: f32 = 0.05;

/// 低于这个长度的动态层位移直接丢弃(米)。
///
/// 与速度判据互补:贴着人群站着时 `resolve_all` 仍会给出亚毫米级的
/// 数值噪声,阈值以下一律当作「没被推」。
pub(crate) const PLAYER_PUSH_DEADZONE: f32 = 1e-3;

/// 行人质量(单位:一个「人」)。
pub(crate) const PED_MASS: f32 = 1.0;

/// 敌人质量(单位:一个「人」)。
pub(crate) const ENEMY_MASS: f32 = 1.0;

/// 轿车质量(单位:一个「人」;30 ≈ 1.5 t 对 75 kg)。
pub(crate) const CAR_MASS: f32 = 30.0;

// ---------------------------------------------------------------------------
// 棕榈布置(修「树木位置不合理 + 高度同质化」)
// ---------------------------------------------------------------------------
//
// 原来棕榈是 `step += 24.0` 的**刚性格点**,三个调用点各自用
// `0.9 + (index % 3) * 0.08` 算缩放 —— 全城只有 3 档高度,而且第 N 棵
// 永远和第 N+3 棵同高、同模型、同位置偏移。整条街看起来像复制粘贴。
//
// 现在:间距在 MIN..MAX 之间按种子抖动,每棵有独立的沿街 / 垂直偏移,
// `scale` 走**连续区间**并且有「成簇的高个」(按 PALM_TALL_FRACTION 抽签),
// 所以高度有起伏而不是一层层齐平。
/// 沿街两棵棕榈的最小间距(米)。
pub(crate) const PALM_PITCH_MIN: f32 = 17.0;

/// 沿街两棵棕榈的最大间距(米)。
pub(crate) const PALM_PITCH_MAX: f32 = 33.0;

/// 沿街两棵棕榈的平均间距(米):`PALM_PITCH_MIN` 与 `PALM_PITCH_MAX` 的中点。
///
/// 树位按 `PALM_PITCH_MAX` 的世界格点铺(见 [`street_slots`]),每格按这个
/// 概率决定种不种,于是相邻两棵的实际间距期望正好落在
/// `[PALM_PITCH_MIN, PALM_PITCH_MAX]` 这个区间里 —— 两端都写在规格里,
/// 不能让任何一个常量变成没人引用的死数。
pub(crate) const PALM_PITCH_AVG: f32 = (PALM_PITCH_MIN + PALM_PITCH_MAX) * 0.5;

/// 沿街方向的随机抖动幅度(米):破掉人造的等距节奏。
pub(crate) const PALM_PITCH_JITTER: f32 = 7.0;

/// 垂直于街方向的随机抖动幅度(米):有的贴路沿、有的靠外侧。
pub(crate) const PALM_SIDE_JITTER: f32 = 1.6;

/// 沿街方向那一维要避开的**路口**半宽(米),等于路面半宽。
///
/// 横向那一维天生安全:`STREET_HALF_WIDTH + SIDEWALK_WIDTH * 0.55` 再加
/// `±PALM_SIDE_JITTER / 2` 落在 `[8.18, 9.78]`,永远在人行道
/// `[7.0, 10.6]` 里。**出问题的是沿街那一维** —— 它是自由走的
/// `step + along_jitter`,每 `STREET_PITCH` 米就会撞上一次垂直街道的
/// 轴线,于是每隔 60 m 就有一棵树种在**十字路口的沥青上**。
///
/// 用户报的是「道路上不能有树木」,那是硬约束,不是观感问题:实测
/// 生成中心 (0, 0) 附近 830 棵里有 138 棵(16.6%)踩在路上,最近的一棵
/// 离路中线只有 **0.03 m**,正对着车道中心。路口这一段直接不种树。
pub(crate) const PALM_JUNCTION_GUARD: f32 = STREET_HALF_WIDTH;

/// 抽中「高个」的概率(0..1):少量高树成簇,避免整片同高。
pub(crate) const PALM_TALL_FRACTION: f32 = 0.22;

/// 普通棕榈的最小缩放。
pub(crate) const PALM_SCALE_MIN: f32 = 0.82;

/// 普通棕榈的最大缩放。
pub(crate) const PALM_SCALE_MAX: f32 = 1.08;

/// 高个棕榈的最小缩放。
pub(crate) const PALM_TALL_SCALE_MIN: f32 = 1.25;

/// 高个棕榈的最大缩放。
pub(crate) const PALM_TALL_SCALE_MAX: f32 = 1.65;

/// 棕榈树干与可进入样板楼门洞之间必须留出的净距(米)。
///
/// 内院棕榈的散布半径抖动到 7.5 m 之后,树干会长到门洞里去;碰撞体
/// 又跟着 `scale` 一起放大(高个 ×1.65),把门彻底堵死。留 6 m 净距。
pub(crate) const PALM_SHOWCASE_CLEARANCE: f32 = 6.0;

/// 棕榈碰撞半径最多按缩放放大到几倍(1.0 = 固定粗细)。
///
/// 树干粗细随树高走合理,但不能无上限:高个 ×1.65 再乘基准半径,
/// 靠墙的树就把过道挤没了。这里封顶。
pub(crate) const PALM_TRUNK_SCALE_CAP: f32 = 1.25;

/// 玩家出生点(世界坐标):`SPAWN_LANE_STREET` 那条街的**东侧车道**,z = 45。
///
/// 选点是拿截图试出来的,踩过三个坑:
///
/// 1. 路中间(x = 30)不行 —— 相机落在另一侧车道,背后除了沥青什么都
///    没有,整屏被单色地面糊死。
/// 2. 人行道(x = 38.5)也不行 —— 行道树在 x = 38.98,相机往后退 7.4 m
///    必然撞进树干,遮挡回避把距离一路压到 2.2 m,镜头直接怼在树皮上。
/// 3. 横对着马路更不行 —— 相机被推到楼面(x ≈ 49)跟前,一出生就是一堵墙。
///
/// 现在这个点:车道内的 x 由 [`lane_x`] 从街道索引派生,离两侧的建筑和
/// 行道树都有 6 m 以上;z = 45 远离 z = 24 / 60 两处路口,取景里是一条
/// 笔直的街道,身后是同一条街往远处延伸,两侧是楼房和行道树。
///
/// **x 必须由 [`lane_x`] 派生。** 它原先写死成 `33.5`,那同样是 30 m 网格
/// 的遗留(见 [`TRAFFIC_LANE_PLAN`]):街道网格变成 60 m 之后,`33.5` 落到
/// 了街区内侧 —— 玩家一出生就站在楼和道具之间,而车队在 56.5 / 63.5 上跑,
/// 于是最近的��离 23 m,超过 `ENTER_VEHICLE_RADIUS = 4.2`,按 F 上不了车。
/// 「上车功能等于不存在」正是这个后果。
pub(crate) const SPAWN_LANE_STREET: i32 = 1;

/// 玩家出生朝向(弧度)。
///
/// 玩家的前向是 `(cos yaw, -sin yaw)`。`yaw = PI/2` 时前向 = `(0, -1)`,
/// 也就是朝北、顺着这条街往远处看 —— 相机落在角色南侧 7.4 m,同样在这
/// 条街上,背后是同一条向远处延伸的马路,而不是楼面。
pub(crate) const SPAWN_YAW: f32 = FRAC_PI_2;

// ===========================================================================
// 场景蓝图:街区网格(多条平行街道 × 横向连接街道)
//
// 城市从「一条 86 m 直线」扩成真正的网格:
//
// ```text
//            x=-120    x=-60     x=0      x=60      x=120
//   z=-120     +--------+--------+--------+--------+
//              | block  | block  | block  | block  |
//   z=-60      +=== 十字路口(南北向街道)==+
//              | block  | block  | block  | block  |
//   z=0        +=== 十字路口 ============+
//              ...
// ```
//
// 城市是**无限**的:街道轴线落在 `STREET_PITCH * k` 上,没有边界。
// = 16 个网格路口,每个路口四角都有红绿灯。
// ===========================================================================
/// 街道半宽(路面,不含人行道)。
pub(crate) const STREET_HALF_WIDTH: f32 = 7.0;

/// 人行道宽度(单侧)。
pub(crate) const SIDEWALK_WIDTH: f32 = 3.6;

/// 相邻两条街道轴线之间的间距(米)。
///
/// 世界是**无限**的:轴线落在 `STREET_PITCH * k`(k 为任意整数)上,没有
/// 边界。原先这是一张写死的 `&[-90, -30, 30, 90]`,城市被锁死在
/// `[-150, 150]`,玩家走到 149 m 就撞墙。
pub(crate) const STREET_PITCH: f32 = 60.0;

/// 街区 / 道具的生成半径(米):以玩家为中心,这个半径内才生成城市内容。
///
/// 真实视距比这个小得多,但建筑有高度、街道有宽度,半径太小会看到
/// 「楼凭空出现」。这一档取 300 m,配合 22 m 的剔除半径已经够远。
pub(crate) const BLOCK_VIEW_RADIUS: f32 = 300.0;

/// 街道索引的绝对值上限(安全阀)。
///
/// 街道索引本身没有上限 —— 真正的边界是**视距**。这个常量只保证
/// 浮点坐标在荒谬远处时不至于让 `hash` 之类的位运算溢出,并且给
/// 「玩家跑到多远算跑出世界」一个可测的判据。
pub(crate) const STREET_INDEX_LIMIT: i32 = 100_000;

/// 街区内部沿道路方向的楼间距(米),留出巷子 / 后巷。
pub(crate) const LOT_PITCH: f32 = 34.0;

/// 建筑围合的「内圈」半径(米):离街区中心多远开始放楼。
pub(crate) const BLOCK_INNER: f32 = 11.0;

/// 地面网格在每个方向上的分段数(整块一次,不是每街区一次)。
pub(crate) const GROUND_SUBDIV: usize = 192;

/// 地面网格单块的边长(米):以玩家为中心,左右各这么多。
///
/// 旧值是 300 m,固定铺满整城。世界改成无限之后
/// 玩家可以一直往外走,300 m 必然不够 —— 现在这块地面会跟着玩家
/// 重新生成,边长只决定「一次生成多少顶点」。
pub(crate) const GROUND_SPAN: f32 = 480.0;

/// 地面网格重新生成的步长(米):玩家走满这么远才重算一次。
pub(crate) const GROUND_CELL_ALIGN: f32 = STREET_PITCH;

/// 楼 / 招牌 / 车 / 行人用的建筑资产池(全部不同资产,循环取用)。
///
/// 网格街区要摆 ~200 栋楼,如果只给 10 个不同资产,同一个 mesh 会以
/// 20 个实例反复出现 —— 顶点数据仍然只解析一次(instancing 友好),
/// 但远景会明显看出「同一栋楼在原地克隆」。12 个资产 + 按格子位置
/// 做 `scale` / `tint` / `yaw` 抖动,足以让成片街区读起来是城市而不是
/// 复制粘贴。
pub(crate) const BUILDING_POOL: &[&str] = &[
    BLDG_DECO_PINK,
    BLDG_MINT_SHOP,
    BLDG_CREAM_BLOCK,
    BLDG_CORAL_HALL,
    BLDG_TEAL_LOFT,
    BLDG_AQUA_ARCADE,
    BLDG_DECO_TEAL,
    BLDG_SAND_MIDRISE,
    BLDG_PINK_TERRACE,
    BLDG_WHITE_LANDMARK,
    BLDG_APRICOT_MOTEL,
    BLDG_LILAC_TOWER,
];

/// 招牌资产池。
pub(crate) const SIGN_POOL: &[&str] = &[
    SIGN_HOTEL,
    SIGN_PIZZA,
    SIGN_CLUB,
    SIGN_BAR,
    SIGN_TROPIC,
    SIGN_ARCADE,
    SIGN_MOTEL,
    SIGN_DINER,
];

/// 车辆资产池。
pub(crate) const VEHICLE_POOL: &[&str] =
    &[CAR_TAXI, CAR_POLICE, CAR_SEDAN, CAR_COUPE, TRUCK_PICKUP];

/// 行人资产池。
pub(crate) const PED_POOL: &[&str] = &[PED_SUIT, PED_STREETWEAR, PED_DRESS, PED_OVERALLS];

/// 楼的位置在街区内的符号偏移(前 / 后 / 左 / 右 四条围合边)。
///
/// 街区的四边都放楼,中间留出 `BLOCK_INNER` 的内院 —— 这样每个街区
/// 内部有建筑围合和巷子,而不是只有沿街一排。
pub(crate) const BLOCK_RING: &[(f32, f32, f32)] = &[
    // (相对街区中心的 dx, dz, 朝向街道的 yaw)
    (0.0, -1.0, 0.0),
    (0.0, 1.0, PI),
    (-1.0, 0.0, FRAC_PI_2),
    (1.0, 0.0, -FRAC_PI_2),
];

/// 可进入样板楼的世界摆放表。
///
/// 两栋楼**固定**摆在出生点两侧,而不是程序化街区生成的随机位置:
/// 玩家出生在 `(33.5, 45)`,两栋分别在西边 `(10, 48)` 和东边 `(52, 48)`,
/// 门洞都朝着出生点 —— 推门就进,不用找。
///
/// 坐标不是拍脑袋写的,是一次性网格搜索的结果,四个条件同时成立:
/// 1. **不压车行道**:外轮廓的每个角离所有街道轴线都大于
///    `STREET_HALF_WIDTH + SIDEWALK_WIDTH`(硬约束,见 `on_roadway`)。
/// 2. **不与程序化楼重叠**:与 `build_city_buildings` 的每一栋都留 1 m。
/// 3. **门前有一块 7 m 长的空地**:沿门洞法线往外采样,既没有别的楼,
///    也没有路灯 / 长椅 / 垃圾桶 —— 玩家不会被一排道具堵在门外。
/// 4. **门朝出生点**:门法线与「指向出生点」的方向夹角 < 37°。
///
/// **朝向**决定了门洞朝向:导出的资产里门洞在本地 **+Z** 面(`interior.py`
/// 的临街面 `y_in`,经 `(x, z, -y)` 导出后成为本地 +Z),`Instance` 的约定
/// 是本地 +Z → 世界 `(sin yaw, 0, cos yaw)`。所以:
/// - `LOFT_YAW = +π/2` → 门法线 `(1, 0)`,门朝 +X,即朝东面的出生点;
/// - `SHOP_YAW = -π/2` → 门法线 `(−1, 0)`,门朝 −X,即朝西面的出生点。
///
/// 两栋的门因此**相向而开**,从街区中间那条空地上就能同时看见两个开口。
pub(crate) const SHOWCASE_YAW: f32 = FRAC_PI_2;

/// 样板楼 B 的朝向与 A 相反,让两栋的门相向。
pub(crate) const SHOWCASE_YAW_FLIPPED: f32 = -FRAC_PI_2;

/// 样板楼 A(LOFT)所在街区的 X 索引 —— 街区中心在 `STREET_PITCH * k`。
pub(crate) const SHOWCASE_LOFT_BLOCK_X: i32 = 0;

/// 样板楼 A(LOFT)相对街区中心的 X 偏移(米)。
///
/// 偏移是必需的:街区中心正好落在两条街道的**中线上**吗?不 —— 街区
/// 中心就是 `PITCH * k`,而街道轴线也在 `PITCH * k`,所以街区中心
/// **就在街道上**。要放楼必须往街区内部挪,离两侧街道都留够
/// `STREET_HALF_WIDTH + SIDEWALK_WIDTH`。
pub(crate) const SHOWCASE_LOFT_OFFSET_X: f32 = -7.0;

/// 样板楼 A(LOFT)所在街区的 Z 索引。
pub(crate) const SHOWCASE_LOFT_BLOCK_Z: i32 = 0;

/// 样板楼 A(LOFT)相对街区中心的 Z 偏移(米)。
pub(crate) const SHOWCASE_LOFT_OFFSET_Z: f32 = 0.0;

/// 样板楼 B(SHOP)所在街区的 X 索引。
pub(crate) const SHOWCASE_SHOP_BLOCK_X: i32 = 0;

/// 样板楼 B(SHOP)相对街区中心的 X 偏移(米)—— 与 A 相向而开。
pub(crate) const SHOWCASE_SHOP_OFFSET_X: f32 = 7.0;

/// 样板楼 B(SHOP)所在街区的 Z 索引。
pub(crate) const SHOWCASE_SHOP_BLOCK_Z: i32 = 0;

/// 样板楼 B(SHOP)相对街区中心的 Z 偏移(米)。
pub(crate) const SHOWCASE_SHOP_OFFSET_Z: f32 = 0.0;

/// 门洞在资产本地坐标里的 Z(米)—— `interior` 里的外墙就铺在这个面上。
///
/// 之前这个数字只以字面量 `6.4` 出现在 `interior.rs` 的四面墙里,验收
/// 脚本要导航到门口时只能从源码里抄一遍。抄出来的数字和真实墙面差
/// 0.1 m 就走不进门,与其抄不如让它成为一处命名常量。
pub(crate) const SHOWCASE_DOOR_Z: f32 = 6.4;

/// 外墙厚度(米)—— 与 `interior` 里铺墙用的 `WALL_T` 是同一个数。
pub(crate) const SHOWCASE_WALL_THICKNESS: f32 = 0.2;

/// 可进入样板楼的**内墙之间**净跨(米),即资产 `floor_ground` 的 XZ 尺寸。
///
/// 这些数字是从 `assets/bldg_*_showcase.json` 的 `floor_ground` 包围盒
/// **逐值量出来的**,不是手算的,所以碰撞体与可见几何一定对齐 —— 差
/// 5 cm 就会出现「站在空气里」或者「被一层看不见的壳挡在门外」。
pub(crate) const SHOWCASE_LOFT_SPAN: Vec2 = [11.50, 9.50];

/// 商铺样板楼的内墙净跨(米)。
pub(crate) const SHOWCASE_SHOP_SPAN: Vec2 = [9.50, 8.50];

/// 样板楼外轮廓(不含挑出的线脚)的半尺寸(米)—— 资产 `shell` 包围盒。
pub(crate) const SHOWCASE_LOFT_HALF: Vec2 = [6.00, 5.00];

/// 商铺样板楼外轮廓半尺寸(米)。
pub(crate) const SHOWCASE_SHOP_HALF: Vec2 = [5.00, 4.50];

/// 楼的最大占地半尺寸(米)—— 用来保证整栋楼都不压到车行道。
///
/// 取一个偏保守的常数而不是逐个资产的包围盒:建筑资产的包围盒差异很大
/// (10.7 m 到 6.4 m),用统一余量判定简单、可预测,而且宁可少摆一栋楼
/// 也不会出现「隐形墙堵住车道」。
pub(crate) const BUILDING_FOOTPRINT_GUARD: f32 = 11.0;

// ===========================================================================
// 游戏状态
// ===========================================================================
/// `DayPhase` 的默认相位(正午)。
pub(crate) const DEFAULT_PHASE: DayPhase = DayPhase::Noon;

/// 玩家走到哪里了才需要重新生成地面 —— 距离当前生成中心超过这一档。
///
/// 地面块边长是 [`GROUND_SPAN`],玩家站在正中时离边缘还有一半。阈值
/// 取一整个街道间距(60 m):世界无限,但**每走一格街道才重算一次**是
/// 够的 —— 地面网格本身就是按街道网格对齐的,重算与不重算在视觉上
/// 完全一致,只有走到块外才必须重来。
pub(crate) const STREAM_REBUILD_STEP: f32 = STREET_PITCH;

/// NPC 换批的伪随机相位由街区坐标算出 —— 同一街区永远得到同一批行人,
/// 不会因为换块抖动而每次都不一样。
pub(crate) const NPC_PHASE_SCALE: f32 = 0.017;

/// 流式窗口边界的预留余量(米):摆件钉在街道轴线旁边的最大偏移。
///
/// 生成窗口的边界是**街道轴线**,而路灯、长椅、报摊、电话亭最多挂在轴线外
/// `STREET_HALF_WIDTH + SIDEWALK_WIDTH + 2.2` ≈ 13.6 m 处,沿线点缀还有
/// `+13 m`。回归测试要断言「窗口内部的世界不许变」,就必须先扣掉这一圈,
/// 否则边界那一档站位的正常进出窗口会被算成凭空消失。
#[cfg(test)]
pub(crate) const STREAM_SNAPSHOT_PAD: f32 = 16.0;

// ===========================================================================
// 交通 AI + 拾取物蓝图
// ===========================================================================
/// 车道中心相对街道中轴线的横向偏移(米)。
///
/// 路面半宽 `STREET_HALF_WIDTH` = 7 m,双向车道各占一半,车道中心落在
/// ±3.5 m 处 —— 正好压在程序化地面画的车道虚线上。
pub(crate) const LANE_OFFSET_X: f32 = 3.5;

/// 车队循环轨道的半长(米):车沿 Z 跑完这一段就 wrap 回起点。
///
/// 世界本身没有边界,这一档只是「车队循环一圈多长」,不是世界的边。
/// 旧值是 `CITY_HALF - 30`,把世界边长当成了循环长度。
pub(crate) const TRAFFIC_HALF: f32 = 600.0;

/// 实心大道具(交通灯 / 路灯)碰撞体不缩放。
pub(crate) const FULL_PROP_COLLIDER_SCALE: f32 = 1.0;

/// 小型路边道具碰撞体缩到 55%(见 [`prop_collider_scale`] 的原因)。
pub(crate) const SMALL_PROP_COLLIDER_SCALE: f32 = 0.55;

/// 锥桶碰撞体缩到 30%(锥桶本来就该被车碾过去,不该挡路)。
pub(crate) const CONE_COLLIDER_SCALE: f32 = 0.3;

/// 棕榈树干碰撞半径(米)—— 只挡人,树叶可以从中穿过。
pub(crate) const PALM_TRUNK_RADIUS: f32 = 0.4;

/// 实心足迹的高度带下沿(米):小腿高度。矮于此的地面贴片 / 车道涂装不该挡人。
pub(crate) const BODY_BAND_LO: f32 = 0.10;

/// 实心足迹的高度带上沿(米):头顶高度。二维碰撞世界没有高度概念,
/// 所以高于这条线的几何(雨篷 / 檐口 / 路灯臂 / 屋顶设备)对行走的玩家
/// 不存在 —— 把它们算进碰撞盒就是空气墙。
pub(crate) const BODY_BAND_HI: f32 = 1.90;

/// 车道缓冲区半宽(米):街道中轴线两侧这么多米内不放静态碰撞体。
pub(crate) const LANE_CLEAR_MARGIN: f32 = 1.8;

/// 「车道落在哪条街上」这条回归测试要填的槽位名(§1.3c:字符串不写死在逻辑里)。
pub(crate) const SLOT_TRAFFIC_LANE: &str = "lane_x";

/// 同上,车辆资产 id 槽位。
pub(crate) const SLOT_TRAFFIC_ASSET: &str = "asset";

/// 同上,车道起始 Z 槽位。
pub(crate) const SLOT_TRAFFIC_START_Z: &str = "start_z";

/// 回归测试:车队车道必须落在沥青面上。{asset} 的车道 x={lane_x} 不在路上。
pub(crate) const T_TRAFFIC_LANE_OFF_ROAD: &str = "车队车道必须压在沥青面上,否则车会骑在楼和道具上:{asset} lane_x={lane_x} \
     start_z={start_z}";

/// 车队车道蓝图表。`街道索引` 交给 [`traffic_lanes`] 在运行时解成坐标。
pub(crate) const TRAFFIC_LANE_PLAN: &[TrafficLane] = &[
    // (资产 id, 街道索引, 车道侧 +1 / -1, 初始 Z, 巡航速度, 行驶方向)
    //
    // 街道索引 1 的轴线在 x = 60,两侧车道就是 60 ± 3.5 = 56.5 / 63.5;
    // 索引 0 的轴线在 x = 0,两侧是 ±3.5。玩家出生点 (33.5, 45) 就在索引 1
    // 那条街上,所以车会从玩家眼前开过 —— 上车测试有确定性。
    (CAR_SEDAN, 1, 1.0, 60.0, crate::traffic::CRUISE_SPEED, -1.0),
    (CAR_TAXI, 1, -1.0, -30.0, crate::traffic::CRUISE_SPEED, 1.0),
    (CAR_COUPE, 1, 1.0, -60.0, crate::traffic::CRUISE_SPEED, -1.0),
    (
        TRUCK_PICKUP,
        1,
        -1.0,
        90.0,
        crate::traffic::CRUISE_SPEED,
        1.0,
    ),
    (
        CAR_POLICE,
        1,
        1.0,
        110.0,
        crate::traffic::CRUISE_SPEED,
        -1.0,
    ),
    (
        CAR_SEDAN,
        -1,
        -1.0,
        -60.0,
        crate::traffic::CRUISE_SPEED,
        1.0,
    ),
    (CAR_TAXI, -1, 1.0, 30.0, crate::traffic::CRUISE_SPEED, -1.0),
    (CAR_COUPE, -1, -1.0, 90.0, crate::traffic::CRUISE_SPEED, 1.0),
    (
        TRUCK_PICKUP,
        -1,
        1.0,
        -90.0,
        crate::traffic::CRUISE_SPEED,
        -1.0,
    ),
    (
        CAR_POLICE,
        -1,
        -1.0,
        100.0,
        crate::traffic::CRUISE_SPEED,
        1.0,
    ),
];

/// 地面拾取物蓝图:`(资产, HUD 名, 世界坐标)`。
///
/// 全部落在 X = 30 这条街的人行道上(`30 ± (7 + 2.2)`),玩家出生点就在
/// 同一条人行道上,走几步就能捡到。`marker_flame` 是任务目标点,夜里靠
/// 自身 emissive 远远就能看见。
pub(crate) const PICKUP_PLACEMENTS: &[(&str, &str, Vec3)] = &[
    // 出生点 (30, 0) 在十字路口。拾取物沿两条街的人行道分布,
    // 坐标从人行道外沿推出(不落在车道缓冲带里,见 TRAFFIC_LANE_PLAN)。
    (PICKUP_HEALTH_PACK, NOTICE_HEALTH, [35.4, 0.35, 36.0]),
    (WEP_SMG, NOTICE_WEAPON, [24.6, 0.35, 24.0]),
    (PICKUP_CASH_STACK, NOTICE_CASH, [24.6, 0.35, 4.0]),
    (PICKUP_ARMOR_VEST, NOTICE_ARMOR, [24.6, 0.35, -12.0]),
    (PICKUP_AMMO_BOX, NOTICE_AMMO, [35.4, 0.35, 56.0]),
    (WEP_PISTOL, NOTICE_WEAPON, [24.6, 0.35, 20.0]),
    (WEP_BAT, NOTICE_WEAPON, [35.4, 0.35, -26.0]),
    (WEP_GRENADE, NOTICE_WEAPON, [24.6, 0.35, 30.0]),
    (PICKUP_HEALTH_PACK, NOTICE_HEALTH, [35.4, 0.35, 40.0]),
    (PICKUP_AMMO_BOX, NOTICE_AMMO, [24.6, 0.35, -30.0]),
    (PICKUP_CASH_STACK, NOTICE_CASH, [35.4, 0.35, 52.0]),
    (MARKER_FLAME, NOTICE_MARKER, [26.5, 0.35, -60.0]),
];

/// 需要按 part 切出可转车轮的车型资产 id。
///
/// 只有这五个 `car_*` 资产带 `tyres` / `hubs` 部件(见各自的
/// `parts[].name`);把它们留一份原始 `MeshAsset` 好切单轮网格。
pub(crate) const CAR_ASSET_IDS: &[&str] =
    &[CAR_TAXI, CAR_COUPE, CAR_SEDAN, CAR_POLICE, TRUCK_PICKUP];

/// 玩家骨架要渲染的 part 名(躯干 / 头 / 头发 + 四肢 + 两只鞋)。
///
/// 顺序不重要(每个 part 一个独立批次),但必须与 `ped_suit.json` 的
/// `parts[].name` 完全一致。
pub(crate) const PLAYER_PARTS: &[&str] = &[
    PART_TORSO,
    PART_HEAD,
    PART_HAIR,
    PART_UPPER_ARM_L,
    PART_LOWER_ARM_L,
    PART_UPPER_ARM_R,
    PART_LOWER_ARM_R,
    PART_UPPER_LEG_L,
    PART_LOWER_LEG_L,
    PART_UPPER_LEG_R,
    PART_LOWER_LEG_R,
    PART_SHOE_L,
    PART_SHOE_R,
];
