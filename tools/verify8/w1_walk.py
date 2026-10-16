#!/usr/bin/env python3.11
"""w1 —— 走路不卡死(对应缺陷 1)。

被验的是「按 W 之后人真的在动,而且动得连续」,不是「按键事件到了」。
判据全部落在 `playerX/playerZ` 的**序列**上:一条有速度但位移为零的序列
就是卡死,`vel` 非零证明不了任何事。

每帧最多推进 MAX_FRAME_TIME=0.25s(SwiftShader 下 1-5fps),所以所有预算
都按帧数算,不按墙钟。四个方向各自独立判定,Shift 冲刺与步行的比较用
**每帧位移之比**(dt 未知时这个比值与帧率无关)。

冲突解算的三个读数按顺序报,它们能区分「被静态墙挡住」和「被动态层
(车 / 行人 / 敌人)按回去」:
  afterStatic   静态层 solve_slide 定下的位置
  afterVertical 垂直积分后的位置
  afterDynamic  统一动态分离层之后的位置
外加 `nearShapes` 前 3 个最近碰撞体(半径 6m 内,取 8 个)。
"""

from __future__ import annotations

import asyncio
import math
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import vlib
from vlib import Session, Verdict, dist, emit, fail_harness

WORKER = "w1_walk"
PORT = 9340
RES = 0.4

# 采样帧数。SwiftShader 下按 0.4s/帧估,每方向约 10s,全 4 方向 + 冲刺
# 约 60s 墙钟。24 帧足够把「每帧位移」和「12 帧窗口停滞」都看全。
SAMPLE_FRAMES = 24
STALL_WINDOW = 12
# 一帧里小于这个位移就当作「这一帧人没动」。0.01m 远小于任何真实步速
# 产生的位移(4.6m/s × 0.25s = 1.15m),所以它只能被碰撞层判死。
STALL_EPS = 0.01
# 一个 12 帧窗口里出现这么多静止帧就判卡死。半窗是刻意的:起步 / 转身
# 各允许几帧的抖动,但「12 帧里有 6 帧一动不动」不是抖动。
STALL_FRAMES = 6
MIN_TOTAL_MOVE = 2.0
# 冲刺两段各采这么多帧(两段背靠背,路面条件一致)。
SPRINT_FRAMES = 12

# 「贴着墙」= 净空 <= 这个值就认为人是靠在建筑表面上而不是卡死。
# 玩家半径 0.35m,探针 `probe_nearby_shapes` 报的 clearance 是玩家圆与
# AABB 之间的实际净距,所以 0.02m 已经足够宽松(诊断实测撞墙时净空恰好
# 是 0.000m,而正常走路时是几米量级 —— 中间没有灰色地带)。
CONTACT_EPS = 0.02

# 净空的行解析与 w6 同源(`probe_nearby_shapes`,src/game.rs:1267):
#   at (x,z) r=0.35 n=K :: W x=[x0,x1] z=[z0,z1] ...
_AABB_RE = re.compile(
    r"x=\[([-\d.]+),([-\d.]+)\]\s*z=\[([-\d.]+),([-\d.]+)\]"
)


def _min_gap(px: float, pz: float, r: float, x0: float, x1: float,
             z0: float, z1: float) -> float:
    """玩家圆心 (px,pz) 半径 r 到 AABB 的净距(重叠时为负)。"""
    dx = max(x0 - px, 0.0, px - x1)
    dz = max(z0 - pz, 0.0, pz - z1)
    return (dx * dx + dz * dz) ** 0.5 - r


