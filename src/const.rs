//! 全项目字符串常量单一来源(rust-standards §1.3c)。
//!
//! verifier `verify_hardcoded_strings.py` 把 `const.rs` 视为字符串字面量的
//! canonical home:其余任何 `.rs` 文件都不允许再内联 ≥ 4 个非平凡字符的
//! 字符串字面量(测试目录、`#[...]` 属性行、format 宏的格式串除外)。
//!
//! 命名约定:
//! - `SCREAMING_SNAKE_CASE`,按 `(name_len, name_lex)` 排序(§1.5)。
//! - `STYLE_*` 是给 euv `html!` 用的整段 CSS。euv 宏的 `style: { k: "v" }`
//!   语法**不接受** const 标识符作为属性值(实测 E0308:宏会把它塞进
//!   `Css::style_string`,要求同构 `AsRef<str>` 元组),因此必须整段提为
//!   一个 const 再写 `style: STYLE_ROOT`。
//! - DOM id / 事件名 / 按键码集中在此,便于与 `index.html` 对照。

pub const DUSK: &str = "DUSK";

pub const BLDG_APRICOT_MOTEL: &str = "bldg_apricot_motel";

pub const BLDG_LILAC_TOWER: &str = "bldg_lilac_tower";

pub const U_EXPOSURE: &str = "u_exposure";

pub const U_TONE_MAP_WHITE: &str = "u_tone_map_white";

pub const HIDE: &str = "hide";

pub const TONE_MAP_WHITE_DUSK: f32 = 2.40;

pub const TONE_MAP_WHITE_DAY: f32 = 2.40;

pub const TONE_MAP_WHITE_NIGHT: f32 = 2.60;

pub const SKY_TINT_GAIN: f32 = 0.012;

pub const KEYA: &str = "KeyA";

pub const KEYD: &str = "KeyD";

pub const KEYR: &str = "KeyR";

pub const KEYS: &str = "KeyS";

pub const KEYT: &str = "KeyT";

pub const KEYW: &str = "KeyW";

/// 跳跃键(GTA V 的 Space)。
pub const KEY_SPACE: &str = "Space";

/// 投掷手雷键(GTA V 的 G)。
pub const KEY_GRENADE: &str = "KeyG";

/// 收起武器 / 切回徒手的键(GTA V 的 H)。
pub const KEY_UNARMED: &str = "KeyH";

/// 切换相机模式的键(GTA V 的 V)。
pub const KEY_CAMERA: &str = "KeyV";

/// 打开 / 关闭地图的键(GTA V 的 Tab)。
pub const KEY_MAP: &str = "Tab";

pub const NOON: &str = "NOON";

pub const NIGHT: &str = "NIGHT";

pub const U_EYE: &str = "u_eye";

pub const U_FOG: &str = "u_fog";

pub const GROUND: &str = "ground";

pub const HUD_ID: &str = "vcw-hud";

pub const WEBGL2: &str = "WebGL2";

pub const ARROWUP: &str = "ArrowUp";

pub const BOOTING: &str = "booting…";

pub const ID_HELP: &str = "vcw-help";

pub const ID_ROOT: &str = "vcw-root";

pub const CANVAS2D: &str = "Canvas2D";

pub const CAR_TAXI: &str = "car_taxi";
pub const PICKUP_HEALTH_PACK: &str = "pickup_health_pack";
pub const PICKUP_ARMOR_VEST: &str = "pickup_armor_vest";
pub const PICKUP_AMMO_BOX: &str = "pickup_ammo_box";
pub const PICKUP_CASH_STACK: &str = "pickup_cash_stack";
pub const WEP_PISTOL: &str = "wep_pistol";
pub const WEP_SMG: &str = "wep_smg";
pub const WEP_BAT: &str = "wep_bat";
pub const WEP_GRENADE: &str = "wep_grenade";
pub const MARKER_FLAME: &str = "marker_flame";

pub const FALLBACK: &str = "fallback";

pub const PED_SUIT: &str = "ped_suit";

pub const SIGN_BAR: &str = "sign_bar";

pub const UI_TITLE: &str = "VICE CITY";

pub const WEBGL2_2: &str = "webgl2";

pub const ARROWDOWN: &str = "ArrowDown";

pub const ARROWLEFT: &str = "ArrowLeft";

pub const CANVAS_ID: &str = "vcw-canvas";

pub const CAR_COUPE: &str = "car_coupe";

pub const CAR_SEDAN: &str = "car_sedan";

pub const GROUND_ID: &str = "ground_procedural";

pub const ID_TOPBAR: &str = "vcw-topbar";

pub const PALM_TALL: &str = "palm_tall";

pub const PED_DRESS: &str = "ped_dress";

pub const SIGN_CLUB: &str = "sign_club";

pub const U_AMBIENT: &str = "u_ambient";

pub const ARROWRIGHT: &str = "ArrowRight";

pub const ATTR_STYLE: &str = "style";

pub const ATTR_VALUE: &str = "value";

pub const CAR_POLICE: &str = "car_police";

pub const LOADING_ID: &str = "vcw-loading";

pub const PALM_SHORT: &str = "palm_short";

pub const PROP_BENCH: &str = "prop_bench";

pub const SIGN_DINER: &str = "sign_diner";

pub const SIGN_HOTEL: &str = "sign_hotel";

pub const SIGN_MOTEL: &str = "sign_motel";

pub const SIGN_PIZZA: &str = "sign_pizza";

pub const ASSETS_BASE: &str = "assets/";

pub const EVENT_INPUT: &str = "input";

pub const EVENT_KEYUP: &str = "keyup";

pub const EVENT_WHEEL: &str = "wheel";

pub const GROUND_PART: &str = "street";

pub const NO_INFO_LOG: &str = "no info log";

pub const PROGRESS_ID: &str = "vcw-progress";

pub const SIGN_ARCADE: &str = "sign_arcade";

pub const SIGN_TROPIC: &str = "sign_tropic";

pub const UI_SUBTITLE: &str = "WASM EDITION";

pub const U_LIGHT_DIR: &str = "u_light_dir";

pub const U_SKY_COLOR: &str = "u_sky_color";

pub const U_VIEW_PROJ: &str = "u_view_proj";

pub const APP_SELECTOR: &str = "#app";

pub const BACKEND_NONE: &str = "init";

pub const DISPLAY_NONE: &str = "display: none";

pub const EVENT_RESIZE: &str = "resize";

pub const PED_OVERALLS: &str = "ped_overalls";

pub const TRUCK_PICKUP: &str = "truck_pickup";

pub const ERR_NO_WINDOW: &str = "no window";

pub const EVENT_KEYDOWN: &str = "keydown";

pub const EXPECT_GROUND: &str = "procedural ground must be valid";

pub const LOG_NO_CANVAS: &str = "[vcw] canvas element missing";

pub const LOG_NO_WINDOW: &str = "[vcw] no window, aborting";

pub const U_LIGHT_COLOR: &str = "u_light_color";

pub const BACKEND_WEBGL2: &str = "WebGL2";

pub const BLDG_DECO_PINK: &str = "bldg_deco_pink";

pub const BLDG_DECO_TEAL: &str = "bldg_deco_teal";

pub const BLDG_MINT_SHOP: &str = "bldg_mint_shop";

pub const BLDG_TEAL_LOFT: &str = "bldg_teal_loft";

pub const BUILDING_SCENE: &str = "building scene…";

pub const EVENT_TOUCHEND: &str = "touchend";

pub const FALLBACK_BLOCK: &str = "fallback_block";

pub const KEY_SHIFT_LEFT: &str = "ShiftLeft";

pub const LOG_NOT_CANVAS: &str = "[vcw] #vcw-canvas is not a <canvas>";

pub const PED_STREETWEAR: &str = "ped_streetwear";

pub const PHASE_LABEL_ID: &str = "vcw-phase-label";

pub const PROP_NEWSSTAND: &str = "prop_newsstand";

pub const PROP_TRASH_BIN: &str = "prop_trash_bin";

pub const STYLE_BLOCK_01: &str =
    "position: relative;width: 100%;height: 100%;overflow: hidden;background: #10121c";

pub const STYLE_BLOCK_02: &str = "position: absolute;top: 0;left: 0;width: 100%;height: 100%;display: block;touch-action: none;cursor: grab";

pub const STYLE_BLOCK_03: &str = "position: absolute;top: 0;left: 0;right: 0;padding: 10px 14px;display: flex;flex-direction: row;align-items: center;justify-content: space-between;box-sizing: border-box;font-family: ui-monospace, monospace;font-size: 12px;color: #ffd9f2;text-shadow: 0 1px 3px rgba(0,0,0,.85);pointer-events: none";

pub const STYLE_BLOCK_04: &str = "background: rgba(10,12,22,.55);border: 1px solid rgba(255,140,200,.35);border-radius: 6px;padding: 5px 9px;letter-spacing: .04em";

pub const STYLE_BLOCK_05: &str = "background: rgba(10,12,22,.55);border: 1px solid rgba(255,140,200,.35);border-radius: 6px;padding: 5px 9px;display: flex;flex-direction: row;align-items: center;gap: 8px;pointer-events: auto";

pub const STYLE_BLOCK_06: &str = "letter-spacing: .18em;color: #7dfcff";

pub const STYLE_BLOCK_07: &str = "width: 120px";

pub const STYLE_BLOCK_08: &str = "opacity: .7";

pub const STYLE_BLOCK_09: &str = "position: absolute;left: 14px;bottom: 12px;padding: 6px 10px;border-radius: 6px;background: rgba(10,12,22,.5);border: 1px solid rgba(125,252,255,.25);font-family: ui-monospace, monospace;font-size: 11px;line-height: 1.55;color: #cfe9ff;pointer-events: none";

pub const STYLE_BLOCK_10: &str = "position: absolute;top: 0;left: 0;right: 0;bottom: 0;display: flex;flex-direction: column;align-items: center;justify-content: center;background: linear-gradient(160deg, #1a1030 0%, #2b1246 55%, #06202e 100%);z-index: 10;font-family: ui-monospace, monospace";

pub const STYLE_BLOCK_11: &str = "font-size: 26px;letter-spacing: .30em;color: #ff7ad9;text-shadow: 0 0 18px rgba(255,122,217,.75)";

pub const STYLE_BLOCK_12: &str =
    "margin-top: 6px;font-size: 11px;letter-spacing: .34em;color: #7dfcff";

pub const STYLE_BLOCK_13: &str = "margin-top: 26px;width: min(340px, 70vw);height: 8px;border-radius: 999px;background: rgba(255,255,255,.12);overflow: hidden";

pub const STYLE_BLOCK_14: &str = "width: 0%;height: 100%;border-radius: 999px;background: linear-gradient(90deg, #ff7ad9, #7dfcff);transition: width .18s ease-out";

pub const STYLE_BLOCK_15: &str =
    "margin-top: 10px;font-size: 11px;letter-spacing: .14em;color: #e6d4ff";

pub const BLDG_CORAL_HALL: &str = "bldg_coral_hall";

pub const ERROR_BAR_STYLE: &str = "width: 100%; background: #ff4d6d";

pub const EVENT_POINTERUP: &str = "pointerup";

/// 指针锁变更通知(进入 / 退出指针锁时浏览器派发)。
///
/// 视角在锁定态下走 `MouseEvent.movement_x/y`,退出锁要靠这个事件把
/// 拖拽状态复位,否则 Esc 之后相机还停在「正在拖」的分支里,下一次
/// 鼠标移动会继续转视角。
pub const EVENT_POINTERLOCKCHANGE: &str = "pointerlockchange";

/// 点击事件:用来在**真实用户手势**里申请指针锁。
///
/// `request_pointer_lock` 必须由用户手势触发,启动时直接调会被浏览器
/// 拒绝,所以只能挂在 click 上。
pub const EVENT_CLICK: &str = "click";

pub const EVENT_TOUCHMOVE: &str = "touchmove";

pub const KEY_SHIFT_RIGHT: &str = "ShiftRight";

pub const LOG_NO_DOCUMENT: &str = "[vcw] no document, aborting";

pub const PHASE_SLIDER_ID: &str = "vcw-phase";

pub const U_EMISSIVE_GAIN: &str = "u_emissive_gain";

pub const U_GLOW_STRENGTH: &str = "u_glow_strength";

pub const BLDG_AQUA_ARCADE: &str = "bldg_aqua_arcade";

pub const BLDG_CREAM_BLOCK: &str = "bldg_cream_block";

pub const EVENT_TOUCHSTART: &str = "touchstart";

pub const INPUT_TYPE_RANGE: &str = "range";

pub const PROGRESS_TEXT_ID: &str = "vcw-progress-text";

pub const PROP_PHONE_BOOTH: &str = "prop_phone_booth";

pub const PROP_STREETLIGHT: &str = "prop_streetlight";

pub const BLDG_PINK_TERRACE: &str = "bldg_pink_terrace";

pub const BLDG_SAND_MIDRISE: &str = "bldg_sand_midrise";

pub const EVENT_POINTERDOWN: &str = "pointerdown";

pub const EVENT_POINTERMOVE: &str = "pointermove";

