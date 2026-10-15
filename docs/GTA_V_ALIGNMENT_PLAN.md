# GTA V 玩法对齐计划 — 2026-10-15

基于两项实测审计 + 线上部署版本复现。**每一条都带 file:line 或实测数字，没有一条是推测。**

## 状态标记

`✅ 已实测确认` / `🔴 P0 阻塞` / `🟡 依赖前置`

---

## 第一阶段：修真实存在的 bug（不是"优化"，是"程序没按自己以为的逻辑跑"）

### ✅ P0 全部五项已交付（PR #4 → `dcf61cd`、PR #6 → `6998bd5` → Pages → 线上复测通过）

| 项 | 状态 | 线上实测 |
|---|---|---|
| P0-A 路口判定死代码 | ✅ | **98 次转向 / 60 s**，6/10 车在**两个轴**上行驶（例：travelX 707 m + travelZ 640 m）。修复前每辆车锁死在一个轴上 |
| P0-B 路线随机数坍缩 | ✅ | 单元测试：256 次抽签同时出现直行与转向 |
| P0-C WASTED 瞬移 | ✅ | 跳变上界从 6932.5 m 降到 `HOSPITAL_SPAWN_MAX_OFFSET`；固定重生点数组已删除，错误路径**编译不过** |
| P0-D 静态层一帧弹出整栋楼 | ✅ | 单帧位移上界 **12.35 m → 2.4 m**；线上实测 1793 帧行走**最大 0.233 m**，0 帧超限 |
| P0-E 碰撞世界从不出流式 | ✅ | 线上实测碰撞体数随换格变化 **1798 → 1792 → 1783**（走 300 m）。修复前这个值冻结在启动时 |

线上截图确认：路口两条轴上都有车（修复前只有一条轴），街道渲染正常，控制台 0 error。

Pages 首次部署两次都因 GitHub 自身 `updating_pages` 超时失败（build job 均成功），重跑后成功 —— 与代码无关。

下一批：P1（IDM 跟车）、P2（红绿灯状态机）、P3（路权 / 停止线）、P4（转弯弧）、P5（流式回收）、P6（行人）。

### P0-A 🔴 路口判定永远不成立 —— AI 车从不转弯（已修）

`src/traffic/impl.rs:367-376`

```rust
let junction: f32 = if direction >= 0.0 {
    street_axis(street_index_at(along) + 1)   // street_index_at 用 .round()
} else { street_axis(street_index_at(along) - 1) };
let crossed: bool = if direction >= 0.0 {
    along >= junction - CAR_ROUTE_EPSILON
} ...
```

**实测**：穷举 802,000 个坐标位置，正向满足 `along >= junction-eps` 的有 **0 个**，
反向满足 `along <= junction+eps` 的也是 **0 个**。

原因：`street_index_at` 是 `(along/60).round()`，所以 `along` 恒落在
`[60k-30, 60k+30)`，而判定要求 `along >= 60(k+1)` —— 差整整一个 60 m 档。

**后果**：`choose_at_junction()` 是生产环境的死代码。10 辆车是 10 条
永不变的直线轨道，`ROUTE_PICK`（62% 直行）从来没生效过。

**修**：用「到街道中轴线的距离」判定过路口，不是「下一个街道索引」：
```rust
let centre: f32 = street_axis(street_index_at(along));
let crossed: bool = if direction >= 0.0 {
    along >= centre && along - previous_along < STREET_PITCH   // 跨过中轴
} else { along <= centre && previous_along - along < STREET_PITCH };
```
需要记住上一帧的 `along`（新增字段 `last_along`），
否则「跨过中轴线」会在中轴线上反复触发。

---

### P0-B 🔴 路线随机数坍缩到 0 —— 路口决策退化成"永远转弯"

`src/traffic/impl.rs:54-74`

```rust
fn get_route_seed(&mut self) -> u32 {
    let seed: u32 = self.route_seed;
    self.route_seed = 0;        // ← 把流状态清零
    seed
}
pub(crate) fn set_route_seed(&mut self) {
    self.route_seed = route_rand(self.route_seed);   // route_rand(0) == 0
}
```