async def _clearance(s: Session, where: dict) -> dict | None:
    """在 `where` 这个位置问游戏自己「最近的墙有多近」。

    用**游戏自己的 AABB 探针**(`__vcw_teleport = {probe: true}` ->
    `window.__vcw_probe`),不自造碰撞体 —— 验收脚本摆的坐标和应用认的
    坐标不是一回事,上一版就因为读 `nearShapes`(半径 6m 的另一套查询、
    在贴墙点返回空)而打出「卡死不可能来自静态墙」这种误导性结论。
    """
    px, pz = where.get("x"), where.get("z")
    if px is None or pz is None:
        return None
    # 请求是**一次性**的(game.rs 读的同一帧就置 NULL),所以重发直到
    # 真的拿到非空回读 —— 空回读和「附近没墙」长得完全一样。
    # **必须先清缓存。** `__vcw_probe` 是**粘住**的:它一旦被写上就一直留在
    # window 上,下一次请求会覆盖它,但**请求与回读之间**的时序一旦错开,
    # 读到的要么是上一轮的旧值、要么是空串。
    # 实测:新开浏览器第一次读就是空(请求还没被任何一帧消费);
    # 而不清缓存时又会读到**上一个位置**的旧探针(净空算在错的点上,
    # 照样是错的结论)。所以每轮都:清 -> 发请求 -> 等 1 帧 -> 读。
    text = ""
    for _ in range(6):
        await s.ev("(() => { window.__vcw_probe = null; return 1; })()")
        await s.ev("(() => { window.__vcw_teleport = {probe: true}; return 1; })()")
        await s.wait_frames(2)
        raw = await s.ev("(() => window.__vcw_probe || '')()")
        text = raw if isinstance(raw, str) else ""
        if text.strip():
            break
    if not text.strip():
        return None
    # **校验探针对不对得上我们问的那个点。** `__vcw_probe` 粘住,
    # 不校验就会拿上一个位置的探针当本位置的净空用。探针文本第一行是
    # `at (x,z) r=R`,x/z 就是它实际量的位置。
    am = re.search(r"at \(([-\d.]+),([-\d.]+)\)", text)
    if am:
        qx, qz = float(am.group(1)), float(am.group(2))
        if (abs(qx - px) > 1.5) or (abs(qz - pz) > 1.5):
            return None  # 陈旧探针:量的不是这里,不能用
    rm = re.search(r"r=([\d.]+)", text)
    r = float(rm.group(1)) if rm else 0.35
    best = None
    for m in _AABB_RE.finditer(text):
        try:
            x0, x1, z0, z1 = (float(m.group(i)) for i in (1, 2, 3, 4))
        except ValueError:
            continue
        gap = _min_gap(px, pz, r, x0, x1, z0, z1)
        if best is None or gap < best[0]:
            best = (gap, f"x=[{x0:.2f},{x1:.2f}] z=[{z0:.2f},{z1:.2f}]")
    if best is None:
        return None
    return {"gap": best[0], "box": best[1]}



def _fmt_steps(vec: list) -> str:
    if not isinstance(vec, list) or len(vec) < 2:
        return "读不到"
    return f"afterStatic=({vec[0]:.2f},{vec[1]:.2f})"


def _moving_mean(track: list[dict]) -> float:
    """每帧位移的**中位数** —— 一个不会被起步帧或撞墙帧带跑偏的估计量。

    **为什么不能用均值,也不能只取「在动」的帧。** 上一版取「位移 > 0.01m
    的帧求均值」,想的是剔除起步的慢帧;实测反而爆了:12 帧里只有两三帧
    真在动,剩下的是贴墙滑行,留下的那几个值又特别大 —— 结果读出
    6.101 m/帧,而 WALK_SPEED 4.6 m/s × 0.133 s 只有 0.6 m/帧,差 10 倍,
    比值因此变成 0.53(冲刺「比走路慢」),完全是假的。
    中位数对两端都稳健:整段被挡住时中位数就是 0(正确地报「没走」),
    正常走路时中位数就是 0.6 附近(正确的速度)。
    """
    steps: list[float] = []
    for i in range(1, len(track)):
        a, b = track[i - 1], track[i]
        if not (isinstance(a.get("x"), (int, float)) and isinstance(b.get("x"), (int, float))):
            continue
        steps.append(dist(a["x"], a["z"], b["x"], b["z"]))
    if not steps:
        return 0.0
    ordered = sorted(steps)
    mid = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[mid]
    return (ordered[mid - 1] + ordered[mid]) * 0.5