pub const EVENT_TOUCHCANCEL: &str = "touchcancel";

pub const FETCHING_MANIFEST: &str = "fetching manifest…";

pub const ID_PROGRESS_TRACK: &str = "vcw-progress-track";

pub const PROP_FIRE_HYDRANT: &str = "prop_fire_hydrant";

pub const PROP_TRAFFICLIGHT: &str = "prop_trafficlight";

pub const PROP_TRAFFIC_CONE: &str = "prop_traffic_cone";

pub const WEBGL2_UNAVAILABLE: &str = "WebGL2 unavailable";

pub const BLDG_WHITE_LANDMARK: &str = "bldg_white_landmark";

pub const EVENT_POINTERCANCEL: &str = "pointercancel";

pub const CANVAS2D_UNAVAILABLE: &str = "Canvas2D unavailable";

pub const CREATE_BUFFER_FAILED: &str = "create_buffer failed";

/// `facing_yaw` 回归测试:渲染矩阵算出的正面必须与速度同向。
pub const T_FACING_MATCHES_VELOCITY: &str = "yaw 下资产正面方向必须与速度同向,而不是差 90°";

/// 回归测试:左右腿必须反相(同相就是齐步走)。
pub const T_LEGS_ANTIPHASE: &str = "左右腿必须反相迈步";

/// 回归测试:同一侧的大腿 / 小腿必须同相位。
pub const T_SAME_SIDE_IN_PHASE: &str = "同侧大腿与小腿必须同相";

/// 回归测试:停下时所有关节必须归零。
pub const T_LIMBS_RELAX_TO_ZERO: &str = "停下时所有肢体摆角必须归零";

/// 回归测试:末端 limb 必须声明父关节,否则腿会断成两截。
pub const T_DISTAL_LIMB_HAS_PARENT: &str = "末端 limb 必须声明父关节";

/// 回归测试:父 part 必须也在计划表里。
pub const T_PARENT_IN_PLAN: &str = "父 part 必须也在 LIMB_PLAN 里";

/// 回归测试:软边界不得干预界内的人。
pub const T_SOFT_LIMIT_SPARE_INSIDE: &str = "软边界不该干预界内的人";

/// 回归测试:越界越深速度必须越小。
pub const T_SOFT_LIMIT_RAMP: &str = "越界越深速度必须单调变小";

/// 回归测试:越界超过一个余量后速度归零。
pub const T_SOFT_LIMIT_STOPS_AT_VOID: &str = "软边界必须在虚空之前把人按住";

/// 回归测试:界内不得有任何回推。
pub const T_SOFT_PUSH_ZERO_INSIDE: &str = "界内不该有回推";

/// 回归测试:分离不得再做位置钳制(否则就是空气墙)。
pub const T_NO_POSITION_CLAMP: &str = "碰撞分离不得再钳位置(那是空气墙)";

/// 回归测试:动态分离必须按质量加权(轻的弹开、重的几乎不动)。
pub const T_DYNAMIC_MASS_WEIGHTED: &str = "动态分离必须按质量加权";

/// 回归测试:不重叠的动态体不得被移动。
pub const T_DYNAMIC_NO_MOVE_WHEN_CLEAR: &str = "不重叠的动态体不该被移动";

/// 回归测试:同类实体之间也必须分开(人不能穿人)。
pub const T_DYNAMIC_SAME_KIND_SEPARATES: &str = "同类动态实体之间也必须分开";

/// 回归测试:动态层也必须把实体从静态形状里推出来。
pub const T_DYNAMIC_STATIC_TOO: &str = "动态体也必须被推出静态形状";

/// `replace_mesh` 的下标越界错误前缀(见 `WebGlRenderer::replace_mesh`)。
pub const REPLACE_MESH_OUT_OF_RANGE: &str = "replace_mesh index out of range";

/// 流式重建后 GPU 网格重传失败时记的日志前缀。
pub const LOG_REPLACE_MESH_FAILED: &str = "[vcw] replace_mesh FAILED for mesh";

/// 单次流式重建最多打印几条重传失败。
pub const REPLACE_MESH_REPORT_LIMIT: usize = 4;

/// 两个人形实体并排时额外的「个人空间」(米)。人不会贴着走。
pub const PEDESTRIAN_PERSONAL_SPACE: f32 = 0.18;

/// 回归测试:走路 / 冲刺必须选中两个不同的速度上限。
///
/// 取代原先两条软边界消息(`T_NO_SOFT_LIMIT` / `T_SOFT_SPEED_INSIDE`)——
/// 流式世界没有边界,那两条断言恒为真,测不出任何东西。
pub const T_WALK_SPEED_PICK: &str = "走路必须选中 WALK_SPEED,实得 {got},应为 {want}";

pub const T_RUN_SPEED_PICK: &str = "按住 Shift 必须选中 RUN_SPEED,实得 {got},应为 {want}";

pub const T_RUN_FASTER_THAN_WALK: &str = "冲刺必须明显快于走路:实测比值 {ratio},下限 {min}";

/// 走 / 跑速度上限之比的下限。
///
/// 取 1.5(真实值 8.4/4.6 = 1.83):远低于 1.83 留出浮点余量,又足够高 ——
/// 「两者被压成同一个值」时比值是 1.0,这条断言能抓住。
pub const RUN_OVER_WALK_MIN: f32 = 1.5;

/// 回归测试:附近有伴时行人必须能开始聊天。
pub const T_PEDS_GATHER_AND_TALK: &str = "附近有伴时行人必须能开始聊天";

/// 回归测试:聊天必须会结束并进入冷却。
pub const T_PEDS_TALK_ENDS: &str = "聊天必须会结束并进入冷却";

/// 回归测试:被撞倒 / 逃跑的行人���参与聊天。
pub const T_PEDS_DOWNED_NO_CHAT: &str = "被撞倒或逃跑的行人不该聊天";

/// 回归测试:扎堆之后每人站位角必须互不相同。
pub const T_PEDS_DISTINCT_SLOTS: &str = "扎堆后每人的站位角必须互不相同";

// ---------------------------------------------------------------------------
// 手雷(第 10 条:地上的 `wep_grenade` 捡起来要有用)
// ---------------------------------------------------------------------------

/// 手雷同时能携带几颗。
pub const GRENADE_TUBES: u32 = 2;
/// 手雷爆炸中心的伤害(米,不是半径)。
pub const GRENADE_DAMAGE: f32 = 90.0;
/// 两次投掷之间的最小间隔(秒)。
pub const GRENADE_COOLDOWN: f32 = 1.2;
/// 手雷的有效投掷距离(米)。
pub const GRENADE_RANGE: f32 = 24.0;
/// HUD 上显示的手雷名。
pub const WEAPON_NAME_GRENADE: &str = "grenade";

/// HUD 上显示的徒手名。
pub const WEAPON_NAME_UNARMED: &str = "UNARMED";

/// 徒手状态:按 H 收起武器的提示。
pub const NOTICE_UNARMED: &str = "weapon holstered";

/// 没有手雷可投的提示。
pub const NOTICE_NO_GRENADE: &str = "no grenades";

/// 投出手雷后的提示。
pub const NOTICE_GRENADE_THROWN: &str = "grenade thrown";

/// 投掷手雷的出手速度(米/秒,水平)。
pub const GRENADE_THROW_SPEED: f32 = 16.0;

/// 投掷手雷的出手仰角(弧度,向上为正)。
pub const GRENADE_THROW_PITCH: f32 = 0.28;

/// 投出的手雷在空中的存活时间(秒),到时结算爆炸。
pub const GRENADE_FLIGHT_TIME: f32 = 1.1;

/// 爆炸对敌人 / 玩家的作用半径(米)。
pub const GRENADE_BLAST_RADIUS: f32 = 6.0;

/// 地图面板(按 Tab)打开时显示的标题。
pub const MAP_TITLE: &str = "VICE CITY · MAP";

/// 地图面板打开时显示的底部提示。
pub const MAP_HINT: &str = "TAB to close";

/// 地图面板的显示样式(打开态)。
pub const STYLE_MAP_PANEL: &str = "position:absolute;left:50%;top:50%;transform:translate(-50%,-50%);width:min(78vw,720px);height:min(78vh,560px);background:rgba(6,12,26,.94);border:2px solid rgba(125,252,255,.55);border-radius:8px;box-shadow:0 0 40px rgba(0,0,0,.7);color:#eaf6ff;font-family:system-ui,sans-serif;padding:18px 22px;display:flex;flex-direction:column;gap:10px;pointer-events:none";

/// 地图面板标题行。
pub const STYLE_MAP_TITLE: &str =
    "font-size:20px;letter-spacing:2px;color:#7dfcff;text-shadow:0 0 12px rgba(125,252,255,.45)";

/// 地图面板底部的坐标 / 提示行。
pub const STYLE_MAP_HINT: &str = "margin-top:auto;font-size:13px;color:#9fb4d8;letter-spacing:1px";

/// 地图面板打开时游戏画面上的遮罩(压暗,让地图读得清)。
pub const STYLE_MAP_DIM: &str =
    "position:absolute;inset:0;background:rgba(2,5,14,.55);pointer-events:none";

/// 相机在第一人称下的眼高(米):比跟随焦点略低一点,接近真实视角。
pub const FIRST_PERSON_HEIGHT: f32 = 1.62;

/// 相机在第一人称下的眼点到焦点的距离(米):0 表示焦点就是眼点。
pub const FIRST_PERSON_DISTANCE: f32 = 0.0;

/// 相机在第一人称下的俯角(弧度):正着看,略微低头。
pub const FIRST_PERSON_PITCH: f32 = 0.0;

pub const EXPECT_FALLBACK_BLOCK: &str = "fallback block must be valid";

pub const LOG_ALL_ASSETS_FAILED: &str =
    "[vcw] every asset fetch failed — using built-in fallback scene";

pub const STATUS_UPLOADING_MESHES: &str = "uploading meshes…";

pub const CREATE_VERTEX_ARRAY_FAILED: &str = "create_vertex_array failed";

pub const CREATE_SHADER_RETURNED_NULL: &str = "create_shader returned null";

pub const CREATE_PROGRAM_RETURNED_NULL: &str = "create_program returned null";

pub const READY_BUILT_IN_FALLBACK_SCENE: &str = "ready (built-in fallback scene)";

pub const FALLING_BACK_TO_BUILT_IN_SCENE: &str = "falling back to built-in scene…";

pub const DRAG_ORBIT_WHEEL_PINCH_ZOOM_WASD_PAN_R_RESET: &str = "WASD walk · SHIFT run · DRAG orbit camera · WHEEL zoom · F enter/exit vehicle · TAB free-look · T time of day · R reset";

pub const NO_RENDERING_BACKEND_AVAILABLE_WEBGL2_AND_CA: &str =
    "No rendering backend available (WebGL2 and Canvas2D both failed).";

pub const PART_TORSO: &str = "torso";

pub const PART_HEAD: &str = "head";

pub const PART_HAIR: &str = "hair";

pub const PART_UPPER_ARM_L: &str = "upper_arm_L";

pub const PART_LOWER_ARM_L: &str = "lower_arm_L";

pub const PART_UPPER_ARM_R: &str = "upper_arm_R";

pub const PART_LOWER_ARM_R: &str = "lower_arm_R";

pub const PART_UPPER_LEG_L: &str = "upper_leg_L";

pub const PART_LOWER_LEG_L: &str = "lower_leg_L";

pub const PART_UPPER_LEG_R: &str = "upper_leg_R";

pub const PART_LOWER_LEG_R: &str = "lower_leg_R";

pub const PART_SHOE_L: &str = "shoe_L";

pub const PART_SHOE_R: &str = "shoe_R";

pub const NOTICE_HEALTH: &str = "+35 HP · health pack";

pub const NOTICE_CASH: &str = "+$250 · cash stack";

pub const NOTICE_WEAPON: &str = "picked up · ";

pub const NOTICE_ENTER: &str = "press WASD to drive · F to exit";

pub const NOTICE_EXIT: &str = "left the vehicle";
pub const NOTICE_NO_CAR: &str = "no vehicle nearby";
pub const NOTICE_ARMOR: &str = "+50 armor · armor vest";
pub const NOTICE_AMMO: &str = "+60 rounds · ammo box";
pub const NOTICE_MARKER: &str = "mission marker reached";

pub const KEYF: &str = "KeyF";

pub const KEYTAB: &str = "Tab";

pub const HUD_TITLE: &str = "VICE CITY WEB";

pub const HUD_DRIVING: &str = "DRIVING";

pub const HUD_ON_FOOT: &str = "ON FOOT";

pub const HUD_THIRD_PERSON: &str = "third-person";

/// 第一人称(按 V 切过去)时 HUD 显示的模式标签。
///
/// **为什么必须有这个常量:**以前 `!third_person` 那一支一律输出
/// `"orbit"`,于是 V 进第一人称后 HUD 写的是「orbit」—— 标签与实际
/// 渲染形态对不上(实测 V 之后 `camDist 8.2 -> 0`、无人身、只有准星,
/// 那明显是第一人称,不是环绕机位)。读数的人只能凭标签猜模式,猜错。
///
/// 判据仍是 `third_person` 这一个布尔量:它同时决定相机走
/// `update_camera`(第三人称)还是直接早退(第一人称)、以及近处剔除半径,
/// 所以它就是「当前到底是什么视角」的唯一真相源,不需要另设一个模式枚举。
pub const HUD_FIRST_PERSON: &str = "first-person";

