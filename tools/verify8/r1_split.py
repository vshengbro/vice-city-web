"""dt 2x 归因：把 0.6133 m/帧 拆到具体某一步。

已确认（r1_raf）：WALK 时 |v| 饱和在 4.5998，dx/frame = 0.61331，
所以隐含 dt = 0.13333 = FIXED_DT * 8，而源码写的是
`dt = delta.min(FIXED_DT * 4)` = 0.06667。差整一倍。

候选：位置在**一次 simulate 里被推进了两次**。
`Player::step` 推 v*dt 一次（player.rs:536），
`step_vertical` 再按 `travel` 做一次室内滑动（game.rs:6072-6083），
但它是**滑动**（move_and_slide），不该凭空多出位移。

真正的判据：直接读 `previous_step_xz` 前后两帧的差 —— 它是
`Player::step` 动手**之前**的位置（game.rs:5901），所以
`after_static - previous_step_xz` 就是静态层这一帧的净位移，
`player_final - after_vertical` 就是垂直层又加了多少。
两者相加应等于 dx/frame；多出来的那份就是重复推进。
"""

from __future__ import annotations

import asyncio
import json
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from vlib import SCRATCH, Session  # noqa: E402

PORT = 9402
WORKER = "r1split"

READ = """(() => {
  const c = window.__vcw || {}, d = window.__VCW_DEBUG__ || {};
  return {frames: c.frames, x: c.playerX, z: c.playerZ, vel: c.vel,
          asx: d.afterStatic, avx: d.afterVertical, adx: d.afterDynamic,
          wr: c.walkReq, grounded: c.grounded, py: c.playerY};
})()"""


def d2(a, b):
    return math.hypot(a[0] - b[0], a[1] - b[1]) if a and b else None


async def main() -> None:
    out: dict = {}
    async with Session(WORKER, PORT) as s:
        await s.boot(min_frames=30)
        await s.ev("window.__vcw_teleport = {walk:true, dx:0, dz:0}")
        await s.wait_frames(6)
        await s.ev("window.__vcw_teleport = {walk:true, dx:1, dz:0}")
        await s.wait_frames(16)

        rows = []
        for _ in range(12):
            v = await s.ev(READ)
            if v:
                rows.append(v)
            await s.wait_frames(3)

        out["rows"] = [
            {"f": r["frames"], "x": round(r["x"], 4), "z": round(r["z"], 4),
             "v": round(math.hypot(*(r.get("vel") or [0, 0])), 4),
             "asx": r.get("asx"), "avx": r.get("avx"), "adx": r.get("adx")}
            for r in rows
        ]

        if len(rows) >= 2:
            a, b = rows[0], rows[-1]
            span = max(1, b["frames"] - a["frames"])
            vmag = math.hypot(*(b.get("vel") or [0, 0]))
            out["frame_span"] = span
            out["terminal_v"] = round(vmag, 4)

            total = d2([a["x"], a["z"]], [b["x"], b["z"]])
            out["total_dx"] = round(total, 4)
            out["dx_per_frame"] = round(total / span, 5)
            out["dt_implied"] = round(total / span / vmag, 5) if vmag else None

            # 静态层：after_static 两帧之差（这一层就是 Player::step 的产出）
            d_static = d2(a.get("asx"), b.get("asx"))
            out["d_static_per_frame"] = round(d_static / span, 5) if d_static else None

            # 垂直层：在**同一帧内** after_vertical - after_static
            d_vert = d2(b.get("asx"), b.get("avx"))
            out["d_vertical_same_frame"] = round(d_vert, 5) if d_vert else None

            # 动态层：after_dynamic
            d_dyn = d2(a.get("adx"), b.get("adx"))
            out["d_dynamic_per_frame"] = round(d_dyn / span, 5) if d_dyn else None

            # 关键账：静态层位移 vs 速度*dt
            if d_static and vmag:
                out["dt_from_static"] = round(d_static / span / vmag, 5)
                out["expected_dx_at_FIXED_DT_x4"] = round(vmag * (1 / 60 * 4), 5)
                out["expected_dx_at_FIXED_DT_x8"] = round(vmag * (1 / 60 * 8), 5)
            # 账要对平：静态 + 垂直 + 动态 vs 总位移
            parts = [x for x in (out.get("d_static_per_frame"),
                                 out.get("d_dynamic_per_frame")) if x]
            out["sum_parts"] = round(sum(parts), 5) if parts else None
            out["unexplained"] = round(
                out["dx_per_frame"] - (out["sum_parts"] or 0.0), 5)

    os.makedirs(f"{SCRATCH}/results", exist_ok=True)
    path = f"{SCRATCH}/results/{WORKER}.json"
    with open(path, "w") as fh:
        json.dump(out, fh, ensure_ascii=False, indent=2)
    print(json.dumps(out, ensure_ascii=False, indent=2))
    print(f"\n# saved {path}")


if __name__ == "__main__":
    asyncio.run(main())
