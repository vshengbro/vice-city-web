#!/usr/bin/env python3.11
"""w3 —— 指针锁 + 鼠标视角 + 方向不反向(对应缺陷 3)。

**这个 worker 的核心是「实测符号」,不是「猜符号再验」。**
验收里最容易自欺的做法是:发现鼠标往右移会让 cameraYaw 减小,于是写下
一个负号,宣布「方向正确」。那什么都没验。本 worker 的做法是把四个量
的**实测符号逐个列出来**,再让人话判据从这些符号里推出来。

四个量:
  1. 鼠标往右拖 -> ΔcameraYaw 的符号
  2. 画面往哪转(右 / 左)—— 几何推不出「人看见什么」,所以用**前后两张
     截图**交给 vision_analyze 用文字描述;同时把可推导的部分(固定世界点
     相对视角的方位角)明确标为「推导」,不与实测混在一起
  3. 按 W -> 角色在世界平面上相对相机前向的夹角。**前向从 game.rs 的源码
     表达式里解析出来**,不写死也不假设(见 `camera_forward()`)。
  4. 脚底 / 头顶的屏幕 y 坐标。按 W 让角色「往屏幕上方远去」,屏幕 y 必须
     下降。**charBox[4] 那个 depth 不再作判据** —— 它是 clamp 到 [0,1] 的
     饱和值,实测恒为 1.00000(见下面 B 的 note)。

人话判据:
  A「鼠标右移 = 视角右转」-> 量 1 与量 2 同号
  B「按 W = 角色往屏幕上方远去」-> 屏幕 y 下降,且世界位移方向与相机前向
     的夹角接近 0

**关于指针锁:指针锁已实现**(game.rs:3829-3841,`request_pointer_lock` +
`pointerlockchange` + `movement_x/y`)。视角同时走 canvas pointerdown 拖拽
(`camera.yaw += dx*0.006`)与指针锁下的 movement 增量。

**关于 (b) Escape 解锁:这一项必须记 SKIP,不能记 FAIL。** 指针锁的
Escape 退出是**浏览器 UI 层**行为(在渲染进程拿到事件之前就被 chrome 的
浏览器外壳吃掉了),CDP 的 `Input.dispatchKeyEvent` 合成事件到不了那里 ——
所以「发了 Escape 还是 locked」**不能**推出「游戏不解锁」。指针锁本身
(locked 是否为真)由 (a) 独立证明。
"""

from __future__ import annotations

import asyncio
import math
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import vlib
from vlib import Session, Verdict, dist, emit, fail_harness, save_shot

WORKER = "w3_pointerlock"
RES = 0.4
GAME_RS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "src", "game.rs")

# 指针锁要求的拖拽像素。0.006 rad/px,240px -> 1.44 rad(约 82 度),足够
# 明显地跨过「符号相反」和「符号正确」的差别。
DRAG_PX = 240
# 转视角后等几帧,让 camera 的 follow 阻尼把画面稳定下来再截图。
SETTLE_FRAMES = 4
# 判定「W 沿相机前向走」的余弦门限。1.0 = 完全沿前向;收官验收实测
# 点积 = 1.0000,所以 0.9 给了足够的余量去吸收「走的过程中撞到什么东西
# 改了位移方向」这一类干扰,又远高于「走反了」(那会接近 -1)与
# 「走成了正交」(0.0)。旧的 0.82 门限配的是两个**错误**候选。
FORWARD_COS_MIN = 0.9

CANVAS = (640, 400)

# 相机前向的源码出处:`src/game.rs:5872` 的
#     let forward: Vec2 = ... else { [camera_yaw.cos(), -camera_yaw.sin()] };
# 同时 `src/camera.rs:988` 的注释与 `src/game.rs:6207` 的 update_aim
# 独立复述了同一个式子。这里从源码**解析**它,解析失败才回落到写死值。
_FORWARD_RE = re.compile(
    r"let\s+forward:\s*Vec2\s*=\s*if\s+world_walk[^{]*\{\s*world_walk\s*\}\s*"
    r"else\s*\{\s*\[(?P<body>[^\]]*)\]\s*\};",
    re.S,
)
_TERM_RE = re.compile(
    r"^\s*(?P<sign>[+-]?)\s*camera_yaw\.(?P<fn>cos|sin)\(\s*\)\s*$"
)