**实测**（位精确模拟 u32 环绕运算）：
```
draws: 0.581102, 0.000000, 0.000000, 0.000000, ...
→ TURN, TURN, TURN, TURN, TURN, TURN
```
`route_rand(0) == 0`（全零是该哈希的不动点），所以第一次抽签之后
流状态永远是 0，而 `next_unit(0) == 0.0`，而 `0.0 < ROUTE_PICK(0.62)`。

**修**：`get_route_seed()` 不该清零。返回当前状态即可：
```rust
fn get_route_seed(&mut self) -> u32 { self.route_seed }
```
这样 `next_unit(seed)` → `set_route_seed()` 变成正常的
`route_rand(prev)` 递推，同一条轨迹可重放（这正是它 doc comment 声称的）。

---

### P0-C 🔴 死亡重生瞬移（这就是你报的"瞬移"）

`src/game/fn.rs:6552-6562`

```rust
let spot: Vec3 = nearest_hospital(game.get_player().get_position());
game.get_mut_player().set_position([resolved[0], GROUND_LEVEL, resolved[1]]);
```

**实测跳变距离**：

| 死亡位置 | 重生点 | 瞬移 |
|---|---|---|
| (65, 30) | (52, 52) | **25.6 m** |
| (75.7, 62.3) | (52, 52) | **25.8 m** |
| (300, 0) | (98, 98) | **224.5 m** |
| (600, 0) | (98, 98) | **511.5 m** |
| (5000, 5000) | (98, 98) | **6932.5 m** |

`HOSPITAL_SPOTS` 是**固定的 12 个点**，而世界是无限生成的
（`street_axis` 支持任意整数索引）。走得越远，重生瞬移越离谱。

**为什么"跨街区"时更容易触发**：
`repopulate_streamed_combatants`（`src/game/fn.rs:5593`）每跨一个格子
就在玩家附近刷新 7 个敌对混混（`src/spawn/fn.rs:97-111`，
半径 34–62 m，`Faction::Thug` 无条件敌对，视距 46 m，射程 26 m）。
3 秒后 `step_death` 把你传送走。

**GTA V 的实际行为**：WASTED 后在**就近的医院门口**复活，
离死亡点几百米是正常的，不会回到城市原点。

**修**：医院按街区程序化生成，不再是固定数组。
```rust
pub(crate) fn hospital_near(pos: Vec2) -> Vec2 {
    let street = street_axis((pos[0] / STREET_PITCH).round() as i32);
    let cross  = street_axis((pos[1] / STREET_PITCH).round() as i32);
    // 在当前街区找一个稳定的门口点（由世界坐标决定，可重放）
    [street + HOSPITAL_OFFSET, cross + HOSPITAL_OFFSET]
}
```
关键：`HOSPITAL_OFFSET` 必须由 `crc32(街区索引)` 决定，
不能用 `hash()`（每进程加盐，重建就变）。

---

### P0-D 🔴 动态碰撞不分子步 —— 一帧可以把玩家弹出整栋楼

`src/game/fn.rs:6834-6846` 走 `resolve_all`（`src/collision/impl.rs:547`），
**没有** `SLIDE_STEP = 0.05` 的细分步进（静态层 `resolve_slide` 有，
见 `src/collision/impl.rs:110-153`，那里有明确注释说明为什么必须细分）。

**弹出量 = 建筑半长 + 玩家半径**：5 m → 5.35 m，19 m → 19.35 m，25 m → 25.35 m。

**修**：给 `resolve_all` 同样的子步进，或者至少对玩家这一对做
`resolve_slide` 等价的处理。

---

### P0-E 🔴 碰撞世界从不出流式重建

`build_collision_world` 只在启动时调用一次
（`src/game/fn.rs:7777-7782`，唯一其他调用方是 `#[cfg(test)]`），
用的是出生点。`BLOCK_VIEW_RADIUS = 300 m` 之外玩家撞的是**看不见的幽灵几何**，
而幽灵 AABB 触发 P0-D 的弹出。

**验证方法**：传送到 x=400，读 `__vcw.collisionShapes`，
它会等于出生时的值。

**修**：把碰撞世界纳入 `step_streamed_surface`，按同样的窗口重建。

---

## 第二阶段：交通与 NPC 的真实规则（你说的"要有强大真实的算法"）

### P1 🟡 跟车模型（IDM）

**现状实测**：`src/traffic/impl.rs:305-317` 只有一个向巡航速度的 lerp。
全仓库 grep `idm|headway|car_ahead|lead_car|time_to_collision` → **0 命中**。
车与车之间唯一的交互是**事后位置推开**（`src/game/fn.rs:6848-6857`），
不是刹车。所以车会互相穿过。