pub const NO_WINDOW: &str = "no window";

pub const PLAYER_PART_EXPECT: &str = "ped_suit part expansion failed";

pub const JSON_NULL: &str = "null";

pub const PART_ARM: &str = "arm";

pub const PART_LEG: &str = "leg";

pub const CHAR_BACKSLASH: &str = "\\";

pub const CHAR_QUOTE: &str = "\"";

pub const JSON_SLASH: &str = "/";

/// 调试快照 JSON 的左括号(硬编码字符串规则要求进常量表)。
pub const DEBUG_OPEN: &str = "{";

/// 调试快照 JSON 的右括号(硬编码字符串规则要求进常量表)。
pub const DEBUG_CLOSE: &str = "}";

/// 白天相位的名字。
pub const PHASE_NOON: &str = "NOON";

/// 黄昏相位的名字。
pub const PHASE_DUSK: &str = "DUSK";

/// 夜间相位的名字。
pub const PHASE_NIGHT: &str = "NIGHT";

/// 验收调试钩子挂在 `window` 上的属性名。
pub const DEBUG_HOOK_NAME: &str = "__vcw";

/// 第二套验收钩子的属性名(只放「角色可见性」这一组字段)。
///
/// 与 [`DEBUG_HOOK_NAME`] 并存而不是改名:前者是全量快照,后者是给
/// 验收脚本直接判断「角色到底画没画」的最小子集 —— 两者同时刷新,
/// 任何一边失效都不会让另一边的读取静默返回 `undefined`。
pub const DEBUG_VISIBILITY_HOOK_NAME: &str = "__VCW_DEBUG__";

/// 绘制缓冲缩放的 query 参数名(`?res=0.5`,不含 `?` / `=`)。
pub const RES_PARAM: &str = "res";
/// 缺省绘制缓冲缩放;`0.0` = 不额外缩放,跟随 devicePixelRatio。
pub const DEFAULT_RES: f32 = 0.0;
#[cfg(test)]

/// 单元测试断言文案:射线必须命中挡在视线中间的墙。
pub const T_RAY_MUST_HIT: &str = "墙在射线路径上,必须命中";
#[cfg(test)]

/// 单元测试断言文案:命中距离应接近「墙距焦点 - 探针半径」。
pub const T_RAY_DISTANCE: &str = "命中距离应接近 3 m 减去探针半径,实际 {distance}";
#[cfg(test)]

/// 单元测试断言文案:被挡时必须报告遮挡。
pub const T_OCCLUSION_REPORTED: &str = "墙挡住了视线,必须报告遮挡";
#[cfg(test)]

/// 单元测试断言文案:遮挡时允许距离必须短于期望距离。
pub const T_OCCLUSION_SHORTER: &str = "遮挡时允许距离必须短于期望距离,allowed={allowed}";
#[cfg(test)]

/// 单元测试断言文案:允许距离必须为正,否则相机会被推到玩家背后。
pub const T_OCCLUSION_POSITIVE: &str =
    "允许距离必须为正,否则相机会被推到玩家背后,allowed={allowed}";
#[cfg(test)]

/// 单元测试断言文案:视线无遮挡时必须返回 `None`。
pub const T_NO_OCCLUSION: &str = "视线无遮挡时必须返回 None,否则相机会无缘无故短一截";
#[cfg(test)]

/// 单元测试断言文案:贴墙余量必须为正。
pub const T_SKIN_POSITIVE: &str = "贴墙余量必须为正,否则相机会贴死在墙面里";
#[cfg(test)]

/// 单元测试断言文案:贴墙余量过大等于没有回避。
pub const T_SKIN_BOUNDED: &str = "贴墙余量过大等于没有回避,当前 {OCCLUSION_SKIN}";
#[cfg(test)]

/// 单元测试断言文案:拉近后距离必须变小。
pub const T_PULL_IN: &str = "拉近后距离必须变小,实际 {after}";
#[cfg(test)]

/// 单元测试断言文案:单帧内不允许一步到位,否则相机会抽搐。
pub const T_PULL_IN_SMOOTH: &str = "一帧(16 ms)不允许一步到位,否则会抽搐,实际 {after}";
#[cfg(test)]

/// 单元测试断言文案:距离硬下限必须兜住。
pub const T_FLOOR_HELD: &str = "距离硬下限必须兜住,实际 {distance} vs 下限 {floor}";
#[cfg(test)]

/// 单元测试断言文案:遮挡消失后必须平滑回到期望距离。
pub const T_RECOVERS: &str = "遮挡消失后必须平滑回到期望距离,实际 {distance}";
#[cfg(test)]

/// 单元测试断言文案:中心线不该打到画面侧面的墙。
pub const T_CENTRE_MISSES: &str = "中心线不该打到侧面的墙,实际 {centre:?}";
#[cfg(test)]

/// 单元测试断言文案:侧线必须发现画面侧面的墙。
pub const T_SIDE_HITS: &str = "侧线必须发现这堵墙,否则相机会停在楼里";

/// WebGL 上下文的 `alpha` 属性名。
pub const GL_ATTR_ALPHA: &str = "alpha";

/// WebGL 上下文的 `depth` 属性名。
pub const GL_ATTR_DEPTH: &str = "depth";

/// WebGL 上下文的 `stencil` 属性名。
pub const GL_ATTR_STENCIL: &str = "stencil";

/// WebGL 上下文的 `antialias` 属性名。
pub const GL_ATTR_ANTIALIAS: &str = "antialias";

/// WebGL 上下文的 `preserveDrawingBuffer` 属性名。
pub const GL_ATTR_PRESERVE_DRAWING_BUFFER: &str = "preserveDrawingBuffer";

/// 调试快照里「没有任何批次」时的占位描述。
pub const NO_BATCHES: &str = "no batches";

/// 单次启动里 `upload_mesh` 最多打印几条错误。
///
/// 30 个资产全挂时会刷 30 行同样的栈,反而把真正的第一条挤走;超过这个
/// 数量只报总数,细节靠 `of N meshes failed` 那一行。
pub const MESH_UPLOAD_ERROR_LIMIT: usize = 5;

/// 手写 JSON 时用的小分隔符(避免为此引入 `serde_json`)。
pub const JSON_COMMA: &str = ",";

/// 验收探针里「角色头顶」的假想高度(米)。
///
/// `ped_suit` 的资产包围盒是 1.75 m,但探针只需要一个稳定的、可复算的
/// 高度来判断角色在画面里占多少像素,所以直接用这个常量,不去查资产。
pub const PED_SCREEN_PROBE_HEIGHT: f32 = 1.75;

/// `MeshAssetGpu.vertices` 里一个顶点的 f32 数量。
///
/// 布局:position(3) + normal(3) + color(3) + emissive(3) = 12。
/// 验收探针按这个步长切顶点算包围盒,常量必须和 `build_gpu_mesh` 一致。
pub const GPU_STRIDE_FLOATS: usize = 12;

/// 判定「角色可见」时允许的**最大**屏幕高度占比(%)。
///
/// 取 45:第三人称相机 8.2 m 之外、FOV 75° 时,1.75 m 高的角色约占
/// 画布高度的 13–15%,留足余量。上限卡在 45% 是因为「角色糊满整屏」
/// 正是 GPU mesh 索引错位最典型的症状 —— 那种状态下投影盒仍然完整
/// 落在画布内,只判「投影成功」会误报成可见。
pub const CHAR_VISIBLE_MAX_SCREEN_PCT: f64 = 45.0;

/// 验收探针:报出玩家周围这个半径(米)内的静态碰撞体。
///
/// 取 6 m:够覆盖「玩家贴面站着」这一情形(距离 0.0x m 就能看见),
/// 又不会把整条街的碰撞体全倒进 JSON。半径太小的坏处是
/// 「陷进楼里、被从内部推开」时报告为空,查不到是谁推的。
pub const NEAR_SHAPE_REPORT_RADIUS: f32 = 6.0;

/// 验收探针:最多报几个最近的静态碰撞体。
///
/// 取 8:够看清「一堵楼 + 几根行道树」这种典型组合,又不会让
/// `__VCW_DEBUG__` 膨胀到每帧都要反序列化一大串。
pub const NEAR_SHAPE_REPORT_COUNT: usize = 8;

/// 验收探针:单段室内隔墙的 JSON 格式串。
///
/// 与静态碰撞体分开报(`nearShapes` vs `nearInteriors`):两者住在**互不
/// 相交的两个碰撞世界**里,所以「静态报告为空而玩家被卡住」正是
/// 「被隔墙推开」的指纹。`embed` 字段标出「圆心已经陷进墙里」——
/// 那会让 `push_out_aabb` 走「点在盒内」分支,把玩家按 `radius + slack`
/// 整个弹出去,是「有速度、无位移」的直接成因。
/// 验收探针:单段隔墙 JSON 的键名骨架(数值用 `REPORT_TOKEN_*` 占位)。
///
/// §1.3c 要求 JSON 键名住在 `const.rs`,但 `format!` 只接受字符串**字面
/// 量** —— 两者只能在这里会合:键名走常量、数值走 `replace` 填回去。
/// 占位符全是 JSON 非法字符,保证填之前不可能是合法输出。
pub const INTERIOR_REPORT_KEYS: &str =
    "{\"d\":@D@,\"embed\":@E@,\"y\":[@Y0@,@Y1@],\"c\":[@CX@,@CZ@],\"h\":[@HX@,@HZ@]}";
/// `INTERIOR_REPORT_KEYS` 里距离的占位符。
pub const REPORT_TOKEN_D: &str = "@D@";
/// `INTERIOR_REPORT_KEYS` 里「是否陷进墙里」的占位符。
pub const REPORT_TOKEN_EMBED: &str = "@E@";
/// `INTERIOR_REPORT_KEYS` 里墙脚高度的占位符。
pub const REPORT_TOKEN_Y0: &str = "@Y0@";
/// `INTERIOR_REPORT_KEYS` 里墙顶高度的占位符。
pub const REPORT_TOKEN_Y1: &str = "@Y1@";
/// `INTERIOR_REPORT_KEYS` 里墙中心 X 的占位符。
pub const REPORT_TOKEN_CX: &str = "@CX@";
/// `INTERIOR_REPORT_KEYS` 里墙中心 Z 的占位符。
pub const REPORT_TOKEN_CZ: &str = "@CZ@";
/// `INTERIOR_REPORT_KEYS` 里墙 X 半长的占位符。
pub const REPORT_TOKEN_HX: &str = "@HX@";
/// `INTERIOR_REPORT_KEYS` 里墙 Z 半长的占位符。
pub const REPORT_TOKEN_HZ: &str = "@HZ@";
// ===========================================================================
// 实时光照增强:阴影 / SSAO / SSR / bloom
//
// §1.3c 要求所有字符串字面量集中在 const.rs,GLSL 里的 uniform 名与
// program 标签同样算字符串,一并收在这里。
// ===========================================================================

/// 阴影贴图的 uniform 名。
pub const U_SHADOW_MAP: &str = "u_shadow_map";

/// 阴影矩阵(世界 → 光空间裁剪坐标)的 uniform 名。
pub const U_SHADOW_MATRIX: &str = "u_shadow_matrix";

/// 阴影参数 `vec4(strength, pcf_radius, depth_bias, normal_offset)` 的 uniform 名。
pub const U_SHADOW_PARAMS: &str = "u_shadow_params";

/// 一个阴影纹素覆盖的世界尺寸(米)uniform 名。
pub const U_SHADOW_TEXEL: &str = "u_shadow_texel";

/// 半球环境光的半球权重 uniform 名(0 = 纯单色 ambient,1 = 纯半球)。
pub const U_AMBIENT_HEMI: &str = "u_ambient_hemi";

/// SSR 的最大追踪距离(米)uniform 名。
pub const U_SSR_MAX_DIST: &str = "u_max_dist";

/// SSR 的步数 uniform 名。
pub const U_SSR_STEPS: &str = "u_steps";

/// 湿度(0 = 干,1 = 湿路面)uniform 名。
pub const U_WETNESS: &str = "u_wetness";

/// 湿度衰减高度 uniform 名。
pub const U_WET_HEIGHT: &str = "u_wet_height";

/// 顶点接触 AO 高度衰减 uniform 名。
pub const U_AO_HEIGHT: &str = "u_ao_height";

/// 顶点接触 AO 最深压暗系数 uniform 名。
pub const U_AO_FLOOR: &str = "u_ao_floor";

/// 天光(半球上半球)环境色 uniform 名。
pub const U_SKY_AMBIENT: &str = "u_sky_ambient";

/// 地面反弹色(半球下半球)uniform 名。
pub const U_GROUND_AMBIENT: &str = "u_ground_ambient";

/// 主场景离屏目标缺失时的错误信息。
pub const SCENE_TARGET_MISSING: &str = "scene render target missing";

