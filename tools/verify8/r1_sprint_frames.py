"""冲刺(Shift)诊断：**按渲染帧**驱动，不用墙钟。

背景（诊断铁律）：软件渲染 rAF ≈ 1.7 fps，而 `__VCW_DEBUG__` 每 3 帧
才发布一次(8161 行 `frame_count % 3 == 0`)。所以：
  * 采样必须按 `frames` 计数器推进，不能按 sleep 秒数 —— 上一版按
    0.05 s 轮询，10 个样本只跨了 1 个渲染帧，测的是发布节流不是运动。
  * 位移率要除以 `frames` 跨度，换算成「每渲染帧」。

要验的三个命题：
  P1  soft_limit_speed 把 RUN/WALK 压成同一值
  P2  `{speed: 8}` 时间加速能改变位移
  P3  dt 钳位到底是多少

`walk` 通道是一次性的，但没发请求时 `walk_request` 保持原值
(4545 行提前 return)，所以只需在**每段开始**发一次；停走要发
`{walk:true, dx:0, dz:0}` —— 判据是「字段存不存在」(4571 行)，
所以 `walk:false` 也算 true，仍然是停走请求。
"""

from __future__ import annotations

import asyncio
import json
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from vlib import SCRATCH, Session  # noqa: E402

PORT = 9401
WORKER = "r1sprint"
WALK, RUN = 4.6, 8.4


async def stop(s):
    await s.ev("window.__vcw_teleport = {walk:true, dx:0, dz:0}")
    await s.wait_frames(6)


async def run_phase(s, *, shift: bool, speed_field=None, settle=12, take=10):
    """开一段直行，稳定后按帧采样。返回 (汇总 dict, v 采样序列)。"""
    await stop(s)
    if speed_field is not None:
        await s.ev(f"window.__vcw_teleport = {{speed:{speed_field}}}")
        await s.wait_frames(2)
    if shift:
        await s.key("ShiftLeft", True)
        await s.wait_frames(2)
    await s.ev("window.__vcw_teleport = {walk:true, dx:1, dz:0}")

    # 速度爬升段：SPEED_RAMP=14，dt 上限 0.0667 → 每帧 ramp≈0.607，
    # 约 8 帧后 |v| 收敛到 wanted 的 99.9%。多给几帧确保到位。
    await s.wait_frames(settle)

    vs, pts = [], []
    f0 = await s.frames()
    for _ in range(take):
        v = await s.st()  # 单次原子快照
        if v:
            vel = v.get("vel") or [0.0, 0.0]
            vs.append(math.hypot(vel[0], vel[1]))
            pts.append((v.get("frames") or 0, v.get("x") or 0.0, v.get("z") or 0.0))
        await s.wait_frames(3)  # 跨一个发布节拍，正好一个样本点
    f1 = await s.frames()

    if shift:
        await s.key("ShiftLeft", False)

    dx = 0.0
    if len(pts) >= 2:
        span = max(1, pts[-1][0] - pts[0][0])
        dx = math.hypot(pts[-1][1] - pts[0][1], pts[-1][2] - pts[0][2]) / span
    tail = vs[-4:] if len(vs) >= 4 else vs
    return {
        "terminal_v": round(sum(tail) / len(tail), 4) if tail else None,
        "v_min": round(min(vs), 4) if vs else None,
        "v_max": round(max(vs), 4) if vs else None,
        "dx_per_frame": round(dx, 5),
        "frame_span": f1 - f0,
        "n_samples": len(vs),
    }, vs


async def main() -> None:
    out: dict = {}
    async with Session(WORKER, PORT) as s:
        await s.boot(min_frames=30)

        # 发布节流指纹：连读 frames，步进应是 3 的倍数
        fs = []
        for _ in range(5):
            fs.append(await s.ev("(() => (window.__vcw||{}).frames || 0)()"))
            await asyncio.sleep(0.3)
        out["frames_steps"] = [b - a for a, b in zip(fs, fs[1:])]

        walk, wvs = await run_phase(s, shift=False, speed_field=1)
        run, rvs = await run_phase(s, shift=True, speed_field=1)
        spd8, svs = await run_phase(s, shift=False, speed_field=8)

        out["walk_1x"] = walk
        out["run_shift"] = run
        out["walk_speed8"] = spd8
        out["walk_v_trace"] = [round(x, 3) for x in wvs]
        out["run_v_trace"] = [round(x, 3) for x in rvs]
        out["speed8_v_trace"] = [round(x, 3) for x in svs]

        out["expected"] = {
            "walk_v": WALK, "run_v": RUN, "run_over_walk": round(RUN / WALK, 4),
        }
        if walk["terminal_v"] and run["terminal_v"]:
            out["v_ratio_run_over_walk"] = round(
                run["terminal_v"] / walk["terminal_v"], 4)
        if walk["dx_per_frame"] and run["dx_per_frame"]:
            out["dx_ratio_run_over_walk"] = round(
                run["dx_per_frame"] / walk["dx_per_frame"], 4)
        if walk["dx_per_frame"] and spd8["dx_per_frame"]:
            out["dx_ratio_speed8_over_walk1x"] = round(
                spd8["dx_per_frame"] / walk["dx_per_frame"], 4)

        # P3: dt 钳位。MAX_FRAME_TIME=0.25 钳的是喂给 simulate 的 delta；
        # simulate 内部再用 `delta.min(FIXED_DT*4)` 收到 0.0667。
        out["dt_source"] = {
            "MAX_FRAME_TIME": 0.25,
            "simulate_clamp": round(1.0 / 60.0 * 4, 5),
        }

    os.makedirs(f"{SCRATCH}/results", exist_ok=True)
    path = f"{SCRATCH}/results/{WORKER}.json"
    with open(path, "w") as fh:
        json.dump(out, fh, ensure_ascii=False, indent=2)
    print(json.dumps(out, ensure_ascii=False, indent=2))
    print(f"\n# saved {path}")


if __name__ == "__main__":
    asyncio.run(main())