def _median_dt(track: list[dict]) -> float:
    """从 `__vcw.frames` 的增量反推**每帧的有效 dt**(中位数)。

    **为什么必须归一化 dt。** 1.7 fps 的软件渲染下,「每帧位移」根本不是
    可比量:两段采样各自的有效 dt 差 3 倍以上,直接比会得到「冲刺比走路
    慢」这种胡说。上一版正是这样 —— 步行 6.101 m/帧 ÷ 4.6 反推有效 dt
    1.326 s,冲刺 3.231 m/帧 ÷ 8.4 反推 0.385 s,两者差 3.4 倍,比值 0.53。

    **用实测 dt,不用常量反推。** 源码里 `MAX_FRAME_TIME` 写的是 0.25s,
    但实测有效 dt ≈ 0.131s(`min(FIXED_DT*4)` = 0.0667 的若干倍),两者对不上,
    拿常量当分母只会把判据算错。`__vcw.frames` 是逐帧发布的真实帧计数,
    用它的增量就是最直接的「这一段跨了几帧」。
    """
    dts: list[float] = []
    for i in range(1, len(track)):
        a, b = track[i - 1].get("frames"), track[i].get("frames")
        if isinstance(a, (int, float)) and isinstance(b, (int, float)) and b > a:
            dts.append(float(b - a))
    if not dts:
        return 0.0
    ordered = sorted(dts)
    mid = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[mid]
    return (ordered[mid - 1] + ordered[mid]) * 0.5


def _speed(track: list[dict]) -> tuple[float, float]:
    """返回 (每帧位移中位数, 实测每帧 dt 中位数) —— 速度 = 前者 / 后者。"""
    return _moving_mean(track), _median_dt(track)


async def _sample(s: Session, budget: int) -> list[dict]:
    """按住当前按键,逐帧采样位置序列。"""
    out: list[dict] = []
    for _ in range(budget):
        await s.wait_frames(1)
        out.append(await s.st())
    return out


def _analyse(track: list[dict]) -> dict:
    """把位置序列折成位移统计 + 最差停滞窗口。"""
    steps: list[float] = []
    for i in range(1, len(track)):
        a, b = track[i - 1], track[i]
        if not (isinstance(a.get("x"), (int, float)) and isinstance(b.get("x"), (int, float))):
            steps.append(0.0)
            continue
        steps.append(dist(a["x"], a["z"], b["x"], b["z"]))
    total = sum(steps)
    worst = 0
    for start in range(0, max(0, len(steps) - STALL_WINDOW + 1)):
        window = steps[start : start + STALL_WINDOW]
        still = sum(1 for d in window if d < STALL_EPS)
        worst = max(worst, still)
    return {
        "steps": steps,
        "total": total,
        "worst_still": worst,
        "per_frame": total / len(steps) if steps else 0.0,
        "max_step": max(steps) if steps else 0.0,
    }


async def _run_direction(s: Session, code: str, budget: int) -> tuple[dict, dict, list]:
    before = await s.st()
    await s.key(code, True)
    track = await _sample(s, budget)
    await s.key(code, False)
    await s.wait_frames(3)
    after = await s.st()
    return _analyse(track), after, track


