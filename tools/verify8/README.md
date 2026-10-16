# verify8 —— 8 路只读验收

对 `vice-city-web` 的感知验收套件。8 个 worker 各占一条缺陷线,互不重叠;
每个 worker 拉起**自己独占的** Chrome(独立端口 + 独立 profile),只读源码、
只驱动自己的浏览器、**不写任何源码文件**。

---

## 核心原则:验收是感知检查,不是数字闸门

这一条决定了这套东西怎么用,所以放第一位。

「三角形数在涨」「stddev > 0」「HTTP 200」「console 无报错」**对
「渲染出来的东西是不是人看的东西」毫无判断力**。它们只能当廉价的存活
护栏 —— 用来回答「页面死了吗」,不能用来回答「它对吗」。

举个具体的:上一轮实测,隐藏角色 limbs 批次(38-50)只让整屏变 ~1308px,
隐藏 peds 批次是 1168px,隐藏全部批次是 1283px。三个数落在同一量级 ——
**全屏像素 diff 的噪声底就是 ~1300px**。这个方法看起来有数字,实际上
信号完全淹没在噪声里。如果当初按「隐藏 limbs 后整屏变化 > 1000px 就算通过」
的闸门去判,三组数会互相矛盾,而哪个是真问题**一个都指不出来**。

所以 w2 走的是**反向实验**:隐藏除 limbs 外的所有批次,让画面里只剩角色,
然后读角色框**内**的像素内容 —— 那是绝对量,没有噪声底。

凡是有外观的东西,worker 都会把截图落到
`/Users/sqs/.hermes/cache/scratch/vcw-v8/shots/`,**必须**再交给
`vision_analyze` 出一段文字描述(角色朝哪、四肢可见吗、是城市还是虚空、
白天还是夜里),描述原文贴进报告才算验过。worker 自己拿不到视觉能力,
所以它们只能把截图路径交出来 —— 这一步由人(或上层 agent)完成。

---

## 判定三态

`Verdict(name, ok, note)` 的 `ok` 是三态,不是布尔:

| ok | 含义 | 什么时候用 |
|----|------|-----------|
| `True` | 通过 | 判据成立,且是**实测**读数支撑的 |
| `False` | 失败 | 判据明确不成立(功能缺失、方向反了、卡死了) |
| `None` | 未能判定 | 读不到数、场景构造不出来、机制不成立导致后续读数无效 |

**`None` 不是「跳过」也不是「通过」。** 它在说「这个问题的前提没建立起来」,
比如「出生点 8m 内没有遮挡物,所以造不出相机穿墙的场景」。把它记成 FAIL
是冤枉,记成 PASS 是撒谎。

harness 自身的问题(Chrome 起不来、CDP 断了、8877 不通)**必须**调
`vlib.fail_harness`,它会落一条 `harness: False` 并以退出码 1 退出。
**绝不能**把 harness 失败包装成应用缺陷 —— 那会让人去修一个没坏的程序。

---

## 端口分配(硬约束)

| worker | CDP 端口 | 负责的线 |
|--------|---------|---------|
| w1_walk | 9340 | 走路不卡死(缺陷 1) |
| w2_visibility | 9341(三次截图**串行**共用,一次只活一个) | 角色 mesh 在 framebuffer 里(缺陷 2) |
| w3_pointerlock | 9342 | 指针锁 + 鼠标视角 + 方向(缺陷 3) |
| w4_keys | 9343 | 键位对齐(缺陷 4) |
| w5_vehicle | 9344 | 上下车 + 驾车(缺陷 5) |
| w6_combat | 9345 | 战斗与掉血(缺陷 1 后半) |
| w7_camera | 9346 | 相机跟手 / 穿墙(缺陷 5) |
| w8_world | 9347 | 世界连贯性(缺陷 5) |

**不属于本套件的端口,不许 probe、不许 kill:**
- `8877` 静态服务器 —— 由持有者负责,worker 和脚本都只读
- `9231-9236` —— `tools/verify-parallel.sh`
- `9390` —— 修复 agent

