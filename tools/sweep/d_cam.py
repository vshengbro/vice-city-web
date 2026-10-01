"""临时诊断:相机到底会不会穿模(用户报的第 8 / 9 条)。

沿玩家自己那条「进样板楼」路线(p4 同款 waypoint),每站采集一次
`camClearance` —— 那是相机算的「眼点到最近碰撞面的距离」,**为负就是
眼睛埋在墙里**。全程 >= 0 就说明遮挡回避是有效的。

同时打印 `camOccluded` / `camDist` / `camDistTarget`,用来确认「距离被
压缩了」而不是「只是没碰到东西」。
"""

import asyncio
import json
import os
import subprocess
import sys

import websockets

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sweep_lib import (  # noqa: E402
    CDP, URL, Driver, record, results, walk_to, wait_frames, wait_teleport,
)


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

        booted = False
        for _ in range(90):
            await asyncio.sleep(1.0)
            n = await d.ev("typeof __vcw !== 'undefined' ? __vcw.carX.length : -1")
            if isinstance(n, int) and n > 0:
                booted = True
                break
        if not booted:
            record("cam.route", False, "page never exposed __vcw")
            return
        await asyncio.sleep(2.0)

        route = json.loads(await d.ev("JSON.stringify(__vcw.route)") or "[]")
        if not route:
            record("cam.route", False, "no waypoint route published")
            return
        print(f"    route: {len(route)} waypoints", flush=True)

        worst = 1e9
        worst_at = None
        samples = 0
        occluded_hits = 0
        trace: list = []
        for index, (tx, tz) in enumerate(route):
            st = await d.st() or {}
            if index == 0:
                await d.ev(
                    f"(() => {{ window.__vcw_teleport = {{x: {tx},"
                    f" z: {tz}}}; return true; }})()")
                await wait_teleport(d, tx, tz)
                st = await wait_frames(d, 4) or (await d.st())
            else:
                await walk_to(d, tx, tz, trace)
                st = await d.st() or {}
            if not st:
                continue
            live = await d.ev_json(
                "JSON.stringify({clearance:__vcw.camClearance,"
                " occ:__vcw.camOccluded, dist:__vcw.cameraDist,"
                " tgt:__vcw.cameraDistTarget,"
                " eye:[__vcw.camEyeX,__vcw.camEyeY,__vcw.camEyeZ]})")
            live = live or {}
            c = live.get("clearance")
            occ = live.get("occ")
            if occ:
                occluded_hits += 1
            if isinstance(c, (int, float)):
                samples += 1
                if c < worst:
                    worst = c
                    worst_at = {"wp": index, "at": [tx, tz],
                                "eye": live.get("eye"), "occ": occ}
                print(f"      wp{index} clearance={c:+.3f} occ={occ} "
                      f"dist={live.get('dist')} -> {live.get('tgt')} "
                      f"eye={[round(v, 2) for v in (live.get('eye') or [0, 0, 0])]}",
                      flush=True)
        print(f"  samples={samples} occluded={occluded_hits} "
              f"worst camClearance={worst:.3f}")
        print(f"  worst at {worst_at}")
        record("cam.no_clip", samples > 0 and worst >= -0.01,
               f"worst clearance {worst:.3f} m over {samples} samples")
        results()


if __name__ == "__main__":
    asyncio.run(main())