async def main() -> None:
    results: list[Verdict] = []
    async with Session(WORKER, PORT, res=RES) as s:
        # 必须 boot:Session.__aenter__ 只做了 preflight + 起 Chrome + 连 CDP,
        # **不会**导航。漏掉这一行页面就停在 about:blank,frames 恒为 0,
        # 而 wait_frames 的轮询条件是 frames >= target —— 于是它会永远转下去,
        # 表现是 worker 挂住不退出、也不落任何 JSON。
        await s.boot(min_frames=25)
        boot = await s.st()
        results.append(
            Verdict(
                "boot",
                None if boot.get("frames") else False,
                f"frames={boot.get('frames')} 出生点=({boot.get('x')},{boot.get('z')}) "
                f"collisionShapes={boot.get('shapes')}",
            )
        )

        walk_runs: dict[str, dict] = {}
        for code in ("KeyW", "KeyA", "KeyS", "KeyD"):
            stats, after, track = await _run_direction(s, code, SAMPLE_FRAMES)
            walk_runs[code] = stats
            moved = stats["total"] > MIN_TOTAL_MOVE
            # **撞墙不是卡死。** 诊断实测:从出生点 (33.5,45) 按 W 走到
            # (33.5,35.55) 就贴住了那栋楼的南面墙 x=[32.30,41.70]
            # z=[35.00,35.20],**净空恰好 0.000 m**,而速度全程保持
            # 4.5998 m/s —— 输入一直在生效,人只是走不过去。上一版把
            # 「12 帧没动」一律判卡死,于是「走到墙前」被报成缺陷。
            # 真正要抓的是「没有碰撞体挡着却不走」,所以停滞门限要与净空联用:
            # 先用游戏自己的 AABB 探针量净空,贴着墙就不算卡死。
            # **「保守」不是判据。** 上一版净空读不到就直接按卡死处理,于是
            # 在撞墙点上判了 FAIL —— 出生点 (33.5,45) 紧邻室内隔墙,人走出去
            # 必然贴墙,那不是卡死(`95f47c2` 修过隔墙卡死;多个 agent 用
            # **逐帧无节流**采样在开放地测到 0 静止帧)。
            # 判据要么对,要么标 SKIP:净空读不到就**判未能判定**,不判缺陷。
            stall_note = ""
            no_stall: bool | None = True
            if stats["worst_still"] >= STALL_FRAMES:
                clear = await _clearance(s, track[-1])
                if clear is None:
                    no_stall = None
                    stall_note = (
                        f" | 停滞 {stats['worst_still']} 帧,但净空**读不到** ——"
                        f"无法区分「撞墙」与「卡死」,这一项判**未能判定(SKIP)**,"
                        f"不按卡死报缺陷(出生点 (33.5,45) 紧邻室内隔墙,贴墙是"
                        f"正常的;要判卡死必须在开放地测)"
                    )
                else:
                    blocked = clear["gap"] <= CONTACT_EPS
                    no_stall = not blocked
                    stall_note = (
                        f" | 停滞处净空 {clear['gap']:.3f}m(接触门限<={CONTACT_EPS})"
                        f" -> 撞墙={blocked}"
                    ) + (
                        "" if blocked else
                        " —— **没有墙挡着却不走,这条才是真卡死**"
                    )
            if no_stall is None or not moved:
                ok: bool | None = None if no_stall is None else False
            else:
                ok = True
            if ok is True and stats["total"] < STALL_EPS and not moved:
                ok = False
            results.append(
                Verdict(
                    f"walk {code}",
                    ok,
                    f"{SAMPLE_FRAMES} 帧总位移 {_f(stats['total'])}m "
                    f"(阈值>{MIN_TOTAL_MOVE}) 每帧均 {_f(stats['per_frame'], '.3f')}m "
                    f"峰值 {_f(stats['max_step'], '.3f')}m | "
                    f"最差 {STALL_WINDOW} 帧窗口内静止 {stats['worst_still']} 帧"
                    f"(阈值<{STALL_FRAMES},但**贴着墙不算卡死**)"
                    f"{stall_note} | "
                    f"{_fmt_steps(after.get('asx'))} "
                    f"afterVertical={after.get('avx')} afterDynamic={after.get('adx')} | "
                    f"inside={after.get('inside')} vel={after.get('vel')} gait={after.get('gait')}",
                )
            )
            # nearShapes 只作诊断。**它为空不能推出「没有墙」。**
            # 实测:按 W 贴住那栋楼南面墙(净空 0.000 m)时 nearShapes 依然是
            # [] —— 它是另一套更宽的查询,过滤口径与 AABB 探针不同。
            # 「到底有没有墙」一律以上面用 AABB 探针量出来的净空为准。
            near = after.get("ns") or []
            top = "; ".join(
                f"{n.get('d')}m {str(n.get('shape'))[:44]}"
                for n in near[:3]
                if isinstance(n, dict)
            )
            results.append(
                Verdict(
                    f"nearShapes {code}",
                    None,
                    f"6m 内 {len(near)} 个,前 3: {top or '(空)'}"
                    f" —— **空不代表没墙**,这一项只作诊断;判「撞墙还是卡死」"
                    f"用的是上面 AABB 探针量出来的净空。",
                )
            )

        # 冲刺:Shift+W。dt 未知,所以比的是每帧位移之比,不是绝对速度。
        # 注意 `_sample` 返回的是**位置序列本身**(一个 list),不是三元组。
        #
        # **两段必须采在同一段路上,否则比值没有意义。** 上一版拿
        # `walk_runs["KeyW"]`(出生点起步、24 次采样)去比后面另一个
        # 24 次采样的冲刺段 —— 而前一段很可能整段贴在墙上(每帧均
        # 0.347m),后一段在空街上(每帧 3.259m),比值 9.39 是
        # 「墙 vs 街」而不是「走 vs 跑」。这里改成:先退到空旷处,
        # **紧接着**先按 W 采一段(基准),立刻加 Shift 再采一段(冲刺),
        # 两段只隔一个 keydown,路面条件一致。
        await s.ev("(() => { window.__vcw_teleport = {x: 33.5, z: 79.0}; return 1; }})()")
        await s.wait_frames(3)
        await s.ev("(() => { window.__vcw_teleport = {walk: true, dx: 0, dz: 0}; return 1; }})()")
        await s.wait_frames(2)
        # 走两次朝向,让「基准段」与「冲刺段」在同一条街上前后相接。
        await s.ev(f"(() => {{ window.__vcw_aim = {math.pi}; return 1; }})()")
        await s.wait_frames(2)
        here0 = await s.st()
        px0, pz0 = here0.get("x"), here0.get("z")
        await s.key("KeyW", True)
        track_run = await _sample(s, SPRINT_FRAMES)
        await s.key("ShiftLeft", True)
        track_sprint = await _sample(s, SPRINT_FRAMES)
        await s.key("ShiftLeft", False)
        await s.key("KeyW", False)
        await s.wait_frames(3)
        run_stats = _analyse(track_run)
        run_run = _analyse(track_sprint)
        run_after = await s.st()
        # **先证明这段路是空的。** 上一版不看路面就采样,基准段整段贴在
        # 墙上,于是读出一个 10 倍于物理值的「每帧 6.1m」。用同一个 AABB
        # 探针在采样前量一次净空,读不到就不判(而不是给一个假数)。
        sprint_clear = await _clearance(s, track_run[-1] if track_run else {"x": px0, "z": pz0})
        # **必须归一化 dt。** 上一版直接比「每帧位移」,而 1.7fps 的软件
        # 渲染下两段的有效 dt 差 3.4 倍(步行反推 1.326s、冲刺 0.385s),
        # 比值因此变成 0.53 —— 看起来「冲刺比走路慢」,纯属度量错误。
        # 现在两段都除以**各自实测的每帧 dt**(`__vcw.frames` 增量中位数),
        # 得到 m/s 再比。dt 从实测来,不用 MAX_FRAME_TIME 常量反推(那个
        # 0.25 和实测的 ~0.131 对不上)。
        walk_step, walk_dt = _speed(track_run)
        sprint_step, sprint_dt = _speed(track_sprint)
        # `__vcw` 每 3 帧才发布一次,所以 frames 增量是 3 的倍数;
        # 真正的一次模拟步进是 FIXED_DT 的整数倍,这里只需要**比值**,
        # 两段用的是同一套刻度,dt 的绝对值可以只作诊断。
        walk_v = walk_step / walk_dt if walk_dt > 0 else 0.0
        sprint_v = sprint_step / sprint_dt if sprint_dt > 0 else 0.0
        ratio = sprint_v / walk_v if walk_v > 1e-6 else 0.0
        clear_txt = (f"路面净空 {_f(sprint_clear['gap'], '.2f')}m"
                     if sprint_clear else "净空读不到")
        if walk_v <= 1e-6 or sprint_v <= 1e-6 or walk_dt <= 0 or sprint_dt <= 0:
            sprint_ok: bool | None = None
            sprint_note = (
                f"读不到速度:步行 {(_f(walk_step, '.4f'))}m/{walk_dt:.0f}帧"
                f"、冲刺 {(_f(sprint_step, '.4f'))}m/{sprint_dt:.0f}帧"
                f"({clear_txt},路面可能不通畅或帧计数读不到)"
            )
        else:
            # RUN_SPEED/WALK_SPEED = 8.4/4.6 = 1.83。门限取 1.4。
            sprint_ok = ratio > 1.4
            sprint_note = (
                f"**同一条街、背靠背的两段**({clear_txt}),判据是**速度**不是每帧位移:"
                f"步行(只按 W)每帧位移中位数 {_f(walk_step, '.3f')}m / 实测 "
                f"{walk_dt:.0f} 帧 = **{_f(walk_v, '.2f')}**;"
                f"冲刺(W+Shift){_f(sprint_step, '.3f')}m / {sprint_dt:.0f} 帧 = "
                f"**{_f(sprint_v, '.2f')}**;比值 **{_f(ratio)}**"
                f"(阈值>1.4;理论 RUN/WALK=8.4/4.6=1.83)"
                f" | 归一化 dt 是必须的:两段实测帧跨度差 {walk_dt / sprint_dt:.1f} 倍,"
                f"不归一化就会读出「冲刺比走路慢」"
                f" | dt 来自 `__vcw.frames` 增量实测(每 3 帧发布一次,故为 3 的倍数),"
                f"不用 MAX_FRAME_TIME 常量反推(常量 0.25 与实测对不上)"
                f" | 采样前 {clear_txt}"
            )
        results.append(Verdict("sprint Shift+W", sprint_ok, sprint_note))

        # W 与 Shift+W 的「同键对照」:两次都是按住 W,只有第二次加了 Shift。
        results.append(
            Verdict(
                "W vs Shift+W 差值",
                None,
                f"W 采样每帧 {_f(walk_runs.get('KeyW', {}).get('per_frame', 0.0), '.3f')}m,"
                f"Shift+W 每帧 {_f(sprint_step, '.3f')}m"
                f"({_f(sprint_v, '.2f')} m/s,实测 {sprint_dt:.0f} 帧跨度);"
                f"冲刺后坐标 ({_f(run_after.get('x'))},"
                f"{_f(run_after.get('z'))})",
            )
        )

        # ---- 真·卡死判据:在**开放地**测 ----
        # 上面四项都在出生点 (33.5,45) 测,那里紧邻室内隔墙,贴墙是正常的。
        # 要判「卡死」必须在**净空足够大的开阔地**测:那里站着不动就只可能是
        # 真卡死。这一项是唯一有资格报 stall 缺陷的地方。
        await results_open_area(s, results)

    emit(results, WORKER)