profile 目录由 `vlib.Session` 自动分配在
`/Users/sqs/.hermes/cache/scratch/vcw-v8/<worker名>`,退出时 `rmtree`。

> w2 因为要对比三种 `?hide=` 策略(`?hide=` 只能在 URL 上,渲染器每帧重读
> query,没有运行期改写接口),必须开三次浏览器。三次**串行**,任何时刻只有
> 一个活着,所以共用 9341 —— 端口不下移、不越界。

---

## 跑法

```bash
# 0) 先由持有者起静态服务器(本套件不负责起它)
(cd /Users/sqs/code/vice-city-web && python3 -m http.server 8877)

# 1) 跑全部 8 组(并行)
tools/verify8/verify8.sh

# 2) 只跑几组
tools/verify8/verify8.sh w1_walk w5_vehicle

# 3) 复用已在跑的 Chrome
tools/verify8/verify8.sh --serve

# 4) 单独跑一个 worker
/Users/sqs/.hermes/hermes-agent/venv/bin/python3.11 tools/verify8/w1_walk.py
```

产物:
- `…/vcw-v8/results/<worker>.json` —— 逐项判定(即使全 SKIP 也必落)
- `…/vcw-v8/<worker>.log` —— stdout
- `…/vcw-v8/shots/<worker>_<tag>.png` —— 待 vision 描述的截图

`verify8.sh` 收尾时会在 `EXIT/INT/TERM` 上对 9340-9347 做精确匹配补刀,
不留占端口的孤儿。**它只按 `remote-debugging-port=<精确端口>` 匹配**,
不会误伤 9231-9236 或 9390。

---

## 每个 worker 测什么

### w1_walk —— 走路不卡死(缺陷 1)
- 四个方向各按住 N 帧采 `playerX/playerZ` 序列 → 总位移 > 2m
- 12 帧滑窗内静止帧 < 6 帧(门限按「一帧 < 0.01m 算静止」,半窗是刻意的:
  起步/转身允许抖动)
- 每方向独立判定
- Shift 冲刺 vs 步行:比**每帧位移之比**(dt 未知时与帧率无关),门限 > 1.15
- 每方向报 `afterStatic` / `afterVertical` / `afterDynamic` 三个读数
  (静态墙挡住了 vs 动态层按回去了)+ `nearShapes` 前 3

> SwiftShader 下 1-5fps,`MAX_FRAME_TIME=0.25s`。**所有预算按帧数算,
> 不按墙钟** —— 30 秒墙钟只有 12 秒模拟时间。

### w2_visibility —— 角色 mesh 真在 framebuffer 里(缺陷 2)
- **(a) 先证明反向实验机制有效**:隐藏除 38-50 外的所有批次,角色框**外**
  区域必须变 > 5000px。机制不成立就**停在这里**并如实标 FAIL —— 后面所有
  hide 读数都失去证据资格
- **(b) 角色框内**用**对比判据**:框内 vs 紧邻框外的均值差 > 12,或框内
  标准差比框外高 > 4。**不能只看 min/max 极差与非黑占比** —— 天空本身
  就是一片亮度渐变,极差轻松过 100、非黑占比必然 100%。初版 worker 就是
  这么判的,实测**假通过**(见下方「已验出的应用缺陷」)
- **(c)** `limbState` 逐节报 `n`(实例数)与模型矩阵平移 `t`,对照玩家坐标:
  `n==0` = 骨骼没写进实例表;`n>0` 但 `t` 偏离玩家 > 1m = 矩阵写错;两者
  都对才轮到怀疑渲染侧
- 另做一组「只留 limbs vs 只留其余」的框内对照,证明框里的东西确实来自
  limbs 批次
- **必须**:把 baseline / only_limbs / hide_limbs_only 三张截图交给
  `vision_analyze`,用文字描述角色朝向与四肢可见性

