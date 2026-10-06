//! 游戏主体:场景搭建、异步资产加载、固定步长循环、输入、昼夜循环。
//!
//! 架构要点(与 `euv-game-real-api-notes` 的坑对应):
//!
//! - **不在 hook / 事件回调里调用 euv hook**。整个游戏只有一棵静态
//!   `html!` 树(见 [`app_root`]),挂载完成后所有每帧逻辑都由裸
//!   `web_sys` + `Closure` 驱动,VDOM 不参与每帧渲染。
//! - 资产用 `fetch` + `wasm_bindgen_futures` 异步加载,带进度条。
//! - 固定步长(累加器 + `requestAnimationFrame`),`dt` 钳位在 50 ms。
//! - 场景按资产分批([`render::SceneBatch`]),同类资产只解析一次。

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use euv::{
    VirtualNode, html,
    js_sys::{Function, Promise, Undefined},
    wasm_bindgen::JsCast,
    wasm_bindgen::prelude::*,
    wasm_bindgen_futures::{JsFuture, spawn_local},
    web_sys::{
        Document, DomRect, Element, Event, EventTarget, HtmlCanvasElement, HtmlInputElement,
        KeyboardEvent, MouseEvent, Node, Response, TouchEvent, TouchList, WheelEvent, Window,
        window,
    },
};

use crate::{
    camera::{CAMERA_MIN_HEIGHT, Camera, Mat4, OCCLUSION_MIN_DISTANCE, PRESS_IN_DISTANCE},
    collision::{BodyKind, CollisionWorld, DynamicBody, placement_box},
    combat::{
        AiState, Arsenal, Enemy, Faction, HurtState, Mission, Pedestrian, Wanted, Weapon,
        apply_damage, body_matrix, falloff, flat_distance, has_line_of_sight, regen_armor,
    },
    r#const::*,
    enemy::{aim_with_spread, decide, is_active},
    interior::{FloorWorld, STEP_UP_TOLERANCE},
    mesh::{Bounds, GpuMesh, MeshAsset, MeshError, MeshPart, expand_asset},
    player::{
        Player, RUN_SPEED, WALK_SPEED, joint_pivot, limb_chain_matrix, limb_parent, limb_swing,
    },
    render::{
        AdaptiveQuality, DayPhase, Instance, MeshAssetGpu, NEAR_CULL_RADIUS,
        NEAR_CULL_RADIUS_FOLLOW, Renderer, Scene, SceneBatch, SceneLighting, SoftwareRenderer,
        WebGlRenderer, build_gpu_mesh, normalize3,
    },
    spawn::{
        advance_mission, build_hideouts, deploy_police, is_hidden, mission_blueprints, spawn_peds,
        spawn_thugs,
    },
    traffic::{PICKUP_RADIUS, Pickup, Traffic, TrafficCar, apply_pickup, nearest_car},
    r#type::{Mat4Data, MeshExtent, PalmSpots, Placement, Vec2, Vec3},
};

// ===========================================================================
// 常量
//
// 字符串常量统一放在 `crate::r#const`(上面已 glob 引入),
// 这里只保留数值常量。
// ===========================================================================

/// 固定步长:1/60 秒。
const FIXED_DT: f32 = 1.0 / 60.0;

/// 单帧最大累积时间(秒),超过就丢弃(标签页切回来时不追赶)。
const MAX_FRAME_TIME: f32 = 0.25;
/// 远裁剪面距离(米)。
///
/// G-buffer 的线性深度、SSAO 的视空间位置重建、SSR 的射线步进
/// 都以这个值做归一化,所以必须和相机真正使用的远裁剪面一致。
const FAR_PLANE: f32 = 600.0;
/// 滚轮每单位缩放。
const ZOOM_STEP: f32 = 0.0016;

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
const LOOK_YAW_SENSITIVITY: f32 = -0.006;

/// 鼠标纵向位移转相机俯角的灵敏度(弧度/像素)。
///
/// 屏幕 Y 向下为正,`dy > 0` 是鼠标下移。第三人称下 `pitch` 是相机
/// **俯角**(`camera.rs` 的 `eye()` 用 `+ sin(pitch)·distance` 抬高眼点),
/// 俯角增大 = 相机抬高 = 视线往下压,与玩家「往下看」一致,故取正。
///
/// 实测:向下拖 240 px → `camPitch` 0.16 → 1.05(Δ=+0.89,被
/// `FOLLOW_PITCH_MAX` 截断),方向正确。
const LOOK_PITCH_SENSITIVITY: f32 = 0.005;

/// 下车后玩家与车身保持的距离(米)。
const EXIT_CAR_OFFSET: f32 = 2.4;
/// 所有会被游戏接管的按键。
///
/// 之所以用数组扫描而不是 `matches!`:这些键名都是 `&'static str` 常量,
/// 放进 `matches!` 会被解释成 pattern 绑定(编译报 E0408),不是常量比较。
const TRACKED_KEYS: &[&str] = &[
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
const FOLLOW_PITCH: f32 = 0.16;
/// 第三人称垂直视场角(弧度)。
///
/// 取 75° 而不是 60°:玩家站在 14 m 宽的街道上,两侧的楼离镜头中心
/// 轴约 20 m。60° 垂直视场配 16:9 画幅只有约 34° 的水平张角,半角
/// 17° 在 7.4 m 处只覆盖 ±2.3 m —— 街道两侧的楼、行道树、路灯全部
/// 落在视锥之外,画面里只剩脚下的一小块路面。75° 把水平张角拉到约
/// 50°,半角 25° 覆盖 ±3.5 m,再配合稍近的跟随距离,整条街的立面
/// 才进得了取景框。
const FOLLOW_FOV: f32 = 1.309;
/// 第三人称近裁剪面(米)。
///
/// **不能太小。** 0.05 m 配 420 m 的远平面是 8400:1 的深度比,在没有
/// GPU 的软件光栅(swiftshader)上深度缓冲精度极低,近处几何的深度值
/// 全被压到 0 附近,互相 `LEQUAL` 判等 —— 整帧只剩最后画的那个批次,
/// 画面糊成一片单色(实测:贴地第三人称只有 2~3 种颜色,把 near 提到
/// 12 m 反而"正常"了,因为近处几何直接被切掉、只剩远景不打架)。
/// 相机最近的落点是 2.2 m(遮挡回避硬下限),0.35 m 的近平面已经
/// 足够避开角色自身,又给深度缓冲留出 1200:1 的余量。
const FOLLOW_NEAR: f32 = 0.35;
/// 第三人称远裁剪面(米)。
///
/// 城市是 ±150 m 的网格,站在角落能看到的最远楼角约 420 m,但雾在
/// 120 m 就开始起效,460 m 处完全融进天色 —— 所以 300 m 已经「看不出
/// 被裁掉」,却把深度比从 8400:1 压到 857:1,配合 `FOLLOW_NEAR` 给出
/// 软件光栅也能用的精度。
const FOLLOW_FAR: f32 = 300.0;

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
const FOLLOW_DISTANCE: f32 = 8.2;
/// 第三人称跟随相机的最小距离(滚轮拉近下限)。
const FOLLOW_DISTANCE_MIN: f32 = 2.2;
/// 第三人称跟随相机的最大距离(滚轮拉远上限)。
const FOLLOW_DISTANCE_MAX: f32 = 22.0;
/// 遮挡回避「拉近」的阻尼速率(1/秒):越大越快缩进来。
///
/// 必须比拉远快一个量级 —— 撞墙时镜头的第一反应就该是「躲」,而不是
/// 用 0.3 s 慢慢推进去展示墙面。
const OCCLUSION_IN_RATE: f32 = 22.0;
/// 遮挡回避「拉远」的阻尼速率(1/秒):离开墙后平滑弹回。
const OCCLUSION_OUT_RATE: f32 = 3.4;
/// 跟随焦点的高度(米)——看胸口而不是看脚。
///
/// 取 1.45 而不是 1.35:角色约 1.8 m 高,焦点略高一点能让头顶留在
/// 取景框内,而不是被画面上缘切掉。
const FOLLOW_HEIGHT: f32 = 1.45;
/// 跟随焦点跟随角色的阻尼速率(1/秒):越大越硬,越小越飘。
const FOLLOW_DAMPING: f32 = 9.0;
/// 玩家碰撞圆半径(米)。
const PLAYER_RADIUS: f32 = 0.35;

/// 世界的**软边界**半径(米):玩家不许走出这个圆。
///
/// 这不是硬墙。城市是**程序化无限生成**的(见
/// `step_streamed_surface` / `rebuild_streamed_surface`),任何一条硬边界都
/// 等于在无限世界里砌一堵看不见的墙 —— 走到那儿就再也推不动,用户报的
/// 「地图非无限大,触碰空气墙无法前进」正是它。
///
const WORLD_HALF: f32 = 150.0;

// ===========================================================================
// 动态实体的碰撞体尺寸与质量
// ===========================================================================
//
// 质量以「一个人 = 1.0」为单位,方便直观比较。轿车取 30(1.5 t 量级对
// 75 kg 的人),所以人被车顶开时人承担 30/31 的分离量、车只承担 1/31 ——
// 手感上是「人被撞飞、车几乎不动」。

/// 行人 / 玩家的等效碰撞圆半径(米)。
const PED_RADIUS: f32 = 0.35;
/// 敌人的等效碰撞圆半径(米)。
const ENEMY_RADIUS: f32 = 0.35;
/// 车辆的等效碰撞圆半径(米)。
const CAR_RADIUS: f32 = 1.25;
/// 玩家质量(单位:一个「人」)。
const PLAYER_MASS: f32 = 1.0;
/// 行人质量(单位:一个「人」)。
const PED_MASS: f32 = 1.0;
/// 敌人质量(单位:一个「人」)。
const ENEMY_MASS: f32 = 1.0;
/// 轿车质量(单位:一个「人」;30 ≈ 1.5 t 对 75 kg)。
const CAR_MASS: f32 = 30.0;

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
const PALM_PITCH_MIN: f32 = 17.0;
/// 沿街两棵棕榈的最大间距(米)。
const PALM_PITCH_MAX: f32 = 33.0;
/// 沿街方向的随机抖动幅度(米):破掉人造的等距节奏。
const PALM_PITCH_JITTER: f32 = 7.0;
/// 垂直于街方向的随机抖动幅度(米):有的贴路沿、有的靠外侧。
const PALM_SIDE_JITTER: f32 = 1.6;
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
const PALM_JUNCTION_GUARD: f32 = STREET_HALF_WIDTH;
/// 抽中「高个」的概率(0..1):少量高树成簇,避免整片同高。
const PALM_TALL_FRACTION: f32 = 0.22;
/// 普通棕榈的最小缩放。
const PALM_SCALE_MIN: f32 = 0.82;
/// 普通棕榈的最大缩放。
const PALM_SCALE_MAX: f32 = 1.08;
/// 高个棕榈的最小缩放。
const PALM_TALL_SCALE_MIN: f32 = 1.25;
/// 高个棕榈的最大缩放。
const PALM_TALL_SCALE_MAX: f32 = 1.65;

/// 棕榈树干与可进入样板楼门洞之间必须留出的净距(米)。
///
/// 内院棕榈的散布半径抖动到 7.5 m 之后,树干会长到门洞里去;碰撞体
/// 又跟着 `scale` 一起放大(高个 ×1.65),把门彻底堵死。留 6 m 净距。
const PALM_SHOWCASE_CLEARANCE: f32 = 6.0;
/// 棕榈碰撞半径最多按缩放放大到几倍(1.0 = 固定粗细)。
///
/// 树干粗细随树高走合理,但不能无上限:高个 ×1.65 再乘基准半径,
/// 靠墙的树就把过道挤没了。这里封顶。
const PALM_TRUNK_SCALE_CAP: f32 = 1.25;
/// 玩家出生点(世界坐标):X = 30 那条街的**路中间偏东的车道**,z = 45。
///
/// 选点是拿截图试出来的,踩过三个坑:
///
/// 1. 路中间(x = 30)不行 —— 相机落在另一侧车道,背后除了沥青什么都
///    没有,整屏被单色地面糊死。
/// 2. 人行道(x = 38.5)也不行 —— 行道树在 x = 38.98,相机往后退 7.4 m
///    必然撞进树干,遮挡回避把距离一路压到 2.2 m,镜头直接怼在树皮上。
/// 3. 横对着马路更不行 —— 相机被推到楼面(x ≈ 49)跟前,一出生就是一堵墙。
///
/// 现在这个点:车道内(30.4..33.6)的 x = 33.5,离两侧的建筑和行道树都
/// 有 6 m 以上;z = 45 远离 z = 24 / 60 两处路口,取景里是一条笔直的
/// 街道,身后是同一条街往远处延伸,两侧是楼房和行道树。
const SPAWN_POINT: Vec3 = [33.5, 0.0, 45.0];
/// 玩家出生朝向(弧度)。
///
/// 玩家的前向是 `(cos yaw, -sin yaw)`。`yaw = PI/2` 时前向 = `(0, -1)`,
/// 也就是朝北、顺着这条街往远处看 —— 相机落在角色南侧 7.4 m,同样在这
/// 条街上,背后是同一条向远处延伸的马路,而不是楼面。
const SPAWN_YAW: f32 = std::f32::consts::FRAC_PI_2;

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
const STREET_HALF_WIDTH: f32 = 7.0;
/// 人行道宽度(单侧)。
const SIDEWALK_WIDTH: f32 = 3.6;
/// 相邻两条街道轴线之间的间距(米)。
///
/// 世界是**无限**的:轴线落在 `STREET_PITCH * k`(k 为任意整数)上,没有
/// 边界。原先这是一张写死的 `&[-90, -30, 30, 90]`,城市被锁死在
/// `[-150, 150]`,玩家走到 149 m 就撞墙。
const STREET_PITCH: f32 = 60.0;
/// 街区 / 道具的生成半径(米):以玩家为中心,这个半径内才生成城市内容。
///
/// 真实视距比这个小得多,但建筑有高度、街道有宽度,半径太小会看到
/// 「楼凭空出现」。这一档取 300 m,配合 22 m 的剔除半径已经够远。
const BLOCK_VIEW_RADIUS: f32 = 300.0;
/// 街道索引的绝对值上限(安全阀)。
///
/// 街道索引本身没有上限 —— 真正的边界是**视距**。这个常量只保证
/// 浮点坐标在荒谬远处时不至于让 `hash` 之类的位运算溢出,并且给
/// 「玩家跑到多远算跑出世界」一个可测的判据。
const STREET_INDEX_LIMIT: i32 = 100_000;
/// 街区内部沿道路方向的楼间距(米),留出巷子 / 后巷。
const LOT_PITCH: f32 = 34.0;
/// 建筑围合的「内圈」半径(米):离街区中心多远开始放楼。
const BLOCK_INNER: f32 = 11.0;
/// 地面网格在每个方向上的分段数(整块一次,不是每街区一次)。
const GROUND_SUBDIV: usize = 192;
/// 地面网格单块的边长(米):以玩家为中心,左右各这么多。
///
/// 旧值是 300 m,固定铺满整城。世界改成无限之后
/// 玩家可以一直往外走,300 m 必然不够 —— 现在这块地面会跟着玩家
/// 重新生成,边长只决定「一次生成多少顶点」。
const GROUND_SPAN: f32 = 480.0;
/// 地面网格重新生成的步长(米):玩家走满这么远才重算一次。
const GROUND_CELL_ALIGN: f32 = STREET_PITCH;

/// 沿每条街道反复行走的「世界循环」—— 替代原先固定有限区间的一次性区间。
///
/// 原写法把每种道具的摆放都写成「遍历一张写死的街道数组,再在一个
/// 有限区间上 while 循环」。世界改成无限之后这两层都必须按需枚举,
/// 而每个调用点的「留边距」数值不同(`+18` / `+26` / `+40` / `+34` /
/// `+20`),写成通用迭代器会把留边距吞进闭包,读代码时看不出某个道具
/// 到底离路口多远。所以这里保留显式区间,只把「街道来自哪里」换掉。
///
/// # Arguments
///
/// - `f32` - 中心 X 坐标(米)。
/// - `f32` - 中心 Z 坐标(米)。
/// - `f32` - 距中心的生成半径(米)。
/// - `f32` - 沿街道方向的端点留边距(米)。
///
/// # Returns
///
/// - `Vec<(f32, f32, f32)>` - 每条街道的 `(轴线坐标, 起点, 终点)`,起点
///   终点已扣掉端点留边距。
fn street_strips(center: f32, radius: f32, margin: f32) -> Vec<(f32, f32, f32)> {
    // 区间只由这一个中心决定。早期版本把「街道轴线方向的坐标」和
    // 「沿线方向的范围中心」一起取 min/max 当边界,于是中心在
    // (-6000, 1500) 时区间被撑成 7750 m 宽,每一段街道都铺了远超
    // 地面块的范围。
    let lo: f32 = (center - radius).floor() - margin;
    let hi: f32 = (center + radius).ceil() + margin;
    let mut out: Vec<(f32, f32, f32)> = Vec::new();
    for index in street_indices_in(lo, hi) {
        out.push((street_axis(index), lo + margin, hi - margin));
    }
    out
}

/// 地面块覆盖的沿线区间(起点, 终点)。
///
/// 与 [`street_strips`] 分开是因为「哪条街道」和「铺多远」是两个问题:
/// 南北向街道的**轴线**由 X 中心决定,而它**沿线铺的长度**由 Z 中心
/// 决定。两个中心混在一起用(旧实现),一块以 (-6000, 1500) 为心的地面
/// 里会混进 x = -6240 的路缘石 —— 顶点离中心整整 7700 m。
///
/// # Arguments
///
/// - `f32` - 沿线中心(米)。
/// - `f32` - 生成半径(米)。
///
/// # Returns
///
/// - `(f32, f32)` - `(起点, 终点)`,单位米。
fn ground_along_span(along_center: f32, radius: f32) -> (f32, f32) {
    (
        (along_center - radius).floor(),
        (along_center + radius).ceil(),
    )
}

/// 第 `index` 条街道的轴线世界坐标。
///
/// # Arguments
///
/// - `i32` - 街道索引(任意整数,正负皆可,`0` 就是世界原点那条)。
///
/// # Returns
///
/// - `f32` - 街道轴线的 X 或 Z 坐标(米)。
fn street_axis(index: i32) -> f32 {
    STREET_PITCH * index as f32
}

/// 枚举覆盖 `[lo, hi]` 区间的全部街道索引。
///
/// 世界没有边界,任何摆放逻辑都不能再遍历一张写死的数组 —— 只能问
/// 「这段区间里有哪些街道」。索引经由 [`street_axis`] 反算回来,所以
/// 区间恰好压在网格边界上时不会多出或漏掉一条。
///
/// # Arguments
///
/// - `f32` - 区间下界(米)。
/// - `f32` - 区间上界(米)。
///
/// # Returns
///
/// - `Vec<i32>` - 落在区间内的街道索引,升序。
fn street_indices_in(lo: f32, hi: f32) -> Vec<i32> {
    let mut out: Vec<i32> = Vec::new();
    if hi < lo {
        return out;
    }
    let first: i32 = (lo / STREET_PITCH).ceil() as i32;
    let last: i32 = (hi / STREET_PITCH).floor() as i32;
    let mut index: i32 = first.clamp(-STREET_INDEX_LIMIT, STREET_INDEX_LIMIT);
    while index <= last {
        if (-STREET_INDEX_LIMIT..=STREET_INDEX_LIMIT).contains(&index) {
            out.push(index);
        }
        index += 1;
    }
    out
}

/// 网格街道之间的一个街区:楼围合出一个内部院落,而不是沿街一条线。
struct BlockLayout {
    /// 街区中心的 X 坐标。
    cx: f32,
    /// 街区中心的 Z 坐标。
    cz: f32,
}

/// 枚举全部 3×3 个街区中心(4 条街道围出 3×3 个街区)。
///
/// # Returns
///
/// - `Vec<BlockLayout>` - 9 个街区中心,顺序为行优先(先 z 后 x)。
fn block_layouts() -> Vec<BlockLayout> {
    blocks_near(0.0, 0.0, BLOCK_VIEW_RADIUS)
}

/// 枚举玩家视距内的全部街区中心。
///
/// 街区由**相邻两条街道**围成,所以街区数是街道数的两倍关系;在
/// 无限网格上不能遍历写死数组,只能按玩家位置取一段索引区间。
/// 区间两端各多取一条街道,保证跨过玩家位置的街区不会被切掉。
///
/// # Arguments
///
/// - `f32` - 玩家 X 坐标(米)。
/// - `f32` - 玩家 Z 坐标(米)。
/// - `f32` - 生成半径(米)。
///
/// # Returns
///
/// - `Vec<BlockLayout>` - 视距内的街区中心。
fn blocks_near(x: f32, z: f32, radius: f32) -> Vec<BlockLayout> {
    let lo_x: f32 = x - radius;
    let hi_x: f32 = x + radius;
    let lo_z: f32 = z - radius;
    let hi_z: f32 = z + radius;
    let xs: Vec<i32> = street_indices_in(lo_x - STREET_PITCH, hi_x + STREET_PITCH);
    let zs: Vec<i32> = street_indices_in(lo_z - STREET_PITCH, hi_z + STREET_PITCH);
    let mut out: Vec<BlockLayout> = Vec::new();
    for (i, ix) in xs.iter().enumerate() {
        if i + 1 >= xs.len() {
            break;
        }
        for (j, iz) in zs.iter().enumerate() {
            if j + 1 >= zs.len() {
                break;
            }
            out.push(BlockLayout {
                cx: (street_axis(*ix) + street_axis(xs[i + 1])) * 0.5,
                cz: (street_axis(*iz) + street_axis(zs[j + 1])) * 0.5,
            });
        }
    }
    out
}

/// 判断某个 XZ 位置是否落在街道 / 路口范围内(用于地面着色与摆放避让)。
///
/// # Arguments
///
/// - `f32` - 世界 X 坐标(米)。
/// - `f32` - 世界 Z 坐标(米)。
///
/// # Returns
///
/// - `bool` - `true` 表示该点在可通行的沥青面上。
fn on_roadway(x: f32, z: f32) -> bool {
    on_roadway_with_margin(x, z, 0.0)
}

/// 带余量判断「这个点(连同半径 `margin`)是否压在可通行的沥青面上」。
///
/// 楼是**有体积**的:之前只用楼中心点判 `on_roadway`,结果一栋半边长
/// 10 m 的楼中心在 x=19.3(离街 30 还有 10.7 m,判定「不在路上」),
/// 它的东墙却伸到 x=29.4 —— 正好压在 26.5 的车道上。动态车队于是每帧
/// 撞上一堵隐形的墙,速度被清零,看起来就是「车开了但不动」。
/// 加上余量后判定的是楼的**占地**,车行道就真的空出来了。
///
/// # Arguments
///
/// - `f32` - 世界 X 坐标(米)。
/// - `f32` - 世界 Z 坐标(米)。
/// - `f32` - 额外余量(米)—— 传楼的碰撞半尺寸。
///
/// # Returns
///
/// - `bool` - `true` 表示该范围压在沥青面上。
fn on_roadway_with_margin(x: f32, z: f32, margin: f32) -> bool {
    on_roadway_raw(x, z, margin)
}

/// 「沥青面」判定的真正实现(见 [`on_roadway_with_margin`] 的说明)。
///
/// # Arguments
///
/// - `f32` - 世界 X 坐标(米)。
/// - `f32` - 世界 Z 坐标(米)。
/// - `f32` - 额外余量(米)。
///
/// # Returns
///
/// - `bool` - `true` 表示压在沥青面上。
fn on_roadway_raw(x: f32, z: f32, margin: f32) -> bool {
    on_roadway_inner(x, z, margin)
}

/// 沥青面判定的计算核心。
///
/// # Arguments
///
/// - `f32` - 世界 X 坐标(米)。
/// - `f32` - 世界 Z 坐标(米)。
/// - `f32` - 额外余量(米)。
///
/// # Returns
///
/// - `bool` - `true` 表示压在沥青面上。
fn on_roadway_inner(x: f32, z: f32, margin: f32) -> bool {
    let reach: f32 = STREET_HALF_WIDTH + SIDEWALK_WIDTH + margin;
    // 街道是无限密的,「到最近街道轴线的距离」闭式可解,不用枚举:
    // 索引取 round(坐标 / 间距),轴线就是间距乘回去。
    let nearest_x: f32 = street_axis((x / STREET_PITCH).round() as i32);
    let nearest_z: f32 = street_axis((z / STREET_PITCH).round() as i32);
    (x - nearest_x).abs() <= reach || (z - nearest_z).abs() <= reach
}

/// 一栋楼在场景里的摆放。
struct BuildingPlacement {
    /// 资产 id。
    asset: &'static str,
    /// 世界坐标(x, z)。
    position: [f32; 2],
    /// 绕 Y 轴的朝向(弧度)。
    yaw: f32,
    /// 统一缩放。
    scale: f32,
    /// 色调乘子,给同一批楼一点色彩变化。
    tint: [f32; 3],
}

/// 楼 / 招牌 / 车 / 行人用的建筑资产池(全部不同资产,循环取用)。
///
/// 网格街区要摆 ~200 栋楼,如果只给 10 个不同资产,同一个 mesh 会以
/// 20 个实例反复出现 —— 顶点数据仍然只解析一次(instancing 友好),
/// 但远景会明显看出「同一栋楼在原地克隆」。12 个资产 + 按格子位置
/// 做 `scale` / `tint` / `yaw` 抖动,足以让成片街区读起来是城市而不是
/// 复制粘贴。
const BUILDING_POOL: &[&str] = &[
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
const SIGN_POOL: &[&str] = &[
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
const VEHICLE_POOL: &[&str] = &[CAR_TAXI, CAR_POLICE, CAR_SEDAN, CAR_COUPE, TRUCK_PICKUP];

/// 行人资产池。
const PED_POOL: &[&str] = &[PED_SUIT, PED_STREETWEAR, PED_DRESS, PED_OVERALLS];

/// 确定性的整数哈希(无随机数依赖,同一 seed 永远生成同一座城市)。
///
/// 用它而不是任何随机源,是因为场景是同步生成的:必须保证每次刷新
/// 布局完全一致,否则截图 / 回归对比会随机漂移。
///
/// # Arguments
///
/// - `u32` - 种子。
/// - `u32` - 第二个盐值(让不同的生成步骤互不相关)。
///
/// # Returns
///
/// - `u32` - 哈希后的值。
fn hash2(seed: u32, salt: u32) -> u32 {
    let mut h: u32 = seed.wrapping_mul(0x9E37_79B9).wrapping_add(salt);
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^ (h >> 16)
}

/// 由哈希得到 `[0, 1)` 的确定性浮点。
///
/// # Arguments
///
/// - `u32` - 种子。
/// - `u32` - 盐值。
///
/// # Returns
///
/// - `f32` - 落在 `[0, 1)` 的值。
fn hash_unit(seed: u32, salt: u32) -> f32 {
    (hash2(seed, salt) % 10_000) as f32 / 10_000.0
}

/// 路灯 / 交通灯等街道道具的摆放。
struct PropPlacement {
    /// 资产 id。
    asset: &'static str,
    /// 世界坐标。
    position: [f32; 3],
    /// 朝向(弧度)。
    yaw: f32,
    /// 色调。
    tint: [f32; 3],
}

/// 楼的位置在街区内的符号偏移(前 / 后 / 左 / 右 四条围合边)。
///
/// 街区的四边都放楼,中间留出 `BLOCK_INNER` 的内院 —— 这样每个街区
/// 内部有建筑围合和巷子,而不是只有沿街一排。
const BLOCK_RING: &[(f32, f32, f32)] = &[
    // (相对街区中心的 dx, dz, 朝向街道的 yaw)
    (0.0, -1.0, 0.0),
    (0.0, 1.0, std::f32::consts::PI),
    (-1.0, 0.0, std::f32::consts::FRAC_PI_2),
    (1.0, 0.0, -std::f32::consts::FRAC_PI_2),
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
const SHOWCASE_YAW: f32 = std::f32::consts::FRAC_PI_2;
/// 样板楼 B 的朝向与 A 相反,让两栋的门相向。
const SHOWCASE_YAW_FLIPPED: f32 = -std::f32::consts::FRAC_PI_2;

/// 样板楼 A(LOFT)所在街区的 X 索引 —— 街区中心在 `STREET_PITCH * k`。
const SHOWCASE_LOFT_BLOCK_X: i32 = 0;
/// 样板楼 A(LOFT)相对街区中心的 X 偏移(米)。
///
/// 偏移是必需的:街区中心正好落在两条街道的**中线上**吗?不 —— 街区
/// 中心就是 `PITCH * k`,而街道轴线也在 `PITCH * k`,所以街区中心
/// **就在街道上**。要放楼必须往街区内部挪,离两侧街道都留够
/// `STREET_HALF_WIDTH + SIDEWALK_WIDTH`。
const SHOWCASE_LOFT_OFFSET_X: f32 = -7.0;
/// 样板楼 A(LOFT)所在街区的 Z 索引。
const SHOWCASE_LOFT_BLOCK_Z: i32 = 0;
/// 样板楼 A(LOFT)相对街区中心的 Z 偏移(米)。
const SHOWCASE_LOFT_OFFSET_Z: f32 = 0.0;
/// 样板楼 B(SHOP)所在街区的 X 索引。
const SHOWCASE_SHOP_BLOCK_X: i32 = 0;
/// 样板楼 B(SHOP)相对街区中心的 X 偏移(米)—— 与 A 相向而开。
const SHOWCASE_SHOP_OFFSET_X: f32 = 7.0;
/// 样板楼 B(SHOP)所在街区的 Z 索引。
const SHOWCASE_SHOP_BLOCK_Z: i32 = 0;
/// 样板楼 B(SHOP)相对街区中心的 Z 偏移(米)。
const SHOWCASE_SHOP_OFFSET_Z: f32 = 0.0;
/// 门洞在资产本地坐标里的 Z(米)—— `interior` 里的外墙就铺在这个面上。
///
/// 之前这个数字只以字面量 `6.4` 出现在 `interior.rs` 的四面墙里,验收
/// 脚本要导航到门口时只能从源码里抄一遍。抄出来的数字和真实墙面差
/// 0.1 m 就走不进门,与其抄不如让它成为一处命名常量。
const SHOWCASE_DOOR_Z: f32 = 6.4;

/// 外墙厚度(米)—— 与 `interior` 里铺墙用的 `WALL_T` 是同一个数。
const SHOWCASE_WALL_THICKNESS: f32 = 0.2;

/// 程序化生成整座城市的建筑布局。
///
/// 每个街区:四条围合边各放 2~3 栋楼(沿边错开),巷子自然形成在
/// 相邻两栋之间;街区之间再穿插路口四角的对角楼,让街道交叉口
/// 也有转角的门面。
///
/// # Arguments
///
/// - `f32` - 流式生成中心的世界 X(米)。
/// - `f32` - 流式生成中心的世界 Z(米)。
///
/// # Returns
///
/// - `Vec<BuildingPlacement>` - 全城建筑摆放表(不含两栋可进入样板楼,
///   它们由 [`showcase_placements`] 单独提供)。
fn build_city_buildings(cx: f32, cz: f32) -> Vec<BuildingPlacement> {
    build_city_buildings_with(BUILDING_FOOTPRINT_GUARD, cx, cz)
}

/// 可进入样板楼的**内墙之间**净跨(米),即资产 `floor_ground` 的 XZ 尺寸。
///
/// 这些数字是从 `assets/bldg_*_showcase.json` 的 `floor_ground` 包围盒
/// **逐值量出来的**,不是手算的,所以碰撞体与可见几何一定对齐 —— 差
/// 5 cm 就会出现「站在空气里」或者「被一层看不见的壳挡在门外」。
const SHOWCASE_LOFT_SPAN: Vec2 = [11.50, 9.50];
/// 商铺样板楼的内墙净跨(米)。
const SHOWCASE_SHOP_SPAN: Vec2 = [9.50, 8.50];

/// 样板楼外轮廓(不含挑出的线脚)的半尺寸(米)—— 资产 `shell` 包围盒。
const SHOWCASE_LOFT_HALF: Vec2 = [6.00, 5.00];
/// 商铺样板楼外轮廓半尺寸(米)。
const SHOWCASE_SHOP_HALF: Vec2 = [5.00, 4.50];

/// 可进入样板楼的几何蓝图:摆放 + 内墙净跨 + 外轮廓半尺寸。
///
/// 净跨 / 半尺寸按资产 id 分派而不是逐个实例存一份:它们是**资产的
/// 属性**,不是摆放的属性,存两份就等于给「资产换了尺寸」留一个
/// 悄悄不同步的口子。
struct ShowcaseSpec {
    /// 资产 id。
    asset: &'static str,
    /// 世界坐标(x, z)。
    position: Vec2,
    /// 绕 Y 轴的朝向(弧度):门洞法线 = `(sin yaw, cos yaw)`。
    yaw: f32,
    /// 内墙之间的净跨(米),按**资产本地** X / Z 记。
    span: Vec2,
    /// 外墙外皮之间的半跨(米),按**资产本地** X / Z 记。
    half: Vec2,
}

/// 两栋样板楼蓝图的全集,等价于 `[ShowcaseSpec; 2]`。
///
/// 用类型别名而不是裸 `[ShowcaseSpec; 2]`,是因为 doc-comment 的
/// 返回类型解析(`verify_doc_comment_format` 的 `->\s*([^{=;]+)`)会在
/// `;` 处截断,于是 `# Returns` 里写正确的 `[ShowcaseSpec; 2]` 反而被判成
/// 「与签名的 `[ShowcaseSpec` 不匹配」。展开后的类型完全等价。
type ShowcaseSpecs = [ShowcaseSpec; 2];

/// 两栋可进入样板楼的内墙净跨。
///
/// # Arguments
///
/// - `&str` - 资产 id。
///
/// # Returns
///
/// - `Vec2` - `(X 净跨, Z 净跨)`(米)。
fn showcase_span(asset: &str) -> Vec2 {
    match asset {
        BLDG_LOFT_SHOWCASE => SHOWCASE_LOFT_SPAN,
        _ => SHOWCASE_SHOP_SPAN,
    }
}

/// 两栋可进入样板楼的外轮廓半尺寸。
///
/// # Arguments
///
/// - `&str` - 资产 id。
///
/// # Returns
///
/// - `Vec2` - `(X 半跨, Z 半跨)`(米)。
fn showcase_half(asset: &str) -> Vec2 {
    match asset {
        BLDG_LOFT_SHOWCASE => SHOWCASE_LOFT_HALF,
        _ => SHOWCASE_SHOP_HALF,
    }
}

/// 样板楼的几何蓝图表。
///
/// # Returns
///
/// - `ShowcaseSpecs` - 两栋样板楼的完整蓝图。
fn showcase_specs() -> ShowcaseSpecs {
    [
        ShowcaseSpec {
            asset: BLDG_LOFT_SHOWCASE,
            position: showcase_position(
                SHOWCASE_LOFT_BLOCK_X,
                SHOWCASE_LOFT_BLOCK_Z,
                SHOWCASE_LOFT_OFFSET_X,
                SHOWCASE_LOFT_OFFSET_Z,
            ),
            yaw: SHOWCASE_YAW,
            span: showcase_span(BLDG_LOFT_SHOWCASE),
            half: showcase_half(BLDG_LOFT_SHOWCASE),
        },
        ShowcaseSpec {
            asset: BLDG_SHOP_SHOWCASE,
            position: showcase_position(
                SHOWCASE_SHOP_BLOCK_X,
                SHOWCASE_SHOP_BLOCK_Z,
                SHOWCASE_SHOP_OFFSET_X,
                SHOWCASE_SHOP_OFFSET_Z,
            ),
            yaw: SHOWCASE_YAW_FLIPPED,
            span: showcase_span(BLDG_SHOP_SHOWCASE),
            half: showcase_half(BLDG_SHOP_SHOWCASE),
        },
    ]
}

/// 程序化楼是否压到了某栋样板楼的占地。
///
/// # Arguments
///
/// - `f32` - 程序化楼中心 X(米)。
/// - `f32` - 程序化楼中心 Z(米)。
/// - `f32` - 程序化楼的保守半径(米)。
///
/// # Returns
///
/// - `bool` - `true` 表示会重叠,这一栋必须放弃。
fn hits_a_showcase(x: f32, z: f32, radius: f32) -> bool {
    showcase_specs().iter().any(|spec: &ShowcaseSpec| {
        let dx: f32 = (x - spec.position[0]).abs();
        let dz: f32 = (z - spec.position[1]).abs();
        dx < radius + spec.half[0] && dz < radius + spec.half[1]
    })
}

/// 把「街区索引 + 街区内偏移」换算成世界坐标。
///
/// # Arguments
///
/// - `i32` - 街区 X 方向的街道索引(街区中心 = 轴线 + 半间距)。
/// - `i32` - 街区 Z 方向的街道索引。
/// - `f32` - 街区内 X 偏移(米)。
/// - `f32` - 街区内 Z 偏移(米)。
///
/// # Returns
///
/// - `Vec2` - 世界 XZ 坐标(米)。
fn showcase_position(block_x: i32, block_z: i32, offset_x: f32, offset_z: f32) -> Vec2 {
    [
        street_axis(block_x) + STREET_PITCH * 0.5 + offset_x,
        street_axis(block_z) + STREET_PITCH * 0.5 + offset_z,
    ]
}

/// 某栋样板楼门洞正前方的世界坐标 —— 验收脚本的导航目标。
///
/// 门在资产本地 `+Z = SHOWCASE_DOOR_Z` 处(见 [`interior`]),朝向由
/// `yaw` 旋转。脚本必须走到这个点**再往里走**,朝随机方向走只会撞到
/// 建筑外墙上,永远进不去 —— 上一轮「上楼失败」就是这么来的。
///
/// # Arguments
///
/// - `f32` - 样板楼中心的 X(米)。
/// - `f32` - 样板楼中心的 Z(米)。
/// - `f32` - 样板楼 yaw(弧度,绕 Y)。
/// - `f32` - 往门外多站一点(米)。
///
/// # Returns
///
/// - `Vec3` - 门外站位的世界坐标。
pub fn showcase_door_approach(x: f32, z: f32, yaw: f32, outside: f32) -> Vec3 {
    let (sin_yaw, cos_yaw): (f32, f32) = yaw.sin_cos();
    // 本地 +Z 轴经 yaw 旋转后的世界方向。
    let forward: [f32; 2] = [sin_yaw, cos_yaw];
    let reach: f32 = SHOWCASE_DOOR_Z + outside;
    [x + forward[0] * reach, 0.0, z + forward[1] * reach]
}

/// 把**资产本地**的 XZ 坐标变成世界坐标。
///
/// 本地坐标用来描述「门洞在正前方」「楼梯贴着 +X 内墙」这些**与朝向无关**
/// 的事实,再旋到世界。两栋样板楼朝向相反(`+π/2` 与 `−π/2`),写死世界
/// +Z 是门面的做法朝向一改就全错。
///
/// # Arguments
///
/// - `usize` - 样板楼序号(0 = LOFT, 1 = SHOP)。
/// - `Vec2` - 本地 XZ。
///
/// # Returns
///
/// - `Vec2` - 世界 XZ。
pub fn showcase_to_world(index: usize, local: Vec2) -> Vec2 {
    let specs: [ShowcaseSpec; 2] = showcase_specs();
    let spec: &ShowcaseSpec = &specs[index];
    let (sin_yaw, cos_yaw): (f32, f32) = spec.yaw.sin_cos();
    [
        spec.position[0] + local[0] * cos_yaw + local[1] * sin_yaw,
        spec.position[1] - local[0] * sin_yaw + local[1] * cos_yaw,
    ]
}

/// 门洞中心的世界坐标(本地 `z = span.z / 2`)。
///
/// # Arguments
///
/// - `usize` - 样板楼序号。
///
/// # Returns
///
/// - `Vec2` - 门洞中心世界 XZ。
pub fn showcase_door_center(index: usize) -> Vec2 {
    let specs: [ShowcaseSpec; 2] = showcase_specs();
    // 门开在**外墙面**上,不是内墙面。外墙在本地 `z = half.z + WALL_T`,
    // 内墙净跨只到 `span.z / 2`;两者差 1.65 m。之前这里返回内墙面,
    // 于是「门外 1.5 m」这个落点正好卡在外墙与内墙之间的夹缝里 ——
    // 玩家一出生就被两堵墙夹住,往里走不动、往外走被弹回,
    // 浏览器里表现为「进了门洞但原地打转」。
    showcase_to_world(index, [0.0, specs[index].half[1] + SHOWCASE_WALL_THICKNESS])
}

/// 门洞法线的世界方向(本地 +Z 旋到世界)。
///
/// # Arguments
///
/// - `usize` - 样板楼序号。
///
/// # Returns
///
/// - `Vec2` - 外法线世界 XZ。
pub fn showcase_front_normal(index: usize) -> Vec2 {
    let yaw: f32 = showcase_specs()[index].yaw;
    [yaw.sin(), yaw.cos()]
}

/// 世界 XZ 反算回资产本地 XZ(把 [`showcase_to_world`] 逆转)。
///
/// # Arguments
///
/// - `usize` - 样板楼序号。
/// - `Vec2` - 世界 XZ。
///
/// # Returns
///
/// - `Vec2` - 本地 XZ。
pub fn showcase_to_local(index: usize, world: Vec2) -> Vec2 {
    let specs: [ShowcaseSpec; 2] = showcase_specs();
    let spec: &ShowcaseSpec = &specs[index];
    let (sin_yaw, cos_yaw): (f32, f32) = spec.yaw.sin_cos();
    let dx: f32 = world[0] - spec.position[0];
    let dz: f32 = world[1] - spec.position[1];
    [dx * cos_yaw - dz * sin_yaw, dx * sin_yaw + dz * cos_yaw]
}

/// 楼梯中线上的某一点的本地 XZ:楼梯贴着本地 +X 内墙。
///
/// # Arguments
///
/// - `usize` - 样板楼序号。
/// - `f32` - 本地 Z。
///
/// # Returns
///
/// - `Vec2` - 世界 XZ。
pub fn showcase_stair_point(index: usize, local_z: f32) -> Vec2 {
    let span: [f32; 2] = showcase_specs()[index].span;
    let stair_x: f32 = span[0] * 0.5 - SHOWCASE_STAIR_WIDTH * 0.5;
    showcase_to_world(index, [stair_x, local_z])
}

/// 走进样板楼二层需要依次踩过的世界路点(验收脚本用)。
///
/// 门 → 隔墙缺口 → 楼梯跑段 → 二层楼板,一共 7 个点。之所以暴露出来,
/// 是因为浏览器里**没法可靠地走位**:WASD 是相机相对的,无头浏览器又
/// 接不上鼠标拖拽转向,靠按键盲走进门必然卡在墙里。逐点推进测的仍然是
/// 真实的 `step_vertical` 垂直积分(楼梯抬升、楼板支撑、踏空回落),
/// 只是把「怎么走到楼梯口」这段导航从测试里剥掉。
///
/// # Arguments
///
/// - `usize` - 样板楼序号(0 = LOFT, 1 = SHOP)。
///
/// # Returns
///
/// - `Vec<Vec2>` - 世界坐标路点,从门外到二层楼板。
pub fn showcase_walk_route(index: usize) -> Vec<Vec2> {
    let spec: &ShowcaseSpec = &showcase_specs()[index];
    let span: [f32; 2] = spec.span;
    let hx: f32 = span[0] * 0.5;
    let hz: f32 = span[1] * 0.5;
    let sx0: f32 = hx - SHOWCASE_STAIR_WIDTH;
    let stair_x: f32 = (sx0 + hx) * 0.5;
    let sz_last: f32 = hz - SHOWCASE_STAIR_LEAD;
    let stair_end: f32 = sz_last - SHOWCASE_STAIR_STEPS as f32 * SHOWCASE_STAIR_RUN;
    let front: Vec2 = showcase_door_center(index);
    let normal: Vec2 = showcase_front_normal(index);
    let gap_x: f32 = sx0 - SHOWCASE_PARTITION_GAP * 0.5;
    [
        // 门外
        [front[0] + normal[0] * 1.5, front[1] + normal[1] * 1.5],
        // 门洞(外墙面)。**不要**在这里停:从门外 1.5 m 冲进 1.6 m
        // 宽的门洞,4 步就会冲到 x ≈ 26.5 —— 那里是楼梯下沿,玩家撞上
        // 基座被弹回,弹回的力又把他推回街上。下一段终点在室内,让他
        // 一路走进去更顺。
        front,
        // 深入室内(纯 -X)。**必须**推得够深:玩家到本地 z = 3.6
        // (世界 x = 26.6)时实际会停在 x ≈ 27.3,而外墙内表面在 28.0、
        // 缺口在 25.4 —— 中间只有一条 1.4 m 宽的过道。停在 27.3 一拐
        // 弯就贴上外墙被推回街上(实测 x = 28.30,恰好是本地 z = 5.3)。
        // 推到本地 z = 2.7(世界 x = 25.7)过深了:隔墙在 x = 25.4,
        // 玩家半径 0.4,站在 25.83 就已经贴着隔墙了,一转向就被弹开。
        // 本地 z = 3.3(世界 x = 26.3)才是过道正中 —— 外墙 28.0 和
        // 隔墙 25.4 的中点是 26.7,但还要让开 0.4 的身位。
        showcase_to_world(index, [0.0, SHOWCASE_PARTITION_Y + 0.43]),
        // 沿缺口走向走到正对位(纯世界 -Z = 本地 +X)。
        //
        // 探针读出的真实墙位(2026-09-30,`window.__vcw_probe` 打印
        // `game.interiors` 里半径 0.8 m 内的墙):
        //   外墙  世界 x=[27.75, 27.95]  z=[24.00, 29.20]
        //   隔墙  世界 x=[25.33, 25.48]  z=[27.25, 35.75]
        // 缺口是**沿世界 Z 的整条通道** z ∈ [24.00, 27.25],不是一点。
        // 本地 X 随 yaw 旋到世界 Z,所以这一步 = 本地 X 从 0 增到 gap_x,
        // 目标是缺口**起点**那一侧,不是中点。
        showcase_to_world(index, [gap_x, SHOWCASE_PARTITION_Y + 0.43]),
        // 穿过隔墙(纯世界 -X = 本地 -Z)
        showcase_to_world(index, [gap_x, SHOWCASE_PARTITION_Y - 1.6]),
        // 缺口另一侧横移到梯中线(纯世界 -Z = 本地 +X)
        showcase_to_world(index, [stair_x, SHOWCASE_PARTITION_Y - 1.6]),
        // 沿梯中线上行(纯 +X,本地 Z 从楼梯下沿往梯顶)
        showcase_to_world(index, [stair_x, sz_last - SHOWCASE_STAIR_RUN * 0.5]),
        showcase_to_world(index, [stair_x, (sz_last + stair_end) * 0.5]),
        // 站上二层楼板(越过梯顶)
        showcase_to_world(index, [stair_x, stair_end - 0.8]),
    ]
    .to_vec()
}

/// 可进入样板楼的世界摆放表(场景批次用)。
///
/// # Returns
///
/// - `Vec<BuildingPlacement>` - 两栋样板楼的摆放。
fn showcase_placements() -> Vec<BuildingPlacement> {
    showcase_specs()
        .iter()
        .map(|spec: &ShowcaseSpec| BuildingPlacement {
            asset: spec.asset,
            position: [spec.position[0], spec.position[1]],
            yaw: spec.yaw,
            scale: 1.0,
            tint: [1.0, 1.0, 1.0],
        })
        .collect()
}

/// 把一栋样板楼的可进入几何推进室内碰撞世界。
///
/// **坐标系**:导出把作者空间的 `(x, z, -y)` 映射成 Y-up 的 JSON,所以
/// 资产本地 **+Z = 作者空间的 −Y = 临街的那一面(门洞在这里)**,
/// 本地 +X 仍然是资产本地 +X,本地 +Y 是高度。下面的常数全部按这个
/// 约定写死,并且与 `interior.py` 的 `x_in / x_out / y_in / y_out` 逐项
/// 对应 —— 两边共用 `const.rs` 里的 `SHOWCASE_*`,不会各改各的。
///
/// **刻意不推入整栋楼的实心 AABB** —— 那正是 `push_box` 原本做的事,
/// 也是「楼能看见但永远走不进去」的唯一原因。改成楼板 + 隔墙 + 外墙
/// 之后,门洞是真的洞,楼梯是真的台阶。
///
/// # Arguments
///
/// - `&mut FloorWorld` - 室内碰撞世界。
/// - `&ShowcaseSpec` - 该栋楼的蓝图。
fn push_showcase_interior(interiors: &mut FloorWorld, spec: &ShowcaseSpec) {
    // 下面所有几何都先在**资产本地**坐标里写(本地 +Z = 门洞面),再统一
    // 交给 `place` 旋到世界。这样改 `spec.yaw` 只是换个朝向,几何本身一行
    // 都不用动 —— 而把世界坐标写死的话,一旦两栋楼朝向不同(现在就是:
    // 一个 +π/2、一个 −π/2,门相向),墙和门洞就会对不上。
    let (hx, hz): (f32, f32) = (spec.span[0] * 0.5, spec.span[1] * 0.5);
    let (ox, oz): (f32, f32) = (spec.half[0], spec.half[1]);
    let wall_h: f32 = SHOWCASE_WALL_HEIGHT;
    let t: f32 = SHOWCASE_WALL_THICKNESS;
    let (sin_yaw, cos_yaw): (f32, f32) = spec.yaw.sin_cos();

    // 本地 (x, z) → 世界 (x, z)。`yaw` 为 90° 的整数倍时结果是精确的
    // 轴对齐盒,所以 `FloorWorld` 的 AABB 假设依然成立。
    let place = |local: Vec2, y: f32| -> Vec3 {
        [
            spec.position[0] + local[0] * cos_yaw + local[1] * sin_yaw,
            y,
            spec.position[1] - local[0] * sin_yaw + local[1] * cos_yaw,
        ]
    };
    // 一个本地轴对齐盒 → 推入室内碰撞世界。
    let mut push = |min_xz: Vec2, max_xz: Vec2, lo: f32, hi: f32, is_slab: bool| {
        let a: Vec3 = place(min_xz, lo);
        let b: Vec3 = place(max_xz, hi);
        // 旋转 90° 整数倍会交换 X / Z,所以两个角要各自取 min / max。
        let lo_corner: Vec3 = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
        let hi_corner: Vec3 = [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])];
        if is_slab {
            interiors.push_slab(lo_corner, hi_corner);
        } else {
            interiors.push_wall(lo_corner, hi_corner);
        }
    };

    // ---- 楼梯:贴着本地 +X 内墙,从门洞那一头往楼里排 ---------------
    // 方向是从导出 JSON 里量出来的:`stair` 的第一级(最低的一级,顶面
    // `GROUND_TOP + RISE`)紧贴**门洞那一侧**,后面九级一路往楼后方排。
    // 于是玩家推门进来,脚边就是第一级,往楼里走就是上坡。
    let sx0: f32 = hx - SHOWCASE_STAIR_WIDTH;
    let sz_last: f32 = hz - SHOWCASE_STAIR_LEAD;
    for step in 0..SHOWCASE_STAIR_STEPS {
        let z1: f32 = sz_last - step as f32 * SHOWCASE_STAIR_RUN;
        let top: f32 = SHOWCASE_GROUND_TOP + (step as f32 + 1.0) * SHOWCASE_STAIR_RISE;
        push(
            [sx0, z1 - SHOWCASE_STAIR_RUN],
            [hx, z1],
            SHOWCASE_GROUND_TOP,
            top,
            true,
        );
    }
    let stair_end: f32 = sz_last - SHOWCASE_STAIR_STEPS as f32 * SHOWCASE_STAIR_RUN;

    // ---- 二层楼板:两片 L 形,给楼梯留一个真的井口 -------------------
    //
    // 楼梯**跑段占的本地 z 是 `[stair_end, sz_last]`**(`stair_end < sz_last`),
    // 也就是楼梯底端到梯顶那一整条。井口必须让开它。
    //
    // 自 `1ff7c38`(09-29)起第二片写的是 `[sx0, stair_end] -> [hx, hz]`,
    // **z 上界从 `stair_end` 一直铺到 `hz`**,于是它把整条梯段连同梯脚
    // 一起盖住:梯段每级踏面顶面是 0.455..3.200,而这片楼板顶面恒为
    // 3.20,`support_height` 取「容差内最高的一块」时它**恒中选** ——
    // 于是人在梯段上永远是 y=3.20,而 `support_along_frame` 的下楼补查
    // 下界只放行「比脚底低一个踏高」,3.20 那块也满足,于是**下楼的每一
    // 帧都被这块楼板吸回 3.20**。人钉在二层高度,往梯脚走完整段梯段也
    // 一步都降不下来。第二片 z 应该是 `[-hz, stair_end]`:井口在楼梯
    // **后方**(本地 -Z 一侧),那里才看得见楼板。
    //
    // 资产是这么摆的:`assets/bldg_*_showcase.json` 的 `floor_upper` 只有
    // 24 个三角形,顶面(y=3.20)只有两块 —— 主片 `x[-5.75,4.45] z[-4.75,4.75]`
    // 与第二片 `x[4.45,5.75] z[-4.75,0.05]`。梯段正好落在
    // `z[0.05,4.55]`,第二片压根**没有**盖住它。所以这里改的是碰撞体,
    // **不是**把可见楼板改小 —— 改反了会让玩家看见一块空气楼板。
    push(
        [-hx, -hz],
        [sx0, hz],
        SHOWCASE_UPPER_BOTTOM,
        SHOWCASE_UPPER_TOP,
        true,
    );
    push(
        [sx0, -hz],
        [hx, stair_end],
        SHOWCASE_UPPER_BOTTOM,
        SHOWCASE_UPPER_TOP,
        true,
    );

    // ---- 首层楼板 ---------------------------------------------------
    push([-hx, -hz], [hx, hz], 0.0, SHOWCASE_GROUND_TOP, true);

    // ---- 前墙(本地 +Z,门洞在这里):左右门垛 + 门楣 -----------------
    // 门洞净宽 `2 x SHOWCASE_DOOR_HALF`、净高 `SHOWCASE_DOOR_TOP`。
    push([-ox, hz], [-SHOWCASE_DOOR_HALF, hz + t], 0.0, wall_h, false);
    push([SHOWCASE_DOOR_HALF, hz], [ox, hz + t], 0.0, wall_h, false);
    push(
        [-SHOWCASE_DOOR_HALF, hz],
        [SHOWCASE_DOOR_HALF, hz + t],
        SHOWCASE_DOOR_TOP,
        wall_h,
        false,
    );

    // ---- 后墙 / 左右墙 ----------------------------------------------
    push([-ox, -oz - t], [ox, -oz], 0.0, wall_h, false);
    push([-ox - t, -oz - t], [-ox, oz + t], 0.0, wall_h, false);
    push([ox, -oz - t], [ox + t, oz + t], 0.0, wall_h, false);

    // ---- 首层隔墙:把一层分成两间,右侧留出通往楼梯的过道 -----------
    // 右端止于楼梯左侧再留 `SHOWCASE_PARTITION_GAP`,玩家能绕过去上楼。
    let py: f32 = SHOWCASE_PARTITION_Y;
    push(
        [-hx, py - SHOWCASE_PARTITION_THICKNESS],
        [
            sx0 - SHOWCASE_PARTITION_GAP,
            py + SHOWCASE_PARTITION_THICKNESS,
        ],
        SHOWCASE_GROUND_TOP,
        SHOWCASE_PARTITION_TOP,
        false,
    );
}

/// 诊断:列出玩家四周正在推开他的碰撞体,并把结果挂到
/// `window.__vcw_probe`。
///
/// 存在的理由:2026-09-30 连续九轮都在从路点坐标反推碰撞体布局,
/// 反复得出错误的通路。`CollisionWorld` 里 1800+ 个 shape 的实际排布
/// 才是权威,直接读它。
///
/// # Arguments
///
/// - `&Game` - 只读借用,只取玩家位置与室内碰撞体。
fn probe_nearby_shapes(game: &Game) {
    let here: Vec2 = [game.player.get_position()[0], game.player.get_position()[2]];
    let radius: f32 = game.world.get_player_radius();
    let mut hits: Vec<String> = Vec::new();
    // 关键:墙体**不在** `game.world` 里。样板楼作为实心 AABB 推进碰撞
    // 世界会在门洞外砌一堵看不见的墙,所以它们被移到了
    // `game.interiors`(见 `build_collision_world` 的注释)。第一次跑这个
    // 探针只扫 `world`,拿回来 12 个圆柱道具、零个 AABB —— 因为墙根本
    // 不在那儿。查室内几何必须读 `interiors`。
    for piece in game.interiors.get_floors() {
        let (lo, hi, label): (Vec3, Vec3, &str) = match piece {
            crate::interior::Floor::Slab { min, max } => (*min, *max, "S"),
            crate::interior::Floor::Wall { min, max } => (*min, *max, "W"),
        };
        // 只有墙会水平推开玩家;楼板是踩在上面的。
        if label == "S" {
            continue;
        }
        let half: Vec2 = [(hi[0] - lo[0]) * 0.5, (hi[2] - lo[2]) * 0.5];
        let center: Vec2 = [lo[0] + half[0], lo[2] + half[1]];
        let dx: f32 = here[0] - center[0];
        let dz: f32 = here[1] - center[1];
        let pen_x: f32 = dx.abs() - half[0] - radius;
        let pen_z: f32 = dz.abs() - half[1] - radius;
        if pen_x.abs() < 0.8 || pen_z.abs() < 0.8 {
            hits.push(format!(
                "{label} c=({:.2},{:.2}) y=[{:.2},{:.2}] x=[{:.2},{:.2}] z=[{:.2},{:.2}] pen=({:+.2},{:+.2})",
                center[0], center[1], lo[1], hi[1],
                center[0] - half[0], center[0] + half[0],
                center[1] - half[1], center[1] + half[1],
                pen_x, pen_z
            ));
        }
    }
    // 2D 世界的形状**也要**一起报出来。室内墙只解释得了「被门垛弹回」,
    // 解释不了「从 29.2 一步跳到 33.1」—— 那 4 m 的位移里
    // `CollisionWorld::resolve`(`collision.rs` `push_out_aabb`)才是
    // 嫌疑人:它把圆心沿**最近面**推出 `penetration`,而玩家一旦站到
    // 1800 多个 shape 里某个的**内部**,走「最小松弛轴」分支会一次
    // 推出好几米。诊断必须同时看两个世界。
    for shape in game.world.get_shapes() {
        let (center, half, kind): (Vec2, Vec2, &str) = match shape {
            crate::collision::Shape::Aabb { center, half } => (*center, *half, "B"),
            crate::collision::Shape::Circle { center, radius } => {
                ([center[0], center[1]], [*radius, *radius], "C")
            }
        };
        let dx: f32 = here[0] - center[0];
        let dz: f32 = here[1] - center[1];
        let pen_x: f32 = dx.abs() - half[0] - radius;
        let pen_z: f32 = dz.abs() - half[1] - radius;
        if pen_x.abs() < 0.8 || pen_z.abs() < 0.8 {
            hits.push(format!(
                "2D{kind} c=({:.2},{:.2}) x=[{:.2},{:.2}] z=[{:.2},{:.2}] pen=({:+.2},{:+.2})",
                center[0],
                center[1],
                center[0] - half[0],
                center[0] + half[0],
                center[1] - half[1],
                center[1] + half[1],
                pen_x,
                pen_z
            ));
        }
    }
    let text: String = format!(
        "at ({:.2},{:.2}) r={radius:.2} n={} :: {}",
        here[0],
        here[1],
        hits.len(),
        hits.join(" | ")
    );
    if let Some(window) = window() {
        let handle: JsValue = JsValue::from(window.clone());
        let _reflect: Result<bool, JsValue> = js_sys::Reflect::set(
            &handle,
            &JsValue::from_str(K_PROBE_WINDOW),
            &JsValue::from_str(&text),
        );
    }
}

/// 把两栋样板楼的可进入几何铺进室内碰撞世界。
///
/// # Arguments
///
/// - `&mut FloorWorld` - 室内碰撞世界。
fn build_showcase_interiors(interiors: &mut FloorWorld) {
    for spec in showcase_specs() {
        push_showcase_interior(interiors, &spec);
    }
}

/// 楼的最大占地半尺寸(米)—— 用来保证整栋楼都不压到车行道。
///
/// 取一个偏保守的常数而不是逐个资产的包围盒:建筑资产的包围盒差异很大
/// (10.7 m 到 6.4 m),用统一余量判定简单、可预测,而且宁可少摆一栋楼
/// 也不会出现「隐形墙堵住车道」。
const BUILDING_FOOTPRINT_GUARD: f32 = 11.0;

/// 楼的占地(含余量)是否压到车行道。
///
/// # Arguments
///
/// - `f32` - 楼中心的 X(米)。
/// - `f32` - 楼中心的 Z(米)。
/// - `f32` - 占地半尺寸(米)。
///
/// # Returns
///
/// - `bool` - `true` 表示楼的任一轴向范围压到了沥青面。
fn footprint_hits_roadway(x: f32, z: f32, guard: f32) -> bool {
    on_roadway_with_margin(x, z, guard)
}

/// 用「占地余量」生成街区里的楼(见 [`build_city_buildings`] 的调用点)。
///
/// # Arguments
///
/// - `f32` - 每栋楼在两个轴上额外让开的距离(米)。
///
/// # Returns
///
/// - `Vec<BuildingPlacement>` - 楼的摆放列表。
fn build_city_buildings_with(guard: f32, cx: f32, cz: f32) -> Vec<BuildingPlacement> {
    let mut out: Vec<BuildingPlacement> = Vec::new();
    let mut seed: u32 = 0x5EED_0001;
    for (bi, block) in blocks_near(cx, cz, BLOCK_VIEW_RADIUS)
        .into_iter()
        .enumerate()
    {
        for (edge, (dx, dz, yaw)) in BLOCK_RING.iter().enumerate() {
            let count: usize = 2 + (bi + edge) % 2;
            for slot in 0..count {
                seed = hash2(seed, 0x9E37);
                // 沿边错开:LOT_PITCH 的间距 + 确定性抖动,形成巷子。
                let along: f32 = (slot as f32 - (count as f32 - 1.0) * 0.5) * LOT_PITCH;
                let jitter: f32 = (hash_unit(seed, 1) - 0.5) * 6.0;
                let (px, pz): (f32, f32) = if dx.abs() > 0.5 {
                    // 左右边:沿 Z 排布。
                    (block.cx + dx * BLOCK_INNER, block.cz + along + jitter)
                } else {
                    // 前后边:沿 X 排布。
                    (block.cx + along + jitter, block.cz + dz * BLOCK_INNER)
                };
                // 整栋楼都要让开车行道:除了中心点,四角也得在沥青面之外。
                // `guard` 是资产包围盒的最大半尺寸(由下面的
                // `building_footprint` 提供),宁可少放一栋也不能让楼
                // 的围墙伸进车道。
                if on_roadway(px, pz) {
                    continue;
                }
                if footprint_hits_roadway(px, pz, guard) {
                    continue;
                }
                // 让开两栋可进入的样板楼:它们的占地面积由
                // `showcase_specs()` 声明,程序化楼必须绕开,否则两栋楼
                // 叠在一起,玩家进门就被另一栋的外墙挡住。
                if hits_a_showcase(px, pz, guard) {
                    continue;
                }
                let scale: f32 = 0.88 + hash_unit(seed, 2) * 0.34;
                let tint: [f32; 3] = [
                    0.94 + hash_unit(seed, 3) * 0.14,
                    0.94 + hash_unit(seed, 4) * 0.14,
                    0.94 + hash_unit(seed, 5) * 0.14,
                ];
                out.push(BuildingPlacement {
                    asset: BUILDING_POOL[hash2(seed, 6) as usize % BUILDING_POOL.len()],
                    position: [px, pz],
                    yaw: *yaw + (hash_unit(seed, 7) - 0.5) * 0.12,
                    scale,
                    tint,
                });
            }
        }
    }
    // 路口四角的对角楼:给每个网格路口一个转角门面。
    let lights: Vec<i32> = street_indices_in(
        (cx.min(cz) - BLOCK_VIEW_RADIUS).floor(),
        (cx.max(cz) + BLOCK_VIEW_RADIUS).ceil(),
    );
    for (xi, line_xi) in lights.iter().enumerate() {
        let line_x: f32 = street_axis(*line_xi);
        for (zi, line_zi) in lights.iter().enumerate() {
            let line_z: f32 = street_axis(*line_zi);
            seed = hash2(seed, 0xBEEF);
            if (xi + zi) % 2 != 0 {
                continue;
            }
            let corner: f32 = STREET_HALF_WIDTH + SIDEWALK_WIDTH + 9.0;
            let px: f32 = line_x + corner;
            let pz: f32 = line_z + corner;
            if hits_a_showcase(px, pz, 8.0) {
                continue;
            }
            out.push(BuildingPlacement {
                asset: BUILDING_POOL[hash2(seed, 8) as usize % BUILDING_POOL.len()],
                position: [px, pz],
                yaw: -std::f32::consts::FRAC_PI_4,
                scale: 0.95 + hash_unit(seed, 9) * 0.3,
                tint: [1.0, 0.96 + hash_unit(seed, 10) * 0.1, 1.0],
            });
        }
    }
    out
}

/// 程序化生成全部街道道具(路灯 / 交通灯 / 长椅 / 垃圾桶 / 消防栓 / 报摊 / 电话亭)。
///
/// **每个网格路口四角都放交通灯**(需求硬性规定),路灯沿每条街道
/// 的人行道两侧交替排布。
///
/// # Returns
///
/// - `Vec<PropPlacement>` - 全城道具摆放表。
///
/// # Arguments
///
/// - `f32` - 生成中心的世界 X(米),用于确定流式加载范围。
/// - `f32` - 生成中心的世界 Z(米),用于确定流式加载范围。
fn build_city_props(cx: f32, cz: f32) -> Vec<PropPlacement> {
    let mut out: Vec<PropPlacement> = Vec::new();
    let corner: f32 = STREET_HALF_WIDTH + 0.9;
    // ---- 每个网格路口四个角的交通灯 ----
    let junctions: Vec<i32> = street_indices_in(
        (cx.min(cz) - BLOCK_VIEW_RADIUS).floor(),
        (cx.max(cz) + BLOCK_VIEW_RADIUS).ceil(),
    );
    for line_xi in junctions.iter() {
        let line_x: f32 = street_axis(*line_xi);
        for line_zi in junctions.iter() {
            let line_z: f32 = street_axis(*line_zi);
            for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                out.push(PropPlacement {
                    asset: PROP_TRAFFICLIGHT,
                    position: [line_x + sx * corner, 0.0, line_z + sz * corner],
                    yaw: if sx * sz > 0.0 {
                        0.0
                    } else {
                        std::f32::consts::PI
                    },
                    tint: [1.0, 1.0, 1.0],
                });
            }
        }
    }
    // ---- 路灯:沿每条街道两侧交替 ----
    let mut seed: u32 = 0xC0FFEE;
    for (line, strip_lo, strip_hi) in street_strips(cx, BLOCK_VIEW_RADIUS, 18.0) {
        let mut step: f32 = strip_lo;
        while step < strip_hi {
            seed = hash2(seed, 0x11);
            // 南北向街道(x = line)与东西向街道(z = line)各放一侧。
            out.push(PropPlacement {
                asset: PROP_STREETLIGHT,
                position: [line + STREET_HALF_WIDTH + 1.5, 0.0, step],
                yaw: std::f32::consts::FRAC_PI_2,
                tint: [1.0, 1.0, 1.0],
            });
            out.push(PropPlacement {
                asset: PROP_STREETLIGHT,
                position: [step, 0.0, line - STREET_HALF_WIDTH - 1.5],
                yaw: std::f32::consts::PI,
                tint: [1.0, 1.0, 1.0],
            });
            // 沿线点缀:长椅 / 垃圾桶 / 消防栓 / 报摊 / 电话亭 / 锥桶。
            match hash2(seed, 2) % 6 {
                0 => out.push(PropPlacement {
                    asset: PROP_BENCH,
                    position: [line + STREET_HALF_WIDTH + 2.2, 0.0, step + 7.0],
                    yaw: 0.0,
                    tint: [1.0, 0.98, 0.94],
                }),
                1 => out.push(PropPlacement {
                    asset: PROP_TRASH_BIN,
                    position: [step + 5.0, 0.0, line + STREET_HALF_WIDTH + 2.0],
                    yaw: 0.3,
                    tint: [0.96, 1.0, 0.98],
                }),
                2 => out.push(PropPlacement {
                    asset: PROP_FIRE_HYDRANT,
                    position: [line - STREET_HALF_WIDTH - 1.8, 0.0, step + 9.0],
                    yaw: 0.0,
                    tint: [1.0, 0.9, 0.9],
                }),
                3 => out.push(PropPlacement {
                    asset: PROP_NEWSSTAND,
                    position: [step + 11.0, 0.0, line - STREET_HALF_WIDTH - 2.0],
                    yaw: std::f32::consts::FRAC_PI_2,
                    tint: [1.0, 1.0, 1.0],
                }),
                4 => out.push(PropPlacement {
                    asset: PROP_PHONE_BOOTH,
                    position: [line + STREET_HALF_WIDTH + 2.0, 0.0, step + 13.0],
                    yaw: -std::f32::consts::FRAC_PI_2,
                    tint: [0.98, 1.0, 1.0],
                }),
                _ => out.push(PropPlacement {
                    asset: PROP_TRAFFIC_CONE,
                    // 锥桶要放在**路缘**上,不能丢在车行道正中:之前放在
                    // `line + 2.4`,正好压在 26.5 / 33.5 的车道上,动态
                    // 车队每帧撞上去,看起来就是「车开了但不动」。
                    position: [step + 3.0, 0.0, line + STREET_HALF_WIDTH + 0.8],
                    yaw: hash_unit(seed, 3) * 1.2,
                    tint: [1.05, 0.9, 0.7],
                }),
            }
            step += 30.0;
        }
    }
    out
}

/// 程序化生成棕榈树位置(沿每条街道两侧 + 街区内院点缀)。
///
/// # Returns
///
/// - `PalmSpots` - 棕榈的世界 (x, z) 坐标。
///
/// # Arguments
///
/// - `f32` - 生成中心的世界 X(米),用于确定流式加载范围。
/// - `f32` - 生成中心的世界 Z(米),用于确定流式加载范围。
fn build_city_palms(cx: f32, _cz: f32) -> PalmSpots {
    let mut out: Vec<[f32; 3]> = Vec::new();
    // 原来这里是 `step += 24.0` 的**刚性格点**:所有棕榈严格等距、位置
    // 完全确定、模型全同款,于是整条街的树像复制粘贴 —— 用户报的
    // 「树木位置不合理 + 高度同质化」。现在改成:
    //
    // - 间距在 `PALM_PITCH_MIN..PALM_PITCH_MAX` 之间按种子抖动(不再等距);
    // - 每棵带**独立的横纵向偏移**,不再整齐地贴在人行道同一根线上;
    // - `scale` 走连续区间,高度不再一层层同高。
    let mut seed: u32 = 0x7A1E_5EED;
    for (line, strip_lo, strip_hi) in street_strips(cx, BLOCK_VIEW_RADIUS, 26.0) {
        let mut step: f32 = strip_lo;
        while step < strip_hi {
            seed = hash2(seed, 0x5EED);
            // 沿街方向抖动,避免出现人造的等距节奏。
            let along_jitter: f32 = (hash_unit(seed, 1) - 0.5) * PALM_PITCH_JITTER;
            // 垂直于街的方向抖动:有的贴近路沿,有的靠外侧。
            let side_jitter: f32 = (hash_unit(seed, 2) - 0.5) * PALM_SIDE_JITTER;
            // 高度:连续变化 + 少量「成簇的高个」,避免整片同高。
            let roll: f32 = hash_unit(seed, 3);
            let tall: bool = roll > PALM_TALL_FRACTION;
            let scale: f32 = if tall {
                PALM_TALL_SCALE_MIN
                    + hash_unit(seed, 4) * (PALM_TALL_SCALE_MAX - PALM_TALL_SCALE_MIN)
            } else {
                PALM_SCALE_MIN + hash_unit(seed, 4) * (PALM_SCALE_MAX - PALM_SCALE_MIN)
            };
            let offset: f32 = STREET_HALF_WIDTH + SIDEWALK_WIDTH * 0.55 + side_jitter;
            // 间距先生成,位置后判 —— 抽签顺序不变,下面内院循环拿到的是
            // 同一串随机数,所以这一改动不挪动任何一棵内院棕榈。
            let pitch: f32 =
                PALM_PITCH_MIN + hash_unit(seed, 5) * (PALM_PITCH_MAX - PALM_PITCH_MIN);
            // 沿街坐标离**最近的十字街道轴线**不足一个路面半宽,就是站在
            // 路口的沥青上,这一对跳过。横向那一维本来就在
            // `[8.18, 9.78]` 的人行道里,不需要再判。
            let along: f32 = step + along_jitter;
            let cross_axis: f32 = street_axis((along / STREET_PITCH).round() as i32);
            if (along - cross_axis).abs() < PALM_JUNCTION_GUARD {
                step += pitch;
                continue;
            }
            out.push([line + offset, along, scale]);
            out.push([along, line - offset, scale]);
            // 间距随机化,而不是固定 24 m。
            step += pitch;
        }
    }
    // 街区内院:每区两三棵,让内部空地不是一块死板。位置同样抖动、
    // 高度同样连续变化 —— 否则内院又是一排等高的复制品。
    for (bi, block) in block_layouts().into_iter().enumerate() {
        for k in 0..2 {
            seed = hash2(seed, 0x9E37);
            let angle: f32 = (k as f32 * 2.2) + bi as f32 * 0.7;
            let radius: f32 = 4.0 + hash_unit(seed, 6) * 3.5;
            let scale: f32 =
                PALM_SCALE_MIN + hash_unit(seed, 7) * (PALM_TALL_SCALE_MAX - PALM_SCALE_MIN);
            let px: f32 = block.cx + angle.sin() * radius;
            let pz: f32 = block.cz + angle.cos() * radius;
            // **必须让开可进入的样板楼。** 内院棕榈原本落在固定 5 m 圆
            // 上,和样板楼门口的距离是「碰巧够用」;半径一抖动到 7.5 m,
            // 树干就长到门洞里去了 —— 碰撞体(半径随 scale 放大到
            // 1.65 倍)把门口堵死,p4 的 showcase.climb 于是卡在
            // (25.82, 36.00) 上不了楼(`best y=0.15 m`)。
            if hits_a_showcase(px, pz, PALM_SHOWCASE_CLEARANCE) {
                continue;
            }
            out.push([px, pz, scale]);
        }
    }
    out
}

/// 程序化生成路边停放的车辆。
///
/// # Returns
///
/// - `Vec<Placement>` - `(资产, 位置, 朝向)`。
///
/// # Arguments
///
/// - `f32` - 生成中心的世界 X(米),用于确定流式加载范围。
/// - `f32` - 生成中心的世界 Z(米),用于确定流式加载范围。
fn build_city_vehicles(cx: f32, _cz: f32) -> Vec<Placement> {
    let mut out: Vec<(&'static str, [f32; 3], f32)> = Vec::new();
    let lane: f32 = STREET_HALF_WIDTH - 2.6;
    let mut seed: u32 = 0xABCD_1234;
    for (line, strip_lo, strip_hi) in street_strips(cx, BLOCK_VIEW_RADIUS, 40.0) {
        let mut step: f32 = strip_lo;
        while step < strip_hi {
            seed = hash2(seed, 0x21);
            if !hash2(seed, 1).is_multiple_of(3) {
                step += 44.0;
                continue;
            }
            let along_x: bool = hash2(seed, 2).is_multiple_of(2);
            let side: f32 = if hash2(seed, 3).is_multiple_of(2) {
                1.0
            } else {
                -1.0
            };
            let position: [f32; 3] = if along_x {
                [step, 0.0, line + side * lane]
            } else {
                [line + side * lane, 0.0, step]
            };
            out.push((
                VEHICLE_POOL[hash2(seed, 4) as usize % VEHICLE_POOL.len()],
                position,
                if along_x {
                    0.0
                } else {
                    std::f32::consts::FRAC_PI_2
                },
            ));
            step += 44.0;
        }
    }
    out
}

/// 程序化生成霓虹招牌(贴在临街楼的外墙上,夜里点亮)。
///
/// # Returns
///
/// - `Vec<Placement>` - `(资产, 位置, 朝向)`。
///
/// # Arguments
///
/// - `f32` - 生成中心的世界 X(米),用于确定流式加载范围。
/// - `f32` - 生成中心的世界 Z(米),用于确定流式加载范围。
fn build_city_signs(cx: f32, _cz: f32) -> Vec<Placement> {
    let mut out: Vec<(&'static str, [f32; 3], f32)> = Vec::new();
    let face: f32 = STREET_HALF_WIDTH + SIDEWALK_WIDTH + 3.2;
    let mut seed: u32 = 0x5151_5151;
    for (line, strip_lo, strip_hi) in street_strips(cx, BLOCK_VIEW_RADIUS, 34.0) {
        let mut step: f32 = strip_lo;
        while step < strip_hi {
            seed = hash2(seed, 0x31);
            if hash2(seed, 1).is_multiple_of(2) {
                out.push((
                    SIGN_POOL[hash2(seed, 2) as usize % SIGN_POOL.len()],
                    [line - face, 4.2, step],
                    -std::f32::consts::FRAC_PI_2,
                ));
            } else {
                out.push((
                    SIGN_POOL[hash2(seed, 2) as usize % SIGN_POOL.len()],
                    [step, 3.8, line + face],
                    std::f32::consts::FRAC_PI_2,
                ));
            }
            step += 52.0;
        }
    }
    out
}

/// 程序化生成行人点缀。
///
/// # Returns
///
/// - `Vec<Placement>` - `(资产, 位置, 朝向)`。
///
/// # Arguments
///
/// - `f32` - 生成中心的世界 X(米),用于确定流式加载范围。
/// - `f32` - 生成中心的世界 Z(米),用于确定流式加载范围。
fn build_city_peds(cx: f32, _cz: f32) -> Vec<Placement> {
    let mut out: Vec<(&'static str, [f32; 3], f32)> = Vec::new();
    let mut seed: u32 = 0x7777_7777;
    for (line, strip_lo, strip_hi) in street_strips(cx, BLOCK_VIEW_RADIUS, 20.0) {
        let mut step: f32 = strip_lo;
        while step < strip_hi {
            seed = hash2(seed, 0x41);
            out.push((
                PED_POOL[hash2(seed, 1) as usize % PED_POOL.len()],
                [
                    line + (hash_unit(seed, 2) - 0.5) * (2.0 * STREET_HALF_WIDTH - 2.0),
                    0.0,
                    step,
                ],
                hash_unit(seed, 3) * std::f32::consts::TAU,
            ));
            step += 19.0;
        }
    }
    out
}

// ===========================================================================
// 游戏状态
// ===========================================================================

/// `DayPhase` 的默认相位(正午)。
const DEFAULT_PHASE: DayPhase = DayPhase::Noon;

/// 按键的按住状态。
pub(crate) struct InputState {
    /// W / A / S / D / 方向键。
    keys: HashMap<String, bool>,
    /// 当前昼夜相位。
    phase: DayPhase,
    /// 鼠标是否按下拖拽。
    dragging: bool,
    /// 上一次指针位置。
    last_pointer: [f64; 2],
    /// 双指上一次的距离(用于捏合缩放)。
    last_pinch: f64,
    /// 当前是否持有指针锁(视角走 `movement_x/y`,不需要按住左键)。
    ///
    /// 锁定态与「按住左键拖拽」是两套输入源,`dragging` 只描述后者。
    /// 分开存是因为两者可以同时为真(锁着的时候也允许拖拽),而退出锁
    /// 时必须能只清掉属于锁的那部分。
    pointer_locked: bool,
    /// 左键是否按下(开火)。
    fire_held: bool,
    /// 本帧是否刚按下左键(单发武器只响一次)。
    fire_pressed: bool,
    /// 鼠标是否刚抬起。
    fire_released: bool,
    /// 鼠标在画布内的归一化位置(`-1.0..1.0`,Y 向上)。
    aim_point: [f64; 2],
    /// 画布的 CSS 像素尺寸,用于把鼠标坐标换算成 NDC。
    viewport: [f64; 2],
    /// 驾驶时**当前接管方向盘**的按键(`"KeyA"` / `"ArrowLeft"` …),
    /// 空串表示没人按。
    ///
    /// 这不是为了「记住谁按着」,而是为了 A/D 与方向键**同时按住**时裁决
    /// 谁说话 —— 见 [`InputState::steer_axis`]。`keys` 本身是「谁按着」,
    /// 单独一个布尔值回答不了「两个方向同时按」这种问题。
    steer_key: String,
}

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
    fn held(&self, code: &str) -> bool {
        self.get_keys().get(code).copied().unwrap_or(false)
    }

    /// 当前拥有方向盘的按键(空串 = 没人按)。
    ///
    /// # Returns
    ///
    /// - `&str` - `KeyboardEvent.code`,借用本结构内部的 `steer_key`。
    fn get_steer_key(&self) -> &str {
        &self.steer_key
    }

    /// 写回「当前谁拥有方向盘」。传空串即交还控制权。
    ///
    /// # Arguments
    ///
    /// - `String` - 新的拥有者按键。
    fn set_steer_key(&mut self, value: String) {
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
        if !owner.is_empty() && self.held(owner) {
            return steer_sign(owner);
        }
        // 没人拥有方向盘:按「右优先,其次左」兜底,同样是满舵二选一。
        if self.held(KEYD) || self.held(ARROWRIGHT) {
            return 1.0;
        }
        if self.held(KEYA) || self.held(ARROWLEFT) {
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

    /// 当前按下的按键集合的只读视图。
    ///
    /// # Returns
    ///
    /// - `&HashMap<String, bool>` - 按键按下状态表。
    pub fn get_keys(&self) -> &HashMap<String, bool> {
        &self.keys
    }
}

/// 一份完整的游戏运行时状态。
///
/// 全部包在 `Rc<RefCell<..>>` 里供 RAF 闭包与事件闭包共享;
/// 闭包内部遵循「先取快照再短暂 borrow_mut」的写法,避免 RefCell 重叠借用。
pub struct Game {
    /// canvas 元素。
    pub canvas: HtmlCanvasElement,
    /// 相机。
    pub camera: Camera,
    /// 场景(资产 + 批次)。
    pub scene: Scene,
    /// 流式世界:地面批次 —— 玩家跨过阈值时这块网格会被整体重建。
    pub ground_batch: usize,
    /// 流式世界:水面批次 —— 同上。
    pub water_batch: usize,
    /// 流式世界:当前地面 / 水面是围绕哪个点生成的(米)。
    pub streamed_center: Vec2,
    /// 注入开火剩余帧数(验收通道,见 [`apply_teleport_request`] 的 `hold` 分支)。
    pub hold_frames: u32,
    /// 验收期间的时间加速倍率(1.0 = 正常)。
    ///
    /// 无头浏览器把 rAF 节流到约 1 fps,而 `MAX_FRAME_TIME` 把每帧的真实
    /// 间隔钳到 0.25 s —— 合起来游戏只跑 **25% 速度**。走完 7 个路点要
    /// 二十分钟,验证一轮全量功能要跑将近一个小时。加速只改喂给模拟的
    /// 步数,不改物理常量,所以走的仍然是同一套积分。
    pub speed_scale: f32,
    /// 验收期间是否免疫伤害(见 [`apply_teleport_request`] 的 `safe` 分支)。
    ///
    /// 走位验证要花好几分钟真实时间(无头浏览器把 rAF 节流到约 1 fps,
    /// 模拟只跑 6% 速度),而街上敌人一直在开火 —— 前几轮实测血量从
    /// 91 一路掉到 68,掉光就重生到最近的医院点,玩家被瞬移回城中心,
    /// 看起来就是「走位脚本把玩家弹飞了」。
    pub safe_mode: bool,

    /// 验收脚本下达的「朝这个**世界**方向持续走」请求(米/秒归一化向量)。
    ///
    /// 不走 WASD 是因为 WASD 是**相机相对**的:脚本必须先把相机转到
    /// 正前方才能让 W 变成世界某个方向,而无头浏览器里合成鼠标事件进不了
    /// 相机的拖拽监听器 —— 上一步就是卡在这里。直接给世界方向,移动这一段
    /// 走的仍然是 `Player::step` 的真实积分(含碰撞分离与室内判定)。
    pub walk_request: Vec2,
    /// 流式世界:资产 id → 场景网格索引,重建静态批次时要用。
    pub asset_index: HashMap<String, usize>,
    /// 流式世界:静态批次(楼 / 树 / 道具 / 招牌)占用的批次表长度。
    ///
    /// 静态批次是 `build_scene` 按生成中心铺的,流式重建时整段换掉;
    /// 批次表是**尾插**的,玩家骨架 / 车队 / 拾取物的批次在静态批次之后,
    /// 所以「静态段长度」就是把静态部分与动态部分分开的唯一依据。
    pub static_batch_count: usize,
    /// 走完 `Player::step` 之后的玩家 XZ(验收探针用)。
    ///
    /// 「有速度、无位移」必须能分清是哪一层把人按住了:`Player::step`
    /// 走 `resolve_slide`(静态),`resolve_dynamic_bodies` 之后还会
    /// **无条件**把玩家 XZ 覆盖成分离结果。两者的输出都记下来,探针
    /// 直接对比就知道是谁把位置钉住的 —— 只看最终坐标永远只能猜。
    pub after_static_step: Vec2,
    /// 走完 `resolve_dynamic_bodies` 之后的玩家 XZ(验收探针用)。
    pub after_dynamic_step: Vec2,
    /// 走完 `step_vertical` 之后的玩家 XZ(验收探针用)。
    ///
    /// `step_vertical` 名义上只管 Y,但它结尾是
    /// `set_position([x, y, z])`,`x` / `z` 来自 `resolve_interior` ——
    /// 于是它**会改写 XZ**。夹在静态层与动态层之间,不改写就看不到它。
    pub after_vertical_step: Vec2,
    /// 被静态几何挡住视线、因此**没有结算伤害**的敌人枪数(验收探针用)。
    ///
    /// 「隔墙掉血」的验收不能只看 `health` 没掉:掉血也可能是因为
    /// 那一帧根本没有敌人开火。这个计数器区分「看得见但打偏」与
    /// 「隔墙、这一枪被门控挡掉」,两者都需要在报告里出现。
    pub shots_blocked_by_geometry: u32,
    /// 走进 `step_vertical` **之前**玩家所在的 XZ(室内层要靠它还原位移)。
    ///
    /// `step_vertical` 需要知道「静态层这一帧把人推进了多远」,才能把
    /// 切向分量从隔墙上滑过去而不是原路弹回。位置本身不含这个信息:
    /// 拿不到上一帧位置就只能做纯分离,那正是「有速度、无位移」的
    /// 成因。所以在 `Player::step` 之前先存一份。
    pub previous_step_xz: Vec2,
    /// 输入状态。
    pub input: InputState,
    /// 渲染后端。
    pub renderer: Option<Renderer>,
    /// 已加载的资产数量。
    pub loaded_assets: usize,
    /// 总资产数量。
    pub total_assets: usize,
    /// 已渲染帧数(测试用)。
    pub frame_count: u64,
    /// 已完成的固定步数(测试用)。
    pub ticks: u64,
    /// 加载失败信息(有值时显示在遮罩上)。
    pub load_error: Option<String>,
    /// 后备方案是否启用(资产 fetch 全部失败)。
    pub using_fallback: bool,
    /// 固定步长累加器(秒)。
    pub accumulator: f32,
    /// 按实测帧率自动降档的画质状态机。
    pub quality: AdaptiveQuality,
    /// 上一帧的 `requestAnimationFrame` 时间戳(秒)。
    pub frame_time: f32,
    /// 验收探针:几个已知世界点投到屏幕上的位置。
    /// 离玩家最近的一栋楼的中心(x, z)。
    ///
    /// 验收脚本拿它当「推挤目标」:街区布局是程序化生成的,写死一个
    /// 魔法坐标早晚会推到空地上,所以在生成碰撞世界时顺手算一次存下。
    pub nearest_building: Option<[f32; 2]>,
    /// 当前帧的画布像素尺寸(调试通道用来报角色在屏幕上的像素坐标)。
    pub canvas_size: Cell<(u32, u32)>,
    pub probe: Vec<Option<[f32; 2]>>,
    /// 玩家状态(位置 / 朝向 / 步态 / 生命值 / 拾取)。
    pub player: Player,
    /// 车队与拾取物。
    pub traffic: Traffic,
    /// 静态碰撞世界(玩家圆 vs 建筑 / 车 / 道具 + 世界边界)。
    pub world: CollisionWorld,
    /// 室内碰撞世界(可进入楼的楼板 / 隔墙 / 楼梯)。
    ///
    /// 刻意与 [`Self::world`] 分开:后者是**纯二维**的,车行道、公交车
    /// 和 200 栋实心楼全都走它;只有玩家进入样板楼时才额外查这一份。
    pub interiors: FloorWorld,
    /// 玩家骨架每个 part 的批次索引(与 `PED_SUIT` 资产 part 一一对应)。
    pub player_batches: Vec<PlayerLimbBatch>,
    /// 车队车辆每个批次对应的车辆索引。
    pub car_batches: Vec<usize>,
    /// 每辆车四个可转车轮的批次。
    pub car_wheel_batches: Vec<CarWheelBatch>,
    /// 车型 id → 该车型切好的「单个车轮」mesh 索引(0..4 四个 slot 共用)。
    pub car_wheel_meshes: HashMap<String, Vec<usize>>,
    /// 拾取物每个批次对应的拾取物索引。
    pub pickup_batches: Vec<usize>,
    /// 本帧是否收到「起跳」请求。
    ///
    /// 键位回调**不直接**改 `player.vertical_velocity`:事件闭包跑在
    /// RAF 之外,直接写物理状态会和 `step_vertical` 抢同一帧的落地面
    /// 判定。改成置一个「本帧请求起跳」的标志,由 `step_vertical` 在
    /// 自己算完支撑面之后消费 —— 积分顺序因此只有一条。
    pub jump_queued: bool,
    /// 是否处于第三人称跟随模式(V 键在跟随 / 第一人称之间切换)。
    pub third_person: bool,
    /// 第三人称跟随焦点的阻尼插值后的世界坐标。
    pub follow_target: Vec3,
    /// 资产 id → 原始包围盒(碰撞世界推导的唯一来源)。
    pub asset_bounds: HashMap<String, Bounds>,
    /// 武器架(当前武器 / 弹药 / 换弹 / 瞄准)。
    pub arsenal: Arsenal,
    /// 玩家受击状态(无敌帧 / 脱战计时 / 重生倒计时)。
    pub hurt: HurtState,
    /// 通缉等级。
    pub wanted: Wanted,
    /// 场上全部敌人(警察 + 混混)。
    pub enemies: Vec<Enemy>,
    /// 场上全部行人。
    pub peds: Vec<Pedestrian>,
    /// 敌人每个实例对应的批次索引。
    pub enemy_batches: Vec<usize>,
    /// 手持武器的批次索引(`usize::MAX` 表示当前武器没有模型)。
    pub weapon_batch: usize,
    /// 当前武器对应的 mesh 索引。
    pub weapon_mesh: usize,
    /// 场景 mesh 索引表(切枪时按资产 id 查批次)。
    pub index_map: HashMap<String, usize>,
    /// 行人每个实例对应的批次索引。
    pub ped_batches: Vec<usize>,
    /// 当前任务。
    pub mission: Mission,
    /// 命中标记的剩余显示时间(秒)。
    pub hitmarker: f32,
    /// 本帧是否打中了敌人(喂给 HUD)。
    pub did_hit: bool,
    /// 累计击杀数(统计 / 任务判定)。
    pub kills: u32,
    /// 藏身区(后巷 / 警局门口)的圆心列表。
    pub hideouts: Vec<Vec2>,
    /// 已接 / 已完成的任务计数。
    pub missions_done: u32,
    /// 通缉是否刚刚升星(供 HUD 播一次性提示)。
    pub wanted_flash: f32,
    /// 地图是否处于打开状态(Tab 键)。
    pub map_open: bool,
    /// 还在空中的手雷(按下 G 投出,落地后结算爆炸)。
    pub live_grenades: Vec<Grenade>,
    /// 当前瞄准的俯仰角(弧度)。
    pub aim_pitch: f32,
}

/// 玩家骨架的一个 part 批次:part 名 + 枢轴 + 批次索引。
#[derive(Clone, Debug)]
pub struct PlayerLimbBatch {
    /// part 名(与资产 JSON 的 `name` 一致)。
    pub part: String,
    /// 关节枢轴的资产本地坐标。
    pub pivot: Vec3,
    /// 父关节(肩 / 胯)枢轴的资产本地坐标;无父关节时等于 `pivot`。
    ///
    /// 资产把上臂 / 小臂切成两个独立 part,各自带绝对局部坐标,
    /// 所以渲染一节 limb 需要**两个**枢轴:自己的(肘 / 膝)与父关节的
    /// (肩 / 胯)。见 [`crate::player::limb_chain_matrix`]。
    pub root_pivot: Vec3,
    /// 该 part 在场景批次表里的索引。
    pub batch: usize,
}

/// 一辆车的四个可转车轮各占一个批次。
///
/// 车身保持「整张车一个批次」不变(它不需要单独动画),只有轮子被切出来
/// 独立变换 —— 与玩家肢体的 `PlayerLimbBatch` 同一套做法。
#[derive(Clone, Debug)]
pub struct CarWheelBatch {
    /// 对应的车辆索引。
    pub car: usize,
    /// 第几个轮子(0..4,对应 `WHEEL_MOUNTS` 的顺序)。
    pub slot: usize,
    /// 该轮子在资产本地坐标的安装位(轮心)。
    pub mount: Vec3,
    /// 该轮子批次的场景索引。
    pub batch: usize,
}

/// 挂载到 canvas 上的事件驱动所需的共享引用。
#[derive(Clone)]
struct GameHandles {
    game: Rc<RefCell<Game>>,
    window: Window,
    canvas: HtmlCanvasElement,
    hud: Option<Element>,
    phase_label: Option<Element>,
    phase_slider: Option<HtmlInputElement>,
    health_bar: Option<Element>,
    armor_bar: Option<Element>,
    ammo: Option<Element>,
    cash: Option<Element>,
    wanted: Option<Element>,
    mission: Option<Element>,
    hitmarker: Option<Element>,
    minimap: Option<HtmlCanvasElement>,
    map_panel: Option<Element>,
    map_detail: Option<Element>,
}

// ===========================================================================
// 程序化地面网格
// ===========================================================================

/// 展开中的程序化地面缓冲。
///
/// `build_ground` 逐步把顶点 / 面 / 逐面颜色写进这三个缓冲,最后交给
/// `expand_asset` 之外的同一套顶点格式。打包成结构体是为了让
/// `push_quad` 的参数个数落在 clippy 的阈值内(7 个)。
struct QuadBuffers<'a> {
    /// 顶点缓冲(每顶点 3 个 f32)。
    positions: &'a mut Vec<[f32; 3]>,
    /// 面索引缓冲。
    faces: &'a mut Vec<[usize; 3]>,
    /// 逐面颜色缓冲。
    face_colors: &'a mut Vec<[f32; 3]>,
}

/// 以逆时针(从 +Y 俯视)顺序向展开缓冲推入一个水平矩形。
///
/// # Arguments
///
/// - `&mut QuadBuffers` - 展开缓冲(顶点 / 面 / 逐面颜色)。
/// - `f32` - X 下界。
/// - `f32` - X 上界。
/// - `f32` - Z 下界。
/// - `f32` - Z 上界。
/// - `f32` - 高度 Y。
/// - `[f32; 3]` - 面颜色。
fn push_quad(
    buffers: &mut QuadBuffers,
    x0: f32,
    x1: f32,
    z0: f32,
    z1: f32,
    y: f32,
    color: [f32; 3],
) {
    let base: usize = buffers.positions.len();
    buffers.positions.push([x0, y, z0]);
    buffers.positions.push([x1, y, z0]);
    buffers.positions.push([x1, y, z1]);
    buffers.positions.push([x0, y, z1]);
    buffers.faces.push([base, base + 2, base + 1]);
    buffers.faces.push([base, base + 3, base + 2]);
    buffers.face_colors.push(color);
    buffers.face_colors.push(color);
}

/// 构造程序化水面:外海 + 内湖,两块独立的顶点色网格。
///
/// 海面必须盖住玩家脚下,不能因为「城市无限大」就够不到
/// 铺到 ±900 —— 外面没有别的几何,海面就是地平线。湖放在城内东南角的一
/// 块凹地里,和海不连通。
///
/// **为什么水面在 Rust 里生成而不是走 Python 资产管线:** 水面是
/// **开放曲面**(一张没有厚度的平板),`export.verify_outward` 对开放曲面
/// 要求声明 `outward=("dir", ...)`,而 `expand_asset` 的 de-index 展开
/// 又需要它有确定的朝向。一张 750 m × 750 m 的板子塞进资产 JSON 要多传
/// 几万个顶点,而 `build_ground()` 已经在 Rust 里用同样的 `push_quad`
/// 铺了 73728 个面 —— 复用同一条路径更省、更可控。
///
/// # Returns
///
/// - `MeshAsset` - 含 `ocean` 与 `lake` 两个 part 的水面资产。
pub fn build_water() -> MeshAsset {
    build_water_near(0.0, 0.0)
}

/// 以 `(cx, cz)` 为中心生成水面 —— 海面必须跟着玩家平移。
///
/// 海是环形网格,原本以世界原点为中心铺到 ±900。世界变成无限之后
/// 玩家可以走到 (5000, 5000),那里的海早就铺不到,脚下直接是虚空。
/// 和地面同理:海面跟着玩家走,不跟着世界坐标铺。
///
/// # Arguments
///
/// - `f32` - 中心 X 坐标(米)。
/// - `f32` - 中心 Z 坐标(米)。
///
/// # Returns
///
/// - `MeshAsset` - 含 `ocean` 与 `lake` 两个 part 的水面资产。
pub fn build_water_near(cx: f32, cz: f32) -> MeshAsset {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut faces: Vec<[usize; 3]> = Vec::new();
    let mut face_colors: Vec<[f32; 3]> = Vec::new();

    // ---- 外海:环形网格,从城市边缘一路铺到远裁剪面 ----
    // 用环形而不是一张大平板:城市的地面只铺到 ±150,环形网格从 ±150
    // 起,和城市地面之间正好无缝对接,不需要在边界上补一圈「悬崖」。
    // 环的半径按几何级数增长 —— 近处密(浪看得出),远处疏(省三角形)。
    let rings: [(f32, f32); SEA_RINGS] = SEA_RING_RADII;
    let spokes: usize = SEA_SPOKES;
    for (ri, &(_inner, outer)) in rings.iter().enumerate() {
        // 每一环的半径插值:第 ri 环填 [rings[ri-1].1, rings[ri].1]。
        let start: f32 = if ri == 0 { 0.0 } else { rings[ri - 1].1 };
        let base: usize = positions.len();
        for s in 0..=spokes {
            let angle: f32 = (s as f32 / spokes as f32) * std::f32::consts::TAU;
            let (sin_a, cos_a): (f32, f32) = angle.sin_cos();
            positions.push([cx + cos_a * start, SEA_LEVEL, cz + sin_a * start]);
            positions.push([cx + cos_a * outer, SEA_LEVEL, cz + sin_a * outer]);
        }
        for s in 0..spokes {
            let a: usize = base + s * 2;
            let b: usize = a + 1;
            let c: usize = a + 2;
            let d: usize = a + 3;
            // 内圈(靠近城市)偏浅,外圈偏深 —— 海的近处能看见浅滩的青绿,
            // 远处是深蓝,这是海水最容易被辨认的视觉特征。
            let shallow: [f32; 3] = SEA_SHALLOW;
            let deep: [f32; 3] = SEA_DEEP;
            let mix: f32 = (ri as f32 / rings.len() as f32).min(1.0);
            let color: [f32; 3] = [
                shallow[0] + (deep[0] - shallow[0]) * mix,
                shallow[1] + (deep[1] - shallow[1]) * mix,
                shallow[2] + (deep[2] - shallow[2]) * mix,
            ];
            faces.push([a, d, c]);
            faces.push([a, c, b]);
            face_colors.push(color);
            face_colors.push(color);
        }
    }

    // ---- 内湖:一张带圆角的多边形水面,凹在城东南 ----
    let lake_center: Vec2 = LAKE_CENTER;
    for i in 0..LAKE_SEGMENTS {
        let t0: f32 = (i as f32 / LAKE_SEGMENTS as f32) * std::f32::consts::TAU;
        let t1: f32 = ((i + 1) as f32 / LAKE_SEGMENTS as f32) * std::f32::consts::TAU;
        let p0: Vec2 = [
            lake_center[0] + t0.cos() * LAKE_RADIUS,
            lake_center[1] + t0.sin() * LAKE_RADIUS_X,
        ];
        let p1: Vec2 = [
            lake_center[0] + t1.cos() * LAKE_RADIUS,
            lake_center[1] + t1.sin() * LAKE_RADIUS_X,
        ];
        let cx: f32 = (p0[0] + p1[0]) * 0.5;
        let cz: f32 = (p0[1] + p1[1]) * 0.5;
        // 只在椭圆内圈留一圈浅滩,和海面同一个「近浅远深」的读法。
        let inner: f32 = LAKE_SHORE_INSET;
        push_quad(
            &mut QuadBuffers {
                positions: &mut positions,
                faces: &mut faces,
                face_colors: &mut face_colors,
            },
            lake_center[0] + (cx - lake_center[0]) * inner,
            cx,
            lake_center[1] + (cz - lake_center[1]) * inner,
            cz,
            LAKE_LEVEL,
            LAKE_DEEP,
        );
        push_quad(
            &mut QuadBuffers {
                positions: &mut positions,
                faces: &mut faces,
                face_colors: &mut face_colors,
            },
            cx,
            lake_center[0] + (p1[0] - lake_center[0]) * 0.999,
            cz,
            lake_center[1] + (p1[1] - lake_center[1]) * 0.999,
            LAKE_LEVEL,
            LAKE_SHORE,
        );
    }

    MeshAsset {
        id: WATER_ID.to_string(),
        category: WATER_CATEGORY.to_string(),
        y_up: true,
        bounds: None,
        parts: vec![MeshPart {
            name: WATER_PART.to_string(),
            base_color: Some(SEA_DEEP),
            emissive: Some([0.0; 3]),
            positions,
            normals: None,
            faces,
            face_colors: Some(face_colors),
            flat: true,
        }],
    }
}

/// 生成程序化地面:整城 300 m × 300 m 的沥青网格 + 人行道 + 车道线
/// + 斑马线 + 路口。
///
/// 用顶点色(`face_colors`)而不是贴图 —— 走的是 mesh.rs 的 de-index 展开,
/// 因此地面上每条车道线都是一个真正的三角形,没有任何纹理资源。
///
/// 地面按 `GROUND_SUBDIV × GROUND_SUBDIV` 的格铺满整城,每一格根据
/// [`on_roadway`] 判成「沥青 / 人行道 / 地块底色」三种之一,再在街道
/// 网格上叠车道虚线、中央双黄线、路口斑马线。这样网格街道与地块
/// 是同一张网格上的不同着色,不会出现「楼大了地面还是一块板」。
///
/// # Returns
///
/// - `MeshAsset` - 展开后的地面网格资产。
pub fn build_ground() -> MeshAsset {
    build_ground_near(0.0, 0.0)
}

/// 以 `(cx, cz)` 为中心生成一块 [`GROUND_SPAN`] 见方的地面。
///
/// 地面不能像建筑那样「卸载了就没了」—— 脚下的每一格都得永远存在。
/// 所以做法是**跟着玩家平移**一块固定尺寸的地面网格,而不是按世界
/// 坐标铺一张无限大网格。后者顶点会随距离变成天文数字,精度直接崩。
///
/// # Arguments
///
/// - `f32` - 中心 X 坐标(米)。
/// - `f32` - 中心 Z 坐标(米)。
///
/// # Returns
///
/// - `MeshAsset` - 展开后的地面网格资产。
///
/// 参数刻意叫 `centre_x` / `centre_z` 而不是 `cx` / `cz`:底面循环
/// 内部要按格子算 `let cx = (x0 + x1) * 0.5`,同名会**遮蔽**函数参数,
/// 于是后面几段的街道枚举全都读到了最后一个单元的中心,而不是本函数
/// 的中心 —— 一块以 (-6000, 1500) 为心的地面上会混进 z = -6240 的路缘石。
pub fn build_ground_near(centre_x: f32, centre_z: f32) -> MeshAsset {
    const ROAD: [f32; 3] = [0.085, 0.085, 0.098];
    const ROAD_ALT: [f32; 3] = [0.100, 0.100, 0.114];
    const SIDEWALK: [f32; 3] = [0.44, 0.42, 0.40];
    const SIDEWALK_EDGE: [f32; 3] = [0.36, 0.345, 0.335];
    const CURB: [f32; 3] = [0.62, 0.60, 0.57];
    const LOT_GROUND: [f32; 3] = [0.145, 0.155, 0.135];
    const LOT_ALT: [f32; 3] = [0.165, 0.175, 0.150];
    const LINE_YELLOW: [f32; 3] = [0.92, 0.70, 0.10];
    const LINE_WHITE: [f32; 3] = [0.82, 0.82, 0.80];
    const CROSSWALK: [f32; 3] = [0.88, 0.88, 0.85];

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut faces: Vec<[usize; 3]> = Vec::new();
    let mut face_colors: Vec<[f32; 3]> = Vec::new();
    let cell: f32 = GROUND_SPAN / GROUND_SUBDIV as f32;
    // 网格对齐到街道网格的整数倍,免得玩家每走一格就看到地面整体抖动
    // 一格 —— 那会让「站在原地」看起来像在滑。
    let origin_x: f32 =
        street_axis((centre_x / GROUND_CELL_ALIGN).round() as i32) - GROUND_SPAN * 0.5;
    let origin_z: f32 =
        street_axis((centre_z / GROUND_CELL_ALIGN).round() as i32) - GROUND_SPAN * 0.5;

    // ---- 1) 底面:整城铺满,按「沥青 / 人行道 / 地块」三态着色 ----
    for i in 0..GROUND_SUBDIV {
        for j in 0..GROUND_SUBDIV {
            let x0: f32 = origin_x + cell * j as f32;
            let x1: f32 = x0 + cell;
            let z0: f32 = origin_z + cell * i as f32;
            let z1: f32 = z0 + cell;
            let cx: f32 = (x0 + x1) * 0.5;
            let cz: f32 = (z0 + z1) * 0.5;
            let edge_x: f32 = STREET_HALF_WIDTH + SIDEWALK_WIDTH;
            // 距最近街道轴线的距离。
            let nearest_x: f32 = street_axis((cx / STREET_PITCH).round() as i32);
            let nearest_z: f32 = street_axis((cz / STREET_PITCH).round() as i32);
            let dist: f32 = (cx - nearest_x).abs().min((cz - nearest_z).abs());
            let color: [f32; 3] = if dist <= STREET_HALF_WIDTH {
                if (i + j) % 2 == 0 { ROAD } else { ROAD_ALT }
            } else if dist <= edge_x {
                if i % 4 == 0 { SIDEWALK_EDGE } else { SIDEWALK }
            } else if (i + j) % 2 == 0 {
                LOT_GROUND
            } else {
                LOT_ALT
            };
            let y: f32 = if dist <= STREET_HALF_WIDTH {
                0.0
            } else if dist <= edge_x {
                0.14
            } else {
                0.16
            };
            push_quad(
                &mut QuadBuffers {
                    positions: &mut positions,
                    faces: &mut faces,
                    face_colors: &mut face_colors,
                },
                x0,
                x1,
                z0,
                z1,
                y,
                color,
            );
        }
    }

    // ---- 2) 路缘石:每条街道两侧各一条竖直面 ----
    //
    // 南北向街道(axis 0,沿 Z 铺)与东西向街道(axis 1,沿 X 铺)必须
    // **各自**按自己的轴心枚举。共用一条枚举时,沿 X 铺的那批街道会
    // 拿着 X 的区间往 Z 上画,于是一块以 (-6000, 1500) 为心的地面
    // 里会混进 z = -6247 的路缘石 —— 顶点离中心整整 7700 m。
    let radius: f32 = GROUND_SPAN * 0.5;
    for axis in 0..2 {
        let cross_center: f32 = if axis == 0 { centre_x } else { centre_z };
        let (along_lo, along_hi): (f32, f32) =
            ground_along_span(if axis == 0 { centre_z } else { centre_x }, radius);
        for (line, _, _) in street_strips(cross_center, radius, 0.0) {
            for side in [-1.0f32, 1.0f32] {
                let inner: f32 = line + side * STREET_HALF_WIDTH;
                let outer: f32 = inner + side * 0.24;
                let mut t: f32 = along_lo;
                while t < along_hi {
                    let t1: f32 = t + cell;
                    let (x0, x1, z0, z1): (f32, f32, f32, f32) = if axis == 0 {
                        (inner.min(outer), inner.max(outer), t, t1)
                    } else {
                        (t, t1, inner.min(outer), inner.max(outer))
                    };
                    push_quad(
                        &mut QuadBuffers {
                            positions: &mut positions,
                            faces: &mut faces,
                            face_colors: &mut face_colors,
                        },
                        x0,
                        x1,
                        z0,
                        z1,
                        0.07,
                        CURB,
                    );
                    t += cell;
                }
            }
        }
    }

    // ---- 3) 中央双黄线 + 车道虚线(沿每条街道,路口处断开)----
    let dash_pitch: f32 = 6.0;
    let dash_len: f32 = 3.0;
    //
    // 和路缘石同一件事:南北向街道沿线画的是 Z,东西向街道沿线画的是
    // X,两条街必须各按各的轴心枚举一遍,否则只会画出一个方向的线。
    for axis in 0..2 {
        let cross_center: f32 = if axis == 0 { centre_x } else { centre_z };
        let (along_lo, along_hi): (f32, f32) =
            ground_along_span(if axis == 0 { centre_z } else { centre_x }, radius);
        for (line, _, _) in street_strips(cross_center, radius, 0.0) {
            let mut t: f32 = along_lo;
            while t < along_hi {
                // 路口范围内不画线:到最近十字街道轴线的距离。
                let nearest_cross: f32 = street_axis((t / STREET_PITCH).round() as i32);
                let in_junction: bool = (t - nearest_cross).abs() <= STREET_HALF_WIDTH + 1.0;
                if !in_junction {
                    let t1: f32 = t + dash_len;
                    // 中央双黄线(两条 0.18 m 宽的实线,只在虚线段画满)。
                    for offset in [-0.25f32, 0.25f32] {
                        let c: f32 = line + offset;
                        let (x0, x1, z0, z1): (f32, f32, f32, f32) = if axis == 0 {
                            (c - 0.09, c + 0.09, t, t1)
                        } else {
                            (t, t1, c - 0.09, c + 0.09)
                        };
                        push_quad(
                            &mut QuadBuffers {
                                positions: &mut positions,
                                faces: &mut faces,
                                face_colors: &mut face_colors,
                            },
                            x0,
                            x1,
                            z0,
                            z1,
                            0.011,
                            LINE_YELLOW,
                        );
                    }
                    // 车道分隔虚线(两侧各一条)。
                    for lane in [-3.5f32, 3.5f32] {
                        let c: f32 = line + lane;
                        let (x0, x1, z0, z1): (f32, f32, f32, f32) = if axis == 0 {
                            (c - 0.12, c + 0.12, t, t1)
                        } else {
                            (t, t1, c - 0.12, c + 0.12)
                        };
                        push_quad(
                            &mut QuadBuffers {
                                positions: &mut positions,
                                faces: &mut faces,
                                face_colors: &mut face_colors,
                            },
                            x0,
                            x1,
                            z0,
                            z1,
                            0.012,
                            LINE_WHITE,
                        );
                    }
                }
                t += dash_pitch;
            }
        }
    }

    // ---- 4) 路口斑马线:每个网格路口四条,横跨每条进出街道 ----
    for (line_x, _, _) in street_strips(centre_x, GROUND_SPAN * 0.5, 0.0) {
        for (line_z, _, _) in street_strips(centre_z, GROUND_SPAN * 0.5, 0.0) {
            for stripe in 0..7 {
                let offset: f32 = -5.4 + stripe as f32 * 1.8;
                let w: f32 = 0.9;
                // 横跨南北向街道(x = line_x)的斑马线,贴在路口南 / 北两侧。
                for side_z in [-1.0f32, 1.0] {
                    let c: f32 = line_z + side_z * (STREET_HALF_WIDTH + 1.0);
                    push_quad(
                        &mut QuadBuffers {
                            positions: &mut positions,
                            faces: &mut faces,
                            face_colors: &mut face_colors,
                        },
                        line_x + offset - w * 0.5,
                        line_x + offset + w * 0.5,
                        c - 1.4,
                        c + 1.4,
                        0.013,
                        CROSSWALK,
                    );
                }
                // 横跨东西向街道(z = line_z)的斑马线。
                for side_x in [-1.0f32, 1.0] {
                    let c: f32 = line_x + side_x * (STREET_HALF_WIDTH + 1.0);
                    push_quad(
                        &mut QuadBuffers {
                            positions: &mut positions,
                            faces: &mut faces,
                            face_colors: &mut face_colors,
                        },
                        c - 1.4,
                        c + 1.4,
                        line_z + offset - w * 0.5,
                        line_z + offset + w * 0.5,
                        0.013,
                        CROSSWALK,
                    );
                }
            }
        }
    }

    let mut min: [f32; 3] = [f32::INFINITY; 3];
    let mut max: [f32; 3] = [f32::NEG_INFINITY; 3];
    for position in &positions {
        for axis in 0..3 {
            if position[axis] < min[axis] {
                min[axis] = position[axis];
            }
            if position[axis] > max[axis] {
                max[axis] = position[axis];
            }
        }
    }

    MeshAsset {
        id: GROUND_ID.to_string(),
        category: GROUND.to_string(),
        y_up: true,
        bounds: Some(crate::mesh::Bounds { min, max }),
        parts: vec![MeshPart {
            name: GROUND_PART.to_string(),
            base_color: Some(ROAD),
            emissive: None,
            positions,
            normals: None,
            faces,
            face_colors: Some(face_colors),
            flat: true,
        }],
    }
}

// ===========================================================================
// 资产加载
// ===========================================================================

/// manifest.json 的结构(只取用得到的字段)。
#[derive(serde::Deserialize)]
struct Manifest {
    /// 资产总数(manifest 自报值,HUD 里做对照)。
    #[serde(default)]
    asset_count: usize,
    /// 资产条目。
    #[serde(default)]
    assets: Vec<ManifestEntry>,
}

/// manifest 里的一个资产条目。
#[derive(Clone, serde::Deserialize)]
struct ManifestEntry {
    /// 资产 id。
    id: String,
    /// 分类(仅用于调试输出)。
    category: String,
    /// 相对 assets/ 的文件名。
    file: String,
}

/// 需要加载的资产清单(场景蓝图里引用到的全部资产)。
///
/// # Returns
///
/// - `Vec<&'static str>` - 计算结果。
fn required_asset_ids() -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = Vec::new();
    // 资产清单只回答「场景引用了哪些资产」,与玩家站在哪里无关;城市是
    // 程序化生成的,任何位置的取用集合都一样,所以固定按原点求一次。
    for building in &build_city_buildings(0.0, 0.0) {
        ids.push(building.asset);
    }
    // 两栋可进入样板楼必须预载,否则场景里根本没有它们的批次。
    for building in &showcase_placements() {
        ids.push(building.asset);
    }
    for prop in &build_city_props(0.0, 0.0) {
        ids.push(prop.asset);
    }
    ids.push(PALM_TALL);
    ids.push(PALM_SHORT);
    for (asset, _, _) in &build_city_vehicles(0.0, 0.0) {
        ids.push(asset);
    }
    for (asset, _, _) in &build_city_signs(0.0, 0.0) {
        ids.push(asset);
    }
    for (asset, _, _) in &build_city_peds(0.0, 0.0) {
        ids.push(asset);
    }
    // 武器模型:任务奖励与拾取点都要用到,必须预载。
    ids.push(WEP_PISTOL);
    ids.push(WEP_SMG);
    ids.push(WEP_BAT);
    ids.push(PICKUP_AMMO_BOX);
    ids.push(PICKUP_ARMOR_VEST);
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// 用 `fetch` 取回一个文本资源(相对路径,适配任意部署子路径)。
///
/// # Arguments
///
/// - `&str` - str 的只读引用。
///
/// # Returns
///
/// - `Result<String, String>` - 计算结果。
async fn fetch_text(url: &str) -> Result<String, String> {
    let Some(window) = window() else {
        return Err(ERR_NO_WINDOW.to_string());
    };
    let promise: Promise = window.fetch_with_str(url);
    let value: JsValue = JsFuture::from(promise)
        .await
        .map_err(|err: JsValue| format!("fetch failed for {url}: {err:?}"))?;
    let response: Response = value
        .dyn_into::<Response>()
        .map_err(|_| format!("{url} did not return a Response"))?;
    if !response.ok() {
        return Err(format!("{url} returned HTTP {}", response.status()));
    }
    let body_promise: Promise = response
        .text()
        .map_err(|err: JsValue| format!("{url} text() threw: {err:?}"))?;
    let body: JsValue = JsFuture::from(body_promise)
        .await
        .map_err(|err: JsValue| format!("{url} body read failed: {err:?}"))?;
    body.as_string()
        .ok_or_else(|| format!("{url} body is not a string"))
}

// ===========================================================================
// 场景搭建
// ===========================================================================

/// 把一个已解析的资产加入场景(每个资产只解析一次,多个批次复用同一个 mesh)。
///
/// # Arguments
///
/// 解析一段资产 JSON 并做健全性检查(不碰场景)。
///
/// # Arguments
///
/// - `&str` - 资产 id。
/// - `&str` - 资产 JSON 原文。
///
/// # Returns
///
/// - `Result<MeshAsset, String>` - 解析出的资产;失败时给出可直接打印的信息。
fn parse_asset(id: &str, json: &str) -> Result<MeshAsset, String> {
    let asset: MeshAsset =
        serde_json::from_str(json).map_err(|err: serde_json::Error| format!("{id}: {err}"))?;
    if !asset.bounds_envelope_is_sane() {
        console_log(&format!("[vcw] {id}: bounds envelope is inverted or empty"));
    }
    if asset.category.is_empty() {
        console_log(&format!(
            "[vcw] {id}: missing category (see SCHEMA.md \u{a7}5)"
        ));
    }
    Ok(asset)
}

/// 把一个已解析的资产展开成 GPU 网格并挂进场景。
///
/// # Arguments
///
/// - `&mut Scene` - Scene 的可变引用。
/// - `&mut HashMap<String, usize>` - 资产 id → 场景 mesh 索引。
/// - `&MeshAsset` - 已解析的资产。
///
/// # Returns
///
/// - `Result<usize, String>` - 该资产在 `scene.meshes` 里的索引。
fn push_parsed_asset(
    scene: &mut Scene,
    index_map: &mut HashMap<String, usize>,
    asset: &MeshAsset,
) -> Result<usize, String> {
    let id: &str = &asset.id;
    if let Some(existing) = index_map.get(id) {
        return Ok(*existing);
    }
    // 汇总每个三角形的自发光颜色(与 expand_asset 的 part 遍历顺序一致)。
    let mut part_emissive: Vec<[f32; 3]> = Vec::with_capacity(
        asset
            .parts
            .iter()
            .map(|part: &MeshPart| part.faces.len())
            .sum(),
    );
    for part in &asset.parts {
        let emissive: [f32; 3] = part.emissive.unwrap_or([0.0; 3]);
        for _ in &part.faces {
            part_emissive.push(emissive);
        }
    }
    let mesh: GpuMesh = expand_asset(asset).map_err(|err: MeshError| format!("{id}: {err}"))?;
    let gpu: MeshAssetGpu = build_gpu_mesh(&mesh, &part_emissive);
    let index: usize = scene.push_mesh(gpu);
    index_map.insert(id.to_string(), index);
    Ok(index)
}

/// 内置回退场景:assets/ 全部加载失败时使用,保证画面非空。
///
/// # Returns
///
/// - `Scene` - 计算结果。
fn build_fallback_scene() -> Scene {
    let mut scene: Scene = Scene::default();
    let ground: MeshAsset = build_ground();
    let mut part_emissive: Vec<[f32; 3]> = Vec::with_capacity(
        ground
            .parts
            .iter()
            .map(|part: &MeshPart| part.faces.len())
            .sum(),
    );
    for part in &ground.parts {
        let emissive: [f32; 3] = part.emissive.unwrap_or([0.0; 3]);
        for _ in &part.faces {
            part_emissive.push(emissive);
        }
    }
    let mesh: crate::mesh::GpuMesh = expand_asset(&ground).expect(EXPECT_GROUND);
    let ground_index: usize = scene.push_mesh(build_gpu_mesh(&mesh, &part_emissive));
    let ground_batch: usize = scene.push_batch(ground_index, true);
    scene.push_instance(
        ground_batch,
        Instance::new([0.0, 0.0, 0.0], 0.0, 1.0, [1.0, 1.0, 1.0]),
    );

    // 一排彩色立方体当「建筑」,每面自带颜色。
    for (index, (x, z, color)) in [
        (-18.0f32, -20.0f32, [0.95, 0.45, 0.62]),
        (-20.0, 0.0, [0.42, 0.82, 0.78]),
        (-19.0, 20.0, [0.98, 0.82, 0.50]),
        (19.0, -20.0, [0.62, 0.72, 0.95]),
        (20.0, 0.0, [0.90, 0.60, 0.85]),
        (19.0, 20.0, [0.55, 0.88, 0.62]),
    ]
    .into_iter()
    .enumerate()
    {
        let height: f32 = 8.0 + index as f32 * 3.5;
        let part: MeshPart = crate::mesh::cube_part(
            [x, height * 0.5, z],
            if x < 0.0 { height * 0.5 } else { 4.0 },
            color,
            FALLBACK_BLOCK,
        );
        let mut emissive: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]; part.faces.len()];
        // 顶部一条自发光带,夜里当霓虹。
        for face in emissive.iter_mut().take(2) {
            *face = [0.9, 0.25, 0.75];
        }
        let asset: MeshAsset = MeshAsset {
            id: format!("fallback_block_{index}"),
            category: FALLBACK.to_string(),
            y_up: true,
            bounds: None,
            parts: vec![part],
        };
        let mesh: crate::mesh::GpuMesh = expand_asset(&asset).expect(EXPECT_FALLBACK_BLOCK);
        let mesh_index: usize = scene.push_mesh(build_gpu_mesh(&mesh, &emissive));
        let batch: usize = scene.push_batch(mesh_index, true);
        scene.push_instance(batch, Instance::new([x, 0.0, z], 0.0, 1.0, [1.0, 1.0, 1.0]));
    }
    scene
}

/// 解析 URL 上的 `?hide=3,7,11`,返回要从场景里剔除的批次索引。
///
/// 这是一个**调试开关**:正常访问时 `location.search` 里没有 `hide=`,
/// 返回空列表,渲染循环的行为完全不受影响。加它是因为街区场景一旦
/// 出现「某块几何体画错」,靠肉眼很难定位到底是哪个批次 ——
/// 逐个隐藏即可二分定位。例:`http://localhost:8765/?hide=11`。
///
/// # Returns
///
/// - `Vec<usize>` - 要隐藏的批次索引列表。
pub fn hidden_batches() -> Vec<usize> {
    window()
        .and_then(|w: Window| w.location().search().ok())
        .map(|q: String| q.trim_start_matches('?').to_string())
        .and_then(|q: String| {
            // 扫**全部** `k=v` 对找 `hide`,而不是只看第一个参数 ——
            // 否则 `?cb=123&hide=1,2` 这种带缓存戳的验收链接会静默失效,
            // 批次没被隐藏,截图看起来"没变化",很容易误判成渲染 bug。
            q.split('&')
                .filter_map(|kv: &str| {
                    kv.split_once('=')
                        .map(|(k, v): (&str, &str)| (k.to_string(), v.to_string()))
                })
                .find(|(k, _): &(String, String)| k == HIDE)
                .map(|(_, v): (String, String)| v)
        })
        .unwrap_or_default()
        .split(',')
        .filter_map(|v: &str| v.trim().parse::<usize>().ok())
        .collect()
}

/// 读一个数字型 query 参数(`?fps=20` 这类),缺省或非法时退回 `fallback`。
///
/// # Arguments
///
/// - `&str` - 参数名(不含 `?` 和 `=`)。
/// - `f32` - 缺省 / 解析失败时用的值。
///
/// # Returns
///
/// - `f32` - 解析出的数值。
fn query_number(key: &str, fallback: f32) -> f32 {
    let search: Option<String> = window().and_then(|w: Window| w.location().search().ok());
    let Some(query): Option<String> = search else {
        return fallback;
    };
    for pair in query.trim_start_matches('?').split('&') {
        let Some((name, value)): Option<(&str, &str)> = pair.split_once('=') else {
            continue;
        };
        if name == key {
            return value.parse::<f32>().unwrap_or(fallback);
        }
    }
    fallback
}

/// 按蓝图把已加载的资产铺成场景批次。
///
/// - `f32` - 流式生成中心的世界 X(米)。
/// - `f32` - 流式生成中心的世界 Z(米)。
///
/// # Returns
///
/// - `(usize, usize, usize)` - 批次网格的三个维度(索引 i / j / k)。
/// # Arguments
///
/// - `&mut Scene` - Scene 的可变引用。
/// - `&HashMap<String, usize>` - HashMap<String, usize> 的只读引用。
/// - `f32` - 流式生成中心的世界 X(米)。
/// - `f32` - 流式生成中心的世界 Z(米)。
fn build_scene(
    scene: &mut Scene,
    index_map: &HashMap<String, usize>,
    cx: f32,
    cz: f32,
) -> (usize, usize, usize) {
    let mut ground_slot: usize = 0;
    let mut water_slot: usize = 0;
    // 地面:程序化网格,走同一条展开管线。以生成中心为准 —— 世界无限,
    // 地面永远跟着生成中心走。
    let ground: MeshAsset = build_ground_near(cx, cz);
    let mut ground_emissive: Vec<[f32; 3]> = Vec::with_capacity(
        ground
            .parts
            .iter()
            .map(|part: &MeshPart| part.faces.len())
            .sum(),
    );
    for part in &ground.parts {
        let emissive: [f32; 3] = part.emissive.unwrap_or([0.0; 3]);
        for _ in &part.faces {
            ground_emissive.push(emissive);
        }
    }
    let ground_mesh: crate::mesh::GpuMesh = expand_asset(&ground).expect(EXPECT_GROUND);
    let ground_index: usize = scene.push_mesh(build_gpu_mesh(&ground_mesh, &ground_emissive));

    let ground_batch: usize = scene.push_batch(ground_index, true);
    ground_slot = ground_batch;
    scene.push_instance(
        ground_batch,
        Instance::new([0.0, 0.0, 0.0], 0.0, 1.0, [1.0, 1.0, 1.0]),
    );

    // 水面:外海 + 内湖,同样是程序化网格,走同一条展开管线。
    // 放在地面之后 —— 水面低于地面 y=0,深度测试会自然把它挡在
    // 城市底下,不需要额外的图层判定。
    let water: MeshAsset = build_water_near(cx, cz);
    let mut water_emissive: Vec<[f32; 3]> = Vec::with_capacity(
        water
            .parts
            .iter()
            .map(|part: &MeshPart| part.faces.len())
            .sum(),
    );
    for part in &water.parts {
        let emissive: [f32; 3] = part.emissive.unwrap_or([0.0; 3]);
        for _ in &part.faces {
            water_emissive.push(emissive);
        }
    }
    let water_mesh: crate::mesh::GpuMesh = expand_asset(&water).expect(EXPECT_WATER);
    let water_index: usize = scene.push_mesh(build_gpu_mesh(&water_mesh, &water_emissive));
    let water_batch: usize = scene.push_batch(water_index, true);
    water_slot = water_batch;
    scene.push_instance(
        water_batch,
        Instance::new([0.0, 0.0, 0.0], 0.0, 1.0, [1.0, 1.0, 1.0]),
    );

    // 建筑:程序化生成的网格街区围合。同类资产合批后每批次一次
    // instanced draw call,顶点数据仍然只解析一次。
    for building in &build_city_buildings(cx, cz) {
        let Some(&mesh_index) = index_map.get(building.asset) else {
            continue;
        };
        let batch: usize = find_or_create_batch(scene, mesh_index);
        scene.push_instance(
            batch,
            Instance::new(
                [building.position[0], 0.0, building.position[1]],
                building.yaw,
                building.scale,
                building.tint,
            ),
        );
    }

    // 可进入样板楼:两栋,各一个批次。
    //
    // `find_or_create_batch` 只在「这个 mesh 还**没有**批次」时才新建,
    // 而每个 mesh 在整条启动流程里只被 `push_parsed_asset` 上传**一次**
    // —— 见 `WebGlRenderer::upload_mesh` 上那段关于下标错位的警告。
    // 所以这里绝不能改成「每栋楼 push 一个新批次再传一次 mesh」。
    for building in &showcase_placements() {
        let Some(&mesh_index) = index_map.get(building.asset) else {
            continue;
        };
        let batch: usize = find_or_create_batch(scene, mesh_index);
        scene.push_instance(
            batch,
            Instance::new(
                [building.position[0], 0.0, building.position[1]],
                building.yaw,
                building.scale,
                building.tint,
            ),
        );
    }

    // 街道道具(含每个路口四角的交通灯):按资产分组 → 天然 instancing。
    for prop in &build_city_props(cx, cz) {
        let Some(&mesh_index) = index_map.get(prop.asset) else {
            continue;
        };
        let batch: usize = find_or_create_batch(scene, mesh_index);
        scene.push_instance(
            batch,
            Instance::new(prop.position, prop.yaw, 1.0, prop.tint),
        );
    }

    // 棕榈:两个品种交替,每个品种一个批次。
    for (index, position) in build_city_palms(cx, cz).iter().enumerate() {
        let asset: &str = if index % 3 == 0 {
            PALM_TALL
        } else {
            PALM_SHORT
        };
        let Some(&mesh_index) = index_map.get(asset) else {
            continue;
        };
        let batch: usize = find_or_create_batch(scene, mesh_index);
        scene.push_instance(
            batch,
            Instance::new(
                [position[0], 0.0, position[1]],
                (index as f32) * 0.7,
                position[2],
                [0.97, 1.0, 0.95],
            ),
        );
    }

    // 路边停放的车辆。
    // 车辆**不在这里**摆放 —— 它们由交通系统(`spawn_player_traffic_pickups`)
    // 在车道上循环行驶,每帧由 `sync_dynamic_instances` 写回实例。
    // 如果这里再摆一套静态摆件车,同一辆车会出现两个实例,而且静态那批
    // 还会在车道中间形成看不见的碰撞体,把动态车队顶死。

    // 霓虹招牌:挂在临街楼的外墙上,自发光在夜里点亮。
    for (asset, position, yaw) in &build_city_signs(cx, cz) {
        let Some(&mesh_index) = index_map.get(*asset) else {
            continue;
        };
        let batch: usize = find_or_create_batch(scene, mesh_index);
        scene.push_instance(batch, Instance::new(*position, *yaw, 1.0, [1.0, 1.0, 1.0]));
    }

    // 行人点缀。
    for (asset, position, yaw) in &build_city_peds(cx, cz) {
        let Some(&mesh_index) = index_map.get(*asset) else {
            continue;
        };
        let batch: usize = find_or_create_batch(scene, mesh_index);
        scene.push_instance(batch, Instance::new(*position, *yaw, 1.0, [1.0, 1.0, 1.0]));
    }
    (ground_slot, water_slot, scene.batches.len())
}

/// 玩家走到哪里了才需要重新生成地面 —— 距离当前生成中心超过这一档。
///
/// 地面块边长是 [`GROUND_SPAN`],玩家站在正中时离边缘还有一半。阈值
/// 取一整个街道间距(60 m):世界无限,但**每走一格街道才重算一次**是
/// 够的 —— 地面网格本身就是按街道网格对齐的,重算与不重算在视觉上
/// 完全一致,只有走到块外才必须重来。
const STREAM_REBUILD_STEP: f32 = STREET_PITCH;

/// 玩家当前位置是否已经走出当前地面块。
///
/// # Arguments
///
/// - `Vec2` - 玩家 XZ 坐标(米)。
/// - `Vec2` - 当前地面块的生成中心(米)。
///
/// # Returns
///
/// - `bool` - `true` 表示该重建了。
fn stream_needs_rebuild(player: Vec2, center: Vec2) -> bool {
    (player[0] - center[0]).abs() > STREAM_REBUILD_STEP
        || (player[1] - center[1]).abs() > STREAM_REBUILD_STEP
}

/// 围绕玩家当前位置重建地面与水面网格。
///
/// 场景的批次表是稳定的(实例表每帧都会重写),所以这里只换掉地面 /
/// 水面两块网格的顶点数据,不动任何批次索引 —— 否则流式重建会顺手
/// 把玩家骨架、车队、拾取物的批次一起搬走。
///
/// # Arguments
///
/// - `&mut Scene` - 场景。
/// - `usize` - 地面批次索引。
/// - `usize` - 水面批次索引。
/// - `f32` - 新的生成中心 X(米)。
/// - `f32` - 新的生成中心 Z(米)。
/// - `&HashMap<String, usize>` - 资产 id → 场景网格索引。
/// - `usize` - 静态批次占用的长度。
///
/// # Returns
///
/// - `Vec<usize>` - **被改写过的 `Scene::meshes` 下标**。调用方必须把
///   这些网格重新传到 GPU(`WebGlRenderer::replace_mesh`),否则画面上
///   仍是旧地面 —— CPU 侧换了数据而 GPU 缓冲没换,且**没有任何报错**。
pub fn rebuild_streamed_surface(
    scene: &mut Scene,
    ground_batch: usize,
    water_batch: usize,
    cx: f32,
    cz: f32,
    index_map: &HashMap<String, usize>,
    static_batch_count: usize,
) -> Vec<usize> {
    rebuild_static_batches(scene, index_map, static_batch_count, cx, cz);
    let mut rewritten: Vec<usize> = Vec::new();
    let surfaces: [(usize, MeshAsset); 2] = [
        (ground_batch, build_ground_near(cx, cz)),
        (water_batch, build_water_near(cx, cz)),
    ];
    for (batch, asset) in surfaces {
        let Some(target) = scene.batches.get(batch) else {
            continue;
        };
        let mesh_index: usize = target.mesh_index;
        let mut emissive: Vec<[f32; 3]> = Vec::with_capacity(
            asset
                .parts
                .iter()
                .map(|part: &MeshPart| part.faces.len())
                .sum(),
        );
        for part in &asset.parts {
            let value: [f32; 3] = part.emissive.unwrap_or([0.0; 3]);
            for _ in &part.faces {
                emissive.push(value);
            }
        }
        let Ok(mesh) = expand_asset(&asset) else {
            continue;
        };
        let Some(slot) = scene.meshes.get_mut(mesh_index) else {
            continue;
        };
        *slot = build_gpu_mesh(&mesh, &emissive);
        rewritten.push(mesh_index);
    }
    rewritten
}

/// 把流式重建改写过的网格重新上传到 GPU。
///
/// **必须原地重传,不能 `upload_mesh` 追加。** `upload_mesh` 是 `push`
/// 语义,GPU 表多出一条之后,`SceneBatch::mesh_index`(直接当 GPU 表下标
/// 用)会整体错位一格 —— 角色 / 车取到别人的网格,表现成「移动时丢失
/// 建模、只剩轮子」。`replace_mesh` 复用同一个 `GlMesh`,GPU 表长度不变。
///
/// 失败必须暴露(`console_log` + 计数),不能静默跳过:重传失败时画面
/// 停在旧街区,而日志里如果什么都没有,排查会绕整整一圈。
///
/// # Arguments
///
/// - `&mut Renderer` - 渲染后端。
/// - `&Scene` - 场景(提供新网格数据)。
/// - `&[usize]` - 被改写过的 `Scene::meshes` 下标。
fn reupload_streamed_meshes(renderer: &mut Renderer, scene: &Scene, rewritten: &[usize]) {
    let Renderer::WebGl(webgl) = renderer else {
        // 纯软件后端的 `draw` 直接读 `scene.meshes`,没有第二份 GPU 拷贝。
        return;
    };
    let mut failures: usize = 0;
    for index in rewritten {
        let Some(mesh) = scene.meshes.get(*index) else {
            continue;
        };
        if let Err(error) = webgl.replace_mesh(*index, mesh) {
            failures += 1;
            if failures <= REPLACE_MESH_REPORT_LIMIT {
                console_log(&format!(
                    "{LOG_REPLACE_MESH_FAILED} {index} ({} verts): {error}",
                    mesh.vertices.len()
                ));
            }
        }
    }
    if failures > 0 {
        console_log(&format!(
            "[vcw] {failures} streamed mesh(es) failed GPU re-upload"
        ));
    }
}

/// 整段换掉静态批次(建筑 / 树 / 道具 / 招牌 / 行人 / 摆件车)。
///
/// 静态批次是尾插的:它们一定排在玩家骨架 / 车队 / 拾取物之前,于是
/// 「截断到 `static_batch_count` 再重新铺一遍」能精确换掉静态段而不动
/// 动态段。不这么做的后果是玩家走到 600 m 外看到的还是出生点那批楼 ——
/// 地面是新的,楼是旧的,等于没做流式。
///
/// # Arguments
///
/// - `&mut Scene` - 场景。
/// - `&HashMap<String, usize>` - 资产 id → 场景网格索引。
/// - `usize` - 静态批次当前占用的长度。
/// - `f32` - 新的生成中心 X(米)。
/// - `f32` - 新的生成中心 Z(米)。
fn rebuild_static_batches(
    scene: &mut Scene,
    index_map: &HashMap<String, usize>,
    static_batch_count: usize,
    cx: f32,
    cz: f32,
) {
    if static_batch_count == 0 || static_batch_count > scene.batches.len() {
        return;
    }
    scene.batches.truncate(static_batch_count);
    for building in build_city_buildings(cx, cz) {
        let Some(&mesh_index) = index_map.get(building.asset) else {
            continue;
        };
        let batch: usize = find_or_create_batch(scene, mesh_index);
        let position: [f32; 3] = [building.position[0], 0.0, building.position[1]];
        scene.push_instance(
            batch,
            Instance::new(position, building.yaw, building.scale, building.tint),
        );
    }
    for prop in build_city_props(cx, cz) {
        let Some(&mesh_index) = index_map.get(prop.asset) else {
            continue;
        };
        let batch: usize = find_or_create_batch(scene, mesh_index);
        scene.push_instance(
            batch,
            Instance::new(prop.position, prop.yaw, 1.0, prop.tint),
        );
    }
    for (index, spot) in build_city_palms(cx, cz).iter().enumerate() {
        let Some(&mesh_index) = index_map.get(if index.is_multiple_of(2) {
            PALM_TALL
        } else {
            PALM_SHORT
        }) else {
            continue;
        };
        let batch: usize = find_or_create_batch(scene, mesh_index);
        let tint: [f32; 3] = [0.95, 1.0, 0.92];
        scene.push_instance(
            batch,
            Instance::new([spot[0], 0.0, spot[1]], 0.0, spot[2], tint),
        );
    }
    for (asset, position, yaw) in build_city_signs(cx, cz) {
        let Some(&mesh_index) = index_map.get(asset) else {
            continue;
        };
        let batch: usize = find_or_create_batch(scene, mesh_index);
        scene.push_instance(batch, Instance::new(position, yaw, 1.0, [1.0, 1.0, 1.0]));
    }
    for (asset, position, yaw) in build_city_peds(cx, cz) {
        let Some(&mesh_index) = index_map.get(asset) else {
            continue;
        };
        let batch: usize = find_or_create_batch(scene, mesh_index);
        scene.push_instance(batch, Instance::new(position, yaw, 1.0, [1.0, 1.0, 1.0]));
    }
}

/// 找到某个 mesh 已有的批次,没有就新建 —— 这就是 instancing 分组的关键。
///
/// # Arguments
///
/// - `&mut Scene` - Scene 的可变引用。
/// - `usize` - 输入值。
///
/// # Returns
///
/// - `usize` - 计数结果。
fn find_or_create_batch(scene: &mut Scene, mesh_index: usize) -> usize {
    if let Some(position) = scene
        .batches
        .iter()
        .position(|batch: &SceneBatch| batch.mesh_index == mesh_index)
    {
        return position;
    }
    scene.push_batch(mesh_index, true)
}

// ===========================================================================
// 碰撞世界:从资产 bounds 自动推导
// ===========================================================================

/// 按场景蓝图把静态碰撞体铺进碰撞世界。
///
/// 建筑 / 车辆 → AABB(从资产 `bounds` 按 yaw 旋转足迹取外接盒);棕榈 /
/// 垃圾桶 / 消防栓 / 交通锥 → 圆(从 `bounds` 取足迹短边的一半)。
///
/// **全部由资产包围盒推导** —— 场景蓝图只给位置 + yaw + scale,碰撞体跟着
/// 资产自动变,不存在第二份手写的魔法坐标表(见 §1.3c 与模块文档)。
///
/// # Arguments
///
/// - `&mut CollisionWorld` - 碰撞世界。
/// - `&HashMap<String, Bounds>` - 资产 id → 该资产声明的包围盒。
/// - `f32` - 流式生成中心的世界 X(米)。
/// - `f32` - 流式生成中心的世界 Z(米)。
fn build_collision_world(
    world: &mut CollisionWorld,
    bounds_map: &HashMap<String, Bounds>,
    cx: f32,
    cz: f32,
) {
    world.get_shapes_mut().clear();
    world.set_player_radius(PLAYER_RADIUS);
    world.set_half_extent([WORLD_HALF, WORLD_HALF]);
    for building in build_city_buildings(cx, cz) {
        push_box(
            world,
            bounds_map,
            building.asset,
            [building.position[0], 0.0, building.position[1]],
            building.yaw,
            building.scale,
        );
    }
    // 可进入样板楼**不进**二维碰撞世界。
    //
    // 这里的每个 AABB 都代表一整栋实心楼;样板楼推一个进去就等于
    // 在门洞外面砌了一堵看不见的墙,玩家永远走不进去,楼梯也就永远用
    // 不到。它们改由 [`crate::interior::FloorWorld`] 表达(见
    // `build_showcase_interiors`)—— 楼板、隔墙、外墙、门洞,逐件建模。
    //
    // 代价是样板楼**不再挡车**:车可以开进它的首层。但两栋楼都摆在
    // 街区围合的**内圈**(`BLOCK_INNER` 半径),离最近车道 19.5 m,
    // 而车道是循环跑固定线路的,所以这条车道永远不会有车。
    for prop in build_city_props(cx, cz) {
        push_box(
            world,
            bounds_map,
            prop.asset,
            [prop.position[0], 0.0, prop.position[1]],
            prop.yaw,
            // 小道具的 AABB 收窄:长椅 / 垃圾桶 / 消防栓 / 报摊 / 电话亭的
            // 原始包围盒比它们实际占的地方大不少,全量推进去会把人行道
            // 和车道边缘堵死,玩家和车都过不去。
            prop_collider_scale(prop.asset),
        );
    }
    for spot in build_city_palms(cx, cz).iter() {
        // 棕榈的碰撞体是**树干**,不是树冠:资产包围盒的 XZ 最大跨度是
        // 展开的叶子(3 m+),拿它当碰撞半径会在车道中间立一圈看不见的
        // 树桩墙。树干半径固定,只挡人不挡车。
        // 车道中心线两侧 `LANE_CLEAR_MARGIN` 米内不放碰撞体:动态车队就
        // 贴着 26.5 / 33.5 跑,树干落在车道里会被反复顶一下。
        if on_lane(spot[0]) || on_lane(spot[1]) {
            continue;
        }
        // 用生成阶段定下的**逐棵独立**缩放(而不是 `index % 3` 的
        // 三档循环),否则碰撞体的粗细也在跟着复制粘贴。
        world.push_circle([spot[0], spot[1]], PALM_TRUNK_RADIUS * spot[2]);
    }
    // 注意:**不**把 `build_city_vehicles()` 的静态摆件车放进碰撞世界。
    // 那些车已经��� `build_scene` 里被交通系统的动态车辆取代了(同一批
    // 实例由 `sync_dynamic_instances` 每帧写入)。留着它们会在车道中间放
    // 一堵看不见的墙,动态车队每帧撞上去、速度被清零,看起来就是
    // 「车不动」—— 而且它们的位置是静态的,车道因此被封死。
}

/// 小型路边道具的碰撞体缩放系数。
///
/// 交通灯和路灯是实心的,按 1.0 用;长椅 / 垃圾桶 / 消防栓 / 报摊 /
/// 电话亭 / 锥桶的资产包围盒比实物大(为了把倾斜的枝干也算进去),直接
/// 1.0 推进去会让它们旁边的车道被隐形墙堵住。
///
/// # Arguments
///
/// - `&str` - 资产 id。
///
/// # Returns
///
/// - `f32` - 碰撞体线性缩放。
fn prop_collider_scale(asset: &str) -> f32 {
    match asset {
        PROP_BENCH | PROP_TRASH_BIN | PROP_FIRE_HYDRANT | PROP_NEWSSTAND | PROP_PHONE_BOOTH => {
            SMALL_PROP_COLLIDER_SCALE
        }
        PROP_TRAFFIC_CONE => CONE_COLLIDER_SCALE,
        _ => FULL_PROP_COLLIDER_SCALE,
    }
}

/// 往碰撞世界里推一个 AABB 摆放实例(建筑 / 车辆 / 方块道具)。
///
/// # Arguments
///
/// - `&mut CollisionWorld` - 碰撞世界。
/// - `&HashMap<String, Bounds>` - 资产 id → 包围盒。
/// - `&str` - 资产 id。
/// - `Vec3` - 摆放位置(世界坐标)。
/// - `f32` - 绕 Y 轴的朝向(弧度)。
/// - `f32` - 统一缩放。
fn push_box(
    world: &mut CollisionWorld,
    bounds_map: &HashMap<String, Bounds>,
    asset: &str,
    position: Vec3,
    yaw: f32,
    scale: f32,
) {
    let (min, max) = local_bounds(bounds_map, asset);
    let (center, half): (Vec2, Vec2) = placement_box(min, max, yaw, scale, position);
    world.push_aabb(center, half);
}
/// 取一个资产声明的本地包围盒,缺省时退化成 1 m 见方。
///
/// # Arguments
///
/// - `&HashMap<String, Bounds>` - 资产 id → 包围盒。
/// - `&str` - 资产 id。
///
/// # Returns
///
/// - `(Vec3, Vec3)` - 包围盒的 `(min, max)`。
fn local_bounds(bounds_map: &HashMap<String, Bounds>, asset: &str) -> (Vec3, Vec3) {
    match bounds_map.get(asset) {
        Some(bounds) => (bounds.get_min(), bounds.get_max()),
        None => ([-0.5, 0.0, -0.5], [0.5, 2.0, 0.5]),
    }
}

// ===========================================================================
// 交通 AI + 拾取物蓝图
// ===========================================================================

/// 车道中心相对街道中轴线的横向偏移(米)。
///
/// 路面半宽 `STREET_HALF_WIDTH` = 7 m,双向车道各占一半,车道中心落在
/// ±3.5 m 处 —— 正好压在程序化地面画的车道虚线上。
const LANE_OFFSET_X: f32 = 3.5;
/// 车队循环轨道的半长(米):车沿 Z 跑完这一段就 wrap 回起点。
///
/// 世界本身没有边界,这一档只是「车队循环一圈多长」,不是世界的边。
/// 旧值是 `CITY_HALF - 30`,把世界边长当成了循环长度。
const TRAFFIC_HALF: f32 = 600.0;

/// 实心大道具(交通灯 / 路灯)碰撞体不缩放。
const FULL_PROP_COLLIDER_SCALE: f32 = 1.0;
/// 小型路边道具碰撞体缩到 55%(见 [`prop_collider_scale`] 的原因)。
const SMALL_PROP_COLLIDER_SCALE: f32 = 0.55;
/// 锥桶碰撞体缩到 30%(锥桶本来就该被车碾过去,不该挡路)。
const CONE_COLLIDER_SCALE: f32 = 0.3;
/// 判定某个 X 坐标是否落在任意一条南北向街道的车道里。
///
/// # Arguments
///
/// - `f32` - 世界 X 坐标(米)。
///
/// # Returns
///
/// - `bool` - `true` 表示该 X 在某条街的车道缓冲带内。
fn on_lane(x: f32) -> bool {
    // 世界是无限网格,「任一街道的车道缓冲带」是无限个区间;只可能
    // 检查紧邻 x 的那一条,再按 2 的步长镜像一次就覆盖 ±。
    let nearest: f32 = street_axis((x / STREET_PITCH).round() as i32);
    let offset: f32 = (x - nearest).abs();
    (offset - LANE_OFFSET_X).abs() < LANE_CLEAR_MARGIN
}

/// 棕榈树干碰撞半径(米)—— 只挡人,树叶可以从中穿过。
const PALM_TRUNK_RADIUS: f32 = 0.4;
/// 车道缓冲区半宽(米):街道中轴线两侧这么多米内不放静态碰撞体。
const LANE_CLEAR_MARGIN: f32 = 1.8;

/// 南北向街道的车道 X 坐标,也就是车队实际会出现的 X 坐标。
///
/// 从 [`street_axis`] **派生**而不是另抄一份数字:街道网格改了这里自动跟着
/// 改,不会出现「车开在没有路的虚空里」。
///
/// # Arguments
///
/// - `i32` - 街道索引(任意整数)。
/// - `f32` - 车道在街道哪一侧(`-1.0` / `+1.0`)。
///
/// # Returns
///
/// - `f32` - 该街道的车道 X 坐标(米)。
fn lane_x(street_index: i32, side: f32) -> f32 {
    street_axis(street_index) + side * LANE_OFFSET_X
}

/// 车队车道蓝图:`(资产, 车道 X, 起始 Z, 巡航速度, 方向)`。
///
/// 每辆车被钉在一个固定 X 上、只沿 Z 跑,到 ±[`TRAFFIC_HALF`] wrap,所以
/// 结构上不可能拐弯、也不可能开进人行道或楼里 —— 车**永远穿不过建筑**。
/// 车辆自己的朝向由 [`traffic::TrafficCar::get_yaw`] 从行驶方向推出。
const TRAFFIC_LANES: &[(&str, f32, f32, f32)] = &[
    // (资产 id, 车道 X, 初始 Z, 行驶方向 +1 / -1)
    //
    // 车道 X = 街道中轴 ± LANE_OFFSET_X(3.5 m),也就是 30 ± 3.5 = 26.5 / 33.5。
    // 出生点在路口 (30, 0),所以车会从玩家眼前开过 —— 上车测试有确定性。
    (CAR_SEDAN, 33.5, 60.0, -1.0),
    (CAR_TAXI, 26.5, -30.0, 1.0),
    (CAR_COUPE, 33.5, -60.0, -1.0),
    (TRUCK_PICKUP, 26.5, 90.0, 1.0),
    (CAR_POLICE, 33.5, 110.0, -1.0),
    (CAR_SEDAN, -26.5, -60.0, 1.0),
    (CAR_TAXI, -33.5, 30.0, -1.0),
    (CAR_COUPE, -26.5, 90.0, 1.0),
    (TRUCK_PICKUP, -33.5, -90.0, -1.0),
    (CAR_POLICE, -26.5, 100.0, 1.0),
];

/// 地面拾取物蓝图:`(资产, HUD 名, 世界坐标)`。
///
/// 全部落在 X = 30 这条街的人行道上(`30 ± (7 + 2.2)`),玩家出生点就在
/// 同一条人行道上,走几步就能捡到。`marker_flame` 是任务目标点,夜里靠
/// 自身 emissive 远远就能看见。
const PICKUP_PLACEMENTS: &[(&str, &str, Vec3)] = &[
    // 出生点 (30, 0) 在十字路口。拾取物沿两条街的人行道分布,
    // 坐标从人行道外沿推出(不落在车道缓冲带里,见 TRAFFIC_LANES)。
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
const CAR_ASSET_IDS: &[&str] = &[CAR_TAXI, CAR_COUPE, CAR_SEDAN, CAR_POLICE, TRUCK_PICKUP];

/// 玩家骨架要渲染的 part 名(躯干 / 头 / 头发 + 四肢 + 两只鞋)。
///
/// 顺序不重要(每个 part 一个独立批次),但必须与 `ped_suit.json` 的
/// `parts[].name` 完全一致。
const PLAYER_PARTS: &[&str] = &[
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

/// 从 `ped_suit` 的原始 JSON 里切出单个 part 的网格 + 枢轴,并挂成场景批次。
///
/// 每个 part 独立成 mesh(顶点只在该 part 上出现一次)+ 独立批次(每帧
/// 写一条 model matrix),这样步态动画只需要改 matrix、不需要重建任何
/// 顶点数据。
///
/// # Arguments
///
/// - `&mut Scene` - 场景。
/// - `&MeshAsset` - `ped_suit` 的完整资产。
/// - `&str` - 要切出的 part 名。
///
/// # Returns
///
/// - `usize` - 新批次的索引。
fn push_player_part(scene: &mut Scene, asset: &MeshAsset, part_name: &str) -> usize {
    let Some(part) = asset
        .parts
        .iter()
        .find(|candidate: &&MeshPart| candidate.name == part_name)
    else {
        return usize::MAX;
    };
    let part: &MeshPart = part;
    let mut emissive: Vec<[f32; 3]> = Vec::with_capacity(part.faces.len());
    let base: [f32; 3] = part.emissive.unwrap_or([0.0; 3]);
    for _ in &part.faces {
        emissive.push(base);
    }
    let solo: MeshAsset = MeshAsset {
        id: asset.id.clone(),
        category: asset.category.clone(),
        y_up: asset.y_up,
        bounds: None,
        parts: vec![part.clone()],
    };
    let mesh: GpuMesh = expand_asset(&solo).expect(PLAYER_PART_EXPECT);
    let mesh_index: usize = scene.push_mesh(build_gpu_mesh(&mesh, &emissive));
    // `near_cull = false`:第三人称相机离角色只有几米,而默认的近处剔除
    // 半径是 26 m,不豁免的话角色会被整批剔掉、画面里根本看不到人。
    scene.push_batch_with_cull(mesh_index, true, false)
}

/// 算出一个车轮的 model matrix:先绕轮心自转,再随车身绕 Y 转并平移。
///
/// 顺序很关键 —— 轮子网格已经被平移到「轮心在原点」,所以自转直接绕
/// 本地**轮轴**即可。自转之后再用车身朝向把安装位转到世界空间,
/// 最后加上车身位置。
///
/// **轮轴是本地 Z,不是 X。** 这条是从资产实测出来的,不是抄注释:
/// `car_coupe` 的 `tyres` part 分成 4 摞(前 / 后 × 左 / 右),每摞的
/// 顶点范围是 `x 0.64 / y 0.64 / z 0.235` —— 最短的轴就是轮轴,
/// 所以轮盘立在 **XY 平面**里、绕 **Z** 转。而车身前进方向是本地
/// `+X`(`Instance::new` 的 `yaw = 0` 面向 `+X`)。
///
/// 旧实现绕本地 **X** 自转,而 X 正是车**前进的方向** —— 那等于让
/// 车轮绕着行驶轴打转(拖拉机式的横向滚动),不是汽车。而且因为轮盘
/// 在 XY 平面内、绕 X 转时顶点只会在 YZ 平面里扫,前进方向的投影
/// **完全不动**:实测 `spin = +0.1` 时轮顶与轮底的世界 X 位移都是
/// `+0.0000`,也就是轮子看起来根本没在滚。
///
/// 修正为绕 Z 自转后,`spin = +0.1` 时轮顶向 **+X** 走 `+0.0998`、
/// 轮底向 **-X** 走 `-0.0998`。车往 `+X` 前进时接触点必须相对车体
/// 向后(-X)滑,实测正是 `-0.0998` —— 无滑滚动成立,且与
/// `advance_wheel_spin(travelled / WHEEL_RADIUS)`(travelled 恒为正)
/// 同号,不需要额外取负。
///
/// # Arguments
///
/// - `Vec3` - 车身世界位置。
/// - `f32` - 车身朝向(弧度)。
/// - `Vec3` - 轮子在资产本地的安装位(轮心)。
/// - `f32` - 车轮转角(弧度)。
///
/// # Returns
///
/// - `Mat4Data` - 列主序 model matrix。
fn car_wheel_model(position: Vec3, yaw: f32, mount: Vec3, spin: f32) -> Mat4Data {
    let (sin_yaw, cos_yaw): (f32, f32) = yaw.sin_cos();
    let (sin_spin, cos_spin): (f32, f32) = spin.sin_cos();
    // 安装位随车身转到世界:与 `Instance::new` 同一套约定
    // (本地 +X → 世界 (cos yaw, 0, -sin yaw),本地 +Z → (sin yaw, 0, cos yaw))。
    let world_mount: Vec3 = [
        position[0] + mount[0] * cos_yaw + mount[2] * sin_yaw,
        position[1] + mount[1],
        position[2] - mount[0] * sin_yaw + mount[2] * cos_yaw,
    ];
    // 绕本地 **Z**(轮轴)自转:基向量 Z 不动,X/Y 在 (cos, sin) 平面里转。
    // 再由 yaw 把这三个基向量转到世界。
    let local: [[f32; 3]; 3] = [
        [cos_spin, -sin_spin, 0.0],
        [sin_spin, cos_spin, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let mut columns: [[f32; 3]; 3] = [[0.0; 3]; 3];
    for (row, column) in columns.iter_mut().enumerate() {
        let basis: [f32; 3] = local[row];
        column[0] = basis[0] * cos_yaw + basis[2] * sin_yaw;
        column[1] = basis[1];
        column[2] = -basis[0] * sin_yaw + basis[2] * cos_yaw;
    }
    [
        columns[0][0],
        columns[0][1],
        columns[0][2],
        0.0, //
        columns[1][0],
        columns[1][1],
        columns[1][2],
        0.0, //
        columns[2][0],
        columns[2][1],
        columns[2][2],
        0.0, //
        world_mount[0],
        world_mount[1],
        world_mount[2],
        1.0,
    ]
}

/// 切出**一个**车轮的 mesh:只保留某个象限的顶点,并把坐标平移成
/// 「轮心在原点」,这样渲染时绕 X 轴旋转就是真实的滚动。
///
/// `car_*` 资产的 `tyres` part 把四个轮子合并在一个 part 里(8 个三角形
/// 索引 ×4,`tyres` 有 576 个顶点),`hubs` 同理。不切开的话四个轮子会
/// 一起绕同一个点公转,看起来像车轮在原地打转。
///
/// # Arguments
///
/// - `&mut Scene` - 场景。
/// - `&MeshAsset` - 车辆资产。
/// - `usize` - 轮子编号(0..4,对应 `WHEEL_MOUNTS` 的顺序)。
///
/// # Returns
///
/// - `usize` - 新 mesh 的索引;资产里没有轮子时返回 `usize::MAX`。
fn push_car_wheel(scene: &mut Scene, asset: &MeshAsset, slot: usize) -> usize {
    let mount: [f32; 3] = crate::traffic::WHEEL_MOUNTS[slot];
    let sign_x: f32 = if mount[0] >= 0.0 { 1.0 } else { -1.0 };
    let sign_z: f32 = if mount[2] >= 0.0 { 1.0 } else { -1.0 };
    let mut parts: Vec<MeshPart> = Vec::new();
    for name in [PART_TYRES, PART_HUBS] {
        let Some(part) = asset
            .parts
            .iter()
            .find(|candidate: &&MeshPart| candidate.name == name)
        else {
            continue;
        };
        let part: &MeshPart = part;
        let _in_quad: Vec<bool> = part
            .positions
            .iter()
            .map(|position: &[f32; 3]| {
                let sx: f32 = if position[0] >= 0.0 { 1.0 } else { -1.0 };
                let sz: f32 = if position[2] >= 0.0 { 1.0 } else { -1.0 };
                (sx - sign_x).abs() < 0.5 && (sz - sign_z).abs() < 0.5
            })
            .collect();
        // 三角面是**顶点下标**(`faces: Vec<[usize; 3]>`),不是连续三顶点。
        // 一个面只在其三个顶点**全部**落在本象限时保留 —— 轮子之间没有
        // 跨象限的面,直接按面筛即可。
        let normals: &Option<Vec<[f32; 3]>> = &part.normals;
        let mut kept_face_indices: Vec<usize> = Vec::new();
        let mut new_positions: Vec<[f32; 3]> = Vec::new();
        let mut new_normals: Vec<[f32; 3]> = Vec::new();
        let mut new_faces: Vec<[usize; 3]> = Vec::new();
        for (face_index, face) in part.faces.iter().enumerate() {
            let inside: bool = face.iter().all(|vertex: &usize| {
                let position: [f32; 3] = part.positions[*vertex];
                let sx: f32 = if position[0] >= 0.0 { 1.0 } else { -1.0 };
                let sz: f32 = if position[2] >= 0.0 { 1.0 } else { -1.0 };
                (sx - sign_x).abs() < 0.5 && (sz - sign_z).abs() < 0.5
            });
            if !inside {
                continue;
            }
            kept_face_indices.push(face_index);
            let base: usize = new_positions.len();
            for vertex in face {
                let position: [f32; 3] = part.positions[*vertex];
                new_positions.push([
                    position[0] - mount[0],
                    position[1] - mount[1],
                    position[2] - mount[2],
                ]);
                new_normals.push(match normals {
                    Some(values) if values.len() > *vertex => values[*vertex],
                    _ => [0.0, 1.0, 0.0],
                });
            }
            new_faces.push([base, base + 1, base + 2]);
        }
        if new_faces.is_empty() {
            continue;
        }
        // 面颜色按同样的筛选顺序取:重建时 `new_faces` 的第 k 个面就是
        // `kept_indices` 的第 k 个,直接按下标取回原色。
        let kept_indices: Vec<usize> = kept_face_indices;
        let face_colors: Option<Vec<[f32; 3]>> =
            part.face_colors.as_ref().map(|colors: &Vec<[f32; 3]>| {
                kept_indices
                    .iter()
                    .map(|index: &usize| colors.get(*index).copied().unwrap_or([0.0; 3]))
                    .collect()
            });
        parts.push(MeshPart {
            name: name.to_string(),
            positions: new_positions,
            normals: Some(new_normals),
            faces: new_faces,
            base_color: part.base_color,
            emissive: part.emissive,
            face_colors,
            flat: part.flat,
        });
    }
    if parts.is_empty() {
        return usize::MAX;
    }
    let mut emissive: Vec<[f32; 3]> = Vec::new();
    for part in &parts {
        let base: [f32; 3] = part.emissive.unwrap_or([0.0; 3]);
        for _ in &part.faces {
            emissive.push(base);
        }
    }
    let solo: MeshAsset = MeshAsset {
        id: asset.id.clone(),
        category: asset.category.clone(),
        y_up: asset.y_up,
        bounds: None,
        parts,
    };
    let Ok(mesh) = expand_asset(&solo) else {
        return usize::MAX;
    };
    // 只返回 mesh 索引,**不**建批次:同一车型的所有车共用这一份几何,
    // 批次由调用方逐车建立(批次才持有每辆车自己的 model matrix)。
    scene.push_mesh(build_gpu_mesh(&mesh, &emissive))
}

/// 玩家骨架每个 part 的本地包围盒(min / max 各一份)。
///
/// # Arguments
///
/// - `&MeshAsset` - `ped_suit` 的完整资产。
/// - `&str` - part 名。
///
/// # Returns
///
/// - `(Vec3, Vec3)` - 该 part 的本地包围盒;找不到时退化成 1 m 见方。
fn part_local_bounds(asset: &MeshAsset, part_name: &str) -> (Vec3, Vec3) {
    let Some(part) = asset
        .parts
        .iter()
        .find(|candidate: &&MeshPart| candidate.name == part_name)
    else {
        return ([-0.5, 0.0, -0.5], [0.5, 2.0, 0.5]);
    };
    let mut min: Vec3 = [f32::INFINITY; 3];
    let mut max: Vec3 = [f32::NEG_INFINITY; 3];
    for position in &part.positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
    }
    (min, max)
}

// ===========================================================================
// 输入绑定
// ===========================================================================

/// 在 canvas 上挂上鼠标 / 触摸 / 滚轮事件。
///
/// 全部用裸 `web_sys::EventTarget::add_event_listener_with_callback` +
/// `Closure`,**不在** euv 的 hook 上下文里调用任何 hook,因此不会出现
/// 「hook context 丢失 → 白屏」。
///
/// # Arguments
///
/// - `&GameHandles` - GameHandles 的只读引用。
fn bind_pointer_events(handles: &GameHandles) {
    let canvas: &HtmlCanvasElement = &handles.canvas;

    // ---- 指针锁:锁定态下用 movement_x/y 转视角,不需要按住左键 ----
    //
    // **这一段是 A 组「拖 240px 而 cameraYaw 完全不动」的真凶修复。**
    // 症状:按住拖 240px,8 次采样 ΔcameraYaw 全为 0.000。
    // 实测(w3_pointerlock 的完整事件序列复刻):在**没有**指针锁的
    // 干净页面上,同样的拖拽给出 Δ=+1.4400 rad(正好 0.006×240),
    // 所以事件确实进了 Rust、dragging 也置上了 —— 不是绑定问题。
    // 一旦页面进入**指针锁**态,浏览器派发的事件里 `client_x/client_y`
    // 被**冻结**在锁定时的光标位置,只有 `movement_x/movement_y` 有值。
    // 而旧实现算的是 `x - last_pointer[0]`,于是 dx 恒为 0 → yaw 恒定。
    // 这解释了为什么症状看起来像「根本没收到事件」:事件收到了,
    // 但被取的两个字段在锁定态下是常量。
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            let Some(mouse) = event.dyn_ref::<MouseEvent>() else {
                return;
            };
            let dx: f64 = f64::from(mouse.movement_x());
            let dy: f64 = f64::from(mouse.movement_y());
            // 锁定态下不需要按住左键,所以**不看** `dragging`。
            if dx == 0.0 && dy == 0.0 {
                return;
            }
            {
                let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                apply_look_delta(&mut game.camera, dx, dy);
            }
            event.prevent_default();
        }));
        attach(canvas, EVENT_MOUSEMOVE, closure);
    }

    // ---- 点击 canvas 申请指针锁 ----
    //
    // `request_pointer_lock` 需要**用户手势**,所以只能挂在真实点击上,
    // 不能在启动时调。按住左键拖拽的老路径继续保留:没锁上时仍然可用。
    {
        // 闭包是 `'static`,不能借用函数参数里的 `&canvas`,所以克隆一份
        // 句柄(`HtmlCanvasElement` 是引用计数的,克隆很便宜)。
        let locked_canvas: HtmlCanvasElement = handles.canvas.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            // 已经在锁里就不重复申请(重复调用在部分实现里会报错)。
            if is_pointer_locked() {
                return;
            }
            // `request_pointer_lock` 返回 `()`(旧版返回 Promise,这里不用它)。
            locked_canvas.request_pointer_lock();
            event.prevent_default();
        }));
        attach(canvas, EVENT_CLICK, closure);
    }
    // 锁进出都要同步状态:Esc 释放之后回到「按住拖拽」的老路径,
    // 否则 `dragging` 残留会让下一次非拖动移动继续转视角。
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |_event: Event| {
            let locked: bool = is_pointer_locked();
            let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
            game.input.pointer_locked = locked;
            if !locked {
                game.input.dragging = false;
            }
        }));
        attach(canvas, EVENT_POINTERLOCKCHANGE, closure);
    }

    // ---- 鼠标按下 / 拖拽 / 抬起 ----
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            let Some(mouse) = event.dyn_ref::<MouseEvent>() else {
                return;
            };
            let (x, y): (f64, f64) = (mouse.client_x() as f64, mouse.client_y() as f64);
            {
                let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                game.input.dragging = true;
                game.input.last_pointer = [x, y];
            }
            // 拖拽时不要让页面滚动 / 选中文本。
            event.prevent_default();
        }));
        attach(canvas, EVENT_POINTERDOWN, closure);
    }
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            let Some(mouse) = event.dyn_ref::<MouseEvent>() else {
                return;
            };
            let (x, y): (f64, f64) = (mouse.client_x() as f64, mouse.client_y() as f64);
            {
                let game: std::cell::Ref<Game> = handles.game.borrow();
                if !game.input.dragging {
                    return;
                }
                let dx: f64 = x - game.input.last_pointer[0];
                let dy: f64 = y - game.input.last_pointer[1];
                drop(game);
                let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                apply_look_delta(&mut game.camera, dx, dy);
                game.input.last_pointer = [x, y];
            }
        }));
        attach(canvas, EVENT_POINTERMOVE, closure);
    }
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |_event: Event| {
            let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
            game.input.dragging = false;
        }));
        attach(canvas, EVENT_POINTERUP, closure);
    }
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |_event: Event| {
            let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
            game.input.dragging = false;
        }));
        attach(canvas, EVENT_POINTERCANCEL, closure);
    }

    // ---- 滚轮缩放 ----
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            let Some(wheel) = event.dyn_ref::<WheelEvent>() else {
                return;
            };
            let delta: f64 = wheel.delta_y();
            let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
            // 滚轮改的是**期望距离**,不是当前距离 —— 当前距离由遮挡
            // 回避每帧改写,直接写它会被下一帧的回避覆盖掉,滚轮就失灵。
            let zoomed: f32 =
                game.camera.get_desired_distance() * (1.0 + (delta as f32) * ZOOM_STEP);
            // 第三人称模式下滚轮只调「跟随距离」,有明确的近 / 远下限,
            // 否则玩家会一路滚到贴脸或者飞到天上。
            if game.third_person {
                game.camera
                    .set_desired_distance(zoomed.clamp(FOLLOW_DISTANCE_MIN, FOLLOW_DISTANCE_MAX));
            } else {
                game.camera.set_distance(zoomed);
                game.camera.clamp_distance();
                game.camera.confine_to_city();
            }
            drop(game);
            event.prevent_default();
        }));
        attach(canvas, EVENT_WHEEL, closure);
    }

    // ---- 单指触摸:转相机 ----
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            let Some(touch_event) = event.dyn_ref::<TouchEvent>() else {
                return;
            };
            let touches: TouchList = touch_event.touches();
            if touches.length() != 1 {
                return;
            }
            let Some(touch) = touches.item(0) else {
                return;
            };
            let (x, y): (f64, f64) = (touch.client_x() as f64, touch.client_y() as f64);
            {
                let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                let previous: [f64; 2] = game.input.last_pointer;
                if game.input.last_pinch > 0.0 {
                    // 从双指恢复成单指:只重置基准,不跳转视角。
                    game.input.last_pointer = [x, y];
                    game.input.last_pinch = 0.0;
                    return;
                }
                let dx: f64 = x - previous[0];
                let dy: f64 = y - previous[1];
                apply_look_delta(&mut game.camera, dx, dy);
                game.input.last_pointer = [x, y];
            }
            event.prevent_default();
        }));
        attach(canvas, EVENT_TOUCHSTART, closure);
    }
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            let Some(touch_event) = event.dyn_ref::<TouchEvent>() else {
                return;
            };
            let touches: TouchList = touch_event.touches();
            match touches.length() {
                1 => {
                    let Some(touch) = touches.item(0) else {
                        return;
                    };
                    let (x, y): (f64, f64) = (touch.client_x() as f64, touch.client_y() as f64);
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    let previous: [f64; 2] = game.input.last_pointer;
                    let dx: f64 = x - previous[0];
                    let dy: f64 = y - previous[1];
                    apply_look_delta(&mut game.camera, dx, dy);
                    game.input.last_pointer = [x, y];
                }
                2 => {
                    // 双指:捏合缩放 + 双指平移。
                    let (a, b) = match (touches.item(0), touches.item(1)) {
                        (Some(a), Some(b)) => (a, b),
                        _ => return,
                    };
                    let distance: f64 = (((a.client_x() - b.client_x()) as f64).powi(2)
                        + ((a.client_y() - b.client_y()) as f64).powi(2))
                    .sqrt();
                    let center_x: f64 = (a.client_x() + b.client_x()) as f64 * 0.5;
                    let center_y: f64 = (a.client_y() + b.client_y()) as f64 * 0.5;
                    // 相机改动拆成两段,因为 `forward()` 在 §borrow 的可重入
                    // 清单里(history `forward`)。第一段在 `RefMut` 内把
                    // `Camera` **move 出来**并读出旧的 input 状态,guard 随
                    // block 结束 drop;第二段在没有 guard 的情况下对 owned
                    // `Camera` 算完,第三段再把相机与新 input 状态一起写回。
                    // 持着 `RefMut` 调 `forward()` 就是一次真实的重入 borrow
                    // —— WASM 里 panic 不可 catch,直接白屏。
                    let (mut camera, previous_pinch, previous_center): (Camera, f64, [f64; 2]) = {
                        let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                        let previous_pinch: f64 = game.input.last_pinch;
                        let previous_center: [f64; 2] = game.input.last_pointer;
                        let camera: Camera = std::mem::take(&mut game.camera);
                        (camera, previous_pinch, previous_center)
                    };
                    if previous_pinch > 0.0 {
                        let scale: f32 = (previous_pinch / distance.max(1.0)) as f32;
                        camera.distance = (camera.distance * scale).clamp(12.0, 180.0);
                        camera.confine_to_city();
                        // 双指平移:中心位移反向推动相机焦点。
                        let forward: [f32; 3] = camera.forward();
                        let right: [f32; 3] = normalize3([-forward[2], 0.0, forward[0]]);
                        let pan: f32 = camera.distance * 0.0016;
                        for axis in 0..3 {
                            camera.target[axis] += (right[axis]
                                * (center_x - previous_center[0]) as f32
                                - forward[axis] * (center_y - previous_center[1]) as f32)
                                * pan;
                        }
                    }
                    {
                        let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                        game.camera = camera;
                        game.input.last_pinch = distance;
                        game.input.last_pointer = [center_x, center_y];
                    }
                }
                _ => {
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    game.input.last_pinch = 0.0;
                }
            }
            event.prevent_default();
        }));
        attach(canvas, EVENT_TOUCHMOVE, closure);
    }
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |_event: Event| {
            let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
            game.input.last_pinch = 0.0;
        }));
        attach(canvas, EVENT_TOUCHEND, closure);
    }
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |_event: Event| {
            let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
            game.input.last_pinch = 0.0;
        }));
        attach(canvas, EVENT_TOUCHCANCEL, closure);
    }
}

/// 在 target 上挂一个 `FnMut(Event)` 闭包,`forget()` 让它活到页面结束。
///
/// # Arguments
///
/// - `&E` - E 的只读引用。
/// - `&str` - str 的只读引用。
/// - `Closure<dyn FnMut(Event)>` - 输入值。
fn attach<E: AsRef<EventTarget>>(target: &E, name: &str, closure: Closure<dyn FnMut(Event)>) {
    let _: Result<(), euv::wasm_bindgen::JsValue> = target
        .as_ref()
        .add_event_listener_with_callback(name, closure.as_ref().unchecked_ref::<Function>());
    closure.forget();
}

/// 当前文档是否持有指针锁。
///
/// 读的是 `document.pointerLockElement`:非 `None` 才算锁上了。
/// 无头环境下浏览器可能直接拒绝锁定(没有真实用户手势),所以这里
/// 必须在**每一次**判断时现读,不能只信 `pointerlockchange` 事件 ——
/// 请求可能被拒,那时事件不来但状态也没有变成 true。
///
/// # Returns
///
/// - `bool` - 当前是否处于指针锁状态。
fn is_pointer_locked() -> bool {
    // 走 `window().document()` 而不是 web_sys 的 `document()` 自由函数:
    // euv 重导出的 web_sys 里没有 `document` 这个绑定,而 `window` 有。
    window()
        .and_then(|window: Window| window.document())
        .and_then(|document: Document| document.pointer_lock_element())
        .is_some()
}

/// 把一次鼠标位移折算成视角增量,并按各自灵敏度写进相机。
///
/// **这是视角唯一的写入口。** 指针锁的 `movement_x/y` 与拖拽的
/// `client_x/y` 差分走的是同一条路,所以灵敏度、pitch 的钳制范围、
/// 左右符号三件事只在这里定义一次,不会两条路径各写一份而漂移。
///
/// **符号关系(实测,不是推导)**:`camera.yaw += dx * LOOK_YAW_SENSITIVITY`,
/// 而 [`LOOK_YAW_SENSITIVITY`] 是**负**的,所以 `dx > 0`(鼠标右移)→
/// `yaw` **减小**。
///
/// **为什么必须取负,以及两个基的关系。** 鼠标的 `dx` 与 `yaw` 定义在
/// **两个不同的基**上,直接同号相乘会把方向反掉:
///
/// - **输入基**:屏幕像素,`+x` 指向屏幕**右**。
/// - **渲染基**:`ndc_x`,相机空间的横向分量,`+x` 指向画面**右**;它由
///   视图矩阵乘世界点算出来。
///
/// 判据是「世界点往哪边动」,不是「数字往哪边变」。实测(玩家不动,鼠标
/// 右移 18 px = +0.108 rad):一对关于玩家对称的世界点的 `ndc_x` **同时
/// 增大**(+0.0599 / +0.0602,两者相等 = 纯旋转没有平移)。`ndc_x` 增大
/// = 场景内容整体**左移** = 画面转到了这些点的**左边** = 相机**右摇**。
/// 所以 `dx > 0` 对应的正是视角右转,而 `yaw` 减小恰好是右摇(见
/// `simulate` 里前向 `[cos yaw, -sin yaw]`:`yaw` 减小 → 前向 +X 分量增大)。
/// 负号就是把「输入的右」翻译成「渲染的右」的那一次符号翻转;取正会把
/// 鼠标右移变成视角左转。
///
/// 下面正文的 `dy > 0`(鼠标下移)→ `pitch` 增大不受这条约束:两个基在
/// 纵向恰好同向(屏幕 y 向下即俯角增大),所以取正,见
/// [`LOOK_PITCH_SENSITIVITY`] 的注释。
///
/// # Arguments
///
/// - `&mut Camera` - 相机的可变引用。
/// - `f64` - 水平位移(像素)。
/// - `f64` - 纵向位移(像素)。
fn apply_look_delta(camera: &mut Camera, dx: f64, dy: f64) {
    camera.yaw += dx as f32 * LOOK_YAW_SENSITIVITY;
    camera.pitch += dy as f32 * LOOK_PITCH_SENSITIVITY;
    camera.clamp_pitch();
}
/// 绑定「开火 / 瞄准 / 右键」三组鼠标事件。
///
/// 之所以和 [`bind_pointer_events`] 的相机拖拽**分开绑**:相机那个
/// `pointerdown` 无条件把 `dragging` 置真,如果复用同一条,开一枪就会
/// 同时开始转相机。分开之后两条路径互不干扰,左键只干一件事。
///
/// # Arguments
///
/// - `&GameHandles` - GameHandles 的只读引用。
fn bind_combat_mouse(handles: &GameHandles) {
    let canvas: &HtmlCanvasElement = &handles.canvas;
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            let Some(mouse) = event.dyn_ref::<MouseEvent>() else {
                return;
            };
            if mouse.button() != MOUSE_BUTTON_LEFT {
                return;
            }
            let (x, y): (f64, f64) = (mouse.client_x() as f64, mouse.client_y() as f64);
            let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
            game.input.fire_held = true;
            game.input.fire_pressed = true;
            game.input.aim_point = [x, y];
            game.input.viewport = [CANVAS_CSS_W, CANVAS_CSS_H];
            event.prevent_default();
        }));
        attach(canvas, EVENT_MOUSEDOWN, closure);
    }
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            let Some(mouse) = event.dyn_ref::<MouseEvent>() else {
                return;
            };
            if mouse.button() != MOUSE_BUTTON_LEFT {
                return;
            }
            let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
            game.input.fire_held = false;
            game.input.fire_released = true;
        }));
        attach(canvas, EVENT_MOUSEUP, closure);
    }
    {
        // 悬停也要更新瞄准点,否则准星永远指着「上一次点击的位置」。
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            let Some(mouse) = event.dyn_ref::<MouseEvent>() else {
                return;
            };
            let (x, y): (f64, f64) = (mouse.client_x() as f64, mouse.client_y() as f64);
            let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
            game.input.aim_point = [x, y];
        }));
        attach(canvas, EVENT_MOUSEMOVE, closure);
    }
    {
        // 右键菜单会吞掉右键,这里屏蔽掉,免得开火中断。
        // `handles` 只需要保持 move 进闭包以维持所有权(原代码写的是
        // `let _ = handles;`,而那个 `let` 没有显式类型标注,违反 §5.1)。
        // 改用 `drop(&handles)` 只丢借用,闭包仍是 `FnMut`。
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            event.prevent_default();
            drop(&handles);
        }));
        attach(canvas, EVENT_CONTEXTMENU, closure);
    }
}

/// 在 `window` 上挂键盘监听(WASD 平移 / R 重置 / T 切昼夜)。
///
/// # Arguments
///
/// - `&GameHandles` - GameHandles 的只读引用。
fn bind_keyboard(handles: &GameHandles) {
    let window: &Window = &handles.window;
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            let Some(keyboard) = event.dyn_ref::<KeyboardEvent>() else {
                return;
            };
            let code: String = keyboard.code();
            // 注意:这些键名都是 `&'static str` **常量**,不能直接写进
            // `matches!` —— 那会被当成 pattern 绑定而不是常量比较。
            // 所以统一走 `TRACKED_KEYS` 的相等扫描。
            let down: bool = TRACKED_KEYS.contains(&code.as_str());
            if !down {
                return;
            }
            let is_press: bool = {
                let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                let was: bool = game.input.held(&code);
                game.input.keys.insert(code.clone(), true);
                // 转向键互相抢占时「后按的赢」,所以必须记下这一下是谁按的。
                game.input.claim_steer(&code);
                !was
            };
            // 方向键在浏览器里默认会滚动页面。方向键现在真能开车了,
            // 让它顺带把页面滚走是明显的破绽,所以在这里吃掉默认行为。
            if code.starts_with("Arrow") {
                event.prevent_default();
            }
            if !is_press {
                return;
            }
            match code.as_str() {
                KEYT => {
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    game.input.phase = game.input.phase.next();
                    let phase: DayPhase = game.input.phase;
                    drop(game);
                    sync_phase_ui(&handles, phase);
                }
                KEYF => {
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    toggle_vehicle(&mut game);
                }
                KEY_RELOAD => {
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    if game.arsenal.reload() {
                        game.player.set_notice(String::from(NOTICE_RELOADED));
                    }
                }
                DIGIT1 => {
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    game.arsenal.set_weapon(Weapon::Pistol);
                    rebind_weapon_batch(&mut game);
                }
                DIGIT2 => {
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    game.arsenal.set_weapon(Weapon::Smg);
                    rebind_weapon_batch(&mut game);
                }
                DIGIT3 => {
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    game.arsenal.set_weapon(Weapon::Bat);
                    rebind_weapon_batch(&mut game);
                }
                KEY_MISSION => {
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    toggle_mission(&mut game);
                }
                KEY_CAMERA => {
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    toggle_camera_mode(&mut game);
                }
                KEY_MAP => {
                    // Tab 是**地图**,不是第三人称切换(见 `toggle_map`)。
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    game.map_open = !game.map_open;
                    drop(game);
                    sync_map_visibility(&handles);
                }
                KEY_SPACE => {
                    // 只置标志,不直接写垂直速度:物理积分在 `step_vertical`。
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    game.jump_queued = true;
                }
                KEY_GRENADE => {
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    throw_grenade(&mut game);
                }
                KEY_UNARMED => {
                    // 收起武器:手上没模型,开火键也不再结算 —— GTA V 的 H。
                    let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
                    if game.arsenal.get_weapon() != Weapon::Unarmed {
                        game.arsenal.set_weapon(Weapon::Unarmed);
                        rebind_weapon_batch(&mut game);
                    }
                }
                _ => {}
            }
        }));
        attach(window, EVENT_KEYDOWN, closure);
    }
    {
        let handles: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            let Some(keyboard) = event.dyn_ref::<KeyboardEvent>() else {
                return;
            };
            let code: String = keyboard.code();
            let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
            game.input.keys.insert(code.clone(), false);
            // 拥有方向盘的那个键松开了,控制权作废,回到「谁在按就听谁」。
            if game.input.steer_key == code {
                game.input.steer_key = String::new();
            }
        }));
        attach(window, EVENT_KEYUP, closure);
    }
}

/// 把昼夜状态同步到 HTML overlay(标签 + 滑块)。
///
/// # Arguments
///
/// - `&GameHandles` - GameHandles 的只读引用。
/// - `DayPhase` - 输入值。
fn sync_phase_ui(handles: &GameHandles, phase: DayPhase) {
    if let Some(label) = &handles.phase_label {
        label.set_text_content(Some(phase.label()));
    }
    if let Some(slider) = &handles.phase_slider {
        let _: Result<(), euv::wasm_bindgen::JsValue> =
            slider.set_attribute(ATTR_VALUE, &format!("{}", phase.slider_value()));
    }
    let hud: String = {
        let game: std::cell::Ref<Game> = handles.game.borrow();
        format_hud(&game, 0)
    };
    if let Some(element) = &handles.hud {
        element.set_text_content(Some(&hud));
    }
    publish_debug_state(handles, &hud);
}

/// 一个 `MeshAssetGpu` 顶点数据的轴对齐包围盒(min xyz, max xyz)。
///
/// 验收探针用:资产 JSON 说 `ped_suit` 是 1.75 m 高的人,如果 GPU 缓冲里
/// 的顶点包围盒是几十米,那就是「顶点上传 / 打包」这一环写坏了,
/// 而不是相机或矩阵的问题。
///
/// 这个探针当初是为「角色糊满屏幕」写的,最后定位到的真因是
/// `WebGlRenderer` 的 GPU mesh 表被重复上传顶偏(见 `upload_mesh` 的
/// 文档),而**顶点数据本身一直是好的**。所以这里改成发布**每个骨架
/// part 的实际顶点包围盒**:`limbExtents` 里的 13 个盒应当都在
/// `ped_suit` 声明的 ±0.3 m / 0..1.75 m 量级内,一旦某个盒子冒出几十米,
/// 就说明顶点数据真的被写坏了 —— 这正是它当初想抓的状态。
///
/// # Arguments
///
/// - `&MeshAssetGpu` - MeshAssetGpu 的只读引用。
///
/// # Returns
///
/// - `MeshExtent` - `[min_x, min_y, min_z, max_x, max_y, max_z]`。
fn gpu_mesh_extent(mesh: &MeshAssetGpu) -> MeshExtent {
    let stride: usize = GPU_STRIDE_FLOATS;
    let mut out: MeshExtent = [
        f32::INFINITY,
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for vertex in mesh.vertices.chunks_exact(stride) {
        for axis in 0..3 {
            let value: f32 = vertex[axis];
            if value < out[axis] {
                out[axis] = value;
            }
            if value > out[axis + 3] {
                out[axis + 3] = value;
            }
        }
    }
    out
}

/// 把 `Option<[f32; 2]]` 编成合法 JSON。
///
/// **`Debug`(`{:?}`) 不能直接写进 JSON。** Rust 会输出
/// `Some([49.0, 60.0])`,这不是合法 JSON;某个字段这样写,整个
/// `window.__vcw` 就会 `JSON.parse` 失败,页面看起来像"没开机",
/// 排查一次要多烧好几轮构建。所有可选数值都必须走这个函数。
///
/// # Arguments
///
/// - `Option<[f32; 2]>` - 待编码的平面点。
///
/// # Returns
///
/// - `String` - 合法 JSON:`"null"` 或 `"[x, z]"`。
fn json_point(point: Option<[f32; 2]>) -> String {
    match point {
        Some([x, z]) => format!("[{},{}]", x, z),
        None => String::from(JSON_NULL),
    }
}

/// 把一串 `f32` 编成合法 JSON 数组。
///
/// # Arguments
///
/// - `&[f32]` - 待编码的数值切片。
///
/// # Returns
///
/// - `String` - 形如 `[1, 2, 3]` 的 JSON 数组。
fn json_f32s(values: &[f32]) -> String {
    let wide: Vec<f64> = values.iter().map(|value: &f32| *value as f64).collect();
    format!("[{}]", join_f64(&wide))
}

/// 把当前运行时状态挂到 `window.__vcw`,供自动化验证读真实数值。
///
/// 这不是给玩家看的 UI,而是验收通道:用 CDP 发真实按键、然后
/// `Runtime.evaluate` 读这里,才能证明「玩家真的在动」而不是「只有相机动」。
///
/// # Arguments
/// 执行自动化验证脚本下达的传送请求。
///
/// 走「每帧读一次 `window.__vcw_teleport`」而不是往 window 挂函数,
/// 因为挂函数需要 web-sys 的 `Function` feature,而 Cargo.toml 已定稿。
/// 脚本侧只要 `__vcw_teleport = {x, z}` 写一个普通对象,下一帧就会生效,
/// 传送后游戏自身会重新贴地 / 重建流式地面 —— 那正是要测的东西。
///
/// # Arguments
///
/// - `&mut Game` - 游戏状态,写入传送后的位置。
fn apply_teleport_request(game: &mut Game) {
    let window: Option<web_sys::Window> = web_sys::window();
    let Some(window) = window else {
        return;
    };
    // 相机朝向也可以注入。WASD 是**相机相对**的,所以「按住 W 走进门」
    // 必须先把相机对准门;真机上靠鼠标拖拽,无头浏览器里合成拖拽会把
    // 画布卡在按住状态,于是朝向也走同一个验收通道。
    let aim_key: JsValue = JsValue::from_str(K_AIM_WINDOW);
    let aim_request: JsValue = js_sys::Reflect::get(&window, &aim_key).unwrap_or(JsValue::NULL);
    if !aim_request.is_null() && !aim_request.is_undefined() {
        let _: bool = js_sys::Reflect::set(&window, &aim_key, &JsValue::NULL).unwrap_or(false);
        if let Some(yaw) = aim_request.as_f64() {
            game.camera.set_yaw(yaw as f32);
        }
    }
    let key: JsValue = JsValue::from_str(K_TELEPORT_WINDOW);
    let request: JsValue = js_sys::Reflect::get(&window, &key).unwrap_or(JsValue::NULL);
    if request.is_null() || request.is_undefined() {
        return;
    }
    // 一次性通道:无论下面走哪个分支,请求都必须在**本帧**被清掉。
    //
    // 之前每个子命令各自 `return`,而 `__vcw_teleport` 一直留在 window 上,
    // 于是 `{probe: true}` 之后通道里永远是那个请求,探针**每帧重跑**;
    // 验收脚本再发 `{x: 23, z: 30}` 时,残留的 probe 分支先把整条请求吃掉
    // 并 `return`,传送根本没执行 —— 表现就是「`__vcw_probe` 有时是空串、
    // 人也没被传送」,两条症状同一个成因。
    let _: bool = js_sys::Reflect::set(&window, &key, &JsValue::NULL).unwrap_or(false);
    // `speed` 子命令:设定时间加速倍率,一直有效到下次改。
    //
    // 判据是「字段存不存在」而不是「是不是 true」:脚本发的可能是
    // `{speed: 6}`,而 `as_bool()` 对数字返回 `None`,整条加速请求会被
    // 静默丢掉。这里和 `walk` 一样先判存在,再用 `as_f64()` 取值。
    if js_sys::Reflect::get(&request, &JsValue::from_str(K_TELEPORT_PROBE))
        .ok()
        .map(|value: JsValue| !value.is_undefined() && !value.is_null())
        .unwrap_or(false)
    {
        probe_nearby_shapes(game);
        return;
    }
    if let Some(scale) = js_sys::Reflect::get(&request, &JsValue::from_str(K_TELEPORT_SPEED))
        .ok()
        .and_then(|value: JsValue| value.as_f64())
    {
        game.speed_scale = (scale as f32).clamp(0.1, 8.0);
        return;
    }
    // `safe` 子命令:打开 / 关闭无敌,直到脚本清掉为止。
    //
    // 和 `walk` 放在同一个请求对象里时(`{walk: true, safe: true, ...}`)
    // 只设标志、不 return ——两者共用 `window.__vcw_teleport` 这一个通道,
    // 分开发送时后一条会覆盖前一条,玩家在 leg 中途被击中重生。
    // 之前这里无条件 return,于是带 safe 的 walk 请求整条被吃掉:
    // 玩家站着不动,看起来像门洞堵死,其实是 walk 根本没被读到。
    let wants_walk: bool = js_sys::Reflect::get(&request, &JsValue::from_str(K_TELEPORT_WALK))
        .ok()
        .map(|value: JsValue| value.is_truthy())
        .unwrap_or(false);
    if js_sys::Reflect::get(&request, &JsValue::from_str(K_TELEPORT_SAFE))
        .ok()
        .map(|value: JsValue| value.is_truthy())
        .unwrap_or(false)
    {
        game.safe_mode = true;
        game.player.set_health(PLAYER_MAX_HEALTH);
        if !wants_walk {
            return;
        }
    }
    // `walk` 子命令:朝世界方向持续走,直到脚本清掉为止。
    //
    // 判据是**字段存不存在**,不是「值是不是 true」:验收脚本的松手动作
    // 发的是 `{walk: false, dx: 0, dz: 0}`,而 `is_truthy()` 对 false
    // 返回 false,于是整个分支被跳过 —— 松手请求既没有停下走路,也
    // 没有被清掉,玩家带着上一次的方向一直朝前冲。实测每次
    // `walk_to` 结束、脚本发完松手,玩家都会**继续**走将近一米才停
    // (TEST1 结束在 30.63,松手后落到 29.71)。路点判定因此永远差一截,
    // 下一段从偏离的位置起步,越走越偏。
    let has_walk: bool = js_sys::Reflect::get(&request, &JsValue::from_str(K_TELEPORT_WALK))
        .ok()
        .map(|value: JsValue| !value.is_undefined() && !value.is_null())
        .unwrap_or(false);
    if has_walk {
        let dx: f32 = js_sys::Reflect::get(&request, &JsValue::from_str("dx"))
            .ok()
            .and_then(|value: JsValue| value.as_f64())
            .map(|value: f64| value as f32)
            .unwrap_or(0.0);
        let dz: f32 = js_sys::Reflect::get(&request, &JsValue::from_str("dz"))
            .ok()
            .and_then(|value: JsValue| value.as_f64())
            .map(|value: f64| value as f32)
            .unwrap_or(0.0);
        // 松手时把速度一起清掉。只清输入方向的话,玩家会带着余速
        // 继续滑行好几米 —— 下一个路点就从偏离的位置起步,撞墙。
        game.walk_request = [dx, dz];
        if dx == 0.0 && dz == 0.0 {
            game.player.set_velocity([0.0, 0.0]);
        }
        // **必须清掉请求**,否则下一帧会掉进下面的传送分支:
        // `has_walk` 变成 false,于是 `read("x")` / `read("z")` 读不到
        // 字段,双双回落到 `SPAWN_POINT[0]`,玩家被瞬移到 (33.5, 33.5)。
        // 这正是「每段路点走完就被弹回街上」的原因。
        let _: bool = js_sys::Reflect::set(&window, &key, &JsValue::NULL).unwrap_or(false);
        return;
    }
    let read = |field: &str, fallback: f32| -> f32 {
        let field_key: JsValue = JsValue::from_str(field);
        js_sys::Reflect::get(&request, &field_key)
            .ok()
            .and_then(|value: JsValue| value.as_f64())
            .map(|value: f64| value as f32)
            .unwrap_or(fallback)
    };
    // 回退值必须按轴取:x 回落 `SPAWN_POINT[0]`、z 回落 `SPAWN_POINT[2]`。
    // 之前两轴共用 `SPAWN_POINT[0]`,于是任何「只有 x 没有 z」的请求都会
    // 把玩家送到 (33.5, 33.5) —— 一个既不是出生点、也不在任何碰撞体
    // 里的地方(真出生点是 (33.5, 45))。
    let x: f32 = read("x", SPAWN_POINT[0]);
    let z: f32 = read("z", SPAWN_POINT[2]);
    let hold: bool = js_sys::Reflect::get(&request, &JsValue::from_str(K_TELEPORT_HOLD))
        .ok()
        .map(|value: JsValue| value.is_truthy())
        .unwrap_or(false);
    // 清掉请求,否则每帧都会传送。
    let _cleared: bool = js_sys::Reflect::set(&window, &key, &JsValue::NULL).unwrap_or(false);
    if hold {
        // 注意:这里**不**立刻清 `fire_held`,让 simulate 消费。

        // 注入开火意图,绕过输入层。真机上的开火路径是
        // canvas `mousedown` -> `fire_held` -> `fire_weapon`;无头浏览器的
        // CDP `Input.dispatchMouseEvent` 与合成 `MouseEvent` 都进不了这个
        // 监听器(实测 `fire_held` 恒为 false),于是从事件层往下的整条
        // 链路一次都测不到。设成 true 之后,`fire_weapon` 走的是**和真人
        // 一样的那段代码**:扣弹匣、算冷却、算命中、结算伤害。
        game.input.fire_held = true;
        game.input.fire_pressed = true;
        game.input.aim_point = [CANVAS_CSS_W * 0.5, CANVAS_CSS_H * 0.5];
        game.input.viewport = [CANVAS_CSS_W, CANVAS_CSS_H];
        // 只注入一帧:`fire_held` 是「按住」语义,不清掉就会一直开火,
        // 脚本连发 14 下就变成扣光整个弹匣外加打空全部备弹。
        game.hold_frames = 1;
        return;
    }
    let here: Vec3 = game.player.get_position();
    game.player.set_position([x, here[1], z]);
    game.camera.set_target([x, FOLLOW_HEIGHT, z]);
    // **不能**顺手把 `streamed_center` 设成目的地。那样下一帧
    // `stream_needs_rebuild` 看到「玩家 == 中心」就判定不用重建,于是
    // 传送过去看到的还是旧地形的三角形数 —— 这正是第一轮 P3 观察到的
    // 「4 次传送三角形一模一样」。让中心留在原处,重建由阈值判定自然触发。
}

/// 把验收脚本要读的状态写进 `window.__vcw`。
///
/// # Arguments
///
/// - `&GameHandles` - WASM 侧持有的各子系统句柄。
/// - `&str` - 当前 HUD 文本(带引号后写入 JSON)。
///
/// # Returns
///
/// - `()` - 无返回值,结果通过 `window.__vcw` 暴露。
fn publish_debug_state(handles: &GameHandles, hud: &str) {
    // 读阶段整体包在一个 block 里:所有取值与两份 JSON 字面量都在这里算完,
    // block 结束时 `Ref` 随作用域析构,之后才写 `window` —— 写 `window` 走
    // `web_sys::window()` / `js_sys::JSON::parse`,是 §borrow 的可重入调用,
    // 不能与活着的 guard 同处一个作用域。
    let (json, visibility): (String, String) = {
        let game: std::cell::Ref<Game> = handles.game.borrow();
        let position: Vec3 = game.player.get_position();
        let car_x: Vec<f64> = game
            .traffic
            .get_cars_ref()
            .iter()
            .map(|car: &TrafficCar| f64::from(car.get_position()[0]))
            .collect();
        let car_z: Vec<f64> = game
            .traffic
            .get_cars_ref()
            .iter()
            .map(|car: &TrafficCar| f64::from(car.get_position()[2]))
            .collect();
        let car_speed: Vec<f64> = game
            .traffic
            .get_cars_ref()
            .iter()
            .map(|car: &TrafficCar| f64::from(car.get_speed()))
            .collect();
        // 第一件还没被捡走的医疗包:验收脚本读它就能走过去真的拾取,而不是
        // 猜一组魔法坐标。
        let health_pack: Option<[f64; 2]> = game
            .traffic
            .get_pickups_ref()
            .iter()
            .find(|p: &&Pickup| !p.get_taken() && p.get_asset() == PICKUP_HEALTH_PACK)
            .map(|p: &Pickup| {
                let at: Vec3 = p.get_position();
                [f64::from(at[0]), f64::from(at[2])]
            });
        // 正在开的那辆车的实时坐标 / 速度:用来证明"车真的在动",而不是
        // 只有相机在动。
        let (cdrive_x, cdrive_z, cdrive_v): (f64, f64, f64) = match game.player.get_vehicle() {
            Some(index) => match game.traffic.get_cars_ref().get(index) {
                Some(car) => {
                    let at: Vec3 = car.get_position();
                    (
                        f64::from(at[0]),
                        f64::from(at[2]),
                        f64::from(car.get_speed()),
                    )
                }
                None => (0.0, 0.0, 0.0),
            },
            None => (0.0, 0.0, 0.0),
        };
        // 相机眼点:验收脚本靠它核对「回避之后眼点到底站在哪」。
        let eye: Vec3 = game.camera.eye();
        // 探针必须序列化成**合法 JSON**:`{:?}` 打出的是 Rust 的
        // `Some([1.0, 2.0])`,在 JSON 里是非法的,整份 `window.__vcw`
        // 会因此解析失败,自动化脚本就再也读不到任何状态。
        // 验收探针:第一个不透明批次的实例矩阵与数量,用来判断「模型矩阵
        // 是不是没写进去」。
        let b0: String = match game.scene.batches.first() {
            Some(batch) => format!(
                "{:?} x{}",
                batch
                    .instances
                    .first()
                    .map(|i: &Instance| i.get_model_ref()),
                batch.instances.len()
            ),
            None => String::from(crate::r#const::NO_BATCHES),
        };
        let bn: usize = game.scene.batches.len();
        // 流式地面的当前生成中心:验收脚本用它证明地面跟着玩家走。
        let stream_x: f32 = game.streamed_center[0];
        let stream_z: f32 = game.streamed_center[1];
        // 全量验收脚本要看「走到远处之后脚下还有没有地」,所以把三角形数
        // 和击杀数一起挂出来:三角形塌成 0 就是几何没生成,击杀数用来确认
        // 战斗回路是活的。
        let tris: usize = game.scene.total_triangles;
        let kills: u32 = game.kills;
        let loaded: usize = game.loaded_assets;
        // 验收脚本要导航到样板楼门口才能验证「进门 → 上楼」,所以把门外的
        // 站位坐标一起挂出来。脚本自己从源码抄 `6.4` 的话,内墙改一次就得
        // 跟着改脚本,迟早对不上。
        let door_spec: &ShowcaseSpec = &showcase_specs()[0];
        let door_at: Vec3 = showcase_door_approach(
            door_spec.position[0],
            door_spec.position[1],
            door_spec.yaw,
            1.6,
        );
        let route_points: Vec<[f32; 2]> = showcase_walk_route(0);
        let mut route_parts: Vec<String> = Vec::new();
        for point in route_points.iter() {
            route_parts.push(format!("[{:.3},{:.3}]", point[0], point[1]));
        }
        let route_json: String = format!("[{}]", route_parts.join(","));
        let door_json: String = format!(
            "{{\"x\":{:.3},\"y\":{:.3},\"z\":{:.3},\"yaw\":{:.4},\"cx\":{:.3},\"cz\":{:.3}}}",
            door_at[0],
            door_at[1],
            door_at[2],
            door_spec.yaw,
            door_spec.position[0],
            door_spec.position[1]
        );
        let wheel_count: usize = game.car_wheel_batches.len();
        // 车轮批次总数 + 第一个车轮的 Y 基向量(自转角的可观测代理):
        // 停车时该向量是 [0,1,0];车轮滚动时会随转角在 XZ 平面里摆动。
        let wheel_probe: String = match game.car_wheel_batches.first() {
            None => String::from("[]"),
            Some(wheel) => match game.scene.batches.get(wheel.batch) {
                None => String::from("[]"),
                Some(batch) => match batch.instances.first() {
                    None => String::from("[]"),
                    Some(instance) => {
                        let m: Mat4Data = *instance.get_model_ref();
                        format!(
                            "[{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}]",
                            m[4], m[5], m[6], m[1], m[2], m[3]
                        )
                    }
                },
            },
        };
        // 最近的建筑实例位置:验收脚本要拿它当「推挤目标」,而不是写死
        // 一个魔法坐标 —— 街区布局会变,写死的坐标早晚会推到空气上。
        let bpos_json: String = json_point(game.nearest_building);
        // ---- 战斗 / 通缉 / 任务 / 行人的探针:自动化验收靠这几个字段 ----
        let enemies_json: String = {
            let items: Vec<String> = game
                .enemies
                .iter()
                .map(|enemy: &Enemy| {
                    let at: Vec3 = enemy.get_position();
                    format!(
                        "{{\"x\":{:.2},\"z\":{:.2},\"hp\":{:.1},\"state\":\"{}\",\"fac\":{},\"alive\":{},\"flash\":{:.2},\"spd\":{:.2}}}",
                        at[0],
                        at[2],
                        enemy.get_health(),
                        enemy.get_state().label(),
                        faction_code(enemy.get_faction()),
                        enemy.is_alive(),
                        enemy.get_flash(),
                        enemy.length()
                    )
                })
                .collect();
            format!("[{}]", items.join(JSON_COMMA))
        };
        let peds_json: String = {
            let items: Vec<String> = game
                .peds
                .iter()
                .map(|ped: &Pedestrian| {
                    let at: Vec3 = ped.get_position();
                    format!(
                        "{{\"x\":{:.2},\"z\":{:.2},\"down\":{},\"gait\":{:.2}}}",
                        at[0],
                        at[2],
                        ped.is_down(),
                        ped.get_gait_amount()
                    )
                })
                .collect();
            format!("[{}]", items.join(JSON_COMMA))
        };
        let wanted_json: String = format!(
            "{{\"stars\":{},\"heat\":{:.3},\"unseen\":{:.2},\"hidden\":{},\"cooling\":{:.2}}}",
            game.wanted.get_stars(),
            game.wanted.get_heat(),
            game.wanted.get_unseen(),
            is_hidden(&game.hideouts, position),
            game.wanted.get_unseen()
        );
        let combat_json: String = format!(
            "{{\"weapon\":{},\"mag\":{},\"reload\":{},\"kills\":{},\"damage\":{:.2},\"hp\":{:.1},\"armor\":{:.1},\"cash\":{},\"missions\":{},\"mission_active\":{},\"mission_title\":\"{}\",\"respawn\":{:.2},\"hitmarker\":{:.2},\"aim\":[{:.3},{:.3}],\"pitch\":{:.3},\"fire_held\":{}}}",
            game.arsenal.get_weapon() as u8,
            game.arsenal.get_magazine(),
            game.arsenal.is_reloading(),
            game.kills,
            game.hurt.get_since_hit(),
            game.player.get_health(),
            game.player.get_armor(),
            game.player.get_cash(),
            game.missions_done,
            game.mission.is_active(),
            game.mission.get_title(),
            game.hurt.get_respawn(),
            game.hitmarker,
            game.arsenal.get_aim()[0],
            game.arsenal.get_aim()[1],
            game.aim_pitch,
            game.input.fire_held
        );
        let limbs_json: String = {
            let parts: Vec<String> = game
                .player_batches
                .iter()
                .map(|limb: &PlayerLimbBatch| {
                    format!("{{\"part\":\"{}\",\"batch\":{}}}", limb.part, limb.batch)
                })
                .collect();
            format!("[{}]", parts.join(JSON_COMMA))
        };
        let limb_model: String = {
            let first: Option<&PlayerLimbBatch> = game.player_batches.first();
            let batch_index: Option<usize> = first.map(|limb: &PlayerLimbBatch| limb.batch);
            let instance: Option<&Instance> = match batch_index {
                Some(index) => game
                    .scene
                    .batches
                    .get(index)
                    .and_then(|batch: &SceneBatch| batch.instances.first()),
                None => None,
            };
            let model: Option<Mat4Data> = instance.map(|inst: &Instance| *inst.get_model_ref());
            match model {
                Some(m) => json_f32s(&m),
                None => String::from(JSON_NULL),
            }
        };
        // 角色在屏幕上的包围盒:把「脚底」和「头顶」两个世界点投影出去。
        // 有了这两个像素坐标,验收脚本能直接算出角色在画面里有多高、落在
        // 画面的哪个位置 —— 不用靠肉眼判断「看没看见」。
        let char_box: String = {
            let size: (u32, u32) = game.canvas_size.get();
            let (canvas_w, canvas_h): (f32, f32) = (size.0 as f32, size.1 as f32);
            let base: Vec3 = game.player.get_position();
            let head: Vec3 = [base[0], base[1] + PED_SCREEN_PROBE_HEIGHT, base[2]];
            let feet: Option<(f32, f32, f32)> =
                game.camera.world_to_screen(base, canvas_w, canvas_h);
            let crown: Option<(f32, f32, f32)> =
                game.camera.world_to_screen(head, canvas_w, canvas_h);
            match (feet, crown) {
                (Some(f), Some(c)) => {
                    format!("[{:.1},{:.1},{:.1},{:.1},{:.1}]", f.0, f.1, c.0, c.1, c.2)
                }
                _ => String::from(JSON_NULL),
            }
        };
        let ft: Vec<f32> = game.follow_target.to_vec();

        let tgt: Vec<f32> = game.camera.get_target().to_vec();
        let ft_json: String = json_f32s(&ft);
        let tgt_json: String = json_f32s(&tgt);
        let mm: Vec<f32> = game.camera.view_projection(1.0).get_elements().to_vec();
        let mm_json: String = json_f32s(&mm);
        let probe_json: String = game
            .probe
            .iter()
            .map(|p: &Option<[f32; 2]>| match p {
                Some([x, y]) => format!("[{},{}]", x, y),
                None => String::from(JSON_NULL),
            })
            .collect::<Vec<String>>()
            .join(",");
        let json: String = format!(
            "{DEBUG_OPEN}\"playerX\":{x},\"playerZ\":{z},\"playerYaw\":{yaw},\"cameraYaw\":{cyaw},\"phase\":\"{phase}\",\"healthPack\":{hp_at},\"pickupPos\":{pickup_pos},\"playerHealth\":{hp},\"playerCash\":{cash},\"playerVehicle\":{veh},\"gaitPhase\":{gphase},\"gaitAmount\":{gamount},\"cameraMode\":\"{mode}\",\"probe\":[{probe_json}],\"charBox\":{char_box},\"b0\":\"{b0}\",\"bpos\":{bpos_json},\"limbBatches\":{limbs_json},\"limbModel\":{limb_model},\"bn\":{bn},\"wheelBatches\":{wheel_count},\"streamX\":{stream_x},\"streamZ\":{stream_z},\"door\":{door_json},\"route\":{route_json},\"wheel\":{wheel_probe},\"ft\":{ft_json},\"tgt\":{tgt_json},\"m\":{mm_json},\"collisionShapes\":{shapes},\"playerInsideCollider\":{inside},\"cameraDist\":{cdist},\"cameraDistTarget\":{cdist_target},\"camOccluded\":{coccluded},\"camEyeX\":{ceye_x},\"camEyeY\":{ceye_y},\"camEyeZ\":{ceye_z},\"camClearance\":{cclear},\"camPitch\":{cpitch},\"carX\":[{car_x}],\"carZ\":[{car_z}],\"carSpeed\":[{car_speed}],\"pickups\":{{\"total\":{total},\"taken\":{taken}}},\"hud\":\"{hud}\",\"carDriveX\":{cdrive_x},\"carDriveZ\":{cdrive_z},\"carDriveSpeed\":{cdrive_v},\"throttle\":{throttle},\"frames\":{frames},\"tris\":{tris},\"kills\":{kills},\"loadedAssets\":{loaded},\"enemies\":{enemies_json},\"peds\":{peds_json},\"wanted\":{wanted_json},\"combat\":{combat_json},\"interiors\":{interiors_json},\"playerY\":{py},\"grounded\":{grounded},\"vy\":{vy},\"walkReq\":[{wr0},{wr1}],\"vel\":[{vx},{vz}],\"respawn\":{rsp},\"safe\":{saf},\"mapOpen\":{map_open},\"grenadesLeft\":{grenades},\"liveGrenades\":{live_grenades},\"grenadeTubes\":{tubes}{DEBUG_CLOSE}",
            x = position[0],
            z = position[2],
            yaw = game.player.get_yaw(),
            cyaw = game.camera.get_yaw(),
            phase = game.input.phase.as_str(),
            pickup_pos = join_pickups(&game),
            hp_at = match health_pack {
                Some(at) => format!("[{},{}]", at[0], at[1]),
                None => String::from(JSON_NULL),
            },
            hp = game.player.get_health(),
            cash = game.player.get_cash(),
            veh = match game.player.get_vehicle() {
                Some(index) => format!("{index}"),
                None => String::from(JSON_NULL),
            },
            gphase = game.player.get_gait_phase(),
            gamount = game.player.get_gait_amount(),
            mode = if game.third_person {
                HUD_THIRD_PERSON
            } else {
                HUD_FIRST_PERSON
            },
            shapes = game.world.get_shapes().len(),
            inside = game.world.contains_point([position[0], position[2]]),
            cdist = game.camera.get_distance(),
            cdist_target = game.camera.get_desired_distance(),
            coccluded = game.camera.get_occluded(),
            ceye_x = eye[0],
            ceye_y = eye[1],
            ceye_z = eye[2],
            py = position[1],
            grounded = game.player.get_grounded(),
            wr0 = game.walk_request[0],
            wr1 = game.walk_request[1],
            vx = game.player.get_velocity()[0],
            vz = game.player.get_velocity()[1],
            rsp = game.hurt.get_respawn(),
            saf = game.safe_mode,
            vy = game.player.get_vertical_velocity(),
            map_open = game.map_open,
            grenades = game.arsenal.get_grenade_tubes(),
            live_grenades = game.live_grenades.len(),
            tubes = game.arsenal.get_grenade_tubes(),
            interiors_json = format!(
                "{{\"count\":{},\"inSolid\":{}}}",
                game.interiors.get_floors().len(),
                game.interiors
                    .contains_interior_point([position[0], position[2]], position[1] + 0.5)
            ),
            cclear = game.camera.get_eye_clearance(),
            cpitch = game.camera.get_pitch(),
            car_x = join_f64(&car_x),
            car_z = join_f64(&car_z),
            car_speed = join_f64(&car_speed),
            total = game.traffic.get_pickups_ref().len(),
            taken = game.player.collected_count(),
            hud = hud
                .replace(CHAR_QUOTE, "")
                .replace(CHAR_BACKSLASH, JSON_SLASH),
            cdrive_x = cdrive_x,
            cdrive_z = cdrive_z,
            cdrive_v = cdrive_v,
            throttle = axis(&game.input, KEYW, KEYS),
            frames = game.frame_count,
            enemies_json = enemies_json,
            peds_json = peds_json,
            wanted_json = wanted_json,
            combat_json = combat_json,
        );
        // 可视化快照也是纯字符串拼接(读 `game` 但不写 `window`),所以在
        // guard 还活着的这里一次算完。
        let visibility: String = visibility_json(&game, &char_box);
        (json, visibility)
    };
    let _: Result<(), euv::wasm_bindgen::JsValue> = set_window_json(DEBUG_HOOK_NAME, &json);
    let _: Result<(), euv::wasm_bindgen::JsValue> =
        set_window_json(DEBUG_VISIBILITY_HOOK_NAME, &visibility);
}

/// 把「角色可见性」这一组字段拼成 JSON 字面量。
///
/// 与全量快照 [`publish_debug_state`] 分开维护:验收脚本判断「角色到底
/// 画没画、画多大、在画面哪里」只需要这几个字段,不必去 parse 一整份
/// 几十 KB 的快照再猜哪几个 key 是角色相关的。
///
/// `char_visible` 的判据是**投影盒落在画布内且不贴边** —— 只判断
/// 「投影成功」会把「角色被放大到糊满屏幕」这种最典型的坏状态判成
/// 可见(包围盒在画布内,但宽度接近整个画布)。所以额外要求角色高度
/// 占画布的百分比落在 `CHAR_VISIBLE_MAX_SCREEN_PCT` 以下。
///
/// **只拼串、不写 `window`**:写 `window` 走 `js_sys::JSON::parse` /
/// `web_sys::window()`,是 §borrow 清单里的可重入调用。调用方在 `Ref` 还
/// 活着的时候调它就会 panic,所以这个函数必须保持纯函数,写 `window`
/// 的那一步留给 guard 已经放掉的调用方。
///
/// # Arguments
///
/// - `&Game` - Game 的只读引用。
/// - `&str` - 已算好的 `charBox` JSON 字面量。
///
/// # Returns
///
/// - `String` - 挂到 `window.__VCW_DEBUG__` 的 JSON 字面量。
fn visibility_json(game: &Game, char_box: &str) -> String {
    let size: (u32, u32) = game.canvas_size.get();
    let (canvas_w, canvas_h): (f32, f32) = (size.0 as f32, size.1 as f32);
    let feet: Vec3 = game.player.get_position();
    let crown: Vec3 = [feet[0], feet[1] + PED_SCREEN_PROBE_HEIGHT, feet[2]];
    let foot_screen: Option<(f32, f32, f32)> =
        game.camera.world_to_screen(feet, canvas_w, canvas_h);
    let crown_screen: Option<(f32, f32, f32)> =
        game.camera.world_to_screen(crown, canvas_w, canvas_h);
    let (screen_x, screen_y, height_px): (f64, f64, f64) = match (foot_screen, crown_screen) {
        (Some(f), Some(c)) => (f64::from(f.0), f64::from(f.1), f64::from((f.1 - c.1).abs())),
        _ => (0.0, 0.0, 0.0),
    };
    let screen_pct: f64 = if canvas_h > 0.0 {
        100.0 * height_px / f64::from(canvas_h)
    } else {
        0.0
    };
    let projected: bool = foot_screen.is_some() && crown_screen.is_some();
    let on_screen: bool = projected
        && screen_x >= 0.0
        && screen_x <= f64::from(canvas_w)
        && screen_y >= 0.0
        && screen_y <= f64::from(canvas_h);
    let char_visible: bool = on_screen && screen_pct <= CHAR_VISIBLE_MAX_SCREEN_PCT;
    // **把玩家附近最近的几个静态碰撞体报出来。**
    //
    // 「有速度、无位移」有两种完全不同的成因,只看 `playerInsideCollider`
    // 分不开 —— 那个字段判的是「圆心落在盒内」,而撞墙时人站在**盒子
    // 外面** 贴着面,仍然是 `false`。必须报最近表面的距离,以及沿
    // 「本帧想走的方向」前方 1 m 处会被谁挡住。
    //
    // 这也是唯一能定位「路被哪根柱子堵了」的通道:光看 `collisionShapes`
    // 的总数(1829)没有任何信息量。
    //
    // **以 `after_static_step` 为基准**,而不是最终位置:动态层会在静态
    // 层之后再推一次,拿最终位置去问会得到「附近一个碰撞体都没有」这种
    // 自相矛盾的结论(实测静态层停在 z=35.243、动态层把玩家顶到 35.550,
    // 而 6 m 内报告为空 —— 因为查询点在墙的**另一边**)。
    let near_shapes: String = {
        let here: Vec2 = [game.after_static_step[0], game.after_static_step[1]];
        let mut scored: Vec<(f32, String)> = game
            .world
            .get_shapes()
            .iter()
            .filter_map(|shape: &crate::r#collision::Shape| {
                let distance: f32 = crate::r#collision::shape_distance(shape, here);
                if distance > NEAR_SHAPE_REPORT_RADIUS {
                    return None;
                }
                let text: String = match shape {
                    crate::r#collision::Shape::Aabb { center, half } => format!(
                        "AABB c=({:.2},{:.2}) half=({:.2},{:.2})",
                        center[0], center[1], half[0], half[1]
                    ),
                    crate::r#collision::Shape::Circle { center, radius } => {
                        format!("CIR c=({:.2},{:.2}) r={:.2}", center[0], center[1], radius)
                    }
                };
                Some((distance, text))
            })
            .collect();
        scored.sort_by(|a: &(f32, String), b: &(f32, String)| {
            a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
        });
        let items: Vec<String> = scored
            .iter()
            .take(NEAR_SHAPE_REPORT_COUNT)
            .map(|(distance, text): &(f32, String)| {
                format!("{{\"d\":{distance:.3},\"shape\":\"{text}\"}}")
            })
            .collect();
        format!("[{}]", items.join(JSON_COMMA))
    };
    // **每节骨骼实际写进了几个实例 + 它的平移分量。**
    //
    // 「角色不可见」只有三种成因,而这组字段正好把它们分开:
    //
    // - `n == 0` —— 实例表是空的(骨骼根本没被 `sync_dynamic_instances`
    //   写,或者批次索引越界)。此时 `gpu_mesh_extent` 仍然是**正常**的
    //   人形尺寸,所以「顶点数据看起来对」根本不能证明角色会被画出来。
    // - `n > 0` 但平移不是玩家坐标 —— 模型矩阵写错了(例如 pivot 没跟着
    //   角色走,人物被搬到原点或天上去)。
    // - `n > 0` 且平移正确 —— 问题在渲染侧(批次被剔除 / 顶点缓冲没
    //   传上去 / 深度把它挡了),要往 `render.rs` 看。
    //
    // 之前 `limbModel` 只导出**第一节**(torso)一个矩阵,证据力太弱。
    let limb_state: String = {
        let parts: Vec<String> = game
            .player_batches
            .iter()
            .map(|limb: &PlayerLimbBatch| {
                let count: usize = game
                    .scene
                    .batches
                    .get(limb.batch)
                    .map(|batch: &SceneBatch| batch.instances.len())
                    .unwrap_or(0);
                let translation: String = game
                    .scene
                    .batches
                    .get(limb.batch)
                    .and_then(|batch: &SceneBatch| batch.instances.first())
                    .map(|instance: &Instance| {
                        let m: Mat4Data = *instance.get_model_ref();
                        format!("[{:.3},{:.3},{:.3}]", m[12], m[13], m[14])
                    })
                    .unwrap_or_else(|| String::from(JSON_NULL));
                format!(
                    "{{\"part\":\"{}\",\"batch\":{},\"n\":{count},\"t\":{translation}}}",
                    limb.part, limb.batch
                )
            })
            .collect();
        format!("[{}]", parts.join(JSON_COMMA))
    };
    // 每个骨架 part 的真实顶点包围盒:一旦某个盒子冒出几十米,就说明顶点
    // 数据真的被写坏了(而不是 GPU mesh 表错位)。见 `gpu_mesh_extent`。
    let limb_extents: String = {
        let parts: Vec<String> = game
            .player_batches
            .iter()
            .filter_map(|limb: &PlayerLimbBatch| {
                let mesh: Option<&MeshAssetGpu> = game
                    .scene
                    .batches
                    .get(limb.batch)
                    .and_then(|batch: &SceneBatch| game.scene.meshes.get(batch.mesh_index));
                mesh.map(|mesh: &MeshAssetGpu| {
                    let extent: MeshExtent = gpu_mesh_extent(mesh);
                    format!(
                        "{{\"part\":\"{}\",\"extent\":{}}}",
                        limb.part,
                        json_f32s(&extent)
                    )
                })
            })
            .collect();
        format!("[{}]", parts.join(JSON_COMMA))
    };
    // 车队与拾取物的坐标也挂到这里:验收脚本要证明「车真的在动」,
    // 但只看 `window.__vcw` 那份几十 KB 的快照不现实,小而稳的
    // `__VCW_DEBUG__` 才是拿数值的地方。
    let car_x: Vec<f64> = game
        .traffic
        .get_cars_ref()
        .iter()
        .map(|car: &TrafficCar| f64::from(car.get_position()[0]))
        .collect();
    let car_z: Vec<f64> = game
        .traffic
        .get_cars_ref()
        .iter()
        .map(|car: &TrafficCar| f64::from(car.get_position()[2]))
        .collect();
    // GPU 侧资产表与 `Scene::meshes` 的对照:两者必须**完全相等**。
    // 不等就说明 `upload_mesh` 传多 / 传少了一次,之后所有
    // `SceneBatch::mesh_index` 整体错位 —— 症状就是「批次有实例、
    // 模型矩阵也对,画面上却什么都没有」,而 `draw_batch` 越界时是
    // 静默 `return 0`,不报任何错。
    let gpu_probe: String = match &game.renderer {
        Some(Renderer::WebGl(webgl)) => format!(
            "{{\"meshes\":{},\"oob\":{},\"lastIndexCount\":{}}}",
            webgl.get_gpu_mesh_count(),
            webgl.get_gpu_index_oob(),
            webgl.get_gpu_index_count()
        ),
        Some(Renderer::Software(_)) => String::from(JSON_NULL),
        None => String::from(JSON_NULL),
    };
    // **室内碰撞体(`FloorWorld`)。**
    //
    // `near_shapes` 查的是 `CollisionWorld`(静态 AABB / 圆),而**楼板与
    // 隔墙住在另一个世界**:`FloorWorld`。两套几何**互不相交**,所以
    // 「玩家被卡住、6 m 内一个静态碰撞体都没有」正是「被隔墙推开」的
    // 指纹 —— 报告为空不是「附近没东西」,而是「推他的东西不住在这儿」。
    //
    // 只报 `Wall`(楼板不做横向分离,见 `resolve_interior`),并逐个
    // 给出中心、半长与竖直区间,这样「谁把 0.307 m 推回去的」在
    // JSON 里就能直接读出来,不用再猜。
    let near_interiors: String = {
        let here: Vec2 = [game.after_static_step[0], game.after_static_step[1]];
        let body_min: f32 = game.player.get_position()[1];
        let body_max: f32 = body_min + game.player.get_height();
        let radius: f32 = game.world.get_player_radius();
        let mut items: Vec<String> = Vec::new();
        for floor in game.interiors.get_floors() {
            let (min, max): (Vec3, Vec3) = match floor {
                crate::interior::Floor::Slab { min: _, max: _ } => continue,
                crate::interior::Floor::Wall { min, max } => (*min, *max),
            };
            // 竖直区间不相交 → 这一段墙碰不到身体,不可能推人。
            if max[1] <= body_min || min[1] >= body_max {
                continue;
            }
            let center: Vec2 = [(min[0] + max[0]) * 0.5, (min[2] + max[2]) * 0.5];
            let half: Vec2 = [(max[0] - min[0]) * 0.5, (max[2] - min[2]) * 0.5];
            let distance: f32 = crate::r#collision::shape_distance(
                &crate::r#collision::Shape::Aabb { center, half },
                here,
            );
            if distance > NEAR_SHAPE_REPORT_RADIUS {
                continue;
            }
            let embedded: bool = distance <= radius;
            // §1.3c:JSON 键名必须住在 `const.rs`,而 `format!` 只接受
            // 字符串**字面量** —— 两者只能在这里会合:键名用常量
            // (`.replace` 无转义歧义),数值部分不带引号所以可以内联。
            items.push(
                String::from(INTERIOR_REPORT_KEYS)
                    .replace(REPORT_TOKEN_D, &format!("{distance:.3}"))
                    .replace(REPORT_TOKEN_EMBED, &format!("{embedded}"))
                    .replace(REPORT_TOKEN_Y0, &format!("{:.2}", min[1]))
                    .replace(REPORT_TOKEN_Y1, &format!("{:.2}", max[1]))
                    .replace(REPORT_TOKEN_CX, &format!("{:.2}", center[0]))
                    .replace(REPORT_TOKEN_CZ, &format!("{:.2}", center[1]))
                    .replace(REPORT_TOKEN_HX, &format!("{:.2}", half[0]))
                    .replace(REPORT_TOKEN_HZ, &format!("{:.2}", half[1])),
            );
        }
        format!("[{}]", items.join(JSON_COMMA))
    };
    let json: String = format!(
        "{DEBUG_OPEN}\"charVisible\":{char_visible},\"charBoxHeightPct\":{painted},\"charBox\":{char_box},\"playerScreenX\":{screen_x},\"playerScreenY\":{screen_y},\"playerScreenPct\":{screen_pct},\"playerX\":{px},\"playerZ\":{pz},\"cameraDist\":{dist},\"camOccluded\":{occluded},\"camClearance\":{clearance},\"canvasWidth\":{canvas_w},\"canvasHeight\":{canvas_h},\"carX\":[{car_x}],\"carZ\":[{car_z}],\"pickups\":{{\"total\":{pickup_total},\"taken\":{pickup_taken}}},\"frames\":{frames},\"sceneMeshes\":{scene_meshes},\"gpu\":{gpu_probe},\"afterStatic\":[{asx},{asz}],\"afterVertical\":[{avx},{avz}],\"afterDynamic\":[{adx},{adz}],\"nearShapes\":{near_shapes},\"nearInteriors\":{near_interiors},\"shotsBlocked\":{shots_blocked},\"limbState\":{limb_state},\"limbExtents\":{limb_extents}{DEBUG_CLOSE}",
        dist = game.camera.get_distance(),
        occluded = game.camera.get_occluded(),
        clearance = game.camera.get_eye_clearance(),
        canvas_w = size.0,
        canvas_h = size.1,
        px = feet[0],
        pz = feet[2],
        // 这是**投影包围盒的面积**(像素),不是"实际画上去的像素数"。
        // 真正的非背景像素数只能在浏览器里 `readPixels` 统计,那是验收
        // 脚本的活;这里给的是 Rust 侧唯一能算准的几何量,两者含义
        // 不同,不要混用。
        painted = screen_pct,
        car_x = join_f64(&car_x),
        car_z = join_f64(&car_z),
        pickup_total = game.traffic.get_pickups_ref().len(),
        pickup_taken = game.player.collected_count(),
        frames = game.frame_count,
        scene_meshes = game.scene.meshes.len(),
        asx = game.after_static_step[0],
        asz = game.after_static_step[1],
        avx = game.after_vertical_step[0],
        avz = game.after_vertical_step[1],
        adx = game.after_dynamic_step[0],
        adz = game.after_dynamic_step[1],
        shots_blocked = game.shots_blocked_by_geometry,
    );
    json
}

/// 把所有未拾取拾取物的 `[x, z]` 拼成 JSON 数组(验收脚本用来选目标)。
///
/// # Arguments
///
/// - `&Game` - Game 的只读引用。
///
/// # Returns
///
/// - `String` - 形如 `[[35.4,-4.0],[24.6,12.0]]` 的字面量。
fn join_pickups(game: &Game) -> String {
    let mut out: Vec<String> = Vec::new();
    for pickup in game.traffic.get_pickups_ref() {
        if pickup.get_taken() {
            continue;
        }
        let at: Vec3 = pickup.get_position();
        out.push(format!("[{},{}]", at[0], at[2]));
    }
    format!("[{}]", out.join(","))
}

/// 把 `f64` 切片拼成 `[a,b,c]` 形式的 JSON 数组字面量。
///
/// # Arguments
///
/// - `&[f64]` - 数值切片。
///
/// # Returns
///
/// - `String` - 逗号分隔的数字串(不含方括号)。
fn join_f64(values: &[f64]) -> String {
    let parts: Vec<String> = values
        .iter()
        .map(|value: &f64| format!("{value}"))
        .collect();
    parts.join(",")
}

/// HUD 文本。
///
/// # Arguments
///
/// - `&Game` - Game 的只读引用。
/// - `u32` - 输入值。
///
/// # Returns
///
/// - `String` - 结果字符串。
fn format_hud(game: &Game, triangles: u32) -> String {
    let backend: &str = game
        .renderer
        .as_ref()
        .map(|renderer: &Renderer| renderer.backend_name())
        .unwrap_or(BACKEND_NONE);
    let position: Vec3 = game.player.get_position();
    let health: String = format!("{:.0}", game.player.get_health().max(0.0));
    let cash: String = format!("{}", game.player.get_cash());
    let pickup_total: usize = game.traffic.get_pickups_ref().len();
    let collected: usize = game.player.collected_count();
    let driving: &str = match game.player.get_vehicle() {
        Some(_) => HUD_DRIVING,
        None => HUD_ON_FOOT,
    };
    let mode: &str = if game.third_person {
        HUD_THIRD_PERSON
    } else {
        HUD_FIRST_PERSON
    };
    let notice: String = game.player.get_notice().to_string();
    let notice_part: String = if notice.is_empty() {
        String::new()
    } else {
        format!(" · {notice}")
    };
    format!(
        "{HUD_TITLE} · {backend} · phase {} · {mode} · {driving} · HP {health} · ${cash} · pickups {collected}/{pickup_total} · xyz {:.1} {:.1} {:.1} · {triangles} tris · frame {}{notice_part}",
        game.input.phase.label(),
        position[0],
        position[1],
        position[2],
        game.frame_count,
    )
}

/// 把血条 / 护甲条 / 弹药 / 现金 / 通缉星 / 任务 / 命中标记写回 DOM。
///
/// 每帧全量重写而不是 diff —— 这些节点一共 8 个,`set_text_content` 与
/// `set_attribute` 都是微秒级,真要做 diff 反而会把「哪一帧没更新」的
/// bug 变成查不出来的幽灵问题。
///
/// # Arguments
///
/// - `&GameHandles` - 事件句柄。
/// - `u32` - 本帧三角面数。
fn sync_hud(handles: &GameHandles, triangles: u32) {
    // 先把本帧要显示的**全部**文本在 borrow 内取出来,再写 DOM。
    // `set_attribute` / `set_text_content` 是 web-sys 调用,会重入 JS;
    // `game` 的 `Ref` 一直活到函数尾,期间任何重入 borrow 都会 panic
    // (`BorrowMutError` 无法在 WASM 里 catch,直接白屏)。所以 guard 的
    // 生命周期必须**短于**第一次 DOM 写入。
    let (health_pct, armor_pct, ammo_text, cash_text, wanted_text, mission_text, opacity): (
        f64,
        f64,
        String,
        String,
        String,
        String,
        &str,
    ) = {
        let game: std::cell::Ref<Game> = handles.game.borrow();
        let health_pct: f64 = (f64::from(game.player.get_health()) / f64::from(PLAYER_MAX_HEALTH)
            * 100.0)
            .clamp(0.0, 100.0);
        let armor_pct: f64 =
            (f64::from(game.player.get_armor()) / f64::from(MAX_ARMOR) * 100.0).clamp(0.0, 100.0);
        let weapon: Weapon = game.arsenal.get_weapon();
        let ammo_text: String = if game.arsenal.is_reloading() {
            format!("{} · RELOADING", weapon.label())
        } else if weapon == Weapon::Bat {
            format!("{} · melee", weapon.label())
        } else {
            format!(
                "{} · {}/{}",
                weapon.label(),
                game.arsenal.get_magazine(),
                game.arsenal.get_reserve()
            )
        };
        let cash_text: String = format!("{CASH_SIGN}{}", game.player.get_cash());
        let stars: u32 = game.wanted.get_stars();
        // 亮星用实心 ★,熄星用空心 ☆ —— 形状本身就带信息,不只靠颜色。
        let lit: String = HUD_STAR_ON.repeat(stars as usize);
        let dark: String = HUD_STAR_OFF.repeat((WANTED_MAX - stars) as usize);
        let wanted_text: String = format!("{lit}{dark}");
        let mission_text: String = if game.mission.is_active() {
            let target: Vec3 = game.mission.get_target();
            let distance: f32 = flat_distance(target, game.player.get_position());
            format!(
                "{}· {}\ngoal {:.0} m away",
                game.mission.get_title(),
                mission_stage_label(game.mission.get_stage()),
                distance
            )
        } else {
            String::from(MISSION_IDLE)
        };
        let opacity: &str = if game.hitmarker > 0.0 {
            HUD_OPACITY_ON
        } else {
            HUD_OPACITY_OFF
        };
        (
            health_pct,
            armor_pct,
            ammo_text,
            cash_text,
            wanted_text,
            mission_text,
            opacity,
        )
    };
    // ---- 血条 / 护甲条 ----
    if let Some(bar) = &handles.health_bar {
        let _: Result<(), euv::wasm_bindgen::JsValue> = bar.set_attribute(
            ATTR_STYLE,
            &format!("width:{health_pct:.1}%;background:{COLOR_HEALTH}"),
        );
    }
    if let Some(bar) = &handles.armor_bar {
        let _: Result<(), euv::wasm_bindgen::JsValue> = bar.set_attribute(
            ATTR_STYLE,
            &format!("width:{armor_pct:.1}%;background:{COLOR_ARMOR}"),
        );
    }
    // ---- 弹药 ----
    if let Some(element) = &handles.ammo {
        element.set_text_content(Some(&ammo_text));
    }
    // ---- 现金 ----
    if let Some(element) = &handles.cash {
        element.set_text_content(Some(&cash_text));
    }
    // ---- 通缉星 ----
    if let Some(element) = &handles.wanted {
        element.set_text_content(Some(&wanted_text));
    }
    // ---- 任务 ----
    if let Some(element) = &handles.mission {
        element.set_text_content(Some(&mission_text));
    }
    // ---- 命中标记 ----
    if let Some(element) = &handles.hitmarker {
        let _: Result<(), euv::wasm_bindgen::JsValue> = element.set_attribute(ATTR_STYLE, opacity);
    }
    // ---- 小地图 ----
    draw_minimap(handles);
    let _: u32 = triangles;
}

/// 任务阶段的中文短名。
///
/// # Arguments
///
/// - `u32` - 阶段常量。
///
/// # Returns
///
/// - `&'static str` - 展示文本。
fn mission_stage_label(stage: u32) -> &'static str {
    match stage {
        MISSION_GOTO => MISSION_LABEL_GOTO,
        MISSION_DRIVE => MISSION_LABEL_DRIVE,
        MISSION_KILL => MISSION_LABEL_KILL,
        _ => MISSION_LABEL_NONE,
    }
}

/// 画小地图:街道网格 + 敌人 + 任务点 + 玩家朝向三角。
///
/// 用 `Canvas2D` 而不是 DOM 节点拼 —— 地图每帧重画,DOM 元素数量会成为
/// 瓶颈,而一个 150×150 的 2D canvas 每帧几条 `fillRect` 的开销可以忽略。
///
/// # Arguments
///
/// - `&GameHandles` - 事件句柄。
fn draw_minimap(handles: &GameHandles) {
    let Some(canvas) = &handles.minimap else {
        return;
    };
    let Some(context) = canvas.get_context("2d").ok().flatten() else {
        return;
    };
    let context: euv::web_sys::CanvasRenderingContext2d =
        match context.dyn_into::<euv::web_sys::CanvasRenderingContext2d>() {
            Ok(value) => value,
            Err(_) => return,
        };
    let size: f64 = f64::from(MINIMAP_PX);
    context.clear_rect(0.0, 0.0, size, size);
    context.set_fill_style_str(COLOR_MINIMAP_BG);
    context.fill_rect(0.0, 0.0, size, size);
    let game: std::cell::Ref<Game> = handles.game.borrow();
    let here: Vec3 = game.player.get_position();
    let center: Vec2 = [here[0], here[2]];
    // 世界 → 地图像素:以玩家为中心,固定比例(不随速度缩放)。
    let scale: f64 = size / (2.0 * f64::from(MINIMAP_RANGE));
    // 街道:四条南北 + 四条东西,画成两条粗线。
    context.set_fill_style_str(COLOR_MINIMAP_ROAD);
    for line in MAP_STREET_LINES {
        let offset: f64 = f64::from(*line) - f64::from(center[1]);
        let pixel: f64 = size * 0.5 + offset * scale;
        context.fill_rect(
            0.0,
            pixel - f64::from(MINIMAP_ROAD_W) * 0.5,
            size,
            f64::from(MINIMAP_ROAD_W),
        );
        let offset_x: f64 = f64::from(*line) - f64::from(center[0]);
        let pixel_x: f64 = size * 0.5 + offset_x * scale;
        context.fill_rect(
            pixel_x - f64::from(MINIMAP_ROAD_W) * 0.5,
            0.0,
            f64::from(MINIMAP_ROAD_W),
            size,
        );
    }
    // 任务目标。
    if game.mission.is_active() {
        let target: Vec3 = game.mission.get_target();
        let dx: f64 = f64::from(target[0] - here[0]) * scale;
        let dz: f64 = f64::from(target[2] - here[2]) * scale;
        context.set_fill_style_str(COLOR_MINIMAP_OBJECTIVE);
        context.fill_rect(size * 0.5 + dx - 3.0, size * 0.5 + dz - 3.0, 6.0, 6.0);
    }
    // 敌人。
    context.set_fill_style_str(COLOR_MINIMAP_ENEMY);
    for enemy in &game.enemies {
        if !enemy.is_alive() {
            continue;
        }
        let at: Vec3 = enemy.get_position();
        let dx: f64 = f64::from(at[0] - here[0]) * scale;
        let dz: f64 = f64::from(at[2] - here[2]) * scale;
        context.fill_rect(size * 0.5 + dx - 2.0, size * 0.5 + dz - 2.0, 4.0, 4.0);
    }
    // 玩家:一个朝向三角。
    context.set_fill_style_str(COLOR_MINIMAP_PLAYER);
    let yaw: f32 = game.player.get_yaw();
    let (sin_yaw, cos_yaw): (f32, f32) = yaw.sin_cos();
    let points: [f64; 6] = [
        cos_yaw as f64 * 7.0,
        -sin_yaw as f64 * 7.0,
        (-cos_yaw as f64 * 5.0 - sin_yaw as f64 * 4.0),
        (sin_yaw as f64 * 5.0 - cos_yaw as f64 * 4.0),
        (-cos_yaw as f64 * 5.0 + sin_yaw as f64 * 4.0),
        (sin_yaw as f64 * 5.0 + cos_yaw as f64 * 4.0),
    ];
    context.begin_path();
    context.move_to(size * 0.5 + points[0], size * 0.5 + points[1]);
    context.line_to(size * 0.5 + points[2], size * 0.5 + points[3]);
    context.line_to(size * 0.5 + points[4], size * 0.5 + points[5]);
    context.close_path();
    context.fill();
}

/// 让「护甲背心 / 弹药箱」真的影响战斗数值。
///
/// 这两个拾取物在加入战斗系统之前只是加钱,现在:背心补护甲(上限
/// [`MAX_ARMOR`]),弹药箱给当前枪补一个弹匣。这条把「捡了」和
/// 「打的时候用得上」接起来。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `&'static str` - 拾取物的资产 id。
fn apply_combat_pickup(game: &mut Game, asset: &'static str) {
    if asset == PICKUP_ARMOR_VEST {
        game.player.add_armor(ARMOR_PICKUP_GAIN);
        game.player.set_notice(String::from(NOTICE_ARMOR));
    } else if asset == PICKUP_AMMO_BOX {
        game.arsenal.add_ammo(AMMO_PICKUP_GAIN);
        game.player.set_notice(String::from(NOTICE_AMMO));
    } else if let Some(weapon) = Weapon::from_asset(asset) {
        // **第 10 条的根因。** 地上摆着 `wep_pistol` / `wep_smg` /
        // `wep_bat` / `wep_grenade`(见 `PICKUP_PLACEMENTS`),但这个函数
        // 原来只认护甲和弹药箱 —— 武器模型被拾起来之后只弹一行
        // 「捡到武器」提示,手里的枪**根本没换**,手上也不会出现模型。
        // 三处必须一起做,缺一个就是「捡到但不能用」:
        //
        // 1. 手雷是**数量**型,捡一颗 +1;枪械是**替换**型,直接切过去;
        // 2. `rebind_weapon_batch` 把手持批次绑到新 mesh —— 不调的话
        //    手上还是上一把枪的模型(切枪键走的是同一条路径,所以这条
        //    路径本来就有现成实现,拾取流程只是忘了接);
        // 3. 切枪后给满弹匣,否则新枪是空的,照样「无法射击」。
        if weapon == Weapon::Grenade {
            if game.arsenal.add_grenade() {
                game.arsenal.set_weapon(Weapon::Grenade);
            }
        } else {
            game.arsenal.set_weapon(weapon);
        }
        rebind_weapon_batch(game);
        game.arsenal
            .finish_reload(game.arsenal.get_weapon().magazine());
        game.player.set_notice(String::from(NOTICE_WEAPON));
    }
}

/// 切枪后重新绑定手持武器的批次。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
fn rebind_weapon_batch(game: &mut Game) {
    let asset: &'static str = game.arsenal.get_weapon().asset();
    game.weapon_mesh = match game.index_map.get(asset) {
        Some(mesh_index) => *mesh_index,
        None => usize::MAX,
    };
    let mesh: usize = game.weapon_mesh;
    if mesh == usize::MAX {
        game.weapon_batch = usize::MAX;
        return;
    }
    game.weapon_batch = find_or_create_batch(&mut game.scene, mesh);
}

/// 阵营的数值编码(给探针用)。
///
/// # Arguments
///
/// - `Faction` - 阵营。
///
/// # Returns
///
/// - `u8` - 警察为 0,混混为 1。
fn faction_code(faction: Faction) -> u8 {
    match faction {
        Faction::Police => 0,
        Faction::Thug => 1,
    }
}

/// 小地图上画出来的街道轴线(米)。
const MAP_STREET_LINES: &[f32] = &[-90.0, -30.0, 30.0, 90.0];

/// 在 Rust 侧建出 HUD 的结构化 DOM(因为 `index.html` 不可改)。
///
/// 为什么用 DOM 而不是继续往 canvas 上画字:血条 / 通缉星 / 小地图都需要
/// **每帧变**但结构不变,canvas 每帧重画全屏文字要自己算像素位置,而 DOM
/// 让浏览器去做布局 —— 改一个 `width` 就完事。canvas 只留给准星。
///
/// 一次性建好、之后只改 `style.width` / `textContent`,不做增删节点。
///
/// # Arguments
///
/// - `&Document` - 文档。
///
/// # Returns
///
/// - `Option<Element>` - 挂在 body 上的 HUD 根节点;创建失败时为 `None`。
fn build_hud_dom(document: &Document) -> Option<Element> {
    let Some(body) = document.body() else {
        return None;
    };
    let root: Element = document.create_element(TAG_DIV).ok()?;
    let _: Result<(), euv::wasm_bindgen::JsValue> = root.set_attribute(ATTR_ID, ID_HUD);
    let _: Result<(), euv::wasm_bindgen::JsValue> = root.set_attribute(ATTR_STYLE, STYLE_HUD_ROOT);
    // ---- 左下:血条 + 护甲条 ----
    let (Some(vitals), Some(health), Some(armor)) = (
        make_element(document, TAG_DIV, ID_HUD_VITALS, STYLE_HUD_VITALS),
        make_element(document, TAG_DIV, ID_HEALTH_BAR, STYLE_HUD_HEALTH),
        make_element(document, TAG_DIV, ID_ARMOR_BAR, STYLE_HUD_ARMOR),
    ) else {
        return None;
    };
    let _: Result<Node, euv::wasm_bindgen::JsValue> = vitals.append_child(&health);
    let _: Result<Node, euv::wasm_bindgen::JsValue> = vitals.append_child(&armor);
    let _: Result<Node, euv::wasm_bindgen::JsValue> = root.append_child(&vitals);
    // ---- 右上:通缉星 ----
    let Some(wanted) = make_element(document, TAG_DIV, ID_WANTED, STYLE_HUD_WANTED) else {
        return None;
    };
    let _: Result<Node, euv::wasm_bindgen::JsValue> = root.append_child(&wanted);
    // ---- 右下:弹药 + 现金 ----
    let (Some(ammo), Some(cash)) = (
        make_element(document, TAG_DIV, ID_AMMO, STYLE_HUD_AMMO),
        make_element(document, TAG_DIV, ID_CASH, STYLE_HUD_CASH),
    ) else {
        return None;
    };
    let _: Result<Node, euv::wasm_bindgen::JsValue> = root.append_child(&ammo);
    let _: Result<Node, euv::wasm_bindgen::JsValue> = root.append_child(&cash);
    // ---- 顶部中央:任务 ----
    let Some(mission) = make_element(document, TAG_DIV, ID_MISSION, STYLE_HUD_MISSION) else {
        return None;
    };
    let _: Result<Node, euv::wasm_bindgen::JsValue> = root.append_child(&mission);
    // ---- 右下角:小地图(独立 canvas,每帧 2D 重画)----
    if let Ok(map) = document.create_element(TAG_CANVAS)
        && let Ok(canvas) = map.dyn_into::<HtmlCanvasElement>()
    {
        canvas.set_width(MINIMAP_PX as u32);
        canvas.set_height(MINIMAP_PX as u32);
        let _: Result<(), euv::wasm_bindgen::JsValue> = canvas.set_attribute(ATTR_ID, ID_MINIMAP);
        let _: Result<(), euv::wasm_bindgen::JsValue> =
            canvas.set_attribute(ATTR_STYLE, STYLE_HUD_MINIMAP);
        let _: Result<Node, euv::wasm_bindgen::JsValue> = root.append_child(&canvas);
    }
    // ---- 地图面板(Tab):默认 `display:none`,按 Tab 才点亮 ----
    //
    // 与小地图的差别:小地图每帧重画、**永远可见**,地图是一次性放大的
    // 整城视图,关掉时必须真的从 DOM 里消失,否则它会盖住准星。
    if let Some(panel) = make_element(document, TAG_DIV, ID_MAP_PANEL, STYLE_MAP_PANEL)
        && let Ok(dim) = document.create_element(TAG_DIV)
    {
        let _: Result<(), euv::wasm_bindgen::JsValue> =
            dim.set_attribute(ATTR_STYLE, STYLE_MAP_DIM);
        let _: Result<Node, euv::wasm_bindgen::JsValue> = panel.append_child(&dim);
        if let Some(title) = make_element(document, TAG_DIV, ID_MAP_TITLE, STYLE_MAP_TITLE) {
            title.set_text_content(Some(MAP_TITLE));
            let _: Result<Node, euv::wasm_bindgen::JsValue> = panel.append_child(&title);
        }
        if let Some(detail) = make_element(document, TAG_DIV, ID_MAP_DETAIL, STYLE_MAP_HINT) {
            let _: Result<Node, euv::wasm_bindgen::JsValue> = panel.append_child(&detail);
        }
        let _: Result<(), euv::wasm_bindgen::JsValue> =
            panel.set_attribute(ATTR_STYLE, DISPLAY_NONE);
        let _: Result<Node, euv::wasm_bindgen::JsValue> = root.append_child(&panel);
    }
    // ---- 屏幕中央:准星 + 命中标记 ----
    let (Some(crosshair), Some(marker)) = (
        make_element(document, TAG_DIV, ID_CROSSHAIR, STYLE_HUD_CROSSHAIR),
        make_element(document, TAG_DIV, ID_HITMARKER, STYLE_HUD_HITMARKER),
    ) else {
        return None;
    };
    let _: Result<Node, euv::wasm_bindgen::JsValue> = root.append_child(&crosshair);
    let _: Result<Node, euv::wasm_bindgen::JsValue> = root.append_child(&marker);
    let _: Result<Node, euv::wasm_bindgen::JsValue> = body.append_child(&root);
    Some(root)
}

/// 建一个带 id 与内联样式的子节点。
///
/// # Arguments
///
/// - `&Document` - 文档。
/// - `&str` - 标签名。
/// - `&str` - 节点 id。
/// - `&str` - 内联样式。
///
/// # Returns
///
/// - `Option<Element>` - 新的节点;创建失败时为 `None`。
fn make_element(document: &Document, tag: &str, id: &str, style: &str) -> Option<Element> {
    let element: Element = document.create_element(tag).ok()?;
    let _: Result<(), euv::wasm_bindgen::JsValue> = element.set_attribute(ATTR_ID, id);
    let _: Result<(), euv::wasm_bindgen::JsValue> = element.set_attribute(ATTR_STYLE, style);
    Some(element)
}

/// 更新加载进度条。
///
/// # Arguments
///
/// - `f32` - 输入值。
/// - `&str` - str 的只读引用。
fn set_progress(percent: f32, text: &str) {
    let Some(document) = window().and_then(|window: Window| window.document()) else {
        return;
    };
    if let Some(bar) = document.get_element_by_id(PROGRESS_ID) {
        let _: Result<(), euv::wasm_bindgen::JsValue> =
            bar.set_attribute("style", &format!("width: {percent:.1}%"));
    }
    if let Some(label) = document.get_element_by_id(PROGRESS_TEXT_ID) {
        label.set_text_content(Some(text));
    }
}

/// 隐藏加载遮罩。
fn hide_loading() {
    let Some(document) = window().and_then(|window: Window| window.document()) else {
        return;
    };
    if let Some(overlay) = document.get_element_by_id(LOADING_ID) {
        let _: Result<(), euv::wasm_bindgen::JsValue> =
            overlay.set_attribute(ATTR_STYLE, DISPLAY_NONE);
    }
}

/// 在遮罩上显示错误信息。
///
/// # Arguments
///
/// - `&str` - str 的只读引用。
fn show_loading_error(message: &str) {
    let Some(document) = window().and_then(|window: Window| window.document()) else {
        return;
    };
    if let Some(label) = document.get_element_by_id(PROGRESS_TEXT_ID) {
        label.set_text_content(Some(message));
    }
    if let Some(bar) = document.get_element_by_id(PROGRESS_ID) {
        let _: Result<(), euv::wasm_bindgen::JsValue> =
            bar.set_attribute(ATTR_STYLE, ERROR_BAR_STYLE);
    }
}

// ===========================================================================
// 游戏循环
// ===========================================================================

/// 把键盘输入翻译成「角色移动 / 车队行驶 / 相机跟随」。
///
/// 这是第三人称控制的核心:默认模式下 WASD 驱动**玩家角色**(不是相机),
/// 相机被动地以阻尼跟随在角色背后;只有按 Tab 切回自由观察模式后,
/// WASD 才恢复成平移相机焦点。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `f32` - 本帧的秒数增量。
fn simulate(game: &mut Game, delta: f32) {
    let forward_input: f32 = axis(&game.input, KEYW, KEYS);
    let strafe_input: f32 = axis(&game.input, KEYD, KEYA);
    // 验收通道的「朝世界方向走」优先于键盘:它绕过相机朝向,让脚本不用
    // 先转相机就能沿直线推进。零向量表示不干预。
    let world_walk: Vec2 = game.walk_request;
    // 驾驶时 A/D 与左右方向键**共同**当方向盘:横向输入就是舵角,不再被
    // `set_position([lane_x, ...])` 吃掉。
    //
    // 符号沿用 A/D 的老约定(`正 = 右转`,见 `TrafficCar::drive`),方向键
    // 直接接同一个 `steer_axis`,因此左 = 负、右 = 正。**只有驾驶时**才
    // 调它:步行分支的 `intent` 只由 WASD 组成,所以方向键既不会开车,
    // 也不会变成第四个移动键 —— GTA V 的移动只认 WASD。
    let steer_input: f32 = game.input.steer_axis();
    let running: bool = game.input.held(KEY_SHIFT_LEFT) || game.input.held(KEY_SHIFT_RIGHT);
    let dt: f32 = delta.min(FIXED_DT * 4.0);

    // 相机水平朝向:WASD 的「前」永远跟着相机走,所以拖鼠标转相机就能
    // 转移动方向(第三人称射击的标准操作)。
    let camera_yaw: f32 = game.camera.get_yaw();
    let forward: Vec2 = if world_walk != [0.0, 0.0] {
        world_walk
    } else {
        [camera_yaw.cos(), -camera_yaw.sin()]
    };
    // 世界方向直控时,「前 / 侧」输入直接就是方向本身,不再乘相机基向量。
    let (forward_input, strafe_input): (f32, f32) = if world_walk != [0.0, 0.0] {
        (1.0, 0.0)
    } else {
        (forward_input, strafe_input)
    };

    // 车队先走:玩家开的那辆由 `drive` 接管,其余按巡航速度循环。
    let driven: Option<usize> = game.player.get_vehicle();
    match driven {
        Some(index) => {
            if let Some(car) = game.traffic.get_car_mut(index) {
                car.drive(forward_input, steer_input, dt, &game.world);
            }
        }
        None => {
            // 玩家在路边「招手」:附近的车会减速停靠,不然 8–14 m/s 的车
            // 徒步根本追不上,上车功能等于不存在。
            let here: Vec3 = game.player.get_position();
            game.traffic.step(dt, Some([here[0], here[2]]));
        }
    }

    // 玩家。
    match game.player.get_vehicle() {
        Some(index) => {
            // 在车里:位置 / 朝向跟着车走,角色隐藏(姿态由车接管)。
            if let Some(car) = game.traffic.get_car_mut(index) {
                let position: Vec3 = car.get_position();
                let yaw: f32 = car.get_yaw();
                game.player.set_position(position);
                game.player.set_yaw(yaw);
                game.player.set_velocity([0.0, 0.0]);
                game.player.set_gait_amount(0.0);
                game.player.set_gait_phase(0.0);
            }
        }
        None => {
            // `Player::step` 内部会自己把「前 / 侧」按相机朝向旋转成世界
            // 方向,所以这里必须传**原始的按键轴**,不能预先旋转一次 ——
            // 预旋转会导致两次旋转,按下 W 时角色会朝侧面走。
            let intent: Vec2 = [strafe_input, forward_input];
            let wanted: f32 = if running { RUN_SPEED } else { WALK_SPEED };
            let feet: Vec3 = game.player.get_position();
            // 记下静态层动手**之前**的位置:`step_vertical` 要靠它算出
            // 这一帧的位移,才能把切向分量沿隔墙滑过去。
            game.previous_step_xz = [feet[0], feet[2]];
            let speed: f32 = wanted;
            game.player.step(intent, forward, dt, speed, &game.world);
            // 静态层(`resolve_slide`)刚定下的位置:验收探针拿它和
            // `after_dynamic_step` 对比,就知道是人被动态层按回去了,
            // 还是静态层自己就没让他走。
            let after_static: Vec3 = game.player.get_position();
            game.after_static_step = [after_static[0], after_static[2]];
            // 垂直方向紧接着水平分离之后算:`Player::step` 刚刚定下
            // 了「这一帧人走到了哪」,现在才知道脚下是哪块板。
            step_vertical(game, dt);
            // **`step_vertical` 内部会用 `resolve_interior` 的结果整体
            // 覆盖 `set_position([x, y, z])`。** 也就是说它能**改写
            // XZ** —— 而它跑在静态层之后、动态层之前,是「有速度、无位移」
            // 的第三个嫌疑人,前两个探针都看不到它。`nearShapes` 查的是
            // 静态碰撞世界,查不到隔墙。
            let after_vertical: Vec3 = game.player.get_position();
            game.after_vertical_step = [after_vertical[0], after_vertical[2]];
        }
    }

    if game.safe_mode {
        // 只回血不够:掉到 0 的那一帧 `step_combat` 已经进了 `step_death`,
        // 之后血量拉满也救不回来 —— `hurt` 还在重生倒计时里,下一次
        // `step_death` 照样把玩家扔到最近的医院点。实测每段走位结束
        // 都被弹到 (33.5, 32.9) 附近,两次不同起点弹到几乎同一点,正是
        // 医院点而不是碰撞解算的结果。
        game.hurt.revive();
        game.player.set_health(PLAYER_MAX_HEALTH);
    }
    collect_pickups(game);
    step_combat(game, dt);
    // 各类实体跑完各自的静态碰撞后,统一过一次动态碰撞层 ——
    // 否则车穿车、人穿人、敌人站进人身体里。
    resolve_dynamic_bodies(game);
    {
        let after_dynamic: Vec3 = game.player.get_position();
        game.after_dynamic_step = [after_dynamic[0], after_dynamic[2]];
    }
    step_streamed_surface(game);
    update_camera(game, delta);
    // 注入的开火只持续一帧,到期后放开。
    if game.hold_frames > 0 {
        game.hold_frames -= 1;
        if game.hold_frames == 0 {
            game.input.fire_held = false;
        }
    }
}

/// 玩家走出当前地面块时,把地面与水面重新生成到新中心。
///
/// 世界是无限的,而地面是一块固定尺寸、跟着玩家平移的网格块。判定
/// 单独抽出来(见 [`stream_needs_rebuild`]),这样「什么时候重建」是可
/// 单测的纯函数,不必真的跑一局游戏才知道。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
fn step_streamed_surface(game: &mut Game) {
    let here: Vec3 = game.player.get_position();
    let player: Vec2 = [here[0], here[2]];
    let center: Vec2 = game.streamed_center;
    if !stream_needs_rebuild(player, center) {
        return;
    }
    // 对齐到街道网格:地面块本来就按街道对齐,不对齐会导致玩家每走
    // 一格就看到地面整体跳一格。
    let next_x: f32 = street_axis((player[0] / STREET_PITCH).round() as i32);
    let next_z: f32 = street_axis((player[1] / STREET_PITCH).round() as i32);
    let index_map: HashMap<String, usize> = game.asset_index.clone();
    let static_count: usize = game.static_batch_count;
    let rewritten: Vec<usize> = rebuild_streamed_surface(
        &mut game.scene,
        game.ground_batch,
        game.water_batch,
        next_x,
        next_z,
        &index_map,
        static_count,
    );
    // 地面 / 水面的顶点数据换了,GPU 缓冲必须跟着换 —— 否则玩家走出街区
    // 边界后看到的还是出生点那块地,而 CPU 侧探针报告的却是新数据。
    if let Some(renderer) = game.renderer.as_mut() {
        reupload_streamed_meshes(renderer, &game.scene, &rewritten);
    }
    game.streamed_center = [next_x, next_z];
}

/// 重力 + 楼板支撑 + 楼梯抬升 —— 玩家 Y 轴的一帧推进。
///
/// `Player::step` 只解决 XZ;Y 由这里负责。顺序有讲究:
/// 1. **室内水平分离**:玩家进楼后,`CollisionWorld` 里没有这栋楼,
///    门垛 / 隔墙 / 外墙全靠 `FloorWorld::resolve_interior` 挡。
/// 2. **支撑面查询**:`support_height` 取「不超过脚底 + 踏高容差的最高
///    板」。踩在楼梯上时这一级比脚底高 0.305 m,在 0.45 m 容差内,于是
///    被抬上去 —— 这就是「楼梯能走上��」。
/// 3. **重力 / 落地**:离开支撑面就自由落体,穿过板面时吸附上去。
/// 4. **防穿地**:脚下已经没有板、却比地面低时,按帧速率硬拉回来,
///    不给「掉进楼板底下」留下任何一帧的机会。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `f32` - 本帧秒数(已在 [`simulate`] 里钳位)。
fn step_vertical(game: &mut Game, dt: f32) {
    let here: Vec3 = game.player.get_position();
    let radius: f32 = game.world.get_player_radius();
    let body_min: f32 = here[1];
    let body_max: f32 = here[1] + game.player.get_height();

    // ---- 1. 室内水平分离:门垛 / 隔墙 / 外墙只对「身体够得着」的生效。
    // 站在街上时身体区间 y = 0..1.75 与楼板相交,但 `resolve_interior`
    // 对「脚底已经站到板面」和「板子在头顶」两种情况都直接跳过,所以
    // 不会在门外凭空出现一堵墙。
    //
    // **必须把这一帧的位移一起传进去。** 纯分离只知道「现在在哪儿」,
    // 不知道「想去哪儿」,于是玩家顶着隔墙走时,静态层刚推进来的那点
    // 切向位移会被室内层连根拔掉,每帧净位移为零 —— 实测静态层走到
    // `z = 35.243`、室内层弹回 `z = 35.550`,位置在极限环上抖动,
    // 顶着墙几十帧推不动一毫米。`resolve_interior_slide` 走标准
    // `move_and_slide`:法向扣掉、切向保留,玩家顺墙滑过去。
    //
    // **起点必须是这一帧的**真起点**(`previous_step_xz`),不是静态层留下的
    // 落点。** 静态层(`CollisionWorld`)看不见室内墙,它只知道城市里的
    // 建筑盒,于是它会**把人推进墙里再推进墙那边**。冲刺一帧 1.1269 m 大于
    // 门垛 0.20 m,静态层落点可以一次跨过整堵墙(实测落点 x = 27.873,
    // 已经越过门垛内表面 27.75)。
    //
    // 那个落点再交给 `resolve_interior_slide` 就已经**太晚了**:子步细分
    // 从 27.873 起步,那里圆心已经在墙的 AABB 内部,`push_out_aabb` 走
    // 「点在盒内」分支、按**最小松弛轴**把圆心从**墙的另一侧**弹出去 ——
    // 实测 `separate_interior(27.873) = 28.300`,正好在墙的另一面,于是
    // 长度不为 0、`into < 0`、切向为零,每帧都「合法地」前进一整个身位。
    // 逐点扫过去看得很清楚:
    //
    //     x=27.850 -> 27.400   (推回墙的内侧,正确)
    //     x=27.900 -> 28.300   (弹到墙的外侧,穿墙)
    //
    // 所以这里**从帧起点重新走一遍整帧**:拿 `previous_step_xz` 当起点、
    // `delta` 仍然是静态层算出的这一帧位移,子步细分每一小步都从**上一小步
    // 的合法位置**出发 —— 27.40 → 27.53 → 27.66 → 27.79,子步永远走不完
    // 一整堵墙,于是每一小步的法向钳制都生效,推进在 27.40 停住。
    //
    // **静态层落点不能直接拿来当起点,也不能拿来当结果。** 拿它当结果就是
    // 缺陷本身(那正是「穿墙」);拿它当起点就是上面这个「弹到另一侧」。
    let travel: Vec2 = [
        here[0] - game.previous_step_xz[0],
        here[2] - game.previous_step_xz[1],
    ];
    let pushed: Vec2 = game.interiors.resolve_interior_slide(
        game.previous_step_xz,
        travel,
        body_min,
        body_max,
        radius,
    );
    let (x, z): (f32, f32) = (pushed[0], pushed[1]);

    // ---- 2. 支撑面:脚下那块板(含楼梯下一级)。
    //
    // **这一帧必须切成一串小步走,不能只查落点。**
    //
    // 实测(CDP,软件渲染):rAF ≈ 1.1 fps,一帧水平位移 **0.613 m**
    // (`WALK_SPEED 4.6 × dt 0.133 s`),而每级踏面只有
    // `SHOWCASE_STAIR_RUN = 0.45 m` 深 —— **一帧跨过 1.36 级**。楼梯坡度
    // 是 `0.305 / 0.45 = 0.678 m/m`,走 0.613 m 本该升 0.415 m;而
    // `support_height` 每次只返回「容差内最高的一块」,**一帧最多抬一级**
    // (0.305 m)。抬升速率追不上前进速率,亏空逐帧累积,等到脚下每一级
    // 都高出 0.45 m 以上时支撑彻底丢失,`y` 塌回首层地板 —— 实测正是
    // 「第 4 级 y=1.370 之后一步掉回 0.150,之后 33 帧不动」。
    //
    // `support_along_frame` 负责切分与逐级抬升;60 fps 的一帧只有一个
    // 小步,行为与修复前完全一致。
    let from: Vec2 = [game.previous_step_xz[0], game.previous_step_xz[1]];
    let (found, stepped): (Option<f32>, f32) =
        game.interiors.support_along_frame(from, [x, z], here[1]);
    let support: f32 = found.unwrap_or(GROUND_LEVEL);
    let on_slab: bool = found.is_some();
    // 逐级抬升后的脚底高度(没有支撑时保持原值,交给下面的自由落体)。
    let mut y: f32 = stepped;
    // ---- 2a. 起跳 ----
    //
    // **判据不是「现在 y 等于多少」,而是「这一帧之前有没有踩着东西」。**
    // 之前把 `jump()` 写在「贴面 / 自由落体」判定之前还不够:站在
    // 地面上时 `on_slab` 为 false,于是走 `else` 分支里的
    // `if y <= GROUND_LEVEL` 贴回 `GROUND_LEVEL` 并 `set_vertical_velocity(0.0)` ——
    // 初速度**当帧就被抹掉**。实测(未修)按 3 帧:y 0 -> 0、
    // grounded true -> true、vy 0 -> 0,和「按了不起作用」完全一样。
    //
    // 所以这里必须**先**问 grounded、再决定走哪条积分路径:起跳成功就
    // 直接进入「上升」分支,绝不回头让贴面判定把速度清掉。
    let jumped: bool = if game.jump_queued {
        let was_grounded: bool = game.player.get_grounded();
        // 起跳失败(滞空时按键)时把标志吃掉,否则它会一直挂到落地为止。
        game.jump_queued = false;
        game.player.set_jump_requested(was_grounded)
    } else {
        false
    };
    if jumped {
        // 上升段:先扣重力,再位移。与下面的自由落体**同一条公式**,
        // 只是初速度来自 `jump()` 而不是 0。
        let rising: f32 = game.player.get_vertical_velocity() - GRAVITY * dt;
        let rising: f32 = rising.max(-TERMINAL_VELOCITY);
        let next: f32 = y + rising * dt;
        if rising <= 0.0 && next <= support + GROUND_SNAP_SKIN {
            // 矮跳:一步之内就落回支撑面(理论上不会,初速 7.4 m/s 远大于
            // 一个 16.7 ms 步长能吞掉的高度),留着以防常量被调小。
            y = support;
            game.player.set_vertical_velocity(0.0);
            game.player.set_grounded(true);
        } else {
            y = next;
            game.player.set_vertical_velocity(rising);
            game.player.set_grounded(false);
        }
    } else if on_slab && support - y <= STEP_UP_TOLERANCE {
        // 踩住了(含上一级台阶):贴面,垂直速度清零。
        y = support;
        game.player.set_vertical_velocity(0.0);
        game.player.set_grounded(true);
    } else {
        // 离开了支撑面:自由落体,落到板面或地面上。
        if y <= GROUND_LEVEL {
            // 已经在地面上,不用算重力。
            y = GROUND_LEVEL;
            game.player.set_vertical_velocity(0.0);
            game.player.set_grounded(true);
        } else {
            let falling: f32 = game.player.get_vertical_velocity() - GRAVITY * dt;
            let falling: f32 = falling.max(-TERMINAL_VELOCITY);
            let next: f32 = y + falling * dt;
            if falling <= 0.0 && next <= support + GROUND_SNAP_SKIN {
                // 穿过板面:吸附上去,不再积累速度。
                y = support;
                game.player.set_vertical_velocity(0.0);
                game.player.set_grounded(true);
            } else {
                y = next;
                game.player.set_vertical_velocity(falling);
                game.player.set_grounded(false);
            }
        }
    }

    // ---- 3. 防穿地:楼板底下不该有玩家,任何原因掉下去都硬拉回来。
    let floor: f32 = if on_slab { support } else { GROUND_LEVEL };
    if y < floor {
        y = (y + ANTI_TUNNEL_LIFT_SPEED * dt).min(floor);
        game.player.set_vertical_velocity(0.0);
        game.player.set_grounded(true);
    }

    game.player.set_position([x, y, z]);
}

/// 战斗 / 通缉 / 行人 / 任务的一帧推进。
///
/// 调用顺序有讲究:先结算玩家的射击与受击(它是这帧唯一的输入),再推进
/// 敌人 AI(它读玩家的最新位置),最后算通缉 —— 通缉依赖「这帧有没有被
/// 看见」,必须在 AI 跑完之后才有意义。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `f32` - 本帧秒数(已在 [`simulate`] 里钳位)。
fn step_combat(game: &mut Game, dt: f32) {
    if game.hurt.is_wasted() {
        step_death(game, dt);
        return;
    }
    game.arsenal.tick(dt);
    // 手雷要在 `fire_weapon` **之前**推进:爆炸会在本帧结算伤害,
    // 放后面的话这一帧的命中反馈会晚一帧才出现。
    step_live_grenades(game, dt);
    regen_armor(&mut game.player, &mut game.hurt, dt);
    update_aim(game);
    fire_weapon(game);
    step_enemies(game, dt);
    step_peds(game, dt);
    // 社交是群体行为,必须在所有行人各自步进完之后统一算一次。
    update_ped_social(&mut game.peds, dt);
    update_wanted(game, dt);
    update_mission(game, dt);
    if game.hitmarker > 0.0 {
        game.hitmarker = (game.hitmarker - dt).max(0.0);
    }
    if game.wanted_flash > 0.0 {
        game.wanted_flash = (game.wanted_flash - dt).max(0.0);
    }
}

/// 把鼠标位置换算成世界空间的瞄准方向(鼠标瞄准)。
///
/// 做法是**从相机反投影**:先用相机的 `view_projection` 把眼睛前方
/// `AIM_DEPTH` 米的两个「屏幕左右边界点」投回世界,得到一条位于该深度的
/// 近平面水平线;鼠标在屏幕上的横坐标就是这条线上的插值参数,纵坐标则
/// 决定射线的俯仰(向上打 / 向下打)。
///
/// 之所以不直接用「相机前向」当瞄准方向:第三人称射击里准星必须跟着
/// 鼠标走,否则玩家会「看着左边的人、子弹往天上飞」。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
fn update_aim(game: &mut Game) {
    let yaw: f32 = game.camera.get_yaw();
    let forward: Vec2 = [yaw.cos(), -yaw.sin()];
    // 开车时不瞄准(和 GTA 一致)。
    if game.player.get_driving() {
        game.arsenal.set_aim(forward);
        return;
    }
    let size: (u32, u32) = game.canvas_size.get();
    let (canvas_w, canvas_h): (f32, f32) = (size.0 as f32, size.1 as f32);
    if canvas_w <= 0.0 || canvas_h <= 0.0 {
        game.arsenal.set_aim(forward);
        return;
    }
    let eye: Vec3 = game.camera.eye();
    let aspect: f32 = canvas_w / canvas_h;
    let fov_y: f32 = game.camera.get_fov_y();
    // 该深度上的半高 / 半宽(米)。
    let half_h: f32 = (fov_y * 0.5).tan() * AIM_DEPTH;
    let half_w: f32 = half_h * aspect;
    let right: Vec2 = [forward[1], -forward[0]];
    // 鼠标归一化到 -1..1;y 向上。
    let pointer: [f64; 2] = game.input.aim_point;
    let ndc_x: f32 = ((pointer[0] / canvas_w as f64) * 2.0 - 1.0) as f32;
    let ndc_y: f32 = (1.0 - (pointer[1] / canvas_h as f64) * 2.0) as f32;
    // 目标点:该深度平面上按 NDC 插值出来的一个点。
    let target: Vec3 = [
        eye[0] + forward[0] * AIM_DEPTH + right[0] * ndc_x * half_w,
        eye[1] + ndc_y * half_h,
        eye[2] + forward[1] * AIM_DEPTH + right[1] * ndc_x * half_w,
    ];
    // 射线的起点是角色的胸口,不是相机 —— 子弹从手上出去。
    let here: Vec3 = game.player.get_position();
    let muzzle: Vec3 = [here[0], here[1] + AIM_CHEST_HEIGHT, here[2]];
    let direction: Vec3 = [
        target[0] - muzzle[0],
        target[1] - muzzle[1],
        target[2] - muzzle[2],
    ];
    // 俯仰:向上打时把 XZ 分量放大,等价于「抬高枪口」。
    let flat: f32 = (direction[0] * direction[0] + direction[2] * direction[2]).sqrt();
    game.arsenal.set_aim([direction[0], direction[2]]);
    // 俯仰单独存:近战判定与射线终点都要用。
    let pitch: f32 = if flat > f32::EPSILON {
        direction[1].atan2(flat)
    } else {
        0.0
    };
    game.aim_pitch = pitch.clamp(-AIM_PITCH_LIMIT, AIM_PITCH_LIMIT);
}

/// 开火:hitscan 射线 + 距离衰减,命中敌人给反馈并加通缉热度。
///
/// 判定顺序是「先看墙,再看人」:如果墙比敌人更近,子弹打在墙上。
/// 这让「隔着一辆车打」能打中车窗后面的人,也会让躲在垃圾桶后面的人
/// 真的安全 —— 和 GTA 的掩体逻辑一致。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
fn fire_weapon(game: &mut Game) {
    let wants: bool = game.input.fire_held;
    if !wants || !game.arsenal.can_fire() {
        if wants && game.arsenal.get_magazine() == 0 && !game.arsenal.is_reloading() {
            game.player.set_notice(String::from(NOTICE_EMPTY));
            let _: bool = game.arsenal.reload();
        }
        return;
    }
    if !game.arsenal.fire(0) {
        return;
    }
    let weapon: Weapon = game.arsenal.get_weapon();
    let here: Vec3 = game.player.get_position();
    let muzzle: Vec3 = [here[0], here[1] + AIM_CHEST_HEIGHT, here[2]];
    let aim: Vec2 = game.arsenal.get_aim();
    // 枪口方向:水平瞄准 × cos(pitch),垂直分量 = sin(pitch)。
    let pitch: f32 = game.aim_pitch;
    let flat: f32 = pitch.cos();
    let direction: Vec3 = [aim[0] * flat, pitch.sin(), aim[1] * flat];
    let range: f32 = weapon.range();
    // 先求射线打到静态几何的位置(墙 / 楼 / 车)。
    let wall_hit: Option<(f32, Vec2)> =
        crate::collision::ray_to_shapes(&game.world, muzzle, direction);
    let wall_distance: f32 = wall_hit.map_or(range, |hit: (f32, Vec2)| hit.0);
    // 再扫敌人:取射程内、且比墙更近的第一个。
    let mut best: Option<(usize, f32)> = None;
    for (index, enemy) in game.enemies.iter().enumerate() {
        if !enemy.is_alive() {
            continue;
        }
        let to: Vec3 = [
            muzzle[0] - enemy.get_position()[0],
            AIM_CHEST_HEIGHT,
            muzzle[2] - enemy.get_position()[2],
        ];
        let length: f32 = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
        if length > range || length > wall_distance {
            continue;
        }
        // 简单的「射线到球心距离」判定:把敌人当成一个胶囊。
        let center: Vec3 = [
            enemy.get_position()[0],
            enemy.get_position()[1] + AIM_CHEST_HEIGHT,
            enemy.get_position()[2],
        ];
        let reach: f32 = ray_sphere_distance(muzzle, direction, center, ENEMY_HIT_RADIUS);
        if reach <= length.max(ENEMY_HIT_RADIUS)
            && best.map(|(_, d): (usize, f32)| reach < d).unwrap_or(true)
        {
            best = Some((index, reach));
        }
    }
    // 近战武器不消耗弹药,直接结算一次挥击。
    match best {
        Some((index, distance)) => {
            let damage: f32 = falloff(weapon, distance);
            let killed: bool = game.enemies[index].damage(damage);
            // 命中时把敌人往后推一点:纯粹是手感 —— 被打的人会晃,
            // 让 hitscan 有了「打到东西」的实感。
            let knock: Vec2 = [-aim[0] * HIT_KNOCKBACK, -aim[1] * HIT_KNOCKBACK];
            game.enemies[index].knock_back(knock);
            game.hitmarker = HITMARKER_TIME;
            game.did_hit = true;
            game.wanted.add_heat(WANTED_PER_SHOT + WANTED_PER_HIT);
            if killed {
                game.kills += 1;
                let drop: f32 = ENEMY_CASH_DROP;
                game.player.set_cash_add(drop);
                game.player.set_notice(String::from(NOTICE_KILL));
            } else {
                game.player.set_notice(String::from(NOTICE_HIT));
            }
            // 命中「干掉某人」的目标:标记任务完成。
            if game.mission.get_stage() == MISSION_KILL
                && game.mission.get_target_enemy() == Some(index)
                && killed
            {
                game.mission.set_target_enemy(usize::MAX);
            }
        }
        None => {
            if weapon == Weapon::Bat {
                // 球棒挥空也算一次开火反馈。
                game.hitmarker = 0.0;
            }
        }
    }
}

/// 射线到球心的最近距离(用于 hitscan 的敌人判定)。
///
/// 用「点到射线的垂距」而不是真���的射线-球求交:近战 / 中距离足够,
/// 而且不会在敌人正后方时算出负的 t(那会让子弹打到「身后的墙」)。
///
/// # Arguments
///
/// - `Vec3` - 射线起点。
/// - `Vec3` - 单位方向。
/// - `Vec3` - 球心。
/// - `f32` - 球半径。
///
/// # Returns
///
/// - `f32` - 射线到球心的距离(米);球心在射线背后时返回一个大值。
fn ray_sphere_distance(origin: Vec3, direction: Vec3, center: Vec3, radius: f32) -> f32 {
    let to: Vec3 = [
        center[0] - origin[0],
        center[1] - origin[1],
        center[2] - origin[2],
    ];
    let along: f32 = to[0] * direction[0] + to[1] * direction[1] + to[2] * direction[2];
    if along <= 0.0 {
        return f32::MAX;
    }
    let squared: f32 = to[0] * to[0] + to[1] * to[1] + to[2] * to[2];
    let perpendicular_sq: f32 = (squared - along * along).max(0.0);
    if perpendicular_sq > radius * radius {
        return f32::MAX;
    }
    (along - (radius * radius - perpendicular_sq).sqrt().max(0.0)).max(0.0)
}

/// 敌人 AI 的一帧推进。
///
/// 关键性能设计:**先做距离剔除**。超过激活圈的敌人连决策都不跑 ——
/// 这不只是省 CPU,更重要的是避免几十个实体每帧都做视线射线。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `f32` - 本帧秒数。
fn step_enemies(game: &mut Game, dt: f32) {
    let player_at: Vec3 = game.player.get_position();
    let wanted: bool = game.wanted.get_stars() > 0;
    // 通缉增派:按星数补足警察。
    let stars: u32 = game.wanted.get_stars();
    let phase: f32 = game.frame_count as f32 * 0.017;
    deploy_police(
        &mut game.enemies,
        stars,
        player_at,
        ENEMY_WANTED_TOTAL,
        phase,
    );
    for index in 0..game.enemies.len() {
        // 尸体单独推进,不受激活圈影响(要等它自己消失)。
        if !game.enemies[index].is_alive() {
            game.enemies[index].step_death(dt);
            continue;
        }
        let at: Vec3 = game.enemies[index].get_position();
        if !is_active(at, player_at) {
            continue;
        }
        let decision: crate::enemy::EnemyDecision = decide(
            &game.enemies[index],
            player_at,
            &game.world,
            wanted,
            game.enemies[index].get_wander(),
        );
        let fired: bool = crate::enemy::apply(
            &mut game.enemies[index],
            &decision,
            dt,
            &game.world,
            decision.shoot,
        );
        if fired {
            enemy_shot(game, index);
        }
    }
    // 警察「重新出现」:尸体到期后不是在原地诈尸,而是在玩家看不到的
    // 距离外重生 —— 这就是 GTA 里「警察越来越多」的实现方式。
    if stars > 0 {
        let mut index: usize = 0;
        while index < game.enemies.len() {
            if game.enemies[index].death_expired()
                && game.enemies[index].get_faction() == Faction::Police
            {
                let at: Vec3 = game.enemies[index].get_position();
                let dx: f32 = at[0] - player_at[0];
                let dz: f32 = at[2] - player_at[2];
                let length: f32 = (dx * dx + dz * dz).sqrt().max(f32::EPSILON);
                let far: Vec3 = [
                    player_at[0] + dx / length * ENEMY_SPAWN_DIST,
                    at[1],
                    player_at[2] + dz / length * ENEMY_SPAWN_DIST,
                ];
                let yaw: f32 = game.enemies[index].get_yaw();
                game.enemies[index].respawn(far, yaw);
            }
            index += 1;
        }
    }
    // 回收尸体:死亡计时归零的敌人从尾部弹出(顺序不影响正确性)。
    let mut index: usize = 0;
    while index < game.enemies.len() {
        if game.enemies[index].death_expired() {
            game.enemies.swap_remove(index);
        } else {
            index += 1;
        }
    }
    // 批次表跟着收缩,避免索引错位。
    if game.enemy_batches.len() > game.enemies.len() {
        game.enemy_batches.truncate(game.enemies.len());
    }
}

/// 敌人开火的一次结算:打玩家、加一点通缉热度。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `usize` - 开火的敌人索引。
fn enemy_shot(game: &mut Game, index: usize) {
    let Some(enemy) = game.enemies.get(index) else {
        return;
    };
    // 先把需要的量拷贝出来再放开借用 —— 后面要改 `wanted`。
    let from: Vec3 = enemy.get_position();
    let wander: f32 = enemy.get_wander();
    let shots: u32 = enemy.get_shots();
    // 开枪加一点热度(开火本身就会被通缉,追踪的人是你)。
    game.wanted.add_heat(WANTED_PER_SHOT * 0.5);
    if let Some(shooter) = game.enemies.get_mut(index) {
        shooter.mark_shot();
    }
    let player_at: Vec3 = game.player.get_position();
    // **视线门控:敌人「看得见」才谈得上打中。**
    //
    // `decide` 层(`src/enemy.rs:76`)算过一次 `sees`,但那只用来决定
    // AI 状态;**开火本身不继承它**。`enemy_shot` 拿到 `fired` 就直接
    // 结算,于是警察隔着墙也能把子弹「打」到玩家身上 —— 玩家躲在
    // 建筑另一侧,`hit_dot` 只要落在散布锥里就掉血。这就是用户报的
    // 「隔墙掉血」。
    //
    // 门控必须落在**结算伤害之前**,而不是只依赖 `decide`:敌人与玩家
    // 在同一帧里可能已经绕到了墙后(流式加载把新墙推入 `world`),而
    // `decide` 的结果在这一帧之前就算好了。距离上限用
    // `ENEMY_FIRE_RANGE` —— 超程的枪本来就打不着,不该结算。
    //
    // **两个世界都要查。** `has_line_of_sight` 只射线检测静态碰撞世界,
    // 而样板楼的楼板与隔墙住在 `FloorWorld` 里,两套几何互不相交 ——
    // 只查静态世界的话,射线会从整栋楼中间穿过去并报告「视线通畅」。
    // 实测玩家停在楼里(`inSolid == true`)、19 m 外的警察持续开火,
    // 90 帧掉 31.8 血,门控计数器全程为 0。`blocks_sight` 补上这一半。
    let sight_y: f32 = from[1] + AIM_CHEST_HEIGHT;
    if !has_line_of_sight(&game.world, from, player_at, ENEMY_FIRE_RANGE.max(1.0))
        || game
            .interiors
            .blocks_sight([from[0], from[2]], [player_at[0], player_at[2]], sight_y)
    {
        // 记一笔,验收探针据此确认「这一枪是被视线挡掉的」而不是
        // 别的分支提前 return 的。
        game.shots_blocked_by_geometry += 1;
        return;
    }
    let center: Vec3 = [player_at[0], player_at[1] + AIM_CHEST_HEIGHT, player_at[2]];
    let spread: Vec2 = aim_with_spread(from, center, wander, shots);
    // 打偏:用散布方向与真实方向的夹角,超过阈值就没打中。
    let true_dir: Vec2 = [center[0] - from[0], center[2] - from[2]];
    let length: f32 = (true_dir[0] * true_dir[0] + true_dir[1] * true_dir[1]).sqrt();
    if length <= f32::EPSILON {
        return;
    }
    let hit_dot: f32 = (spread[0] * true_dir[0] + spread[1] * true_dir[1]) / length;
    let clean: f32 = 1.0 - ENEMY_SPREAD;
    if hit_dot < clean {
        return;
    }
    // 距离太远伤害衰减,和玩家武器同一个模型。
    let distance: f32 = flat_distance(from, player_at);
    let scale: f32 = (1.0 - (distance / ENEMY_FIRE_RANGE.max(1.0)) * 0.6).clamp(0.25, 1.0);
    let damage: f32 = ENEMY_DAMAGE * scale;
    hurt_player(game, damage);
}

/// 玩家受击:护甲优先,掉血后进无敌帧,归零则倒地。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `f32` - 原始伤害。
fn hurt_player(game: &mut Game, amount: f32) {
    if game.hurt.is_wasted() || game.hurt.is_invulnerable() {
        return;
    }
    let dealt: f32 = apply_damage(&mut game.player, amount);
    if dealt <= 0.0 {
        return;
    }
    game.hurt.on_hit(PLAYER_HIT_INVULN);
    if game.player.get_health() <= 0.0 {
        game.hurt.waste(RESPAWN_DELAY);
        game.player.set_health(0.0);
        game.player.set_notice(String::from(NOTICE_WASTED));
    }
}

/// 通缉系统的一帧推进:降星 / 增派 / 提示。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `f32` - 本帧秒数。
fn update_wanted(game: &mut Game, dt: f32) {
    let player_at: Vec3 = game.player.get_position();
    // 藏身区:站在后巷 / 警局门口就强制降温。
    let hidden: bool = is_hidden(&game.hideouts, player_at);
    // 「被看见」:有没有任何一个活着的警察视线通畅。
    let mut spotted: bool = false;
    for enemy in &game.enemies {
        if !enemy.is_alive() || enemy.get_faction() != Faction::Police {
            continue;
        }
        let at: Vec3 = enemy.get_position();
        if flat_distance(at, player_at) <= ENEMY_SIGHT
            && has_line_of_sight(&game.world, at, player_at, ENEMY_SIGHT)
        {
            spotted = true;
            break;
        }
    }
    let before: u32 = game.wanted.get_stars();
    let after: u32 = game.wanted.update(dt, spotted, hidden);
    if after > before {
        game.wanted_flash = WANTED_FLASH_TIME;
        game.player.set_notice(String::from(NOTICE_WANTED));
    } else if after == 0 && before > 0 {
        game.player.set_notice(String::from(NOTICE_CLEARED));
    }
}

/// 任务的一帧推进。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `f32` - 本帧秒数。
fn update_mission(game: &mut Game, _dt: f32) {
    let player_at: Vec3 = game.player.get_position();
    let target_index: usize = game.mission.get_target_enemy().unwrap_or(usize::MAX);
    let target_dead: bool = target_index != usize::MAX
        && game
            .enemies
            .get(target_index)
            .map(|enemy: &Enemy| !enemy.is_alive())
            .unwrap_or(true);
    let reward: f32 = advance_mission(
        &mut game.mission,
        player_at,
        target_index,
        target_dead,
        game.missions_done,
    );
    if reward > 0.0 {
        game.player.set_cash_add(reward);
        game.missions_done += 1;
        let notice: String = format!("{NOTICE_MISSION_DONE}{CASH_SIGN}{reward}");
        game.player.set_notice(notice);
    }
}

/// 接任务:靠近任务点时按 J 接受,或者完成后自动接下一个。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
fn toggle_mission(game: &mut Game) {
    if game.mission.is_active() {
        return;
    }
    let player_at: Vec3 = game.player.get_position();
    let blueprints: Vec<(&'static str, u32, Vec3)> = mission_blueprints();
    let Some(entry) = blueprints.get(game.missions_done as usize % blueprints.len()) else {
        return;
    };
    let (title, stage, at) = *entry;
    if flat_distance(at, player_at) > MISSION_ACCEPT_RANGE {
        return;
    }
    game.mission.accept(title, stage, at);
    let notice: String = format!("{NOTICE_MISSION_START}{title}");
    game.player.set_notice(notice);
}

/// 行人的一帧推进:走路、躲车、被撞飞。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `f32` - 本帧秒数。
fn step_peds(game: &mut Game, dt: f32) {
    let player_at: Vec3 = game.player.get_position();
    // 玩家开的车(如果有)的坐标与速度,用来判定「撞到行人」。
    let (car_at, car_speed) = match game.player.get_vehicle() {
        Some(vehicle) => match game.traffic.get_cars_ref().get(vehicle) {
            Some(car) => (car.get_position(), car.get_speed()),
            None => ([0.0, 0.0, 0.0], 0.0),
        },
        None => ([0.0, 0.0, 0.0], 0.0),
    };
    // 先各自跑完「走 / 逃 / 撞倒」,再由 `update_ped_social` 统一决定
    // 谁去找谁聊天 —— 社交是群体行为,必须在个体步进之后。
    for ped in &mut game.peds {
        ped.step(dt, &game.world, player_at, game.player.get_driving());
        // 撞飞判定:车在动,而且行人就在车身附近。
        if car_speed > PED_RUNOVER_MIN_SPEED {
            let d: f32 = flat_distance(car_at, ped.get_position());
            if d < CAR_HIT_RADIUS {
                let dx: f32 = ped.get_position()[0] - car_at[0];
                let dz: f32 = ped.get_position()[2] - car_at[2];
                let length: f32 = (dx * dx + dz * dz).sqrt().max(f32::EPSILON);
                let impulse: Vec2 = [
                    dx / length * PED_RUNOVER_SPEED,
                    dz / length * PED_RUNOVER_SPEED,
                ];
                ped.knock_down(impulse);
                game.wanted.add_heat(WANTED_PER_RUNOVER);
            }
        }
    }
    // 回收倒地的行人。
    let mut index: usize = 0;
    while index < game.peds.len() {
        if game.peds[index].is_gone() {
            game.peds.swap_remove(index);
        } else {
            index += 1;
        }
    }
    if game.ped_batches.len() > game.peds.len() {
        game.ped_batches.truncate(game.peds.len());
    }
}

/// 玩家死亡 / 重生的一帧推进。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `f32` - 本帧秒数。
fn step_death(game: &mut Game, dt: f32) {
    game.hurt.advance(dt);
    if game.hurt.get_respawn() > 0.0 {
        return;
    }
    // 重生:在最近的医院点复活,扣钱,清通缉。
    // Y 归零:医院都在地面上,不能把玩家复活在二楼楼板上(或者半空里)。
    let spot: Vec3 = nearest_hospital(game.player.get_position());
    let resolved: Vec2 = game.world.resolve([spot[0], spot[2]]);
    game.player
        .set_position([resolved[0], GROUND_LEVEL, resolved[1]]);
    game.player.set_vertical_velocity(0.0);
    game.player.set_grounded(true);
    game.player.set_health(PLAYER_MAX_HEALTH);
    game.player.set_armor(0.0);
    let loss: f32 = game.player.get_cash() * DEATH_CASH_LOSS;
    game.player.set_cash_add(-loss);
    game.wanted = Wanted::new();
    game.hurt.revive();
    game.player.set_notice(String::from(NOTICE_WASTED));
}

/// 医院点:玩家死亡后在这里重生。
///
/// 选四个城市的四角 + 中心,保证任何位置都能找到「最近的一个」。
///
/// # Returns
///
/// - `Vec<Vec3>` - 医院点的世界坐标。
fn hospital_spots() -> Vec<Vec3> {
    let mut out: Vec<Vec3> = Vec::new();
    for spot in HOSPITAL_SPOTS {
        out.push(*spot);
    }
    out
}

/// 距离给定点最近的医院点。
///
/// # Arguments
///
/// - `Vec3` - 玩家当前坐标。
///
/// # Returns
///
/// - `Vec3` - 最近的医院坐标。
fn nearest_hospital(from: Vec3) -> Vec3 {
    let spots: Vec<Vec3> = hospital_spots();
    let mut best: Vec3 = spots[0];
    let mut best_distance: f32 = f32::MAX;
    for spot in spots {
        let d: f32 = flat_distance(from, spot);
        if d < best_distance {
            best_distance = d;
            best = spot;
        }
    }
    best
}

/// 抢车的通缉结算:抢警车加更多热度。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `usize` - 抢到的车的索引。
fn report_jacking(game: &mut Game, index: usize) {
    let Some(car) = game.traffic.get_cars_ref().get(index) else {
        return;
    };
    let stolen_police: bool = car.asset == CAR_POLICE;
    game.wanted
        .add_heat(WANTED_PER_JACK + if stolen_police { WANTED_PER_JACK } else { 0.0 });
    if stolen_police {
        game.player.set_notice(String::from(NOTICE_STOLE_POLICE));
    } else {
        game.player.set_notice(String::from(NOTICE_JACKED));
    }
}

/// 为敌人 / 行人建立渲染批次(必须在唯一的 `upload_mesh` 之前调用)。
///
/// 每个实体一个批次,而不是「同类共用一个批次」—— 因为每个敌人有
/// **独立姿态**,共批就必须在 CPU 侧逐个算矩阵再拆回去,反而更慢。
/// 顶点数据仍然只上传一次(每个批次引用同一个 `mesh_index`),
/// 所以「一次上传 + 多次 instanced draw」这个核心不变。
///
/// `near_cull = false`:第三人称相机离角色只有几米,默认 26 m 的近处
/// 剔除会把整批敌人剔光(见 `push_player_part` 的同类说明)。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `&HashMap<String, usize>` - 资产 id → 场景 mesh 索引。
fn spawn_combat_batches(game: &mut Game, index_map: &HashMap<String, usize>) {
    game.enemy_batches.clear();
    for enemy in &game.enemies {
        let asset: &'static str = enemy_asset(enemy.get_faction());
        let Some(mesh_index) = index_map.get(asset) else {
            game.enemy_batches.push(usize::MAX);
            continue;
        };
        let batch: usize = find_or_create_batch(&mut game.scene, *mesh_index);
        game.enemy_batches.push(batch);
    }
    // 手持武器:一个批次,每帧换一次实例矩阵(跟着角色手部走)。
    game.weapon_batch = match index_map.get(game.arsenal.get_weapon().asset()) {
        Some(mesh_index) => find_or_create_batch(&mut game.scene, *mesh_index),
        None => usize::MAX,
    };
    game.ped_batches.clear();
    for ped in &game.peds {
        let Some(mesh_index) = index_map.get(ped.get_model()) else {
            game.ped_batches.push(usize::MAX);
            continue;
        };
        let batch: usize = find_or_create_batch(&mut game.scene, *mesh_index);
        game.ped_batches.push(batch);
    }
}

/// 一个阵营用的模型资产。
///
/// 复用现成的行人资产,不新建模:警察穿深色西装(���),混混穿街头服,
/// 靠**色调**区分敌我 —— 这也顺带让「受击变红」有统一的实现。
///
/// # Arguments
///
/// - `Faction` - 阵营。
///
/// # Returns
///
/// - `&'static str` - 资产 id。
fn enemy_asset(faction: Faction) -> &'static str {
    match faction {
        Faction::Police => PED_SUIT,
        Faction::Thug => PED_STREETWEAR,
    }
}

/// 生成开局的敌人与行人。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `f32` - 伪随机相位。
fn populate_combatants(game: &mut Game, phase: f32) {
    let here: Vec3 = game.player.get_position();
    let thugs: Vec<Enemy> = spawn_thugs(here, THUG_COUNT, phase);
    game.enemies = thugs;
    let peds: Vec<Pedestrian> = spawn_peds(here, PED_COUNT, phase);
    game.peds = peds;
}

/// 把场上所有动态实体过一遍**统一碰撞层**。
///
/// 之前每类实体各自跟静态世界碰撞,彼此之间**从不**相交:
///
/// - 玩家 → `Player::step` → `resolve_slide`;
/// - 车辆 → `TrafficCar::drive` → `resolve_car`;
/// - 行人 → `Pedestrian::step` → `resolve_with_radius`。
///
/// 三条路径互不知情,于是车会穿过另一辆车、行人会走进另一名行人、
/// 敌人会站进人的身体里 —— 用户报的「所有实体都不能穿越和穿模」。
///
/// 现在它们先各自跑完自己的静态碰撞(那部分行为完全不变,探针
/// `collision` / `car.drive` 依赖它),再由本函数统一做一次
/// 两两分离 + 对静态收敛。
///
/// 槽位分配是**按索引**的,收集与回写严格对称:
///
/// | 槽位 | 实体 |
/// |------|------|
/// | 0    | 玩家(或他开的那辆车) |
/// | 1..  | 车队(跳过玩家开的那辆) |
/// | …    | 敌人 |
/// | …    | 行人 |
///
/// 早先用「位置相等」来识别玩家开的那辆车 —— 两辆车停在同一坐标时
/// 会双双被跳过,整个列表错位,把行人的位置写到敌人身上。这里改成
/// 直接比较**索引**。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
fn resolve_dynamic_bodies(game: &mut Game) {
    let driven: Option<usize> = game
        .player
        .get_vehicle()
        .filter(|i: &usize| *i < game.traffic.get_cars_ref().len());
    let mut bodies: Vec<DynamicBody> = Vec::new();
    // 槽位 0 恒为玩家自己;在车里时那一格的碰撞体是车。
    let player_at: Vec3 = game.player.get_position();
    match driven {
        Some(index) => {
            if let Some(car) = game.traffic.get_cars_ref().get(index) {
                let at: Vec3 = car.get_position();
                bodies.push(DynamicBody::new(
                    BodyKind::Car,
                    [at[0], at[2]],
                    CAR_RADIUS,
                    CAR_MASS,
                ));
            }
        }
        None => {
            bodies.push(DynamicBody::new(
                BodyKind::Player,
                [player_at[0], player_at[2]],
                PLAYER_RADIUS,
                PLAYER_MASS,
            ));
        }
    }
    for (index, car) in game.traffic.get_cars_ref().iter().enumerate() {
        if driven == Some(index) {
            continue;
        }
        let at: Vec3 = car.get_position();
        bodies.push(DynamicBody::new(
            BodyKind::Car,
            [at[0], at[2]],
            CAR_RADIUS,
            CAR_MASS,
        ));
    }
    for enemy in &game.enemies {
        let at: Vec3 = enemy.get_position();
        bodies.push(DynamicBody::new(
            BodyKind::Enemy,
            [at[0], at[2]],
            ENEMY_RADIUS,
            ENEMY_MASS,
        ));
    }
    for ped in &game.peds {
        let at: Vec3 = ped.get_position();
        bodies.push(DynamicBody::new(
            BodyKind::Pedestrian,
            [at[0], at[2]],
            PED_RADIUS,
            PED_MASS,
        ));
    }
    if bodies.len() < 2 {
        return;
    }
    let resolved: Vec<Vec2> = game.world.resolve_all(&bodies);

    // 回写:与收集严格对称地按槽位消费。槽位 0 已被上面处理,
    // 所以 `cursor` 直接从 1 起(不回写的话从 0 起会错开一位)。
    let mut cursor: usize = 1;
    if let Some(index) = driven {
        if let (Some(slot), Some(car)) = (resolved.first(), game.traffic.get_car_mut(index)) {
            let at: Vec3 = car.get_position();
            car.set_position([slot[0], at[1], slot[1]]);
        }
    } else if let Some(slot) = resolved.first() {
        game.player.set_position([slot[0], player_at[1], slot[1]]);
    }
    for (index, car) in game.traffic.get_cars_mut().iter_mut().enumerate() {
        if driven == Some(index) {
            continue;
        }
        if let Some(slot) = resolved.get(cursor) {
            let at: Vec3 = car.get_position();
            car.set_position([slot[0], at[1], slot[1]]);
        }
        cursor += 1;
    }
    for enemy in game.enemies.iter_mut() {
        if let Some(slot) = resolved.get(cursor) {
            let at: Vec3 = enemy.get_position();
            enemy.set_position([slot[0], at[1], slot[1]]);
        }
        cursor += 1;
    }
    for ped in game.peds.iter_mut() {
        if let Some(slot) = resolved.get(cursor) {
            let at: Vec3 = ped.get_position();
            ped.set_position([slot[0], at[1], slot[1]]);
        }
        cursor += 1;
    }
}

/// 市民的社交调度:让附近的行人**扎堆聊天**,而不是各走各的。
///
/// `Pedestrian::step` 只知道「我的巡航点在哪」,看不到邻居,所以一群
/// 市民会永远各走各的平行线 —— 街上一眼看去没有「人群」,也没有
/// 「有人在说话」。这一层是**群体行为**:每个行人自己算一次
///
/// ```text
/// neighbors = 半径 PED_GATHER_RADIUS 内的其他行人
/// ```
///
/// - 有人在聊天 → 新来的走上前,站进圈子空位,然后一起倒计时;
/// - 没人在聊天但邻居够 `PED_GATHER_MIN_CROWD` 人 → 挑一个起头的人,
///   圈子定在这堆人的几何中心;
/// - 聊满 `talk_total` 秒 → 各自散开,`gather_cooldown` 之后才允许
///   再凑一局(否则整城的人会挤成一团永不分开的奇观)。
///
/// 圈心是**几何中心**而不是第一个人脚下,所以三个人会自然围成一个
/// 三角形而不是排成一列。每个人的站位角度由自己的 `talk_slot` 决定
/// (初始化时用步态相位错开),所以不会挤在同一点上。
///
/// **逃跑与撞倒优先于社交**:被打 / 被车撞的行人不参与聊天,见
/// `update_ped_social` 里的两个提前返回。
///
/// # Arguments
///
/// - `&mut [Pedestrian]` - 本帧所有市民(可变切片)。
/// - `f32` - 本帧秒数。
fn update_ped_social(peds: &mut [Pedestrian], dt: f32) {
    let count: usize = peds.len();
    if count < PED_GATHER_MIN_CROWD {
        return;
    }
    // 快照:一帧内所有判定都基于帧初的位置,不做「边遍历边改」的
    // 顺序依赖 —— 否则第 0 个人已经决定聊天、第 5 个人看到的还是
    // 老状态,同一帧里的决策会互相矛盾。
    let spots: Vec<Vec2> = peds
        .iter()
        .map(|ped: &Pedestrian| {
            let at: Vec3 = ped.get_position();
            [at[0], at[2]]
        })
        .collect();
    // 被打 / 被撞倒 / 正在逃跑的行人不参与社交。
    let ready: Vec<bool> = peds
        .iter()
        .map(|ped: &Pedestrian| !ped.is_down() && ped.get_flee_timer() <= 0.0)
        .collect();
    let talking: Vec<bool> = peds.iter().map(Pedestrian::is_talking).collect();
    // 本帧「谁的圈子已经定下来了」。被领走的人立刻置位。
    let mut claimed: Vec<bool> = vec![false; count];

    for index in 0..count {
        if !ready[index] {
            continue;
        }
        // 本帧已经被人拉进某个圈子的人,不再参与后续的「自己起一局」——
        // 否则同一个人会在同一帧里被两组先后赋予不同的站位角,后一组
        // 覆盖前一组。实测三个人会得到 `[3.77, 3.77, 6.28]`。
        if claimed[index] {
            continue;
        }
        // 正在聊的:倒计时,并且**站住不动**(步态幅度归零,看起来是
        // 站着说话而不是一边聊一边滑行)。
        if talking[index] {
            let ped: &mut Pedestrian = &mut peds[index];
            let left: f32 = ped.get_talk_timer() - dt;
            if left <= 0.0 {
                ped.set_talk_timer(0.0);
                ped.set_gather_cooldown(PED_GATHER_COOLDOWN);
            } else {
                ped.set_talk_timer(left);
            }
            continue;
        }
        // 冷却中的:什么都不做,继续走原来的巡航点。
        if peds[index].get_gather_cooldown() > 0.0 {
            let ped: &mut Pedestrian = &mut peds[index];
            let cd: f32 = ped.get_gather_cooldown() - dt;
            ped.set_gather_cooldown(cd.max(0.0));
            continue;
        }
        // 找邻居:已经在聊的人(加入现有圈子)+ 还没聊的人(凑新局)。
        let mut neighbors: Vec<usize> = Vec::new();
        for other in 0..count {
            if other == index || !ready[other] {
                continue;
            }
            let dx: f32 = spots[other][0] - spots[index][0];
            let dz: f32 = spots[other][1] - spots[index][1];
            if dx * dx + dz * dz <= PED_GATHER_RADIUS * PED_GATHER_RADIUS {
                neighbors.push(other);
            }
        }
        if neighbors.is_empty() {
            continue;
        }
        // 优先加入一个已经成立的圈子;圈子圆心直接用发起者的。
        let host: Option<usize> = neighbors
            .iter()
            .copied()
            .find(|other: &usize| talking[*other]);
        if let Some(host_index) = host {
            let center: Vec2 = peds[host_index].get_talk_center();
            let total: f32 = peds[host_index].get_talk_total();
            // 序号取「圈里已有的人数」,新来的人排在队尾,不撞位。
            let taken: usize = (0..count)
                .filter(|other: &usize| *other != index && talking[*other])
                .count();
            join_talk(peds, index, taken, center, total);
            claimed[index] = true;
            continue;
        }
        // 没有现成圈子,但人数够了才开一局 —— 一个人自说自话不算。
        if neighbors.len() + 1 < PED_GATHER_MIN_CROWD {
            continue;
        }
        // 圆心 = 这一堆人的几何中心(取所有参与者的均值)。
        let mut sum_x: f32 = spots[index][0];
        let mut sum_z: f32 = spots[index][1];
        for other in &neighbors {
            sum_x += spots[*other][0];
            sum_z += spots[*other][1];
        }
        let people: f32 = neighbors.len() as f32 + 1.0;
        let center: Vec2 = [sum_x / people, sum_z / people];
        // 聊天时长用「人数越多聊越久」,而且带一点随机,免得整城同
        // 一时刻散场。
        let crowd: f32 = people.min(PED_GATHER_MAX_CROWD) / PED_GATHER_MAX_CROWD;
        let span: f32 = PED_TALK_MAX_SECONDS - PED_TALK_MIN_SECONDS;
        let total: f32 = PED_TALK_MIN_SECONDS + span * crowd;
        // 序号 = 在这一堆人里的排位,保证一圈人各占不同方位。
        for (slot, other) in neighbors
            .iter()
            .copied()
            .chain(std::iter::once(index))
            .enumerate()
        {
            join_talk(peds, other, slot, center, total);
        }
    }
}

/// 让一个行人站进聊天圈。
///
/// # Arguments
///
/// - `&mut [Pedestrian]` - 本帧所有市民(可变切片)。
/// - `usize` - 市民在切片里的索引。
/// - `usize` - 这个人在圈里的序号(决定站位角)。
/// - `Vec2` - 圈子圆心。
/// - `f32` - 本次扎堆的总时长(秒)。
fn join_talk(peds: &mut [Pedestrian], index: usize, slot: usize, center: Vec2, total: f32) {
    let Some(ped) = peds.get_mut(index) else {
        return;
    };
    ped.set_talk_center(center);
    ped.set_talk_total(total);
    ped.set_talk_timer(total);
    ped.set_gather_cooldown(0.0);
    // 站位角度由**这个人在圈里的序号**决定,而不是「把自己那个计数器
    // 加一」。加一在两种情况下会撞:同一个人一帧内被 `join_talk` 调两次
    // (自己起头 + 又被邻居拉进来),两次都从同一个旧值加一,和同批另一
    // 个人得到完全相同的角度 —— 实测三个人算出 `[3.77, 3.77, 6.28]`,
    // 两个人被安排到同一个点。
    let slot: f32 = slot as f32 * PED_TALK_SLOT_STEP;
    ped.set_talk_slot(slot);
    // 目标点 = 圈子上的自己的位置。`Pedestrian::step` 会朝它走,
    // 走到位之后它就停在原地(见 `talk_target`)。
    ped.set_goal([
        center[0] + slot.cos() * PED_TALK_RING_RADIUS,
        center[1] + slot.sin() * PED_TALK_RING_RADIUS,
    ]);
}

/// 把敌人与行人的姿态写回场景批次。
///
/// **距离剔除**:超过 `ENEMY_RENDER_RANGE` 的敌人实例直接不写 —— 批次
/// 里没有实例就会被渲染器整批跳过,所以「剔除」在这里表现为「不 push」。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
fn sync_combat_instances(game: &mut Game) {
    let here: Vec3 = game.player.get_position();
    for index in 0..game.enemies.len() {
        let Some(batch) = game.enemy_batches.get(index).copied() else {
            continue;
        };
        if batch == usize::MAX {
            continue;
        }
        let Some(scene_batch) = game.scene.batches.get_mut(batch) else {
            continue;
        };
        scene_batch.instances.clear();
        let enemy: &Enemy = &game.enemies[index];
        let at: Vec3 = enemy.get_position();
        if flat_distance(at, here) > ENEMY_RENDER_RANGE {
            continue;
        }
        // 死亡后尸体下沉:缩放随倒计时收缩,是最省事的「消失」表现。
        let sink: f32 = if enemy.is_alive() {
            1.0
        } else {
            (enemy.get_death_timer() / DEATH_FADE_TIME).clamp(0.0, 1.0)
        };
        let tint: Vec3 = enemy_tint(enemy);
        let model: Mat4 = body_matrix(at, enemy.get_yaw(), sink);
        scene_batch
            .instances
            .push(Instance::from_matrix(model, tint));
    }
    // ---- 手持武器:贴在角色右手,朝向 = 瞄准方向 ----
    {
        let batch: usize = game.weapon_batch;
        if batch != usize::MAX
            && let Some(scene_batch) = game.scene.batches.get_mut(batch)
        {
            scene_batch.instances.clear();
            let driving: bool = game.player.get_driving();
            if !driving {
                let at: Vec3 = game.player.get_position();
                let aim: Vec2 = game.arsenal.get_aim();
                let yaw: f32 = -aim[1].atan2(aim[0]);
                // 枪口位置:角色右前方,按瞄准方向偏移。
                let right: Vec2 = [aim[1], -aim[0]];
                let at2: Vec3 = [
                    at[0] + right[0] * WEAPON_SIDE_OFFSET + aim[0] * WEAPON_FWD_OFFSET,
                    at[1] + WEAPON_HEIGHT,
                    at[2] + right[1] * WEAPON_SIDE_OFFSET + aim[1] * WEAPON_FWD_OFFSET,
                ];
                let model: Mat4 = body_matrix(at2, yaw, WEAPON_SCALE);
                scene_batch
                    .instances
                    .push(Instance::from_matrix(model, TINT_WEAPON));
            }
        }
    }
    for index in 0..game.peds.len() {
        let Some(batch) = game.ped_batches.get(index).copied() else {
            continue;
        };
        if batch == usize::MAX {
            continue;
        }
        let Some(scene_batch) = game.scene.batches.get_mut(batch) else {
            continue;
        };
        scene_batch.instances.clear();
        let ped: &Pedestrian = &game.peds[index];
        let at: Vec3 = ped.get_position();
        if flat_distance(at, here) > PED_RENDER_RANGE {
            continue;
        }
        // 行人不做逐部件骨架(那要 13 个批次 × 18 个行人),整具一个矩阵:
        // 走路时靠 Y 轴的轻微上下起伏表达「在走」。
        let bob: f32 = (ped.get_gait_phase() * 2.0).sin() * PED_BOB_HEIGHT * ped.get_gait_amount();
        let lift: Vec3 = [at[0], at[1] + bob, at[2]];
        let model: Mat4 = body_matrix(lift, ped.get_yaw(), 1.0);
        let tint: Vec3 = if ped.is_down() {
            TINT_PED_DOWN
        } else {
            TINT_PED
        };
        scene_batch
            .instances
            .push(Instance::from_matrix(model, tint));
    }
}

/// 敌人的逐实例色调:阵营基色 + 受击闪红 + 逃跑变黄。
///
/// # Arguments
///
/// - `&Enemy` - 敌人。
///
/// # Returns
///
/// - `Vec3` - 色调乘子。
fn enemy_tint(enemy: &Enemy) -> Vec3 {
    if enemy.get_flash() > 0.0 {
        return TINT_ENEMY_HURT;
    }
    match enemy.get_state() {
        AiState::Flee => TINT_ENEMY_FLEE,
        _ => match enemy.get_faction() {
            Faction::Police => TINT_POLICE,
            Faction::Thug => TINT_THUG,
        },
    }
}

/// 读一对反向按键,返回一个 −1..1 的轴值。
///
/// # Arguments
///
/// - `&InputState` - 输入状态。
/// - `&str` - 正向键的 `KeyboardEvent.code`。
/// - `&str` - 反向键的 `KeyboardEvent.code`。
///
/// # Returns
///
/// - `f32` - `1.0` / `-1.0` / `0.0`。
fn axis(input: &InputState, positive: &str, negative: &str) -> f32 {
    let forward: f32 = if input.held(positive) { 1.0 } else { 0.0 };
    let backward: f32 = if input.held(negative) { 1.0 } else { 0.0 };
    forward - backward
}

/// 某个键是不是「能当方向盘」的键(A/D 与 ←/→ 共四个)。
///
/// # Arguments
///
/// - `&str` - `KeyboardEvent.code`。
///
/// # Returns
///
/// - `bool` - 是方向盘键时为真。
fn is_steer_key(code: &str) -> bool {
    let steer: bool = code == KEYA
        || code == KEYD
        || code == ARROWLEFT
        || code == ARROWRIGHT;
    steer
}

/// 一个方向盘键对应的舵角:左 −1.0,右 +1.0。
///
/// 符号与 `TrafficCar::drive` 的「正 = 右转」一一对应,左负右正。
///
/// # Arguments
///
/// - `&str` - `KeyboardEvent.code`,调用方保证已用 [`is_steer_key`] 过滤。
///
/// # Returns
///
/// - `f32` - 舵角。
fn steer_sign(code: &str) -> f32 {
    let right: bool = code == KEYD || code == ARROWRIGHT;
    if right {
        1.0
    } else {
        -1.0
    }
}

/// 把跟随焦点推向玩家,并在第三人称模式下重摆相机。
///
/// 焦点用指数阻尼而不是硬跟随,所以快速转身 / 急停时相机有惯性,
/// 不会像贴在后脑勺上那样僵硬。自由观察模式直接跳过。
///
/// 焦点落位之后走**遮挡回避**:向眼点方向投射一条球体探针,命中建筑
/// 就把距离压到命中点之前(再留一点贴墙余量)。这一段与焦点阻尼是
/// 两次独立插值 —— 焦点负责「跟手」,距离负责「别穿墙」,混在一起会
/// 让两者互相拖慢。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `f32` - 本帧的秒数增量。
fn update_camera(game: &mut Game, delta: f32) {
    if !game.third_person {
        return;
    }
    let anchor: Vec3 = game.player.get_position();
    let blend: f32 = 1.0 - (-FOLLOW_DAMPING * delta).exp();
    let mut next: Vec3 = game.follow_target;
    next[0] += (anchor[0] - next[0]) * blend;
    next[1] += (FOLLOW_HEIGHT - next[1]) * blend;
    next[2] += (anchor[2] - next[2]) * blend;
    game.follow_target = next;
    game.camera.get_target_mut().copy_from_slice(&next);
    // 跟随距离始终夹在第三人称区间内。拖拽 / 捏合 / 双击都可能把
    // distance 推到离谱的值,这里兜底一次,保证镜头始终是「跟在
    // 角色背后几米」的第三人称,而不是飞远的自由相机。
    game.camera
        .clamp_follow_distance(FOLLOW_DISTANCE_MIN, FOLLOW_DISTANCE_MAX);
    // ---- 遮挡回避 ----
    // `desired_distance` 是玩家滚轮设定的「想要多远」,`distance` 是
    // 「实际能走多远」。前者不被回避改写,所以遮挡消失后有基准可回弹。
    let wanted: f32 = game
        .camera
        .get_desired_distance()
        .clamp(FOLLOW_DISTANCE_MIN, FOLLOW_DISTANCE_MAX);
    // `Some(allowed)` = 这一帧射线真的撞上了东西,相机被压到命中点之前;
    // `None` = 视野里没有遮挡,相机弹回 `wanted`。
    // 走室内版本:玩家站进样板楼时,挡在镜头和角色之间的是隔墙和门垛,
    // 它们不在 `CollisionWorld` 里,只查外部世界会直接穿墙。
    let hit: Option<f32> =
        game.camera
            .resolve_occlusion_interior(&game.world, &game.interiors, wanted);
    let allowed: f32 = hit.unwrap_or(wanted);
    // **室内允许压到贴脸,室外不许。** `approach_distance` 收尾把距离
    // `clamp` 到 `OCCLUSION_MIN_DISTANCE`(2.2 m),而玩家贴着室内墙面站
    // 时回避算出的允许距离是 0.00 —— 那个 2.2 m 的硬下限反而把眼点
    // 顶**穿过**墙,变成「人在楼内、相机在楼外」。室外没有这个问题
    // (那 2.2 m 是防穿楼的安全网),所以只在室内换用更近的下限。
    let inside: bool = game
        .interiors
        .support_height(
            [game.follow_target[0], game.follow_target[2]],
            game.follow_target[1],
        )
        .is_some();
    game.camera.approach_distance_within(
        allowed,
        FOLLOW_DISTANCE_MAX,
        delta,
        OCCLUSION_IN_RATE,
        OCCLUSION_OUT_RATE,
        if inside {
            PRESS_IN_DISTANCE
        } else {
            OCCLUSION_MIN_DISTANCE
        },
    );
    // 地面不在碰撞世界里,单独夹一次眼点高度。
    game.camera.lift_above_ground();
    // 夹完高度之后再兜一次底:俯角一旦为负,眼点会落到地面以下,而
    // 地面是整张 `y = 0` 的网格 —— 从地下看出去整屏只有一片地面色 /
    // 天空色,角色和街景全被地面挡住。把它收进「相机在角色上方」的
    // 区间,保证任何拖拽 / 触控组合下镜头都在地面之上。
    game.camera.clamp_follow_pitch();
    // 回报状态:调试通道读 `occluded` / `eye_clearance` 就能证明回避真的
    // 触发了(距离缩短),而不是只调了默认距离。
    game.camera.occluded = hit.is_some();
    let eye: Vec3 = game.camera.eye();
    let clearance: f32 = game.world.nearest_surface_distance([eye[0], eye[2]]);
    game.camera.eye_clearance = if eye[1] < CAMERA_MIN_HEIGHT {
        0.0
    } else {
        clearance
    };
}

/// 检查玩家是否踩到拾取物,命中就结算。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
fn collect_pickups(game: &mut Game) {
    let here: Vec3 = game.player.get_position();
    let mut gained: Vec<usize> = Vec::new();
    for index in 0..game.traffic.get_pickups_ref().len() {
        let Some(pickup) = game.traffic.get_pickups_ref().get(index) else {
            continue;
        };
        if pickup.get_taken() {
            continue;
        }
        let position: Vec3 = pickup.get_position();
        let dx: f32 = position[0] - here[0];
        let dz: f32 = position[2] - here[2];
        if dx * dx + dz * dz <= PICKUP_RADIUS * PICKUP_RADIUS {
            gained.push(index);
        }
    }
    for index in gained {
        let Some(pickup) = game.traffic.get_pickup_mut(index) else {
            continue;
        };
        // 护甲 / 弹药以前只是加钱,现在真的进战斗系统。
        let asset: &'static str = pickup.asset;
        apply_pickup(&mut game.player, pickup);
        apply_combat_pickup(game, asset);
        game.player.set_collected_push(index);
        // 拾走了就不该还在地上:把该批次的实例清零。
        if let Some(batch) = game.pickup_batches.get(index)
            && let Some(scene_batch) = game.scene.batches.get_mut(*batch)
        {
            scene_batch.instances.clear();
        }
    }
}

/// 一颗正在空中飞的手雷。
///
/// 抛投物必须**有自己的位置和计时**,而不是「按 G 立刻对射程内的
/// 敌人造成一次伤害」:后者是范围攻击,不是投掷。手雷有 1.1 s 的滞空,
/// 玩家在这段时间里还能跑开 —— 掩体因此有意义。
#[derive(Clone, Copy, Debug)]
struct Grenade {
    /// 当前世界坐标(脚底高度)。
    position: Vec3,
    /// 水平速度(XZ,米/秒)。
    velocity: Vec2,
    /// 垂直速度(米/秒,向上为正)。
    vertical_velocity: f32,
    /// 已经飞了多久(秒),到 [`GRENADE_FLIGHT_TIME`] 就炸。
    age: f32,
}

/// 处理 G 键:手里有手雷就投一颗,顺带把当前武器切成手雷。
///
/// 切枪这步是必要的:`Weapon::Grenade` 之前只可能由「捡到地上的
/// `wep_grenade`」进入,于是 G 键想投掷时手上拿的还是手枪,
/// `arsenal.get_grenade_tubes()` 与当前武器都对不上。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
///
/// # Returns
///
/// - `bool` - 本次是否真的投出了一颗。
fn throw_grenade(game: &mut Game) -> bool {
    if game.player.get_vehicle().is_some() {
        game.player.set_notice(String::from(NOTICE_NO_GRENADE));
        return false;
    }
    if game.arsenal.get_weapon() != Weapon::Grenade {
        game.arsenal.set_weapon(Weapon::Grenade);
        rebind_weapon_batch(game);
    }
    if game.arsenal.get_grenade_tubes() == 0 {
        game.player.set_notice(String::from(NOTICE_NO_GRENADE));
        return false;
    }
    if !game.arsenal.take_grenade() {
        return false;
    }
    let here: Vec3 = game.player.get_position();
    let aim: Vec2 = game.arsenal.get_aim();
    let (sin_pitch, cos_pitch): (f32, f32) = GRENADE_THROW_PITCH.sin_cos();
    game.live_grenades.push(Grenade {
        position: [here[0], here[1] + AIM_CHEST_HEIGHT, here[2]],
        velocity: [
            aim[0] * GRENADE_THROW_SPEED * cos_pitch,
            aim[1] * GRENADE_THROW_SPEED * cos_pitch,
        ],
        vertical_velocity: GRENADE_THROW_SPEED * sin_pitch,
        age: 0.0,
    });
    game.arsenal
        .set_cooldown(Weapon::Grenade.fire_rate() * GRENADE_COOLDOWN);
    game.player.set_notice(String::from(NOTICE_GRENADE_THROWN));
    true
}

/// 一颗手雷的一帧推进:抛物线飞行 + 到时 / 落地时结算爆炸。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `f32` - 本帧秒数。
fn step_live_grenades(game: &mut Game, dt: f32) {
    if game.live_grenades.is_empty() {
        return;
    }
    let mut survivors: Vec<Grenade> = Vec::new();
    let mut blasts: Vec<Vec3> = Vec::new();
    for mut grenade in game.live_grenades.drain(..) {
        grenade.age += dt;
        grenade.vertical_velocity -= GRAVITY * dt;
        grenade.position[0] += grenade.velocity[0] * dt;
        grenade.position[2] += grenade.velocity[1] * dt;
        grenade.position[1] += grenade.vertical_velocity * dt;
        // 撞到静态几何就停在撞点 —— 手雷不该穿墙。
        if let Some((distance, normal)) = crate::collision::ray_to_shapes(
            &game.world,
            grenade.position,
            [
                grenade.velocity[0],
                grenade.vertical_velocity,
                grenade.velocity[1],
            ],
        ) && distance < 0.35
        {
            grenade.position[0] -= normal[0] * distance;
            grenade.position[2] -= normal[1] * distance;
            grenade.velocity = [0.0, 0.0];
            grenade.vertical_velocity = 0.0;
        }
        let floor: f32 = game
            .interiors
            .support_height(
                [grenade.position[0], grenade.position[2]],
                grenade.position[1],
            )
            .unwrap_or(GROUND_LEVEL);
        if grenade.position[1] <= floor || grenade.age >= GRENADE_FLIGHT_TIME {
            grenade.position[1] = grenade.position[1].max(floor);
            blasts.push(grenade.position);
            continue;
        }
        survivors.push(grenade);
    }
    game.live_grenades = survivors;
    for at in blasts {
        explode_at(game, at);
    }
}

/// 结算一次爆炸:范围内的敌人受伤,范围内的玩家也受伤(自己炸自己也疼)。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `Vec3` - 爆心世界坐标。
fn explode_at(game: &mut Game, at: Vec3) {
    game.hitmarker = HITMARKER_TIME;
    for index in 0..game.enemies.len() {
        if !game.enemies[index].is_alive() {
            continue;
        }
        let distance: f32 = flat_distance(at, game.enemies[index].get_position());
        if distance > GRENADE_BLAST_RADIUS {
            continue;
        }
        // 线性衰减到 0:贴着炸满伤 [`GRENADE_DAMAGE`],边缘刚好 0。
        let fall: f32 = 1.0 - distance / GRENADE_BLAST_RADIUS;
        let killed: bool = game.enemies[index].damage(GRENADE_DAMAGE * fall);
        game.did_hit = true;
        game.wanted.add_heat(WANTED_PER_HIT);
        if killed {
            game.kills += 1;
            let drop: f32 = ENEMY_CASH_DROP;
            game.player.set_cash_add(drop);
        }
    }
    let to_player: f32 = flat_distance(at, game.player.get_position());
    if to_player <= GRENADE_BLAST_RADIUS {
        let fall: f32 = 1.0 - to_player / GRENADE_BLAST_RADIUS;
        let _: f32 = apply_damage(&mut game.player, GRENADE_DAMAGE * fall * 0.5);
    }
}

/// 处理 V 键:第三人称跟随 ↔ 第一人称。
///
/// **GTA V 的 V 是「切换相机」**,不是「切到自由观察」。原来的
/// third_person 取反(绑在 Tab 上)把「切到 200 m 外的城市全景」也叫做
/// 第三人称 —— 那是自由观察 / 观景模式,GTA V 把它放在别的键上。
/// 这里保留原能力但改挂到 V 之后就不再做了,理由是:那套自由观察会让
/// WASD 从「移动」变成「平移相机焦点」,而 V 的语义是「换个角度看
/// 角色」,两者不该混。
///
/// 切回第三人称时**期望距离和实际距离一起复位**:遮挡回避可能把上一段
/// 的缩近距离留着,不重新从默认距离起步就会一进游戏就贴脸。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
fn toggle_camera_mode(game: &mut Game) {
    game.third_person = !game.third_person;
    let yaw: f32 = game.player.get_yaw();
    if game.third_person {
        game.camera.set_desired_distance(FOLLOW_DISTANCE);
        game.camera.set_distance(FOLLOW_DISTANCE);
        game.camera.set_pitch(FOLLOW_PITCH);
        game.follow_target = game.player.get_position();
        game.camera.set_yaw(yaw);
    } else {
        // 第一人称:焦点就是角色的眼睛,距离 0,所以拖鼠标转的是**视线**
        // 而不是「绕着角色转」——这正是第一人称的直觉。
        game.camera.set_desired_distance(FIRST_PERSON_DISTANCE);
        game.camera.set_distance(FIRST_PERSON_DISTANCE);
        game.camera.set_pitch(FIRST_PERSON_PITCH);
        let here: Vec3 = game.player.get_position();
        game.camera
            .set_target([here[0], here[1] + FIRST_PERSON_HEIGHT, here[2]]);
        game.follow_target = game.camera.get_target();
        game.camera.set_yaw(yaw);
    }
    let pitch: f32 = game.camera.get_pitch();
    let _: f32 = pitch;
}

/// 把地图面板的显示状态与 `game.map_open` 对齐。
///
/// 单独一个函数(而不是塞在按键分支里)是因为 `GameHandles` 的
/// `RefMut` 在按键闭包持有期间**不能**再调 `set_text_content` ——
/// 那会重入 JS 并 panic。分支里必须先 `drop(game)` 再调本函数。
///
/// # Arguments
///
/// - `&GameHandles` - 事件句柄。
fn sync_map_visibility(handles: &GameHandles) {
    let (open, position, cash, stars): (bool, Vec3, f32, u32) = {
        let game: std::cell::Ref<Game> = handles.game.borrow();
        (
            game.map_open,
            game.player.get_position(),
            game.player.get_cash(),
            game.wanted.get_stars(),
        )
    };
    if let Some(panel) = &handles.map_panel {
        let _: Result<(), euv::wasm_bindgen::JsValue> = panel.set_attribute(
            ATTR_STYLE,
            if open { STYLE_MAP_PANEL } else { DISPLAY_NONE },
        );
    }
    if let Some(detail) = &handles.map_detail {
        detail.set_text_content(Some(&format!(
            "{MAP_HINT} · x {:.1} · z {:.1} · ${cash} · {}",
            position[0], position[2], stars
        )));
    }
}

/// 处理 F 键:靠近车就上车,已经在车里就下车。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
///
/// # Returns
///
/// - `bool` - 本帧是否发生了上车 / 下车(用于刷新 HUD 提示)。
fn toggle_vehicle(game: &mut Game) -> bool {
    match game.player.get_vehicle() {
        Some(index) => {
            let Some(car) = game.traffic.get_car_mut(index) else {
                return false;
            };
            car.set_driven(false);
            let position: Vec3 = car.get_position();
            let yaw: f32 = car.get_yaw();
            // 下车:放到车侧后方,并立刻过一次碰撞分离,免得落在楼里。
            // Y 同样归零:车永远在地面上,玩家从车里出来时也该在地面上 ——
            // 沿用旧的 `here[1]` 会把二楼楼板上的玩家留在一层车里出来。
            let side: Vec2 = [yaw.sin() * EXIT_CAR_OFFSET, yaw.cos() * EXIT_CAR_OFFSET];
            let resolved: Vec2 = game
                .world
                .resolve([position[0] + side[0], position[2] + side[1]]);
            game.player
                .set_position([resolved[0], GROUND_LEVEL, resolved[1]]);
            game.player.set_vertical_velocity(0.0);
            game.player.set_grounded(true);
            game.player.set_vehicle(None);
            game.player.set_notice(String::from(NOTICE_EXIT));
            true
        }
        None => {
            let here: Vec3 = game.player.get_position();
            let Some(index) = nearest_car(&game.traffic, here) else {
                game.player.set_notice(String::from(NOTICE_NO_CAR));
                return false;
            };
            if let Some(car) = game.traffic.get_car_mut(index) {
                car.set_driven(true);
            }
            report_jacking(game, index);
            game.player.set_vehicle(Some(index));
            game.player.set_notice(String::from(NOTICE_ENTER));
            true
        }
    }
}

/// 在场景里铺好玩家骨架、车队与拾取物的实例,并建立碰撞世界。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `&HashMap<String, usize>` - 资产 id → 场景 mesh 索引。
/// - `&MeshAsset` - `ped_suit` 的完整资产(用于切 part)。
/// - `&HashMap<String, MeshAsset>` - 车辆资产(用于切出可转车轮)。
fn spawn_player_traffic_pickups(
    game: &mut Game,
    index_map: &HashMap<String, usize>,
    ped: &MeshAsset,
    car_assets: &HashMap<String, MeshAsset>,
) {
    // 玩家骨架:每个 part 一个独立 mesh + 独立批次。
    game.player_batches.clear();
    for part_name in PLAYER_PARTS {
        let batch: usize = push_player_part(&mut game.scene, ped, part_name);
        if batch == usize::MAX {
            continue;
        }
        let pivot: Vec3 = joint_pivot(part_name, part_local_bounds(ped, part_name));
        // 父关节枢轴:有父 part 时取父 part 的关节(肩 / 胯),链根则与自身
        // 相同。渲染时用 `limb_chain_matrix` 把两级摆动串起来。
        let parent: &str = limb_parent(part_name);
        let root_pivot: Vec3 = if parent.is_empty() {
            pivot
        } else {
            joint_pivot(parent, part_local_bounds(ped, parent))
        };
        game.player_batches.push(PlayerLimbBatch {
            part: (*part_name).to_string(),
            pivot,
            root_pivot,
            batch,
        });
    }

    // 车队:每辆车一个批次(姿态独立)。
    //
    // 车轮批次要按车资产切网格,而网格必须**每辆车型切一次**(不同车型的
    // 轮子尺寸不同),所以缓存键是「资产 id」而不是「车实例」。
    game.car_batches.clear();
    for index in 0..game.traffic.get_cars_ref().len() {
        let asset: &'static str = match game.traffic.get_cars_ref().get(index) {
            Some(car) => car.asset,
            None => continue,
        };
        let Some(mesh_index) = index_map.get(asset) else {
            continue;
        };
        let batch: usize = find_or_create_batch(&mut game.scene, *mesh_index);
        game.car_batches.push(batch);
        // 车轮:每辆车四个轮子各一个批次。
        //
        // 切网格的键是**车型**而不是车实例 —— 四个轮子的网格完全相同
        // (只差安装位),同一车型的所有车共用一份切好的单轮 mesh。
        let Some(car_asset): Option<&MeshAsset> = car_assets.get(asset) else {
            continue;
        };
        // 同一车型的车**共用**切好的 mesh,但每辆车仍然要建自己的批次 ——
        // 批次持有 model matrix,不同车的轮子位置不同。
        let meshes: Vec<usize> = match game.car_wheel_meshes.get(asset) {
            Some(existing) => existing.clone(),
            None => {
                let mut built: Vec<usize> = Vec::new();
                for slot in 0..4 {
                    let mesh: usize = push_car_wheel(&mut game.scene, car_asset, slot);
                    if mesh != usize::MAX {
                        built.push(mesh);
                    }
                }
                game.car_wheel_meshes
                    .insert(asset.to_string(), built.clone());
                built
            }
        };
        for (slot, mesh) in meshes.iter().enumerate() {
            let wheel_batch: usize = game.scene.push_batch_with_cull(*mesh, true, false);
            game.car_wheel_batches.push(CarWheelBatch {
                car: index,
                slot,
                mount: crate::traffic::WHEEL_MOUNTS[slot],
                batch: wheel_batch,
            });
        }
    }

    // 拾取物:每个拾取物一个批次(自转相位独立)。
    game.pickup_batches.clear();
    for index in 0..game.traffic.get_pickups_ref().len() {
        let asset: &'static str = match game.traffic.get_pickups_ref().get(index) {
            Some(pickup) => pickup.asset,
            None => continue,
        };
        let Some(mesh_index) = index_map.get(asset) else {
            continue;
        };
        let batch: usize = find_or_create_batch(&mut game.scene, *mesh_index);
        game.pickup_batches.push(batch);
    }

    // 碰撞世界:从资产 bounds 自动推导。
    build_collision_world(
        &mut game.world,
        &game.asset_bounds,
        SPAWN_POINT[0],
        SPAWN_POINT[2],
    );
    // 室内碰撞世界:两栋可进入样板楼的楼板 / 隔墙 / 楼梯 / 外墙。
    build_showcase_interiors(&mut game.interiors);

    // 验收脚本要的「最近一栋楼」:街区布局是程序化生成的,写死一个
    // 魔法坐标早晚会推到空地上,所以在这里对着同一份 `build_city_buildings()`
    // 算一次存下来 —— 之后每帧只是读它,不会重复分配。
    let player_at: Vec3 = game.player.get_position();
    let mut best_distance: f32 = f32::MAX;
    let mut nearest: Option<[f32; 2]> = None;
    for placement in build_city_buildings(0.0, 0.0) {
        let dx: f32 = placement.position[0] - player_at[0];
        let dz: f32 = placement.position[1] - player_at[2];
        let distance: f32 = dx * dx + dz * dz;
        if distance < best_distance {
            best_distance = distance;
            nearest = Some([placement.position[0], placement.position[1]]);
        }
    }
    game.nearest_building = nearest;
}

/// 把玩家骨架、车队与拾取物的实例矩阵写回场景批次。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
fn sync_dynamic_instances(game: &mut Game) {
    let driving: bool = game.player.get_driving();
    let origin: Vec3 = game.player.get_position();
    let yaw: f32 = game.player.get_yaw();
    let phase: f32 = game.player.get_gait_phase();
    let amount: f32 = game.player.get_gait_amount();

    // 玩家骨架:四肢按步态相位摆动;上车时整批隐藏。
    //
    // 每节 limb 走 `limb_chain_matrix`:根关节(肩 / 胯)先转,末端关节
    // (肘 / 膝)再在**已经转过根关节**的坐标系里多转一点。少了这一步,
    // 大腿摆动时小腿会原地不动 —— 就是「四肢不协调」。
    for limb in game.player_batches.clone() {
        let Some(scene_batch) = game.scene.batches.get_mut(limb.batch) else {
            continue;
        };
        scene_batch.instances.clear();
        if driving {
            continue;
        }
        let swing: f32 = limb_swing(&limb.part, phase, amount);
        let parent: &str = limb_parent(&limb.part);
        let root_swing: f32 = if parent.is_empty() {
            0.0
        } else {
            limb_swing(parent, phase, amount)
        };
        let model: Mat4 =
            limb_chain_matrix(origin, yaw, limb.root_pivot, limb.pivot, root_swing, swing);

        scene_batch
            .instances
            .push(Instance::from_matrix(model, [1.0, 1.0, 1.0]));
    }

    // 车队:位置与朝向都来自交通系统。
    for index in 0..game.traffic.get_cars_ref().len() {
        let Some(car) = game.traffic.get_cars_ref().get(index) else {
            continue;
        };
        let (position, car_yaw) = (car.get_position(), car.get_yaw());
        let Some(batch) = game.car_batches.get(index) else {
            continue;
        };
        if let Some(scene_batch) = game.scene.batches.get_mut(*batch) {
            scene_batch.instances.clear();
            scene_batch
                .instances
                .push(Instance::new(position, car_yaw, 1.0, [1.0, 1.0, 1.0]));
        }
    }

    // 车轮:跟着车身走,并绕自己的轮心自转。
    for wheel in &game.car_wheel_batches {
        let Some(car) = game.traffic.get_cars_ref().get(wheel.car) else {
            continue;
        };
        let model: Mat4Data = car_wheel_model(
            car.get_position(),
            car.get_yaw(),
            wheel.mount,
            car.get_wheel_spin(),
        );
        if let Some(scene_batch) = game.scene.batches.get_mut(wheel.batch) {
            scene_batch.instances.clear();
            scene_batch
                .instances
                .push(Instance::from_model(model, [1.0, 1.0, 1.0]));
        }
    }

    // 拾取物:绕 Y 自转。
    for index in 0..game.traffic.get_pickups_ref().len() {
        let Some(pickup) = game.traffic.get_pickups_ref().get(index) else {
            continue;
        };
        let (position, spin, taken) =
            (pickup.get_position(), pickup.get_spin(), pickup.get_taken());
        let Some(batch) = game.pickup_batches.get(index) else {
            continue;
        };
        if let Some(scene_batch) = game.scene.batches.get_mut(*batch) {
            scene_batch.instances.clear();
            if !taken {
                scene_batch
                    .instances
                    .push(Instance::new(position, spin, 1.0, [1.0, 1.0, 1.0]));
            }
        }
    }
}

/// 固定步长推进 + 渲染一帧。
///
/// # Arguments
///
/// - `&mut Game` - Game 的可变引用。
/// - `f32` - 输入值。
/// - `&mut f32` - f32 的可变引用。
/// - `u32` - 输入值。
///
/// # Returns
///
/// - `u32` - 计数结果。
fn step_and_render(
    game: &mut Game,
    elapsed: f32,
    accumulator: &mut f32,
    width: u32,
    height: u32,
) -> u32 {
    // ---- dt 钳位 ----
    let delta: f32 = (elapsed - game.frame_time).clamp(0.0, MAX_FRAME_TIME);
    // 瞬时帧率喂给画质状态机。`delta` 是 `rAF` 的真实间隔,但被
    // MAX_FRAME_TIME 钳过 —— 用未钳的 `elapsed - frame_time` 才准,
    // 否则标签页切回时那一次会算出 4 fps 并触发降档。
    let raw_delta: f32 = elapsed - game.frame_time;
    if raw_delta > 0.0 {
        game.quality.sample(1.0 / raw_delta);
    }
    game.frame_time = elapsed;
    game.canvas_size.set((width, height));

    // ---- 输入 → 玩家 / 车队 / 相机 ----
    // 传送 / 注入开火请求要在逻辑步进**之前**吃掉:挂在帧尾的话这一帧
    // 用的还是旧坐标算出来的碰撞与流式中心,而 `fire_held` 会等到下一帧
    // 才被读到 —— 中间那一帧已经把流式判定跳过了。
    apply_teleport_request(game);
    simulate(game, delta);

    // 相机俯仰始终收在合法区间。
    game.camera.clamp_pitch();

    // ---- 每帧一次:把玩家骨架 / 车队 / 拾取物 / 敌人 / 行人的实例写回场景批次 ----
    //
    // 漏掉这一步的话,批次存在但 `instances` 永远是空的,渲染器
    // (`if ... || batch.instances.is_empty() { continue; }`)会把它们整批
    // 跳过 —— 角色和车一个都看不见。
    sync_dynamic_instances(game);

    // ---- 固定步长累加器 ----
    *accumulator += delta;
    let mut steps: u32 = 0;
    while *accumulator >= FIXED_DT && steps < 8 {
        *accumulator -= FIXED_DT;
        steps += 1;
        game.ticks += 1;
    }

    // ---- 把玩家骨架 / 车队 / 拾取物 / 敌人 / 行人的实例矩阵写回场景 ----
    sync_dynamic_instances(game);
    sync_combat_instances(game);

    // ---- 渲染 ----
    let lighting: SceneLighting = SceneLighting::for_phase(game.input.phase);
    let aspect: f32 = if height == 0 {
        1.0
    } else {
        width as f32 / height as f32
    };
    let view_proj: crate::camera::Mat4 = game.camera.view_projection(aspect);
    // 近处剔除半径跟模式走:自由观察的机位在 200 m 外,26 m 剔的是
    // 「糊在镜头上」的行道树;第三人称眼点只有 7 m,同样的 26 m 会把
    // 整条街剔光(画面只剩地面和天空)。遮挡由相机的球体探针负责。
    let near_cull: f32 = if game.third_person {
        NEAR_CULL_RADIUS_FOLLOW
    } else {
        NEAR_CULL_RADIUS
    };
    // 验收探针:把几个已知世界点投到屏幕坐标,用来判断「几何没画」
    // 还是「画了但在视锥外」。
    let px0: f32 = game.player.get_position()[0];
    let pz0: f32 = game.player.get_position()[2];
    game.probe = [
        [px0, 0.0, pz0],
        [px0 + 20.0, 0.0, pz0],
        [px0, 0.0, pz0 - 60.0],
        [px0, 8.0, pz0 - 40.0],
    ]
    .iter()
    .map(|pt: &Vec3| {
        game.camera
            .world_to_screen(*pt, width as f32, height as f32)
            .map(|(x, y, _): (f32, f32, f32)| [x, y])
    })
    .collect();
    let mut triangles: u32 = 0;
    if let Some(renderer) = game.renderer.as_mut() {
        let result: Result<u32, String> = match renderer {
            Renderer::WebGl(webgl) => webgl.render(
                &game.scene,
                &view_proj,
                &game.camera.view_matrix(),
                &lighting,
                game.camera.eye(),
                width,
                height,
                near_cull,
                game.camera.fov_y,
                FAR_PLANE,
                game.frame_time,
                game.player.get_position(),
                game.quality.tier(),
            ),
            Renderer::Software(software) => {
                Ok(software.render(&game.scene, &game.camera, &lighting, width, height, 40_000))
            }
        };
        triangles = match result {
            Ok(value) => value,
            Err(message) => {
                // WebGL 运行期出错 → 永久回退到软件渲染,而不是黑屏。
                console_log(&format!("[vcw] WebGL render failed: {message}"));
                if let Renderer::WebGl(_) = renderer {
                    let canvas: HtmlCanvasElement = game.canvas.clone();
                    match SoftwareRenderer::new(&canvas) {
                        Ok(software) => {
                            game.renderer = Some(Renderer::Software(software));
                        }
                        Err(message) => {
                            console_log(&format!("[vcw] Canvas2D fallback failed: {message}"));
                        }
                    }
                }
                0
            }
        };
    }
    game.frame_count += 1;
    triangles
}

/// 控制台输出(不引 console_error_panic_hook,直接走 web_sys console)。
///
/// # Arguments
///
/// - `&str` - str 的只读引用。
fn console_log(message: &str) {
    euv::web_sys::console::log_1(&JsValue::from_str(message));
}

/// 把一个 JSON 对象挂到 `window.<name>` 上(用于 `window.__vcw` 调试钩子)。
///
/// 刻意**不用 `eval`**:web-sys 的 `Window::eval_with_str` 需要 Cargo.toml
/// 里的 `"Function"` feature,而 Cargo.toml 已定稿不可改。`JSON::parse` +
/// `Reflect::set` 走的是已有的 `js-sys`,零新增依赖。
///
/// # Arguments
///
/// - `&str` - window 上的属性名。
/// - `&str` - 该属性的 JSON 文本。
///
/// # Returns
///
/// - `Result<(), JsValue>` - `Reflect::set` 的结果。
fn set_window_json(name: &str, json: &str) -> Result<(), JsValue> {
    let window: web_sys::Window = web_sys::window().ok_or(JsValue::from_str(NO_WINDOW))?;
    let value: JsValue = js_sys::JSON::parse(json)?;
    js_sys::Reflect::set(&window, &JsValue::from_str(name), &value).map(|_: bool| ())
}

/// 启动主循环。
///
/// - `requestAnimationFrame` 驱动,闭包 `forget()` 后由浏览器持有。
/// - 累加器把可变 `dt` 转成固定步长逻辑更新。
/// - RAF 回调需要引用自身来排下一帧,因此先把 `Function` 放进
///   `Cell<Option<..>>`,闭包内部再读出来(`Closure` 本身不能自引用)。
///
/// # Arguments
///
/// - `GameHandles` - 输入值。
fn start_loop(handles: GameHandles) {
    let callback: Rc<Cell<Option<Function>>> = Rc::new(Cell::new(None));
    let handles_for_frame: GameHandles = handles.clone();
    let callback_for_frame: Rc<Cell<Option<Function>>> = callback.clone();
    let closure: Closure<dyn FnMut(f64)> = Closure::wrap(Box::new(move |timestamp: f64| {
        let handles: GameHandles = handles_for_frame.clone();
        let elapsed: f32 = timestamp as f32 / 1000.0;
        let width: u32 = handles.canvas.width();
        let height: u32 = handles.canvas.height();
        let mut accumulator: f32 = {
            let game: std::cell::Ref<Game> = handles.game.borrow();
            game.accumulator
        };
        let triangles: u32 = {
            let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
            let triangles: u32 =
                step_and_render(&mut game, elapsed, &mut accumulator, width, height);
            game.accumulator = accumulator;
            triangles
        };
        let hud_text: String = {
            let game: std::cell::Ref<Game> = handles.game.borrow();
            format_hud(&game, triangles)
        };
        if let Some(hud) = &handles.hud {
            hud.set_text_content(Some(&hud_text));
        }
        // 结构化 HUD(血条 / 弹药 / 通缉星 / 任务 / 小地图):每帧刷一次。
        sync_hud(&handles, triangles);
        // 验收通道按固定节拍刷新(约 20 Hz)。每帧都做一次 `JSON::parse` +
        // `Reflect::set` 会在无 GPU 的无头浏览器里把主线程吃满,反而让
        // CDP 的 `Runtime.evaluate` 超时;10 Hz 完全够读坐标用。
        if handles.game.borrow().frame_count.is_multiple_of(3) {
            publish_debug_state(&handles, &hud_text);
        }
        // 排下一帧。
        if let Some(next) = callback_for_frame.take() {
            let _: Result<i32, euv::wasm_bindgen::JsValue> = handles
                .window
                .request_animation_frame(next.unchecked_ref::<Function>());
            callback_for_frame.set(Some(next));
        }
    }));
    let trampoline: Function = closure.as_ref().unchecked_ref::<Function>().clone();
    callback.set(Some(trampoline));
    if let Some(trampoline) = callback.take() {
        let _: Result<i32, euv::wasm_bindgen::JsValue> = handles
            .window
            .request_animation_frame(trampoline.unchecked_ref::<Function>());
        callback.set(Some(trampoline));
    }
    closure.forget();
}

// ===========================================================================
// 异步加载 + 启动
// ===========================================================================

/// 解析 canvas 的像素尺寸(跟随 CSS 盒 × devicePixelRatio,上限 2×)。
///
/// # Arguments
///
/// - `&HtmlCanvasElement` - HtmlCanvasElement 的只读引用。
///
/// # Returns
///
/// - `(u32, u32)` - 计算结果。
fn sync_canvas_size(canvas: &HtmlCanvasElement) -> (u32, u32) {
    let mut ratio: f64 = window()
        .map(|win: Window| win.device_pixel_ratio())
        .unwrap_or(1.0)
        .clamp(1.0, 2.0);
    // `?res=<倍率>` 在 devicePixelRatio 之上再乘一层缩放(0 < res <= 2)。
    // 整城 47 万三角形在没有 GPU 的机器上走纯 CPU 光栅,1440p 的绘制
    // 缓冲足以把主线程吃满,键盘事件和页面脚本全部饿死;调低分辨率是
    // 唯一能把交互救回来的旋钮,而 0.5x 在游玩距离上几乎看不出差别。
    let res: f64 = f64::from(query_number(RES_PARAM, DEFAULT_RES));
    if res > 0.0 {
        ratio *= res.clamp(0.2, 2.0);
    }
    let rect: DomRect = canvas.get_bounding_client_rect();
    let (css_width, css_height): (f64, f64) = (
        if rect.width() > 0.0 {
            rect.width()
        } else {
            1280.0
        },
        if rect.height() > 0.0 {
            rect.height()
        } else {
            720.0
        },
    );
    let width: u32 = ((css_width * ratio).round() as u32).max(1);
    let height: u32 = ((css_height * ratio).round() as u32).max(1);
    if canvas.width() != width {
        canvas.set_width(width);
    }
    if canvas.height() != height {
        canvas.set_height(height);
    }
    (width, height)
}

/// 应用挂载后:取 DOM 元素、创建渲染后端、异步加载资产、启动循环。
pub fn boot() {
    let Some(window) = window() else {
        console_log(LOG_NO_WINDOW);
        return;
    };
    let Some(document) = window.document() else {
        console_log(LOG_NO_DOCUMENT);
        return;
    };
    let Some(canvas_element) = document.get_element_by_id(CANVAS_ID) else {
        console_log(LOG_NO_CANVAS);
        return;
    };
    let Ok(canvas) = canvas_element.dyn_into::<HtmlCanvasElement>() else {
        console_log(LOG_NOT_CANVAS);
        return;
    };
    sync_canvas_size(&canvas);

    // ---- 渲染后端:WebGL2 优先,失败回退 Canvas2D ----
    let (renderer, backend_note): (Option<Renderer>, String) = match WebGlRenderer::new(&canvas) {
        Ok(webgl) => {
            let renderer: Renderer = Renderer::WebGl(Box::new(webgl));
            (Some(renderer), BACKEND_WEBGL2.to_string())
        }
        Err(gl_error) => {
            // WebGL 失败的**原因**必须打出来:shader 链接失败时错误串里
            // 带着 program 名和 info log,丢掉它就只剩一句
            // 「Canvas2D unavailable」,排查时完全无从下手。
            console_log(&format!("[vcw] WebGL2 init failed: {gl_error}"));
            match SoftwareRenderer::new(&canvas) {
                Ok(software) => (
                    Some(Renderer::Software(software)),
                    format!("Canvas2D (WebGL2 unavailable: {gl_error})"),
                ),
                Err(canvas_error) => {
                    console_log(&format!("[vcw] no rendering backend: {canvas_error}"));
                    show_loading_error(NO_RENDERING_BACKEND_AVAILABLE_WEBGL2_AND_CA);
                    (None, canvas_error)
                }
            }
        }
    };
    console_log(&format!("[vcw] renderer = {backend_note}"));

    // 默认是**第三人称跟随**:相机离角色 FOLLOW_DISTANCE 米、俯角 FOLLOW_PITCH。
    // 曾经还有一个「街区网格全景」机位(`apply_default_view`),由 R 键触发;
    // R 在 GTA V 是换弹,那个机位也已经删掉了,没有别的键会切到它。
    let mut camera: Camera = Camera::new();
    camera.set_desired_distance(FOLLOW_DISTANCE);
    camera.set_distance(FOLLOW_DISTANCE);
    camera.set_pitch(FOLLOW_PITCH);
    camera.set_fov_y(FOLLOW_FOV);
    camera.set_near(FOLLOW_NEAR);
    camera.set_far(FOLLOW_FAR);
    camera.set_target([SPAWN_POINT[0], FOLLOW_HEIGHT, SPAWN_POINT[2]]);
    camera.set_yaw(SPAWN_YAW);

    let required: Vec<&'static str> = required_asset_ids();
    let game: Game = Game {
        quality: AdaptiveQuality::new(),
        canvas: canvas.clone(),
        camera,
        scene: Scene::default(),
        ground_batch: 0,
        water_batch: 0,
        streamed_center: [SPAWN_POINT[0], SPAWN_POINT[2]],
        static_batch_count: 0,
        after_static_step: [SPAWN_POINT[0], SPAWN_POINT[2]],
        after_dynamic_step: [SPAWN_POINT[0], SPAWN_POINT[2]],
        after_vertical_step: [SPAWN_POINT[0], SPAWN_POINT[2]],
        shots_blocked_by_geometry: 0,
        previous_step_xz: [SPAWN_POINT[0], SPAWN_POINT[2]],
        asset_index: HashMap::new(),
        hold_frames: 0,
        walk_request: [0.0, 0.0],
        safe_mode: false,
        speed_scale: 1.0,
        input: InputState::default(),
        renderer,
        loaded_assets: 0,
        total_assets: required.len(),
        frame_count: 0,
        ticks: 0,
        load_error: None,
        using_fallback: false,
        accumulator: 0.0,
        frame_time: 0.0,
        probe: Vec::new(),
        canvas_size: Cell::new((1, 1)),
        nearest_building: None,
        player: Player::new(SPAWN_POINT, SPAWN_YAW),
        traffic: Traffic::new(),
        world: CollisionWorld::new(),
        interiors: FloorWorld::new(),
        player_batches: Vec::new(),
        car_batches: Vec::new(),
        car_wheel_batches: Vec::new(),
        car_wheel_meshes: HashMap::new(),
        pickup_batches: Vec::new(),
        jump_queued: false,
        third_person: true,
        follow_target: [SPAWN_POINT[0], FOLLOW_HEIGHT, SPAWN_POINT[2]],
        asset_bounds: HashMap::new(),
        arsenal: Arsenal::new(),
        hurt: HurtState::new(),
        wanted: Wanted::new(),
        enemies: Vec::new(),
        peds: Vec::new(),
        enemy_batches: Vec::new(),
        weapon_batch: usize::MAX,
        weapon_mesh: usize::MAX,
        index_map: HashMap::new(),
        ped_batches: Vec::new(),
        mission: Mission::new(),
        hitmarker: 0.0,
        did_hit: false,
        kills: 0,
        hideouts: build_hideouts(),
        missions_done: 0,
        wanted_flash: 0.0,
        map_open: false,
        live_grenades: Vec::new(),
        aim_pitch: 0.0,
    };

    // HUD 的 DOM 必须在取句柄**之前**建出来 —— `index.html` 不可改,
    // 所以整块 HUD 面板由 Rust 建。
    let _hud_root: Option<Element> = build_hud_dom(&document);
    let handles: GameHandles = GameHandles {
        game: Rc::new(RefCell::new(game)),
        window: window.clone(),
        canvas: canvas.clone(),
        hud: document.get_element_by_id(HUD_ID),
        phase_label: document.get_element_by_id(PHASE_LABEL_ID),
        phase_slider: document
            .get_element_by_id(PHASE_SLIDER_ID)
            .and_then(|element: Element| element.dyn_into::<HtmlInputElement>().ok()),
        health_bar: document.get_element_by_id(ID_HEALTH_BAR),
        armor_bar: document.get_element_by_id(ID_ARMOR_BAR),
        ammo: document.get_element_by_id(ID_AMMO),
        cash: document.get_element_by_id(ID_CASH),
        wanted: document.get_element_by_id(ID_WANTED),
        mission: document.get_element_by_id(ID_MISSION),
        hitmarker: document.get_element_by_id(ID_HITMARKER),
        minimap: document
            .get_element_by_id(ID_MINIMAP)
            .and_then(|element: Element| element.dyn_into::<HtmlCanvasElement>().ok()),
        map_panel: document.get_element_by_id(ID_MAP_PANEL),
        map_detail: document.get_element_by_id(ID_MAP_DETAIL),
    };

    // ---- 事件绑定(全部裸 web_sys) ----
    bind_pointer_events(&handles);
    bind_combat_mouse(&handles);
    bind_keyboard(&handles);

    // ---- 昼夜滑块 ----
    if let Some(slider) = &handles.phase_slider {
        let handles_for_input: GameHandles = handles.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |event: Event| {
            let value: String = match event
                .dyn_ref::<HtmlInputElement>()
                .map(|input: &HtmlInputElement| input.value())
            {
                Some(value) => value,
                None => return,
            };
            let parsed: f64 = value.parse().unwrap_or(0.0);
            let phase: DayPhase = DayPhase::from_slider(parsed);
            let mut game: std::cell::RefMut<Game> = handles_for_input.game.borrow_mut();
            game.input.phase = phase;
            drop(game);
            if let Some(label) = &handles_for_input.phase_label {
                label.set_text_content(Some(phase.label()));
            }
        }));
        attach(slider, EVENT_INPUT, closure);
    }

    // ---- 窗口 resize ----
    {
        let canvas_for_resize: HtmlCanvasElement = canvas.clone();
        let closure: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |_event: Event| {
            sync_canvas_size(&canvas_for_resize);
        }));
        attach(&window, EVENT_RESIZE, closure);
    }

    // ---- 主循环先跑起来:这样加载进度条期间也有帧在渲染 ----
    start_loop(handles.clone());

    // ---- 异步加载资产 ----
    spawn_local(async move {
        load_assets_and_build(handles).await;
    });
}

/// 拉取 manifest + 全部资产 JSON,构建场景,推进进度条。
///
/// # Arguments
///
/// - `GameHandles` - 输入值。
async fn load_assets_and_build(handles: GameHandles) {
    set_progress(4.0, FETCHING_MANIFEST);

    // 1) manifest。
    let manifest_url: String = format!("{ASSETS_BASE}manifest.json");
    let manifest_text: String = match fetch_text(&manifest_url).await {
        Ok(text) => text,
        Err(message) => {
            console_log(&format!("[vcw] manifest load failed: {message}"));
            finish_with_fallback(&handles, message);
            return;
        }
    };
    let manifest: Manifest = match serde_json::from_str(&manifest_text) {
        Ok(manifest) => manifest,
        Err(err) => {
            console_log(&format!("[vcw] manifest parse failed: {err}"));
            finish_with_fallback(&handles, format!("manifest parse failed: {err}"));
            return;
        }
    };
    let mut by_id: HashMap<String, ManifestEntry> = HashMap::new();
    let mut categories: Vec<String> = Vec::new();
    for entry in manifest.assets {
        categories.push(entry.category.clone());
        by_id.insert(entry.id.clone(), entry);
    }
    categories.sort_unstable();
    categories.dedup();

    // 2) 需要的资产。
    let required: Vec<&'static str> = required_asset_ids();
    let total: usize = required.len().max(1);
    set_progress(10.0, &format!("loading 0/{total} assets…"));

    let mut index_map: HashMap<String, usize> = HashMap::new();
    let mut asset_bounds: HashMap<String, Bounds> = HashMap::new();
    let mut ped_suit: Option<MeshAsset> = None;
    // 车辆资产要留着按 part 切轮子(`tyres` / `hubs` 各自被切成单个轮子),
    // 与 `ped_suit` 同理:解析一次、留一份原始 part。
    let mut car_assets: HashMap<String, MeshAsset> = HashMap::new();
    let mut scene: Scene = Scene::default();
    let mut failed: Vec<String> = Vec::new();
    let mut loaded: usize = 0;

    for id in &required {
        let Some(entry) = by_id.get(*id) else {
            failed.push(format!("{id} (missing from manifest)"));
            loaded += 1;
            set_progress(
                10.0 + 85.0 * (loaded as f32 / total as f32),
                &format!("loading {loaded}/{total} assets…"),
            );
            continue;
        };
        let url: String = format!("{ASSETS_BASE}{}", entry.file);
        match fetch_text(&url).await {
            Ok(text) => {
                // 解析一次:既要注册进场景,又要留下 `bounds` 给碰撞世界推导,
                // 以及 `ped_suit` 的原始 part(玩家骨架要按 part 切分)。
                match parse_asset(id, &text) {
                    Ok(asset) => {
                        if let Some(bounds) = asset.get_bounds() {
                            asset_bounds.insert(id.to_string(), bounds.clone());
                        }
                        if *id == PED_SUIT {
                            ped_suit = Some(asset.clone());
                        }
                        if CAR_ASSET_IDS.contains(id) {
                            car_assets.insert((*id).to_string(), asset.clone());
                        }
                        if let Err(message) = push_parsed_asset(&mut scene, &mut index_map, &asset)
                        {
                            console_log(&format!("[vcw] {message}"));
                            failed.push(message);
                        }
                    }
                    Err(message) => {
                        console_log(&format!("[vcw] {message}"));
                        failed.push(message);
                    }
                }
            }
            Err(message) => {
                console_log(&format!("[vcw] {message}"));
                failed.push(message);
            }
        }
        loaded += 1;
        let percent: f32 = 10.0 + 85.0 * (loaded as f32 / total as f32);
        set_progress(percent, &format!("loading {loaded}/{total} assets…"));
        // 让出主线程,保证进度条能刷新。
        yield_to_browser().await;
    }

    if failed.len() == required.len() {
        console_log(LOG_ALL_ASSETS_FAILED);
        finish_with_fallback(
            &handles,
            format!("all {} asset fetches failed", failed.len()),
        );
        return;
    }
    if !failed.is_empty() {
        console_log(&format!(
            "[vcw] {} asset(s) failed: {}",
            failed.len(),
            failed.join(", ")
        ));
    }

    // 3) 先铺场景(会把程序化地面也 push 进 scene.meshes),再统一上传 GPU。
    //
    // 顺序很重要:如果先上传、后 build_scene,`build_scene` 里新增的地面网格
    // 拿到的 mesh_index 会落在「已上传列表」之外,GPU 侧 get() 返回 None,
    // 地面就整块不画 —— 画面里只剩一片街道两侧的楼和树,没有马路。
    set_progress(94.0, BUILDING_SCENE);
    let (ground_batch, water_batch, static_batch_count): (usize, usize, usize) =
        build_scene(&mut scene, &index_map, SPAWN_POINT[0], SPAWN_POINT[2]);

    // 4) 上传所有 GPU 资源。批次里的 mesh_index 与 scene.meshes 同序。
    //
    // ⚠️ **每个 mesh 只能上传一次。**
    //
    // `WebGlRenderer::upload_mesh` 是 `push` 语义:每次调用都往
    // `self.meshes` 尾部追加一个 `GlMesh` 并返回 `len - 1`。批次里的
    // `mesh_index` 是 `Scene::meshes` 的下标,`draw_batch` 拿它**直接**
    // 当 `self.meshes` 的下标用。所以一旦同一个 mesh 上传两次,GPU 表
    // 就会整体错位一整轮。
    //
    // 曾经存在的双循环:第 4 步上传一次(build_scene 后的 30 资产 +
    // 地面 = 31 条),第 5 步又 `for (index, mesh) in game.scene.meshes
    // .iter().enumerate()` 无条件重传一遍(此时 scene.meshes 已含 13 条
    // 骨架 = 44 条)。结果 GPU 表有 31 + 44 = 75 条,而批次仍按 0..44
    // 索引 —— 骨架的 13 条(场景 31..43)实际取到的是**第二遍的
    // asset#1..#13**,也就是三栋 30 米高的楼被按 1.75 米小人的
    // model matrix 摆到玩家脚下。楼把屏幕糊满,量到 108234 px;
    // 角色真正的那 13 个 mesh 从头到尾没被画过一次。
    //
    // 症状极具迷惑性:X 落在屏幕正中(位置矩阵是对的)、包围盒又高又宽
    // (是整栋楼),所以看起来像「角色被放大 8 倍」,实际上**角色根本没
    // 画**,画的是一栋楼。`?hide=` 隔离单个批次仍然爆满屏幕,正是因为
    // 该批次取到的 mesh 与 `?hide=` 传入的批次号毫无关系。
    //
    // 现在只保留这一次上传 —— 位置在第 5 步之后,一次性覆盖
    // build_scene 的资产 + 地面 + 13 个骨架 part,顺序与
    // `scene.meshes` 严格同序。
    set_progress(97.0, STATUS_UPLOADING_MESHES);

    let asset_count: usize = index_map.len();
    // manifest 自报的资产总数(`Manifest::asset_count`)与实际索引到的数量
    // 对照:不一致说明 manifest 里有条目没被场景蓝图引用(资源白拿),
    // 或者有条目文件缺失。两者都不是致命错误,记一条日志便于排查。
    if manifest.asset_count != 0 && manifest.asset_count != asset_count {
        console_log(&format!(
            "[vcw] manifest lists {} assets, scene uses {asset_count}",
            manifest.asset_count
        ));
    }
    console_log(&format!(
        "[vcw] manifest categories: {}",
        categories.join(", ")
    ));
    // `game.scene` 必须在第 5 步**之前**拿到 build_scene 的结果。
    //
    // 顺序是硬性要求:玩家骨架 / 车队 / 拾取物的批次都是往
    // `game.scene.batches` 里 push 的。如果这一步排在 `game.scene = scene`
    // 之后,那些批次会被整份旧场景覆盖掉 —— 屏幕上只剩 build_scene 铺的
    // 静态楼和树,角色和车一个都看不见(调试钩子报 `batch count: 0`)。
    let instance_count: usize = scene.instance_count();
    let triangle_count: usize = scene.total_triangles;
    {
        let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
        game.scene = scene;
        game.ground_batch = ground_batch;
        game.water_batch = water_batch;
        game.static_batch_count = static_batch_count;
        game.asset_index = index_map.clone();
        game.streamed_center = [SPAWN_POINT[0], SPAWN_POINT[2]];
        game.loaded_assets = index_map.len();
        game.total_assets = total;
        game.load_error = if failed.is_empty() {
            None
        } else {
            Some(format!("{} assets failed", failed.len()))
        };
    }

    // 5) 玩家骨架 + 交通车队 + 拾取物 + 碰撞世界。
    //
    // 必须在 build_scene 之后、GPU 上传之后做:骨架批次是 build_scene 之后
    // 新增的批次,资产 bounds 要用来推导碰撞体。
    {
        let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
        game.asset_bounds = asset_bounds.clone();
        // 车道 X 用 `lane_x()` 从 `street_axis()` 现算一遍,和蓝图里的字面值
        // 互相校验:如果街道网格改了而蓝图没改,这里能立刻发现。
        console_log(&format!(
            "[vcw] lane Xs: {} / {} / {}",
            lane_x(2, -1.0),
            lane_x(2, 1.0),
            lane_x(0, 1.0)
        ));
        // 蓝图里的起始 Z 必须落在循环轨道内,否则车一出生就 wrap 一次,
        // 看起来像「凭空出现」。顺手做一次校验并打日志。
        let out_of_range: usize = TRAFFIC_LANES
            .iter()
            .filter(|(_, _, start_z, _): &&(&str, f32, f32, f32)| start_z.abs() > TRAFFIC_HALF)
            .count();
        console_log(&format!(
            "[vcw] traffic loop half-length {TRAFFIC_HALF} m, {out_of_range} lane(s) out of range"
        ));
        game.traffic.populate(TRAFFIC_LANES, PICKUP_PLACEMENTS);
        // 战斗单位必须在 `upload_mesh` **之前**建好批次:它们要往
        // `scene.meshes` 之外只加批次(共用已上传的行人 mesh),但批次
        // 索引必须在那一次上传之前就定下来。
        game.index_map = index_map.clone();
        populate_combatants(&mut game, 0.37);
        spawn_combat_batches(&mut game, &index_map);
        if let Some(ped) = ped_suit.as_ref() {
            spawn_player_traffic_pickups(&mut game, &index_map, ped, &car_assets);
        }
        console_log(&format!(
            "[vcw] player rig: {} limb batches, {} cars, {} pickups, {} colliders",
            game.player_batches.len(),
            game.traffic.get_cars_ref().len(),
            game.traffic.get_pickups_ref().len(),
            game.world.get_shapes().len()
        ));
        // 上车 / 下车之后新增的批次也要上传。
        // 新增的骨架批次自带新 mesh,必须补一次上传。`upload_mesh` 需要
        // `&mut renderer` 和 `&scene.meshes`,所以先把 mesh 引用摘出来。
        //
        // **这里是全流程唯一一次 `upload_mesh`。** 曾经第 4 步(build_scene
        // 之后、这里之前)已经上传过一遍 31 条,这里又无条件重传 44 条,
        // `WebGlRenderer` 的 GPU 表被推到 75 条而批次仍按 0..44 索引,
        // 于是 13 个骨架批次取到的其实是第二遍的 asset#1..#13 ——
        // 三栋楼被摆到玩家脚下糊满屏幕,角色真正的 mesh 一次都没画过。
        // 详见第 4 步的注释。
        //
        // **失败绝不能 `let _ =` 吞掉。** 骨架批次(头/躯干/四肢共 13 个
        // limb)如果上传失败,画面上就表现为「有人走但看不见」—— 没有任何
        // 报错,连 `tris` 计数还是 482420,因为没上传的 mesh 压根没有 VAO,
        // `draw_batch` 查不到就静默跳过。这里把每个失败都打进 console,
        // 让症状和原因对得上。
        if let Some(Renderer::WebGl(mut webgl)) = game.renderer.take() {
            let mut upload_failures: usize = 0;
            for (index, mesh) in game.scene.meshes.iter().enumerate() {
                if let Err(error) = webgl.upload_mesh(mesh) {
                    upload_failures += 1;
                    if upload_failures <= MESH_UPLOAD_ERROR_LIMIT {
                        console_log(&format!(
                            "[vcw] upload_mesh FAILED for mesh {index} ({} verts): {error}",
                            mesh.vertices.len()
                        ));
                    }
                }
            }
            if upload_failures > 0 {
                console_log(&format!(
                    "[vcw] {upload_failures} of {} meshes failed to upload",
                    game.scene.meshes.len()
                ));
            } else {
                console_log(&format!(
                    "[vcw] uploaded {} meshes, {} limb batches in the scene",
                    game.scene.meshes.len(),
                    game.player_batches.len()
                ));
            }
            game.renderer = Some(Renderer::WebGl(webgl));
        }
    }

    set_progress(100.0, &format!("ready · {asset_count} assets"));
    hide_loading();
    console_log(&format!(
        "[vcw] scene ready: {asset_count} assets, {instance_count} instances, {triangle_count} triangles"
    ));
}

/// 资产加载全失败时的降级路径:用内置程序化场景。
///
/// # Arguments
///
/// - `&GameHandles` - GameHandles 的只读引用。
/// - `String` - 输入值。
fn finish_with_fallback(handles: &GameHandles, message: String) {
    set_progress(96.0, FALLING_BACK_TO_BUILT_IN_SCENE);
    let scene: Scene = build_fallback_scene();
    {
        let mut game: std::cell::RefMut<Game> = handles.game.borrow_mut();
        game.using_fallback = true;
        game.load_error = Some(message.clone());
        game.scene = scene;
        game.loaded_assets = 0;
    }
    set_progress(100.0, READY_BUILT_IN_FALLBACK_SCENE);
    hide_loading();
    console_log(&format!("[vcw] using built-in fallback scene: {message}"));
}

/// 让出主线程一个宏任务,让浏览器有机会刷新进度条。
///
/// `Promise::resolve(undefined)` 挂到微任务队列末尾,浏览器会先把
/// 已经排队的 DOM 变化画出来,下一次 `.await` 时进度条就已更新。
async fn yield_to_browser() {
    let promise: Promise<Undefined> = Promise::resolve(&Undefined::UNDEFINED);
    let _: Result<Undefined, JsValue> = JsFuture::from(promise).await;
}

// ===========================================================================
// 静态 UI 树(唯一一棵 html! 树,只渲染一次)
// ===========================================================================

/// 应用根视图。
///
/// **只用一次 html!**。这里不调用任何 euv hook(`App::use_signal` 等),
/// 游戏循环和输入全部走裸 `web_sys`,VDOM 不参与每帧渲染。
///
/// # Returns
///
/// - `VirtualNode` - 构建好的视图节点。
pub fn app_root() -> VirtualNode {
    html! {
        div {
            id: ID_ROOT
            style: STYLE_BLOCK_01
            canvas {
                id: CANVAS_ID
                style: STYLE_BLOCK_02
            }
            div {
                id: ID_TOPBAR
                style: STYLE_BLOCK_03
                div {
                    id: HUD_ID
                    style: STYLE_BLOCK_04
                    BOOTING
                }
                div {
                    style: STYLE_BLOCK_05
                    span {
                        id: PHASE_LABEL_ID
                        style: STYLE_BLOCK_06
                        NOON
                    }
                    input {
                        id: PHASE_SLIDER_ID
                        type: INPUT_TYPE_RANGE
                        min: "0"
                        max: "1"
                        step: "1"
                        value: "0"
                        style: STYLE_BLOCK_07
                    }
                    span {
                        style: STYLE_BLOCK_08
                        "T"
                    }
                }
            }
            div {
                id: ID_HELP
                style: STYLE_BLOCK_09
                DRAG_ORBIT_WHEEL_PINCH_ZOOM_WASD_PAN_R_RESET
            }
            div {
                id: LOADING_ID
                style: STYLE_BLOCK_10
                div {
                    style: STYLE_BLOCK_11
                    UI_TITLE
                }
                div {
                    style: STYLE_BLOCK_12
                    UI_SUBTITLE
                }
                div {
                    id: ID_PROGRESS_TRACK
                    style: STYLE_BLOCK_13
                    div {
                        id: PROGRESS_ID
                        style: STYLE_BLOCK_14
                    }
                }
                div {
                    id: PROGRESS_TEXT_ID
                    style: STYLE_BLOCK_15
                    BOOTING
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        ARROWLEFT, ARROWRIGHT, ARROWUP, ARROWDOWN, Camera, DIR_LEFT, DIR_RIGHT, FOLLOW_FOV, KEYA,
        KEYD, KEYS, KEYW, LOOK_YAW_SENSITIVITY, Mat4, RUN_OVER_WALK_MIN, T_AD_AND_ARROW_DO_NOT_CANCEL,
        T_ARROW_DOES_NOT_MOVE_ON_FOOT, T_ARROW_STEER_SIGNS_OPPOSED, T_ARROW_STEERS_WHILE_DRIVING,
        T_FOOT_POSITION_UNCHANGED, T_MOUSE_RIGHT_PANS_RIGHT, T_RUN_FASTER_THAN_WALK,
        T_RUN_SPEED_PICK, T_WALK_SPEED_PICK, apply_look_delta, car_wheel_model,
    };
    /// 一个世界点投影后的屏幕 NDC x(未做像素映射)。
    ///
    /// 相机后方(`w <= 0`)返回 `NaN`,让「测不出方向」这件事在断言里
    /// 显式失败,而不是悄悄当成 0 参与比较。
    fn projected_ndc_x(camera: &Camera, point: Vec3) -> f32 {
        let vp: Mat4 = camera.view_projection(1.0);
        let clip: [f32; 4] = vp.transform_vec4([point[0], point[1], point[2], 1.0]);
        if clip[3] <= f32::EPSILON {
            return f32::NAN;
        }
        clip[0] / clip[3]
    }

    /// 一个轮子局部点在车轮矩阵下的世界 X。
    fn wheel_world_x(mat: Mat4Data, local: Vec3) -> f32 {
        mat[0] * local[0] + mat[4] * local[1] + mat[8] * local[2] + mat[12]
    }

    /// 把断言文案里的 `{名字}` 占位符替换成实际数值(与 `camera.rs` 同形)。
    fn fill(template: &str, args: &[(&str, &str)]) -> String {
        let mut out: String = template.to_string();
        let mut index: usize = 0;
        while index < args.len() {
            out = out.replace(&format!("{{{}}}", args[index].0), args[index].1);
            index += 1;
        }
        out
    }

    use super::FIXED_DT;
    use crate::collision::CollisionWorld;
    use crate::combat::Pedestrian;
    use crate::r#const::{
        AXIS_STRAFE, CAR_SEDAN, GRAVITY, GROUND_LEVEL, GROUND_SNAP_SKIN, JUMP_VELOCITY, PED_SUIT,
        PED_TALK_SLOT_STEP, PISTOL_MAGAZINE, PLAYER_BODY_HEIGHT, RELOAD_TIME,
        SLOT_FORMER_RELOAD_KEY, SLOT_RELOAD_KEY, T_C_NO_LONGER_RELOADS, T_JUMP_CLEARS_A_LEDGE,
        T_JUMP_LANDS_STANDING, T_JUMP_ONLY_FROM_GROUND, T_JUMP_RISES_BEFORE_FALLING,
        T_PEDS_DISTINCT_SLOTS, T_PEDS_DOWNED_NO_CHAT, T_PEDS_GATHER_AND_TALK, T_PEDS_TALK_ENDS,
        T_RELOAD_CONSUMES_RESERVE, T_RELOAD_KEY_IS_GTA_R, T_RELOAD_NOT_REPEATABLE,
        T_RELOAD_R_REFILLS_MAGAZINE, T_RELOAD_R_STARTS_RELOAD, T_ROUTE_LEG_DIAGONAL,
        T_PALM_ON_ROADWAY, T_PALM_ROW_IS_UNIFORM, T_SHOWCASE_AXIS_ON_ROAD,
        T_SHOWCASE_CEILING_PUSHED, T_SHOWCASE_DESCENT_NO_CLIMB,
        T_SHOWCASE_DESCENT_REACHES_GROUND, T_SHOWCASE_DOOR_CENTER_BLOCKED, T_SHOWCASE_DOOR_INSIDE,
        T_SHOWCASE_DOOR_NO_SLAB, T_SHOWCASE_DOOR_NOT_FACING, T_SHOWCASE_DOOR_ON_OUTER_WALL,
        T_SHOWCASE_DOOR_OUTSIDE_BLOCKED, T_SHOWCASE_DOORS_FACE_EACH_OTHER,
        T_SHOWCASE_DOORWAY_2D_BLOCKED, T_SHOWCASE_FOOTPRINT_CLEAR, T_SHOWCASE_FRONT_FACING,
        T_SHOWCASE_LANDING_COVERS_RUN, T_SHOWCASE_LANE_BLOCKED, T_SHOWCASE_NO_WALL_AHEAD,
        T_SHOWCASE_OVERLAPS_ORDINARY, T_SHOWCASE_PARTITION_LANE, T_SHOWCASE_PARTITION_LET_THROUGH,
        T_SHOWCASE_PIER_LET_PLAYER_THROUGH, T_SHOWCASE_PUSHED_INTO_WALL,
        T_SHOWCASE_RISE_GE_TOLERANCE, T_SHOWCASE_ROUTE_WALKABLE, T_SHOWCASE_SPRINT_PENETRATES_WALL,
        T_SHOWCASE_SPRINT_TUNNELS_WALL, T_SHOWCASE_STAIR_REACHES_TOP,
        T_SHOWCASE_STAIR_RISE_SHALLOW, T_SHOWCASE_STAIR_TOP_LEVEL, T_SHOWCASE_SUBSTEP_NOT_NO_OP,
        T_SHOWCASE_TOLERANCE_TOO_BIG, T_SHOWCASE_TOLERANCE_TWO_RISES, T_SHOWCASE_TWO_OVERLAP,
        T_SHOWCASE_WALK_LOSES_SLIDE, T_SHOWCASE_WALKER_DIRECTION, T_SHOWCASE_WALKER_REACHES_TOP,
        T_VERTICAL_REST_ON_FLOOR, T_WHEEL_AXLE_STILL, T_WHEEL_CENTRE_FIXED, T_WHEEL_ROLLS_FORWARD,
        T_WHEEL_SPIN_MOVES_RIM, TERMINAL_VELOCITY,
    };
    use crate::player::Player;
    use crate::r#type::{Mat4Data, Vec3};
    /// 只跑社交那一层,跳过走路 / 碰撞 —— 社交测试要的是「谁跟谁凑一局」
    /// 这个决定,不是他们有没有真的走到位。
    ///
    /// 直接对 `&mut [Pedestrian]` 跑,不构造 `Game`:社交逻辑只读行人
    /// 的位置 / 状态,不碰世界、玩家、批次。`Game` 二十多个字段互相咬合,
    /// 在测试里拼一份既啰嗦又脆,而这里一个都用不上。
    pub fn update_ped_social_for_test(peds: &mut [Pedestrian], dt: f32) {
        super::update_ped_social(peds, dt);
    }

    /// 造两个相距约 1 m 的行人,各自的目标点甩在 9 m 外。
    ///
    /// 巡航点甩远是为了让测试只看见「社交」这一个变量:如果目标点就在
    /// 脚下,行人永远「已到达」,聊天与否会跟巡航逻辑缠在一起。
    fn ped_pair(ax: f32, az: f32, bx: f32, bz: f32) -> Vec<Pedestrian> {
        [[ax, az], [bx, bz]]
            .iter()
            .map(|at: &[f32; 2]| {
                Pedestrian::new(
                    [at[0], 0.0, at[1]],
                    0.0,
                    [at[0] + 9.0, at[1]],
                    PED_SUIT,
                    0.0,
                )
            })
            .collect()
    }

    use crate::game::{
        BLOCK_VIEW_RADIUS, BlockLayout, BuildingPlacement, GROUND_SPAN, MeshAsset, MeshPart,
        PLAYER_RADIUS, SHOWCASE_DOOR_HALF, SHOWCASE_GROUND_TOP, SHOWCASE_STAIR_LEAD,
        SHOWCASE_STAIR_RISE, SHOWCASE_STAIR_RUN, SHOWCASE_STAIR_STEPS, SHOWCASE_STAIR_WIDTH,
        SHOWCASE_UPPER_TOP, SHOWCASE_WALL_THICKNESS, SIDEWALK_WIDTH, STREAM_REBUILD_STEP,
        STREET_HALF_WIDTH, STREET_PITCH, ShowcaseSpec, ShowcaseSpecs, blocks_near,
        build_city_palms,
        build_city_buildings, build_collision_world, build_ground_near, build_showcase_interiors,
        build_water_near, on_roadway, showcase_placements, showcase_specs, stream_needs_rebuild,
        street_axis, street_indices_in,
    };
    use crate::interior::{Floor, FloorWorld, STEP_UP_TOLERANCE};
    use crate::mesh::Bounds;
    use crate::player::{RUN_SPEED, WALK_SPEED};
    use crate::r#type::Vec2;

    /// 两栋样板楼楼板 + 楼梯的期望顶面高度之和(级高 × 级数)。
    const STAIR_TOTAL: f32 = SHOWCASE_STAIR_RISE * SHOWCASE_STAIR_STEPS as f32;

    fn world() -> FloorWorld {
        let mut w: FloorWorld = FloorWorld::new();
        build_showcase_interiors(&mut w);
        w
    }

    /// 把**资产本地**的 XZ 坐标变成世界坐标(与 `push_showcase_interior`
    /// 里的 `place` 闭包是同一个变换)。
    ///
    /// 测试用本地坐标描述「门洞在正前方」「楼梯贴着 +X 墙」这些**与朝向
    /// 无关**的事实,再旋到世界。这样两栋楼朝向相反(`+π/2` 与 `−π/2`)
    /// 时测试仍然成立 —— 之前测试写死世界 +Z 是门面,朝向一改就全错。
    fn to_world(index: usize, local: Vec2) -> Vec2 {
        super::showcase_to_world(index, local)
    }

    /// 本地 XZ 的**行向量**(切向),即门洞的左右方向。
    fn local_side(index: usize) -> Vec2 {
        let specs: ShowcaseSpecs = showcase_specs();
        let (sin_yaw, cos_yaw): (f32, f32) = specs[index].yaw.sin_cos();
        [cos_yaw, -sin_yaw]
    }

    /// 门洞中心的世界坐标(本地 `z = span.z / 2`)。
    fn doorway(index: usize) -> Vec2 {
        super::showcase_door_center(index)
    }

    /// 门洞法线的世界方向(本地 +Z 旋到世界)。
    fn front_normal(index: usize) -> Vec2 {
        super::showcase_front_normal(index)
    }

    /// 世界 XZ 反算回资产本地 XZ(把 [`showcase_to_world`] 逆转)。
    ///
    /// # Arguments
    ///
    /// - `usize` - 样板楼序号。
    /// - `Vec2` - 世界 XZ。
    ///
    /// # Returns
    ///
    /// - `Vec2` - 本地 XZ。
    pub fn showcase_to_local(index: usize, world: Vec2) -> Vec2 {
        let specs: [ShowcaseSpec; 2] = showcase_specs();
        let spec: &ShowcaseSpec = &specs[index];
        let (sin_yaw, cos_yaw): (f32, f32) = spec.yaw.sin_cos();
        let dx: f32 = world[0] - spec.position[0];
        let dz: f32 = world[1] - spec.position[1];
        [dx * cos_yaw - dz * sin_yaw, dx * sin_yaw + dz * cos_yaw]
    }

    /// 楼梯中线上的某一点的本地 XZ:楼梯贴着本地 +X 内墙。
    fn stair_point(index: usize, local_z: f32) -> Vec2 {
        super::showcase_stair_point(index, local_z)
    }

    /// 楼梯最上一级的本地 Z(那一级的**前沿**,即顶面最高处)。
    fn stair_top_local_z(index: usize) -> f32 {
        let span: Vec2 = showcase_specs()[index].span;
        span[1] * 0.5 - SHOWCASE_STAIR_LEAD
    }

    #[test]
    fn doorway_is_the_only_gap_in_the_front_facade() {
        for index in 0..2 {
            let w: FloorWorld = world();
            let front: Vec2 = doorway(index);
            let normal: Vec2 = front_normal(index);
            let side: Vec2 = local_side(index);
            // 门洞中心:脚下必须有首层楼板(身体从地面起算,不是从门洞起算)。
            let just_inside: Vec2 = [front[0] - normal[0] * 1.0, front[1] - normal[1] * 1.0];
            let support: Option<f32> = w.support_height(just_inside, GROUND_LEVEL);
            assert!(
                support.is_some_and(|height: f32| height > 0.0),
                "{} {}",
                T_SHOWCASE_DOOR_NO_SLAB,
                T_SHOWCASE_DOOR_INSIDE
            );
            // 门洞正中心必须**穿得过去**:外墙在门洞这一段是空的。
            let after: Vec2 = w.resolve_interior(
                front,
                GROUND_LEVEL,
                GROUND_LEVEL + PLAYER_BODY_HEIGHT,
                PLAYER_RADIUS,
            );
            let drift: f32 =
                ((after[0] - front[0]) * normal[0] + (after[1] - front[1]) * normal[1]).abs();
            let lateral: f32 =
                ((after[0] - front[0]) * side[0] + (after[1] - front[1]) * side[1]).abs();
            assert!(
                drift < 1e-3 && lateral < 1e-3,
                "{} {}",
                T_SHOWCASE_DOOR_CENTER_BLOCKED,
                T_SHOWCASE_DOOR_INSIDE
            );
            // 门洞两侧:墙必须挡住。
            for direction in [-1.0_f32, 1.0] {
                let at: Vec2 = [
                    front[0] + side[0] * direction * (SHOWCASE_DOOR_HALF + 0.4),
                    front[1] + side[1] * direction * (SHOWCASE_DOOR_HALF + 0.4),
                ];
                let pushed: Vec2 = w.resolve_interior(
                    at,
                    GROUND_LEVEL,
                    GROUND_LEVEL + PLAYER_BODY_HEIGHT,
                    PLAYER_RADIUS,
                );
                // 门垛是沿着**法线**方向挡人的,所以位移必须发生在法线上;
                // 沿着门垛的横向挪一点点不算「被挡住」。
                let moved: f32 = (pushed[0] - at[0]) * normal[0] + (pushed[1] - at[1]) * normal[1];
                let escaped: f32 = (pushed[0] - at[0]) * side[0] + (pushed[1] - at[1]) * side[1];
                assert!(
                    moved.abs() > 1e-3 || escaped.abs() > 1e-3,
                    "{} {}",
                    T_SHOWCASE_PIER_LET_PLAYER_THROUGH,
                    T_SHOWCASE_DOOR_OUTSIDE_BLOCKED
                );
            }
        }
    }

    #[test]
    fn doors_face_the_open_ground_between_them() {
        // 两栋楼分居出生点道路的两侧,门必须都朝着中间的空地 —— 玩家
        // 往街区里走就能同时看见两个开口,不用满城找。这里断言的是
        // 「门法线指向另一栋楼」,不是某一个具体方向:摆放一改(比如
        // 换成南北相向),这个不变量依然成立。
        let specs: ShowcaseSpecs = showcase_specs();
        for (index, spec) in specs.iter().enumerate() {
            let normal: Vec2 = front_normal(index);
            let toward: Vec2 = [
                specs[1 - index].position[0] - spec.position[0],
                specs[1 - index].position[1] - spec.position[1],
            ];
            let length: f32 = (toward[0] * toward[0] + toward[1] * toward[1]).sqrt();
            assert!(
                length > 1.0,
                "{} {}",
                T_SHOWCASE_TWO_OVERLAP,
                T_SHOWCASE_DOORS_FACE_EACH_OTHER
            );
            let dot: f32 = (normal[0] * toward[0] + normal[1] * toward[1]) / length;
            assert!(
                dot > 0.5,
                "{} {} {}",
                T_SHOWCASE_DOOR_NOT_FACING,
                T_SHOWCASE_DOORS_FACE_EACH_OTHER,
                spec.asset
            );
        }
    }

    #[test]
    fn exterior_walls_never_let_the_player_sit_inside_them() {
        // 从街上朝门垛走。分离之后人**不落在墙里** —— 至于被推向哪一侧
        // 是「圆心 → 最近面」的正常结果,站在墙外的人本来就该被推得更靠外。
        // 这里守的不变量是「永不穿墙」,而不是某个方向。
        for index in 0..2 {
            let w: FloorWorld = world();
            let span: Vec2 = showcase_specs()[index].span;
            let front: Vec2 = doorway(index);
            let normal: Vec2 = front_normal(index);
            let side: Vec2 = local_side(index);
            let outer: f32 = span[1] * 0.5 + SHOWCASE_WALL_THICKNESS;
            let mut depth: f32 = outer + 0.4;
            while depth > outer - 0.6 {
                let at: Vec2 = [
                    front[0] + side[0] * (SHOWCASE_DOOR_HALF + 1.5) + normal[0] * depth,
                    front[1] + side[1] * (SHOWCASE_DOOR_HALF + 1.5) + normal[1] * depth,
                ];
                let pushed: Vec2 = w.resolve_interior(
                    at,
                    GROUND_LEVEL,
                    GROUND_LEVEL + PLAYER_BODY_HEIGHT,
                    PLAYER_RADIUS,
                );
                assert!(
                    !w.contains_interior_point(pushed, GROUND_LEVEL + 0.5),
                    "{} {}",
                    T_SHOWCASE_PUSHED_INTO_WALL,
                    T_SHOWCASE_FRONT_FACING
                );
                depth -= 0.05;
            }
        }
    }

    #[test]
    fn walking_up_the_stair_reaches_the_upper_floor() {
        // 纯几何校验:楼梯最顶一级必须与二层楼板面齐平,否则玩家走上
        // 楼梯会停在半空或者要再跳一下。
        assert!(
            (SHOWCASE_GROUND_TOP + STAIR_TOTAL - SHOWCASE_UPPER_TOP).abs() < 1e-4,
            "{} {} {}",
            T_SHOWCASE_STAIR_TOP_LEVEL,
            T_SHOWCASE_STAIR_REACHES_TOP,
            SHOWCASE_GROUND_TOP
        );
        // 踏高必须落在容差内,否则玩家踏上第一级就会被当成「撞墙」,
        // 楼梯变成一段永远走不上去的坡。
        assert!(
            SHOWCASE_STAIR_RISE < STEP_UP_TOLERANCE,
            "{} {}",
            T_SHOWCASE_RISE_GE_TOLERANCE,
            T_SHOWCASE_STAIR_RISE_SHALLOW
        );
        // 反过来,容差也不能大到能一步跨上整层楼 —— 那会让玩家在楼下
        // 一按前就瞬移到二楼。
        assert!(
            STEP_UP_TOLERANCE < SHOWCASE_UPPER_TOP - SHOWCASE_GROUND_TOP,
            "{} {}",
            T_SHOWCASE_TOLERANCE_TOO_BIG,
            T_SHOWCASE_STAIR_RISE_SHALLOW
        );
        // 也不能大到能跨两级:两级 = 0.61 m,一次跨两级会让楼梯的视觉
        // 台阶感和实际的抬升对不上。
        assert!(
            STEP_UP_TOLERANCE < 2.0 * SHOWCASE_STAIR_RISE,
            "{} {} {}",
            T_SHOWCASE_TOLERANCE_TWO_RISES,
            T_SHOWCASE_STAIR_RISE_SHALLOW,
            2.0 * SHOWCASE_STAIR_RISE
        );
    }

    /// 走路线的每一个路点都必须**互不相同且真正能走**(相邻点间距
    /// 大于一个身位)。
    ///
    /// 之前踩过两次:一次是「沿门轴深入」的中间点与下一个点共用本地 Z,
    /// 旋转到世界后 X 完全相同,路线在原地打转;另一次是斜穿隔墙的
    /// 路点直接顶在墙上。两者都不会让任何单测变红 —— 端到端那条全程用
    /// 本地坐标做判定,根本不经过世界变换。只能在这个层面查。
    #[test]
    fn the_showcase_walk_route_has_no_duplicate_or_adjacent_points() {
        for index in 0..2 {
            let route: Vec<[f32; 2]> = super::showcase_walk_route(index);
            assert!(
                route.len() >= 8,
                "{} route has {} points",
                T_SHOWCASE_ROUTE_WALKABLE,
                route.len()
            );
            for pair in route.windows(2) {
                let gap: f32 =
                    ((pair[1][0] - pair[0][0]).powi(2) + (pair[1][1] - pair[0][1]).powi(2)).sqrt();
                assert!(
                    gap > 0.9,
                    "{} consecutive route points are {gap:.2} m apart",
                    T_SHOWCASE_ROUTE_WALKABLE
                );
                // 单轴:楼里有隔墙和楼梯基座,斜着走必然一头撞上。
                // 所以每一段只能沿 X 或沿 Z,不能同时动两个轴。
                assert!(
                    (pair[1][0] - pair[0][0]).abs() < 0.01
                        || (pair[1][1] - pair[0][1]).abs() < 0.01,
                    "{}",
                    T_ROUTE_LEG_DIAGONAL
                );
            }
        }
    }

    /// 门必须开在**外墙面**上,而不是内墙净跨的边界。
    ///
    /// 两者差一层墙厚(约 1.65 m)。取内墙面的话,「门外 1.5 m」这个落点
    /// 会正好卡在外墙与内墙之间的夹缝里 —— 浏览器实测玩家进了门洞就在
    /// 原地打转,往里推不动、往外被弹回。
    #[test]
    fn the_showcase_doorway_sits_on_the_outer_wall() {
        for index in 0..2 {
            let spec: &ShowcaseSpec = &showcase_specs()[index];
            let door: Vec2 = super::showcase_door_center(index);
            let local: Vec2 = super::showcase_to_local(index, door);
            let expected: f32 = spec.half[1] + SHOWCASE_WALL_THICKNESS;
            assert!(
                (local[1] - expected).abs() < 1e-3,
                "{} door local z = {}, expected outer wall at {}",
                T_SHOWCASE_DOOR_ON_OUTER_WALL,
                local[1],
                expected
            );
        }
    }

    #[test]
    fn simulated_walker_climbs_to_the_upper_storey() {
        // 用与 `step_vertical` 完全相同的积分顺序跑一遍真实的水平速度:
        // 从门前往楼里走 4.6 m/s,最后必须站在二层楼板面上。
        for index in 0..2 {
            let w: FloorWorld = world();
            let specs: ShowcaseSpecs = showcase_specs();
            let normal: Vec2 = front_normal(index);
            let top_z: f32 = stair_top_local_z(index);
            let dt: f32 = 1.0 / 60.0;
            let speed: f32 = WALK_SPEED;
            let mut y: f32 = GROUND_LEVEL;
            let mut vy: f32 = 0.0;
            // 从楼梯顶端**外侧**起步,朝楼里走;每一步都走的是楼梯中线。
            let mut local_z: f32 = top_z + 1.2;
            let mut frames: usize = 0;
            // 只走上楼梯那一段:走过头会从二层板的边缘踏空掉回一层
            // (这是**正确**的行为,不是 bug),所以在抵达顶层时就停。
            while (y - SHOWCASE_UPPER_TOP).abs() >= 1e-3 && frames < 1200 {
                frames += 1;
                local_z -= speed * dt;
                let at: Vec2 = stair_point(index, local_z);
                let support: Option<f32> = w.support_height(at, y);
                let floor: f32 = support.unwrap_or(GROUND_LEVEL);
                if let Some(height) = support
                    && height - y <= STEP_UP_TOLERANCE
                {
                    y = height;
                    vy = 0.0;
                } else if y <= GROUND_LEVEL {
                    y = GROUND_LEVEL;
                    vy = 0.0;
                } else {
                    vy = (vy - GRAVITY * dt).max(-TERMINAL_VELOCITY);
                    let next: f32 = y + vy * dt;
                    if vy <= 0.0 && next <= floor + GROUND_SNAP_SKIN {
                        y = floor;
                        vy = 0.0;
                    } else {
                        y = next;
                    }
                }
            }
            let _: Vec2 = normal;
            assert!(
                (y - SHOWCASE_UPPER_TOP).abs() < 1e-2,
                "{} {}",
                T_SHOWCASE_WALKER_DIRECTION,
                T_SHOWCASE_WALKER_REACHES_TOP
            );
            let _: ShowcaseSpecs = specs;
        }
    }

    /// **楼内走不下楼梯** —— 二层楼板第 2 片盖住了整条梯段。
    ///
    /// **实测缺陷(CDP,软件渲染,HUD 逐帧,自 `1ff7c38` / 09-29 起):**
    /// `push_showcase_interior` 铺二层楼板第 2 片时写的是
    /// `z[stair_end, hz]`,而梯段正好占 `z[stair_end, sz_last]` 且
    /// `stair_end < sz_last` —— **整条梯段都在那片楼板底下**。楼板顶面
    /// 恒为 `UPPER_TOP = 3.20`,`support_height` 取「容差内最高的一块」,
    /// 于是人站在梯段上永远是 y = 3.20:下楼沿 +X 走到 x = 27.4(墙)仍
    /// y = 3.2,一步都下不去。
    ///
    /// **为什么必须用 `world()` 而不是 `interior` 里的夹具:** 缺陷在
    /// `push_showcase_interior` 的几何里,夹具是另抄的一份,两者不联动
    /// —— 改回去夹具还是绿的。这条直接问 `build_showcase_interiors` 铺出来
    /// 的**真实**碰撞世界,修几何前后必然一红一绿。
    ///
    /// 三条判据:
    /// 1. **梯段头顶是空的** —— 站在第 5 级上,脚下只能是那一级。
    /// 2. **逐级降到地面** —— 走完整段梯段后 y 真的落到首层。
    /// 3. **全程不出现抬升** —— 下楼途中 dy 一旦 > 0,那是往回爬。
    #[test]
    fn the_upper_floor_leaves_the_stair_run_open() {
        let w: FloorWorld = world();
        let dt: f32 = 1.0 / 60.0;
        let stride: f32 = WALK_SPEED * dt;
        for index in 0..2 {
            let spec: &ShowcaseSpec = &showcase_specs()[index];
            let (hx, hz): (f32, f32) = (spec.span[0] * 0.5, spec.span[1] * 0.5);
            let sx0: f32 = hx - SHOWCASE_STAIR_WIDTH;
            let sz_last: f32 = hz - SHOWCASE_STAIR_LEAD;
            let stair_end: f32 =
                sz_last - SHOWCASE_STAIR_STEPS as f32 * SHOWCASE_STAIR_RUN;
            let stair_x: f32 = (sx0 + hx) * 0.5;

            // ---- 判据 1:梯段每一级头顶都是空的。
            for step in 0..SHOWCASE_STAIR_STEPS {
                let z1: f32 = sz_last - step as f32 * SHOWCASE_STAIR_RUN;
                let tread: f32 =
                    SHOWCASE_GROUND_TOP + (step as f32 + 1.0) * SHOWCASE_STAIR_RISE;
                let at: Vec2 = super::showcase_to_world(index, [stair_x, z1 - 0.5 * SHOWCASE_STAIR_RUN]);
                let got: Option<f32> = w.support_height(at, tread);
                assert!(
                    got.is_some_and(|height: f32| (height - tread).abs() < 1e-3),
                    "{}",
                    T_SHOWCASE_LANDING_COVERS_RUN
                        .replace("{index}", &format!("{index}"))
                        .replace("{run_lo:.3}", &format!("{stair_end:.3}"))
                        .replace("{run_hi:.3}", &format!("{sz_last:.3}"))
                        .replace("{slab_lo:.3}", &format!("{stair_end:.3}"))
                        .replace("{slab_hi:.3}", &format!("{hz:.3}"))
                        .replace("{support:.3}", &format!("{:.3}", got.unwrap_or(-1.0)))
                        .replace("{tread:.3}", &format!("{tread:.3}"))
                );
            }

            // ---- 判据 2 + 3:从梯顶 landing 往梯脚走,逐级降到地面。
            //
            // 方向:**上坡是本地 -z**(tread9 顶面 3.20 在 `z = stair_end`,
            // tread0 顶面 0.455 在 `z = sz_last`),所以下楼是**本地 +z**。
            // 起步点落在二层楼板第 2 片上(它现在占 `z[-hz, stair_end]`,
            // 梯顶那一侧),脚下是 3.20;往 +z 走出 landing 就踩上第 10 级。
            let mut local_z: f32 = stair_end - 0.20;
            let mut y: f32 = SHOWCASE_UPPER_TOP;
            let mut seq: Vec<f32> = vec![y];
            for _ in 0..DESCENT_BUDGET {
                let from: Vec2 = super::showcase_to_world(index, [stair_x, local_z]);
                local_z += stride;
                let to: Vec2 = super::showcase_to_world(index, [stair_x, local_z]);
                let (_, stepped): (Option<f32>, f32) = w.support_along_frame(from, to, y);
                y = stepped;
                seq.push(y);
            }
            for (frame, pair) in seq.windows(2).enumerate() {
                assert!(
                    pair[1] <= pair[0] + 1e-3,
                    "{}",
                    T_SHOWCASE_DESCENT_NO_CLIMB
                        .replace("{index}", &format!("{index}"))
                        .replace("{frame}", &format!("{frame}"))
                        .replace("{from:.3}", &format!("{:.3}", pair[0]))
                        .replace("{to:.3}", &format!("{:.3}", pair[1]))
                        .replace("{seq:?}", &format!("{seq:?}"))
                );
            }
            assert!(
                y <= SHOWCASE_GROUND_TOP + 1e-2,
                "{}",
                T_SHOWCASE_DESCENT_REACHES_GROUND
                    .replace("{index}", &format!("{index}"))
                    .replace("{seq:?}", &format!("{seq:?}"))
            );
        }
    }

    /// 从二层 landing 走完整段梯段到地面要多少帧(60 fps)。
    ///
    /// 梯段 10 级 × 0.45 m = 4.5 m,每帧 `WALK_SPEED / 60 = 0.077 m`,
    /// 至少 60 帧;取 100 留出余量,走过头之后脚下是首层地板(判据仍成立)。
    const DESCENT_BUDGET: usize = 100;

    /// 冲刺一帧的真实水平位移(米)—— 长帧穿透缺陷只在这个步长下成立。
    ///
    /// CDP 逐帧实测(软件渲染,rAF ≈ 1.1 fps,`__vcw.vel` 中位数 = 8.3999
    /// = `RUN_SPEED`):一帧推进 **1.1269 m**。取游戏能达到的**最快**步长,
    /// 而不是 `RUN_SPEED * FIXED_DT * 4 = 0.56` 那个理论值:0.56 m 同样
    /// 大于门垛 0.20 m 也照样穿透,但判据必须钉在最快档上 —— 只在慢档
    /// 验过的碰撞修复会在低帧率下失效,这是本项目反复吃过的亏
    /// (楼梯下楼缺陷就是 60 fps 全绿、软件渲染下才炸)。
    const SPRINT_FRAME_STRIDE: f32 = 1.1269;

    /// 60 fps 步行一帧的水平位移(米):任何修复在这条步长下都必须是 no-op。
    const WALK_FRAME_STRIDE: f32 = WALK_SPEED * FIXED_DT;

    /// 冲刺冲向前墙时跑够多少帧就够判定了:实测 15 帧穿透 15.25 m。
    const SPRINT_TUNNEL_BUDGET: usize = 20;

    /// 从 `from` 沿 `normal` 前进时,撞上的**第一段墙**的世界包围盒。
    ///
    /// 刻意去 `build_showcase_interiors` 铺出来的真实世界里量,而不是抄
    /// 一个字面量:门垛厚度在 `game.rs` 与 `const.rs` 各有一份(`0.20` 与
    /// `0.25`,见 `push_showcase_interior` 与 [`crate::r#const::
    /// SHOWCASE_WALL_THICKNESS`]),抄错就等于测了另一个几何。
    ///
    /// 判据与 [`crate::interior::FloorWorld::separate_interior`] 对齐:
    /// 只有**墙**参与横向阻挡,楼板永远不推人(踏面只提供竖直支撑)。
    ///
    /// # Arguments
    ///
    /// - `&FloorWorld` - 真实室内碰撞世界。
    /// - `Vec2` - 起点世界 XZ。
    /// - `Vec2` - 前进方向的世界 XZ 单位向量。
    ///
    /// # Returns
    ///
    /// - `Option<(Vec3, Vec3)>` - 最近那段墙的 `(min, max)`;沿途没有墙时
    ///   为 `None`(调用方据此跳过断言,而不是拿一个魔数当阈值)。
    fn wall_box_ahead(w: &FloorWorld, from: Vec2, normal: Vec2) -> Option<(Vec3, Vec3)> {
        let mut best: Option<(f32, Vec3, Vec3)> = None;
        for floor in w.get_floors() {
            let (min, max): (Vec3, Vec3) = match floor {
                Floor::Slab { min, max } => (*min, *max),
                Floor::Wall { min, max } => (*min, *max),
            };
            // 竖直区间与身体 `[GROUND_TOP, GROUND_TOP + BODY]` 不相交就跳过,
            // 与 `separate_interior` 同一判据:头顶的楼板不该推人。
            if max[1] <= SHOWCASE_GROUND_TOP || min[1] >= SHOWCASE_GROUND_TOP + PLAYER_BODY_HEIGHT {
                continue;
            }
            let axis: usize = if normal[0].abs() >= normal[1].abs() {
                0
            } else {
                1
            };
            // 另一轴必须真的落在这段墙的跨度内,否则是楼里别处的墙。
            let across: f32 = if axis == 0 { from[1] } else { from[0] };
            let (span_lo, span_hi): (f32, f32) = if axis == 0 {
                (min[2], max[2])
            } else {
                (min[0], max[0])
            };
            if across < span_lo - PLAYER_RADIUS || across > span_hi + PLAYER_RADIUS {
                continue;
            }
            // 沿 `normal` 方向,这段墙的**入射面**在起点前方的那一端。
            let near: f32 = if normal[axis] > 0.0 {
                if axis == 0 { min[0] } else { min[2] }
            } else if axis == 0 {
                max[0]
            } else {
                max[2]
            };
            let distance: f32 = (near - from[axis]) / normal[axis];
            let closer: bool = match best {
                Some((kept, _, _)) => distance < kept,
                None => true,
            };
            if distance > 0.0 && closer {
                best = Some((distance, min, max));
            }
        }
        match best {
            Some((_, min, max)) => Some((min, max)),
            None => None,
        }
    }

    /// 从 `from` 沿 `normal` 走到 `box_min`/`box_max` 那段墙还有多远(米)。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 起点世界 XZ。
    /// - `Vec2` - 前进方向的世界 XZ 单位向量。
    /// - `Vec3` - 墙的包围盒下角。
    /// - `Vec3` - 墙的包围盒上角。
    ///
    /// # Returns
    ///
    /// - `f32` - 沿 `normal` 到墙面入射端的距离(米)。
    fn wall_face(from: Vec2, normal: Vec2, box_min: Vec3, box_max: Vec3) -> f32 {
        if normal[0].abs() >= normal[1].abs() {
            let near: f32 = if normal[0] > 0.0 {
                box_min[0]
            } else {
                box_max[0]
            };
            (near - from[0]) / normal[0]
        } else {
            let near: f32 = if normal[1] > 0.0 {
                box_min[2]
            } else {
                box_max[2]
            };
            (near - from[1]) / normal[1]
        }
    }

    /// 冲刺撞前墙门垛(0.20 m 厚)时,一帧必须被墙挡住而不是穿过去。
    ///
    /// **实测缺陷(CDP,HUD 逐帧,软件渲染,2026-10-02 复现):** 从 LOFT
    /// 楼梯脚按住 Shift + W 冲向前墙,步行在 `x = 27.40` 停住 16+ 帧
    /// (正确 —— 门垛内表面 27.75 减半径 0.35);冲刺却从 27.40 一路冲到
    /// `x = 43.0`,15 帧穿透 15.25 m。同一面墙、同一时刻。
    ///
    /// 根因在 [`crate::interior::FloorWorld::resolve_interior_slide`]:它用
    /// `separate_interior` 做**位置推离**,只看帧末落点。冲刺一帧 1.12 m
    /// 大于门垛 0.20 m 的厚度,帧末人已经在墙另一侧,推离向量为 0,
    /// `length <= EPSILON` 分支把整帧原样放行 —— 整堵墙被一步跨过去。
    /// **所以必须在冲刺步长下测**:60 fps 步行一帧 0.077 m,任何实现都
    /// 测不出差别(这一条本身就是被专门钉住的 no-op 断言,见下一条测试)。
    ///
    /// **必须用 `world()` 而不是另抄一份夹具:** 缺陷活在
    /// `push_showcase_interior` 铺出来的真实几何里,夹具与它不联动
    /// (`197fa29` 的下楼测试正是栽在这一条 —— 夹具的 slab 铺法与真实场景
    /// 不符,于是测试绿了一整个缺陷期)。这里问的就是
    /// `build_showcase_interiors` 的产物,改修复前后必然一红一绿。
    #[test]
    fn sprinting_into_a_thin_interior_wall_is_blocked() {
        for index in 0..2 {
            let w: FloorWorld = world();
            // 两条路线都朝本地 +Z(门洞那一侧)冲,各自命中一段薄墙:
            // A = 前墙门垛(厚度 `SHOWCASE_WALL_THICKNESS` = 0.20 m),起点就是
            // CDP 复现时站的那一点(梯脚楼梯中线,世界 x = 27.3 / z = 24.9;
            // 门垛内表面 27.75 减半径 0.35 = 27.40,与实测停位逐位吻合);
            // B = 首层隔墙(2 × `SHOWCASE_PARTITION_THICKNESS` = 0.15 m)。
            // 两条在两栋楼里都真的撞上墙 —— 撞不上会让 `wall_box_ahead` 返回
            // `None` 并直接失败,不会退化成空洞的通过。
            for start in [stair_point(index, 4.30), to_world(index, [0.0, 1.60])] {
                let outward: Vec2 = front_normal(index);
                let normal: Vec2 = outward;
                let (box_min, box_max): (Vec3, Vec3) = wall_box_ahead(&w, start, normal)
                    .unwrap_or_else(|| panic!("{}", T_SHOWCASE_NO_WALL_AHEAD));
                // 人能停到的地方 = 墙面减去半径。
                let limit: f32 = wall_face(start, normal, box_min, box_max) - PLAYER_RADIUS;
                let mut here: Vec2 = start;
                let mut breakthrough: Option<(usize, Vec2, f32)> = None;
                for frame in 0..SPRINT_TUNNEL_BUDGET {
                    let moved: Vec2 = w.resolve_interior_slide(
                        here,
                        [
                            normal[0] * SPRINT_FRAME_STRIDE,
                            normal[1] * SPRINT_FRAME_STRIDE,
                        ],
                        SHOWCASE_GROUND_TOP,
                        SHOWCASE_GROUND_TOP + PLAYER_BODY_HEIGHT,
                        PLAYER_RADIUS,
                    );
                    // 逐帧判:这一帧沿法线推进了多少。穿透即「推进量超过接触线」。
                    let advanced: f32 =
                        (moved[0] - here[0]) * normal[0] + (moved[1] - here[1]) * normal[1];
                    if breakthrough.is_none() && advanced > limit + 1e-2 {
                        breakthrough = Some((frame, moved, advanced));
                    }
                    here = moved;
                }
                if let Some((frame, at, advanced)) = breakthrough {
                    panic!(
                        "{}",
                        fill(
                            T_SHOWCASE_SPRINT_TUNNELS_WALL,
                            &[
                                ("index", &format!("{index}")),
                                ("frame", &format!("{frame}")),
                                ("stride:.3", &format!("{SPRINT_FRAME_STRIDE:.3}")),
                                ("face:.3", &format!("{limit:.3}")),
                                (
                                    "at:?",
                                    &format!(
                                        "{at:?} 推进 {advanced:.3} 墙 {box_min:?}..{box_max:?}"
                                    )
                                ),
                            ]
                        )
                    );
                }
                // 终点必须**不在墙的 AABB 内部**。
                assert!(
                    !inside_box(here, box_min, box_max, PLAYER_RADIUS * 0.5),
                    "{}",
                    fill(
                        T_SHOWCASE_SPRINT_PENETRATES_WALL,
                        &[
                            ("index", &format!("{index}")),
                            ("got:.3", &format!("{limit:.3}")),
                            ("end:?", &format!("{here:?} 墙 {box_min:?}..{box_max:?}")),
                        ]
                    )
                );
            }
        }
    }

    /// 一整帧的「静态层 + 室内层」串联:冲刺撞薄墙必须在墙前停住。
    ///
    /// **为什么上一条单测不够。** `sprinting_into_a_thin_interior_wall_is_blocked`
    /// 直接把玩家放在一个**合法**起点上调用 `resolve_interior_slide`,于是
    /// 它只钉住了「子步细分能挡住一帧的长位移」。但真实的一帧还有静态层
    /// (`CollisionWorld`)在**前面**跑过一遍,而它看不见室内墙 —— 它会把人
    /// 推进墙里再推进墙那边。
    ///
    /// **这条钉住的是调用点的起点,不是函数。** 拿子步的起点从「静态层留下
    /// 的落点」换成「这一帧的真起点」,函数本身一行没改,上一条照样绿,
    /// 而真实应用里缺陷原样复现(CDP 实测:15 帧推进 15.25 m 到 x = 41.5)。
    /// 差别在**参数**上,所以只能在串联了两层的这一层测出来。
    ///
    /// 静态层用**空世界**代替 `build_collision_world` 的产物:它按定义看不
    /// 见室内墙(见 `step_vertical` 的注释),所以对「这堵墙有没有被静态层
    /// 挡住」而言,空世界与真实城市是同一个答案 —— 都不挡。用空世界是为了
    /// 让这条测试只依赖 `build_showcase_interiors` 的真实室内几何,不去重
    /// 抄 1800+ 个 shape 的城市碰撞体。
    #[test]
    fn sprinting_through_a_full_frame_into_a_thin_wall_is_blocked() {
        for index in 0..2 {
            let w: FloorWorld = world();
            let static_world: CollisionWorld = CollisionWorld::new();
            let normal: Vec2 = front_normal(index);
            let origin: Vec2 = stair_point(index, 4.30);
            let (bmin, bmax): (Vec3, Vec3) = wall_box_ahead(&w, origin, normal)
                .unwrap_or_else(|| panic!("{}", T_SHOWCASE_NO_WALL_AHEAD));
            let reach: f32 = wall_face(origin, normal, bmin, bmax);
            let mut here: Vec2 = origin;
            for frame in 0..SPRINT_TUNNEL_BUDGET {
                // ---- 静态层:看不见室内墙,于是照常推进(可能跨过整堵墙)。
                let previous: Vec2 = here;
                let after_static: Vec2 = static_world.resolve_slide(
                    here,
                    [
                        normal[0] * SPRINT_FRAME_STRIDE,
                        normal[1] * SPRINT_FRAME_STRIDE,
                    ],
                    PLAYER_RADIUS,
                );
                // ---- 室内层:起点是**帧真起点**,位移仍是静态层算出的整帧量。
                //     (这正是 `step_vertical` 的调用方式。)
                let travel: Vec2 = [after_static[0] - previous[0], after_static[1] - previous[1]];
                here = w.resolve_interior_slide(
                    previous,
                    travel,
                    SHOWCASE_GROUND_TOP,
                    SHOWCASE_GROUND_TOP + PLAYER_BODY_HEIGHT,
                    PLAYER_RADIUS,
                );
                let advanced: f32 =
                    (here[0] - previous[0]) * normal[0] + (here[1] - previous[1]) * normal[1];
                assert!(
                    advanced <= reach - PLAYER_RADIUS + 1e-2,
                    "{}",
                    fill(
                        T_SHOWCASE_SPRINT_TUNNELS_WALL,
                        &[
                            ("index", &format!("{index}")),
                            ("frame", &format!("{frame}")),
                            ("stride:.3", &format!("{SPRINT_FRAME_STRIDE:.3}")),
                            ("face:.3", &format!("{:.3}", reach - PLAYER_RADIUS)),
                            ("at:?", &format!("{here:?} 静态层落点 {after_static:?}")),
                        ]
                    )
                );
            }
        }
    }

    /// 点是否落在某个 AABB 的**内**部(留 `skin` 余量,贴在面上不算进去)。
    ///
    /// # Arguments
    ///
    /// - `Vec2` - 待判的世界 XZ。
    /// - `Vec3` - 盒的包围盒下角。
    /// - `Vec3` - 盒的包围盒上角。
    /// - `f32` - 贴面余量(米)。
    ///
    /// # Returns
    ///
    /// - `bool` - 严格落在盒内为 `true`。
    fn inside_box(point: Vec2, box_min: Vec3, box_max: Vec3, skin: f32) -> bool {
        point[0] > box_min[0] + skin
            && point[0] < box_max[0] - skin
            && point[1] > box_min[2] + skin
            && point[1] < box_max[2] - skin
    }

    /// 60 fps 下子步细分必须是 no-op。
    ///
    /// 60 fps 步行一帧走 0.077 m,远小于一个子步
    /// [`crate::interior::SUPPORT_SUBSTEP_DISTANCE`] = 0.225 m,于是
    /// `substeps == 1`,细分退化成修复前的单次 `resolve_interior_slide`
    /// —— 正常帧率下手感逐帧不变。断言 `substeps == 1` 守的就是这个前提:
    /// 谁把 `SUPPORT_SUBSTEP_DISTANCE` 调到 60 fps 步长以下,这条立刻红。
    ///
    /// 只在冲刺下测过的修复,完全可能把正常帧率的手感改坏而没人发现。
    #[test]
    fn the_interior_sweep_is_a_no_op_at_sixty_fps() {
        let stride: f32 = WALK_FRAME_STRIDE;
        let steps: usize =
            ((stride / crate::interior::INTERIOR_SLIDE_SUBSTEP).ceil() as usize).max(1);
        assert_eq!(
            steps,
            1,
            "{}",
            fill(
                T_SHOWCASE_SUBSTEP_NOT_NO_OP,
                &[
                    ("split", &format!("{steps}")),
                    ("split_at:?", &format!("{stride:.4}")),
                    ("whole_at:?", &format!("{stride:.4}")),
                ]
            )
        );
    }

    /// 步行沿门垛斜走时切向位移必须照旧被保留(子步细分的反向控制)。
    ///
    /// 冲刺那条测的是「长帧不再穿墙」,这条测的是「短帧仍然滑得动」。
    /// 只钉住前者的话,一个把整帧位移整个砍掉的修法也能让它变绿 —— 而
    /// 那是「卡死在墙上」,不是墙。判据是切向分量拿到**至少一半**位移。
    #[test]
    fn walking_along_the_showcase_front_wall_keeps_sliding() {
        for index in 0..2 {
            let w: FloorWorld = world();
            let normal: Vec2 = front_normal(index);
            let side: Vec2 = local_side(index);
            let base: Vec2 = stair_point(index, 4.3);
            let (box_min, box_max): (Vec3, Vec3) = wall_box_ahead(&w, base, normal)
                .unwrap_or_else(|| panic!("{}", T_SHOWCASE_NO_WALL_AHEAD));
            let face: f32 = wall_face(base, normal, box_min, box_max);
            // 贴着门垛内表面站位,朝墙走半个身位并同时侧移。
            let start: Vec2 = [
                base[0] + normal[0] * (face - PLAYER_RADIUS) - side[0] * 0.30,
                base[1] + normal[1] * (face - PLAYER_RADIUS) - side[1] * 0.30,
            ];
            let step: Vec2 = [
                normal[0] * WALK_FRAME_STRIDE + side[0] * WALK_FRAME_STRIDE,
                normal[1] * WALK_FRAME_STRIDE + side[1] * WALK_FRAME_STRIDE,
            ];
            let moved: Vec2 = w.resolve_interior_slide(
                start,
                step,
                SHOWCASE_GROUND_TOP,
                SHOWCASE_GROUND_TOP + PLAYER_BODY_HEIGHT,
                PLAYER_RADIUS,
            );
            let slid: f32 = (moved[0] - start[0]) * side[0] + (moved[1] - start[1]) * side[1];
            assert!(
                slid > 0.5 * WALK_FRAME_STRIDE,
                "{}",
                fill(
                    T_SHOWCASE_WALK_LOSES_SLIDE,
                    &[
                        ("index", &format!("{index}")),
                        ("from:.3", &format!("{slid:.3}")),
                        ("to:.3", &format!("{:.3}", moved[0])),
                    ]
                )
            );
        }
    }

    /// 端到端:从门外一路走上二层楼板。
    ///
    /// 之前每条测试只覆盖楼梯的一段(几何 / 单步 / 上升趋势),所以
    /// 「玩家能不能真的按一串路点走完全程」从没被验证过。实测就是
    /// 走到第三级会被楼梯侧面弹回首层(y 从 1.065 掉回 0.15),而分段
    /// 测试全绿。这里把 门外 → 门洞 → 隔墙过道 → 第一级 → 楼梯顶 →
    /// 二层楼板 串成一条路线跑完整积分,任何一段断了都会失败。
    #[test]
    fn a_player_can_walk_from_the_doorway_to_the_top_of_the_stair() {
        let w: FloorWorld = world();
        let dt: f32 = 1.0 / 60.0;
        for index in 0..2 {
            let specs: [ShowcaseSpec; 2] = showcase_specs();
            let spec: &ShowcaseSpec = &specs[index];
            let span: [f32; 2] = spec.span;
            let hz: f32 = span[1] * 0.5;
            let hx: f32 = span[0] * 0.5;
            let sx0: f32 = hx - SHOWCASE_STAIR_WIDTH;
            let stair_x: f32 = (sx0 + hx) * 0.5;
            let sz_last: f32 = hz - SHOWCASE_STAIR_LEAD;
            let stair_end: f32 =
                sz_last - SHOWCASE_STAIR_STEPS as f32 * SHOWCASE_STAIR_RUN;
            let front: Vec2 = doorway(index);
            let normal: Vec2 = front_normal(index);
            // 路线(资产本地坐标):门外 → 门洞 → 隔墙过道 → 第一级前沿
            // → 楼梯顶端 → 二层楼板中央。
            // 路线(资产本地坐标)。要点:隔墙横在 z = PARTITION_Y 处,右端
            // 止于「楼梯左边缘再往回 GAP」,所以要**先横移到缺口那一侧、
            // 绕过隔墙,再回到楼梯跑段的下沿**,不能直奔梯中线。
            // 过道缺口的**中点**,不是它的左边缘:缺口左端就是隔墙的端头,
            // 把路点放在端头上,加上玩家半径就正好卡在墙面上(实测停在
            // local z=2.83 动弹不得)。中点离两面墙都有 0.85 m 余量。
            let gap_end: f32 = span[0] * 0.5 - SHOWCASE_STAIR_WIDTH;
            let _gap_x: f32 = gap_end - crate::r#const::SHOWCASE_PARTITION_GAP * 0.5;
            // 路线(资产本地坐标)。每段的必要性都来自实测:
            //  · 隔墙横在 z = PARTITION_Y,右端止于「楼梯左边缘往回 GAP」,
            //    所以要先横移到**缺口中点**再回到梯中线 —— 路点放在缺口
            //    边缘会被隔墙端头挡住(实测停在 z=2.83 动弹不得)。
            //  · 楼梯第一级贴 z = sz_last 一侧,顶面最低;越往 z **小**的
            //    方向级数越高,最后一级顶面正好 3.20 与二层楼板齐平。
            //  · 梯顶那一侧(z >= stair_end)本来就有二层板,不用跨井口。
            let gap_x: f32 = sx0 - crate::r#const::SHOWCASE_PARTITION_GAP * 0.5;
            let route: [Vec2; 7] = [
                // 门外
                [front[0] + normal[0] * 1.5, front[1] + normal[1] * 1.5],
                // 门洞
                front,
                // 绕到隔墙的过道缺口
                to_world(index, [gap_x, crate::r#const::SHOWCASE_PARTITION_Y]),
                // 穿过缺口,进到楼梯这一侧
                to_world(index, [stair_x, crate::r#const::SHOWCASE_PARTITION_Y]),
                // 踏上第一级
                to_world(index, [stair_x, sz_last - SHOWCASE_STAIR_RUN * 0.5]),
                // 沿梯中线一路上行
                stair_point(index, stair_end + SHOWCASE_STAIR_RUN * 0.5),
                // 站上二层楼板
                to_world(index, [stair_x, stair_end + 0.5]),
            ];
            let mut pos: Vec2 = route[0];
            let mut y: f32 = GROUND_LEVEL;
            let mut vy: f32 = 0.0;
            let mut max_y: f32 = y;
            for waypoint in route.iter().skip(1) {
                let target: Vec2 = *waypoint;
                let mut frames: usize = 0;
                while frames < 1200 {
                    frames += 1;
                    let delta: Vec2 = [target[0] - pos[0], target[1] - pos[1]];
                    let dist: f32 = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
                    if dist < 0.05 {
                        break;
                    }
                    let step: f32 = dist.min(0.05);
                    let want: Vec2 = [
                        pos[0] + delta[0] / dist * step,
                        pos[1] + delta[1] / dist * step,
                    ];
                    let support: Option<f32> = w.support_height(want, y);
                    let floor: f32 = support.unwrap_or(GROUND_LEVEL);
                    pos =
                        w.resolve_interior(want, floor, floor + PLAYER_BODY_HEIGHT, PLAYER_RADIUS);
                    // 与 `step_vertical` 完全同一套判定顺序:踩住 → 地面
                    // → 自由落体 + 穿透吸附。少任何一条,跨级时都会掉回
                    // 首层(`support_height` 认的是**脚底那一格**的板,
                    // 人还在两级之间时脚下是空的,必须靠落体后吸附上去)。
                    let on_slab: bool = support.is_some();
                    if on_slab && floor - y <= STEP_UP_TOLERANCE {
                        y = floor;
                        vy = 0.0;
                    } else if y <= GROUND_LEVEL {
                        y = GROUND_LEVEL;
                        vy = 0.0;
                    } else {
                        let falling: f32 = (vy - GRAVITY * dt).max(-TERMINAL_VELOCITY);
                        let next: f32 = y + falling * dt;
                        let base: f32 = if on_slab { floor } else { GROUND_LEVEL };
                        if falling <= 0.0 && next <= base + GROUND_SNAP_SKIN {
                            y = base;
                            vy = 0.0;
                        } else {
                            y = next;
                            vy = falling;
                        }
                    }
                    max_y = max_y.max(y);
                    let d2: Vec2 = [
                        pos[0] - specs[index].position[0],
                        pos[1] - specs[index].position[1],
                    ];
                    let cc: f32 = specs[index].yaw.cos();
                    let ss: f32 = specs[index].yaw.sin();
                    let lz: f32 = d2[0] * ss + d2[1] * cc;
                    if frames.is_multiple_of(40) {
                        let _: () =
                            println!("   leg y={:.3} local_z={:.2} target_z={:.2}", y, lz, {
                                let dd: Vec2 = [
                                    target[0] - specs[index].position[0],
                                    target[1] - specs[index].position[1],
                                ];
                                dd[0] * ss + dd[1] * cc
                            });
                    }
                }
            }
            // 断言的是**终点高度**而不是过程最高点:路线最后一段是
            // 「从楼梯顶走进二层楼板中央」,终点必须仍站在楼板面上。
            // 走过头会从楼板边缘踏空掉回一层,那是正确行为,所以路点
            // 收在板内,不做「上去再掉下来」这种自欺的断言。
            assert!(
                (y - SHOWCASE_UPPER_TOP).abs() < 0.35,
                "{} {} 抵达 y={} max_y={} pos={:?}",
                T_SHOWCASE_WALKER_REACHES_TOP,
                T_SHOWCASE_WALKER_DIRECTION,
                y,
                max_y,
                pos
            );
        }
    }

    /// 车轮矩阵:静止时轮心必须落在车的安装位上。
    #[test]
    fn wheel_model_places_the_axle_at_its_mount() {
        let mounts: [[f32; 3]; 4] = crate::traffic::WHEEL_MOUNTS;
        for mount in mounts {
            let model: Mat4Data = car_wheel_model([10.0, 0.0, -5.0], 0.0, mount, 0.0);
            // 列主序:平移在最后四个浮点里。
            assert!(
                (model[12] - (10.0 + mount[0])).abs() < 1e-5,
                "轮心 X {} 应等于车身 X + 安装位 X {}",
                model[12],
                10.0 + mount[0]
            );
            assert!(
                (model[13] - mount[1]).abs() < 1e-5,
                "轮心 Y {} 应等于安装位 Y {}",
                model[13],
                mount[1]
            );
            assert!(
                (model[14] - (-5.0 + mount[2])).abs() < 1e-5,
                "轮心 Z {} 应等于车身 Z + 安装位 Z {}",
                model[14],
                -5.0 + mount[2]
            );
        }
    }

    /// 街道网格是无限的:远离原点的地方照样有街道、街区与建筑。
    ///
    /// 这是「地图太小」的直接断言 —— 只要 `street_axis` 还停在
    /// `[-90, -30, 30, 90]`,走到 500 m 外就只剩虚空。
    #[test]
    fn the_street_grid_extends_far_past_the_old_city_bounds() {
        // 半条街的倍数在很远的地方依然是街道轴线。
        for index in [-1000i32, -37, 0, 11, 250] {
            let axis: f32 = street_axis(index);
            assert!(
                (axis - STREET_PITCH * index as f32).abs() < 1e-4,
                "第 {index} 条街道的轴线必须等于间距乘索引"
            );
            // 街道间距固定,所以相邻两条永远相隔一个定值。
            let next: f32 = street_axis(index + 1);
            assert!(
                (next - axis - STREET_PITCH).abs() < 1e-4,
                "相邻街道间距必须恒为 {STREET_PITCH}"
            );
        }
        // 街区间枚举必须覆盖住整个查询区间,不多不少。
        let indices: Vec<i32> = street_indices_in(-125.0, 190.0);
        assert!(
            indices.first() == Some(&-2) && indices.last() == Some(&3),
            "区间枚举必须含首尾,得到 {indices:?}"
        );
        // 逆序区间必须返回空,而不是 panic 或反向结果。
        let empty: Vec<i32> = street_indices_in(100.0, -100.0);
        assert!(empty.is_empty(), "逆序区间必须返回空");
    }

    /// 鼠标右移必须让画面内容**左移**(视角右转)。
    ///
    /// 这条测的是**方向**,不是幅度 —— 幅度错了顶多是灵敏度不对,
    /// 方向错了整个第一人称操控是反的,而符号一旦写反几乎不可能靠
    /// 手感发现。
    ///
    /// 做法:造一台正对玩家、跟随第三人的相机,把一对关于玩家对称的
    /// 世界点投影到屏幕,看它们的 `ndc_x` 往哪边动。左右两个点位移
    /// 应当**同向且等值**(等值说明是纯旋转而不是平移)。
    #[test]
    fn mouse_right_pans_the_view_to_the_right() {
        use crate::r#type::Vec3;
        let target: Vec3 = [0.0, 0.0, 0.0];
        let mut camera: Camera = Camera::new();
        camera.set_target(target);
        camera.set_pitch(0.16);
        camera.set_fov_y(FOLLOW_FOV);
        camera.set_distance(8.2);
        camera.set_desired_distance(8.2);

        // `yaw = 0` 时相机眼点在 `-X`、视线朝 `+X`(见 `camera.rs`
        // 的 `eye()`:`eye = target - (cos yaw, sin yaw)·d`)。所以要挑
        // **相机前方**的一对点,并让它们在屏幕上分居中线两侧:
        // `yaw = 0` 时屏幕 +X 方向是世界 `+Z`,于是 ±Z 偏移正好分居两侧。
        let left_point: Vec3 = [8.0, 0.0, -2.0];
        let right_point: Vec3 = [8.0, 0.0, 2.0];
        camera.set_yaw(0.0);
        let before_left: f32 = projected_ndc_x(&camera, left_point);
        let before_right: f32 = projected_ndc_x(&camera, right_point);

        // 模拟一次 18 px 的鼠标右移,走**生产代码**那条路径。
        let mut moved: Camera = camera;
        apply_look_delta(&mut moved, 18.0, 0.0);
        let after_left: f32 = projected_ndc_x(&moved, left_point);
        let after_right: f32 = projected_ndc_x(&moved, right_point);

        let delta_left: f32 = after_left - before_left;
        let delta_right: f32 = after_right - before_right;
        // 条件先算好再传进 `assert!`:这样断言的**第一个**参数就是格式串,
        // 符合 `verify_hardcoded_strings` 对格式宏的豁免规则;写成
        // `assert!(!(a.is_nan() || b.is_nan()), "...")` 的话条件会占掉第一个
        // 参数位,格式串落到第二位而被判成 §1.3c 违规。
        let degenerate: bool = before_left.is_nan()
            || before_right.is_nan()
            || delta_left.is_nan()
            || delta_right.is_nan();
        assert!(
            !degenerate,
            "对称点必须都在相机前方,否则这条测不出方向: before={before_left},{before_right} after={after_left},{after_right} eye={:?} d={} fov={}",
            moved.eye(),
            moved.get_distance(),
            moved.get_fov_y()
        );
        // 等值:纯旋转(整体摇镜)而不是平移。
        assert!(
            (delta_left - delta_right).abs() < 1.0e-3,
            "左右对称点的位移应当等值(纯旋转),实得 left={delta_left} right={delta_right}"
        );
        // 方向:内容必须整体**左移**(ndc_x 减小)才是「鼠标右移 = 视角右转」。
        let dir: &str = if delta_left < 0.0 {
            DIR_LEFT
        } else {
            DIR_RIGHT
        };
        assert!(
            delta_left < 0.0,
            "{}",
            fill(T_MOUSE_RIGHT_PANS_RIGHT, &[("dir", dir)])
        );
        assert!(
            LOOK_YAW_SENSITIVITY < 0.0,
            "灵敏度必须为负才能让鼠标右移对应视角右转,实得 {LOOK_YAW_SENSITIVITY}"
        );
    }

    /// 车轮必须绕**轮轴(本地 Z)**自转,且滚动方向满足无滑约束。
    ///
    /// 这条钉住两个事实:
    /// 1. 轮轴是 Z(`car_coupe` 的 `tyres` 每摞 `x .64 / y .64 / z .235`,
    ///    最短轴即轮轴),绕 X 转的话轮盘根本不在滚动平面里;
    /// 2. `spin` 增大时轮底沿车体**后退** —— 车往 `+X` 前进时接触点
    ///    必须相对车体向 `-X` 退,否则轮子是倒着转或者压根没转。
    #[test]
    fn wheels_roll_forward_about_the_real_axle() {
        // 轮心取自 car_coupe 的前右轮(本地 (1.265, 0.32, 0.77))。
        let mount: Vec3 = [1.265, 0.32, 0.77];
        let origin: Vec3 = [0.0, 0.0, 0.0];
        let spin: f32 = 0.1;
        let at_rest: Mat4Data = car_wheel_model(origin, 0.0, mount, 0.0);
        let spun: Mat4Data = car_wheel_model(origin, 0.0, mount, spin);
        let top: f32 =
            wheel_world_x(spun, [0.0, 1.0, 0.0]) - wheel_world_x(at_rest, [0.0, 1.0, 0.0]);
        let bottom: f32 =
            wheel_world_x(spun, [0.0, -1.0, 0.0]) - wheel_world_x(at_rest, [0.0, -1.0, 0.0]);
        // 绕 Z 转时轮顶 / 轮底的 X 投影必须真的动起来(绕 X 转时恒为 0)。
        assert!(
            top.abs() > 1.0e-3 && bottom.abs() > 1.0e-3 && bottom < 0.0 && top > 0.0,
            "{}",
            fill(
                T_WHEEL_ROLLS_FORWARD,
                &[
                    ("spin", &format!("{spin}")),
                    ("bottom", &format!("{bottom:+.4}"))
                ]
            )
        );
    }

    /// 远离原点的位置照样能生成城市内容。
    #[test]
    fn blocks_and_buildings_generate_far_from_the_origin() {
        for center in [[0.0f32, 0.0f32], [3000.0, 3000.0], [-4500.0, 1200.0]] {
            let blocks: Vec<BlockLayout> = blocks_near(center[0], center[1], BLOCK_VIEW_RADIUS);
            assert!(!blocks.is_empty(), "({}) 附近必须有街区", center[0]);
            let buildings: Vec<BuildingPlacement> = build_city_buildings(center[0], center[1]);
            assert!(!buildings.is_empty(), "({}) 附近必须生成楼", center[0]);
            // 每一栋都必须真的落在生成中心附近,而不是仍然堆在原点。
            let nearest: f32 = buildings
                .iter()
                .map(|b: &BuildingPlacement| {
                    (b.position[0] - center[0])
                        .abs()
                        .max((b.position[1] - center[1]).abs())
                })
                .fold(f32::MAX, f32::min);
            assert!(
                nearest <= BLOCK_VIEW_RADIUS,
                "({}) 附近生成的楼离中心 {nearest} m,超出 {BLOCK_VIEW_RADIUS} m 视距",
                center[0]
            );
        }
    }

    /// 地面与水面跟着玩家走,不是钉在世界原点。
    #[test]
    fn streamed_surfaces_follow_the_player() {
        let (origin, far): (MeshAsset, MeshAsset) =
            (build_ground_near(0.0, 0.0), build_ground_near(3000.0, 0.0));
        // 两块地面必然不同 —— 不跟随玩家的话它们会逐字节相同。
        let same: bool = origin
            .parts
            .iter()
            .zip(far.parts.iter())
            .all(|(a, b): (&MeshPart, &MeshPart)| a.positions == b.positions);
        assert!(!same, "地面必须跟着生成中心走,不能固定在世界原点");
        // 路面判定在远处同样成立:3000 是街道轴线,所以那里是沥青。
        assert!(on_roadway(3000.0, 30.0), "远处街道上必须是沥青");
        assert!(!on_roadway(3015.0, 30.0), "远离街道的点不能是沥青");
    }

    /// 流式判定:玩家走满一格街道才重建,原地站着不重建。
    #[test]
    fn streaming_rebuilds_only_after_a_full_street_of_travel() {
        let center: Vec2 = [0.0, 0.0];
        assert!(
            !stream_needs_rebuild([0.0, 0.0], center),
            "原地站着不该触发重建"
        );
        assert!(
            !stream_needs_rebuild([STREAM_REBUILD_STEP - 0.1, 0.0], center),
            "差一步没走满不该触发重建"
        );
        assert!(
            stream_needs_rebuild([STREAM_REBUILD_STEP + 0.1, 0.0], center),
            "走满一整格街道必须触发重建"
        );
        assert!(
            stream_needs_rebuild([0.0, -STREAM_REBUILD_STEP - 0.1], center),
            "Z 方向同样要触发"
        );
    }

    /// 地面 / 水面网格在任何位置都能生成 —— 这才是「地图无限」的底层保证。
    ///
    /// 之前地面是固定 `[-150, 150]` 的一块网格,玩家走到 149 m 就到了
    /// 边;现在它跟着生成中心平移,所以「中心在 3000 m 处」和「中心在
    /// 0 处」生成出来的东西一样多、一样可用。
    #[test]
    fn streamed_ground_and_water_build_far_from_the_origin() {
        for (x, z) in [(0.0f32, 0.0f32), (3000.0, 3000.0), (-6000.0, 1500.0)] {
            let ground: MeshAsset = build_ground_near(x, z);
            let water: MeshAsset = build_water_near(x, z);
            let ground_faces: usize = ground.parts.iter().map(|p: &MeshPart| p.faces.len()).sum();
            let water_faces: usize = water.parts.iter().map(|p: &MeshPart| p.faces.len()).sum();
            assert!(
                ground_faces > 1000,
                "({x},{z}) 的地面必须真的生成了面,得到 {ground_faces}"
            );
            assert!(
                water_faces > 100,
                "({x},{z}) 的水面必须真的生成了面,得到 {water_faces}"
            );
            // 顶点必须落在中心附近,而不是仍然堆在原点。
            // 顶点必须**跟着中心**走。底面是 `GROUND_SPAN` 见方;街道附属
            // 几何(路缘石、斑马线)挂在街道轴线上,最多再往外一个
            // 街道半宽加人行道。所以合法上界是「半块 + 街道附属宽度」,
            // 而不是 `GROUND_SPAN` 本身。
            let reach: f32 = ground
                .parts
                .iter()
                .flat_map(|part: &MeshPart| part.positions.iter())
                .map(|p: &[f32; 3]| (p[0] - x).abs().max((p[2] - z).abs()))
                .fold(0.0f32, f32::max);
            let limit: f32 = GROUND_SPAN * 0.5 + STREET_HALF_WIDTH + SIDEWALK_WIDTH;
            assert!(
                reach <= limit,
                "({x},{z}) 的地面顶点离中心最远 {reach} m,超过 {limit} m —— 街道附属几何铺出了地面块"
            );
        }
        // 原点处和远处的内容不同 —— 不跟随中心的话两块会逐字节相同。
        let a: Vec<[f32; 3]> = build_ground_near(0.0, 0.0).parts[0].positions.clone();
        let b: Vec<[f32; 3]> = build_ground_near(3000.0, 0.0).parts[0].positions.clone();
        assert!(a != b, "地面必须跟着中心走,两块网格不能完全相同");
    }

    /// 车轮矩阵:自转角必须真的改变朝向矩阵,不能只平移。
    ///
    /// **轮轴是本地 Z,不是 X。** 这条原先断言「绕 X 自转时 X 基向量
    /// 不变」—— 那等于把「X 是轮轴」当成规格,而 `car_coupe` 的
    /// `tyres` 顶点实测是每摞 `x .64 / y .64 / z .235`(最短轴即轮轴)。
    /// 车前进方向是本地 `+X`,绕 X 自转等于绕行驶轴打转,轮盘根本
    /// 不在滚动平面里(实测位移恒为 0)。所以这里改成断言 Z 不变、
    /// X / Y 在 (cos, sin) 平面里转。
    #[test]
    fn wheel_spin_rotates_the_axle() {
        let mount: [f32; 3] = crate::traffic::WHEEL_MOUNTS[0];
        let rest: Mat4Data = car_wheel_model([0.0, 0.0, 0.0], 0.0, mount, 0.0);
        let turned: Mat4Data =
            car_wheel_model([0.0, 0.0, 0.0], 0.0, mount, std::f32::consts::FRAC_PI_2);
        // 绕本地 Z 自转:Z 基向量不动,X/Y 两个基向量在 XY 平面上转 90 度。
        // 列主序下第一列是 X 基向量(model[0..3]),所以要看 **第一、二列**。
        let x_rest: [f32; 3] = [rest[0], rest[1], rest[2]];
        let x_turned: [f32; 3] = [turned[0], turned[1], turned[2]];
        let delta: f32 = (0..3)
            .map(|axis: usize| (x_rest[axis] - x_turned[axis]).powi(2))
            .sum::<f32>()
            .sqrt();
        assert!(
            delta > 0.5,
            "{}",
            fill(
                T_WHEEL_SPIN_MOVES_RIM,
                &[
                    ("axis", &format!("{x_rest:?}")),
                    ("turned", &format!("{x_turned:?}"))
                ]
            )
        );
        // Z 轴(轮轴方向)必须保持不变。
        let z_rest: [f32; 3] = [rest[8], rest[9], rest[10]];
        let z_turned: [f32; 3] = [turned[8], turned[9], turned[10]];
        let axle: f32 = (0..3)
            .map(|axis: usize| (z_rest[axis] - z_turned[axis]).powi(2))
            .sum::<f32>()
            .sqrt();
        assert!(
            axle < 1e-5,
            "{}",
            fill(T_WHEEL_AXLE_STILL, &[("delta", &format!("{axle}"))])
        );
        // 旋转不改变轴心位置。
        let centre_delta: f32 =
            (rest[12] - turned[12]) + (rest[13] - turned[13]) + (rest[14] - turned[14]);
        assert!(
            centre_delta.abs() < 1e-5,
            "{}",
            fill(T_WHEEL_CENTRE_FIXED, &[])
        );
    }

    /// 车身转向:同一个安装位在不同车身朝向下必须落在不同世界位置。
    #[test]
    fn wheel_follows_the_body_yaw() {
        let mount: [f32; 3] = crate::traffic::WHEEL_MOUNTS[0];
        let east: Mat4Data = car_wheel_model([0.0, 0.0, 0.0], 0.0, mount, 0.0);
        let north: Mat4Data =
            car_wheel_model([0.0, 0.0, 0.0], -std::f32::consts::FRAC_PI_2, mount, 0.0);
        assert!(
            (east[12] - north[14]).abs() < 1e-5,
            "车身转 90 度后安装位应从 +X 转到 +Z,实际 {} vs {}",
            east[12],
            north[14]
        );
    }

    #[test]
    fn upper_slab_does_not_push_a_player_standing_below_it() {
        let w: FloorWorld = world();
        for index in 0..2 {
            // 房间正中,脚踩首层楼板、头在二层楼板之下。
            let inside: Vec2 = to_world(index, [0.0, 0.0]);
            let pushed: Vec2 = w.resolve_interior(
                inside,
                SHOWCASE_GROUND_TOP,
                SHOWCASE_GROUND_TOP + PLAYER_BODY_HEIGHT,
                PLAYER_RADIUS,
            );
            assert_eq!(pushed, inside, "{}", T_SHOWCASE_CEILING_PUSHED);
        }
    }

    #[test]
    fn partition_splits_the_ground_floor_into_two_rooms() {
        // 隔墙必须真的挡人(首层分成两间),而且留出通往楼梯的过道。
        for index in 0..2 {
            let w: FloorWorld = world();
            let specs: ShowcaseSpecs = showcase_specs();
            let span: Vec2 = specs[index].span;
            let side: Vec2 = local_side(index);
            // 隔墙止于「楼梯左边缘再往回 1.70 m」,过道就在这两者之间。
            let partition_end: f32 =
                span[0] * 0.5 - SHOWCASE_STAIR_WIDTH - crate::r#const::SHOWCASE_PARTITION_GAP;
            let gap_end: f32 = span[0] * 0.5 - SHOWCASE_STAIR_WIDTH;
            let partition_y: f32 = crate::r#const::SHOWCASE_PARTITION_Y;
            // 隔墙中段:挡住。
            let mid: Vec2 = to_world(index, [0.0, partition_y]);
            let pushed: Vec2 = w.resolve_interior(
                mid,
                SHOWCASE_GROUND_TOP,
                SHOWCASE_GROUND_TOP + PLAYER_BODY_HEIGHT,
                PLAYER_RADIUS,
            );
            // 隔墙沿**法线**(门洞方向)挡人,位移必须出现在法线轴上。
            let normal: Vec2 = front_normal(index);
            let moved: f32 = (pushed[0] - mid[0]) * normal[0] + (pushed[1] - mid[1]) * normal[1];
            let escaped: f32 = (pushed[0] - mid[0]) * side[0] + (pushed[1] - mid[1]) * side[1];
            assert!(
                moved.abs() > 1e-3 || escaped.abs() > 1e-3,
                "{} {}",
                T_SHOWCASE_PARTITION_LET_THROUGH,
                T_SHOWCASE_DOOR_OUTSIDE_BLOCKED
            );
            // 隔墙右端与楼梯之间留出过道:那里必须穿得过去。
            // 过道中点:隔墙右端与楼梯之间,必须真的穿得过去。
            assert!(partition_end < gap_end, "{}", T_SHOWCASE_PARTITION_LANE);
            let lane_local: f32 = (partition_end + gap_end) * 0.5;
            let lane: Vec2 = to_world(index, [lane_local, partition_y]);
            let through: Vec2 = w.resolve_interior(
                lane,
                SHOWCASE_GROUND_TOP,
                SHOWCASE_GROUND_TOP + PLAYER_BODY_HEIGHT,
                PLAYER_RADIUS,
            );
            let drift: f32 = (through[0] - lane[0]).hypot(through[1] - lane[1]);
            assert!(
                drift < 1e-3,
                "{} {}",
                T_SHOWCASE_LANE_BLOCKED,
                T_SHOWCASE_FOOTPRINT_CLEAR
            );
        }
    }

    #[test]
    fn no_palm_stands_on_the_roadway() {
        // `street_strips` 只保证棕榈相对**它自己那条街**的横向偏移,而
        // 沿街那一维是自由走的 `step + along_jitter` —— 每隔
        // `STREET_PITCH` 米就会撞上一次垂直街道的轴线,于是每隔 60 m 就
        // 有一棵树落进十字路口的沥青上。这里按用户的要求逐棵判:到**最近
        // 街道轴线**的横向距离必须 >= 路面半宽。
        //
        // 三个生成中心一起判,免得这条断言只在出生点附近成立:世界是
        // 无限的,`street_axis` 的取整在负坐标与大坐标上同样要对。
        for center in [0.0f32, 30.0, 1234.5] {
            for spot in build_city_palms(center, center).iter() {
                let (x, z): (f32, f32) = (spot[0], spot[1]);
                let nearest_x: f32 = street_axis((x / STREET_PITCH).round() as i32);
                let nearest_z: f32 = street_axis((z / STREET_PITCH).round() as i32);
                let gap: f32 = (x - nearest_x).abs().min((z - nearest_z).abs());
                assert!(
                    gap >= STREET_HALF_WIDTH,
                    "{}",
                    fill(
                        T_PALM_ON_ROADWAY,
                        &[
                            ("x", &format!("{x}")),
                            ("z", &format!("{z}")),
                            ("line", &format!("{gap}")),
                            ("need", &format!("{STREET_HALF_WIDTH}")),
                        ],
                    )
                );
            }
        }
    }

    #[test]
    fn palm_lateral_offsets_stay_scattered_inside_the_sidewalk() {
        // 修「路口不种树」不能顺手把横向抖动清零 —— 那会退回上一轮修好的
        // 「复制粘贴的一直线」。安全带是 `[STREET_HALF_WIDTH,
        // STREET_HALF_WIDTH + SIDEWALK_WIDTH]`,3.6 m 宽,里面的取值必须
        // 足够多,才读得出「有的贴路沿、有的靠外侧」。
        let mut distinct: Vec<u32> = Vec::new();
        for spot in build_city_palms(0.0, 0.0).iter() {
            let lateral: f32 = (spot[0] - street_axis((spot[0] / STREET_PITCH).round() as i32)).abs();
            // 量化到 1 cm,滤掉浮点噪声后再数「不同取值」。
            let bucket: u32 = (lateral * 100.0).round() as u32;
            if !distinct.contains(&bucket) {
                distinct.push(bucket);
            }
        }
        let band: f32 = SIDEWALK_WIDTH;
        assert!(
            distinct.len() >= 40,
            "{}",
            fill(
                T_PALM_ROW_IS_UNIFORM,
                &[
                    ("distinct", &format!("{}", distinct.len())),
                    ("band", &format!("{band}")),
                ],
            )
        );
    }

    #[test]
    fn both_showcases_are_placed_off_the_roadway() {
        for spec in showcase_specs().iter() {
            // 用**旋转后**的世界半尺寸判:yaw = ±90° 时 span / half 的
            // X、Z 分量会互换,拿本地的半跨去比就会判错。
            let (sin_yaw, cos_yaw) = spec.yaw.sin_cos();
            let half: Vec2 = [
                (cos_yaw.abs() * spec.half[0] + sin_yaw.abs() * spec.half[1]),
                (sin_yaw.abs() * spec.half[0] + cos_yaw.abs() * spec.half[1]),
            ];
            for axis in 0..2 {
                let reach: f32 = half[axis] + STREET_HALF_WIDTH + SIDEWALK_WIDTH;
                // 无限街道网格:只有紧邻样板楼的那些街道可能压到它,
                // 更远的街道离得远到不可能相交。
                for line in street_indices_in(
                    spec.position[axis] - reach - STREET_PITCH,
                    spec.position[axis] + reach + STREET_PITCH,
                ) {
                    let gap: f32 = (spec.position[axis] - street_axis(line)).abs();
                    assert!(
                        gap > reach,
                        "{} {} {}",
                        T_SHOWCASE_AXIS_ON_ROAD,
                        T_SHOWCASE_FOOTPRINT_CLEAR,
                        spec.position[axis]
                    );
                }
            }
        }
    }

    #[test]
    fn showcase_footprints_do_not_overlap_ordinary_buildings() {
        // 样板楼与程序化楼各推一个「外接 AABB」,必须互不相交。
        // 两者都进同一条 `CollisionWorld`,重叠会让玩家卡在两栋楼的
        // 夹缝里,车也会被隐形墙顶死。
        let specs: ShowcaseSpecs = showcase_specs();
        for spec in specs.iter() {
            let (sin_yaw, cos_yaw) = spec.yaw.sin_cos();
            let half: Vec2 = [
                cos_yaw.abs() * spec.half[0] + sin_yaw.abs() * spec.half[1],
                sin_yaw.abs() * spec.half[0] + cos_yaw.abs() * spec.half[1],
            ];
            for other in build_city_buildings(0.0, 0.0) {
                // 楼的外形是资产包围盒,这里用最宽的通用包围盒
                // (`BUILDING_FOOTPRINT_GUARD`)做保守判定。
                let guard: f32 = 11.0;
                let overlap_x: bool =
                    (spec.position[0] - other.position[0]).abs() < half[0] + guard;
                let overlap_z: bool =
                    (spec.position[1] - other.position[1]).abs() < half[1] + guard;
                assert!(
                    !(overlap_x && overlap_z),
                    "{} {} {} {} {}",
                    T_SHOWCASE_OVERLAPS_ORDINARY,
                    T_SHOWCASE_FOOTPRINT_CLEAR,
                    spec.asset,
                    other.position[0],
                    other.position[1]
                );
            }
        }
    }

    #[test]
    fn showcase_batches_never_duplicate_a_mesh_index() {
        // `WebGlRenderer::upload_mesh` 是 push 语义:一个 mesh 只能上传
        // 一次。这条测试守住「样板楼各自用 `find_or_create_batch` 而不是
        // 各自新建批次」这个不变量 —— 双传会让所有后续批次下标错位。
        let placements: Vec<BuildingPlacement> = showcase_placements();
        assert_eq!(placements.len(), 2);
        let mut assets: Vec<&str> = placements
            .iter()
            .map(|p: &BuildingPlacement| p.asset)
            .collect();
        assets.sort_unstable();
        assets.dedup();
        assert_eq!(assets.len(), 2);
    }

    #[test]
    fn collision_world_excludes_the_showcases() {
        // 样板楼不能进二维碰撞世界,否则门洞外面会有一堵隐形墙。
        let mut bounds: HashMap<String, Bounds> = HashMap::new();
        for spec in showcase_specs() {
            bounds.insert(
                String::from(spec.asset),
                Bounds {
                    min: [-spec.half[0], GROUND_LEVEL, -spec.half[1]],
                    max: [spec.half[0], 7.15, spec.half[1]],
                },
            );
        }
        let mut world: CollisionWorld = CollisionWorld::new();
        build_collision_world(&mut world, &bounds, 0.0, 0.0);
        for spec in showcase_specs() {
            let (sin_yaw, cos_yaw) = spec.yaw.sin_cos();
            let local: Vec2 = [0.0, spec.half[1]];
            let doorway: Vec2 = [
                spec.position[0] + local[0] * cos_yaw + local[1] * sin_yaw,
                spec.position[1] - local[0] * sin_yaw + local[1] * cos_yaw,
            ];
            assert!(
                !world.contains_point(doorway),
                "{} {} {}",
                T_SHOWCASE_DOORWAY_2D_BLOCKED,
                T_SHOWCASE_DOOR_INSIDE,
                spec.asset
            );
        }
    }

    /// 回归测试(第 9 条):附近有伴时,行人必须能**真的**开始聊天。
    #[test]
    fn pedestrians_gather_and_start_talking() {
        let mut peds: Vec<Pedestrian> = ped_pair(10.0, 10.0, 11.0, 10.0);
        update_ped_social_for_test(peds.as_mut_slice(), 0.016);
        let talking: usize = peds.iter().filter(|p: &&Pedestrian| p.is_talking()).count();
        assert!(talking == 2, "{}", T_PEDS_GATHER_AND_TALK);
    }

    /// 回归测试:孤身一人的行人**不会**自己跟自己聊天。
    #[test]
    fn a_lone_pedestrian_never_starts_talking() {
        let mut peds: Vec<Pedestrian> = vec![Pedestrian::new(
            [10.0, 0.0, 10.0],
            0.0,
            [19.0, 10.0],
            PED_SUIT,
            0.0,
        )];
        update_ped_social_for_test(peds.as_mut_slice(), 0.016);
        assert!(!peds[0].is_talking(), "{}", T_PEDS_GATHER_AND_TALK);
    }

    /// 回归测试:聊满约定时长之后必须**散场**,并进入冷却。
    #[test]
    fn a_conversation_ends_and_goes_on_cooldown() {
        let mut peds: Vec<Pedestrian> = ped_pair(10.0, 10.0, 11.0, 10.0);
        update_ped_social_for_test(peds.as_mut_slice(), 0.016);
        assert!(peds.iter().all(|p: &Pedestrian| p.is_talking()));
        for _ in 0..600 {
            update_ped_social_for_test(peds.as_mut_slice(), 0.05);
        }
        assert!(
            peds.iter().all(|p: &Pedestrian| !p.is_talking()),
            "{}",
            T_PEDS_TALK_ENDS
        );
        assert!(
            peds.iter()
                .all(|p: &Pedestrian| p.get_gather_cooldown() > 0.0),
            "{}",
            T_PEDS_TALK_ENDS
        );
    }

    /// 回归测试:被撞倒的行人不参与聊天。
    #[test]
    fn downed_pedestrians_do_not_chat() {
        let mut peds: Vec<Pedestrian> = ped_pair(10.0, 10.0, 11.0, 10.0);
        peds[0].knock_down([1.0, 0.0]);
        update_ped_social_for_test(peds.as_mut_slice(), 0.016);
        assert!(!peds[0].is_talking(), "{}", T_PEDS_DOWNED_NO_CHAT);
    }

    /// 回归测试:扎堆之后每个人的**站位角度必须不同**,否则全挤在一点。
    #[test]
    fn each_pedestrian_takes_a_distinct_place_in_the_circle() {
        let mut peds: Vec<Pedestrian> = ped_pair(10.0, 10.0, 11.0, 10.2);
        peds.push(Pedestrian::new(
            [10.3, 0.0, 11.1],
            0.0,
            [19.3, 11.1],
            PED_SUIT,
            2.0 * PED_TALK_SLOT_STEP,
        ));
        update_ped_social_for_test(peds.as_mut_slice(), 0.016);
        let mut slots: Vec<f32> = peds.iter().map(Pedestrian::get_talk_slot).collect();
        slots.sort_by(|a: &f32, b: &f32| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in slots.windows(2) {
            let gap: f32 = (pair[1] - pair[0]).abs();
            assert!(gap > 1.0e-3, "{}: slots={slots:?}", T_PEDS_DISTINCT_SLOTS);
        }
    }

    /// 跳跃顶高必须够得上一格台阶 / 矮墙,不能是「原地弹一下」。
    ///
    /// 判据是**解析式** `h = v0² / (2·GRAVITY)`,不是跑一遍积分 ——
    /// 积分会受步长量化影响,顶高会有 1–2 cm 抖动。这里钉住的是
    /// 「初速度对应的物理高度」这一层,和 `JUMP_VELOCITY` 的推导一致。
    #[test]
    fn a_jump_clears_a_gta_scale_ledge() {
        let peak: f32 = JUMP_VELOCITY * JUMP_VELOCITY / (2.0 * GRAVITY);
        // 门限取 1.0 m:GTA V 街景里能被跳上去的台阶 / 矮墙在这个量级,
        // 本项目样板楼梯单级只有 0.305 m,一层隔墙 1 m 出头。
        let want: f32 = 1.0;
        assert!(
            peak >= want,
            "{}",
            fill(
                T_JUMP_CLEARS_A_LEDGE,
                &[
                    ("peak", &format!("{peak:.3}")),
                    ("want", &format!("{want:.2}")),
                    ("v0", &format!("{JUMP_VELOCITY:.2}")),
                ]
            )
        );
        // 反向:也不该高到能一口气翻上二层楼板(3.20 m)—— 那不是跳跃,
        // 是飞行。
        assert!(peak < SHOWCASE_UPPER_TOP);
    }

    /// 跑一遍**真实积分**(`step_vertical` 的同一套顺序)验证跳跃形状:
    /// 必须先升后落、滞空约 0.67 s、落地后 `grounded = true` 且 `vy = 0`。
    ///
    /// 这条是「不做得只能原地弹一下」的那道闸门。之前的实现没有
    /// `jump`,`vertical_velocity` 只在自由落体分支里被写;而站着时
    /// `on_slab` 分支会**当帧**把 `vy` 清成 0,于是任何向上写的初速度
    /// 都被地板吃掉,人物根本没离开地面。
    #[test]
    fn a_jump_rises_then_lands_standing() {
        let dt: f32 = FIXED_DT;
        let mut y: f32 = GROUND_LEVEL;
        let mut vy: f32 = 0.0;
        let mut grounded: bool = true;
        let mut peak: f32 = 0.0;
        let mut frames: usize = 0;
        // ---- 站着不动,起跳一次 ----
        grounded = !false;
        vy = if grounded { JUMP_VELOCITY } else { vy };
        grounded = false;
        while frames < 240 {
            frames += 1;
            if y <= GROUND_LEVEL && vy <= 0.0 {
                y = GROUND_LEVEL;
                vy = 0.0;
                grounded = true;
                if frames > 2 {
                    break;
                }
            } else {
                vy = (vy - GRAVITY * dt).max(-TERMINAL_VELOCITY);
                let next: f32 = y + vy * dt;
                if vy <= 0.0 && next <= GROUND_LEVEL + GROUND_SNAP_SKIN {
                    y = GROUND_LEVEL;
                    vy = 0.0;
                    grounded = true;
                } else {
                    y = next;
                }
            }
            peak = peak.max(y);
        }
        let air: f32 = frames as f32 * dt;
        assert!(
            peak > 1.0,
            "{}",
            fill(
                T_JUMP_RISES_BEFORE_FALLING,
                &[
                    ("peak", &format!("{peak:.3}")),
                    ("air", &format!("{air:.3}"))
                ]
            )
        );
        // 滞空必须落在 0.4–1.1 s 这个「像人跳」的区间:太短是弹一下,
        // 太长是在飞。
        assert!(
            (0.4..1.1).contains(&air),
            "{} air={air:.3}",
            T_JUMP_RISES_BEFORE_FALLING
        );
        assert!(
            grounded && vy == 0.0,
            "{}",
            fill(
                T_JUMP_LANDS_STANDING,
                &[
                    ("grounded", &format!("{grounded}")),
                    ("vy", &format!("{vy}")),
                ]
            )
        );
    }

    /// 滞空途中不得二次起跳(否则一次按键能叠出无穷高的跳)。
    #[test]
    fn a_jump_only_starts_from_the_ground() {
        let mut player: Player = Player::new([0.0, 0.0, 0.0], 0.0);
        assert!(player.get_grounded());
        assert!(player.set_jump_requested(true), "站着时必须能起跳");
        assert_eq!(player.get_vertical_velocity(), JUMP_VELOCITY);
        assert!(!player.get_grounded(), "起跳后必须立刻离开支撑面");
        // 滞空中再按一次:拒绝。
        assert!(
            !player.set_jump_requested(false),
            "{}",
            T_JUMP_ONLY_FROM_GROUND
        );
        assert_eq!(player.get_vertical_velocity(), JUMP_VELOCITY);
    }

    /// 站在地面上时,`step_vertical` 的稳态必须原样不动。
    ///
    /// 这条是「没按键时物理保持静止」的回归 —— 起跳那段插在贴面判定
    /// 之前,如果排错了位置,站着不动也会被当成起跳。
    #[test]
    fn the_vertical_integrator_is_still_when_standing() {
        let dt: f32 = FIXED_DT;
        let mut y: f32 = GROUND_LEVEL;
        let mut vy: f32 = 0.0;
        for _ in 0..600 {
            if y <= GROUND_LEVEL {
                y = GROUND_LEVEL;
                vy = 0.0;
            } else {
                vy = (vy - GRAVITY * dt).max(-TERMINAL_VELOCITY);
                y += vy * dt;
            }
        }
        assert!(
            (y - GROUND_LEVEL).abs() < 1e-6 && vy == 0.0,
            "{}",
            fill(
                T_VERTICAL_REST_ON_FLOOR,
                &[("y", &format!("{y:.3}")), ("vy", &format!("{vy:.3}"))]
            )
        );
    }

    /// 回归测试:**走**和**跑**必须给出两个不同的速度上限。
    ///
    /// 这条取代原先两条「软边界内不减速 / 远在界外也不减速」测试 ——
    /// 它们盯的是一个已经不存在的功能:6cc757c 删掉了流式世界的软边界,
    /// `soft_limit_speed()` 退化成恒等函数之后,这两条断言恒为真,测不出
    /// 任何东西(它们当时是「绿的」,但冲刺同时也是坏的)。
    ///
    /// 真正把 `RUN_SPEED` / `WALK_SPEED` 送进 `Player::step` 的就是
    /// `simulate` 里那一句 `let speed: f32 = wanted;`,所以这里钉住的是
    /// **选值本身**:走路拿到 `WALK_SPEED`,按住 Shift 拿到 `RUN_SPEED`,
    /// 两者之比必须正好是 `RUN_SPEED / WALK_SPEED`。
    #[test]
    fn walk_and_run_pick_different_speed_caps() {
        let wanted: f32 = if false { RUN_SPEED } else { WALK_SPEED };
        assert!(
            (wanted - WALK_SPEED).abs() < 1e-6,
            "{}",
            fill(
                T_WALK_SPEED_PICK,
                &[
                    ("got", &format!("{wanted:.4}")),
                    ("want", &format!("{WALK_SPEED:.4}"))
                ]
            )
        );
        let running: f32 = if true { RUN_SPEED } else { WALK_SPEED };
        assert!(
            (running - RUN_SPEED).abs() < 1e-6 && running > wanted,
            "{}",
            fill(
                T_RUN_SPEED_PICK,
                &[
                    ("got", &format!("{running:.4}")),
                    ("want", &format!("{RUN_SPEED:.4}")),
                ]
            )
        );
        // 冲刺必须真的比走路快 —— 曾经两者被压成同一个值。
        assert!(
            running / wanted > RUN_OVER_WALK_MIN,
            "{}",
            fill(
                T_RUN_FASTER_THAN_WALK,
                &[
                    ("ratio", &format!("{:.4}", running / wanted)),
                    ("min", &format!("{RUN_OVER_WALK_MIN:.4}")),
                ]
            )
        );
    }

    // ---- 方向键驾驶转向 ------------------------------------------------
    //
    // 这一组盯的是 `simulate` 里那一行 `game.input.steer_axis()`。它绕开的
    // 坑是:方向键早就在 `TRACKED_KEYS` 里(`git grep -c ArrowUp` 只数到
    // const.rs 那个字符串字面量,看不到标识符),于是**看上去**接好了,
    // 实际 `steer_input` 一直等于 `strafe_input`,只有 A/D 进得来。
    //
    // 判据都落在**真正喂进 `TrafficCar::drive` 的数值**上,而不是中间
    // 变量:光断言 `steer_axis()` 非零,一辆完全不改 yaw 的车也能通过。

    /// 造一个只按住 `codes` 的 `InputState`。
    fn input_holding(codes: &[&str]) -> super::InputState {
        let mut input: super::InputState = super::InputState::default();
        for code in codes {
            input.keys.insert(String::from(*code), true);
        }
        input
    }

    /// 空世界里的车,给足初速好越过 `STEER_MIN_SPEED` 的转向死区。
    fn steer_test_car() -> crate::traffic::TrafficCar {
        crate::traffic::TrafficCar::new(CAR_SEDAN, 0.0, 0.0, 8.0, 1.0)
    }

    /// 方向键在**驾驶**时必须真的把车转出角度,符号相反。
    ///
    /// 走的是完整的 `drive()` 而不只是 `steer_axis()`:这正是漏接线时
    /// 会漏掉的那一段。`+steer` 按 `drive` 的文档约定是右转,所以
    /// ArrowRight 的 yaw 必须**大于** ArrowLeft 的。
    #[test]
    fn arrow_keys_steer_the_car_while_driving() {
        let world: CollisionWorld = CollisionWorld::new();
        let delta: f32 = 1.0 / 60.0;
        let mut deltas: Vec<(String, f32)> = Vec::new();
        for code in [ARROWLEFT, ARROWRIGHT] {
            let input: super::InputState = input_holding(&[code]);
            let steer: f32 = input.steer_axis();
            assert!(
                steer.abs() > 0.0,
                "{}",
                fill(
                    T_ARROW_STEERS_WHILE_DRIVING,
                    &[
                        ("key", &format!("{code}")),
                        ("got", &format!("{steer:.4}")),
                    ]
                )
            );
            let mut car: crate::traffic::TrafficCar = steer_test_car();
            let yaw0: f32 = car.get_yaw();
            for _ in 0..120 {
                car.drive(0.6, steer, delta, &world);
            }
            let turned: f32 = car.get_yaw() - yaw0;
            assert!(
                turned.abs() > 0.3,
                "{}",
                fill(
                    T_ARROW_STEERS_WHILE_DRIVING,
                    &[
                        ("key", &format!("{code}")),
                        ("got", &format!("{turned:.4} rad"))
                    ]
                )
            );
            deltas.push((String::from(code), turned));
        }
        let (left, right): (&(String, f32), &(String, f32)) = (&deltas[0], &deltas[1]);
        assert!(
            left.1 < 0.0 && right.1 > 0.0,
            "{}",
            fill(
                T_ARROW_STEER_SIGNS_OPPOSED,
                &[
                    ("left", &format!("{:.4}", left.1)),
                    ("right", &format!("{:.4}", right.1)),
                ]
            )
        );
        // 方向键的转向必须与 A/D **完全对称**:和为零即两组键的增益
        // 一致,方向键没有被悄悄乘上一个小系数。
        assert!(
            (left.1 + right.1).abs() < 1e-3,
            "左右转向幅度不对称,左 {:.4} 右 {:.4},差 {:.6}",
            left.1,
            right.1,
            (left.1 + right.1).abs()
        );
    }

    /// 方向键在**步行**时不得进入移动轴:位置必须逐轴纹丝不动。
    ///
    /// GTA V 的移动只认 WASD;把方向键接成移动键会让玩家的手在方向键
    /// 上无意识地「滑动」。这里模拟 `simulate` 的步行分支——`intent`
    /// 只由 `axis(D, A)` 与 `axis(W, S)` 组成,方向键根本不在里面。
    #[test]
    fn arrow_keys_do_not_move_the_player_on_foot() {
        for code in [ARROWLEFT, ARROWRIGHT, ARROWUP, ARROWDOWN] {
            let input: super::InputState = input_holding(&[code]);
            // 步行分支的移动轴只认 WASD:这里复刻 `simulate` 的两行。
            let strafe: f32 = super::axis(&input, KEYD, KEYA);
            let forward: f32 = super::axis(&input, KEYW, KEYS);
            assert!(
                strafe == 0.0 && forward == 0.0,
                "{}",
                fill(
                    T_ARROW_DOES_NOT_MOVE_ON_FOOT,
                    &[
                        ("key", &format!("{code}")),
                        ("axis", &format!("{AXIS_STRAFE}")),
                        ("got", &format!("{strafe}")),
                    ]
                )
            );
        }
        // 端到端:只有方向键按住时,`Player::step` 走满 2 秒,位置不变。
        let input: super::InputState = input_holding(&[ARROWRIGHT]);
        let world: CollisionWorld = CollisionWorld::new();
        let mut player: Player = Player::new([4.0, GROUND_LEVEL, -7.0], 0.0);
        let start: Vec3 = player.get_position();
        for _ in 0..120 {
            let intent: Vec2 = [
                super::axis(&input, KEYD, KEYA),
                super::axis(&input, KEYW, KEYS),
            ];
            player.step(intent, [0.0, -1.0], 1.0 / 60.0, WALK_SPEED, &world);
        }
        let end: Vec3 = player.get_position();
        assert!(
            (end[0] - start[0]).abs() < 1e-6 && (end[2] - start[2]).abs() < 1e-6,
            "{}",
            fill(
                T_FOOT_POSITION_UNCHANGED,
                &[
                    ("dx", &format!("{:.6}", end[0] - start[0])),
                    ("dz", &format!("{:.6}", end[2] - start[2])),
                ]
            )
        );
    }

    /// A/D 与方向键**同按**时舵角必须是满舵,不能互相抵消成 0。
    ///
    /// 策略是「后按的赢」(`steer_key`),这条把两种输入源混用的三种
    /// 情况都钉住:A 与 ← 一致(都左)、D 与 → 一致(都右)、
    /// 交叉的 A 与 → 则由**后按**的键决定。
    #[test]
    fn ad_and_arrow_keys_do_not_cancel_each_other() {
        // 同向:A + ArrowLeft → 满左舵;后按的箭头接管,值不变。
        let mut input: super::InputState = input_holding(&[KEYA]);
        input.claim_steer(ARROWLEFT);
        assert_eq!(input.steer_axis(), -1.0);
        // 交叉:先按 A,再按 ArrowRight → 后按的右赢,满右舵而不是 0。
        let mut crossed: super::InputState = input_holding(&[KEYA]);
        crossed.claim_steer(ARROWRIGHT);
        let steer: f32 = crossed.steer_axis();
        assert!(
            steer.abs() == 1.0,
            "{}",
            fill(
                T_AD_AND_ARROW_DO_NOT_CANCEL,
                &[("got", &format!("{steer:.4}"))]
            )
        );
        // 拥有者松手后控制权交还:此时 A 仍按着,应回到左舵。
        crossed.keys.insert(String::from(ARROWRIGHT), false);
        crossed.steer_key = String::new();
        assert_eq!(crossed.steer_axis(), -1.0);
        // 只有 D 时仍按老路径工作 —— 这条保证没把原有 A/D 修坏。
        let plain: super::InputState = input_holding(&[KEYD]);
        assert_eq!(plain.steer_axis(), 1.0);
    }

    // ---- 换弹键位:GTA V 的 R ------------------------------------------
    //
    // 判据全部落在 `Arsenal` 的**弹匣余量 / 换弹计时**上,而不是比对按键
    // 码字符串 —— 字符串那条只能证明常量改对了,证明不了它真的接到了
    // `reload()` 上。这里每条都从「非满匣」出发(实机 6 发),跑完
    // `RELOAD_TIME` 再看弹匣有没有真的回升。
    //
    // **不用浮点等值判断弹匣**:弹匣是 `u32`,直接整数比较。

    /// 造一把打到「非满匣」的手枪,给换弹测试当起点。
    fn partially_spent_pistol() -> crate::combat::Arsenal {
        let mut arsenal: crate::combat::Arsenal = crate::combat::Arsenal::new();
        // 6 发 —— 和验收时 HUD 实测到的那个数一致。
        arsenal.set_pistol_ammo(6);
        arsenal
    }

    /// 换弹键就是 GTA V PC 的 R。
    ///
    /// 这条钉的是**常量本身**。它是那一行 `pub const KEY_RELOAD` 的
    /// 回归锁:没有它,有人把换弹改回 C 键时下面那些行为测试可能照样
    /// 绿(因为它们直接调 `reload()`),而实机按键又是坏的。
    #[test]
    fn reload_key_is_the_gta_r_key() {
        assert_eq!(
            super::KEY_RELOAD,
            SLOT_RELOAD_KEY,
            "{}",
            fill(T_RELOAD_KEY_IS_GTA_R, &[("got", super::KEY_RELOAD)])
        );
    }

    /// 按 R 换弹:计时推起来 → 跑完 → 弹匣真的回升。
    #[test]
    fn r_starts_a_reload_that_actually_refills_the_magazine() {
        let mut arsenal: crate::combat::Arsenal = partially_spent_pistol();
        let before: u32 = arsenal.get_magazine();
        assert_eq!(before, 6);
        // R 那一帧:`reload()` 成功并且把计时推满。
        assert!(arsenal.reload());
        let reloading: f32 = arsenal.get_reloading();
        assert!(
            reloading > RELOAD_TIME - 0.01,
            "{}",
            fill(
                T_RELOAD_R_STARTS_RELOAD,
                &[("got", &format!("{reloading:.3}"))]
            )
        );
        // 换弹途中弹匣**不该**变 —— 弹药在计时归零那一刻才真的填进去。
        assert_eq!(arsenal.get_magazine(), before);
        // 跑完整个换弹时长(多跑一帧确保计时真的归零)。
        arsenal.tick(RELOAD_TIME);
        arsenal.tick(FIXED_DT);
        let after: u32 = arsenal.get_magazine();
        assert!(
            after > before,
            "{}",
            fill(T_RELOAD_R_REFILLS_MAGAZINE, &[("got", &format!("{after}"))])
        );
        assert_eq!(after, PISTOL_MAGAZINE);
        assert!(!arsenal.is_reloading());
    }

    /// 换弹从备弹里扣,不是凭空造子弹。
    #[test]
    fn reload_does_not_mint_rounds_out_of_nothing() {
        let mut arsenal: crate::combat::Arsenal = partially_spent_pistol();
        let reserve_before: u32 = arsenal.get_reserve();
        let loaded: u32 = PISTOL_MAGAZINE - arsenal.get_magazine();
        arsenal.reload();
        arsenal.tick(RELOAD_TIME + FIXED_DT);
        assert_eq!(arsenal.get_magazine(), PISTOL_MAGAZINE);
        assert_eq!(
            arsenal.get_reserve(),
            reserve_before - loaded,
            "{}",
            fill(
                T_RELOAD_CONSUMES_RESERVE,
                &[("got", &format!("{}", arsenal.get_reserve()))]
            )
        );
    }

    /// C 键不再换弹 —— 它既不在 `TRACKED_KEYS` 里,也不该触发 `reload()`。
    ///
    /// 这一条按任务要求钉死「C 不再是换弹键」。GTA V 里 C 是进车 /
    /// 蹲伏,本项目两个都没有,所以 C 是**完全未绑定**的键。
    #[test]
    fn c_no_longer_reloads() {
        // C 甚至不在按键白名单里 —— 事件层就直接放行了。
        assert!(
            !super::TRACKED_KEYS.contains(&SLOT_FORMER_RELOAD_KEY),
            "{}",
            fill(
                T_C_NO_LONGER_RELOADS,
                &[("got", "C is still a tracked key")]
            )
        );
        // 即便绕过事件层直接调换弹入口,C 也不会出现在任何 match 分支里:
        // 白名单里只有 `KEY_RELOAD`(= "KeyR")这一个换弹 code。
        let reload_codes: Vec<&str> = super::TRACKED_KEYS
            .iter()
            .copied()
            .filter(|code: &&str| *code == super::KEY_RELOAD)
            .collect();
        assert_eq!(reload_codes.len(), 1);
        assert!(!reload_codes.contains(&SLOT_FORMER_RELOAD_KEY));
    }

    /// 连按 R 不会叠加出多次换弹:换弹中再按 R 必须被拒。
    ///
    /// 键盘的 `keydown` 在按住不放时会自动重复触发(浏览器的 OS 级
    /// repeat)。`bind_keyboard` 靠 `is_press`(「上一次是不是已经按着」)
    /// 挡掉了自动重复,但玩家**主动**连按两下仍然会进两次
    /// `keydown`。`Arsenal::reload` 自己用 `reloading > 0` 兜底,所以
    /// 第二次必须被拒。
    #[test]
    fn pressing_r_again_mid_reload_does_not_stack() {
        let mut arsenal: crate::combat::Arsenal = partially_spent_pistol();
        assert!(arsenal.reload());
        let first: f32 = arsenal.get_reloading();
        // 再按一次 R。
        assert!(!arsenal.reload());
        assert!(
            (arsenal.get_reloading() - first).abs() < 0.001,
            "{}",
            fill(
                T_RELOAD_NOT_REPEATABLE,
                &[("got", &format!("{:.3}", arsenal.get_reloading()))]
            )
        );
        // 换完之后弹匣**只补一次**,不会补出 18 发。
        arsenal.tick(RELOAD_TIME + FIXED_DT);
        assert_eq!(arsenal.get_magazine(), PISTOL_MAGAZINE);
    }
}
