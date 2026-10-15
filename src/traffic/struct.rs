use super::*;
/// 一辆参与交通仿真的车。
#[derive(Data, Clone, Debug)]
pub struct TrafficCar {
    /// 资产 id。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) asset: &'static str,
    /// 当前世界坐标。
    #[get(type(copy), pub(crate))]
    pub(crate) position: Vec3,
    /// 沿车道前进的当前速度(米/秒,恒为正,倒车 = 换方向)。
    #[get(type(copy), pub(crate))]
    pub(crate) speed: f32,
    /// 巡航速度(米/秒)。
    #[get(type(copy), pub(crate))]
    pub(crate) cruise: f32,
    /// 行车的固定 X 坐标(米)—— 玩家驾驶时不再被强制拉回。
    #[get(type(copy), pub(crate))]
    pub(crate) lane_x: f32,
    /// 行驶方向:`1.0` = 沿 +Z,`-1.0` = 沿 -Z。
    ///
    /// 只描述 AI 巡航状态。玩家驾驶时以 [`Self::yaw`] 为准。
    #[set(skip, pub(crate))]
    #[get(type(copy), pub(crate))]
    pub(crate) direction: f32,
    /// 车身朝向(弧度,绕 Y 轴)。AI 巡航时由 `direction` 推出,玩家驾驶
    /// 时由转向输入连续改变 —— 这是「车能转向」的载体。
    #[get(type(copy), pub(crate))]
    pub(crate) yaw: f32,
    /// 垂直车身的侧向速度(米/秒)。
    ///
    /// 转向把一部分速度甩到这个轴上,它按 [`LATERAL_GRIP`] 指数衰减回 0。
    /// 非零即代表车正在滑,也就是漂移。
    #[get(type(copy), pub(crate))]
    pub(crate) lateral: f32,
    /// 车轮累计转角(弧度)—— 渲染用,`距离 / 轮半径` 积分出来。
    #[get(type(copy), pub(crate))]
    pub(crate) wheel_spin: f32,
    /// 玩家是否正在驾驶这辆车。
    #[get(type(copy), pub(crate))]
    pub(crate) driven: bool,
    /// 这辆车为了接正在招手的玩家而停靠,已经停了多久(秒)。
    ///
    /// 用来给停靠**设上限**:玩家一直站在路边不动时,车不该永远停在那儿
    /// 把整条车道堵死。计时归零后这辆车放弃等客,恢复正常巡航,车道让给
    /// 后面那辆 —— 现实里的出租车也是这么做的。
    ///
    /// `0.0` = 当前不是停靠状态(它在正常跑)。
    #[get(type(copy), pub(crate))]
    pub(crate) hailing_seconds: f32,
    /// 车当前所在街道的网格索引(`street_axis` 的参数)。
    ///
    /// AI 巡航的**路网坐标**:配合 [`Self::lane_side`] 与
    /// [`Self::lane_axis_index`] 就能完全确定车在网格上的位置与朝向,
    /// 不需要再存一条「世界坐标 + 直线方向」的退化车道。
    #[get(type(copy), pub(crate))]
    pub(crate) route_street: i32,
    /// 车道在街道中轴线的哪一侧(`-1.0` / `+1.0`)。
    #[get(type(copy), pub(crate))]
    pub(crate) route_side: f32,
    /// 行进轴:`0.0` = 沿 Z 走(南北向街道),`1.0` = 沿 X 走(东西向街道)。
    #[get(type(copy), pub(crate))]
    pub(crate) route_axis: i32,
    #[get(type(copy))]
    /// 转向抽签用的伪随机流状态(每辆车一份)。
    #[set(skip, pub(crate))]
    #[get(skip, pub(crate))]
    pub(crate) route_seed: u32,
    /// 上一帧的「沿街坐标」(米)—— 判定是否**跨过**了街道中轴线用。
    ///
    /// 路口判定必须比较「本帧位置」和「上一帧位置」,只看本帧位置
    /// 无法区分「正在越过中轴线」和「停在中轴线上」:后者会让
    /// `choose_at_junction()` 每帧重复触发。
    #[get(type(copy), pub(crate))]
    #[set(pub(crate))]
    pub(crate) last_along: f32,
}
/// 地面拾取物。
#[derive(Data, Clone, Debug)]
pub struct Pickup {
    /// 资产 id。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) asset: &'static str,
    /// HUD 上显示的名字。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) label: &'static str,
    /// 世界坐标。
    #[get(type(copy), pub(crate))]
    pub(crate) position: Vec3,
    /// 是否已被拾走。
    #[get(type(copy), pub(crate))]
    pub(crate) taken: bool,
    /// 自转相位(弧度),让拾取物在原地缓缓转。
    #[get(type(copy), pub(crate))]
    pub(crate) spin: f32,
}
/// 车队与拾取物的集合。
#[derive(Data, Clone, Debug)]
pub struct Traffic {
    /// 车队车辆。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) cars: Vec<TrafficCar>,
    /// 地面拾取物。
    #[get(pub(crate))]
    #[set(pub(crate))]
    pub(crate) pickups: Vec<Pickup>,
}