> **`charBox` 不是矩形。** 它是 `[脚底x, 脚底y, 头顶x, 头顶y, depth]` 两个点。
> 人笔直朝相机走时两个点的 x **完全相同**(实测 feet.x == crown.x == 256),
> 框宽算出来是 0 —— 撑到 1px 只能得到一条 3px 宽的竖线,里面装不下一个人。
> w2 的 `_pad_box()` 负责:两点撑成矩形、**按身高 0.25 倍反推宽度**、再从
> canvas 分辨率(512×263)缩放到截图分辨率(1280×657,`captureScreenshot`
> 给的是整个页面含 canvas 之外的区域)。「按身高反推宽度」是本套件**唯一
> 引入的几何假设**,写在代码注释里以便对照。

### w3_pointerlock —— 指针锁 + 方向不反向(缺陷 3)
- (a) 点 canvas 后 `document.pointerLockElement !== null`
- (b) Escape 后变回 null
- (c) 按住拖拽时 `cameraYaw` **连续**变化(不是只在最后跳一下)
- (d) **方向真值表**:四个量的**实测符号**逐个列出 ——
  鼠标右移 → ΔcameraYaw 的符号 / 画面往哪转(截图 + vision)/
  按 W → 世界位移与相机前向的夹角(**两个候选朝向都算、都报**)/
  按 W → `charBox[4]`(NDC depth)是否下降
- 判据 A「鼠标右移 = 视角右转」、判据 B「按 W = 角色往屏幕上方远去」

> **不许靠「加上一个负号试试」。** 两个候选朝向的余弦值都打进表里,读者
> 能看到是谁对谁错 —— 那才是实测符号,而不是把结果凑成想要的样子。
> 数学符号只能证明数学关系,**证明不了玩家看到画面往哪转**,所以还要截图。

### w4_keys —— 键位对齐 GTA V(缺陷 4)
- 先从 `src/game.rs` / `src/const.rs` 抠出真值表(`TRACKED_KEYS` + `keydown`
  的 `match` 分支),再拿实测对这份表 —— **不拿 GTA V 官网的键位表**
- 逐键 A/B,每键报一张表:键名 / 哪个字段变了 / 变前 / 变后 / 符合 GTA V 是或否
- **键码和数值常量都从源码读**(`_key_const` / `_num_const`),不在 harness
  里留副本。换弹键从 `KeyC` 迁到 `KeyR` 那一次,就是因为写死了一份键名表
- 项目里没有的键**报缺失(FAIL),不判 PASS**;但「有键位」也不等于
  「接了线」—— 必须有 A/B 读数撑着,grep 只作诊断写进 note
- R 换弹单独走 `results_reload_r`:需要非满匣基线、每帧重发的一帧开火注入、
  以及对 `#vcw-ammo` 的轮询(通用 A/B 循环做不到这三件事)

### w5_vehicle —— 上下车 + 驾车(缺陷 5)
- 传送到最近的车旁(2.5m < `ENTER_VEHICLE_RADIUS=4.2`)→ F → `playerVehicle` 非 null
- 按 W → 变的是 `carDriveX/carDriveZ/carDriveSpeed`,且同期 `playerX`
  单独不动(证明动力来自车)
- F → `playerVehicle` 回 null
- **车不穿楼**:用游戏**自己**的碰撞世界当判据 ——
  `__vcw_teleport={probe:true}` 触发 `probe_nearby_shapes`,把玩家附近每个
  AABB 打成 `2DB c=(cx,cz) x=[x0,x1] z=[z0,z1]` 写进 `__vcw_probe`。
  采样车心,判它是否落进某个 AABB 内部(收缩 0.5m 容差,贴墙行驶不算穿)

> **为什么不用 assets JSON 的 bounds 当判据**:楼的世界位置来自
> `build_city_buildings_with` —— `hash2`/`hash_unit` 伪随机、三重剔除、
> 带 scale 与 yaw 抖动。在 Python 里复刻它等于把游戏的 RNG 抄一遍,抄错了
> 会得出「车在楼里」的错误结论,而真实原因是脚本里的楼和游戏里的楼不是
> 同一栋。用 `game.world` 才是真源 —— 那正是碰撞解算在用的数据。