/// bloom 模糊的采样展开(以 texel 为单位)。
pub const BLOOM_BLUR_SPREAD: f32 = 1.6;

/// bloom 合成的最小增益(夜晚关灯时也别完全变成硬边色块)。
pub const BLOOM_MIN_GAIN: f32 = 0.35;

/// G-buffer pass 的视图矩阵 uniform 名。
pub const U_VIEW: &str = "u_view";

/// 远裁剪面 uniform 名(深度归一化用)。
pub const U_FAR_PLANE: &str = "u_far_plane";

/// bloom 模糊方向 uniform 名。
pub const U_BLOOM_DIR: &str = "u_bloom_dir";

/// 胶片颗粒的时间种子 uniform 名。
pub const U_TIME: &str = "u_time";

/// 采样数 uniform 名(SSAO)。
pub const U_SAMPLES: &str = "u_samples";

/// AO 模糊半径 uniform 名。
pub const U_BLUR_RADIUS: &str = "u_radius";

/// SSAO 的 texel 尺寸 uniform 名。
pub const U_TEXEL_SIZE: &str = "u_texel_size";

/// bloom 模糊的输入贴图 uniform 名。
pub const U_BLOOM_MAP: &str = "u_bloom_map";

/// 贴图分配失败时的错误信息。
pub const CREATE_TEXTURE_FAILED: &str = "create_texture failed";

/// framebuffer 分配失败时的错误信息。
pub const CREATE_FRAMEBUFFER_FAILED: &str = "create_framebuffer failed";

/// FBO 不完整时的错误信息。
pub const FRAMEBUFFER_INCOMPLETE: &str = "framebuffer incomplete";

/// WebGL2 语义的「FBO 完整」状态码。
///
/// web-sys 的 `WebGl2RenderingContext::FRAMEBUFFER_COMPLETE` 是 36053
/// (WebGL 1 时代的编号),而 WebGL2 的 `checkFramebufferStatus` 在真正
/// 完整时返回 36054。只比前者会让这个判断恒为假,于是每个 FBO 都被误判
/// 成不完整、每帧回退软件渲染、画面全黑。两个值都接受。
pub const FRAMEBUFFER_COMPLETE_WEBGL2_OFFSET: u32 = 36054;

/// SSAO / AO 模糊的强度 uniform 名。
pub const U_AO_STRENGTH: &str = "u_ao_strength";

/// SSAO 结果贴图的 uniform 名。
pub const U_AO_MAP: &str = "u_ao_map";

/// SSAO 采样的 G-buffer 贴图 uniform 名。
pub const U_GBUFFER: &str = "u_gbuffer";

/// 重建视空间位置用的投影参数 `(tan_half_fov_x, tan_half_fov_y)` uniform 名。
pub const U_PROJ_PARAMS: &str = "u_proj_params";

/// SSAO 世界半径 uniform 名。
pub const U_AO_RADIUS: &str = "u_radius";

/// SSAO 采样幂次 uniform 名(越大对比越硬)。
pub const U_AO_POWER: &str = "u_power";

/// AO 模糊用的 AO 贴图 uniform 名。
pub const U_AO_BLUR_MAP: &str = "u_ao_map";

/// 主场景颜色贴图的 uniform 名(SSR / bloom / 合成共读)。
pub const U_SCENE_COLOR: &str = "u_scene_color";

/// SSR 结果贴图的 uniform 名。
pub const U_SSR_MAP: &str = "u_ssr_map";

/// SSR 强度 uniform 名。
pub const U_SSR_STRENGTH: &str = "u_ssr_strength";

/// bloom 强度 uniform 名。
pub const U_BLOOM_STRENGTH: &str = "u_bloom_strength";

/// bloom 亮度阈值 uniform 名。
pub const U_BLOOM_THRESHOLD: &str = "u_bloom_threshold";

/// 色调分级的 lift(黑场染色)uniform 名。
pub const U_GRADE_LIFT: &str = "u_grade_lift";

/// 色调分级的 gamma(中间调)uniform 名。
pub const U_GRADE_GAMMA: &str = "u_grade_gamma";

/// 色调分级的 gain(亮场染色)uniform 名。
pub const U_GRADE_GAIN: &str = "u_grade_gain";

/// 暗角强度 uniform 名。
pub const U_VIGNETTE: &str = "u_vignette";

/// 胶片颗粒强度 uniform 名。
pub const U_GRAIN: &str = "u_grain";

/// 阴影 program 的标签(出错时的可读名字)。
pub const PROGRAM_SHADOW: &str = "shadow";

/// 法线 + 深度预渲染 program 的标签。
pub const PROGRAM_GBUFFER: &str = "gbuffer";

/// SSAO program 的标签。
pub const PROGRAM_SSAO: &str = "ssao";

/// AO 双边模糊 program 的标签。
pub const PROGRAM_AO_BLUR: &str = "ao_blur";

/// SSR program 的标签。
pub const PROGRAM_SSR: &str = "ssr";

/// bloom 亮度提取 program 的标签。
pub const PROGRAM_BRIGHT: &str = "bright";

/// bloom 高斯模糊 program 的标签(水平 / 垂直共用一个 program)。
pub const PROGRAM_MAIN: &str = "main";
pub const PROGRAM_GLOW: &str = "glow";
pub const PROGRAM_BLUR: &str = "blur";

/// 合成(色调分级 + 暗角 + 颗粒)program 的标签。
pub const PROGRAM_COMPOSITE: &str = "composite";

/// 阴影贴图边长(texel)。
///
/// 2048 是「正午地面上一辆车(4 m 长)要投出可辨认的影子」的下限:
/// shadow frustum 半宽取 [`SHADOW_HALF_EXTENT`],于是每 texel 约
/// 90 / 2048 ≈ 4.4 cm,配 3×3 PCF 得到约 13 cm 的半影。
pub const SHADOW_MAP_SIZE: u32 = 2048;

/// 阴影正交视锥的半宽 / 半深(米)。
///
/// 90 m 覆盖第三人称相机能看到的近景街区:玩家周围 45 m 内的一切都会
/// 投出影子,再远的东西丢掉影子反而没人会注意到(它们已经被雾吃掉大半)。
pub const SHADOW_HALF_EXTENT: f32 = 45.0;

/// 阴影光源沿光方向后撤的距离(米)。
///
/// 取 3 倍半宽(135 m),让视锥的近平面落在城市最高楼(约 40 m)之上,
/// 避免高楼顶被切掉。
pub const SHADOW_LIGHT_DISTANCE: f32 = 135.0;

/// 阴影正交视锥的近裁剪距离(米)。
pub const SHADOW_NEAR: f32 = 1.0;

/// 阴影正交视锥的远裁剪距离(米)。
pub const SHADOW_FAR: f32 = 320.0;

/// 法线 + 深度 G-buffer 的分辨率缩放(相对主画面)。
///
/// 取 1.0 全分辨率:SSAO 的边缘质量直接由这张图的分辨率决定,降到半分辨率
/// 之后墙角处的暗部会明显发虚。G-buffer 只写两个通道组,带宽不高。
pub const GBUFFER_SCALE: f32 = 1.0;

/// SSAO 的分辨率缩放(相对 G-buffer)。
///
/// SSAO 之后还要做一次双边模糊把噪声抹平,所以半分辨率在视觉上几乎无损,
/// 却省掉四分之三的采样开销 —— 在软件光栅(SwiftShader)上这是必需的。
pub const SSAO_SCALE: f32 = 0.5;

/// SSR 的分辨率缩放(相对主画面)。
pub const SSR_SCALE: f32 = 0.5;

/// bloom 的分辨率缩放(相对主画面)。
pub const BLOOM_SCALE: f32 = 0.5;

/// SSAO 的世界半径(米)。
///
/// 1.6 m 刚好覆盖「楼与楼之间」「车与地面之间」这种一臂宽的接触关系;
/// 再大就会把整条街的墙根一起压暗,看起来像脏雾而不是遮蔽。
pub const SSAO_RADIUS: f32 = 1.6;

/// SSAO 的采样幂次(越大对比越硬)。
pub const SSAO_POWER: f32 = 2.2;

/// SSAO 的采样数(固定循环,不做抖动)。
pub const SSAO_SAMPLES: i32 = 8;

/// AO 双边模糊的半径(以半分辨率 texel 为单位)。
pub const AO_BLUR_RADIUS: i32 = 4;

/// SSR 的光线步数。
pub const SSR_STEPS: i32 = 20;

/// SSR 的最大追踪距离(米)。
pub const SSR_MAX_DIST: f32 = 28.0;

/// 湿地面反射率的判定上界:世界高度高于这个值就不是路面。
///
/// 路面 y = 0、人行道 y = 0.14、地块 y = 0.16 —— 用高度把「积水只积在
/// 低洼的车行道上」这件事表达出来,不需要给每个面加材质通道。
pub const WET_SURFACE_MAX_HEIGHT: f32 = 0.06;

/// 烘焙进顶点色的「向下表面接触 AO」最大压暗比例。
///

/// 烘焙接触 AO 生效的高度上限(米)—— 只有贴地的面才被压暗。
pub const BAKED_CONTACT_AO_HEIGHT: f32 = 1.2;

/// 顶点接触 AO 在 y=0 处的最深压暗系数(0.34 = 压到 66% 亮度)。
pub const CONTACT_SHADOW_FLOOR: f32 = 0.66;

/// `light_dir.y` 超过这个值就认为视线与 up 共线,必须换 up 向量。
pub const SHADOW_DEGENERATE_UP_Y: f32 = 0.98;

/// 阴影深度偏置(以纹素为单位)。
///
/// 取 1.6:2048 的阴影贴图上 1.6 个纹素约 7 cm,足以压掉地面自阴影的
/// 痤疮又不会明显让影子「浮」起来(Peter-Panning)。
pub const SHADOW_DEPTH_BIAS_TEXELS: f32 = 1.6;

/// 阴影法线偏移(以纹素为单位)。
///
/// 取 1.4:斜率缩放之外再叠一层法线偏移,是自阴影痤疮的双保险。
pub const SHADOW_NORMAL_OFFSET_TEXELS: f32 = 1.4;

/// 阴影 PCF 的采样半径(以纹素为单位)。
///
/// 3×3 核 → 半径 1.0 刚好覆盖相邻纹素;再大只会把半影抹得更宽、
/// 看起来发虚,不会更「真实」。
pub const SHADOW_PCF_RADIUS: f32 = 1.0;

/// 主 pass 的色调分级暗角基准强度(乘以相位的 `vignette`)。
pub const VIGNETTE_BASE: f32 = 0.55;

/// 胶片颗粒的基准强度(乘以相位的 `grain`)。
pub const GRAIN_BASE: f32 = 0.035;

/// bloom 的强度(乘以相位的 `ssr_strength` 之外的独立常数)。
pub const BLOOM_STRENGTH: f32 = 0.85;

/// bloom 亮度提取的阈值(线性空间)。
pub const BLOOM_THRESHOLD: f32 = 0.62;

/// ===========================================================================
/// 战斗 / 通缉 / 任务系统常量(本作新增)
/// ===========================================================================

// ---- DOM id ----

/// 生命条填充层。
pub const ID_HEALTH_BAR: &str = "vcw-health-bar";
/// 护甲条填充层。
pub const ID_ARMOR_BAR: &str = "vcw-armor-bar";
/// 通缉星数容器。
pub const ID_WANTED: &str = "vcw-wanted";
/// 弹药计数文本。
pub const ID_AMMO: &str = "vcw-ammo";
/// 当前任务 / 提示文本。
pub const ID_MISSION: &str = "vcw-mission";
/// 现金文本。
pub const ID_CASH: &str = "vcw-cash";
/// 小地图 canvas。
pub const ID_MINIMAP: &str = "vcw-minimap";
/// 准星命中标记。
pub const ID_HITMARKER: &str = "vcw-hitmarker";
/// 屏幕中央准星。
pub const ID_CROSSHAIR: &str = "vcw-crosshair";

/// 地图面板的 id(Tab 打开)。
pub const ID_MAP_PANEL: &str = "vcw-map";

/// 地图面板底部坐标 / 提示行的 id。
pub const ID_MAP_DETAIL: &str = "vcw-map-detail";

/// 地图面板标题的 id。
pub const ID_MAP_TITLE: &str = "vcw-map-title";

/// 开火时的事件名。
pub const EVENT_MOUSEDOWN: &str = "mousedown";
/// 松开左键时的事件名。
pub const EVENT_MOUSEUP: &str = "mouseup";
/// 鼠标移动事件名(单独绑一条,用于悬停瞄准)。
pub const EVENT_MOUSEMOVE: &str = "mousemove";
/// 防止右键菜单吃掉右键。
pub const EVENT_CONTEXTMENU: &str = "contextmenu";

/// 换弹键。
pub const KEY_RELOAD: &str = "KeyC";
/// 切回手枪。
pub const DIGIT1: &str = "Digit1";
/// 切冲锋枪。
pub const DIGIT2: &str = "Digit2";
/// 切球棒。
pub const DIGIT3: &str = "Digit3";
/// 接 / 交任务。
pub const KEY_MISSION: &str = "KeyJ";

