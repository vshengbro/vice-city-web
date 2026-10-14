use super::*;

/// 藏身区圆心:后巷与警局门口。
///
/// 规则很简单 —— 站进任何一个圆里,通缉就会**强制降温**且不被视为
/// 「被看见」。选点标准是「街边够窄、车进不来、人能贴墙」,所以全部
/// 落在街区的内圈空地上(见 `block_layouts` 的 `BLOCK_INNER`)。
///
/// 街区对角方向的一半(米):`STREET_LINES` 是 ±30 / ±90,取最外侧那条的
/// 中点,得到 4 个街区中心。
pub(crate) const BLOCK_HALF: f32 = 60.0;

/// 街区中心在 X / Z 上从原点偏移的距离(米)。
pub(crate) const BLOCK_OFFSET: f32 = 60.0;

/// 警局门口:街区边缘、贴着一栋楼的位置(与后巷同构,只是取外圈)。
pub(crate) const STATION_OFFSET: f32 = 78.0;

/// 藏身区圆心在「相邻两条街道中点」上的额外偏移(米)。
///
/// 街区的几何中心是两组街道的交点,那里通常是一片空地(四周是楼),
/// 玩家跑进去之后四周的楼会挡住所有警察的视线 —— 这就是「后巷甩警」的
/// 真实原理:靠几何遮挡,不靠无敌帧。偏移把这个点往街区中心推一点,
/// 避免玩家停在正中间(四周都是开阔地,反而没遮挡)。
pub(crate) const HIDE_OFFSET: f32 = 2.0;

/// 警察增派点的散布半径(米):新警察在玩家周围的这个半径里冒出来。
pub(crate) const DEPLOY_RING_MIN: f32 = 16.0;

/// 警察增派点的散布半径上限(米)。
pub(crate) const DEPLOY_RING_MAX: f32 = 27.0;

/// 混混的生成距离下限(米)。
pub(crate) const THUG_SPAWN_MIN: f32 = 34.0;

/// 混混的生成距离上限(米)。
pub(crate) const THUG_SPAWN_MAX: f32 = 62.0;

/// 任务目标点所在的街道轴线(米):取**街道网格的一格**,也就是
/// [`STREET_PITCH`]。原先写死 ±30 —— 那是 30 m 街道间距时代的坐标,
/// 而世界早就是 60 m 一格,于是三个目标全落在两个街区之间的空地里
/// (实测离最近的街道轴线 21.8 m),既不是马路也不是人行道。
///
/// 目标落在街边的人行道上 —— 走路能到、开车也能到。
pub(crate) const MISSION_ROAD_X: f32 = crate::STREET_PITCH;

/// 任务目标点的 Z 坐标(米):同样是街道网格的一格。
pub(crate) const MISSION_ROAD_Z: f32 = crate::STREET_PITCH;

/// 目标点往人行道方向的偏移(米):`STREET_HALF_WIDTH + 1.2`。
pub(crate) const MISSION_SIDEWALK: f32 = 8.2;

/// 任务蓝图表:`(标题, 阶段, 目标坐标)`。
///
/// 阶段见 `MISSION_GOTO` / `MISSION_DRIVE` / `MISSION_KILL`。三个目标
/// 分别落在三条不同的街上,强制玩家真的横穿城市。
pub(crate) const MISSION_TABLE: &[(&str, u32, Vec3)] = &[
    (
        MISSION_TITLE_A,
        MISSION_GOTO,
        [
            MISSION_ROAD_X + MISSION_SIDEWALK,
            0.0,
            MISSION_ROAD_Z + MISSION_SIDEWALK,
        ],
    ),
    (
        MISSION_TITLE_B,
        MISSION_DRIVE,
        [-MISSION_ROAD_X - MISSION_SIDEWALK, 0.0, -MISSION_ROAD_Z],
    ),
    (
        MISSION_TITLE_C,
        MISSION_KILL,
        [
            MISSION_ROAD_X - MISSION_SIDEWALK,
            0.0,
            -MISSION_ROAD_Z - MISSION_SIDEWALK,
        ],
    ),
];

/// 一个任务串起来的段数。
pub(crate) const MISSION_CHAIN_LENGTH: u32 = 2;
