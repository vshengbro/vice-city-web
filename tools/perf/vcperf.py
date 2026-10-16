"""帧率测量器 —— 复用**已存在**的 Chrome 实例,只新建一个 tab。

硬约束:禁止新开浏览器窗口。一个 Chrome 实例 + 一个 tab,页面切换用
`Page.navigate` 在同一个 tab 里复用。启动前先查已有实例的
`remote-debugging-port`,有就直接连,不 `subprocess.Popen` 新的 Chrome。

「关掉阴影能跑多快」这个基准是整个优化的标尺,测不准后面全白干,
所以这个脚本只做一件事:在固定视点、固定时长下报一个可信的 fps。
"""

from __future__ import annotations

import asyncio
import json
import os
import statistics
import sys
import time
import urllib.request

import websockets

BASE_URL = "http://127.0.0.1:8877/index.html"
DEFAULT_PORT = 9240

# 探针挂在 window 上,与游戏自己的 rAF 链**并联**而不替换它。
# 替换会把被测对象换掉 —— 测出来的就是探针的节奏,不是游戏的节奏。
PROBE = """(() => {
  if (window.__pf) { return 'already'; }
  const st = {t: [], f0: (window.__vcw||{}).frames || 0, marks: []};
  window.__pf = st;
  const tick = (ts) => {
    st.t.push(ts);
    if (st.t.length > 2000) st.t.shift();
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  return 'ok';
})()"""

READ = """(() => {
  const p = window.__pf;
  const c = window.__vcw || {};
  return {stamps: p.t.slice(), f0: p.f0,
          frames: c.frames || 0, tris: c.tris || 0,
          loaded: c.loadedAssets || 0,
          hud: (c.hud || '').slice(-160),
          x: c.playerX, z: c.playerZ};
})()"""


def find_port(preferred: int = DEFAULT_PORT) -> int:
    """复用已在跑的 Chrome;只在真的一个都没有时才由调用方自己起。"""
    for port in (preferred, 9240, 9333, 9222):
        try:
            with urllib.request.urlopen(
                f"http://127.0.0.1:{port}/json/version", timeout=3
            ) as r:
                if r.status == 200:
                    return port
        except Exception:  # noqa: BLE001
            continue
    raise SystemExit("no chrome with a debugging port is running")


def new_tab(port: int, url: str) -> str:
    """在**已有** Chrome 实例里新建一个 tab(不是新窗口)。"""
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/json/new?{url}", method="PUT"
    )
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.loads(r.read())["webSocketDebuggerUrl"]


class Cdp:
    def __init__(self, ws_url: str) -> None:
        self.ws_url = ws_url
        self.ws = None
        self._n = 0

    async def __aenter__(self) -> "Cdp":
        self.ws = await websockets.connect(self.ws_url, max_size=200 * 1024 * 1024)
        return self

    async def __aexit__(self, *exc) -> None:
        if self.ws:
            await self.ws.close()

    async def cmd(self, method: str, **params) -> dict:
        assert self.ws is not None
        self._n += 1
        mid = self._n
        await self.ws.send(json.dumps({"id": mid, "method": method, "params": params}))
        while True:
            msg = json.loads(await self.ws.recv())
            if msg.get("id") == mid:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})

    async def ev(self, expr: str):
        r = await self.cmd(
            "Runtime.evaluate", expression=expr, returnByValue=True, awaitPromise=True
        )
        if r.get("exceptionDetails"):
            return None
        return r.get("result", {}).get("value")


