use super::*;
/// 网格街道之间的一个街区:楼围合出一个内部院落,而不是沿街一条线。
#[derive(Data)]
pub(crate) struct BlockLayout {
    /// 街区中心的 X 坐标。
    #[get(type(copy), pub(crate))]
    pub(crate) cx: f32,
    /// 街区中心的 Z 坐标。
    #[get(type(copy), pub(crate))]
    pub(crate) cz: f32,
    /// 街区西侧那条街道的索引 —— 摆件种子由它定,不用枚举序号。
    #[get(type(copy), pub(crate))]
    pub(crate) ix: i32,
    /// 街区北侧那条街道的索引 —— 摆件种子由它定,不用枚举序号。
    #[get(type(copy), pub(crate))]
    pub(crate) iz: i32,
}
/// 一栋楼在场景里的摆放。
#[derive(Data)]
pub(crate) struct BuildingPlacement {
    /// 资产 id。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) asset: &'static str,
    /// 世界坐标(x, z)。
    #[get(type(copy), pub(crate))]
    pub(crate) position: [f32; 2],
    /// 绕 Y 轴的朝向(弧度)。
    #[get(type(copy), pub(crate))]
    pub(crate) yaw: f32,
    /// 统一缩放。
    #[get(type(copy), pub(crate))]
    pub(crate) scale: f32,
    /// 色调乘子,给同一批楼一点色彩变化。
    #[get(type(copy), pub(crate))]
    pub(crate) tint: [f32; 3],
}
/// 路灯 / 交通灯等街道道具的摆放。
#[derive(Data)]
pub(crate) struct PropPlacement {
    /// 资产 id。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) asset: &'static str,
    /// 世界坐标。
    #[get(type(copy), pub(crate))]
    pub(crate) position: [f32; 3],
    /// 朝向(弧度)。
    #[get(type(copy), pub(crate))]
    pub(crate) yaw: f32,
    /// 色调。
    #[get(type(copy), pub(crate))]
    pub(crate) tint: [f32; 3],
}
/// 可进入样板楼的几何蓝图:摆放 + 内墙净跨 + 外轮廓半尺寸。
///
/// 净跨 / 半尺寸按资产 id 分派而不是逐个实例存一份:它们是**资产的
/// 属性**,不是摆放的属性,存两份就等于给「资产换了尺寸」留一个
/// 悄悄不同步的口子。
#[derive(Data)]
pub(crate) struct ShowcaseSpec {
    /// 资产 id。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) asset: &'static str,
    /// 世界坐标(x, z)。
    #[get(type(copy), pub(crate))]
    pub(crate) position: Vec2,
    /// 绕 Y 轴的朝向(弧度):门洞法线 = `(sin yaw, cos yaw)`。
    #[get(type(copy), pub(crate))]
    pub(crate) yaw: f32,
    /// 内墙之间的净跨(米),按**资产本地** X / Z 记。
    #[get(type(copy), pub(crate))]
    pub(crate) span: Vec2,
    /// 外墙外皮之间的半跨(米),按**资产本地** X / Z 记。
    #[get(type(copy), pub(crate))]
    pub(crate) half: Vec2,
}
/// 按键的按住状态。
#[derive(Data)]
pub(crate) struct InputState {
    /// W / A / S / D / 方向键。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) keys: HashMap<String, bool>,
    /// 当前昼夜相位。
    #[get(type(copy), pub(crate))]
    pub(crate) phase: DayPhase,
    /// 鼠标是否按下拖拽。
    #[get(type(copy), pub(crate))]
    pub(crate) dragging: bool,
    /// 上一次指针位置。
    #[get(type(copy), pub(crate))]
    pub(crate) last_pointer: [f64; 2],
    /// 双指上一次的距离(用于捏合缩放)。
    #[get(type(copy), pub(crate))]
    pub(crate) last_pinch: f64,
    /// 当前是否持有指针锁(视角走 `movement_x/y`,不需要按住左键)。
    ///
    /// 锁定态与「按住左键拖拽」是两套输入源,`dragging` 只描述后者。
    /// 分开存是因为两者可以同时为真(锁着的时候也允许拖拽),而退出锁
    /// 时必须能只清掉属于锁的那部分。
    #[get(type(copy), pub(crate))]
    pub(crate) pointer_locked: bool,
    /// 左键是否按下(开火)。
    #[get(type(copy), pub(crate))]
    pub(crate) fire_held: bool,
    /// 本帧是否刚按下左键(单发武器只响一次)。
    #[get(type(copy), pub(crate))]
    pub(crate) fire_pressed: bool,
    /// 鼠标是否刚抬起。
    #[get(type(copy), pub(crate))]
    pub(crate) fire_released: bool,
    /// 鼠标在画布内的归一化位置(`-1.0..1.0`,Y 向上)。
    #[get(type(copy), pub(crate))]
    pub(crate) aim_point: [f64; 2],
    /// 画布的 CSS 像素尺寸,用于把鼠标坐标换算成 NDC。
    #[get(type(copy), pub(crate))]
    pub(crate) viewport: [f64; 2],
    /// 驾驶时**当前接管方向盘**的按键(`"KeyA"` / `"ArrowLeft"` …),
    /// 空串表示没人按。
    ///
    /// 这不是为了「记住谁按着」,而是为了 A/D 与方向键**同时按住**时裁决
    /// 谁说话 —— 见 [`InputState::steer_axis`]。`keys` 本身是「谁按着」,
    /// 单独一个布尔值回答不了「两个方向同时按」这种问题。
    #[set(skip, pub(crate))]
    pub(crate) steer_key: String,
}
/// 一份完整的游戏运行时状态。
///
/// 全部包在 `Rc<RefCell<..>>` 里供 RAF 闭包与事件闭包共享;
/// 闭包内部遵循「先取快照再短暂 borrow_mut」的写法,避免 RefCell 重叠借用。
#[derive(Data)]
pub struct Game {
    /// canvas 元素。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) canvas: HtmlCanvasElement,
    /// 相机。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) camera: Camera,
    /// 场景(资产 + 批次)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) scene: Scene,
    /// 流式世界:地面批次 —— 玩家跨过阈值时这块网格会被整体重建。
    #[get(type(copy), pub(crate))]
    pub(crate) ground_batch: usize,
    /// 流式世界:水面批次 —— 同上。
    #[get(type(copy), pub(crate))]
    pub(crate) water_batch: usize,
    /// 流式世界:当前地面 / 水面是围绕哪个点生成的(米)。
    #[get(type(copy), pub(crate))]
    pub(crate) streamed_center: Vec2,
    /// 注入开火剩余帧数(验收通道,见 [`apply_teleport_request`] 的 `hold` 分支)。
    #[get(type(copy), pub(crate))]
    pub(crate) hold_frames: u32,
    /// 验收期间的时间加速倍率(1.0 = 正常)。
    ///
    /// 无头浏览器把 rAF 节流到约 1 fps,而 `MAX_FRAME_TIME` 把每帧的真实
    /// 间隔钳到 0.25 s —— 合起来游戏只跑 **25% 速度**。走完 7 个路点要
    /// 二十分钟,验证一轮全量功能要跑将近一个小时。加速只改喂给模拟的
    /// 步数,不改物理常量,所以走的仍然是同一套积分。
    #[get(type(copy), pub(crate))]
    pub(crate) speed_scale: f32,
    /// 验收期间是否免疫伤害(见 [`apply_teleport_request`] 的 `safe` 分支)。
    ///
    /// 走位验证要花好几分钟真实时间(无头浏览器把 rAF 节流到约 1 fps,
    /// 模拟只跑 6% 速度),而街上敌人一直在开火 —— 前几轮实测血量从
    /// 91 一路掉到 68,掉光就重生到最近的医院点,玩家被瞬移回城中心,
    /// 看起来就是「走位脚本把玩家弹飞了」。
    #[get(type(copy), pub(crate))]
    pub(crate) safe_mode: bool,
    /// 验收脚本下达的「朝这个**世界**方向持续走」请求(米/秒归一化向量)。
    ///
    /// 不走 WASD 是因为 WASD 是**相机相对**的:脚本必须先把相机转到
    /// 正前方才能让 W 变成世界某个方向,而无头浏览器里合成鼠标事件进不了
    /// 相机的拖拽监听器 —— 上一步就是卡在这里。直接给世界方向,移动这一段
    /// 走的仍然是 `Player::step` 的真实积分(含碰撞分离与室内判定)。
    #[get(type(copy), pub(crate))]
    pub(crate) walk_request: Vec2,
    /// 流式世界:资产 id → 场景网格索引,重建静态批次时要用。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) asset_index: HashMap<String, usize>,
    /// 流式世界:静态批次(楼 / 树 / 道具 / 招牌)占用的批次表长度。
    ///
    /// 静态批次是 `build_scene` 按生成中心铺的,流式重建时整段换掉;
    /// 批次表是**尾插**的,玩家骨架 / 车队 / 拾取物的批次在静态批次之后,
    /// 所以「静态段长度」就是把静态部分与动态部分分开的唯一依据。
    #[get(type(copy), pub(crate))]
    pub(crate) static_batch_count: usize,
    /// 走完 `Player::step` 之后的玩家 XZ(验收探针用)。
    ///
    /// 「有速度、无位移」必须能分清是哪一层把人按住了:`Player::step`
    /// 走 `resolve_slide`(静态),`resolve_dynamic_bodies` 之后还会
    /// **无条件**把玩家 XZ 覆盖成分离结果。两者的输出都记下来,探针
    /// 直接对比就知道是谁把位置钉住的 —— 只看最终坐标永远只能猜。
    #[get(type(copy), pub(crate))]
    pub(crate) after_static_step: Vec2,
    /// 走完 `resolve_dynamic_bodies` 之后的玩家 XZ(验收探针用)。
    #[get(type(copy), pub(crate))]
    pub(crate) after_dynamic_step: Vec2,
    /// 走完 `step_vertical` 之后的玩家 XZ(验收探针用)。
    ///
    /// `step_vertical` 名义上只管 Y,但它结尾是
    /// `set_position([x, y, z])`,`x` / `z` 来自 `resolve_interior` ——
    /// 于是它**会改写 XZ**。夹在静态层与动态层之间,不改写就看不到它。
    #[get(type(copy), pub(crate))]
    pub(crate) after_vertical_step: Vec2,
    /// 被静态几何挡住视线、因此**没有结算伤害**的敌人枪数(验收探针用)。
    ///
    /// 「隔墙掉血」的验收不能只看 `health` 没掉:掉血也可能是因为
    /// 那一帧根本没有敌人开火。这个计数器区分「看得见但打偏」与
    /// 「隔墙、这一枪被门控挡掉」,两者都需要在报告里出现。
    #[get(type(copy), pub(crate))]
    pub(crate) shots_blocked_by_geometry: u32,
    /// 走进 `step_vertical` **之前**玩家所在的 XZ(室内层要靠它还原位移)。
    ///
    /// `step_vertical` 需要知道「静态层这一帧把人推进了多远」,才能把
    /// 切向分量从隔墙上滑过去而不是原路弹回。位置本身不含这个信息:
    /// 拿不到上一帧位置就只能做纯分离,那正是「有速度、无位移」的
    /// 成因。所以在 `Player::step` 之前先存一份。
    #[get(type(copy), pub(crate))]
    pub(crate) previous_step_xz: Vec2,
    /// 输入状态。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) input: InputState,
    /// 渲染后端。
    /// 本帧**真的**提交给 GPU 的三角形数。
    ///
    /// HUD 上那个 `{N} tris` 读的是 `scene.total_triangles`,一个静态
    /// 常量 —— WebGL2 与 WebGPU 会显示同一个数,哪怕其中一个后端一帧
    /// 都没画出来。要判断某个后端是否真的在画,只能看这个字段。
    #[get(type(copy), pub(crate))]
    pub(crate) gpu_submitted: u32,
    /// 资产是否已经构建完、可以往 GPU 上传了。
    ///
    /// ⚠️ WebGPU 的设备获取是**异步**的,而资产加载也是异步的,两条
    /// `spawn_local` 谁先跑完是不确定的:资产那边可能先把 mesh 全部
    /// 推给**上一个** renderer(或根本没 renderer),于是 WebGPU 装好
    /// 时自己手里是空的 —— 表现为「上传 80 个 mesh 成功、每帧也报告
    /// 提交了几十万三角形,画布却只有一片天空」:那些批次查不到 mesh,
    /// 被静默跳过。
    ///
    /// 这里用一个布尔量把「有 renderer」和「有资产」两件事**都**满足
    /// 之后的上传串起来,顺序问题就不存在了。
    #[get(type(copy), pub(crate))]
    pub(crate) assets_built: bool,
    #[get(skip, pub(crate))]
    pub(crate) renderer: Option<Renderer>,
    /// 已加载的资产数量。
    #[get(type(copy), pub(crate))]
    pub(crate) loaded_assets: usize,
    /// 总资产数量。
    #[get(type(copy), pub(crate))]
    pub(crate) total_assets: usize,
    /// 已渲染帧数(测试用)。
    #[get(type(copy), pub(crate))]
    pub(crate) frame_count: u64,
    /// 已完成的固定步数(测试用)。
    #[get(type(copy), pub(crate))]
    pub(crate) ticks: u64,
    /// 加载失败信息(有值时显示在遮罩上)。
    #[get(type(clone), pub(crate))]
    pub(crate) load_error: Option<String>,
    /// 后备方案是否启用(资产 fetch 全部失败)。
    #[get(type(copy), pub(crate))]
    pub(crate) using_fallback: bool,
    /// 固定步长累加器(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) accumulator: f32,
    /// 按实测帧率自动降档的画质状态机。
    #[get(type(copy), pub(crate))]
    pub(crate) quality: AdaptiveQuality,
    /// 上一帧的 `requestAnimationFrame` 时间戳(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) frame_time: f32,
    /// 验收探针:几个已知世界点投到屏幕上的位置。
    /// 离玩家最近的一栋楼的中心(x, z)。
    ///
    /// 验收脚本拿它当「推挤目标」:街区布局是程序化生成的,写死一个
    /// 魔法坐标早晚会推到空地上,所以在生成碰撞世界时顺手算一次存下。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) nearest_building: Option<[f32; 2]>,
    /// 当前帧的画布像素尺寸(调试通道用来报角色在屏幕上的像素坐标)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) canvas_size: Cell<(u32, u32)>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) probe: Vec<Option<[f32; 2]>>,
    /// 玩家状态(位置 / 朝向 / 步态 / 生命值 / 拾取)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) player: Player,
    /// 车队与拾取物。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) traffic: Traffic,
    /// 静态碰撞世界(玩家圆 vs 建筑 / 车 / 道具 + 世界边界)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) world: CollisionWorld,
    /// 室内碰撞世界(可进入楼的楼板 / 隔墙 / 楼梯)。
    ///
    /// 刻意与 [`Self::world`] 分开:后者是**纯二维**的,车行道、公交车
    /// 和 200 栋实心楼全都走它;只有玩家进入样板楼时才额外查这一份。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) interiors: FloorWorld,
    /// 玩家骨架每个 part 的批次索引(与 `PED_SUIT` 资产 part 一一对应)。
    #[get(type(clone), pub(crate))]
    pub(crate) player_batches: Vec<PlayerLimbBatch>,
    /// 车队车辆每个批次对应的车辆索引。
    #[get(type(clone), pub(crate))]
    pub(crate) car_batches: Vec<usize>,
    /// 每辆车四个可转车轮的批次。
    #[get(type(clone), pub(crate))]
    pub(crate) car_wheel_batches: Vec<CarWheelBatch>,
    /// 车型 id → 该车型切好的「单个车轮」mesh 索引(0..4 四个 slot 共用)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) car_wheel_meshes: HashMap<String, Vec<usize>>,
    /// 拾取物每个批次对应的拾取物索引。
    #[get(type(clone), pub(crate))]
    pub(crate) pickup_batches: Vec<usize>,
    /// 本帧是否收到「起跳」请求。
    ///
    /// 键位回调**不直接**改 `player.vertical_velocity`:事件闭包跑在
    /// RAF 之外,直接写物理状态会和 `step_vertical` 抢同一帧的落地面
    /// 判定。改成置一个「本帧请求起跳」的标志,由 `step_vertical` 在
    /// 自己算完支撑面之后消费 —— 积分顺序因此只有一条。
    #[get(type(copy), pub(crate))]
    pub(crate) jump_queued: bool,
    /// 是否处于第三人称跟随模式(V 键在跟随 / 第一人称之间切换)。
    #[get(type(copy), pub(crate))]
    pub(crate) third_person: bool,
    /// 第三人称跟随焦点的阻尼插值后的世界坐标。
    #[get(type(copy), pub(crate))]
    pub(crate) follow_target: Vec3,
    /// 资产 id → 原始包围盒(碰撞世界推导的唯一来源)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) asset_bounds: HashMap<String, Bounds>,
    /// 武器架(当前武器 / 弹药 / 换弹 / 瞄准)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) arsenal: Arsenal,
    /// 玩家受击状态(无敌帧 / 脱战计时 / 重生倒计时)。
    #[get(type(copy), pub(crate))]
    pub(crate) hurt: HurtState,
    /// 通缉等级。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) wanted: Wanted,
    /// 场上全部敌人(警察 + 混混)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) enemies: Vec<Enemy>,
    /// 场上全部行人。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) peds: Vec<Pedestrian>,
    /// 敌人每个实例对应的批次索引。
    #[get(type(clone), pub(crate))]
    pub(crate) enemy_batches: Vec<usize>,
    /// 手持武器的批次索引(`usize::MAX` 表示当前武器没有模型)。
    #[get(type(copy), pub(crate))]
    pub(crate) weapon_batch: usize,
    /// 当前武器对应的 mesh 索引。
    #[get(type(copy), pub(crate))]
    pub(crate) weapon_mesh: usize,
    /// 场景 mesh 索引表(切枪时按资产 id 查批次)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) index_map: HashMap<String, usize>,
    /// 行人每个实例对应的批次索引。
    #[get(type(clone), pub(crate))]
    pub(crate) ped_batches: Vec<usize>,
    /// 当前任务。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) mission: Mission,
    /// 命中标记的剩余显示时间(秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) hitmarker: f32,
    /// 本帧是否打中了敌人(喂给 HUD)。
    #[get(type(copy), pub(crate))]
    pub(crate) did_hit: bool,
    /// 累计击杀数(统计 / 任务判定)。
    #[get(type(copy), pub(crate))]
    pub(crate) kills: u32,
    /// 藏身区(后巷 / 警局门口)的圆心列表。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) hideouts: Vec<Vec2>,
    /// 已接 / 已完成的任务计数。
    #[get(type(copy), pub(crate))]
    pub(crate) missions_done: u32,
    /// 通缉是否刚刚升星(供 HUD 播一次性提示)。
    #[get(type(copy), pub(crate))]
    pub(crate) wanted_flash: f32,
    /// 地图是否处于打开状态(Tab 键)。
    #[get(type(copy), pub(crate))]
    pub(crate) map_open: bool,
    /// 还在空中的手雷(按下 G 投出,落地后结算爆炸)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) live_grenades: Vec<Grenade>,
    /// 当前瞄准的俯仰角(弧度)。
    #[get(type(copy), pub(crate))]
    pub(crate) aim_pitch: f32,
}
/// 玩家骨架的一个 part 批次:part 名 + 枢轴 + 批次索引。
#[derive(Data, Clone, Debug)]
pub struct PlayerLimbBatch {
    /// part 名(与资产 JSON 的 `name` 一致)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) part: String,
    /// 关节枢轴的资产本地坐标。
    #[get(type(copy), pub(crate))]
    pub(crate) pivot: Vec3,
    /// 父关节(肩 / 胯)枢轴的资产本地坐标;无父关节时等于 `pivot`。
    ///
    /// 资产把上臂 / 小臂切成两个独立 part,各自带绝对局部坐标,
    /// 所以渲染一节 limb 需要**两个**枢轴:自己的(肘 / 膝)与父关节的
    /// (肩 / 胯)。见 [`crate::player::limb_chain_matrix`]。
    #[get(type(copy), pub(crate))]
    pub(crate) root_pivot: Vec3,
    /// 该 part 在场景批次表里的索引。
    #[get(type(copy), pub(crate))]
    pub(crate) batch: usize,
}
/// 一辆车的四个可转车轮各占一个批次。
///
/// 车身保持「整张车一个批次」不变(它不需要单独动画),只有轮子被切出来
/// 独立变换 —— 与玩家肢体的 `PlayerLimbBatch` 同一套做法。
#[derive(Data, Clone, Debug)]
pub struct CarWheelBatch {
    /// 对应的车辆索引。
    #[get(type(copy), pub(crate))]
    pub(crate) car: usize,
    /// 第几个轮子(0..4,对应 `WHEEL_MOUNTS` 的顺序)。
    #[get(type(copy), pub(crate))]
    pub(crate) slot: usize,
    /// 该轮子在资产本地坐标的安装位(轮心)。
    #[get(type(copy), pub(crate))]
    pub(crate) mount: Vec3,
    /// 该轮子批次的场景索引。
    #[get(type(copy), pub(crate))]
    pub(crate) batch: usize,
}
/// 挂载到 canvas 上的事件驱动所需的共享引用。
#[derive(Data, Clone)]
pub(crate) struct GameHandles {
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) game: Rc<RefCell<Game>>,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) window: Window,
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) canvas: HtmlCanvasElement,
    #[get(type(clone), pub(crate))]
    pub(crate) hud: Option<Element>,
    #[get(type(clone), pub(crate))]
    pub(crate) phase_label: Option<Element>,
    #[get(type(clone), pub(crate))]
    pub(crate) phase_slider: Option<HtmlInputElement>,
    #[get(type(clone), pub(crate))]
    pub(crate) health_bar: Option<Element>,
    #[get(type(clone), pub(crate))]
    pub(crate) armor_bar: Option<Element>,
    #[get(type(clone), pub(crate))]
    pub(crate) ammo: Option<Element>,
    #[get(type(clone), pub(crate))]
    pub(crate) cash: Option<Element>,
    #[get(type(clone), pub(crate))]
    pub(crate) wanted: Option<Element>,
    #[get(type(clone), pub(crate))]
    pub(crate) mission: Option<Element>,
    #[get(type(clone), pub(crate))]
    pub(crate) hitmarker: Option<Element>,
    #[get(type(clone), pub(crate))]
    pub(crate) minimap: Option<HtmlCanvasElement>,
    #[get(type(clone), pub(crate))]
    pub(crate) map_panel: Option<Element>,
    #[get(type(clone), pub(crate))]
    pub(crate) map_detail: Option<Element>,
}
// ===========================================================================
// 程序化地面网格
// ===========================================================================
/// 展开中的程序化地面缓冲。
///
/// `build_ground` 逐步把顶点 / 面 / 逐面颜色写进这三个缓冲,最后交给
/// `expand_asset` 之外的同一套顶点格式。打包成结构体是为了让
/// `push_quad` 的参数个数落在 clippy 的阈值内(7 个)。
#[derive(Data)]
pub(crate) struct QuadBuffers<'a> {
    /// 顶点缓冲(每顶点 3 个 f32)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) positions: &'a mut Vec<[f32; 3]>,
    /// 面索引缓冲。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) faces: &'a mut Vec<[usize; 3]>,
    /// 逐面颜色缓冲。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) face_colors: &'a mut Vec<[f32; 3]>,
}
// ===========================================================================
// 资产加载
// ===========================================================================
/// manifest.json 的结构(只取用得到的字段)。
#[derive(Data, serde::Deserialize)]
pub(crate) struct Manifest {
    /// 资产总数(manifest 自报值,HUD 里做对照)。
    #[serde(default)]
    #[get(type(copy), pub(crate))]
    pub(crate) asset_count: usize,
    /// 资产条目。
    #[serde(default)]
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) assets: Vec<ManifestEntry>,
}
/// manifest 里的一个资产条目。
#[derive(Data, Clone, serde::Deserialize)]
pub(crate) struct ManifestEntry {
    /// 资产 id。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) id: String,
    /// 分类(仅用于调试输出)。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) category: String,
    /// 相对 assets/ 的文件名。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) file: String,
}
/// 一颗正在空中飞的手雷。
///
/// 抛投物必须**有自己的位置和计时**,而不是「按 G 立刻对射程内的
/// 敌人造成一次伤害」:后者是范围攻击,不是投掷。手雷有 1.1 s 的滞空,
/// 玩家在这段时间里还能跑开 —— 掩体因此有意义。
#[derive(Data, Clone, Copy, Debug)]
pub(crate) struct Grenade {
    /// 当前世界坐标(脚底高度)。
    #[get(type(copy), pub(crate))]
    pub(crate) position: Vec3,
    /// 水平速度(XZ,米/秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) velocity: Vec2,
    /// 垂直速度(米/秒,向上为正)。
    #[get(type(copy), pub(crate))]
    pub(crate) vertical_velocity: f32,
    /// 已经飞了多久(秒),到 [`GRENADE_FLIGHT_TIME`] 就炸。
    #[get(type(copy), pub(crate))]
    pub(crate) age: f32,
}