/// 生命条的颜色。
pub const COLOR_HEALTH: &str = "linear-gradient(90deg,#ff4d6d,#ffb36b)";
/// 护甲条的颜色。
pub const COLOR_ARMOR: &str = "linear-gradient(90deg,#7dfcff,#5b8cff)";
/// 小地图底色。
pub const COLOR_MINIMAP_BG: &str = "#0b1020";
/// 小地图道路线颜色。
pub const COLOR_MINIMAP_ROAD: &str = "#2b3a5c";
/// 小地图里的玩家三角形颜色。
pub const COLOR_MINIMAP_PLAYER: &str = "#ff7ad9";
/// 小地图里的敌人圆点颜色。
pub const COLOR_MINIMAP_ENEMY: &str = "#ff4d6d";
/// 小地图里的任务点颜色。
pub const COLOR_MINIMAP_OBJECTIVE: &str = "#7dfcff";

/// HUD 状态条的通用样式(容器)。
pub const STYLE_STATUS: &str = "position:absolute;left:14px;bottom:74px;width:230px;display:flex;flex-direction:column;gap:5px;pointer-events:none;font-family:ui-monospace,monospace";
/// 生命 / 护甲条的轨道样式。
pub const STYLE_BAR_TRACK: &str =
    "width:100%;height:9px;border-radius:999px;background:rgba(255,255,255,.14);overflow:hidden";
/// 弹药 / 现金行的样式。
pub const STYLE_STAT_ROW: &str = "display:flex;flex-direction:row;align-items:center;justify-content:space-between;font-size:11px;color:#cfe9ff;text-shadow:0 1px 3px rgba(0,0,0,.85)";
/// 任务提示行的样式。
pub const STYLE_MISSION: &str = "position:absolute;right:14px;top:56px;max-width:340px;padding:7px 11px;border-radius:6px;background:rgba(10,12,22,.62);border:1px solid rgba(125,252,255,.3);font-family:ui-monospace,monospace;font-size:12px;line-height:1.5;color:#e6f6ff;pointer-events:none;white-space:pre-line";
/// 通缉星容器的样式。
pub const STYLE_WANTED: &str = "position:absolute;right:14px;top:14px;display:flex;flex-direction:row;gap:3px;font-size:17px;line-height:1;pointer-events:none;text-shadow:0 1px 4px rgba(0,0,0,.9)";
/// 单颗星的样式。
pub const STYLE_STAR: &str = "width:19px;height:19px;display:flex;align-items:center;justify-content:center;font-size:19px;line-height:1";
/// 准星的样式(一个十字,不挡视线)。
pub const STYLE_CROSSHAIR: &str = "position:absolute;left:50%;top:50%;width:20px;height:20px;margin:-10px 0 0 -10px;pointer-events:none;opacity:.85";
/// 准星四臂的样式。
pub const STYLE_CROSSHAIR_ARM: &str =
    "position:absolute;background:rgba(125,252,255,.95);box-shadow:0 0 3px rgba(0,0,0,.9)";
/// 命中标记的样式。
pub const STYLE_HITMARKER: &str = "position:absolute;left:50%;top:50%;width:26px;height:26px;margin:-13px 0 0 -13px;pointer-events:none;opacity:0;transition:opacity .12s linear";
/// 小地图 canvas 的样式。
pub const STYLE_MINIMAP: &str = "position:absolute;right:14px;bottom:14px;width:150px;height:150px;border-radius:8px;border:1px solid rgba(125,252,255,.32);background:#0b1020;pointer-events:none";

/// 子弹命中敌人的提示。
pub const NOTICE_HIT: &str = "hit";
/// 击杀提示。
pub const NOTICE_KILL: &str = "target down";
/// 换弹提示。
pub const NOTICE_RELOADED: &str = "reloaded";
/// 弹药打空提示。
pub const NOTICE_EMPTY: &str = "click · out of ammo";
/// 通缉提示。
pub const NOTICE_WANTED: &str = "WANTED";
/// 甩掉通缉的提示。
pub const NOTICE_CLEARED: &str = "you lost them";
/// 玩家被击倒的提示。
pub const NOTICE_WASTED: &str = "WASTED";
/// 任务开始的提示前缀。
pub const NOTICE_MISSION_START: &str = "job: ";
/// 任务完成提示前缀。
pub const NOTICE_MISSION_DONE: &str = "job done +$";
/// 抢车的提示。
pub const NOTICE_JACKED: &str = "vehicle jacked";
/// 抢走警车的提示。
pub const NOTICE_STOLE_POLICE: &str = "police cruiser jacked";

/// 任务完成奖励的文案模板里的货币符号。
pub const CASH_SIGN: &str = "$";

// ---- 武器数值 ----

/// 手枪弹匣容量。
pub const PISTOL_MAGAZINE: u32 = 12;
/// 冲锋枪弹匣容量。
pub const SMG_MAGAZINE: u32 = 30;
/// 手枪射速(发/秒)。
pub const PISTOL_FIRE_RATE: f32 = 5.0;
/// 冲锋枪射速(发/秒)。
pub const SMG_FIRE_RATE: f32 = 11.0;
/// 手枪单发伤害。
pub const PISTOL_DAMAGE: f32 = 26.0;
/// 冲锋枪单发伤害。
pub const SMG_DAMAGE: f32 = 15.0;
/// 球棒单次挥击伤害。
pub const BAT_DAMAGE: f32 = 55.0;
/// 球棒挥击间隔(秒)。
pub const BAT_COOLDOWN: f32 = 0.55;
/// 球棒攻击距离(米)。
pub const BAT_RANGE: f32 = 2.6;
/// 枪械最大射程(米):超过这个距离射线不再判定。
pub const GUN_RANGE: f32 = 90.0;
/// 距离衰减:到最大射程时伤害乘的系数。
pub const FALLOFF_MIN: f32 = 0.35;
/// 距离衰减的过渡距离(米):超过后开始线性衰减。
pub const FALLOFF_START: f32 = 22.0;
/// 换弹耗时(秒)。
pub const RELOAD_TIME: f32 = 1.1;
/// 敌人被击杀后多久消失(秒)。
pub const DEATH_FADE_TIME: f32 = 2.0;

// ---- 敌人数值 ----

/// 警察敌人初始血量。
pub const ENEMY_HEALTH: f32 = 100.0;
/// 敌对 NPC 初始血量。
pub const THUG_HEALTH: f32 = 70.0;
/// 敌人移动速度(米/秒)。
pub const ENEMY_SPEED: f32 = 5.2;
/// 敌人发现玩家的最大距离(米)。
pub const ENEMY_SIGHT: f32 = 46.0;
/// 敌人开火的最大距离(米)。
pub const ENEMY_FIRE_RANGE: f32 = 26.0;
/// 敌人每发伤害。
pub const ENEMY_DAMAGE: f32 = 7.5;
/// 敌人射速(发/秒)。
pub const ENEMY_FIRE_RATE: f32 = 1.6;
/// 敌人瞄准散布(弧度):太大打不准,太小压迫感过强。
pub const ENEMY_SPREAD: f32 = 0.07;
/// 敌人受击后的硬直(秒)。
pub const ENEMY_HIT_STUN: f32 = 0.18;
/// 敌人尸体掉落现金。
pub const ENEMY_CASH_DROP: f32 = 60.0;
/// 敌人碰撞半径(米)。
pub const ENEMY_RADIUS: f32 = 0.45;
/// 敌人最低血量:低于这个值会逃跑。
pub const ENEMY_FLEE_HEALTH: f32 = 22.0;
/// 逃跑的移动速度倍率。
pub const ENEMY_FLEE_SPEED_GAIN: f32 = 1.25;

// ---- 玩家受击 / 死亡 ----

/// 护甲的减伤比例(0.65 = 挡掉 65% 伤害)。
pub const ARMOR_ABSORB: f32 = 0.65;
/// 玩家初始护甲。
pub const MAX_ARMOR: f32 = 100.0;
/// 护甲的自然回复速度(点/秒,脱战后生效)。
pub const ARMOR_REGEN: f32 = 6.0;
/// 护甲脱战回复的延迟(秒)。
pub const ARMOR_REGEN_DELAY: f32 = 7.0;
/// 玩家最大生命值。
pub const PLAYER_MAX_HEALTH: f32 = 100.0;
/// 玩家受击后的无敌时间(秒),避免被一帧多人打空。
pub const PLAYER_HIT_INVULN: f32 = 0.45;
/// 死亡时损失的现金比例。
pub const DEATH_CASH_LOSS: f32 = 0.15;
/// 倒地后重生的时间(秒)。
pub const RESPAWN_DELAY: f32 = 3.0;

// ---- 通缉 ----

/// 通缉星上限。
pub const WANTED_MAX: u32 = 5;
/// 开一枪累积的「热度」,达到一个阈值升一颗星。
pub const WANTED_PER_SHOT: f32 = 3.0;
/// 打人累积的热度。
pub const WANTED_PER_HIT: f32 = 9.0;
/// 抢车累积的热度。
pub const WANTED_PER_JACK: f32 = 22.0;
/// 碾死行人累积的热度。
pub const WANTED_PER_RUNOVER: f32 = 30.0;
/// 每颗星所需的热度。
pub const WANTED_PER_STAR: f32 = 34.0;
/// 甩掉通缉需要的静止时间(秒)—— 不被警察看见这么久就掉一颗星。
pub const WANTED_COOLDOWN: f32 = 15.0;
/// 每颗星的警察增派人数。
pub const WANTED_COP_PER_STAR: u32 = 2;
/// 藏身区的判定半径(米):后巷 / 警局门口的圆心。
pub const HIDE_RADIUS: f32 = 4.0;
/// 站在藏身区里会累积的「降温」倍率(每秒抵消多少热度)。
pub const HIDE_COOL_RATE: f32 = 42.0;
/// 掉星的阈值进度(热度满一星后回落到这个比例以下才真正掉星)。
pub const WANTED_STEP_DOWN: f32 = 0.25;

// ---- 任务 ----

/// 任务的三个阶段。
pub const MISSION_NONE: u32 = 0;
/// 「走到某地」阶段。
pub const MISSION_GOTO: u32 = 1;
/// 「干掉某人」阶段。
pub const MISSION_KILL: u32 = 2;
/// 「把车开到某地」阶段。
pub const MISSION_DRIVE: u32 = 3;
/// 任务完成判定半径(米)。
pub const MISSION_RADIUS: f32 = 4.5;
/// 任务报酬(美元)。
pub const MISSION_REWARD: f32 = 900.0;
/// 接任务所需的最大距离(米)。
pub const MISSION_ACCEPT_RANGE: f32 = 4.0;

// ---- 行人 ----

/// 街上行人的总数上限(性能预算:每个行人 ≈ 630 三角面)。
pub const PED_COUNT: usize = 18;
/// 行人被车撞到时飞出去的速度(米/秒)。
pub const PED_RUNOVER_SPEED: f32 = 9.0;
/// 行人倒地后多久消失(秒)。
pub const PED_DOWN_TIME: f32 = 4.0;
/// 行人察觉危险的距离(米):会跑开。
pub const PED_ALERT_RADIUS: f32 = 11.0;
/// 行人逃跑的持续时间(秒)。
pub const PED_FLEE_TIME: f32 = 3.2;
/// 行人的巡航速度(米/秒)。
pub const PED_SPEED: f32 = 1.7;

// ---- 载具 ----

/// 撞到行人时判定为「碾到」的相对速度下限(米/秒)。
pub const PED_RUNOVER_MIN_SPEED: f32 = 3.2;

// ---- 小地图 ----

/// 小地图的像素边长。
pub const MINIMAP_PX: f32 = 150.0;
/// 小地图覆盖的世界半径(米)。
pub const MINIMAP_RANGE: f32 = 70.0;
/// 小地图里道路线的宽度(像素)。
pub const MINIMAP_ROAD_W: f32 = 3.0;

// ---- 战斗补充常量 ----

/// 冲锋枪开局备弹。
pub const SMG_RESERVE: u32 = 60;
/// 敌人受击闪光的时长(秒)。
pub const ENEMY_FLASH_TIME: f32 = 0.12;
/// 敌人逃跑状态的持续时间(秒)。
pub const ENEMY_FLEE_TIME: f32 = 2.6;
/// HUD 上手枪的短名。
pub const WEAPON_NAME_PISTOL: &str = "PISTOL";
/// HUD 上冲锋枪的短名。
pub const WEAPON_NAME_SMG: &str = "SMG";
/// HUD 上球棒的短名。
pub const WEAPON_NAME_BAT: &str = "BAT";
/// AI 状态:站桩。
pub const AI_STATE_IDLE: &str = "idle";
/// AI 状态:巡逻。
pub const AI_STATE_PATROL: &str = "patrol";
/// AI 状态:追击。
pub const AI_STATE_CHASE: &str = "chase";
/// AI 状态:开火。
pub const AI_STATE_ATTACK: &str = "attack";
/// AI 状态:逃跑。
pub const AI_STATE_FLEE: &str = "flee";
/// AI 状态:死亡。
pub const AI_STATE_DEAD: &str = "dead";