### w6_combat —— 战斗与掉血(缺陷 1 后半)
- (a) **先开无敌**,站定 60 帧采样 `playerHealth` —— 证明「无人攻击时不掉血」,
  与「挨打掉血」分开
- (b) 传送到敌人(THUG_HEALTH=70)附近,**两个候选 yaw 都试**,取真打中的那个;
  开火走 `__vcw_teleport={hold:true}`(CDP 的 `dispatchMouseEvent` 进不了
  canvas `mousedown`,`fire_held` 恒为 false)
- (c) **隔墙**:用 slab 法证明墙 AABB **真的与玩家→敌人的连线相交**且两端
  各留 1.5m。构造不出真实隔断就记 SKIP,绝不假通过
- (d) 武器切换:`Digit2` 切 SMG + 开火后 mag 减少
- **掉血归因**:玩家血量只由 `hurt_player` 修改(护甲优先 + 0.45s 无敌帧),
  无坠楼/环境伤害分支 —— 所以掉血只可能是敌人开枪打中了

### w7_camera —— 相机跟手不抖、不穿墙(缺陷 5)
- (a) 按 W 走 60 帧逐帧采样 `cameraDist` → 相邻帧差 > 0.5m 的帧占比 < 20%
  (跟手不是看「距离变多快」,是看**相邻帧差**);末段与 `cameraDistTarget`
  的差 ≤ 1.2m(收敛)
- (b) 主动造遮挡(转 10 个朝向)→ `camOccluded=true` 时 `camClearance > 0`。
  造不出遮挡场景就记 SKIP
- (c) 滚轮:发 `WheelEvent`,前后对比 `cameraDistTarget`,并检查 clamp 在
  `[2.2, 22.0]` 内。第三人称下滚轮写的是 desired_distance,所以读 target
- (d) Tab 切第三人称能切能恢复;V 单独记缺失
- **必须**:走前/走后两张截图交给 `vision_analyze`

### w8_world —— 世界连贯性(缺陷 5)
- (a) **不按「三角形数在涨」判泄漏** —— 涨是正常的(流式地面在铺)。按
  **单帧增量** > 2000 和**静止时后半程净增** > 4000 判
- (b) 传送 2 次到 ±180m 的远处 → `loadedAssets` / `streamX,streamZ` 变化,
  且画面仍有内容
- (c) 传送到 `healthPack`(专用字段)或 `pickupPos` 最近点 → `pickups.taken`
  增加,且玩家确实踩进 `PICKUP_RADIUS=2.2m` 内
- (d) 按 T → `phase` 变化 + 画面平均亮度变化 ≥ 3
- (e) `hud` 非空
- **必须**:初始 / 传送后 / 昼夜前后截图交给 `vision_analyze`

---

## vlib 已知缺口(本套件绕过,未改 vlib)

`vlib.py` 只读。以下全部是**实跑撞出来的**,不是猜测。补齐后各 worker 里
的本地 wrapper 就可以删掉。

### A. `Session.st()` 漏读的字段(已实测确认)

`st()` 一次 ev 出 33 个字段,但 `window.__vcw` 实际有约 60 个。缺的字段:

| 缺的字段 | 谁在用 |
|---------|--------|
| `carDriveX` / `carDriveZ` / `carDriveSpeed` | w5(**上车/驾车唯一的证据**,缺了直接 KeyError) |
| `combat` 整块(weapon / mag / reload / kills / hp / armor / cash / mission) | w4 换弹、w6 武器切换 |
| `enemies` / `peds` 数组 | w6 全部战斗判据 |
| `wanted`(stars / heat / unseen / hidden) | w6 隔墙归因 |
| `hud` | w8 (e)、w4 notice |
| `phase`(昼夜档位) | w8 (d) |
| `loadedAssets` / `streamX` / `streamZ` | w8 (b) 流式加载 |
| `pickups{total,taken}` / `healthPack` / `pickupPos` | w8 (c) 拾取 |
| `camClearance` | w7 (b) 相机穿墙 |
| `camEyeX/Y/Z` | w5/w7 眼点位置 |
| `bpos`(最近建筑) | w5 撞击目标 |
| `bn`(批次数) / `limbBatches` | w8 (a) |
| `playerCash` | w8 (c) 拾取收益 |

