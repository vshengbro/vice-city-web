"""dt 钳位定位：位移为什么比 v * 0.0667 大一倍。

假设：__vcw 的 `vel` 是 player.step 内部按 `achieved/dt` 反算的、并且被
`.clamp(-speed, speed)` 削在上限上，所以它是**饱和值**；而**位置**还被
`step_vertical` 二次改写过 XZ（game.rs:5912 注释明说）。于是
`dx/vel` 这个反推出来的 dt 根本不是 simulate 用的 dt。

验证方法：同一个渲染帧里同时读
  after_static / after_vertical / 玩家最终位置
（这三个探针 __VCW_DEBUG__ 每 3 帧发一次），把每段位移拆开归因。
"""

from __future__ import annotations

import asyncio
import json
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from vlib import SCRATCH, Session  # noqa: E402

PORT = 9332
WORKER = "r1dt"


async def main() -> None:
    out: dict = {}
    async with Session(WORKER, PORT) as s:
        await s.boot(min_frames=30)
        await s.ev("window.__vcw_teleport = {walk:true, dx:0, dz:0}")
        await s.wait_frames(5)
        await s.ev("window.__vcw_teleport = {walk:true, dx:1, dz:0}")
        await s.wait_frames(12)

        rows = []
        f0 = await s.frames()
        for _ in range(9):
            v = await s.st()  # 原子快照
            if not v:
                continue
            vel = v.get("vel") or [0.0, 0.0]
            rows.append({
                "f": v.get("frames"),
                "x": v.get("x"), "z": v.get("z"),
                "vx": round(vel[0], 4),
                "v": round(math.hypot(vel[0], vel[1]), 4),
                "asx": v.get("asx"), "avx": v.get("avx"),
            })
            await s.wait_frames(3)
        f1 = await s.frames()

        out["rows"] = rows
        out["frame_span"] = f1 - f0
        if len(rows) >= 2:
            a, b = rows[0], rows[-1]
            span = max(1, b["f"] - a["f"])
            dx = math.hypot(b["x"] - a["x"], b["z"] - a["z"])
            out["total_dx"] = round(dx, 4)
            out["dx_per_frame"] = round(dx / span, 5)
            out["terminal_v"] = b["v"]
            out["implied_dt_dx_over_v"] = round(dx / span / b["v"], 5)
            # 各段位移归因：静态层 -> 垂直层 -> 动态层
            # afterStatic / afterVertical / afterDynamic 都是长度为 2 的
            # 数组 [x, z]（game.rs:5241 的 format 串）。
            def pt(row, key):
                val = row.get(key)
                return val if isinstance(val, list) and len(val) >= 2 else None

            a_s, a_v = pt(a, "asx"), pt(a, "avx")
            b_s, b_v = pt(b, "asx"), pt(b, "avx")
            if None not in (a_s, a_v, b_s, b_v):
                d_static = math.hypot(b_s[0] - a_s[0], b_s[1] - a_s[1])
                d_vert = math.hypot(b_v[0] - b_s[0], b_v[1] - b_s[1])
                out["d_after_static_per_frame"] = round(d_static / span, 5)
                out["d_vertical_extra_per_frame"] = round(d_vert / span, 5)
                out["residual_per_frame"] = round(
                    out["dx_per_frame"] - d_static / span - d_vert / span, 5)
                out["dt_from_static_layer"] = round(
                    d_static / span / b["v"], 5) if b["v"] else None

    out["clamps"] = {
        "MAX_FRAME_TIME_on_delta": 0.25,
        "simulate_dt_min_FIXED_DT_x4": round(1.0 / 60.0 * 4, 5),
    }
    os.makedirs(f"{SCRATCH}/results", exist_ok=True)
    path = f"{SCRATCH}/results/{WORKER}.json"
    with open(path, "w") as fh:
        json.dump(out, fh, ensure_ascii=False, indent=2)
    print(json.dumps(out, ensure_ascii=False, indent=2))
    print(f"\n# saved {path}")


if __name__ == "__main__":
    asyncio.run(main())
