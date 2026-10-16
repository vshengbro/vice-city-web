#!/usr/bin/env python3.11
"""w7 —— 相机跟手不抖、不穿墙(对应缺陷 5)。

(a) 按 W 走 60 帧,逐帧采样 cameraDist / cameraDistTarget / camOccluded。
    判「跟手」不是看距离变不变得快,而是看**相邻帧差**:跟随距离由遮挡
    回避每帧改写,一个稳定的跟随距离每帧只差一点点(<0.5m)。反复跳变到
    2m/帧、又在 8m 和 2m 之间来回,才是「抖」。
    判「收敛」看末段与 cameraDistTarget 的差 —— 稳定态下 cameraDist 应
    贴近 target(回避时才会低于它)。
(b) 相机不穿墙:`camOccluded` 为 true 时 `camClearance` 必须 > 0。
    两者都在 __VCW_DEBUG__ 里。注意 camClearance 为 true 时也可能为 0
    (眼点正好贴在面上),所以只在 >0 才是「有净空」,0 记 FAIL。
(c) 滚轮缩放:发 WheelEvent,前后对比 cameraDistTarget,并检查被 clamp 在
    [FOLLOW_DISTANCE_MIN=2.2, FOLLOW_DISTANCE_MAX=22.0] 内。第三人称下
    滚轮写的是 desired_distance(不是 distance),所以读 target 才看得到。
(d) 切相机:**V** 双向可达 —— 按 V 读 mode 变化,再按 V 读回切回。
    `KEY_CAMERA => toggle_camera_mode` 在 `src/game.rs:4324`,往
    `game.third_person` 取反,`cameraMode` 报的是 `HUD_THIRD_PERSON` /
    `HUD_FIRST_PERSON` 两个字符串(`src/game.rs:4954`)。
    **Tab 不再参与这一项**:Tab 现在是**地图**(`KEY_MAP => map_open` 取反,
    `src/game.rs:4334`),它从来就不是切视角的键。地图由 w4 验收。

**感知验收。** 走之前 / 走之后两张截图交给 vision_analyze,用文字描述
「相机有没有怼在脸前 / 穿进墙里 / 角色有没有被挡住」。
"""

from __future__ import annotations

import asyncio
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import vlib
from vlib import Session, Verdict, box_stats, emit, fail_harness, save_shot

WORKER = "w7_camera"
RES = 0.4

WALK_FRAMES = 60
# 跟随距离的相邻帧差门限。回避把距离压到目标之前时,单帧变化可能超过
# 这个值(真的撞墙了),所以这项是「统计有多少帧超限」,不是「一帧超限
# 就 FAIL」。
JUMP_EPS = 0.5
# 允许超限的帧数占比(真撞到墙角时合理)。
JUMP_RATE_MAX = 0.20
# 末段与目标的偏差(稳定态应当贴近 target)
SETTLE_EPS = 1.2
# 滚轮后 target 至少要有变化才算「滚轮生效」
WHEEL_MIN_DELTA = 0.2
FOLLOW_MIN, FOLLOW_MAX = 2.2, 22.0

# `cameraMode` 报的是哪两个字符串,从 const.rs 读,不写死。
# src/const.rs:562 / 574:`HUD_THIRD_PERSON` / `HUD_FIRST_PERSON`。
_CONST_RS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "src", "const.rs")
try:
    with open(_CONST_RS, encoding="utf-8") as _fh:
        _c = _fh.read()
    THIRD_PERSON = re.search(r'HUD_THIRD_PERSON:\s*&str\s*=\s*"([^"]+)"', _c).group(1)
    FIRST_PERSON = re.search(r'HUD_FIRST_PERSON:\s*&str\s*=\s*"([^"]+)"', _c).group(1)
except Exception:  # noqa: BLE001
    THIRD_PERSON, FIRST_PERSON = "third-person", "first-person"


async def _wheel(s: Session, delta_y: float, times: int = 1) -> None:
    """在 canvas 上发 WheelEvent。vlib 没有滚轮封装,直接走 CDP。"""
    for _ in range(times):
        await s.cmd(
            "Input.dispatchMouseEvent",
            type="mouseWheel", x=640, y=400,
            deltaX=0, deltaY=delta_y,
            button="none", buttons=0,
        )
        await asyncio.sleep(0.1)


