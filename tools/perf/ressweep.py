"""画布分辨率扫描:确认瓶颈到底在 GPU 的哪一段。

已有的两条结论(全部实测):
  * 降 `SHADOW_MAP_SIZE` 2048→1024:446.85/449.66 → 449.03/449.53/449.58 fps,
    落在 ±2 fps 的run-to-run 噪声里。**贴图边长与顶点吞吐无关**,
    证实「降分辨率换帧率」这条路是空的。
  * 阴影 pass 整关掉 = 448.91 fps,剔除后 = 444.48 fps,已吃到 92% 的上限。

剩下的问题是:那 ~2.24 ms 花在哪儿?如果瓶颈真是顶点吞吐,
砍画布分辨率(填充率)就**不该**有明显收益;如果砍分辨率帧率明显上涨,
说明真正卡住的是像素/带宽,那阴影就不是主要矛盾。

这个扫描把画布缩到 12.5% / 25% / 50% / 100% 像素,各测一次。
注意 headless Chrome 截不出画面,但帧率读数是真的 —— 画面长什么样
不影响 rAF 节奏的统计。
"""

from __future__ import annotations

import asyncio
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from vcsession import SCRATCH, Cdp, gpu_info  # noqa: E402


async def measure(tag: str, scale: float, seconds: float = 12.0) -> dict:
    out: dict = {"tag": tag, "scale": scale}
    async with Cdp() as c:
        await c.cmd("Runtime.enable")
        await c.cmd("Page.enable")
        await c.cmd("Emulation.setFocusEmulationEnabled", enabled=True)
        await c.cmd("Page.bringToFront")
        await c.cmd("Network.setCacheDisabled", cacheDisabled=True)
        # res= 参数是游戏自己的分辨率缩放钩子
        await c.goto(f"http://127.0.0.1:8877/index.html?res={scale}&perf={tag}")
        booted = False
        for _ in range(180):
            await asyncio.sleep(1.0)
            n = await c.ev("(() => (window.__vcw||{}).frames || 0)()")
            if isinstance(n, int) and n > 25:
                booted = True
                break
        if not booted:
            out["error"] = "never booted"
            return out

        await c.ev("window.__vcw_teleport = {walk:false, dx:0, dz:0}")
        await asyncio.sleep(2.0)
        h = await c.ev("""(() => {
          const c = window.__vcw || {}; const hud = c.hud || '';
          const el = document.querySelector('canvas');
          return {backend: (hud.split(' · ')[1] || '?'),
                  drew: (hud.match(/· (\\d+) tris/) || [])[1] || '0',
                  canvas: el ? [el.width, el.height] : null,
                  pixels: el ? el.width * el.height : 0};
        })()""") or {}
        out["backend"] = h.get("backend")
        out["hud_drew_tris"] = int(h.get("drew", 0) or 0)
        out["canvas"] = h.get("canvas")
        out["pixels"] = h.get("pixels")
        if out["backend"] in ("init", "?", "none") or out["hud_drew_tris"] <= 0:
            out["error"] = "renderer not up — dead loop"
            return out

        await c.ev("""(() => {
          if (window.__pf) return;
          const st = {t: []};
          window.__pf = st;
          const tick = (ts) => { st.t.push(ts);
            if (st.t.length > 4000) st.t.shift();
            requestAnimationFrame(tick); };
          requestAnimationFrame(tick);
        })()""")
        await asyncio.sleep(5.0)
        await asyncio.sleep(seconds)
        st = await c.ev("(() => window.__pf.t.slice())()") or []
        cut = max(1, int(len(st) * 0.25))
        steady = st[cut:]
        dts = sorted((steady[i] - steady[i - 1]) / 1000.0
                     for i in range(1, len(steady)))
        dts = [d for d in dts if d > 0]
        if not dts:
            out["error"] = "no samples"
            return out
        n = len(dts)
        out["mean_dt_ms"] = round(sum(dts) / n * 1000, 4)
        out["fps_mean"] = round(1.0 / (sum(dts) / n), 2)
        out["fps_median"] = round(1.0 / dts[n // 2], 2)
    return out


async def main() -> None:
    scales = [float(a) for a in sys.argv[1:]] or [1.0, 0.5, 0.25, 0.125]
    results = []
    for s in scales:
        r = await measure(f"res-{s}", s)
        results.append(r)
        print(f"scale={s:5.3f}  canvas={r.get('canvas')}  "
              f"px={r.get('pixels')}  fps={r.get('fps_mean')}  "
              f"dt={r.get('mean_dt_ms')} ms", flush=True)
    os.makedirs(SCRATCH, exist_ok=True)
    with open(f"{SCRATCH}/resolution-sweep.json", "w") as fh:
        json.dump({"gpu": await gpu_info(), "runs": results}, fh,
                  ensure_ascii=False, indent=2)
    print(f"\n# saved {SCRATCH}/resolution-sweep.json")


if __name__ == "__main__":
    asyncio.run(main())