async def measure(port: int, tag: str, seconds: float, quiet_ms: float,
                   res: float = 1.0) -> dict:
    """固定视点跑一段,报 raf fps。返回可直接进报告的 dict。"""
    url = f"{BASE_URL}?res={res}&perf={tag}"
    ws_url = new_tab(port, url)
    out: dict = {"tag": tag, "url": url}
    async with Cdp(ws_url) as c:
        await c.cmd("Runtime.enable")
        await c.cmd("Page.enable")
        await c.cmd("Emulation.setFocusEmulationEnabled", enabled=True)
        await c.cmd("Page.bringToFront")
        await c.cmd("Network.setCacheDisabled", cacheDisabled=True)

        # 等游戏真的开起来(场景流式生成,首帧不等于就绪)
        booted = 0
        for _ in range(180):
            await asyncio.sleep(1.0)
            n = await c.ev("(() => (window.__vcw||{}).frames || 0)()")
            if isinstance(n, int) and n > 25:
                booted = n
                break
        if not booted:
            out["error"] = "page never booted"
            return out
        out["booted_at_frames"] = booted

        # 固定视点:不走路、不开车,只有站着不动的稳态帧率可比。
        await c.ev("window.__vcw_teleport = {walk:false, dx:0, dz:0}")
        await asyncio.sleep(2.0)

        renderer = await c.ev("""(() => {
          try {
            const gl = document.createElement('canvas').getContext('webgl2');
            if (!gl) return 'no webgl2';
            const d = gl.getExtension('WEBGL_debug_renderer_info');
            return d ? gl.getParameter(d.UNMASKED_RENDERER_WEBGL) : 'no ext';
          } catch (e) { return 'err ' + e.message; }
        })()""")
        out["renderer"] = renderer

        await c.ev(PROBE)
        await asyncio.sleep(quiet_ms / 1000.0)  # 丢掉预热帧

        # **健康闸门:后端必须是真 WebGL2 且真的画出了三角形。**
        # 踩过的坑:`--disable-gpu` 起的 Chrome 里 `WebGlRenderer::new()` 失败,
        # `game.renderer = None`,HUD backend 打 `init`(BACKEND_NONE)+ `0 tris`,
        # 一个三角形都没画 —— 但 rAF 照样按 60 Hz 空转,测出来是 13 fps 的
        # 漂亮数字,拿它做基线整个归因就废了(HUD 里的 tris 才是判据:
        # `__vcw.tris` 是场景三角形数,不是本帧绘制数)。
        health = await c.ev("""(() => {
          const h = (window.__vcw || {}).hud || '';
          return {hud: h, drew: (h.match(/· (\\d+) tris/) || [])[1] || '0',
                  backend: (h.split(' · ')[1] || '?')};
        })()""") or {}
        drew = int(health.get("drew", 0) or 0)
        backend = health.get("backend", "?")
        out["backend"] = backend
        out["hud_drew_tris"] = drew
        if backend in ("init", "?", "none") or drew <= 0:
            out["error"] = (
                f"renderer not up: backend={backend!r} drew={drew} tris. "
                "This instance has no live GL (check --disable-gpu). "
                "The rAF cadence of a dead loop is NOT a frame rate."
            )
            return out

        t0 = time.time()
        await asyncio.sleep(seconds)
        wall = time.time() - t0
        snap = await c.ev(READ)
        if not snap:
            out["error"] = "probe read failed"
            return out

        st = snap["stamps"]
        # 掐掉预热段:前 25% 的间隔是 shader 编译 / 管线编译的尖峰。
        cut = max(1, int(len(st) * 0.25))
        steady = st[cut:]
        dts = [(steady[i] - steady[i - 1]) / 1000.0 for i in range(1, len(steady))]
        dts = [d for d in dts if d > 0]

        out["wall_s"] = round(wall, 2)
        out["raf_ticks_total"] = len(st)
        out["steady_samples"] = len(dts)
        out["game_frames_delta"] = snap["frames"] - snap["f0"]
        out["tris"] = snap["tris"]
        out["loaded_assets"] = snap["loaded"]
        out["mean_dt_ms"] = round(statistics.mean(dts) * 1000, 3) if dts else None
        out["median_dt_ms"] = round(statistics.median(dts) * 1000, 3) if dts else None
        out["p90_dt_ms"] = (
            round(sorted(dts)[int(len(dts) * 0.9)] * 1000, 3) if dts else None
        )
        out["fps_mean"] = (
            round(1.0 / statistics.mean(dts), 2) if dts else None
        )
        out["fps_median"] = (
            round(1.0 / statistics.median(dts), 2) if dts else None
        )
        # 帧预算按帧数算:窗口内 rAF tick 除以墙钟,和 dt 口径互为对照。
        out["fps_by_ticks"] = (
            round(len(dts) / wall, 2) if wall > 0 and dts else None
        )
        out["hud_tail"] = snap["hud"]
        out["player"] = [snap["x"], snap["z"]]
        return out


async def main() -> None:
    tag = sys.argv[1] if len(sys.argv) > 1 else "base"
    seconds = float(sys.argv[2]) if len(sys.argv) > 2 else 12.0
    quiet = float(sys.argv[3]) if len(sys.argv) > 3 else 4000.0
    port = find_port()
    print(f"# reusing chrome on :{port} (one instance, one new tab)", flush=True)
    res = await measure(port, tag, seconds, quiet)
    scratch = "/Users/sqs/.hermes/cache/scratch/vcw-perf"
    os.makedirs(scratch, exist_ok=True)
    with open(f"{scratch}/{tag}.json", "w") as fh:
        json.dump(res, fh, ensure_ascii=False, indent=2)
    print(json.dumps(res, ensure_ascii=False, indent=2))
    print(f"\n# saved {scratch}/{tag}.json")


if __name__ == "__main__":
    asyncio.run(main())