`st()` 现在有 `coccl`(camOccluded)但**没有** `camClearance` —— 成对的
字段只取一半,比都不取更容易误导:w7 会以为「有 camOccluded 就够判穿墙」。

### B. `diff_pixels()` 空输入会抛异常(**已实测炸过**)

```python
def diff_pixels(a, b, tol=10):
    ...
    return over, 100.0 * over / len(a), mx   # len(a)==0 -> ZeroDivisionError
```

角色投影失败 / 完全在画布外时,「框内」本来就一个像素都没有,`len(a)==0`
是一个**有意义的判定状态**,不是异常。它现在会把整个 worker 打成
harness failure,把真判定吞掉(w2 实测炸过两次)。

**建议**:`if not a or len(a) != len(b): return 0, 0.0, 0.0`。
（w2 已用本地 `_diff()` 绕过,未改 vlib。）

### C. `box_stats()` 的 box 语义需要说明

`box_stats(w, h, luma, box)` 期望 `box = [x0, y0, x1, y1, _]`,即**已归一化
的矩形**。但 `__vcw.charBox` 的真实格式是
`[feet.x, feet.y, crown.x, crown.y, depth]` —— **两个点,不是矩形**。

直接喂进去会算出 `宽 = |crown.x - feet.x|`,而正对镜头时**恰好是 0**,
得到一个空框,进而触发 B 里的 ZeroDivisionError。

w2 的 `_pad_box()` 现在负责两件事:两点撑成矩形(至少 1px)、再从 canvas
内部分辨率(`canvasWidth/Height`,res=0.4 时约 512×263)缩放到截图分辨率
(实测 1280×657 —— `Page.captureScreenshot` 给的是**整个页面**,含 canvas
之外的区域)。

**建议**:`st()` 增一个 `charBoxRect`(已归一化、已换算到截图坐标系),
或在 `st()` 的 docstring 里写明 charBox 是两点格式 + 坐标系。

### D. 缺的封装

1. **没有滚轮封装** —— w7 直接发 `Input.dispatchMouseEvent(type=mouseWheel,
   deltaY=...)`。
2. **没有拖拽封装** —— `click` / `mouse_move` 都有,缺「按住 + N 段 move +
   抬起」。w3 自己写 `_drag()` 发 pointer 序列;这段代码是 w3 (c) 能测出
   `ΔcameraYaw=+1.4400 rad`(恰好 0.006×240)的直接原因 —— 少了它会误报
   「鼠标视角完全没反应」。
3. **`Session` 没有额外 query 参数** —— w2 只能改 `s.url` 后重新 `boot()`,
   逼它三次重开浏览器(串行)。给 `Session.__init__` 加个 `extra_query`
   会干净很多。
4. **没有开火注入封装** —— w6 直接写 `window.__vcw_teleport={hold:true}`。

### E. `Session.__aenter__` 不导航 —— 极易漏,且失败方式很隐蔽

`__aenter__` 只做 preflight + 起 Chrome + 连 CDP,**不导航**。
漏掉 `await s.boot()` 的后果:页面停在 `about:blank`,`frames` 恒为 0,
而 `wait_frames` 的轮询条件是 `frames >= target`,于是它**永远转下去**
—— worker 不退出、不落 JSON、脚本的 `wait` 一直挂着。

w1 第一次跑就踩了这个坑(实测 400 秒无输出、无 results 文件,最后手动
kill)。**建议**:要么 `__aenter__` 里自动 `boot()`,要么 `wait_frames`
在 `frames` 连续 N 轮不增长时抛错并提示「页面可能没 boot」。

