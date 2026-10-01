"""临时诊断:p1 的 walk 为什么不动。

同时上报玩家位置 / `walkReq` / `vel`,以及**最近一辆动态车**的位置 ——
如果有一辆车就停在出生点旁边,质量加权分离会每帧把玩家推开、
车几乎不动,净位移就是 0。
"""

import asyncio
import json
import os
import subprocess
import sys

import websockets

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sweep_lib import CDP, URL, Driver, record, results, walk_until  # noqa: E402


async def main() -> None:
    subprocess.run(["curl", "-s", "--noproxy", "*", "-X", "PUT",
                    f"{CDP}/json/new?about:blank"],
                   capture_output=True, text=True, timeout=30)
    out = subprocess.run(["curl", "-s", "--noproxy", "*", f"{CDP}/json/list"],
                         capture_output=True, text=True, timeout=30).stdout
    pages = [t for t in json.loads(out) if t.get("type") == "page"]
    pages.sort(key=lambda t: t.get("id", ""), reverse=True)
    async with websockets.connect(pages[0]["webSocketDebuggerUrl"],
                                  max_size=128 * 1024 * 1024) as ws:
        d = Driver(ws)
        await d.cmd("Runtime.enable")
        await d.cmd("Page.enable")
        await d.cmd("Network.enable")
        await d.cmd("Network.setCacheDisabled", {"cacheDisabled": True})
        await d.cmd("Emulation.setFocusEmulationEnabled", {"enabled": True})
        await d.cmd("Page.bringToFront")
        await d.cmd("Page.navigate", {"url": URL})

        for _ in range(90):
            await asyncio.sleep(1.0)
            n = await d.ev("typeof __vcw !== 'undefined' ? __vcw.carX.length : -1")
            if isinstance(n, int) and n > 0:
                break
        await asyncio.sleep(2.0)

        async def snap() -> dict:
            raw = await d.ev(
                "JSON.stringify((() => {"
                " let best=1e9,bx=0,bz=0;"
                " for (let i=0;i<__vcw.carX.length;i++){"
                "  const dd=Math.hypot(__vcw.carX[i]-__vcw.playerX,"
                "                     __vcw.carZ[i]-__vcw.playerZ);"
                "  if(dd<best){best=dd;bx=__vcw.carX[i];bz=__vcw.carZ[i];}}"
                " return {px:__vcw.playerX, pz:__vcw.playerZ,"
                "  frames:__vcw.frames, walkReq:__vcw.walkReq, vel:__vcw.vel,"
                "  nearestCar:[bx,bz], nearestD:best, veh:__vcw.playerVehicle,"
                "  dbgSpeed:__vcw.dbgSpeed, keys:[...__vcw.walkReq]};"
                "})())")
            print("   raw:", repr(raw)[:200])
            import json as _j
            try:
                return _j.loads(raw) if raw else {}
            except Exception:
                return {}

        # 先单独确认键盘事件到底进不进 Rust:按下 W,看 walkReq / vel。
        await d.down("KeyW", "w")
        await asyncio.sleep(1.5)
        probe = await snap()
        print(f"  keydown-only: {probe}")
        await d.up("KeyW", "w")
        await asyncio.sleep(0.5)

        before = await snap()
        print(f"  before: {before}")
        await d.down("KeyW", "w")
        for i in range(24):
            await asyncio.sleep(0.35)
            q = await snap()
            print(f"   [{i:2d}] px={q.get('px')} pz={q.get('pz')} "
                  f"f={q.get('frames')} vel={q.get('vel')} "
                  f"dbgSpeed={q.get('dbgSpeed')}")
        await d.up("KeyW", "w")
        after = await snap()
        print(f"  after : {after}")
        moved = ((after.get("px", 0) - before.get("px", 0)) ** 2
                 + (after.get("pz", 0) - before.get("pz", 0)) ** 2) ** 0.5
        print(f"  moved={moved:.2f} m")
        record("walk", moved > 1.0, f"moved {moved:.2f} m")


if __name__ == "__main__":
    asyncio.run(main())
