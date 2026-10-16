"""dt 钳位终局定位：真实 rAF 节奏 vs frame_count vs 位移。

矛盾（r1_dt 实测）：`dx/frame / |v|` = 0.13333 s，而源码里
`dt = delta.min(FIXED_DT*4)` = 0.06667 s —— 正好差 2 倍。
`MAX_FRAME_TIME` = 0.25 钳的是 rAF 的 `delta`，也不是 0.1333。

三种可能，这个探针一次分辨：
  A. 真实 rAF 间隔 ≈ 0.588 s(1.7 fps) → dt 被 min() 收到 0.0667，
     位移应为 v*0.0667 = 0.3067 m/帧。实测 0.6133 = 2 倍。
  B. `simulate` 每帧跑了两次（两个 rAF 链共用一个 Game）。
     那么 frame_count 每 tick +2，但 dx/frame 仍是 v*dt —— 不符。
  C. rAF 间隔 ≈ 0.133 s，dt 根本没被 min 收到（间隔 < 0.0667？不可能）。

所以关键量是：**真实 rAF tick 数** 与 **frame_count 增量** 的比值。
探针在页面里自己装一条 rAF 计数器（不改游戏代码），
再和 window.__vcw.frames 对比。
"""

from __future__ import annotations

import asyncio
import json
import math
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from vlib import SCRATCH, Session  # noqa: E402

PORT = 9333
WORKER = "r1raf"

PROBE = """(() => {
  if (!window.__probe) {
    window.__probe = {ticks: 0, stamps: [], marks: []};
    let last = null;
    const tick = (ts) => {
      const p = window.__probe;
      p.ticks += 1;
      if (last !== null) p.stamps.push(ts - last);
      last = ts;
      if (p.stamps.length > 400) p.stamps.shift();
      p.marks.push([p.ticks, (window.__vcw||{}).frames || 0,
                    (window.__vcw||{}).playerX, (window.__vcw||{}).playerZ]);
      if (p.marks.length > 400) p.marks.shift();
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }
  return true;
})()"""

READ = """(() => {
  const p = window.__probe;
  const c = window.__vcw || {};
  return {ticks: p.ticks, stamps: p.stamps.slice(-30),
          marks: p.marks.slice(-30),
          frames: c.frames, x: c.playerX, z: c.playerZ,
          vel: c.vel};
})()"""


async def main() -> None:
    out: dict = {}
    async with Session(WORKER, PORT) as s:
        await s.boot(min_frames=30)
        await s.ev(PROBE)

        await s.ev("window.__vcw_teleport = {walk:true, dx:0, dz:0}")
        await s.wait_frames(6)
        await s.ev("window.__vcw_teleport = {walk:true, dx:1, dz:0}")
        await s.wait_frames(14)  # 让速度收敛到 WALK_SPEED

        a = await s.ev(READ)
        t0 = time.time()
        await s.wait_frames(30)
        b = await s.ev(READ)
        wall = time.time() - t0

        out["wall_s"] = round(wall, 3)
        out["ticks_a"] = a["ticks"]
        out["ticks_b"] = b["ticks"]
        out["raf_ticks_in_window"] = b["ticks"] - a["ticks"]
        out["frames_a"] = a["frames"]
        out["frames_b"] = b["frames"]
        out["frame_count_delta"] = b["frames"] - a["frames"]

        # 每 tick 的真实 rAF 间隔（ms）
        st = b["stamps"]
        if st:
            out["raf_interval_ms"] = {
                "n": len(st),
                "mean": round(sum(st) / len(st), 2),
                "min": round(min(st), 2),
                "max": round(max(st), 2),
            }
            out["raf_interval_s"] = round(sum(st) / len(st) / 1000.0, 5)
            out["raf_fps"] = round(1000.0 / (sum(st) / len(st)), 3)

        # 关键比值：frame_count 每真实 rAF tick 涨多少
        if out["raf_ticks_in_window"]:
            out["frames_per_raf_tick"] = round(
                out["frame_count_delta"] / out["raf_ticks_in_window"], 4)

        vel = b.get("vel") or [0.0, 0.0]
        vmag = math.hypot(vel[0], vel[1])
        out["terminal_v"] = round(vmag, 4)
        out["v_is_saturated_at_walk"] = abs(vmag - 4.6) < 0.01

        dframes = out["frame_count_delta"]
        if dframes and a["x"] is not None and b["x"] is not None:
            dx = math.hypot(b["x"] - a["x"], b["z"] - a["z"])
            out["total_dx"] = round(dx, 4)
            out["dx_per_frame_count"] = round(dx / dframes, 5)
            if vmag:
                out["dt_implied"] = round(dx / dframes / vmag, 5)
        out["dt_implied_per_raf_tick"] = round(
            out.get("dx_per_frame_count", 0) * out.get("frames_per_raf_tick", 1)
            / vmag, 5) if vmag else None

        out["marks_tail"] = b["marks"][-8:]

    out["source_clamps"] = {
        "MAX_FRAME_TIME": 0.25,
        "simulate_dt = delta.min(FIXED_DT*4)": round(1.0 / 60.0 * 4, 5),
        "WALK_SPEED": 4.6, "RUN_SPEED": 8.4,
    }
    os.makedirs(f"{SCRATCH}/results", exist_ok=True)
    path = f"{SCRATCH}/results/{WORKER}.json"
    with open(path, "w") as fh:
        json.dump(out, fh, ensure_ascii=False, indent=2)
    print(json.dumps(out, ensure_ascii=False, indent=2))
    print(f"\n# saved {path}")


if __name__ == "__main__":
    asyncio.run(main())