/// 敌人受击后的色调(闪烁)。
pub const TINT_ENEMY_HURT: [f32; 3] = [2.4, 0.5, 0.6];
/// 敌人正常色调(警察偏蓝)。
pub const TINT_POLICE: [f32; 3] = [0.55, 0.75, 1.25];
/// 敌人正常色调(混混偏暖)。
pub const TINT_THUG: [f32; 3] = [1.25, 0.72, 0.62];
/// 敌人临死前最后一口气的色调。
pub const TINT_ENEMY_FLEE: [f32; 3] = [1.6, 1.1, 0.55];
/// 车辆损坏后的色调(耐久越低越暗)。
pub const TINT_VEHICLE_MIN: [f32; 3] = [0.55, 0.42, 0.42];

/// 敌人巡逻相位推进速率(弧度/秒)。
pub const ENEMY_WANDER_RATE: f32 = 0.55;
/// 敌人重新选巡逻点的时间(秒)。
pub const ENEMY_PATROL_RESELECT: f32 = 3.4;

/// 任务 A 的标题。
pub const MISSION_TITLE_A: &str = "check in at the corner";
/// 任务 B 的标题。
pub const MISSION_TITLE_B: &str = "drive the plaza loop";
/// 任务 C 的标题。
pub const MISSION_TITLE_C: &str = "clear the block";
/// 手上没有任务时 HUD 里的提示。
pub const MISSION_IDLE: &str = "no job — press J at a marker";

/// 瞄准反投影使用的深度(米):决定鼠标移动在世界里对应多大的横向位移。
pub const AIM_DEPTH: f32 = 26.0;
/// 枪口的高度(米):子弹从角色胸口偏上一点出去。
pub const AIM_CHEST_HEIGHT: f32 = 1.32;
/// 俯仰的硬上限(弧度):防止打到正上 / 正下变成垂直射线。
pub const AIM_PITCH_LIMIT: f32 = 0.9;

/// 敌人被 hitscan 命中的判定半径(米)。
pub const ENEMY_HIT_RADIUS: f32 = 0.75;
/// 命中标记的显示时长(秒)。
pub const HITMARKER_TIME: f32 = 0.16;

/// 通缉升星的一次性提示时长(秒)。
pub const WANTED_FLASH_TIME: f32 = 0.9;
/// 通缉状态下希望场上维持的警察总数上限。
pub const ENEMY_WANTED_TOTAL: u32 = 8;
/// 行人逃跑时的速度(米/秒)。
pub const PED_FLEE_SPEED: f32 = 5.4;
/// 行人的碰撞半径(米)。
pub const PED_RADIUS: f32 = 0.4;
/// 行人转身速率(1/秒)。
pub const PED_TURN_RATE: f32 = 9.0;
/// 行人步态相位推进速率(rad/米)。
pub const PED_GAIT_RATE: f32 = 2.4;
/// 行人步态回零速率(1/秒)。
pub const PED_RELAX_RATE: f32 = 6.0;
/// 车判定「撞到行人」的半径(米)。
pub const CAR_HIT_RADIUS: f32 = 2.0;

/// 两行人相距小于这个距离就算「同一堆人」(米)。
pub const PED_GATHER_RADIUS: f32 = 4.5;
/// 凑成「一堆人」所需的最小行人数(算上自己)。
pub const PED_GATHER_MIN_CROWD: usize = 2;
/// 扎堆聊天的最短时长(秒):到了点也不能立刻散架。
pub const PED_TALK_MIN_SECONDS: f32 = 2.5;
/// 扎堆聊天的最长时长(秒):聊太久会显得卡住。
pub const PED_TALK_MAX_SECONDS: f32 = 9.0;
/// 聊天时围成的圈子半径(米):不贴脸,也不站成一排。
pub const PED_TALK_RING_RADIUS: f32 = 1.6;
/// 一堆人聊完之后,过多久才允许再凑一局(秒)。防止整城挤成一团。
pub const PED_GATHER_COOLDOWN: f32 = 6.0;
/// 聊天时长按人数折算时的「满员」人数:超过这么多就不再加长。
pub const PED_GATHER_MAX_CROWD: f32 = 5.0;
/// 每个人加入圈子时站位角度的递增量(弧度)。取 2π/5,让最多五个人
/// 正好均匀占满一圈互不重叠。
pub const PED_TALK_SLOT_STEP: f32 = 1.256_637_1;

/// 医院点坐标(XZ),分��在城市四角与中心附近。
pub const HOSPITAL_SPOTS: &[[f32; 3]] = &[
    [HOSPITAL_A_X, 0.0, HOSPITAL_A_Z],
    [HOSPITAL_B_X, 0.0, HOSPITAL_B_Z],
    [HOSPITAL_C_X, 0.0, HOSPITAL_C_Z],
    [HOSPITAL_D_X, 0.0, HOSPITAL_D_Z],
    [HOSPITAL_E_X, 0.0, HOSPITAL_E_Z],
];

/// 医院 A 的 X 坐标(米)。
pub const HOSPITAL_A_X: f32 = 8.2;
/// 医院 A 的 Z 坐标(米)。
pub const HOSPITAL_A_Z: f32 = 8.2;
/// 医院 B 的 X 坐标(米)。
pub const HOSPITAL_B_X: f32 = -52.0;
/// 医院 B 的 Z 坐标(米)。
pub const HOSPITAL_B_Z: f32 = 52.0;
/// 医院 C 的 X 坐标(米)。
pub const HOSPITAL_C_X: f32 = 52.0;
/// 医院 C 的 Z 坐标(米)。
pub const HOSPITAL_C_Z: f32 = -52.0;
/// 医院 D 的 X 坐标(米)。
pub const HOSPITAL_D_X: f32 = -98.0;
/// 医院 D 的 Z 坐标(米)。
pub const HOSPITAL_D_Z: f32 = -98.0;
/// 医院 E 的 X 坐标(米)。
pub const HOSPITAL_E_X: f32 = 98.0;
/// 医院 E 的 Z 坐标(米)。
pub const HOSPITAL_E_Z: f32 = 98.0;

/// 敌人剔除后不再写实例的距离(米)。
pub const ENEMY_RENDER_RANGE: f32 = 62.0;
/// 行人剔除后不再写实例的距离(米)。
pub const PED_RENDER_RANGE: f32 = 52.0;
/// 行走时身体的上下起伏幅度(米)。
pub const PED_BOB_HEIGHT: f32 = 0.045;
/// 城市里的常驻混混数量。
pub const THUG_COUNT: usize = 7;
/// 行人的正常色调。
pub const TINT_PED: [f32; 3] = [1.0, 1.0, 1.0];
/// 行人倒地后的色调。
pub const TINT_PED_DOWN: [f32; 3] = [0.6, 0.45, 0.48];

/// 鼠标左键的 `MouseEvent::button()` 取值。
pub const MOUSE_BUTTON_LEFT: i16 = 0;
/// 画布的 CSS 兜底宽度(指针坐标换算用)。
pub const CANVAS_CSS_W: f64 = 1280.0;
/// 画布的 CSS 兜底高度(指针坐标换算用)。
pub const CANVAS_CSS_H: f64 = 720.0;

/// 通缉星点亮的字符。
pub const HUD_STAR_ON: &str = "\u{2605}";
/// 通缉星熄灭的字符(同样的五角星,靠颜色区分亮灭)。
pub const HUD_STAR_OFF: &str = "\u{2606}";
/// 命中标记显示时的样式。
pub const HUD_OPACITY_ON: &str = "opacity:1";
/// 命中标记隐藏时的样式。
pub const HUD_OPACITY_OFF: &str = "opacity:0";
/// 任务阶段:无。
pub const MISSION_LABEL_NONE: &str = "—";
/// 任务阶段:走到某地。
pub const MISSION_LABEL_GOTO: &str = "walk to the marker";
/// 任务阶段:开车到某地。
pub const MISSION_LABEL_DRIVE: &str = "drive there";
/// 任务阶段:干掉某人。
pub const MISSION_LABEL_KILL: &str = "take out the target";

// ---- HUD DOM 标签 / 属性 / 样式 ----

/// HUD 根节点的 id。
pub const ID_HUD: &str = "vcw-hud";
/// 血条 / 护甲条的容器 id。
pub const ID_HUD_VITALS: &str = "vcw-vitals";
/// `Element::set_attribute` 的 `id` 属性名。
pub const ATTR_ID: &str = "id";
/// `document.create_element` 的 div 标签名。
pub const TAG_DIV: &str = "div";
/// `document.create_element` 的 canvas 标签名。
pub const TAG_CANVAS: &str = "canvas";
/// HUD 根:全屏、不吃指针事件,避免挡住左键开火。
pub const STYLE_HUD_ROOT: &str =
    "position:fixed;inset:0;pointer-events:none;z-index:20;font-family:system-ui,sans-serif";
/// 左下角的血条组。
pub const STYLE_HUD_VITALS: &str = "position:absolute;left:22px;bottom:26px;width:260px;display:flex;flex-direction:column;gap:6px";
/// 血条:从 100% 宽开始,由 `sync_hud` 每帧改 width。
pub const STYLE_HUD_HEALTH: &str = "height:14px;width:100%;background:#2ee06a;border-radius:3px;box-shadow:0 0 10px rgba(46,224,106,.55)";
/// 护甲条:同形状,蓝色。
pub const STYLE_HUD_ARMOR: &str = "height:9px;width:0%;background:#3aa0ff;border-radius:3px;box-shadow:0 0 8px rgba(58,160,255,.5)";
/// 右上角通缉星。
pub const STYLE_HUD_WANTED: &str = "position:absolute;right:26px;top:22px;font-size:30px;letter-spacing:3px;color:#ffd24a;text-shadow:0 0 12px rgba(255,210,74,.6)";
/// 右下角弹药。
pub const STYLE_HUD_AMMO: &str = "position:absolute;right:196px;bottom:30px;font-size:17px;color:#eaf6ff;text-shadow:0 2px 6px rgba(0,0,0,.85)";
/// 右下角现金。
pub const STYLE_HUD_CASH: &str = "position:absolute;right:26px;bottom:196px;font-size:20px;color:#7dffb0;text-shadow:0 2px 6px rgba(0,0,0,.85)";
/// 顶部任务文字。
pub const STYLE_HUD_MISSION: &str = "position:absolute;left:50%;top:20px;transform:translateX(-50%);font-size:16px;color:#ffe6a8;text-align:center;text-shadow:0 2px 6px rgba(0,0,0,.9);white-space:pre-line";
/// 准星:一个空心小方块。
pub const STYLE_HUD_CROSSHAIR: &str = "position:absolute;left:50%;top:50%;width:16px;height:16px;margin:-8px 0 0 -8px;border:2px solid rgba(255,255,255,.85);border-radius:50%;box-shadow:0 0 6px rgba(0,0,0,.7)";
/// 命中标记:默认透明,由 `sync_hud` 点亮。
pub const STYLE_HUD_HITMARKER: &str = "position:absolute;left:50%;top:50%;width:26px;height:26px;margin:-13px 0 0 -13px;opacity:0;pointer-events:none;background:linear-gradient(45deg,transparent 45%,#ff5a5a 45%,#ff5a5a 55%,transparent 55%),linear-gradient(-45deg,transparent 45%,#ff5a5a 45%,#ff5a5a 55%,transparent 55%)";
/// 小地图画布。
pub const STYLE_HUD_MINIMAP: &str = "position:absolute;right:22px;bottom:22px;width:150px;height:150px;border:2px solid rgba(255,255,255,.28);border-radius:4px;background:#0b1020";

/// 护甲背心给的护甲值。
pub const ARMOR_PICKUP_GAIN: f32 = 50.0;
/// 弹药箱给的备弹数。
pub const AMMO_PICKUP_GAIN: u32 = 24;

/// 手持武器相对角色的右侧偏移(米)。
pub const WEAPON_SIDE_OFFSET: f32 = 0.24;
/// 手持武器相对角色的前方偏移(米)。
pub const WEAPON_FWD_OFFSET: f32 = 0.42;
/// 手持武器的高度(米)。
pub const WEAPON_HEIGHT: f32 = 1.22;
/// 手持武器的模型缩放(资产是「单独一把枪」的尺寸)。
pub const WEAPON_SCALE: f32 = 0.85;
/// 手持武器的色调。
pub const TINT_WEAPON: [f32; 3] = [1.0, 1.0, 1.0];

/// 命中把敌人往后推的速度(米/秒)。
pub const HIT_KNOCKBACK: f32 = 2.4;
/// 增援警察在多远之外重生(米)。
pub const ENEMY_SPAWN_DIST: f32 = 34.0;

/// 手枪的开局备弹。
pub const PISTOL_RESERVE: u32 = 48;

// ===========================================================================
// 水体:外海 + 内湖
// ===========================================================================

/// 水面资产的 id。
pub const WATER_ID: &str = "water_procedural";

/// 水面资产的分类。
pub const WATER_CATEGORY: &str = "terrain";

