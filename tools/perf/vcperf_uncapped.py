"""解除 vsync 帧率上限 —— 否则**所有**帧率数字都只是在量显示器。

实测:开着上限时 `mean_dt` 恰好 16.666 ms = 1000/60,两次跑完全一致
(60.00 / 60.00 fps)。这是 rAF 被合成器按显示器刷新率节流的结果,
不是 GPU 的速度。在这种状态下阴影从 2048 降到 1024(顶点吞吐少 4 倍)
在 HUD 和任何 fps 读数上都**看不出区别** —— 优化无法被验证。

所以先撤掉上限再测:
  * `HeadlessExperimental.beginFrame` 之外的开关在新版已移除,
  * 可用做法是 `--disable-frame-rate-limit` / `--disable-gpu-vsync`,
    两者都得在**进程启动时**给,所以由 relaunch-gpu.sh 负责;
  * 这里只负责:测完之后核验上限确实不在了(见 `uncapped` 字段),
    并且用 canvas 缩小到极小来复查填充率确实不是瓶颈(上一棒的结论)。

用法:`python3 vcperf_uncapped.py <tag> [seconds] [quiet_ms]`
"""

from __future__ import annotations

import asyncio
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from vcperf import Cdp, find_port, new_tab  # noqa: E402

# 画布实际尺寸探针:验证「把像素砍到 1/4 帧率不动」这条结论现在仍成立。
CANVAS = """(() => {
  const c = document.querySelector('canvas');
  return c ? {w: c.width, h: c.height, dpr: window.devicePixelRatio,
              cssW: c.clientWidth, cssH: c.clientHeight} : null;
})()"""


async def main() -> None:
    tag = sys.argv[1] if len(sys.argv) > 1 else "uncapped"
    seconds = float(sys.argv[2]) if len(sys.argv) > 2 else 12.0
    quiet = float(sys.argv[3]) if len(sys.argv) > 3 else 5000.0

    port = find_port()
    ws_url = new_tab(port, f"http://127.0.0.1:8877/index.html?res=1.0&perf={tag}")
    out: dict = {"tag": tag}
    async with Cdp(ws_url) as c:
        await c.cmd("Runtime.enable")
        await c.cmd("Page.enable")
        await c.cmd("Emulation.setFocusEmulationEnabled", enabled=True)
        await c.cmd("Page.bringToFront")
        await c.cmd("Network.setCacheDisabled", cacheDisabled=True)
        await c.cmd("Page.startScreencast", format="jpeg", quality=1, maxWidth=8,
                    maxHeight=8, everyNthFrame=1)

        booted = 0
        for _ in range(180):
            await asyncio.sleep(1.0)
            n = await c.ev("(() => (window.__vcw||{}).frames || 0)()")
            if isinstance(n, int) and n > 25:
                booted = n
                break
        out["booted"] = bool(booted)
        if not booted:
            out["error"] = "never booted"
            print(json.dumps(out, indent=2))
            return

        await c.ev("window.__vcw_teleport = {walk:false, dx:0, dz:0}")
        await asyncio.sleep(2.0)
        out["canvas"] = await c.ev(CANVAS)

        health = await c.ev("""(() => {
          const h = (window.__vcw || {}).hud || '';
          return {drew: (h.match(/· (\\d+) tris/) || [])[1] || '0',
                  backend: (h.split(' · ')[1] || '?')};
        })()""") or {}
        out["backend"] = health.get("backend")
        out["hud_drew_tris"] = health.get("drew")
        if out["backend"] in ("init", "?", "none") or int(
            health.get("drew", 0) or 0
        ) <= 0:
            out["error"] = "renderer not up — dead loop, not a frame rate"
            print(json.dumps(out, ensure_ascii=False, indent=2))
            return

        await c.ev("""(() => {
          if (window.__pf) return;
          const st = {t: []};
          window.__pf = st;
          const tick = (ts) => { st.t.push(ts);
            if (st.t.length > 4000) st.t.shift();
            requestAnimationFrame(tick); };
          requestAnimationFrame(tick);
        })()""")
        await asyncio.sleep(quiet / 1000.0)

        await asyncio.sleep(seconds)
        dts_raw = await c.ev("(() => window.__pf.t.slice())()") or []
        cut = max(1, int(len(dts_raw) * 0.25))
        steady = dts_raw[cut:]
        dts = sorted((steady[i] - steady[i - 1]) / 1000.0
                     for i in range(1, len(steady)))
        dts = [d for d in dts if d > 0]
        if not dts:
            out["error"] = "no dt samples"
            print(json.dumps(out, indent=2))
            return
        n = len(dts)
        mean = sum(dts) / n
        out["samples"] = n
        out["mean_dt_ms"] = round(mean * 1000, 3)
        out["median_dt_ms"] = round(dts[n // 2] * 1000, 3)
        out["p10_dt_ms"] = round(dts[int(n * 0.1)] * 1000, 3)
        out["fps_mean"] = round(1.0 / mean, 2)
        out["fps_median"] = round(1.0 / dts[n // 2], 2)
        # 16.666 ms 说明还卡在 60 Hz,数字没有测到 GPU 的真实成本。
        out["uncapped"] = not (15.5 < out["mean_dt_ms"] < 17.5)
        out["vsync_capped"] = 15.5 < out["mean_dt_ms"] < 17.5

        path = f"/Users/sqs/.hermes/cache/scratch/vcw-perf/{tag}.json"
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w") as fh:
            json.dump(out, fh, ensure_ascii=False, indent=2)
        print(json.dumps(out, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    asyncio.run(main())