（上面任一条补上之前,各 worker 的 `asyncio.wait_for` 看门狗
（`WALL_CLOCK_LIMIT_S=1500`）会兜住,超时按 harness failure 落盘。）

---

## 已验出的应用缺陷(实跑,带读数)

最后一轮实跑:8 个 worker 全部落盘,**0 处 harness failure**,95 项判定
(33 通过 / 20 失败 / 42 未能判定)。下面是**有实测读数支撑**的结论。

### 缺陷 2(角色不可见)—— 复现了,而且证据链完整

| 环节 | 读数 | 结论 |
|------|------|------|
| 反向实验机制 | 隐藏 38 个非 limbs 批次 → 角色框外变化 **580,878px(69.4%)** | 批次隐藏机制**有效** |
| `limbState` | 13 节骨骼 **n 全为 1**,`n==0` 的有 **0** 节 | 骨骼**已写进实例表** |
| 模型矩阵 | torso/head/hair 的 `t=[33.5, 0, 45]`,与玩家 (33.5, 45) 偏差 **0.00m** | 矩阵**跟着角色走** |
| 帧缓冲内容 | charBox 区域均值 **200.9**;全图亮度直方图 **88% 的像素**落在 luma 192-224 同一档 | 框内与背景**统计上不可区分** |

三段证据都指向「实例表和矩阵都对,问题在渲染侧」。`vision_analyze`
对「只留 limbs」截图的原文描述(裁切放大 4× 后):

> 这张图显示为一个**纯黑色的正方形画面**,整体没有可辨认的明暗变化、纹理、
> 线条或其他图案。画面中没有文字、物体或背景细节可供识别。
> - **看不到人形轮廓**;**看不到头、躯干、手臂或腿**;也无法判断人物朝向。

对整图的描述同样确认:「画面中**没有能够清楚辨认的人体**」。

**所以缺陷 2 成立,而且比预想的更严重** —— 不是「角色有点小」或「被
挡住」,是在**只留骨骼批次**的画面里根本找不到人。

> ⚠️ 顺带记一条**假通过**:初版 w2 用「luma 极差 > 40 且非黑 > 15%」判
> 「框内有内容」,这一条**通过了**,但 vision 说框里什么都没有。原因是
> 天空渐变本身就满足那两条(极差 172、非黑 100%)。已改成对比判据。
> 这正是「验收是感知检查不是数字闸门」的实例 —— 数字判据自己没被骗,
> 是我给的判据选错了。

### 缺陷 5(上下车 / 驾车)—— (a)(b)(c) 全通过

| 项 | 读数 | 判定 |
|----|------|------|
| 上车 | 距车 2.50m(门限 4.2m),`playerVehicle None -> 0` | pass |
| 驾车 | 车位移 **27.15m**,`carDriveSpeed` 序列 2.21→4.41→…→**22.01 m/s** | pass |
| 动力来源 | 同期 player 位移同为 27.15m,且 (33.51,30.18) == 车心 (33.51,30.18) | pass |
| 下车 | `playerVehicle 0 -> None`,下车点离车心 2.40m,`grounded=True` | pass |
| 不穿楼 | 未能判定 —— 见下 |

(d) 撞楼测试**未能构造出场景**:`__vcw_probe` 只报查询点 1.15m 内的碰撞体,
而车停在空旷车道上时正好 0 个。w5 已改成**朝 `bpos`(游戏自己算的最近
建筑)开、一边开一边探**,但这轮还没跑到那一步。

### 缺陷 3(方向)—— 鼠标视角正常,指针锁不存在

- (c) 拖拽 240px:`ΔcameraYaw` 序列 `[0.180 × 7]`,总 **+1.4400 rad** ——
  与 `camera.yaw += dx*0.006` 的理论值 0.006×240 = **1.440** 精确吻合。
  **鼠标视角工作正常。**