**修**：Intelligent Driver Model
```rust
let gap: f32 = lead_distance - CAR_LENGTH;          // m
let dv: f32 = speed - lead_speed;                   // m/s
let s_star: f32 = MIN_GAP + speed * TIME_HEADWAY;   // 期望间距
let accel: f32 = MAX_ACCEL * (1.0 - (speed / cruise).powi(4)
                              - (s_star / gap).powi(2));
```

### P2 🟡 红绿灯状态机

**现状实测**：`prop_trafficlight` 只是静态装饰网格
（`src/game/fn.rs:1078-1084`，每个路口放 4 个）。
全仓库无灯态、无相位、无计时器。
历史审计实测：**12 次路口穿越速度恒为 11.50 m/s，零方差** ——
证明完全没有信号逻辑。

**修**：每个路口一个相位状态机，南北 / 东西对冲，含黄灯过渡：
```rust
pub enum Signal { Green, Amber, Red }
pub fn phase_at(junction: (i32,i32), t: f32) -> (Signal, Signal)
```
停止线按 `STREET_HALF_WIDTH + 2` 米布置，红灯时目标速度钳到 0。

### P3 🟡 路权

**现状**：无 `yield` / `right_of_way` 任何符号。路口就是 `traffic/impl.rs:388` 的掷硬币。

**修**：转弯车让直行车；路口占用预约（先进入者获得通行权）；
不让两车同时进入同一路口格。

### P4 🟡 车道图 + 真实转弯弧

**现状实测**：横向坐标每帧被强制吸附到车道中心
（`traffic/impl.rs:361-365`），所以 AI 车永远不会偏移、不会变道、
不会被推出车道。路口处是**位置跳变**而非转弯曲线。

**修**：路口用二次贝塞尔曲线（入口切线 → 出口切线），
车沿曲线插值朝向。保留解析网格做生成器，但行驶走折线。

### P5 🟡 交通流式回收

**现状实测**：`populate()` 只在初始化调一次
（`src/traffic/impl.rs:622-642`），`despawn|recycle|pool` grep → **0 命中**。
实测最远的车在 **1792 m** 外仍在逐帧模拟。`TRAFFIC_HALF = 600.0`
只用于启动日志断言（`src/game/fn.rs:8818-8824`）。

**修**：超出 `CAR_DESPAWN_DIST`（140 m）回收，在视锥外
60–110 m 的车道上重新播种；`TRAFFIC_LANE_PLAN` 提到 ~24 条。

### P6 🟡 行人

**现状实测**：
- 无寻路：目标是随机点，**穿楼穿马路**（`src/combat/impl.rs:872-884`）
- 无互相避让：`Pedestrian::step` 从不看邻居，只有全局位置推开
- **对枪声零反应**：`set_flee_timer` 只有两个写入点 ——
  玩家靠近（`src/combat/impl.rs:843`）和被击倒归零（`:780`）。
  grep `gun|fire|muzzle|enemy|cover|panic` 于 `Pedestrian::step` → **0 命中**
- 固定 18 个，无距离回收
- 60 m 换格时**整批瞬移**（`src/game/fn.rs:5616-5622`）

**修**：人行道路径（沿街道外侧偏移）、分离转向、
枪声/爆炸在 `PED_PANIC_RADIUS`（45 m）内广播 `noise` 事件、
行人有 HP 且进入命中扫描。

### P7 🟡 确定性 + 帧率无关

**现状实测**：`game.ticks` 累加器只自增，**没有任何地方读它**
（`src/game/fn.rs:7960-7966`）；`simulate` 每 rAF 跑一次，
`dt = delta.min(FIXED_DT*4.0)`（`src/game/fn.rs:5438`）。
所以 15 fps 以下世界变慢动作，**且不同帧率下同一时刻结果不同**。

**修**：真正的固定步长累加器驱动模拟（渲染插值），种子随机替代全零哈希。

---

## 第三阶段：交付

- `tools/verify/gate.sh` 全绿（audit + fmt + wasm build + bindgen）
- 每条修复都配一个可复跑的断言测试（不是肉眼看）
- PR → 自动合并 → Pages → 线上按同一套指标复测