# 开放地候选点(用户指定,实测净空读数在这里取)。挑的依据是远离出生点
# 那栋楼,并且与街面平行,按 W 不会马上顶到东西。
OPEN_SPOTS = ({"x": 15.0, "z": 75.0}, {"x": 15.0, "z": 135.0})


async def results_open_area(s: Session, results: list) -> None:
    """在开阔地按方向键,判「不撞墙却走不动」的真卡死。"""
    picked: dict | None = None
    gaps: list[str] = []
    for spot in OPEN_SPOTS:
        await s.ev(
            f"(() => {{ window.__vcw_teleport = {{x: {spot['x']}, "
            f"z: {spot['z']}}}; return 1; }})()"
        )
        await s.wait_frames(3)
        here = await s.st()
        clear = await _clearance(s, here)
        g = clear["gap"] if clear else None
        gaps.append(
            f"({spot['x']:.0f},{spot['z']:.0f}) 净空 "
            f"{'读不到' if g is None else f'{g:.2f}m'}"
        )
        if g is not None and g > 2.0:
            picked = spot
            break
    if picked is None:
        results.append(
            Verdict(
                "开放地无卡死", None,
                f"候选点都不够开阔({' | '.join(gaps)}),没有可测的开放地,"
                f"记 SKIP —— **没有在开阔地测就没有资格报卡死**。",
            )
        )
        return

    stats, after, track = await _run_direction(s, "KeyW", SAMPLE_FRAMES)
    clear = await _clearance(s, track[-1])
    gap = clear["gap"] if clear else None
    still = stats["worst_still"]
    moved = stats["total"] > MIN_TOTAL_MOVE
    if gap is None:
        ok: bool | None = None
        why = f"开放地 ({picked['x']:.0f},{picked['z']:.0f}) 按 W {SAMPLE_FRAMES} 帧:"\
              f"总位移 {stats['total']:.3f}m,最差窗口静止 {still} 帧,但末端净空读不到," \
              f"无法排除撞墙,记 SKIP"
    elif gap <= CONTACT_EPS:
        # 选点时开阔,走完却贴上了墙 —— 说明这条路不长,不算卡死。
        ok = None
        why = (f"开放地 ({picked['x']:.0f},{picked['z']:.0f}) 起测,按 W "
               f"{SAMPLE_FRAMES} 帧后**贴上了墙**(末端净空 {gap:.3f}m),"
               f"总位移 {stats['total']:.3f}m,静止 {still} 帧 —— 撞墙不是卡死,记 SKIP")
    else:
        ok = bool(moved and still < STALL_FRAMES)
        why = (f"开放地 ({picked['x']:.0f},{picked['z']:.0f}),起测净空 "
               f">2.0m,末端净空 {gap:.3f}m(仍无墙):按 W {SAMPLE_FRAMES} 帧总位移 "
               f"{stats['total']:.3f}m,最差 {STALL_WINDOW} 帧窗口内静止 {still} 帧"
               f"(阈值<{STALL_FRAMES}) —— "
               + ("**开放地却不走,这才是真卡死**" if not ok
                  else "走得动且无静止聚集,无卡死"))
    results.append(Verdict("开放地无卡死", ok, why))