/// 水面 part 名。
pub const WATER_PART: &str = "surface";

/// 海平面高度(米)。略低于城市地面 y=0,让海岸线低于人行道。
pub const SEA_LEVEL: f32 = -1.2;

/// 湖面高度(米)。
pub const LAKE_LEVEL: f32 = 0.34;

/// 海面环形网格的环数。
pub const SEA_RINGS: usize = 6;

/// 海面环形网格的辐条数。
pub const SEA_SPOKES: usize = 48;

/// 海面环的半径序列(内半径, 外半径),从城市边缘一路铺到远裁剪面。
///
/// 相机 far = 900,海面铺到 900 正好填满地平线;再往外就是天空。
/// 半径按几何级数走:近处 20 m 一环(浪的细节看得出),最外一环
/// 一次跨 400 m(只贡献一条远处的蓝色带)。
pub const SEA_RING_RADII: [(f32, f32); 6] = [
    (0.0, 170.0),
    (170.0, 240.0),
    (240.0, 360.0),
    (360.0, 540.0),
    (540.0, 700.0),
    (700.0, 900.0),
];

/// 浅海颜色(靠城市一侧)。
pub const SEA_SHALLOW: [f32; 3] = [0.055, 0.290, 0.330];

/// 深海颜色(最外圈)。
pub const SEA_DEEP: [f32; 3] = [0.012, 0.072, 0.155];

/// 湖心深水颜色。
pub const LAKE_DEEP: [f32; 3] = [0.030, 0.170, 0.240];

/// 湖岸浅滩颜色。
pub const LAKE_SHORE: [f32; 3] = [0.090, 0.330, 0.320];

/// 湖中心的世界 XZ 坐标。
pub const LAKE_CENTER: [f32; 2] = [92.0, 96.0];

/// 湖的 X 半径(米)。
pub const LAKE_RADIUS: f32 = 26.0;

/// 湖的 Z 半径(米)—— 刻意比 X 半径大,湖是椭圆的,不是正圆。
pub const LAKE_RADIUS_X: f32 = 17.0;

/// 湖的岸边浅滩内缩比例。
pub const LAKE_SHORE_INSET: f32 = 0.55;

/// 湖面扇区数。
pub const LAKE_SEGMENTS: usize = 24;

/// 水面每个面的自发光(全黑 —— 水面的「亮」来自反射,不是自发光)。
pub const WATER_EMISSIVE: [f32; 3] = [0.0, 0.0, 0.0];

/// 水面展开失败的错误信息。
pub const EXPECT_WATER: &str = "procedural water must be valid";

// ===========================================================================
// 自适应画质阈值
// ===========================================================================

/// 平滑帧率的指数平滑系数(越大越跟随瞬时值,越小越迟钝)。
pub const FPS_SMOOTHING: f32 = 0.90;

/// 低于这个帧率开始累计「慢帧」。
pub const QUALITY_DOWN_FPS: f32 = 24.0;

/// 高于这个帧率算「快」。
pub const QUALITY_UP_FPS: f32 = 55.0;

/// 连续多少帧慢才真的降一档。
pub const SLOW_FRAME_THRESHOLD: u32 = 30;

/// 连续多少帧快才考虑升档(当前实现是单向降级,这个值只用于计数)。
pub const FAST_FRAME_THRESHOLD: u32 = 180;

/// 建管线后前多少帧不参与降级判定。
pub const QUALITY_WARMUP_FRAMES: u32 = 20;

/// 跳过 SSAO 时 AO 贴图的中性值(1.0 = 完全不遮蔽)。
pub const NEUTRAL_AO: f32 = 1.0;

/// 跳过 SSR 时反射贴图的中性值(0.0 = 未命中,回退到环境色)。
pub const NEUTRAL_SSR: f32 = 0.0;

// ===========================================================================
// 可进入的样板楼(室内系统)
// ===========================================================================

/// 样板楼 A 的资产 id:紫丁香色 LOFT,12 × 10 m,带门洞 / 楼板 / 楼梯。
pub const BLDG_LOFT_SHOWCASE: &str = "bldg_loft_showcase";

/// 样板楼 B 的资产 id:珊瑚色商铺,10 × 9 m,带雨棚。
pub const BLDG_SHOP_SHOWCASE: &str = "bldg_shop_showcase";

/// 室内楼梯级高(米):必须与 `interior.py` 的 `STAIR_RISE` 一致。
pub const SHOWCASE_STAIR_RISE: f32 = 0.305;

/// 室内楼梯踏面进深(米):必须与 `interior.py` 的 `STAIR_RUN` 一致。
pub const SHOWCASE_STAIR_RUN: f32 = 0.45;

/// 室内楼梯级数:必须与 `interior.py` 的 `STAIR_STEPS` 一致。
pub const SHOWCASE_STAIR_STEPS: usize = 10;

/// 样板楼首层楼板面高度(米)。
pub const SHOWCASE_GROUND_TOP: f32 = 0.15;

/// 样板楼二层楼板下表面高度(米)。
pub const SHOWCASE_UPPER_BOTTOM: f32 = 2.95;

/// 样板楼二层楼板上表面高度(米),与楼梯顶端齐平。
pub const SHOWCASE_UPPER_TOP: f32 = 3.20;

/// 样板楼外墙厚度(米)。
pub const SHOWCASE_WALL_THICKNESS: f32 = 0.25;

/// 样板楼门洞的半宽(米)——门洞净宽 1.60 m。
pub const SHOWCASE_DOOR_HALF: f32 = 0.80;

/// 样板楼门洞净高(米)——门洞上沿高度。
pub const SHOWCASE_DOOR_TOP: f32 = 2.45;

/// 样板楼隔墙的顶面高度(米)。
pub const SHOWCASE_PARTITION_TOP: f32 = 2.75;

/// 样板楼外墙高度(米)。
pub const SHOWCASE_WALL_HEIGHT: f32 = 6.40;

/// 样板楼室内楼梯宽度(米)。
pub const SHOWCASE_STAIR_WIDTH: f32 = 1.30;

/// 第一级踏面到临街内墙的间隙(米)。
pub const SHOWCASE_STAIR_LEAD: f32 = 0.20;

/// 样板楼首层隔墙的厚度(米)。
pub const SHOWCASE_PARTITION_THICKNESS: f32 = 0.075;

/// 样板楼首层隔墙中心相对楼中心的 Z 偏移(米,本地坐标)。
pub const SHOWCASE_PARTITION_Y: f32 = 2.40;

/// 隔墙右端与楼梯之间保留的过道宽度(米)。
pub const SHOWCASE_PARTITION_GAP: f32 = 1.70;

/// 玩家身体高度(米):脚底到头顶,决定室内墙是否「够得着」。
pub const PLAYER_BODY_HEIGHT: f32 = 1.75;

/// 城市地面的高度(米):整张地面网格就在 y = 0。
pub const GROUND_LEVEL: f32 = 0.0;

/// 顶着墙斜走时必须保留切向位移,不能位移归零。
pub const T_COLLISION_SLIDE_KEEPS_TANGENT: &str = "滑动分离必须沿墙保留切向位移";

/// 正面顶墙仍然必须被挡住,不能穿墙。
pub const T_COLLISION_SLIDE_STOPS_AT_WALL: &str = "正面顶墙必须停住,不能穿墙";

/// 空地上滑动后,x 轴必须等于「起点 x + 位移 x」。
pub const T_COLLISION_SLIDE_OPEN_GROUND_X: &str = "空地滑行后 x 必须等于起点加位移";

/// 空地上滑动后,z 轴必须等于「起点 z + 位移 z」。
pub const T_COLLISION_SLIDE_OPEN_GROUND_Z: &str = "空地滑行后 z 必须等于起点加位移";

/// 重力加速度(米/秒²)。
pub const GRAVITY: f32 = 22.0;

/// 下落速度上限(米/秒):防止穿过薄楼板。
pub const TERMINAL_VELOCITY: f32 = 34.0;

/// 落地下沉容差(米):脚底在楼板面下方这么多之内仍然算站住,防抖。
pub const GROUND_SNAP_SKIN: f32 = 0.06;

/// 玩家每秒被拉回「站在当前楼板上」的最大高度(米/秒)——防穿地。
pub const ANTI_TUNNEL_LIFT_SPEED: f32 = 6.0;

/// 跳跃初速度(米/秒,向上为正)。
///
/// **不是拍脑袋定的,是按「能跳上 GTA V 量级的矮墙」反解的。**
/// 平抛顶高 `h = v0² / (2·g)`,代入本项目的 `GRAVITY = 22.0`:
///
/// | `v0` (m/s) | 顶高 (m) | 滞空 (s) | 步行 4.6 m/s 前进 (m) |
/// |---|---|---|---|
/// | 4.5 | 0.46 | 0.41 | 1.9 |
/// | 6.0 | 0.82 | 0.55 | 2.5 |
/// | **7.4** | **1.245** | **0.673** | **3.1** |
/// | 8.5 | 1.64 | 0.77 | 3.5 |
///
/// GTA V 街景里能被跳上去的台阶 / 矮墙在 1.0–1.2 m 量级(本项目
/// 样板楼梯的单级踏高 `SHOWCASE_STAIR_RISE` 只有 0.305 m,一层隔墙
/// 才 1 m 出头),`v0 = 6.0` 只到 0.82 m,够不上一整级台阶的两倍;
/// `8.5` 又能翻越 `SHOWCASE_UPPER_TOP = 3.20 m` 那道门槛太多,手感飘。
/// 取 **7.4** ⇒ 顶高 **1.245 m**,滞空 0.67 s:站着跳能上 1.0–1.2 m
/// 的台阶,冲刺(8.4 m/s)起跳能横跨 5.6 m,和成熟第三人称射击一致。
pub const JUMP_VELOCITY: f32 = 7.4;

/// 回归测试:跳跃顶高必须够得上一格台阶的两倍。
pub const T_JUMP_CLEARS_A_LEDGE: &str =
    "跳跃顶高 {peak:.3} m 必须 >= {want:.2} m(1.0-1.2 m 量级的台阶 / 矮墙),实跳初速 {v0} m/s";

/// 回归测试:跳跃必须先升后落,不能一按就往下掉。
pub const T_JUMP_RISES_BEFORE_FALLING: &str = "跳跃必须先升后落,顶点 {peak:.3} m,滞空 {air:.3} s";

/// 回归测试:落地后必须回到「站稳」状态。
pub const T_JUMP_LANDS_STANDING: &str =
    "落地后必须 grounded=true 且 vy=0,实际 grounded={grounded} vy={vy}";

/// 回归测试:滞空途中不得二次起跳。
pub const T_JUMP_ONLY_FROM_GROUND: &str = "滞空中(y={y:.3})不得再起跳,实际起跳后 vy={vy:.3}";

/// 回归测试:积分器的稳态必须原样停在地面上。
pub const T_VERTICAL_REST_ON_FLOOR: &str = "站在地板上时积分器必须保持原位,{y:.3}/{vy:.3}";

/// 回归测试:徒手不能被当成一种「可拾取的地面模型」。
pub const T_UNARMED_NOT_A_PICKUP: &str = "空串不是武器资产,徒手不能被 from_asset 反查出来";

/// 回归测试:徒手永远不能开火。
pub const T_UNARMED_CANNOT_FIRE: &str = "徒手不该能开火,实际 can_fire={ok}";

/// 回归测试:徒手换不了弹。
pub const T_UNARMED_NO_RELOAD: &str = "徒手不该能换弹,实际 reload={ok}";

/// 回归测试:投掉一颗手雷后计数必须 -1,投完就不再给投。
pub const T_GRENADE_COUNT_DOWN: &str = "投掷后手雷数必须 -1,实际 {before} -> {after}";

/// 回归测试:手上没有手雷时不得凭空投出。
pub const T_GRENADE_NONE_LEFT: &str = "手上没有手雷时 take_grenade 必须返回 false";

/// 回归测试:徒手没有模型资产,手持批次必须落空。
pub const T_UNARMED_HAS_NO_MESH: &str = "徒手不该持有手持模型,实际 asset={asset:?}";

// ===========================================================================
// 室内系统单元测试断言文案
// ===========================================================================

/// 单元测试断言文案:沿楼梯上行的高度必须逐级递增。
pub const T_INTERIOR_STAIR_MONOTONIC: &str = "必须走满所有台阶,实际 {heights:?}";

/// 单元测试断言文案:楼梯必须能被一级一级走上去并与二层楼板齐平。
pub const T_INTERIOR_STAIR_CLIMBS: &str = "楼梯必须逐级抬升并抵达二层楼板,实际 {pair:?}";

/// 单元测试断言文案:二级楼板必须高于一步的踏高容差。
pub const T_INTERIOR_UPPER_FLOOR_ABOVE: &str = "踏高容差不能大到一步跨上二层楼板";

/// 单元测试断言文案:两室之间的隔墙必须挡住玩家。
pub const T_INTERIOR_WALL_BLOCKS: &str = "隔墙必须挡住玩家,实际 {pushed:?}";

/// 单元测试断言文案:不在墙上的玩家必须能自由通过。
pub const T_INTERIOR_WALL_PASSES: &str = "远离隔墙时必须原地不动,实际 {away:?}";

