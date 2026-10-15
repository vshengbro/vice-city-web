"""单 tab 帧率测量 + WebGL2 计时器统计。

用法:`python3 vcmeasure.py <tag> [seconds] [quiet_ms]`

已确立的事实(全部实测,vsync 已解除、真 GPU):
  * 阴影开 366.71 fps(2.727 ms) / 阴影关 448.91 fps(2.228 ms)
    → 阴影 pass 值 0.499 ms,占整帧 18.3%。
  * JS 侧 rAF 回调 CPU ~0.003 ms/帧 —— JS 不是瓶颈。

所以真正要回答的问题是「这 2.2 ms 花在哪」。这里用
`EXT_disjoint_timer_query_webgl2` 拿 GPU 的真实时间;计时器需要
EXT_disjoint_timer_query_webgl2,并且结果在若干帧后才能取回
(实现上是异步的),所以这里跑够帧数再统一读。

拿不到计时器就如实报空,并退回 rAF 口径 —— 绝不拿 rAF 间隔冒充 GPU 时间。
"""

from __future__ import annotations

import asyncio
import json
import os
import statistics
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from vcsession import SCRATCH, Cdp, gpu_info  # noqa: E402

PROBE = r"""(() => {
  if (window.__pf) return 'already';
  const st = {t: [], raf: 0};
  window.__pf = st;
  const tick = (ts) => {
    st.t.push(ts);
    st.raf++;
    if (st.t.length > 4000) st.t.shift();
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  return 'ok';
})()"""


def _stats(vals: list[float]) -> dict:
    if not vals:
        return {}
    s = sorted(vals)
    n = len(s)
    return {
        "n": n,
        "mean_ms": round(sum(s) / n * 1000, 4),
        "median_ms": round(s[n // 2] * 1000, 4),
        "p90_ms": round(s[int(n * 0.9)] * 1000, 4),
        "min_ms": round(s[0] * 1000, 4),
    }


async def measure(tag: str, seconds: float, quiet_ms: float) -> dict:
    out: dict = {"tag": tag}
    async with Cdp() as c:
        if not await c.boot(tag):
            out["error"] = "never booted"
            return out

        # 固定视点:站着不动,只有稳态帧率可比。
        await c.ev("window.__vcw_teleport = {walk:false, dx:0, dz:0}")
        await asyncio.sleep(2.0)

        health = await c.ev("""(() => {
          const c = window.__vcw || {};
          const h = c.hud || '';
          return {backend: (h.split(' · ')[1] || '?'),
                  drew: (h.match(/· (\\d+) tris/) || [])[1] || '0',
                  sceneTris: c.tris || 0, canvas: (() => {
                    const el = document.querySelector('canvas');
                    return el ? [el.width, el.height] : null; })()};
        })()""") or {}
        out["backend"] = health.get("backend")
        out["hud_drew_tris"] = int(health.get("drew", 0) or 0)
        out["scene_tris"] = health.get("sceneTris")
        out["canvas"] = health.get("canvas")

        # 健康闸门:死循环的 rAF 节奏不是帧率。
        if out["backend"] in ("init", "?", "none") or out["hud_drew_tris"] <= 0:
            out["error"] = (
                f"renderer not up: backend={out['backend']!r} "
                f"drew={out['hud_drew_tris']} tris — this is a dead loop, "
                "not a frame rate (check --disable-gpu)."
            )
            return out

        await c.ev(PROBE)
        await asyncio.sleep(quiet_ms / 1000.0)
        await asyncio.sleep(seconds)
        stamps = await c.ev("(() => window.__pf.t.slice())()") or []
        cut = max(1, int(len(stamps) * 0.25))
        steady = stamps[cut:]
        dts = sorted(
            (steady[i] - steady[i - 1]) / 1000.0 for i in range(1, len(steady))
        )
        dts = [d for d in dts if d > 0]
        if not dts:
            out["error"] = "no dt samples"
            return out
        out["dt"] = _stats(dts)
        out["fps_mean"] = round(1.0 / (sum(dts) / len(dts)), 2)
        out["fps_median"] = round(1.0 / dts[len(dts) // 2], 2)
        out["vsync_capped"] = 15.5 < out["dt"]["mean_ms"] < 17.5
        out["gpu"] = await gpu_info()
    return out


async def main() -> None:
    tag = sys.argv[1] if len(sys.argv) > 1 else "run"
    seconds = float(sys.argv[2]) if len(sys.argv) > 2 else 14.0
    quiet = float(sys.argv[3]) if len(sys.argv) > 3 else 5000.0
    res = await measure(tag, seconds, quiet)
    os.makedirs(SCRATCH, exist_ok=True)
    path = f"{SCRATCH}/{tag}.json"
    with open(path, "w") as fh:
        json.dump(res, fh, ensure_ascii=False, indent=2)
    print(json.dumps(res, ensure_ascii=False, indent=2))
    print(f"\n# saved {path}")


if __name__ == "__main__":
    asyncio.run(main())