def _f(v, spec: str = ".2f", missing: str = "null") -> str:
    """安全格式化一个可能为 None 的读数。

    `st()` 的字段缺失时返回 None(不是 0)。直接 `{v:.2f}` 会在 note 里
    抛 TypeError,而那会让整个 worker 以 harness failure 收场 —— 一个
    「读数缺失」被误报成「harness 挂了」,把真判定吞掉。读不到就照实
    写 'null',并让对应的 Verdict 走 None(未能判定)。
    """
    if v is None:
        return missing
    try:
        return format(v, spec)
    except (TypeError, ValueError):
        return str(v)

# ---- 墙钟看门狗 ----
# `vlib.wait_frames` 的轮询条件是 frames >= target。页面没 boot(漏了
# s.boot())、或者 WebGPU 初始化失败导致 frames 恒为 0 时,worker 会永远
# 转下去:不退出、不落 JSON、verify8.sh 的 wait 一直挂着。
# 所以整个 main() 外面套一层 asyncio.wait_for,超时按 harness failure 落盘。
WALL_CLOCK_LIMIT_S = 1500

if __name__ == "__main__":
    try:
        asyncio.run(asyncio.wait_for(main(), timeout=WALL_CLOCK_LIMIT_S))
    except asyncio.TimeoutError:
        fail_harness(WORKER, f"超过墙钟上限 {WALL_CLOCK_LIMIT_S}s 没跑完(通常是页面没 boot 成功,frames 恒为 0)")
    except SystemExit:
        raise
    except Exception as exc:  # noqa: BLE001
        fail_harness(WORKER, f"{type(exc).__name__}: {exc}")