async def main() -> None:
    results: list[Verdict] = []
    async with Session(WORKER, 9346, res=RES) as s:
        await s.boot(min_frames=25)
        st = await s.st()
        results.append(
            Verdict(
                "boot",
                None if st.get("frames") else False,
                f"frames={st.get('frames')} cameraMode={st.get('mode')} "
                f"cameraDist={st.get('camDist')} cameraDistTarget={st.get('cdistT')} "
                f"camOccluded={st.get('coccl')} charBox={st.get('cb')}",
            )
        )

        png0 = await s.shot()
        path0 = save_shot(WORKER, "before_walk", png0)
        w0, h0, luma0 = vlib.decode_png(png0)

        # ---- (a) 走 60 帧,采样相机跟随 ----
        track: list[dict] = []
        await s.key("KeyW", True)
        for _ in range(WALK_FRAMES):
            await s.wait_frames(1)
            track.append(await s.st())
        await s.key("KeyW", False)
        await s.wait_frames(3)
        after = await s.st()

        dists = [t.get("camDist") for t in track if isinstance(t.get("camDist"), (int, float))]
        targets = [t.get("cdistT") for t in track if isinstance(t.get("cdistT"), (int, float))]
        occl = [t.get("coccl") for t in track]
        if len(dists) < 10:
            results.append(Verdict("a 跟随采样", None, f"cameraDist 读数不足:{len(dists)}/{WALK_FRAMES}"))
        else:
            deltas = [abs(dists[i] - dists[i - 1]) for i in range(1, len(dists))]
            jumps = [d for d in deltas if d > JUMP_EPS]
            rate = len(jumps) / len(deltas)
            tail_n = min(12, len(dists))
            tail_gap = abs(dists[-1] - targets[-1]) if targets else None
            smooth = rate <= JUMP_RATE_MAX
            settled = tail_gap is not None and tail_gap <= SETTLE_EPS
            results.append(
                Verdict(
                    "a 跟随距离不抖",
                    True if smooth else False,
                    f"{len(dists)} 帧 | 相邻帧差:中位 {sorted(deltas)[len(deltas) // 2]:.3f}m "
                    f"最大 {max(deltas):.3f}m | 超 {JUMP_EPS}m 的有 {len(jumps)} 帧"
                    f"({100 * rate:.0f}%,门限<={100 * JUMP_RATE_MAX:.0f}%) "
                    f"| 序列(每 6 帧取 1):{[round(dists[i], 2) for i in range(0, len(dists), 6)]}",
                )
            )
            # **被遮挡时 cameraDist 本来就该低于 target。** 跟随回避会把
            # 距离压到 FOLLOW_DISTANCE_MIN(2.2)好让相机不被墙挡住,这时
            # `dist != target` 是**正确行为**,拿它判「没收敛」就是假失败
            # —— 上一轮正是这样报的(末段 2.20 vs target 8.2,而那 21/60 帧
            # camOcculated 为 true)。
            # 所以只在**没被遮挡**的帧上判收敛。
            clear_idx = [i for i, o in enumerate(occl) if not o]
            if clear_idx:
                tail = clear_idx[-min(4, len(clear_idx)):]
                clear_gap = max(abs(dists[i] - targets[i]) for i in tail)
            else:
                clear_gap = None
            if tail_gap is None or clear_gap is None:
                settled = None
            else:
                settled = clear_gap <= SETTLE_EPS
            results.append(
                Verdict(
                    "a 跟随收敛到目标",
                    settled,
                    f"末段 cameraDist={dists[-1]:.2f} target={targets[-1] if targets else '?'}"
                    f"(含遮挡帧,差 {tail_gap if tail_gap is None else round(tail_gap, 2)}m)"
                    f" | **只看未被遮挡的帧**:最近 {len(tail) if clear_idx else 0} 个"
                    f" camOccluded=false 的帧上,|dist-target| 最大 "
                    f"{_f(clear_gap, '.2f')}m(门限<={SETTLE_EPS})"
                    f" | 被遮挡 {sum(1 for o in occl if o)}/{len(occl)} 帧,"
                    f"那些帧上 distance 被压到 "
                    f"{FOLLOW_MIN}(FOLLOW_DISTANCE_MIN)是为了不让相机穿墙,"
                    f"**不是没收敛**;要验「没撞墙时跟得稳」看 (b) 的 camClearance",
                )
            )

        png1 = await s.shot()
        path1 = save_shot(WORKER, "after_walk", png1)
        w1, h1, luma1 = vlib.decode_png(png1)
        results.append(
            Verdict(
                "走前/走后对比(待 vision)",
                None,
                f"before={path0} after={path1} | 走过之后玩家位置 "
                f"({_f(after.get('x'), '.1f')},{_f(after.get('z'), '.1f')}),"
                f"角色框 {after.get('cb')}",
            )
        )

        # ---- (b) 相机不穿墙 ----
        # 采样 camOccluded=true 的帧,看 camClearance。
        cur = await s.st()
        clr = cur.get("coccl"), cur.get("lim")
        # __VCW_DEBUG__ 里有 camClearance,st() 没读它 —— 这里直接 ev 补。
        vis = await s.ev(
            "(() => { const d = window.__VCW_DEBUG__ || {};"
            " return {coccl: d.camOccluded, cclear: d.camClearance, dist: d.cameraDist}; })()"
        ) or {}
        # 主动造一个遮挡:把相机怼到建筑边上。走位期间已经采过一遍,
        # 这里再补一次:传送回出生点,原地转 360 度采样,总会有一帧被挡。
        await s.ev("(() => { window.__vcw_aim = 0.0; return 1; }})()")
        occ_samples = []
        for i in range(10):
            await s.ev(f"(() => {{ window.__vcw_aim = {i * 0.628}; return 1; }})()")
            await s.wait_frames(2)
            snap = await s.ev(
                "(() => { const d = window.__VCW_DEBUG__ || {}; const c = window.__vcw || {};"
                " return {coccl: d.camOccluded, cclear: d.camClearance,"
                " dist: d.cameraDist, cyaw: c.cameraYaw}; })()"
            ) or {}
            occ_samples.append(snap)
        occluded = [s2 for s2 in occ_samples if s2.get("coccl")]
        if not occluded:
            results.append(
                Verdict(
                    "b 相机不穿墙", None,
                    f"转了 10 个朝向,camOccluded 全为 false —— 出生点附近 8.2m 跟随距离"
                    f"内没有遮挡物,没有制造出「被遮挡」的场景,未能判定。"
                    f"采样: " + ", ".join(
                        f"yaw{s2.get('cyaw', 0):.1f}→dist{s2.get('dist', 0):.1f}" for s2 in occ_samples
                    ),
                )
            )
        else:
            bad = [s2 for s2 in occluded if not (s2.get("cclear") or 0) > 0]
            worst = min((s2.get("cclear") or 0) for s2 in occluded)
            results.append(
                Verdict(
                    "b 相机不穿墙",
                    True if not bad else False,
                    f"{len(occluded)}/{len(occ_samples)} 个朝向 camOccluded=true,"
                    f"其中 camClearance<=0 的 {len(bad)} 个;最小净空 {worst:.3f}m "
                    f"| 明细:" + ", ".join(
                        f"yaw{s2.get('cyaw', 0):.1f} clear={s2.get('cclear')} dist={s2.get('dist', 0):.1f}"
                        for s2 in occluded[:6]
                    ) + " | 期望:被遮挡时净空>0",
                )
            )
        results.append(
            Verdict(
                "b 净空读数通道",
                None,
                f"camOccluded/camClearance 来自 __VCW_DEBUG__(vlib.st() 没读这两个字段,"
                f"本 worker 自己 ev 补的)。当前: coccl={vis.get('coccl')} "
                f"cclear={vis.get('cclear')} dist={vis.get('dist')}",
            )
        )

        # ---- (c) 滚轮缩放 ----
        w_before = await _rich_target(s)
        await _wheel(s, -240.0, times=3)
        await s.wait_frames(3)
        w_in = await _rich_target(s)
        await _wheel(s, 240.0, times=6)
        await s.wait_frames(3)
        w_out = await _rich_target(s)
        in_range = (FOLLOW_MIN - 0.01) <= w_in["target"] <= (FOLLOW_MAX + 0.01)
        out_range = (FOLLOW_MIN - 0.01) <= w_out["target"] <= (FOLLOW_MAX + 0.01)
        in_delta = w_in["target"] - w_before["target"]
        out_delta = w_out["target"] - w_in["target"]
        results.append(
            Verdict(
                "c 滚轮拉近",
                True if (in_delta < -WHEEL_MIN_DELTA and in_range) else False,
                f"滚轮上推 3 格:cameraDistTarget {w_before['target']:.2f} -> "
                f"{w_in['target']:.2f}(Δ{in_delta:+.2f},门限<-{WHEEL_MIN_DELTA}) "
                f"| cameraDist {w_before['dist']:.2f} -> {w_in['dist']:.2f} "
                f"| clamp 区间 [{FOLLOW_MIN},{FOLLOW_MAX}] 内:{in_range}",
            )
        )
        results.append(
            Verdict(
                "c 滚轮拉远 + clamp",
                True if (out_delta > WHEEL_MIN_DELTA and out_range) else False,
                f"滚轮下拉 6 格:cameraDistTarget {w_in['target']:.2f} -> "
                f"{w_out['target']:.2f}(Δ{out_delta:+.2f}) | 仍在 "
                f"[{FOLLOW_MIN},{FOLLOW_MAX}] 内:{out_range}"
                f" | 注:第三人称下滚轮写的是 desired_distance,所以读 target 才看得到;"
                f"限位常量在 game.rs FOLLOW_DISTANCE_MIN/MAX",
            )
        )

        # ---- (d) 切相机:V 双向 A/B ----
        # 真 A/B:按 V 读 mode 变化,再按 V 读回切回。**两个方向都要走到**,
        # 只测单向的话,「V 只会进第一人称、出不来」也会被判成通过。
        d_before = await s.st()
        mode0 = d_before.get("mode")
        dist0 = d_before.get("camDist")
        await s.tap("KeyV", 130)
        await s.wait_frames(3)
        d_v = await s.st()
        mode1 = d_v.get("mode")
        # 第一次按 V 的读数可能落在事件闭包跑完之前,多等一轮再确认。
        if mode1 == mode0:
            await s.wait_frames(3)
            d_v = await s.st()
            mode1 = d_v.get("mode")
        await s.tap("KeyV", 130)
        await s.wait_frames(3)
        d_v2 = await s.st()
        mode2 = d_v2.get("mode")
        if mode2 == mode1:
            await s.wait_frames(3)
            d_v2 = await s.st()
            mode2 = d_v2.get("mode")

        changed = mode1 != mode0
        restored = mode2 == mode0
        # 进第一人称时 cameraDist 归 0(眼点即焦点,`FIRST_PERSON_DISTANCE`),
        # 但**不能**把 camDist==0 当成「进了 orbit」:本实现只有
        # third-person / first-person 两档,`toggle_camera_mode` 走的是
        # `third_person` 取反,不存在 orbit。
        is_fp = mode1 == FIRST_PERSON and mode0 == THIRD_PERSON
        results.append(
            Verdict(
                "d V 切相机(双向)",
                True if (changed and restored) else False,
                f"按 V 两次:cameraMode {mode0} -> {mode1} -> {mode2}"
                f"(第一次改变了 mode={changed},第二次切回={restored})"
                f" | 第一次切换进入 first-person={is_fp}"
                f" | cameraDist {dist0} -> {d_v.get('camDist')} -> {d_v2.get('camDist')}"
                f" target {d_before.get('cdistT')} -> {d_v.get('cdistT')} -> {d_v2.get('cdistT')}"
                f" | 源码 src/game.rs:4324 KEY_CAMERA => toggle_camera_mode"
                f"(third_person 取反;只有 third/first 两档,没有 orbit)",
            )
        )
        # 切回之后如果相机仍被遮挡,dist 会被回避逻辑压低,此时
        # `|dist - target|` 大是**正确**的。给几帧让它先稳定,再看。
        await s.wait_frames(6)
        d_v3 = await s.st()
        settled_gap = (
            abs((d_v3.get("camDist") or 0) - (d_v3.get("cdistT") or 0))
            if d_v3.get("camDist") is not None else None
        )
        results.append(
            Verdict(
                "d 切回第三人称后跟随恢复",
                None if settled_gap is None else
                (True if (settled_gap <= SETTLE_EPS or d_v3.get("coccl")) else False),
                f"切回并再等 6 帧后:cameraMode={d_v3.get('mode')} "
                f"cameraDist={d_v3.get('camDist')} target={d_v3.get('cdistT')} "
                f"coccl={d_v3.get('coccl')} 差={_f(settled_gap, '.2f')}m"
                f" | 仍被遮挡时 distance 被回避压到 {FOLLOW_MIN} 是**正确**的,"
                f"所以 coccl=true 时这一项不判失败"
                f" | V 分支会把 distance/target 一起复位到 FOLLOW_DISTANCE"
                f"(第一人称侧则是 FIRST_PERSON_DISTANCE=0),所以这一项看的是"
                f"「切回来之后跟随距离与目标是否重新一致」,不是看它等于多少。",
            )
        )
        results.append(
            Verdict(
                "d Tab 不是切视角键",
                None,
                f"Tab 现在是**地图**(`KEY_MAP => map_open` 取反,src/game.rs:4334),"
                f"与 cameraMode 无关。切视角由 V 承担,已在上一条双向验证;"
                f"地图开合(mapOpen + #vcw-map 的 display)由 w4_keys 验收。"
                f"旧版 worker 拿 Tab 判「切第三人称」,与实现矛盾。",
            )
        )

    emit(results, WORKER)


async def _rich_target(s: Session) -> dict:
    """读 cameraDist / cameraDistTarget(vlib.st() 有 cdistT,但 dist 一起补齐)。"""
    v = await s.ev(
        "(() => { const d = window.__VCW_DEBUG__ || {}; const c = window.__vcw || {};"
        " return {dist: d.cameraDist, target: c.cameraDistTarget,"
        " coccl: d.camOccluded}; })()"
    ) or {}
    return {
        "dist": float(v.get("dist") or 0.0),
        "target": float(v.get("target") or 0.0),
        "coccl": v.get("coccl"),
    }



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
