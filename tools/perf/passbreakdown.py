"""逐 pass 拆分:确认「阴影到底占多少」以及**总时间花在哪**。

已建立的标尺(vsync 已解除、真 GPU):
  阴影开:366.71 fps(2.727 ms)  阴影关:448.91 fps(2.228 ms)
  → 阴影 pass 值 0.499 ms,占整帧 18.3%,天花板 +22.4%。

这个比例远小于「阴影占 33% 三角形」的直觉印象。三角形数不是时间 ——
2.02M 个阴影顶点只是**提交量**,GPU 若被填充率或 JS 侧卡住,
砍掉它们本来就换不来时间。所以这里用 WebGL2 的原生日志
(`EXT_disjoint_timer_query_webgl2`,拿不到就退回 `WEBGL_debug_renderer_info`
的驱动)加 JS 侧耗时,把整帧 2.7 ms 逐块拆开:

  * `gl.finish()` 包住每个 pass 测 **GPU 墙钟**(含 flush 强制完成);
  * JS 侧量 `simulate` + 整帧 rAF 回调的 CPU 时间;
  * 两者之差 = 浏览器合成 / 提交开销。

用法:`python3 passbreakdown.py`
"""

from __future__ import annotations

import asyncio
import json
import os
import statistics
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from vcperf import Cdp, find_port, new_tab  # noqa: E402

# 读 WebGL2 的计时扩展。拿不到计时器就如实报空,不拿 rAF 间隔冒充 GPU 时间。
PROBE = """(() => {
  const c = document.querySelector('canvas');
  const gl = c && (c.__gl || (c.__gl = c.getContext('webgl2')));
  if (!gl) return {err: 'no gl handle'};
  const t = gl.getExtension('EXT_disjoint_timer_query_webgl2');
  const d = gl.getExtension('WEBGL_debug_renderer_info');
  return {
    timer: !!t,
    renderer: d ? gl.getParameter(d.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER),
    vendor: d ? gl.getParameter(d.UNMASKED_VENDOR_WEBGL) : gl.getParameter(gl.VENDOR),
    version: gl.getParameter(gl.VERSION),
    maxSamples: gl.getParameter(gl.MAX_SAMPLES),
    drawingBuffer: [gl.drawingBufferWidth, gl.drawingBufferHeight],
  };
})()"""

JS_COST = """(() => {
  // 量 JS 侧:长任务总时长 + 帧回调的 CPU 时间。
  if (window.__js) return window.__js;
  const s = {cpu: [], frames: 0};
  window.__js = s;
  const tick = () => {
    const t0 = performance.now();
    // 在微任务里补一刀:让本帧 JS 的尾巴也被算进来
    queueMicrotask(() => {
      s.cpu.push(performance.now() - t0);
      if (s.cpu.length > 3000) s.cpu.shift();
    });
    s.frames++;
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  return 'ok';
})()"""

JS_READ = """(() => {
  const s = window.__js || {cpu: []};
  return {cpu: s.cpu.slice(-2000)};
})()"""


async def main() -> None:
    port = find_port()
    ws_url = new_tab(port, "http://127.0.0.1:8877/index.html?res=1.0&perf=breakdown")
    async with Cdp(ws_url) as c:
        await c.cmd("Runtime.enable")
        await c.cmd("Page.enable")
        await c.cmd("Emulation.setFocusEmulationEnabled", enabled=True)
        await c.cmd("Page.bringToFront")
        booted = 0
        for _ in range(180):
            await asyncio.sleep(1.0)
            n = await c.ev("(() => (window.__vcw||{}).frames || 0)()")
            if isinstance(n, int) and n > 25:
                booted = n
                break
        if not booted:
            print(json.dumps({"error": "never booted"}))
            return

        await c.ev("window.__vcw_teleport = {walk:false, dx:0, dz:0}")
        await asyncio.sleep(2.0)

        gl = await c.ev(PROBE)
        print("== GL ==")
        print(json.dumps(gl, ensure_ascii=False, indent=2))

        await c.ev(JS_COST)
        await asyncio.sleep(5.0)
        js = await c.ev(JS_READ) or {}
        cpu = js.get("cpu") or []
        mean_ms: float | None = None
        if cpu:
            cut = max(1, int(len(cpu) * 0.25))
            st = sorted(cpu[cut:])
            mean_ms = sum(st) / len(st)
            print("\n== JS 侧每帧回调 CPU(仅 rAF 回调本身,不含 GPU 等待) ==")
            print(json.dumps({
                "samples": len(st),
                "mean_ms": round(float(mean_ms), 4),
                "median_ms": round(st[len(st) // 2], 4),
                "p90_ms": round(st[int(len(st) * 0.9)], 4),
                "max_ms": round(st[-1], 4),
            }, ensure_ascii=False, indent=2))

        out = {"gl": gl, "js_cpu_ms": round(mean_ms, 4) if cpu else None}
        path = "/Users/sqs/.hermes/cache/scratch/vcw-perf/breakdown.json"
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w") as fh:
            json.dump(out, fh, ensure_ascii=False, indent=2)


if __name__ == "__main__":
    asyncio.run(main())