def camera_forward() -> tuple[object, str]:
    """从 game.rs 解析出相机前向,返回 (前向函数, 出处说明)。

    **为什么必须从源码解析。** 上一版 worker 把前向「假设」成
    `(-sin, -cos)` 或 `(sin, cos)`,两个都与实现 `[cos, -sin]` **正交**,
    点积恒为 0,于是 `FORWARD_COS_MIN` 门限在结构上永远不可能通过 ——
    一个无论应用好坏都必 FAIL 的判据。写死是对的、但写错过的教训更贵:
    解析让「实现改了前向约定」变成 harness 自己能发现的事。
    """
    try:
        with open(GAME_RS, encoding="utf-8") as fh:
            text = fh.read()
    except OSError as exc:
        return (lambda yaw: (math.cos(yaw), -math.sin(yaw))), f"读不到 {GAME_RS}({exc}),用写死值"
    m = _FORWARD_RE.search(text)
    if not m:
        return (lambda yaw: (math.cos(yaw), -math.sin(yaw))), f"{GAME_RS} 里没找到 forward 表达式,用写死值"
    terms: list[tuple[int, str]] = []
    for part in m.group("body").split(","):
        t = _TERM_RE.match(part)
        if not t:
            return (
                lambda yaw: (math.cos(yaw), -math.sin(yaw)),
                f"{GAME_RS} 的 forward 表达式含未识别的项 {part.strip()!r},用写死值",
            )
        terms.append((-1 if t.group("sign") == "-" else 1, t.group("fn")))
    if len(terms) != 2:
        return (lambda yaw: (math.cos(yaw), -math.sin(yaw))), f"forward 不是二维,用写死值"
    (s0, f0), (s1, f1) = terms
    line = text[: m.start()].count("\n") + 1
    src = f"{os.path.basename(GAME_RS)}:{line} -> [{f0}({'+' if s0 > 0 else '-'}camera_yaw), {f1}({'+' if s1 > 0 else '-'}camera_yaw)]"
    return (lambda yaw, s0=s0, f0=f0, s1=s1, f1=f1: (
        s0 * (math.cos(yaw) if f0 == "cos" else math.sin(yaw)),
        s1 * (math.cos(yaw) if f1 == "cos" else math.sin(yaw)),
    )), f"从源码解析: {src}"


FORWARD_FN, FORWARD_SRC = camera_forward()


async def _lock_state(s: Session):
    return await s.ev("(() => ({locked: document.pointerLockElement !== null,"
                     " tag: document.pointerLockElement ?"
                     " document.pointerLockElement.tagName : null}))()")


async def _drag(s: Session, x0: int, y0: int, dx: int, dy: int = 0, steps: int = 12) -> None:
    """在 canvas 上走一遍真实的 pointerdown -> N 段 move -> pointerup。

    vlib 只有 `click` / `mouse_move`,没有拖拽,所以这里直接发 CDP 事件:
    pointerdown 用 mousePressed(浏览器会派发 pointerdown),再逐段 mouseMoved。
    """
    await s.cmd("Input.dispatchMouseEvent", type="mousePressed", x=x0, y=y0,
                button="left", buttons=1, clickCount=1)
    await asyncio.sleep(0.08)
    for i in range(1, steps + 1):
        await s.cmd(
            "Input.dispatchMouseEvent",
            type="mouseMoved",
            x=int(x0 + dx * i / steps),
            y=int(y0 + dy * i / steps),
            button="left",
            buttons=1,
        )
        await asyncio.sleep(0.05)
    await s.cmd("Input.dispatchMouseEvent", type="mouseReleased",
                x=int(x0 + dx), y=int(y0 + dy), button="left", buttons=0, clickCount=1)
    await asyncio.sleep(0.08)