- (a) 点 canvas 后 `pointerLockElement` 恒为 `null` —— 这条读数是
  pre-fix 的。现在 `src/game.rs:3939-3953` 在 canvas `click` 上调
  `request_pointer_lock()`,并用 `pointerlockchange` 同步 `dragging`,
  指针锁**已实现**;该条需要重测。
- (d) W 位移与相机前向的夹角 cos ≈ **0.000** —— 三个字符集采样
  W 位移只有 0.372m 就停住(撞上东西),方向没测出来。

### 缺陷 4(键位)—— 七项缺失**已全部修复**,w4 现在应当全绿

这七条结论是 pre-`43c340a` / pre-`0e45d69` 时代的。当时实测 Space 时
`y 0->0`、`grounded True->True`、`vy 0->0`,证明按了确实什么也没发生。
应用侧后来把这些键全部接上了,**并且把 R 从「重置视角」改成了换弹**
(GTA V 的键)。当前真相见下方「键位与指针的真相表」—— w4 现在的判据
已按那张表重写,包括把 `KeyC 换弹` 改成 `R 换弹`(配 `C 不再换弹` 的
反向对照),以及把 `R 重置视角` 改成 `R 不再重置/拉走相机(回归项)`。

---

## 键位与指针的真相表(源码实证,不是环境问题)

这张表是键位验收的唯一真源。**下面每一行都按 HEAD `0e45d69` 的源码核对过**,
不是照抄上一轮的结论 —— 上一轮这张表整张停留在应用修好之前的状态,对着
一个正确的应用报了七条假 FAIL。

| 项 | 真相 | 证据 |
|----|------|------|
| 指针锁 | **已实现**:canvas `click` 调 `request_pointer_lock()`,并监听 `pointerlockchange` 同步 `dragging` | game.rs:3939-3967 |
| Space 跳跃 | **已实现**:`KEY_SPACE => game.jump_queued = true`,物理在 `step_vertical` 积分 | game.rs:4412-4416 |
| KeyG 手雷 | **已实现**:`KEY_GRENADE => throw_grenade` | game.rs:4417-4420 |
| KeyH 徒手 | **已实现**:`KEY_UNARMED => set_weapon(Weapon::Unarmed)` | game.rs:4421-4428 |
| Tab 地图 | **是地图**,不是切视角:`KEY_MAP => map_open` 取反 + `sync_map_visibility` | game.rs:4405-4411 |
| KeyV 切相机 | **就是 KeyV**:`KEY_CAMERA => toggle_camera_mode`(V 一直在,不是绑在 Tab) | game.rs:4401-4404 |
| 方向键 | **只在驾驶时接管转向**:`steer_axis()` 把 ←/→ 与 A/D 并列,且只有 `car.drive` 消费它;步行分支只读 WASD | game.rs:1885-1899 / 5929 / 5950-5954 |
| R 换弹 | **R 就是换弹**(`KEY_RELOAD = "KeyR"`);旧的 KeyC 已无任何 keydown 分支 | const.rs:1099 / game.rs:4376-4381 |
| R 重置视角 | **功能已删除**:`camera.reset()` + `apply_default_view()` 一起删掉了 | game.rs:8367(注释记录) |

GTA V PC 的 R 是换弹 —— 本项目现在**也是** R 换弹,原「R = 重置视角」
这条不对齐的判定**已在 `0e45d69` 修掉**。旧实现按 R 会把相机从
`FOLLOW_DISTANCE = 8.2` 推到 22 m(`apply_default_view` 设 250,
`update_camera` 每帧钳到 `FOLLOW_DISTANCE_MAX = 22`),而且再也回不来;
现在这两个函数都不存在了,w4 以「R 不再重置/拉走相机(回归项)」守住它。

**本表由 `vcw-verify8-harness` 的维护者在 HEAD `0e45d69` 逐行按源码核对
(2026-10-06),证据列的行号全部来自该 commit。** 这张表此前**整张都是
pre-`43c340a` / pre-`0e45d69` 的状态**,其中七行断言的缺陷已被应用修好,
照抄会让 8-worker 套件对着一个正确的应用报七条假 FAIL。