/// 单元测试断言文案:头顶的楼板不得推开玩家。
pub const T_INTERIOR_CEILING_ABOVE: &str = "头顶的楼板不得推开玩家,实际 {inside:?}";

/// 单元测试断言文案:门洞是唯一能穿过的缺口。
pub const T_INTERIOR_DOORWAY_PASSES: &str = "门洞中心必须能穿过去,实际 {through:?}";

/// 单元测试断言文案:门洞两侧的墙与门楣必须挡住玩家。
pub const T_INTERIOR_DOORWAY_BLOCKS: &str = "门洞之外的墙面必须挡住玩家,实际 {blocked:?}";

/// 单元测试断言文案:脚下没有楼板时不得凭空生成支撑面。
pub const T_INTERIOR_NO_SLAB_UNDER: &str = "楼板外侧不得出现支撑面";

// ===========================================================================
// 样板楼接线单元测试断言文案
// ===========================================================================

/// 单元测试断言文案:门洞里必须站得住人(有首层楼板)。
pub const T_SHOWCASE_DOOR_INSIDE: &str = "门洞内侧必须落在首层楼板上";

/// 单元测试断言文案:门洞两侧的墙必须挡住玩家。
pub const T_SHOWCASE_DOOR_OUTSIDE_BLOCKED: &str = "门洞两侧的墙必须挡住玩家";

/// 单元测试断言文案:外墙必须把玩家挡在临街面之外。
pub const T_SHOWCASE_FRONT_FACING: &str = "外墙必须把玩家挡在临街面之外";

/// 单元测试断言文案:楼梯总高必须与二层楼板齐平。
pub const T_SHOWCASE_STAIR_REACHES_TOP: &str = "楼梯总高必须不小于二层楼板面高度";

/// 单元测试断言文案:单级踏高必须小于踏高容差,否则爬不上楼梯。
// ---- 车轮渲染:资产 part 名(§1.3c 字面量入 const) ----

pub const PART_TYRES: &str = "tyres";
pub const PART_HUBS: &str = "hubs";

pub const T_SHOWCASE_STAIR_RISE_SHALLOW: &str = "单级踏高必须小于踏高容差,否则爬不上楼梯";

/// 单元测试断言文案:模拟行走必须真的把玩家带到二层楼板面。
pub const T_SHOWCASE_WALKER_REACHES_TOP: &str = "模拟沿楼梯行走必须抵达二层楼板面";

/// 单元测试断言文案:两栋样板楼的门必须相向而开。
pub const T_SHOWCASE_DOORS_FACE_EACH_OTHER: &str = "两栋样板楼的门洞必须相向,便于从中间走进去";

/// 单元测试断言文案:样板楼占地必须避开车行道与相邻的普通楼。
pub const T_SHOWCASE_FOOTPRINT_CLEAR: &str = "样板楼占地不能和普通楼重叠";

/// 单元测试断言文案:相邻两级踏步的高度必须严格递增。
pub const T_INTERIOR_STAIR_PAIR: &str = "楼梯相邻两级必须严格递增 {pair:?}";

/// 单元测试断言文案:头顶楼板不得把玩家横向推开。
pub const T_INTERIOR_CEILING_INSIDE: &str = "头顶楼板不得推开玩家 {inside:?}";

/// 单元测试断言文案:门洞中心必须能穿过去。
pub const T_INTERIOR_DOORWAY_THROUGH: &str = "门洞中心必须穿得过去 {through:?}";

// ===========================================================================
// 样板楼测试断言文案(§1.3c:字面量一律进 const.rs)
// ===========================================================================

/// 单元测试断言文案:门洞内侧必须踩得到首层楼板。
pub const T_SHOWCASE_DOOR_NO_SLAB: &str = "门洞里没有首层楼板,support={support:?}";

/// 单元测试断言文案:门洞中心必须穿得过去。
pub const T_SHOWCASE_DOOR_CENTER_BLOCKED: &str = "门洞中心被挡住了 after={after:?}";

/// 单元测试断言文案:门垛必须挡住玩家。
pub const T_SHOWCASE_PIER_LET_PLAYER_THROUGH: &str = "门垛没有挡住玩家 at={at:?} pushed={pushed:?}";

/// 单元测试断言文案:两栋样板楼不得互相重叠。
pub const T_SHOWCASE_TWO_OVERLAP: &str = "两栋楼重合了";

/// 单元测试断言文案:两扇门必须相向。
pub const T_SHOWCASE_DOOR_NOT_FACING: &str = "门法线没有指向另一栋楼(dot={dot})";

/// 单元测试断言文案:分离器不得把玩家留进墙里。
pub const T_SHOWCASE_PUSHED_INTO_WALL: &str = "的人被推进了墙里 {pushed:?}";

/// 单元测试断言文案:楼梯总高必须与二层楼板齐平。
pub const T_SHOWCASE_STAIR_TOP_LEVEL: &str =
    "最高一级 {}{STAIR_TOTAL} 与楼板面 {SHOWCASE_UPPER_TOP} 不齐平";

/// 单元测试断言文案:单级踏高必须小于踏高容差。
pub const T_SHOWCASE_RISE_GE_TOLERANCE: &str =
    "踏高 {SHOWCASE_STAIR_RISE} >= 容差 {STEP_UP_TOLERANCE}";

/// 单元测试断言文案:容差不得大到能一步跨上一层。
pub const T_SHOWCASE_TOLERANCE_TOO_BIG: &str = "容差 {STEP_UP_TOLERANCE} 大到能一步跨上一层";

/// 单元测试断言文案:容差必须小于两级踏高之和。
pub const T_SHOWCASE_TOLERANCE_TWO_RISES: &str = "容差必须小于两级踏高 {}";

/// 单元测试断言文案:模拟行走必须爬到二层。
pub const T_SHOWCASE_WALKER_DIRECTION: &str = "走了 {frames} 帧,楼梯方向 (normal {:?}) 下的 y={y}";

/// 单元测试断言文案:头顶的楼板不得把玩家推开。
pub const T_SHOWCASE_CEILING_PUSHED: &str = "楼 {index} 的头顶楼板把玩家推开了";

/// 单元测试断言文案:隔墙必须挡住玩家。
pub const T_SHOWCASE_PARTITION_LET_THROUGH: &str =
    "的隔墙没有挡住玩家 mid={mid:?} pushed={pushed:?}";

/// 单元测试断言文案:隔墙与楼梯之间必须留出过道。
pub const T_SHOWCASE_PARTITION_LANE: &str =
    "隔墙必须与楼梯之间留出过道,partition_end={partition_end}";

/// 单元测试断言文案:过道必须走得通。
pub const T_SHOWCASE_LANE_BLOCKED: &str = "的过道被堵住了 lane={lane:?} through={through:?}";

/// 单元测试断言文案:占地不得压上车行道。
pub const T_SHOWCASE_AXIS_ON_ROAD: &str = "的轴 {axis} 压到了车行道:x={} 街={line} 半径={reach}";

/// 单元测试断言文案:占地不得与普通楼重叠。
pub const T_SHOWCASE_OVERLAPS_ORDINARY: &str = "与普通楼 ({:.1},{:.1}) 的保守包围盒重叠";

/// 单元测试断言文案:二维碰撞体不得堵死门洞。
pub const T_SHOWCASE_DOORWAY_2D_BLOCKED: &str = "的门洞被二维碰撞体堵住";

/// 断言信息:样板楼门洞必须落在外墙面上,而不是内墙净跨的边界。
pub const T_SHOWCASE_DOOR_ON_OUTER_WALL: &str = "showcase doorway is on the inner wall face";

/// 断言信息:走路线的路点必须互不相同、相邻间距大于一个身位。
pub const T_SHOWCASE_ROUTE_WALKABLE: &str =
    "showcase walk route has duplicate or too-close waypoints";

/// 验收通道:探针结果挂在这个 window 属性上。
pub const K_PROBE_WINDOW: &str = "__vcw_probe";
/// 验收通道:`__vcw_teleport` 请求里的 `probe` 字段 —— 打印玩家四周的墙。
pub const K_TELEPORT_PROBE: &str = "probe";
/// 验收通道:`__vcw_teleport` 请求里的 `speed` 字段 —— 时间加速倍率。
pub const K_TELEPORT_SPEED: &str = "speed";
/// 验收通道:`__vcw_teleport` 请求里的 `safe` 字段 —— 打开无敌。
pub const K_TELEPORT_SAFE: &str = "safe";
/// 验收通道:`__vcw_teleport` 请求里的 `walk` 字段 —— 持续走位方向。
pub const K_TELEPORT_WALK: &str = "walk";
/// 验收通道:`__vcw_teleport` 请求里的 `hold` 字段 —— 注入开火剩余帧数。
pub const K_TELEPORT_HOLD: &str = "hold";

/// 验收通道:瞄准请求挂在这个 window 属性上。
pub const K_AIM_WINDOW: &str = "__vcw_aim";
/// 断言信息:路线的一段同时动了 X 和 Z(楼里有隔墙,斜线会撞上)。
pub const T_ROUTE_LEG_DIAGONAL: &str = "a route leg moves on both axes";

/// 视角方向的回归测试:鼠标右移必须让画面内容**左移**(视角右转),
/// 而不是相反。
pub const T_MOUSE_RIGHT_PANS_RIGHT: &str =
    "鼠标右移后画面内容必须向左移(视角右转),实测内容移向了 {dir}";

/// 上面那条模板里要填的「移向左边」分支。
pub const DIR_LEFT: &str = "左";

/// 上面那条模板里要填的「移向右边」分支。
pub const DIR_RIGHT: &str = "右";

/// 骨架回归测试:每节 limb 的顶点必须停在资产给定的**绝对高度**上。
///
/// 关节枢轴(`joint_pivot`)是从顶点包围盒推出来的,已经含有角色的绝对
/// 高度(肩 y≈1.43)。顶点本来就在那个高度上,矩阵再平移一次就会把整节
/// part 抬高一整个身高量级 —— 表现是「四肢飘在头顶上方、之间全是
/// 空隙」。
pub const T_LIMB_STAYS_AT_ASSET_HEIGHT: &str = "{part} 被抬高了 {off:.3} m(实际 y {got:.2}..{got_hi:.2},资产给的是 {want:.2}..{want_hi:.2}): 关节枢轴不应再被当成平移量";

/// 骨架回归测试:末端关节必须继承父关节的摆角(不能各转各的)。
pub const T_DISTAL_INHERITS_PARENT_SWING: &str =
    "末端关节没有继承父关节的摆角: 实得 {got},手算应为 {want}";

/// 上面两条骨架模板里填 part 名的占位符键。
pub const KEY_PART: &str = "part";

/// 车轮滚动方向的回归测试:车前进时,轮底必须相对车体**向后**滑。
///
/// 无滑滚动的运动学约束:接触点相对地面瞬时不动,所以车往 `+X` 走时,
/// 接触点沿轮缘向 `-X` 退。轮轴是本地 `Z`(实测自 `car_coupe` 的
/// `tyres` 顶点:每只轮子 `x 0.64 / y 0.64 / z 0.235`,最短轴是 Z)。
pub const T_WHEEL_ROLLS_FORWARD: &str =
    "车轮滚动方向反了:spin={spin} 时轮底沿车体后退了 {bottom:+.4} m,应当为负(车往 +X 前进)";

/// 自转 90 度必须带动轮缘基向量(X / Y 之一)改变方向。
pub const T_WHEEL_SPIN_MOVES_RIM: &str = "自转 90 度必须改变轮缘基向量:rest={axis} turned={turned}";

/// 绕轮轴(Z)自转时,轮轴基向量本身不得改变。
pub const T_WHEEL_AXLE_STILL: &str = "绕 Z 自转不得转动 Z 轴本身,变化量 {delta}";

/// 自转只绕轮心,不得把轮心挪走。
pub const T_WHEEL_CENTRE_FIXED: &str = "自转不得移动轮心";

/// 长帧跨过多级踏面时,支撑面必须被认出来(线上「上到第 4 级就卡住」的回归)。
pub const T_INTERIOR_LONG_FRAME_LADDER: &str =
    "一帧跨过多级踏面时支撑面必须仍能认出踏面,否则楼梯会塌回首层";

/// 长帧抬升必须真的爬到顶,而不只是认出踏面。
pub const T_INTERIOR_LONG_FRAME_HEIGHT: &str =
    "长帧跨过 1.36 级后脚底必须累积升到二层楼板面,实得 {height}";

/// 上面那条模板里待替换的高度占位符。
pub const T_INTERIOR_HEIGHT_TOKEN: &str = "{height}";

/// 支撑面不得低于脚底 —— 否则首层地板会把人从楼梯中段拽回地面。
pub const T_INTERIOR_NO_DOWNWARD_SNAP: &str =
    "脚下的板低于脚底时必须判为无支撑(首层地板会把人吸回地面)";

/// 验收通道:传送 / 走位 / 加速 / 探针请求挂在这个 window 属性上。
pub const K_TELEPORT_WINDOW: &str = "__vcw_teleport";