async def main() -> None:
    results: list[Verdict] = []
    async with Session(WORKER, 9342, res=RES) as s:
        await s.boot(min_frames=25)
        st0 = await s.st()
        results.append(
            Verdict(
                "boot",
                None if st0.get("frames") else False,
                f"frames={st0.get('frames')} 画布={st0.get('cw')}x{st0.get('ch')} "
                f"初始 cameraYaw={_f(st0.get('cyaw'), '.4f')} playerYaw={_f(st0.get('yaw'), '.4f')} "
                f"charBox={st0.get('cb')}",
            )
        )

        # ---- (a) 点击 canvas 后是否有指针锁 ----
        await s.click(*CANVAS)
        await s.wait_frames(2)
        lock_a = await _lock_state(s)
        results.append(
            Verdict(
                "a 点击后指针锁",
                True if lock_a and lock_a.get("locked") else False,
                f"pointerLockElement={'非 null(tag=' + str(lock_a.get('tag')) + ')' if lock_a and lock_a.get('locked') else 'null'}"
                f" —— 期望非 null。src/ 内无任何 request_pointer_lock 调用,"
                f"视角实际走 canvas pointerdown 拖拽。",
            )
        )

        # ---- (b) Escape 之后是否解锁 ----
        # 先真的发一次 requestPointerLock,这样 (b) 才是在测「有锁之后
        # Esc 能不能解」,而不是在测一个从来没锁上的状态。
        forced = await s.ev(
            "(() => { const c = document.querySelector('canvas');"
            " if (!c) return 'no-canvas';"
            " c.requestPointerLock && c.requestPointerLock();"
            " return 'requested'; })()"
        )
        await s.wait_frames(3)
        lock_forced = await _lock_state(s)
        await s.cmd("Input.dispatchKeyEvent", type="rawKeyDown", code="Escape",
                    key="Escape", windowsVirtualKeyCode=27)
        await s.cmd("Input.dispatchKeyEvent", type="keyUp", code="Escape",
                    key="Escape", windowsVirtualKeyCode=27)
        await s.wait_frames(3)
        lock_b = await _lock_state(s)
        results.append(
            Verdict(
                "b Escape 后解锁",
                None,
                f"手动 requestPointerLock({forced})后 locked="
                f"{lock_forced.get('locked') if lock_forced else '读不到'};"
                f"发 Escape 之后 locked={lock_b.get('locked') if lock_b else '读不到'}"
                " —— **记 SKIP 而不是 FAIL,这是有意为之**:指针锁的 Escape 退出"
                "发生在**浏览器外壳层**,在事件到达页面前就被吃掉了。CDP 的"
                " Input.dispatchKeyEvent 合成事件走不到那一层,所以「发 Escape "
                "后仍 locked」对**任何**网页都成立,不携带关于本游戏的信息 ——"
                "拿它当判据必然假失败。指针锁本身由 (a) 独立证明。",
            )
        )

        # ---- (c) 只发鼠标移动,不发按键,cameraYaw 是否连续变化 ----
        # 真实路径是「按住拖」。所以这一项是:按住不放,分多段 move,
        # 逐段读 yaw,看它是否**连续**变化(而不是只在最后跳一下)。
        before = await s.st()
        samples: list[float] = []
        await s.cmd("Input.dispatchMouseEvent", type="mousePressed", x=CANVAS[0], y=CANVAS[1],
                    button="left", buttons=1, clickCount=1)
        for i in range(1, 9):
            await s.cmd("Input.dispatchMouseEvent", type="mouseMoved",
                        x=CANVAS[0] + int(DRAG_PX * i / 8), y=CANVAS[1],
                        button="left", buttons=1)
            await s.wait_frames(1)
            cur = await s.st()
            if isinstance(cur.get("cyaw"), (int, float)):
                samples.append(cur["cyaw"])
        await s.cmd("Input.dispatchMouseEvent", type="mouseReleased",
                    x=CANVAS[0] + DRAG_PX, y=CANVAS[1], button="left", buttons=0)
        await s.wait_frames(SETTLE_FRAMES)
        after = await s.st()

        deltas = [samples[i] - samples[i - 1] for i in range(1, len(samples))]
        y0v = before.get("cyaw")
        d_total = (samples[-1] - y0v) if (samples and isinstance(y0v, (int, float))) else 0.0
        continuous = len([d for d in deltas if abs(d) > 1e-4]) >= max(2, len(deltas) - 1)
        # 理论值:0.006 rad/px × 240px = 1.44 rad。
        results.append(
            Verdict(
                "c 拖拽时 cameraYaw 连续变化",
                True if (continuous and abs(d_total) > 0.1) else False,
                f"按住拖 {DRAG_PX}px 期间 8 次采样 ΔcameraYaw={[_f(d, '.3f') for d in deltas]} "
                f"总 Δ={_f(d_total, '+.4f')} rad;期望 0.006×{DRAG_PX}={0.006 * DRAG_PX:.3f} rad",
            )
        )

        # ---- (2) 画面往哪转:截图对,交给 vision ----
        png_a = await s.shot()
        path_a = save_shot(WORKER, "yaw_drag_right", png_a)
        # 反向再拖回同样距离,拍第二张:两张的差异方向就是「画面往哪转」。
        await _drag(s, CANVAS[0] + DRAG_PX, CANVAS[1], -DRAG_PX)
        await s.wait_frames(SETTLE_FRAMES)
        png_b = await s.shot()
        path_b = save_shot(WORKER, "yaw_drag_left", png_b)
        y_back = await s.st()
        results.append(
            Verdict(
                "反向拖拽回到原朝向",
                None,
                f"右拖后 cameraYaw={_f(after.get('cyaw'), '.4f')},左拖 {DRAG_PX}px 后="
                f"{_f(y_back.get('cyaw'), '.4f')} "
                f"(起始 {_f(y0v, '.4f')})",
            )
        )
        results.append(
            Verdict(
                "画面方向截图(待 vision 描述)",
                None,
                f"右拖后={path_a} 左拖后={path_b} —— 判据 A「鼠标右移=视角右转」"
                "必须由这两张图的文字描述确认,不能由 ΔcameraYaw 的符号推出来:符号只说明"
                "数学关系,不说明玩家看到画面往哪转。",
            )
        )

        # ---- (3)+(4) 按 W:世界位移方向 vs 相机前向,以及屏幕方向 ----
        w_before = await s.st()
        await s.key("KeyW", True)
        # 走 3 帧足够看出方向,又不会撞上东西把位移归零。
        await s.wait_frames(3)
        w_mid = await s.st()
        await s.key("KeyW", False)
        await s.wait_frames(2)

        cx0, cz0 = w_before.get("x"), w_before.get("z")
        cx1, cz1 = w_mid.get("x"), w_mid.get("z")
        cyaw = w_before.get("cyaw")
        moved = dist(cx0, cz0, cx1, cz1) if None not in (cx0, cz0, cx1, cz1) else 0.0

        # 前向用 `camera_forward()` 从 game.rs 解析出来的那个,不是猜的。
        if moved > 1e-3 and isinstance(cyaw, (int, float)):
            mx, mz = cx1 - cx0, cz1 - cz0
            norm = math.hypot(mx, mz)
            ux, uz = mx / norm, mz / norm
            fx, fz = FORWARD_FN(cyaw)
            cosang = ux * fx + uz * fz
            results.append(
                Verdict(
                    "W 位移沿相机前向",
                    True if cosang >= FORWARD_COS_MIN else False,
                    f"cameraYaw={_f(cyaw, '.4f')} -> 源码前向=({fx:+.4f},{fz:+.4f})"
                    f" | W 位移 ({_f(mx, '+.3f')},{_f(mz, '+.3f')}) 长 {_f(moved, '.3f')}m"
                    f" | 归一化点积={_f(cosang, '+.4f')}"
                    f"(门限>={FORWARD_COS_MIN};0=正交,-1=完全走反)"
                    f" | {FORWARD_SRC}",
                )
            )
        else:
            results.append(
                Verdict("W 位移沿相机前向", None,
                        f"读不到:位移 {moved:.3f}m,cyaw={cyaw}(没走出位移就不判方向)")
            )

        box0, box1 = w_before.get("cb"), w_mid.get("cb")
        d_sx = d_sy = d_depth = None
        if isinstance(box0, list) and isinstance(box1, list) and len(box0) >= 5 and len(box1) >= 5:
            d_depth = box1[4] - box0[4]
            d_sx = box1[0] - box0[0]
            d_sy = box1[1] - box0[1]
            # 判据 B 的第一半:**屏幕上移** = 脚底点 y 变小(y 轴向下为正)。
            #
            # 旧版用的是 charBox[4] 那个 depth,但它**不是可用的距离量**:
            # `camera.world_to_screen`(src/camera.rs:1079)把 NDC z 做了
            # `.clamp(0.0, 1.0)` 再线性映射,于是「正前方几步之内」的角色
            # 全部饱和在 1.00000 —— 实测 Δ=+0.00000。拿一个饱和常量判
            # 「远离了没有」必然恒 FAIL,与应用好坏无关。
            feet_y0, feet_y1 = box0[1], box1[1]
            crown_y0, crown_y1 = box0[3], box1[3]
            feet_up = feet_y1 < feet_y0
            height0, height1 = abs(crown_y0 - feet_y0), abs(crown_y1 - feet_y1)
            grew = height1 > height0
            # 屏幕 y 下降 = 角色在画面里往「上」走 = 远离相机 / 往远处去。
            # 高度不要求严格变大:正前方走时投影高度会略降,只要不塌到 0。
            on_screen = (
                True if (feet_up and height1 > 0.5) else
                None if abs(feet_y1 - feet_y0) < 1e-6 else False
            )
            results.append(
                Verdict(
                    "B W 让角色往屏幕上方远去",
                    on_screen,
                    f"脚底屏幕 y {_f(feet_y0, '.1f')} -> {_f(feet_y1, '.1f')}"
                    f" Δ={_f(feet_y1 - feet_y0, '+.2f')}px(y 轴向下为正,负=往屏幕上方走)"
                    f" | 投影高 {_f(height0, '.1f')} -> {_f(height1, '.1f')}px(变高={grew})"
                    f" | 画布 {st0.get('cw')}x{st0.get('ch')}"
                    f" | **depth 不再作判据**:charBox[4] 是 world_to_screen 里"
                    f" clamp 到 [0,1] 的饱和 NDC z,本段读数 {_f(box0[4], '.5f')} -> "
                    f"{_f(box1[4], '.5f')}(Δ={_f(d_depth, '+.5f')}),恒等于 1.0,"
                    f"对「远近」零分辨力。",
                )
            )
            # 判据 B 的第二半:屏幕上应当主要往「上」走,横向漂移不应与
            # 纵向同量级(那说明角色在斜着走而不是正前方)。
            lateral = abs(d_sx) if abs(d_sy) > 1e-6 else 0.0
            results.append(
                Verdict(
                    "B W 屏幕位移偏上",
                    None if abs(d_sy) < 1e-6 else (True if abs(d_sy) >= lateral else False),
                    f"Δscreen=({_f(d_sx, '+.1f')},{_f(d_sy, '+.1f')})px:Y 轴向下为正,所以「往屏幕上方」"
                    f"要求 Δy 为负。当前 Δy={_f(d_sy, '+.1f')}{' 是负值,符合' if d_sy < 0 else ' 不是负值'}。",
                )
            )
        else:
            results.append(Verdict("B 屏幕方向读数", None, f"charBox 读不到:{box0} / {box1}"))

        # ---- 符号真值表 ----
        sign_yaw = "增" if d_total > 0 else ("减" if d_total < 0 else "不变")
        rows = [
            f"鼠标右拖 {DRAG_PX}px -> ΔcameraYaw = {d_total:+.4f} rad({sign_yaw})",
            f"W 按下 -> Δ(playerX,playerZ) = "
            f"{(cx1 - cx0) if None not in (cx0, cx1) else float('nan'):+.3f},"
            f"{(cz1 - cz0) if None not in (cz0, cz1) else float('nan'):+.3f} m",
            f"W 按下 -> ΔcharBox[0](屏幕 x) = {d_sx:+.1f}px" if isinstance(box1, list) else "W -> Δscreen x 读不到",
            f"W 按下 -> ΔcharBox[4](depth) = {d_depth:+.5f}" if isinstance(box1, list) else "W -> depth 读不到",
        ]
        results.append(Verdict("方向符号真值表", None, " | ".join(rows)))

        # 判据 A 的数学半。**符号要说对。** `LOOK_YAW_SENSITIVITY = -0.006`
        # 是负的,所以 dx>0(鼠标右移)让 cameraYaw **减小**;而 `camera.rs`
        # 的眼点公式把相机摆在 `-forward` 方向,于是 yaw 减小 = 相机绕角色
        # 往**右**转、画面内容**左**移。实测 Δ=-1.4400 rad(减)与这个一致。
        # 旧版这段硬写着「为『增』」,在自己算出的负数旁边 —— 判据不判,
        # 但会把读数的人带偏。
        yaw_down = d_total < 0
        results.append(
            Verdict(
                "A 鼠标右移=视角右转(数学半)",
                None,
                f"ΔcameraYaw={_f(d_total, '+.4f')} rad,为「{sign_yaw}」"
                f"(LOOK_YAW_SENSITIVITY=-0.006 是负的,所以鼠标右移让 yaw 减小;"
                f"而 camera.rs 的眼点在 -forward 方向,故 yaw 减小 = 相机绕角色右转"
                f"、画面内容左移 —— 与实测符号一致={yaw_down})。"
                f"数学半到此为止;感知半(画面是否真的往右转)必须由上面两张截图的"
                f"vision 描述确认,几何和符号都推不出「人眼看到什么」。",
            )
        )

    emit(results, WORKER)



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
